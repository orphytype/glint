//! The butterflies on Home: a few that live over the garden. Each wanders on a heading that
//! turns by itself, beats its wings in bursts with glides between, keeps clear of the shapes and
//! the words, and now and then picks one of them and settles on it for a while, opening and
//! closing its wings. A press on one sends it (and any near it) off in a hurry.
//!
//! The simulation runs in `poll`, a step a frame while any of them moves; the view binds each one's
//! place and size to [`Live`] values, so nothing is laid out again while they fly. The compositor
//! reads the rest (heading, how open the wings are, the bank of a turn, the height) from here for
//! `fx::BUTTERFLY`.
//!
//! Positions are in the butterfly layer's own points (the top of Home, scrolling with it).

use std::f32::consts::{PI, TAU};
use std::time::Duration;

use rune::Live;

use crate::ui::blobs::Rng;

/// Something to keep clear of in flight, and to land on: a shape (`round`) or a block of words.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    pub id: u64,
    /// [x, y, w, h] in the layer's points.
    pub rect: [f32; 4],
    pub round: bool,
}

impl Spot {
    fn centre(&self) -> [f32; 2] {
        [self.rect[0] + self.rect[2] * 0.5, self.rect[1] + self.rect[3] * 0.5]
    }

    /// How far `p` is outside it (negative inside), and the way out.
    fn away(&self, p: [f32; 2]) -> (f32, [f32; 2]) {
        let c = self.centre();
        let (dx, dy) = (p[0] - c[0], p[1] - c[1]);
        if self.round {
            let r = self.rect[2].min(self.rect[3]) * 0.43;
            let l = (dx * dx + dy * dy).sqrt().max(1e-3);
            (l - r, [dx / l, dy / l])
        } else {
            let (hx, hy) = (self.rect[2] * 0.5, self.rect[3] * 0.5);
            let (qx, qy) = (dx.abs() - hx, dy.abs() - hy);
            let out = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
            let d = out + qx.max(qy).min(0.0);
            let dir = if qx > qy { [dx.signum(), 0.0] } else { [0.0, dy.signum()] };
            (d, dir)
        }
    }

    /// A place to settle on it: anywhere on a shape's face, on the upper part of a block of words.
    fn perch(&self, rng: &mut Rng) -> [f32; 2] {
        let c = self.centre();
        if self.round {
            let r = self.rect[2].min(self.rect[3]) * 0.43 * rng.range(0.0, 0.7);
            let a = rng.range(0.0, TAU);
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        } else if self.rect[2] < 80.0 {
            c
        } else {
            [self.rect[0] + self.rect[2] * rng.range(0.15, 0.85), self.rect[1] + self.rect[3] * rng.range(0.25, 0.45)]
        }
    }
}

/// Where the layer is, as the view last measured it.
#[derive(Clone, Debug, Default)]
pub struct Geom {
    /// The layer's [x, y, w, h] in the window when the view ran.
    pub layer: [f32; 4],
    /// The page's scroll offset at that moment.
    pub scroll: f32,
    /// The window's band the page shows through: [top, bottom].
    pub view: [f32; 2],
    pub spots: Vec<Spot>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Wander { until: f64 },
    Approach { spot: u64, at: [f32; 2] },
    /// `at` is relative to the spot's corner, so it rides along if the spot moves.
    Landed { spot: u64, at: [f32; 2], until: f64 },
    Flee { until: f64 },
}

pub struct Bug {
    pub seed: f32,
    pub hue: f32,
    /// Wing tip to wing tip, points.
    pub span: f32,
    /// The middle, in layer points.
    pub pos: [f32; 2],
    /// Which way it travels (radians, screen space: 0 to the right, π/2 down).
    pub heading: f32,
    /// Wings: 1 spread flat, near 0 folded up together.
    pub open: f32,
    /// -1..1, into a turn.
    pub bank: f32,
    /// 0 on a surface .. 1 high.
    pub z: f32,
    speed: f32,
    turn: f32,
    turn_want: f32,
    turn_next: f64,
    phase: f32,
    /// Beating (true) or gliding, and until when.
    beating: bool,
    beat_until: f64,
    /// At rest: when the wings next close and open, and when the one doing so began.
    flick_at: f64,
    flick_from: f64,
    mode: Mode,
    pub off: Live<[f32; 2]>,
    pub scale: Live<f32>,
}

impl Bug {
    /// The square the compositor draws it (and its shadow) in.
    pub fn boxed(&self) -> f32 {
        (self.span * 1.9).round()
    }

    #[cfg(test)]
    fn flying(&self) -> bool {
        !matches!(self.mode, Mode::Landed { .. })
    }

