// Hardware trilinear lookup across the tent-filtered blur pyramid: 1 texture tap instead of
// 8-tap dual-level cubic B-spline, eliminating the main iGPU fragment bottleneck.

// The backdrop blurred by about `sigma` pixels, at pass position `p`.
fn blurred(t: texture_2d<f32>, p: vec2<f32>, sigma: f32) -> vec3<f32> {
    let ruv = clamp((p - u.region.xy) / (u.region.zw - u.region.xy), vec2<f32>(0.0), vec2<f32>(1.0));
    let top = f32(textureNumLevels(t)) - 1.0;
    let l = clamp(log2(max(sigma / 1.58, 1.0)), 0.0, top);
    return textureSampleLevel(t, smp, ruv, l).rgb;
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
