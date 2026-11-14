//! Procedural music for the offline demo catalogue.
//!
//! Demo mode must sound like a music app without any account, so every demo track is composed and
//! rendered here from a seed: a key, a four-chord loop, a bass line, drums, an arpeggio or chord
//! stabs and a melody, then a ping-pong delay, a small reverb and a soft master limiter. The same
//! seed always yields the same bytes.

use std::f32::consts::{PI, TAU};

use crate::wav;

pub const RATE: u32 = 44_100;
const SR: f32 = RATE as f32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Style {
    /// Laid-back beats and electric-piano chords.
    Chill,
    /// Saw pads, a busy arpeggio, gated bass.
    Synthwave,
    /// Slow washes and bells, no drums.
    Ambient,
    /// Four-on-the-floor with chord stabs.
    House,
}

impl Style {
    pub const ALL: [Style; 4] = [Style::Chill, Style::Synthwave, Style::Ambient, Style::House];

    pub fn name(self) -> &'static str {
        match self {
            Style::Chill => "chill",
            Style::Synthwave => "synthwave",
            Style::Ambient => "ambient",
            Style::House => "house",
        }
    }

    /// Tempo range in BPM.
    pub fn bpm_range(self) -> (u32, u32) {
        match self {
            Style::Chill => (76, 92),
            Style::Synthwave => (100, 116),
            Style::Ambient => (60, 76),
            Style::House => (120, 126),
        }
    }
}

/// Interleaved stereo samples at [`RATE`].
pub fn render(seed: u64, style: Style, secs: f32) -> Vec<f32> {
    Composer::new(seed, style, secs.clamp(4.0, 600.0)).render()
}

/// [`render`] as a 16-bit WAV file, ready for the decoder.
pub fn demo_wav(seed: u64, style: Style, secs: f32) -> Vec<u8> {
    wav::encode(&render(seed, style, secs), 2, RATE)
}

// ---------------------------------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    /// SplitMix64.
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// In `0.0..1.0`.
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// In `-1.0..1.0`.
    fn noise(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn chance(&mut self, p: f32) -> bool {
        self.unit() < p
    }
}

fn mtof(midi: f32) -> f32 {
    440.0 * ((midi - 69.0) / 12.0).exp2()
}

/// Where a note lands: stereo gains plus how much goes to the effect sends.
#[derive(Clone, Copy)]
struct Out {
    gl: f32,
    gr: f32,
    rev: f32,
    dly: f32,
}

impl Out {
    fn new(pan: f32, rev: f32, dly: f32) -> Out {
        let a = (pan.clamp(-1.0, 1.0) + 1.0) * 0.25 * PI;
        Out { gl: a.cos(), gr: a.sin(), rev, dly }
    }
}

struct Mix {
    l: Vec<f32>,
    r: Vec<f32>,
    rev: Vec<f32>,
    dly: Vec<f32>,
}

impl Mix {
    fn new(frames: usize) -> Mix {
        Mix { l: vec![0.0; frames], r: vec![0.0; frames], rev: vec![0.0; frames], dly: vec![0.0; frames] }
    }

    #[inline]
    fn put(&mut self, i: usize, v: f32, o: Out) {
        if let Some(l) = self.l.get_mut(i) {
            *l += v * o.gl;
            self.r[i] += v * o.gr;
            self.rev[i] += v * o.rev;
            self.dly[i] += v * o.dly;
        }
    }
}

// -- Instruments ---------------------------------------------------------------------------------

fn kick(m: &mut Mix, start: usize, vel: f32) {
    let out = Out::new(0.0, 0.0, 0.0);
    let mut phase = 0.0f32;
    for i in 0..(0.38 * SR) as usize {
        let t = i as f32 / SR;
        phase += TAU * (44.0 + 125.0 * (-t * 30.0).exp()) / SR;
        let click = if i < 48 { (1.0 - i as f32 / 48.0) * 0.35 } else { 0.0 };
        m.put(start + i, (phase.sin() * (-t * 8.0).exp() + click) * vel, out);
    }
}