    fn publish(&self) {
        let b = self.boxed();
        self.off.set([self.pos[0] - b * 0.5, self.pos[1] - b * 0.5]);
        self.scale.set(0.8 + 0.36 * self.z);
    }
}

pub struct Swarm {
    pub seed: u64,
    pub bugs: Vec<Bug>,
    rng: Rng,
    placed: bool,
}

impl Swarm {
    pub fn new(seed: u64) -> Swarm {
        let mut rng = Rng::new(seed ^ 0x6275_6773);
        let n = 3 + rng.pick(2);
        // Colours apart from each other around the wheel, from one start.
        let h0 = rng.range(0.0, 360.0);
        let bugs = (0..n)
            .map(|i| Bug {
                seed: rng.seed(),
                hue: (h0 + i as f32 * 360.0 / n as f32 + rng.range(-25.0, 25.0)).rem_euclid(360.0),
                span: rng.range(40.0, 56.0),
                pos: [0.0, 0.0],
                heading: rng.range(0.0, TAU),
                open: 1.0,
                bank: 0.0,
                z: 0.5,
                speed: 70.0,
                turn: 0.0,
                turn_want: 0.0,
                turn_next: 0.0,
                phase: rng.range(0.0, TAU),
                beating: true,
                beat_until: 0.0,
                flick_at: 0.0,
                flick_from: -9.0,
                mode: Mode::Wander { until: 0.0 },
                off: Live::new([-200.0, -200.0]),
                scale: Live::new(1.0),
            })
            .collect();
        Swarm { seed, bugs, rng, placed: false }
    }

    /// The bugs near `i` (and `i` itself) take off fast, away from it.
    pub fn scare(&mut self, i: usize, now: f64) {
        let Some(p) = self.bugs.get(i).map(|b| b.pos) else { return };
        for (j, b) in self.bugs.iter_mut().enumerate() {
            let (dx, dy) = (b.pos[0] - p[0], b.pos[1] - p[1]);
            let d = (dx * dx + dy * dy).sqrt();
            if j != i && d > 170.0 {
                continue;
            }
            // Away from the press, and upward-ish; the one pressed goes any way it likes.
            let base = if j == i { self.rng.range(0.0, TAU) } else { dy.atan2(dx) };
            b.heading = base + self.rng.range(-0.5, 0.5);
            b.turn = 0.0;
            b.turn_want = self.rng.range(-1.0, 1.0);
            b.speed = b.speed.max(150.0);
            b.beating = true;
            b.mode = Mode::Flee { until: now + self.rng.range(0.9, 1.5) as f64 };
        }
    }

