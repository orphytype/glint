//! Cover art. A view asks for a picture by URL and gets a `Pixels` handle straight away; the
//! image arrives later (fetched, decoded and, for demo covers, painted on a worker) and RUNE
//! redraws the picture without running the view again.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use rune::Pixels;

use crate::palette::Palette;

#[derive(Clone)]
pub struct ArtCache(Arc<Mutex<Inner>>);

struct Inner {
    map: HashMap<String, Entry>,
    wanted: Vec<String>,
    tick: u64,
    /// Bumped every time the view is built: what the last views asked for is on screen, and is
    /// never let go of to make room (it would only be asked for again, load, fade in, push out
    /// another, and so on: covers blinking forever on a page with many of them).
    view: u64,
    /// Bumped every time a picture lands, so the app knows to build the view again (a cover that
    /// arrives fades in instead of appearing in one frame).
    landed: u64,
    /// Pictures a shader draws as well (an artist in a shape): a copy of their pixels is kept,
    /// since the one RUNE holds cannot be read back.
    keep: HashSet<String>,
}

struct Entry {
    pixels: Pixels,
    palette: Option<Palette>,
    /// A tiny copy of the picture, for the shader behind the player (the picture RUNE holds cannot
    /// be read back).
    thumb: Option<Arc<Thumb>>,
    ready: bool,
    /// When the picture landed.
    done: Option<std::time::Instant>,
    used: u64,
    /// The view that last asked for it.
    seen: u64,
    /// What the decoded picture takes in memory.
    bytes: usize,
    /// The pixels again, for a picture in `Inner::keep`.
    full: Option<Arc<Decoded>>,
}

/// A cover shrunk for the shaders (sRGB, RGBA, square): `THUMB` texels a side for the blurred
/// backdrop, and a sharper copy (`sharp_side`, up to `SHARP`) for the morph between two covers.
pub struct Thumb {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub sharp_side: u32,
    pub sharp: Vec<u8>,
}

/// The thumbnail's side in texels: small on purpose, it is only ever seen through a heavy blur.
pub const THUMB: u32 = 40;

/// The sharper copy's side at most: the morph is over in about a second and breaks the picture
/// into blocks for most of it, so this is plenty.
pub const SHARP: u32 = 256;

impl Thumb {
    /// Both copies of `rgba` (`w` x `h`).
    pub fn from_rgba(w: u32, h: u32, rgba: &[u8]) -> Option<Thumb> {
        if w == 0 || h == 0 || rgba.len() < (w * h * 4) as usize {
            return None;
        }
        let side = SHARP.min(w.min(h)).max(THUMB);
        Some(Thumb { width: THUMB, height: THUMB, rgba: shrink(w, h, rgba, THUMB), sharp_side: side, sharp: shrink(w, h, rgba, side) })
    }

    /// What the two copies take in memory.
    pub fn bytes(&self) -> usize {
        self.rgba.len() + self.sharp.len()
    }
}

/// [`shrink`] for a square picture `side` texels a side.
pub fn shrink_square(side: u32, rgba: &[u8], n: u32) -> Vec<u8> {
    shrink(side, side, rgba, n)
}

/// An area average of `rgba` (`w` x `h`) to `n` texels a side.
fn shrink(w: u32, h: u32, rgba: &[u8], n: u32) -> Vec<u8> {
    let mut out = vec![0u8; (n * n * 4) as usize];
    for ty in 0..n {
        let (y0, y1) = (ty * h / n, ((ty + 1) * h / n).max(ty * h / n + 1).min(h));
        for tx in 0..n {
            let (x0, x1) = (tx * w / n, ((tx + 1) * w / n).max(tx * w / n + 1).min(w));
            let mut acc = [0u32; 4];
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = ((y * w + x) * 4) as usize;
                    for c in 0..4 {
                        acc[c] += u32::from(rgba[i + c]);
                    }
                }
            }
            let cnt = ((y1 - y0) * (x1 - x0)).max(1);
            let o = ((ty * n + tx) * 4) as usize;
            for c in 0..4 {
                out[o + c] = (acc[c] / cnt) as u8;
            }
        }
    }
    out
}

pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Finished pictures kept, most; and the memory they may take together before the oldest go.
const KEEP: usize = 240;
const BUDGET: usize = 48 << 20;

