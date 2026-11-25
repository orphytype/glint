//! Minimalist pre-lyrics bead loader that dissolves into grains colliding with the actual glyph
//! contours of the first lyric line.

use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;

use rune::{El, Shape, TextStyle, Transition, canvas, el};

use crate::app::{Load, State};
use crate::theme as th;

/// Lead-in window (ms) during which the 3 beads light up beat by beat before vocals begin.
const COUNT_MS: i64 = 3000;
/// Width and height of the minimalist loader slot above the first lyric line.
pub(super) const COUNT_W: f32 = 60.0;
const COUNT_H: f32 = 38.0;
/// Duration (seconds) over which dissolved grains bounce across the letters and fade out.
const GRAIN_S: f32 = 1.9;
/// Horizontal sub-samples per point for the cached letter-contour height map.
const SUB: f32 = 2.0;

/// What text sits immediately beneath `ly-note` so grains land on its real letter shapes.
pub(super) struct TargetLine<'a> {
    pub(super) text: &'a str,
    pub(super) style: TextStyle,
    pub(super) font: u8,
    /// Vertical distance in points from the top of `ly-note` to the top of this text box.
    pub(super) top_y: f32,
}

/// Start time in ms of the first sung line when late enough for an intro loader.
pub fn count_start(l: &ym_api::Lyrics) -> Option<u32> {
    if !l.synced {
        return None;
    }
    let first = l.lines.iter().find(|x| !x.text.trim().is_empty() || !x.words.is_empty())?;
    let ms = first.words.first().map(|w| w.start_ms).or(first.time_ms)?;
    (i64::from(ms) > COUNT_MS).then_some(ms)
}

/// Whether the intro loader or grain scatter is currently animating.
pub fn counting(s: &State) -> bool {
    let Load::Ready(l) = &s.lyrics.lyrics else { return false };
    let Some(first) = count_start(l) else { return false };
    let left = i64::from(first) - i64::from(s.lyric_ms());
    let tail = if crate::fx::cpu_mode() || s.settings.reduce_motion { 0 } else { (GRAIN_S * 1000.0) as i64 + 100 };
    s.player.playing && left > -tail
}

/// Cached 1D top-surface contour of the first wrapped visual line below `ly-note`.
struct LetterContour {
    /// For each column `i` (`x = i / SUB`), the highest ink Y relative to the text box top, or
    /// `f32::NAN` where the column is empty space (between letters or words).
    surface: Vec<f32>,
    /// Local surface tangent `dy / dx` at finite columns, for realistic bounces and rolling.
    slope: Vec<f32>,
    /// Horizontal span `[min_x, max_x]` containing ink.
    min_x: f32,
    max_x: f32,
    /// Approximate bottom of ink (baseline) relative to text box top.
    bottom_y: f32,
}

impl LetterContour {
    fn sample(&self, x: f32) -> Option<(f32, f32)> {
        if x < 0.0 || self.surface.is_empty() {
            return None;
        }
        let idx = (x * SUB).round() as isize;
        if idx < 0 || idx as usize >= self.surface.len() {
            return None;
        }
        let i = idx as usize;
        let y = self.surface[i];
        y.is_finite().then(|| (y, self.slope[i]))
    }

    /// True if there is ink within ±`radius` horizontally (prevents tiny grains slipping through 1px cracks).
    fn sample_with_radius(&self, x: f32, r: f32) -> Option<(f32, f32)> {
        if let Some(hit) = self.sample(x) {
            return Some(hit);
        }
        let left = self.sample(x - r * 0.65);
        let right = self.sample(x + r * 0.65);
        match (left, right) {
            (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    }
}

/// Extracts the first wrapped visual line of `text` fitting inside `max_w`.
fn first_visual_line(text: &str, style: TextStyle, max_w: f32) -> &str {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "";
    }
    if rune::text::measure(trimmed, &[], style, None).0 <= max_w + 0.5 {
        return trimmed;
    }
    let mut best_end = 0usize;
    let mut i = 0usize;
    let bytes = trimmed.as_bytes();
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let word_start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if word_start == i {
            break;
        }
        let cand = &trimmed[..i];
        if rune::text::measure(cand, &[], style, None).0 <= max_w + 0.5 {
            best_end = i;
        } else {
            if best_end == 0 {
                best_end = i;
            }
            break;
        }
    }
    if best_end == 0 { trimmed } else { &trimmed[..best_end] }
}

