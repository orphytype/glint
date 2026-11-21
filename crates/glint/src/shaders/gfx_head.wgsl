// Shared bindings and pixel helpers for Glint compositor fragment shaders.
@group(1) @binding(0) var bd: texture_2d<f32>;
@group(1) @binding(1) var pyr: texture_2d<f32>;
@group(1) @binding(2) var smp: sampler;

fn rect_clip(frag: vec2<f32>) -> f32 {
    return clip_alpha(frag + u.misc.xy, u.clip, u.fade);
}

fn out_px(rgb: vec3<f32>, a: f32) -> vec4<f32> {
    return vec4<f32>(rgb * a, a);
}
