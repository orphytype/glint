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
        let (omega, zeta) = if open { (std::f32::consts::TAU / 0.24, 0.90) } else { (std::f32::consts::TAU / 0.18, 1.0) };
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
        let (response, zeta) = if open { (0.34, 0.86) } else { (0.24, 1.0) };
        self.spring(target, std::f32::consts::TAU / response, zeta, dt)
    }

    /// Springs to any `target` value with `response` seconds and damping ratio `zeta`.
    pub fn settle(&mut self, target: f32, response: f32, zeta: f32, dt: f32) -> bool {
        if self.t == target && self.v == 0.0 {
            return false;
        }
        self.spring(target, std::f32::consts::TAU / response, zeta, dt)
    }

    /// Critically damped, interruptible spring used for the Now Playing overlay.
    pub fn glide(&mut self, open: bool, dt: f32, reduce_motion: bool) -> bool {
        let target = if open { 1.0 } else { 0.0 };
        if self.t == target && self.v == 0.0 {
            return false;
        }
        if reduce_motion {
            (self.t, self.v) = (target, 0.0);
            return true;
        }
        let (response, zeta) = if open { (0.36, 0.98) } else { (0.28, 1.0) };
        self.spring(target, std::f32::consts::TAU / response, zeta, dt)
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

struct ScrollThumb {
    rect: rune::Live<[f32; 4]>,
    vp_h: f32,
    track_h: f32,
    x: f32,
    w: f32,
}

impl ScrollThumb {
    fn sync(&self, offset: f32, max: f32) {
        if max <= 2.0 || self.track_h <= 1.0 || self.vp_h <= 1.0 {
            self.rect.set([self.x, 0.0, 0.0, 0.0]);
            return;
        }
        let total = self.vp_h + max;
        let thumb_h = (self.track_h * (self.vp_h / total)).clamp(28.0, self.track_h);
        let frac = (offset / max).clamp(0.0, 1.0);
        let y = frac * (self.track_h - thumb_h);
        self.rect.set([self.x, y, self.w, thumb_h]);
    }
}

thread_local! {
    /// Last reported `[offset, max]` scroll position for each scroll container key.
    static PAGE_SCROLL: RefCell<HashMap<String, [f32; 2]>> = RefCell::new(HashMap::new());
    /// Live thumb rectangles updated directly from `on_scroll_offset` without rebuilding the view.
    static SCROLL_THUMBS: RefCell<HashMap<String, ScrollThumb>> = RefCell::new(HashMap::new());
    /// Scroll key of the active page, used to forward wheel events over inner widgets.
    pub static PAGE_KEY: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Last reported `[offset, max]` scroll state for `key`.
pub fn page_scroll(key: &str) -> Option<[f32; 2]> {
    PAGE_SCROLL.with_borrow(|m| m.get(key).copied())
}

/// Records the latest scroll position and moves the matching live scrollbar thumb in place.
pub fn report_scroll(key: &str, p: rune::ScrollPos) {
    let (offset, max) = (p.offset[1], p.max[1]);
    PAGE_SCROLL.with_borrow_mut(|m| {
        if m.len() > 24 && !m.contains_key(key) {
            m.retain(|k, _| !k.starts_with("sheet-scroll-"));
        }
        m.insert(key.to_owned(), [offset, max]);
    });
    SCROLL_THUMBS.with_borrow(|m| {
        if let Some(t) = m.get(key) {
            t.sync(offset, max);
        }
    });
}

/// Registers geometry for `key`'s scrollbar thumb and returns its live `[x, y, w, h]` handle.
pub fn scroll_thumb(key: &str, vp_h: f32, track_h: f32, x: f32, w: f32) -> rune::Live<[f32; 4]> {
    let pos = page_scroll(key).unwrap_or([0.0, 0.0]);
    SCROLL_THUMBS.with_borrow_mut(|m| {
        if m.len() > 24 && !m.contains_key(key) {
            m.retain(|k, _| !k.starts_with("sheet-scroll-"));
        }
        let t = m.entry(key.to_owned()).or_insert_with(|| ScrollThumb { rect: rune::Live::new([x, 0.0, 0.0, 0.0]), vp_h, track_h, x, w });
        t.vp_h = vp_h;
        t.track_h = track_h;
        t.x = x;
        t.w = w;
        t.sync(pos[0], pos[1]);
        t.rect.clone()
    })
}

/// Drops cached scroll state for `key` (called before switching to a page so stale thumbs hide).
pub fn clear_scroll(key: &str) {
    PAGE_SCROLL.with_borrow_mut(|m| {
        m.remove(key);
    });
    SCROLL_THUMBS.with_borrow(|m| {
        if let Some(t) = m.get(key) {
            t.sync(0.0, 0.0);
        }
    });
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
