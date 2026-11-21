//! Live analysis of what is playing: a lock-free-ish tap on the audio thread plus an FFT
//! analyser the UI calls once per frame.
//!
//! The audio thread never blocks on this: it `try_lock`s the ring and simply skips a block if the
//! UI happens to be copying (a few microseconds). The UI side turns the latest 2048 samples into
//! log-spaced, smoothed bands in 0..1 and a loudness level.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

use crate::drive::{Drive, DriveTracker};
use crate::tone::{self, Tone};

pub const FFT_SIZE: usize = 2048;
const RING: usize = 8192;
/// Samples the audio thread gathers before it touches the ring.
pub(crate) const TAP_BLOCK: usize = 256;
const F_LOW: f32 = 20.0;
const F_HIGH: f32 = 16_000.0;
/// dB range mapped onto 0..1.
const DB_FLOOR: f32 = -78.0;
const DB_CEIL: f32 = -14.0;

struct Ring {
    buf: Box<[f32; RING]>,
    write: usize,
}

/// Written by the audio thread, read by [`Analyzer`].
pub struct TapShared {
    ring: Mutex<Ring>,
    level: AtomicU32,
    rate: AtomicU32,
}

impl Default for TapShared {
    fn default() -> Self {
        TapShared { ring: Mutex::new(Ring { buf: Box::new([0.0; RING]), write: 0 }), level: AtomicU32::new(0), rate: AtomicU32::new(44_100) }
    }
}

impl TapShared {
    pub(crate) fn set_rate(&self, rate: u32) {
        self.rate.store(rate.max(1), Ordering::Relaxed);
    }

    /// Called from the audio thread with a block of mono samples.
    pub(crate) fn push(&self, block: &[f32]) {
        let rms = (block.iter().map(|s| s * s).sum::<f32>() / block.len().max(1) as f32).sqrt();
        self.level.store(rms.to_bits(), Ordering::Relaxed);
        if let Ok(mut ring) = self.ring.try_lock() {
            for &s in block {
                let w = ring.write;
                ring.buf[w] = s;
                ring.write = (w + 1) % RING;
            }
        }
    }

    /// Copies the newest `out.len()` samples (oldest first).
    fn latest(&self, out: &mut [f32]) {
        let ring = self.ring.lock().unwrap_or_else(PoisonError::into_inner);
        let n = out.len().min(RING);
        let start = (ring.write + RING - n) % RING;
        for (i, o) in out.iter_mut().enumerate() {
            *o = ring.buf[(start + i) % RING];
        }
    }

    /// Forgets the audio (after a seek or a stop) so stale data does not light the display.
    pub(crate) fn clear(&self) {
        let mut ring = self.ring.lock().unwrap_or_else(PoisonError::into_inner);
        ring.buf.fill(0.0);
        self.level.store(0, Ordering::Relaxed);
    }
}

/// Turns the tap into bands. Keep one per UI.
pub struct Analyzer {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    samples: Vec<f32>,
    spectrum: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    bands: Vec<f32>,
    /// The unsmoothed band heights of this frame, kept to avoid allocating them every frame.
    target: Vec<f32>,
    /// Magnitudes for [`tone::read_into`], likewise reused.
    tone_mag: Vec<f32>,
    level: f32,
    tone: Tone,
    drive: DriveTracker,
    /// The magnitudes scaled so a full-scale sine is 1, for the drive.
    scaled: Vec<f32>,
    felt: Drive,
    last: Option<Instant>,
}

impl Default for Analyzer {
    fn default() -> Self {
        Analyzer::new()
    }
}

