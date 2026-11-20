//! Glint's mark: a pebble from Home with a four-pointed glint cut out of it, high on the left (the
//! files are in `brand/`). Both outlines come from functions: the pebble is Home's (`lq_edge` at
//! rest, as `ui/outline.rs` has it) with the seed the mark was drawn from, and the glint is a
//! star whose sides draw in between its four points. They are placed on the 48-unit grid the
//! brand files use, so what is drawn here matches `brand/svg`. `scripts/icons.py` draws the app's
//! icon files from the same numbers.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, TAU};

/// The grid the mark is laid out on, and the room it leaves at each side.
pub const GRID: f32 = 48.0;
const EDGE: f32 = 2.0;
/// The seed of the pebble the mark was drawn from.
const SEED: f32 = 1.9;
/// The glint's middle on the grid, and how far its points reach from it.
const GLINT_AT: [f32; 2] = [19.0, 17.0];
const GLINT_REACH: f32 = 20.0 * (44.0 / 48.0) * 0.5;

/// The pebble's edge at angle `a`, about 1 (y down).
fn pebble_r(a: f32) -> f32 {
    let w = 0.045 * (3.0 * a + SEED).sin() + 0.030 * (5.0 * a + SEED * 1.7).sin() + 0.018 * (7.0 * a + SEED * 2.3).sin();
    1.0 + 0.45 * w
}

/// The glint's edge at angle `a`: 1 at its points (right, down, left, up), 0.30 between them.
fn glint_r(a: f32) -> f32 {
    let tri = (((a + FRAC_PI_4) / FRAC_PI_2).rem_euclid(1.0) * 2.0 - 1.0).abs();
    0.30 + 0.70 * (1.0 - tri).powf(2.4)
}

/// How the pebble sits on the grid: its middle in its own units, and the scale that makes its
/// longer side fill the grid less the edges.
fn fit() -> ([f32; 2], f32) {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for i in 0..1024 {
        let a = i as f32 / 1024.0 * TAU;
        let r = pebble_r(a);
        let (x, y) = (r * a.cos(), r * a.sin());
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
    }
    ([(x0 + x1) * 0.5, (y0 + y1) * 0.5], (GRID - 2.0 * EDGE) / (x1 - x0).max(y1 - y0))
}

/// The pebble's outline on the grid, `n` points.
pub fn pebble(n: usize) -> Vec<[f32; 2]> {
    let (c, k) = fit();
    (0..n)
        .map(|i| {
            let a = i as f32 / n as f32 * TAU;
            let r = pebble_r(a);
            [(r * a.cos() - c[0]) * k + GRID * 0.5, (r * a.sin() - c[1]) * k + GRID * 0.5]
        })
        .collect()
}

/// The glint's outline on the grid, `n` points (a multiple of 4 keeps its four points sharp).
pub fn glint(n: usize) -> Vec<[f32; 2]> {
    (0..n)
        .map(|i| {
            let a = i as f32 / n as f32 * TAU;
            let r = glint_r(a) * GLINT_REACH;
            [GLINT_AT[0] + r * a.cos(), GLINT_AT[1] + r * a.sin()]
        })
        .collect()
}
