// Glint's compositor shaders, drawn at RUNE effect slots (see fx.rs). `CLIP_WGSL` and
// `common.wgsl` are prepended, so a slot is cut by the same rectangle, feathers and rounded
// corners as the UI around it.
//
//   fs_field   the page's dithered backdrop
//   fs_pattern small dithered tiles (illustration backgrounds)
//   fs_glass   liquid glass: refraction at the rim, chromatic dispersion, specular rim
//   fs_veil    progressive blur at the top and bottom of the page
//   fs_blit    puts the last frame back when nothing has moved since it
//   fs_bars    the seek line, as bars that dance to the music
//   fs_orb     the moods of My Wave, as small spheres of moving colour
//   fs_stage   behind the open player: a burst under the cover and rays driven by the music
//   fs_shape   the garden on Home: shapes of many families, morphing, filled with a dither
//   fs_liquid  the flowing colour: worked out small (fs_liquid_lo), read back at full size

@group(1) @binding(0) var bd: texture_2d<f32>;    // the frame so far, pass-sized
@group(1) @binding(1) var pyr: texture_2d<f32>;   // its blurred pyramid over `region`
@group(1) @binding(2) var smp: sampler;

// ---- blur pyramid ------------------------------------------------------------------------------

// Cubic B-spline lookup of one level, with four bilinear taps.
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

// The backdrop blurred by about `sigma` pixels, at pass position `p`. Level 0 is a blur of
// roughly 1.6 px and every level doubles it.
fn blurred(t: texture_2d<f32>, p: vec2<f32>, sigma: f32) -> vec3<f32> {
    let ruv = clamp((p - u.region.xy) / (u.region.zw - u.region.xy), vec2<f32>(0.0), vec2<f32>(1.0));
    let top = f32(textureNumLevels(t)) - 1.0;
    let l = clamp(log2(max(sigma / 1.58, 1.0)), 0.0, top);
    let a = i32(floor(l));
    let b = min(a + 1, i32(top));
    return mix(bspline(t, ruv, a), bspline(t, ruv, b), fract(l));
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

fn rect_clip(frag: vec2<f32>) -> f32 {
    return clip_alpha(frag + u.misc.xy, u.clip, u.fade);
}

fn out_px(rgb: vec3<f32>, a: f32) -> vec4<f32> {
    return vec4<f32>(rgb * a, a);
}

// ---- field: the page's backdrop ----------------------------------------------------------------

@fragment
fn fs_field(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let k = max(u.misc.w, 0.5);
    let p = frag.xy;
    let uv = (p - u.rect.xy) / max(u.rect.zw, vec2<f32>(1.0));
    let asp = u.rect.z / max(u.rect.w, 1.0);
    let t = u.misc.z * u.p0.y;
    let level = u.p0.z;
    let beat = u.p0.w;
    let canvas = work(u.tint.rgb);
    let ink = work(u.ink.rgb);

    // Two slow pools of light, one high and one low behind the dock, in the page's own grey.
    let c1 = vec2<f32>(0.50 + 0.10 * sin(t * 0.05), -0.10);
    let d1 = (uv - c1) * vec2<f32>(asp * 0.62, 1.0);
    let g1 = exp(-dot(d1, d1) / 0.55);
    let c2 = vec2<f32>(0.5 + 0.18 * sin(t * 0.04 + 1.7), 1.12);
    let d2 = (uv - c2) * vec2<f32>(asp * 0.75, 1.0);
    let g2 = exp(-dot(d2, d2) / (0.30 + 0.05 * beat));
    let g = clamp(g1 * 0.55 + g2 * (0.55 + 0.35 * level), 0.0, 1.0);

    // An ordered dither of that gradient, in cells of about two points: the gradient reads as
    // a fine stipple instead of a smooth fade.
    let cell = max(1.0, round(2.0 * k));
    let thr = bayer8(floor(p / cell));
    let steps = 7.0;
    let lvl = floor(g * steps + thr) / steps;
    let amp = u.p0.x;
    var col = mix(canvas, ink, lvl * amp);
    // A touch of film grain so flat areas are never perfectly flat.
    col = col + (ign(p + vec2<f32>(floor(t * 8.0) * 17.0, 3.0)) - 0.5) * (1.5 / 255.0);
    return out_px(col, rect_clip(p));
}

// ---- pattern: dithered tiles -------------------------------------------------------------------

fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let w = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash21(i), hash21(i + vec2<f32>(1.0, 0.0)), w.x), mix(hash21(i + vec2<f32>(0.0, 1.0)), hash21(i + vec2<f32>(1.0, 1.0)), w.x), w.y);
}

fn pattern_value(id: i32, uv: vec2<f32>, asp: f32, t: f32, seed: f32) -> f32 {
    let q = vec2<f32>(uv.x * asp, uv.y);
    if (id == 0) {
        // A light from the lower left.
        let d = length((uv - vec2<f32>(0.12, 1.05)) * vec2<f32>(asp, 1.0));
        return clamp(1.15 - d * 0.95, 0.0, 1.0) * (0.85 + 0.15 * sin(t * 0.6 + seed));
    }
    if (id == 1) {
        // Long waves.
        let w = 0.5 + 0.5 * sin(q.x * 5.0 + t * 0.7 + seed + 1.6 * sin(q.y * 3.2 - t * 0.35));
        return w * smoothstep(1.05, 0.15, uv.y) * 0.95;
    }
    if (id == 2) {
        // Rings.
        let r = length((uv - vec2<f32>(0.5, 0.55)) * vec2<f32>(asp, 1.0));
        return (0.5 + 0.5 * sin(r * 19.0 - t * 1.4 + seed)) * smoothstep(0.95, 0.1, r);
    }
    if (id == 3) {
        // A diagonal sweep.
        return clamp((uv.x * 0.9 + (1.0 - uv.y) * 0.7 - 0.25 + 0.12 * sin(t * 0.4 + seed)) * 0.8, 0.0, 1.0);
    }
    if (id == 4) {
        // Drifting clouds.
        let n = vnoise(q * 3.2 + vec2<f32>(t * 0.12, seed * 7.0)) * 0.65 + vnoise(q * 7.0 - vec2<f32>(0.0, t * 0.2)) * 0.35;
        return clamp((n - 0.25) * 1.7, 0.0, 1.0);
    }
    // Halftone dots that grow toward a corner.
    let cell = fract(q * 7.0) - 0.5;
    let big = clamp(1.05 - length(uv - vec2<f32>(0.85, 0.2)) * 1.1, 0.0, 1.0);
    return smoothstep(0.5, 0.0, length(cell) / (0.12 + 0.5 * big)) * (0.35 + 0.65 * big);
}

@fragment
fn fs_pattern(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let k = max(u.misc.w, 0.5);
    let p = frag.xy;
    let local = p - u.rect.xy - u.rect.zw * 0.5;
    let cov = clamp(0.5 - sd_rounded(local, u.rect.zw * 0.5, u.radii), 0.0, 1.0) * rect_clip(p);
    if (cov <= 0.0) { discard; }
    if (u.p0.x < -0.5) {
        // A skeleton: a soft band of light, tilted, that crosses the whole window from the top
        // left to the bottom right in 1.3 s and then rests 0.7 s. Measured in the window's
        // points, so every block on the page is lit by the same ray as it passes.
        let pt = p / k;
        let span = (u.view.x + u.view.y * 0.6) / k;
        let along = pt.x + pt.y * 0.6;
        let cyc = fract(u.misc.z / 2.0) * 2.0;
        let head = mix(-260.0, span + 260.0, clamp(cyc / 1.3, 0.0, 1.0));
        let z = (along - head) / 150.0;
        let ray = exp(-z * z * 2.2) * step(cyc, 1.3);
        return out_px(work(u.ink.rgb), u.ink.a * ray * cov);
    }
    let uv = (p - u.rect.xy) / max(u.rect.zw, vec2<f32>(1.0));
    let cell = max(2.0, round(u.p1.x * k));
    // Sample the pattern at the cell's centre so each dot is one flat square.
    let cc = (floor(p / cell) + 0.5) * cell;
    let v = pattern_value(i32(u.p0.x + 0.5), (cc - u.rect.xy) / max(u.rect.zw, vec2<f32>(1.0)), u.rect.z / max(u.rect.w, 1.0), u.misc.z * u.p0.w, u.p0.y);
    let thr = bayer8(floor(p / cell));
    let lit = select(0.0, 1.0, v * u.p0.z > thr);
    let ink = work(u.ink.rgb);
    return out_px(ink, lit * cov * u.p1.y);
}

// ---- glass -------------------------------------------------------------------------------------

fn sdq(q: vec2<f32>) -> f32 {
    return sd_rounded(q, u.rect.zw * 0.5, u.radii);
}

