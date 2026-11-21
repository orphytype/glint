// ---- mood: mood pebbles (20) and pictures (9..15) on Home -------------------------------------

fn sh_poly7(p: vec2<f32>, v: array<vec2<f32>, 7>) -> f32 {
    var vv = v;
    var d = dot(p - vv[0], p - vv[0]);
    var s = 1.0;
    var j = 6;
    for (var i = 0; i < 7; i = i + 1) {
        let e = vv[j] - vv[i];
        let w = p - vv[i];
        let b = w - e * clamp(dot(w, e) / dot(e, e), 0.0, 1.0);
        d = min(d, dot(b, b));
        let c1 = p.y >= vv[i].y;
        let c2 = p.y < vv[j].y;
        let c3 = e.x * w.y > e.y * w.x;
        if ((c1 && c2 && c3) || (!c1 && !c2 && !c3)) {
            s = -s;
        }
        j = i;
    }
    return s * sqrt(d);
}

fn sh_heart(q0: vec2<f32>, seed: f32, T: f32) -> f32 {
    let lean = (sh_h(seed, 80.0) - 0.5) * 0.14;
    let uneven = (sh_h(seed, 81.0) - 0.5) * 0.08;
    var q = q0;
    q.x = q.x * (1.0 + select(-uneven, uneven, q.x < 0.0)) + lean * (q.y + 1.0) * 0.5;
    let s = 0.56 + 0.04 * sh_h(seed, 82.0);
    var p = vec2<f32>(abs(q.x) * s, -q.y * s + 0.52);
    var d: f32;
    if (p.y + p.x > 1.0) {
        d = length(p - vec2<f32>(0.25, 0.75)) - 0.35355;
    } else {
        let a = p - vec2<f32>(0.0, 1.0);
        let b = p - 0.5 * max(p.x + p.y, 0.0);
        d = sqrt(min(dot(a, a), dot(b, b))) * sign(p.x - p.y);
    }
    let beat = 0.012 * sin(T * 1.6 + seed);
    let ang = atan2(q.y, q.x);
    return d / s - 0.05 - beat + 0.018 * sin(ang * 5.0 + seed * 3.0 + T * 0.25);
}

fn sh_moon(q: vec2<f32>, seed: f32, T: f32) -> f32 {
    let disc = length(q) - 0.94;
    let off = vec2<f32>(0.50, -0.44) * (0.94 + 0.12 * sh_h(seed, 110.0));
    let bite = length(q - off - 0.015 * vec2<f32>(sin(T * 0.2 + seed), cos(T * 0.17))) - (0.64 + 0.04 * sh_h(seed, 111.0));
    return max(disc, -bite);
}

fn sh_drop(q0: vec2<f32>, seed: f32, T: f32) -> f32 {
    let bend = (sh_h(seed, 120.0) - 0.5) * 0.5;
    let up = clamp((0.4 - q0.y) / 1.4, 0.0, 1.0);
    var p = vec2<f32>(q0.x - bend * up * up + 0.03 * sin(T * 0.8 + seed) * up, 0.36 - q0.y);
    let r1 = 0.60 + 0.06 * sh_h(seed, 121.0);
    let r2 = 0.04;
    let h = 1.30;
    p.x = abs(p.x);
    let b = (r1 - r2) / h;
    let a = sqrt(1.0 - b * b);
    let kk = dot(p, vec2<f32>(-b, a));
    if (kk < 0.0) {
        return length(p) - r1;
    }
    if (kk > a * h) {
        return length(p - vec2<f32>(0.0, h)) - r2;
    }
    return dot(p, vec2<f32>(a, b)) - r1;
}

fn sh_star(q: vec2<f32>, seed: f32, T: f32) -> f32 {
    let a = atan2(q.y, q.x) + 0.04 * sin(T * 0.3 + seed);
    let tri4 = abs(fract(a * 2.0 / 3.1415927) * 2.0 - 1.0);
    let tri8 = abs(fract(a * 2.0 / 3.1415927 + 0.5) * 2.0 - 1.0);
    let core = 0.40 + 0.06 * sh_h(seed, 132.0);
    let big = mix(core, 1.0, pow(1.0 - tri4, 1.3 + sh_h(seed, 130.0)));
    let small = mix(core, 0.62 + 0.12 * sh_h(seed, 131.0), pow(1.0 - tri8, 1.6));
    let r = max(big, small);
    return (length(q) - r) * 0.75;
}

