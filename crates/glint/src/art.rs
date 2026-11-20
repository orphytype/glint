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
    /// Playing covers that need the 256px sharp copy (at most two: current and previous).
    want_sharp: Vec<String>,
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
    /// The picture melted into its colours (`ArtCache::soft`), made the first time it is asked for.
    soft: Option<Pixels>,
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
    /// The 40px thumbnail, plus the 256px sharp copy when `with_sharp` is set (only for playing covers).
    pub fn from_rgba(w: u32, h: u32, rgba: &[u8], with_sharp: bool) -> Option<Thumb> {
        if w == 0 || h == 0 || rgba.len() < (w * h * 4) as usize {
            return None;
        }
        let (sharp_side, sharp) = if with_sharp {
            let side = SHARP.min(w.min(h)).max(THUMB);
            (side, shrink(w, h, rgba, side))
        } else {
            (0, Vec::new())
        };
        Some(Thumb { width: THUMB, height: THUMB, rgba: shrink(w, h, rgba, THUMB), sharp_side, sharp })
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

/// `rgba` (`w` x `h`) blurred three times over by a box 9 texels wide (close to a gaussian a
/// ninth of the side across) and its colours pushed a quarter further from their grey.
fn melt(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let (w, h) = (w as usize, h as usize);
    let mut a: Vec<f32> = rgba.iter().map(|&v| f32::from(v)).collect();
    let mut b = vec![0.0f32; a.len()];
    const R: isize = 4;
    for _ in 0..3 {
        for (along, across, step_along, step_across) in [(w, h, 4, w * 4), (h, w, w * 4, 4)] {
            for j in 0..across {
                for i in 0..along {
                    let mut acc = [0.0f32; 4];
                    for d in -R..=R {
                        let k = (i as isize + d).clamp(0, along as isize - 1) as usize;
                        let o = j * step_across + k * step_along;
                        for c in 0..4 {
                            acc[c] += a[o + c];
                        }
                    }
                    let o = j * step_across + i * step_along;
                    for c in 0..4 {
                        b[o + c] = acc[c] / (2 * R + 1) as f32;
                    }
                }
            }
            std::mem::swap(&mut a, &mut b);
        }
    }
    let mut out = vec![0u8; a.len()];
    for (px, o) in a.as_chunks::<4>().0.iter().zip(out.as_chunks_mut::<4>().0.iter_mut()) {
        let grey = px[0] * 0.2126 + px[1] * 0.7152 + px[2] * 0.0722;
        for c in 0..3 {
            o[c] = (grey + (px[c] - grey) * 1.25).round().clamp(0.0, 255.0) as u8;
        }
        o[3] = 255;
    }
    out
}

pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Finished pictures kept, most; and the memory they may take together before the oldest go.
/// Each live `Pixels` also keeps a copy in RUNE's CPU and GPU image atlas, so keep this compact.
const KEEP: usize = 96;
const BUDGET: usize = 24 << 20;

impl ArtCache {
    pub fn new() -> ArtCache {
        ArtCache(Arc::new(Mutex::new(Inner { map: HashMap::new(), wanted: Vec::new(), tick: 0, view: 1, landed: 0, keep: HashSet::new(), want_sharp: Vec::new() })))
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
        g.map.insert(
            key.to_owned(),
            Entry { pixels: pixels.clone(), palette: None, thumb: None, ready: false, done: None, used: tick, seen: view, bytes: 0, full: None, soft: None },
        );
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

    /// Marks `key` as a playing cover that needs the 256px sharp copy (for the cover morph and
    /// My Wave). Drops sharp copies from older covers so only the last two stay in RAM.
    pub fn want_sharp(&self, key: &str) {
        let mut g = self.lock();
        if let Some(pos) = g.want_sharp.iter().position(|k| k == key) {
            let k = g.want_sharp.remove(pos);
            g.want_sharp.push(k);
        } else {
            g.want_sharp.push(key.to_owned());
            while g.want_sharp.len() > 2 {
                let old = g.want_sharp.remove(0);
                if let Some(e) = g.map.get_mut(&old)
                    && let Some(t) = &e.thumb
                    && !t.sharp.is_empty()
                {
                    e.bytes = e.bytes.saturating_sub(t.sharp.len());
                    e.thumb = Some(Arc::new(Thumb { width: t.width, height: t.height, rgba: t.rgba.clone(), sharp_side: 0, sharp: Vec::new() }));
                }
            }
        }
        // If this cover was already decoded for a page without its sharp copy, reload it from disk.
        let needs_reload = g.map.get(key).is_some_and(|e| e.ready && e.thumb.as_ref().is_none_or(|t| t.sharp.is_empty()));
        if needs_reload && !g.wanted.iter().any(|k| k == key) {
            g.wanted.push(key.to_owned());
        }
    }

    /// The pixels of `key` for a shader, once they are in. Asking marks the picture as one whose
    /// pixels are kept; ask before it lands (when the view first shows it), or it is loaded again.
    pub fn full(&self, key: &str) -> Option<Arc<Decoded>> {
        let mut g = self.lock();
        if g.keep.insert(key.to_owned()) {
            // Only the last two are kept: a shape shows one picture at a time.
            if g.keep.len() > 2 {
                let drop: Vec<String> = g.keep.iter().filter(|k| *k != key).take(g.keep.len() - 2).cloned().collect();
                for k in drop {
                    g.keep.remove(&k);
                    if let Some(e) = g.map.get_mut(&k)
                        && let Some(f) = e.full.take()
                    {
                        e.bytes = e.bytes.saturating_sub(f.rgba.len());
                    }
                }
            }
        }
        g.map.get(key).and_then(|e| e.full.clone())
    }

    /// The picture of `key` melted into its colours: its thumbnail blurred far past recognition
    /// and a little richer, for a page to lay behind its head. Drawn as large as a window, it stays
    /// smooth (there is nothing sharp left in it to show its few texels). `None` until the picture
    /// is in.
    pub fn soft(&self, key: &str) -> Option<Pixels> {
        let mut g = self.lock();
        let e = g.map.get_mut(key)?;
        if let Some(p) = &e.soft {
            return Some(p.clone());
        }
        let t = e.thumb.clone()?;
        let px = Pixels::new();
        e.soft = Some(px.clone());
        drop(g);
        px.set(t.width, t.height, melt(t.width, t.height, &t.rgba));
        Some(px)
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
        let with_sharp = self.lock().want_sharp.iter().any(|k| k == key);
        let palette = Palette::from_rgba(image.width, image.height, &image.rgba);
        let thumb = Thumb::from_rgba(image.width, image.height, &image.rgba, with_sharp).map(Arc::new);
        let thumb_bytes = thumb.as_ref().map_or(0, |t| t.bytes());
        let mut g = self.lock();
        let full = g.keep.contains(key).then(|| Arc::new(Decoded { width: image.width, height: image.height, rgba: image.rgba.clone() }));
        let full_bytes = full.as_ref().map_or(0, |f| f.rgba.len());
        let Some(e) = g.map.get_mut(key) else { return };
        let was_ready = e.ready;
        let pixels = e.pixels.clone();
        e.palette = Some(palette);
        e.thumb = thumb;
        e.ready = true;
        if !was_ready {
            e.done = Some(std::time::Instant::now());
        }
        e.bytes = image.rgba.len() + thumb_bytes + full_bytes;
        e.full = full;
        g.landed += 1;
        // Keep memory bounded: drop the least recently used finished pictures, by how many there
        // are and by what they take (a few large covers weigh as much as hundreds of thumbnails).
        // A picture on screen is held by its view as well, so it outlives its place here.
        let total: usize = g.map.values().map(|e| e.bytes).sum();
        if g.map.len() > KEEP + 24 || total > BUDGET {
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
        if !was_ready {
            pixels.set(image.width, image.height, image.rgba);
        }
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
