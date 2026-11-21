//! Wave stations, transport controls, queue editing and equaliser application.

use std::time::Duration;

use ym_api::{Track, WaveEvent};

use super::taper;
use crate::app::{FromName, Page, PlayingFrom, Source, State, Station};
use crate::i18n::S;
use crate::store::{Repeat, WaveTune};

impl State {
    pub fn start_wave(&mut self, preset: &str) {
        self.wave_choice = preset.to_owned();
        self.mark_settings_dirty();
        let name = crate::ui::wave_title(preset);
        self.wave_label = Some((S::WaveTitle, FromName::Word(name), Some(Page::Home)));
        self.run_wave(preset);
    }

    /// Starts a wave seeded by one thing ([`Station`]) rather than one of My Wave's moods.
    pub fn start_station(&mut self, station: &Station, name: &str) {
        let id = station.id();
        let (kind, page) = match station {
            Station::Track(_) => (S::WaveByTrack, None),
            Station::Artist(a) => (S::WaveByArtist, Some(Page::Artist(a.clone()))),
            Station::Album(a) => (S::WaveByRelease, Some(Page::Album(a.clone()))),
            Station::Collection => (S::WaveByCollection, Some(Page::Library)),
        };
        self.wave_label = Some((kind, FromName::Text(name.to_owned()), page));
        self.run_wave(&id);
    }

    pub fn wave_tune(&self, id: &str) -> WaveTune {
        self.settings.wave_tunes.get(id).copied().unwrap_or_else(|| WaveTune::of(id))
    }

    fn wave_knobs(&self, id: &str) -> Option<ym_api::WaveSettings> {
        WaveTune::applies(id).then(|| self.wave_tune(id).knobs())
    }

    pub fn set_wave_tune(&mut self, id: &str, tune: WaveTune) {
        if tune == self.wave_tune(id) {
            return;
        }
        if tune == WaveTune::of(id) {
            self.settings.wave_tunes.remove(id);
        } else {
            self.settings.wave_tunes.insert(id.to_owned(), tune);
        }
        self.mark_settings_dirty();
        if matches!(&self.player.source, Source::Wave { preset, .. } if preset == id) {
            self.start_wave(id);
        }
    }

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
                        s.playing_from = Some(PlayingFrom { wave: true, kind, name, page });
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

    pub(super) fn top_up_wave(&mut self) {
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
            if s.player.wave_gen != wave {
                return;
            }
            s.player.wave_fetching = false;
            if !matches!(&s.player.source, Source::Wave { preset, .. } if *preset == asked) {
                return;
            }
            let Ok(batch) = res else { return };
            let known: std::collections::HashSet<&str> = s.player.queue.iter().map(|t| t.id.as_str()).collect();
            let fresh: Vec<Track> = batch.tracks.into_iter().filter(|t| t.is_available() && !known.contains(t.id.as_str())).collect();
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

    pub(super) fn wave_event(&mut self, event: WaveEvent, played_secs: f64) {
        let Source::Wave { preset, batch, .. } = self.player.source.clone() else { return };
        let Some(id) = self.current().map(|t| t.id.clone()) else { return };
        let b = self.backend.clone();
        self.job(async move { b.wave_feedback(&preset, event, &id, batch.as_deref(), played_secs).await }, |_, _, _| {});
    }

    pub fn toggle_pause(&mut self) {
        if self.current().is_none() {
            if !self.player.queue.is_empty() {
                self.play_index(0);
            }
            return;
        }
        self.player.playing = !self.player.playing;
        self.playback_dirty = true;
        if self.player.playing && self.player.resume_ms.is_some() && !self.player.loading {
            self.load_current();
            return;
        }
        if let Some(e) = &self.player.engine {
            e.set_paused(!self.player.playing);
        }
        if self.player.playing && !self.player.loading && self.player.pos_ms == 0 && self.player.engine.as_ref().is_some_and(|e| e.state().token.is_none()) {
            self.load_current();
        }
    }

    pub fn can_next(&self) -> bool {
        let p = &self.player;
        p.index.is_some_and(|i| i + 1 < p.queue.len() || p.repeat != Repeat::Off || matches!(p.source, Source::Wave { .. }))
    }

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
            self.play_index(0);
        }
    }

    pub fn prev(&mut self) {
        if !self.can_prev() {
            return;
        }
        let Some(i) = self.player.index else { return };
        if self.player.pos_ms > 3500 && !self.player.loading {
            self.seek_ms(0);
        } else if i > 0 {
            self.play_index(i - 1);
        } else {
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
        self.lyrics_gen = self.lyrics_gen.wrapping_add(1);
        self.palette_key = None;
        self.palette_tpl = None;
    }

    pub fn apply_eq(&mut self) {
        if let Some(e) = &self.player.engine {
            e.set_eq(&self.player.eq);
        }
        self.mark_settings_dirty();
    }
}
