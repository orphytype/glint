//! Per-frame playback sync, session snapshot/restore, simulated spectrum and play reporting.

use ym_api::Track;
use ym_audio::Event;

use super::write_time;
use crate::app::{Load, Source, State};
use crate::store::PlaybackSnapshot;

/// The track playing now, as the play report will tell it: how long it really played and where it stopped.
pub struct Listen {
    track_id: String,
    album_id: Option<String>,
    play_id: String,
    length_secs: f64,
    played_secs: f64,
    end_secs: f64,
}

impl Listen {
    pub(super) fn of(track: &Track) -> Listen {
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
    pub fn sync_lives(&self) {
        let (pos, dur) = (self.player.pos_ms, self.player.dur_ms);
        let vol_px = self.player.volume * self.vol_w.get();
        if (self.vol_drawn.get() - vol_px).abs() > 0.05 {
            self.vol_drawn.set(vol_px);
            self.lives.volume.set([0.0, 0.0, vol_px, 4.0]);
        }
        let secs = pos / 1000;
        if self.clock_secs.replace(secs) != secs {
            let mut text = self.clock_text.borrow_mut();
            text.clear();
            write_time(&mut text, secs);
            self.lives.clock.set(text.as_str());
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

    pub fn playback_snapshot(&self) -> Option<PlaybackSnapshot> {
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
        Some(PlaybackSnapshot { queue: queue[from..to].to_vec(), index: index - from, pos_ms: self.player.pos_ms, wave })
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

    pub fn save_playback_async(&mut self) {
        self.playback_dirty = false;
        self.playback_saved_at = self.now;
        if self.headless {
            return;
        }
        let snap = self.playback_snapshot().unwrap_or_default();
        self.store.save_playback_async(snap);
    }

    /// Puts last time's track back in the player, paused where it was: nothing is fetched until play.
    pub fn restore_playback(&mut self, snap: PlaybackSnapshot) {
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
        if let Some(key) = State::cover_key(track.cover_template()) {
            self.art.pixels(&key);
        }
        self.sync_lives();
    }

    /// Returns true when something the view shows (not a live value) changed.
    pub fn tick_player(&mut self, dt: f32) -> bool {
        let mut changed = false;
        let playing = self.player.playing && !self.player.loading;
        let pos = self.player.pos_ms;
        let current_id = self.player.index.and_then(|i| self.player.queue.get(i)).map(|t| t.id.as_str());
        if let Some(l) = self.listen.as_mut()
            && current_id == Some(l.track_id.as_str())
        {
            if playing {
                l.played_secs += f64::from(dt);
            }
            l.end_secs = pos as f64 / 1000.0;
        }
        if let Some(e) = self.player.engine.as_ref() {
            let st = e.state();
            let mut fresh = st.token == Some(self.player.token) && !self.scrubbing;
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
            if (running && self.player.sim_started) || (running && self.headless) {
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

    /// Reports the track that was playing (a second or more of it).
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