fn sh_sun(q: vec2<f32>, seed: f32, T: f32) -> f32 {
    let n = floor(9.0 + sh_h(seed, 150.0) * 5.0);
    let a = atan2(q.y, q.x) + T * 0.05;
    let tri = abs(fract(a * n / 6.2831853) * 2.0 - 1.0);
    let ray = pow(1.0 - tri, 1.6);
    let r = 0.70 + 0.27 * ray * (0.85 + 0.15 * sin(T * 1.2 + seed));
    return (length(q) - r) * 0.85;
}

fn shape_ext(fam: i32) -> f32 {
    switch (fam) {
        case 9: { return 1.23; }
        case 10: { return 1.17; }
        case 11: { return 0.95; }
        case 12: { return 1.02; }
        case 13: { return 0.88; }
        case 14: { return 1.15; }
        case 15: { return 0.98; }
        case 20: { return 1.06; }
        default: { return 1.045; }
    }
}

fn shape_r(fam: i32, size: f32, k: f32) -> f32 {
    var base = size * 0.42;
    if (fam == 20) {
        base = size * 0.43;
    } else if (fam == 13) {
        base = size * 0.48;
    }
    return min(base, (size * 0.5 - 8.5 * k) / (shape_ext(fam) * 1.04));
}

fn mood_f(fam: i32, q: vec2<f32>, seed: f32, T: f32, amp: f32) -> f32 {
    if (fam == 20) {
        return length(q) - lq_edge(atan2(q.y, q.x), seed, T, amp);
    }
    if (fam == 10 || fam == 14) {
        // Share a single sh_poly7 call site across bolt and crown so FXC only unrolls it once.
        var v: array<vec2<f32>, 7>;
        var qp = q;
        var pad = 0.06;
        if (fam == 10) {
            v = array<vec2<f32>, 7>(
                vec2<f32>(0.08, -1.00), vec2<f32>(0.66, -1.00), vec2<f32>(0.24, -0.16), vec2<f32>(0.70, -0.16),
                vec2<f32>(-0.32, 1.02), vec2<f32>(-0.02, 0.14), vec2<f32>(-0.56, 0.14));
            for (var i = 0; i < 7; i = i + 1) {
                let fi = f32(i);
                v[i] = v[i] + (vec2<f32>(sh_h(seed, 90.0 + fi), sh_h(seed, 100.0 + fi)) - 0.5) * 0.14;
            }
            let zap = smoothstep(0.92, 1.0, sin(T * 0.7 + seed * 2.0));
            let wob = 0.02 * zap * sin(q.y * 23.0 + T * 30.0);
            qp = q + vec2<f32>(wob, 0.0);
            pad = 0.07;
        } else {
            let lift = (sh_h(seed, 140.0) - 0.5) * 0.12;
            v = array<vec2<f32>, 7>(
                vec2<f32>(-0.86, 0.62), vec2<f32>(0.86, 0.62), vec2<f32>(0.98, -0.56 + lift), vec2<f32>(0.46, -0.06),
                vec2<f32>(0.0, -0.82 - lift * 0.5), vec2<f32>(-0.46, -0.06), vec2<f32>(-0.98, -0.56 + lift));
        }
        var d = sh_poly7(qp, v) - pad;
        if (fam == 14) {
            for (var i = 0; i < 3; i = i + 1) {
                let tip = v[2 + 2 * i];
                let r = 0.13 + 0.02 * smoothstep(0.6, 1.0, sin(T * 1.4 - f32(i) * 1.1 + seed));
                d = sh_smin(d, length(q - tip) - r, 0.06);
            }
        }
        return d;
    }
    switch (fam) {
        case 9: { return sh_heart(q, seed, T); }
        case 11: { return sh_moon(q, seed, T); }
        case 12: { return sh_drop(q, seed, T); }
        case 13: { return sh_star(q, seed, T); }
        case 15: { return sh_sun(q, seed, T); }
        default: {}
    }
    return length(q) - 0.98;
}

