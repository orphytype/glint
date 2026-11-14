//! Album-art colour. A small Oklab k-means picks what the cover is made of; the results are then
//! pulled into ranges that look good on the near-black UI (an accent that is bright enough to
//! read, aurora colours that are saturated without being neon).

/// sRGB 0..1 → linear.
fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

pub fn rgb_to_oklab(c: [f32; 3]) -> [f32; 3] {
    let (r, g, b) = (to_linear(c[0]), to_linear(c[1]), to_linear(c[2]));
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s, 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s, 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s]
}

/// Linear RGB, not clamped.
fn oklab_to_linear([ll, a, b]: [f32; 3]) -> [f32; 3] {
    let l = (ll + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m = (ll - 0.105_561_35 * a - 0.063_854_17 * b).powi(3);
    let s = (ll - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    [4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_93 * s, -1.268_438 * l + 2.609_757_4 * m - 0.341_319_4 * s, -0.004_196_086 * l - 0.703_418_6 * m + 1.707_614_7 * s]
}

/// An OKLCH colour as sRGB 0..1, with the chroma reduced until it fits the gamut.
pub fn oklch_to_srgb(l: f32, c: f32, h_deg: f32) -> [f32; 3] {
    let h = h_deg.to_radians();
    let in_gamut = |c: f32| {
        let lin = oklab_to_linear([l, c * h.cos(), c * h.sin()]);
        lin.iter().all(|v| (-0.0005..=1.0005).contains(v))
    };
    let mut c = c.max(0.0);
    if !in_gamut(c) {
        let (mut lo, mut hi) = (0.0, c);
        for _ in 0..14 {
            let mid = (lo + hi) * 0.5;
            if in_gamut(mid) { lo = mid } else { hi = mid }
        }
        c = lo;
    }
    let lin = oklab_to_linear([l, c * h.cos(), c * h.sin()]);
    [to_srgb(lin[0]), to_srgb(lin[1]), to_srgb(lin[2])]
}

fn lch(lab: [f32; 3]) -> (f32, f32, f32) {
    (lab[0], lab[1].hypot(lab[2]), lab[2].atan2(lab[1]).to_degrees().rem_euclid(360.0))
}

#[cfg(test)]
fn hue_gap(a: f32, b: f32) -> f32 {
    let d = (a - b).abs().rem_euclid(360.0);
    d.min(360.0 - d)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// Five aurora colours, sRGB 0..1, from the darkest to the brightest.
    pub colors: [[f32; 3]; 5],
    /// A vivid colour that reads on a dark UI.
    pub accent: [f32; 3],
    /// How colourful the art is (0 grey .. 1 vivid); drives how strongly the aurora is tinted.
    pub vibrance: f32,
    /// The cover's own colour for the wave card: its HSL hue (degrees), how colourful it really is
    /// (0 grey .. 1 vivid) and how light (0..1). A grey cover gets grey ribbons, a dull brown one
    /// dull brown ones.
    pub wave: [f32; 3],
}

/// The HSL hue of an sRGB colour, degrees.
fn hsl_hue(c: [f32; 3]) -> f32 {
    let (r, g, b) = (c[0], c[1], c[2]);
    let max = r.max(g).max(b);
    let d = max - r.min(g).min(b);
    if d < 1e-4 {
        return 0.0;
    }
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    h * 60.0
}

impl Palette {
    /// A palette built from one hue, for covers that are grey or not loaded yet.
    pub fn from_hue(hue: f32) -> Palette {
        let c = |l: f32, ch: f32, dh: f32| oklch_to_srgb(l, ch, hue + dh);
        let accent = c(0.72, 0.17, 0.0);
        Palette { colors: [c(0.22, 0.07, -18.0), c(0.38, 0.14, -8.0), c(0.58, 0.19, 0.0), c(0.70, 0.17, 38.0), c(0.82, 0.10, 62.0)], accent, vibrance: 0.7, wave: [hsl_hue(accent), 0.75, 0.55] }
    }

    /// The default look before any music plays: violet to magenta.
    pub fn idle() -> Palette {
        Palette::from_hue(292.0)
    }

    /// Extracts a palette from straight-alpha RGBA8 pixels.
    pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Palette {
        let (w, h) = (width as usize, height as usize);
        if w == 0 || h == 0 || rgba.len() < w * h * 4 {
            return Palette::idle();
        }
        // At most ~1000 samples on a regular grid.
        let step = (((w * h) as f32 / 1024.0).sqrt().ceil() as usize).max(1);
        let mut points: Vec<[f32; 3]> = Vec::with_capacity(1100);
        for y in (0..h).step_by(step) {
            for x in (0..w).step_by(step) {
                let i = (y * w + x) * 4;
                if rgba[i + 3] < 128 {
                    continue;
                }
                points.push(rgb_to_oklab([rgba[i] as f32 / 255.0, rgba[i + 1] as f32 / 255.0, rgba[i + 2] as f32 / 255.0]));
            }
        }
        if points.len() < 8 {
            return Palette::idle();
        }
        let clusters = kmeans(&points, 6);
        Palette::from_clusters(&clusters)
    }

    fn from_clusters(clusters: &[([f32; 3], f32)]) -> Palette {
        // (L, C, hue, weight) of each cluster.
        let lchw: Vec<(f32, f32, f32, f32)> = clusters
            .iter()
            .map(|(c, w)| {
                let (l, ch, h) = lch(*c);
                (l, ch, h, *w)
            })
            .collect();
        let vibrance = (lchw.iter().map(|(_, c, _, w)| c * w).sum::<f32>() / 0.16).clamp(0.0, 1.0);

        // The accent: the colourful, reasonably populous cluster. Grey art has none, and gets a
        // neutral one rather than a colour the cover does not have.
        let best = lchw
            .iter()
            .filter(|(l, _, _, _)| (0.2..0.95).contains(l))
            .max_by(|a, b| {
                let score = |(l, c, _, w): &(f32, f32, f32, f32)| c * (0.35 + w.sqrt()) * (1.0 - (l - 0.65).abs());
                score(a).total_cmp(&score(b))
            })
            .copied();
        let accent = match best {
            Some((_, c, h, _)) if c > 0.035 => oklch_to_srgb(0.74, (c * 1.25).clamp(0.10, 0.20), h),
            _ => oklch_to_srgb(0.86, 0.0, 0.0),
        };

        // The colours the liquid flows through are the cover's own. They are picked along its
        // lightness: with the clusters sorted dark to light and laid end to end by their share of
        // the picture, each of the five stops takes the cluster under its place on that line, so a
        // cover that is mostly one colour is mostly that colour and a grey one stays grey.
        // Specks (the thin lines of a grid, a logo) are left out: a colour that is on a few percent
        // of the cover does not belong in a wash over the whole window.
        let mut by_light: Vec<_> = lchw.iter().copied().filter(|t| t.3 >= 0.07).collect();
        if by_light.is_empty() {
            by_light = lchw.clone();
        }
        by_light.sort_by(|a, b| a.0.total_cmp(&b.0));
        // Colourful parts count for more than their share of the picture (a cover that is half
        // black and half red should flow red, not grey), grey ones for less; among clusters that
        // are all grey the shares stay what they were.
        let share = |t: &(f32, f32, f32, f32)| t.3 * (0.15 + 6.0 * t.1);
        let total: f32 = by_light.iter().map(share).sum::<f32>().max(1e-6);
        let mut run = 0.0;
        let spans: Vec<f32> = by_light
            .iter()
            .map(|t| {
                let mid = (run + share(t) * 0.5) / total;
                run += share(t);
                mid
            })
            .collect();
        const AT: [f32; 5] = [0.06, 0.27, 0.50, 0.73, 0.94];
        // Lightness the five stops lean toward, so that a flat cover still has depth to flow through.
        const LEAN: [f32; 5] = [0.20, 0.36, 0.52, 0.67, 0.80];
        let colors = std::array::from_fn(|i| {
            let k = (0..spans.len()).min_by(|&a, &b| (spans[a] - AT[i]).abs().total_cmp(&(spans[b] - AT[i]).abs())).unwrap_or(0);
            let (l, c, h, _) = by_light[k];
            // Out of the near-black end and short of white, half way to where the stop belongs.
            let l = 0.5 * (0.16 + 0.70 * l) + 0.5 * LEAN[i];
            // The shader pushes the colour away from grey a little; authored chroma allows for it.
            oklch_to_srgb(l, (c * 1.15).min(0.20), h)
        });
        // The wave card's colour: the colourful part of the cover that covers the most of it (a
        // speck of red on a black and white cover does not make the card red), how colourful that
        // really is, and how light. Its hue is the HSL hue the card's shader works in.
        let main = lchw
            .iter()
            .filter(|t| t.3 >= 0.04)
            .max_by(|a, b| {
                let score = |t: &(f32, f32, f32, f32)| t.1 * t.3.powf(0.75);
                score(a).total_cmp(&score(b))
            })
            .copied();
        let wave = match main {
            Some((l, c, h, _)) => {
                let sat = ((c - 0.018) / 0.12).clamp(0.0, 1.0) * (0.4 + 0.6 * (vibrance * 2.0).min(1.0));
                [hsl_hue(oklch_to_srgb(0.65, c.max(0.05), h)), sat, l.clamp(0.0, 1.0)]
            }
            None => [0.0, 0.0, 0.6],
        };
        Palette { colors, accent, vibrance, wave }
    }
}

/// Deterministic k-means in Oklab: farthest-point seeding, then Lloyd iterations. Returns
/// `(centre, share of points)` for each non-empty cluster.
fn kmeans(points: &[[f32; 3]], k: usize) -> Vec<([f32; 3], f32)> {
    let dist2 = |a: &[f32; 3], b: &[f32; 3]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2);
    let n = points.len();
    let mean = points.iter().fold([0.0; 3], |m, p| [m[0] + p[0] / n as f32, m[1] + p[1] / n as f32, m[2] + p[2] / n as f32]);
    // Seed with the point nearest the mean, then repeatedly the one farthest from all seeds.
    let first = points.iter().min_by(|a, b| dist2(a, &mean).total_cmp(&dist2(b, &mean))).copied().unwrap_or(mean);
    let mut centres = vec![first];
    while centres.len() < k {
        let far = points
            .iter()
            .max_by(|a, b| {
                let da = centres.iter().map(|c| dist2(a, c)).fold(f32::MAX, f32::min);
                let db = centres.iter().map(|c| dist2(b, c)).fold(f32::MAX, f32::min);
                da.total_cmp(&db)
            })
            .copied()
            .unwrap_or(first);
        if centres.iter().any(|c| dist2(c, &far) < 1e-6) {
            break;
        }
        centres.push(far);
    }
    let mut counts = vec![0usize; centres.len()];
    for _ in 0..12 {
        let mut sums = vec![[0.0f32; 3]; centres.len()];
        counts.iter_mut().for_each(|c| *c = 0);
        for p in points {
            let (i, _) = centres.iter().enumerate().map(|(i, c)| (i, dist2(p, c))).min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
            sums[i][0] += p[0];
            sums[i][1] += p[1];
            sums[i][2] += p[2];
            counts[i] += 1;
        }
        for (i, c) in centres.iter_mut().enumerate() {
            if counts[i] > 0 {
                *c = [sums[i][0] / counts[i] as f32, sums[i][1] / counts[i] as f32, sums[i][2] / counts[i] as f32];
            }
        }
    }
    centres.into_iter().zip(counts).filter(|(_, n)| *n > 0).map(|(c, n)| (c, n as f32 / points.len() as f32)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        let mut v = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let c = f(x, y);
                v.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        v
    }

    fn hue_of_srgb(c: [f32; 3]) -> f32 {
        lch(rgb_to_oklab(c)).2
    }

    #[test]
    fn oklab_round_trips() {
        for c in [[0.1, 0.5, 0.9], [1.0, 0.0, 0.0], [0.5, 0.5, 0.5], [0.9, 0.8, 0.1]] {
            let lab = rgb_to_oklab(c);
            let lin = oklab_to_linear(lab);
            let back = [to_srgb(lin[0]), to_srgb(lin[1]), to_srgb(lin[2])];
            for i in 0..3 {
                assert!((back[i] - c[i]).abs() < 1e-3, "{c:?} -> {back:?}");
            }
        }
    }

    #[test]
    fn the_wave_colour_is_grey_for_black_and_white_art_with_a_red_speck() {
        // White ground, black figures, a thin red scarf: the card's ribbons should be grey.
        let px = solid(64, 64, |x, y| if (30..33).contains(&x) && (20..30).contains(&y) { [200, 20, 30] } else if (x / 8 + y / 8) % 3 == 0 { [20, 20, 22] } else { [245, 245, 245] });
        let p = Palette::from_rgba(64, 64, &px);
        assert!(p.wave[1] < 0.15, "grey art gave a colourful card: {:?}", p.wave);
    }

    #[test]
    fn the_wave_colour_of_dull_dark_art_is_dull_and_dark() {
        // A murky olive-brown drawing.
        let px = solid(64, 64, |x, y| if (x + y) % 5 == 0 { [70, 62, 40] } else { [38, 34, 26] });
        let p = Palette::from_rgba(64, 64, &px);
        let [h, s, l] = p.wave;
        assert!((25.0..70.0).contains(&h), "hue {h} is not olive-brown");
        assert!(s < 0.5, "saturation {s} too high for murky art");
        assert!(l < 0.45, "lightness {l} too high for dark art");
    }

    #[test]
    fn the_wave_colour_of_a_vivid_cover_is_vivid() {
        let px = solid(64, 64, |x, y| if (x + y) % 9 == 0 { [250, 240, 235] } else { [200, 20, 40] });
        let p = Palette::from_rgba(64, 64, &px);
        let [h, s, _] = p.wave;
        assert!(h < 15.0 || h > 340.0, "hue {h} is not red");
        assert!(s > 0.6, "saturation {s} too low for a red cover");
    }

    #[test]
    fn a_red_cover_gives_a_red_accent() {
        let px = solid(64, 64, |x, y| if (x + y) % 9 == 0 { [250, 240, 235] } else { [200, 20, 40] });
        let p = Palette::from_rgba(64, 64, &px);
        let red = hue_of_srgb([200.0 / 255.0, 20.0 / 255.0, 40.0 / 255.0]);
        assert!(hue_gap(hue_of_srgb(p.accent), red) < 25.0, "accent hue {} vs {}", hue_of_srgb(p.accent), red);
        assert!(p.vibrance > 0.3);
    }

    #[test]
    fn two_colour_art_keeps_both_hues() {
        let px = solid(64, 64, |x, _| if x < 32 { [20, 60, 220] } else { [240, 180, 20] });
        let p = Palette::from_rgba(64, 64, &px);
        let hues: Vec<f32> = p.colors.iter().map(|c| hue_of_srgb(*c)).collect();
        let blue = hue_of_srgb([20.0 / 255.0, 60.0 / 255.0, 220.0 / 255.0]);
        let yellow = hue_of_srgb([240.0 / 255.0, 180.0 / 255.0, 20.0 / 255.0]);
        assert!(hues.iter().any(|h| hue_gap(*h, blue) < 30.0), "{hues:?}");
        assert!(hues.iter().any(|h| hue_gap(*h, yellow) < 30.0), "{hues:?}");
    }

    #[test]
    fn grey_art_stays_grey() {
        let px = solid(32, 32, |x, y| {
            let v = ((x * 7 + y * 3) % 200) as u8 + 20;
            [v, v, v]
        });
        let p = Palette::from_rgba(32, 32, &px);
        for c in p.colors {
            assert!(lch(rgb_to_oklab(c)).1 < 0.02, "a black and white cover gives a coloured wash: {c:?}");
        }
        assert!(lch(rgb_to_oklab(p.accent)).1 < 0.02, "and a coloured accent");
    }

    #[test]
    fn a_dark_blue_and_red_cover_gives_no_orange() {
        // Mostly deep blue, some deep red: nothing in the picture is orange or yellow.
        let px = solid(64, 64, |x, y| if (x / 8 + y / 8) % 4 == 0 { [150, 20, 35] } else { [15, 30, 120] });
        let p = Palette::from_rgba(64, 64, &px);
        let blue = hue_of_srgb([15.0 / 255.0, 30.0 / 255.0, 120.0 / 255.0]);
        let red = hue_of_srgb([150.0 / 255.0, 20.0 / 255.0, 35.0 / 255.0]);
        for c in p.colors {
            if lch(rgb_to_oklab(c)).1 < 0.03 {
                continue;
            }
            let h = hue_of_srgb(c);
            assert!(hue_gap(h, blue) < 35.0 || hue_gap(h, red) < 35.0, "hue {h} is neither the cover's blue {blue} nor its red {red}");
        }
    }

    #[test]
    fn a_one_colour_cover_is_that_colour_at_every_stop() {
        let px = solid(32, 32, |x, y| [30 + ((x + y) % 5) as u8, 90 + ((x * y) % 7) as u8, 200]);
        let p = Palette::from_rgba(32, 32, &px);
        let blue = hue_of_srgb([30.0 / 255.0, 92.0 / 255.0, 200.0 / 255.0]);
        for c in p.colors {
            assert!(hue_gap(hue_of_srgb(c), blue) < 25.0, "{c:?}");
        }
        let l: Vec<f32> = p.colors.iter().map(|c| rgb_to_oklab(*c)[0]).collect();
        assert!(l.windows(2).all(|w| w[0] < w[1]), "the stops still run from dark to light: {l:?}");
    }

    #[test]
    fn everything_is_a_valid_colour() {
        let px = solid(40, 40, |x, y| [(x * 6) as u8, (y * 6) as u8, ((x ^ y) * 5) as u8]);
        let p = Palette::from_rgba(40, 40, &px);
        for c in p.colors.iter().chain(std::iter::once(&p.accent)) {
            assert!(c.iter().all(|v| (0.0..=1.0).contains(v) && v.is_finite()), "{c:?}");
        }
        // Accents must be readable on a dark UI.
        assert!(lch(rgb_to_oklab(p.accent)).0 > 0.6);
    }

    #[test]
    fn degenerate_input_falls_back() {
        assert_eq!(Palette::from_rgba(0, 0, &[]), Palette::idle());
        assert_eq!(Palette::from_rgba(4, 4, &[0; 3]), Palette::idle());
    }
}
