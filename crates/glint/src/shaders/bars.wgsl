// ---- bars: the seek line -----------------------------------------------------------------------

fn band(i: i32) -> f32 {
    let j = clamp(i, 0, 11);
    if (j < 4) { return u.p0[j]; }
    if (j < 8) { return u.p1[j - 4]; }
    return u.p2[j - 8];
}

fn bar_noise(x: f32, seed: f32) -> f32 {
    let i = floor(x);
    let f = fract(x);
    let a = hash21(vec2<f32>(i, seed));
    let b = hash21(vec2<f32>(i + 1.0, seed));
    let s = f * f * (3.0 - 2.0 * f);
    return mix(a, b, s);
}

fn wave_at(row: i32, a: f32, b: f32) -> f32 {
    let i0 = i32(floor(a * 256.0));
    let i1 = max(i32(ceil(b * 256.0)), i0 + 1);
    var sum = 0.0;
    var c = 0.0;
    for (var i = i0; i < min(i1, i0 + 8); i = i + 1) {
        let j = clamp(i, 0, 255);
        sum = sum + textureLoad(pyr, vec2<i32>(j / 4, row), 0)[j % 4];
        c = c + 1.0;
    }
    return sum / max(c, 1.0);
}

fn wave_h(v: f32) -> f32 {
    return 0.06 + 0.48 * clamp(v, 0.0, 1.0);
}

fn bar_wave(idx: f32, seed: f32) -> f32 {
    return 0.15 + 0.32 * (0.72 * bar_noise(idx * 0.18, seed) + 0.28 * hash21(vec2<f32>(idx, seed * 1.7)));
}

@fragment
fn fs_bars(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let k = max(u.misc.w, 0.5);
    let p = frag.xy;
    let local = (p - u.rect.xy) / k;
    let size = u.rect.zw / k;
    let pitch = max(u.p3.x, 2.0);
    let n = max(floor(size.x / pitch), 1.0);
    let step = size.x / n;
    let idx = floor(local.x / step);
    if (idx < 0.0 || idx >= n) { return vec4<f32>(0.0); }
    let cx = (idx + 0.5) * step;

    var h = mix(bar_wave(idx, u.p4.x), bar_wave(idx, u.region.z), smoothstep(0.0, 1.0, u.p4.y));
    let from_real = u.p4.w >= 1.5;
    let real = clamp(u.p4.w - select(0.0, 2.0, from_real), 0.0, 1.0);
    if (real > 0.0 || from_real) {
        let wa = idx / n;
        let wb = (idx + 1.0) / n;
        let now = wave_h(wave_at(0, wa, wb));
        let before = select(h, wave_h(wave_at(1, wa, wb)), from_real);
        h = mix(before, now, smoothstep(0.0, 1.0, real));
    }

    let t = u.p3.w;
    let beat = u.p3.z;
    let energy = clamp(u.region.y * 4.0, 0.0, 1.0);
    let r1 = hash21(vec2<f32>(idx, 3.7));
    let r2 = hash21(vec2<f32>(idx, 9.1));
    let j = i32(idx * 7.0 - floor(idx * 7.0 / 12.0) * 12.0);
    let lift = 1.0 + 2.4 * f32(j) / 11.0;
    let own = 0.5 + 0.5 * sin(t * (3.0 + 5.0 * r2) + 6.2832 * r1);
    let drive = clamp(lift * (0.62 * band(j) + 0.38 * band(j + 5)), 0.0, 1.0);
    let lively = 0.55 + 0.45 * energy;
    let swell = 0.5 + 0.5 * sin(idx * 0.55 - t * 2.6);
    let mult = 0.80 + 0.45 * drive + 0.22 * (own - 0.5) * lively + 0.10 * (swell - 0.5) * lively;
    let dance_made = clamp(h * mult + 0.05 * beat * r2, 0.08, 0.60);
    let stir = 0.94 + 0.12 * drive + 0.08 * (own - 0.5) * lively + 0.04 * (swell - 0.5) * lively;
    let dance_real = clamp(h * stir + 0.02 * beat * r2, 0.06, 0.66);
    let dance = mix(dance_made, dance_real, real);
    h = mix(h, dance, clamp(u.region.w, 0.0, 1.0));

    let max_h = size.y - 2.0;
    let half_h = max(0.5 * h * max_h, u.p3.y * 0.5);
    let half_w = u.p3.y * 0.5;
    let q = vec2<f32>(local.x - cx, local.y - size.y * 0.5);
    let dd = length(max(abs(q) - vec2<f32>(0.0, half_h - half_w), vec2<f32>(0.0))) - half_w;
    let cover = clamp(0.5 - dd * k, 0.0, 1.0);
    if (cover <= 0.001) { return vec4<f32>(0.0); }

    let head = u.region.x * n;
    let played = 1.0 - smoothstep(head - 0.5, head + 0.5, idx + 0.5);
    let col = mix(work(u.ink.rgb), work(u.tint.rgb), played);
    let a = mix(u.ink.a, 1.0, played) * cover * rect_clip(p) * clamp(u.p4.z, 0.0, 1.0);
    return out_px(col, a);
}
