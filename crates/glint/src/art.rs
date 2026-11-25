//! Cover art. A view asks for a picture by URL and gets a `Pixels` handle straight away; the
//! image arrives later (loaded from disk or network and decoded on a worker) and RUNE redraws the
//! picture without running the view again.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rune::Pixels;

use crate::palette::Palette;

/// Small cover size for track lists, queues, mini avatars, mini-player bar, and pinned shortcuts.
pub const COVER_SM: u32 = 100;
/// Card cover size for grids, shelves, page headers, Stage Home, palettes, and desktop media cards.
pub const COVER_CARD: u32 = 200;
/// Large cover size loaded only inside open Now Playing for the active track's hero cover.
pub const COVER_LG: u32 = 600;
/// Default cover size shared by desktop media cards, downloads, and palettes.
pub const COVER_PX: u32 = COVER_CARD;

/// Target decoded side derived from a cover URL's trailing `WxH` segment (`100`, `200`, or `600`).
/// Capped at `200` in CPU mode to keep software-rendered RAM minimal.
pub fn decode_px_for_key(key: &str) -> u32 {
    let cpu = crate::fx::cpu_mode();
    let max_px = if cpu { COVER_CARD } else { COVER_LG };
    let path = key.split(['?', '#']).next().unwrap_or(key);
    let seg = path.rsplit('/').next().unwrap_or("");
    if let Some((w, h)) = seg.split_once('x')
        && let (Ok(w), Ok(h)) = (w.parse::<u32>(), h.parse::<u32>())
        && w > 0
        && h > 0
    {
        let side = w.max(h);
        if cpu && side == COVER_CARD {
            return 160;
        }
        return side.clamp(64, max_px);
    }
    COVER_CARD.min(max_px)
}

#[derive(Clone)]
pub struct ArtCache(Arc<Mutex<Inner>>);

struct Inner {
    map: HashMap<String, Entry>,
    /// Compact compressed bytes kept in RAM so revisiting recently evicted pages or sending a
    /// media card cover avoids re-reading disk or network.
    raw: HashMap<String, (Arc<[u8]>, u64)>,
    raw_bytes: usize,
    wanted: Vec<String>,
    in_flight: HashSet<String>,
    failed: HashMap<String, Instant>,
    tick: u64,
    /// Bumped only when the user actually navigates to a new page or tab, used to prioritize
    /// previous-page covers during offscreen eviction.
    page: u64,
    /// Bumped at the start of each view pass so entries currently mounted in the UI tree
    /// (`e.seen_view == view_gen`) are never evicted mid-frame or while scrolling.
    view_gen: u64,
    /// Keys whose palette or soft backdrop was asked for while not yet decoded.
    watched: HashSet<String>,
    /// Bumped only when a watched cover lands, so loading list covers never forces a full rebuild.
    watched_landed: u64,
}

struct Entry {
    pixels: Pixels,
    palette: Option<Palette>,
    /// A tiny copy of the picture, for the shaders (the picture RUNE holds cannot be read back).
    thumb: Option<Arc<Thumb>>,
    ready: bool,
    used: u64,
    /// Page generation that last requested this cover.
    page: u64,
    /// View pass generation that last mounted this cover.
    seen_view: u64,
    /// What the decoded picture takes in memory.
    bytes: usize,
    /// The picture melted into its colours (`ArtCache::soft`), made the first time it is asked for.
    soft: Option<Pixels>,
    /// Pre-composited opaque 40x40 stage backdrop `(is_light, pixels)` for zero-overdraw Now Playing.
    stage_bg: Option<(bool, Pixels)>,
}

/// A cover shrunk for the shaders (sRGB, RGBA, square): `THUMB` texels a side for the blurred
/// backdrop.
pub struct Thumb {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The thumbnail's side in texels: small on purpose, it is only ever seen through a heavy blur.
pub const THUMB: u32 = 40;

impl Thumb {
    /// The 40px thumbnail for backdrop colour sampling and blur.
    pub fn from_rgba(w: u32, h: u32, rgba: &[u8]) -> Option<Thumb> {
        if w == 0 || h == 0 || rgba.len() < (w * h * 4) as usize {
            return None;
        }
        Some(Thumb { width: THUMB, height: THUMB, rgba: shrink(w, h, rgba, THUMB) })
    }

