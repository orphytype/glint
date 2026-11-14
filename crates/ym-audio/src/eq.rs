//! A 10-band graphic equaliser: one peaking biquad per band (RBJ audio-EQ cookbook), cascaded.
//!
//! The settings live in an [`EqShared`] of atomics so the UI can change them while audio is
//! playing; the audio thread notices the version bump and recomputes its coefficients.
//! Bands that sit at 0 dB are skipped, and a flat EQ costs nothing at all.

use std::f64::consts::PI;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

pub const BANDS: usize = 10;
/// Centre frequencies, one octave apart.
pub const BAND_HZ: [f32; BANDS] = [31.5, 63.0, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0];
pub const MAX_GAIN_DB: f32 = 12.0;
/// Quality factor: about an octave wide, so neighbouring bands blend into a smooth curve.
const Q: f64 = 1.2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EqSettings {
    pub enabled: bool,
    /// Extra gain before the bands, to make room for boosts.
    pub preamp_db: f32,
    pub gains_db: [f32; BANDS],
}

impl Default for EqSettings {
    fn default() -> Self {
        EqSettings { enabled: true, preamp_db: 0.0, gains_db: [0.0; BANDS] }
    }
}

/// Named curves for the UI.
pub const PRESETS: &[(&str, [f32; BANDS])] = &[
    ("flat", [0.0; BANDS]),
    ("bass", [6.0, 5.0, 4.0, 2.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    ("treble", [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 4.0, 5.0, 6.0]),
    ("vocal", [-2.0, -2.0, -1.0, 1.0, 3.0, 4.0, 3.0, 1.0, 0.0, -1.0]),
    ("rock", [4.0, 3.0, 2.0, -1.0, -2.0, -1.0, 1.0, 3.0, 4.0, 4.0]),
    ("pop", [-1.0, 1.0, 3.0, 4.0, 2.0, 0.0, -1.0, -1.0, 1.0, 2.0]),
    ("electronic", [5.0, 4.0, 1.0, 0.0, -2.0, 1.0, 0.0, 1.0, 4.0, 5.0]),
    ("acoustic", [3.0, 3.0, 2.0, 1.0, 2.0, 2.0, 3.0, 3.0, 2.0, 1.0]),
    ("classical", [3.0, 2.0, 1.0, 1.0, 0.0, 0.0, -1.0, -1.0, -2.0, -2.0]),
    ("loudness", [5.0, 3.0, 0.0, 0.0, -1.0, 0.0, 0.0, 2.0, 4.0, 3.0]),
];

impl EqSettings {
    pub fn preset(name: &str) -> Option<EqSettings> {
        PRESETS.iter().find(|(n, _)| *n == name).map(|(_, g)| EqSettings { gains_db: *g, ..EqSettings::default() })
    }

    /// The preset these gains match, if any.
    pub fn preset_name(&self) -> Option<&'static str> {
        PRESETS.iter().find(|(_, g)| g.iter().zip(&self.gains_db).all(|(a, b)| (a - b).abs() < 0.05)).map(|(n, _)| *n)
    }

    /// Limits every value to the supported range (and replaces NaNs by 0).
    pub fn sanitized(mut self) -> EqSettings {
        fn fix(v: f32) -> f32 {
            if v.is_finite() { v.clamp(-MAX_GAIN_DB, MAX_GAIN_DB) } else { 0.0 }
        }
        self.preamp_db = fix(self.preamp_db);
        self.gains_db = self.gains_db.map(fix);
        self
    }

    /// Headroom the engine takes off automatically when bands boost, so a bass boost does not clip.
    pub fn auto_preamp_db(&self) -> f32 {
        -0.6 * self.gains_db.iter().copied().fold(0.0, f32::max)
    }
}

/// The live copy of the settings, shared with the audio thread.
#[derive(Debug)]
pub struct EqShared {
    version: AtomicU32,
    enabled: AtomicBool,
    preamp: AtomicU32,
    gains: [AtomicU32; BANDS],
}

impl Default for EqShared {
    fn default() -> Self {
        EqShared {
            version: AtomicU32::new(1),
            enabled: AtomicBool::new(true),
            preamp: AtomicU32::new(0f32.to_bits()),
            gains: std::array::from_fn(|_| AtomicU32::new(0f32.to_bits())),
        }
    }
}

impl EqShared {
    pub fn set(&self, settings: &EqSettings) {
        let s = settings.sanitized();
        self.enabled.store(s.enabled, Ordering::Relaxed);
        self.preamp.store(s.preamp_db.to_bits(), Ordering::Relaxed);
        for (slot, g) in self.gains.iter().zip(s.gains_db) {
            slot.store(g.to_bits(), Ordering::Relaxed);
        }
        self.version.fetch_add(1, Ordering::Release);
    }

    pub fn get(&self) -> EqSettings {
        EqSettings {
            enabled: self.enabled.load(Ordering::Relaxed),
            preamp_db: f32::from_bits(self.preamp.load(Ordering::Relaxed)),
            gains_db: std::array::from_fn(|i| f32::from_bits(self.gains[i].load(Ordering::Relaxed))),
        }
    }

    fn version(&self) -> u32 {
        self.version.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Coef {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

/// RBJ peaking-EQ coefficients (normalised so a0 = 1).
fn peaking(fs: f64, f0: f64, q: f64, gain_db: f64) -> Coef {
    let a = 10f64.powf(gain_db / 40.0);
    let w0 = 2.0 * PI * f0 / fs;
    let (sin, cos) = w0.sin_cos();
    let alpha = sin / (2.0 * q);
    let a0 = 1.0 + alpha / a;
    Coef { b0: (1.0 + alpha * a) / a0, b1: -2.0 * cos / a0, b2: (1.0 - alpha * a) / a0, a1: -2.0 * cos / a0, a2: (1.0 - alpha / a) / a0 }
}

/// Per-track filter state (one chain of biquads per channel).
#[derive(Debug)]
pub struct EqFilter {
    rate: f64,
    seen: u32,
    /// Everything off: `process` returns its input untouched.
    bypass: bool,
    preamp: f64,
    coefs: [Coef; BANDS],
    active: usize,
    /// `[channel][band] -> [z1, z2]` (transposed direct form II).
    state: Vec<[[f64; 2]; BANDS]>,
}

impl EqFilter {
    pub fn new(channels: usize, rate: u32) -> EqFilter {
        EqFilter { rate: f64::from(rate.max(1)), seen: 0, bypass: true, preamp: 1.0, coefs: [Coef::default(); BANDS], active: 0, state: vec![[[0.0; 2]; BANDS]; channels.max(1)] }
    }

    /// Picks up new settings if they changed. Cheap enough to call every few hundred frames.
    pub fn refresh(&mut self, shared: &EqShared) {
        let version = shared.version();
        if version == self.seen {
            return;
        }
        self.seen = version;
        let s = shared.get();
        let mut active = 0;
        let before = self.active;
        // Bands above ~0.45 of the sampling rate cannot be represented and are dropped.
        for (hz, gain) in BAND_HZ.iter().zip(s.gains_db) {
            if gain.abs() >= 0.05 && f64::from(*hz) < self.rate * 0.45 {
                self.coefs[active] = peaking(self.rate, f64::from(*hz), Q, f64::from(gain));
                active += 1;
            }
        }
        self.active = active;
        let total_db = s.preamp_db + s.auto_preamp_db();
        self.preamp = 10f64.powf(f64::from(total_db) / 20.0);
        self.bypass = !s.enabled || (active == 0 && total_db.abs() < 0.01);
        if active != before {
            // The set of bands changed: stale state would click.
            self.reset();
        }
    }

    pub fn reset(&mut self) {
        self.state.fill([[0.0; 2]; BANDS]);
    }

    #[inline]
    pub fn process(&mut self, ch: usize, x: f32) -> f32 {
        if self.bypass {
            return x;
        }
        let last = self.state.len() - 1;
        let state = &mut self.state[ch.min(last)];
        let mut y = f64::from(x) * self.preamp;
        for (c, z) in self.coefs[..self.active].iter().zip(state.iter_mut()) {
            let out = c.b0 * y + z[0];
            z[0] = c.b1 * y - c.a1 * out + z[1];
            z[1] = c.b2 * y - c.a2 * out;
            y = out;
        }
        y as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// |H(e^jw)| in dB of one biquad.
    fn response_db(c: &Coef, fs: f64, hz: f64) -> f64 {
        let w = 2.0 * PI * hz / fs;
        let (c1, c2) = (w.cos(), (2.0 * w).cos());
        let num = c.b0 * c.b0 + c.b1 * c.b1 + c.b2 * c.b2 + 2.0 * (c.b0 * c.b1 + c.b1 * c.b2) * c1 + 2.0 * c.b0 * c.b2 * c2;
        let den = 1.0 + c.a1 * c.a1 + c.a2 * c.a2 + 2.0 * (c.a1 + c.a1 * c.a2) * c1 + 2.0 * c.a2 * c2;
        10.0 * (num / den).log10()
    }

    #[test]
    fn peaking_filter_hits_its_gain_at_the_centre_and_is_flat_far_away() {
        for gain in [-12.0, -6.0, 3.0, 9.0, 12.0] {
            let c = peaking(48_000.0, 1_000.0, Q, gain);
            assert!((response_db(&c, 48_000.0, 1_000.0) - gain).abs() < 0.01, "gain {gain}");
            assert!(response_db(&c, 48_000.0, 20.0).abs() < 0.5);
            assert!(response_db(&c, 48_000.0, 20_000.0).abs() < 0.5);
        }
        // 0 dB is the identity.
        assert!(response_db(&peaking(44_100.0, 250.0, Q, 0.0), 44_100.0, 250.0).abs() < 1e-9);
    }

    #[test]
    fn flat_eq_is_a_bypass() {
        let shared = EqShared::default();
        let mut f = EqFilter::new(2, 44_100);
        f.refresh(&shared);
        assert!(f.bypass);
        assert_eq!(f.process(0, 0.25), 0.25);
    }

    #[test]
    fn boost_amplifies_a_tone_at_the_band_and_not_elsewhere() {
        let shared = EqShared::default();
        let mut s = EqSettings::default();
        s.gains_db[5] = 12.0; // 1 kHz
        s.preamp_db = -auto_cancel(&s); // measure the filter alone
        shared.set(&s);
        let rms = |hz: f64| {
            let mut f = EqFilter::new(1, 44_100);
            f.refresh(&shared);
            let n = 44_100;
            let (mut e_in, mut e_out) = (0.0f64, 0.0f64);
            for i in 0..n {
                let x = (2.0 * PI * hz * i as f64 / 44_100.0).sin() as f32 * 0.1;
                let y = f.process(0, x);
                if i > n / 2 {
                    e_in += f64::from(x).powi(2);
                    e_out += f64::from(y).powi(2);
                }
            }
            10.0 * (e_out / e_in).log10()
        };
        assert!((rms(1_000.0) - 12.0).abs() < 0.3, "1 kHz: {}", rms(1_000.0));
        assert!(rms(60.0).abs() < 1.0, "60 Hz: {}", rms(60.0));
        assert!(rms(12_000.0).abs() < 1.0, "12 kHz: {}", rms(12_000.0));
    }

    /// The preamp that exactly undoes the automatic headroom.
    fn auto_cancel(s: &EqSettings) -> f32 {
        s.auto_preamp_db()
    }

    #[test]
    fn auto_preamp_makes_headroom_for_boosts() {
        let mut s = EqSettings::default();
        assert_eq!(s.auto_preamp_db(), 0.0);
        s.gains_db = [6.0, -3.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 12.0];
        assert!((s.auto_preamp_db() + 7.2).abs() < 1e-5);
        // Cuts alone need none.
        s.gains_db = [-6.0; BANDS];
        assert_eq!(s.auto_preamp_db(), 0.0);
    }

    #[test]
    fn settings_round_trip_and_are_sanitised() {
        let shared = EqShared::default();
        let mut s = EqSettings::preset("rock").unwrap();
        s.enabled = false;
        s.preamp_db = -2.5;
        shared.set(&s);
        assert_eq!(shared.get(), s);
        let wild = EqSettings { enabled: true, preamp_db: f32::NAN, gains_db: [99.0, -99.0, f32::INFINITY, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0] };
        shared.set(&wild);
        let got = shared.get();
        assert_eq!(got.preamp_db, 0.0);
        assert_eq!(&got.gains_db[..3], &[MAX_GAIN_DB, -MAX_GAIN_DB, 0.0]);
        assert_eq!(EqSettings::preset("rock").unwrap().preset_name(), Some("rock"));
        assert!(EqSettings::preset("nope").is_none());
    }

    #[test]
    fn changing_settings_is_noticed_and_disabling_bypasses() {
        let shared = EqShared::default();
        let mut f = EqFilter::new(2, 48_000);
        f.refresh(&shared);
        assert!(f.bypass);
        shared.set(&EqSettings::preset("bass").unwrap());
        f.refresh(&shared);
        assert!(!f.bypass && f.active > 0);
        let mut off = EqSettings::preset("bass").unwrap();
        off.enabled = false;
        shared.set(&off);
        f.refresh(&shared);
        assert!(f.bypass);
        assert_eq!(f.process(1, 0.5), 0.5);
    }

    #[test]
    fn bands_beyond_nyquist_are_dropped() {
        let shared = EqShared::default();
        let mut s = EqSettings::default();
        s.gains_db[9] = 6.0; // 16 kHz
        shared.set(&s);
        let mut low_rate = EqFilter::new(1, 22_050);
        low_rate.refresh(&shared);
        assert_eq!(low_rate.active, 0);
        let mut cd = EqFilter::new(1, 44_100);
        cd.refresh(&shared);
        assert_eq!(cd.active, 1);
    }
}
