fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let w = f * f * (3.0 - 2.0 * f);
    let px = vec4<f32>(i.x, i.x + 1.0, i.x, i.x + 1.0);
    let py = vec4<f32>(i.y, i.y, i.y + 1.0, i.y + 1.0);
    let a = fract(px * 0.1031);
    let b = fract(py * 0.1031);
    let d = a * (b + 33.33) + b * (a + 33.33) + a * (a + 33.33);
    let r = fract(((a + d) + (b + d)) * (a + d));
    return mix(mix(r.x, r.y, w.x), mix(r.z, r.w, w.x), w.y);
}

// Value noise summed over `octaves`, each finer and fainter.
fn fbm(p: vec2<f32>, octaves: i32) -> f32 {
    var a = 0.5;
    var s = 0.0;
    var q = p;
    var n = 0.0;
    for (var i = 0; i < octaves; i = i + 1) {
        s = s + a * vnoise(q);
        n = n + a;
        q = q * 2.03 + vec2<f32>(17.1, 9.2);
        a = a * 0.5;
    }
    return s / n;
}
