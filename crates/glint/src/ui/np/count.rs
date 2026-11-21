//! Animated 3-2-1 morphing count-in above timed lyrics, crumbling into bouncing grains on beat 0.

use std::sync::OnceLock;

use rune::{El, Shape, Transition, canvas, el};

use crate::app::{Load, State};
use crate::theme as th;

const COUNT_MS: i64 = 3000;
const MORPH_MS: f32 = 600.0;
const COUNT_PTS: usize = 72;
pub(super) const COUNT_W: f32 = 52.0;
const COUNT_H: f32 = 60.0;
const RING_PAD: f32 = 18.0;
const GRAIN_S: f32 = 1.7;
const STROKE: f32 = 4.2;

/// Start time in ms of the first sung line when late enough for a 3-second count-in.
pub fn count_start(l: &ym_api::Lyrics) -> Option<u32> {
    if !l.synced {
        return None;
    }
    let first = l.lines.iter().find(|x| !x.text.trim().is_empty() || !x.words.is_empty())?;
    let ms = first.words.first().map(|w| w.start_ms).or(first.time_ms)?;
    (i64::from(ms) > COUNT_MS).then_some(ms)
}

/// Whether the count-in morph or grain scatter is currently active.
pub fn counting(s: &State) -> bool {
    let Load::Ready(l) = &s.lyrics.lyrics else { return false };
    let Some(first) = count_start(l) else { return false };
    let left = i64::from(first) - i64::from(s.lyric_ms());
    s.player.playing && left <= COUNT_MS + 50 && left > -(GRAIN_S * 1000.0) as i64 - 100
}

/// Closed-form parabolic gravity and bounce integration (converges in <= 4 bounces).
fn simulate_grain(mut x: f32, mut y: f32, mut vx: f32, mut vy: f32, land: f32, mut rem: f32) -> (f32, f32) {
    const G: f32 = 1500.0;
    while rem > 0.0 {
        let h = (land - y).max(0.0);
        let disc = (vy * vy + 2.0 * G * h).max(0.0);
        let t_hit = (-vy + disc.sqrt()) / G;
        if rem <= t_hit {
            x += vx * rem;
            y += vy * rem + 0.5 * G * rem * rem;
            break;
        }
        x += vx * t_hit;
        y = land;
        rem -= t_hit;
        vy = -(vy + G * t_hit) * 0.32;
        vx *= 0.6;
        if vy.abs() < 18.0 {
            break;
        }
    }
    (x, y)
}

