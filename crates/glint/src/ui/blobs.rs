//! The shapes of Home. Each is drawn on the GPU (`fx::SHAPE`, `fs_shape`) from a family and a
//! seed: My Wave a body of glowing liquid, each mood a picture of what it is (a heart, a moon, a
//! crown...) printed in the same liquid in its own colour,
//! a wave card's ribbons, an artist's picture. The seed sets how a shape's edge swells and how its
//! colours flow, so the same seed draws the same garden; a new start of the app, or the button
//! that rolls them, draws another.

use std::cell::RefCell;
use std::collections::HashMap;

use rune::{El, Pixels, Transition, el};

use crate::fx;
use crate::theme as th;

/// A small, fast, seedable random source (xorshift64*).
#[derive(Clone, Copy)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng((seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03) | 1)
    }

    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// In `lo..hi`.
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * ((self.next() >> 40) as f32 / (1u64 << 24) as f32)
    }

    pub fn pick(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    /// A seed for one shape: the shader hashes it, so any value in a wide range will do.
    pub fn seed(&mut self) -> f32 {
        self.range(1.0, 997.0).floor() + self.range(0.0, 1.0)
    }
}

/// What a shape is (the shader's family numbers).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// Ribbons of light flowing across a dithered gradient (it fills its box): a wave card.
    Ribbon = 17,
    /// Stage Home My Wave: 3D-textured fluid wave backdrop.
    Liquid = 19,
    /// A rounder, calmer pebble of the same liquid.
    Pebble = 20,
    // The pictures of the moods, in the same printed liquid, each drawn a little differently for
    // every seed.
    /// A heart, its lobes uneven, beating slowly.
    Heart = 9,
    /// A lightning bolt that crackles now and then.
    Bolt = 10,
    /// A crescent moon.
    Moon = 11,
    /// A raindrop, its point up.
    Drop = 12,
    /// A compass star, four long points and four short.
    Star = 13,
    /// A crown, a jewel on each of its three points.
    Crown = 14,
    /// A sun with turning rays.
    Sun = 15,
}

impl Family {
    /// How far the shape reaches out from its middle along either axis, in its own units, at the
    /// worst its seed and sway can make it (as `shape_ext` in the shader). Measured, not guessed:
    /// a heart's lobes reach well past its radius, and a reach set short cut its ring off.
    fn reach(self) -> f32 {
        match self {
            Family::Heart => 1.23,
            Family::Bolt => 1.17,
            Family::Crown => 1.15,
            Family::Moon => 0.95,
            Family::Drop => 1.02,
            Family::Star => 0.88,
            Family::Sun => 0.98,
            Family::Pebble => 1.06,
            _ => 1.045,
        }
    }
}

/// The picture of a mood (of My Wave's moods, by their id): a heart for what you love, a star
/// for something new, a crown for what everyone plays, a bolt of energy, a calm moon, a sun for
/// fun and a drop for sadness.
pub fn mood_family(id: &str) -> Family {
    match id {
        "favorite" => Family::Heart,
        "discover" => Family::Star,
        "popular" => Family::Crown,
        "energetic" => Family::Bolt,
        "calm" => Family::Moon,
        "fun" => Family::Sun,
        "sad" => Family::Drop,
        _ => Family::Pebble,
    }
}

/// Points kept clear between a shape's furthest reach and its box: the ring of the chosen one
/// stands 4.5 out and is 1.3 thick either side, and the slot cuts off at the box (`shape_r`).
const SHAPE_MARGIN: f32 = 8.5;

/// The radius (points) `fs_shape` draws a shape of a `side` box about, so a shape's place before
/// the compositor is up can be the same circle.
pub fn radius(family: Family, side: f32) -> f32 {
    match family {
        Family::Liquid => side * 0.40,
        Family::Ribbon => side * 0.5 / 1.045,
        // As `shape_r` in the shader: a part of the box, with room for the ring.
        f => {
            let base = match f {
                Family::Pebble => 0.43,
                Family::Star => 0.48,
                _ => 0.42,
            } * side;
            base.min((side * 0.5 - SHAPE_MARGIN) / (f.reach() * 1.04))
        }
    }
}