fn snare(m: &mut Mix, start: usize, vel: f32, rng: &mut Rng) {
    let out = Out::new(0.04, 0.22, 0.0);
    let mut prev = 0.0;
    for i in 0..(0.24 * SR) as usize {
        let t = i as f32 / SR;
        let n = rng.noise();
        let hp = n - prev * 0.55;
        prev = n;
        let body = (TAU * 187.0 * t).sin() * (-t * 26.0).exp();
        m.put(start + i, (0.42 * body + 0.58 * hp * (-t * 19.0).exp()) * vel, out);
    }
}

fn clap(m: &mut Mix, start: usize, vel: f32, rng: &mut Rng) {
    let out = Out::new(-0.05, 0.3, 0.0);
    let mut prev = 0.0;
    for i in 0..(0.3 * SR) as usize {
        let t = i as f32 / SR;
        let n = rng.noise();
        let hp = n - prev * 0.5;
        prev = n;
        // Three quick bursts, then a tail.
        let burst = [0.0, 0.011, 0.022].iter().map(|b| if t >= *b { (-(t - b) * 90.0).exp() } else { 0.0 }).fold(0.0, f32::max);
        m.put(start + i, hp * (burst * 0.7 + (-t * 14.0).exp() * 0.3) * vel, out);
    }
}

fn hat(m: &mut Mix, start: usize, vel: f32, open: bool, pan: f32, rng: &mut Rng) {
    let out = Out::new(pan, 0.1, 0.0);
    let (len, decay) = if open { (0.28, 12.0) } else { (0.06, 85.0) };
    let mut prev = 0.0;
    for i in 0..(len * SR) as usize {
        let t = i as f32 / SR;
        let n = rng.noise();
        let hp = n - prev;
        prev = n;
        m.put(start + i, hp * 0.5 * (-t * decay).exp() * vel, out);
    }
}

fn bass(m: &mut Mix, start: usize, len: usize, midi: f32, vel: f32, saw: bool) {
    let out = Out::new(0.0, 0.0, 0.0);
    let f = mtof(midi);
    let (mut phase, mut lp) = (0.0f32, 0.0f32);
    let (att, rel) = ((0.004 * SR) as usize, (0.05 * SR) as usize);
    let a = 1.0 - (-TAU * 420.0 / SR).exp();
    for i in 0..len + rel {
        phase = (phase + f / SR).fract();
        let sub = (TAU * phase).sin();
        let tone = if saw {
            lp += a * ((2.0 * phase - 1.0) - lp);
            0.65 * lp + 0.55 * sub
        } else {
            sub + 0.3 * (2.0 * TAU * phase).sin()
        };
        let env = if i < att {
            i as f32 / att as f32
        } else if i < len {
            1.0
        } else {
            1.0 - (i - len) as f32 / rel as f32
        };
        m.put(start + i, (tone * 1.5).tanh() * 0.7 * env * vel, out);
    }
}

/// A detuned-saw pad voice through a gentle low-pass.
fn pad(m: &mut Mix, start: usize, len: usize, midi: f32, vel: f32, pan: f32, bright: f32) {
    let out = Out::new(pan, 0.35, 0.0);
    let f = mtof(midi);
    let detune = [0.9948, 1.0, 1.0052];
    let mut ph = [0.0f32, 0.31, 0.67];
    let a = 1.0 - (-TAU * (500.0 + 2600.0 * bright) / SR).exp();
    let (att, rel) = ((0.4 * SR) as usize, (0.8 * SR) as usize);
    let mut y = 0.0f32;
    for i in 0..len + rel {
        let mut s = 0.0;
        for (p, d) in ph.iter_mut().zip(detune) {
            *p = (*p + f * d / SR).fract();
            s += 2.0 * *p - 1.0;
        }
        y += a * (s / 3.0 - y);
        let env = if i < att {
            i as f32 / att as f32
        } else if i < len {
            1.0
        } else {
            1.0 - (i - len) as f32 / rel as f32
        };
        m.put(start + i, y * env * vel, out);
    }
}

