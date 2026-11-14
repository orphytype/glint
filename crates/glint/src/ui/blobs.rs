//! The shapes of Home. Each is drawn on the GPU (`fx::SHAPE`, `fs_shape`) from a family and a
//! seed: superformula stars and gems, blobs made of a few waves, flowers, butterflies, drops that
//! merge and part, squircles, rings and arches, pinwheels. The seed picks the shape's own numbers
//! and its fill (an ordered dither in three tones, halftone dots, hatching or op-art rings), and
//! the shapes morph slowly on their own. The same seed draws the same garden; a new start of the
//! app, or the button that rolls them, draws another.

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
    /// Stars, gems and soft polygons (a superformula).
    Super = 0,
    /// A blob of a few slow waves.
    Waves = 1,
    Flower = 2,
    /// A butterfly at rest, opening and closing its wings now and then.
    Butterfly = 3,
    /// Drops that drift, merge and part.
    Drops = 4,
    /// A squircle, a ring or an arch.
    #[allow(dead_code)]
    Squircle = 5,
    Pinwheel = 6,
    // Pictures of a mood, each drawn a little differently for every seed.
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
    /// A flame with licking sides.
    Flame = 14,
    /// A sun with turning rays.
    Sun = 15,
    /// A rounded tile of a noisy, dithered gradient (it fills its box).
    #[allow(dead_code)]
    Tile = 16,
    /// Ribbons of light flowing across a dithered gradient (it fills its box): a wave card.
    Ribbon = 17,
    /// An artist's picture: nearly a circle, its edge breathing a very little (`avatar`).
    Avatar = 18,
}

impl Family {
    /// The room for words inside a picture family, in its own units (its radius is 1, y down,
    /// before its lean): [centre x, centre y, width, height, turn (radians)]. A heart's room is
    /// across its lobes, a drop's and a flame's low in the round part, a crescent's along its
    /// thick part, a star's across its middle and its short side points. A tall family has a
    /// second room up along its height (turned a quarter, read upward) for a long single word.
    pub fn rooms(self) -> &'static [[f32; 5]] {
        const UP: f32 = -std::f32::consts::FRAC_PI_2;
        match self {
            Family::Heart => &[[0.0, -0.16, 1.32, 0.62, 0.0]],
            Family::Bolt => &[[0.07, -0.01, 1.0, 0.36, -0.12]],
            Family::Moon => &[[-0.40, 0.34, 1.0, 0.46, 0.80]],
            Family::Drop => &[[0.0, 0.40, 0.80, 0.52, 0.0]],
            Family::Star => &[[0.0, 0.0, 1.12, 0.52, 0.0]],
            Family::Flame => &[[0.0, 0.36, 0.62, 0.44, 0.0], [0.0, 0.10, 1.16, 0.46, UP]],
            Family::Sun => &[[0.0, 0.0, 1.08, 0.72, 0.0]],
            _ => &[[0.0, 0.0, 1.2, 0.7, 0.0]],
        }
    }
}

/// Every family a mood can be (a flying butterfly cannot be pressed).
#[allow(dead_code)]
pub const FAMILIES: [Family; 7] = [Family::Super, Family::Waves, Family::Flower, Family::Butterfly, Family::Drops, Family::Squircle, Family::Pinwheel];

/// How far a picture family (9 and up) leans for `seed`, radians; `fs_shape` leans it the same.
pub fn lean(seed: f32) -> f32 {
    ((seed * 0.618_034).fract() - 0.5) * 0.45
}

/// A picture family is drawn this much inside its box, so the ring of the chosen one fits.
pub const PICTURE_FIT: f32 = 0.86;

/// The radius (points) `fs_shape` draws a shape of a `side` box at.
pub fn radius(family: Family, side: f32) -> f32 {
    let r = (side * 0.5 - (7.0f32).min(side * 0.07)) / 1.07;
    if family as u32 >= 9 { r * PICTURE_FIT } else { r }
}


/// The families that can carry words across their middle (no hole, no body in the way).
pub const SOLID: [Family; 5] = [Family::Super, Family::Waves, Family::Flower, Family::Drops, Family::Pinwheel];

