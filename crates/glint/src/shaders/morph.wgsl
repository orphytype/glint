// ---- morph: the playing track's 2x2 matrix-dot indicator -----------------------------------------

fn sd_round_box(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fs_morph(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = frag.xy;
    let size = min(u.rect.z, u.rect.w);
    let c = u.rect.xy + u.rect.zw * 0.5;
    let run = clamp(u.p0.z, 0.0, 1.0);
    let live = clamp(u.p0.w, 0.0, 1.0);
    let t = u.p0.x;
    let lv = vec3<f32>(u.p1.x, u.p1.y, u.p1.z);
    let beat = clamp(u.p1.w, 0.0, 1.0);
    let energy = clamp((lv.x + lv.y + lv.z) * 0.34 + beat * 0.16, 0.0, 1.0);
    let band = vec4<f32>(lv.x, lv.y, lv.z, (lv.x + lv.z) * 0.5);

    // 2x2 matrix of soft rounded squares in clockwise order: TL, TR, BR, BL.
    let pitch = size * 0.35;
    var offs = array<vec2<f32>, 4>(
        vec2<f32>(-0.5, -0.5),
        vec2<f32>(0.5, -0.5),
        vec2<f32>(0.5, 0.5),
        vec2<f32>(-0.5, 0.5),
    );

    var cov = 0.0;
    for (var i = 0; i < 4; i = i + 1) {
        let fi = f32(i);
        // Calm, predictable clockwise traveling wave with smooth musical breathing.
        let phase = fract(t * 0.56 - fi * 0.25);
        let wave = pow(0.5 + 0.5 * cos(phase * 6.2831853), 1.65);
        let music_lift = mix(0.0, (0.16 * energy + 0.14 * band[i]) * (0.45 + 0.55 * wave), live);
        let level = clamp(wave * (0.82 + 0.18 * energy) + music_lift, 0.0, 1.0);

        let half_s = mix(size * 0.108, mix(size * 0.104, size * 0.126, level), run);
        let radius = half_s * 0.48;
        let center = c + offs[i] * pitch;
        let d = sd_round_box(p - center, vec2<f32>(half_s), radius);
        let shape_a = clamp(0.55 - d, 0.0, 1.0);
        let halo = exp(-max(d, 0.0) / max(size * 0.11, 1.0)) * 0.24 * level * run;
        let dot_alpha = mix(0.68, mix(0.24, 1.0, level), run);
        cov = max(cov, max(shape_a * dot_alpha, halo));
    }

    cov = cov * rect_clip(p);
    if (cov <= 0.0) { discard; }
    return out_px(work(u.tint.rgb), cov);
}