/// A plucked saw whose filter closes as it decays (arpeggios and house stabs).
fn pluck(m: &mut Mix, start: usize, midi: f32, vel: f32, pan: f32, decay: f32, dly: f32) {
    let out = Out::new(pan, 0.25, dly);
    let f = mtof(midi);
    let (mut p1, mut p2, mut y) = (0.0f32, 0.5f32, 0.0f32);
    for i in 0..(0.55 * SR) as usize {
        let t = i as f32 / SR;
        let a = 1.0 - (-TAU * (500.0 + 4800.0 * (-t * 10.0).exp()) / SR).exp();
        p1 = (p1 + f / SR).fract();
        p2 = (p2 + f * 1.004 / SR).fract();
        y += a * ((2.0 * p1 - 1.0) * 0.6 + (2.0 * p2 - 1.0) * 0.4 - y);
        m.put(start + i, y * (-t * decay).exp() * (1.0 - (-t * 400.0).exp()) * vel, out);
    }
}

/// An electric-piano / bell tone: a few decaying partials with a little tremolo.
fn keys(m: &mut Mix, start: usize, len: usize, midi: f32, vel: f32, pan: f32, bell: bool) {
    let out = Out::new(pan, 0.4, if bell { 0.35 } else { 0.1 });
    let f = mtof(midi);
    let rel = (0.35 * SR) as usize;
    let (d1, d2, d3) = if bell { (0.8, 1.6, 3.0) } else { (1.7, 3.2, 8.0) };
    for i in 0..len + rel {
        let t = i as f32 / SR;
        let w = TAU * f * t;
        let tone = w.sin() * (-t * d1).exp() + 0.42 * (2.0 * w).sin() * (-t * d2).exp() + 0.28 * (if bell { 3.01 } else { 4.0 } * w).sin() * (-t * d3).exp();
        let trem = 1.0 + 0.12 * (TAU * 4.6 * t).sin();
        let env = (1.0 - (-t * 500.0).exp()) * if i < len { 1.0 } else { 1.0 - (i - len) as f32 / rel as f32 };
        m.put(start + i, tone * trem * env * vel, out);
    }
}

/// A soft triangle lead with vibrato that fades in.
fn lead(m: &mut Mix, start: usize, len: usize, midi: f32, vel: f32, pan: f32) {
    let out = Out::new(pan, 0.3, 0.4);
    let f = mtof(midi);
    let mut phase = 0.0f32;
    let (att, rel) = ((0.014 * SR) as usize, (0.16 * SR) as usize);
    for i in 0..len + rel {
        let t = i as f32 / SR;
        let vib = 1.0 + 0.0035 * (TAU * 5.6 * t).sin() * (t * 6.0).min(1.0);
        phase = (phase + f * vib / SR).fract();
        let tri = 4.0 * (phase - 0.5).abs() - 1.0;
        let env = if i < att {
            i as f32 / att as f32
        } else if i < len {
            1.0
        } else {
            1.0 - (i - len) as f32 / rel as f32
        };
        m.put(start + i, (0.8 * tri + 0.2 * (TAU * phase).sin()) * env * vel, out);
    }
}

// -- Effects ----------------------------------------------------------------------------------

struct Comb {
    buf: Vec<f32>,
    idx: usize,
    lp: f32,
}

impl Comb {
    fn new(n: usize) -> Comb {
        Comb { buf: vec![0.0; n], idx: 0, lp: 0.0 }
    }

    #[inline]
    fn run(&mut self, x: f32, feedback: f32, damp: f32) -> f32 {
        let y = self.buf[self.idx];
        self.lp += damp * (y - self.lp);
        self.buf[self.idx] = x + self.lp * feedback;
        self.idx = (self.idx + 1) % self.buf.len();
        y
    }
}

struct Allpass {
    buf: Vec<f32>,
    idx: usize,
}

impl Allpass {
    fn new(n: usize) -> Allpass {
        Allpass { buf: vec![0.0; n], idx: 0 }
    }

    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        let delayed = self.buf[self.idx];
        let y = delayed - 0.5 * x;
        self.buf[self.idx] = x + 0.5 * delayed;
        self.idx = (self.idx + 1) % self.buf.len();
        y
    }
}

