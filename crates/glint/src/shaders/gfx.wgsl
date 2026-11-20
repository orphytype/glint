// Glint's compositor shaders, drawn at RUNE effect slots (see fx.rs). `CLIP_WGSL` and
// `common.wgsl` are prepended, so a slot is cut by the same rectangle, feathers and rounded
// corners as the UI around it.
//
//   fs_field    the page's dithered backdrop
//   fs_glass    liquid glass: refraction at the rim, chromatic dispersion, specular rim
//   fs_veil     progressive blur at the top and bottom of the page
//   fs_blit     puts the last frame back when nothing has moved since it
//   fs_artmorph one cover breaking into blocks and settling into the next
//   fs_bars     the seek line, as bars that dance to the music
//   fs_morph    the playing track's marker
//   fs_shape    the garden on Home: shapes of many families, morphing, filled with a dither
//   fs_stage    behind the open player: a burst under the cover and rays driven by the music

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

// ---- noise -----------------------------------------------------------------------------------

fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let w = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash21(i), hash21(i + vec2<f32>(1.0, 0.0)), w.x), mix(hash21(i + vec2<f32>(0.0, 1.0)), hash21(i + vec2<f32>(1.0, 1.0)), w.x), w.y);
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
//   p4.w: how much the track's measured waveform shows (0..1), plus 2 when the shape it grows
//         out of is measured too (row 1 of `pyr`) rather than made up
//   pyr: the measured waveforms, 256 values a row, four to a texel: row 0 now, row 1 before
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

// The measured loudness of row `row` over the part [a, b) of the track (0..1 of its length).
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

