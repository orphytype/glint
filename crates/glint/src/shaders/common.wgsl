// Shared by every Glint shader (prepended): the uniform block, colour helpers and noise.
// One block serves every effect so the compositor has a single upload path; each effect reads
// the few fields it needs. Positions are in pass pixels unless a field says "window".

struct U {
    rect: vec4<f32>,     // the slot, pass pixels: x, y, w, h
    bounds: vec4<f32>,   // the quad that gets drawn, pass pixels: x0, y0, x1, y1
    clip: vec4<f32>,     // the slot's clip rectangle, window pixels
    fade: vec4<u32>,     // packed soft edges and corner radii for clip_alpha
    radii: vec4<f32>,    // corner radii in pixels: tl, tr, br, bl (negative: continuous corners)
    region: vec4<f32>,   // where the blurred copy of the backdrop sits, pass pixels: x0 y0 x1 y1
    view: vec4<f32>,     // pass size (w, h), sRGB-encoded blending (1/0), light theme (1/0)
    misc: vec4<f32>,     // pass origin in the window (x, y), seconds, device pixels per point
    p0: vec4<f32>,       // effect parameters, see each shader
    p1: vec4<f32>,
    p2: vec4<f32>,
    p3: vec4<f32>,
    tint: vec4<f32>,     // an authored sRGB colour and its strength
    ink: vec4<f32>,      // the text colour (sRGB), and the accent's red, green in zw
    p4: vec4<f32>,       // more effect parameters
};

@group(0) @binding(0) var<uniform> u: U;

fn srgb_to_lin(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((max(c, vec3<f32>(0.0)) + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

// An authored sRGB colour in the space the pass blends in.
fn work(c: vec3<f32>) -> vec3<f32> {
    return select(srgb_to_lin(c), c, u.view.z > 0.5);
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn hash21(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 = p3 + dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

// Interleaved gradient noise: a cheap, even dither for hiding banding.
fn ign(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
}

// 8x8 Bayer threshold in 0..1 for an integer cell.
fn bayer8(c: vec2<f32>) -> f32 {
    let x = u32(c.x) & 7u;
    let y = u32(c.y) & 7u;
    var v = 0u;
    // Interleave the bits of x ^ y and y, the standard construction.
    let a = x ^ y;
    v = ((a & 1u) << 5u) | ((y & 1u) << 4u) | (((a >> 1u) & 1u) << 3u) | (((y >> 1u) & 1u) << 2u) | (((a >> 2u) & 1u) << 1u) | ((y >> 2u) & 1u);
    return (f32(v) + 0.5) / 64.0;
}

fn smoother(x: f32) -> f32 {
    let t = clamp(x, 0.0, 1.0);
    return t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
}

struct VOut {
    @builtin(position) pos: vec4<f32>,
};

// A quad over `bounds`.
@vertex
fn vs(@builtin(vertex_index) vi: u32) -> VOut {
    let c = vec2<f32>(f32(vi & 1u), f32(vi >> 1u));
    let p = mix(u.bounds.xy, u.bounds.zw, c);
    var o: VOut;
    o.pos = vec4<f32>(p.x / u.view.x * 2.0 - 1.0, 1.0 - p.y / u.view.y * 2.0, 0.0, 1.0);
    return o;
}