/// A Schroeder reverb: four combs and two all-passes per side, slightly detuned between sides.
fn reverb(input: &[f32], feedback: f32) -> (Vec<f32>, Vec<f32>) {
    let side = |extra: usize| {
        let mut combs = [1557 + extra, 1617 + extra, 1491 + extra, 1422 + extra].map(Comb::new);
        let mut aps = [Allpass::new(225 + extra), Allpass::new(556 + extra)];
        input
            .iter()
            .map(|&x| {
                let x = x * 0.2;
                let mut y: f32 = combs.iter_mut().map(|c| c.run(x, feedback, 0.3)).sum();
                for ap in &mut aps {
                    y = ap.run(y);
                }
                y
            })
            .collect::<Vec<f32>>()
    };
    (side(0), side(23))
}

/// A ping-pong delay: the echo alternates sides and darkens as it repeats.
fn pingpong(input: &[f32], delay: usize, feedback: f32) -> (Vec<f32>, Vec<f32>) {
    let delay = delay.max(1);
    let (mut bl, mut br) = (vec![0.0f32; delay], vec![0.0f32; delay]);
    let (mut lp_l, mut lp_r) = (0.0f32, 0.0f32);
    let (mut out_l, mut out_r) = (Vec::with_capacity(input.len()), Vec::with_capacity(input.len()));
    let mut idx = 0;
    for &x in input {
        let (dl, dr) = (bl[idx], br[idx]);
        out_l.push(dl);
        out_r.push(dr);
        lp_l += 0.4 * (dl - lp_l);
        lp_r += 0.4 * (dr - lp_r);
        bl[idx] = x + lp_r * feedback;
        br[idx] = lp_l * feedback;
        idx = (idx + 1) % delay;
    }
    (out_l, out_r)
}

// -- Composition ------------------------------------------------------------------------------

const MINOR: [i32; 7] = [0, 2, 3, 5, 7, 8, 10];
const MAJOR: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];
const DORIAN: [i32; 7] = [0, 2, 3, 5, 7, 9, 10];

/// Semitones above the tonic of scale degree `degree` (any integer, octaves carry).
fn degree_semitones(scale: &[i32; 7], degree: i32) -> i32 {
    scale[degree.rem_euclid(7) as usize] + 12 * degree.div_euclid(7)
}

/// A diatonic chord (stacked thirds) as semitones above the tonic.
fn chord(scale: &[i32; 7], degree: i32, sevenths: bool) -> Vec<i32> {
    let n = if sevenths { 4 } else { 3 };
    (0..n).map(|k| degree_semitones(scale, degree + 2 * k)).collect()
}

/// Folds a pitch into `[lo, lo + 12)`.
fn fold(midi: i32, lo: i32) -> i32 {
    lo + (midi - lo).rem_euclid(12)
}

struct Composer {
    rng: Rng,
    style: Style,
    frames: usize,
}

impl Composer {
    fn new(seed: u64, style: Style, secs: f32) -> Composer {
        Composer { rng: Rng::new(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ style as u64), style, frames: (secs * SR) as usize }
    }

