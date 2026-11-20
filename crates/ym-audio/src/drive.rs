//! What the music is doing right now, the way a listener feels it: the kick, the snare or clap,
//! the hats, and the voice. Read from each frame's raw spectrum (not the smoothed bars), so a hit
//! lands on the frame it happens and a sung phrase is told apart from a loud chord.
//!
//! Hits are spectral flux (how much louder each bin got since the last frame, on a log scale)
//! within a band, measured against that band's own recent average and spread: a hit is how far
//! above its usual it stands, so a busy track and a sparse one both read their own accents. The
//! voice is the energy of the vocal range where it is tonal (low spectral flatness: harmonics
//! rather than noise), stretched over the range the track has used lately.

/// The reading: each value 0..1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Drive {
    /// The kick: jumps on a low-end hit, gone in about a fifth of a second.
    pub kick: f32,
    /// The snare or clap: a hit across the upper middle, gone a little quicker.
    pub snare: f32,
    /// The hats and cymbals: a hit up top, short.
    pub hats: f32,
    /// How much a voice (or a lead line) carries the music now: rises in a few hundredths of a
    /// second, lets go over a third.
    pub vocal: f32,
    /// A sung syllable starting: a hit in the vocal range while it is tonal.
    pub syllable: f32,
    /// Loudness, stretched over the range the track has used lately.
    pub energy: f32,
    /// This frame's strongest drum hit (not held): for picking out beats.
    pub onset: f32,
}

/// One band's hit detector: its recent average flux, the spread around it, and the held hit.
#[derive(Clone, Copy, Default)]
struct Hits {
    mean: f32,
    var: f32,
    held: f32,
    now: f32,
}

impl Hits {
    /// Takes this frame's `flux`, and holds a hit that fades with `decay` seconds.
    fn step(&mut self, flux: f32, dt: f32, decay: f32) -> f32 {
        let sd = self.var.max(0.0).sqrt();
        self.now = ((flux - self.mean - 0.4 * sd) / (2.6 * sd + 0.004)).clamp(0.0, 1.0);
        // The average forgets over a second and a half; a hit pulls it up only a little.
        let a = 1.0 - (-dt / 1.5).exp();
        let dev = flux.min(self.mean + 4.0 * sd + 0.02) - self.mean;
        self.mean += dev * a;
        self.var += (dev * dev - self.var) * a;
        self.held = (self.held * (-dt / decay).exp()).max(self.now);
        self.held
    }
}

/// A value read between the quietest and the loudest it has been lately.
#[derive(Clone, Copy)]
struct Gain {
    lo: f32,
    hi: f32,
}

impl Gain {
    const NEW: Gain = Gain { lo: 0.0, hi: 0.1 };

    fn read(&mut self, x: f32, dt: f32) -> f32 {
        self.lo = if x < self.lo { x } else { self.lo + (x - self.lo) * (1.0 - (-dt * 0.3).exp()) };
        self.hi = if x > self.hi { x } else { self.hi + (x - self.hi) * (1.0 - (-dt * 0.2).exp()) };
        ((x - self.lo) / (self.hi - self.lo).max(0.08)).clamp(0.0, 1.0)
    }
}

/// The bands, in Hz.
const KICK: (f32, f32) = (35.0, 150.0);
const SNARE: (f32, f32) = (1_500.0, 7_000.0);
const SNARE_BODY: (f32, f32) = (160.0, 320.0);
const HATS: (f32, f32) = (7_000.0, 16_000.0);
const VOICE: (f32, f32) = (220.0, 3_800.0);

/// Follows [`Drive`] frame by frame. Keep one per analyser.
pub struct DriveTracker {
    prev: Vec<f32>,
    log: Vec<f32>,
    kick: Hits,
    snare: Hits,
    hats: Hits,
    syllable: Hits,
    voice_gain: Gain,
    energy_gain: Gain,
    out: Drive,
}

