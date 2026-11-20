//! The launch screen. Over a dark window the mark fades in, then moves left while the word comes
//! out from under it, ending on the lockup from `brand/svg/lockup-reversed.svg`. It stays until the
//! window has something steady to show (the effects compositor is up and Home is not still
//! loading), and never less than the time the motion takes. Then it fades out. Everything the app
//! does while it starts happens under it.

use rune::{El, Shape, Transition, el};

use crate::app::{Load, Session, State};
use crate::theme as th;
use crate::{fx, mark, wordmark};

/// The ink and the paper of `brand/brand.json`.
const INK: u32 = 0x121212;
const PAPER: u32 = 0xf4f2ee;

/// The height of a capital in the word, in points. The rest of the lockup is measured in it.
const CAP: f32 = 46.0;
/// The mark is one and a half capitals tall and sits 0.6 of a capital before the word, with its
/// middle half a capital above the baseline.
const MARK_H: f32 = 1.5;
const GAP: f32 = 0.6;
const BASELINE: f32 = 0.5;

/// Seconds from the launch: the mark fades in over `APPEAR`, the move runs from `MOVE_AT` for
/// `MOVE`, and the screen stays at least `HOLD`. Once the app is ready it waits `SETTLE` more, so
/// what came in under it has finished moving. It goes after `GIVE_UP` whatever happens, and fades
/// out over `FADE`.
const APPEAR: f64 = 0.18;
const MOVE_AT: f64 = 0.10;
const MOVE: f64 = 0.42;
const HOLD: f64 = 0.55;
const SETTLE: f64 = 0.25;
const GIVE_UP: f64 = 6.0;
const FADE: f64 = 0.28;

/// Where the launch screen is, in the app's clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct Launch {
    /// When it first drew.
    start: Option<f64>,
    /// When the app was first ready, from the start.
    ready: Option<f64>,
    /// When it started to fade out, from the start.
    leave: Option<f64>,
}

/// Whether the window can be seen. A start at login with "start in the tray" opens it hidden, and
/// the launch screen waits for it: run while nobody looks, it would be over by the time the tray
/// brings the window up.
fn window_shown(s: &State) -> bool {
    s.app.as_ref().is_none_or(|app| app.is_visible())
}

/// The launch screen is waiting for the window to be shown (the caller checks again soon).
pub fn waiting(s: &State) -> bool {
    s.launch.is_some_and(|l| l.start.is_none()) && !window_shown(s)
}

/// Moves the launch screen on. Returns whether it is still on screen, so the next frame comes.
pub fn step(s: &mut State) -> bool {
    let steady = fx::ready() && (s.session == Session::LoggedOut || !matches!(s.home, Load::Loading));
    let now = s.now;
    if waiting(s) {
        return false;
    }
    let Some(l) = s.launch.as_mut() else { return false };
    let t = now - *l.start.get_or_insert(now);
    if steady && l.ready.is_none() {
        l.ready = Some(t);
    }
    if l.leave.is_none() {
        let go = match l.ready {
            Some(r) => (r + SETTLE).max(HOLD),
            None => GIVE_UP,
        };
        if t >= go {
            l.leave = Some(t);
        }
    }
    if l.leave.is_some_and(|at| t >= at + FADE) {
        s.launch = None;
    }
    true
}

/// The launch screen over the whole window, while there is one.
pub fn view(s: &State) -> Option<El> {
    let l = s.launch?;
    let t = l.start.map_or(0.0, |start| s.now - start);
    let reduce = s.settings.reduce_motion;
    let appear = ease_out_cubic(t / APPEAR);
    let moved = if reduce { 1.0 } else { ease_out_back((t - MOVE_AT) / MOVE) };
    let gone = l.leave.map_or(0.0, |at| ease_in_out((t - at) / FADE));

    // The lockup in capitals, its middle at 0: the mark's middle ends at `mark_x`, the word's
    // origin at `word_x`. The word starts with its right edge at the mark's middle.
    let pebble = mark::pebble(192);
    let (top, bottom) = pebble.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p[1]), b.max(p[1])));
    let (left, right) = pebble.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p[0]), b.max(p[0])));
    let k = MARK_H / (bottom - top);
    let mark_w = (right - left) * k;
    let width = mark_w + GAP + wordmark::BOX[2];
    let mark_end = -width * 0.5 + mark_w * 0.5;
    let word_end = mark_end + mark_w * 0.5 + GAP;
    let mark_x = mark_end * moved;
    let word_x = -wordmark::BOX[2] + (word_end + wordmark::BOX[2]) * moved;

    // The canvas holds every place the lockup passes through, with room for the overshoot.
    let (cw, ch) = ((width + 1.0) * CAP, 2.6 * CAP);
    let (ox, oy) = (cw * 0.5, ch * 0.5);
    let (ink, paper) = (th::rgb(INK), th::rgb(PAPER));
    let scale = (0.86 + 0.14 * appear) * k * CAP;
    let mid = [(left + right) * 0.5, (top + bottom) * 0.5];
    let place_mark = |pts: Vec<[f32; 2]>| pts.into_iter().map(|[x, y]| [ox + mark_x * CAP + (x - mid[0]) * scale, oy + (y - mid[1]) * scale]).collect::<Vec<_>>();
    let place_word = |pts: &[[f32; 2]]| pts.iter().map(|[x, y]| [ox + (word_x + x) * CAP, oy + (BASELINE + y) * CAP]).collect::<Vec<_>>();

    let mut shape = Shape::new();
    for c in wordmark::BODY {
        shape = shape.fill(place_word(c), paper);
    }
    for c in wordmark::HOLES {
        shape = shape.fill(place_word(c), ink);
    }
    // The word comes out from behind the mark: nothing of it shows left of the mark's right edge.
    // A canvas doesn't clip, so the cover reaches as far left as the word ever is.
    let cut = ox + mark_x * CAP + (right - mid[0]) * scale;
    let far = ox - (wordmark::BOX[2] + width) * CAP;
    shape = shape.fill(vec![[far, 0.0], [cut, 0.0], [cut, ch], [far, ch]], ink);
    let mark_a = paper.alpha(appear);
    shape = shape.fill(place_mark(pebble), mark_a).fill(place_mark(mark::glint(192)), ink);

    Some(
        el().key("launch")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .center()
            .fill(ink)
            .opacity(1.0 - gone)
            .transition(Transition::none())
            .on_press(|_s: &mut State| {})
            .child(rune::canvas(shape).w(cw).h(ch)),
    )
}

fn ease_out_cubic(t: f64) -> f32 {
    let t = t.clamp(0.0, 1.0) as f32;
    1.0 - (1.0 - t).powi(3)
}

/// Out past the end by about 5% and back.
fn ease_out_back(t: f64) -> f32 {
    let t = t.clamp(0.0, 1.0) as f32;
    let (c1, c3) = (1.2, 2.2);
    1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
}

fn ease_in_out(t: f64) -> f32 {
    let t = t.clamp(0.0, 1.0) as f32;
    t * t * (3.0 - 2.0 * t)
}