impl ArtCache {
    pub fn new() -> ArtCache {
        ArtCache(Arc::new(Mutex::new(Inner { map: HashMap::new(), wanted: Vec::new(), tick: 0, view: 1, landed: 0, keep: HashSet::new() })))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A new view is being built: what it asks for counts as on screen.
    pub fn begin_view(&self) {
        self.lock().view += 1;
    }

    /// The picture for `key` (a cover URL). The first ask schedules the load.
    pub fn pixels(&self, key: &str) -> Pixels {
        let mut g = self.lock();
        g.tick += 1;
        let (tick, view) = (g.tick, g.view);
        if let Some(e) = g.map.get_mut(key) {
            e.used = tick;
            e.seen = view;
            return e.pixels.clone();
        }
        let pixels = Pixels::new();
        g.map.insert(key.to_owned(), Entry { pixels: pixels.clone(), palette: None, thumb: None, ready: false, done: None, used: tick, seen: view, bytes: 0, full: None });
        g.wanted.push(key.to_owned());
        pixels
    }

    pub fn palette(&self, key: &str) -> Option<Palette> {
        self.lock().map.get(key).and_then(|e| e.palette)
    }

    /// Whether the picture is decoded and handed to its `Pixels`.
    pub fn ready(&self, key: &str) -> bool {
        self.lock().map.get(key).is_some_and(|e| e.ready)
    }

    /// Whether the picture landed just now (a view built now shows it for the first time, so it
    /// fades in; one built later shows it at once).
    pub fn fresh(&self, key: &str) -> bool {
        self.lock().map.get(key).and_then(|e| e.done).is_some_and(|d| d.elapsed().as_secs_f32() < 0.5)
    }

    /// How many pictures have landed so far.
    pub fn landed(&self) -> u64 {
        self.lock().landed
    }

    /// The pixels of `key` for a shader, once they are in. Asking marks the picture as one whose
    /// pixels are kept; ask before it lands (when the view first shows it), or it is loaded again.
    pub fn full(&self, key: &str) -> Option<Arc<Decoded>> {
        let mut g = self.lock();
        if g.keep.insert(key.to_owned()) {
            // Only the last few are kept: a shape shows one picture at a time.
            if g.keep.len() > 8 {
                let drop: Vec<String> = g.keep.iter().filter(|k| *k != key).take(g.keep.len() - 8).cloned().collect();
                for k in drop {
                    g.keep.remove(&k);
                    if let Some(e) = g.map.get_mut(&k) {
                        e.full = None;
                    }
                }
            }
        }
        g.map.get(key).and_then(|e| e.full.clone())
    }

    pub fn thumb(&self, key: &str) -> Option<Arc<Thumb>> {
        self.lock().map.get(key).and_then(|e| e.thumb.clone())
    }

    /// Keys asked for since the last call, each of which needs loading.
    pub fn take_wanted(&self) -> Vec<String> {
        std::mem::take(&mut self.lock().wanted)
    }

    /// Stores a loaded image (or, with `None`, gives up on the key) and works out its palette.
    ///
    /// A failed load is forgotten rather than kept as an entry: the next view that asks for the
    /// picture schedules it again, instead of waiting forever on a load that already gave up.
    pub fn finish(&self, key: &str, image: Option<Decoded>) {
        let Some(image) = image else {
            self.lock().map.remove(key);
            return;
        };
        let palette = Palette::from_rgba(image.width, image.height, &image.rgba);
        let thumb = Thumb::from_rgba(image.width, image.height, &image.rgba).map(Arc::new);
        let thumb_bytes = thumb.as_ref().map_or(0, |t| t.bytes());
        let mut g = self.lock();
        let full = g.keep.contains(key).then(|| Arc::new(Decoded { width: image.width, height: image.height, rgba: image.rgba.clone() }));
        let full_bytes = full.as_ref().map_or(0, |f| f.rgba.len());
        let Some(e) = g.map.get_mut(key) else { return };
        let pixels = e.pixels.clone();
        e.palette = Some(palette);
        e.thumb = thumb;
        e.ready = true;
        e.done = Some(std::time::Instant::now());
        e.bytes = image.rgba.len() + thumb_bytes + full_bytes;
        e.full = full;
        g.landed += 1;
        // Keep memory bounded: drop the least recently used finished pictures, by how many there
        // are and by what they take (a few large covers weigh as much as hundreds of thumbnails).
        // A picture on screen is held by its view as well, so it outlives its place here.
        let total: usize = g.map.values().map(|e| e.bytes).sum();
        if g.map.len() > KEEP + 40 || total > BUDGET {
            // Only what the last two views did not ask for can go: the page on screen keeps all
            // of its pictures, even past the budget.
            let recent = g.view.saturating_sub(1);
            let mut by_use: Vec<(u64, String)> = g.map.iter().filter(|(_, e)| e.ready && e.seen < recent).map(|(k, e)| (e.used, k.clone())).collect();
            by_use.sort_unstable();
            let (mut count, mut total) = (g.map.len(), total);
            for (_, k) in by_use {
                if count <= KEEP && total <= BUDGET * 3 / 4 {
                    break;
                }
                if let Some(e) = g.map.remove(&k) {
                    count -= 1;
                    total -= e.bytes;
                }
            }
        }
        drop(g);
        // Hand the pixels over outside the lock: RUNE wakes its views from here.
        pixels.set(image.width, image.height, image.rgba);
    }
}

impl Default for ArtCache {
    fn default() -> Self {
        ArtCache::new()
    }
}

/// Decodes a JPEG/PNG/WebP and shrinks it so its longest side is at most `max_side`.
pub fn decode(bytes: &[u8], max_side: u32) -> Option<Decoded> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = if img.width().max(img.height()) > max_side { img.resize(max_side, max_side, image::imageops::FilterType::Triangle) } else { img };
    let rgba = img.to_rgba8();
    Some(Decoded { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() })
}

