// One-time compute initialization for the music-player-wave 3D material field.
// Verbatim reconstruction math from wave-basis.bin -> 160x152x128 RG8Unorm 3D texture.

const WIDTH = 160u;
const HEIGHT = 152u;
const SLICES = 128u;
const MODES = 48u;
const PIXELS = WIDTH * HEIGHT;

const MEAN_OFFSET = 0u;         // packed f16x2 mean material at each grid point
const TIME_OFFSET = 24320u;     // packed f16 temporal weights, slice-major
const RANGE_OFFSET = 27392u;    // start, split, end of the terms for each mode
const SCALE_OFFSET = 27536u;    // one f32 amplitude per separable term
const X_OFFSET = 28688u;        // signed normalized bytes, term-major
const Y_OFFSET = 74768u;
const PADDED_ROW_WORDS = 128u;  // RG8 upload rows padded to 512 bytes

@group(0) @binding(3) var<storage, read> basis: array<u32>;
@group(0) @binding(4) var<storage, read_write> spatial_modes: array<u32>;
@group(0) @binding(5) var<storage, read_write> packed_cycle: array<u32>;

fn separable_sum(start: u32, end: u32, xy: vec2u) -> f32 {
    var value = 0.0;
    for (var term = start; term < end; term++) {
        let x4 = basis[X_OFFSET + term * (WIDTH / 4u) + xy.x / 4u];
        let y4 = basis[Y_OFFSET + term * (HEIGHT / 4u) + xy.y / 4u];
        let x = unpack4x8snorm(x4)[xy.x % 4u];
        let y = unpack4x8snorm(y4)[xy.y % 4u];
        value += bitcast<f32>(basis[SCALE_OFFSET + term]) * x * y;
    }
    return value;
}

@compute @workgroup_size(8, 8, 1)
fn fit_spatial(@builtin(global_invocation_id) id: vec3u) {
    if (id.x >= WIDTH || id.y >= HEIGHT || id.z >= MODES) { return; }
    let offset = RANGE_OFFSET + 3u * id.z;
    let start = basis[offset];
    let split = basis[offset + 1u];
    let end = basis[offset + 2u];
    let bands = vec2f(separable_sum(start, split, id.xy), separable_sum(split, end, id.xy));
    spatial_modes[id.z * PIXELS + id.y * WIDTH + id.x] = pack2x16float(bands);
}

fn ordered_weights(raw: vec2f) -> vec2f {
    let coverage = saturate(raw.x);
    return vec2f(coverage, clamp(raw.y, 0.0, coverage));
}

@compute @workgroup_size(8, 8, 1)
fn fit_cycle(@builtin(global_invocation_id) id: vec3u) {
    if (id.x >= WIDTH / 2u || id.y >= HEIGHT || id.z >= SLICES) { return; }
    let pixel = id.y * WIDTH + id.x * 2u;
    var left = unpack2x16float(basis[MEAN_OFFSET + pixel]);
    var right = unpack2x16float(basis[MEAN_OFFSET + pixel + 1u]);
    for (var pair = 0u; pair < MODES / 2u; pair++) {
        let a = unpack2x16float(basis[TIME_OFFSET + id.z * (MODES / 2u) + pair]);
        let base = pixel + 2u * pair * PIXELS;
        left += a.x * unpack2x16float(spatial_modes[base])
              + a.y * unpack2x16float(spatial_modes[base + PIXELS]);
        right += a.x * unpack2x16float(spatial_modes[base + 1u])
               + a.y * unpack2x16float(spatial_modes[base + PIXELS + 1u]);
    }
    let dst = (id.z * HEIGHT + id.y) * PADDED_ROW_WORDS + id.x;
    packed_cycle[dst] = pack4x8unorm(vec4f(ordered_weights(left), ordered_weights(right)));
}