    fn render(mut self) -> Vec<f32> {
        let style = self.style;
        let rng_seed = self.rng.next();
        let mut rng = Rng::new(rng_seed);
        let (lo, hi) = style.bpm_range();
        let bpm = (lo + rng.below((hi - lo + 1) as usize) as u32) as f32;
        let beat = 60.0 / bpm * SR;
        let step = beat / 4.0;
        let bar = beat * 4.0;
        let bars = (self.frames as f32 / bar).ceil() as usize + 1;
        let swing = if style == Style::Chill { 0.18 * step } else { 0.0 };

        let tonic = 36 + rng.below(12) as i32; // C2..B2
        let scale = match style {
            Style::Chill => *[&MINOR, &DORIAN][rng.below(2)],
            Style::Synthwave | Style::House => *[&MINOR, &DORIAN, &MINOR][rng.below(3)],
            Style::Ambient => *[&MAJOR, &DORIAN, &MINOR][rng.below(3)],
        };
        let minorish = scale[2] == 3;
        let progression: [i32; 4] = if minorish {
            *[[0, 5, 2, 6], [0, 3, 5, 4], [0, 6, 5, 6], [0, 5, 3, 6]].get(rng.below(4)).unwrap_or(&[0, 5, 2, 6])
        } else {
            *[[0, 4, 5, 3], [0, 5, 3, 4], [0, 3, 4, 3]].get(rng.below(3)).unwrap_or(&[0, 4, 5, 3])
        };
        let sevenths = matches!(style, Style::Chill | Style::House | Style::Ambient);

        let motif = self.motif(&mut rng, &scale, &progression, sevenths);
        let mut mix = Mix::new(self.frames);
        let at = |b: usize, s: f32| (b as f32 * bar + s * step) as usize;
        let outro = bars.saturating_sub(3);

        for b in 0..bars {
            let degree = progression[b % 4];
            let notes: Vec<i32> = chord(&scale, degree, sevenths);
            let root = tonic + notes[0];
            let phrase_end = b % 4 == 3;
            let drums_on = b >= 2 && b < outro && style != Style::Ambient;
            let bass_on = b >= 1 && b < outro + 1;
            let melody_on = b >= 4 && b < outro;

            // Harmony.
            match style {
                Style::Chill => {
                    // Electric-piano stabs on the 1 and the "and" of 2, wide voicing.
                    for (s, len, vel) in [(0.0, 5.0, 0.2), (6.0, 3.0, 0.15), (10.0, 4.0, 0.13)] {
                        if s > 0.0 && b < 2 {
                            continue;
                        }
                        for (k, n) in notes.iter().enumerate() {
                            let midi = fold(tonic + n + 24, 55) as f32;
                            keys(&mut mix, at(b, s) + swing as usize, (len * step) as usize, midi, vel, -0.35 + 0.23 * k as f32, false);
                        }
                    }
                }
                Style::Synthwave | Style::Ambient | Style::House => {
                    let long = style == Style::Ambient;
                    if !long || b % 2 == 0 {
                        let len = if long { (bar * 2.0) as usize } else { bar as usize };
                        let (vel, bright) = match style {
                            Style::Synthwave => (0.085, 0.55),
                            Style::Ambient => (0.1, 0.25),
                            _ => (0.05, 0.4),
                        };
                        for (k, n) in notes.iter().enumerate() {
                            let midi = fold(tonic + n + 12, 52) as f32 + if k == 0 { -12.0 } else { 0.0 };
                            pad(&mut mix, at(b, 0.0), len, midi, vel, -0.5 + 0.34 * k as f32, bright);
                        }
                    }
                }
            }

            // Bass.
            if bass_on {
                let r = (root + 12 * i32::from(style != Style::Chill)) as f32;
                match style {
                    Style::Chill => {
                        bass(&mut mix, at(b, 0.0) + swing as usize / 2, (6.0 * step) as usize, r, 0.5, false);
                        bass(&mut mix, at(b, 10.0), (4.0 * step) as usize, r + if rng.chance(0.5) { 7.0 } else { 12.0 }, 0.42, false);
                    }
                    Style::Synthwave => {
                        for s in (0..16).step_by(2) {
                            let oct = if s % 8 == 4 { 12.0 } else { 0.0 };
                            bass(&mut mix, at(b, s as f32), (1.7 * step) as usize, r + oct - 12.0, 0.4, true);
                        }
                    }
                    Style::House => {
                        for s in [2, 6, 10, 14] {
                            bass(&mut mix, at(b, s as f32), (1.6 * step) as usize, r - 12.0 + if s == 14 && rng.chance(0.4) { 7.0 } else { 0.0 }, 0.5, true);
                        }
                    }
                    Style::Ambient => bass(&mut mix, at(b, 0.0), (bar * 0.95) as usize, r - 12.0, 0.4, false),
                }
            }

            // Arpeggio / stabs.
            if b >= 2 && b < outro {
                match style {
                    Style::Synthwave => {
                        for s in 0..16 {
                            let n = notes[(s * 7 / 4) % notes.len()] + 12 * ((s / 3) % 3) as i32;
                            let accent = if s % 4 == 0 { 1.0 } else { 0.7 };
                            pluck(&mut mix, at(b, s as f32), (tonic + n + 36) as f32, 0.12 * accent, if s % 2 == 0 { -0.45 } else { 0.45 }, 7.0, 0.45);
                        }
                    }
                    Style::House => {
                        for s in [3, 6, 10, 13] {
                            for (k, n) in notes.iter().enumerate() {
                                pluck(&mut mix, at(b, s as f32), fold(tonic + n + 36, 60) as f32, 0.07, -0.3 + 0.2 * k as f32, 9.0, 0.3);
                            }
                        }
                    }
                    _ => {}
                }
            }

            // Drums.
            if drums_on {
                let drop = b % 8 == 7 && style != Style::Chill; // a short breakdown before the next phrase
                match style {
                    Style::Chill => {
                        for s in [0, 7, 10] {
                            kick(&mut mix, at(b, s as f32), if s == 0 { 0.62 } else { 0.5 });
                        }
                        if phrase_end && rng.chance(0.5) {
                            kick(&mut mix, at(b, 14.0), 0.45);
                        }
                        for s in [4, 12] {
                            snare(&mut mix, at(b, s as f32), 0.4, &mut rng);
                        }
                        for s in 0..16 {
                            let sw = if s % 2 == 1 { swing as usize } else { 0 };
                            if s % 2 == 0 {
                                hat(&mut mix, at(b, s as f32) + sw, if s % 4 == 0 { 0.5 } else { 0.32 }, false, 0.25, &mut rng);
                            } else if rng.chance(0.3) {
                                hat(&mut mix, at(b, s as f32) + sw, 0.16, false, 0.3, &mut rng);
                            }
                        }
                    }
                    Style::Synthwave => {
                        if !drop {
                            for s in [0, 8] {
                                kick(&mut mix, at(b, s as f32), 0.66);
                            }
                            if rng.chance(0.4) {
                                kick(&mut mix, at(b, 10.0), 0.5);
                            }
                        }
                        for s in [4, 12] {
                            snare(&mut mix, at(b, s as f32), 0.5, &mut rng);
                        }
                        for s in (0..16).step_by(2) {
                            hat(&mut mix, at(b, s as f32), 0.3, s == 14, 0.25, &mut rng);
                        }
                        if phrase_end {
                            for s in [12, 13, 14, 15] {
                                snare(&mut mix, at(b, s as f32), 0.22 + 0.06 * (s - 12) as f32, &mut rng);
                            }
                        }
                    }
                    Style::House => {
                        if !drop {
                            for s in [0, 4, 8, 12] {
                                kick(&mut mix, at(b, s as f32), 0.74);
                            }
                        }
                        for s in [4, 12] {
                            clap(&mut mix, at(b, s as f32), 0.4, &mut rng);
                        }
                        for s in [2, 6, 10, 14] {
                            hat(&mut mix, at(b, s as f32), 0.42, true, -0.2, &mut rng);
                        }
                        for s in (1..16).step_by(2) {
                            hat(&mut mix, at(b, s as f32), 0.14, false, 0.3, &mut rng);
                        }
                    }
                    Style::Ambient => {}
                }
            }

            // Melody (and, in the ambient style, sparse bells in its place).
            if melody_on || (style == Style::Ambient && b >= 1 && b < outro) {
                let bell = style == Style::Ambient;
                for &(s, len, degree, vel) in &motif[b % 4] {
                    if bell && !rng.chance(0.45) {
                        continue;
                    }
                    let midi = tonic + degree_semitones(&scale, degree) + 36;
                    if bell {
                        keys(&mut mix, at(b, s as f32), (len as f32 * step) as usize, midi as f32 + 12.0, 0.16, rng.noise() * 0.6, true);
                    } else if style == Style::Chill {
                        keys(&mut mix, at(b, s as f32) + swing as usize, (len as f32 * step) as usize, midi as f32, vel * 0.8, 0.25, false);
                    } else {
                        lead(&mut mix, at(b, s as f32), (len as f32 * step) as usize, midi as f32, vel * 0.85, 0.15);
                    }
                }
            }
        }

        self.master(mix, beat)
    }

