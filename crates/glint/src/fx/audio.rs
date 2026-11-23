//! Smoothed audio readings, onset detection, harmony tracking and cover palette easing.

use crate::app::State;

/// What the stage remembers of the song: a clock that only runs while music plays, the readings
/// of the music eased so none of them jumps, and the harmony's direction.
pub(super) struct Song {
    pub(super) clock: f32,
    pub(super) spec: [f32; 32],
    spec_hi: [f32; 32],
    pub(super) bass: f32,
    pub(super) voice: f32,
    pub(super) treble: f32,
    pub(super) kick: f32,
    pub(super) snare: f32,
    pub(super) loud: f32,
    /// The beat as the lively stage shows it: follows `Smooth::beat` in over ~45 ms and out over
    /// ~260 ms, so the rays swell on the hit and settle without twitching.
    pub(super) throb: f32,
    chroma: [f32; 12],
    key: [f32; 2],
    pub(super) ground_t: f32,
    pub(super) peak: [f32; 2],
    pub(super) peak_primed: bool,
}

impl Song {
    pub(super) fn new() -> Song {
        Song {
            clock: 0.0,
            spec: [0.0; 32],
            spec_hi: [0.05; 32],
            bass: 0.0,
            voice: 0.0,
            treble: 0.0,
            kick: 0.0,
            snare: 0.0,
            loud: 0.0,
            throb: 0.0,
            chroma: [0.0; 12],
            key: [0.0, 0.0],
            ground_t: 0.0,
            peak: [0.5, 0.5],
            peak_primed: false,
        }
    }

    pub(super) fn hear(&mut self, dt: f32, tone: &ym_audio::Tone, bands: &[f32; 32], drive: &ym_audio::Drive, loud: f32, live: bool) {
        let ease = |x: &mut f32, to: f32, tau: f32| *x += (to - *x) * (1.0 - (-dt / tau).exp());
        for i in 0..32 {
            let b = if live { bands[i].max(0.0) } else { 0.0 };
            let hi = &mut self.spec_hi[i];
            *hi = if b > *hi { b } else { (*hi + (b - *hi) * (1.0 - (-dt / 6.0).exp())).max(0.03) };
            let to = (b / *hi).clamp(0.0, 1.0);
            let tau = if to > self.spec[i] { 0.035 } else { 0.16 };
            ease(&mut self.spec[i], to, tau);
        }
        let mean = |r: std::ops::Range<usize>| self.spec[r.clone()].iter().sum::<f32>() / r.len() as f32;
        let d = if live { *drive } else { ym_audio::Drive::default() };
        self.bass = mean(0..5).max(0.6 * self.kick);
        self.voice = (d.vocal * (0.75 + 0.25 * mean(8..20)) + 0.45 * d.syllable).min(1.0);
        self.treble = (0.6 * mean(19..32)).max(d.hats);
        let (kick, snare) = (d.kick * 0.8, d.snare * 0.7);
        let (kick_tau, snare_tau) = (if kick > self.kick { 0.06 } else { 0.34 }, if snare > self.snare { 0.06 } else { 0.30 });
        ease(&mut self.kick, kick, kick_tau);
        ease(&mut self.snare, snare, snare_tau);
        for (c, t) in self.chroma.iter_mut().zip(tone.chroma) {
            let to = if live { t } else { 0.0 };
            let tau = if to > *c { 0.12 } else { 0.6 };
            ease(c, to, tau);
        }
        let heard = live && tone.pitch > 0.0;
        ease(&mut self.loud, if live { loud } else { 0.0 }, 0.15);
        if heard {
            let h = self.harmony();
            for j in 0..2 {
                ease(&mut self.key[j], h[j] * h[2], 6.0);
            }
        }
    }

    pub(super) fn key_dir(&self) -> [f32; 2] {
        let l = (self.key[0] * self.key[0] + self.key[1] * self.key[1]).sqrt();
        if l < 1e-4 { [1.0, 0.0] } else { [self.key[0] / l, self.key[1] / l] }
    }

    pub(super) fn follow_beat(&mut self, dt: f32, beat: f32) {
        let tau = if beat > self.throb { 0.045 } else { 0.26 };
        self.throb += (beat.clamp(0.0, 1.0) - self.throb) * (1.0 - (-dt / tau).exp());
    }

    pub(super) fn advance(&mut self, dt: f32) {
        self.clock += dt;
    }