fn grade(c: vec3<f32>) -> vec3<f32> {
    var col = c;
    let l = luma(col);
    col = mix(vec3<f32>(l), col, u.p1.y);
    let wp = u.p1.x;
    if (wp != 0.0) {
        col = mix(col, select(vec3<f32>(0.0), vec3<f32>(1.0), wp > 0.0), abs(wp));
    }
    if (u.p1.z != 0.0) {
        col = clamp((col - 0.5) * (1.0 + u.p1.z) + 0.5, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    return col;
}

@fragment
fn fs_glass(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let k = max(u.misc.w, 0.5);
    let p = frag.xy;
    let half = u.rect.zw * 0.5;
    let q = p - u.rect.xy - half;
    let sd = sdq(q);
    let clipped = rect_clip(p);
    let cov = clamp(0.5 - sd, 0.0, 1.0) * clipped;

    // A soft shadow under the glass: the quad is the rect plus the shadow's reach, and the
    // shadow sits a little below the body.
    let reach = max(u.rect.x - u.bounds.x - 1.0, 0.0);
    var drop_a = 0.0;
    if (reach > 1.0 && u.ink.w > 0.0) {
        let d2 = sdq(q - vec2<f32>(0.0, reach * 0.3));
        drop_a = u.ink.w * (1.0 - smoother(d2 / reach)) * clipped * u.p3.y;
    }
    if (cov <= 0.0 && drop_a <= 0.002) { discard; }

    let dist = max(-sd, 0.0);
    // The rim's outward normal, from the distance field itself.
    let e = 1.0;
    var grad = vec2<f32>(sdq(q + vec2<f32>(e, 0.0)) - sdq(q - vec2<f32>(e, 0.0)), sdq(q + vec2<f32>(0.0, e)) - sdq(q - vec2<f32>(0.0, e))) * (0.5 / e);
    let gl = length(grad);
    let n2 = grad / max(gl, 1e-4);

    // Refraction: close to the rim the picture is pulled toward the inside, hard at the edge
    // and fading out over the bevel.
    var disp = vec2<f32>(0.0);
    let width = max(u.p0.y, 1.0);
    if (dist < width && u.p0.x != 0.0) {
        let t = dist / width;
        let cutoff = max(1.0 - t * t, 0.0);
        let profile = exp(-t / 0.225) * cutoff * cutoff;
        let mag = profile * min(abs(u.p0.x), min(u.rect.z, u.rect.w) * 0.5) * sign(u.p0.x);
        disp = -n2 * smoother(gl / 0.5) * mag;
    }
    let rc = p + disp;

    // Dispersion: red and blue are read a little apart along the push, most at the corners.
    let corner = abs((q.x * q.y) / max(half.x * half.y, 1.0));
    let ca = disp * u.p0.z * (0.45 + corner);
    let sigma = u.p0.w;
    var col = vec3<f32>(0.0);
    if (u.p0.z > 0.0) {
        col.r = soften(bd, pyr, rc + ca, sigma).r;
        col.g = soften(bd, pyr, rc, sigma).g;
        col.b = soften(bd, pyr, rc - ca, sigma).b;
    } else {
        col = soften(bd, pyr, rc, sigma);
    }

    col = grade(col);
    col = mix(col, work(u.tint.rgb), u.tint.a);

    // The bevel catches a little more light.
    let bevel = 1.0 - smoother(dist / max(width * 1.2, 1.0));
    col = col * (1.0 + u.p1.w * bevel * bevel * bevel);

    // Specular rim: one highlight facing the light and a softer one opposite, as a glass edge has.
    let edge_w = max(u.p3.w * k, 1.5);
    let edge = 1.0 - smoother(dist / edge_w);
    let ang = u.p3.z;
    let l2 = vec2<f32>(cos(ang), sin(ang));
    let nrm = normalize(vec3<f32>(-n2.x, -n2.y, 1.0));
    let s1 = pow(max(dot(nrm, normalize(vec3<f32>(l2, 1.0))), 0.0), max(u.p2.y, 1.0));
    let s2 = pow(max(dot(nrm, normalize(vec3<f32>(-l2, 1.0))), 0.0), max(u.p2.y, 1.0)) * 0.6;
    let spec = clamp((s1 + s2) * u.p2.x * edge, 0.0, 1.0);
    // A hairline just inside the edge, brighter where the rim faces up.
    let band = 1.0 - smoothstep(max(u.p2.w * k - 1.0, 0.0), max(u.p2.w * k, 0.5), dist);
    let up = clamp(0.5 - 0.5 * n2.y, 0.0, 1.0);
    let rim = clamp(u.p2.z * band * (0.35 + 0.65 * up), 0.0, 1.0);
    let shade = u.p3.x * edge;
    col = mix(col, vec3<f32>(0.0), shade * (1.0 - spec));
    col = mix(col, work(vec3<f32>(1.0)), clamp(spec + rim * (1.0 - spec), 0.0, 1.0));

    // Dither, so a dark glass never bands.
    col = col + (ign(p) - 0.5) * (1.2 / 255.0);
    let a = cov * u.p3.y;
    // Premultiplied: the glass over the shadow, the shadow being black.
    return vec4<f32>(max(col, vec3<f32>(0.0)) * a, a + (1.0 - a) * drop_a);
}

// ---- veil: progressive blur ---------------------------------------------------------------------

// 1 at the very edge, easing to 0 `h` pixels in.
fn veil_mask(d: f32, h: f32) -> f32 {
    if (h <= 0.0) { return 0.0; }
    let t = clamp(1.0 - d / h, 0.0, 1.0);
    // Ease in so the blur starts gently and is strongest right at the edge.
    return t * t * (3.0 - 2.0 * t);
}

@fragment
fn fs_veil(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = frag.xy;
    let top = u.p0.x;
    let bottom = u.p0.y;
    var m = max(veil_mask(p.y - u.rect.y, top), veil_mask(u.rect.y + u.rect.w - p.y, bottom));
    // A whole-slot veil: the strength is minus `top`.
    if (top < 0.0) { m = clamp(-top, 0.0, 1.0); }
    let a = m * rect_clip(p);
    if (a <= 0.001) { discard; }
    let sigma = u.p0.z * m;
    var col = soften(bd, pyr, p, sigma);
    // At the very edge the page also eases into the canvas, so text never meets a hard cut.
    let fade = u.p0.w * m * m * m;
    col = mix(col, work(u.tint.rgb), fade);
    col = col + (ign(p) - 0.5) * (1.2 / 255.0);
    return out_px(col, a);
}

// ---- blit: the last frame again ---------------------------------------------------------------

@fragment
fn fs_blit(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(bd, vec2<i32>(frag.xy), 0);
}

// ---- liquid: the flowing colour behind My Wave and Now Playing -----------------------------------
//
// Two rounds of domain-warped noise fold a field of colour into slow, liquid shapes; the cover's
// five colours (dark to light, p1.xyz, p2.xyz, p3.xyz, tint.rgb, ink.rgb) are laid over it.
//   p0: strength, wash (0 a soft blob in the middle, 1 the whole rectangle), brightness, beat
//   p1.w: seed

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

fn liquid_ramp(x: f32) -> vec3<f32> {
    let s = clamp(x, 0.0, 1.0) * 4.0;
    let c0 = u.p1.xyz;
    let c1 = u.p2.xyz;
    let c2 = u.p3.xyz;
    let c3 = u.tint.rgb;
    let c4 = u.ink.rgb;
    if (s < 1.0) { return mix(c0, c1, s); }
    if (s < 2.0) { return mix(c1, c2, s - 1.0); }
    if (s < 3.0) { return mix(c2, c3, s - 2.0); }
    return mix(c3, c4, s - 3.0);
}

// The colour field at `uv` (0..1 over the slot) for a slot `asp` times as wide as it is tall:
// straight colour in authored sRGB, and its coverage.
fn liquid_rgba(uv: vec2<f32>, asp: f32) -> vec4<f32> {
    let strength = u.p0.x;
    let wash = u.p0.y > 0.5;
    let bright = u.p0.z;
    let beat = u.p0.w;
    let seed = u.p1.w;
    let t = u.misc.z;
    let light = u.view.w;
    let q = (uv - 0.5) * vec2<f32>(asp, 1.0);
    let s2 = vec2<f32>(seed * 3.1, seed * 1.7);
    var col = vec3<f32>(0.0);
    var a = 0.0;

    if (wash) {
        let w = fbm(q * 1.15 + s2 + vec2<f32>(t * 0.20, -t * 0.15), 3);
        let f = fbm(q * 1.45 + 2.6 * vec2<f32>(w, 1.0 - w) + vec2<f32>(-t * 0.11, t * 0.13), 3);
        let vig = smoothstep(1.25, 0.15, length(q * vec2<f32>(0.75, 1.0)));
        let x = clamp((f - 0.5) * 2.6 + 0.50 + vig * 0.22 + beat * 0.05, 0.0, 1.0) * 0.74;
        col = liquid_ramp(x);
        col = mix(vec3<f32>(luma(col)), col, 1.45) * bright;
        col = mix(col, vec3<f32>(1.0), light * 0.55);
        a = strength * (0.55 + 0.45 * vig);
    } else {
        // A living mass: five lobes on orbits of their own merge, pinch off and come back together;
        // rings leave its middle (harder on the beat), a bright seam runs just inside its edge, a
        // glint sits high on the left, and a few motes drift around it, twinkling.
        let r0 = length(q);
        if (r0 > 0.92) { return vec4<f32>(0.0); }
        let breath = 0.5 + 0.5 * sin(t * 0.8 + seed * 2.0);
        let swell = 1.0 + 0.12 * beat + 0.05 * breath;
        // A slow warp, so the common edge of the lobes flows and does not stay round.
        let w1 = vec2<f32>(fbm(q * 1.2 + s2 + vec2<f32>(0.0, t * 0.28), 2), fbm(q * 1.2 + s2 + vec2<f32>(5.2, 1.3 - t * 0.24), 2));
        let qw = q + (w1 - 0.5) * 0.26;
        var field = 0.0;
        for (var i = 0; i < 5; i = i + 1) {
            let fi = f32(i);
            let ph = fi * 1.2566 + seed * 2.3;
            let sp = 0.34 + 0.07 * fi;
            let reach = 0.08 + 0.10 * (0.5 + 0.5 * sin(t * (0.19 + 0.04 * fi) + fi * 2.1));
            let c = vec2<f32>(cos(t * sp + ph) * 1.4, sin(t * sp * 0.8 + ph * 1.7)) * reach;
            let sg = (0.132 + 0.02 * sin(t * 0.55 + fi * 1.9)) * swell;
            let d = qw - c;
            field = field + exp(-dot(d, d) / (2.0 * sg * sg));
        }
        var ring = 0.0;
        for (var k = 0; k < 2; k = k + 1) {
            let ph = fract(t * 0.13 + 0.5 * f32(k) + seed * 0.37);
            let x = (r0 - (0.16 + 0.34 * ph)) * 15.0;
            ring = ring + exp(-x * x) * (1.0 - ph) * (1.0 - ph);
        }
        ring = ring * (0.35 + 0.65 * beat) * 0.5;
        var motes = 0.0;
        for (var i = 0; i < 7; i = i + 1) {
            let fi = f32(i);
            let dir = select(1.0, -1.0, i % 2 == 1);
            let an = t * (0.10 + 0.017 * fi) * dir + fi * 0.9 + seed;
            let rad = 0.30 + 0.12 * fract(fi * 0.618) + 0.03 * sin(t * 0.6 + fi);
            let dm = q - vec2<f32>(cos(an) * 1.25, sin(an)) * rad;
            motes = motes + exp(-dot(dm, dm) / 0.00045) * (0.35 + 0.65 * (0.5 + 0.5 * sin(t * 1.7 + fi * 2.3)));
        }
        let m = smoothstep(0.10, 1.05, field);
        let ang = atan2(q.y, q.x);
        let core = smoothstep(0.15, 2.4, field);
        let swirl = 0.14 * sin(ang * 2.0 - t * 0.5 + field * 2.2 + seed) * smoothstep(0.1, 0.7, field) * smoothstep(0.02, 0.22, r0);
        let x = clamp(0.06 + 0.20 * light + 0.66 * core + swirl + 0.05 * beat, 0.0, 0.88);
        var body = liquid_ramp(x);
        body = mix(vec3<f32>(luma(body)), body, 1.45) * bright;
        let seam = exp(-pow((field - 0.62) * 3.0, 2.0));
        let gd = q - vec2<f32>(-0.10, -0.15);
        let glint = exp(-dot(gd, gd) / 0.012) * smoothstep(0.5, 1.5, field);
        body = body + u.ink.rgb * (seam * 0.18 + glint * 0.20 + ring * 0.40);
        let spark = mix(liquid_ramp(0.8), u.ink.rgb, 0.5) * bright;
        // The slot's edges are no place for it to end: it is gone before it gets there.
        let win = smoothstep(0.50, 0.32, abs(q.y)) * smoothstep(0.5 * asp, 0.5 * asp - 0.18, abs(q.x));
        let ab = pow(m, 0.9) * strength * win;
        let ae = clamp(ring * 0.55 + motes * 0.5, 0.0, 1.0) * strength * win;
        a = ab + ae * (1.0 - ab);
        col = (body * ab + spark * ae * (1.0 - ab)) / max(a, 0.001);
        col = mix(col, vec3<f32>(1.0), light * 0.30);
    }
    return vec4<f32>(clamp(col, vec3<f32>(0.0), vec3<f32>(1.0)), clamp(a, 0.0, 1.0));
}

// ---- contour: the music drawn as a map ----------------------------------------------------------
//
// The liquid slot's other look (its small texture, its clock): the contour lines of a slow
// landscape instead of a fill. The land rises to a peak at a source point and rolls away from it;
// as the clock runs the lines travel out of the peak (faster with the music, in a surge on every
// beat), every fifth line is drawn heavier as on a survey map, and the lines near the peak take
// the accent while the rest stay in the ink.
//   p0: strength, mode (2 soft edges, 3 a disc, 4 the whole slot), glow, beat
//   p1: source x, y (0..1 over the slot), lines per unit of height, seed
//   p2: the accent (sRGB)            p3: the ink (sRGB), w: a line's weight in points
//   p4: x the lines' phase (0..5), y how far the peak reaches (slot heights), z how wild the land is,
//       w how strongly the lines are drawn (1 for a backdrop, more for a small mark)
// The small texture keeps four numbers per texel: the height, the heat (nearness to the peak), the
// coverage, and a broad nearness that quiets the lines far from the peak, so words over the far
// side of a map stay clean.

fn ct_lo(uv: vec2<f32>, asp: f32) -> vec4<f32> {
    let t = u.misc.z;
    let seed = u.p1.w;
    let mode = u.p0.y;
    let q = (uv - u.p1.xy) * vec2<f32>(asp, 1.0);
    let s2 = vec2<f32>(seed * 3.1, seed * 1.7);
    let reach = max(u.p4.y, 0.05);
    let wild = u.p4.z;
    // The music (contour mode): tint = bass, mid, treble, loudness (each 0..1, stretched to the
    // track); ink = two rings sent out by kicks, as (age in seconds, strength) pairs.
    let bass = u.tint.x;
    let mid = u.tint.y;
    let w = vec2<f32>(fbm(q * 1.1 + s2 + vec2<f32>(0.0, t * 0.07), 3), fbm(q * 1.1 + s2 + vec2<f32>(4.1, 2.7 - t * 0.06), 3));
    // The mids stir the land a little harder.
    let qw = q + (w - 0.5) * (0.35 + 0.35 * wild) * (1.0 + 0.25 * mid);
    let rw = length(qw) / reach;
    // The bass lifts the peak, so its rings push outward with every note of it.
    let hill = exp(-rw * rw * 1.4 / (1.0 + 0.22 * bass));
    let land = fbm(qw * 1.6 + s2 * 0.5 + vec2<f32>(t * 0.02, -t * 0.015), 4);
    var h = hill * (1.3 + 0.45 * bass) + land * (0.5 + 0.6 * wild) - 0.22 * rw;
    var heat = exp(-rw * rw * 2.0) * (1.0 + 0.25 * bass);
    // Each kick: a low ridge that rolls slowly out of the peak and flattens as it goes. The lines
    // bunch up on it and lean to the accent, so the beat is seen travelling across the map without
    // anything flashing.
    for (var k = 0; k < 2; k = k + 1) {
        let age = select(u.ink.x, u.ink.z, k == 1);
        let st = select(u.ink.y, u.ink.w, k == 1);
        if (st > 0.0) {
            let life = clamp(1.0 - age / 2.2, 0.0, 1.0);
            let r0 = 0.12 + age * 0.75;
            let d = (rw - r0) / (0.2 + 0.18 * age);
            let ridge = exp(-d * d) * st * life * life * smoothstep(0.0, 0.25, age);
            h = h + ridge * 0.26;
            heat = heat + ridge * 0.32;
        }
    }
    let near = exp(-rw * rw * 0.16);
    var cover = 1.0;
    if (mode > 2.5 && mode < 3.5) {
        let r = length((uv - 0.5) * vec2<f32>(asp, 1.0));
        cover = smoothstep(0.5, 0.40, r);
    } else if (mode < 2.5) {
        let e = min(min(uv.x, 1.0 - uv.x) * asp, min(uv.y, 1.0 - uv.y));
        cover = smoothstep(0.0, 0.18, e);
    }
    return vec4<f32>(h, heat, cover, near);
}

// The lines, at full size, from the small texture.
fn ct_px(p: vec2<f32>, uv: vec2<f32>) -> vec4<f32> {
    let c = bspline4(pyr, uv);
    let v = c.r * u.p1.z - u.p4.x;
    let dv = max(fwidth(v), 0.0001);
    let fv = abs(fract(v + 0.5) - 0.5);
    let idx = floor(v + 0.5);
    let major = select(0.0, 1.0, abs(idx - 5.0 * floor(idx / 5.0)) < 0.5);
    let bass = u.tint.x;
    let mid = u.tint.y;
    let treb = u.tint.z;
    let loud = u.tint.w;
    let kick = u.p0.w;
    // Heavy lines thicken with the bass; every line a touch on a kick.
    let hw = u.p3.w * u.misc.w * (0.5 + 0.5 * major) * (1.0 + 0.45 * bass * major + 0.1 * kick);
    var line = clamp(hw + 0.5 - fv / dv, 0.0, 1.0);
    // The highs make the lines shimmer: each one breathes on a slow clock of its own (well under
    // one cycle a second, and never all together, so it reads as a glint and not a flicker).
    let ph = fract(sin(idx * 12.9898 + 4.1) * 43758.547);
    let shimmer = 1.0 + treb * 0.3 * sin(u.misc.z * (1.2 + 1.8 * ph) + ph * 6.283);
    // Where the lines crowd into a smear they thin out instead.
    line = line * smoothstep(0.5, 0.16, dv);
    let heat = clamp(c.g, 0.0, 1.0);
    let cover = clamp(c.b, 0.0, 1.0);
    let light = u.view.w;
    let base = (mix(0.16, 0.25, light) + 0.10 * major) * mix(0.32, 1.0, clamp(c.a, 0.0, 1.0));
    let lift = (1.0 + 0.3 * loud) * shimmer * (1.0 + 0.18 * kick * min(heat, 1.0));
    let la = clamp(line * cover * u.p0.x * mix(base, 0.9, min(heat * heat, 1.0)) * lift * u.p4.w, 0.0, 1.0);
    let glow = min(heat * heat, 1.4) * cover * u.p0.x * u.p0.z * (0.08 + 0.08 * mid + 0.03 * kick);
    let lc = mix(u.p3.rgb, u.p2.rgb, smoothstep(0.04, 0.6, heat));
    let a = la + glow * (1.0 - la);
    if (a <= 0.0005) { discard; }
    let col = (lc * la + u.p2.rgb * glow * (1.0 - la)) / max(a, 0.0001);
    var o = work(clamp(col, vec3<f32>(0.0), vec3<f32>(1.0)));
    o = o + (ign(p) - 0.5) * (1.5 / 255.0);
    return out_px(max(o, vec3<f32>(0.0)), a * rect_clip(p));
}

// ---- artmorph: one cover turning into the next -------------------------------------------------
//
// Laid over a cover while the track changes. `bd` is the new cover and `pyr` the last one (the
// sharper copies, 256 texels a side). p0 = progress 0..1, seed. The picture is cut into a fixed
// grid of tiles; a wave crosses it, and as it reaches a tile the old pixels coarsen into blocks
// and drift along a slow flow, then the new ones come in the same way and sharpen. Nothing
// flashes: the colours only ever crossfade, a tile at a time.

@fragment
fn fs_artmorph(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = frag.xy;
    let local = p - u.rect.xy - u.rect.zw * 0.5;
    let cov = clamp(0.5 - sd_rounded(local, u.rect.zw * 0.5, u.radii), 0.0, 1.0) * rect_clip(p);
    if (cov <= 0.0) { discard; }
    let uv = clamp((p - u.rect.xy) / max(u.rect.zw, vec2<f32>(1.0)), vec2<f32>(0.0), vec2<f32>(1.0));
    // Holding (p0.z): the last cover, whole, while the new one is decoded.
    if (u.p0.z > 0.5) {
        return out_px(work(textureSampleLevel(bd, smp, uv, 0.0).rgb), cov);
    }
    let t = clamp(u.p0.x, 0.0, 1.0);
    let seed = u.p0.y;
    // The wave: across the cover in a direction of the track's own, each tile a little early or late.
    let G = 12.0;
    let tile = floor(uv * G);
    let tc = (tile + 0.5) / G;
    let ang = seed * 2.399;
    let dir = vec2<f32>(cos(ang), sin(ang));
    let along = dot(tc - 0.5, dir) / 1.42 + 0.5;
    let start = (0.7 * along + 0.3 * hash21(tile + seed * 7.0)) * 0.45;
    let tb = smoothstep(start, start + 0.45, t);
    // How broken this tile is: not at all before and after, most at its turn.
    let e = sin(3.14159265 * tb);
    // A slow flow carries the pixels while they are loose.
    let fl = vec2<f32>(fbm(uv * 2.6 + vec2<f32>(seed, 1.3), 3), fbm(uv * 2.6 + vec2<f32>(4.7, seed), 3)) - 0.5;
    let q = uv + fl * 0.16 * e;
    // Blocks: 1, 2, 4, 8 or 16 texels a side, coarsest at the tile's turn.
    let bs = exp2(floor(e * 4.0 + 0.5)) / 256.0;
    let qb = (floor(q / bs) + 0.5) * bs;
    let a = textureSampleLevel(pyr, smp, clamp(qb, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb;
    let b = textureSampleLevel(bd, smp, clamp(qb, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb;
    let col = mix(a, b, smoothstep(0.3, 0.7, tb));
    // Over the last moment the layer fades, leaving the real (sharper) new cover underneath.
    let fade = 1.0 - smoothstep(0.9, 1.0, t);
    var o = work(clamp(col, vec3<f32>(0.0), vec3<f32>(1.0)));
    return out_px(o, cov * fade);
}

// The field is smooth, so it is worked out on a small texture (`Pipe::LiquidLo`: `rect` is that
// texture, `region.zw` the slot's real size) and `fs_liquid` only reads it back, at full size.
@fragment
fn fs_liquid_lo(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = frag.xy / max(u.rect.zw, vec2<f32>(1.0));
    if (u.p0.y > 1.5) { return ct_lo(uv, u.region.z / max(u.region.w, 1.0)); }
    let c = liquid_rgba(uv, u.region.z / max(u.region.w, 1.0));
    return vec4<f32>(c.rgb * c.a, c.a);
}

// Cubic B-spline lookup of a four-channel texture (the smooth upscale of the small field).
fn bspline4(t: texture_2d<f32>, uv: vec2<f32>) -> vec4<f32> {
    let sz = vec2<f32>(textureDimensions(t, 0));
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
    let a = textureSampleLevel(t, smp, vec2<f32>(h0.x, h0.y), 0.0);
    let b = textureSampleLevel(t, smp, vec2<f32>(h1.x, h0.y), 0.0);
    let c = textureSampleLevel(t, smp, vec2<f32>(h0.x, h1.y), 0.0);
    let d = textureSampleLevel(t, smp, vec2<f32>(h1.x, h1.y), 0.0);
    return g0.y * (g0.x * a + g1.x * b) + g1.y * (g0.x * c + g1.x * d);
}

@fragment
fn fs_liquid(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = frag.xy;
    let uv = clamp((p - u.rect.xy) / max(u.rect.zw, vec2<f32>(1.0)), vec2<f32>(0.0), vec2<f32>(1.0));
    if (u.p0.y > 1.5) { return ct_px(p, uv); }
    let c = bspline4(pyr, uv);
    let a = clamp(c.a, 0.0, 1.0);
    if (a <= 0.0005) { discard; }
    var col = work(clamp(c.rgb / a, vec3<f32>(0.0), vec3<f32>(1.0)));
    col = col + (ign(p) - 0.5) * (2.0 / 255.0);
    return out_px(max(col, vec3<f32>(0.0)), a * rect_clip(p));
}

// ---- bars: the seek line -----------------------------------------------------------------------
//
// A row of thin vertical bars across the slot, as many as fit at the pitch asked for. Their heights
// are a waveform of the track (a stable pattern from its seed). While music plays every bar moves
// with it, all along the row and not only by the playhead: each listens to its own part of the
// spectrum, breathes on a clock of its own, and a swell runs through the row; a paused track settles
// back into the waveform. The played bars are lit, the rest dim, both in the text colour.
//   rect: the slot            region.x: progress 0..1       region.y: loudness 0..1
//   region.z: the seed        region.w: how much the music moves the bars, 0..1
//   p0..p2: twelve spectrum bands, low to high
//   p3.x: pitch in points     p3.y: a bar's width in points
//   p3.z: the beat 0..1       p3.w: the bars' clock in seconds (it runs with the music)
//   p4.x: the seed the row is changing from    p4.y: how far it has changed, 0..1 (1: done)
//   p4.z: how much of the row has faded in, 0..1
//   tint: the played colour   ink: the unplayed colour (its strength in .a)

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

// The waveform: slow swells and quick differences between neighbours.
fn bar_wave(idx: f32, seed: f32) -> f32 {
    return 0.15 + 0.32 * (0.72 * bar_noise(idx * 0.18, seed) + 0.28 * hash21(vec2<f32>(idx, seed * 1.7)));
}

@fragment
fn fs_bars(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let k = max(u.misc.w, 0.5);
    let p = frag.xy;
    let local = (p - u.rect.xy) / k;                 // in points from the slot's corner
    let size = u.rect.zw / k;
    let pitch = max(u.p3.x, 2.0);
    let n = max(floor(size.x / pitch), 1.0);
    // The bars are spread over the whole width, with half a pitch of room at each end.
    let step = size.x / n;
    let idx = floor(local.x / step);
    if (idx < 0.0 || idx >= n) { return vec4<f32>(0.0); }
    let cx = (idx + 0.5) * step;

    // A new track's waveform grows out of the old one instead of replacing it.
    var h = mix(bar_wave(idx, u.p4.x), bar_wave(idx, u.region.z), smoothstep(0.0, 1.0, u.p4.y));

    // The music, over the whole row. Neighbouring bars listen to bands far apart, and each one's
    // own clock keeps two bars on the same band from moving alike; the treble is lifted, since a
    // spectrum falls away toward the high end.
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
    // Each bar is its waveform height times a factor that follows its band, its own clock and
    // the swell running through the row, with a kick on the beat.
    let mult = 0.82 + 0.70 * drive + 0.26 * (own - 0.5) * lively + 0.12 * (swell - 0.5) * lively;
    let dance = clamp(h * mult + 0.07 * beat * r2, 0.10, 0.66);
    h = mix(h, dance, clamp(u.region.w, 0.0, 1.0));

    let max_h = size.y - 2.0;
    let half_h = max(0.5 * h * max_h, u.p3.y * 0.5);
    let half_w = u.p3.y * 0.5;
    let q = vec2<f32>(local.x - cx, local.y - size.y * 0.5);
    // A capsule: a bar as wide as `half_w` twice, as tall as `half_h` twice, rounded all round.
    let dd = length(max(abs(q) - vec2<f32>(0.0, half_h - half_w), vec2<f32>(0.0))) - half_w;
    let cover = clamp(0.5 - dd * k, 0.0, 1.0);
    if (cover <= 0.001) { return vec4<f32>(0.0); }

    // Played or not, with a soft step across the bar at the playhead.
    let head = u.region.x * n;
    let played = 1.0 - smoothstep(head - 0.5, head + 0.5, idx + 0.5);
    let col = mix(work(u.ink.rgb), work(u.tint.rgb), played);
    let a = mix(u.ink.a, 1.0, played) * cover * rect_clip(p) * clamp(u.p4.z, 0.0, 1.0);
    return out_px(col, a);
}

// ---- morph: the playing track's marker ---------------------------------------------------------
//
// A rounded square that turns into a circle and back while it makes a quarter turn, then starts
// over (a square looks the same every quarter turn, so the loop has no seam). Each half is eased
// in and out on its own, as a CSS keyframe animation does.
//   p0.x: the clock the marker runs on, in seconds (it only advances while music plays)
//   p0.y: seconds for one loop
//   tint.rgb: its colour

fn ease_io(x: f32) -> f32 {
    let t = clamp(x, 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

fn sd_rbox(q: vec2<f32>, h: f32, r: f32) -> f32 {
    let d = abs(q) - vec2<f32>(h - r);
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0) - r;
}

@fragment
// The playing marker: three rounded bars, the bass, the middle and the highs of the music
// (p1, each stretched over what the track uses), each with a little sway of its own so a steady
// note still breathes. p0.z is how far they are up: at 0 they are three still dots.
fn fs_morph(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = frag.xy;
    let size = min(u.rect.z, u.rect.w);
    let c = u.rect.xy + u.rect.zw * 0.5;
    let bw = size * 0.16;
    let gap = size * 0.28;
    let tall = size * 0.66;
    let run = clamp(u.p0.z, 0.0, 1.0);
    let t = u.p0.x;
    var lv = vec3<f32>(u.p1.x, u.p1.y, u.p1.z);
    var a = 0.0;
    for (var i = 0; i < 3; i = i + 1) {
        let fi = f32(i);
        let sway = 0.5 + 0.5 * sin(t * (4.3 + 1.9 * fi) + fi * 2.4);
        let level = clamp(0.72 * lv[i] + 0.28 * sway, 0.0, 1.0);
        let h = mix(bw, mix(tall * 0.34, tall, level), run);
        let q = vec2<f32>(p.x - (c.x + (fi - 1.0) * gap), p.y - c.y);
        let yy = max(abs(q.y) - (h - bw) * 0.5, 0.0);
        let d = length(vec2<f32>(q.x, yy)) - bw * 0.5;
        a = max(a, clamp(0.5 - d, 0.0, 1.0));
    }
    let cov = a * rect_clip(p);
    if (cov <= 0.0) { discard; }
    return out_px(work(u.tint.rgb), cov);
}

// ---- orbs: the moods of My Wave -------------------------------------------------------------------
//
// Each mood is a small glass-like sphere with its own colours and its own way of moving. The surface
// is flowing colour (warped 3D noise on the sphere, turning, so it rolls), lit from the upper left
// with a soft rim and a highlight; the mood's character is laid over it: a heartbeat, ripples and a
// spark in orbit, rising sparks, quick flames, a slow breath, a bounce, falling drops. The slot is
// larger than the sphere, since the glow of a chosen one needs the room.
//   p0.x: the mood 0..6: favourite, discover, popular, energetic, calm, fun, sad
//   p0.y: how chosen it is, 0..1 (it grows a little, brightens and glows)
//   p0.z: how much of itself it shows when it is not chosen, 0..1
//   p0.w: the clock in seconds (it runs while the page is awake, see `wave_t` in fx.rs)
//   p1.x: how much of it has faded in, 0..1

fn bump(x: f32, c: f32, w: f32) -> f32 {
    let z = (x - c) / w;
    return exp(-z * z);
}

// The orb only needs a soft flowing colour field. Two projected 2D octaves preserve the layered
// look while avoiding the twelve 3D noise evaluations the old surface used for every pixel.
fn orb_fbm2(p: vec2<f32>, seed: f32) -> f32 {
    return 0.68 * vnoise(p + vec2<f32>(seed * 1.7, seed * 0.9))
        + 0.32 * vnoise(p * 2.03 + vec2<f32>(5.3 + seed, 1.7 - seed * 0.4));
}

// The three colours of a mood, dark to light, authored in sRGB.
fn orb_deep(m: i32) -> vec3<f32> {
    if (m == 0) { return vec3<f32>(0.36, 0.03, 0.14); }
    if (m == 1) { return vec3<f32>(0.01, 0.18, 0.27); }
    if (m == 2) { return vec3<f32>(0.40, 0.17, 0.00); }
    if (m == 3) { return vec3<f32>(0.34, 0.05, 0.00); }
    if (m == 4) { return vec3<f32>(0.03, 0.14, 0.32); }
    if (m == 5) { return vec3<f32>(0.24, 0.03, 0.40); }
    return vec3<f32>(0.04, 0.07, 0.17);
}

fn orb_mid(m: i32) -> vec3<f32> {
    if (m == 0) { return vec3<f32>(0.93, 0.13, 0.36); }
    if (m == 1) { return vec3<f32>(0.08, 0.72, 0.78); }
    if (m == 2) { return vec3<f32>(1.00, 0.69, 0.13); }
    if (m == 3) { return vec3<f32>(1.00, 0.35, 0.10); }
    if (m == 4) { return vec3<f32>(0.29, 0.56, 0.90); }
    if (m == 5) { return vec3<f32>(0.77, 0.28, 0.88); }
    return vec3<f32>(0.23, 0.31, 0.56);
}

fn orb_light(m: i32) -> vec3<f32> {
    if (m == 0) { return vec3<f32>(1.00, 0.67, 0.76); }
    if (m == 1) { return vec3<f32>(0.72, 0.97, 1.00); }
    if (m == 2) { return vec3<f32>(1.00, 0.95, 0.69); }
    if (m == 3) { return vec3<f32>(1.00, 0.86, 0.35); }
    if (m == 4) { return vec3<f32>(0.75, 0.90, 1.00); }
    if (m == 5) { return vec3<f32>(1.00, 0.80, 0.96); }
    return vec3<f32>(0.60, 0.68, 0.86);
}

@fragment
fn fs_orb(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = frag.xy;
    let m = clamp(i32(u.p0.x + 0.5), 0, 6);
    let on = clamp(u.p0.y, 0.0, 1.0);
    let vis = mix(clamp(u.p0.z, 0.0, 1.0), 1.0, on);
    let T = u.p0.w;
    let size = min(u.rect.z, u.rect.w);

    // How the mood moves the whole sphere: a swell, a hop, a sag.
    var scale = 1.0 + 0.07 * on;
    var shift = 0.0;                     // up, in radii
    var sq = vec2<f32>(1.0, 1.0);
    var thump = 0.0;
    if (m == 0) {
        let b = fract(T * 0.85);
        thump = bump(b, 0.07, 0.045) + 0.65 * bump(b, 0.27, 0.06);
        scale = scale * (1.0 + 0.055 * thump);
    } else if (m == 4) {
        scale = scale * (1.0 + 0.035 * sin(T * 0.8));
    } else if (m == 5) {
        let h = abs(sin(T * 3.1));
        let land = pow(1.0 - h, 5.0);
        shift = 0.14 * h - 0.03;
        sq = vec2<f32>(1.0 + 0.10 * land, 1.0 - 0.10 * land);
    } else if (m == 6) {
        shift = -0.05 + 0.012 * sin(T * 0.6);
        sq = vec2<f32>(1.02, 0.965);
    }
    let r = size * 0.3056 * scale;
    let c = u.rect.xy + u.rect.zw * 0.5 - vec2<f32>(0.0, shift * r);
    let q = (p - c) / (r * sq);
    let len = length(q);

    let deep = orb_deep(m);
    let mid = orb_mid(m);
    let light = orb_light(m);

    // The glow around it, a little for every mood and a lot for the chosen one.
    let cov = clamp((1.0 - len) * r + 0.5, 0.0, 1.0);
    // It is gone before the slot's edge, so no box shows behind the chosen one.
    let edge = min(min(p.x - u.rect.x, u.rect.x + u.rect.z - p.x), min(p.y - u.rect.y, u.rect.y + u.rect.w - p.y));
    let room = smoothstep(0.0, 0.30 * size, edge);
    let halo = (1.0 - cov) * exp(-max(len - 1.0, 0.0) * 3.4) * (0.07 + 0.50 * on) * vis * room;
    if (len > 1.9 && halo < 0.002) { discard; }

    var col = mid;
    if (cov > 0.0) {
        let z = sqrt(max(1.0 - len * len, 0.0));
        let nrm = vec3<f32>(q.x, q.y, z);
        // The surface rolls: its colour field turns about the vertical axis at the mood's pace.
        var pace = 0.30;
        if (m == 1) { pace = 0.45; }
        if (m == 2) { pace = 0.55; }
        if (m == 3) { pace = 1.5; }
        if (m == 4) { pace = 0.16; }
        if (m == 5) { pace = 0.7; }
        if (m == 6) { pace = 0.13; }
        let ang = T * pace;
        let ca = cos(ang);
        let sa = sin(ang);
        let s3 = vec3<f32>(nrm.x * ca + nrm.z * sa, nrm.y, -nrm.x * sa + nrm.z * ca) * 1.55;
        let seed = f32(m) * 7.3;
        let w = vec3<f32>(
            orb_fbm2(s3.xy + vec2<f32>(T * 0.11, 3.0), seed),
            orb_fbm2(s3.yz + vec2<f32>(2.0 - T * 0.09, 1.0), seed + 7.0),
            orb_fbm2(s3.xz + vec2<f32>(9.0, T * 0.07), seed + 3.0)
        );
        let warped = s3.xy * 1.2 + 2.3 * (w.xy - 0.5);
        let f = orb_fbm2(warped + vec2<f32>(0.0, 5.5), seed + w.z * 4.0);
        let x = clamp((f - 0.26) * 2.1, 0.0, 1.0);
        col = mix(deep, mid, smoothstep(0.0, 0.55, x));
        col = mix(col, light, smoothstep(0.5, 1.0, x) * 0.85);

        // Lit from the upper left, wrapped so the far side is dim but not black.
        let L = normalize(vec3<f32>(-0.45, -0.60, 0.66));
        let diff = clamp(dot(nrm, L) * 0.5 + 0.5, 0.0, 1.0);
        col = col * (0.52 + 0.78 * diff);

        // The mood on top.
        if (m == 0) {
            col = col + light * thump * (1.0 - len) * 0.55;
        } else if (m == 1) {
            let rg = 0.5 + 0.5 * sin(len * 11.0 - T * 2.4);
            col = col + light * pow(rg, 6.0) * (1.0 - len) * 0.30;
            let a = T * 1.3;
            let d = length(q - vec2<f32>(cos(a), sin(a) * 0.8) * 0.62);
            col = col + vec3<f32>(1.0) * exp(-d * d * 90.0) * 0.9;
        } else if (m == 2) {
            for (var i = 0; i < 5; i = i + 1) {
                let fi = f32(i);
                let sx = (hash21(vec2<f32>(fi, 3.1)) * 2.0 - 1.0) * 0.62;
                let ph = fract(T * (0.22 + 0.10 * hash21(vec2<f32>(fi, 7.7))) + hash21(vec2<f32>(fi, 1.3)));
                let rr = 0.045 + 0.05 * hash21(vec2<f32>(fi, 9.9));
                let d = length(q - vec2<f32>(sx + 0.05 * sin(T * 2.0 + fi * 3.0), 0.85 - ph * 1.7));
                col = col + mix(light, vec3<f32>(1.0), 0.5) * smoothstep(rr, rr * 0.35, d) * sin(ph * 3.14159) * 0.85;
            }
        } else if (m == 3) {
            col = col * (0.93 + 0.10 * sin(T * 19.0 + f * 6.0));
            col = col + light * pow(clamp(f * 1.4 - 0.2, 0.0, 1.0), 2.0) * 0.25;
        } else if (m == 4) {
            col = col + light * 0.12 * (0.5 + 0.5 * sin(q.y * 3.0 - T * 0.6)) * z;
        } else if (m == 5) {
            let d = length(q - vec2<f32>(0.34 * sin(T * 1.7), 0.30 * cos(T * 2.3)));
            col = col + light * exp(-d * d * 7.0) * 0.40;
        } else {
            for (var i = 0; i < 3; i = i + 1) {
                let fi = f32(i);
                let sx = (hash21(vec2<f32>(fi, 5.5)) * 2.0 - 1.0) * 0.55;
                let ph = fract(T * (0.16 + 0.07 * hash21(vec2<f32>(fi, 2.2))) + hash21(vec2<f32>(fi, 8.1)));
                let dy = q.y - (-0.9 + ph * 1.8);
                col = col + light * smoothstep(0.035, 0.0, abs(q.x - sx)) * smoothstep(0.30, 0.0, abs(dy)) * 0.30;
            }
            col = col * 0.88;
        }

        // The rim of the glass, and a small bright highlight with a soft broad one behind it.
        col = col + light * pow(1.0 - z, 3.0) * (0.32 + 0.34 * on);
        let hv = normalize(L + vec3<f32>(0.0, 0.0, 1.0));
        col = col + vec3<f32>(1.0) * pow(clamp(dot(nrm, hv), 0.0, 1.0), 48.0) * 0.55;
        col = col + vec3<f32>(1.0) * smoothstep(0.55, 0.0, length(q - vec2<f32>(-0.32, -0.38))) * 0.20;

        col = mix(vec3<f32>(luma(col)), col, 0.80 + 0.35 * on) * (0.62 + 0.38 * vis) * (0.84 + 0.22 * on);
    }
    let rgb = work(clamp(col, vec3<f32>(0.0), vec3<f32>(1.0))) * cov + work(mid) * halo;
    let clip = rect_clip(p) * clamp(u.p1.x, 0.0, 1.0);
    let a = (cov + halo) * clip;
    if (a <= 0.002) { discard; }
    let dither = (ign(p) - 0.5) * (1.5 / 255.0);
    return vec4<f32>(max(rgb * clip + vec3<f32>(dither) * a, vec3<f32>(0.0)), a);
}

// ---- shape: the garden on Home -----------------------------------------------------------------
//
// One shape of a family, morphing slowly and filled with a dither, like a riso print.
//   p0: family, seed, hue in degrees (below 0: the playing cover, `bd`), clock
//   p1: hover 0..1, chosen 0..1, appear 0..1, how loud the music is
// Families: 0 superformula (stars, gems, soft polygons), 1 a blob of a few waves, 2 a flower,
// 3 a butterfly, 4 drops that merge and part, 5 a squircle / ring / arch, 6 a pinwheel,
// 7 a butterfly in flight (it wanders about its slot). Each seed picks the shape's own numbers
// and one of four fills: an ordered dither in three tones, halftone dots, hatching, or op-art
// rings. The edge is worked out from the screen derivatives of the shape's function, so any
// family is smooth at any size, and the chosen one wears a ring a little outside it.

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

// The radius of a polar family at angle `a` (and distance `len`, for the pinwheel's twist).
fn sh_polar(fam: i32, a: f32, len: f32, seed: f32, T: f32) -> f32 {
    if (fam == 0) {
        let m = floor(3.0 + sh_h(seed, 1.0) * 6.0);
        let n1 = mix(0.7, 3.2, sh_h(seed, 2.0));
        let n2 = max(mix(0.7, 3.4, sh_h(seed, 3.0)) + 0.30 * sin(T * 0.19 + seed), 0.45);
        let x = m * a * 0.25;
        let r = pow(pow(abs(cos(x)) + 1e-6, n2) + pow(abs(sin(x)) + 1e-6, n2), -1.0 / n1);
        let rmax = max(1.0, pow(2.0, (n2 * 0.5 - 1.0) / n1));
        return max(r / rmax, 0.28);
    }
    if (fam == 1) {
        var r = 0.0;
        var amp = 0.0;
        for (var i = 2; i < 6; i = i + 1) {
            let fi = f32(i);
            let ak = (0.04 + 0.10 * sh_h(seed, fi)) / pow(fi - 1.0, 0.55);
            let dir = select(-1.0, 1.0, sh_h(seed, fi + 20.0) > 0.5);
            let ph = sh_h(seed, fi + 10.0) * 6.2832 + dir * T * (0.05 + 0.09 * sh_h(seed, fi + 30.0));
            r = r + ak * cos(fi * a + ph);
            amp = amp + ak;
        }
        return 1.0 - amp + r;
    }
    if (fam == 2) {
        let k = floor(4.0 + sh_h(seed, 4.0) * 6.0);
        let p = mix(0.45, 2.2, sh_h(seed, 5.0));
        let inner = mix(0.42, 0.72, sh_h(seed, 6.0)) + 0.04 * sin(T * 0.3 + seed);
        let c = pow(abs(cos(k * a * 0.5 + T * 0.03)) + 1e-6, p);
        return mix(inner, 1.0, c);
    }
    // 6: the pinwheel, its arms bent by how far out they are.
    let k = floor(3.0 + sh_h(seed, 7.0) * 5.0);
    let tw = mix(1.0, 2.4, sh_h(seed, 8.0)) * select(-1.0, 1.0, sh_h(seed, 9.0) > 0.5);
    let c = 0.5 + 0.5 * cos(k * (a - tw * len) + T * 0.05);
    return mix(0.36, 1.0, pow(c + 1e-6, 1.4));
}

// A butterfly, its body along y (head up), its wings folded by `flap` (1 open, small: closed).
fn sh_butterfly(q0: vec2<f32>, seed: f32, flap: f32) -> f32 {
    var q = q0;
    q.x = abs(q.x) / max(flap, 0.10);
    let ur = vec2<f32>(0.44 + 0.06 * sh_h(seed, 12.0), 0.31 + 0.06 * sh_h(seed, 13.0));
    let lr = vec2<f32>(0.27 + 0.06 * sh_h(seed, 14.0), 0.25 + 0.06 * sh_h(seed, 15.0));
    let uw = sh_ell(sh_rot(q - vec2<f32>(0.43, -0.20), -0.50 - 0.2 * sh_h(seed, 16.0)), ur);
    var lw = sh_ell(sh_rot(q - vec2<f32>(0.30, 0.30), 0.55), lr);
    // Some carry tails on the lower wings.
    if (sh_h(seed, 17.0) > 0.5) {
        lw = sh_smin(lw, sh_seg(q, vec2<f32>(0.32, 0.40), vec2<f32>(0.40, 0.78)) - 0.05, 0.06);
    }
    let w = sh_smin(uw, lw, 0.07);
    let body = sh_seg(q0, vec2<f32>(0.0, -0.36), vec2<f32>(0.0, 0.44)) - 0.05;
    let ax = vec2<f32>(abs(q0.x), q0.y);
    let ant = sh_seg(ax, vec2<f32>(0.02, -0.38), vec2<f32>(0.17, -0.70)) - 0.016;
    return min(min(w * max(flap, 0.10), body), ant);
}

// ---- the moods' own shapes (families 9 to 15): each a picture of its mood, a little different
// for every seed (proportions, a lean, a wobble), alive on the clock `T`.

// A polygon's signed distance (inside negative), `n` points of `v`.
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

// A heart: two lobes and a point, the lobes a touch uneven, the point leaning a little.
fn sh_heart(q0: vec2<f32>, seed: f32, T: f32) -> f32 {
    let lean = (sh_h(seed, 80.0) - 0.5) * 0.22;
    let uneven = (sh_h(seed, 81.0) - 0.5) * 0.18;
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
    // A slow beat, and a hand-drawn wobble along the edge.
    let beat = 0.012 * sin(T * 1.6 + seed);
    let ang = atan2(q.y, q.x);
    return d / s - 0.05 - beat + 0.018 * sin(ang * 5.0 + seed * 3.0 + T * 0.25);
}

// A lightning bolt, its corners nudged by the seed.
fn sh_bolt(q: vec2<f32>, seed: f32, T: f32) -> f32 {
    var v = array<vec2<f32>, 7>(
        vec2<f32>(0.08, -1.00), vec2<f32>(0.66, -1.00), vec2<f32>(0.24, -0.16), vec2<f32>(0.70, -0.16),
        vec2<f32>(-0.32, 1.02), vec2<f32>(-0.02, 0.14), vec2<f32>(-0.56, 0.14));
    for (var i = 0; i < 7; i = i + 1) {
        let fi = f32(i);
        v[i] = v[i] + (vec2<f32>(sh_h(seed, 90.0 + fi), sh_h(seed, 100.0 + fi)) - 0.5) * 0.14;
    }
    // A crackle runs along it now and then.
    let zap = smoothstep(0.92, 1.0, sin(T * 0.7 + seed * 2.0));
    let wob = 0.02 * zap * sin(q.y * 23.0 + T * 30.0);
    return sh_poly7(q + vec2<f32>(wob, 0.0), v) - 0.07;
}

// A crescent moon: a full disc with a bite out of it, its thickness and lean the seed's.
fn sh_moon(q: vec2<f32>, seed: f32, T: f32) -> f32 {
    let disc = length(q) - 0.94;
    let off = vec2<f32>(0.40, -0.34) * (0.85 + 0.35 * sh_h(seed, 110.0));
    let bite = length(q - off - 0.015 * vec2<f32>(sin(T * 0.2 + seed), cos(T * 0.17))) - (0.70 + 0.08 * sh_h(seed, 111.0));
    return max(disc, -bite);
}

// A raindrop, its point up and bending a little to one side.
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

// A compass star: four long points, four short ones between them.
fn sh_star(q: vec2<f32>, seed: f32, T: f32) -> f32 {
    let a = atan2(q.y, q.x) + 0.04 * sin(T * 0.3 + seed);
    // Each point a straight-sided spike (a triangle wave around the middle), the long ones
    // and the short ones each a little longer or shorter for every seed.
    let tri4 = abs(fract(a * 2.0 / 3.1415927) * 2.0 - 1.0);
    let tri8 = abs(fract(a * 2.0 / 3.1415927 + 0.5) * 2.0 - 1.0);
    let core = 0.40 + 0.06 * sh_h(seed, 132.0);
    let big = mix(core, 1.0, pow(1.0 - tri4, 1.3 + sh_h(seed, 130.0)));
    let small = mix(core, 0.62 + 0.12 * sh_h(seed, 131.0), pow(1.0 - tri8, 1.6));
    let r = max(big, small);
    return (length(q) - r) * 0.75;
}

// A flame: a drop with licking sides and two or three tongues at the top.
fn sh_flame(q0: vec2<f32>, seed: f32, T: f32) -> f32 {
    // Height along it: 0 at the round foot, 1 at the tip.
    let t = (0.86 - q0.y) / 1.86;
    let tc = clamp(t, 0.0, 1.0);
    // The tip sways and the sides lick upward, more toward the top.
    let sway = 0.16 * tc * tc * sin(T * 1.9 + tc * 4.0 + seed) + (sh_h(seed, 141.0) - 0.5) * 0.3 * tc * tc;
    let x = q0.x - sway;
    let side = select(1.0, -1.0, x < 0.0);
    let lick = 1.0 + 0.16 * tc * sin(tc * 11.0 - T * 4.2 + side * 1.9 + seed);
    // A round foot, widest a third of the way up, narrowing to a point.
    let wide = 0.66 + 0.06 * sh_h(seed, 140.0);
    let w = wide * 1.75 * sqrt(tc) * pow(1.0 - tc, 0.85) * lick;
    let f = abs(x) - w;
    let ends = max(-t, t - 1.0) * 1.86;
    return max(f, ends) * 0.85;
}

// A sun: a round face and rays all round, which slowly turn.
fn sh_sun(q: vec2<f32>, seed: f32, T: f32) -> f32 {
    let n = floor(9.0 + sh_h(seed, 150.0) * 5.0);
    let a = atan2(q.y, q.x) + T * 0.05;
    let tri = abs(fract(a * n / 6.2831853) * 2.0 - 1.0);
    let ray = pow(1.0 - tri, 1.6);
    let r = 0.70 + 0.27 * ray * (0.85 + 0.15 * sin(T * 1.2 + seed));
    return (length(q) - r) * 0.85;
}

// An artist's picture (family 18): nearly a circle, its edge breathing a very little, that
// becomes a rounded square as `sq` goes to 1 (the photo viewer opens it so). The circle is a
// rounded box whose corners are as round as it is wide, so the two are one function.
fn sh_avatar(q: vec2<f32>, seed: f32, T: f32, sq: f32) -> f32 {
    let a = atan2(q.y, q.x);
    // Keep the profile picture a true circle. The organic edge appears only while it expands
    // into the photo viewer, so the opening starts from exactly the shape shown on the profile.
    let w = (0.022 * cos(2.0 * a + seed + T * 0.07) + 0.016 * cos(3.0 * a + seed * 1.7 - T * 0.05)) * sq;
    let cr = mix(1.0, 0.075, sq);
    let b = abs(q) - vec2<f32>(1.0 - cr);
    let bx = length(max(b, vec2<f32>(0.0))) + min(max(b.x, b.y), 0.0) - cr;
    return bx - (w - 0.04) * (1.0 - sq);
}

// The shape's function in its own space, negative inside.
fn sh_f(fam: i32, q: vec2<f32>, seed: f32, T: f32, flap: f32) -> f32 {
    let len = length(q);
    switch (fam) {
        case 18: { return sh_avatar(q, seed, T, fract(u.p0.x)); }
        case 9: { return sh_heart(q, seed, T); }
        case 10: { return sh_bolt(q, seed, T); }
        case 11: { return sh_moon(q, seed, T); }
        case 12: { return sh_drop(q, seed, T); }
        case 13: { return sh_star(q, seed, T); }
        case 14: { return sh_flame(q, seed, T); }
        case 15: { return sh_sun(q, seed, T); }
        default: {}
    }
    if (fam == 0 || fam == 1 || fam == 2 || fam == 6) {
        return len - sh_polar(fam, atan2(q.y, q.x), len, seed, T);
    }
    if (fam == 3 || fam == 7) {
        return sh_butterfly(q, seed, flap);
    }
    if (fam == 4) {
        var s = 0.0;
        let n = 3 + i32(sh_h(seed, 18.0) * 3.0);
        for (var i = 0; i < n; i = i + 1) {
            let fi = f32(i);
            let ang = sh_h(seed, 40.0 + fi) * 6.2832 + T * (0.06 + 0.07 * sh_h(seed, 50.0 + fi)) * select(-1.0, 1.0, i % 2 == 0);
            let rad = 0.18 + 0.30 * sh_h(seed, 60.0 + fi);
            let rr = 0.26 + 0.14 * sh_h(seed, 70.0 + fi);
            let c = vec2<f32>(cos(ang), sin(ang)) * rad;
            let d = q - c;
            s = s + rr * rr / max(dot(d, d), 1e-4);
        }
        return max(1.0 - s, len - 0.98);
    }
    // 5: a squircle, a ring or an arch.
    let v = i32(sh_h(seed, 19.0) * 3.0);
    let n = mix(2.4, 5.0, sh_h(seed, 21.0));
    let asp = vec2<f32>(1.0, mix(0.80, 1.0, sh_h(seed, 22.0)));
    let qa = abs(q) / asp + vec2<f32>(1e-6);
    let sq = pow(pow(qa.x, n) + pow(qa.y, n), 1.0 / n) - 1.0;
    if (v == 1) {
        let hx = abs(q.x - 0.06 * sin(T * 0.2)) + 1e-6;
        let hole = pow(pow(hx, n) + pow(abs(q.y) + 1e-6, n), 1.0 / n) - mix(0.30, 0.48, sh_h(seed, 23.0));
        return max(sq, -hole);
    }
    if (v == 2) {
        // An arch: round on top, square below.
        let w = 0.72;
        let top = length(vec2<f32>(q.x, min(q.y + 0.25, 0.0))) - w;
        let box = max(abs(q.x) - w, abs(q.y - 0.36) - 0.62);
        return select(max(top, -q.y - 0.98), box, q.y > -0.25);
    }
    return sq;
}

// A butterfly the app flies about (family 8). It is seen from above: its wings fold up toward
// each other as it beats them, so each wing narrows across (`open`), the wing on the inside of a
// turn more than the other (`bank`), and the folded part catches less light. The higher it flies
// (`alt`) the further its shadow falls below it, and the softer.
//   p2: heading (radians, the way it travels), open (0..1), bank (-1..1), altitude (0..1)
fn butterfly_px(p: vec2<f32>, k: f32, seed: f32, hue: f32, light: bool) -> vec4<f32> {
    let heading = u.p2.x;
    let open = clamp(u.p2.y, 0.06, 1.0);
    let bank = clamp(u.p2.z, -1.0, 1.0);
    let alt = clamp(u.p2.w, 0.0, 1.0);
    let c = u.rect.xy + u.rect.zw * 0.5;
    // The box leaves room for the shadow: the butterfly itself is a little over half of it.
    let R = min(u.rect.z, u.rect.w) * 0.30;
    let a = heading + 1.5707963;
    let qb = sh_rot((p - c) / R, a);
    let o = select(open * (1.0 - 0.45 * bank), open * (1.0 + 0.45 * bank), qb.x < 0.0);
    let f = sh_butterfly(qb, seed, clamp(o, 0.06, 1.0));
    let g = length(vec2<f32>(dpdx(f), dpdy(f)));
    let dpx = f / max(g, 1e-5);
    let cov = clamp(0.5 - dpx, 0.0, 1.0);
    // The shadow: the same outline further down and to the right, soft with height.
    let off = vec2<f32>(0.3, 0.75) * R * (0.12 + 0.6 * alt);
    let qs = sh_rot((p - off - c) / R, a);
    let os = select(open * (1.0 - 0.45 * bank), open * (1.0 + 0.45 * bank), qs.x < 0.0);
    let fs = sh_butterfly(qs, seed, clamp(os, 0.06, 1.0));
    let gs = length(vec2<f32>(dpdx(fs), dpdy(fs)));
    let soft = (1.0 + 7.0 * alt) * k + R * 0.04 * alt;
    let sh = (1.0 - smoothstep(-soft, soft, fs / max(gs, 1e-5))) * mix(0.30, 0.12, alt);
    if (cov <= 0.0 && sh <= 0.002) { discard; }

    // The wing's tone: light falls from above, so a folded wing is darker; eyespots, a dark rim.
    let qa = vec2<f32>(abs(qb.x) / max(o, 0.08), qb.y);
    var tone = 0.30 + 0.55 * o + 0.18 * (fbm(qa * 2.2 + vec2<f32>(seed), 3) - 0.5);
    let e = length(qa - vec2<f32>(0.50, -0.24));
    tone = tone + 0.5 * smoothstep(0.13, 0.10, e) - 0.6 * smoothstep(0.075, 0.055, e);
    tone = tone - 0.35 * smoothstep(-0.10 * R, -0.03 * R, dpx);
    let body = smoothstep(0.08, 0.04, abs(qb.x)) * step(abs(qb.y + 0.04), 0.5);
    tone = clamp(tone - 0.7 * body, 0.0, 1.0);
    var dark = sh_hsl(hue, 0.45, 0.14);
    var mid = sh_hsl(hue, 0.55, 0.36);
    var hi = sh_hsl(hue, 0.70, 0.64);
    if (light) {
        dark = sh_hsl(hue, 0.45, 0.32);
        mid = sh_hsl(hue, 0.60, 0.55);
        hi = sh_hsl(hue, 0.70, 0.80);
    }
    let cell = max(1.0, round(k * 1.5));
    let thr = bayer8(floor((p - u.rect.xy) / cell));
    let x = tone * 1.999;
    let lvl = floor(x) + step(thr, fract(x));
    let col = select(select(hi, mid, lvl < 1.5), dark, lvl < 0.5);
    let ink = select(vec3<f32>(0.0), vec3<f32>(0.10, 0.08, 0.12), light);
    let rgb = work(col) * cov + work(ink) * sh * (1.0 - cov);
    let alpha = max(cov, sh);
    let clip = rect_clip(p);
    return vec4<f32>(rgb * clip, alpha * clip);
}

// A tile (family 16): a rounded block of the hue as a printed card would carry it: a diagonal
// gradient, a few soft bands of light drifting across like waves, a coarse grain, and all of it
// dithered to a few tones. A hue of 1000 or more is grey.
fn tile_px(p: vec2<f32>, k: f32, seed: f32, hue: f32, T: f32, light: bool) -> vec4<f32> {
    let half = u.rect.zw * 0.5;
    let c = u.rect.xy + half;
    let rad = min(16.0 * k, min(half.x, half.y));
    let qq = abs(p - c) - half + vec2<f32>(rad);
    let d = length(max(qq, vec2<f32>(0.0))) + min(max(qq.x, qq.y), 0.0) - rad;
    let cov = clamp(0.5 - d, 0.0, 1.0);
    if (cov <= 0.0) { discard; }
    let uv = (p - u.rect.xy) / max(u.rect.zw, vec2<f32>(1.0));
    let grey = hue >= 999.0;
    let t = clamp(uv.x * 0.6 + uv.y * 0.4, 0.0, 1.0);
    var la = 0.44;
    var lb = 0.17;
    if (light) { la = 0.80; lb = 0.60; }
    if (grey) {
        la = select(0.25, 0.90, light);
        lb = select(0.18, 0.80, light);
    }
    // Bands of light, each a slow wave across the tile.
    var wave = 0.0;
    for (var i = 0; i < 3; i = i + 1) {
        let fi = f32(i);
        let y0 = 0.22 + 0.27 * fi + 0.09 * sin(uv.x * (3.2 + 1.6 * fi) + seed * 1.3 + fi * 2.1 + T * 0.22);
        let band = (uv.y - y0) / (0.05 + 0.02 * fi);
        wave = wave + exp(-band * band) * (0.11 - 0.03 * fi);
    }
    let cell = max(1.0, round(k * 1.5));
    let cp = floor((p - u.rect.xy) / cell);
    let grain = fract(sin(dot(cp, vec2<f32>(12.9898, 78.233)) + seed * 7.1) * 43758.5453) - 0.5;
    let l = mix(la, lb, t) + wave * select(1.0, 0.8, light) * select(1.0, 0.55, grey) + grain * select(0.07, 0.025, grey);
    let col0 = sh_hsl(hue + 26.0 * t, select(0.58, 0.0, grey), clamp(l, 0.0, 1.0));
    // Dithered to five tones a channel (a grey card, behind words, to finer steps: only a soft print).
    let thr = bayer8(cp);
    let steps = select(4.0, 24.0, grey);
    let x = col0 * steps;
    let col = (floor(x) + step(vec3<f32>(thr), fract(x))) / steps;
    let clip = rect_clip(p);
    return vec4<f32>(work(col) * cov * clip, cov * clip);
}

// A wave card's ground: four ribbons of light, each a gradient of the hue and its neighbours,
// flowing across a deep ground and printed in a dither that is coarse where they shine and fine in
// the dark (so the words on the left stay clean). Under the pointer they quicken, thicken and a
// sheen sweeps over; while the card's wave plays they swell with the music.
fn ribbon_px(p: vec2<f32>, k: f32, seed: f32, packed: f32, T: f32, light: bool, hov: f32, on: f32, music: f32) -> vec4<f32> {
    // The cover's hue, how colourful it is and how light (see `blobs::ribbon`).
    let idx = floor(packed / 360.0 + 0.0001);
    let hue = packed - 360.0 * idx;
    let csat = (idx - 16.0 * floor(idx / 16.0 + 0.0001)) / 15.0;
    let clight = floor(idx / 16.0 + 0.0001) / 15.0;
    let half = u.rect.zw * 0.5;
    let c = u.rect.xy + half;
    let rad = min(14.0 * k, min(half.x, half.y));
    let qq = abs(p - c) - half + vec2<f32>(rad);
    let d = length(max(qq, vec2<f32>(0.0))) + min(max(qq.x, qq.y), 0.0) - rad;
    let cov = clamp(0.5 - d, 0.0, 1.0);
    if (cov <= 0.0) { discard; }
    let size = max(u.rect.zw, vec2<f32>(1.0));
    let uv = (p - u.rect.xy) / size;
    let aspect = size.x / size.y;
    let x = uv.x * aspect;
    // One steady clock (a speed that changed with the pointer would jump the phase of a clock this
    // old); the pointer and the music change only how far and how bright the ribbons go.
    let t = T * 0.42 + seed * 3.1;
    var ground = sh_hsl(hue - 8.0 + 10.0 * uv.x, 0.45 * csat, 0.05 + 0.04 * clight + 0.05 * uv.x * uv.x);
    if (light) { ground = sh_hsl(hue - 8.0 + 10.0 * uv.x, 0.35 * csat, 0.92 - 0.05 * uv.x); }
    // Ribbons as colourful as the cover (grey ones for a grey cover), lighter for a light one.
    let rsat = 0.88 * csat;
    let rl = mix(0.34, 0.60, clight);
    let swell = 1.0 + 0.6 * music * on + 0.18 * hov;
    var glow = vec3<f32>(0.0);
    for (var i = 0; i < 4; i = i + 1) {
        let fi = f32(i);
        let f = 0.85 + 0.42 * fi;
        let y0 = 0.5 + (fi - 1.5) * 0.11
            + 0.15 * swell * sin(x * f + t * (0.7 + 0.13 * fi) + fi * 1.7)
            + 0.04 * sin(x * f * 1.7 + t * 0.5 + fi * 4.0);
        let dy = (uv.y - y0) * size.y / k;
        let thick = 1.6 + 1.1 * fi + 0.9 * hov + 0.8 * music * on;
        let core = exp(-dy * dy / (thick * thick));
        let halo = exp(-abs(dy) / (9.0 + 4.0 * fi)) * 0.32;
        // The cover's own hue, a step either way, lighter toward the right.
        let rc = sh_hsl(hue + (fi - 1.5) * 6.0 + 8.0 * (uv.x - 0.5), rsat, rl + 0.05 * fi + 0.08 * uv.x);
        glow = glow + rc * (core * 0.9 + halo) * (0.5 + 0.13 * fi);
    }
    // A sheen across it now and then, and at once under the pointer.
    let sweep = fract(T * 0.045 + seed * 0.13) * 2.8 - 0.9;
    let sd = (uv.x + (uv.y - 0.5) * 0.4 / aspect - sweep) * aspect * 2.2;
    let sheen = exp(-sd * sd) * (0.05 + 0.16 * hov);
    // Quieter under the words on the left and the arrow on the right.
    let quiet = mix(0.16, 0.85, smoothstep(0.30, 0.78, uv.x)) * (1.0 - 0.62 * smoothstep(0.80, 0.97, uv.x));
    let a = max(glow.r, max(glow.g, glow.b)) * quiet;
    var col = ground + glow * quiet + vec3<f32>(sheen);
    if (light) {
        col = mix(ground, glow / max(a / quiet, 0.001) * 0.85, clamp(a * 0.75, 0.0, 0.8)) - vec3<f32>(sheen * 0.4);
    }
    // A thin lit rim.
    col = col + vec3<f32>(smoothstep(-1.4 * k, 0.0, d) * select(0.07, -0.05, light));
    let cell = max(1.0, round(k * 1.5));
    let cp = floor((p - u.rect.xy) / cell);
    // A still grain (one that changed every frame would crawl through the dither).
    let grain = fract(sin(dot(cp, vec2<f32>(12.9898, 78.233)) + seed * 7.1) * 43758.5453) - 0.5;
    col = clamp(col + grain * 0.02, vec3<f32>(0.0), vec3<f32>(1.0));
    // Coarse steps where it shines, fine ones in the dark.
    let steps = mix(20.0, 6.0, clamp(a * 1.4, 0.0, 1.0));
    let thr = bayer8(cp);
    let q = col * steps;
    col = (floor(q) + step(vec3<f32>(thr), fract(q))) / steps;
    let clip = rect_clip(p);
    return vec4<f32>(work(col) * cov * clip, cov * clip);
}

@fragment
fn fs_shape(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = frag.xy;
    let k = max(u.misc.w, 1.0);
    let fam = i32(floor(u.p0.x + 0.001));
    let seed = u.p0.y;
    let T = u.p0.w;
    let hov = clamp(u.p1.x, 0.0, 1.0);
    let on = clamp(u.p1.y, 0.0, 1.0);
    let appear = clamp(u.p1.z, 0.0, 1.0);
    let music = clamp(u.p1.w, 0.0, 1.0);
    let light = u.view.w > 0.5;
    let size = min(u.rect.z, u.rect.w);
    var c = u.rect.xy + u.rect.zw * 0.5;
    // Room for the ring of the chosen one (little shapes get a ring-sized margin of their own).
    var R = (size * 0.5 - min(7.0 * k, size * 0.07)) / 1.07;
    R = R * (1.0 + 0.045 * hov + 0.025 * on + 0.02 * music) * mix(0.82, 1.0, appear);
    var ang = sh_h(seed, 30.0) * 6.2832 + T * (sh_h(seed, 31.0) - 0.5) * 0.10;
    if (fam >= 9) {
        // A picture stands nearly upright: only a lean of its own (`blobs::lean`, the words on it
        // lean with it), and a slow sway. It sits a little inside its box so the ring fits.
        ang = (fract(seed * 0.618034) - 0.5) * 0.45 + 0.03 * sin(T * 0.3 + seed);
        R = R * 0.86;
    }
    if (fam == 8) {
        return butterfly_px(p, k, seed, u.p0.z, light);
    }
    if (fam == 16) {
        return tile_px(p, k, seed, u.p0.z, T, light);
    }
    if (fam == 17) {
        return ribbon_px(p, k, seed, u.p0.z, T, light, hov, on, music);
    }
    var flap = 1.0;
    if (fam == 18) {
        // A picture fills its box (a little room to lift under the pointer) and stands upright.
        R = size * 0.5 / 1.045 * (1.0 + 0.045 * hov) * mix(0.82, 1.0, appear);
        ang = 0.0;
    }
    if (fam == 3) {
        // Resting: wings open and close now and then, slowly; a quick flutter under the pointer.
        ang = (sh_h(seed, 30.0) - 0.5) * 0.5 + 0.06 * sin(T * 0.4 + seed);
        flap = mix(0.78, 1.0, smoothstep(-0.2, 0.6, sin(T * 0.9 + seed * 3.0)));
        flap = mix(flap, 0.75 + 0.25 * cos(T * 6.0), hov);
    } else if (fam == 7) {
        // In flight: along a slow loop through the slot, facing where it goes, beating in bursts
        // with glides between.
        let ph = seed * 1.7;
        let sp = 0.16 + 0.06 * sh_h(seed, 32.0);
        // Its span (points) rides in the family number's fraction.
        R = fract(u.p0.x) * 1000.0 * k * 0.5 / 0.95;
        let A = max(u.rect.zw * 0.5 - vec2<f32>(R * 1.1), vec2<f32>(0.0));
        let tt = T * sp;
        c = c + A * vec2<f32>(sin(tt + ph), 0.9 * sin(tt * 1.37 + ph * 0.6));
        let v = vec2<f32>(A.x * cos(tt + ph), A.y * 1.233 * cos(tt * 1.37 + ph * 0.6));
        ang = atan2(v.y, v.x) + 1.5707963;
        let burst = smoothstep(-0.3, 0.4, sin(T * 0.7 + seed));
        flap = mix(1.0, 0.25 + 0.75 * abs(cos(T * 5.5 + seed)), burst);
    }
    let qs = (p - c) / R;
    let q = sh_rot(qs, ang);
    let f = sh_f(fam, q, seed, T, flap);
    // Pixels from the edge, from how fast the function changes across the screen.
    let g = length(vec2<f32>(dpdx(f), dpdy(f)));
    let dpx = f / max(g, 1e-5);
    let cov = clamp(0.5 - dpx, 0.0, 1.0);
    let ring = clamp(1.3 * k - abs(dpx - 4.5 * k) + 0.5, 0.0, 1.0) * on;
    let clip = rect_clip(p) * appear;
    if ((cov <= 0.0 && ring <= 0.0) || clip <= 0.0) { discard; }

    // How light each point is, before the dither.
    let len = length(q);
    let lgt = dot(qs, normalize(vec2<f32>(-0.6, -0.8)));
    var tone = 0.42 + 0.26 * lgt + 0.42 * (fbm(q * 1.5 + vec2<f32>(seed * 3.1 + T * 0.05, seed - T * 0.04), 3) - 0.5);
    tone = tone + 0.20 * smoothstep(-9.0 * k, -1.0 * k, dpx);
    if (fam == 2) {
        let a = atan2(q.y, q.x);
        let kk = floor(4.0 + sh_h(seed, 4.0) * 6.0);
        tone = tone + 0.12 * cos(kk * a) - 0.45 * smoothstep(0.24, 0.16, len);
    } else if (fam == 3 || fam == 7) {
        // Eyespots on the upper wings, a dark band along the edge, and the body.
        let bq = vec2<f32>(abs(q.x) / max(flap, 0.10), q.y);
        let e = length(bq - vec2<f32>(0.50, -0.24));
        tone = tone + 0.55 * smoothstep(0.13, 0.10, e) - 0.65 * smoothstep(0.075, 0.055, e);
        tone = tone - 0.35 * smoothstep(-0.10 * R, -0.03 * R, dpx);
        tone = tone - 0.6 * smoothstep(0.08, 0.04, abs(q.x)) * step(abs(q.y), 0.5);
    }
    // Under the words, a little darker, so they read.
    if (fam != 3 && fam != 7) {
        tone = tone - 0.14 * smoothstep(0.65, 0.0, len);
    }
    tone = clamp(tone + 0.06 * hov, 0.0, 1.0);

    let hue = u.p0.z;
    let style = i32(sh_h(seed, 33.0) * 4.0);
    let cell = max(1.0, round(k * clamp(size / k / 120.0, 1.5, 3.0)));
    let cc = floor((p - u.rect.xy) / cell);
    let thr = bayer8(cc);
    var col: vec3<f32>;
    if (hue < -1500.0) {
        // A picture (an artist), as it is: only a soft light from the top left and a rim a touch
        // darker, so it reads as a shape and not a hole cut in the page.
        let uv = clamp((p - c) / (2.0 * R) + 0.5, vec2<f32>(0.0), vec2<f32>(1.0));
        col = textureSampleLevel(bd, smp, uv, 0.0).rgb;
        col = col * (0.97 + 0.06 * lgt) * mix(1.0, 0.86, smoothstep(-10.0 * k, 0.0, dpx));
        col = col + vec3<f32>(0.04 * hov);
    } else if (hue < 0.0) {
        // The playing cover, as a dithered print: each channel in four steps.
        let uv = clamp((p - (u.rect.xy + u.rect.zw * 0.5)) / (2.0 * R) + 0.5, vec2<f32>(0.0), vec2<f32>(1.0));
        // A new cover comes in as the print's own grain: cell by cell, in a wave from one side,
        // the dither's order deciding which cell turns first.
        let mixv = clamp(u.p2.x, 0.0, 1.0);
        let wave = dot(uv - 0.5, normalize(vec2<f32>(cos(seed), sin(seed)))) * 0.5 + 0.5;
        let turn = mixv * 1.6 - wave * 0.6;
        var src = textureSampleLevel(bd, smp, uv, 0.0).rgb;
        if (turn < thr) {
            src = textureSampleLevel(pyr, smp, uv, 0.0).rgb;
        }
        col = floor(src * 3.0 + vec3<f32>(thr)) / 3.0;
        let calm = smoothstep(0.0, 0.85, len);
        if (light) {
            col = mix(vec3<f32>(0.97), col, mix(0.35, 0.85, calm));
        } else {
            col = col * mix(0.28, 0.85, calm);
        }
    } else {
        var dark = sh_hsl(hue, 0.45, 0.13);
        var mid = sh_hsl(hue, 0.50, 0.30);
        var hi = sh_hsl(hue, 0.62, 0.56);
        if (light) {
            dark = sh_hsl(hue, 0.45, 0.50);
            mid = sh_hsl(hue, 0.55, 0.70);
            hi = sh_hsl(hue, 0.65, 0.88);
        }
        if (style == 1) {
            // Halftone dots on a turned grid, bigger where it is lighter.
            let pitch = cell * 3.0;
            let gp = sh_rot(p - u.rect.xy, 0.52) / pitch;
            let dd = length(fract(gp) - 0.5) * pitch;
            let dot_r = sqrt(tone) * 0.62 * pitch;
            let m = clamp(dot_r - dd + 0.5, 0.0, 1.0);
            col = mix(dark, mix(mid, hi, smoothstep(0.5, 0.9, tone)), m);
        } else if (style == 2) {
            // Hatching: lines across the shape, thicker where it is lighter.
            let pitch = cell * 2.5;
            let gp = sh_rot(p - u.rect.xy, 0.8 + sh_h(seed, 34.0) * 1.4).y / pitch;
            let dd = abs(fract(gp) - 0.5) * pitch;
            let m = clamp(tone * 0.5 * pitch - dd + 0.5, 0.0, 1.0);
            col = mix(dark, mix(mid, hi, smoothstep(0.55, 0.95, tone)), m);
        } else {
            var t2 = tone;
            if (style == 3) {
                // Op-art rings that drift out from the middle.
                t2 = mix(tone, 0.5 + 0.5 * sin(len * 16.0 - T * 0.35), 0.45);
            }
            // Three tones, dithered between neighbours.
            let x = clamp(t2, 0.0, 0.999) * 2.0;
            let lvl = floor(x) + step(thr, fract(x));
            col = select(select(hi, mid, lvl < 1.5), dark, lvl < 0.5);
        }
        col = mix(vec3<f32>(luma(col)), col, 1.0 + 0.25 * on);
    }
    let ink = u.ink.rgb;
    let rgb = work(clamp(col, vec3<f32>(0.0), vec3<f32>(1.0))) * cov + work(ink) * ring * (1.0 - cov);
    let a = max(cov, ring) * clip;
    return vec4<f32>(rgb * clip, a);
}

// ---- stage: behind the open player ---------------------------------------------------------------
//
// Rays of light come out from under the cover, each as long as its band of the music is loud
// now; they turn slowly, and the glints along the diagonals flare on the kick. The ground is the
// cover's own colours, drifting very slowly, and around the cover a soft halo takes the hue of
// the harmony.
//
// `bd` is the playing cover (40 texels a side), `pyr` the live readings: eight texels of spectrum
// (32 bands), then the look chosen in the player's panel, then the beat, the pulse and whether
// anything moves, then the low end, the middle, the top and the kick.
//   p0: strength, seed, where the cover's middle is (0..1 of the slot)
//   p1: song clock (s)
//   p2: now: loudness
//   p3: the key (a direction on the circle of fifths: cos, sin), how tonal it is now, the
//       ground's clock
//   region.rgb, p4.rgb, tint.rgb: the cover's palette, dark to light; region.w: how much of the
//   cover itself shows in the ground; tint.w: half the cover's side (px); ink.rgb: the canvas

// The live readings of the music, one row: texels 0..7 the spectrum now (32 bands), texel 8 the
// look chosen in the player's panel (the flash mode and how much the music moves it; each 0..2),
// texel 9 the beat, the pulse and whether anything moves, texel 10 the low end, the middle, the
// top and the kick.
const ST_LIVE: i32 = 0;

fn st_look(i: i32) -> vec4<f32> {
    return textureLoad(pyr, vec2<i32>(8 + i, ST_LIVE), 0);
}

fn st_live(b: f32) -> f32 {
    let bb = clamp(b, 0.0, 31.0);
    let i = i32(floor(bb));
    let j = min(i + 1, 31);
    let a = textureLoad(pyr, vec2<i32>(i / 4, ST_LIVE), 0)[i % 4];
    let c = textureLoad(pyr, vec2<i32>(j / 4, ST_LIVE), 0)[j % 4];
    return mix(a, c, bb - f32(i));
}

fn st_band_of(a: f32) -> f32 {
    return pow(clamp(abs(a) / 3.14159265, 0.0, 1.0), 1.2) * 31.0;
}

// How far `p` is outside a rounded square (half side `h`, corner radius `rad`), negative inside.
// Its level lines further out are rounded squares whose corners round off more the further they
// are: the cover's own shape, softening as it goes.
fn st_rbox(p: vec2<f32>, h: f32, rad: f32) -> f32 {
    let q = abs(p) - vec2<f32>(h - rad);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - rad;
}

@fragment
fn fs_stage(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = frag.xy;
    let k = max(u.misc.w, 1.0);
    let come = clamp(u.p0.x, 0.0, 1.0);
    let clip = rect_clip(p) * come;
    if (clip <= 0.0) { discard; }
    let light = u.view.w > 0.5;
    let seed = u.p0.y;
    let uv = (p - u.rect.xy) / max(u.rect.zw, vec2<f32>(1.0));
    let peak = u.rect.xy + u.p0.zw * u.rect.zw;
    let rel = p - peak;
    let r = length(rel);
    let th = atan2(rel.y, rel.x) + 3.14159265;
    let reach = length(u.rect.zw);
    let look = st_look(0);
    let look2 = st_look(1);
    let beat = clamp(look2.x, 0.0, 1.0);
    let moving = look2.z;

    // The ground: the palette in slow folds, the cover itself breathing through them.
    let deep = u.region.rgb;
    let mid = u.p4.rgb;
    let hi = u.tint.rgb;
    let gt = u.p3.w;
    let asp = vec2<f32>(u.rect.z / max(u.rect.w, 1.0), 1.0);
    let q = uv * asp * 1.25 + vec2<f32>(seed * 0.37, seed * 0.11);
    let n1 = fbm(q + vec2<f32>(gt * 0.020, -gt * 0.015), 3);
    let n2 = fbm(q * 1.6 + 1.3 * vec2<f32>(n1, -n1) + vec2<f32>(-gt * 0.012, gt * 0.017), 3);
    let near = 1.0 - smoothstep(0.0, 0.75 * reach, r);
    let x = clamp(n2 * 1.25 - 0.18 + 0.30 * near, 0.0, 1.0);
    var ground = mix(mix(deep, mid, smoothstep(0.0, 0.55, x)), hi, smoothstep(0.6, 1.0, x) * 0.6);
    let loud_now = clamp(u.p2.y, 0.0, 1.0);

    // How far outside the cover this point is, in the cover's own shape.
    let cover = u.tint.w;
    let corner = cover * 0.12;
    let d = st_rbox(rel, cover, corner);
    let react = select(select(1.0, 1.7, look.y > 1.5), 0.45, look.y < 0.5);
    // The cover's colour at its edge, used to tint the burst.
    let edge_dir = rel / max(max(abs(rel.x), abs(rel.y)), 1.0);
    let edge_col = textureSampleLevel(bd, smp, clamp(vec2<f32>(0.5) + 0.46 * edge_dir, vec2<f32>(0.02), vec2<f32>(0.98)), 0.0).rgb;

    // The rays: three inexpensive fans leave the cover and react to the music. The first fan is
    // broad and bass-heavy, the second follows the melody, and the third shimmers with the highs.
    // They are independent of the flash setting: this control only changes the bloom under the
    // cover, so turning the flash off does not also remove the light rays.
    let clock = u.p1.x;
    let dist = max(d, 0.0);
    let parts = st_look(2);
    let bass = clamp(parts.x, 0.0, 1.0);
    let melody = clamp(parts.y, 0.0, 1.0);
    let top = clamp(parts.z, 0.0, 1.0);
    let kick = clamp(parts.w, 0.0, 1.0);
    let from_under = smoothstep(-2.0 * k, 6.0 * k, d);
    var rays = 0.0;
    let turn = clock * 0.012 * moving;
    for (var i = 0; i < 3; i = i + 1) {
        let fi = f32(i);
        let n = select(select(30.0, 18.0, i == 1), 10.0, i == 0);
        let dir = select(1.0, -1.0, i == 1);
        let shift = turn * dir * (1.0 + 0.5 * fi) + fi * 1.7 + seed;
        let cell = (th + shift) * n / 6.2831853;
        let j = floor(cell);
        let off = (fract(cell) - 0.5) * 6.2831853 / n * max(r, 1.0);
        let jit = sh_h(j + fi * 31.0, 77.0);
        let phi = (j + 0.5) * 6.2831853 / n - shift - 3.14159265;
        let lb = clamp(st_live(st_band_of(atan2(cos(phi), sin(phi)))), 0.0, 1.0);
        var len = 0.0;
        var lit = 0.0;
        var wide = 1.0;
        if (i == 0) {
            len = cover * (0.18 + react * (0.30 * bass + 0.95 * kick) * (0.6 + 0.8 * jit));
            lit = 0.30 + react * (0.35 * bass + 0.9 * kick);
            wide = 1.6 + 1.4 * kick;
        } else if (i == 1) {
            len = cover * (0.10 + react * (0.20 + 0.85 * jit) * lb * (0.5 + melody));
            lit = 0.30 + 0.55 * lb * react;
        } else {
            len = cover * (0.06 + react * 0.45 * jit * top * (0.4 + lb));
            lit = (0.20 + 0.5 * top) * (0.6 + 0.4 * sin(clock * 0.9 + jit * 40.0));
            wide = 0.7;
        }
        let thin = exp(-pow(off / (k * (0.6 + 0.5 * jit) * wide + 0.005 * dist), 2.0));
        rays = rays + thin * exp(-dist / max(len, 1.0)) * lit * (0.65 + 0.35 * jit);
    }
    let g = 0.7853982 + 0.12 * sin(clock * 0.04 + seed);
    let gl = cover * (0.45 + react * (0.9 * kick + 0.4 * bass) + 0.3 * loud_now);
    for (var i = 0; i < 2; i = i + 1) {
        let ga = g + f32(i) * 1.5707963;
        let perp = abs(rel.x * sin(ga) - rel.y * cos(ga));
        rays = rays + exp(-perp / (k * (0.8 + 0.8 * kick) + 0.004 * dist)) * exp(-dist / gl) * (0.6 + 0.6 * kick);
    }
    // Keep the existing ray treatment fixed; the picker below only changes the separate flash.
    rays = rays * 0.75 * from_under;

    // The colour of the rays: the cover's edge, a little of its lightest colour, with a little
    // more of its own colour than a grey would have.
    var light_col = mix(edge_col, hi, 0.30);
    light_col = mix(vec3<f32>(luma(light_col)), light_col, 1.25);
    let ray_col = mix(light_col, vec3<f32>(1.0), 0.35 + 0.4 * exp(-dist / (cover * 0.3)));

    // The halo: the key's hue around the cover, as loud as the music.
    let key_ang = atan2(u.p3.y, u.p3.x);
    let tonal = clamp(u.p3.z, 0.0, 1.0);
    let halo_col = mix(mid, sh_hsl(key_ang * 57.29578 + 360.0, 0.55, 0.58), 0.30 + 0.3 * tonal);
    // The flash is a broad bloom directly beneath the cover. Its three levels affect only this
    // bloom; the rays above remain visible at every level.
    let flash_mode = clamp(look.x, 0.0, 2.0);
    let flash_strength = select(select(0.0, 0.55, flash_mode > 0.5), 1.0, flash_mode > 1.5);
    let halo = flash_strength * exp(-max(d, 0.0) / ((130.0 + 60.0 * bass) * k))
        * (0.08 + 0.16 * loud_now + 0.10 * beat + 0.22 * kick * react);

    var col: vec3<f32>;
    if (light) {
        // Paper: a pale wash of the cover, the rays in its deeper tones.
        let paper = u.ink.rgb;
        col = mix(paper, ground, 0.26);
        col = mix(col, mix(halo_col, paper, 0.4), halo * 0.8);
        col = mix(col, mix(ray_col, deep, 0.40), clamp(rays * 0.45, 0.0, 0.55));
    } else {
        // Dark: the ground sinks low, brightest near the cover.
        col = ground * (0.30 + 0.10 * near + 0.04 * loud_now);
        col = mix(u.ink.rgb, col, 0.94);
        col = col + halo_col * halo;
        col = col + ray_col * rays * 0.55;
    }
    var o = work(clamp(col, vec3<f32>(0.0), vec3<f32>(1.0)));
    o = o + (ign(p) - 0.5) * (2.0 / 255.0);
    return out_px(max(o, vec3<f32>(0.0)), clip);
}

// ---- chart: a small bar chart in a details tile ------------------------------------------------
//
// Bars that rise out of the tile's foot, each printed in an ordered dither of three tones of one
// hue: deep at the foot, light toward the top of the chart, so a tall bar reaches the light tone
// and a short one stays deep. Behind each a faint dotted column shows its room.
//   p0.x: the bar count + 64 × the hue (whole degrees); its fraction is how far the bars have risen
//   p0.yzw: six 4-bit heights each (bar 0 in the lowest bits of p0.y), up to eighteen bars

fn chart_level(i: u32) -> f32 {
    var word = u.p0.y;
    if (i / 6u == 1u) { word = u.p0.z; }
    if (i / 6u == 2u) { word = u.p0.w; }
    let bits = u32(max(word, 0.0) + 0.5);
    return f32((bits >> ((i % 6u) * 4u)) & 15u);
}

fn chart_hue(h: f32) -> vec3<f32> {
    let k = vec3<f32>(0.0, 2.0 / 3.0, 1.0 / 3.0);
    return clamp(abs(fract(vec3<f32>(h) + k) * 6.0 - 3.0) - 1.0, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn sd_bar(q: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let d = abs(q) - half + vec2<f32>(r);
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0) - r;
}

@fragment
fn fs_chart(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let k = max(u.misc.w, 0.5);
    let p = frag.xy;
    let local = (p - u.rect.xy) / k;
    let size = u.rect.zw / k;
    let head = floor(u.p0.x);
    let risen = clamp((u.p0.x - head) * 1.0101, 0.0, 1.0);
    let n = max(head - 64.0 * floor(head / 64.0), 1.0);
    let hue = floor(head / 64.0) / 360.0;
    // Bars no wider than a few points each, the group flush right.
    let step = min(size.x / n, 14.0);
    let x0 = size.x - step * n;
    let idx = floor((local.x - x0) / step);
    if (idx < 0.0 || idx >= n) { return vec4<f32>(0.0); }
    let gap = clamp(step * 0.32, 1.5, 4.0);
    let bw = max(step - gap, 1.0);
    let cx = x0 + (idx + 0.5) * step;
    let r = min(bw * 0.5, 2.5);

    // Each bar rises a little after the one before it.
    let own = smoother(risen * 1.5 - 0.5 * idx / n);
    let h = max(mix(0.14, 1.0, chart_level(u32(idx)) / 15.0) * size.y * own, min(bw, 2.0));
    let d = sd_bar(local - vec2<f32>(cx, size.y - h * 0.5), vec2<f32>(bw * 0.5, h * 0.5), r);
    let cover = clamp(0.5 - d * k, 0.0, 1.0);
    let room_d = sd_bar(local - vec2<f32>(cx, size.y * 0.5), vec2<f32>(bw * 0.5, size.y * 0.5), r);
    let room = clamp(0.5 - room_d * k, 0.0, 1.0);
    if (cover <= 0.001 && room <= 0.001) { return vec4<f32>(0.0); }

    // Printed on a grid of whole points, so the dither reads as a print at any scale.
    let cell = floor(local);
    let thr = bayer8(cell);
    let light = u.view.w > 0.5;
    let up = clamp(1.0 - local.y / max(size.y, 1.0), 0.0, 1.0);
    // Soft tones of one hue that drifts a little from the first bar to the last.
    let hb = hue + 0.09 * idx / max(n - 1.0, 1.0);
    var deep = mix(chart_hue(hb), vec3<f32>(0.13, 0.12, 0.17), 0.64);
    var mid = mix(chart_hue(hb + 0.04), vec3<f32>(0.72, 0.72, 0.78), 0.48);
    var top = mix(chart_hue(hb + 0.09), vec3<f32>(1.0), 0.80);
    if (light) {
        deep = mix(chart_hue(hb), vec3<f32>(0.18, 0.17, 0.22), 0.45);
        mid = mix(chart_hue(hb + 0.04), vec3<f32>(0.45), 0.35);
        top = mix(chart_hue(hb + 0.09), vec3<f32>(0.80), 0.55);
    }
    let tone = clamp(floor(up * 2.0 + thr), 0.0, 2.0);
    var col = deep;
    if (tone > 0.5) { col = mid; }
    if (tone > 1.5) { col = top; }

    // The empty part of the column: a sparse dot of the deep tone.
    let ghost = select(0.0, 0.22, thr < 0.19) * room * (1.0 - cover);
    let c = work(col) * cover + work(deep) * ghost;
    let a = (cover + ghost) * rect_clip(p);
    return vec4<f32>(c * rect_clip(p), a);
}