    /// What the thumbnail takes in memory.
    pub fn bytes(&self) -> usize {
        self.rgba.len()
    }
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

/// Hard ceiling for offscreen decoded covers while actively scrolling (mounted covers are never
/// evicted so visible rows never flash or reload).
fn keep_limits() -> (usize, usize) {
    if crate::fx::cpu_mode() { (24, 1_800_000) } else { (32, 3_600_000) }
}

/// Tight offscreen cushion retained after scrolling away or changing page so RAM stays well under
/// 90 MiB while keeping immediate scroll-back warm.
fn offscreen_limits() -> (usize, usize) {
    if crate::fx::cpu_mode() { (4, 512 * 1024) } else { (8, 1_024 * 1024) }
}

fn raw_limits() -> (usize, usize) {
    if crate::fx::cpu_mode() { (12, 256 * 1024) } else { (32, 512 * 1024) }
}
/// Wait before retrying a cover that failed to load, so broken URLs do not spin every frame.
const FAIL_COOLDOWN: Duration = Duration::from_secs(20);

impl Inner {
    fn trim_raw(&mut self, keep: usize, budget: usize, keep_keys: &[&str]) -> bool {
        if self.raw.len() <= keep && self.raw_bytes <= budget {
            return false;
        }
        let mut by_use: Vec<(u64, String)> = self.raw.iter().filter(|(k, _)| !keep_keys.contains(&k.as_str())).map(|(k, (_, u))| (*u, k.clone())).collect();
        by_use.sort_unstable();
        let mut evicted = false;
        for (_, k) in by_use {
            if self.raw.len() <= keep && self.raw_bytes <= budget * 3 / 4 {
                break;
            }
            if let Some((old, _)) = self.raw.remove(&k) {
                self.raw_bytes = self.raw_bytes.saturating_sub(old.len());
                evicted = true;
            }
        }
        if evicted && self.raw.capacity() > self.raw.len() * 2 + 16 {
            self.raw.shrink_to_fit();
        }
        evicted
    }

