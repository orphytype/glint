// ---- morph: the playing track's marker ---------------------------------------------------------

@fragment
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
