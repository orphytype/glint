//! Open-player stage backdrop and Home garden shape/butterfly slot staging.

use std::sync::atomic::Ordering;

use rune::render::{Instance, KIND_EFFECT};

use super::gpu::{Gpu, Pipe};
use super::slots::{FrameCtx, Job, SlotRef, SlotSummary, base, ease_out, rgb4};
use super::{COVER_MARK, Gfx, WAVE_FRAME, WAVE_POINTER};

fn hsl_rgb(h_deg: f32, s: f32, l: f32) -> [f32; 3] {
    let h = h_deg.rem_euclid(360.0) / 60.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c * 0.5;
    [(r1 + m).clamp(0.0, 1.0), (g1 + m).clamp(0.0, 1.0), (b1 + m).clamp(0.0, 1.0)]
}

/// Computes a rich, non-clipping fluid tint from either the playing cover's palette or the active
/// preset hue, preventing dark/monochrome covers from blowing out to flat white.
fn wave_target_rgb(cover_accent: [f32; 3], cover_wave: [f32; 3], has_cover: bool, preset_hue: f32, light: bool) -> [f32; 3] {
    let preset = hsl_rgb(preset_hue, if light { 0.72 } else { 0.78 }, if light { 0.52 } else { 0.48 });
    if !has_cover {
        return preset;
    }
    let cov_sat = cover_wave[1].clamp(0.0, 1.0);
    if cov_sat < 0.14 {
        // Monochrome or near-neutral album art: classy deep slate-indigo tinted slightly by preset.
        let slate = if light { [0.46, 0.50, 0.62] } else { [0.36, 0.41, 0.56] };
        let mix = (cov_sat / 0.14) * 0.35;
        return [
            slate[0] + (preset[0] - slate[0]) * mix,
            slate[1] + (preset[1] - slate[1]) * mix,
            slate[2] + (preset[2] - slate[2]) * mix,
        ];
    }
    // Saturated jewel-tone cover hue so Yandex-style fluid contours and right-stage color wash
    // stay vibrant and rich (never washed out to pastel fog or flat white).
    let from_hue = hsl_rgb(cover_wave[0], (cov_sat * 1.05).clamp(0.62, 0.94), if light { 0.52 } else { 0.48 });
    let cmax = cover_accent[0].max(cover_accent[1]).max(cover_accent[2]).max(0.18);
    let scale = (if light { 0.80 } else { 0.78 } / cmax).clamp(0.65, 2.0);
    let norm = [(cover_accent[0] * scale).clamp(0.04, 0.94), (cover_accent[1] * scale).clamp(0.04, 0.94), (cover_accent[2] * scale).clamp(0.04, 0.94)];
    [from_hue[0] * 0.62 + norm[0] * 0.38, from_hue[1] * 0.62 + norm[1] * 0.38, from_hue[2] * 0.62 + norm[2] * 0.38]
}

