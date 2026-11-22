// ---- wave: My Wave liquid shape on Home (family 19) -------------------------------------------

fn wave_look() -> vec2<i32> {
    let code = i32(floor(fract(u.p0.x) * 64.0));
    return vec2<i32>(code % 8, code / 8);
}

fn wave_cloud(q: vec2<f32>, seed: f32, T: f32, swell: f32) -> f32 {
    let p = q - vec2<f32>(0.0, 0.11);
    let half = vec2<f32>(0.98, 0.23);
    let b = abs(p - vec2<f32>(0.0, 0.23)) - (half - vec2<f32>(half.y));
    var d = length(max(b, vec2<f32>(0.0))) + min(max(b.x, b.y), 0.0) - half.y * swell;
    for (var i = 0; i < 4; i = i + 1) {
        let fi = f32(i);
        var c: vec2<f32>;
        var r: f32;
        if (i == 0) {
            c = vec2<f32>(-0.62, 0.10);
            r = 0.36;
        } else if (i == 1) {
            c = vec2<f32>(-0.18, -0.20);
            r = 0.48;
        } else if (i == 2) {
            c = vec2<f32>(0.36, -0.14);
            r = 0.44;
        } else {
            c = vec2<f32>(0.76, 0.14);
            r = 0.32;
        }
        c = c + (vec2<f32>(sh_h(seed, 81.0 + fi), sh_h(seed, 85.0 + fi)) - 0.5) * vec2<f32>(0.08, 0.06);
        c = c + 0.02 * vec2<f32>(sin(T * 0.11 + fi * 2.1 + seed), cos(T * 0.09 + fi * 1.7));
        r = r * (0.95 + 0.08 * sh_h(seed, 89.0 + fi)) * swell;
        d = sh_smin(d, length(p - c) - r, 0.035);
    }
    return d;
}

fn wave_form(form: i32, q: vec2<f32>, seed: f32, T: f32, amp: f32) -> f32 {
    let len = length(q);
    let a = atan2(q.y, q.x);
    let edge = lq_edge(a, seed, T, amp);
    if (form <= 0) {
        return len - edge;
    }
    let swell = 1.0 + (edge - 1.0) * 0.6;
    if (form == 3) {
        return wave_cloud(q, seed, T, swell);
    }
    var r: f32;
    if (form == 1) {
        let k = floor(5.0 + sh_h(seed, 4.0) * 3.99);
        let pw = mix(0.7, 1.8, sh_h(seed, 5.0));
        let inner = mix(0.76, 0.84, sh_h(seed, 6.0)) + 0.03 * sin(T * 0.3 + seed);
        let c = pow(abs(cos(k * a * 0.5 + T * 0.03)) + 1e-6, pw);
        r = mix(inner, 1.03, c);
    } else {
        let k = floor(3.0 + sh_h(seed, 7.0) * 3.99);
        let tw = mix(0.9, 1.8, sh_h(seed, 8.0)) * select(-1.0, 1.0, sh_h(seed, 9.0) > 0.5);
        let c = 0.5 + 0.5 * cos(k * (a - tw * len) + T * 0.05);
        r = mix(0.72, 1.04, pow(c + 1e-6, 1.4));
    }
    let f = len - r * swell;
    return sh_smin(f, sh_ell(q, vec2<f32>(0.90, 0.66)), 0.16);
}

fn wave_print(print: i32, col: vec3<f32>, p: vec2<f32>, k: f32, size: f32, seed: f32, light: bool) -> vec3<f32> {
    let cell = max(1.0, round(k * clamp(size / k / 120.0, 1.5, 3.0)));
    let lp = p - u.rect.xy;
    if (print == 1) {
        let thr = bayer8(floor(lp / cell));
        return floor(col * 3.0 + vec3<f32>(thr)) / 3.0;
    }
    let tone = clamp(luma(col) * 1.25, 0.0, 1.0);
    let paper = select(col * 0.22, mix(col, vec3<f32>(1.0), 0.80), light);
    let dots = select(min(col * 1.3 + vec3<f32>(0.04), vec3<f32>(1.0)), col * 0.80, light);
    let amount = select(tone, 1.0 - 0.7 * tone, light);
    var m: f32;
    if (print == 2) {
        let pitch = cell * 3.0;
        let gp = sh_rot(lp, 0.52) / pitch;
        let dd = length(fract(gp) - 0.5) * pitch;
        m = clamp(sqrt(amount) * 0.62 * pitch - dd + 0.5, 0.0, 1.0);
    } else {
        let pitch = cell * 2.5;
        let gp = sh_rot(lp, 0.8 + sh_h(seed, 34.0) * 1.4).y / pitch;
        let dd = abs(fract(gp) - 0.5) * pitch;
        m = clamp(amount * 0.5 * pitch - dd + 0.5, 0.0, 1.0);
    }
    return mix(paper, dots, m);
}

