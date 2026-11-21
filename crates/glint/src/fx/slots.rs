//! Glass presets, slot helpers, and per-slot uniform staging for UI effects.

use rune::render::{Instance, KIND_EFFECT};
use rune_fx::{Fx, FxLayer, FxSlot, Shape, SparkleParams, rgb};

use super::gpu::{Gpu, Pipe, Region, U};
use super::{BARS, BUTTERFLY, FIELD, GLASS, Gfx, MORPH, SHAPE, SNAP, SPARKLES, STAGE, VEIL, WAVE_N, hex3, off_kind};
use crate::app::State;
use crate::store::Effects;
use crate::theme;

const MORPH_SECS: f32 = 1.5;
const BARS_SWAP_S: f32 = 0.32;
pub(super) const APPEAR_S: f32 = 0.35;

/// The glass presets; a `GLASS` slot's variant byte picks one (`glass(Glass::Dock)`).
#[derive(Clone, Copy, Debug)]
pub enum Glass {
    /// The dock: a long pill over scrolling content.
    Dock = 0,
    /// A card over the page.
    Card = 1,
    /// A small button or chip.
    Chip = 2,
    /// A menu or popover: a deep, quiet frost with hardly any rim.
    Menu = 3,
}

/// The mark for a glass slot.
pub const fn glass(g: Glass) -> u32 {
    GLASS | ((g as u32) << 8)
}

struct Spec {
    refr: f32,
    edge: f32,
    chroma: f32,
    blur: f32,
    sat: f32,
    ambient: f32,
    spec: f32,
    spec_exp: f32,
    rim: f32,
    shade: f32,
    tint: f32,
    cast: f32,
    reach: f32,
}

const SPECS: [Spec; 4] = [
    Spec { refr: 16.0, edge: 22.0, chroma: 0.38, blur: 12.0, sat: 1.10, ambient: 0.10, spec: 0.60, spec_exp: 22.0, rim: 0.42, shade: 0.16, tint: 0.34, cast: 0.0, reach: 0.0 },
    Spec { refr: 10.0, edge: 16.0, chroma: 0.35, blur: 10.0, sat: 1.05, ambient: 0.08, spec: 0.45, spec_exp: 20.0, rim: 0.32, shade: 0.12, tint: 0.28, cast: 0.20, reach: 22.0 },
    Spec { refr: 5.0, edge: 9.0, chroma: 0.20, blur: 6.0, sat: 1.05, ambient: 0.06, spec: 0.50, spec_exp: 20.0, rim: 0.38, shade: 0.08, tint: 0.24, cast: 0.0, reach: 0.0 },
    Spec { refr: 2.0, edge: 8.0, chroma: 0.0, blur: 16.0, sat: 1.45, ambient: 0.02, spec: 0.18, spec_exp: 24.0, rim: 0.16, shade: 0.0, tint: 0.16, cast: 0.28, reach: 26.0 },
];

pub(super) struct Job {
    pub(super) pipe: Pipe,
    pub(super) u: usize,
    pub(super) region: Option<Region>,
    pub(super) tag: u8,
}

#[derive(Clone, Copy)]
pub(super) struct BarMem {
    pub(super) seed: f32,
    from: f32,
    swap: f32,
    shown: f32,
    pub(super) age: f32,
    pub(super) gone: f32,
    real: f32,
}

impl BarMem {
    pub(super) const NEW: BarMem = BarMem { seed: -1.0, from: 0.0, swap: 1.0, shown: 0.0, age: 0.0, gone: 1.0, real: 0.0 };
}

pub(super) struct FrameCtx<'a> {
    pub(super) device: &'a wgpu::Device,
    pub(super) queue: &'a wgpu::Queue,
    pub(super) st: &'a State,
    pub(super) pass_size: [u32; 2],
    pub(super) srgb: bool,
    pub(super) dt: f32,
    pub(super) t: f32,
    pub(super) quality: Effects,
    pub(super) reduced: bool,
    pub(super) playing_level: bool,
    pub(super) song_live: bool,
    pub(super) flow: f32,
    pub(super) cover_mix: f32,
    pub(super) canvas: [f32; 3],
    pub(super) ink: [f32; 3],
    pub(super) accent: [f32; 3],
    pub(super) bars_were_up: bool,
    pub(super) cover_rect: Option<[f32; 4]>,
}

