// ---- wave_stage: hardware-filtered 3D-texture My Wave playback (music-player-wave) ------------
//
// Preserves the exact motion, folds, luminous ridges, radial streaks, and horizontal wash of
// `wave-basis.bin`, adapting only the 3-stop palette (`shadow`, `wash`, `crest`) and gentle
// playback speed to the active track/preset.

const WAVE_SLICES: f32 = 128.0;
const WAVE_CYCLE_SECONDS: f32 = 254.0 / 30.0;
const WAVE_REF_ASPECT: f32 = 752.0 / 700.0;
// Visual centroid of the glowing wave ring inside the 752x700 recording (which has extra color
// wash room on the right), so placing `focal` at the track cover centers the ring dead on it.
const WAVE_RING_CENTER: vec2<f32> = vec2<f32>(0.445, 0.445);

@group(1) @binding(0) var wave_field: texture_3d<f32>;
@group(1) @binding(1) var field_sampler: sampler;

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

    // Frame the 752x700 wave recording around the left-stage cover center (`u.region.xy`), mirrored
    // horizontally so the luminous color wash flows across the left behind the player while the
    // right edge softly fades into the dark canvas behind the Cormorant arc wheel.
    let focal = u.rect.xy + u.region.xy * size;
    let frame_h = min(size.y, max(size.x * 0.92, 360.0));
    let frame_w = max(frame_h * WAVE_REF_ASPECT, u.region.z * size.x);
    let aspect = frame_w / max(frame_h, 1.0);
    let ratio = max(aspect / WAVE_REF_ASPECT, 0.0001);
    let crop = min(vec2<f32>(ratio, 1.0 / ratio), vec2<f32>(1.0));
    let zoom = max(u.region.w, 0.25);
    let corner = (p - focal) / vec2<f32>(frame_w, frame_h) + 0.5;
    let rel_uv = (corner - 0.5) * crop / zoom;
    let uv = vec2<f32>(WAVE_RING_CENTER.x - rel_uv.x, WAVE_RING_CENTER.y + rel_uv.y);

    // Slice zero represents the start of the recording; sample its texel center with W repeat.
    let z = fract(u.p0.w / WAVE_CYCLE_SECONDS) + 0.5 / WAVE_SLICES;
    let material = textureSampleLevel(wave_field, field_sampler, vec3<f32>(uv, z), 0.0).rg;

    let shadow = u.p3.rgb;
    let wash = u.tint.rgb;
    let crest = u.p4.rgb;
    var rgb = shadow * (1.0 - material.x) + wash * (material.x - material.y) + crest * material.y;

    let print = i32(floor(fract(u.p0.x) * 64.0)) / 8;
    if (print > 0) {
        rgb = wave_print_stage(print, rgb, p, k, min(size.x, size.y), u.p0.y, light);
    } else {
        rgb = clamp(rgb + (ign(p) - 0.5) / 255.0, vec3<f32>(0.0), vec3<f32>(1.0));
    }

    // Soft right fade where the mirrored wave meets the right-side Cormorant arc wheel.
    let fade = smoothstep(-0.08, 0.24, uv.x);
    let alpha = clip * fade;
    if (alpha <= 0.001) { discard; }
    return vec4<f32>(work(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0))) * alpha, alpha);
}
