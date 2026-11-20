//! Playback: the queue, shuffle and repeat, gapless hand-over to the next track, the endless
//! Wave, and the per-frame sync of position and spectrum into the UI state.
//!
//! The audio engine lives on its own thread and talks back through [`ym_audio::Event`]s that
//! arrive here as `on_audio_event`. When there is no sound device (or in a headless run) a
//! simulated clock stands in, so the whole UI, lyrics and visuals still work.

use std::time::Duration;

use ym_api::{Track, WaveEvent};
use ym_audio::{Engine, Event, LoadHint};

use crate::app::{DownloadFail, DownloadState, Load, Source, State, Station};
use crate::i18n::S;
use crate::net::{self, Audio};
use crate::store::{DownloadedTrack, Repeat};

/// Slider position (0..1) to the gain the engine plays at. The slider is linear in decibels, 40 dB
/// from its top to the bottom, which is how loudness is heard: the middle of the slider is 20 dB
/// down (a tenth of the amplitude) and 80 % is still 8 dB down, so a "normal" setting is no longer
/// close to full scale. The last few per cent fade to silence so that zero is silent.
pub fn taper(p: f32) -> f32 {
    const RANGE_DB: f32 = 40.0;
    if !p.is_finite() || p <= 0.0 {
        return 0.0;
    }
    let p = p.min(1.0);
    10f32.powf(-RANGE_DB * (1.0 - p) / 20.0) * (p / 0.04).min(1.0)
}

pub fn fmt_time(ms: u64) -> String {
    let mut out = String::with_capacity(5);
    write_time(&mut out, ms / 1000);
    out
}

/// Writes `m:ss` into `out` without allocating another string.
fn write_time(out: &mut String, secs: u64) {
    use std::fmt::Write as _;
    let _ = write!(out, "{}:{:02}", secs / 60, secs % 60);
}

/// How many tracks of a batch download at once.
const DOWNLOAD_PARALLEL: usize = 8;

