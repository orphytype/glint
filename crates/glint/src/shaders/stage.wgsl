// ---- stage: behind the open player -------------------------------------------------------------

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

fn st_band_of(phi: f32) -> f32 {
    let a = 1.5707963 - phi;
    let w = a - 6.2831853 * floor((a + 3.14159265) / 6.2831853);
    return pow(clamp(abs(w) / 3.14159265, 0.0, 1.0), 1.2) * 31.0;
}

fn st_rbox(p: vec2<f32>, h: f32, rad: f32) -> f32 {
    let q = abs(p) - vec2<f32>(h - rad);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - rad;
}

fn bd_bayer(c: vec2<f32>) -> f32 {
    let x = i32(c.x) & 3;
    let y = i32(c.y) & 3;
    let m = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    return (m[y * 4 + x] + 0.5) / 16.0;
}

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

fn bd_soft(uv: vec2<f32>) -> vec3<f32> {
    let lod = clamp(u.p1.z, 0.0, 9.0);
    let q = bd_uv(uv);
    let dims = vec2<f32>(textureDimensions(bd, 0));
    var c = textureSampleLevel(bd, smp, q, lod).rgb * 0.2;
    if (lod > 0.05) {
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
        let b = 12.0 * k;
        let cp = (floor((p - u.rect.xy) / b) + 0.5) * b;
        c = bd_soft(cp / size);
    } else if (eff == 4) {
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
        let t = bd_bayer(floor(p / (2.0 * k))) - 0.5;
        let lv = 5.0;
        c = floor(c * lv + 0.5 + t) / lv;
    } else if (eff == 2) {
        let g = hash21(floor(p / k) + vec2<f32>(fract(clock * 13.1) * 97.0, fract(clock * 7.7) * 61.0)) - 0.5;
        c = c + g * 0.10 * (0.4 + luma(c));
    } else if (eff == 5) {
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
    let reach = length(u.rect.zw);
    let look = st_look(0);
    let look2 = st_look(1);
    let beat = clamp(look2.x, 0.0, 1.0);
    let moving = look2.z;
    let snare = clamp(look2.w, 0.0, 1.0);

    let fx_mode = clamp(look.x, 0.0, 2.0);
    let fx_on = fx_mode > 0.5;
    let lively = fx_mode > 1.5;

    let deep = u.region.rgb;
    let mid = u.p4.rgb;
    let hi = u.tint.rgb;
    let near = 1.0 - smoothstep(0.0, 0.75 * reach, r);
    var ground = deep;
    if (u.p1.y <= 0.5) {
        let gt = select(0.0, u.p3.w, fx_on);
        let asp = vec2<f32>(u.rect.z / max(u.rect.w, 1.0), 1.0);
        let q = uv * asp * 1.25 + vec2<f32>(seed * 0.37, seed * 0.11);
        let n1 = fbm(q + vec2<f32>(gt * 0.020, -gt * 0.015), 2);
        let n2 = fbm(q * 1.6 + 1.3 * vec2<f32>(n1, -n1) + vec2<f32>(-gt * 0.012, gt * 0.017), 3);
        let x = clamp(n2 * 1.25 - 0.18 + 0.30 * near, 0.0, 1.0);
        ground = mix(mix(deep, mid, smoothstep(0.0, 0.55, x)), hi, smoothstep(0.6, 1.0, x) * 0.6);
    }
    let loud_now = clamp(u.p2.y, 0.0, 1.0);

    let cover = u.tint.w;
    let corner = cover * 0.12;
    let d = st_rbox(rel, cover, corner);
    let rays_on = fx_on;
    let react = select(0.28, 1.0, lively);
    let ray_gain = select(0.42, 0.75, lively);
    let clock = u.p1.x;
    let dist = max(d, 0.0);
    let parts = st_look(2);
    let bass = clamp(parts.x, 0.0, 1.0);
    let voice = clamp(parts.y, 0.0, 1.0);
    let top = clamp(parts.z, 0.0, 1.0);
    let kick = clamp(parts.w, 0.0, 1.0);
    let from_under = smoothstep(-2.0 * k, 6.0 * k, d);
    var rays = 0.0;
    var ray_col = vec3<f32>(0.0);
    if (rays_on && from_under > 0.0) {
        let th = atan2(rel.y, rel.x) + 3.14159265;
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
            let wide = select(select(0.7, 1.0 + 0.5 * react * voice, i == 1), 1.6 + 0.9 * react * kick, i == 0);
            let z = off / (k * (0.6 + 0.5 * jit) * wide + 0.005 * dist);
            if (abs(z) < 3.5) {
                var len = 0.0;
                var lit = 0.0;
                if (i == 0) {
                    len = cover * (0.18 + react * (0.30 * bass + 0.60 * kick) * (0.6 + 0.8 * jit));
                    lit = 0.30 + react * (0.30 * bass + 0.40 * kick);
                } else {
                    let phi = (j + 0.5) * 6.2831853 / n - shift - 3.14159265;
                    let lb = clamp(st_live(st_band_of(phi)), 0.0, 1.0);
                    if (i == 1) {
                        len = cover * (0.10 + react * (0.18 + 0.85 * jit) * lb * (0.35 + 1.35 * voice));
                        lit = 0.28 + react * lb * (0.30 + 0.70 * voice);
                    } else {
                        len = cover * (0.06 + react * 0.45 * jit * top * (0.4 + lb));
                        lit = (0.20 + 0.5 * react * top) * (0.6 + 0.4 * sin(clock * 0.9 + jit * 40.0));
                    }
                }
                let thin = exp(-z * z);
                rays = rays + thin * exp(-dist / max(len, 1.0)) * lit * (0.65 + 0.35 * jit);
            }
        }
        let g = 0.7853982 + 0.12 * sin(clock * 0.04 + seed);
        let hit = max(kick, snare);
        let gl = cover * (0.45 + react * (0.35 * kick + 0.30 * snare + 0.35 * bass) + 0.3 * react * loud_now);
        let fall = exp(-dist / gl);
        let sg = sin(g);
        let cg = cos(g);
        let perp0 = abs(rel.x * sg - rel.y * cg);
        let perp1 = abs(rel.x * cg + rel.y * sg);
        let gw = k * (0.8 + 0.5 * react * hit) + 0.004 * dist;
        let cross_base = select(0.35, 0.50, lively);
        rays = rays + (exp(-perp0 / gw) + exp(-perp1 / gw)) * fall * (cross_base + react * (0.18 * kick + 0.18 * snare));
        rays = rays * ray_gain * from_under;
        if (rays > 0.0005) {
            let edge_dir = rel / max(max(abs(rel.x), abs(rel.y)), 1.0);
            let edge_col = textureSampleLevel(bd, smp, clamp(vec2<f32>(0.5) + 0.46 * edge_dir, vec2<f32>(0.02), vec2<f32>(0.98)), select(0.0, 5.0, u.p1.y > 0.5)).rgb;
            var light_col = mix(edge_col, hi, 0.30);
            light_col = mix(vec3<f32>(luma(light_col)), light_col, 1.25);
            ray_col = mix(light_col, vec3<f32>(1.0), 0.35 + 0.4 * exp(-dist / (cover * 0.3)));
        }
    }

    let flash_strength = select(select(0.0, 0.48, fx_on), 1.0, lively);
    var halo = 0.0;
    var halo_col = mid;
    if (flash_strength > 0.0) {
        let key_ang = atan2(u.p3.y, u.p3.x);
        let tonal = clamp(u.p3.z, 0.0, 1.0);
        halo_col = mix(mid, sh_hsl(key_ang * 57.29578 + 360.0, 0.55, 0.58), 0.30 + 0.3 * tonal);
        let pulse = select(0.07 * loud_now + 0.02 * beat + 0.25 * (0.04 * kick + 0.03 * snare + 0.05 * voice), 0.12 * loud_now + 0.04 * beat + 0.09 * kick + 0.05 * snare + 0.08 * voice, lively);
        halo = flash_strength * exp(-dist / ((130.0 + react * (60.0 * bass + 50.0 * voice)) * k)) * (0.08 + pulse);
    }

    var col: vec3<f32>;
    if (u.p1.y > 0.5) {
        col = work(bd_picture(p, uv, k, clock));
        col = col + halo_col * halo * 0.8;
        col = col + ray_col * rays * 0.45;
    } else if (light) {
        let paper = u.ink.rgb;
        col = mix(paper, ground, 0.26);
        col = mix(col, mix(halo_col, paper, 0.4), halo * 0.8);
        col = mix(col, mix(ray_col, deep, 0.40), clamp(rays * 0.45, 0.0, 0.55));
    } else {
        let g_pulse = select(0.0, select(0.45, 1.0, lively), fx_on);
        col = ground * (0.30 + (0.10 + g_pulse * 0.08 * voice) * near + g_pulse * 0.04 * loud_now);
        col = mix(u.ink.rgb, col, 0.94);
        col = col + halo_col * halo;
        col = col + ray_col * rays * 0.55;
    }
    var o = work(clamp(col, vec3<f32>(0.0), vec3<f32>(1.0)));
    o = o + (ign(p) - 0.5) * (2.0 / 255.0);
    return out_px(max(o, vec3<f32>(0.0)), clip);
}