/// Selects bundled static TTF bytes matching `style` so swash can rasterize exact letter shapes
/// with zero extra `TextSystem` memory.
fn font_bytes_for(style: TextStyle, lyric_font: u8) -> Option<&'static [u8]> {
    if style.family == th::DISPLAY || (lyric_font == 2 && style.family != th::SANS) {
        crate::APP_FONTS.get(1).copied()
    } else if style.family == th::SANS {
        crate::APP_FONTS.first().copied()
    } else {
        None
    }
}

fn build_contour(text: &str, style: TextStyle, lyric_font: u8, block_w: f32, align: u8) -> LetterContour {
    let cols = ((block_w * SUB).ceil() as usize).max(1);
    let mut surface = vec![f32::NAN; cols];
    let vis = first_visual_line(text, style, block_w);
    let vis_w = rune::text::measure(vis, &[], style, None).0;
    let line_x0 = match align {
        1 => ((block_w - vis_w) * 0.5).max(0.0),
        2 => (block_w - vis_w).max(0.0),
        _ => 0.0,
    };

    let font_ref = font_bytes_for(style, lyric_font).and_then(|bytes| swash::FontRef::from_index(bytes, 0));
    let mut scale_cx = swash::scale::ScaleContext::new();
    let mut min_x = block_w;
    let mut max_x = 0.0f32;
    let mut bottom_y = style.line_height * 0.85;

    for (byte_idx, ch) in vis.char_indices() {
        if ch.is_whitespace() {
            continue;
        }
        let ch_str = &vis[byte_idx..byte_idx + ch.len_utf8()];
        let Some(ink) = rune::text::ink(ch_str, style) else { continue };
        let [ix, iy, iw, ih] = ink.bounds;
        if iw <= 0.1 || ih <= 0.1 {
            continue;
        }
        let char_x = line_x0 + rune::text::advance(&vis[..byte_idx], style);
        let gx0 = char_x + ix;
        let gx1 = gx0 + iw;
        min_x = min_x.min(gx0);
        max_x = max_x.max(gx1);
        bottom_y = bottom_y.max(iy + ih);

        let mut rasterized = false;
        if let Some(font) = font_ref {
            let gid = font.charmap().map(ch);
            if gid != 0 {
                let mut scaler = scale_cx.builder(font).size(style.size * SUB).hint(false).variations(&[("wght", style.weight)]).build();
                if let Some(img) = swash::scale::Render::new(&[swash::scale::Source::Outline]).format(swash::zeno::Format::Alpha).render(&mut scaler, gid) {
                    let (rw, rh) = (img.placement.width as usize, img.placement.height as usize);
                    if rw > 0 && rh > 0 && img.data.len() >= rw * rh {
                        // Find tight vertical ink bounds in the raster so we align 1:1 with `rune::text::ink`.
                        let mut r_top = rh;
                        let mut r_bot = 0usize;
                        let mut r_left = rw;
                        let mut r_right = 0usize;
                        for py in 0..rh {
                            for px in 0..rw {
                                if img.data[py * rw + px] >= 56 {
                                    r_top = r_top.min(py);
                                    r_bot = r_bot.max(py);
                                    r_left = r_left.min(px);
                                    r_right = r_right.max(px);
                                }
                            }
                        }
                        if r_top <= r_bot && r_left <= r_right {
                            let span_x = (r_right - r_left + 1) as f32;
                            let span_y = (r_bot - r_top + 1) as f32;
                            let c0 = ((gx0 * SUB).floor() as isize).clamp(0, cols as isize - 1) as usize;
                            let c1 = ((gx1 * SUB).ceil() as isize).clamp(0, cols as isize - 1) as usize;
                            for (c, slot) in surface.iter_mut().enumerate().take(c1 + 1).skip(c0) {
                                let x_pt = c as f32 / SUB;
                                let u = ((x_pt - gx0) / iw).clamp(0.0, 0.999);
                                let px = (r_left + (u * span_x) as usize).min(r_right);
                                if let Some(py) = (r_top..=r_bot).find(|&py| img.data[py * rw + px] >= 56) {
                                    let v = (py - r_top) as f32 / span_y;
                                    let y_pt = iy + v * ih;
                                    *slot = if slot.is_finite() { slot.min(y_pt) } else { y_pt };
                                }
                            }
                            rasterized = true;
                        }
                    }
                }
            }
        }
        if !rasterized {
            let c0 = ((gx0 * SUB).floor() as isize).clamp(0, cols as isize - 1) as usize;
            let c1 = ((gx1 * SUB).ceil() as isize).clamp(0, cols as isize - 1) as usize;
            for slot in surface.iter_mut().take(c1 + 1).skip(c0) {
                *slot = if slot.is_finite() { slot.min(iy) } else { iy };
            }
        }
    }

    let mut slope = vec![0.0f32; cols];
    for i in 0..cols {
        if !surface[i].is_finite() {
            continue;
        }
        let left = (i >= 2 && surface[i - 2].is_finite()).then(|| surface[i - 2]);
        let right = (i + 2 < cols && surface[i + 2].is_finite()).then(|| surface[i + 2]);
        let dy_dx = match (left, right) {
            (Some(l), Some(r)) => (r - l) / (4.0 / SUB),
            (Some(l), None) => (surface[i] - l) / (2.0 / SUB),
            (None, Some(r)) => (r - surface[i]) / (2.0 / SUB),
            (None, None) => 0.0,
        };
        slope[i] = dy_dx.clamp(-2.0, 2.0);
    }

    if min_x > max_x {
        (min_x, max_x) = (line_x0, (line_x0 + vis_w).min(block_w));
    }
    LetterContour { surface, slope, min_x, max_x, bottom_y }
}