    /// Four bars of melody: `(start step, length in steps, scale degree, velocity)` per bar.
    fn motif(&mut self, rng: &mut Rng, scale: &[i32; 7], progression: &[i32; 4], sevenths: bool) -> Vec<Vec<(u32, u32, i32, f32)>> {
        const RHYTHMS: [&[(u32, u32)]; 5] = [
            &[(0, 4), (4, 2), (6, 2), (8, 4), (12, 4)],
            &[(0, 6), (6, 2), (8, 2), (10, 2), (12, 4)],
            &[(2, 2), (4, 4), (8, 2), (10, 6)],
            &[(0, 2), (3, 3), (8, 8)],
            &[(0, 3), (3, 3), (6, 2), (8, 4), (12, 2), (14, 2)],
        ];
        let mut at = 14 + rng.below(5) as i32; // scale degree, around the 2nd octave
        progression
            .iter()
            .map(|&degree| {
                let tones = chord(scale, degree, sevenths);
                let rhythm = RHYTHMS[rng.below(RHYTHMS.len())];
                rhythm
                    .iter()
                    .map(|&(s, len)| {
                        if s % 4 == 0 {
                            // On the beat: a chord tone near where we are.
                            let target = (0..3).flat_map(|o| tones.iter().map(move |t| (t, o))).map(|(t, o)| {
                                // express the chord tone as a degree in the current octave range
                                let semis = t + 12 * (o + 1);
                                (0..30).min_by_key(|d| (degree_semitones(scale, *d) - semis).abs()).unwrap_or(14)
                            });
                            at = target.min_by_key(|d| (d - at).abs()).unwrap_or(at);
                        } else {
                            at += [-2, -1, -1, 1, 1, 2][rng.below(6)];
                        }
                        at = at.clamp(10, 22);
                        (s, len, at, 0.2 + 0.05 * rng.unit())
                    })
                    .collect()
            })
            .collect()
    }