/// `n` families in a random order, each before any comes twice, so a row of shapes is as varied
/// as it can be.
#[cfg(test)]
pub fn deal(rng: &mut Rng, n: usize) -> Vec<Family> {
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        let mut deck = FAMILIES.to_vec();
        // Fisher-Yates.
        for i in (1..deck.len()).rev() {
            deck.swap(i, rng.pick(i + 1));
        }
        // No family twice in a row across two decks.
        if out.last() == deck.first() {
            deck.rotate_left(1);
        }
        out.extend(deck);
    }
    out.truncate(n);
    out
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
    /// The picture `State::pic_key` names, as it is (that hue until it is in).
    Picture(f32),
}

impl Fill {
    fn data(self) -> f32 {
        match self {
            Fill::Hue(h) => h.rem_euclid(360.0),
            Fill::Cover(h) => -(h.rem_euclid(360.0) + 1.0),
            Fill::Picture(h) => -(h.rem_euclid(360.0) + 1000.0),
        }
    }
}


/// A wave card's ground filling its parent: ribbons of light in `hue` and its neighbours flowing
/// across a dithered gradient, quicker under the pointer (`hovered`) and swelling with the music
/// while its wave plays (`on`).
/// `sat` and `light` (0..1 each) travel in the hue's slot: `hue + 360 * (s + 16 * l)`, each in
/// sixteen steps.
pub fn ribbon(key: impl Into<String>, seed: f32, hue: f32, sat: f32, light: f32, hovered: bool, on: bool) -> El {
    let look = Look { chosen: on, hovered, order: 0 };
    let (sq, lq) = ((sat.clamp(0.0, 1.0) * 15.0).round(), (light.clamp(0.0, 1.0) * 15.0).round());
    let packed = hue.rem_euclid(360.0).min(359.9) + 360.0 * (sq + 16.0 * lq);
    el().key(key).absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).effect(fx::SHAPE).effect_data([Family::Ribbon as u32 as f32, seed, packed, look.flags()])
}

/// The seed and the stand-in hue of the artist's picture on the page keyed `page_key` (the same on
/// the page and in the photo viewer, so the two are one shape to the compositor).
pub fn avatar_seed(page_key: &str) -> (f32, f32) {
    let h = page_key.bytes().fold(2166136261u32, |a, b| (a ^ u32::from(b)).wrapping_mul(16777619));
    (1.0 + (h % 997) as f32 + 0.37, (h % 360) as f32)
}

/// An artist's picture (`Fill::Picture`) in their shape, `side` points across: nearly a circle,
/// and a rounded square as `square` goes to 1 (it rides in the family number's fraction).
pub fn avatar(key: impl Into<String>, seed: f32, fill: Fill, look: Look, square: f32, side: f32) -> El {
    el().key(key)
        .w(side)
        .h(side)
        .transition(Transition::none())
        .effect(fx::SHAPE)
        .effect_data([Family::Avatar as u32 as f32 + square.clamp(0.0, 0.99), seed, fill.data(), look.flags()])
}

/// One shape filling a `w` by `h` box.
pub fn shape(key: impl Into<String>, family: Family, seed: f32, fill: Fill, look: Look, w: f32, h: f32) -> El {
    el().key(key)
        .w(w)
        .h(h)
        .transition(Transition::none())
        .effect(fx::SHAPE)
        .effect_data([family as u32 as f32, seed, fill.data(), look.flags()])
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_deal_shows_every_family_before_any_comes_twice() {
        for s in 0..40 {
            let d = deal(&mut Rng::new(s), 7);
            let mut seen = d.clone();
            seen.sort_by_key(|f| *f as u32);
            seen.dedup();
            assert_eq!(seen.len(), 7, "seed {s}: {d:?}");
            let long = deal(&mut Rng::new(s), 20);
            assert!(long.windows(2).all(|w| w[0] != w[1]), "seed {s}: {long:?}");
        }
    }

    #[test]
    fn a_seed_always_draws_the_same_garden() {
        let a: Vec<f32> = { let mut r = Rng::new(42); (0..5).map(|_| r.seed()).collect() };
        let b: Vec<f32> = { let mut r = Rng::new(42); (0..5).map(|_| r.seed()).collect() };
        let c: Vec<f32> = { let mut r = Rng::new(43); (0..5).map(|_| r.seed()).collect() };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn the_cover_fill_keeps_its_fallback_hue() {
        assert_eq!(Fill::Hue(400.0).data(), 40.0);
        let d = Fill::Cover(205.0).data();
        assert!(d < 0.0);
        assert_eq!(-d - 1.0, 205.0);
        let f = Look { chosen: true, hovered: true, order: 3 }.flags() as u32;
        assert_eq!((f & 1, f & 2, f >> 2), (1, 2, 3));
    }
}
