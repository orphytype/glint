//! CPU evaluation of the shape signed-distance fields (used when pre-rendering shapes with
//! GPU shaders disabled).

use super::blobs::Family;

fn fract(x: f32) -> f32 {
    x - x.floor()
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn len(x: f32, y: f32) -> f32 {
    (x * x + y * y).sqrt()
}

/// `hash21` in `common.wgsl`.
fn hash21(x: f32, y: f32) -> f32 {
    let (mut a, mut b, mut c) = (fract(x * 0.1031), fract(y * 0.1031), fract(x * 0.1031));
    let d = a * (b + 33.33) + b * (c + 33.33) + c * (a + 33.33);
    a += d;
    b += d;
    c += d;
    fract((a + b) * c)
}

/// `sh_h`: the seed's number `i`, 0..1.
fn h(seed: f32, i: f32) -> f32 {
    hash21(seed * 0.731 + i * 1.37, i * 7.13 + seed * 0.117)
}

fn smin(a: f32, b: f32, k: f32) -> f32 {
    let t = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
    mix(b, a, t) - k * t * (1.0 - t)
}

fn ell(x: f32, y: f32, rx: f32, ry: f32) -> f32 {
    (len(x / rx, y / ry) - 1.0) * rx.min(ry)
}

fn lq_edge(a: f32, seed: f32, t: f32, amp: f32) -> f32 {
    let w = 0.045 * (3.0 * a + t * 0.42 + seed).sin() + 0.030 * (5.0 * a - t * 0.31 + seed * 1.7).sin() + 0.018 * (7.0 * a + t * 0.55 + seed * 2.3).sin();
    1.0 + amp * w
}

/// A polygon's signed distance, negative inside (`sh_poly7`).
fn poly(px: f32, py: f32, v: &[[f32; 2]]) -> f32 {
    let mut d = (px - v[0][0]).powi(2) + (py - v[0][1]).powi(2);
    let mut s = 1.0;
    let mut j = v.len() - 1;
    for i in 0..v.len() {
        let (ex, ey) = (v[j][0] - v[i][0], v[j][1] - v[i][1]);
        let (wx, wy) = (px - v[i][0], py - v[i][1]);
        let t = ((wx * ex + wy * ey) / (ex * ex + ey * ey)).clamp(0.0, 1.0);
        let (bx, by) = (wx - ex * t, wy - ey * t);
        d = d.min(bx * bx + by * by);
        let (c1, c2, c3) = (py >= v[i][1], py < v[j][1], ex * wy > ey * wx);
        if (c1 && c2 && c3) || (!c1 && !c2 && !c3) {
            s = -s;
        }
        j = i;
    }
    s * d.sqrt()
}

fn heart(x0: f32, y0: f32, seed: f32, t: f32) -> f32 {
    let lean = (h(seed, 80.0) - 0.5) * 0.14;
    let uneven = (h(seed, 81.0) - 0.5) * 0.08;
    let qx = x0 * (1.0 + if x0 < 0.0 { uneven } else { -uneven }) + lean * (y0 + 1.0) * 0.5;
    let qy = y0;
    let s = 0.56 + 0.04 * h(seed, 82.0);
    let (px, py) = (qx.abs() * s, -qy * s + 0.52);
    let d = if py + px > 1.0 {
        len(px - 0.25, py - 0.75) - 0.35355
    } else {
        let m = 0.5 * (px + py).max(0.0);
        let (ax, ay, bx, by) = (px, py - 1.0, px - m, py - m);
        (ax * ax + ay * ay).min(bx * bx + by * by).sqrt() * (px - py).signum()
    };
    let beat = 0.012 * (t * 1.6 + seed).sin();
    d / s - 0.05 - beat + 0.018 * (qy.atan2(qx) * 5.0 + seed * 3.0 + t * 0.25).sin()
}

fn bolt(x: f32, y: f32, seed: f32, t: f32) -> f32 {
    let mut v = [[0.08, -1.00], [0.66, -1.00], [0.24, -0.16], [0.70, -0.16], [-0.32, 1.02], [-0.02, 0.14], [-0.56, 0.14]];
    for (i, p) in v.iter_mut().enumerate() {
        let fi = i as f32;
        p[0] += (h(seed, 90.0 + fi) - 0.5) * 0.14;
        p[1] += (h(seed, 100.0 + fi) - 0.5) * 0.14;
    }
    let zap = smoothstep(0.92, 1.0, (t * 0.7 + seed * 2.0).sin());
    let wob = 0.02 * zap * (y * 23.0 + t * 30.0).sin();
    poly(x + wob, y, &v) - 0.07
}

fn moon(x: f32, y: f32, seed: f32, t: f32) -> f32 {
    let disc = len(x, y) - 0.94;
    let k = 0.94 + 0.12 * h(seed, 110.0);
    let (ox, oy) = (0.50 * k + 0.015 * (t * 0.2 + seed).sin(), -0.44 * k + 0.015 * (t * 0.17).cos());
    let bite = len(x - ox, y - oy) - (0.64 + 0.04 * h(seed, 111.0));
    disc.max(-bite)
}

fn drop(x0: f32, y0: f32, seed: f32, t: f32) -> f32 {
    let bend = (h(seed, 120.0) - 0.5) * 0.5;
    let up = ((0.4 - y0) / 1.4).clamp(0.0, 1.0);
    let px = (x0 - bend * up * up + 0.03 * (t * 0.8 + seed).sin() * up).abs();
    let py = 0.36 - y0;
    let (r1, r2, hh) = (0.60 + 0.06 * h(seed, 121.0), 0.04, 1.30);
    let b = (r1 - r2) / hh;
    let a = (1.0 - b * b).sqrt();
    let kk = -b * px + a * py;
    if kk < 0.0 {
        len(px, py) - r1
    } else if kk > a * hh {
        len(px, py - hh) - r2
    } else {
        a * px + b * py - r1
    }
}

fn star(x: f32, y: f32, seed: f32, t: f32) -> f32 {
    let a = y.atan2(x) + 0.04 * (t * 0.3 + seed).sin();
    let tri4 = (fract(a * 2.0 / std::f32::consts::PI) * 2.0 - 1.0).abs();
    let tri8 = (fract(a * 2.0 / std::f32::consts::PI + 0.5) * 2.0 - 1.0).abs();
    let core = 0.40 + 0.06 * h(seed, 132.0);
    let big = mix(core, 1.0, (1.0 - tri4).powf(1.3 + h(seed, 130.0)));
    let small = mix(core, 0.62 + 0.12 * h(seed, 131.0), (1.0 - tri8).powf(1.6));
    (len(x, y) - big.max(small)) * 0.75
}

fn crown(x: f32, y: f32, seed: f32, t: f32) -> f32 {
    let lift = (h(seed, 140.0) - 0.5) * 0.12;
    let v = [[-0.86, 0.62], [0.86, 0.62], [0.98, -0.56 + lift], [0.46, -0.06], [0.0, -0.82 - lift * 0.5], [-0.46, -0.06], [-0.98, -0.56 + lift]];
    let mut d = poly(x, y, &v) - 0.06;
    for i in 0..3 {
        let tip = v[2 + 2 * i];
        let r = 0.13 + 0.02 * smoothstep(0.6, 1.0, (t * 1.4 - i as f32 * 1.1 + seed).sin());
        d = smin(d, len(x - tip[0], y - tip[1]) - r, 0.06);
    }
    d
}

fn sun(x: f32, y: f32, seed: f32, t: f32) -> f32 {
    let n = (9.0 + h(seed, 150.0) * 5.0).floor();
    let a = y.atan2(x) + t * 0.05;
    let tri = (fract(a * n / std::f32::consts::TAU) * 2.0 - 1.0).abs();
    let ray = (1.0 - tri).powf(1.6);
    let r = 0.70 + 0.27 * ray * (0.85 + 0.15 * (t * 1.2 + seed).sin());
    (len(x, y) - r) * 0.85
}

/// `wave_form`: My Wave in its form, without the pull toward the pointer.
fn wave_form(form: u8, x: f32, y: f32, seed: f32, t: f32, amp: f32) -> f32 {
    let l = len(x, y);
    let a = y.atan2(x);
    let edge = lq_edge(a, seed, t, amp);
    if form == 0 {
        return l - edge;
    }
    let swell = 1.0 + (edge - 1.0) * 0.6;
    let f = if form == 4 {
        let mut s = 0.0;
        let n = 4 + (h(seed, 18.0) * 2.99) as i32;
        for i in 0..n {
            let fi = i as f32;
            let dir = if i % 2 == 0 { 1.0 } else { -1.0 };
            let ang = fi / n as f32 * std::f32::consts::TAU + (h(seed, 40.0 + fi) - 0.5) * 0.9 + dir * 0.18 * (t * (0.05 + 0.05 * h(seed, 50.0 + fi)) + fi).sin();
            let rad = 0.40 + 0.16 * h(seed, 60.0 + fi);
            let rr = (0.36 + 0.10 * h(seed, 70.0 + fi)) * swell;
            let (dx, dy) = (x - ang.cos() * rad, y - ang.sin() * rad);
            s += rr * rr / (dx * dx + dy * dy).max(1e-4);
        }
        ((1.0 - s) * 0.4).max(l - 1.14)
    } else {
        let r = match form {
            1 => {
                let k = (5.0 + h(seed, 4.0) * 3.99).floor();
                let pw = mix(0.7, 1.8, h(seed, 5.0));
                let inner = mix(0.76, 0.84, h(seed, 6.0)) + 0.03 * (t * 0.3 + seed).sin();
                mix(inner, 1.03, ((k * a * 0.5 + t * 0.03).cos().abs() + 1e-6).powf(pw))
            }
            2 => {
                let k = (3.0 + h(seed, 7.0) * 3.99).floor();
                let tw = mix(0.9, 1.8, h(seed, 8.0)) * if h(seed, 9.0) > 0.5 { 1.0 } else { -1.0 };
                let c = 0.5 + 0.5 * (k * (a - tw * l) + t * 0.05).cos();
                mix(0.72, 1.04, (c + 1e-6).powf(1.4))
            }
            3 => {
                let mut r = 0.94;
                for i in 2..6 {
                    let fi = i as f32;
                    let ak = (0.03 + 0.05 * h(seed, fi)) / (fi - 1.0).powf(0.45);
                    let dir = if h(seed, fi + 20.0) > 0.5 { 1.0 } else { -1.0 };
                    let ph = h(seed, fi + 10.0) * std::f32::consts::TAU + dir * t * (0.05 + 0.09 * h(seed, fi + 30.0));
                    r += ak * (fi * a + ph).cos();
                }
                r
            }
            _ => {
                let m = 2.0 * (2.0 + h(seed, 1.0) * 2.99).floor();
                let n1 = mix(1.4, 3.2, h(seed, 2.0));
                let n2 = (mix(1.2, 3.0, h(seed, 3.0)) + 0.30 * (t * 0.19 + seed).sin()).max(0.7);
                let xs = m * a * 0.25;
                let sf = ((xs.cos().abs() + 1e-6).powf(n2) + (xs.sin().abs() + 1e-6).powf(n2)).powf(-1.0 / n1);
                let smax = 1.0f32.max(2.0f32.powf((n2 * 0.5 - 1.0) / n1));
                mix(0.70, 1.03, (sf / smax).clamp(0.0, 1.0))
            }
        };
        l - r * swell
    };
    smin(f, ell(x, y, 0.90, 0.66), 0.16)
}

/// Evaluates a shape's signed distance in its own space (radius 1, y down, negative inside) at rest.
pub fn eval_field(family: Family, form: u8, x: f32, y: f32, seed: f32) -> f32 {
    field(family, form, x, y, seed, 0.0)
}

/// The shape's function in its own space (radius 1, y down), negative inside, as `sh_f` draws it
/// at rest (no hover, no music) at the clock's `t`.
fn field(family: Family, form: u8, x: f32, y: f32, seed: f32, t: f32) -> f32 {
    match family {
        Family::Liquid => wave_form(form, x, y, seed, t, 1.0),
        Family::Pebble => len(x, y) - lq_edge(y.atan2(x), seed, t, 0.45),
        Family::Heart => heart(x, y, seed, t),
        Family::Bolt => bolt(x, y, seed, t),
        Family::Moon => moon(x, y, seed, t),
        Family::Drop => drop(x, y, seed, t),
        Family::Star => star(x, y, seed, t),
        Family::Crown => crown(x, y, seed, t),
        Family::Sun => sun(x, y, seed, t),
        _ => len(x, y) - 0.98,
    }
}