/// Renders the morphing note/countdown above timed lyrics (`align`: 0 left, 1 center, 2 right).
pub(super) fn count_note(first: u32, pos: u32, w: f32, align: u8) -> El {
    let since = COUNT_MS - (i64::from(first) - i64::from(pos));
    let x0 = match align {
        1 => ((w - COUNT_W) * 0.5).floor(),
        2 => w - COUNT_W,
        _ => -2.0,
    };
    let wrap = el().key("ly-note").w_full().h(COUNT_H).transition(Transition::none());
    let ink = th::text();
    if since >= COUNT_MS {
        let gone = (since - COUNT_MS) as f32 / 1000.0;
        if gone >= GRAIN_S {
            return wrap;
        }
        let fade = 1.0 - ((gone - 0.85) / (GRAIN_S - 0.85)).clamp(0.0, 1.0);
        let a = 0.92 * fade;
        if a <= 0.01 {
            return wrap;
        }
        let pts = one_points();
        let hex = unit_hex();
        let floor = COUNT_H + 20.0;
        let room = floor + 24.0;
        let color = ink.alpha(a);
        let mut shape = Shape::new();
        for (i, p) in pts.iter().enumerate() {
            let h = |k: u32| ((i as u32).wrapping_mul(2_654_435_761).wrapping_add(k.wrapping_mul(40_503)) % 1000) as f32 / 1000.0;
            let start = (1.0 - p[1]) * 0.10 + h(1) * 0.06;
            let age = gone - start;
            let (mut x, mut y) = (pad_x(p[0]), pad_y(p[1]));
            let size = 2.2 + 1.4 * h(2);
            if age > 0.0 {
                let vx = (h(3) - 0.5) * 90.0 + (p[0] - 0.55) * 60.0;
                let vy = -(25.0 + 70.0 * h(4));
                let land = floor + 4.0 * h(5) - size;
                (x, y) = simulate_grain(x, y, vx, vy, land, age);
            }
            let r = size * 0.5;
            let ox = 40.0 + x;
            let dot: Vec<[f32; 2]> = hex.iter().map(|&[cx, cy]| [ox + r * cx, y + r * cy]).collect();
            shape = shape.fill(dot, color);
        }
        let span = COUNT_W + 80.0;
        let layer = canvas(shape.view_box(span, room)).key("ly-note-grains").absolute(x0 - 40.0, 0.0).w(span).h(room).transition(Transition::none()).motion(rune::Spring::NONE);
        return wrap.child(layer);
    }
    let (from, to, local) = if since < 0 {
        (0, 0, f32::INFINITY)
    } else {
        let k = ((since / 1000) as usize).min(2);
        (k, k + 1, (since - k as i64 * 1000) as f32)
    };
    let tt = local / MORPH_MS;
    let pop = 0.86 + 0.14 * settle(local / 520.0);
    let weight = STROKE * (1.0 + 0.45 * (-local / 240.0).exp());
    let a = resampled_figure(from);
    let b = resampled_figure(to);
    let last = (COUNT_PTS - 1) as f32;
    let place = |x: f32, y: f32| [pad_x(0.5 + (x - 0.5) * pop) + RING_PAD, pad_y(0.5 + (y - 0.5) * pop) + RING_PAD];
    let pts: Vec<[f32; 2]> = a
        .iter()
        .zip(b)
        .enumerate()
        .map(|(i, (p, q))| {
            let u = ((tt - 0.32 * i as f32 / last) / 0.68).clamp(0.0, 1.0);
            let e = settle(u);
            let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
            let swing = 0.24 * (std::f32::consts::PI * u).sin();
            place(p[0] + dx * e - dy * swing, p[1] + dy * e + dx * swing)
        })
        .collect();
    let mut shape = Shape::new().stroke_with(pts, false, weight, rune::LineCap::Round, ink.alpha(0.92));
    let foot = if to == 3 { settle(tt).max(0.0) } else { 0.0 };
    if foot > 0.0 {
        let (l, r) = (0.58 - 0.26 * foot, 0.58 + 0.26 * foot);
        shape = shape.stroke_with(vec![place(l, 0.98), place(r, 0.98)], false, weight, rune::LineCap::Round, ink.alpha(0.92 * foot.min(1.0)));
    }
    let ring = local / 650.0;
    if ring < 1.0 {
        let grow = 1.0 - (1.0 - ring).powi(3);
        let rad = 24.0 + 16.0 * grow;
        let (cx, cy) = (RING_PAD + COUNT_W * 0.5, RING_PAD + COUNT_H * 0.5);
        let circle: Vec<[f32; 2]> = unit_ring().iter().map(|&[ux, uy]| [cx + rad * ux, cy + rad * 1.08 * uy]).collect();
        shape = shape.stroke_with(circle, true, 1.6, rune::LineCap::Round, ink.alpha(0.30 * (1.0 - ring).powi(2)));
    }
    let (bw, bh) = (COUNT_W + 2.0 * RING_PAD, COUNT_H + 2.0 * RING_PAD);
    wrap.child(canvas(shape.view_box(bw, bh)).key("ly-note-fig").absolute(x0 - RING_PAD, -RING_PAD).w(bw).h(bh).transition(Transition::none()).motion(rune::Spring::NONE))
}

fn figure(k: usize) -> Vec<[f32; 2]> {
    match k {
        0 => {
            let mut p = vec![[0.86, 0.42], [0.80, 0.27], [0.68, 0.16], [0.58, 0.05], [0.58, 0.30], [0.58, 0.55], [0.58, 0.79]];
            for i in 1..=24 {
                let a = std::f32::consts::TAU * i as f32 / 24.0;
                p.push([0.40 + 0.18 * a.cos(), 0.80 + 0.14 * a.sin()]);
            }
            p
        }
        1 => vec![
            [0.18, 0.18],
            [0.30, 0.07],
            [0.50, 0.03],
            [0.70, 0.08],
            [0.80, 0.22],
            [0.76, 0.36],
            [0.62, 0.45],
            [0.46, 0.48],
            [0.62, 0.51],
            [0.80, 0.62],
            [0.84, 0.78],
            [0.74, 0.92],
            [0.52, 0.98],
            [0.30, 0.95],
            [0.16, 0.84],
        ],
        2 => vec![[0.18, 0.24], [0.28, 0.09], [0.48, 0.02], [0.68, 0.06], [0.80, 0.20], [0.78, 0.36], [0.62, 0.54], [0.40, 0.75], [0.18, 0.97], [0.52, 0.97], [0.86, 0.97]],
        _ => vec![[0.26, 0.24], [0.42, 0.13], [0.58, 0.02], [0.58, 0.35], [0.58, 0.67], [0.58, 0.98]],
    }
}

