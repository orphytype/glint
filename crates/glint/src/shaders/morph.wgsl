// ---- morph: the playing track's marker ---------------------------------------------------------

@fragment
fn fs_morph(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = frag.xy;
    let size = min(u.rect.z, u.rect.w);
    let c = u.rect.xy + u.rect.zw * 0.5;
    let bw = size * 0.16;
    let gap = size * 0.28;
    let tall = size * 0.78;
    let run = clamp(u.p0.z, 0.0, 1.0);
    let live = clamp(u.p0.w, 0.0, 1.0);
    let t = u.p0.x;
    let lv = vec3<f32>(u.p1.x, u.p1.y, u.p1.z);
    let beat = clamp(u.p1.w, 0.0, 1.0);
    // Anchor bars to a clean shared baseline while playing, relaxing to centered dots when paused.
    let base_y = c.y + tall * 0.44;
    var a = 0.0;
    for (var i = 0; i < 3; i = i + 1) {
        let fi = f32(i);
        let w1 = sin(t * (5.8 + 2.3 * fi) + fi * 2.15);
        let w2 = sin(t * (9.4 - 1.7 * fi) + fi * 4.1 + 0.8);
        let wave = clamp(0.5 + 0.34 * w1 + 0.22 * w2, 0.0, 1.0);
        let shaped_wave = wave * wave * (3.0 - 2.0 * wave);
        let reactive = clamp(lv[i] * (0.52 + 0.62 * shaped_wave) + 0.18 * shaped_wave + 0.18 * beat * (1.0 - abs(fi - 1.0) * 0.45), 0.0, 1.0);
        let level = mix(0.10 + 0.86 * shaped_wave, reactive, live);
        let h = mix(bw, mix(bw * 1.15, tall, level), run);
        let cy = mix(c.y, base_y - h * 0.5, run);
        let q = vec2<f32>(p.x - (c.x + (fi - 1.0) * gap), p.y - cy);
        let yy = max(abs(q.y) - (h - bw) * 0.5, 0.0);
        let d = length(vec2<f32>(q.x, yy)) - bw * 0.5;
        a = max(a, clamp(0.5 - d, 0.0, 1.0));
    }
    let cov = a * rect_clip(p);
    if (cov <= 0.0) { discard; }
    return out_px(work(u.tint.rgb), cov);
}