    /// Delay, reverb, soft limiting, level and fades.
    fn master(&self, mix: Mix, beat: f32) -> Vec<f32> {
        let n = self.frames;
        let (dl, dr) = pingpong(&mix.dly, (beat * 0.75) as usize, 0.42);
        let (rl, rr) = reverb(&mix.rev, if self.style == Style::Ambient { 0.9 } else { 0.84 });
        let (wet, echo) = match self.style {
            Style::Ambient => (1.5, 0.7),
            Style::Chill => (1.0, 0.6),
            _ => (0.8, 0.7),
        };

        let mut out = Vec::with_capacity(n * 2);
        let (mut peak, mut ring) = (0.0f32, 0.0f32);
        for i in 0..n {
            let l = mix.l[i] + dl[i] * echo + rl[i] * wet;
            let r = mix.r[i] + dr[i] * echo + rr[i] * wet;
            // Gentle saturation glues the mix and tames peaks.
            let (l, r) = ((l * 1.25).tanh(), (r * 1.25).tanh());
            peak = peak.max(l.abs()).max(r.abs());
            ring += (l.abs() - ring) * 0.0005;
            out.push(l);
            out.push(r);
        }
        let _ = ring;
        // Normalise to about -1 dBFS, then fade in over 0.4 s and out over 3 s.
        let gain = if peak > 1e-4 { 0.89 / peak } else { 1.0 };
        let (fin, fout) = ((0.4 * SR) as usize, ((3.0 * SR) as usize).min(n / 2));
        for i in 0..n {
            let mut g = gain;
            if i < fin {
                g *= i as f32 / fin as f32;
            }
            if i + fout > n {
                let k = (n - i) as f32 / fout as f32;
                g *= k * k;
            }
            out[2 * i] *= g;
            out[2 * i + 1] *= g;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LoadHint, source::Track};

    fn stats(samples: &[f32]) -> (f32, f32) {
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
        (peak, rms)
    }

    #[test]
    fn every_style_renders_audible_clean_audio_of_the_right_length() {
        for style in Style::ALL {
            let s = render(7, style, 14.0);
            assert_eq!(s.len(), (14.0 * SR) as usize * 2, "{style:?}");
            assert!(s.iter().all(|v| v.is_finite()), "{style:?} produced NaN/inf");
            let (peak, rms) = stats(&s);
            assert!((0.8..=0.9).contains(&peak), "{style:?} peak {peak}");
            assert!(rms > 0.05, "{style:?} is too quiet: rms {rms}");
            // Fade in and out: no click at either end.
            assert!(s[..40].iter().all(|v| v.abs() < 0.02), "{style:?} starts abruptly");
            assert!(s[s.len() - 40..].iter().all(|v| v.abs() < 0.01), "{style:?} ends abruptly");
        }
    }

    #[test]
    fn rendering_is_deterministic_and_seed_sensitive() {
        let a = render(1, Style::House, 8.0);
        assert_eq!(a, render(1, Style::House, 8.0));
        assert_ne!(a, render(2, Style::House, 8.0));
        assert_ne!(a, render(1, Style::Chill, 8.0));
    }

    #[test]
    fn the_music_has_dynamics_and_both_channels_are_used() {
        let s = render(3, Style::Synthwave, 20.0);
        // Loudness varies over the arrangement (intro is lighter than the full groove).
        let win = SR as usize * 2;
        let rms: Vec<f32> = s.chunks(win).take(9).map(|c| stats(c).1).collect();
        let (lo, hi) = (rms.iter().copied().fold(f32::MAX, f32::min), rms.iter().copied().fold(0.0, f32::max));
        assert!(hi > lo * 1.15, "flat arrangement: {rms:?}");
        let (mut l, mut r) = (0.0f64, 0.0f64);
        for f in s.as_chunks::<2>().0 {
            l += f64::from(f[0]).powi(2);
            r += f64::from(f[1]).powi(2);
        }
        assert!(l > 0.0 && r > 0.0 && (l / r).ln().abs() < 0.7, "badly unbalanced: {l} vs {r}");
    }

    #[test]
    fn styles_sound_different_in_spectrum() {
        // Low-band share of the energy: house has a four-on-the-floor kick, ambient has none.
        fn low_share(samples: &[f32]) -> f32 {
            // One-pole low-pass at ~150 Hz on the left channel.
            let a = 1.0 - (-TAU * 150.0 / SR).exp();
            let (mut y, mut low, mut total) = (0.0f32, 0.0f32, 0.0f32);
            for f in samples.as_chunks::<2>().0.iter().skip(SR as usize) {
                y += a * (f[0] - y);
                low += y * y;
                total += f[0] * f[0];
            }
            low / total
        }
        let house = low_share(&render(5, Style::House, 12.0));
        let ambient = low_share(&render(5, Style::Ambient, 12.0));
        assert!(house > ambient, "house {house} vs ambient {ambient}");
    }

    #[test]
    fn the_wav_decodes_with_the_real_decoder_and_reports_its_length() {
        let wav = demo_wav(11, Style::Chill, 9.0);
        let t = Track::open(1, wav.into(), &LoadHint::default()).expect("decodes");
        assert_eq!((t.channels, t.rate), (2, RATE));
        let d = t.duration.expect("WAV knows its length").as_secs_f32();
        assert!((d - 9.0).abs() < 0.05, "{d}");
    }

    #[test]
    fn scale_and_chord_helpers() {
        assert_eq!(degree_semitones(&MINOR, 0), 0);
        assert_eq!(degree_semitones(&MINOR, 2), 3);
        assert_eq!(degree_semitones(&MINOR, 7), 12);
        assert_eq!(degree_semitones(&MINOR, -1), -2);
        assert_eq!(chord(&MINOR, 0, false), vec![0, 3, 7]);
        assert_eq!(chord(&MAJOR, 0, true), vec![0, 4, 7, 11]);
        assert_eq!(chord(&MAJOR, 4, false), vec![7, 11, 14]);
        assert_eq!(fold(70, 55), 58);
        assert_eq!(fold(40, 55), 64);
        assert!((mtof(69.0) - 440.0).abs() < 1e-3 && (mtof(57.0) - 220.0).abs() < 1e-2);
    }

    #[test]
    fn the_rng_is_uniform_enough() {
        let mut r = Rng::new(42);
        let xs: Vec<f32> = (0..10_000).map(|_| r.unit()).collect();
        assert!(xs.iter().all(|x| (0.0..1.0).contains(x)));
        let mean = xs.iter().sum::<f32>() / xs.len() as f32;
        assert!((mean - 0.5).abs() < 0.02, "{mean}");
        assert!((0..1000).all(|_| r.below(5) < 5));
    }
}
