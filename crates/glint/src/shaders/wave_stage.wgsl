// ---- wave_stage: continuous non-repeating procedural My Wave backdrop --------------------------
//
// Based on the continuous block-indexed quadratic B-spline wave shader (`wave.wgsl`):
// evaluates 13 independent phase lanes with continuous position and velocity across 4-second
// blocks (never loops), builds the warped front-surface creases, radial light vanes, and bloom
// directly at screen pixel resolution, and frames the wave behind Home's album cover while fading
// cleanly into the dark canvas toward the sidebar and right wave wheel.

const WAVE_TAU: f32 = 6.28318530718;
const WAVE_BLOCK_SECONDS: f32 = 4.0;
const WAVE_REF_ASPECT: f32 = 752.0 / 700.0;

fn wave_hash32(input: u32) -> u32 {
    var h = (input ^ (input >> 16u)) * 0x7feb352du;
    h = (h ^ (h >> 15u)) * 0x846ca68bu;
    return h ^ (h >> 16u);
}

fn wave_fraction(bits: u32) -> f32 {
    return f32(bits >> 8u) * (1.0 / 16777216.0);
}

// Each motion has a sustained forward drift and an independent, bounded variation in speed.
// A quadratic B-spline joins the random offsets with matching velocity, including at block boundaries.
fn wave_phase(seed: u32, block: u32, local_seconds: f32, lane: u32, turns_per_block: f32) -> f32 {
    let stream = wave_hash32(seed ^ ((lane + 1u) * 0x9e3779b9u));
    let speed = turns_per_block * (1.04 + 0.20 * wave_fraction(wave_hash32(stream)));
    let step = u32(speed * 4294967296.0);
    let start = wave_fraction(wave_hash32(stream ^ 0xa511e9b3u) + block * step);
    let tu = local_seconds / WAVE_BLOCK_SECONDS;
    let id = stream + block;
    let knots = vec3<f32>(
        wave_fraction(wave_hash32(id - 1u)),
        wave_fraction(wave_hash32(id)),
        wave_fraction(wave_hash32(id + 1u)),
    );
    let weights = vec3<f32>(0.5 * (1.0 - tu) * (1.0 - tu), 0.5 + tu * (1.0 - tu), 0.5 * tu * tu);
    return WAVE_TAU * (start + tu * speed + 0.50 * speed * dot(weights, knots));
}

fn wave_crest_profile(signed_fold: f32, width: f32) -> f32 {
    let d = signed_fold / width;
    return 1.0 / (1.0 + d * d);
}