pub(super) struct SlotSummary {
    pub(super) moving: bool,
    pub(super) flows: bool,
    pub(super) shapes_present: bool,
    pub(super) bars_n: usize,
}

#[derive(Clone, Copy)]
pub(super) struct SlotRef<'a> {
    pub(super) idx: u32,
    pub(super) inst: &'a Instance,
    pub(super) scale: f32,
    pub(super) data: [f32; 4],
}

pub(super) fn layer(kind: u32, d: [f32; 4], shape: Shape, t: f32, accent: [f32; 3]) -> Option<FxLayer> {
    let fx = match kind {
        SPARKLES => {
            Fx::sparkles(SparkleParams { burst: (d[0] >= 0.0).then_some(d[0]), count: d[1] as u32, size: d[2], color: rgb(hex3(accent)), margin: 18.0, ..Default::default() })
        }
        _ => return None,
    };
    Some(fx.on(shape).at(t))
}

pub(super) fn base(i: &Instance, k: f32, pass: [u32; 2], srgb: bool, t: f32, grow: f32, keep_inside_clip: bool) -> U {
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

fn pyramid_tag(slot: u32, wanted: bool) -> u8 {
    if !wanted { 0 } else { 16 + (slot % 112) as u8 }
}

pub(super) fn ease_out(x: f32) -> f32 {
    1.0 - (1.0 - x).powi(3)
}

pub(super) fn rgb4(c: [f32; 3], a: f32) -> [f32; 4] {
    [c[0], c[1], c[2], a]
}

impl Gfx {
    pub(super) fn prepare_slots(&mut self, gpu: &mut Gpu, instances: &[Instance], marks: &[u32], ctx: &FrameCtx<'_>) -> SlotSummary {
        self.jobs.clear();
        self.snaps.clear();
        self.at.clear();
        self.layers.clear();
        self.placements.clear();

        let mut summary = SlotSummary { moving: false, flows: false, shapes_present: false, bars_n: 0 };
        let mut morph_stepped = false;

        for (n, i) in instances.iter().enumerate() {
            if i.params[1] != KIND_EFFECT {
                continue;
            }
            let mark = marks.get(n).copied().unwrap_or(0);
            let (kind, variant) = (mark & 0xff, (mark >> 8) & 0xff);
            if off_kind(kind) {
                continue;
            }
            let k = i.params[3];
            let d = i.uv;
            match kind {
                FIELD => {
                    let mut u = base(i, k, ctx.pass_size, ctx.srgb, 0.0, 0.0, true);
                    let flat = ctx.quality == Effects::Off;
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
                        self.smooth.level,
                        self.smooth.beat,
                    ];
                    u.tint = rgb4(ctx.canvas, 1.0);
                    u.ink = rgb4(ctx.ink, 0.0);
                    let u = gpu.stage(ctx.device, &u);
                    self.jobs.push((n as u32, Job { pipe: Pipe::Field, u, region: None, tag: 0 }));
                    summary.moving |= ctx.playing_level && !flat && !ctx.reduced;
                }
                GLASS => {
                    let s = &SPECS[(variant as usize).min(SPECS.len() - 1)];
                    let (hide, press, hover) = (d[0].clamp(0.0, 1.0), d[1].clamp(0.0, 1.0), d[2].clamp(0.0, 1.0));
                    if hide >= 0.999 {
                        continue;
                    }
                    let (mut refr, mut chroma, mut blur) = (s.refr, s.chroma, s.blur);
                    let mut tint = s.tint + 0.10 * hover + 0.16 * press;
                    match ctx.quality {
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
                    let mut u = base(i, k, ctx.pass_size, ctx.srgb, ctx.t, s.reach * k, false);
                    let reach = refr * k + 8.0 * k;
                    let [x, y, w, h] = i.rect;
                    let region = (blur > 0.0).then(|| Region::plan([x - reach, y - reach, x + w + reach, y + h + reach], ctx.pass_size, blur * k)).flatten();
                    if let Some(r) = &region {
                        u.region = r.rect;
                    }
                    u.p0 = [refr * k, s.edge * k, chroma, if region.is_some() { blur * k } else { 0.0 }];
                    u.p1 = [0.0, s.sat, 0.0, s.ambient];
                    u.p2 = [s.spec, s.spec_exp, s.rim, 1.0];
                    u.p3 = [s.shade, 1.0 - hide, -0.9, 3.0];
                    u.tint = rgb4(ctx.canvas, tint.clamp(0.0, 0.95));
                    u.ink = rgb4(ctx.ink, if theme::light() { s.cast * 0.36 } else { s.cast });
                    let u = gpu.stage(ctx.device, &u);
                    let tag = pyramid_tag(n as u32, region.is_some());
                    self.jobs.push((n as u32, Job { pipe: Pipe::Glass, u, region, tag }));
                }
                VEIL => {
                    if ctx.quality == Effects::Off || (d[0] == 0.0 && d[1] == 0.0) {
                        continue;
                    }
                    let mut u = base(i, k, ctx.pass_size, ctx.srgb, ctx.t, 0.0, true);
                    let [x, y, w, h] = i.rect;
                    let region = Region::plan([x, y, x + w, y + h], ctx.pass_size, d[2] * k);
                    if let Some(r) = &region {
                        u.region = r.rect;
                    }
                    u.p0 = [if d[0] < 0.0 { d[0] } else { d[0] * k }, d[1] * k, d[2] * k, d[3]];
                    u.tint = rgb4(ctx.canvas, 1.0);
                    let u = gpu.stage(ctx.device, &u);
                    let tag = pyramid_tag(n as u32, region.is_some());
                    self.jobs.push((n as u32, Job { pipe: Pipe::Veil, u, region, tag }));
                }
                MORPH => {
                    let run = d[0] > 0.5 && !ctx.reduced;
                    let want = if run { 1.0 } else { 0.0 };
                    if !morph_stepped {
                        morph_stepped = true;
                        self.marker_run += (want - self.marker_run) * (1.0 - (-ctx.dt * 9.0).exp());
                        if ctx.reduced {
                            self.marker_run = want;
                        }
                    }
                    let mut u = base(i, k, ctx.pass_size, ctx.srgb, 0.0, 0.0, true);
                    u.p0 = [self.morph_t, MORPH_SECS, if ctx.reduced { 0.0 } else { self.marker_run }, 0.0];
                    u.p1 = [self.smooth.norm[0], self.smooth.norm[1], self.smooth.norm[2], 0.0];
                    summary.moving |= (self.marker_run - want).abs() > 0.002;
                    u.tint = rgb4(if d[1] > 0.5 { [1.0, 1.0, 1.0] } else { ctx.accent }, 1.0);
                    let u = gpu.stage(ctx.device, &u);
                    self.jobs.push((n as u32, Job { pipe: Pipe::Morph, u, region: None, tag: 0 }));
                    self.morph_on |= run;
                    summary.moving |= run;
                }
                BARS => self.stage_bars(gpu, SlotRef { idx: n as u32, inst: i, scale: k, data: d }, ctx, &mut summary),
                STAGE => self.stage_player(gpu, SlotRef { idx: n as u32, inst: i, scale: k, data: d }, ctx, &mut summary),
                SHAPE => self.stage_shape(gpu, SlotRef { idx: n as u32, inst: i, scale: k, data: d }, ctx, &mut summary),
                BUTTERFLY => self.stage_butterfly(gpu, SlotRef { idx: n as u32, inst: i, scale: k, data: d }, ctx, &mut summary),
                SNAP => {
                    // Snapshot slots only trigger backdrop capture; nothing is staged or drawn.
                    self.snaps.push(n as u32);
                }
                _ => {
                    if ctx.quality == Effects::Off && kind == SPARKLES {
                        continue;
                    }
                    let [x, y, w, h] = i.rect.map(|v| v / k);
                    let r = i.radii.iter().fold(0.0f32, |a, v| a.max(v.abs())) / k;
                    let Some(l) = layer(kind, d, Shape::rounded(x, y, w, h, r), ctx.t, ctx.accent) else { continue };
                    summary.moving |= l.is_animating() && !ctx.reduced;
                    self.at.push(n as u32);
                    self.layers.push(l);
                    self.placements.push(FxSlot::default().scale(k).clip(i.clip, i.fade));
                }
            }
        }
        summary
    }

    fn stage_bars(&mut self, gpu: &mut Gpu, slot_ref: SlotRef<'_>, ctx: &FrameCtx<'_>, summary: &mut SlotSummary) {
        let SlotRef { idx: slot_idx, inst: i, scale: k, data: d } = slot_ref;
        let slot = summary.bars_n.min(self.bars.len() - 1);
        summary.bars_n += 1;
        let m = &mut self.bars[slot];
        let p = &ctx.st.player;
        let target = if p.dur_ms > 0 { (p.pos_ms as f32 / p.dur_ms as f32).clamp(0.0, 1.0) } else { 0.0 };
        let still = ctx.reduced || ctx.quality == Effects::Off;
        if m.seed < 0.0 || still {
            let age = if m.seed < 0.0 && ctx.bars_were_up { APPEAR_S } else { m.age };
            *m = BarMem { seed: d[0], from: d[0], swap: 1.0, shown: target, age, gone: 0.0, real: self.wave_mix };
        } else if (m.seed - d[0]).abs() > 0.01 {
            if m.swap > 0.5 {
                m.from = m.seed;
            }
            m.seed = d[0];
            m.swap = 0.0;
        }
        m.gone = 0.0;
        m.age += ctx.dt;
        m.swap = (m.swap + ctx.dt / BARS_SWAP_S).min(1.0);
        let follow = if ctx.st.scrubbing || still { 1.0 } else { 1.0 - (-ctx.dt * 16.0).exp() };
        m.shown += (target - m.shown) * follow;
        if (target - m.shown).abs() < 0.0004 {
            m.shown = target;
        } else {
            summary.moving = true;
        }
        summary.moving |= m.swap < 1.0 || m.age < APPEAR_S || self.bars_live > 0.0;
        let appear = if still { 1.0 } else { ease_out((m.age / APPEAR_S).clamp(0.0, 1.0)) };
        let wave = ctx.st.current().and_then(|t| ctx.st.waveforms.get(&t.id).map(|w| (t.id.as_str(), w)));
        if let Some((id, w)) = wave
            && id != self.wave_id
        {
            let shown_real = self.wave_mix > 0.5 && !self.wave_now.is_empty();
            let from: Vec<f32> = if shown_real { self.wave_now.clone() } else { vec![0.0; WAVE_N] };
            self.wave_now = w.to_vec();
            self.wave_now.resize(WAVE_N, 0.0);
            gpu.set_wave(ctx.device, ctx.queue, &self.wave_now, &from);
            self.wave_id = id.to_owned();
            self.wave_from_real = shown_real;
            self.wave_mix = 0.0;
        }
        let real_target = if wave.is_some() { 1.0 } else { 0.0 };
        if still {
            self.wave_mix = real_target;
        } else if slot == 0 {
            self.wave_mix += (real_target - self.wave_mix) * (1.0 - (-ctx.dt * 9.0).exp());
            if (real_target - self.wave_mix).abs() < 0.002 {
                self.wave_mix = real_target;
            }
        }
        summary.moving |= self.wave_mix != real_target;
        m.real = self.wave_mix;
        let mut u = base(i, k, ctx.pass_size, ctx.srgb, ctx.t, 0.0, true);
        u.region = [m.shown, self.smooth.level, m.seed, self.bars_live];
        let spec = &ctx.st.spectrum;
        let band = |j: usize| {
            let (a, b) = (j * 32 / 12, ((j + 1) * 32 / 12).max(j * 32 / 12 + 1));
            spec[a..b.min(32)].iter().sum::<f32>() / (b.min(32) - a).max(1) as f32
        };
        u.p0 = [band(0), band(1), band(2), band(3)];
        u.p1 = [band(4), band(5), band(6), band(7)];
        u.p2 = [band(8), band(9), band(10), band(11)];
        u.p3 = [d[1].max(2.0), d[2].max(1.0), self.smooth.beat, self.bars_t];
        u.p4 = [m.from, m.swap, appear, m.real + if self.wave_from_real { 2.0 } else { 0.0 }];
        u.tint = rgb4(ctx.ink, 1.0);
        u.ink = rgb4(ctx.ink, 0.28);
        let u = gpu.stage(ctx.device, &u);
        self.jobs.push((slot_idx, Job { pipe: Pipe::Bars, u, region: None, tag: 0 }));
    }
}