@fragment
fn fs_wave(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = frag.xy;
    let k = max(u.misc.w, 1.0);
    let seed = u.p0.y;
    let T = u.p0.w;
    let hov = clamp(u.p1.x, 0.0, 1.0);
    let on = clamp(u.p1.y, 0.0, 1.0);
    let appear = clamp(u.p1.z, 0.0, 1.0);
    let music = clamp(u.p1.w, 0.0, 1.0);
    let light = u.view.w > 0.5;
    let size = min(u.rect.z, u.rect.w);
    let c = u.rect.xy + u.rect.zw * 0.5;
    let R = size * 0.40;
    let amp = 1.0 + 0.5 * music + 0.35 * hov;
    let qs = (p - c) / R;
    let q = qs;
    let ptr = u.p2.yz;
    let pres = clamp(u.p2.w, 0.0, 1.0) * smoothstep(0.0, 0.3, length(ptr));
    let toward = max(dot(normalize(q + vec2<f32>(1e-5)), normalize(ptr + vec2<f32>(1e-5))), 0.0);
    let pull = 0.075 * pres * pow(toward, 5.0) * clamp(length(ptr), 0.0, 1.3);
    let wl = wave_look();
    let f = wave_form(wl.x, q, seed, T, amp) - pull;
    let g = length(vec2<f32>(dpdx(f), dpdy(f)));
    let dpx = f / max(g, 1e-5);
    let cov = clamp(0.5 - dpx, 0.0, 1.0);
    let ring = clamp(1.3 * k - abs(dpx - 4.5 * k) + 0.5, 0.0, 1.0) * on;
    let clip = rect_clip(p) * appear;
    let hue = u.p0.z;
    let ink = u.ink.rgb;

    let room = smoothstep(size * 0.5, size * 0.43, length(p - c));
    let glow = exp(-max(f, 0.0) / 0.075) * (0.30 + 0.35 * music + 0.15 * hov) * room * (1.0 - cov);
    if ((cov <= 0.0 && ring <= 0.0 && glow <= 0.002) || clip <= 0.0) { discard; }
    var col = vec3<f32>(0.0);
    if (cov > 0.0 || hue < 0.0) {
        col = lq_color(true, q, qs, f, seed, T, hue, hov, music, light);
        if (wl.y > 0) {
            col = wave_print(wl.y, col, p, k, size, seed, light);
        } else {
            col = clamp(col + (ign(p) - 0.5) / 255.0, vec3<f32>(0.0), vec3<f32>(1.0));
        }
        // Soft center readability scrim so the title and mood caption stay legible over bright covers.
        let qr = q * vec2<f32>(1.15, 1.85);
        let center = exp(-dot(qr, qr));
        if (light) {
            col = mix(col, vec3<f32>(0.97, 0.96, 0.95), center * 0.34);
        } else {
            col = col * (1.0 - (0.20 + 0.34 * center));
        }
    }
    var gcol = select(sh_hsl(hue + 20.0, 0.75, 0.55), sh_hsl(hue + 20.0, 0.70, 0.62), light);
    if (hue < 0.0) {
        gcol = mix(col, vec3<f32>(luma(col)), 0.2);
    }
    let ga = glow * select(1.0, 0.55, light);
    let rgb = work(col) * cov + work(ink) * ring * (1.0 - cov) + work(gcol) * ga * (1.0 - max(cov, ring));
    let a = max(max(cov, ring), ga);
    return vec4<f32>(rgb * clip, a * clip);
}