fn wave_material(
    uv: vec2<f32>,
    rotation: vec4<f32>,
    drift: vec4<f32>,
    shape: vec4<f32>,
    rays_v: vec4<f32>,
) -> vec2<f32> {
    let wash = 0.012 + 0.958 * smoothstep(-0.045, 1.095, uv.x);
    let p = (uv - vec2<f32>(0.505, 0.376) - 0.018 * shape.xy) * vec2<f32>(3.27, 3.05);
    let support = p * inverseSqrt(max(1.0, dot(p, p)));
    let warp = sin(vec2<f32>(dot(support, vec2<f32>(2.75, 1.68)), dot(support, vec2<f32>(-1.84, 2.95))) + drift.xy);
    let q = p + vec2<f32>(0.153, 0.165) * warp;
    let r2 = dot(q, q);
    let r = sqrt(r2 + 0.00001);
    // Continue the view of the front surface along directions tangent to its silhouette.
    // Each crease naturally tapers into a ray outside the body.
    let inside = 1.0 - r2;
    let z = sqrt(0.5 * (inside + sqrt(inside * inside + 0.035)));
    let n = normalize(vec3<f32>(q, z));
    let yaw = rotation.xy;
    let tilt = rotation.zw;
    let a = vec3<f32>(yaw.x * n.x - yaw.y * n.z, n.y, yaw.y * n.x + yaw.x * n.z);
    let s = vec3<f32>(a.x, tilt.x * a.y - tilt.y * a.z, tilt.y * a.y + tilt.x * a.z);

    let bend = sin(
        vec3<f32>(
            dot(s, vec3<f32>(2.6, 1.05, -2.2)),
            dot(s, vec3<f32>(-1.55, 3.2, 1.25)),
            dot(s, vec3<f32>(2.3, -1.75, 2.4)),
        ) + vec3<f32>(drift.z, -drift.w, drift.x),
    );
    let roll = sin(dot(s, vec3<f32>(2.4, 3.2, -1.7)) + 0.71 * bend.x - drift.y);
    let f = dot(s, vec3<f32>(0.24, 0.90, 0.29)) + 0.45 * bend.x + 0.19 * bend.z + 0.105 * roll - 0.08 * shape.x;
    let g = dot(s, vec3<f32>(-0.82, 0.21, 0.63)) + 0.36 * bend.y - 0.13 * bend.x - 0.11 * roll + 0.09 * shape.y;
    let h = dot(s, vec3<f32>(0.48, -0.32, 0.84)) + 0.33 * bend.z + 0.13 * shape.z;
    let lit_f = smoothstep(-0.82, 0.58, g + 0.21 * bend.y);
    let lit_g = smoothstep(-0.67, 0.80, -h + 0.18 * bend.x);
    let lit_h = smoothstep(0.12, 1.0, -f * g);

    let vane_f = exp2(-abs(f) * select(5.2, 18.0, f < 0.0));
    let vane_g = exp2(-abs(g) * select(20.0, 5.5, g < 0.0));
    let vane_h = exp2(-abs(h) * select(9.0, 26.0, h > 0.0));
    let reach = 0.43 * vane_f + 0.38 * vane_g + 0.22 * vane_h;

    // Intersect drifting angular waves to concentrate light into vanes.
    let dir = p * inverseSqrt(max(dot(p, p), 0.01));
    let d2 = vec2<f32>(dir.x * dir.x - dir.y * dir.y, 2.0 * dir.x * dir.y);
    let d3 = vec2<f32>(dot(d2, vec2<f32>(dir.x, -dir.y)), dot(d2, dir.yx));
    let d5 = vec2<f32>(dot(d2, vec2<f32>(d3.x, -d3.y)), dot(d2, d3.yx));
    let fan = dot(d3, rays_v.xy) + 0.79 * dot(d5, rays_v.zw) + 0.27 * dot(dir, shape.zw);
    let fan_slope = 3.0 * dot(vec2<f32>(-d3.y, d3.x), rays_v.xy) + 3.95 * dot(vec2<f32>(-d5.y, d5.x), rays_v.zw);
    let angular_dist = fan * inverseSqrt(1.7 + fan_slope * fan_slope);
    let vane = exp2(-abs(angular_dist) * select(10.0, 49.0, angular_dist > 0.0));
    let incidence = smoothstep(-0.85, 0.92, 0.67 * dot(d2, rays_v.zw) + 0.39 * bend.z - 0.27 * dir.x);
    let boundary = 0.92 + 0.11 * bend.x + 0.078 * bend.y;
    let core = 1.0 - smoothstep(boundary - 0.36, boundary + 0.24 + 0.10 * vane, r);
    let outer = 1.0 - smoothstep(boundary - 0.13, boundary + 0.54, r);
    let beam = vane * (1.0 - smoothstep(boundary - 0.05, boundary + 0.40 + 1.03 * incidence, r));
    let rays = clamp(reach * outer + (0.19 + 0.53 * incidence + 0.09 * bend.y) * beam, 0.0, 1.0);
    let cover = core + (1.0 - core) * rays;
    let body_tone = 0.949 + 0.026 * bend.y - 0.030 * smoothstep(-0.1, 0.65, g);
    let shadow = smoothstep(-0.1, 0.9, dir.x) * (1.0 - core) * (0.22 * outer + 0.37 * beam);
    let density = mix(wash * (1.0 - shadow), body_tone, cover) - 0.116 * cover * (1.0 - core);
    let soft = 0.262 + 0.094 * bend.x - 0.060 * bend.z + 0.132 * smoothstep(-0.18, 0.26, -f) + 0.042 * roll;
    let creases = 0.42 * lit_f * wave_crest_profile(f, select(0.042, 0.026, f > 0.0))
                + 0.33 * lit_g * wave_crest_profile(g, select(0.029, 0.045, g > 0.0))
                + 0.17 * lit_h * wave_crest_profile(h, 0.035);
    let bloom = lit_f * (0.13 * wave_crest_profile(f, 0.16) + 0.07 * wave_crest_profile(f - 0.12, 0.35))
              + lit_g * (0.095 * wave_crest_profile(g, 0.20) + 0.04 * wave_crest_profile(g + 0.10, 0.38));
    let sheen = soft * cover + (creases + bloom) * (0.90 * core + 0.10 * outer);
    return vec2<f32>(clamp(density, 0.0, 1.0), clamp(sheen, 0.0, max(density, 0.0)));
}