fn hint(ext: &str, duration_ms: Option<u64>) -> LoadHint {
    let h = LoadHint::default().ext(ext);
    match duration_ms {
        Some(ms) if ms > 0 => h.duration(Duration::from_millis(ms)),
        _ => h,
    }
}

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
        for track in tracks {
            if store.remove(&track.id).is_err() {
                failed = true;
                continue;
            }
            self.downloads.remove(&track.id);
            self.download_titles.remove(&track.id);
            self.downloaded_tracks.remove(&track.id);
            self.download_order.retain(|id| id != &track.id);
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
            let id = match page {
                crate::app::Page::Artist(id) => id,
                _ => return,
            };
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
        for track in &tracks {
            self.download_errors.remove(&track.id);
            self.downloads.insert(track.id.clone(), DownloadState::Downloading);
            self.download_titles.insert(track.id.clone(), track.full_title());
            self.downloaded_tracks.insert(track.id.clone(), track.clone());
            self.download_order.retain(|id| id != &track.id);
            self.download_order.insert(0, track.id.clone());
        }
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
            self.download_activity = Some(crate::app::DownloadActivity { total: tracks.len(), active: tracks.len(), done: 0, failed: 0, changed_at: self.now });
        }
        // Run one job per track so each completion is applied immediately. The manager can then
        // replace that row's spinner as soon as its file is ready instead of waiting for the
        // entire batch to finish. Most of a track's time goes to waiting on the network (two
        // lookups before the file, then the lyrics and the cover), so a good number run at once;
        // a page of a thousand downloads must still not open a thousand connections.
        let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(DOWNLOAD_PARALLEL));
        for track in tracks {
            let id = track.id.clone();
            let job_backend = backend.clone();
            let job_downloads = downloads.clone();
            let job_cache = cache.clone();
            let job_fkcensor = fkcensor.clone();
            let gate = gate.clone();
            self.job(
                async move {
                    // A track the service marks as unavailable has no file to give: say so at
                    // once instead of waiting in line for it.
                    if !track.is_available() {
                        return (id, Err(DownloadFail::Unavailable));
                    }
                    let _permit = gate.acquire_owned().await.ok();
                    // The lyrics and the cover come while the file does. Neither is worth holding
                    // a track up for: a lyrics service that hangs gives up after a few seconds.
                    let cover_url = track.cover_url(600);
                    let extras = async {
                        let lyrics = tokio::time::timeout(std::time::Duration::from_secs(8), job_backend.lyrics(&track)).await.ok().and_then(Result::ok).unwrap_or_default();
                        let cover = match &cover_url {
                            None => None,
                            Some(url) => match job_backend.image(url, 600).await {
                                Some(image) => tokio::task::spawn_blocking(move || crate::art::encode_png(&image)).await.ok().flatten(),
                                None => None,
                            },
                        };
                        (lyrics, cover)
                    };
                    let file = async {
                        // A dropped connection or a busy server is tried again a couple of times
                        // before the track counts as failed.
                        let mut attempt = 0u32;
                        loop {
                            match job_backend.audio(&track, quality, Some(job_downloads.clone()), &job_fkcensor, fkcensor_on, false, true).await {
                                Err(e) if e.is_transient() && attempt < 2 => {
                                    attempt += 1;
                                    tokio::time::sleep(std::time::Duration::from_millis(1000 * u64::from(attempt))).await;
                                }
                                other => break other,
                            }
                        }
                    };
                    let (audio, (lyrics, cover)) = tokio::join!(file, extras);
                    let audio = match audio {
                        Ok(audio) => audio,
                        Err(e) => return (id, Err(DownloadFail::of(&e))),
                    };
                    // Tagging and writing the file are work for a thread of their own, so the
                    // ones that wait on the network are not held up behind a disk.
                    let (put_track, put_downloads, put_cover, put_cache, put_cover_url) = (track.clone(), job_downloads, cover, job_cache, cover_url);
                    let stored = tokio::task::spawn_blocking(move || {
                        let tagged = crate::audio_tags::embed(&audio.bytes, audio.ext, &put_track, put_cover.as_deref());
                        // Kept under the version it really is: an uncensored file that could not be had
                        // leaves the original, which must not be filed as the uncensored one.
                        let variant = if audio.clean { "replacement" } else { "original" };
                        let audio_key = format!("{}-{:?}-{variant}", put_track.id, quality);
                        let audio_ok = put_downloads.put_audio_with_ext(&audio_key, &tagged, audio.ext).is_ok();
                        let item = DownloadedTrack { track: put_track, lyrics, ext: audio.ext.to_owned() };
                        let metadata_ok = audio_ok && put_downloads.put(&item).is_ok();
                        // Art is a nicety: a full or missing cache must not fail the track itself.
                        if let (Some(url), Some(bytes), Some(cache)) = (put_cover_url.as_ref(), put_cover.as_deref(), put_cache.as_ref()) {
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

    fn engine_ready(&mut self) -> bool {
        if self.headless || self.player.engine_failed {
            return false;
        }
        if self.player.engine.is_some() {
            return true;
        }
        let Some(app) = self.app.clone() else { return false };
        match Engine::new(move |ev| {
            let app = app.clone();
            app.update(move |s, _| s.on_audio_event(ev));
        }) {
            Ok(engine) => {
                engine.set_volume(taper(self.player.volume));
                engine.set_eq(&self.player.eq);
                self.player.engine = Some(engine);
                true
            }
            Err(_) => {
                self.player.engine_failed = true;
                let text = self.tr(S::NoAudioDevice);
                self.toast(text);
                false
            }
        }
    }

    fn rand(&mut self) -> u64 {
        let mut x = self.now.to_bits() ^ ((self.player.token_seed + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15));
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.player.token_seed = self.player.token_seed.wrapping_add(1);
        x
    }

    fn shuffled(&mut self, mut items: Vec<Track>) -> Vec<Track> {
        for i in (1..items.len()).rev() {
            let j = (self.rand() % (i as u64 + 1)) as usize;
            items.swap(i, j);
        }
        items
    }

    // ---- starting playback --------------------------------------------------------------------

    /// Replaces the queue with `tracks` and starts at `start`.
    pub fn play_queue(&mut self, tracks: Vec<Track>, start: usize, source: Source) {
        // `start` counts the list as given. Dropping the tracks that cannot play shifts everything
        // after them, so the start is counted again among those that stay: the clicked track, or
        // when it cannot play itself, the first one after it that can.
        let start = tracks.iter().take(start).filter(|t| t.is_available()).count();
        let tracks: Vec<Track> = tracks.into_iter().filter(|t| t.is_available()).collect();
        if tracks.is_empty() {
            let text = self.tr(S::NotAvailable);
            self.toast(text);
            return;
        }
        let start = start.min(tracks.len() - 1);
        // Whoever started this says where it came from once it has started.
        self.playing_from = None;
        self.player.resume_ms = None;
        self.player.pending_index = None;
        self.player.pending_token = None;
        self.player.original = tracks.clone();
        self.player.source = source;
        if self.player.shuffle {
            let first = tracks[start].clone();
            let rest: Vec<Track> = tracks.into_iter().enumerate().filter(|(i, _)| *i != start).map(|(_, t)| t).collect();
            let mut q = vec![first];
            q.extend(self.shuffled(rest));
            self.player.queue = q;
            self.player.index = Some(0);
        } else {
            self.player.queue = tracks;
            self.player.index = Some(start);
        }
        self.load_current();
    }

    pub fn play_index(&mut self, index: usize) {
        if index >= self.player.queue.len() {
            return;
        }
        if self.player.index != Some(index) {
            self.player.resume_ms = None;
        }
        // A track that is playing and is not the one asked for: the switch waits for the new track
        // to be fetched, and the choice replaces the one that was being fetched before it (two
        // presses in a row must not leave the first one standing).
        let switching = self.player.engine.is_some() && self.player.index.is_some() && self.player.index != Some(index) && self.player.playing;
        if switching {
            self.player.pending_index = Some(index);
        } else {
            self.player.index = Some(index);
            self.player.pending_index = None;
        }
        self.load_current();
    }

    /// Fetches the current track again because what gets fetched for it changed (a clean version
    /// appeared or the setting for them was switched). It stays where it was, and a paused track
    /// stays paused. Last time's track that hasn't been loaded yet is left alone: play fetches it.
    pub fn reload_current(&mut self) {
        if self.player.index.is_none() || self.player.resume_ms.is_some() {
            return;
        }
        let playing = self.player.playing;
        // While a switch is being fetched, that track is fetched again from its start.
        if self.player.pending_index.is_none() {
            self.player.resume_ms = Some(self.player.pos_ms);
        }
        self.load_current();
        self.player.playing = playing;
    }

    fn load_current(&mut self) {
        let target = self.player.pending_index.or(self.player.index);
        let Some(track) = target.and_then(|i| self.player.queue.get(i)).cloned() else { return };
        let waiting_for_switch = self.player.pending_index.is_some();
        self.player.token_seed += 1;
        let token = self.player.token_seed;
        if waiting_for_switch {
            self.player.pending_token = Some(token);
        } else {
            self.player.token = token;
        }
        self.player.next = None;
        self.player.next_loading = None;
        if let Some(e) = &self.player.engine {
            e.unqueue();
            // Whatever was playing stops now. The track the user asked for takes its place, and it
            // must not play over the old one while it is still being fetched; when it arrives it
            // starts on its own, with nothing of the old one left to interfere with it.
            e.stop();
        }
        self.player.loading = true;
        self.player.playing = true;
        self.player.seek_hold = None;
        // Last time's track starts where it was left; anything else from its start.
        let start = if waiting_for_switch { 0 } else { self.player.resume_ms.take().unwrap_or(0) };
        if !waiting_for_switch {
            self.player.pos_ms = start;
            self.player.dur_ms = track.duration_ms.unwrap_or(0);
        }
        self.player.sim_started = false;
        if !waiting_for_switch {
            self.on_track_changed(&track);
        }
        self.engine_ready();
        let (b, q, downloads, fkcensor, fkcensor_on) =
            (self.backend.clone(), self.settings.quality.api(), self.download_store.clone(), self.fkcensor.clone(), self.settings.fkcensor);
        self.job(async move { b.audio(&track, q, downloads, &fkcensor, fkcensor_on, false, false).await }, move |s, _, res| s.on_audio(token, res, start));
    }

    fn on_track_changed(&mut self, track: &Track) {
        self.playback_dirty = true;
        self.finish_listen();
        self.listen = Some(Listen::of(track));
        self.listening_history.retain(|old| old.id != track.id);
        self.listening_history.insert(0, track.clone());
        self.listening_history.truncate(30);
        self.lyrics.active = None;
        self.scroll_lyrics = true;
        self.load_lyrics_for_current();
        // Start the cover loading early so the palette is ready when it matters.
        for size in [100, 400] {
            if let Some(key) = State::cover_key(track.cover_template(), size) {
                self.art.pixels(&key);
            }
        }
    }

    pub fn on_audio(&mut self, token: u64, res: net::Result<Audio>, start_ms: u64) {
        let expected = self.player.pending_token.unwrap_or(self.player.token);
        if token != expected {
            return;
        }
        self.player.loading = false;
        match res {
            Ok(a) => {
                if let Some(index) = self.player.pending_index.take() {
                    self.player.index = Some(index);
                    self.player.token = token;
                    self.player.pending_token = None;
                    self.player.pos_ms = 0;
                    self.player.dur_ms = self.current().and_then(|t| t.duration_ms).unwrap_or(0);
                    if let Some(track) = self.current().cloned() {
                        self.on_track_changed(&track);
                    }
                }
                if a.clean
                    && let Some(t) = self.current()
                {
                    self.clean_tracks.insert(t.id.clone());
                }
                let dur = self.current().and_then(|t| t.duration_ms);
                let playing = self.player.playing;
                if self.engine_ready()
                    && let Some(e) = &self.player.engine
                {
                    e.load(token, a.bytes.clone(), hint(a.ext, dur), playing, Duration::from_millis(start_ms));
                }
                self.measure_waveform(a.bytes, a.ext, dur);
                self.player.sim_started = true;
                self.wave_event(WaveEvent::TrackStarted, 0.0);
                self.prefetch_next();
            }
            Err(e) => {
                self.error_toast(&e);
                // Offline, the tracks that are not downloaded would fail one by one, each with
                // its own wait: go straight to the next one that is on disk.
                if matches!(e, ym_api::Error::Network(_))
                    && self.player.pending_index.is_none()
                    && let Some(i) = self.next_downloaded()
                {
                    self.play_index(i);
                    return;
                }
                self.skip_failed();
            }
        }
    }

    /// Measures the current track's loudness along its length for the seek line, once per
    /// track, on a worker.
    fn measure_waveform(&mut self, bytes: std::sync::Arc<[u8]>, ext: &'static str, dur: Option<u64>) {
        let Some(id) = self.current().map(|t| t.id.clone()) else { return };
        if self.waveforms.contains_key(&id) || !self.waveform_jobs.insert(id.clone()) {
            return;
        }
        let h = hint(ext, dur);
        self.job(async move { tokio::task::spawn_blocking(move || ym_audio::waveform(bytes, &h, crate::fx::WAVE_N).ok()).await.ok().flatten() }, move |s, _, wave| {
            s.waveform_jobs.remove(&id);
            let Some(wave) = wave else { return };
            // A few dozen tracks' worth is plenty (each is a kilobyte).
            if s.waveforms.len() >= 64 {
                let cur = s.current().map(|t| t.id.clone());
                s.waveforms.retain(|k, _| Some(k) == cur.as_ref());
            }
            s.waveforms.insert(id, wave.into());
        });
    }

    /// The next track of the queue after the current one that is downloaded.
    fn next_downloaded(&self) -> Option<usize> {
        let cur = self.player.index?;
        (cur + 1..self.player.queue.len()).find(|&i| matches!(self.downloads.get(&self.player.queue[i].id), Some(DownloadState::Done)))
    }

    /// The current track cannot be played: move on, unless that was the last one.
    fn skip_failed(&mut self) {
        if self.player.pending_index.take().is_some() {
            self.player.pending_token = None;
            self.player.loading = false;
            // The track that was playing was stopped to make way for the new one, so it starts
            // again rather than leaving the app silent.
            self.load_current();
            return;
        }
        let cur = self.player.index.unwrap_or(0);
        if cur + 1 < self.player.queue.len() {
            self.play_index(cur + 1);
        } else {
            self.player.playing = false;
        }
    }

    // ---- gapless ------------------------------------------------------------------------------

    fn next_index(&self) -> Option<usize> {
        let p = &self.player;
        let i = p.index?;
        match p.repeat {
            Repeat::One => Some(i),
            _ if i + 1 < p.queue.len() => Some(i + 1),
            Repeat::All if !p.queue.is_empty() => Some(0),
            _ => None,
        }
    }

    fn invalidate_next(&mut self) {
        self.player.next = None;
        self.player.next_loading = None;
        if let Some(e) = &self.player.engine {
            e.unqueue();
        }
        self.prefetch_next();
    }

    fn prefetch_next(&mut self) {
        if self.player.engine.is_none() || self.player.next.is_some() || self.player.next_loading.is_some() || !self.player.sim_started {
            return;
        }
        self.top_up_wave();
        let Some(ni) = self.next_index() else { return };
        let Some(track) = self.player.queue.get(ni).cloned() else { return };
        if !track.is_available() {
            return;
        }
        self.player.token_seed += 1;
        let token = self.player.token_seed;
        self.player.next_loading = Some(token);
        let parent = self.player.token;
        let (b, q, downloads, fkcensor, fkcensor_on) =
            (self.backend.clone(), self.settings.quality.api(), self.download_store.clone(), self.fkcensor.clone(), self.settings.fkcensor);
        self.job(async move { b.audio(&track, q, downloads, &fkcensor, fkcensor_on, false, false).await }, move |s, _, res| s.on_next_audio(parent, token, ni, res));
    }

    fn on_next_audio(&mut self, parent: u64, token: u64, index: usize, res: net::Result<Audio>) {
        if self.player.token != parent || self.player.next_loading != Some(token) {
            return;
        }
        self.player.next_loading = None;
        let Ok(a) = res else { return };
        if a.clean
            && let Some(t) = self.player.queue.get(index)
        {
            self.clean_tracks.insert(t.id.clone());
        }
        let dur = self.player.queue.get(index).and_then(|t| t.duration_ms);
        if let Some(e) = &self.player.engine {
            e.enqueue(token, a.bytes, hint(a.ext, dur));
            self.player.next = Some((token, index));
        }
    }

    pub fn on_audio_event(&mut self, ev: Event) {
        match ev {
            Event::Advanced { token } => {
                if let Some((nt, ni)) = self.player.next
                    && nt == token
                {
                    let played = self.player.dur_ms as f64 / 1000.0;
                    self.wave_event(WaveEvent::TrackFinished, played);
                    self.player.next = None;
                    self.player.token = token;
                    self.player.index = Some(ni);
                    self.player.pos_ms = 0;
                    if let Some(t) = self.current().cloned() {
                        self.player.dur_ms = t.duration_ms.unwrap_or(0);
                        self.on_track_changed(&t);
                    }
                    self.wave_event(WaveEvent::TrackStarted, 0.0);
                    self.prefetch_next();
                }
            }
            Event::Ended { token } => {
                if token == self.player.token {
                    self.handle_end();
                }
            }
            Event::Error { token, message: _ } => {
                if token == self.player.token {
                    let text = self.tr(S::PlaybackFailed);
                    self.toast(text);
                    self.skip_failed();
                } else if self.player.next.is_some_and(|(t, _)| t == token) {
                    self.player.next = None;
                }
            }
            Event::DeviceError(m) => self.toast(m),
        }
    }

    fn handle_end(&mut self) {
        let finished = self.player.dur_ms as f64 / 1000.0;
        self.wave_event(WaveEvent::TrackFinished, finished);
        match self.next_index() {
            Some(i) => self.play_index(i),
            None => {
                if matches!(self.player.source, Source::Wave { .. }) && !self.player.wave_fetching {
                    // The Wave never ends: wait for the next batch.
                    self.player.playing = false;
                    self.top_up_wave();
                } else {
                    self.player.playing = false;
                    self.player.pos_ms = 0;
                    if let Some(e) = &self.player.engine {
                        e.stop();
                    }
                }
            }
        }
    }

    // ---- the wave -----------------------------------------------------------------------------

    pub fn start_wave(&mut self, preset: &str) {
        self.wave_choice = preset.to_owned();
        self.mark_settings_dirty();
        let name = crate::ui::wave_title(preset);
        self.wave_label = Some((S::WaveTitle, crate::app::FromName::Word(name), Some(crate::app::Page::Home)));
        self.run_wave(preset);
    }

    /// Starts a wave seeded by one thing ([`Station`]) rather than one of My Wave's moods; the
    /// mood picked on Home stays as it is. `name` is what it is seeded by, as shown.
    pub fn start_station(&mut self, station: &Station, name: &str) {
        let id = station.id();
        let (kind, page) = match station {
            Station::Track(_) => (S::WaveByTrack, None),
            Station::Artist(a) => (S::WaveByArtist, Some(crate::app::Page::Artist(a.clone()))),
            Station::Album(a) => (S::WaveByRelease, Some(crate::app::Page::Album(a.clone()))),
            Station::Collection => (S::WaveByCollection, Some(crate::app::Page::Library)),
        };
        self.wave_label = Some((kind, crate::app::FromName::Text(name.to_owned()), page));
        self.run_wave(&id);
    }

    /// The knobs the wave `id` plays with (`WaveTune`), for one of My Wave's own.
    pub fn wave_tune(&self, id: &str) -> crate::store::WaveTune {
        self.settings.wave_tunes.get(id).copied().unwrap_or_else(|| crate::store::WaveTune::of(id))
    }

    fn wave_knobs(&self, id: &str) -> Option<ym_api::WaveSettings> {
        crate::store::WaveTune::applies(id).then(|| self.wave_tune(id).knobs())
    }

    /// Sets the knobs of the wave `id`, kept with the settings. If that wave is playing it starts
    /// again with them, so what plays is always what the knobs say.
    pub fn set_wave_tune(&mut self, id: &str, tune: crate::store::WaveTune) {
        if tune == self.wave_tune(id) {
            return;
        }
        if tune == crate::store::WaveTune::of(id) {
            self.settings.wave_tunes.remove(id);
        } else {
            self.settings.wave_tunes.insert(id.to_owned(), tune);
        }
        self.mark_settings_dirty();
        if matches!(&self.player.source, Source::Wave { preset, .. } if preset == id) {
            self.start_wave(id);
        }
    }

    /// Asks for the first batch of a wave. Always: a wave asked for while another was still
    /// loading replaces it (it used to be ignored, so the mood pressed showed as chosen while the
    /// one before played).
    fn run_wave(&mut self, preset: &str) {
        self.player.wave_gen += 1;
        let wave = self.player.wave_gen;
        self.player.wave_fetching = true;
        let (b, p) = (self.backend.clone(), preset.to_owned());
        let p2 = p.clone();
        let knobs = self.wave_knobs(preset);
        self.job(async move { b.wave(&p2, knobs, None, 0).await }, move |s, _, res| {
            if s.player.wave_gen != wave {
                return;
            }
            s.player.wave_fetching = false;
            match res {
                Ok(batch) if !batch.tracks.is_empty() => {
                    s.play_queue(batch.tracks, 0, Source::Wave { preset: p, batch: batch.batch_id, round: 1 });
                    if let Some((kind, name, page)) = s.wave_label.take() {
                        s.playing_from = Some(crate::app::PlayingFrom { wave: true, kind, name, page });
                    }
                    s.wave_event(WaveEvent::RadioStarted, 0.0);
                }
                Ok(_) => {
                    let text = s.tr(S::Nothing);
                    s.toast(text);
                }
                Err(e) => s.error_toast(&e),
            }
        });
    }

    fn top_up_wave(&mut self) {
        let Source::Wave { preset, round, .. } = self.player.source.clone() else { return };
        let remaining = self.player.queue.len().saturating_sub(self.player.index.map_or(0, |i| i + 1));
        if remaining > 2 || self.player.wave_fetching {
            return;
        }
        self.player.wave_fetching = true;
        let wave = self.player.wave_gen;
        let after = self.player.queue.last().map(|t| t.id.clone());
        let b = self.backend.clone();
        let knobs = self.wave_knobs(&preset);
        let asked = preset.clone();
        self.job(async move { b.wave(&preset, knobs, after.as_deref(), round).await }, move |s, _, res| {
            // Another wave started meanwhile (it owns the fetch now), or something else plays:
            // these tracks are the old wave's, not to be added on.
            if s.player.wave_gen != wave {
                return;
            }
            s.player.wave_fetching = false;
            if !matches!(&s.player.source, Source::Wave { preset, .. } if *preset == asked) {
                return;
            }
            let Ok(batch) = res else { return };
            let known: std::collections::HashSet<String> = s.player.queue.iter().map(|t| t.id.clone()).collect();
            let fresh: Vec<Track> = batch.tracks.into_iter().filter(|t| t.is_available() && !known.contains(&t.id)).collect();
            if let Source::Wave { batch: b, round, .. } = &mut s.player.source {
                *b = batch.batch_id;
                *round += 1;
            }
            let was_idle = !s.player.playing && s.player.index.is_some_and(|i| i + 1 >= s.player.queue.len());
            s.player.queue.extend(fresh.iter().cloned());
            s.player.original.extend(fresh);
            if was_idle && s.player.queue.len() > s.player.index.unwrap_or(0) + 1 {
                s.play_index(s.player.index.unwrap_or(0) + 1);
            } else {
                s.prefetch_next();
            }
        });
    }

    fn wave_event(&mut self, event: WaveEvent, played_secs: f64) {
        let Source::Wave { preset, batch, .. } = self.player.source.clone() else { return };
        let Some(id) = self.current().map(|t| t.id.clone()) else { return };
        let b = self.backend.clone();
        self.job(async move { b.wave_feedback(&preset, event, &id, batch.as_deref(), played_secs).await }, |_, _, _| {});
    }

    // ---- transport ----------------------------------------------------------------------------

    pub fn toggle_pause(&mut self) {
        if self.current().is_none() {
            if !self.player.queue.is_empty() {
                self.play_index(0);
            }
            return;
        }
        self.player.playing = !self.player.playing;
        self.playback_dirty = true;
        // Last time's track, not loaded yet: the first play fetches it, at where it was left.
        if self.player.playing && self.player.resume_ms.is_some() && !self.player.loading {
            self.load_current();
            return;
        }
        if let Some(e) = &self.player.engine {
            e.set_paused(!self.player.playing);
        }
        // Playback finished earlier: pressing play starts the track again. (Not while a track is
        // being fetched: that one is on its way.)
        if self.player.playing && !self.player.loading && self.player.pos_ms == 0 && self.player.engine.as_ref().is_some_and(|e| e.state().token.is_none()) {
            self.load_current();
        }
    }

    /// Whether "next" has somewhere to go: a later track, the wave's next batch, or (with repeat
    /// on) the queue's start again.
    pub fn can_next(&self) -> bool {
        let p = &self.player;
        p.index.is_some_and(|i| i + 1 < p.queue.len() || p.repeat != Repeat::Off || matches!(p.source, Source::Wave { .. }))
    }

    /// Whether "previous" has somewhere to go: an earlier track or (with repeat on) the queue's end.
    pub fn can_prev(&self) -> bool {
        let p = &self.player;
        p.index.is_some_and(|i| i > 0 || (p.repeat != Repeat::Off && !p.queue.is_empty()))
    }

    pub fn next(&mut self) {
        if !self.can_next() {
            return;
        }
        let skipped = self.player.pos_ms as f64 / 1000.0;
        self.wave_event(WaveEvent::Skip, skipped);
        let Some(i) = self.player.index else { return };
        if i + 1 < self.player.queue.len() {
            self.play_index(i + 1);
        } else if matches!(self.player.source, Source::Wave { .. }) {
            self.top_up_wave();
        } else {
            // Repeat is on (either kind): the queue starts over.
            self.play_index(0);
        }
    }

    pub fn prev(&mut self) {
        if !self.can_prev() {
            return;
        }
        let Some(i) = self.player.index else { return };
        // Like every player: a few seconds in, "previous" restarts the track.
        if self.player.pos_ms > 3500 && !self.player.loading {
            self.seek_ms(0);
        } else if i > 0 {
            self.play_index(i - 1);
        } else {
            // Repeat is on: from the first track back round to the last.
            self.play_index(self.player.queue.len() - 1);
        }
    }

    pub fn seek_frac(&mut self, f: f32) {
        let dur = self.player.dur_ms;
        self.seek_ms((f.clamp(0.0, 1.0) as f64 * dur as f64) as u64);
    }

    pub fn seek_ms(&mut self, ms: u64) {
        if self.player.loading {
            return;
        }
        self.player.pos_ms = ms;
        self.playback_dirty = true;
        if self.player.resume_ms.is_some() {
            self.player.resume_ms = Some(ms);
        } else if let Some(e) = &self.player.engine {
            e.seek(Duration::from_millis(ms));
            self.player.seek_hold = Some((ms, self.now));
        }
        self.lyrics.active = None;
        self.scroll_lyrics = true;
        self.sync_lives();
    }

    pub fn set_volume(&mut self, v: f32) {
        let v = v.clamp(0.0, 1.0);
        self.player.volume = v;
        if let Some(e) = &self.player.engine {
            e.set_volume(taper(v));
        }
        self.sync_lives();
        self.mark_settings_dirty();
    }

    pub fn toggle_mute(&mut self) {
        let v = if self.player.volume > 0.001 { 0.0 } else { 0.8 };
        self.set_volume(v);
    }

    pub fn toggle_shuffle(&mut self) {
        self.player.shuffle = !self.player.shuffle;
        self.mark_settings_dirty();
        let Some(cur) = self.current().cloned() else { return };
        if self.player.shuffle {
            let rest: Vec<Track> = self.player.queue.iter().enumerate().filter(|(i, _)| Some(*i) != self.player.index).map(|(_, t)| t.clone()).collect();
            let mut q = vec![cur];
            q.extend(self.shuffled(rest));
            self.player.queue = q;
            self.player.index = Some(0);
        } else {
            let original = std::mem::take(&mut self.player.original);
            if let Some(i) = original.iter().position(|t| t.id == cur.id) {
                self.player.queue = original.clone();
                self.player.index = Some(i);
            }
            self.player.original = original;
        }
        self.invalidate_next();
    }

    pub fn cycle_repeat(&mut self) {
        self.player.repeat = self.player.repeat.next();
        self.mark_settings_dirty();
        self.invalidate_next();
    }

    // ---- queue editing ------------------------------------------------------------------------

    pub fn queue_next(&mut self, track: Track) {
        if self.player.queue.is_empty() {
            self.play_queue(vec![track], 0, Source::Manual);
            return;
        }
        let at = self.player.index.map_or(0, |i| i + 1);
        self.player.queue.insert(at, track.clone());
        self.player.original.push(track);
        let text = self.tr(S::PlayNextToast);
        self.toast(text);
        self.invalidate_next();
    }

    /// Puts a whole list after the playing track (`next`) or at the end of the queue, in order.
    pub fn queue_many(&mut self, tracks: Vec<Track>, next: bool) {
        if tracks.is_empty() {
            return;
        }
        if self.player.queue.is_empty() {
            self.play_queue(tracks, 0, Source::Manual);
            return;
        }
        let at = if next { self.player.index.map_or(0, |i| i + 1) } else { self.player.queue.len() };
        self.player.queue.splice(at..at, tracks.iter().cloned());
        self.player.original.extend(tracks);
        let text = self.tr(if next { S::PlayNextToast } else { S::QueuedToast });
        self.toast(text);
        if next {
            self.invalidate_next();
        } else {
            self.prefetch_next();
        }
    }

    pub fn queue_last(&mut self, track: Track) {
        if self.player.queue.is_empty() {
            self.play_queue(vec![track], 0, Source::Manual);
            return;
        }
        self.player.queue.push(track.clone());
        self.player.original.push(track);
        let text = self.tr(S::QueuedToast);
        self.toast(text);
        self.prefetch_next();
    }

    /// Drops an upcoming track from the queue (the playing one and the played ones stay).
    pub fn remove_from_queue(&mut self, i: usize) {
        if Some(i) <= self.player.index || i >= self.player.queue.len() {
            return;
        }
        let gone = self.player.queue.remove(i);
        self.player.original.retain(|t| t.id != gone.id);
        self.invalidate_next();
    }

    pub fn stop_all(&mut self) {
        self.finish_listen();
        if let Some(e) = &self.player.engine {
            e.stop();
        }
        self.player.queue.clear();
        self.player.original.clear();
        self.player.index = None;
        self.player.pending_index = None;
        self.player.pending_token = None;
        self.player.playing = false;
        self.player.loading = false;
        self.player.pos_ms = 0;
        self.player.dur_ms = 0;
        self.player.next = None;
        self.player.next_loading = None;
        self.player.resume_ms = None;
        self.player.seek_hold = None;
        self.playback_dirty = true;
        self.player.token = self.player.token_seed + 1;
        self.player.token_seed += 1;
        self.lyrics = Default::default();
        self.palette_key = None;
    }

    pub fn apply_eq(&mut self) {
        if let Some(e) = &self.player.engine {
            e.set_eq(&self.player.eq);
        }
        self.mark_settings_dirty();
    }

    // ---- per frame ----------------------------------------------------------------------------

    pub fn sync_lives(&self) {
        let (pos, dur) = (self.player.pos_ms, self.player.dur_ms);
        let frac = if dur > 0 { (pos as f32 / dur as f32).clamp(0.0, 1.0) } else { 0.0 };
        self.lives.seek.set([0.0, 0.0, frac * self.bar_w.get(), 4.0]);
        self.lives.seek_big.set([0.0, 0.0, frac * self.bar_big_w.get(), 6.0]);
        self.lives.volume.set([0.0, 0.0, self.player.volume * self.vol_w.get(), 4.0]);
        // The clock only shows whole seconds, so format it when the second changes, into a string
        // kept for the next one. Called every frame, this must not allocate two Strings a frame.
        let secs = pos / 1000;
        if self.clock_secs.replace(secs) != secs {
            let mut text = self.clock_text.borrow_mut();
            text.clear();
            write_time(&mut text, secs);
            self.lives.clock.set(text.as_str());
            self.lives.clock_big.set(text.as_str());
        }
        let left = dur.saturating_sub(pos) / 1000;
        if self.remaining_secs.replace(left) != left {
            let mut text = self.remaining_text.borrow_mut();
            text.clear();
            text.push('-');
            write_time(&mut text, left);
            self.lives.remaining.set(text.as_str());
        }
    }

    // ---- picking up where the last session left off -------------------------------------------

    /// What is playing, as it is saved for the next start: the queue (the part of a long one
    /// around the current track), the track and where in it.
    pub fn playback_snapshot(&self) -> Option<crate::store::PlaybackSnapshot> {
        let index = self.player.index?;
        let queue = &self.player.queue;
        if index >= queue.len() {
            return None;
        }
        let from = index.saturating_sub(100);
        let to = (index + 200).min(queue.len());
        let wave = match &self.player.source {
            Source::Wave { preset, .. } => Some(preset.clone()),
            Source::Manual => None,
        };
        Some(crate::store::PlaybackSnapshot {
            queue: queue[from..to].to_vec(),
            index: index - from,
            pos_ms: self.player.pos_ms,
            wave,
        })
    }

    pub fn save_playback_now(&mut self) {
        self.playback_dirty = false;
        self.playback_saved_at = self.now;
        if self.headless {
            return;
        }
        let snap = self.playback_snapshot().unwrap_or_default();
        self.store.save_playback(&snap);
    }

    /// Puts last time's track back in the player, paused where it was: nothing is fetched until
    /// play is pressed.
    pub fn restore_playback(&mut self, snap: crate::store::PlaybackSnapshot) {
        if snap.index >= snap.queue.len() || self.player.index.is_some() {
            return;
        }
        let track = snap.queue[snap.index].clone();
        let dur = track.duration_ms.unwrap_or(0);
        let pos = if dur > 0 { snap.pos_ms.min(dur.saturating_sub(1000)) } else { snap.pos_ms };
        self.player.original = snap.queue.clone();
        self.player.queue = snap.queue;
        self.player.index = Some(snap.index);
        self.player.source = match snap.wave {
            Some(preset) => Source::Wave { preset, batch: None, round: 0 },
            None => Source::Manual,
        };
        self.player.playing = false;
        self.player.loading = false;
        self.player.pos_ms = pos;
        self.player.dur_ms = dur;
        self.player.resume_ms = Some(pos);
        self.load_lyrics_for_current();
        for size in [100, 400] {
            if let Some(key) = State::cover_key(track.cover_template(), size) {
                self.art.pixels(&key);
            }
        }
        self.sync_lives();
    }

    /// Returns true when something the view shows (not a live value) changed.
    pub fn tick_player(&mut self, dt: f32) -> bool {
        let mut changed = false;
        // How long the current track has really played, for the play report.
        let (playing, pos, current) = (self.player.playing && !self.player.loading, self.player.pos_ms, self.current().map(|t| t.id.clone()));
        if let Some(l) = self.listen.as_mut()
            && current.as_deref() == Some(l.track_id.as_str())
        {
            if playing {
                l.played_secs += f64::from(dt);
            }
            l.end_secs = pos as f64 / 1000.0;
        }
        if let Some(e) = self.player.engine.as_ref() {
            let st = e.state();
            let mut fresh = st.token == Some(self.player.token) && !self.scrubbing;
            // Right after a seek the engine may still report where it was: the seek's position
            // stands until the engine's comes near it (or half a second has gone by).
            if let Some((to, at)) = self.player.seek_hold {
                let off = (st.position.as_millis() as i64 - to as i64).abs();
                if off < 1500 || self.now - at > 0.5 {
                    self.player.seek_hold = None;
                } else {
                    fresh = false;
                }
            }
            if fresh {
                self.player.pos_ms = st.position.as_millis() as u64;
                if let Some(d) = st.duration {
                    self.player.dur_ms = d.as_millis() as u64;
                }
            }
            e.spectrum(&mut self.spectrum);
            self.level = e.level();
            self.tone = e.tone();
            self.drive = e.drive();
        } else {
            let running = self.player.index.is_some() && self.player.playing && !self.player.loading;
            if running && self.player.sim_started || (running && self.headless) {
                self.player.pos_ms += (dt * 1000.0) as u64;
                if self.player.dur_ms > 0 && self.player.pos_ms >= self.player.dur_ms {
                    self.player.pos_ms = 0;
                    let token = self.player.token;
                    self.on_audio_event(Event::Ended { token });
                    changed = true;
                }
            }
            self.simulate_spectrum(running, dt);
        }
        self.sync_lives();

        if let Load::Ready(l) = &self.lyrics.lyrics {
            let active = l.active(self.lyric_ms());
            if active != self.lyrics.active {
                self.lyrics.active = active;
                self.scroll_lyrics = true;
                changed = true;
            }
        }
        changed
    }

    /// Stand-in spectrum for runs without a sound device: a pulsing, tilted bar field.
    fn simulate_spectrum(&mut self, running: bool, dt: f32) {
        let t = self.now as f32;
        let kick = (-(t * 2.0).fract() * 6.0).exp();
        let mut level = 0.0;
        for (i, b) in self.spectrum.iter_mut().enumerate() {
            let f = i as f32 / 31.0;
            let target = if running {
                let base = (1.0 - f).powf(1.6) * 0.55;
                let wob = 0.5 + 0.5 * (t * (1.3 + f * 3.1) + f * 9.0).sin();
                (base * (0.35 + 0.65 * wob) + kick * (1.0 - f) * 0.35).clamp(0.0, 1.0)
            } else {
                0.0
            };
            *b += (target - *b) * (1.0 - (-dt * 14.0).exp());
            level += *b / 32.0;
        }
        self.level = level;
        // A drum on every beat, a clap on the off-beats, and a voice that sings two bars of four.
        self.drive = if running {
            let snare = (-((t * 2.0 + 0.5).fract()) * 7.0).exp();
            let voice = if (t / 3.6).fract() < 0.5 { 0.8 } else { 0.1 };
            ym_audio::Drive {
                kick,
                snare,
                hats: (-((t * 4.0).fract()) * 12.0).exp() * 0.6,
                vocal: voice,
                syllable: if voice > 0.5 { (-((t / 0.45).fract()) * 5.0).exp() } else { 0.0 },
                energy: level,
                onset: 0.0,
            }
        } else {
            ym_audio::Drive::default()
        };
        // And a tune: a melody that walks a scale over I-V-vi-IV.
        self.tone = if running {
            const TUNE: [f32; 16] = [60.0, 64.0, 67.0, 72.0, 71.0, 67.0, 64.0, 62.0, 60.0, 62.0, 64.0, 65.0, 67.0, 69.0, 67.0, 64.0];
            const CHORDS: [[usize; 3]; 4] = [[0, 4, 7], [7, 11, 2], [9, 0, 4], [5, 9, 0]];
            let note = TUNE[(t / 0.45) as usize % TUNE.len()];
            let mut chroma = [0.12f32; 12];
            for pc in CHORDS[(t / 1.8) as usize % CHORDS.len()] {
                chroma[pc] = 1.0;
            }
            chroma[note as usize % 12] = chroma[note as usize % 12].max(0.7);
            ym_audio::Tone { chroma, pitch: note, clarity: 0.7, centroid: 0.45 + 0.1 * (t * 0.3).sin() }
        } else {
            ym_audio::Tone::default()
        };
    }
}

/// The track playing now, as the play report will tell it: how long it really played (not
/// where it got to: seeking skips) and where it stopped.
pub struct Listen {
    track_id: String,
    album_id: Option<String>,
    play_id: String,
    length_secs: f64,
    played_secs: f64,
    end_secs: f64,
}

impl Listen {
    fn of(track: &Track) -> Listen {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        Listen {
            track_id: track.id.clone(),
            album_id: track.albums.first().map(|a| a.id.clone()),
            play_id: format!("{now:x}-{:x}-{:x}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)),
            length_secs: track.duration_ms.unwrap_or(0) as f64 / 1000.0,
            played_secs: 0.0,
            end_secs: 0.0,
        }
    }
}

impl State {
    /// Reports the track that was playing (a second or more of it), so the account's history
    /// and its recommendations count it, as the official apps do.
    pub fn finish_listen(&mut self) {
        let Some(l) = self.listen.take() else { return };
        let Some(uid) = self.account.as_ref().map(|a| a.uid).filter(|uid| *uid != 0) else { return };
        if l.played_secs < 1.0 {
            return;
        }
        let report = ym_api::PlayReport {
            track_id: l.track_id,
            album_id: l.album_id,
            playlist_id: None,
            from: "desktop_win-glint".into(),
            play_id: l.play_id,
            track_length_secs: l.length_secs,
            total_played_secs: l.played_secs,
            end_position_secs: l.end_secs,
        };
        let b = self.backend.clone();
        self.job(async move { b.report_play(uid, report).await }, |_, _, _| {});
    }
}