pub fn encode_png(image: &Decoded) -> Option<Vec<u8>> {
    let rgba = image::RgbaImage::from_raw(image.width, image.height, image.rgba.clone())?;
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(rgba).write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_on_screen_are_kept_even_past_the_budget() {
        let art = ArtCache::new();
        art.begin_view();
        let keys: Vec<String> = (0..100).map(|i| format!("https://avatars.yandex.net/x{i}/400x400")).collect();
        for k in &keys {
            let _ = art.pixels(k);
        }
        for k in &keys {
            art.finish(k, Some(Decoded { width: 400, height: 400, rgba: vec![128; 400 * 400 * 4] }));
        }
        // The same page is built again: nothing it shows was dropped, so nothing loads again.
        art.begin_view();
        for k in &keys {
            let _ = art.pixels(k);
            assert!(art.ready(k), "{k} was evicted while on screen");
        }
        assert!(art.take_wanted().len() == 100, "only the first asks were loads");
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(w, h, |x, y| image::Rgba([(x * 255 / w) as u8, (y * 255 / h) as u8, 90, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    impl ArtCache {
        fn is_ready(&self, key: &str) -> bool {
            self.lock().map.get(key).is_some_and(|e| e.ready)
        }
    }

    #[test]
    fn decode_shrinks_and_keeps_pixels() {
        let d = decode(&png(200, 100), 50).unwrap();
        assert_eq!((d.width, d.height), (50, 25));
        assert_eq!(d.rgba.len(), 50 * 25 * 4);
        let small = decode(&png(20, 20), 50).unwrap();
        assert_eq!((small.width, small.height), (20, 20), "never upscaled");
        assert!(decode(b"definitely not an image", 50).is_none());
    }

    #[test]
    fn first_ask_schedules_a_load_once() {
        let cache = ArtCache::new();
        let a = cache.pixels("https://x/1");
        let _b = cache.pixels("https://x/1");
        assert_eq!(cache.take_wanted(), vec!["https://x/1".to_string()]);
        assert!(cache.take_wanted().is_empty());
        assert!(!cache.is_ready("https://x/1"));
        cache.finish("https://x/1", decode(&png(30, 30), 64));
        assert!(cache.is_ready("https://x/1"));
        assert!(cache.palette("https://x/1").is_some());
        assert_eq!(a.size(), (30, 30), "handles handed out earlier see the image");
    }

    #[test]
    fn a_failed_load_is_tried_again_by_the_next_view() {
        let cache = ArtCache::new();
        let _ = cache.pixels("https://x/broken");
        assert_eq!(cache.take_wanted(), vec!["https://x/broken".to_string()]);
        let landed = cache.landed();
        cache.finish("https://x/broken", None);
        assert_eq!(cache.landed(), landed, "a failure is not a picture landing");
        assert!(!cache.ready("https://x/broken"));
        // The next view asks for the same picture again: the load is scheduled once more.
        cache.begin_view();
        let _ = cache.pixels("https://x/broken");
        assert_eq!(cache.take_wanted(), vec!["https://x/broken".to_string()]);
        cache.finish("https://x/broken", decode(&png(20, 20), 64));
        assert!(cache.ready("https://x/broken"));
        assert_eq!(cache.landed(), landed + 1);
    }

    #[test]
    fn old_pictures_are_evicted() {
        let cache = ArtCache::new();
        for i in 0..500 {
            cache.begin_view();
            let key = format!("k{i}");
            cache.pixels(&key);
            cache.finish(&key, Some(Decoded { width: 2, height: 2, rgba: vec![200; 16] }));
        }
        let kept = cache.lock().map.len();
        assert!(kept <= KEEP + 40, "{kept} entries kept");
        assert!(cache.is_ready("k499"), "the newest survive");
    }

    #[test]
    fn a_few_large_covers_cannot_fill_memory() {
        let cache = ArtCache::new();
        // 1000 px covers are 4 MB each: well under the count limit, far over the memory one.
        for i in 0..30 {
            cache.begin_view();
            let key = format!("big{i}");
            cache.pixels(&key);
            cache.finish(&key, Some(Decoded { width: 1000, height: 1000, rgba: vec![90; 4_000_000] }));
        }
        let held: usize = cache.lock().map.values().map(|e| e.bytes).sum();
        assert!(held <= BUDGET, "{held} bytes kept");
        assert!(cache.is_ready("big29"), "the newest survive");
        assert!(!cache.is_ready("big0"), "the oldest went");
    }
}
