//! Offline track downloads, batch deletion and embedded metadata tagging.

use std::collections::HashSet;

use ym_api::Track;

use crate::app::{DownloadActivity, DownloadFail, DownloadState, Load, Page, State};
use crate::i18n::S;
use crate::store::DownloadedTrack;

/// How many tracks of a batch download at once.
const DOWNLOAD_PARALLEL: usize = 8;

impl State {
    pub fn ask_download(&mut self, tracks: Vec<Track>) {
        let tracks: Vec<Track> = tracks.into_iter().filter(|track| !matches!(self.downloads.get(&track.id), Some(DownloadState::Downloading | DownloadState::Done))).collect();
        if tracks.len() > 1 {
            self.download_confirm = Some(tracks);
        } else {
            self.download_tracks(tracks);
        }
    }

    pub fn confirm_download(&mut self) {
        if let Some(tracks) = self.download_confirm.take() {
            self.download_tracks(tracks);
        }
    }

    pub fn cancel_download(&mut self) {
        self.download_confirm = None;
    }

    pub fn ask_delete_download(&mut self, track: Track) {
        if matches!(self.downloads.get(&track.id), Some(DownloadState::Done)) {
            self.delete_download_confirm = Some(vec![track]);
        }
    }

    pub fn ask_delete_downloads(&mut self, tracks: Vec<Track>) {
        let tracks: Vec<Track> = tracks.into_iter().filter(|track| matches!(self.downloads.get(&track.id), Some(DownloadState::Done))).collect();
        if !tracks.is_empty() {
            self.delete_download_confirm = Some(tracks);
        }
    }

    pub fn confirm_delete_download(&mut self) {
        let Some(tracks) = self.delete_download_confirm.take() else { return };
        let Some(store) = &self.download_store else { return };
        let mut failed = false;
        let mut removed = HashSet::with_capacity(tracks.len());
        for track in tracks {
            if store.remove(&track.id).is_err() {
                failed = true;
                continue;
            }
            self.downloads.remove(&track.id);
            self.download_errors.remove(&track.id);
            self.deleted_downloads.insert(track.id.clone());
            self.download_titles.remove(&track.id);
            self.downloaded_tracks.remove(&track.id);
            removed.insert(track.id);
        }
        if !removed.is_empty() {
            self.download_order.retain(|id| !removed.contains(id));
        }
        if failed {
            self.toast(self.tr(S::DownloadDeleteFailed));
        } else {
            self.toast(self.tr(S::DownloadDeleted));
        }
    }

    pub fn cancel_delete_download(&mut self) {
        self.delete_download_confirm = None;
    }

    /// Downloads every track belonging to the open release or artist. Artist pages initially
    /// contain only popular tracks, so fetch the complete list before starting the batch.
    pub fn download_detail(&mut self) {
        let Some(page) = self.detail.page.clone() else { return };
        if self.detail.is_artist {
            if let Load::Ready(tracks) = &self.detail.all_tracks {
                self.ask_download(tracks.clone());
                return;
            }
            if self.detail.all_tracks.is_loading() {
                return;
            }
            let generation = self.detail.generation;
            let Page::Artist(id) = page else { return };
            self.detail.all_tracks = Load::Loading;
            let backend = self.backend.clone();
            self.job(async move { backend.artist_tracks(&id).await }, move |s, _, result| {
                if s.detail.generation != generation {
                    return;
                }
                match result {
                    Ok(tracks) => {
                        s.detail.all_tracks = Load::Ready(tracks.clone());
                        s.ask_download(tracks);
                    }
                    Err(error) => {
                        s.detail.all_tracks = Load::Failed(error.to_string());
                        s.error_toast(&error);
                    }
                }
            });
            return;
        }
        self.ask_download(self.detail.tracks.clone());
    }

    pub fn delete_detail_downloads(&mut self) {
        let tracks = if self.detail.is_artist {
            match &self.detail.all_tracks {
                Load::Ready(tracks) => tracks.clone(),
                _ => return,
            }
        } else {
            self.detail.tracks.clone()
        };
        self.ask_delete_downloads(tracks);
    }

