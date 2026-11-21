//! UI spring animations (`Pop`) and native spring wheel scrolling (`WheelGlide`).

use std::cell::RefCell;
use std::collections::HashMap;

/// A pop-up that opens and closes on a spring: `t` is openness (0 shut, 1 open), `v` is velocity.
///
/// Velocity is preserved when the target flips, so reversing mid-flight has no jump. Reduce motion
/// snaps straight to the target.
#[derive(Clone, Copy, Debug, Default)]
pub struct Pop {
    pub t: f32,
    pub v: f32,
}

impl Pop {
    /// Steps toward open or shut over `dt` seconds. Returns whether anything moved.
    pub fn step(&mut self, open: bool, dt: f32, reduce_motion: bool) -> bool {
        let target = if open { 1.0 } else { 0.0 };
        if self.t == target && self.v == 0.0 {
            return false;
        }
        if reduce_motion {
            (self.t, self.v) = (target, 0.0);
            return true;
        }
        let (omega, zeta) = if open { (std::f32::consts::TAU / 0.40, 0.92) } else { (std::f32::consts::TAU / 0.30, 1.0) };
        self.spring(target, omega, zeta, dt)
    }

    /// Spring for the details card: slight overshoot on open, critically damped close.
    pub fn morph(&mut self, open: bool, dt: f32, reduce_motion: bool) -> bool {
        let target = if open { 1.0 } else { 0.0 };
        if self.t == target && self.v == 0.0 {
            return false;
        }
        if reduce_motion {
            (self.t, self.v) = (target, 0.0);
            return true;
        }
        let (response, zeta) = if open { (0.52, 0.8) } else { (0.4, 1.0) };
        self.spring(target, std::f32::consts::TAU / response, zeta, dt)
    }

    /// Springs to any `target` value with `response` seconds and damping ratio `zeta`.
    pub fn settle(&mut self, target: f32, response: f32, zeta: f32, dt: f32) -> bool {
        if self.t == target && self.v == 0.0 {
            return false;
        }
        self.spring(target, std::f32::consts::TAU / response, zeta, dt)
    }

    /// Slower, critically damped spring used for the Now Playing overlay.
    pub fn glide(&mut self, open: bool, dt: f32, reduce_motion: bool) -> bool {
        let target = if open { 1.0 } else { 0.0 };
        if self.t == target && self.v == 0.0 {
            return false;
        }
        if reduce_motion {
            (self.t, self.v) = (target, 0.0);
            return true;
        }
        let omega = std::f32::consts::TAU / if open { 0.62 } else { 0.5 };
        self.spring(target, omega, 1.0, dt)
    }

    fn spring(&mut self, target: f32, omega: f32, zeta: f32, dt: f32) -> bool {
        let steps = (dt * 240.0).ceil().clamp(1.0, 30.0);
        let h = dt / steps;
        for _ in 0..steps as u32 {
            let accel = omega * omega * (target - self.t) - 2.0 * zeta * omega * self.v;
            self.v += accel * h;
            self.t += self.v * h;
        }
        if (self.t - target).abs() < 0.002 && self.v.abs() < 0.02 {
            (self.t, self.v) = (target, 0.0);
        }
        true
    }

    /// Whether the spring has not yet settled at `open`.
    pub fn moving(self, open: bool) -> bool {
        self.t != (if open { 1.0 } else { 0.0 }) || self.v != 0.0
    }
}

/// Mouse-wheel scrolling handed off to RUNE's native spring (`Tx::scroll_to_with`) so notched wheel
/// scrolling runs at the display's full refresh rate without rebuilding the view.
#[derive(Clone, Debug, Default)]
pub struct WheelGlide {
    pub key: String,
    pub target: f32,
    pub at: f64,
    pub active: bool,
}

/// Distance one wheel notch scrolls a page, in points (RUNE's default step is 40).
pub const WHEEL_STEP: f32 = 70.0;

/// Spring used for notched mouse-wheel scrolling.
const WHEEL_SPRING: rune::Spring = rune::Spring::new(0.24, 0.0);

/// How far ahead of playback the synced lyrics run, in milliseconds.
pub const LYRIC_LEAD_MS: u64 = 250;

thread_local! {
    /// Last reported `[offset, max]` scroll position for each page scroll key.
    pub static PAGE_SCROLL: RefCell<HashMap<String, [f32; 2]>> = RefCell::new(HashMap::new());
    /// Scroll key of the active page, used to forward wheel events over inner widgets.
    pub static PAGE_KEY: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Last reported `[offset, max]` scroll state for `key`.
pub fn page_scroll(key: &str) -> Option<[f32; 2]> {
    PAGE_SCROLL.with_borrow(|m| m.get(key).copied())
}

impl WheelGlide {
    /// Applies a wheel delta (`dy` points, positive scrolling up) to scroll container `key`.
    pub fn wheel_to(&mut self, key: &str, dy: f32, now: f64, reduce: bool, tx: &mut rune::Tx) {
        let [offset, max] = page_scroll(key).unwrap_or([0.0, f32::MAX]);
        let delta = -(dy / 40.0) * WHEEL_STEP;
        let fresh = !self.active || self.key != key || now - self.at > 0.35 || (self.target - offset).abs() < 0.5 || (delta != 0.0 && (self.target - offset) * delta < 0.0);
        if fresh {
            self.key = key.to_owned();
            self.target = offset;
        }
        let next = (self.target + delta).clamp(0.0, max.max(0.0));
        self.target = next;
        self.at = now;
        self.active = (next - offset).abs() > 0.5;
        if self.active || !fresh {
            if reduce {
                tx.scroll_to_y(key, next);
            } else {
                tx.scroll_to_with(key, 0.0, next, WHEEL_SPRING);
            }
        }
    }

    /// Forwards a wheel delta to the current page's scroll container.
    pub fn wheel_page(&mut self, dy: f32, now: f64, reduce: bool, tx: &mut rune::Tx) -> bool {
        let key = PAGE_KEY.with_borrow(Clone::clone);
        if key.is_empty() {
            return false;
        }
        self.wheel_to(&key, dy, now, reduce, tx);
        true
    }
}
