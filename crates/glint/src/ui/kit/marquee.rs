//! Overflowing line marquee scrolling and row-width measurement cache.

use std::cell::RefCell;
use std::collections::HashMap;

use rune::{El, Tx, el, row};

use super::under;
use crate::app::State;

const MARQUEE_GAP: f32 = 48.0;
const MARQUEE_SPEED: f32 = 58.0;
const MARQUEE_SLOW: f32 = 0.55;

thread_local! {
    static MARQUEE_UNDER: RefCell<Option<String>> = const { RefCell::new(None) };
    static ROOM: RefCell<HashMap<String, f32>> = RefCell::new(HashMap::new());
}

/// Renders a single-line widget inside `width`, looping smoothly when `line_w > width` and `moving`.
pub fn marquee(key: impl Into<String>, width: f32, line_w: f32, moving: bool, reduce: bool, build: impl Fn() -> El) -> El {
    let key: String = key.into();
    if !width.is_finite() {
        return el().key(key).w_full().min_w(0.0).clip_fade(0.0, 0.0, 26.0, 0.0).child(row().child(build()));
    }
    if line_w <= width + 0.5 {
        return build();
    }
    if !moving || reduce {
        let strip = row().key(format!("{key}-strip")).w(line_w).items_center().child(build());
        return el().key(key).w(width).min_w(0.0).clip_fade(0.0, 0.0, 26.0, 0.0).child(strip);
    }
    const LEAD: f32 = 14.0;
    let travel = line_w + MARQUEE_GAP;
    let strip_w = LEAD + 2.0 * line_w + MARQUEE_GAP;
    let slow = MARQUEE_UNDER.with_borrow(|k| k.as_deref() == Some(key.as_str()));
    let speed = if slow { MARQUEE_SPEED * MARQUEE_SLOW } else { MARQUEE_SPEED };
    let period = f64::from(travel / speed * 1.25 + 1.1);
    let spec = rune::LoopSpec::slide(period, [0.0, 0.0], [-travel / strip_w, 0.0]).easing(rune::Easing::cubic(0.36, 0.0, 0.2, 1.0));
    let copy = || row().w(line_w).min_w(line_w).items_center().child(build());
    let strip = row().key(format!("{key}-strip")).w(strip_w).pl(LEAD).gap(MARQUEE_GAP).items_center().child(copy()).child(copy()).looping(spec);
    let hover_key = key.clone();
    el().key(key)
        .w(width + LEAD)
        .ml(-LEAD)
        .min_w(0.0)
        .clip_fade(LEAD, 0.0, 26.0, 0.0)
        .on_hover_change(move |_s: &mut State, on: bool, _tx: &mut Tx| {
            MARQUEE_UNDER.with_borrow_mut(|k| {
                if on {
                    *k = Some(hover_key.clone());
                } else if k.as_deref() == Some(hover_key.as_str()) {
                    *k = None;
                }
            });
        })
        .child(strip)
}

pub fn marquee_hover(key: String) -> impl Fn(&mut State, bool, &mut Tx) + 'static {
    move |s: &mut State, on: bool, _tx: &mut Tx| {
        if on {
            if s.marquee_hot.as_deref() != Some(key.as_str()) {
                s.marquee_hot = Some(key.clone());
            }
        } else if s.marquee_hot.as_deref() == Some(key.as_str()) {
            s.marquee_hot = None;
        }
    }
}

pub fn room(key: &str) -> f32 {
    ROOM.with_borrow(|m| m.get(key).copied()).unwrap_or(f32::INFINITY)
}

/// Records `e`'s laid-out width for `key`, skipping rebuilds and key clones when unchanged.
pub fn measure_room(e: El, key: String) -> El {
    e.on_layout_tx(move |_s: &mut State, r: [f32; 4], tx: &mut Tx| {
        let changed = ROOM.with_borrow_mut(|m| {
            if let Some(old) = m.get_mut(&key) {
                let diff = (*old - r[2]).abs() > 1.0;
                *old = r[2];
                diff
            } else {
                m.insert(key.clone(), r[2]);
                true
            }
        });
        if !changed {
            tx.live_only();
        }
    })
}

pub fn marquee_on(s: &State, key: &str) -> bool {
    s.marquee_hot.as_deref() == Some(key) && !under()
}
