//! Pictures the OS shows for us: the app's icon (cut out of its icon file) and the cover for the
//! media card.

/// The tray icon's edge in pixels. Windows scales it to what the taskbar's DPI wants.
pub const TRAY_PX: u32 = 32;

/// Straight (not premultiplied) RGBA of the app's icon, `size` x `size`: the icon file's 256 px
/// picture, shrunk with a Lanczos filter. A blank square if the file can't be read.
pub fn app_icon(size: u32) -> Vec<u8> {
    let decoded = window_icon().and_then(|png| image::load_from_memory_with_format(png, image::ImageFormat::Png).ok());
    match decoded {
        Some(img) => image::imageops::resize(&img.into_rgba8(), size, size, image::imageops::FilterType::Lanczos3).into_raw(),
        None => vec![0; (size * size * 4) as usize],
    }
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

/// The app's icon file: the light pebble with its glint on a dark tile, at every size Windows asks
/// for. `build.rs` embeds it in the exe, which is where Explorer, the Start menu, pins, Task
/// Manager and the tray's "other icons" list read the icon and the name from. `scripts/icons.py`
/// writes it, with the macOS and Linux icons, from the outlines in `crate::mark`.
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
