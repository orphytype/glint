//! What the music is doing tonally, from the same FFT as the bands: which of the twelve pitch
//! classes sound (a chroma vector, so chords and keys show), the most likely melody pitch and how
//! clearly it stands out, and how bright the sound is (its spectral centroid).
//!
//! None of this is a transcription; it is a steady, musical reading for pictures to follow.

use rustfft::num_complex::Complex;

/// The tonal reading of one moment.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tone {
    /// Energy of each pitch class (C, C#, D, ... B), the strongest at 1, 0..1.
    pub chroma: [f32; 12],
    /// The most likely melody pitch, as a MIDI note number with a fraction (69 is A4 = 440 Hz);
    /// 0 when nothing sounds.
    pub pitch: f32,
    /// How clearly that pitch stands out above the rest of its range, 0..1.
    pub clarity: f32,
    /// Spectral centroid on a log scale: 0 at 100 Hz, 1 at 10 kHz.
    pub centroid: f32,
}

/// The MIDI note of a frequency.
pub fn midi(hz: f32) -> f32 {
    69.0 + 12.0 * (hz / 440.0).log2()
}

/// Range the melody is looked for in (Hz): from about G2 up to C6.
const MELODY_LO: f32 = 98.0;
const MELODY_HI: f32 = 1050.0;
/// Range the chroma is gathered from: low enough for bass notes to count (their upper partials do
/// most of the work at this resolution), high enough to leave the cymbals out.
const CHROMA_LO: f32 = 60.0;
const CHROMA_HI: f32 = 4200.0;

/// Reads `bins` (the first half of an FFT of `fft_size` samples at `rate` Hz).
#[cfg(test)]
pub fn read(bins: &[Complex<f32>], rate: f32, fft_size: usize) -> Tone {
    let mut mag = Vec::with_capacity(bins.len());
    read_into(bins, rate, fft_size, &mut mag)
}