fn with_contour<R>(target: &TargetLine<'_>, block_w: f32, align: u8, f: impl FnOnce(&LetterContour) -> R) -> R {
    thread_local! {
        static CACHE: RefCell<Option<(u64, LetterContour)>> = const { RefCell::new(None) };
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    target.text.hash(&mut hasher);
    target.style.size.to_bits().hash(&mut hasher);
    target.style.weight.to_bits().hash(&mut hasher);
    target.style.line_height.to_bits().hash(&mut hasher);
    target.font.hash(&mut hasher);
    ((block_w * 4.0) as u32).hash(&mut hasher);
    align.hash(&mut hasher);
    let key = hasher.finish();

    CACHE.with_borrow_mut(|slot| {
        if !slot.as_ref().is_some_and(|(k, _)| *k == key) {
            let contour = build_contour(target.text, target.style, target.font, block_w, align);
            *slot = Some((key, contour));
        }
        if let Some((_, contour)) = slot.as_ref() { f(contour) } else { unreachable!() }
    })
}

/// Integrates a single grain under gravity and bounces/rolls it against the real glyph top contour.
/// Returns `(x, y, visibility_alpha)` where grains falling through word spaces fade softly below the line.
fn simulate_on_contour(pos: [f32; 2], vel: [f32; 2], r: f32, top_y: f32, contour: &LetterContour, age: f32, block_w: f32) -> (f32, f32, f32) {
    let (mut x, mut y) = (pos[0], pos[1]);
    let (mut vx, mut vy) = (vel[0], vel[1]);
    const G: f32 = 1080.0;
    const DT: f32 = 1.0 / 60.0;
    let steps = (age / DT).ceil() as usize;
    let mut rem = age;
    let mut settled = false;
    let max_drop = top_y + contour.bottom_y + 12.0;

    for _ in 0..steps.min(120) {
        let dt = rem.min(DT);
        if dt <= 0.0 {
            break;
        }
        rem -= dt;
        if settled {
            // Gently check if resting on a steep slope; allow tiny slow roll.
            if let Some((sy, slope)) = contour.sample_with_radius(x, r) {
                let target_y = top_y + sy - r;
                if slope.abs() > 0.35 {
                    x += slope * 18.0 * dt;
                    if let Some((sy2, _)) = contour.sample_with_radius(x, r) {
                        y = top_y + sy2 - r;
                    } else {
                        settled = false;
                        vy = 18.0;
                    }
                } else {
                    y = target_y;
                }
            } else {
                settled = false;
            }
            continue;
        }
        let prev_y = y;
        vy += G * dt;
        vx *= 1.0 - 0.55 * dt;
        x = (x + vx * dt).clamp(r, (block_w - r).max(r));
        y += vy * dt;

        if let Some((sy, slope)) = contour.sample_with_radius(x, r) {
            let hit_y = top_y + sy - r;
            if vy > 0.0 && prev_y <= hit_y + 5.0 && y >= hit_y {
                y = hit_y;
                let inv = 1.0 / (1.0 + slope * slope).sqrt();
                let (nx, ny) = (-slope * inv, -inv);
                let (tx, ty) = (inv, slope * inv);
                let vn = vx * nx + vy * ny;
                let vt = vx * tx + vy * ty;
                if vn < 0.0 {
                    let bounce_n = -vn * 0.28;
                    // Push slightly along the letter's real slope so curved tops deflect grains naturally.
                    let slide_t = vt * 0.74 + slope * vn.abs() * 0.24;
                    vx = tx * slide_t + nx * bounce_n;
                    vy = ty * slide_t + ny * bounce_n;
                    if bounce_n < 24.0 && slide_t.abs() < 26.0 && slope.abs() < 0.55 {
                        vx = 0.0;
                        vy = 0.0;
                        settled = true;
                    }
                }
            }
        }
        if y >= max_drop {
            y = max_drop;
            break;
        }
    }

    let gap_fade = if y > top_y + contour.bottom_y * 0.42 { 1.0 - ((y - (top_y + contour.bottom_y * 0.42)) / (contour.bottom_y * 0.50 + 4.0)).clamp(0.0, 1.0) } else { 1.0 };
    (x, y, gap_fade)
}

/// Renders the minimalist 3-bead intro loader and its real letter-contour grain scatter.
pub(super) fn count_note(first: u32, pos: u32, w: f32, align: u8, reduce: bool, target: TargetLine<'_>) -> El {
    let since = COUNT_MS - (i64::from(first) - i64::from(pos));
    let x0 = match align {
        1 => ((w - COUNT_W) * 0.5).floor(),
        2 => (w - COUNT_W).max(0.0),
        _ => 0.0,
    };
    let wrap = el().key("ly-note").w_full().h(COUNT_H).transition(Transition::none());
    let ink = th::text();
    let beads_x = [x0 + 12.0, x0 + COUNT_W * 0.5, x0 + COUNT_W - 12.0];
    let cy = COUNT_H * 0.5;
    let ring = unit_dot();

    if since >= COUNT_MS {
        if crate::fx::cpu_mode() || reduce {
            return wrap;
        }
        let gone = (since - COUNT_MS) as f32 / 1000.0;
        if gone >= GRAIN_S {
            return wrap;
        }
        let fade = 1.0 - ((gone - 0.95) / (GRAIN_S - 0.95)).clamp(0.0, 1.0);
        let fade = fade * fade * (3.0 - 2.0 * fade);
        if fade <= 0.01 {
            return wrap;
        }
        let shape_and_h = with_contour(&target, w, align, |contour| {
            let mut shape = Shape::new();
            let canvas_h = target.top_y + contour.bottom_y + 18.0;
            // Choose a horizontal cascade band over the real letters near the loader.
            let span_l = contour.min_x.min(x0);
            let span_r = contour.max_x.max(x0 + COUNT_W);
            let (band_l, band_r) = match align {
                1 => {
                    let mid = (x0 + COUNT_W * 0.5).clamp(span_l, span_r);
                    ((mid - 155.0).max(span_l), (mid + 155.0).min(span_r))
                }
                2 => ((span_r - 280.0).max(span_l), span_r),
                _ => (span_l, (span_l + 280.0).min(span_r)),
            };
            const PER_BEAD: usize = 10;
            let total = 3 * PER_BEAD;
            for idx in 0..total {
                let bead = idx / PER_BEAD;
                let h = |k: u32| ((idx as u32).wrapping_mul(2_654_435_761).wrapping_add(k.wrapping_mul(40_503)) % 1000) as f32 / 1000.0;
                let ang = std::f32::consts::TAU * (idx % PER_BEAD) as f32 / PER_BEAD as f32 + h(0) * 0.5;
                let rad_in = 3.8 * h(1).sqrt();
                let sx = beads_x[bead] + ang.cos() * rad_in;
                let sy = cy + ang.sin() * rad_in;
                let start = 0.015 * bead as f32 + h(2) * 0.09;
                let age = gone - start;
                let r = 1.35 + 0.85 * h(3);
                let (gx, gy, gap_a) = if age > 0.0 {
                    // Aim particles evenly across the letter band so the whole silhouette is traced.
                    let frac = ((idx as f32 + 0.3 + 0.4 * h(4)) / total as f32).clamp(0.0, 1.0);
                    let aim_x = band_l + (band_r - band_l) * frac;
                    let flight = 0.30 + 0.14 * h(5);
                    let vx = ((aim_x - sx) / flight).clamp(-420.0, 420.0);
                    let vy = -(36.0 + 74.0 * h(6));
                    simulate_on_contour([sx, sy], [vx, vy], r, target.top_y, contour, age, w)
                } else {
                    (sx, sy, 1.0)
                };
                let alpha = 0.90 * fade * gap_a;
                if alpha <= 0.015 {
                    continue;
                }
                let poly: Vec<[f32; 2]> = ring.iter().map(|&[ux, uy]| [gx + r * ux, gy + r * uy]).collect();
                shape = shape.fill(poly, ink.alpha(alpha));
            }
            (shape, canvas_h)
        });
        let (shape, canvas_h) = shape_and_h;
        let layer = canvas(shape.view_box(w, canvas_h)).key("ly-note-grains").absolute(0.0, 0.0).w(w).h(canvas_h).transition(Transition::none()).motion(rune::Spring::NONE);
        return wrap.child(layer);
    }

    let mut shape = Shape::new();
    let t_sec = pos as f32 / 1000.0;
    let gather = if since > COUNT_MS - 260 { ((since - (COUNT_MS - 260)) as f32 / 260.0).clamp(0.0, 1.0) } else { 0.0 };
    let gather_e = gather * gather * (3.0 - 2.0 * gather);
    let mid_x = COUNT_W * 0.5;

    for i in 0..3 {
        let base_x = [12.0, COUNT_W * 0.5, COUNT_W - 12.0][i];
        let (r, dy, alpha) = if since < 0 {
            // Calm, meditative travelling breath wave while waiting through a longer intro.
            let wave = 0.5 + 0.5 * (t_sec * 2.4 - i as f32 * 0.85).sin();
            (3.5 + 1.0 * wave, -1.8 * wave, 0.28 + 0.34 * wave)
        } else {
            // Clean 3-beat progression: smooth cubic ease without bubble overshoot.
            let beat_ms = (since as f32 - i as f32 * 1000.0).max(0.0);
            let u = (beat_ms / 520.0).clamp(0.0, 1.0);
            let e = 1.0 - (1.0 - u).powi(3);
            let lift = if u > 0.0 && u < 1.0 { -3.6 * (std::f32::consts::PI * u).sin() * (1.0 - 0.3 * u) } else { 0.0 };
            let r = 3.7 + 1.15 * e + 0.35 * (std::f32::consts::PI * u).sin();
            let a = 0.26 + 0.68 * e + 0.04 * gather_e;
            (r, lift, a.min(0.96))
        };
        let bx = base_x + (mid_x - base_x) * (0.08 * gather_e);
        let by = cy + dy;
        let poly: Vec<[f32; 2]> = ring.iter().map(|&[ux, uy]| [bx + r * ux, by + r * uy]).collect();
        shape = shape.fill(poly, ink.alpha(alpha));
    }

    wrap.child(canvas(shape.view_box(COUNT_W, COUNT_H)).key("ly-note-fig").absolute(x0, 0.0).w(COUNT_W).h(COUNT_H).transition(Transition::none()).motion(rune::Spring::NONE))
}

fn unit_dot() -> &'static [[f32; 2]; 10] {
    static DOT: OnceLock<[[f32; 2]; 10]> = OnceLock::new();
    DOT.get_or_init(|| {
        std::array::from_fn(|k| {
            let ang = std::f32::consts::TAU * k as f32 / 10.0;
            [ang.cos(), ang.sin()]
        })
    })
}
