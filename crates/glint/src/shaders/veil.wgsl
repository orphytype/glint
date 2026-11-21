// ---- veil: progressive blur --------------------------------------------------------------------

fn veil_mask(d: f32, h: f32) -> f32 {
    if (h <= 0.0) { return 0.0; }
    let t = clamp(1.0 - d / h, 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

@fragment
fn fs_veil(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = frag.xy;
    let top = u.p0.x;
    let bottom = u.p0.y;
    var m = max(veil_mask(p.y - u.rect.y, top), veil_mask(u.rect.y + u.rect.w - p.y, bottom));
    if (top < 0.0) { m = clamp(-top, 0.0, 1.0); }
    let a = m * rect_clip(p);
    if (a <= 0.001) { discard; }
    let sigma = u.p0.z * m;
    var col = soften(bd, pyr, p, sigma);
    let fade = u.p0.w * m * m * m;
    col = mix(col, work(u.tint.rgb), fade);
    col = col + (ign(p) - 0.5) * (1.2 / 255.0);
    return out_px(col, a);
}
