//! Playback: the queue, shuffle and repeat, gapless hand-over to the next track, the endless
//! Wave, and the per-frame sync of position and spectrum into the UI state.
//!
//! The audio engine lives on its own thread and talks back through [`ym_audio::Event`]s that
//! arrive here as `on_audio_event`. When there is no sound device (or in a headless run) a
//! simulated clock stands in, so the whole UI, lyrics and visuals still work.

use std::time::Duration;

use ym_api::{Track, WaveEvent};
use ym_audio::{Engine, Event, LoadHint};

use crate::app::{DownloadState, Load, Source, State, Station};
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

fn hint(ext: &str, duration_ms: Option<u64>) -> LoadHint {
    let h = LoadHint::default().ext(ext);
    match duration_ms {
        Some(ms) if ms > 0 => h.duration(Duration::from_millis(ms)),
        _ => h,
    }
}

impl State {
    pub fn ask_download(&mut self, tracks: Vec<Track>) {
        let tracks: Vec<Track> = tracks
            .into_iter()
            .filter(|track| !matches!(self.downloads.get(&track.id), Some(DownloadState::Downloading | DownloadState::Done)))
            .collect();
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
            self.delete_download_confirm = Some(track);
        }
    }

    pub fn confirm_delete_download(&mut self) {
        let Some(track) = self.delete_download_confirm.take() else { return };
        let Some(store) = &self.download_store else { return };
        match store.remove(&track.id) {
            Ok(()) => {
                self.downloads.remove(&track.id);
                self.download_titles.remove(&track.id);
                self.toast(self.tr(S::DownloadDeleted));
            }
            Err(_) => self.toast(self.tr(S::DownloadDeleteFailed)),
        }
    }

    pub fn cancel_delete_download(&mut self) {
        self.delete_download_confirm = None;
    }

    pub fn download_tracks(&mut self, tracks: Vec<Track>) {
        if tracks.is_empty() {
            return;
        }
        for track in &tracks {
            self.downloads.insert(track.id.clone(), DownloadState::Downloading);
            self.download_titles.insert(track.id.clone(), track.full_title());
        }
        let Some(downloads) = self.download_store.clone() else {
            for track in &tracks {
                self.downloads.insert(track.id.clone(), DownloadState::Failed);
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
        // entire batch to finish. A few at a time is plenty, though: a page of a hundred
        // downloads must not open a hundred connections at once.
        let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(3));
        for track in tracks {
            let id = track.id.clone();
            let job_backend = backend.clone();
            let job_downloads = downloads.clone();
            let job_cache = cache.clone();
            let job_fkcensor = fkcensor.clone();
            let gate = gate.clone();
            self.job(async move {
                let _permit = gate.acquire_owned().await.ok();
                let audio = job_backend
                    .audio(&track, quality, Some(job_downloads.clone()), &job_fkcensor, fkcensor_on, false)
                    .await;
                let mut ok = false;
                if let Ok(audio) = audio
                    && let Ok(lyrics) = job_backend.lyrics(&track).await
                {
                    let cover_url = track.cover_url(600);
                    let cover = match &cover_url {
                        None => None,
                        Some(url) => job_backend.image(url, 600).await.and_then(|image| crate::art::encode_png(&image)),
                    };
                    let tagged = crate::audio_tags::embed(&audio.bytes, audio.ext, &track, cover.as_deref());
                    let variant = if fkcensor_on && job_fkcensor.replacement(&track.id).is_some() { "replacement" } else { "original" };
                    let audio_key = format!("{}-{:?}-{variant}", track.id, quality);
                    let audio_ok = job_downloads.put_audio_with_ext(&audio_key, &tagged, audio.ext).is_ok();
                    let item = DownloadedTrack { track: track.clone(), lyrics, ext: audio.ext.to_owned() };
                    let metadata_ok = audio_ok && job_downloads.put(&item).is_ok();
                    // Art is a nicety: a full or missing cache must not fail the track itself.
                    if let (Some(url), Some(bytes), Some(cache)) = (cover_url.as_ref(), cover.as_deref(), job_cache.as_ref()) {
                        let _ = cache.put(url, bytes);
                    }
                    ok = metadata_ok;
                }
                (id, ok)
            }, |s, _, (id, ok)| {
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
            });
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
        let tracks: Vec<Track> = tracks.into_iter().filter(|t| t.is_available()).collect();
        if tracks.is_empty() {
            let text = self.tr(S::NotAvailable);
            self.toast(text);
            return;
        }
        let start = start.min(tracks.len() - 1);
        // Whoever started this says where it came from once it has started.
        self.playing_from = None;
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
        if !waiting_for_switch {
            self.player.pos_ms = 0;
            self.player.dur_ms = track.duration_ms.unwrap_or(0);
        }
        self.player.sim_started = false;
        if !waiting_for_switch {
            self.on_track_changed(&track);
        }
        self.engine_ready();
        let (b, q, downloads, fkcensor, fkcensor_on) = (self.backend.clone(), self.settings.quality.api(), self.download_store.clone(), self.fkcensor.clone(), self.settings.fkcensor);
        self.job(async move { b.audio(&track, q, downloads, &fkcensor, fkcensor_on, false).await }, move |s, _, res| s.on_audio(token, res));
    }

    fn on_track_changed(&mut self, track: &Track) {
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

    pub fn on_audio(&mut self, token: u64, res: net::Result<Audio>) {
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
                let dur = self.current().and_then(|t| t.duration_ms);
                let playing = self.player.playing;
                if self.engine_ready()
                    && let Some(e) = &self.player.engine
                {
                    e.load(token, a.bytes, hint(a.ext, dur), playing, Duration::ZERO);
                }
                self.player.sim_started = true;
                self.wave_event(WaveEvent::TrackStarted, 0.0);
                self.prefetch_next();
            }
            Err(e) => {
                self.error_toast(&e);
                self.skip_failed();
            }
        }
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
        let (b, q, downloads, fkcensor, fkcensor_on) = (self.backend.clone(), self.settings.quality.api(), self.download_store.clone(), self.fkcensor.clone(), self.settings.fkcensor);
        self.job(async move { b.audio(&track, q, downloads, &fkcensor, fkcensor_on, false).await }, move |s, _, res| s.on_next_audio(parent, token, ni, res));
    }

    fn on_next_audio(&mut self, parent: u64, token: u64, index: usize, res: net::Result<Audio>) {
        if self.player.token != parent || self.player.next_loading != Some(token) {
            return;
        }
        self.player.next_loading = None;
        let Ok(a) = res else { return };
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

    fn run_wave(&mut self, preset: &str) {
        if self.player.wave_fetching {
            return;
        }
        self.player.wave_fetching = true;
        let (b, p) = (self.backend.clone(), preset.to_owned());
        let p2 = p.clone();
        self.job(async move { b.wave(&p2, None, 0).await }, move |s, _, res| {
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
        let after = self.player.queue.last().map(|t| t.id.clone());
        let b = self.backend.clone();
        self.job(async move { b.wave(&preset, after.as_deref(), round).await }, move |s, _, res| {
            s.player.wave_fetching = false;
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
        if let Some(e) = &self.player.engine {
            e.set_paused(!self.player.playing);
        }
        // Playback finished earlier: pressing play starts the track again. (Not while a track is
        // being fetched: that one is on its way.)
        if self.player.playing && !self.player.loading && self.player.pos_ms == 0 && self.player.engine.as_ref().is_some_and(|e| e.state().token.is_none()) {
            self.load_current();
        }
    }

    pub fn next(&mut self) {
        let skipped = self.player.pos_ms as f64 / 1000.0;
        self.wave_event(WaveEvent::Skip, skipped);
        let Some(i) = self.player.index else { return };
        if i + 1 < self.player.queue.len() {
            self.play_index(i + 1);
        } else if self.player.repeat == Repeat::All || matches!(self.player.source, Source::Wave { .. }) {
            if matches!(self.player.source, Source::Wave { .. }) {
                self.top_up_wave();
            } else {
                self.play_index(0);
            }
        }
    }

    pub fn prev(&mut self) {
        let Some(i) = self.player.index else { return };
        // Like every player: a few seconds in, "previous" restarts the track.
        if (self.player.pos_ms > 3500 && !self.player.loading) || i == 0 {
            if self.player.loading {
                return;
            }
            self.seek_ms(0);
        } else {
            self.play_index(i - 1);
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
        if let Some(e) = &self.player.engine {
            e.seek(Duration::from_millis(ms));
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

    /// Returns true when something the view shows (not a live value) changed.
    pub fn tick_player(&mut self, dt: f32) -> bool {
        let mut changed = false;
        if let Some(e) = self.player.engine.as_ref() {
            let st = e.state();
            if st.token == Some(self.player.token) && !self.scrubbing {
                self.player.pos_ms = st.position.as_millis() as u64;
                if let Some(d) = st.duration {
                    self.player.dur_ms = d.as_millis() as u64;
                }
            }
            e.spectrum(&mut self.spectrum);
            self.level = e.level();
            self.tone = e.tone();
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
            let active = l.active(self.player.pos_ms as u32);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    fn state() -> State {
        let mut s = State::new(None, Store::none(), true);
        s.backend = net::Backend::demo();
        s
    }

    #[test]
    fn the_volume_slider_is_quiet_until_it_is_high() {
        assert_eq!(taper(0.0), 0.0);
        assert_eq!(taper(f32::NAN), 0.0);
        assert!((taper(1.0) - 1.0).abs() < 1e-6);
        assert!((taper(7.0) - 1.0).abs() < 1e-6, "above the top is the top");
        // The middle is a tenth of the amplitude, 80 % is under half of it.
        assert!(taper(0.5) < 0.11, "{}", taper(0.5));
        assert!(taper(0.8) < 0.42, "{}", taper(0.8));
        // Every step up is louder, and the fade at the bottom does not break that.
        let mut last = 0.0;
        for i in 1..=100 {
            let g = taper(i as f32 / 100.0);
            assert!(g > last, "step {i}: {g} after {last}");
            last = g;
        }
    }

    fn tracks(n: usize) -> Vec<Track> {
        let c = crate::demo::Catalog::new();
        c.tracks.into_iter().take(n).collect()
    }

    impl State {
        /// Plays one track inside the list it was clicked in.
        fn play_from(&mut self, list: &[Track], index: usize) {
            self.play_queue(list.to_vec(), index, Source::Manual);
        }
    }

    #[test]
    fn the_badge_where_it_plays_from_follows_the_language() {
        use crate::app::{FromName, PlayingFrom};
        use crate::i18n::Lang;
        let wave = PlayingFrom { wave: true, kind: S::WaveTitle, name: FromName::Word(S::WaveCalm), page: None };
        assert_eq!(wave.line(Lang::En), "My Wave · Calm");
        assert_eq!(wave.line(Lang::Ru), "Моя волна · Спокойное");
        let album = PlayingFrom { wave: false, kind: S::Album, name: FromName::Text("МЫШКОЙ".into()), page: None };
        assert_eq!(album.line(Lang::En), "Album · МЫШКОЙ");
        assert_eq!(album.line(Lang::Ru), "Альбом · МЫШКОЙ");
    }

    #[test]
    fn clock_format() {        assert_eq!(fmt_time(0), "0:00");
        assert_eq!(fmt_time(65_900), "1:05");
        assert_eq!(fmt_time(3_600_000), "60:00");
    }

    #[test]
    fn play_queue_starts_at_the_clicked_track() {
        let mut s = state();
        let t = tracks(6);
        s.play_from(&t, 3);
        assert_eq!(s.player.index, Some(3));
        assert_eq!(s.current().unwrap().id, t[3].id);
        assert!(s.player.playing && s.player.loading);
    }

    #[test]
    fn shuffle_keeps_the_current_track_first_and_restores_order() {
        let mut s = state();
        let t = tracks(8);
        s.play_from(&t, 2);
        s.toggle_shuffle();
        assert_eq!(s.player.index, Some(0));
        assert_eq!(s.current().unwrap().id, t[2].id);
        let mut ids: Vec<_> = s.player.queue.iter().map(|x| x.id.clone()).collect();
        ids.sort();
        let mut want: Vec<_> = t.iter().map(|x| x.id.clone()).collect();
        want.sort();
        assert_eq!(ids, want, "shuffle must be a permutation");
        s.toggle_shuffle();
        assert_eq!(s.player.queue.iter().map(|x| &x.id).collect::<Vec<_>>(), t.iter().map(|x| &x.id).collect::<Vec<_>>());
        assert_eq!(s.player.index, Some(2));
    }

    #[test]
    fn next_prev_and_repeat() {
        let mut s = state();
        let t = tracks(3);
        s.play_from(&t, 0);
        s.next();
        assert_eq!(s.player.index, Some(1));
        s.player.loading = false;
        s.player.pos_ms = 10_000;
        s.prev();
        assert_eq!(s.player.index, Some(1), "after a few seconds prev restarts the track");
        assert_eq!(s.player.pos_ms, 0);
        s.prev();
        assert_eq!(s.player.index, Some(0));
        s.player.index = Some(2);
        assert_eq!(s.next_index(), None);
        s.player.repeat = Repeat::All;
        assert_eq!(s.next_index(), Some(0));
        s.player.repeat = Repeat::One;
        assert_eq!(s.next_index(), Some(2));
    }

    #[test]
    fn the_menu_spring_settles_and_turns_round_midway_without_a_jump() {
        let mut s = state();
        s.menu_open = true;
        for _ in 0..10 {
            s.tick_menu(1.0 / 60.0);
        }
        let half = s.menu_t;
        assert!(half > 0.05 && half < 0.99, "mid-way: {half}");
        assert!(s.menu_moving() && s.menu_visible());
        // Pressing again turns it round from where it is, still carrying its speed.
        s.menu_open = false;
        s.tick_menu(1.0 / 60.0);
        assert!((s.menu_t - half).abs() < 0.12, "no jump: {half} -> {}", s.menu_t);
        for _ in 0..180 {
            s.tick_menu(1.0 / 60.0);
        }
        assert_eq!((s.menu_t, s.menu_v), (0.0, 0.0));
        assert!(!s.menu_moving() && !s.menu_visible());
        // Reduce motion lands it at once.
        s.settings.reduce_motion = true;
        s.menu_open = true;
        s.tick_menu(1.0 / 60.0);
        assert_eq!(s.menu_t, 1.0);
    }

    #[test]
    fn queue_editing() {
        let mut s = state();
        let t = tracks(5);
        s.play_from(&t, 0);
        s.queue_next(t[4].clone());
        assert_eq!(s.player.queue[1].id, t[4].id);
        s.queue_last(t[2].clone());
        assert_eq!(s.player.queue.last().unwrap().id, t[2].id);
        s.remove_from_queue(0);
        assert_eq!(s.player.queue.len(), 7, "the playing track cannot be removed");
        s.remove_from_queue(1);
        assert_eq!(s.player.queue.len(), 6);
    }

    #[test]
    fn simulated_clock_runs_and_wraps() {
        let mut s = state();
        let t = tracks(2);
        s.play_from(&t, 0);
        s.player.loading = false;
        for _ in 0..60 {
            s.now += 1.0 / 60.0;
            s.tick_player(1.0 / 60.0);
        }
        assert!((900..=1100).contains(&s.player.pos_ms), "{}", s.player.pos_ms);
        // Running off the end moves to the next track (via Ended -> next index).
        s.player.pos_ms = s.player.dur_ms - 5;
        s.tick_player(0.05);
        assert_eq!(s.player.index, Some(1));
        assert!(s.spectrum.iter().any(|b| *b > 0.01));
    }

    #[test]
    fn the_progress_bar_follows_the_position() {
        let mut s = state();
        let t = tracks(1);
        s.play_from(&t, 0);
        s.player.loading = false;
        s.bar_w.set(400.0);
        s.player.pos_ms = s.player.dur_ms / 2;
        s.sync_lives();
        // Live values read back as the last published value.
        assert!((s.lives.seek.get()[2] - 200.0).abs() < 1.0, "{:?}", s.lives.seek.get());
    }
}