impl Analyzer {
    pub fn new() -> Analyzer {
        let fft = FftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE);
        let scratch = vec![Complex::new(0.0, 0.0); fft.get_inplace_scratch_len()];
        // Hann window.
        let window = (0..FFT_SIZE).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FFT_SIZE as f32).cos()).collect();
        Analyzer {
            fft,
            window,
            samples: vec![0.0; FFT_SIZE],
            spectrum: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            scratch,
            bands: Vec::new(),
            target: Vec::new(),
            tone_mag: Vec::new(),
            level: 0.0,
            tone: Tone::default(),
            drive: DriveTracker::default(),
            scaled: Vec::new(),
            felt: Drive::default(),
            last: None,
        }
    }

    /// Fills `out` with band heights in 0..1, low to high. When nothing is `playing` the bands
    /// fall to zero instead of freezing on the last audio.
    pub fn bands(&mut self, tap: &TapShared, playing: bool, out: &mut [f32]) {
        let now = Instant::now();
        let dt = self.last.replace(now).map_or(1.0 / 60.0, |t| now.duration_since(t).as_secs_f32());
        self.step(tap, playing, out, dt.clamp(0.001, 0.25));
    }

    /// [`Analyzer::bands`] with the elapsed time given explicitly (deterministic for tests).
    pub(crate) fn step(&mut self, tap: &TapShared, playing: bool, out: &mut [f32], dt: f32) {
        let n = out.len();
        if n == 0 {
            return;
        }
        if self.bands.len() != n {
            self.bands = vec![0.0; n];
        }
        if self.target.len() != n {
            self.target = vec![0.0; n];
        }
        let rate = tap.rate.load(Ordering::Relaxed) as f32;

        if playing {
            tap.latest(&mut self.samples);
            for ((c, s), w) in self.spectrum.iter_mut().zip(&self.samples).zip(&self.window) {
                *c = Complex::new(s * w, 0.0);
            }
            self.fft.process_with_scratch(&mut self.spectrum, &mut self.scratch);
            band_levels(&self.spectrum[..FFT_SIZE / 2], rate, &mut self.target);
            self.tone = tone::read_into(&self.spectrum[..FFT_SIZE / 2], rate, FFT_SIZE, &mut self.tone_mag);
            // A full-scale sine under a Hann window peaks at N/4.
            let norm = 4.0 / FFT_SIZE as f32;
            self.scaled.clear();
            self.scaled.extend(self.tone_mag.iter().map(|m| m * norm));
        } else {
            // Nothing plays: the target is silence, so the bars fall rather than freeze.
            self.target.fill(0.0);
            self.tone = Tone::default();
        }

        // Fast attack, slower release: bars jump up with the beat and fall gracefully.
        let (rise, fall) = (1.0 - (-dt / 0.035).exp(), 1.0 - (-dt / 0.16).exp());
        for (b, t) in self.bands.iter_mut().zip(&self.target) {
            *b += (t - *b) * if t > b { rise } else { fall };
        }
        out.copy_from_slice(&self.bands);

        let rms = if playing { f32::from_bits(tap.level.load(Ordering::Relaxed)) } else { 0.0 };
        let lvl = ((20.0 * rms.max(1e-6).log10() + 54.0) / 54.0).clamp(0.0, 1.0);
        self.level += (lvl - self.level) * if lvl > self.level { rise } else { fall };
        self.felt = if playing && !self.scaled.is_empty() { self.drive.step(&self.scaled, rate / FFT_SIZE as f32, lvl, dt) } else { self.drive.quiet(dt) };
    }

    /// The kick, snare, hats and voice as of the last [`Analyzer::bands`] call.
    pub fn drive(&self) -> Drive {
        self.felt
    }

    /// The tonal reading (chroma, melody pitch, brightness) as of the last [`Analyzer::bands`]
    /// call. Not smoothed: it is read from that moment's FFT alone.
    pub fn tone(&self) -> Tone {
        self.tone
    }

    /// Smoothed loudness in 0..1 as of the last [`Analyzer::bands`] call.
    pub fn level(&self) -> f32 {
        self.level
    }
}

/// Maps FFT bins to `out.len()` log-spaced bands (0..1).
fn band_levels(bins: &[Complex<f32>], rate: f32, out: &mut [f32]) {
    let n = out.len();
    let nyquist = rate * 0.5;
    let hi = F_HIGH.min(nyquist * 0.95);
    let bin_hz = rate / FFT_SIZE as f32;
    // A full-scale sine under a Hann window peaks at N/4.
    let norm = 4.0 / FFT_SIZE as f32;
    for (i, o) in out.iter_mut().enumerate() {
        let f0 = F_LOW * (hi / F_LOW).powf(i as f32 / n as f32);
        let f1 = F_LOW * (hi / F_LOW).powf((i + 1) as f32 / n as f32);
        let lo_bin = (f0 / bin_hz).floor() as usize;
        let hi_bin = ((f1 / bin_hz).ceil() as usize).max(lo_bin + 1).min(bins.len());
        let lo_bin = lo_bin.min(hi_bin.saturating_sub(1));
        let slice = &bins[lo_bin..hi_bin];
        let peak = slice.iter().map(|c| c.norm()).fold(0.0f32, f32::max) * norm;
        // A narrow peak catches tonal content, while the energy term keeps short bass hits and
        // bright transients visible even when their energy falls between FFT bins.
        let energy = (slice.iter().map(Complex::norm_sqr).sum::<f32>() / slice.len().max(1) as f32).sqrt() * norm * 1.8;
        let amplitude = peak.max(energy);
        // Tilt: music has far less energy up high; +3 dB per octave keeps the right side alive.
        let centre = (f0 * f1).sqrt();
        let tilt = 3.0 * (centre / 1000.0).log2();
        let db = 20.0 * amplitude.max(1e-9).log10() + tilt;
        *o = ((db - DB_FLOOR) / (DB_CEIL - DB_FLOOR)).clamp(0.0, 1.0);
    }
}