fn wave_print_stage(print: i32, col: vec3<f32>, p: vec2<f32>, k: f32, size: f32, seed: f32, light: bool) -> vec3<f32> {
    let cell = max(1.0, round(k * clamp(size / k / 150.0, 1.25, 2.2)));
    let lp = p - u.rect.xy;
    if (print == 1) {
        let thr = bayer8(floor(lp / cell));
        return floor(col * 5.0 + vec3<f32>(thr)) / 5.0;
    }
    let tone = clamp(luma(col) * 1.15, 0.0, 1.0);
    let paper = select(col * 0.36, mix(col, vec3<f32>(1.0), 0.72), light);
    let dots = select(min(col * 1.25 + vec3<f32>(0.03), vec3<f32>(1.0)), col * 0.84, light);
    let amount = select(tone, 1.0 - 0.62 * tone, light);
    var m: f32;
    if (print == 2) {
        let pitch = cell * 2.8;
        let gp = sh_rot(lp, 0.52) / pitch;
        let dd = length(fract(gp) - 0.5) * pitch;
        m = clamp(sqrt(amount) * 0.60 * pitch - dd + 0.5, 0.0, 1.0);
    } else {
        let pitch = cell * 2.4;
        let gp = sh_rot(lp, 0.7 + sh_h(seed, 34.0) * 1.2).y / pitch;
        let dd = abs(fract(gp) - 0.5) * pitch;
        m = clamp(amount * 0.52 * pitch - dd + 0.5, 0.0, 1.0);
    }
    return mix(paper, dots, m);
}

@fragment
fn fs_wave_stage(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = frag.xy;
    let appear = clamp(u.p1.z, 0.0, 1.0);
    let clip = clip_alpha(p + u.misc.xy, u.clip, u.fade) * appear;
    if (clip <= 0.0) { discard; }

    let size = max(u.rect.zw, vec2<f32>(1.0));
    let k = max(u.misc.w, 1.0);
    let light = u.view.w > 0.5;
    let seed = u32(max(u.p0.y, 0.0));
    let block = u32(max(u.p0.z, 0.0));
    let local_seconds = clamp(u.p0.w, 0.0, WAVE_BLOCK_SECONDS);

    let suv = (p - u.rect.xy) / size;
    let focal = u.rect.xy + u.region.xy * size;
    let ref_h = max(min(size.y, max(size.x * 0.85, 420.0)), 1.0);
    let ref_size = vec2<f32>(ref_h * WAVE_REF_ASPECT, ref_h);
    let zoom = max(u.region.w, 0.35);
    let uv = (p - focal) / (ref_size * zoom) + vec2<f32>(0.505, 0.376);

    let yaw = wave_phase(seed, block, local_seconds, 0u, 0.285);
    let tilt = 0.55 * sin(wave_phase(seed, block, local_seconds, 1u, 0.133))
             + 0.24 * sin(wave_phase(seed, block, local_seconds, 2u, 0.202));
    let drift = vec4<f32>(
        wave_phase(seed, block, local_seconds, 3u, 0.193),
        wave_phase(seed, block, local_seconds, 4u, 0.247),
        wave_phase(seed, block, local_seconds, 5u, 0.158),
        wave_phase(seed, block, local_seconds, 6u, 0.209),
    );
    let shape = sin(vec4<f32>(
        wave_phase(seed, block, local_seconds, 7u, 0.107),
        wave_phase(seed, block, local_seconds, 8u, 0.149),
        wave_phase(seed, block, local_seconds, 9u, 0.181),
        wave_phase(seed, block, local_seconds, 10u, 0.129),
    ));
    let fan = vec2<f32>(
        wave_phase(seed, block, local_seconds, 11u, 0.34),
        wave_phase(seed, block, local_seconds, 12u, 0.265),
    );
    let rotation = vec4<f32>(cos(yaw), sin(yaw), cos(tilt), sin(tilt));
    let rays_v = vec4<f32>(cos(fan.x), sin(fan.x), cos(fan.y), sin(fan.y));

    let m_raw = wave_material(uv, rotation, drift, shape, rays_v);

    // Keep the left edge approaching the sidebar dark (closer to Yandex Music's black) and
    // dissolve smoothly into pure black behind the right wave wheel.
    let left_fade = 0.18 + 0.82 * smoothstep(0.0, 0.28, suv.x);
    let right_fade = 1.0 - smoothstep(0.48, 0.80, suv.x);
    let edge_fade = left_fade * right_fade;
    let m = m_raw * edge_fade;

    let shadow = u.p3.rgb;
    let wash = u.tint.rgb;
    let crest = u.p4.rgb;
    var rgb = shadow * (1.0 - m.x) + wash * (m.x - m.y) + crest * m.y;

    let print = i32(floor(fract(u.p0.x) * 64.0)) / 8;
    if (print > 0) {
        rgb = wave_print_stage(print, rgb, p, k, min(size.x, size.y), u.p0.y, light);
    } else {
        rgb = clamp(rgb + (ign(p) - 0.5) / 255.0, vec3<f32>(0.0), vec3<f32>(1.0));
    }

    let alpha = clip * smoothstep(0.0, 0.08, m.x + m.y * 0.8);
    if (alpha <= 0.001) { discard; }
    return vec4<f32>(work(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0))) * alpha, alpha);
}
