// The blur pyramid. Level 0 takes the region of the backdrop copy at half resolution; every
// level after it halves the one before. Each step is a 4x4 tent (1 3 3 1 each way) read with
// `textureLoad`, a smooth, flicker-free blur on its own; the draw shaders then read the levels
// with a cubic B-spline filter, so a wide blur has no blocks.
//
// Not `textureSample`: on some drivers a filtered read of one level of a texture while another of
// its levels is the target comes back as garbage (the glass chips turned green), and plain loads
// of exact texels need no filtering anyway.
//
// `common.wgsl` is prepended (its vertex shader is not used here).

@group(1) @binding(0) var src: texture_2d<f32>;
@group(1) @binding(1) var smp: sampler;

struct FOut {
    @builtin(position) pos: vec4<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> FOut {
    let c = vec2<f32>(f32(vi & 1u), f32(vi >> 1u));
    var o: FOut;
    o.pos = vec4<f32>(c.x * 2.0 - 1.0, 1.0 - c.y * 2.0, 0.0, 1.0);
    return o;
}

// The tent around the 2x2 texels of `src` at `at` (its top-left), clamped to `lim` texels.
fn tent(at: vec2<i32>, lim: vec2<i32>) -> vec4<f32> {
    let w = array<f32, 4>(0.125, 0.375, 0.375, 0.125);
    var c = vec4<f32>(0.0);
    for (var j = 0; j < 4; j++) {
        for (var i = 0; i < 4; i++) {
            let q = clamp(at + vec2<i32>(i - 1, j - 1), vec2<i32>(0), lim - vec2<i32>(1));
            c += textureLoad(src, q, 0) * (w[i] * w[j]);
        }
    }
    return c;
}

// Level 0: the region of the backdrop, in pass pixels, mapped onto this target. The region is
// always exactly twice the target's size, so the target is half the region's resolution.
@fragment
fn fs_down0(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    // Pass pixels to the copy's texels (the same, unless the copy is a different size).
    let lim = vec2<i32>(textureDimensions(src, 0));
    let k = vec2<f32>(lim) / max(u.view.xy, vec2<f32>(1.0));
    let px = u.region.xy + floor(frag.xy) * 2.0;
    var c = tent(vec2<i32>(floor(px * k)), lim);
    c.a = 1.0;
    return c;
}

// Level n: the level before it, which this target is half the size of.
@fragment
fn fs_down(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let lim = vec2<i32>(textureDimensions(src, 0));
    var c = tent(vec2<i32>(floor(frag.xy)) * 2, lim);
    c.a = 1.0;
    return c;
}