    pub fn download_tracks(&mut self, tracks: Vec<Track>) {
        if tracks.is_empty() {
            return;
        }
        let mut incoming = HashSet::with_capacity(tracks.len());
        let mut front = Vec::with_capacity(tracks.len());
        for track in &tracks {
            self.deleted_downloads.remove(&track.id);
            self.download_errors.remove(&track.id);
            self.downloads.insert(track.id.clone(), DownloadState::Downloading);
            self.download_titles.insert(track.id.clone(), track.full_title());
            self.downloaded_tracks.insert(track.id.clone(), track.clone());
            if incoming.insert(track.id.clone()) {
                front.push(track.id.clone());
            }
        }
        // Reverse `front` so the newest track in the batch stands first, matching prior `insert(0)`.
        front.reverse();
        self.download_order.retain(|id| !incoming.contains(id));
        front.append(&mut self.download_order);
        self.download_order = front;

        let Some(downloads) = self.download_store.clone() else {
            for track in &tracks {
                self.downloads.insert(track.id.clone(), DownloadState::Failed);
                self.download_errors.insert(track.id.clone(), DownloadFail::Storage);
            }
            self.toast(self.tr(S::PlaybackFailed));
            return;
        };
        let backend = self.backend.clone();
        let cache = self.cache.clone();
        let quality = self.settings.quality.api();
        let fkcensor = self.fkcensor.clone();
        let fkcensor_on = self.settings.fkcensor;
        if let Some(activity) = &mut self.download_activity
            && activity.active > 0
        {
            activity.total += tracks.len();
            activity.active += tracks.len();
            activity.changed_at = self.now;
        } else {
            self.download_activity = Some(DownloadActivity { total: tracks.len(), active: tracks.len(), done: 0, failed: 0, changed_at: self.now });
        }
        let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(DOWNLOAD_PARALLEL));
        for track in tracks {
            let id = track.id.clone();
            let job_backend = backend.clone();
            let job_downloads = downloads.clone();
            let job_cache = cache.clone();
            let job_art = self.art.clone();
            let job_fkcensor = fkcensor.clone();
            let gate = gate.clone();
            self.job(
                async move {
                    if !track.is_available() {
                        return (id, Err(DownloadFail::Unavailable));
                    }
                    let _permit = gate.acquire_owned().await.ok();
                    let cover_url = State::cover_key(track.cover_template());
                    let extras = async {
                        let lyrics = tokio::time::timeout(std::time::Duration::from_secs(8), job_backend.lyrics(&track)).await.ok().and_then(Result::ok).unwrap_or_default();
                        let (cover_png, cover_raw) = match &cover_url {
                            None => (None, None),
                            Some(url) => {
                                let from_ram = job_art.encoded(url);
                                let disk_cache = job_cache.clone();
                                let url_disk = url.clone();
                                let local = tokio::task::spawn_blocking(move || from_ram.or_else(|| disk_cache.as_ref().and_then(|d| d.get(&url_disk)))).await.ok().flatten();
                                let (raw, from_net) = match local {
                                    Some(bytes) => (Some(bytes), false),
                                    None => (job_backend.image_bytes(url).await, true),
                                };
                                let png = match &raw {
                                    Some(bytes) => {
                                        let bytes = bytes.clone();
                                        tokio::task::spawn_blocking(move || {
                                            let img = crate::art::decode(&bytes, crate::art::COVER_PX)?;
                                            crate::art::encode_png(&img)
                                        })
                                        .await
                                        .ok()
                                        .flatten()
                                    }
                                    None => match job_backend.image(url, crate::art::COVER_PX).await {
                                        Some(image) => tokio::task::spawn_blocking(move || crate::art::encode_png(&image)).await.ok().flatten(),
                                        None => None,
                                    },
                                };
                                (png, from_net.then_some(raw).flatten())
                            }
                        };
                        (lyrics, cover_png, cover_raw)
                    };
                    let file = async {
                        let mut attempt = 0u32;
                        loop {
                            match job_backend.audio(&track, quality, Some(job_downloads.clone()), &job_fkcensor, fkcensor_on, true).await {
                                Err(e) if e.is_transient() && attempt < 2 => {
                                    attempt += 1;
                                    tokio::time::sleep(std::time::Duration::from_millis(1000 * u64::from(attempt))).await;
                                }
                                other => break other,
                            }
                        }
                    };
                    let (audio, (lyrics, cover, cover_raw)) = tokio::join!(file, extras);
                    let audio = match audio {
                        Ok(audio) => audio,
                        Err(e) => return (id, Err(DownloadFail::of(&e))),
                    };
                    let (put_track, put_downloads, put_cover, put_raw, put_cache, put_cover_url) = (track.clone(), job_downloads, cover, cover_raw, job_cache, cover_url);
                    let stored = tokio::task::spawn_blocking(move || {
                        let tagged = crate::audio_tags::embed(&audio.bytes, audio.ext, &put_track, put_cover.as_deref());
                        let variant = if audio.clean { "replacement" } else { "original" };
                        let audio_key = format!("{}-{:?}-{variant}", put_track.id, quality);
                        let audio_ok = put_downloads.put_audio_with_ext(&audio_key, &tagged, audio.ext).is_ok();
                        let item = DownloadedTrack { track: put_track, lyrics, ext: audio.ext.to_owned() };
                        let metadata_ok = audio_ok && put_downloads.put(&item).is_ok();
                        if let (Some(url), Some(bytes), Some(cache)) = (put_cover_url.as_ref(), put_raw.as_deref(), put_cache.as_ref()) {
                            let _ = cache.put(url, bytes);
                        }
                        metadata_ok.then_some(audio.clean)
                    })
                    .await
                    .ok()
                    .flatten();
                    (id, stored.ok_or(DownloadFail::Storage))
                },
                |s, _, (id, result)| {
                    let ok = result.is_ok();
                    if result == Ok(true) {
                        s.clean_tracks.insert(id.clone());
                    }
                    match result {
                        Ok(_) => {
                            s.download_errors.remove(&id);
                        }
                        Err(why) => {
                            s.download_errors.insert(id.clone(), why);
                        }
                    }
                    s.downloads.insert(id, if ok { DownloadState::Done } else { DownloadState::Failed });
                    if let Some(activity) = &mut s.download_activity {
                        activity.active = activity.active.saturating_sub(1);
                        if ok {
                            activity.done += 1;
                        } else {
                            activity.failed += 1;
                        }
                        activity.changed_at = s.now;
                    }
                },
            );
        }
    }
}
