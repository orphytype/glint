//! The compositor. Every `El::effect` slot in the view becomes one of Glint's own WGSL
//! effects (`shaders/gfx.wgsl`: the dithered page, dither tiles, liquid glass, the progressive
//! blur at the page's edges, flowing colour, the seek bars, the mood spheres) or a `rune-fx` layer
//! (the like burst and the loading shimmer).
//!
//! The frame is cut once for everything that blurs: the `SNAP` slot, placed after the page and
//! before the glass and the veils, asks RUNE for a copy of the frame so far, and every blur of the
//! frame is made from that one copy. (RUNE copies the whole frame at every backdrop slot, and that
//! cost more than all the glass shading put together.)
//!
//! A slot's mark says which effect it is (the low byte; the next byte is a variant) and its
//! `effect_data` carries the few live numbers that effect needs, so the view stays the single
//! description of the UI while the GPU does the painting at exactly the right place in the
//! painter order.

mod gpu;

use std::collections::HashMap;

use rune::compose::{Compositor, Frame};
use rune::render::{EffectPhase, EffectSite, Instance, KIND_EFFECT};
use rune_fx::*;

use crate::app::State;
use crate::store::Effects;
use crate::theme;
use gpu::{Binds, Gpu, Pipe, Region, U};

// Marks of the slots Glint paints itself.
pub const FIELD: u32 = 16;
pub const PATTERN: u32 = 17;
pub const GLASS: u32 = 18;
pub const VEIL: u32 = 19;
pub const SNAP: u32 = 21;
pub const LIQUID: u32 = 22;
pub const MORPH: u32 = 23;
pub const BARS: u32 = 24;
pub const ORB: u32 = 25;
/// Over a cover: when the track changes, the last cover breaks into blocks that drift and settle
/// into the new one (`fs_artmorph`). Draws nothing the rest of the time. d: seed.
pub const ARTMORPH: u32 = 27;
/// A shape of the garden on Home (`fs_shape`). d: family, seed, hue in degrees (`-(hue + 1)` to
/// fill it with the playing cover, that hue if there is none; `-(hue + 1000)` to fill it with the
/// picture `State::pic_key` names, as it is, that hue until it is in), and
/// `chosen + 2 hovered + 4 order` (the order staggers how they come up).
pub const SHAPE: u32 = 28;
/// Behind the open player (`fs_stage`): a flash that blooms under the cover and carries the
/// song's energy outward. d: strength, `seed + 100 * cover side (points)`, where the cover's
/// middle is (0..1 of the slot).
pub const STAGE: u32 = 29;
/// One of the butterflies on Home (`fs_shape`, family 8): the simulation in `bugs.rs` says where it
/// is heading, how open its wings are, its bank and its height. d: its index in the swarm, seed, hue.
pub const BUTTERFLY: u32 = 30;
/// A small bar chart in a details tile (`fs_chart`), bars printed in a dithered gradient.
/// d: the bar count + 64 × the hue in whole degrees, its fraction how far the bars have risen;
/// then six 4-bit heights in each of the other three (see [`chart_data`]).
pub const CHART: u32 = 31;

/// The effect data of a [`CHART`]: up to eighteen `values` (any scale; the largest is the full
/// height, and zero the foot, or with `from_min` the smallest a quarter of the way up, so close
/// values still differ), drawn in `hue` degrees, `risen` (0..1) of the way up.
pub fn chart_data(values: &[f32], from_min: bool, hue: f32, risen: f32) -> [f32; 4] {
    let n = values.len().min(18);
    let max = values.iter().take(n).fold(0.0f32, |m, v| m.max(*v));
    let min = if from_min { values.iter().take(n).fold(f32::MAX, |m, v| m.min(*v)) } else { 0.0 };
    let mut words = [0u32; 3];
    for (i, v) in values.iter().take(n).enumerate() {
        let f = if max > min { (v - min) / (max - min) } else { 1.0 };
        let f = if from_min { 0.25 + 0.75 * f } else { f };
        let level = if max > 0.0 { (f.clamp(0.0, 1.0) * 15.0).round() as u32 } else { 0 };
        words[i / 6] |= level.min(15) << ((i % 6) * 4);
    }
    let hue = (hue.rem_euclid(360.0)).round();
    let head = n.max(1) as f32 + 64.0 * hue;
    [head + risen.clamp(0.0, 1.0) * 0.99, words[0] as f32, words[1] as f32, words[2] as f32]
}

/// What the stage remembers of the song: a clock that only runs while music plays, the readings
/// of the music eased so none of them jumps, and the harmony's direction.
struct Song {
    clock: f32,
    /// The spectrum now, eased (quick up, slower down), and each band's recent loudest.
    spec: [f32; 32],
    spec_hi: [f32; 32],
    /// The low end, the middle (the melody) and the top now, the low end's slow average, and the
    /// kick: how far the low end just jumped over that average, quick up and gone in a fifth of a
    /// second, so the flash responds to the drums.
    bass: f32,
    mid: f32,
    treble: f32,
    bass_slow: f32,
    kick: f32,
    loud: f32,
    chroma: [f32; 12],
    /// The key, roughly: where the harmony has leaned over the last several seconds, as a
    /// direction on the circle of fifths (not normalised; its length is how settled it is).
    key: [f32; 2],
    ground_t: f32,
    peak: [f32; 2],
    peak_primed: bool,
}

impl Song {
    fn new() -> Song {
        Song {
            clock: 0.0,
            spec: [0.0; 32],
            spec_hi: [0.05; 32],
            bass: 0.0,
            mid: 0.0,
            treble: 0.0,
            bass_slow: 0.0,
            kick: 0.0,
            loud: 0.0,
            chroma: [0.0; 12],
            key: [0.0, 0.0],
            ground_t: 0.0,
            peak: [0.5, 0.5],
            peak_primed: false,
        }
    }

