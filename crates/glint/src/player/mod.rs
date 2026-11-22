//! Playback: the queue, shuffle and repeat, gapless hand-over to the next track, the endless
//! Wave, offline downloads and per-frame sync into UI state.

mod downloads;
mod sync;
mod transport;

use std::time::Duration;

use ym_api::{Track, WaveEvent};
use ym_audio::{Engine, Event, LoadHint};

use crate::app::{DownloadState, Source, State};
use crate::i18n::S;
use crate::net::{self, Audio};
use crate::store::Repeat;

pub use sync::Listen;

/// Slider position (0..1) to the gain the engine plays at (linear in dB over a 40 dB range).
pub fn taper(p: f32) -> f32 {
    const RANGE_DB: f32 = 40.0;
    if !p.is_finite() || p <= 0.0 {
        return 0.0;
    }
    let p = p.min(1.0);
    10f32.powf(-RANGE_DB * (1.0 - p) / 20.0) * (p / 0.04).min(1.0)
}

pub fn fmt_time(ms: u64) -> String {
    let mut out = String::with_capacity(8);
    write_time(&mut out, ms / 1000);
    out
}

/// Writes `h:mm:ss` (when an hour or longer) or `m:ss` into `out` without allocating another string.
pub(super) fn write_time(out: &mut String, secs: u64) {
    use std::fmt::Write as _;
    if secs >= 3600 {
        let _ = write!(out, "{}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60);
    } else {
        let _ = write!(out, "{}:{:02}", secs / 60, secs % 60);
    }
}

fn hint(ext: &str, duration_ms: Option<u64>) -> LoadHint {
    let h = LoadHint::default().ext(ext);
    match duration_ms {
        Some(ms) if ms > 0 => h.duration(Duration::from_millis(ms)),
        _ => h,
    }
}

impl State {
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

    pub(super) fn shuffled(&mut self, mut items: Vec<Track>) -> Vec<Track> {
        for i in (1..items.len()).rev() {
            let j = (self.rand() % (i as u64 + 1)) as usize;
            items.swap(i, j);
        }
        items
    }

    /// Replaces the queue with `tracks` and starts at `start`.
    pub fn play_queue(&mut self, tracks: Vec<Track>, start: usize, source: Source) {
        let start = tracks.iter().take(start).filter(|t| self.track_playable(t)).count();
        let tracks: Vec<Track> = tracks.into_iter().filter(|t| self.track_playable(t)).collect();
        if tracks.is_empty() {
            let text = self.tr(S::NotAvailable);
            self.toast(text);
            return;
        }
        let start = start.min(tracks.len() - 1);
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
        let switching = self.player.engine.is_some() && self.player.index.is_some() && self.player.index != Some(index) && self.player.playing;
        if switching {
            self.player.pending_index = Some(index);
        } else {
            self.player.index = Some(index);
            self.player.pending_index = None;
        }
        self.load_current();
    }

    /// Fetches the current track again when its audio source changes (e.g. FkCensor toggled).
    pub fn reload_current(&mut self) {
        if self.player.index.is_none() || self.player.resume_ms.is_some() {
            return;
        }
        let playing = self.player.playing;
        if self.player.pending_index.is_none() {
            self.player.resume_ms = Some(self.player.pos_ms);
        }
        self.load_current();
        self.player.playing = playing;
    }

    pub(super) fn load_current(&mut self) {
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
        self.player.next_audio = None;
        self.player.next_loading = None;
        if let Some(e) = &self.player.engine {
            e.unqueue();
            e.stop();
        }
        self.player.loading = true;
        self.player.playing = true;
        self.player.seek_hold = None;
        let start = if waiting_for_switch { 0 } else { self.player.resume_ms.take().unwrap_or(0) };
        if !waiting_for_switch {
            self.player.pos_ms = start;
            self.player.dur_ms = track.duration_ms.unwrap_or(0);
            self.player.current_audio = None;
        }
        self.player.sim_started = false;
        if !waiting_for_switch {
            self.on_track_changed(&track);
        }
        #[cfg(feature = "tooling")]
        if matches!(self.backend, net::Backend::Fixture(_)) {
            self.player.loading = false;
            self.player.current_audio = Some(net::fixture_playing_quality(&track, self.settings.quality));
            self.remember_max_quality(track.id.clone(), net::fixture_max_quality(&track));
            return;
        }
        self.engine_ready();
        let (b, q, downloads, fkcensor, fkcensor_on) =
            (self.backend.clone(), self.settings.quality.api(), self.download_store.clone(), self.fkcensor.clone(), self.settings.fkcensor);
        self.job(async move { b.audio(&track, q, downloads, &fkcensor, fkcensor_on, false).await }, move |s, _, res| s.on_audio(token, q, res, start));
    }

    pub(super) fn remember_max_quality(&mut self, id: String, quality: net::StreamQuality) {
        if self.track_max_quality.len() >= 128 && !self.track_max_quality.contains_key(&id) {
            let cur = self.current().map(|t| t.id.clone());
            self.track_max_quality.retain(|k, _| Some(k) == cur.as_ref());
        }
        self.track_max_quality.insert(id, quality);
    }

    pub(super) fn probe_max_quality(&mut self, track: &Track) {
        if self.track_max_quality.contains_key(&track.id) || !self.max_quality_jobs.insert(track.id.clone()) {
            return;
        }
        let b = self.backend.clone();
        let t = track.clone();
        let id = track.id.clone();
        self.job(async move { b.max_quality(&t).await }, move |s, _, quality| {
            s.max_quality_jobs.remove(&id);
            if let Some(quality) = quality {
                s.remember_max_quality(id, quality);
            }
        });
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
        if self.settings.quality != crate::store::QualityPref::Best || !self.player.loading {
            self.probe_max_quality(track);
        }
        if let Some(key) = State::cover_key(track.cover_template()) {
            self.art.pixels(&key);
        }
    }

    pub fn on_audio(&mut self, token: u64, requested: ym_api::Quality, res: net::Result<Audio>, start_ms: u64) {
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
                self.player.current_audio = Some(a.quality);
                if let Some(t) = self.current().cloned() {
                    if a.clean {
                        self.clean_tracks.insert(t.id.clone());
                    }
                    if requested == ym_api::Quality::Best && !a.from_disk && !a.clean {
                        self.remember_max_quality(t.id, a.quality);
                    } else {
                        self.probe_max_quality(&t);
                    }
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

    fn measure_waveform(&mut self, bytes: std::sync::Arc<[u8]>, ext: &'static str, dur: Option<u64>) {
        let Some(id) = self.current().map(|t| t.id.clone()) else { return };
        if self.waveforms.contains_key(&id) || !self.waveform_jobs.insert(id.clone()) {
            return;
        }
        let h = hint(ext, dur);
        self.job(async move { tokio::task::spawn_blocking(move || ym_audio::waveform(bytes, &h, crate::fx::WAVE_N).ok()).await.ok().flatten() }, move |s, _, wave| {
            s.waveform_jobs.remove(&id);
            let Some(wave) = wave else { return };
            if s.waveforms.len() >= 64 {
                let cur = s.current().map(|t| t.id.clone());
                s.waveforms.retain(|k, _| Some(k) == cur.as_ref());
            }
            s.waveforms.insert(id, wave.into());
        });
    }

    fn next_downloaded(&self) -> Option<usize> {
        let cur = self.player.index?;
        (cur + 1..self.player.queue.len()).find(|&i| matches!(self.downloads.get(&self.player.queue[i].id), Some(DownloadState::Done)))
    }

    fn skip_failed(&mut self) {
        if self.player.pending_index.take().is_some() {
            self.player.pending_token = None;
            self.player.loading = false;
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

    pub(super) fn invalidate_next(&mut self) {
        self.player.next = None;
        self.player.next_audio = None;
        self.player.next_loading = None;
        if let Some(e) = &self.player.engine {
            e.unqueue();
        }
        self.prefetch_next();
    }

    pub(super) fn prefetch_next(&mut self) {
        if self.player.engine.is_none() || self.player.next.is_some() || self.player.next_loading.is_some() || !self.player.sim_started {
            return;
        }
        self.top_up_wave();
        let Some(ni) = self.next_index() else { return };
        let Some(track) = self.player.queue.get(ni).cloned() else { return };
        if !self.track_playable(&track) {
            return;
        }
        self.player.token_seed += 1;
        let token = self.player.token_seed;
        self.player.next_loading = Some(token);
        let parent = self.player.token;
        let (b, q, downloads, fkcensor, fkcensor_on) =
            (self.backend.clone(), self.settings.quality.api(), self.download_store.clone(), self.fkcensor.clone(), self.settings.fkcensor);
        self.job(async move { b.audio(&track, q, downloads, &fkcensor, fkcensor_on, false).await }, move |s, _, res| s.on_next_audio(parent, token, ni, q, res));
    }

    fn on_next_audio(&mut self, parent: u64, token: u64, index: usize, requested: ym_api::Quality, res: net::Result<Audio>) {
        if self.player.token != parent || self.player.next_loading != Some(token) {
            return;
        }
        self.player.next_loading = None;
        let Ok(a) = res else { return };
        if let Some(t) = self.player.queue.get(index).cloned() {
            if a.clean {
                self.clean_tracks.insert(t.id.clone());
            }
            if requested == ym_api::Quality::Best && !a.from_disk && !a.clean {
                self.remember_max_quality(t.id, a.quality);
            }
        }
        let dur = self.player.queue.get(index).and_then(|t| t.duration_ms);
        if let Some(e) = &self.player.engine {
            e.enqueue(token, a.bytes, hint(a.ext, dur));
            self.player.next = Some((token, index));
            self.player.next_audio = Some(a.quality);
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
                    self.player.current_audio = self.player.next_audio.take();
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
                    self.player.next_audio = None;
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
}
