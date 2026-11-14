//! The illustrations: small line drawings made in code (`rune::Shape`), so they are crisp at any
//! size and take the page's ink and accent. Each is a handful of ink strokes, a few faint ones, and
//! one shape in the accent. They trace themselves on when they appear (a `Transition::draw`), and
//! then the accent breathes, slowly, so an empty page still looks like something is alive on it.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use rune::{Color, El, LineCap, LoopSpec, Shape, Spring, SpringPair, Transition, canvas, el};

use crate::theme as th;

/// The points of an arc of the circle at `c`, radius `r`, from angle `a0` to `a1` (radians, 0 at the
/// right, turning clockwise on the screen).
fn arc(c: [f32; 2], r: f32, a0: f32, a1: f32) -> Vec<[f32; 2]> {
    let n = (((a1 - a0).abs() * r) / 2.5).ceil().max(6.0) as usize;
    (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / n as f32;
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}

fn circle(c: [f32; 2], r: f32) -> Vec<[f32; 2]> {
    let n = ((TAU * r) / 2.5).ceil().max(16.0) as usize;
    (0..n)
        .map(|i| {
            let a = TAU * i as f32 / n as f32;
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}

fn polar(c: [f32; 2], r: f32, a: f32) -> [f32; 2] {
    [c[0] + r * a.cos(), c[1] + r * a.sin()]
}

/// A rounded rectangle's outline.
fn rrect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Vec<[f32; 2]> {
    let r = r.min(w * 0.5).min(h * 0.5);
    let mut p = Vec::new();
    p.extend(arc([x + w - r, y + r], r, -FRAC_PI_2, 0.0));
    p.extend(arc([x + w - r, y + h - r], r, 0.0, FRAC_PI_2));
    p.extend(arc([x + r, y + h - r], r, FRAC_PI_2, PI));
    p.extend(arc([x + r, y + r], r, PI, PI + FRAC_PI_2));
    p
}

fn rotate(pts: Vec<[f32; 2]>, about: [f32; 2], a: f32) -> Vec<[f32; 2]> {
    let (s, c) = a.sin_cos();
    pts.into_iter()
        .map(|[x, y]| {
            let (dx, dy) = (x - about[0], y - about[1]);
            [about[0] + dx * c - dy * s, about[1] + dx * s + dy * c]
        })
        .collect()
}

const W: f32 = 2.0;
const THIN: f32 = 1.5;

/// The illustrations, one for each empty or waiting state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ill {
    /// A record on a turntable, the arm resting on it: the welcome.
    Record,
    /// A lens over a little contour map: searching, or nothing found.
    Lens,
    /// Two sleeves and a record rolling out of one: an empty shelf.
    Shelf,
    /// A microphone and a speech bubble: no lyrics.
    Mic,
    /// A cassette: the queue is empty, or the track has no words.
    Tape,
    /// A cloud with its cable pulled out: offline.
    Unplugged,
}

/// The box every illustration is laid out in.
const BOX: (f32, f32) = (160.0, 124.0);

impl Ill {
    fn shape(self, ink: Color, faint: Color, accent: Color, hole: Color) -> Shape {
        let s = Shape::new();
        match self {
            Ill::Record => {
                let c = [62.0, 66.0];
                let mut s = s.stroke_with(circle(c, 50.0), true, W, LineCap::Round, ink);
                for (i, r) in [43.0f32, 37.0, 31.0, 25.0].into_iter().enumerate() {
                    let a0 = -2.4 + 0.7 * i as f32;
                    s = s.stroke_with(arc(c, r, a0, a0 + 4.6), false, THIN, LineCap::Round, faint);
                }
                s = s.fill(circle(c, 15.0), accent).fill(circle(c, 2.6), hole);
                // The arm: its pivot, the tube, the head resting in the grooves.
                let pivot = [134.0, 22.0];
                s = s.stroke_with(circle(pivot, 8.0), true, W, LineCap::Round, ink).fill(circle(pivot, 2.4), ink);
                s = s.stroke_with(vec![pivot, [131.0, 70.0], [104.0, 94.0]], false, W, LineCap::Round, ink);
                s = s.line([104.0, 94.0], [96.0, 100.0], 6.0, LineCap::Round, ink);
                s = s.fill(circle([150.0, 110.0], 2.0), faint).fill(circle([14.0, 18.0], 1.6), faint).fill(circle([150.0, 52.0], 1.4), faint);
                s
            }
            Ill::Lens => {
                let c = [66.0, 56.0];
                let r = 36.0;
                let mut s = s;
                // Contour lines inside the lens: a small hill, seen from above.
                for (k, rr) in [8.0f32, 15.0, 22.0, 29.0].into_iter().enumerate() {
                    let centre = [c[0] - 6.0 + k as f32 * 1.5, c[1] - 4.0 + k as f32 * 1.2];
                    let pts: Vec<[f32; 2]> = (0..=64)
                        .map(|i| {
                            let a = TAU * i as f32 / 64.0;
                            let wob = 1.0 + 0.10 * (3.0 * a + k as f32).sin() + 0.05 * (5.0 * a).cos();
                            polar(centre, rr * wob, a)
                        })
                        .filter(|p| ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2)).sqrt() < r - 5.0)
                        .collect();
                    if pts.len() > 2 {
                        s = s.stroke_with(pts, false, THIN, LineCap::Round, faint);
                    }
                }
                s = s.fill(circle([c[0] - 6.0, c[1] - 4.0], 5.0), accent);
                s = s.stroke_with(circle(c, r), true, W, LineCap::Round, ink);
                s = s.line(polar(c, r + 1.0, 0.78), [126.0, 114.0], 7.0, LineCap::Round, ink);
                s = s.fill(circle([138.0, 22.0], 2.2), faint).fill(circle([20.0, 104.0], 1.6), faint).fill(circle([126.0, 46.0], 1.4), faint);
                s
            }
            Ill::Shelf => {
                let floor = 106.0;
                let mut s = s.line([6.0, floor], [154.0, floor], THIN, LineCap::Round, faint);
                // A sleeve leaning on the right.
                s = s.stroke_with(rotate(rrect(96.0, floor - 70.0, 54.0, 70.0, 3.0), [96.0, floor], 0.16), true, W, LineCap::Round, faint);
                // The record rolling out of the upright sleeve, then the sleeve over it.
                let rc = [74.0, floor - 36.0];
                s = s.stroke_with(arc(rc, 34.0, -FRAC_PI_2, FRAC_PI_2), false, W, LineCap::Round, ink);
                s = s.stroke_with(arc(rc, 26.0, -1.2, 1.2), false, THIN, LineCap::Round, faint);
                s = s.stroke_with(arc(rc, 19.0, -1.0, 1.0), false, THIN, LineCap::Round, faint);
                s = s.fill(circle(rc, 10.0), accent).fill(circle(rc, 2.2), hole);
                s = s.stroke_with(rrect(12.0, floor - 72.0, 62.0, 72.0, 3.0), true, W, LineCap::Round, ink);
                s = s.line([22.0, floor - 58.0], [48.0, floor - 58.0], THIN, LineCap::Round, faint);
                s = s.line([22.0, floor - 50.0], [38.0, floor - 50.0], THIN, LineCap::Round, faint);
                s.fill(circle([150.0, 14.0], 1.8), faint)
            }
            Ill::Mic => {
                let mut s = s;
                // Sound leaving it.
                let c = [58.0, 40.0];
                for r in [34.0f32, 44.0] {
                    s = s.stroke_with(arc(c, r, -0.55, 0.55), false, THIN, LineCap::Round, faint);
                    s = s.stroke_with(arc(c, r, PI - 0.55, PI + 0.55), false, THIN, LineCap::Round, faint);
                }
                s = s.stroke_with(rrect(44.0, 12.0, 28.0, 50.0, 14.0), true, W, LineCap::Round, ink);
                for y in [26.0f32, 33.0, 40.0] {
                    s = s.line([51.0, y], [65.0, y], THIN, LineCap::Round, faint);
                }
                s = s.stroke_with(arc([58.0, 44.0], 24.0, 0.0, PI), false, W, LineCap::Round, ink);
                s = s.line([58.0, 68.0], [58.0, 98.0], W, LineCap::Round, ink).line([40.0, 98.0], [76.0, 98.0], W, LineCap::Round, ink);
                // A speech bubble, with nothing in it but three dots.
                let b = [120.0, 40.0];
                let mut bubble = circle(b, 20.0);
                bubble.push([106.0, 62.0]);
                s = s.fill(rotate(bubble, b, 0.0), accent);
                for k in -1..=1 {
                    s = s.fill(circle([b[0] + 8.0 * k as f32, b[1]], 2.4), hole);
                }
                s.fill(circle([150.0, 100.0], 1.8), faint).fill(circle([16.0, 92.0], 1.6), faint)
            }
            Ill::Tape => {
                let mut s = s.fill(rrect(28.0, 26.0, 104.0, 10.0, 3.0), accent);
                s = s.stroke_with(rrect(14.0, 16.0, 132.0, 90.0, 9.0), true, W, LineCap::Round, ink);
                s = s.stroke_with(rrect(28.0, 26.0, 104.0, 48.0, 4.0), true, THIN, LineCap::Round, faint);
                s = s.stroke_with(rrect(50.0, 44.0, 60.0, 22.0, 11.0), true, W, LineCap::Round, ink);
                for c in [[61.0f32, 55.0], [99.0, 55.0]] {
                    s = s.stroke_with(circle(c, 7.0), true, W, LineCap::Round, ink);
                    for k in 0..3 {
                        let a = TAU * k as f32 / 3.0 + 0.4;
                        s = s.line(polar(c, 2.0, a), polar(c, 5.0, a), THIN, LineCap::Round, faint);
                    }
                }
                s = s.stroke_with(vec![[36.0, 106.0], [44.0, 90.0], [116.0, 90.0], [124.0, 106.0]], false, THIN, LineCap::Round, faint);
                s.fill(circle([46.0, 98.0], 2.2), faint).fill(circle([114.0, 98.0], 2.2), faint)
            }
            Ill::Unplugged => {
                let mut s = s;
                // The cloud: three bumps on a flat base.
                let mut cloud = arc([44.0, 58.0], 18.0, FRAC_PI_2, PI + 1.2);
                cloud.extend(arc([74.0, 44.0], 24.0, PI + 0.5, TAU - 0.2));
                cloud.extend(arc([108.0, 56.0], 19.0, -1.6, FRAC_PI_2));
                cloud.push([44.0, 76.0]);
                s = s.stroke_with(cloud, true, W, LineCap::Round, ink);
                // The cable from the cloud, ending in a socket; the plug hangs below, pulled out.
                s = s.stroke_with(vec![[76.0, 76.0], [76.0, 84.0]], false, W, LineCap::Round, ink);
                s = s.stroke_with(rrect(68.0, 84.0, 16.0, 8.0, 2.0), true, W, LineCap::Round, ink);
                s = s.stroke_with(rrect(68.0, 104.0, 16.0, 10.0, 2.0), true, W, LineCap::Round, ink);
                s = s.line([72.0, 104.0], [72.0, 98.0], W, LineCap::Round, ink).line([80.0, 104.0], [80.0, 98.0], W, LineCap::Round, ink);
                let mut tail = Vec::new();
                for i in 0..=20 {
                    let t = i as f32 / 20.0;
                    tail.push([76.0 + 44.0 * t, 114.0 + 8.0 * (t * PI).sin()]);
                }
                s = s.stroke_with(tail, false, W, LineCap::Round, ink);
                // The spark between them.
                let sp = [100.0, 96.0];
                let star: Vec<[f32; 2]> = (0..8).map(|k| polar(sp, if k % 2 == 0 { 9.0 } else { 3.0 }, k as f32 * PI / 4.0)).collect();
                s = s.fill(star, accent);
                s.fill(circle([140.0, 24.0], 1.8), faint).fill(circle([18.0, 30.0], 1.6), faint)
            }
        }
    }
}

/// An illustration as an element `size` points wide: it draws itself on after `delay` seconds, and
/// then its accent shape breathes, very slowly.
pub fn ill(m: Ill, size: f32, accent: Color, delay: f32) -> El {
    let k = size / BOX.0;
    let (w, h) = (BOX.0 * k, BOX.1 * k);
    let none = Color::TRANSPARENT;
    let draw = || Transition::draw().combined(Transition::opacity()).springs(SpringPair::new(Spring::new(1.1, 0.0), Spring::new(0.2, 0.0)));
    let place = |c: El| c.absolute((w - BOX.0) * 0.5, (h - BOX.1) * 0.5).w(BOX.0).h(BOX.1).scale(k);
    let lines = place(canvas(m.shape(th::text(), th::veil(0.30), none, none))).transition(draw()).enter_delay(delay);
    let colour = place(canvas(m.shape(none, none, accent, th::bg()))).transition(draw()).enter_delay(delay + 0.25).looping(LoopSpec::opacity(3.6, 0.0, 0.62, 1.0));
    el().w(w).h(h).child(colour).child(lines)
}
