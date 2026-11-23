// ---- ribbon: a wave card's ribbons (family 17) -------------------------------------------------

@fragment
fn fs_ribbon(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = frag.xy;
    let k = max(u.misc.w, 1.0);
    let seed = u.p0.y;
    let packed = u.p0.z;
    let T = u.p0.w;
    let hov = clamp(u.p1.x, 0.0, 1.0);
    let on = clamp(u.p1.y, 0.0, 1.0);
    let music = clamp(u.p1.w, 0.0, 1.0);
    let light = u.view.w > 0.5;

    let idx = floor(packed / 360.0 + 0.0001);
    let hue = packed - 360.0 * idx;
    let csat = (idx - 16.0 * floor(idx / 16.0 + 0.0001)) / 15.0;
    let clight = floor(idx / 16.0 + 0.0001) / 15.0;
    let half = u.rect.zw * 0.5;
    let c = u.rect.xy + half;
    let rad = min(select(14.0 * k, u.radii.x, u.radii.x > 0.0), min(half.x, half.y));
    let qq = abs(p - c) - half + vec2<f32>(rad);
    let d = length(max(qq, vec2<f32>(0.0))) + min(max(qq.x, qq.y), 0.0) - rad;
    let cov = clamp(0.5 - d, 0.0, 1.0);
    if (cov <= 0.0) { discard; }
    let size = max(u.rect.zw, vec2<f32>(1.0));
    let uv = (p - u.rect.xy) / size;
    let aspect = size.x / size.y;
    let x = uv.x * aspect;
    let t = T * 0.42 + seed * 3.1;
    var ground = sh_hsl(hue - 14.0 + 22.0 * uv.x, 0.50 * csat, 0.06 + 0.045 * clight + 0.05 * uv.x * uv.x);
    if (light) { ground = sh_hsl(hue - 14.0 + 22.0 * uv.x, 0.36 * csat, 0.93 - 0.05 * uv.x); }
    let rsat = 0.90 * csat;
    let rl = mix(0.38, 0.62, clight);
    let swell = 1.0 + 0.65 * music * on + 0.20 * hov;
    var glow = vec3<f32>(0.0);
    for (var i = 0; i < 4; i = i + 1) {
        let fi = f32(i);
        let f = 0.80 + 0.38 * fi;
        let y0 = 0.5 + (fi - 1.5) * 0.12
            + 0.16 * swell * sin(x * f + t * (0.68 + 0.14 * fi) + fi * 1.7)
            + 0.045 * sin(x * f * 1.7 + t * 0.48 + fi * 4.0);
        let dy = (uv.y - y0) * size.y / k;
        let thick = 1.8 + 1.2 * fi + 1.0 * hov + 0.9 * music * on;
        let core = exp(-dy * dy / (thick * thick));
        let halo = exp(-abs(dy) / (11.0 + 5.0 * fi)) * 0.36;
        let rc = sh_hsl(hue + (fi - 1.5) * 10.0 + 18.0 * (uv.x - 0.5), rsat, rl + 0.05 * fi + 0.07 * uv.x);
        glow = glow + rc * (core * 0.92 + halo) * (0.52 + 0.13 * fi);
    }
    let sweep = fract(T * 0.045 + seed * 0.13) * 2.8 - 0.9;
    let sd = (uv.x + (uv.y - 0.5) * 0.4 / aspect - sweep) * aspect * 2.2;
    let sheen = exp(-sd * sd) * (0.04 + 0.14 * hov);
    let quiet = mix(0.22, 0.92, smoothstep(0.18, 0.72, uv.x)) * (1.0 - 0.45 * smoothstep(0.84, 0.99, uv.x));
    let a = max(glow.r, max(glow.g, glow.b)) * quiet;
    var col = ground + glow * quiet + vec3<f32>(sheen);
    if (light) {
        col = mix(ground, glow / max(a / quiet, 0.001) * 0.84, clamp(a * 0.72, 0.0, 0.76)) - vec3<f32>(sheen * 0.35);
    }
    // The caption and the station name sit on the left: shade the ribbons under them so the text
    // stays readable (lighten instead in the light theme, where the text is dark).
    let under_text = (1.0 - smoothstep(0.08, 0.70, uv.x)) * smoothstep(1.6, 2.4, aspect);
    if (light) {
        col = mix(col, ground, 0.45 * under_text);
    } else {
        col = col * (1.0 - 0.38 * under_text) * 0.92;
    }
    col = col + vec3<f32>(smoothstep(-1.4 * k, 0.0, d) * select(0.07, -0.05, light));
    // Printed in ordered dither, coarser where the ribbons glow.
    let cell = max(1.0, round(k * 1.5));
    let cp = floor((p - u.rect.xy) / cell);
    let grain = fract(sin(dot(cp, vec2<f32>(12.9898, 78.233)) + seed * 7.1) * 43758.5453) - 0.5;
    col = clamp(col + grain * 0.02, vec3<f32>(0.0), vec3<f32>(1.0));
    let steps = mix(20.0, 6.0, clamp(a * 1.4, 0.0, 1.0));
    let thr = bayer8(cp);
    let q = col * steps;
    col = (floor(q) + step(vec3<f32>(thr), fract(q))) / steps;
    let clip = rect_clip(p);
    return vec4<f32>(work(col) * cov * clip, cov * clip);
}