    /// Harmony as a direction on the circle of fifths (cos, sin) and its strength.
    pub(super) fn harmony(&self) -> [f32; 3] {
        let (mut x, mut y, mut tot) = (0.0, 0.0, 0.0);
        for (i, c) in self.chroma.iter().enumerate() {
            let a = ((i * 7) % 12) as f32 * std::f32::consts::TAU / 12.0;
            x += c * a.cos();
            y += c * a.sin();
            tot += c;
        }
        let len = (x * x + y * y).sqrt();
        if len < 1e-4 { [1.0, 0.0, 0.0] } else { [x / len, y / len, (len / tot.max(1e-4)).min(1.0)] }
    }
}

/// Smoothed palette colors and normalized audio bands read by the shaders.
pub(super) struct Smooth {
    /// Only palette slots 0, 2 and 4 are consumed by `stage.wgsl`.
    pub(super) pal: [[f32; 3]; 3],
    pub(super) accent: [f32; 3],
    pub(super) level: f32,
    pub(super) beat: f32,
    primed: bool,
    /// Auto-gained bass, mid, treble and loudness in `0..1`.
    pub(super) norm: [f32; 4],
    range: [[f32; 2]; 4],
    since_onset: f32,
    pulse: f32,
    pub(super) tempo: f32,
}

impl Smooth {
    const ONSET_GAP_S: f32 = 0.4;

    pub(super) fn new() -> Smooth {
        Smooth {
            pal: [[0.0; 3]; 3],
            accent: [0.6, 0.5, 1.0],
            level: 0.0,
            beat: 0.0,
            primed: false,
            norm: [0.0; 4],
            range: [[0.0, 0.1]; 4],
            since_onset: 1.0,
            pulse: 0.0,
            tempo: 120.0,
        }
    }

    fn listen(&mut self, dt: f32, st: &State) {
        let mean = |b: &[f32; 32], r: std::ops::Range<usize>| b[r.clone()].iter().sum::<f32>() / r.len() as f32;
        let b = &st.spectrum;
        let raw = [mean(b, 0..5), mean(b, 5..16), mean(b, 16..32), st.level.clamp(0.0, 1.0)];
        let on = st.player.playing && raw[3] > 0.002;
        for (j, x) in raw.into_iter().enumerate() {
            let [lo, hi] = &mut self.range[j];
            if on {
                *lo = if x < *lo { x } else { *lo + (x - *lo) * (1.0 - (-dt * 0.35).exp()) };
                *hi = if x > *hi { x } else { *hi + (x - *hi) * (1.0 - (-dt * 0.22).exp()) };
            }
            let span = (*hi - *lo).max(0.06);
            let n = if on { ((x - *lo) / span).clamp(0.0, 1.0) } else { 0.0 };
            let k = 1.0 - (-dt * if n > self.norm[j] { 10.0 } else { 4.0 }).exp();
            self.norm[j] += (n - self.norm[j]) * k;
        }
        let hit = st.drive.onset;
        self.since_onset += dt;
        if on && self.since_onset > Self::ONSET_GAP_S && hit > 0.45 {
            let period = self.since_onset;
            if (0.25..=1.5).contains(&period) {
                let target = (60.0 / period).clamp(60.0, 180.0);
                self.tempo += (target - self.tempo) * 0.18;
            }
            self.since_onset = 0.0;
            self.pulse = self.pulse.max(hit.clamp(0.45, 1.0));
        }
        self.pulse *= (-dt * 2.6).exp();
        if !on {
            self.pulse *= (-dt * 6.0).exp();
        }
    }

    pub(super) fn stirring(&self) -> bool {
        self.pulse > 0.01 || self.norm.iter().any(|n| *n > 0.01)
    }

    pub(super) fn update(&mut self, dt: f32, st: &State) {
        let target = st.palette;
        let pal_target = [target.colors[0], target.colors[2], target.colors[4]];
        if !self.primed {
            self.pal = pal_target;
            self.accent = target.accent;
            self.primed = true;
        }
        let k = 1.0 - (-dt * 2.4).exp();
        for (c, t) in self.pal.iter_mut().zip(pal_target) {
            for i in 0..3 {
                c[i] += (t[i] - c[i]) * k;
            }
        }
        for i in 0..3 {
            self.accent[i] += (target.accent[i] - self.accent[i]) * k;
        }
        let a = 1.0 - (-dt * 12.0).exp();
        self.level += (st.level.clamp(0.0, 1.0) - self.level) * a;
        self.listen(dt, st);
        let d = &st.drive;
        self.beat = (self.beat * (-dt * 5.5).exp()).max(d.kick).max(0.75 * d.snare).max(self.pulse * 0.9);
    }
}
