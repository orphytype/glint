//! The shapes of Home. Each is drawn on the GPU (`fx::SHAPE`, `fs_shape`) from a family and a
//! seed: My Wave a body of glowing liquid, each mood a picture of what it is (a heart, a moon, a
//! crown...) printed in the same liquid in its own colour,
//! a wave card's ribbons, an artist's picture. The seed sets how a shape's edge swells and how its
//! colours flow, so the same seed draws the same garden; a new start of the app, or the button
//! that rolls them, draws another.

use rune::{El, Transition, el};

use crate::fx;

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
    /// My Wave: glowing liquid whose edge swells with the music, with a glow just outside.
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
    /// The room for words on a shape, in its own units (its radius is 1, y down): centre x,
    /// centre y, width, height. Each is centred where the shape's weight is (worked out from its
    /// area, then eased toward the middle of its outline, which is where the eye puts it): across a
    /// heart's lobes, in a crescent's body, in a drop's round part, in a crown's band.
    pub fn room(self) -> [f32; 4] {
        match self {
            Family::Heart => [0.0, -0.16, 1.4, 0.62],
            Family::Bolt => [0.12, -0.12, 1.0, 0.36],
            Family::Moon => [-0.28, 0.18, 1.0, 0.44],
            Family::Drop => [0.0, 0.22, 1.0, 0.52],
            Family::Star => [0.0, 0.0, 1.2, 0.56],
            Family::Crown => [0.0, 0.26, 1.5, 0.5],
            Family::Sun => [0.0, 0.0, 1.2, 0.76],
            _ => [0.0, 0.0, 1.42, 0.9],
        }
    }

    /// How much the words on the shape are turned, clockwise in radians. A crescent's name leans
    /// down to the right, a little, as its lower curve does under it.
    pub fn tilt(self) -> f32 {
        match self {
            Family::Moon => 0.36,
            _ => 0.0,
        }
    }

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
/// for something new, a crown for what everyone plays, a bolt of energy, a calm moon, a sun for fun and a
/// drop for sadness.
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

fn shape_slot(base: El, data: [f32; 4]) -> El {
    if fx::shapes_ready() { base.effect(fx::SHAPE).effect_data(data) } else { base }
}

/// A wave card's ground filling its parent: ribbons of light in `hue` and its neighbours flowing
/// across a dithered gradient, quicker under the pointer (`hovered`) and swelling with the music
/// while its wave plays (`on`).
/// `sat` and `light` (0..1 each) travel in the hue's slot: `hue + 360 * (s + 16 * l)`, each in
/// sixteen steps.
pub fn ribbon(key: impl Into<String>, seed: f32, [hue, sat, light]: [f32; 3], hovered: bool, on: bool, radius: f32) -> El {
    let look = Look { chosen: on, hovered, order: 0 };
    let (sq, lq) = ((sat.clamp(0.0, 1.0) * 15.0).round(), (light.clamp(0.0, 1.0) * 15.0).round());
    let packed = hue.rem_euclid(360.0).min(359.9) + 360.0 * (sq + 16.0 * lq);
    shape_slot(el().key(key).absolute(0.0, 0.0).w_full().h_full().radius(radius).transition(Transition::none()), [Family::Ribbon as u32 as f32, seed, packed, look.flags()])
}

/// My Wave's shape (`Family::Liquid`) filling a `side` box, in the form and print the user picked
/// (`WaveLook`): they ride in the family number's fraction, as `(form + 8 * print + 0.5) / 64`.
pub fn wave(key: impl Into<String>, seed: f32, fill: Fill, look: Look, wave: crate::store::WaveLook, side: f32) -> El {
    let code = f32::from(wave.form.min(7)) + 8.0 * f32::from(wave.print.min(7));
    shape_slot(el().key(key).w(side).h(side).transition(Transition::none()), [Family::Liquid as u32 as f32 + (code + 0.5) / 64.0, seed, fill.data(), look.flags()])
}

/// One shape filling a `w` by `h` box.
pub fn shape(key: impl Into<String>, family: Family, seed: f32, fill: Fill, look: Look, w: f32, h: f32) -> El {
    shape_slot(el().key(key).w(w).h(h).transition(Transition::none()), [family as u32 as f32, seed, fill.data(), look.flags()])
}