/// [`read`] with the magnitude buffer kept between calls, so a per-frame reader allocates once.
pub fn read_into(bins: &[Complex<f32>], rate: f32, fft_size: usize, mag: &mut Vec<f32>) -> Tone {
    let bin_hz = rate / fft_size as f32;
    mag.clear();
    mag.extend(bins.iter().map(|c| c.norm()));
    let n = mag.len();
    let mut t = Tone::default();
    let total: f32 = mag.iter().sum();
    if n < 8 || total <= 1e-6 {
        return t;
    }

    // Chroma: each bin's power shared between its two nearest semitones.
    let (c0, c1) = (((CHROMA_LO / bin_hz).ceil() as usize).max(1), ((CHROMA_HI / bin_hz) as usize).min(n - 1));
    for (k, m) in mag.iter().enumerate().take(c1 + 1).skip(c0) {
        let note = midi(k as f32 * bin_hz);
        let lo = note.floor();
        let w = note - lo;
        let p = m * m;
        let pc = |x: f32| (x as i32).rem_euclid(12) as usize;
        t.chroma[pc(lo)] += p * (1.0 - w);
        t.chroma[pc(lo + 1.0)] += p * w;
    }
    let top = t.chroma.iter().copied().fold(0.0f32, f32::max);
    if top > 0.0 {
        // Square root: power back toward amplitude, so weaker notes of a chord still show.
        for c in &mut t.chroma {
            *c = (*c / top).sqrt();
        }
    }

    // Melody: the harmonic product spectrum over the melody range (a note's partials line up at
    // 1, 2 and 3 times its frequency), refined between bins.
    let (m0, m1) = (((MELODY_LO / bin_hz).floor() as usize).max(1), ((MELODY_HI / bin_hz).ceil() as usize).min((n - 1) / 3));
    if m1 > m0 + 2 {
        let hps = |k: usize| mag[k] * (mag[2 * k] + 1e-9).sqrt() * (mag[3 * k] + 1e-9).cbrt();
        // One pass for the peak, its mean and the bin it sits on: no per-frame score buffer.
        let (mut best, mut peak, mut sum) = (m0, 0.0f32, 0.0f32);
        for k in m0..=m1 {
            let v = hps(k);
            sum += v;
            if v > peak {
                peak = v;
                best = k;
            }
        }
        let mean = sum / (m1 - m0 + 1) as f32;
        if peak > 0.0 {
            let k = best;
            // Parabolic interpolation on the magnitude around the peak bin.
            let (a, b, c) = (mag[k - 1], mag[k], mag[k + 1]);
            let den = a - 2.0 * b + c;
            let off = if den.abs() > 1e-9 { (0.5 * (a - c) / den).clamp(-0.5, 0.5) } else { 0.0 };
            t.pitch = midi((k as f32 + off) * bin_hz);
            t.clarity = (1.0 - mean / peak).clamp(0.0, 1.0).powi(2);
        }
    }

    // Brightness: the magnitude-weighted mean frequency, on a log scale.
    let (s0, s1) = ((100.0 / bin_hz) as usize, ((10_000.0 / bin_hz) as usize).min(n - 1));
    let (mut num, mut den) = (0.0, 0.0);
    for (k, m) in mag.iter().enumerate().take(s1 + 1).skip(s0.max(1)) {
        num += k as f32 * bin_hz * m;
        den += m;
    }
    if den > 0.0 {
        t.centroid = ((num / den / 100.0).log2() / 100f32.log2()).clamp(0.0, 1.0);
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustfft::FftPlanner;

    const RATE: f32 = 44_100.0;
    const N: usize = 2048;

    /// The FFT of `tones` (Hz, amplitude), each with a few softer harmonics like an instrument.
    fn spectrum(tones: &[(f32, f32)]) -> Vec<Complex<f32>> {
        let mut buf: Vec<Complex<f32>> = (0..N)
            .map(|i| {
                let t = i as f32 / RATE;
                let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos();
                let s: f32 = tones.iter().map(|&(f, a)| (1..=4).map(|h| a / h as f32 * (std::f32::consts::TAU * f * h as f32 * t).sin()).sum::<f32>()).sum();
                Complex::new(s * w, 0.0)
            })
            .collect();
        FftPlanner::new().plan_fft_forward(N).process(&mut buf);
        buf.truncate(N / 2);
        buf
    }

    fn strongest(c: &[f32; 12]) -> usize {
        c.iter().copied().enumerate().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap().0
    }

    #[test]
    fn a_note_is_found_and_named() {
        let t = read(&spectrum(&[(440.0, 0.5)]), RATE, N);
        assert!((t.pitch - 69.0).abs() < 0.6, "A4 read as {}", t.pitch);
        assert_eq!(strongest(&t.chroma), 9, "A is pitch class 9: {:?}", t.chroma);
        assert!(t.clarity > 0.5, "a lone note is clear: {}", t.clarity);
        let low = read(&spectrum(&[(196.0, 0.5)]), RATE, N);
        assert!((low.pitch - 55.0).abs() < 0.8, "G3 read as {}", low.pitch);
    }

    #[test]
    fn a_chord_lights_its_notes() {
        // C major: C4, E4, G4.
        let t = read(&spectrum(&[(261.6, 0.4), (329.6, 0.4), (392.0, 0.4)]), RATE, N);
        for pc in [0, 4, 7] {
            assert!(t.chroma[pc] > 0.5, "pitch class {pc}: {:?}", t.chroma);
        }
        for pc in [1, 3, 6, 8, 10] {
            assert!(t.chroma[pc] < t.chroma[0].min(t.chroma[4]).min(t.chroma[7]), "pitch class {pc} stays under the chord: {:?}", t.chroma);
        }
    }

    #[test]
    fn brighter_sound_has_a_higher_centroid() {
        let dull = read(&spectrum(&[(150.0, 0.5)]), RATE, N);
        let bright = read(&spectrum(&[(2500.0, 0.5)]), RATE, N);
        assert!(bright.centroid > dull.centroid + 0.2, "{} vs {}", bright.centroid, dull.centroid);
    }

    #[test]
    fn read_into_reuses_the_scratch_buffer() {
        let bins = spectrum(&[(440.0, 0.5)]);
        let mut mag = vec![1.0; 5];
        let reused = read_into(&bins, RATE, N, &mut mag);
        assert_eq!(mag.len(), bins.len(), "the buffer is refilled, not grown");
        assert_eq!(reused, read(&bins, RATE, N));
    }

    #[test]
    fn silence_reads_as_nothing() {
        let t = read(&vec![Complex::new(0.0, 0.0); N / 2], RATE, N);
        assert_eq!(t, Tone::default());
    }
}
