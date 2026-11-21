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

    // An ordered dither of that gradient, in cells of about two points.
    let cell = max(1.0, round(2.0 * k));
    let thr = bayer8(floor(p / cell));
    let steps = 7.0;
    let lvl = floor(g * steps + thr) / steps;
    let amp = u.p0.x;
    var col = mix(canvas, ink, lvl * amp);
    col = col + (ign(p + vec2<f32>(floor(t * 8.0) * 17.0, 3.0)) - 0.5) * (1.5 / 255.0);
    return out_px(col, rect_clip(p));
}
