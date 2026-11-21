// Cubic B-spline lookup of one blur pyramid level, with four bilinear taps.
fn bspline(t: texture_2d<f32>, uv: vec2<f32>, lvl: i32) -> vec3<f32> {
    let sz = vec2<f32>(textureDimensions(t, lvl));
    let pos = uv * sz - 0.5;
    let i = floor(pos);
    let f = pos - i;
    let f2 = f * f;
    let f3 = f2 * f;
    let w0 = (1.0 - f) * (1.0 - f) * (1.0 - f) / 6.0;
    let w1 = (3.0 * f3 - 6.0 * f2 + 4.0) / 6.0;
    let w2 = (-3.0 * f3 + 3.0 * f2 + 3.0 * f + 1.0) / 6.0;
    let w3 = f3 / 6.0;
    let g0 = w0 + w1;
    let g1 = w2 + w3;
    let h0 = (i - 1.0 + w1 / g0 + 0.5) / sz;
    let h1 = (i + 1.0 + w3 / g1 + 0.5) / sz;
    let l = f32(lvl);
    let a = textureSampleLevel(t, smp, vec2<f32>(h0.x, h0.y), l).rgb;
    let b = textureSampleLevel(t, smp, vec2<f32>(h1.x, h0.y), l).rgb;
    let c = textureSampleLevel(t, smp, vec2<f32>(h0.x, h1.y), l).rgb;
    let d = textureSampleLevel(t, smp, vec2<f32>(h1.x, h1.y), l).rgb;
    return g0.y * (g0.x * a + g1.x * b) + g1.y * (g0.x * c + g1.x * d);
}

// The backdrop blurred by about `sigma` pixels, at pass position `p`.
fn blurred(t: texture_2d<f32>, p: vec2<f32>, sigma: f32) -> vec3<f32> {
    let ruv = clamp((p - u.region.xy) / (u.region.zw - u.region.xy), vec2<f32>(0.0), vec2<f32>(1.0));
    let top = f32(textureNumLevels(t)) - 1.0;
    let l = clamp(log2(max(sigma / 1.58, 1.0)), 0.0, top);
    let a = i32(floor(l));
    let frac = fract(l);
    if (frac < 0.001 || a >= i32(top)) { return bspline(t, ruv, a); }
    return mix(bspline(t, ruv, a), bspline(t, ruv, a + 1), frac);
}

fn sharp(t: texture_2d<f32>, p: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(t, smp, p / u.view.xy, 0.0).rgb;
}

// `sigma` of 0 is the page as drawn; small values fade from it into level 0.
fn soften(t: texture_2d<f32>, tp: texture_2d<f32>, p: vec2<f32>, sigma: f32) -> vec3<f32> {
    if (sigma <= 0.0) { return sharp(t, p); }
    let b = blurred(tp, p, sigma);
    if (sigma >= 1.58) { return b; }
    return mix(sharp(t, p), b, clamp(sigma / 1.58, 0.0, 1.0));
}