/// Derives the 3-stop sRGB palette (`shadow`, `wash`, `crest`) for the 3D-texture wave playback
/// from the smoothed target hue and active page canvas so ridges glow cleanly and shadows blend
/// seamlessly into the dark or light page background.
fn wave_stops(rgb: [f32; 3], canvas: [f32; 3], light: bool) -> ([f32; 3], [f32; 3], [f32; 3]) {
    if light {
        let shadow = [
            (canvas[0] * 0.94 + rgb[0] * 0.06).clamp(0.0, 1.0),
            (canvas[1] * 0.94 + rgb[1] * 0.06).clamp(0.0, 1.0),
            (canvas[2] * 0.94 + rgb[2] * 0.06).clamp(0.0, 1.0),
        ];
        let wash = [
            (canvas[0] * 0.60 + rgb[0] * 0.40).clamp(0.0, 1.0),
            (canvas[1] * 0.60 + rgb[1] * 0.40).clamp(0.0, 1.0),
            (canvas[2] * 0.60 + rgb[2] * 0.40).clamp(0.0, 1.0),
        ];
        let crest = [
            (rgb[0] * 0.80 + 0.20).clamp(0.0, 1.0),
            (rgb[1] * 0.80 + 0.20).clamp(0.0, 1.0),
            (rgb[2] * 0.80 + 0.20).clamp(0.0, 1.0),
        ];
        return (shadow, wash, crest);
    }
    let shadow = [
        (canvas[0] * 0.92 + rgb[0] * 0.08).clamp(0.0, 1.0),
        (canvas[1] * 0.92 + rgb[1] * 0.08).clamp(0.0, 1.0),
        (canvas[2] * 0.92 + rgb[2] * 0.08).clamp(0.0, 1.0),
    ];
    // Cap perceptual wash luminance in dark mode so bright yellow/lime covers keep velvety depth
    // (like the reference wave) and white typography stays effortlessly legible on every track.
    let raw_wash = [rgb[0] * 0.84, rgb[1] * 0.84, rgb[2] * 0.84];
    let luma = 0.2126 * raw_wash[0] + 0.7152 * raw_wash[1] + 0.0722 * raw_wash[2];
    let scale = if luma > 0.34 { (0.34 / luma).powf(0.72) } else { 1.0 };
    let wash = [(raw_wash[0] * scale).clamp(0.0, 1.0), (raw_wash[1] * scale).clamp(0.0, 1.0), (raw_wash[2] * scale).clamp(0.0, 1.0)];
    let crest = [
        (0.70 + rgb[0] * 0.30).clamp(0.0, 1.0),
        (0.68 + rgb[1] * 0.32).clamp(0.0, 1.0),
        (0.66 + rgb[2] * 0.34).clamp(0.0, 1.0),
    ];
    (shadow, wash, crest)
}

/// Finds the largest `COVER_MARK` rect drawn this frame (only scanned while the player is visible).
pub(super) fn find_cover_rect(instances: &[Instance], marks: &[u32]) -> Option<[f32; 4]> {
    instances
        .iter()
        .enumerate()
        .filter(|(n, i)| i.params[1] == KIND_EFFECT && marks.get(*n).copied().unwrap_or(0) & 0xff == COVER_MARK)
        .map(|(_, i)| i.rect)
        .max_by(|a, b| (a[2] * a[3]).total_cmp(&(b[2] * b[3])))
}

impl Gfx {
    pub(super) fn stage_player(&mut self, gpu: &mut Gpu, slot_ref: SlotRef<'_>, ctx: &FrameCtx<'_>, summary: &mut SlotSummary) {
        let SlotRef { idx: slot_idx, inst: i, scale: k, data: d } = slot_ref;
        if !ctx.shaders {
            return;
        }
        let song = &mut self.song;
        let look = ctx.st.settings.stage;
        let mut live = [[0.0f32; 4]; 11];
        for (j, t) in live.iter_mut().take(8).enumerate() {
            t.copy_from_slice(&song.spec[j * 4..j * 4 + 4]);
        }
        live[8] = [look.effects as f32, 0.0, 0.0, 0.0];
        song.follow_beat(ctx.dt, if ctx.st.player.playing { self.smooth.beat } else { 0.0 });
        live[9] = [self.smooth.beat, song.throb, if ctx.reduced { 0.0 } else { 1.0 }, song.snare];
        live[10] = [song.bass, song.voice, song.treble, song.kick];
        gpu.set_live(ctx.device, ctx.queue, &live);
        let cover_pt = (d[1] / 100.0).floor();
        let seed = d[1] - cover_pt * 100.0;
        let [sx, sy, sw, sh] = i.rect;
        let mut half = cover_pt * 0.5 * k;
        if let Some([cx, cy, cw, ch]) = ctx.cover_rect.filter(|r| r[2] > 1.0 && r[3] > 1.0) {
            song.peak = [(cx + cw * 0.5 - sx) / sw.max(1.0), (cy + ch * 0.5 - sy) / sh.max(1.0)];
            song.peak_primed = true;
            half = cw.min(ch) * 0.5;
        } else {
            let want = [d[2], d[3]];
            if !song.peak_primed || ctx.reduced {
                (song.peak, song.peak_primed) = (want, true);
            } else {
                let e = 1.0 - (-ctx.dt * 7.0).exp();
                for j in 0..2 {
                    song.peak[j] += (want[j] - song.peak[j]) * e;
                }
                summary.moving |= (want[0] - song.peak[0]).abs() + (want[1] - song.peak[1]).abs() > 0.0005;
            }
        }
        let bd = ctx.st.backdrop.as_ref().filter(|_| ctx.st.settings.backdrop.on);
        if let Some(img) = bd {
            gpu.set_backdrop(ctx.device, ctx.queue, img);
        }
        gpu.use_backdrop(bd.is_some());
        let look_bd = ctx.st.settings.backdrop;
        let mut u = base(i, k, ctx.pass_size, ctx.srgb, 0.0, 0.0, true);
        u.p0 = [d[0], seed, song.peak[0], song.peak[1]];
        u.p1 = [song.clock, if bd.is_some() { 1.0 } else { 0.0 }, look_bd.blur * 7.0, look_bd.dim];
        u.p2 = [0.0, song.loud, f32::from(look_bd.effect), bd.map_or(1.0, |b| b.aspect())];
        summary.moving |= bd.is_some() && look_bd.effect == 2 && !ctx.reduced;
        let key = song.key_dir();
        u.p3 = [key[0], key[1], song.harmony()[2], song.ground_t];
        u.region = rgb4(self.smooth.pal[0], 0.0);
        u.p4 = rgb4(self.smooth.pal[1], 0.0);
        u.tint = rgb4(self.smooth.pal[2], half);
        u.ink = rgb4(ctx.canvas, 1.0);
        let u = gpu.stage(ctx.device, &u);
        self.jobs.push((slot_idx, Job { pipe: Pipe::Stage, u, region: None, tag: 0, shared: false }));
        let stage_anim = look.effects > 0;
        summary.audio_reactive |= stage_anim && ctx.st.player.playing;
        summary.moving |= ctx.song_live && stage_anim;
    }