    /// Eases the readings toward this frame's (`live`: music is playing). Chords come up in about
    /// a tenth of a second and fade over half a second; the melody glides; nothing steps.
    fn hear(&mut self, dt: f32, tone: &ym_audio::Tone, bands: &[f32; 32], loud: f32, live: bool) {
        let ease = |x: &mut f32, to: f32, tau: f32| *x += (to - *x) * (1.0 - (-dt / tau).exp());
        // Each band against the loudest it has been lately (forgotten over some seconds), so the
        // quiet treble moves the picture as much as the bass does.
        for i in 0..32 {
            let b = if live { bands[i].max(0.0) } else { 0.0 };
            let hi = &mut self.spec_hi[i];
            *hi = if b > *hi { b } else { (*hi + (b - *hi) * (1.0 - (-dt / 6.0).exp())).max(0.03) };
            let to = (b / *hi).clamp(0.0, 1.0);
            let tau = if to > self.spec[i] { 0.035 } else { 0.16 };
            ease(&mut self.spec[i], to, tau);
        }
        let mean = |r: std::ops::Range<usize>| self.spec[r.clone()].iter().sum::<f32>() / r.len() as f32;
        let (bass, mid, treble) = (mean(0..5), mean(6..18), mean(19..32));
        (self.bass, self.mid, self.treble) = (bass, mid, treble);
        ease(&mut self.bass_slow, bass, 0.45);
        let jump = ((bass - self.bass_slow * 1.05) * 3.2).clamp(0.0, 1.0);
        if jump > self.kick {
            self.kick = jump;
        } else {
            self.kick *= (-dt / 0.18).exp();
        }
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

    /// The key's direction (cos, sin); straight up the circle until there has been music.
    fn key_dir(&self) -> [f32; 2] {
        let l = (self.key[0] * self.key[0] + self.key[1] * self.key[1]).sqrt();
        if l < 1e-4 { [1.0, 0.0] } else { [self.key[0] / l, self.key[1] / l] }
    }

    /// The clock moves on by `dt` (only while music plays): the flash pulses on it.
    fn advance(&mut self, dt: f32) {
        self.clock += dt;
    }

    /// The harmony as a direction on the circle of fifths (cos, sin) and how strongly the music
    /// leans that way (0: every pitch class alike, or silence).
    fn harmony(&self) -> [f32; 3] {
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

/// How long the morph from one cover to the next takes.
const ART_MORPH_S: f32 = 1.4;
/// How long the cover waits, holding the last picture, for the new one to be decoded.
const ART_WAIT_S: f32 = 2.0;
// rune-fx layers.
pub const SPARKLES: u32 = 6;
pub const SHIMMER: u32 = 7;

/// Seconds one loop of the playing marker takes.
const MORPH_SECS: f32 = 1.5;

/// Seconds a new track's waveform takes to grow out of the last one's in the seek bars.
const BARS_SWAP_S: f32 = 0.32;

/// A measuring tool, not a setting: `GLINT_FXOFF=liquid,glass,veil,snap,field,morph,bars,orbs`
/// leaves those pieces of the compositor out, to see what each costs, and `fps` prints the frame
/// rate and the time per frame (see `scripts/perf.ps1`).
pub fn off(name: &str) -> bool {
    static V: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    V.get_or_init(|| std::env::var("GLINT_FXOFF").unwrap_or_default()).split(',').any(|x| x == name)
}

fn hex3(c: [f32; 3]) -> u32 {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
    (q(c[0]) << 16) | (q(c[1]) << 8) | q(c[2])
}

// ---------------------------------------------------------------------------------------------
// Glass.

/// The glass presets; a `GLASS` slot's variant byte picks one (`glass(Glass::Dock)`).
#[derive(Clone, Copy, Debug)]
pub enum Glass {
    /// The dock: a long pill over scrolling content.
    Dock = 0,
    /// A card over the page.
    Card = 1,
    /// A small button or chip.
    Chip = 2,
}

/// The mark for a glass slot.
pub const fn glass(g: Glass) -> u32 {
    GLASS | ((g as u32) << 8)
}


/// How a contour slot is cut: soft edges that fade into the page, a disc, or the whole slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// Only the player page and sign-in draw contours now; the shader keeps every cut.
#[allow(dead_code)]
pub enum Contour {
    Soft = 2,
    /// A disc.
    Disc = 3,
    Full = 4,
    /// The whole slot, with a peak that glides when it is moved (the player page's).
    Stage = 5,
}

/// The mark for a contour slot: the music drawn as the lines of a map (`gfx.wgsl`, `ct_lo`).
#[allow(dead_code)]
pub const fn contour(c: Contour) -> u32 {
    LIQUID | ((c as u32) << 8)
}

/// The data of a contour slot: its strength (0..1), a hue in degrees for its accent (below 0: the
/// app's accent, which follows the cover), a seed for its landscape, and where its peak is (0..1
/// over the slot).
#[allow(dead_code)]
pub fn contour_data(strength: f32, hue: f32, seed: f32, peak: [f32; 2]) -> [f32; 4] {
    [strength, hue, seed, (peak[0].clamp(0.0, 1.0) * 100.0).round() + peak[1].clamp(0.0, 0.99)]
}

/// Lines per unit of height, how far the peak reaches, how wild the land is, and how strongly the
/// lines are drawn, for each cut.
fn contour_shape(c: u32) -> [f32; 4] {
    match c {
        3 => [3.2, 0.62, 0.05, 3.2],
        4 => [7.0, 0.40, 0.85, 1.2],
        5 => [7.0, 0.30, 0.70, 1.0],
        _ => [8.0, 0.50, 0.60, 1.0],
    }
}


struct Spec {
    /// How far the rim pulls the picture inward, in points, and over what width.
    refr: f32,
    edge: f32,
    /// Colour fringing at the rim, 0..1.
    chroma: f32,
    /// Blur of what shows through, in points (sigma).
    blur: f32,
    sat: f32,
    contrast: f32,
    /// The bevel's extra light.
    ambient: f32,
    spec: f32,
    spec_exp: f32,
    rim: f32,
    shade: f32,
    /// How much of the page colour is mixed in.
    tint: f32,
    /// The soft shadow's strength and reach (points).
    cast: f32,
    reach: f32,
}

const SPECS: [Spec; 3] = [
    Spec {
        refr: 16.0,
        edge: 22.0,
        chroma: 0.38,
        blur: 12.0,
        sat: 1.10,
        contrast: 0.0,
        ambient: 0.10,
        spec: 0.60,
        spec_exp: 22.0,
        rim: 0.42,
        shade: 0.16,
        tint: 0.34,
        cast: 0.0,
        reach: 0.0,
    },
    Spec {
        refr: 10.0,
        edge: 16.0,
        chroma: 0.35,
        blur: 10.0,
        sat: 1.05,
        contrast: 0.0,
        ambient: 0.08,
        spec: 0.45,
        spec_exp: 20.0,
        rim: 0.32,
        shade: 0.12,
        tint: 0.28,
        cast: 0.20,
        reach: 22.0,
    },
    Spec {
        refr: 5.0,
        edge: 9.0,
        chroma: 0.20,
        blur: 6.0,
        sat: 1.05,
        contrast: 0.0,
        ambient: 0.06,
        spec: 0.50,
        spec_exp: 20.0,
        rim: 0.38,
        shade: 0.08,
        tint: 0.24,
        cast: 0.0,
        reach: 0.0,
    },
];

// ---------------------------------------------------------------------------------------------
// What the shaders see of the app: a palette that eases toward the playing cover's, and audio
// bands that have been shaped into bass, mid, treble and a beat pulse.

struct Smooth {
    pal: [[f32; 3]; 5],
    accent: [f32; 3],
    bands: [f32; 32],
    level: f32,
    bass: f32,
    mid: f32,
    treb: f32,
    slow_bass: f32,
    beat: f32,
    vibrance: f32,
    primed: bool,
    /// The music as the maps see it: bass, mid, treble and loudness, each stretched over the range
    /// the track has actually used lately (an automatic gain), so a quiet or a heavily mastered
    /// track moves the picture as much as any other. 0..1, smoothed.
    norm: [f32; 4],
    /// The low and high water marks behind `norm`.
    range: [[f32; 2]; 4],
    /// Last frame's bands, for the onset detector (spectral flux of the low end).
    prev: [f32; 32],
    flux_mean: f32,
    flux_var: f32,
    since_onset: f32,
    /// A kick: jumps to the onset's strength and dies away in a quarter of a second or so.
    pulse: f32,
    /// Tempo estimated from the spacing of onsets, used to pace the stage's flash.
    tempo: f32,
    /// The rings each onset sends out of a map's peak: age in seconds and strength (0: none).
    shocks: [[f32; 2]; 2],
}

impl Smooth {
    fn new() -> Smooth {
        Smooth {
            pal: [[0.0; 3]; 5],
            accent: [0.6, 0.5, 1.0],
            bands: [0.0; 32],
            level: 0.0,
            bass: 0.0,
            mid: 0.0,
            treb: 0.0,
            slow_bass: 0.0,
            beat: 0.0,
            vibrance: 0.7,
            primed: false,
            norm: [0.0; 4],
            range: [[0.0, 0.1]; 4],
            prev: [0.0; 32],
            flux_mean: 0.0,
            flux_var: 0.0,
            since_onset: 1.0,
            pulse: 0.0,
            tempo: 120.0,
            shocks: [[0.0, 0.0]; 2],
        }
    }

    /// How long a shock ring lives.
    const SHOCK_S: f32 = 2.2;

    /// The shortest time between two kicks the picture answers. Kept above a third of a second, so
    /// nothing on screen pulses more than about two and a half times a second whatever the tempo
    /// (photosensitivity guidance asks for no more than three flashes a second).
    const ONSET_GAP_S: f32 = 0.4;

    /// Follows the music: the stretched bands, onsets, and the rings they send out.
    fn listen(&mut self, dt: f32, st: &State) {
        let mean = |b: &[f32; 32], r: std::ops::Range<usize>| b[r.clone()].iter().sum::<f32>() / r.len() as f32;
        let b = st.spectrum;
        let raw = [mean(&b, 0..5), mean(&b, 5..16), mean(&b, 16..32), st.level.clamp(0.0, 1.0)];
        let on = st.player.playing && raw[3] > 0.002;
        // Automatic gain: the low mark follows the quietest the band has been, the high mark the
        // loudest, both forgetting over a few seconds; the band is then read between the two.
        for (j, x) in raw.into_iter().enumerate() {
            let [lo, hi] = &mut self.range[j];
            if on {
                *lo = if x < *lo { x } else { *lo + (x - *lo) * (1.0 - (-dt * 0.35).exp()) };
                *hi = if x > *hi { x } else { *hi + (x - *hi) * (1.0 - (-dt * 0.22).exp()) };
            }
            let span = (*hi - *lo).max(0.06);
            let n = if on { ((x - *lo) / span).clamp(0.0, 1.0) } else { 0.0 };
            // Quick to rise, a little slower to fall, so a kick reads as a kick.
            let k = 1.0 - (-dt * if n > self.norm[j] { 10.0 } else { 4.0 }).exp();
            self.norm[j] += (n - self.norm[j]) * k;
        }
        // Onsets: how much the low end grew since the last frame, against its own recent average.
        let mut flux = 0.0;
        for i in 0..8 {
            flux += (b[i] - self.prev[i]).max(0.0) * if i < 5 { 1.0 } else { 0.5 };
        }
        self.prev = b;
        flux /= dt.max(0.004) * 60.0; // per sixtieth of a second, whatever the frame rate
        let a = 1.0 - (-dt * 2.0).exp();
        let dev = flux - self.flux_mean;
        self.flux_mean += dev * a;
        self.flux_var += (dev * dev - self.flux_var) * a;
        self.since_onset += dt;
        let sd = self.flux_var.max(0.0).sqrt();
        if on && self.since_onset > Self::ONSET_GAP_S && flux > self.flux_mean + 1.4 * sd && flux > 0.012 {
            let period = self.since_onset;
            if (0.25..=1.5).contains(&period) {
                let target = (60.0 / period).clamp(60.0, 180.0);
                self.tempo += (target - self.tempo) * 0.18;
            }
            self.since_onset = 0.0;
            let strength = (0.45 + (flux - self.flux_mean) / (4.0 * sd + 0.02)).clamp(0.45, 1.0);
            self.pulse = self.pulse.max(strength);
            // The older ring makes way.
            let slot = if self.shocks[0][1] <= 0.0 || self.shocks[0][0] > self.shocks[1][0] { 0 } else { 1 };
            self.shocks[slot] = [0.0, strength];
        }
        self.pulse *= (-dt * 2.6).exp();
        for s in &mut self.shocks {
            if s[1] > 0.0 {
                s[0] += dt;
                if s[0] > Self::SHOCK_S {
                    *s = [0.0, 0.0];
                }
            }
        }
        if !on {
            self.pulse *= (-dt * 6.0).exp();
        }
    }

    /// Whether anything of the music is still moving the picture.
    fn stirring(&self) -> bool {
        self.pulse > 0.01 || self.shocks.iter().any(|s| s[1] > 0.0) || self.norm.iter().any(|n| *n > 0.01)
    }

    fn update(&mut self, dt: f32, st: &State) {
        let target = st.palette;
        if !self.primed {
            self.pal = target.colors;
            self.accent = target.accent;
            self.vibrance = target.vibrance;
            self.primed = true;
        }
        let k = 1.0 - (-dt * 2.4).exp();
        for (c, t) in self.pal.iter_mut().zip(target.colors) {
            for i in 0..3 {
                c[i] += (t[i] - c[i]) * k;
            }
        }
        for i in 0..3 {
            self.accent[i] += (target.accent[i] - self.accent[i]) * k;
        }
        self.vibrance += (target.vibrance - self.vibrance) * k;

        self.bands = st.spectrum;
        let mean = |r: std::ops::Range<usize>| self.bands[r.clone()].iter().sum::<f32>() / r.len() as f32;
        let (bass, mid, treb) = (mean(0..5), mean(5..16), mean(16..32));
        let a = 1.0 - (-dt * 12.0).exp();
        self.bass += (bass - self.bass) * a;
        self.mid += (mid - self.mid) * a;
        self.treb += (treb - self.treb) * a;
        self.level += (st.level.clamp(0.0, 1.0) - self.level) * a;
        self.slow_bass += (bass - self.slow_bass) * (1.0 - (-dt * 1.4).exp());
        let kick = ((bass - self.slow_bass * 1.12) * 4.5).clamp(0.0, 1.0);
        self.listen(dt, st);
        self.beat = (self.beat * (-dt * 5.5).exp()).max(kick).max(self.pulse * 0.9);
    }
}

// ---------------------------------------------------------------------------------------------
// rune-fx layers.

fn layer(kind: u32, d: [f32; 4], shape: Shape, t: f32, accent: [f32; 3]) -> Option<FxLayer> {
    let fx = match kind {
        SPARKLES => {
            Fx::sparkles(SparkleParams { burst: (d[0] >= 0.0).then_some(d[0]), count: d[1] as u32, size: d[2], color: rgb(hex3(accent)), margin: 18.0, ..Default::default() })
        }
        SHIMMER => Fx::shimmer(ShimmerParams { intensity: d[0], period: d[1], band: d[2], ..Default::default() }),
        _ => return None,
    };
    Some(fx.on(shape).at(t))
}

// ---------------------------------------------------------------------------------------------

/// A slot of ours, ready to be recorded: which pipeline, which uniform buffer, and the pyramid
/// (or capture rectangle) it needs.
struct Job {
    pipe: Pipe,
    u: usize,
    region: Option<Region>,
    /// Which of the frame's blur pyramids is this slot's (they are all made at the snapshot, so
    /// two slots of one size must not share one).
    tag: u8,
    /// The small texture a liquid slot reads back (its size in texels).
    lo: Option<[u32; 2]>,
}

/// What a seek-bars slot remembers: the waveform it is showing and the one it is leaving, how far
/// it has got from one to the other, and the playhead as it is drawn (it eases to the real one).
#[derive(Clone, Copy)]
struct BarMem {
    seed: f32,
    from: f32,
    swap: f32,
    shown: f32,
    age: f32,
    gone: f32,
}

impl BarMem {
    const NEW: BarMem = BarMem { seed: -1.0, from: 0.0, swap: 1.0, shown: 0.0, age: 0.0, gone: 1.0 };
}

pub struct Gfx {
    fx: Option<FxRenderer>,
    /// Whether rune-fx has been made to build its effects once (see `compose`).
    fx_warm: bool,
    gpu: Option<Gpu>,
    smooth: Smooth,
    animating: bool,
    /// The dithered page's own clock: it only runs while music plays, so a paused app is still.
    field_t: f32,
    /// The liquid colour's clock and the five colours it is currently showing (they ease toward
    /// the cover's, or toward the hue a slot asks for).
    liquid_t: f32,
    /// How long the liquid has been on screen with nothing else going on. It flows for a while,
    /// slows to a stop and then lets the app go idle, so a parked window costs nothing.
    liquid_idle: f32,
    /// The playing marker's clock: it advances while a marker is on screen and music plays, so a
    /// paused marker holds its pose.
    morph_t: f32,
    /// How far the playing marker's bars are up (0: three dots).
    marker_run: f32,
    morph_on: bool,
    /// The mood spheres' clock: it runs at the pace of the liquid, so it stops when the liquid does.
    wave_t: f32,
    /// How far each mood sphere has got to being the chosen one; below 0 until it has been drawn once.
    orb_on: [f32; 8],
    /// How long the spheres have been on screen (they fade up), and for how long they have been gone.
    orb_age: f32,
    orb_gone: f32,
    /// The garden's clock and how much the music moves it, eased.
    shape_t: f32,
    shape_music: f32,
    /// Each shape on screen (by family and seed): how hovered and how chosen it is, eased, and
    /// how long it has been up. Forgotten once the garden has been away for a moment.
    shapes: HashMap<u32, [f32; 3]>,
    shapes_gone: f32,
    /// What the stage behind the open player remembers of the song.
    song: Song,
    /// Seconds since the playing cover changed (large: long ago), and how long the new one has
    /// been awaited.
    art_change: f32,
    art_wait: f32,
    /// The seek bars: what each slot remembers, how much the music moves them (it eases in and
    /// out, so pausing settles them instead of snapping) and the clock they dance on.
    bars: [BarMem; 3],
    bars_live: f32,
    bars_t: f32,
    /// Whether a liquid slot was drawn last frame (coming back to one starts the flow again).
    liquid_seen: bool,
    lp: [[f32; 3]; 5],
    lp_primed: bool,
    /// How long each kind of liquid slot (blob, wash) has been on screen, and for how long it has
    /// been absent: a slot coming into view fades up instead of appearing at full strength.
    liquid_age: [f32; 3],
    liquid_gone: [f32; 3],
    /// The contour lines' phase: they travel out of their peak, faster with the music and in a
    /// surge on each beat. It runs from 0 to 5 (every fifth line is a heavy one) and wraps.
    ct_phase: f32,
    /// How fast the rings travel: eased towards what the music asks, so the speed never jumps.
    ct_speed: f32,
    /// Where the player page's peak is drawn: it glides to where it is asked to be.
    ct_peak: [f32; 2],
    ct_peak_primed: bool,
    /// Whether the frame drawn last is a picture the next one can reuse: passive effects can
    /// advance between redraws, and the frame is kept on the GPU.
    repeatable: bool,
    /// When that frame was drawn (`Runtime::clock`) and what the window looked like then.
    full_at: f64,
    look: u64,
    /// Time of the frames shown again, owed to the next frame drawn for real so that the clocks
    /// below run at the pace of the window and not of the screen.
    owed: f32,
    frames: u32,
    reused: u32,
    upd_ms: f64,
    cpu_ms: f64,
    fps_at: f64,
    prof: Option<Prof>,
}

/// Dev tool (`GLINT_FXOFF=fps`): the GPU time of each frame, from timestamps written around
/// everything the compositor and RUNE record, read back a frame later and printed every second.
struct Prof {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    pending: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// 0: free, 1: a copy is recorded (the next frame asks to map it), 2: mapping.
    state: u8,
    sum_ms: f64,
    n: u32,
    max_ms: f64,
}

/// A frame that comes this soon after the last one drawn, with nothing changed since, shows that
/// one again. Passive effects are capped at roughly thirty pictures a second while input and
/// scrolling still draw immediately.
const REPEAT_BELOW_S: f64 = 0.032;

/// Seconds a liquid slot takes to fade up when it comes into view.
const LIQUID_APPEAR_S: f32 = 0.4;

/// Seconds the mood spheres and the seek bars take to fade up when they come into view.
const APPEAR_S: f32 = 0.35;

/// Seconds the liquid flows on its own before it slows down, and how long the slowing takes.
const LIQUID_FLOW_S: f32 = 40.0;
const LIQUID_EASE_S: f32 = 4.0;

impl Gfx {
    pub fn new() -> Gfx {
        Gfx {
            fx: None,
            fx_warm: false,
            gpu: None,
            smooth: Smooth::new(),
            animating: true,
            field_t: 0.0,
            liquid_t: 0.0,
            liquid_idle: 0.0,
            morph_t: 0.0,
            marker_run: 0.0,
            morph_on: false,
            wave_t: 0.0,
            orb_on: [-1.0; 8],
            orb_age: 0.0,
            orb_gone: 1.0,
            shape_t: 0.0,
            shape_music: 0.0,
            shapes: HashMap::new(),
            shapes_gone: 1.0,
            song: Song::new(),
            art_change: 99.0,
            art_wait: 0.0,
            bars: [BarMem::NEW; 3],
            bars_live: 0.0,
            bars_t: 0.0,
            liquid_seen: false,
            lp: [[0.0; 3]; 5],
            lp_primed: false,
            liquid_age: [0.0; 3],
            liquid_gone: [1.0; 3],
            ct_phase: 0.0,
            ct_speed: 0.03,
            ct_peak: [0.5, 0.5],
            ct_peak_primed: false,
            repeatable: false,
            full_at: 0.0,
            look: 0,
            owed: 0.0,
            frames: 0,
            reused: 0,
            upd_ms: 0.0,
            cpu_ms: 0.0,
            fps_at: 0.0,
            prof: None,
        }
    }
}

/// The uniforms every slot shares: where it is, how it is clipped, and what the pass is.
fn base(i: &Instance, k: f32, pass: [u32; 2], srgb: bool, t: f32, grow: f32, keep_inside_clip: bool) -> U {
    let [x, y, w, h] = i.rect;
    let c = i.clip;
    let g = grow + 1.0;
    let mut bounds = [x - g, y - g, x + w + g, y + h + g];
    if keep_inside_clip {
        bounds = [bounds[0].max(c[0]), bounds[1].max(c[1]), bounds[2].min(c[2]), bounds[3].min(c[3])];
    }
    U {
        rect: i.rect,
        bounds,
        clip: c,
        fade: i.fade,
        radii: i.radii,
        view: [pass[0] as f32, pass[1] as f32, if srgb { 1.0 } else { 0.0 }, if theme::light() { 1.0 } else { 0.0 }],
        misc: [0.0, 0.0, t, k],
        ..U::default()
    }
}

/// A stable pyramid tag for a slot that has a region to blur (0 for one that has none).
///
/// Tags used to be allocated afresh on every frame. Keeping them tied to the effect slot lets the
/// GPU retain a pyramid when the page behind a glass pane is unchanged.
fn pyramid_tag(slot: u32, wanted: bool) -> u8 {
    if !wanted {
        return 0;
    }
    16 + (slot % 112) as u8
}

/// Fast at first, slow to settle (a cubic ease-out).
/// The id the playing cover goes to the GPU under: its cache key, hashed (an `Arc`'s address is
/// no good, a new cover's thumb can land at the address the old one had).
fn art_id(key: Option<&str>) -> usize {
    key.map_or(0, |k| k.bytes().fold(0xcbf2_9ce4_8422_2325u64, |a, b| (a ^ b as u64).wrapping_mul(0x100_0000_01b3)) as usize)
}

fn ease_out(x: f32) -> f32 {
    1.0 - (1.0 - x).powi(3)
}

fn rgb4(c: [f32; 3], a: f32) -> [f32; 4] {
    [c[0], c[1], c[2], a]
}

/// What the window looks like, as far as a picture of it goes. Per-tick overlays are deliberately
/// left out because they do not change the backdrop under glass or veils.
fn look_of(f: &Frame<'_, State>) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut eat = |w: u64| {
        h = (h ^ w).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        h ^= h >> 29;
    };
    let st = &f.rt.state;
    eat(u64::from(f.size.0) << 32 | u64::from(f.size.1));
    eat(st.settings.effects as u64 | u64::from(st.settings.reduce_motion) << 8 | u64::from(theme::light()) << 9);
    for (n, instance) in f.rt.instances.iter().enumerate() {
        let mark = f.rt.marks.get(n).copied().unwrap_or(0);
        if matches!(mark & 0xff, BARS | MORPH | BUTTERFLY) {
            continue;
        }
        for word in bytemuck::cast_slice::<Instance, u32>(std::slice::from_ref(instance)) {
            eat(u64::from(*word));
        }
        eat(u64::from(mark));
    }
    h
}

impl Gfx {
    /// Puts the picture of the last frame drawn on screen again, when the screen asks for a frame
    /// before anything has changed since it. Returns whether it did; the frame is then done.
    fn show_again(&mut self, f: &mut Frame<'_, State>, look: u64) -> bool {
        if !self.repeatable || look != self.look || !(0.0..REPEAT_BELOW_S).contains(&(f.rt.clock - self.full_at)) {
            return false;
        }
        let size = [f.size.0, f.size.1];
        let Some(gpu) = self.gpu.as_mut().filter(|g| g.key == f.device_key()) else { return false };
        gpu.begin(size);
        if !gpu.has_kept(size, f.format) {
            return false;
        }
        gpu.touch_pyramids();
        let (w, h) = (size[0] as f32, size[1] as f32);
        let u = gpu.upload(f.device, f.queue, &U { bounds: [0.0, 0.0, w, h], view: [w, h, 0.0, 0.0], ..U::default() });
        gpu.show_kept(f.device, f.encoder, f.target, f.format, u);
        self.owed += f.dt.clamp(0.0, 0.1) as f32;
        self.reused += 1;
        true
    }
}

impl Compositor<State> for Gfx {
    fn features(&self) -> wgpu::Features {
        if off("fps") { wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS } else { wgpu::Features::empty() }
    }

    fn animating(&self) -> bool {
        self.animating
    }

    fn render(&mut self, f: &mut Frame<'_, State>) {
        let began = std::time::Instant::now();
        self.upd_ms += f.update_ms;
        self.draw(f);
        self.cpu_ms += began.elapsed().as_secs_f64() * 1e3;
    }
}

impl Gfx {
    fn draw(&mut self, f: &mut Frame<'_, State>) {
        let look = look_of(f);
        if self.show_again(f, look) {
            return;
        }
        let (device, queue, size) = (f.device, f.queue, f.size);
        let pass_size = [size.0, size.1];
        let st = &f.rt.state;
        let quality = st.settings.effects;
        let reduced = st.settings.reduce_motion;
        let accent = theme::accent_srgb(st.settings.accent, self.smooth.accent);
        let dt = (f.dt + f64::from(std::mem::take(&mut self.owed))).clamp(0.0, 0.1) as f32;
        self.smooth.update(dt, st);
        // Reduced motion keeps the look but freezes the clock.
        let t = if reduced { 7.3 } else { f.rt.t as f32 };
        let sm = &self.smooth;
        let srgb = f.srgb_blend();
        let (canvas, ink) = (theme::canvas(), theme::ink());
        let playing_level = sm.level > 0.002;
        if !reduced && quality != Effects::Off {
            self.field_t += dt * (0.12 + sm.level * 1.4);
        }
        // The liquid flows on its own; music makes it flow faster. With the light effects it holds
        // still, and after a while of nothing happening it eases to a stop (see `LIQUID_FLOW_S`).
        if playing_level || !self.liquid_seen {
            self.liquid_idle = 0.0;
        } else {
            self.liquid_idle += dt;
        }
        let flow = ((LIQUID_FLOW_S + LIQUID_EASE_S - self.liquid_idle) / LIQUID_EASE_S).clamp(0.0, 1.0);
        let flow = flow * flow * (3.0 - 2.0 * flow);
        if !reduced && quality == Effects::Full {
            self.liquid_t += dt * (0.55 + sm.level * 1.6) * flow;
        }
        if !reduced && quality != Effects::Off {
            self.wave_t += dt * flow;
            // The garden moves a little quicker, and its big shape breathes, with the music.
            let want = if st.player.playing { sm.norm[3] } else { 0.0 };
            self.shape_music += (want - self.shape_music) * (1.0 - (-dt * 3.0).exp());
            self.shape_t += dt * flow * (1.0 + 0.5 * self.shape_music);
        }
        // The stage hears the music every frame, open or not, so its readings are ready when it
        // opens; its clock moves only while music plays.
        let heard = st.player.playing && sm.level > 0.002;
        let song_live = heard && !reduced && quality == Effects::Full;
        let loud = sm.norm[3];
        self.song.hear(dt, &st.tone, &sm.bands, loud, heard);
        if song_live {
            self.song.advance(dt * (sm.tempo / 120.0).clamp(0.75, 1.5));
        }
        if !reduced && quality == Effects::Full {
            self.song.ground_t += dt * flow;
        }
        if !reduced && quality == Effects::Full {
            // Rings drift out of the peak on their own and a little faster with the music. The speed
            // eases (about half a second) and is capped at under half a line a second, so the map
            // never races or jerks, however busy the track.
            let want = (0.03 + 0.16 * sm.norm[3] + 0.28 * sm.pulse).min(0.42);
            self.ct_speed += (want - self.ct_speed) * (1.0 - (-dt * 2.0).exp());
            self.ct_phase = (self.ct_phase + dt * self.ct_speed * flow).rem_euclid(5.0);
        }
        // The music moves the seek bars while it plays; it eases in and out (a paused track settles
        // back into its waveform), and the bars' clock only runs while it does.
        let bars_want = if st.player.playing && !reduced && quality != Effects::Off { 1.0 } else { 0.0 };
        let rate = if bars_want > self.bars_live { 9.0 } else { 6.0 };
        self.bars_live += (bars_want - self.bars_live) * (1.0 - (-dt * rate).exp());
        if (bars_want - self.bars_live).abs() < 0.002 {
            self.bars_live = bars_want;
        }
        self.bars_t += dt * self.bars_live;
        let mut bars_n = 0usize;
        // Seek bars on screen a moment ago (in the bar, or the player's): a row that takes over
        // from them shows at once instead of fading up from nothing.
        let bars_were_up = self.bars.iter().any(|b| b.seed >= 0.0 && b.age >= APPEAR_S);
        let mut orbs_present = false;
        let mut shapes_present = false;
        if self.morph_on && !reduced {
            self.morph_t += dt;
        }
        self.morph_on = false;
        let mut liquid_eased = false;
        let mut liquid_present = [false; 3];
        let mut ct_seen = false;

        let gpu = match &mut self.gpu {
            Some(g) if g.key == f.device_key() => g,
            slot => slot.insert(Gpu::new(device, f.device_key())),
        };
        gpu.ensure(device, f.format);
        gpu.begin(pass_size);
        // The playing cover goes to the GPU once per track; the one before it stays, for the
        // crossfade behind the player and the morph on the cover itself.
        if quality != Effects::Off
            && let Some(th) = &st.cover_thumb
            && gpu.set_art(device, queue, art_id(st.palette_key.as_deref()), th)
        {
            self.art_change = 0.0;
        }
        let art_changing = self.art_change < ART_MORPH_S;
        self.art_change = if reduced { 99.0 } else { self.art_change + dt };
        let cover_mix = (self.art_change / 1.1).clamp(0.0, 1.0);
        let art_morph = (self.art_change / ART_MORPH_S).clamp(0.0, 1.0);
        // A new track whose cover is not decoded yet: the old one holds, briefly. This is read
        // here, from the state as this frame shows it, so the hold starts on the very frame the
        // track changes (a click or key lands after the app's own per-frame work).
        let want = st.current().and_then(|t| State::cover_key(t.cover_template(), 400));
        let pending = want.is_some() && st.palette_key.is_some() && want != st.palette_key;
        self.art_wait = if pending { self.art_wait + dt } else { 0.0 };
        let art_hold = pending && self.art_wait < ART_WAIT_S;
        if off("fps") && device.features().contains(wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS) {
            let p = self.prof.get_or_insert_with(|| Prof {
                set: device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("frame time"), ty: wgpu::QueryType::Timestamp, count: 2 }),
                resolve: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("frame time"),
                    size: 16,
                    usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                }),
                read: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("frame time"),
                    size: 16,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                pending: Default::default(),
                state: 0,
                sum_ms: 0.0,
                n: 0,
                max_ms: 0.0,
            });
            if p.state == 1 {
                let flag = p.pending.clone();
                p.read.slice(..).map_async(wgpu::MapMode::Read, move |_| flag.store(true, std::sync::atomic::Ordering::Release));
                p.state = 2;
            }
            let _ = device.poll(wgpu::PollType::Poll);
            if p.state == 2 && p.pending.load(std::sync::atomic::Ordering::Acquire) {
                let v: [u64; 2] = match p.read.slice(..).get_mapped_range() {
                    Ok(data) => [u64::from_le_bytes(data[0..8].try_into().unwrap()), u64::from_le_bytes(data[8..16].try_into().unwrap())],
                    Err(_) => [0, 0],
                };
                p.read.unmap();
                p.state = 0;
                p.pending.store(false, std::sync::atomic::Ordering::Release);
                let ms = v[1].saturating_sub(v[0]) as f64 * f64::from(queue.get_timestamp_period()) / 1.0e6;
                p.sum_ms += ms;
                p.n += 1;
                p.max_ms = p.max_ms.max(ms);
            }
            if p.state == 0 {
                f.encoder.write_timestamp(&p.set, 0);
            }
        }

        // One pass over the slots: ours get a uniform block, rune-fx's become layers.
        let marks = &f.rt.marks;
        let mut at: Vec<u32> = Vec::new();
        let mut layers: Vec<FxLayer> = Vec::new();
        let mut placements: Vec<FxSlot> = Vec::new();
        let mut jobs: HashMap<u32, Job> = HashMap::new();
        let mut lows: Vec<(usize, [u32; 2], u8)> = Vec::new();
        let mut moving = art_changing;
        for (n, i) in f.rt.instances.iter().enumerate() {
            if i.params[1] != KIND_EFFECT {
                continue;
            }
            let mark = marks.get(n).copied().unwrap_or(0);
            let (kind, variant) = (mark & 0xff, (mark >> 8) & 0xff);
            let k = i.params[3];
            let d = i.uv;
            let skip = match kind {
                LIQUID => off("liquid"),
                GLASS => off("glass"),
                VEIL => off("veil"),
                SNAP => off("snap"),
                FIELD => off("field"),
                MORPH => off("morph"),
                BARS => off("bars"),
                ORB => off("orbs"),
                ARTMORPH => off("artmorph"),
                SHAPE | BUTTERFLY => off("shapes"),
                STAGE => off("stage"),
                _ => false,
            };
            if skip {
                continue;
            }
            match kind {
                FIELD => {
                    let mut u = base(i, k, pass_size, srgb, 0.0, 0.0, true);
                    let flat = quality == Effects::Off;
                    u.misc[2] = self.field_t;
                    u.p0 = [
                        if flat {
                            0.0
                        } else if theme::light() {
                            0.09
                        } else {
                            0.12
                        },
                        1.0,
                        sm.level,
                        sm.beat,
                    ];
                    u.tint = rgb4(canvas, 1.0);
                    u.ink = rgb4(ink, 0.0);
                    let u = gpu.upload(device, queue, &u);
                    jobs.insert(n as u32, Job { pipe: Pipe::Field, u, region: None, tag: 0, lo: None });
                    moving |= playing_level && !flat && !reduced;
                }
                PATTERN if d[0] < -0.5 => {
                    // A skeleton: one soft ray for the whole window, sweeping from the top left
                    // to the bottom right now and then (still, at one place, with less motion).
                    let mut u = base(i, k, pass_size, srgb, if reduced { 0.35 } else { t }, 0.0, true);
                    u.p0 = [-1.0, 0.0, 0.0, 0.0];
                    u.ink = rgb4([1.0, 1.0, 1.0], if theme::light() { 0.70 } else { 0.075 });
                    let u = gpu.upload(device, queue, &u);
                    jobs.insert(n as u32, Job { pipe: Pipe::Pattern, u, region: None, tag: 0, lo: None });
                    moving |= !reduced;
                }
                PATTERN => {
                    let mut u = base(i, k, pass_size, srgb, t, 0.0, true);
                    let live = quality == Effects::Full && !reduced && d[3] > 0.0;
                    u.p0 = [d[0], d[1], d[2], if live { d[3] } else { 0.0 }];
                    u.p1 = [(variant.max(2)) as f32 * k, 0.62, 0.0, 0.0];
                    u.ink = rgb4(ink, 0.0);
                    let u = gpu.upload(device, queue, &u);
                    jobs.insert(n as u32, Job { pipe: Pipe::Pattern, u, region: None, tag: 0, lo: None });
                    moving |= live;
                }
                GLASS => {
                    let s = &SPECS[(variant as usize).min(SPECS.len() - 1)];
                    let (hide, press, hover) = (d[0].clamp(0.0, 1.0), d[1].clamp(0.0, 1.0), d[2].clamp(0.0, 1.0));
                    let (mut refr, mut chroma, mut blur) = (s.refr, s.chroma, s.blur);
                    let mut tint = s.tint + 0.10 * hover + 0.16 * press;
                    match quality {
                        Effects::Off => {
                            (refr, chroma, blur) = (0.0, 0.0, 0.0);
                            tint += 0.35;
                        }
                        Effects::Light => {
                            chroma = 0.0;
                            refr *= 0.6;
                        }
                        Effects::Full => {}
                    }
                    let mut u = base(i, k, pass_size, srgb, t, s.reach * k, false);
                    let reach = refr * k + 8.0 * k;
                    let [x, y, w, h] = i.rect;
                    let region = (blur > 0.0).then(|| Region::plan([x - reach, y - reach, x + w + reach, y + h + reach], pass_size, blur * k)).flatten();
                    if let Some(r) = &region {
                        u.region = r.rect;
                    }
                    u.p0 = [refr * k, s.edge * k, chroma, if region.is_some() { blur * k } else { 0.0 }];
                    u.p1 = [0.0, s.sat, s.contrast, s.ambient];
                    u.p2 = [s.spec, s.spec_exp, s.rim, 1.0];
                    u.p3 = [s.shade, 1.0 - hide, -0.9, 3.0];
                    u.tint = rgb4(canvas, tint.clamp(0.0, 0.95));
                    // Near-black ink makes a heavy shadow on a light page, so it casts less there.
                    u.ink = rgb4(ink, if theme::light() { s.cast * 0.36 } else { s.cast });
                    let u = gpu.upload(device, queue, &u);
                    let tag = pyramid_tag(n as u32, region.is_some());
                    jobs.insert(n as u32, Job { pipe: Pipe::Glass, u, region, tag, lo: None });
                }
                VEIL => {
                    if quality == Effects::Off {
                        continue;
                    }
                    let mut u = base(i, k, pass_size, srgb, t, 0.0, true);
                    let [x, y, w, h] = i.rect;
                    let region = Region::plan([x, y, x + w, y + h], pass_size, d[2] * k);
                    if let Some(r) = &region {
                        u.region = r.rect;
                    }
                    // A top below zero is a veil over the whole slot at that strength (the sheet
                    // behind a cover opened large), not a band.
                    u.p0 = [if d[0] < 0.0 { d[0] } else { d[0] * k }, d[1] * k, d[2] * k, d[3]];
                    u.tint = rgb4(canvas, 1.0);
                    let u = gpu.upload(device, queue, &u);
                    let tag = pyramid_tag(n as u32, region.is_some());
                    jobs.insert(n as u32, Job { pipe: Pipe::Veil, u, region, tag, lo: None });
                }
                LIQUID if variant >= 2 => {
                    if quality == Effects::Off {
                        continue;
                    }
                    liquid_present[2] = true;
                    let appear = if reduced { 1.0 } else { ease_out((self.liquid_age[2] / LIQUID_APPEAR_S).clamp(0.0, 1.0)) };
                    moving |= appear < 1.0;
                    let [lines, reach, wild, ink_k] = contour_shape(variant);
                    let tint = if d[1] >= 0.0 { crate::palette::Palette::from_hue(d[1]).accent } else { accent };
                    let mut peak = [(d[3].floor() / 100.0).clamp(0.0, 1.0), d[3].fract()];
                    if variant == 5 {
                        if !self.ct_peak_primed || reduced {
                            self.ct_peak = peak;
                            self.ct_peak_primed = true;
                        }
                        let k = 1.0 - (-dt * 7.0).exp();
                        let before = self.ct_peak;
                        for j in 0..2 {
                            self.ct_peak[j] += (peak[j] - self.ct_peak[j]) * k;
                        }
                        if (before[0] - peak[0]).abs() + (before[1] - peak[1]).abs() > 0.0005 {
                            moving = true;
                        } else {
                            self.ct_peak = peak;
                        }
                        peak = self.ct_peak;
                    }
                    let mut u = base(i, k, pass_size, srgb, self.liquid_t, 0.0, true);
                    // The music: the map swells with the bass, its lines glow with the mids and shimmer
                    // with the highs, and every kick sends a ring out of the peak (`Smooth::listen`).
                    let live = if quality == Effects::Full && !reduced { 1.0 } else { 0.0 };
                    u.p0 = [d[0] * appear, variant as f32, if theme::light() { 0.8 } else { 1.0 }, sm.pulse * live];
                    u.p1 = [peak[0], peak[1], lines, d[2]];
                    u.p2 = rgb4(tint, 1.0);
                    u.p3 = rgb4(ink, 1.0);
                    u.p4 = [self.ct_phase, reach, wild, ink_k];
                    u.tint = [sm.norm[0] * live, sm.norm[1] * live, sm.norm[2] * live, sm.norm[3] * live];
                    let [s0, s1] = sm.shocks;
                    u.ink = [s0[0], s0[1] * live, s1[0], s1[1] * live];
                    let [_, _, w, h] = i.rect;
                    let div = (w.max(h) / 300.0).ceil().clamp(2.0, 8.0);
                    let dims = [((w / div).ceil() as u32).max(2), ((h / div).ceil() as u32).max(2)];
                    let mut lo = u;
                    lo.rect = [0.0, 0.0, dims[0] as f32, dims[1] as f32];
                    lo.bounds = lo.rect;
                    lo.view[0] = dims[0] as f32;
                    lo.view[1] = dims[1] as f32;
                    lo.misc[0] = 0.0;
                    lo.misc[1] = 0.0;
                    lo.region = [0.0, 0.0, w, h];
                    let lo = gpu.upload(device, queue, &lo);
                    let lo_tag = 128u8.saturating_add(lows.len().min(127) as u8);
                    lows.push((lo, dims, lo_tag));
                    let u = gpu.upload(device, queue, &u);
                    jobs.insert(n as u32, Job { pipe: Pipe::Liquid, u, region: None, tag: lo_tag, lo: Some(dims) });
                    moving |= quality == Effects::Full && !reduced && flow > 0.0;
                    // The landscape flows on the liquid's clock, so it counts as one being on screen.
                    ct_seen = true;
                }
                LIQUID => {
                    if quality == Effects::Off {
                        continue;
                    }
                    // d: strength, a hue in degrees to colour it by (below 0: the playing cover's), seed.
                    let target = if d[1] >= 0.0 { crate::palette::Palette::from_hue(d[1]).colors } else { sm.pal };
                    if !self.lp_primed {
                        self.lp = target;
                        self.lp_primed = true;
                    } else if !liquid_eased {
                        let k = 1.0 - (-dt * 3.0).exp();
                        for (c, t) in self.lp.iter_mut().zip(target) {
                            for j in 0..3 {
                                c[j] += (t[j] - c[j]) * k;
                            }
                        }
                    }
                    liquid_eased = true;
                    let wash = variant == 1;
                    let kind_ix = usize::from(wash);
                    liquid_present[kind_ix] = true;
                    let appear = if reduced { 1.0 } else { ease_out((self.liquid_age[kind_ix] / LIQUID_APPEAR_S).clamp(0.0, 1.0)) };
                    moving |= appear < 1.0;
                    let mut u = base(i, k, pass_size, srgb, self.liquid_t, 0.0, true);
                    let bright = if theme::light() {
                        1.0
                    } else if wash {
                        0.70
                    } else {
                        0.84
                    };
                    u.p0 = [d[0] * appear, if wash { 1.0 } else { 0.0 }, bright, sm.beat];
                    u.p1 = [self.lp[0][0], self.lp[0][1], self.lp[0][2], d[2]];
                    u.p2 = rgb4(self.lp[1], 0.0);
                    u.p3 = rgb4(self.lp[2], 0.0);
                    u.tint = rgb4(self.lp[3], 0.0);
                    u.ink = rgb4(self.lp[4], 0.0);
                    // The field is smooth: it is worked out on a texture a few times smaller than
                    // the slot (`liquid_lo`) and read back at full size.
                    let [_, _, w, h] = i.rect;
                    let div = (w.max(h) / 360.0).ceil().clamp(2.0, 6.0);
                    let dims = [((w / div).ceil() as u32).max(2), ((h / div).ceil() as u32).max(2)];
                    let mut lo = u;
                    lo.rect = [0.0, 0.0, dims[0] as f32, dims[1] as f32];
                    lo.bounds = lo.rect;
                    lo.view[0] = dims[0] as f32;
                    lo.view[1] = dims[1] as f32;
                    lo.misc[0] = 0.0;
                    lo.misc[1] = 0.0;
                    lo.region = [0.0, 0.0, w, h];
                    let lo = gpu.upload(device, queue, &lo);
                    let lo_tag = 128u8.saturating_add(lows.len().min(127) as u8);
                    lows.push((lo, dims, lo_tag));
                    let u = gpu.upload(device, queue, &u);
                    jobs.insert(n as u32, Job { pipe: Pipe::Liquid, u, region: None, tag: lo_tag, lo: Some(dims) });
                    moving |= quality == Effects::Full && !reduced && flow > 0.0;
                }
                MORPH => {
                    // d[0]: 1 while the music plays (the marker turns), 0 when it holds its pose.
                    let run = d[0] > 0.5 && !reduced;
                    let mut u = base(i, k, pass_size, srgb, 0.0, 0.0, true);
                    // How far the bars are up: they rise out of the dots when the music starts and
                    // settle back into them when it stops.
                    let want = if run { 1.0 } else { 0.0 };
                    self.marker_run += (want - self.marker_run) * (1.0 - (-dt * 9.0).exp());
                    if reduced {
                        self.marker_run = want;
                    }
                    u.p0 = [self.morph_t, MORPH_SECS, if reduced { 0.0 } else { self.marker_run }, 0.0];
                    u.p1 = [sm.norm[0], sm.norm[1], sm.norm[2], sm.norm[3]];
                    moving |= (self.marker_run - want).abs() > 0.002;
                    // d[1]: 1 for a marker that sits on a cover, which is white; else the accent.
                    u.tint = rgb4(if d[1] > 0.5 { [1.0, 1.0, 1.0] } else { accent }, 1.0);
                    let u = gpu.upload(device, queue, &u);
                    jobs.insert(n as u32, Job { pipe: Pipe::Morph, u, region: None, tag: 0, lo: None });
                    self.morph_on |= run;
                    moving |= run;
                }
                BARS => {
                    // d: a seed for the waveform, the pitch and the width of a bar in points.
                    let slot = bars_n.min(self.bars.len() - 1);
                    bars_n += 1;
                    let m = &mut self.bars[slot];
                    let p = &st.player;
                    let target = if p.dur_ms > 0 { (p.pos_ms as f32 / p.dur_ms as f32).clamp(0.0, 1.0) } else { 0.0 };
                    let still = reduced || quality == Effects::Off;
                    if m.seed < 0.0 || still {
                        let age = if m.seed < 0.0 && bars_were_up { APPEAR_S } else { m.age };
                        *m = BarMem { seed: d[0], from: d[0], swap: 1.0, shown: target, age, gone: 0.0 };
                    } else if (m.seed - d[0]).abs() > 0.01 {
                        // A new track: its waveform grows out of the last one's. (Interrupted
                        // halfway, the row keeps leaving the older one.)
                        if m.swap > 0.5 {
                            m.from = m.seed;
                        }
                        m.seed = d[0];
                        m.swap = 0.0;
                    }
                    m.gone = 0.0;
                    m.age += dt;
                    m.swap = (m.swap + dt / BARS_SWAP_S).min(1.0);
                    // The playhead as drawn eases to the real one, so a jump (a new track, a
                    // seek by key) is a quick glide and not a cut; dragging it follows the finger.
                    let follow = if st.scrubbing || still { 1.0 } else { 1.0 - (-dt * 16.0).exp() };
                    m.shown += (target - m.shown) * follow;
                    if (target - m.shown).abs() < 0.0004 {
                        m.shown = target;
                    } else {
                        moving = true;
                    }
                    moving |= m.swap < 1.0 || m.age < APPEAR_S || self.bars_live > 0.0;
                    let appear = if still { 1.0 } else { ease_out((m.age / APPEAR_S).clamp(0.0, 1.0)) };
                    let mut u = base(i, k, pass_size, srgb, t, 0.0, true);
                    u.region = [m.shown, sm.level, m.seed, self.bars_live];
                    let band = |j: usize| {
                        // Twelve groups of the 32 bands.
                        let (a, b) = (j * 32 / 12, ((j + 1) * 32 / 12).max(j * 32 / 12 + 1));
                        sm.bands[a..b.min(32)].iter().sum::<f32>() / (b.min(32) - a).max(1) as f32
                    };
                    u.p0 = [band(0), band(1), band(2), band(3)];
                    u.p1 = [band(4), band(5), band(6), band(7)];
                    u.p2 = [band(8), band(9), band(10), band(11)];
                    u.p3 = [d[1].max(2.0), d[2].max(1.0), sm.beat, self.bars_t];
                    u.p4 = [m.from, m.swap, appear, 0.0];
                    // Played or not, the bars are the text colour: white on the dark page.
                    u.tint = rgb4(ink, 1.0);
                    u.ink = rgb4(ink, 0.28);
                    let u = gpu.upload(device, queue, &u);
                    jobs.insert(n as u32, Job { pipe: Pipe::Bars, u, region: None, tag: 0, lo: None });
                }
                CHART => {
                    let mut u = base(i, k, pass_size, srgb, 0.0, 0.0, true);
                    u.p0 = d;
                    let u = gpu.upload(device, queue, &u);
                    jobs.insert(n as u32, Job { pipe: Pipe::Chart, u, region: None, tag: 0, lo: None });
                }
                ORB => {
                    // d: which mood, how chosen it is (0 or 1), how much of itself it shows when it is not.
                    orbs_present = true;
                    let mut u = base(i, k, pass_size, srgb, 0.0, 0.0, true);
                    let still = reduced || quality == Effects::Off;
                    // A mood turns into the chosen one (grows, brightens, glows) over a moment.
                    let ix = (d[0].max(0.0) as usize).min(7);
                    let on = &mut self.orb_on[ix];
                    if *on < 0.0 || still || (d[1] - *on).abs() < 0.005 {
                        *on = d[1];
                    } else {
                        *on += (d[1] - *on) * (1.0 - (-dt * 12.0).exp());
                        moving = true;
                    }
                    let appear = if still { 1.0 } else { ease_out((self.orb_age / APPEAR_S).clamp(0.0, 1.0)) };
                    moving |= appear < 1.0;
                    u.p0 = [d[0], *on, d[2], if still { 2.3 } else { self.wave_t }];
                    u.p1 = [appear, 0.0, 0.0, 0.0];
                    let u = gpu.upload(device, queue, &u);
                    jobs.insert(n as u32, Job { pipe: Pipe::Orb, u, region: None, tag: 0, lo: None });
                    moving |= !still && flow > 0.0;
                }
                ARTMORPH => {
                    // Only while a change is under way (or the new picture is awaited), and only
                    // with the full effects and motion.
                    let t = if art_hold { 0.0 } else { art_morph };
                    if quality != Effects::Full || reduced || !gpu.has_art() || (t >= 1.0 && !art_hold) {
                        continue;
                    }
                    let mut u = base(i, k, pass_size, srgb, 0.0, 0.0, true);
                    // Holding: the new track's cover is not decoded yet, so what is on the GPU as
                    // "playing" is still the last track's: show exactly that (the texture before
                    // it is two tracks old).
                    u.p0 = [t, d[0], if art_hold { 1.0 } else { 0.0 }, 0.0];
                    let u = gpu.upload(device, queue, &u);
                    jobs.insert(n as u32, Job { pipe: Pipe::ArtMorph, u, region: None, tag: 0, lo: None });
                    moving = true;
                }
                STAGE => {
                    if quality == Effects::Off {
                        continue;
                    }
                    let song = &mut self.song;
                    // This frame: the spectrum now, the look chosen in the player's panel, the beat.
                    let look = st.settings.stage;
                    let mut live = [[0.0f32; 4]; 11];
                    for (j, t) in live.iter_mut().take(8).enumerate() {
                        t.copy_from_slice(&song.spec[j * 4..j * 4 + 4]);
                    }
                    live[8] = [look.flash as f32, look.react as f32, 0.0, 0.0];
                    live[9] = [sm.beat, sm.pulse, if reduced { 0.0 } else { 1.0 }, 0.0];
                    live[10] = [song.bass, song.mid, song.treble, song.kick];
                    gpu.set_live(device, queue, &live);
                    // The cover's middle glides to where it is asked to be (it slides aside for the lyrics).
                    let want = [d[2], d[3]];
                    if !song.peak_primed || reduced {
                        (song.peak, song.peak_primed) = (want, true);
                    } else {
                        let e = 1.0 - (-dt * 7.0).exp();
                        for j in 0..2 {
                            song.peak[j] += (want[j] - song.peak[j]) * e;
                        }
                        moving |= (want[0] - song.peak[0]).abs() + (want[1] - song.peak[1]).abs() > 0.0005;
                    }
                    let cover_pt = (d[1] / 100.0).floor();
                    let seed = d[1] - cover_pt * 100.0;
                    let mut u = base(i, k, pass_size, srgb, 0.0, 0.0, true);
                    u.p0 = [d[0], seed, song.peak[0], song.peak[1]];
                    u.p1 = [song.clock, 0.0, 0.0, 0.0];
                    u.p2 = [0.0, song.loud, 0.0, 0.0];
                    let key = song.key_dir();
                    u.p3 = [key[0], key[1], song.harmony()[2], song.ground_t];
                    let art = if gpu.has_art() { cover_mix } else { 0.0 };
                    u.region = rgb4(sm.pal[0], art);
                    u.p4 = rgb4(sm.pal[2], 0.0);
                    u.tint = rgb4(sm.pal[4], cover_pt * 0.5 * k);
                    u.ink = rgb4(canvas, 1.0);
                    let u = gpu.upload(device, queue, &u);
                    jobs.insert(n as u32, Job { pipe: Pipe::Stage, u, region: None, tag: 0, lo: None });
                    moving |= song_live || (quality == Effects::Full && !reduced && flow > 0.0);
                    moving |= cover_mix < 1.0;
                    ct_seen = true;
                }
                SHAPE => {
                    shapes_present = true;
                    let still = reduced || quality == Effects::Off;
                    let flags = d[3].max(0.0) as u32;
                    let id = d[1].to_bits() ^ (d[0] as u32).wrapping_mul(0x9e37_79b9);
                    let m = self.shapes.entry(id).or_insert([-1.0, -1.0, 0.0]);
                    // Hover and choice ease in and out; a shape new to the page comes up in turn.
                    for (j, want) in [(flags & 2 != 0) as u32 as f32, (flags & 1) as f32].into_iter().enumerate() {
                        if m[j] < 0.0 || still || (want - m[j]).abs() < 0.004 {
                            m[j] = want;
                        } else {
                            m[j] += (want - m[j]) * (1.0 - (-dt * 11.0).exp());
                            moving = true;
                        }
                    }
                    m[2] += dt;
                    let order = (flags >> 2) as f32;
                    let appear = if still { 1.0 } else { ease_out(((m[2] - 0.04 * order) / 0.55).clamp(0.0, 1.0)) };
                    moving |= appear < 1.0;
                    let mut hue = d[2];
                    let mut tag = 0;
                    if hue <= -1000.0 {
                        hue = -hue - 1000.0;
                        let key = st.pic_key.borrow().clone();
                        if let Some(img) = key.and_then(|k| st.art.full(&k)) {
                            // Shrunk to what it covers on screen.
                            let side = i.rect[2].min(i.rect[3]).max(1.0) as u32;
                            gpu.set_pic(device, queue, std::sync::Arc::as_ptr(&img) as usize, &img, side);
                            if gpu.has_pic() {
                                hue = -2000.0;
                                tag = 1;
                            }
                        }
                    } else if hue < 0.0 && !gpu.has_art() {
                        hue = -hue - 1.0;
                    }
                    let mut u = base(i, k, pass_size, srgb, 0.0, 0.0, true);
                    u.p0 = [d[0], d[1], hue, if still { 3.0 } else { self.shape_t }];
                    u.p1 = [m[0], m[1], appear, if still { 0.0 } else { self.shape_music }];
                    // A shape filled with the cover dissolves into the next one, grain by grain.
                    let current_cover_ready = st.current().and_then(|t| State::cover_key(t.cover_template(), 400)).is_some_and(|key| st.palette_key.as_deref() == Some(key.as_str()));
                    let mix = if hue < 0.0 && hue > -1500.0 { if current_cover_ready { 1.0 } else { cover_mix } } else { 1.0 };
                    u.p2 = [mix, 0.0, 0.0, 0.0];
                    moving |= mix < 1.0;
                    u.ink = rgb4(ink, 1.0);
                    let u = gpu.upload(device, queue, &u);
                    jobs.insert(n as u32, Job { pipe: Pipe::Shape, u, region: None, tag, lo: None });
                    moving |= !still && flow > 0.0;
                    // They flow on the liquid's clock, so they stop with it when nothing happens.
                    ct_seen = true;
                }
                BUTTERFLY => {
                    shapes_present = true;
                    let Some(b) = st.swarm.bugs.get(d[0].max(0.0) as usize) else { continue };
                    let mut u = base(i, k, pass_size, srgb, 0.0, 0.0, true);
                    u.p0 = [8.0, d[1], d[2], self.shape_t];
                    u.p2 = [b.heading, b.open, b.bank, b.z];
                    u.ink = rgb4(ink, 1.0);
                    let u = gpu.upload(device, queue, &u);
                    jobs.insert(n as u32, Job { pipe: Pipe::Shape, u, region: None, tag: 0, lo: None });
                }
                SNAP => {
                    // Nothing is drawn here: the frame is copied at this slot, and every blur of
                    // the frame (the glass, the veils) is made from that copy.
                    let u = gpu.upload(device, queue, &base(i, k, pass_size, srgb, t, 0.0, true));
                    jobs.insert(n as u32, Job { pipe: Pipe::Snap, u, region: None, tag: 0, lo: None });
                }
                _ => {
                    if quality == Effects::Off && matches!(kind, SHIMMER | SPARKLES) {
                        continue;
                    }
                    let [x, y, w, h] = i.rect.map(|v| v / k);
                    let r = i.radii.iter().fold(0.0f32, |a, v| a.max(v.abs())) / k;
                    let Some(l) = layer(kind, d, Shape::rounded(x, y, w, h, r), t, accent) else { continue };
                    moving |= l.is_animating() && !reduced;
                    at.push(n as u32);
                    layers.push(l);
                    placements.push(FxSlot::default().scale(k).clip(i.clip, i.fade));
                }
            }
        }
        // A dynamic effect changes the pixels copied by SNAP, so its pyramid must be refreshed.
        // Static pages keep their existing pyramid textures and avoid the full downsample chain.
        let backdrop_dynamic = jobs.values().any(|job| {
            matches!(job.pipe, Pipe::Field | Pipe::Pattern | Pipe::Liquid | Pipe::Bars | Pipe::Orb | Pipe::ArtMorph | Pipe::Shape | Pipe::Stage)
        });
        let pyramid_key = if backdrop_dynamic { look ^ f.rt.clock.to_bits() } else { look };
        self.liquid_seen = liquid_eased || ct_seen;
        // A slot that was not drawn this frame has been left; when it comes back it starts again.
        for m in self.bars.iter_mut().skip(bars_n) {
            m.gone += dt;
            if m.gone > 0.25 {
                *m = BarMem::NEW;
            }
        }
        if shapes_present {
            self.shapes_gone = 0.0;
        } else {
            self.shapes_gone += dt;
            if self.shapes_gone > 0.25 {
                self.shapes.clear();
            }
        }
        if orbs_present {
            self.orb_gone = 0.0;
            self.orb_age += dt;
        } else {
            self.orb_gone += dt;
            if self.orb_gone > 0.25 {
                self.orb_age = 0.0;
            }
        }
        for (k, present) in liquid_present.into_iter().enumerate() {
            if present {
                self.liquid_gone[k] = 0.0;
                self.liquid_age[k] += dt;
            } else {
                // A slot rebuilt for a frame is the same slot; one gone for longer starts again.
                self.liquid_gone[k] += dt;
                if self.liquid_gone[k] > 0.25 {
                    self.liquid_age[k] = 0.0;
                }
            }
        }
        self.animating = moving || sm.level > 0.002 || sm.beat > 0.01 || sm.stirring();
        if off("fps") {
            self.frames += 1;
            if f.rt.clock - self.fps_at >= 1.0 {
                let gpu_ms = self.prof.as_ref().map_or(String::new(), |p| format!(" gpu {:.2} ms avg, {:.2} max over {} frames", p.sum_ms / f64::from(p.n.max(1)), p.max_ms, p.n));
                let n = f64::from((self.frames + self.reused).max(1));
                eprintln!(
                    "fps {:.1} (drawn {}, reused {}) runtime {:.2} ms, ours {:.2} ms per frame{gpu_ms}",
                    n / (f.rt.clock - self.fps_at),
                    self.frames,
                    self.reused,
                    self.upd_ms / n,
                    self.cpu_ms / n
                );
                let song = &self.song;
                let h = song.harmony();
                eprintln!(
                    "stage: loud {:.2} harmony {:.0} deg x{:.2}",
                    song.loud,
                    h[1].atan2(h[0]).to_degrees(),
                    h[2]
                );
                if let Some(p) = self.prof.as_mut() {
                    (p.sum_ms, p.n, p.max_ms) = (0.0, 0, 0.0);
                }
                (self.frames, self.reused, self.upd_ms, self.cpu_ms, self.fps_at) = (0, 0, 0.0, 0.0, f.rt.clock);
            }
        }

        let fx = self.fx.get_or_insert_with(|| FxRenderer::new(device, f.format, FxOptions { dither: true }));
        fx.set_srgb_blend(srgb);
        fx.add_format(device, wgpu::TextureFormat::Rgba16Float);
        // rune-fx builds an effect's pipeline the first time one is on screen: ~265 ms for the
        // loading shimmer, so the first page that showed skeletons (the first album or artist
        // opened) froze for a quarter of a second, and the first like would have too. Both are
        // built here, with the first frame. (rune-fx skips what is off screen, so the two sit in
        // the window's corner, a point big and all but invisible; they are only prepared, never drawn.)
        if !self.fx_warm {
            self.fx_warm = true;
            let spot = Shape::rounded(1.0, 1.0, 2.0, 2.0, 1.0);
            let warm: Vec<FxLayer> = [layer(SHIMMER, [0.01, 2.0, 0.45, 0.0], spot, t, accent), layer(SPARKLES, [t, 1.0, 0.5, 0.0], spot, t, accent)].into_iter().flatten().collect();
            let slots: Vec<FxSlot> = warm.iter().map(|_| FxSlot::default()).collect();
            fx.prepare_with_slots(device, queue, &FxFrame { viewport: [size.0, size.1], scale: 1.0, layers: &warm, backdrop: None, sources: &[] }, &slots);
        }
        fx.prepare_with_slots(device, queue, &FxFrame { viewport: [size.0, size.1], scale: 1.0, layers: &layers, backdrop: None, sources: &[] }, &placements);

        for (u, dims, tag) in lows {
            gpu.liquid_lo(device, f.encoder, u, dims, tag);
        }

        let clear = f.rt.clear_color();
        let timing = self.prof.as_ref().is_some_and(|p| p.state == 0);
        // The copy of the frame the snapshot slot asks for, kept for the glass and the veils drawn
        // after it.
        let mut snap: Option<wgpu::TextureView> = None;
        let mut snaps: Vec<u32> = jobs.iter().filter(|(_, j)| j.pipe == Pipe::Snap).map(|(n, _)| *n).collect();
        snaps.sort_unstable();
        f.ui.prepare(device, queue, &mut f.rt.text.atlas, &f.rt.instances, size);
        f.ui.draw_with_backdrop(f.encoder, f.target, clear, &mut |phase, s: &EffectSite<'_>| {
            if let Some(job) = jobs.get(&s.index) {
                if s.in_layer {
                    // Inside a group that fades: a slot that draws on its own (no backdrop) is
                    // drawn into the group's layer, so it fades with the group instead of
                    // vanishing until the fade is over. Glass and veils read the frame behind
                    // them, which a layer does not have, so they still wait.
                    if matches!(job.pipe, Pipe::Glass | Pipe::Veil | Pipe::Snap) {
                        return;
                    }
                    if let EffectPhase::Draw(pass) = phase {
                        gpu.ensure_pipe(device, s.format, job.pipe);
                        let u = gpu.upload_in_layer(device, queue, job.u, s.origin, s.size);
                        let b = Binds { bd: None, pyr: job.lo, tag: job.tag };
                        gpu.draw(device, pass, s.format, job.pipe, u, b);
                    }
                    return;
                }
                match phase {
                    EffectPhase::Backdrop(enc) => {
                        if job.pipe != Pipe::Snap {
                            return;
                        }
                        let Some(bd) = s.backdrop else { return };
                        // The blur of every slot drawn after this snapshot and before the next one
                        // (a second snapshot is taken only under a cover opened large, so the
                        // sheet's blur sees the rail and the bar too).
                        let next = snaps.iter().copied().find(|&n| n > s.index).unwrap_or(u32::MAX);
                        for (&n, other) in &jobs {
                            if n <= s.index || n >= next {
                                continue;
                            }
                            if let Some(r) = &other.region {
                                gpu.pyramid(device, enc, bd, other.u, r, other.tag, pyramid_key);
                            }
                        }
                        snap = Some(bd.clone());
                    }
                    EffectPhase::Draw(pass) => {
                        let reads_backdrop = matches!(job.pipe, Pipe::Glass | Pipe::Veil);
                        if job.pipe == Pipe::Snap || (reads_backdrop && snap.is_none()) {
                            return;
                        }
                        let b = Binds { bd: if reads_backdrop { snap.as_ref() } else { None }, pyr: job.region.map(|r| r.dims).or(job.lo), tag: job.tag };
                        gpu.draw(device, pass, s.format, job.pipe, job.u, b);
                    }
                }
                return;
            }
            let Some(k) = at.iter().position(|n| *n == s.index) else { return };
            match phase {
                EffectPhase::Backdrop(enc) => {
                    if let Some(bd) = s.backdrop {
                        fx.blur_backdrop(device, queue, enc, bd, k);
                    }
                }
                EffectPhase::Draw(pass) => fx.draw_layer_in(pass, s.format, s.origin, k),
            }
        });
        // A window that only moves on its own keeps this frame for the next one to show again.
        self.repeatable = self.animating;
        self.full_at = f.rt.clock;
        self.look = look;
        if self.repeatable {
            gpu.keep_frame(device, f.encoder, f.target, f.format);
        }
        if let (true, Some(p)) = (timing, self.prof.as_mut()) {
            f.encoder.write_timestamp(&p.set, 1);
            f.encoder.resolve_query_set(&p.set, 0..2, &p.resolve, 0);
            f.encoder.copy_buffer_to_buffer(&p.resolve, 0, &p.read, 0, 16);
            p.state = 1;
        }
    }
}
