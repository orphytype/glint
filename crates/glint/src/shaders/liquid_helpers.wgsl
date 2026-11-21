// The colour of the liquid at `q` (its own space, radius 1); `f` is the edge function there.
fn lq_color(big: bool, q: vec2<f32>, qs: vec2<f32>, f: f32, seed: f32, T: f32, hue: f32, hov: f32, music: f32, light: bool) -> vec3<f32> {
    let t = T * select(0.045, 0.075, big);
    let w0 = q * 1.15;
    let o = vec2<f32>(fbm(w0 + vec2<f32>(seed * 1.3 + t, seed * 0.7 - t * 0.8), 3), fbm(w0 + vec2<f32>(-seed * 0.9 - t * 0.7, seed * 1.9 + t), 3)) - 0.5;
    let w = w0 + o * 1.7;
    let n = fbm(w * 1.05 + vec2<f32>(t * 0.6, -t * 0.4), 4);
    var c0: vec3<f32>;
    var c1: vec3<f32>;
    var c2: vec3<f32>;
    if (hue < 0.0) {
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
    let m = fbm(w * 1.6 - vec2<f32>(t * 0.9, t * 0.5) + vec2<f32>(seed), 3);
    let fold = smoothstep(0.50, 0.70, m) * (1.0 - smoothstep(0.70, 0.86, m));
    let crest = pow(1.0 - abs(sin((m * 1.4 + n * 0.5) * 6.2832)), 3.0) * smoothstep(0.45, 0.65, m);
    let shine = (fold * 0.6 + crest * 0.4) * select(0.35, 0.55 + 0.45 * music + 0.2 * hov, big);
    let glint = select(mix(c2, vec3<f32>(1.0), 0.5), vec3<f32>(1.0), light);
    col = col + glint * shine * select(0.6, 0.35, light);
    let inner = exp(-dot(qs - vec2<f32>(-0.12, -0.18), qs - vec2<f32>(-0.12, -0.18)) * 2.2);
    col = col + c2 * inner * select(0.18, 0.10, light);
    let lgt = dot(qs, normalize(vec2<f32>(-0.6, -0.8)));
    col = col * (0.88 + 0.16 * lgt);
    let rim = smoothstep(-0.20, 0.0, f);
    col = mix(col, select(c2 * 1.15, vec3<f32>(1.0), light), rim * select(0.30, 0.40, light));
    if (big) {
        let d = qs - u.p2.yz;
        col = col + c2 * exp(-dot(d, d) * 5.0) * 0.30 * clamp(u.p2.w, 0.0, 1.0);
    }
    let hq = sh_rot(qs - vec2<f32>(-0.38, -0.46), 0.65) * vec2<f32>(1.0, 1.9);
    let hd = length(hq);
    col = col + vec3<f32>(1.0) * (smoothstep(0.34, 0.0, hd) * 0.16 + smoothstep(0.10, 0.0, hd) * 0.22) * select(1.0, 0.6, light);
    let mid = smoothstep(0.78, 0.0, length(qs)) * select(0.55, 1.0, big);
    if (light) {
        col = mix(col, vec3<f32>(1.0), 0.30 * mid);
    } else {
        col = col * mix(1.0, 0.62, mid);
    }
    return clamp(col, vec3<f32>(0.0), vec3<f32>(1.0));
}