    /// One step of `dt` seconds. Returns how soon the next step is wanted (`None`: nothing to do).
    pub fn step(&mut self, now: f64, dt: f32, g: &Geom, scroll: f32) -> Option<Duration> {
        let [_, ly, w, h] = g.layer;
        if w < 100.0 || h < 100.0 {
            return None;
        }
        // The band of the layer that shows right now.
        let ly_now = ly - (scroll - g.scroll);
        let vis = [(g.view[0] - ly_now).max(0.0), (g.view[1] - ly_now).min(h)];
        if vis[1] - vis[0] < 60.0 {
            return None;
        }
        let margin = 28.0;
        let bx = [margin, w - margin];
        let by = [vis[0] + margin, (vis[1] - margin).max(vis[0] + margin + 1.0)];
        if !self.placed {
            self.placed = true;
            for b in &mut self.bugs {
                b.pos = [self.rng.range(bx[0], bx[1]), self.rng.range(by[0], by[1])];
                b.mode = Mode::Wander { until: now + self.rng.range(1.5, 7.0) as f64 };
                b.beat_until = now;
            }
        }
        let mut soonest = 2.0f64;
        let mut busy = false;
        for i in 0..self.bugs.len() {
            let taken: Vec<u64> = self
                .bugs
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .filter_map(|(_, o)| match o.mode {
                    Mode::Approach { spot, .. } | Mode::Landed { spot, .. } => Some(spot),
                    _ => None,
                })
                .collect();
            let rng = &mut self.rng;
            let b = &mut self.bugs[i];
            match b.mode {
                Mode::Landed { spot, at, until } => {
                    let Some(s) = g.spots.iter().find(|s| s.id == spot) else {
                        b.mode = Mode::Wander { until: now + 3.0 };
                        continue;
                    };
                    b.pos = [s.rect[0] + at[0], s.rect[1] + at[1]];
                    b.z = (b.z - dt * 3.0).max(0.0);
                    b.bank *= 0.8;
                    // Wings spread in the sun; now and then they fold up and open again, slowly.
                    if now >= b.flick_at {
                        b.flick_from = now;
                        b.flick_at = now + rng.range(1.4, 4.5) as f64;
                    }
                    let ft = (now - b.flick_from) as f32;
                    let flick = if ft < 1.1 { (ft / 1.1 * PI).sin() } else { 0.0 };
                    let open = 1.0 - 0.82 * flick;
                    if (open - b.open).abs() > 1e-4 || b.z > 0.0 {
                        busy = true;
                    }
                    b.open = open;
                    if now >= until {
                        b.mode = Mode::Wander { until: now + rng.range(4.0, 10.0) as f64 };
                        b.heading = rng.range(-PI, 0.0);
                        b.speed = 40.0;
                        b.beating = true;
                        b.beat_until = now + 1.0;
                    } else if ft >= 1.1 {
                        soonest = soonest.min((b.flick_at - now).min(until - now).max(0.016));
                    }
                    b.publish();
                    continue;
                }
                Mode::Wander { until } if now >= until => {
                    // Pick somewhere free to settle that can be seen.
                    let free: Vec<&Spot> = g
                        .spots
                        .iter()
                        .filter(|s| !taken.contains(&s.id))
                        .filter(|s| {
                            let c = s.centre();
                            c[1] > by[0] && c[1] < by[1]
                        })
                        .collect();
                    if free.is_empty() {
                        b.mode = Mode::Wander { until: now + 3.0 };
                    } else {
                        let s = free[rng.pick(free.len())];
                        let p = s.perch(rng);
                        b.mode = Mode::Approach { spot: s.id, at: [p[0] - s.rect[0], p[1] - s.rect[1]] };
                    }
                }
                Mode::Flee { until } if now >= until => {
                    b.mode = Mode::Wander { until: now + rng.range(5.0, 11.0) as f64 };
                }
                _ => {}
            }
            busy = true;

            // ---- in flight ----
            let fleeing = matches!(b.mode, Mode::Flee { .. });
            let mut steer = [0.0f32, 0.0f32];
            let mut target: Option<[f32; 2]> = None;
            let mut skip = None;
            if let Mode::Approach { spot, at } = b.mode {
                match g.spots.iter().find(|s| s.id == spot) {
                    Some(s) => {
                        target = Some([s.rect[0] + at[0], s.rect[1] + at[1]]);
                        skip = Some(spot);
                    }
                    None => b.mode = Mode::Wander { until: now + 2.0 },
                }
            }
            // Keep inside what shows (a fleeing one may leave it, and comes back after).
            let edge = 70.0;
            let push = |v: f32, lo: f32, hi: f32| -> f32 {
                if v < lo + edge {
                    ((lo + edge - v) / edge).min(2.0)
                } else if v > hi - edge {
                    -((v - (hi - edge)) / edge).min(2.0)
                } else {
                    0.0
                }
            };
            if !fleeing {
                steer[0] += push(b.pos[0], bx[0], bx[1]) * 1.4;
                steer[1] += push(b.pos[1], by[0], by[1]) * 1.4;
            }
            // Clear of the shapes and the words, unless that is where it is going.
            for s in &g.spots {
                if Some(s.id) == skip {
                    continue;
                }
                let (d, dir) = s.away(b.pos);
                let reach = 34.0;
                if d < reach {
                    let k = if d < 0.0 { 2.5 } else { (1.0 - d / reach) * 1.6 };
                    steer[0] += dir[0] * k;
                    steer[1] += dir[1] * k;
                }
            }
            // The heading turns on its own, a new mind every second or so.
            if now >= b.turn_next {
                b.turn_want = rng.range(-2.4, 2.4) * if fleeing { 0.4 } else { 1.0 };
                b.turn_next = now + rng.range(0.35, 1.4) as f64;
            }
            b.turn += (b.turn_want - b.turn) * (1.0 - (-dt * 3.0).exp());
            let mut dh = b.turn * dt;
            let mut want_speed = if fleeing { 230.0 } else { 78.0 };
            if let Some(t) = target {
                let (dx, dy) = (t[0] - b.pos[0], t[1] - b.pos[1]);
                let dist = (dx * dx + dy * dy).sqrt();
                if dist < 3.0
                    && let Mode::Approach { spot, at } = b.mode
                {
                    b.mode = Mode::Landed { spot, at, until: now + rng.range(3.0, 9.0) as f64 };
                    b.flick_at = now + rng.range(0.6, 2.0) as f64;
                    b.flick_from = -9.0;
                    b.pos = t;
                    b.speed = 0.0;
                    b.heading += rng.range(-0.6, 0.6);
                    b.publish();
                    continue;
                }
                // Toward it, ever more surely as it nears, slowing to settle.
                steer[0] += dx / dist * 2.2;
                steer[1] += dy / dist * 2.2;
                want_speed = (dist * 1.6).clamp(26.0, 84.0);
                dh *= (dist / 120.0).clamp(0.1, 1.0);
            }
            let s2 = steer[0] * steer[0] + steer[1] * steer[1];
            if s2 > 1e-4 {
                let a = steer[1].atan2(steer[0]);
                let mut diff = (a - b.heading).rem_euclid(TAU);
                if diff > PI {
                    diff -= TAU;
                }
                let rate = 4.5 * s2.sqrt().min(2.5);
                dh += diff.clamp(-rate * dt, rate * dt);
            }
            b.heading = (b.heading + dh).rem_euclid(TAU);
            let turn_rate = dh / dt.max(1e-3);
            b.bank += ((turn_rate * 0.3).clamp(-1.0, 1.0) - b.bank) * (1.0 - (-dt * 6.0).exp());

            // Beats in bursts, glides between (not while fleeing or landing close).
            if now >= b.beat_until {
                b.beating = fleeing || !b.beating || target.is_some_and(|_| want_speed < 40.0);
                b.beat_until = now + if b.beating { rng.range(0.5, 1.5) } else { rng.range(0.25, 0.7) } as f64;
            }
            let hz = if fleeing { 14.0 } else { 9.0 + 2.0 * (b.seed.fract() - 0.5) };
            if b.beating {
                b.phase = (b.phase + dt * hz * TAU).rem_euclid(TAU);
                b.open = 0.14 + 0.86 * (0.5 + 0.5 * b.phase.cos());
            } else {
                b.open += (0.96 - b.open) * (1.0 - (-dt * 10.0).exp());
                want_speed *= 0.8;
            }
            b.speed += (want_speed - b.speed) * (1.0 - (-dt * 2.5).exp());
            // Each beat lifts it a little: a bob across its path.
            let bob = if b.beating { b.phase.sin() * 10.0 * dt * hz } else { 0.0 };
            let (c, s) = (b.heading.cos(), b.heading.sin());
            b.pos[0] += c * b.speed * dt - s * bob * 0.3;
            b.pos[1] += s * b.speed * dt + c * bob * 0.3 - if b.beating { 0.0 } else { 6.0 * dt };
            b.pos[0] = b.pos[0].clamp(-60.0, w + 60.0);
            b.pos[1] = b.pos[1].clamp(-60.0, h + 60.0);
            // Height: high when fleeing, coming down to land, a slow rise and fall otherwise.
            let z_want = match (fleeing, target) {
                (true, _) => 1.0,
                (_, Some(t)) => (((t[0] - b.pos[0]).hypot(t[1] - b.pos[1])) / 200.0).clamp(0.0, 0.6),
                _ => 0.5 + 0.18 * ((now as f32) * 0.5 + b.seed).sin(),
            };
            b.z += (z_want - b.z) * (1.0 - (-dt * 2.0).exp());
            b.publish();
        }
        if busy {
            Some(Duration::from_millis(16))
        } else {
            Some(Duration::from_secs_f64(soonest.max(0.016)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geom() -> Geom {
        Geom {
            layer: [100.0, 50.0, 900.0, 600.0],
            scroll: 0.0,
            view: [60.0, 760.0],
            spots: vec![
                Spot { id: 1, rect: [0.0, 100.0, 400.0, 400.0], round: true },
                Spot { id: 2, rect: [500.0, 120.0, 150.0, 150.0], round: true },
                Spot { id: 3, rect: [0.0, 0.0, 300.0, 50.0], round: false },
            ],
        }
    }

    #[test]
    fn butterflies_settle_somewhere_and_take_off_again() {
        let g = geom();
        let mut sw = Swarm::new(7);
        let mut t = 0.0;
        let mut landed = false;
        let mut flew_again = false;
        for _ in 0..(60 * 60) {
            t += 1.0 / 60.0;
            sw.step(t, 1.0 / 60.0, &g, 0.0);
            for b in &sw.bugs {
                assert!(b.pos[0].is_finite() && b.pos[1].is_finite());
                assert!(b.open > 0.0 && b.open <= 1.0);
            }
            if sw.bugs.iter().any(|b| !b.flying()) {
                landed = true;
            } else if landed {
                flew_again = true;
            }
        }
        assert!(landed, "no butterfly ever settled");
        assert!(flew_again || sw.bugs.iter().any(Bug::flying), "they never took off again");
    }

    #[test]
    fn a_scared_butterfly_flies_off() {
        let g = geom();
        let mut sw = Swarm::new(3);
        sw.step(0.0, 0.016, &g, 0.0);
        sw.scare(0, 0.0);
        let p0 = sw.bugs[0].pos;
        for k in 1..30 {
            sw.step(k as f64 / 60.0, 1.0 / 60.0, &g, 0.0);
        }
        let p1 = sw.bugs[0].pos;
        assert!((p1[0] - p0[0]).hypot(p1[1] - p0[1]) > 40.0);
    }
}
