//! Cover art loading, palette extraction, and synced lyrics lookup.

use std::sync::Arc;

use ym_api::Track;
use ym_audio::DiskCache;

use super::{LYRIC_LEAD_MS, Load, LyricsUi, State};
use crate::art::{ArtCache, COVER_CARD, decode_px_for_key};
use crate::net::{self, Backend};

impl State {
    /// Lyrics clock, slightly ahead of playback so lines light up just before they are sung.
    pub fn lyric_ms(&self) -> u32 {
        (self.player.pos_ms + LYRIC_LEAD_MS).min(u64::from(u32::MAX)) as u32
    }

    pub fn current(&self) -> Option<&Track> {
        self.player.index.and_then(|i| self.player.queue.get(i))
    }

    /// Cover URL (also the cache key) at an explicit CDN size (`100`, `200`, or `600`).
    pub fn cover_key_px(template: Option<&str>, px: u32) -> Option<String> {
        template.map(|t| ym_api::cover::url(t, px))
    }

    /// Canonical 200x200 cover URL for cards, headers, palettes, and desktop media controls.
    pub fn cover_key(template: Option<&str>) -> Option<String> {
        Self::cover_key_px(template, COVER_CARD)
    }

    pub fn load_lyrics_for_current(&mut self) {
        let Some(track) = self.current().cloned() else { return };
        if self.lyrics.for_track.as_deref() == Some(track.id.as_str()) {
            return;
        }
        self.lyrics = LyricsUi { for_track: Some(track.id.clone()), lyrics: Load::Loading, active: None, hover: self.lyrics.hover, user_scroll_at: None };
        self.lyrics_gen = self.lyrics_gen.wrapping_add(1);
        let (b, fc, fc_on) = (self.backend.clone(), self.fkcensor.clone(), self.settings.fkcensor);
        let id = track.id.clone();
        let lookup_id = id.clone();
        let downloads = self.download_store.clone();
        self.job(
            async move {
                if let Some(store) = &downloads
                    && let Some(item) = store.get(&lookup_id)
                {
                    return item.lyrics;
                }
                b.lyrics(&track, &fc, fc_on).await.unwrap_or_default()
            },
            move |s, _, l| {
                if s.lyrics.for_track.as_deref() == Some(id.as_str()) {
                    s.lyrics.lyrics = Load::Ready(l);
                    s.lyrics_gen = s.lyrics_gen.wrapping_add(1);
                    s.scroll_lyrics = true;
                }
            },
        );
    }

    /// Starts cover loads queued by the view and applies the playing track's palette when ready.
    pub(super) fn drive_art(&mut self) -> bool {
        for key in self.art.take_wanted() {
            if self.headless {
                #[cfg(feature = "tooling")]
                if let Some(img) = paint_fixture_cover(&key) {
                    self.art.finish(&key, Some(img), None);
                }
                continue;
            }
            spawn_cover_load(self.backend.clone(), self.art.clone(), key, self.cache.clone());
        }
        // Only rebuild the view when a watched cover or playing cover lands; list covers repaint via `Pixels::set`.
        let landed = self.art.watched_landed();
        let fresh = landed != self.art_landed;
        self.art_landed = landed;
        let Some(track) = self.current() else { return fresh };
        let Some(tpl) = track.cover_template() else { return fresh };
        if self.palette_key.is_some() && self.palette_tpl.as_deref() == Some(tpl) {
            return fresh;
        }
        let key = ym_api::cover::url(tpl, COVER_CARD);
        if self.palette_key.as_deref() != Some(key.as_str()) {
            let _ = self.art.pixels(&key);
        }
        if let Some(p) = self.art.palette(&key)
            && let Some(thumb) = self.art.thumb(&key)
        {
            let tpl = tpl.to_owned();
            self.palette = p;
            self.cover_thumb = Some(thumb);
            self.palette_key = Some(key);
            self.palette_tpl = Some(tpl);
            return true;
        }
        fresh
    }
}

#[cfg(feature = "tooling")]
fn paint_fixture_cover(key: &str) -> Option<crate::art::Decoded> {
    let (hue, style, seed, size) = crate::fixtures::parse_cover_url(key)?;
    let size = size.min(decode_px_for_key(key));
    let rgba = crate::fixtures::cover_rgba(hue, style, seed, size);
    Some(crate::art::Decoded { width: size, height: size, rgba })
}

fn spawn_cover_load(backend: Backend, art: ArtCache, key: String, disk: Option<Arc<DiskCache>>) {
    net::spawn(async move {
        let decode_px = decode_px_for_key(&key);
        // Fast path: compressed bytes warm in RAM or on disk, decoded on the bounded blocking pool.
        let from_ram = art.encoded(&key);
        let hit = {
            let _permit = net::decode_limiter().acquire().await.ok();
            let art2 = art.clone();
            let k2 = key.clone();
            let disk2 = disk.clone();
            tokio::task::spawn_blocking(move || {
                let raw = from_ram.or_else(|| disk2.as_ref().and_then(|d| d.get(&k2)))?;
                let img = crate::art::decode(&raw, decode_px)?;
                art2.finish(&k2, Some(img), Some(raw));
                Some(())
            })
            .await
            .ok()
            .flatten()
            .is_some()
        };
        if hit {
            return;
        }
        if let Some(raw) = backend.image_bytes(&key).await {
            let _permit = net::decode_limiter().acquire().await.ok();
            let art2 = art.clone();
            let k2 = key.clone();
            let disk2 = disk.clone();
            let _ = tokio::task::spawn_blocking(move || {
                if let Some(img) = crate::art::decode(&raw, decode_px) {
                    if let Some(d) = disk2 {
                        let _ = d.put(&k2, &raw);
                    }
                    art2.finish(&k2, Some(img), Some(raw));
                } else {
                    art2.finish(&k2, None, None);
                }
            })
            .await;
            return;
        }
        if let Some(img) = backend.image(&key, decode_px).await {
            let _ = tokio::task::spawn_blocking(move || art.finish(&key, Some(img), None)).await;
        } else {
            art.finish(&key, None, None);
        }
    });
}