impl Default for DriveTracker {
    fn default() -> Self {
        DriveTracker {
            prev: Vec::new(),
            log: Vec::new(),
            kick: Hits::default(),
            snare: Hits::default(),
            hats: Hits::default(),
            syllable: Hits::default(),
            voice_gain: Gain::NEW,
            energy_gain: Gain::NEW,
            out: Drive::default(),
        }
    }
}

impl DriveTracker {
    /// Reads one frame: `mag` is the magnitude of each FFT bin (the first half, `bin_hz` apart),
    /// scaled so a full-scale sine is 1; `level` the frame's loudness in 0..1.
    pub fn step(&mut self, mag: &[f32], bin_hz: f32, level: f32, dt: f32) -> Drive {
        let n = mag.len();
        if n < 8 {
            return self.quiet(dt);
        }
        self.log.clear();
        self.log.extend(mag.iter().map(|m| (1.0 + 200.0 * m).ln()));
        if self.prev.len() != n {
            self.prev = self.log.clone();
        }
        let range = |(lo, hi): (f32, f32)| ((lo / bin_hz).floor() as usize).clamp(1, n.max(2) - 1)..((hi / bin_hz).ceil() as usize).clamp(2, n.max(2));
        // How much louder the band got, per bin, per sixtieth of a second.
        let per = 1.0 / (dt * 60.0).clamp(0.25, 4.0);
        let flux = |r: std::ops::Range<usize>, log: &[f32], prev: &[f32]| {
            let len = r.len().max(1) as f32;
            r.map(|k| (log[k] - prev[k]).max(0.0)).sum::<f32>() / len * per
        };
        let f_kick = flux(range(KICK), &self.log, &self.prev);
        let f_snare = flux(range(SNARE), &self.log, &self.prev) + 0.5 * flux(range(SNARE_BODY), &self.log, &self.prev);
        let f_hats = flux(range(HATS), &self.log, &self.prev);
        let voice = range(VOICE);
        let f_voice = flux(voice.clone(), &self.log, &self.prev);
        self.prev.clone_from(&self.log);

        // The voice: the vocal range's power where it is tonal. Spectral flatness (geometric over
        // arithmetic mean of the power) is near 1 for noise, near 0 for a sung or played note.
        let (mut sum, mut log_sum) = (0.0f32, 0.0f32);
        for &m in &mag[voice.clone()] {
            let p = m * m + 1e-10;
            sum += p;
            log_sum += p.ln();
        }
        let len = voice.len().max(1) as f32;
        let flat = ((log_sum / len).exp() / (sum / len)).clamp(0.0, 1.0);
        let tonal = (1.0 - flat).powf(1.5);
        let db = 10.0 * (sum / len).max(1e-12).log10();
        let present = self.voice_gain.read(((db + 90.0) / 70.0).clamp(0.0, 1.0), dt) * tonal;
        let tau = if present > self.out.vocal { 0.04 } else { 0.32 };
        let vocal = self.out.vocal + (present - self.out.vocal) * (1.0 - (-dt / tau).exp());

        let kick = self.kick.step(f_kick, dt, 0.2);
        let snare = self.snare.step(f_snare, dt, 0.16);
        let hats = self.hats.step(f_hats, dt, 0.09);
        // A syllable is a hit in the vocal range that is not just the drums spilling into it.
        let syllable = self.syllable.step((f_voice - 0.5 * f_snare).max(0.0) * (0.3 + 0.7 * tonal), dt, 0.22);
        let energy = self.energy_gain.read(level, dt);
        let onset = self.kick.now.max(0.8 * self.snare.now);
        self.out = Drive { kick, snare, hats, vocal, syllable, energy, onset };
        self.out
    }

    /// Lets everything fall away (nothing plays).
    pub fn quiet(&mut self, dt: f32) -> Drive {
        let fade = (-dt / 0.15).exp();
        let o = &mut self.out;
        for v in [&mut o.kick, &mut o.snare, &mut o.hats, &mut o.vocal, &mut o.syllable, &mut o.energy] {
            *v *= fade;
        }
        o.onset = 0.0;
        self.prev.clear();
        self.out
    }
}