fn mood_color(q: vec2<f32>, qs: vec2<f32>, f: f32, seed: f32, T: f32, hue: f32, light: bool) -> vec3<f32> {
    let t = T * 0.045;
    let w0 = q * 1.15;
    let o = vec2<f32>(fbm(w0 + vec2<f32>(seed * 1.3 + t, seed * 0.7 - t * 0.8), 3), fbm(w0 + vec2<f32>(-seed * 0.9 - t * 0.7, seed * 1.9 + t), 3)) - 0.5;
    let w = w0 + o * 1.7;
    let n = fbm(w * 1.05 + vec2<f32>(t * 0.6, -t * 0.4), 4);
    var c0 = sh_hsl(hue - 22.0, 0.66, 0.27);
    var c1 = sh_hsl(hue, 0.72, 0.47);
    var c2 = sh_hsl(hue + 30.0, 0.85, 0.68);
    if (light) {
        c0 = sh_hsl(hue - 26.0, 0.62, 0.66);
        c1 = sh_hsl(hue, 0.70, 0.78);
        c2 = sh_hsl(hue + 34.0, 0.78, 0.88);
    }
    var col = mix(c0, c1, smoothstep(0.28, 0.55, n));
    col = mix(col, c2, smoothstep(0.56, 0.82, n));
    let m = fbm(w * 1.6 - vec2<f32>(t * 0.9, t * 0.5) + vec2<f32>(seed), 3);
    let fold = smoothstep(0.50, 0.70, m) * (1.0 - smoothstep(0.70, 0.86, m));
    let crest = pow(1.0 - abs(sin((m * 1.4 + n * 0.5) * 6.2832)), 3.0) * smoothstep(0.45, 0.65, m);
    let shine = (fold * 0.6 + crest * 0.4) * 0.35;
    let glint = select(mix(c2, vec3<f32>(1.0), 0.5), vec3<f32>(1.0), light);
    col = col + glint * shine * select(0.6, 0.35, light);
    let inner = exp(-dot(qs - vec2<f32>(-0.12, -0.18), qs - vec2<f32>(-0.12, -0.18)) * 2.2);
    col = col + c2 * inner * select(0.18, 0.10, light);
    let lgt = dot(qs, normalize(vec2<f32>(-0.6, -0.8)));
    col = col * (0.88 + 0.16 * lgt);
    let rim = smoothstep(-0.20, 0.0, f);
    col = mix(col, select(c2 * 1.15, vec3<f32>(1.0), light), rim * select(0.30, 0.40, light));
    let hq = sh_rot(qs - vec2<f32>(-0.38, -0.46), 0.65) * vec2<f32>(1.0, 1.9);
    let hd = length(hq);
    col = col + vec3<f32>(1.0) * (smoothstep(0.34, 0.0, hd) * 0.16 + smoothstep(0.10, 0.0, hd) * 0.22) * select(1.0, 0.6, light);
    let mid = smoothstep(0.78, 0.0, length(qs)) * 0.55;
    if (light) {
        col = mix(col, vec3<f32>(1.0), 0.30 * mid);
    } else {
        col = col * mix(1.0, 0.62, mid);
    }
    return clamp(col, vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs_mood(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = frag.xy;
    let k = max(u.misc.w, 1.0);
    let fam = i32(floor(u.p0.x + 0.001));
    let seed = u.p0.y;
    let T = u.p0.w;
    let hov = clamp(u.p1.x, 0.0, 1.0);
    let on = clamp(u.p1.y, 0.0, 1.0);
    let appear = clamp(u.p1.z, 0.0, 1.0);
    let light = u.view.w > 0.5;
    let size = min(u.rect.z, u.rect.w);
    let c = u.rect.xy + u.rect.zw * 0.5;
    let R = shape_r(fam, size, k) * (1.0 + 0.04 * hov);
    var amp = 0.0;
    var turn = 0.0;
    if (fam == 20) {
        amp = 0.45 + 0.2 * hov;
    } else if (fam >= 9 && fam <= 15) {
        turn = 0.035 * sin(T * 0.3 + seed);
    }
    let qs = (p - c) / R;
    let q = sh_rot(qs, turn);
    let f = mood_f(fam, q, seed, T, amp);
    let g = length(vec2<f32>(dpdx(f), dpdy(f)));
    let dpx = f / max(g, 1e-5);
    let cov = clamp(0.5 - dpx, 0.0, 1.0);
    let ring = clamp(1.3 * k - abs(dpx - 4.5 * k) + 0.5, 0.0, 1.0) * on;
    let clip = rect_clip(p) * appear;
    let hue = u.p0.z;
    let ink = u.ink.rgb;
    if ((cov <= 0.0 && ring <= 0.0) || clip <= 0.0) { discard; }
    var col = vec3<f32>(0.0);
    if (cov > 0.0) {
        col = mood_color(q, qs, f, seed, T, hue, light);
        let cell = max(1.0, round(k * clamp(size / k / 70.0, 1.5, 2.5)));
        let thr = bayer8(floor((p - u.rect.xy) / cell));
        col = floor(col * 4.0 + vec3<f32>(thr)) / 4.0;
    }
    let rgb = work(col) * cov + work(ink) * ring * (1.0 - cov);
    let a = max(cov, ring);
    return vec4<f32>(rgb * clip, a * clip);
}