    fn put_raw(&mut self, key: &str, bytes: Arc<[u8]>) {
        self.tick += 1;
        let tick = self.tick;
        let len = bytes.len();
        if let Some((old, _)) = self.raw.insert(key.to_owned(), (bytes, tick)) {
            self.raw_bytes = self.raw_bytes.saturating_sub(old.len());
        }
        self.raw_bytes = self.raw_bytes.saturating_add(len);
        let (raw_keep, raw_budget) = raw_limits();
        if self.raw.len() > raw_keep + 8 || self.raw_bytes > raw_budget {
            self.trim_raw(raw_keep, raw_budget, &[]);
        }
    }
}

impl ArtCache {
    pub fn new() -> ArtCache {
        ArtCache(Arc::new(Mutex::new(Inner {
            map: HashMap::new(),
            raw: HashMap::new(),
            raw_bytes: 0,
            wanted: Vec::new(),
            in_flight: HashSet::new(),
            failed: HashMap::new(),
            tick: 0,
            page: 1,
            view_gen: 1,
            watched: HashSet::new(),
            watched_landed: 0,
        })))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Starts a new view pass: advances `view_gen` so every cover mounted during this pass is
    /// stamped as live and protected from eviction, while unmounted offscreen covers can be freed.
    pub fn begin_view(&self) {
        let mut g = self.lock();
        g.watched.clear();
        g.view_gen = g.view_gen.wrapping_add(1);
    }

    /// Clears the per-view watched set without advancing `view_gen` (used while full-screen Now
    /// Playing covers the page underneath so its covers stay cached and never flash on close).
    pub fn clear_watched(&self) {
        self.lock().watched.clear();
    }

    /// Marks navigation to a new page or collection tab so previous-page covers evict first.
    pub fn bump_page(&self) {
        self.lock().page += 1;
    }

    /// The picture for `key` (a cover URL). The first ask schedules the load.
    pub fn pixels(&self, key: &str) -> Pixels {
        let mut g = self.lock();
        g.tick += 1;
        let (tick, page, view_gen) = (g.tick, g.page, g.view_gen);
        if let Some(e) = g.map.get_mut(key) {
            e.used = tick;
            e.page = page;
            e.seen_view = view_gen;
            return e.pixels.clone();
        }
        if let Some(&at) = g.failed.get(key) {
            if at.elapsed() < FAIL_COOLDOWN {
                return Pixels::new();
            }
            g.failed.remove(key);
        }
        let pixels = Pixels::new();
        g.map.insert(
            key.to_owned(),
            Entry { pixels: pixels.clone(), palette: None, thumb: None, ready: false, used: tick, page, seen_view: view_gen, bytes: 0, soft: None, stage_bg: None },
        );
        if g.in_flight.insert(key.to_owned()) {
            g.wanted.push(key.to_owned());
        }
        pixels
    }

    pub fn palette(&self, key: &str) -> Option<Palette> {
        let mut g = self.lock();
        if let Some(p) = g.map.get(key).and_then(|e| e.palette) {
            return Some(p);
        }
        g.watched.insert(key.to_owned());
        None
    }

    /// Whether the picture is decoded and handed to its `Pixels`.
    pub fn ready(&self, key: &str) -> bool {
        self.lock().map.get(key).is_some_and(|e| e.ready)
    }

    /// Marks `key` so a single view rebuild fires when it finishes decoding (used by 600px hero
    /// covers to drop their temporary 300px fallback).
    pub fn watch(&self, key: &str) {
        let mut g = self.lock();
        if !g.map.get(key).is_some_and(|e| e.ready) {
            g.watched.insert(key.to_owned());
        }
    }

    /// How many watched or playing pictures have landed so far.
    pub fn watched_landed(&self) -> u64 {
        self.lock().watched_landed
    }

    /// Compressed bytes in RAM for `key`, if still warm.
    pub fn encoded(&self, key: &str) -> Option<Arc<[u8]>> {
        let mut g = self.lock();
        g.tick += 1;
        let tick = g.tick;
        let entry = g.raw.get_mut(key)?;
        entry.1 = tick;
        Some(entry.0.clone())
    }

    /// Saves compressed bytes in the RAM cache.
    pub fn store_encoded(&self, key: &str, bytes: Arc<[u8]>) {
        self.lock().put_raw(key, bytes);
    }

    /// Clears the in-memory compressed byte cache (called when the user deletes cached content).
    pub fn clear_raw(&self) {
        let mut g = self.lock();
        g.raw.clear();
        g.raw_bytes = 0;
    }

    /// Evicts unmounted covers (`e.seen_view < cur_view`, except `keep_keys`) while NEVER touching
    /// covers currently mounted in the active view pass (guaranteeing zero visible flickering).
    /// Immediately frees unmounted 600px hero covers and trims remaining offscreen covers down to
    /// `offscreen_limits`. Returns true if any decoded texture or raw entry was freed.
    pub fn trim_offscreen(&self, keep_keys: &[&str]) -> bool {
        let mut g = self.lock();
        let (raw_keep, raw_budget) = raw_limits();
        let mut evicted = g.trim_raw(raw_keep, raw_budget, keep_keys);
        let cur_view = g.view_gen;
        let cur_page = g.page;

        // 1. Immediately drop any unmounted 600x600 hero cover (>300 KiB RGBA) not in keep_keys.
        let before = g.map.len();
        g.map.retain(|k, e| !e.ready || e.seen_view == cur_view || keep_keys.contains(&k.as_str()) || e.bytes <= 300_000);
        evicted |= g.map.len() < before;

        // 2. Drop melted backdrops on unmounted non-kept covers so stale pages release textures.
        for (k, e) in &mut g.map {
            if e.seen_view < cur_view && !keep_keys.contains(&k.as_str()) {
                if e.soft.take().is_some() {
                    evicted = true;
                }
                if e.stage_bg.take().is_some() {
                    evicted = true;
                }
            }
        }

        // 3. Trim remaining unmounted offscreen covers down to a small warm scroll-back cushion,
        //    evicting previous-page entries first, then oldest LRU.
        let mut offscreen: Vec<(bool, u64, usize, String)> = g
            .map
            .iter()
            .filter(|(k, e)| e.ready && e.seen_view < cur_view && !keep_keys.contains(&k.as_str()))
            .map(|(k, e)| (e.page >= cur_page, e.used, e.bytes, k.clone()))
            .collect();
        let (off_keep, off_budget) = offscreen_limits();
        let mut off_count = offscreen.len();
        let mut off_bytes: usize = offscreen.iter().map(|x| x.2).sum();
        if off_count > off_keep || off_bytes > off_budget {
            offscreen.sort_unstable();
            for (_, _, b, k) in offscreen {
                if off_count <= off_keep && off_bytes <= off_budget {
                    break;
                }
                if g.map.remove(&k).is_some() {
                    off_count -= 1;
                    off_bytes = off_bytes.saturating_sub(b);
                    evicted = true;
                }
            }
        }
        if evicted && g.map.capacity() > g.map.len() * 2 + 24 {
            g.map.shrink_to_fit();
        }
        evicted
    }

    /// Drops all decoded covers except `keep_key` when the main window hides to tray or mini player,
    /// and trims compressed raw bytes in both GPU and CPU modes so hidden RAM stays minimal.
    pub fn trim_hidden(&self, keep_key: Option<&str>) {
        let mut g = self.lock();
        g.map.retain(|k, e| !e.ready || keep_key == Some(k.as_str()));
        for e in g.map.values_mut() {
            e.soft = None;
            e.stage_bg = None;
        }
        let keep = keep_key.map(|k| [k]);
        let keep_slice: &[&str] = match &keep {
            Some(a) => a,
            None => &[],
        };
        g.trim_raw(6, 192 * 1024, keep_slice);
        if g.map.capacity() > g.map.len() * 2 + 8 {
            g.map.shrink_to_fit();
        }
    }

    /// Diagnostic counts: `(decoded_count, decoded_bytes, raw_count, raw_bytes)`.
    pub fn stats(&self) -> (usize, usize, usize, usize) {
        let g = self.lock();
        let decoded_bytes: usize = g.map.values().map(|e| e.bytes).sum();
        (g.map.len(), decoded_bytes, g.raw.len(), g.raw_bytes)
    }

    /// The picture of `key` melted into its colours: its thumbnail blurred far past recognition
    /// and a little richer, for a page to lay behind its head. `None` until the picture is in.
    pub fn soft(&self, key: &str) -> Option<Pixels> {
        let mut g = self.lock();
        let Some(e) = g.map.get_mut(key) else {
            g.watched.insert(key.to_owned());
            return None;
        };
        if let Some(p) = &e.soft {
            return Some(p.clone());
        }
        let Some(t) = e.thumb.clone() else {
            g.watched.insert(key.to_owned());
            return None;
        };
        let px = Pixels::new();
        e.soft = Some(px.clone());
        drop(g);
        px.set(t.width, t.height, melt(t.width, t.height, &t.rgba));
        Some(px)
    }

    /// Pre-composited opaque 40x40 Now Playing backdrop (melted cover blended onto canvas + wash),
    /// allowing shaders-off Now Playing to draw its background in a single opaque quad with zero
    /// full-window alpha overdraw.
    pub fn soft_stage(&self, key: &str, canvas: [f32; 3], light: bool) -> Option<Pixels> {
        let mut g = self.lock();
        let Some(e) = g.map.get_mut(key) else {
            g.watched.insert(key.to_owned());
            return None;
        };
        if let Some((was_light, p)) = &e.stage_bg
            && *was_light == light
        {
            return Some(p.clone());
        }
        let Some(t) = e.thumb.clone() else {
            g.watched.insert(key.to_owned());
            return None;
        };
        let px = Pixels::new();
        e.stage_bg = Some((light, px.clone()));
        drop(g);
        let mut rgba = melt(t.width, t.height, &t.rgba);
        let alpha = if light { 0.34 } else { 0.54 };
        let (wash_rgb, wash_a) = if light { ([255.0f32, 255.0, 255.0], 0.32) } else { ([0.0f32, 0.0, 0.0], 0.38) };
        let bg = [canvas[0] * 255.0, canvas[1] * 255.0, canvas[2] * 255.0];
        for chunk in rgba.as_chunks_mut::<4>().0 {
            for c in 0..3 {
                let over_bg = bg[c] * (1.0 - alpha) + f32::from(chunk[c]) * alpha;
                let final_c = over_bg * (1.0 - wash_a) + wash_rgb[c] * wash_a;
                chunk[c] = final_c.round().clamp(0.0, 255.0) as u8;
            }
            chunk[3] = 255;
        }
        px.set(t.width, t.height, rgba);
        Some(px)
    }

    pub fn thumb(&self, key: &str) -> Option<Arc<Thumb>> {
        self.lock().map.get(key).and_then(|e| e.thumb.clone())
    }

    /// Keys asked for since the last call, each of which needs loading.
    pub fn take_wanted(&self) -> Vec<String> {
        std::mem::take(&mut self.lock().wanted)
    }

    /// Stores a loaded image (or, with `None`, records a short backoff) and works out its palette.
    pub fn finish(&self, key: &str, image: Option<Decoded>, raw: Option<Arc<[u8]>>) {
        let Some(image) = image else {
            let mut g = self.lock();
            g.in_flight.remove(key);
            g.map.remove(key);
            g.failed.insert(key.to_owned(), Instant::now());
            return;
        };
        let side = decode_px_for_key(key);
        let want_meta = (150..=COVER_CARD).contains(&side);
        let palette = want_meta.then(|| Palette::from_rgba(image.width, image.height, &image.rgba));
        let thumb = want_meta.then(|| Thumb::from_rgba(image.width, image.height, &image.rgba).map(Arc::new)).flatten();
        let thumb_bytes = thumb.as_ref().map_or(0, |t| t.bytes());
        let mut g = self.lock();
        g.in_flight.remove(key);
        g.failed.remove(key);
        if let Some(bytes) = raw {
            g.put_raw(key, bytes);
        }
        let Some(e) = g.map.get_mut(key) else { return };
        let was_ready = e.ready;
        let pixels = e.pixels.clone();
        e.palette = palette;
        e.thumb = thumb;
        e.ready = true;
        e.bytes = image.rgba.len() + thumb_bytes;
        if g.watched.remove(key) {
            g.watched_landed += 1;
        }
        // Keep memory bounded while NEVER evicting covers currently mounted in the active view pass
        // (`e.seen_view == cur_view`), preventing visible cover flicker during scrolling.
        let (keep, budget) = keep_limits();
        let total: usize = g.map.values().map(|e| e.bytes).sum();
        if g.map.len() > keep + 10 || total > budget {
            let cur_page = g.page;
            let cur_view = g.view_gen;
            let mut by_prio: Vec<(bool, u64, String)> = g
                .map
                .iter()
                .filter(|(k, e)| e.ready && k.as_str() != key && e.seen_view < cur_view)
                .map(|(k, e)| (e.page >= cur_page, e.used, k.clone()))
                .collect();
            by_prio.sort_unstable();
            let (mut count, mut total) = (g.map.len(), total);
            for (_, _, k) in by_prio {
                if count <= keep && total <= budget * 3 / 4 {
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
    use image::ImageEncoder as _;
    let expected = (image.width as usize).checked_mul(image.height as usize)?.checked_mul(4)?;
    if image.rgba.len() < expected {
        return None;
    }
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out).write_image(&image.rgba[..expected], image.width, image.height, image::ExtendedColorType::Rgba8).ok()?;
    Some(out)
}
