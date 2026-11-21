fn sh_h(seed: f32, i: f32) -> f32 {
    return hash21(vec2<f32>(seed * 0.731 + i * 1.37, i * 7.13 + seed * 0.117));
}

fn sh_rot(q: vec2<f32>, a: f32) -> vec2<f32> {
    let c = cos(a);
    let s = sin(a);
    return vec2<f32>(c * q.x + s * q.y, -s * q.x + c * q.y);
}

fn sh_hsl(h: f32, s: f32, l: f32) -> vec3<f32> {
    let k = (vec3<f32>(0.0, 8.0, 4.0) + fract(h / 360.0) * 12.0) % vec3<f32>(12.0);
    let a = s * min(l, 1.0 - l);
    return vec3<f32>(l) - a * clamp(min(k - 3.0, 9.0 - k), vec3<f32>(-1.0), vec3<f32>(1.0));
}

fn sh_smin(a: f32, b: f32, k: f32) -> f32 {
    let h = clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
    return mix(b, a, h) - k * h * (1.0 - h);
}

fn sh_ell(p: vec2<f32>, r: vec2<f32>) -> f32 {
    return (length(p / r) - 1.0) * min(r.x, r.y);
}

fn sh_seg(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    return length(pa - ba * h);
}

fn lq_edge(a: f32, seed: f32, T: f32, amp: f32) -> f32 {
    let w = 0.045 * sin(3.0 * a + T * 0.42 + seed) + 0.030 * sin(5.0 * a - T * 0.31 + seed * 1.7) + 0.018 * sin(7.0 * a + T * 0.55 + seed * 2.3);
    return 1.0 + amp * w;
}
