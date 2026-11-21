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

    let reach = max(u.rect.x - u.bounds.x - 1.0, 0.0);
    var drop_a = 0.0;
    if (reach > 1.0 && u.ink.w > 0.0) {
        let d2 = sdq(q - vec2<f32>(0.0, reach * 0.3));
        drop_a = u.ink.w * (1.0 - smoother(d2 / reach)) * clipped * u.p3.y;
    }
    if (cov <= 0.0 && drop_a <= 0.002) { discard; }

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
    var col = vec3<f32>(0.0);
    if (u.p0.z > 0.0 && dot(ca, ca) > 1e-4) {
        col.r = soften(bd, pyr, rc + ca, sigma).r;
        col.g = soften(bd, pyr, rc, sigma).g;
        col.b = soften(bd, pyr, rc - ca, sigma).b;
    } else {
        col = soften(bd, pyr, rc, sigma);
    }

    col = grade(col);
    col = mix(col, work(u.tint.rgb), u.tint.a);

    let bevel = 1.0 - smoother(dist / max(width * 1.2, 1.0));
    col = col * (1.0 + u.p1.w * bevel * bevel * bevel);

    let edge_w = max(u.p3.w * k, 1.5);
    let edge = 1.0 - smoother(dist / edge_w);
    let ang = u.p3.z;
    let l2 = vec2<f32>(cos(ang), sin(ang));
    let nrm = normalize(vec3<f32>(-n2.x, -n2.y, 1.0));
    let s1 = pow(max(dot(nrm, normalize(vec3<f32>(l2, 1.0))), 0.0), max(u.p2.y, 1.0));
    let s2 = pow(max(dot(nrm, normalize(vec3<f32>(-l2, 1.0))), 0.0), max(u.p2.y, 1.0)) * 0.6;
    let spec = clamp((s1 + s2) * u.p2.x * edge, 0.0, 1.0);
    let band = 1.0 - smoothstep(max(u.p2.w * k - 1.0, 0.0), max(u.p2.w * k, 0.5), dist);
    let up = clamp(0.5 - 0.5 * n2.y, 0.0, 1.0);
    let rim = clamp(u.p2.z * band * (0.35 + 0.65 * up), 0.0, 1.0);
    let shade = u.p3.x * edge;
    col = mix(col, vec3<f32>(0.0), shade * (1.0 - spec));
    col = mix(col, work(vec3<f32>(1.0)), clamp(spec + rim * (1.0 - spec), 0.0, 1.0));

    col = col + (ign(p) - 0.5) * (1.2 / 255.0);
    let a = cov * u.p3.y;
    return vec4<f32>(max(col, vec3<f32>(0.0)) * a, a + (1.0 - a) * drop_a);
}