fn resample(pts: &[[f32; 2]], n: usize) -> Vec<[f32; 2]> {
    let mut dense = Vec::new();
    for i in 0..pts.len().saturating_sub(1) {
        let p0 = pts[i.saturating_sub(1)];
        let (p1, p2) = (pts[i], pts[i + 1]);
        let p3 = pts[(i + 2).min(pts.len() - 1)];
        for j in 0..8 {
            let t = j as f32 / 8.0;
            let (t2, t3) = (t * t, t * t * t);
            let c = |a: f32, b: f32, c: f32, d: f32| 0.5 * (2.0 * b + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3);
            dense.push([c(p0[0], p1[0], p2[0], p3[0]), c(p0[1], p1[1], p2[1], p3[1])]);
        }
    }
    dense.push(*pts.last().unwrap_or(&[0.0, 0.0]));
    let mut acc = vec![0.0f32];
    for w in dense.windows(2) {
        let d = ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt();
        acc.push(acc.last().copied().unwrap_or(0.0) + d);
    }
    let total = acc.last().copied().unwrap_or(0.0).max(1e-6);
    let mut out = Vec::with_capacity(n);
    let mut k = 0;
    for i in 0..n {
        let want = total * i as f32 / (n - 1) as f32;
        while k + 1 < acc.len() - 1 && acc[k + 1] < want {
            k += 1;
        }
        let span = (acc[k + 1] - acc[k]).max(1e-6);
        let f = ((want - acc[k]) / span).clamp(0.0, 1.0);
        out.push([dense[k][0] + (dense[k + 1][0] - dense[k][0]) * f, dense[k][1] + (dense[k + 1][1] - dense[k][1]) * f]);
    }
    out
}

fn settle(u: f32) -> f32 {
    if u <= 0.0 {
        0.0
    } else if u >= 1.0 {
        1.0
    } else {
        1.0 - (-6.0 * u).exp() * (7.0 * u).cos()
    }
}

fn pad_x(x: f32) -> f32 {
    5.0 + x * (COUNT_W - 10.0)
}

fn pad_y(y: f32) -> f32 {
    5.0 + y * (COUNT_H - 10.0)
}

fn resampled_figure(k: usize) -> &'static [[f32; 2]] {
    static FIGURES: OnceLock<[Vec<[f32; 2]>; 4]> = OnceLock::new();
    let all = FIGURES.get_or_init(|| [resample(&figure(0), COUNT_PTS), resample(&figure(1), COUNT_PTS), resample(&figure(2), COUNT_PTS), resample(&figure(3), COUNT_PTS)]);
    &all[k.min(3)]
}

fn one_points() -> &'static [[f32; 2]] {
    static ONE: OnceLock<Vec<[f32; 2]>> = OnceLock::new();
    ONE.get_or_init(|| {
        let mut p = resample(&figure(3), 30);
        for i in 0..=10 {
            p.push([0.32 + 0.52 * i as f32 / 10.0, 0.98]);
        }
        p
    })
}

fn unit_hex() -> &'static [[f32; 2]; 6] {
    static HEX: OnceLock<[[f32; 2]; 6]> = OnceLock::new();
    HEX.get_or_init(|| {
        std::array::from_fn(|k| {
            let ang = std::f32::consts::TAU * k as f32 / 6.0;
            [ang.cos(), ang.sin()]
        })
    })
}

fn unit_ring() -> &'static [[f32; 2]; 48] {
    static RING: OnceLock<[[f32; 2]; 48]> = OnceLock::new();
    RING.get_or_init(|| {
        std::array::from_fn(|k| {
            let ang = std::f32::consts::TAU * k as f32 / 48.0;
            [ang.cos(), ang.sin()]
        })
    })
}