// A measured loudness (0..1) as a bar's height, in the made-up waveform's range.
fn wave_h(v: f32) -> f32 {
    return 0.06 + 0.48 * clamp(v, 0.0, 1.0);
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
    // The measured one: each bar is how loud the track is over its part of the length, so the
    // row is a map of the track, the same wherever it is paused.
    let from_real = u.p4.w >= 1.5;
    let real = clamp(u.p4.w - select(0.0, 2.0, from_real), 0.0, 1.0);
    if (real > 0.0 || from_real) {
        let wa = idx / n;
        let wb = (idx + 1.0) / n;
        let now = wave_h(wave_at(0, wa, wb));
        let before = select(h, wave_h(wave_at(1, wa, wb)), from_real);
        h = mix(before, now, smoothstep(0.0, 1.0, real));
    }

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
    let mult = 0.80 + 0.45 * drive + 0.22 * (own - 0.5) * lively + 0.10 * (swell - 0.5) * lively;
    let dance_made = clamp(h * mult + 0.05 * beat * r2, 0.08, 0.60);
    // Over a measured waveform the music only stirs the bars, a little and around their own
    // height, so the map of the track stays readable while it plays. The bars stay well under
    // the slot's top even on a track whose bass never lets up, so loud and quiet parts still differ.
    let stir = 0.94 + 0.12 * drive + 0.08 * (own - 0.5) * lively + 0.04 * (swell - 0.5) * lively;
    let dance_real = clamp(h * stir + 0.02 * beat * r2, 0.06, 0.66);
    let dance = mix(dance_made, dance_real, real);
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

// ---- shape: the garden on Home -----------------------------------------------------------------
//
//   p0: family, seed, hue in degrees (below 0: the playing cover, `bd`), clock
//   p1: hover 0..1, chosen 0..1, appear 0..1, how loud the music is
//   p2: x how far the cover has turned into the next one; for My Wave, yz the pointer in the
//       liquid's units and w how present it is
// Families: 8 a flying butterfly of the swarm, 17 a wave card's ribbons, 18 an artist's picture,
// 19 My Wave's liquid, 20 a mood's pebble of the same liquid. The edge is worked out from the
// screen derivatives of the shape's function, so it is smooth at any size, and the chosen one
// wears a ring a little outside it.

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

// ---- liquid: My Wave (19) and the moods' pebbles (20) -------------------------------------------
//
// A body of glowing liquid. Its edge swells in a few slow waves (more with the music and under
// the pointer); inside, its colours (the hue and its two neighbours, or the playing cover) are
// carried round by a slow warped flow, with bright threads of light through them as through
// water, a glossy highlight at the top left, a rim of light, and (My Wave) a glow just outside.
// A pebble is the same liquid, rounder and calmer. Both are drawn about the radius
// `blobs::radius` gives, so their places before the compositor is up are the same circles.

// The edge's distance from the middle at angle `a`, about 1, the waves `amp` strong.
fn lq_edge(a: f32, seed: f32, T: f32, amp: f32) -> f32 {
    let w = 0.045 * sin(3.0 * a + T * 0.42 + seed) + 0.030 * sin(5.0 * a - T * 0.31 + seed * 1.7) + 0.018 * sin(7.0 * a + T * 0.55 + seed * 2.3);
    return 1.0 + amp * w;
}

// The colour of the liquid at `q` (its own space, radius 1); `f` is the edge function there.
fn lq_color(fam: i32, q: vec2<f32>, qs: vec2<f32>, f: f32, seed: f32, T: f32, hue: f32, hov: f32, music: f32, light: bool) -> vec3<f32> {
    let big = fam == 19;
    // The clock already runs quicker under the pointer (fx.rs sums the extra per shape); scaling
    // it here by the hover would jump the phase of a clock this old back and forth.
    let t = T * select(0.045, 0.075, big);
    // The flow: the space warped by a slow field, then read again.
    let w0 = q * 1.15;
    let o = vec2<f32>(fbm(w0 + vec2<f32>(seed * 1.3 + t, seed * 0.7 - t * 0.8), 3), fbm(w0 + vec2<f32>(-seed * 0.9 - t * 0.7, seed * 1.9 + t), 3)) - 0.5;
    let w = w0 + o * 1.7;
    let n = fbm(w * 1.05 + vec2<f32>(t * 0.6, -t * 0.4), 4);
    var c0: vec3<f32>;
    var c1: vec3<f32>;
    var c2: vec3<f32>;
    if (hue < 0.0) {
        // The playing cover, stirred: read where the flow carries each point, turning into the
        // next cover as a slow wash across the shape.
        let uv = clamp(0.5 + (q * 0.8 + o * 1.1) * 0.40, vec2<f32>(0.0), vec2<f32>(1.0));
        let turn = smoothstep(0.0, 1.0, clamp(u.p2.x * 1.6 - (dot(q, normalize(vec2<f32>(cos(seed), sin(seed)))) * 0.25 + 0.5) * 0.6, 0.0, 1.0));
        let base = mix(textureSampleLevel(pyr, smp, uv, 0.0).rgb, textureSampleLevel(bd, smp, uv, 0.0).rgb, turn);
        c1 = base;
        c0 = base * 0.55;
        c2 = mix(base, vec3<f32>(1.0), 0.35);
        if (light) {
            c0 = mix(base, vec3<f32>(1.0), 0.25);
            c1 = mix(base, vec3<f32>(1.0), 0.45);
            c2 = mix(base, vec3<f32>(1.0), 0.7);
        }
    } else if (light) {
        c0 = sh_hsl(hue - 26.0, 0.62, 0.66);
        c1 = sh_hsl(hue, 0.70, 0.78);
        c2 = sh_hsl(hue + 34.0, 0.78, 0.88);
    } else {
        c0 = sh_hsl(hue - 22.0, 0.66, 0.27);
        c1 = sh_hsl(hue, 0.72, 0.47);
        c2 = sh_hsl(hue + 30.0, 0.85, 0.68);
    }
    var col = mix(c0, c1, smoothstep(0.28, 0.55, n));
    col = mix(col, c2, smoothstep(0.56, 0.82, n));
    // Light gathering in the liquid as through moving water: broad soft folds where a second
    // field crests, brightest along their middles.
    let m = fbm(w * 1.6 - vec2<f32>(t * 0.9, t * 0.5) + vec2<f32>(seed), 3);
    let fold = smoothstep(0.50, 0.70, m) * (1.0 - smoothstep(0.70, 0.86, m));
    let crest = pow(1.0 - abs(sin((m * 1.4 + n * 0.5) * 6.2832)), 3.0) * smoothstep(0.45, 0.65, m);
    let shine = (fold * 0.6 + crest * 0.4) * select(0.35, 0.55 + 0.45 * music + 0.2 * hov, big);
    let glint = select(mix(c2, vec3<f32>(1.0), 0.5), vec3<f32>(1.0), light);
    col = col + glint * shine * select(0.6, 0.35, light);
    // A light inside it, toward the top: the liquid glows from within.
    let inner = exp(-dot(qs - vec2<f32>(-0.12, -0.18), qs - vec2<f32>(-0.12, -0.18)) * 2.2);
    col = col + c2 * inner * select(0.18, 0.10, light);
    // Lit from the top left: a little darker toward the bottom right.
    let lgt = dot(qs, normalize(vec2<f32>(-0.6, -0.8)));
    col = col * (0.88 + 0.16 * lgt);
    // A rim of light just inside the edge.
    let rim = smoothstep(-0.20, 0.0, f);
    col = mix(col, select(c2 * 1.15, vec3<f32>(1.0), light), rim * select(0.30, 0.40, light));
    if (big) {
        // A light that follows the pointer through the liquid.
        let d = qs - u.p2.yz;
        col = col + c2 * exp(-dot(d, d) * 5.0) * 0.30 * clamp(u.p2.w, 0.0, 1.0);
    }
    // A glossy highlight, soft, with a brighter heart.
    let hq = sh_rot(qs - vec2<f32>(-0.38, -0.46), 0.65) * vec2<f32>(1.0, 1.9);
    let hd = length(hq);
    col = col + vec3<f32>(1.0) * (smoothstep(0.34, 0.0, hd) * 0.16 + smoothstep(0.10, 0.0, hd) * 0.22) * select(1.0, 0.6, light);
    // Calmer in the middle, where the name (and My Wave's button) is.
    let mid = smoothstep(0.78, 0.0, length(qs)) * select(0.55, 1.0, big);
    if (light) {
        col = mix(col, vec3<f32>(1.0), 0.30 * mid);
    } else {
        col = col * mix(1.0, 0.62, mid);
    }
    return clamp(col, vec3<f32>(0.0), vec3<f32>(1.0));
}

// ---- the pictures of the moods (families 9..15) ------------------------------------------------
//
// Each a signed distance in the shape's own space (its radius about 1, y down), drawn a little
// differently for every seed, and filled with the same printed liquid as a pebble.

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
    let lean = (sh_h(seed, 80.0) - 0.5) * 0.14;
    let uneven = (sh_h(seed, 81.0) - 0.5) * 0.08;
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
    // A full crescent: the bite well off to the top right, so its body is wide enough for a name.
    let off = vec2<f32>(0.50, -0.44) * (0.94 + 0.12 * sh_h(seed, 110.0));
    let bite = length(q - off - 0.015 * vec2<f32>(sin(T * 0.2 + seed), cos(T * 0.17))) - (0.64 + 0.04 * sh_h(seed, 111.0));
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

// A crown: a band with three points, a jewel on each that glints in turn, its points a little
// different for every seed.
fn sh_crown(q: vec2<f32>, seed: f32, T: f32) -> f32 {
    let lift = (sh_h(seed, 140.0) - 0.5) * 0.12;
    let v = array<vec2<f32>, 7>(
        vec2<f32>(-0.86, 0.62), vec2<f32>(0.86, 0.62), vec2<f32>(0.98, -0.56 + lift), vec2<f32>(0.46, -0.06),
        vec2<f32>(0.0, -0.82 - lift * 0.5), vec2<f32>(-0.46, -0.06), vec2<f32>(-0.98, -0.56 + lift));
    var d = sh_poly7(q, v) - 0.06;
    for (var i = 0; i < 3; i = i + 1) {
        let tip = v[2 + 2 * i];
        let r = 0.13 + 0.02 * smoothstep(0.6, 1.0, sin(T * 1.4 - f32(i) * 1.1 + seed));
        d = sh_smin(d, length(q - tip) - r, 0.06);
    }
    return d;
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
    return bx - w * (1.0 - sq);
}

// How far a pebble or a picture reaches out from its middle, in its own units (a star's points,
// a bolt's corners, a pebble's swell at its widest).
fn shape_ext(fam: i32) -> f32 {
    switch (fam) {
        case 9: { return 1.23; }
        case 10: { return 1.17; }
        case 11: { return 0.95; }
        case 12: { return 1.02; }
        case 13: { return 0.88; }
        case 14: { return 1.15; }
        case 15: { return 0.98; }
        case 20: { return 1.06; }
        default: { return 1.045; }
    }
}

// ---- My Wave's own look (`WaveLook`) ------------------------------------------------------------
//
// Its form and print ride in the fraction of the family number: (form + 8 * print + 0.5) / 64.

fn wave_look() -> vec2<i32> {
    let code = i32(floor(fract(u.p0.x) * 64.0));
    return vec2<i32>(code % 8, code / 8);
}

// A cloud as a child draws one: a flat bottom with rounded ends, and four round puffs heaped on
// it, the tallest left of the middle. Each puff drifts a little and swells with the music. The
// puffs meet in sharp notches, so each one reads as its own. It has no core of its own: the
// puffs and the base already cover the name and its button, from x -0.8 to 0.8 and y -0.2 to 0.5.
fn wave_cloud(q: vec2<f32>, seed: f32, T: f32, swell: f32) -> f32 {
    // Its middle is 0.11 above the base's, so the whole cloud sits centred in its box.
    let p = q - vec2<f32>(0.0, 0.11);
    // The base: a pill along the bottom.
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

// My Wave's form about radius 1, alive on the clock and swelling with the music (`amp`):
// 0 liquid, 1 a flower, 2 a pinwheel, 3 a cloud. Each but the cloud is joined smoothly
// to a broad core, so whatever its seed it is never narrow where the name and its button sit:
// always as wide and as tall as they are, with its own edge around them.
fn wave_form(form: i32, q: vec2<f32>, seed: f32, T: f32, amp: f32) -> f32 {
    let len = length(q);
    let a = atan2(q.y, q.x);
    let edge = lq_edge(a, seed, T, amp);
    if (form <= 0) {
        return len - edge;
    }
    // A little of the liquid's breathing on every form, so each swells with the music.
    let swell = 1.0 + (edge - 1.0) * 0.6;
    if (form == 3) {
        return wave_cloud(q, seed, T, swell);
    }
    var r: f32;
    if (form == 1) {
        // A flower: five to eight round petals.
        let k = floor(5.0 + sh_h(seed, 4.0) * 3.99);
        let pw = mix(0.7, 1.8, sh_h(seed, 5.0));
        let inner = mix(0.76, 0.84, sh_h(seed, 6.0)) + 0.03 * sin(T * 0.3 + seed);
        let c = pow(abs(cos(k * a * 0.5 + T * 0.03)) + 1e-6, pw);
        r = mix(inner, 1.03, c);
    } else {
        // A pinwheel: three to six arms bent by how far out they are.
        let k = floor(3.0 + sh_h(seed, 7.0) * 3.99);
        let tw = mix(0.9, 1.8, sh_h(seed, 8.0)) * select(-1.0, 1.0, sh_h(seed, 9.0) > 0.5);
        let c = 0.5 + 0.5 * cos(k * (a - tw * len) + T * 0.05);
        r = mix(0.72, 1.04, pow(c + 1e-6, 1.4));
    }
    let f = len - r * swell;
    // The core: as wide and as tall as the name and its button, with room around them.
    return sh_smin(f, sh_ell(q, vec2<f32>(0.90, 0.66)), 0.16);
}

// My Wave printed (`WaveLook::print`): 1 an ordered dither, each channel in three steps; 2 halftone
// dots on a turned grid, larger where it is lighter; 3 lines across it, thicker where it is
// lighter. The grain is fixed to the slot, a couple of pixels a cell.
fn wave_print(print: i32, col: vec3<f32>, p: vec2<f32>, k: f32, size: f32, seed: f32, light: bool) -> vec3<f32> {
    let cell = max(1.0, round(k * clamp(size / k / 120.0, 1.5, 3.0)));
    let lp = p - u.rect.xy;
    if (print == 1) {
        let thr = bayer8(floor(lp / cell));
        return floor(col * 3.0 + vec3<f32>(thr)) / 3.0;
    }
    // On a dark page light dots on dark paper; on a light one dark dots on pale paper.
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

// The radius a pebble or a picture is drawn about in a box `size` pixels across (`blobs::radius`):
// a set part of the box, but never so large that the shape, lifted under the pointer, and the
// ring of the chosen one (4.5 points out, 1.3 thick either side) would run past the box: the slot
// cuts off there, so 8.5 points are kept clear (`blobs::SHAPE_MARGIN`). The star
// is drawn larger: its points are thin, and it weighs as much as the others so.
fn shape_r(fam: i32, size: f32, k: f32) -> f32 {
    var base = size * 0.42;
    if (fam == 20) {
        base = size * 0.43;
    } else if (fam == 13) {
        base = size * 0.48;
    }
    return min(base, (size * 0.5 - 8.5 * k) / (shape_ext(fam) * 1.04));
}

// The shape's function in its own space, negative inside.
fn sh_f(fam: i32, q: vec2<f32>, seed: f32, T: f32, amp: f32) -> f32 {
    let len = length(q);
    if (fam == 18) {
        return sh_avatar(q, seed, T, fract(u.p0.x));
    }
    if (fam == 19) {
        // My Wave reaches toward the pointer: the edge swells on the pointer's side.
        let ptr = u.p2.yz;
        let pres = clamp(u.p2.w, 0.0, 1.0) * smoothstep(0.0, 0.3, length(ptr));
        let toward = max(dot(normalize(q + vec2<f32>(1e-5)), normalize(ptr + vec2<f32>(1e-5))), 0.0);
        let pull = 0.075 * pres * pow(toward, 5.0) * clamp(length(ptr), 0.0, 1.3);
        return wave_form(wave_look().x, q, seed, T, amp) - pull;
    }
    if (fam == 20) {
        return len - lq_edge(atan2(q.y, q.x), seed, T, amp);
    }
    switch (fam) {
        case 9: { return sh_heart(q, seed, T); }
        case 10: { return sh_bolt(q, seed, T); }
        case 11: { return sh_moon(q, seed, T); }
        case 12: { return sh_drop(q, seed, T); }
        case 13: { return sh_star(q, seed, T); }
        case 14: { return sh_crown(q, seed, T); }
        case 15: { return sh_sun(q, seed, T); }
        default: {}
    }
    return len - 0.98;
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

// The shapes are four pipelines, each this function with its `part` fixed, so the shader compiler
// drops the other parts and builds each quickly and side by side: 0 an artist's picture (and any
// other plain shape), 1 a butterfly, 2 a wave card's ribbons, 3 My Wave's liquid and the moods.
@fragment
fn fs_shape(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    return shape_px(frag, 0);
}

@fragment
fn fs_butterfly(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    return shape_px(frag, 1);
}

@fragment
fn fs_ribbon(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    return shape_px(frag, 2);
}

@fragment
fn fs_liquid(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    return shape_px(frag, 3);
}

fn shape_px(frag: vec4<f32>, part: i32) -> vec4<f32> {
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
    let c = u.rect.xy + u.rect.zw * 0.5;
    if (part == 1) {
        return butterfly_px(p, k, seed, u.p0.z, light);
    }
    if (part == 2) {
        return ribbon_px(p, k, seed, u.p0.z, T, light, hov, on, music);
    }
    // The radius each family is drawn about (`blobs::radius`), and how strong its waves are.
    var R = size * 0.5 / 1.045 * (1.0 + 0.045 * hov) * mix(0.82, 1.0, appear);
    var amp = 0.0;
    if (fam == 19) {
        R = size * 0.40;
        amp = 1.0 + 0.5 * music + 0.35 * hov;
    } else if (fam == 20) {
        R = shape_r(fam, size, k) * (1.0 + 0.04 * hov);
        amp = 0.45 + 0.2 * hov;
    }
    // A mood's picture stands upright, swaying a very little.
    let picture = fam >= 9 && fam <= 15;
    var turn = 0.0;
    if (picture) {
        R = shape_r(fam, size, k) * (1.0 + 0.04 * hov);
        turn = 0.035 * sin(T * 0.3 + seed);
    }
    let qs = (p - c) / R;
    let q = sh_rot(qs, turn);
    let f = sh_f(fam, q, seed, T, amp);
    // Pixels from the edge, from how fast the function changes across the screen.
    let g = length(vec2<f32>(dpdx(f), dpdy(f)));
    let dpx = f / max(g, 1e-5);
    let cov = clamp(0.5 - dpx, 0.0, 1.0);
    let ring = clamp(1.3 * k - abs(dpx - 4.5 * k) + 0.5, 0.0, 1.0) * on;
    let clip = rect_clip(p) * appear;
    let hue = u.p0.z;
    let ink = u.ink.rgb;

    if (part == 3) {
        // My Wave glows a little past its edge, fading out well inside its box.
        var glow = 0.0;
        if (fam == 19) {
            let room = smoothstep(size * 0.5, size * 0.43, length(p - c));
            glow = exp(-max(f, 0.0) / 0.075) * (0.30 + 0.35 * music + 0.15 * hov) * room * (1.0 - cov);
        }
        if ((cov <= 0.0 && ring <= 0.0 && glow <= 0.002) || clip <= 0.0) { discard; }
        var col = lq_color(fam, q, qs, f, seed, T, hue, hov, music, light);
        if (fam != 19) {
            // A mood's pebble or picture is printed: each channel in a few steps, an ordered dither between
            // them, in cells a couple of pixels wide.
            let cell = max(1.0, round(k * clamp(size / k / 70.0, 1.5, 2.5)));
            let thr = bayer8(floor((p - u.rect.xy) / cell));
            col = floor(col * 4.0 + vec3<f32>(thr)) / 4.0;
        } else if (wave_look().y > 0) {
            col = wave_print(wave_look().y, col, p, k, size, seed, light);
        } else {
            // A grain of noise against banding in the smooth gradients.
            col = col + (bayer8(floor(p - u.rect.xy)) - 0.5) / 255.0;
        }
        var gcol = select(sh_hsl(hue + 20.0, 0.75, 0.55), sh_hsl(hue + 20.0, 0.70, 0.62), light);
        if (hue < 0.0) {
            // The liquid's own colour just there, so the glow carries on what is at the edge.
            gcol = mix(col, vec3<f32>(luma(col)), 0.2);
        }
        let ga = glow * select(1.0, 0.55, light);
        let rgb = work(col) * cov + work(ink) * ring * (1.0 - cov) + work(gcol) * ga * (1.0 - max(cov, ring));
        let a = max(max(cov, ring), ga);
        return vec4<f32>(rgb * clip, a * clip);
    }

    if ((cov <= 0.0 && ring <= 0.0) || clip <= 0.0) { discard; }
    // An artist's picture (or its hue while it comes): as it is, with only a soft light from the
    // top left and a rim a touch darker, so it reads as a shape and not a hole cut in the page.
    let lgt = dot(qs, normalize(vec2<f32>(-0.6, -0.8)));
    var col: vec3<f32>;
    if (hue < -1500.0) {
        let uv = clamp((p - c) / (2.0 * R) + 0.5, vec2<f32>(0.0), vec2<f32>(1.0));
        col = textureSampleLevel(bd, smp, uv, 0.0).rgb;
    } else {
        col = select(sh_hsl(hue, 0.40, 0.30), sh_hsl(hue, 0.45, 0.80), light);
    }
    col = col * (0.97 + 0.06 * lgt) * mix(1.0, 0.86, smoothstep(-10.0 * k, 0.0, dpx));
    col = col + vec3<f32>(0.04 * hov);
    let rgb = work(clamp(col, vec3<f32>(0.0), vec3<f32>(1.0))) * cov + work(ink) * ring * (1.0 - cov);
    let a = max(cov, ring) * clip;
    return vec4<f32>(rgb * clip, a);
}

// ---- stage: behind the open player ---------------------------------------------------------------
//
// Rays of light come out from under the cover: a broad fan that the kick throws out, a fan that
// follows the voice (each ray as long as its band is loud while someone sings), and fine rays the
// hats make shimmer. The glints along the diagonals flare on the snare and the kick. The ground is
// the cover's own colours, drifting very slowly, warming near the cover while the voice carries,
// and around the cover a soft halo takes the hue of the harmony.
//
// `bd` is the playing cover (40 texels a side), `pyr` the live readings: eight texels of spectrum
// (32 bands), then the look chosen in the player's panel, then the beat, the pulse, whether
// anything moves and the snare, then the low end, the voice, the top and the kick.
//   p0: strength, seed, where the cover's middle is (0..1 of the slot)
//   p1: song clock (s)
//   p2: now: loudness
//   p3: the key (a direction on the circle of fifths: cos, sin), how tonal it is now, the
//       ground's clock
//   region.rgb, p4.rgb, tint.rgb: the cover's palette, dark to light; region.w: how much of the
//   cover itself shows in the ground; tint.w: half the cover's side (px); ink.rgb: the canvas

// The live readings of the music, one row: texels 0..7 the spectrum now (32 bands), texel 8 the
// look chosen in the player's panel (the flash mode and how much the music moves it; each 0..2),
// texel 9 the beat, the pulse, whether anything moves and the snare, texel 10 the low end, the
// voice, the top and the kick.
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

// The band a ray at angle `phi` stands for: straight down is the lowest, straight up the
// highest, mirrored left and right. (`phi` folded into -pi..pi about the vertical, without a
// second atan2.)
fn st_band_of(phi: f32) -> f32 {
    let a = 1.5707963 - phi;
    let w = a - 6.2831853 * floor((a + 3.14159265) / 6.2831853);
    return pow(clamp(abs(w) / 3.14159265, 0.0, 1.0), 1.2) * 31.0;
}

// How far `p` is outside a rounded square (half side `h`, corner radius `rad`), negative inside.
// Its level lines further out are rounded squares whose corners round off more the further they
// are: the cover's own shape, softening as it goes.
fn st_rbox(p: vec2<f32>, h: f32, rad: f32) -> f32 {
    let q = abs(p) - vec2<f32>(h - rad);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - rad;
}

// ---- the user's picture behind the player -------------------------------------------------------
//
// With a picture of the user's own (`p1.y`), `bd` is that picture with all its smaller copies.
// It fills the slot as a photo fills a frame (cropped, never stretched), as soft as `p1.z` (a
// level of the copies, read through eight taps so the steps between levels never show), darkened
// by `p1.w`, and with the effect `p2.z` laid over it: 1 ordered dither, 2 film grain, 3 pixels,
// 4 halftone dots, 5 scanlines. `p2.w` is the picture's width over its height.

// The 4x4 Bayer threshold at a cell, 0..1.
fn bd_bayer(c: vec2<f32>) -> f32 {
    let x = i32(c.x) & 3;
    let y = i32(c.y) & 3;
    let m = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    return (m[y * 4 + x] + 0.5) / 16.0;
}

// Where a point of the slot falls on the picture, cropped to fill it.
fn bd_uv(uv: vec2<f32>) -> vec2<f32> {
    let view = u.rect.z / max(u.rect.w, 1.0);
    let pic = max(u.p2.w, 0.01);
    var q = uv - 0.5;
    if (view > pic) {
        q.y = q.y * pic / view;
    } else {
        q.x = q.x * view / pic;
    }
    return q + 0.5;
}

// The picture at `uv` (of the slot), as soft as the chosen blur.
fn bd_soft(uv: vec2<f32>) -> vec3<f32> {
    let lod = clamp(u.p1.z, 0.0, 9.0);
    let q = bd_uv(uv);
    let dims = vec2<f32>(textureDimensions(bd, 0));
    var c = textureSampleLevel(bd, smp, q, lod).rgb * 0.2;
    if (lod > 0.05) {
        // Eight taps on a ring about one texel of the level read wide, which turns the level's
        // blocks into a round blur.
        let r = exp2(lod) * 0.9 / dims;
        for (var i = 0; i < 8; i = i + 1) {
            let a = f32(i) * 0.7853982 + 0.39;
            c = c + textureSampleLevel(bd, smp, q + vec2<f32>(cos(a), sin(a)) * r, lod).rgb * 0.1;
        }
    } else {
        c = c * 5.0;
    }
    return c;
}

fn bd_picture(p: vec2<f32>, uv: vec2<f32>, k: f32, clock: f32) -> vec3<f32> {
    let eff = i32(u.p2.z + 0.5);
    let size = u.rect.zw;
    var c: vec3<f32>;
    if (eff == 3) {
        // Pixels: each block one colour of the picture.
        let b = 12.0 * k;
        let cp = (floor((p - u.rect.xy) / b) + 0.5) * b;
        c = bd_soft(cp / size);
    } else if (eff == 4) {
        // Halftone: a dot per cell of a turned grid, as large as the picture is light there.
        let cell = 9.0 * k;
        let rot = mat2x2<f32>(0.7071, -0.7071, 0.7071, 0.7071);
        let q = rot * (p - u.rect.xy);
        let ci = floor(q / cell) + 0.5;
        let centre = transpose(rot) * (ci * cell);
        let base = bd_soft(centre / size);
        let l = luma(base);
        let rad = sqrt(clamp(l, 0.0, 1.0)) * 0.62 * cell;
        let d = length(q - ci * cell);
        let dot = 1.0 - smoothstep(rad - 0.8 * k, rad + 0.8 * k, d);
        c = mix(base * 0.18, base * 1.15, dot);
    } else {
        c = bd_soft(uv);
    }
    c = c * (1.0 - clamp(u.p1.w, 0.0, 1.0) * 0.85);
    if (eff == 1) {
        // Ordered dither to a few levels a channel, in cells of two pixels.
        let t = bd_bayer(floor(p / (2.0 * k))) - 0.5;
        let lv = 5.0;
        c = floor(c * lv + 0.5 + t) / lv;
    } else if (eff == 2) {
        // Film grain that changes every frame.
        let g = hash21(floor(p / k) + vec2<f32>(fract(clock * 13.1) * 97.0, fract(clock * 7.7) * 61.0)) - 0.5;
        c = c + g * 0.10 * (0.4 + luma(c));
    } else if (eff == 5) {
        // Scanlines, with the channels a little apart as on a tube.
        let line = 0.72 + 0.28 * (0.5 + 0.5 * cos(p.y / k * 3.14159265 * 0.5));
        let sh = 1.2 * k / size.x;
        let r = bd_soft(uv + vec2<f32>(sh, 0.0)).r * (1.0 - clamp(u.p1.w, 0.0, 1.0) * 0.85);
        let bb = bd_soft(uv - vec2<f32>(sh, 0.0)).b * (1.0 - clamp(u.p1.w, 0.0, 1.0) * 0.85);
        c = vec3<f32>(r, c.g, bb) * line;
    }
    return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
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
    let snare = clamp(look2.w, 0.0, 1.0);

    // The ground: the palette in slow folds, the cover itself breathing through them.
    let deep = u.region.rgb;
    let mid = u.p4.rgb;
    let hi = u.tint.rgb;
    let gt = u.p3.w;
    let asp = vec2<f32>(u.rect.z / max(u.rect.w, 1.0), 1.0);
    let q = uv * asp * 1.25 + vec2<f32>(seed * 0.37, seed * 0.11);
    let n1 = fbm(q + vec2<f32>(gt * 0.020, -gt * 0.015), 2);
    let n2 = fbm(q * 1.6 + 1.3 * vec2<f32>(n1, -n1) + vec2<f32>(-gt * 0.012, gt * 0.017), 3);
    let near = 1.0 - smoothstep(0.0, 0.75 * reach, r);
    let x = clamp(n2 * 1.25 - 0.18 + 0.30 * near, 0.0, 1.0);
    var ground = mix(mix(deep, mid, smoothstep(0.0, 0.55, x)), hi, smoothstep(0.6, 1.0, x) * 0.6);
    let loud_now = clamp(u.p2.y, 0.0, 1.0);

    // How far outside the cover this point is, in the cover's own shape.
    let cover = u.tint.w;
    let corner = cover * 0.12;
    let d = st_rbox(rel, cover, corner);
    // The reaction picked in the player's panel: 0 off, 1 calm, 2 lively.
    let rays_on = look.y > 0.5;
    let react = select(0.45, 1.0, look.y > 1.5);
    // The cover's colour at its edge, used to tint the burst.
    let edge_dir = rel / max(max(abs(rel.x), abs(rel.y)), 1.0);
    let edge_col = textureSampleLevel(bd, smp, clamp(vec2<f32>(0.5) + 0.46 * edge_dir, vec2<f32>(0.02), vec2<f32>(0.98)), select(0.0, 5.0, u.p1.y > 0.5)).rgb;

    // The rays: three inexpensive fans leave the cover and react to the music. The first fan is
    // broad and bass-heavy, the second follows the melody, and the third shimmers with the highs.
    // They are independent of the flash setting: the flash only changes the bloom under the
    // cover, so turning it off does not remove the rays. The reaction does: off draws none.
    let clock = u.p1.x;
    let dist = max(d, 0.0);
    let parts = st_look(2);
    let bass = clamp(parts.x, 0.0, 1.0);
    let voice = clamp(parts.y, 0.0, 1.0);
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
        var len = 0.0;
        var lit = 0.0;
        var wide = 1.0;
        if (i == 0) {
            // The kick's fan: broad, thrown out on each hit.
            len = cover * (0.18 + react * (0.30 * bass + 0.60 * kick) * (0.6 + 0.8 * jit));
            lit = 0.30 + react * (0.30 * bass + 0.40 * kick);
            wide = 1.6 + 0.9 * kick;
        } else {
            let phi = (j + 0.5) * 6.2831853 / n - shift - 3.14159265;
            let lb = clamp(st_live(st_band_of(phi)), 0.0, 1.0);
            if (i == 1) {
                // The voice's fan: it reaches out while someone sings.
                len = cover * (0.10 + react * (0.18 + 0.85 * jit) * lb * (0.35 + 1.35 * voice));
                lit = 0.28 + react * lb * (0.30 + 0.70 * voice);
                wide = 1.0 + 0.5 * voice;
            } else {
                // The hats' fine rays.
                len = cover * (0.06 + react * 0.45 * jit * top * (0.4 + lb));
                lit = (0.20 + 0.5 * top) * (0.6 + 0.4 * sin(clock * 0.9 + jit * 40.0));
                wide = 0.7;
            }
        }
        let z = off / (k * (0.6 + 0.5 * jit) * wide + 0.005 * dist);
        let thin = exp(-z * z);
        rays = rays + thin * exp(-dist / max(len, 1.0)) * lit * (0.65 + 0.35 * jit);
    }
    let g = 0.7853982 + 0.12 * sin(clock * 0.04 + seed);
    let hit = max(kick, snare);
    let gl = cover * (0.45 + react * (0.35 * kick + 0.30 * snare + 0.35 * bass) + 0.3 * loud_now);
    let fall = exp(-dist / gl);
    let sg = sin(g);
    let cg = cos(g);
    // The two diagonals, one across the other.
    let perp0 = abs(rel.x * sg - rel.y * cg);
    let perp1 = abs(rel.x * cg + rel.y * sg);
    let gw = k * (0.8 + 0.5 * hit) + 0.004 * dist;
    rays = rays + (exp(-perp0 / gw) + exp(-perp1 / gw)) * fall * (0.50 + 0.18 * kick + 0.18 * snare);
    rays = rays * 0.75 * from_under * select(0.0, 1.0, rays_on);

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
    // bloom. With the reaction off it no longer moves with the kick, the snare or the voice.
    let flash_mode = clamp(look.x, 0.0, 2.0);
    let flash_strength = select(select(0.0, 0.55, flash_mode > 0.5), 1.0, flash_mode > 1.5);
    let halo = flash_strength * exp(-max(d, 0.0) / ((130.0 + 60.0 * bass + 50.0 * voice) * k))
        * (0.08 + 0.12 * loud_now + 0.04 * beat + select(0.0, react, rays_on) * (0.09 * kick + 0.05 * snare + 0.08 * voice));

    var col: vec3<f32>;
    if (u.p1.y > 0.5) {
        // The user's picture, and the light from the cover over it.
        col = work(bd_picture(p, uv, k, clock));
        col = col + halo_col * halo * 0.8;
        col = col + ray_col * rays * 0.45;
    } else if (light) {
        // Paper: a pale wash of the cover, the rays in its deeper tones.
        let paper = u.ink.rgb;
        col = mix(paper, ground, 0.26);
        col = mix(col, mix(halo_col, paper, 0.4), halo * 0.8);
        col = mix(col, mix(ray_col, deep, 0.40), clamp(rays * 0.45, 0.0, 0.55));
    } else {
        // Dark: the ground sinks low, brightest near the cover.
        col = ground * (0.30 + (0.10 + 0.08 * voice) * near + 0.04 * loud_now);
        col = mix(u.ink.rgb, col, 0.94);
        col = col + halo_col * halo;
        col = col + ray_col * rays * 0.55;
    }
    var o = work(clamp(col, vec3<f32>(0.0), vec3<f32>(1.0)));
    o = o + (ign(p) - 0.5) * (2.0 / 255.0);
    return out_px(max(o, vec3<f32>(0.0)), clip);
}
