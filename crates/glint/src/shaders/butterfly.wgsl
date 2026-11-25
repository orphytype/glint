// ---- butterfly: flying butterflies on Home (family 8) ------------------------------------------

fn sh_butterfly(q0: vec2<f32>, seed: f32, flap: f32) -> f32 {
    var q = q0;
    q.x = abs(q.x) / max(flap, 0.10);
    let ur = vec2<f32>(0.44 + 0.06 * sh_h(seed, 12.0), 0.31 + 0.06 * sh_h(seed, 13.0));
    let lr = vec2<f32>(0.27 + 0.06 * sh_h(seed, 14.0), 0.25 + 0.06 * sh_h(seed, 15.0));
    let uw = sh_ell(sh_rot(q - vec2<f32>(0.43, -0.20), -0.50 - 0.2 * sh_h(seed, 16.0)), ur);
    var lw = sh_ell(sh_rot(q - vec2<f32>(0.30, 0.30), 0.55), lr);
    if (sh_h(seed, 17.0) > 0.5) {
        lw = sh_smin(lw, sh_seg(q, vec2<f32>(0.32, 0.40), vec2<f32>(0.40, 0.78)) - 0.05, 0.06);
    }
    let w = sh_smin(uw, lw, 0.07);
    let body = sh_seg(q0, vec2<f32>(0.0, -0.36), vec2<f32>(0.0, 0.44)) - 0.05;
    let ax = vec2<f32>(abs(q0.x), q0.y);
    let ant = sh_seg(ax, vec2<f32>(0.02, -0.38), vec2<f32>(0.17, -0.70)) - 0.016;
    return min(min(w * max(flap, 0.10), body), ant);
}

@fragment
fn fs_butterfly(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = frag.xy;
    let k = max(u.misc.w, 1.0);
    let seed = u.p0.y;
    let hue = u.p0.z;
    let light = u.view.w > 0.5;
    let heading = u.p2.x;
    let open = clamp(u.p2.y, 0.06, 1.0);
    let bank = clamp(u.p2.z, -1.0, 1.0);
    let alt = clamp(u.p2.w, 0.0, 1.0);
    let c = u.rect.xy + u.rect.zw * 0.5;
    let R = min(u.rect.z, u.rect.w) * 0.30;
    let a = heading + 1.5707963;
    let qb = sh_rot((p - c) / R, a);
    let off = vec2<f32>(0.3, 0.75) * R * (0.12 + 0.6 * alt);
    let qs = sh_rot((p - off - c) / R, a);
    if (dot(qb, qb) > 2.6 && dot(qs, qs) > 2.6) { discard; }

    let o = select(open * (1.0 - 0.45 * bank), open * (1.0 + 0.45 * bank), qb.x < 0.0);
    let f = sh_butterfly(qb, seed, clamp(o, 0.06, 1.0));
    let g = length(vec2<f32>(dpdx(f), dpdy(f)));
    let dpx = f / max(g, 1e-5);
    let cov = clamp(0.5 - dpx, 0.0, 1.0);
    let os = select(open * (1.0 - 0.45 * bank), open * (1.0 + 0.45 * bank), qs.x < 0.0);
    let fs = sh_butterfly(qs, seed, clamp(os, 0.06, 1.0));
    let gs = length(vec2<f32>(dpdx(fs), dpdy(fs)));
    let soft = (1.0 + 7.0 * alt) * k + R * 0.04 * alt;
    let sh = (1.0 - smoothstep(-soft, soft, fs / max(gs, 1e-5))) * mix(0.30, 0.12, alt);
    if (cov <= 0.0 && sh <= 0.002) { discard; }

    let ink = select(vec3<f32>(0.0), vec3<f32>(0.10, 0.08, 0.12), light);
    let clip = rect_clip(p);
    if (cov <= 0.0) {
        return vec4<f32>(work(ink) * sh * clip, sh * clip);
    }

    let qa = vec2<f32>(abs(qb.x) / max(o, 0.08), qb.y);
    var tone = 0.30 + 0.55 * o + 0.16 * (fbm(qa * 2.2 + vec2<f32>(seed), 2) - 0.5);
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
    let rgb = work(col) * cov + work(ink) * sh * (1.0 - cov);
    let alpha = max(cov, sh);
    return vec4<f32>(rgb * clip, alpha * clip);
}