/// How a shape stands: chosen (it wears a ring), under the pointer (it lifts), and its place in
/// the order the garden comes up in.
#[derive(Clone, Copy, Debug, Default)]
pub struct Look {
    pub chosen: bool,
    pub hovered: bool,
    pub order: u32,
}

impl Look {
    fn flags(self) -> f32 {
        (self.chosen as u32 + 2 * self.hovered as u32 + 4 * self.order.min(60)) as f32
    }
}

/// How the shape is filled: a hue of its own, or the playing cover (that hue if nothing plays).
#[derive(Clone, Copy, Debug)]
pub enum Fill {
    Hue(f32),
    Cover(f32),
    /// The playing cover, whole at once rather than washing over the one before (the first
    /// cover after its own colour).
    CoverNow(f32),
}

impl Fill {
    fn data(self) -> f32 {
        match self {
            Fill::Hue(h) => h.rem_euclid(360.0),
            Fill::Cover(h) => -(h.rem_euclid(360.0) + 1.0),
            Fill::CoverNow(h) => -(h.rem_euclid(360.0) + 501.0),
        }
    }
}

fn hsl_to_rgb(h_deg: f32, s: f32, l: f32) -> [f32; 3] {
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

fn rgb_color(c: [f32; 3], a: f32) -> rune::Color {
    th::srgb(c).alpha(a)
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct CpuShapeKey {
    family: u32,
    form: u8,
    seed: u32,
    hue_q: u32,
    chosen: bool,
    hovered: bool,
    light: bool,
    px_side: u32,
}

type CpuAuraKey = (u32, u8, u32, bool, bool);

thread_local! {
    static CPU_SHAPE_CACHE: RefCell<HashMap<CpuShapeKey, Pixels>> = RefCell::new(HashMap::new());
    static CPU_AURA_CACHE: RefCell<HashMap<CpuAuraKey, Pixels>> = RefCell::new(HashMap::new());
}

/// Drops cached CPU-rendered Home shapes and wave textures when the main window hides.
pub fn clear_cache() {
    CPU_SHAPE_CACHE.with_borrow_mut(HashMap::clear);
    CPU_AURA_CACHE.with_borrow_mut(HashMap::clear);
}

/// Pre-rendered CPU fallback for the soft ragged layered fluid morph behind Stage Home when GPU
/// shaders are off or in CPU mode. Compact (220x220 = ~193 KB) and cached.
fn cpu_wave_aura_pixels(seed: f32, form: u8, hue: f32, hovered: bool) -> Pixels {
    let light = th::light();
    let hue_q = (hue.rem_euclid(360.0) * 2.0).round() as u32;
    let key = (seed.to_bits(), form, hue_q, hovered, light);
    if let Some(p) = CPU_AURA_CACHE.with_borrow(|c| c.get(&key).cloned()) {
        return p;
    }
    const W: usize = 220;
    const H: usize = 220;
    let mut rgba = vec![0u8; W * H * 4];
    let base = hsl_to_rgb(hue, if light { 0.72 } else { 0.78 }, if light { 0.52 } else { 0.48 });
    let cool = hsl_to_rgb(hue - 20.0, if light { 0.68 } else { 0.74 }, if light { 0.54 } else { 0.44 });
    let warm = hsl_to_rgb(hue + 22.0, if light { 0.76 } else { 0.82 }, if light { 0.56 } else { 0.54 });
    let page_bg = if light { [0.969, 0.969, 0.961] } else { [0.039, 0.039, 0.039] };

    let smooth = |e0: f32, e1: f32, x: f32| {
        let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let wave_n = |x: f32, y: f32, ph: f32| {
        0.52 * (x * 2.7 + ph).sin() * (y * 2.4 - ph * 0.7).cos()
            + 0.30 * (x * 5.1 - y * 3.8 + ph * 1.4).sin()
            + 0.18 * (x * 4.2 + y * 5.9 - ph * 1.1).cos()
    };

    for y in 0..H {
        let fy = (y as f32 + 0.5) / H as f32;
        let uy = fy - 0.46;
        for x in 0..W {
            let fx = (x as f32 + 0.5) / W as f32;
            let ux = fx - 0.50;
            let raw_dist = ux.hypot(uy);
            let wx = wave_n(ux * 2.2, uy * 2.2, seed * 0.31);
            let wy = wave_n(ux * 2.2 + 2.4, uy * 2.2 - 1.7, seed * 0.43 + 1.2);
            let left_fade = smooth(0.0, 0.09, fx) * smooth(0.02, 0.34, fx + 0.10 * wx + 0.05 * wy);
            if left_fade <= 0.002 {
                continue;
            }
            let pa_x = ux + wx * 0.13;
            let pa_y = uy + wy * 0.13;
            let n1 = wave_n(pa_x * 2.3, pa_y * 2.3, seed * 0.19);
            let n2 = wave_n((ux - wy * 0.12) * 3.1, (uy + wx * 0.12) * 3.1, seed * 0.57 + 2.3);
            let n3 = wave_n((ux + wx * 0.09) * 3.9, (uy - wy * 0.10) * 3.9, seed * 0.73 - 1.1);

            let env1 = (-(ux * ux * 1.9 + uy * uy * 2.3)).exp();
            let d1 = env1 * 1.10 + n1 * 0.52 - 0.34;
            let veil1 = smooth(0.0, 0.46, d1);
            let env2 = (-(ux * ux * 2.5 + uy * uy * 2.3)).exp();
            let d2 = env2 * 1.02 + n2 * 0.56 - 0.33;
            let veil2 = smooth(0.0, 0.40, d2);
            let under2 = smooth(0.0, 0.24, -d2) * (-(-d2).max(0.0) * 5.0).exp() * veil1;
            let tear2 = smooth(-0.04, 0.48, n1 + 0.20 * wy);
            let crest2 = smooth(-0.04, 0.14, d2) * (-d2.max(0.0) * 4.4).exp() * tear2 * (-raw_dist * 1.5).exp();
            let env3 = (-(ux * ux * 3.1 + uy * uy * 2.9)).exp();
            let d3 = env3 * 0.94 + n3 * 0.58 - 0.35;
            let veil3 = smooth(0.0, 0.36, d3);
            let under3 = smooth(0.0, 0.20, -d3) * (-(-d3).max(0.0) * 5.6).exp() * veil2;
            let wisp3 = smooth(-0.03, 0.12, d3) * (-d3.max(0.0) * 5.0).exp() * smooth(0.02, 0.48, n2) * (-raw_dist * 1.8).exp();

            let ang = uy.atan2(ux) + 0.24 * wy;
            let beam = (-(ang * 3.0 + seed * 0.3).sin().abs() * 5.2).exp() + (-(ang * 5.0 - seed * 0.2).sin().abs() * 6.8).exp() * 0.55;
            let rays = (beam * smooth(0.10, 0.42, raw_dist) * smooth(0.90, 0.22, raw_dist) * (0.24 + 0.76 * veil1) * if hovered { 0.38 } else { 0.30 }).clamp(0.0, 0.44);

            let halo = (-(ux * ux * 1.2 + uy * uy * 1.5)).exp();
            let o = (y * W + x) * 4;
            for c in 0..3 {
                let mut v = if light {
                    let mist = page_bg[c] + (cool[c] * 0.5 + base[c] * 0.5 - page_bg[c]) * 0.22;
                    let l1 = cool[c] * 0.58 + page_bg[c] * 0.42;
                    let l2 = base[c] * 0.76 + 0.24;
                    let l3 = warm[c] * 0.44 + 0.56;
                    let mut col = page_bg[c] + (mist - page_bg[c]) * (halo * 0.62);
                    col += (l1 - col) * (veil1 * 0.54);
                    col += (cool[c] * 0.42 + page_bg[c] * 0.45 - col) * (under2 * 0.32);
                    col += (l2 - col) * (veil2 * 0.50 + rays * 0.32).clamp(0.0, 0.70);
                    col += (base[c] * 0.44 + page_bg[c] * 0.42 - col) * (under3 * 0.26);
                    col += (l3 - col) * (veil3 * 0.44 + crest2 * 0.50 + wisp3 * 0.54).clamp(0.0, 0.80);
                    col
                } else {
                    let mist = (cool[c] * 0.5 + base[c] * 0.5) * 0.28;
                    let l1 = cool[c] * 0.36 + base[c] * 0.38;
                    let l2 = (base[c] * 1.06).min(1.0) * 0.64 + warm[c] * 0.34;
                    let l3 = (warm[c] * 1.24).min(1.0) * 0.62 + 0.38;
                    let mut col = page_bg[c] + (mist - page_bg[c]) * (halo * 0.72);
                    col += (l1 - col) * (veil1 * 0.72);
                    col *= 1.0 - under2 * 0.36;
                    col += (l2 - col) * (veil2 * 0.60 + rays * 0.40).clamp(0.0, 0.78);
                    col *= 1.0 - under3 * 0.30;
                    col += (l3 - col) * (veil3 * 0.50 + crest2 * 0.56 + wisp3 * 0.60).clamp(0.0, 0.78);
                    col
                };
                let cradle = (-(ux * ux * 4.8 + uy * uy * 6.0)).exp() * 0.20;
                if light {
                    v += (page_bg[c] - v) * cradle;
                } else {
                    v *= 1.0 - cradle;
                }
                rgba[o + c] = (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            }
            let alpha = (left_fade * (halo * 0.70 + veil1 * 0.90 + veil2 * 0.85 + rays)).clamp(0.0, 1.0);
            rgba[o + 3] = (alpha * 255.0 + 0.5) as u8;
        }
    }
    let pixels = Pixels::new();
    pixels.set(W as u32, H as u32, rgba);
    CPU_AURA_CACHE.with_borrow_mut(|c| {
        if c.len() >= 8 {
            c.clear();
        }
        c.insert(key, pixels.clone());
    });
    pixels
}

/// Pre-renders a Home garden shape into a cached CPU `Pixels` texture when GPU shaders are off
/// (or in CPU mode), reusing the exact SDFs from `ui::outline`.
fn cpu_shape_pixels(family: Family, form: u8, seed: f32, hue: f32, look: Look, side: f32) -> Pixels {
    let light = th::light();
    let px_side = (side.round() as u32).clamp(48, 220);
    let hue_q = (hue.rem_euclid(360.0) * 2.0).round() as u32;
    let key = CpuShapeKey { family: family as u32, form, seed: seed.to_bits(), hue_q, chosen: look.chosen, hovered: look.hovered, light, px_side };
    if let Some(p) = CPU_SHAPE_CACHE.with_borrow(|c| c.get(&key).cloned()) {
        return p;
    }
    let (chosen, hovered) = (look.chosen, look.hovered);
    let n = px_side as usize;
    let mut rgba = vec![0u8; n * n * 4];
    let r = radius(family, px_side as f32).max(1.0);
    let inv_r = 1.0 / r;
    let half = px_side as f32 * 0.5;
    let picture = matches!(family, Family::Heart | Family::Bolt | Family::Moon | Family::Drop | Family::Star | Family::Crown | Family::Sun);
    let turn = if picture { 0.035 * seed.sin() } else { 0.0 };
    let (ct, st) = (turn.cos(), turn.sin());
    let (sat, l_lo, l_hi) = if light {
        (0.56, if hovered { 0.54 } else { 0.50 }, if hovered { 0.66 } else { 0.62 })
    } else {
        (0.52, if hovered { 0.30 } else { 0.26 }, if hovered { 0.44 } else { 0.39 })
    };
    let c0 = hsl_to_rgb(hue - 5.0, sat, l_lo);
    let c1 = hsl_to_rgb(hue + 6.0, (sat * 1.06).min(0.75), l_hi);
    let rim_col = if light { [1.0, 1.0, 1.0] } else { [0.96, 0.97, 1.0] };
    let ring_col = if light { [0.08, 0.08, 0.10] } else { [0.95, 0.95, 0.97] };
    let ring_dist = 4.5 * inv_r;
    let ring_half = 1.3 * inv_r;
    let rim_width = (2.2 * inv_r).max(0.025);

    for y in 0..n {
        let dy = (y as f32 + 0.5 - half) * inv_r;
        for x in 0..n {
            let dx = (x as f32 + 0.5 - half) * inv_r;
            let (qx, qy) = (ct * dx + st * dy, -st * dx + ct * dy);
            let d = super::outline::eval_field(family, form, qx, qy, seed);
            let aa = (1.25 * inv_r).max(0.012);
            let fill_a = ((aa - d) / (2.0 * aa)).clamp(0.0, 1.0);
            let ring_a = if chosen {
                let rd = (d - ring_dist).abs() - ring_half;
                ((aa - rd) / (2.0 * aa)).clamp(0.0, 1.0) * 0.85
            } else {
                0.0
            };
            let a = fill_a.max(ring_a);
            if a <= 0.002 {
                continue;
            }
            // Soft radial dome shading from upper-left + crisp inner rim highlight + fine grain to prevent banding.
            let dome = (1.0 - ((dx + 0.18).powi(2) + (dy + 0.22).powi(2)).sqrt() * 0.62).clamp(0.0, 1.0);
            let dome = dome * dome * (3.0 - 2.0 * dome);
            let rim = if d < 0.0 { ((d + rim_width) / rim_width).clamp(0.0, 1.0) } else { 0.0 };
            let rim = rim * rim * if light { 0.22 } else { 0.16 };
            let h = ((x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263)).wrapping_mul(1_274_126_177);
            let grain = ((h >> 24) as f32 / 255.0 - 0.5) * 0.018;
            let mut cr = c0[0] + (c1[0] - c0[0]) * dome + (rim_col[0] - c1[0]) * rim + grain;
            let mut cg = c0[1] + (c1[1] - c0[1]) * dome + (rim_col[1] - c1[1]) * rim + grain;
            let mut cb = c0[2] + (c1[2] - c0[2]) * dome + (rim_col[2] - c1[2]) * rim + grain;
            if ring_a > fill_a {
                let t = ring_a / a.max(1e-4);
                cr = cr + (ring_col[0] - cr) * t;
                cg = cg + (ring_col[1] - cg) * t;
                cb = cb + (ring_col[2] - cb) * t;
            }
            let o = (y * n + x) * 4;
            rgba[o] = (cr.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            rgba[o + 1] = (cg.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            rgba[o + 2] = (cb.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            rgba[o + 3] = (a.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        }
    }
    let pixels = Pixels::new();
    pixels.set(px_side, px_side, rgba);
    CPU_SHAPE_CACHE.with_borrow_mut(|c| {
        if c.len() >= 20 {
            c.clear();
        }
        c.insert(key, pixels.clone());
    });
    pixels
}

fn fill_hue(fill: Fill) -> f32 {
    match fill {
        Fill::Hue(h) | Fill::Cover(h) | Fill::CoverNow(h) => h,
    }
}

fn shape_slot(base: El, data: [f32; 4]) -> El {
    base.effect(fx::SHAPE).effect_data(data)
}

/// Subtle translucent wash tinted by the playing cover's hue (`Palette::wave`), or `None` for
/// monochrome covers so neutral backdrops stay clean grey/charcoal.
pub fn cover_tint([hue, sat, _]: [f32; 3], is_light: bool, alpha: f32) -> Option<rune::Color> {
    let sat = sat.clamp(0.0, 1.0);
    if sat <= 0.08 || alpha <= 0.001 {
        return None;
    }
    Some(rgb_color(hsl_to_rgb(hue, (sat * 0.45).min(0.45), if is_light { 0.44 } else { 0.56 }), alpha))
}

/// A wave card's ground filling its parent: ribbons of light in `hue` and its neighbours flowing
/// across a dithered gradient, quicker under the pointer (`hovered`) and swelling with the music
/// while its wave plays (`on`).
/// `sat` and `light` (0..1 each) travel in the hue's slot: `hue + 360 * (s + 16 * l)`, each in
/// sixteen steps.
pub fn ribbon(key: impl Into<String>, seed: f32, hsl: [f32; 3], hovered: bool, on: bool, radius: f32) -> El {
    ribbon_alpha(key, seed, hsl, hovered, on, radius, 1.0)
}

/// Same as [`ribbon`], multiplied by `alpha` (`0..1`) directly in the shader / fallback fills so
/// fading a wave card never allocates an isolated offscreen compositing layer.
pub fn ribbon_alpha(key: impl Into<String>, seed: f32, [hue, sat, light]: [f32; 3], hovered: bool, on: bool, radius: f32, alpha: f32) -> El {
    let a = alpha.clamp(0.0, 1.0);
    let base = el().key(key).absolute(0.0, 0.0).w_full().h_full().radius(radius).transition(Transition::none());
    if a <= 0.002 {
        return base;
    }
    if !fx::ribbon_ready() {
        let is_light = th::light();
        let glass = th::veil(match (is_light, on || hovered) {
            (true, true) => 0.075,
            (true, false) => 0.045,
            (false, true) => 0.10,
            (false, false) => 0.065,
        } * a);
        let border = th::veil(if on {
            0.24
        } else if hovered {
            0.17
        } else {
            0.11
        } * a);
        let surface = base.fill(glass).border(1.0, border);
        let tint_a = match (is_light, on || hovered) {
            (true, true) => 0.10,
            (true, false) => 0.055,
            (false, true) => 0.12,
            (false, false) => 0.07,
        } * a;
        return match cover_tint([hue, sat, light], is_light, tint_a) {
            Some(tint) => surface.child(el().absolute(0.0, 0.0).w_full().h_full().radius(radius).fill(tint)),
            None => surface,
        };
    }
    let hide_steps = ((1.0 - a) * 60.0).round() as u32;
    let look = Look { chosen: on, hovered, order: hide_steps };
    let (sq, lq) = ((sat.clamp(0.0, 1.0) * 15.0).round(), (light.clamp(0.0, 1.0) * 15.0).round());
    let packed = hue.rem_euclid(360.0).min(359.9) + 360.0 * (sq + 16.0 * lq);
    shape_slot(base, [Family::Ribbon as u32 as f32, seed, packed, look.flags()])
}

/// Stage Home's full-bleed 3D-texture fluid backdrop (`Family::Liquid`, rendered by
/// `fs_wave_stage` on GPU and cached aura texture on CPU / shaders-off).
#[expect(clippy::too_many_arguments, reason = "explicit stage geometry and visual parameters")]
pub fn wave_aura(key: impl Into<String>, seed: f32, fill: Fill, look: Look, wave: crate::store::WaveLook, w: f32, h: f32, radius: f32) -> El {
    let mut base = el().key(key).w(w).h(h).transition(Transition::none());
    if radius > 0.0 {
        base = base.radius(radius);
    }
    if !fx::wave_ready() {
        let px = cpu_wave_aura_pixels(seed, wave.form.min(7), fill_hue(fill), look.hovered);
        if radius > 0.0 {
            base = base.clip_rounded(radius);
        }
        return base.pixels_fit(&px, rune::SurfaceFit::Fill);
    }
    let code = f32::from(wave.form.min(7)) + 8.0 * f32::from(wave.print.min(7));
    shape_slot(base, [Family::Liquid as u32 as f32 + (code + 0.5) / 64.0, seed, fill.data(), look.flags()])
}

/// One shape filling a `w` by `h` box.
pub fn shape(key: impl Into<String>, family: Family, seed: f32, fill: Fill, look: Look, w: f32, h: f32) -> El {
    let base = el().key(key).w(w).h(h).transition(Transition::none());
    if !fx::mood_ready() {
        let side = w.min(h);
        let px = cpu_shape_pixels(family, 0, seed, fill_hue(fill), look, side);
        return base.pixels_fit(&px, rune::SurfaceFit::Fill);
    }
    shape_slot(base, [family as u32 as f32, seed, fill.data(), look.flags()])
}
