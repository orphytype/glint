//! Pictures the OS shows for us: the tray icon, painted in code (so there is no image file to
//! ship and the size is a parameter), and the cover for the media card.

/// The tray icon's edge in pixels. Windows scales it to what the taskbar's DPI wants.
pub const TRAY_PX: u32 = 32;

/// Straight (not premultiplied) RGBA of the app's icon, `size` × `size`: Glint's globe (see
/// [`master`]), drawn once at [`MASTER`] px and shrunk with a Lanczos filter, so the dither that
/// prints it at full size melts into its colours when small.
pub fn orb(size: u32) -> Vec<u8> {
    let full = master();
    if size == MASTER {
        return full.clone();
    }
    let img = image::RgbaImage::from_raw(MASTER, MASTER, full.clone()).expect("a whole picture");
    image::imageops::resize(&img, size, size, image::imageops::FilterType::Lanczos3).into_raw()
}

/// The edge the icon is painted at.
const MASTER: u32 = 512;

/// The ribbons of light round the globe: latitude, how often it waves, its phase, how far it
/// waves, its width, its colour.
const RIBBONS: [(f32, f32, f32, f32, f32, [f32; 3]); 4] = [
    (-0.42, 2.2, 0.4, 0.10, 0.037, [150.0, 110.0, 255.0]),
    (-0.12, 2.9, 2.0, 0.13, 0.030, [255.0, 80.0, 200.0]),
    (0.16, 2.0, 3.9, 0.12, 0.042, [70.0, 215.0, 255.0]),
    (0.44, 3.4, 5.1, 0.09, 0.025, [255.0, 205.0, 110.0]),
];

const BAYER8: [[u8; 8]; 8] = [
    [0, 32, 8, 40, 2, 34, 10, 42],
    [48, 16, 56, 24, 50, 18, 58, 26],
    [12, 44, 4, 36, 14, 46, 6, 38],
    [60, 28, 52, 20, 62, 30, 54, 22],
    [3, 35, 11, 43, 1, 33, 9, 41],
    [51, 19, 59, 27, 49, 17, 57, 25],
    [15, 47, 7, 39, 13, 45, 5, 37],
    [63, 31, 55, 23, 61, 29, 53, 21],
];

/// The icon at [`MASTER`] px: a dark globe on a near-black rounded tile, ribbons of light wrapped
/// round it like latitudes, halftone dots in its shadow, a rim of light on its lit edge and a
/// four-point glint at its highlight; printed in an ordered dither, coarse where it is bright and
/// fine in the dark. Painted once.
fn master() -> &'static Vec<u8> {
    static PIC: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    PIC.get_or_init(|| {
        let m = MASTER;
        let mut out = Vec::with_capacity((m * m * 4) as usize);
        for py in 0..m {
            for px in 0..m {
                out.extend_from_slice(&paint(px, py, m));
            }
        }
        out
    })
}

