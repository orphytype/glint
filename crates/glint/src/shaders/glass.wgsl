// ---- glass: liquid glass -----------------------------------------------------------------------

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
    if (cov <= 0.0) { discard; }

    let dist = max(-sd, 0.0);
    let e = 1.0;
    var grad = vec2<f32>(sdq(q + vec2<f32>(e, 0.0)) - sdq(q - vec2<f32>(e, 0.0)), sdq(q + vec2<f32>(0.0, e)) - sdq(q - vec2<f32>(0.0, e))) * (0.5 / e);
    let gl = length(grad);
    let n2 = grad / max(gl, 1e-4);

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

    let corner = abs((q.x * q.y) / max(half.x * half.y, 1.0));
    let ca = disp * u.p0.z * (0.45 + corner);
    let sigma = u.p0.w;
    var col: vec3<f32>;
    if (sigma < 0.0) {
        col = work(u.tint.rgb);
    } else {
        col = soften(bd, pyr, rc, sigma);
        if (u.p0.z > 0.0 && dot(ca, ca) > 0.25) {
            col.r = soften(bd, pyr, rc + ca, sigma).r;
            col.b = soften(bd, pyr, rc - ca, sigma).b;
        }
    }

    let is_light = u.view.w > 0.5;
    col = grade(col);
    let tint_rgb = select(work(u.tint.rgb), work(vec3<f32>(1.0)), is_light);
    col = mix(col, tint_rgb, select(u.tint.a, clamp(u.tint.a + 0.14, 0.0, 0.92), sigma < 0.0));

    let bevel = 1.0 - smoother(dist / max(width * 1.15, 1.0));
    let lift = u.p1.w * bevel * bevel;
    let white = work(vec3<f32>(1.0));
    col = select(col * (1.0 + lift) + white * (lift * 0.08), col + (white - col) * (lift * 0.22), is_light);

    let rim_w = u.p2.w * k;
    let band = select(0.0, 1.0 - smoothstep(max(rim_w - 0.85, 0.0), rim_w + 0.35, dist), rim_w > 0.0);
    let ang = u.p3.z;
    let l2 = vec2<f32>(cos(ang), sin(ang));
    let nrm = normalize(vec3<f32>(-n2.x, -n2.y, 1.15));
    let s1 = pow(max(dot(nrm, normalize(vec3<f32>(l2, 0.9))), 0.0), max(u.p2.y, 1.0));
    let s2 = pow(max(dot(nrm, normalize(vec3<f32>(-l2, 0.9))), 0.0), max(u.p2.y, 1.0)) * 0.35;
    let spec = clamp((s1 + s2) * u.p2.x * band, 0.0, 0.16);
    let up = clamp(0.5 - 0.5 * n2.y, 0.0, 1.0);
    if (is_light) {
        let hair = band * 0.14 * (1.0 - up * 0.28);
        col = mix(col, vec3<f32>(0.0), hair);
        col = mix(col, white, clamp(spec * 0.65, 0.0, 0.18));
    } else {
        let rim = clamp(u.p2.z * band * (0.72 + 0.28 * up) + spec, 0.0, 0.24);
        col = mix(col, white, rim);
    }

    col = col + (ign(p) - 0.5) * (1.2 / 255.0);
    let a = cov * u.p3.y;
    return vec4<f32>(max(col, vec3<f32>(0.0)) * a, a);
}
