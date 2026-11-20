//! The user's own picture behind the open player, in place of the cover's colours. It is copied
//! into Glint's folder when picked (so moving or deleting the original changes nothing), decoded
//! on a worker, and handed to the GPU with every smaller copy of itself down to one texel: the
//! stage samples the copy as soft as the chosen blur, then lays the chosen effect over it
//! (`fs_stage` in gfx.wgsl).

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::app::State;
use crate::i18n::S;

/// The longest side the picture is kept at: sharp on a large screen, and a few megabytes at most.
const MAX_SIDE: u32 = 2048;

/// The effects that can be laid over it, in the order Settings lists them.
pub const EFFECTS: [S; 6] = [S::BdNone, S::BdDither, S::BdGrain, S::BdPixels, S::BdHalftone, S::BdLines];

/// A picture and its smaller copies, largest first (sRGB RGBA, each half the one before).
pub struct Image {
    /// Unique per load, so the GPU uploads each picture once.
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub levels: Vec<(u32, u32, Vec<u8>)>,
}

impl Image {
    pub fn aspect(&self) -> f32 {
        self.width as f32 / self.height.max(1) as f32
    }
}

/// Decodes `bytes` and makes its chain of smaller copies.
pub fn build(bytes: &[u8]) -> Option<Image> {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let img = image::load_from_memory(bytes).ok()?;
    let img = if img.width().max(img.height()) > MAX_SIDE { img.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle) } else { img };
    let mut cur = img.to_rgba8();
    let (width, height) = cur.dimensions();
    if width == 0 || height == 0 {
        return None;
    }
    let mut levels = Vec::new();
    loop {
        let (w, h) = cur.dimensions();
        let next = (w > 1 || h > 1).then(|| image::imageops::resize(&cur, (w / 2).max(1), (h / 2).max(1), image::imageops::FilterType::Triangle));
        levels.push((w, h, cur.into_raw()));
        match next {
            Some(n) => cur = n,
            None => break,
        }
    }
    Some(Image { id: NEXT.fetch_add(1, Ordering::Relaxed), width, height, levels })
}

fn read(path: &Path) -> Option<Image> {
    build(&std::fs::read(path).ok()?)
}

impl State {
    /// Loads the picture when it is switched on or replaced, and lets it go when it is switched off.
    pub fn sync_backdrop(&mut self) {
        let want = self.settings.backdrop.on.then_some(self.settings.backdrop.rev);
        if want == self.backdrop_rev {
            return;
        }
        self.backdrop_rev = want;
        let Some(rev) = want else {
            self.backdrop = None;
            return;
        };
        let Some(path) = self.store.backdrop_file() else { return };
        self.job(async move { tokio::task::spawn_blocking(move || read(&path)).await.ok().flatten() }, move |s, _, img| {
            if s.backdrop_rev == Some(rev) {
                s.backdrop = img.map(Arc::new);
            }
        });
    }

    /// Asks for a picture and, when one is chosen, copies it in and shows it.
    pub fn pick_backdrop(&mut self, tx: &mut rune::Tx) {
        let options = rune::FileDialogOptions { title: self.tr(S::BdPick).to_owned(), ..Default::default() }.filter(self.tr(S::BdImages), &["jpg", "jpeg", "png", "webp"]);
        tx.open_file(options, |s: &mut State, result, _tx| {
            let Ok(Some(path)) = result else { return };
            let Some(dest) = s.store.backdrop_file() else { return };
            s.job(
                async move {
                    tokio::task::spawn_blocking(move || {
                        let bytes = std::fs::read(&path).ok()?;
                        build(&bytes)?;
                        std::fs::write(&dest, &bytes).ok()
                    })
                    .await
                    .ok()
                    .flatten()
                    .is_some()
                },
                |s, _, ok| {
                    if ok {
                        s.settings.backdrop.on = true;
                        s.settings.backdrop.rev = s.settings.backdrop.rev.wrapping_add(1);
                        s.mark_settings_dirty();
                    } else {
                        let text = s.tr(S::BdFailed);
                        s.toast(text);
                    }
                },
            );
        });
    }

    /// Back to the cover's colours; the copied picture is deleted.
    pub fn drop_backdrop(&mut self) {
        self.settings.backdrop.on = false;
        if let Some(path) = self.store.backdrop_file() {
            let _ = std::fs::remove_file(path);
        }
        self.mark_settings_dirty();
    }
}