    pub(super) fn stage_shape(&mut self, gpu: &mut Gpu, slot_ref: SlotRef<'_>, ctx: &FrameCtx<'_>, summary: &mut SlotSummary) {
        let SlotRef { idx: slot_idx, inst: i, scale: k, data: d } = slot_ref;
        summary.shapes_present = true;
        let still = ctx.reduced || !ctx.shaders;
        let flags = d[3].max(0.0) as u32;
        let id = d[1].to_bits() ^ (d[0] as u32).wrapping_mul(0x9e37_79b9);
        let m = self.shapes.entry(id).or_insert([-1.0, -1.0, 0.0, 0.0]);
        for (j, want) in [(flags & 2 != 0) as u32 as f32, (flags & 1) as f32].into_iter().enumerate() {
            if m[j] < 0.0 || still || (want - m[j]).abs() < 0.004 {
                m[j] = want;
            } else {
                m[j] += (want - m[j]) * (1.0 - (-ctx.dt * 11.0).exp());
                summary.moving = true;
            }
        }
        m[2] += ctx.dt;
        if !still {
            m[3] += ctx.dt * 0.8 * m[0];
        }
        let order = (flags >> 2) as f32;
        let appear = if still { 1.0 } else { ease_out(((m[2] - 0.04 * order) / 0.55).clamp(0.0, 1.0)) };
        summary.moving |= appear < 1.0;
        let mut hue = d[2];
        let whole = hue <= -500.0;
        if whole {
            hue += 500.0;
        }
        if hue < 0.0 && !gpu.has_art() {
            hue = -hue - 1.0;
        }
        let fam = d[0] as u32;
        let pipe = match fam {
            17 => Pipe::Ribbon,
            19 => Pipe::Wave,
            _ => Pipe::Mood,
        };
        let mut u = base(i, k, ctx.pass_size, ctx.srgb, 0.0, 0.0, true);
        let raw_t = if still { 3.0 } else { self.shape_t + m[3] };
        let shape_t = if pipe == Pipe::Ribbon { (raw_t * 24.0).floor() / 24.0 } else { raw_t };
        u.p0 = [d[0], d[1], hue, shape_t];
        u.p1 = [m[0], m[1], appear, if still { 0.0 } else { self.shape_music }];
        let mix = if hue < 0.0 && !whole { ctx.cover_mix } else { 1.0 };
        let ribbon_hide = if pipe == Pipe::Ribbon { (order / 60.0).clamp(0.0, 1.0) } else { 0.0 };
        u.p2 = [mix, ribbon_hide, 0.0, 0.0];
        summary.moving |= mix < 1.0;
        if pipe == Pipe::Wave {
            let has_cover = d[2] < 0.0 && ctx.st.palette_key.is_some();
            let target_rgb = wave_target_rgb(ctx.st.palette.accent, ctx.st.palette.wave, has_cover, hue.abs(), crate::theme::light());
            if !self.wave_rgb_init || still {
                self.wave_rgb = target_rgb;
                self.wave_rgb_init = true;
            } else {
                let blend = 1.0 - (-ctx.dt * 2.2).exp();
                let mut diff = 0.0f32;
                for j in 0..3 {
                    let delta = target_rgb[j] - self.wave_rgb[j];
                    self.wave_rgb[j] += delta * blend;
                    diff += delta.abs();
                }
                if diff > 0.004 {
                    summary.moving = true;
                }
            }
            let (shadow, wash, crest) = wave_stops(self.wave_rgb, ctx.canvas, crate::theme::light());
            u.p0[3] = if still { 1.8 } else { self.wave_t };
            u.p1 = [m[0], m[1], appear, if still { 0.0 } else { self.wave_energy }];
            u.region = [
                f32::from_bits(WAVE_FRAME[0].load(Ordering::Relaxed)),
                f32::from_bits(WAVE_FRAME[1].load(Ordering::Relaxed)),
                f32::from_bits(WAVE_FRAME[2].load(Ordering::Relaxed)),
                f32::from_bits(WAVE_FRAME[3].load(Ordering::Relaxed)),
            ];
            u.p3 = rgb4(shadow, 1.0);
            u.tint = rgb4(wash, 1.0);
            u.p4 = rgb4(crest, 1.0);
            if !still {
                let want = [f32::from_bits(WAVE_POINTER[0].load(Ordering::Relaxed)), f32::from_bits(WAVE_POINTER[1].load(Ordering::Relaxed))];
                let p = &mut self.wave_ptr;
                let ease = 1.0 - (-ctx.dt * 5.0).exp();
                if p[2] < 0.01 {
                    p[0] = want[0];
                    p[1] = want[1];
                } else {
                    p[0] += (want[0] - p[0]) * ease;
                    p[1] += (want[1] - p[1]) * ease;
                }
                p[2] = m[0];
                u.p2 = [mix, p[0], p[1], p[2]];
            }
        }
        u.ink = rgb4(ctx.ink, 1.0);
        let u = gpu.stage(ctx.device, &u);
        self.jobs.push((slot_idx, Job { pipe, u, region: None, tag: 0, shared: false }));
        let active_ribbon = pipe == Pipe::Ribbon && (m[0] > 0.0 || m[1] > 0.0);
        if pipe == Pipe::Ribbon {
            summary.ribbon_moving |= !still && active_ribbon;
        } else if fam == 19 {
            // Stage fluid veils drift continuously at calm speed while Stage Home is visible.
            summary.moving |= !still;
            summary.audio_reactive |= !still && ctx.st.player.playing;
        } else {
            summary.audio_reactive |= !still;
            summary.moving |= !still && (m[0] > 0.0 || ctx.flow > 0.0);
            summary.flows = true;
        }
    }

    pub(super) fn stage_butterfly(&mut self, gpu: &mut Gpu, slot_ref: SlotRef<'_>, ctx: &FrameCtx<'_>, summary: &mut SlotSummary) {
        let SlotRef { idx: slot_idx, inst: i, scale: k, data: d } = slot_ref;
        summary.shapes_present = true;
        let Some(b) = ctx.st.swarm.bugs.get(d[0].max(0.0) as usize) else { return };
        let mut u = base(i, k, ctx.pass_size, ctx.srgb, 0.0, 0.0, true);
        u.p0 = [8.0, d[1], d[2], self.shape_t];
        u.p2 = [b.heading, b.open, b.bank, b.z];
        u.ink = rgb4(ctx.ink, 1.0);
        let u = gpu.stage(ctx.device, &u);
        self.jobs.push((slot_idx, Job { pipe: Pipe::Butterfly, u, region: None, tag: 0, shared: false }));
    }
}