fn paint(ix: u32, iy: u32, m: u32) -> [u8; 4] {
    let (fx, fy, mf) = (ix as f32 + 0.5, iy as f32 + 0.5, m as f32);
    let (x, y) = (fx / mf * 2.0 - 1.0, fy / mf * 2.0 - 1.0);
    let pxu = 2.0 / mf;
    let add = |c: &mut [f32; 3], d: [f32; 3], k: f32| {
        for i in 0..3 {
            c[i] += d[i] * k;
        }
    };
    // The tile: a superellipse, near-black with a violet breath round the globe.
    let sq = (x.abs().powi(5) + y.abs().powi(5)).powf(0.2) - 0.80;
    let tile_a = (0.5 - sq / pxu).clamp(0.0, 1.0);
    if tile_a <= 0.0 {
        return [0, 0, 0, 0];
    }
    let r = (x * x + y * y).sqrt();
    let up = 1.0 - (y + 1.0) * 0.5;
    let mut bg = [0.030 + 0.015 * up, 0.026 + 0.010 * up, 0.045 + 0.025 * up];
    let globe_r = 0.66;
    if r > globe_r {
        let halo = (-((r - globe_r) / 0.09).powi(2)).exp();
        add(&mut bg, [0.45, 0.30, 1.0], halo * 0.16);
    }
    // The globe.
    let inside = (0.5 - (r - globe_r) / pxu).clamp(0.0, 1.0);
    let z = (1.0 - (r / globe_r).powi(2)).max(0.0).sqrt();
    let (nx, ny) = (x / globe_r, y / globe_r);
    let light = (-0.55 * nx - 0.65 * ny + 0.52 * z).clamp(0.0, 1.0);
    let mut body = [0.04 + 0.13 * light, 0.035 + 0.08 * light, 0.10 + 0.26 * light];
    // Halftone dots in its shadow.
    let cell = mf / 36.0;
    let (cx, cy) = (((fx / cell).floor() + 0.5) * cell, ((fy / cell).floor() + 0.5) * cell);
    let shadow = (0.55 - light).clamp(0.0, 1.0) * z;
    let dot = (shadow * 0.42 * cell - ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt() + 0.5).clamp(0.0, 1.0);
    add(&mut body, [0.20, 0.22, 0.55], dot * 0.55);
    // The ribbons, wrapped like latitudes: their height follows the globe's depth there.
    let span = (1.0 - nx * nx).max(0.0).sqrt();
    let lat = ny / span.max(0.05);
    for (l0, f, ph, amp, w, c) in RIBBONS {
        let yc = l0 + amp * (nx * f * 1.6 + ph).sin();
        let dl = (lat - yc) * span.max(0.02f32.sqrt());
        let core = (-(dl / w).powi(2)).exp();
        let halo = 0.30 * (-dl.abs() / (w * 3.2)).exp();
        let mix = 0.5 + 0.5 * (nx * 2.0 + ph).sin();
        let col = [c[0] / 255.0, c[1] / 255.0, c[2] / 255.0];
        let k = (core * 1.05 + halo) * (0.35 + 0.65 * z);
        for i in 0..3 {
            let other = (col[(i + 1) % 3] * 0.4 + col[i] * 0.8).min(1.0);
            body[i] += (col[i] * mix + other * (1.0 - mix)) * k;
        }
    }
    // A rim of light on the lit edge, and the highlight.
    let rim = (-((r - globe_r) / 0.025).powi(2)).exp() * (-0.6 * nx - 0.8 * ny).clamp(0.0, 1.0);
    add(&mut body, [0.55, 0.65, 1.0], rim * 0.65);
    let (hx, hy) = (-0.26, -0.30);
    let spec = (-(((x - hx) / 0.17).powi(2) + ((y - hy) / 0.11).powi(2))).exp();
    add(&mut body, [1.0, 1.0, 1.0], spec * 0.45);
    let mut col = [0.0; 3];
    for i in 0..3 {
        col[i] = bg[i] * (1.0 - inside) + body[i] * inside;
    }
    // The glint: a four-point sparkle at the highlight, reaching past the globe.
    let (gx, gy) = (x - hx, y - hy);
    let gr = (gx * gx + gy * gy).sqrt();
    let arm = (-gy.abs() / 0.010).exp() * (-gx.abs() / 0.22).exp() + (-gx.abs() / 0.010).exp() * (-gy.abs() / 0.22).exp();
    let diag = ((-(gx - gy).abs() / 0.008).exp() + (-(gx + gy).abs() / 0.008).exp()) * (-gr / 0.07).exp() * 0.5;
    let core = (-(gr / 0.035).powi(2)).exp();
    let sparkle = (arm * 0.95 + diag + core * 1.2).clamp(0.0, 1.6);
    add(&mut col, [1.0, 0.97, 1.0], sparkle);
    // Printed in an ordered dither.
    let lum = col[0].max(col[1]).max(col[2]);
    let steps = if lum > 0.25 { 5.0 } else { 14.0 };
    let thr = BAYER8[((iy / 3) % 8) as usize][((ix / 3) % 8) as usize] as f32 / 64.0 + 1.0 / 128.0;
    let mut px = [0u8; 4];
    for i in 0..3 {
        let q = col[i].clamp(0.0, 1.0) * steps;
        let v = (q.floor() + if q.fract() > thr { 1.0 } else { 0.0 }) / steps;
        px[i] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    px[3] = (tile_a * 255.0).round() as u8;
    px
}

/// A PNG of a cover (straight RGBA), shrunk so its longest side is at most `max`. The media card
/// takes an encoded picture, not pixels.
pub fn cover_png(width: u32, height: u32, rgba: Vec<u8>, max: u32) -> Option<Vec<u8>> {
    let img = image::RgbaImage::from_raw(width, height, rgba)?;
    let img = if width.max(height) > max {
        let (w, h) = if width >= height { (max, (height * max / width).max(1)) } else { ((width * max / height).max(1), max) };
        image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// The app's icon file: the orb at every size Windows asks for. `build.rs` embeds it in the exe,
/// which is where Explorer, the Start menu, pins, Task Manager and the tray's "other icons" list
/// read the icon and the name from. It is generated from [`orb`] (a test fails when the two drift
/// apart, and says how to write the file again), so it is data, not a second design.
const ICO: &[u8] = include_bytes!("../../assets/glint.ico");

/// The icon's 256 px picture as a PNG, for the window's title bar, its taskbar button and Alt+Tab.
/// It is cut out of the icon file, so the painter does not have to run at start-up.
pub fn window_icon() -> Option<&'static [u8]> {
    frames(ICO).into_iter().find(|&(size, data)| size == 256 && data.starts_with(b"\x89PNG")).map(|(_, data)| data)
}

/// The pictures in an `.ico` file as `(edge in pixels, bytes)`: a PNG, or a bitmap without its
/// file header. A file that is not an icon has none.
fn frames(ico: &[u8]) -> Vec<(u32, &[u8])> {
    let le = |at: usize, len: usize| -> Option<usize> {
        let bytes = ico.get(at..at.checked_add(len)?)?;
        Some(bytes.iter().rev().fold(0usize, |n, &b| (n << 8) | b as usize))
    };
    // Reserved 0, type 1 (icon, 2 is a cursor), then how many pictures follow.
    if le(0, 2) != Some(0) || le(2, 2) != Some(1) {
        return Vec::new();
    }
    let count = le(4, 2).unwrap_or(0);
    (0..count)
        .filter_map(|i| {
            let entry = 6 + i * 16;
            // A width of 0 means 256. The entry ends with the picture's length and where it starts.
            let size = match *ico.get(entry)? {
                0 => 256,
                w => w as u32,
            };
            let (len, at) = (le(entry + 8, 4)?, le(entry + 12, 4)?);
            Some((size, ico.get(at..at.checked_add(len)?)?))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(img: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size + x) * 4) as usize;
        [img[i], img[i + 1], img[i + 2], img[i + 3]]
    }

    #[test]
    fn the_icon_is_a_dark_tile_with_a_lit_globe() {
        let s = 64;
        let img = orb(s);
        assert_eq!(img.len(), (s * s * 4) as usize);
        assert_eq!(px(&img, s, 0, 0)[3], 0, "the corners are transparent");
        assert_eq!(px(&img, s, s - 1, s - 1)[3], 0);
        // The tile's top edge is a tenth of the way down.
        let edge = px(&img, s, s / 2, 8);
        assert!(edge[3] > 200, "the tile reaches near the top, got {edge:?}");
        assert!(edge[0] < 40 && edge[1] < 40 && edge[2] < 60, "the tile is near black, got {edge:?}");
        // The glint, up and left of the middle, is the brightest place.
        let (gx, gy) = ((0.37 * s as f32) as u32, (0.35 * s as f32) as u32);
        let glint = (gx - 2..=gx + 2).flat_map(|x| (gy - 2..=gy + 2).map(move |y| (x, y))).map(|(x, y)| px(&img, s, x, y)).map(|p| p[0] as u32 + p[1] as u32 + p[2] as u32).max().unwrap();
        assert!(glint > 600, "the glint is near white, got {glint}");
    }

    #[test]
    fn the_edge_is_antialiased() {
        let s = TRAY_PX;
        let img = orb(s);
        let soft = (0..s).flat_map(|x| (0..s).map(move |y| (x, y))).filter(|&(x, y)| matches!(px(&img, s, x, y)[3], 1..=254)).count();
        assert!(soft >= 20, "only {soft} partially covered pixels");
    }

    #[test]
    fn the_orb_works_at_other_sizes() {
        for size in [16, 20, 24, 48] {
            let img = orb(size);
            assert_eq!(img.len(), (size * size * 4) as usize);
            assert_eq!(px(&img, size, 0, 0)[3], 0);
            assert_eq!(px(&img, size, size / 2, size / 2)[3], 255);
        }
    }

    #[test]
    fn a_cover_becomes_a_png_that_decodes_back() {
        let rgba: Vec<u8> = (0..40 * 20).flat_map(|i| [(i % 40 * 6) as u8, 80, 200, 255]).collect();
        let png = cover_png(40, 20, rgba.clone(), 100).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let back = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!((back.width(), back.height()), (40, 20), "never upscaled");
        assert_eq!(back.into_raw(), rgba);
    }

    #[test]
    fn a_big_cover_shrinks_and_keeps_its_aspect() {
        let png = cover_png(600, 300, vec![128; 600 * 300 * 4], 300).unwrap();
        let back = image::load_from_memory(&png).unwrap();
        assert_eq!((back.width(), back.height()), (300, 150));
    }

    #[test]
    fn a_broken_cover_is_refused() {
        assert!(cover_png(10, 10, vec![0; 5], 100).is_none(), "pixels that don't fill the size");
    }

    // ---- the icon file ----

    /// The sizes in the icon file: what the shell asks for at 100 to 300 % scaling, plus the big
    /// views (128 and 256).
    const ICON_SIZES: [u32; 9] = [16, 20, 24, 32, 40, 48, 64, 128, 256];

    /// An `.ico` with the orb at each size. Pictures below 256 px are 32-bit bitmaps, which every
    /// Windows and every resource compiler reads; the 256 px one is a PNG, which keeps the file small.
    fn ico(sizes: &[u32]) -> Vec<u8> {
        let pictures: Vec<Vec<u8>> = sizes.iter().map(|&s| if s >= 256 { png(s) } else { bitmap(s) }).collect();
        let mut out = vec![0, 0, 1, 0]; // reserved, then type 1: icons
        out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
        let mut at = 6 + 16 * sizes.len();
        for (&size, data) in sizes.iter().zip(&pictures) {
            let edge = if size >= 256 { 0 } else { size as u8 }; // 0 stands for 256
            out.extend_from_slice(&[edge, edge, 0, 0]); // width, height, no palette, reserved
            out.extend_from_slice(&1u16.to_le_bytes()); // planes
            out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(at as u32).to_le_bytes());
            at += data.len();
        }
        pictures.into_iter().for_each(|data| out.extend(data));
        out
    }

    /// A picture as the bitmap an icon holds: a header, the colours bottom row first (blue, green,
    /// red, alpha), then the 1-bit mask of what is see-through that old readers still look at.
    fn bitmap(size: u32) -> Vec<u8> {
        let rgba = orb(size);
        let mask_row = size.div_ceil(32) * 4;
        let mut out = Vec::new();
        out.extend_from_slice(&40u32.to_le_bytes()); // the header's own size
        out.extend_from_slice(&size.to_le_bytes()); // width
        out.extend_from_slice(&(size * 2).to_le_bytes()); // height: the colours and the mask stacked
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&0u32.to_le_bytes()); // not compressed
        out.extend_from_slice(&((size * 4 + mask_row) * size).to_le_bytes()); // size of the pixels
        out.extend_from_slice(&[0; 16]); // resolution and palette sizes: unused
        for y in (0..size).rev() {
            for x in 0..size {
                let i = ((y * size + x) * 4) as usize;
                out.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
            }
        }
        for y in (0..size).rev() {
            let mut row = vec![0u8; mask_row as usize];
            for x in 0..size {
                if rgba[((y * size + x) * 4 + 3) as usize] == 0 {
                    row[(x / 8) as usize] |= 0x80 >> (x % 8);
                }
            }
            out.extend(row);
        }
        out
    }

    fn png(size: u32) -> Vec<u8> {
        use image::codecs::png::{CompressionType, FilterType, PngEncoder};
        use image::{ExtendedColorType, ImageEncoder};
        let mut out = Vec::new();
        PngEncoder::new_with_quality(&mut out, CompressionType::Best, FilterType::Adaptive).write_image(&orb(size), size, size, ExtendedColorType::Rgba8).unwrap();
        out
    }

    /// The straight RGBA of one picture from an icon file, read back the way an icon reader does.
    fn decode(size: u32, data: &[u8]) -> Vec<u8> {
        if data.starts_with(b"\x89PNG") {
            return image::load_from_memory(data).unwrap().to_rgba8().into_raw();
        }
        let le = |at: usize| u32::from_le_bytes(data[at..at + 4].try_into().unwrap());
        assert_eq!((le(0), le(4), le(8)), (40, size, size * 2), "a bitmap with a mask under the colours");
        assert_eq!(u16::from_le_bytes([data[14], data[15]]), 32, "bits per pixel");
        let mask_row = size.div_ceil(32) * 4;
        let colours = 40 + (size * size * 4) as usize;
        assert_eq!(data.len(), colours + (mask_row * size) as usize, "colours and mask fill the picture");
        let mut rgba = vec![0u8; (size * size * 4) as usize];
        for stored in 0..size {
            let y = size - 1 - stored; // the file stores the bottom row first
            for x in 0..size {
                let from = 40 + ((stored * size + x) * 4) as usize;
                let to = ((y * size + x) * 4) as usize;
                rgba[to..to + 4].copy_from_slice(&[data[from + 2], data[from + 1], data[from], data[from + 3]]);
                let hidden = data[colours + (stored * mask_row + x / 8) as usize] & (0x80 >> (x % 8)) != 0;
                assert_eq!(hidden, rgba[to + 3] == 0, "the mask agrees with the alpha at {x},{y} of {size}");
            }
        }
        rgba
    }

    #[test]
    fn the_icon_file_matches_the_orb() {
        let pictures = frames(ICO);
        assert_eq!(pictures.iter().map(|p| p.0).collect::<Vec<_>>(), ICON_SIZES, "sizes in assets/glint.ico");
        for (size, data) in pictures {
            let (got, want) = (decode(size, data), orb(size));
            // Not bit for bit: another platform's maths library may round a gradient one step apart.
            let worst = got.iter().zip(&want).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
            assert!(worst <= 2, "the {size} px picture in assets/glint.ico is {worst} steps off the orb");
        }
    }

    #[test]
    fn the_window_icon_is_the_256_px_png() {
        let png = window_icon().expect("the icon file holds a 256 px PNG");
        let img = image::load_from_memory(png).unwrap();
        assert_eq!((img.width(), img.height()), (256, 256));
        assert_eq!(img.to_rgba8().into_raw(), decode(256, png), "and it is the picture the file shows");
    }

    #[test]
    fn the_icon_reader_survives_garbage() {
        assert!(frames(b"").is_empty());
        assert!(frames(b"this is not an icon file at all").is_empty());
        assert!(frames(&[0, 0, 2, 0, 1, 0]).is_empty(), "a cursor file is not an icon file");
        let good = ico(&[16, 32]);
        assert_eq!(frames(&good).iter().map(|p| p.0).collect::<Vec<_>>(), [16, 32]);
        // A cut-off file keeps the pictures that are whole, and nothing panics anywhere in it.
        assert_eq!(frames(&good[..good.len() - 1]).len(), 1);
        (0..good.len()).for_each(|cut| drop(frames(&good[..cut])));
    }

    #[test]
    fn the_encoder_writes_a_mask_that_matches_the_ball() {
        let file = ico(&[16, 24]);
        for (size, data) in frames(&file) {
            let rgba = decode(size, data); // asserts the mask against the alpha as it goes
            assert_eq!(rgba, orb(size), "a bitmap holds the pixels exactly");
        }
    }
}
