//! Core runtime helpers, settings persistence, toasts, and the per-frame `tick`.

use std::future::Future;

use rune::Tx;
use ym_api::Track;

use super::{APP_URL, DownloadState, Load, Page, PinnedItem, Session, Source, State, Toast};
use crate::i18n::{Lang, S};
use crate::net::{self, Backend};
use crate::store::ThemeMode;
use crate::theme::{self, Pal};

impl State {
    pub fn resize_settles_in(&self) -> f64 {
        self.resized_at.get() + 0.12 - self.now
    }

    pub fn is_resizing(&self) -> bool {
        self.resize_settles_in() > 0.0
    }

    /// Runs `fut` on the background runtime and applies its result on the UI thread.
    pub fn job<T: Send + 'static>(&self, fut: impl Future<Output = T> + Send + 'static, apply: impl FnOnce(&mut State, &mut Tx, T) + Send + 'static) {
        let Some(app) = self.app.clone() else { return };
        net::spawn(async move {
            let out = fut.await;
            app.update(move |s, tx| {
                let was = s.track_mark();
                apply(s, tx, out);
                s.animate_if_moved(tx, &was);
            });
        });
    }

    /// Which track plays: its place in the queue and its id.
    pub fn track_mark(&self) -> (Option<usize>, Option<String>) {
        (self.player.index, self.current().map(|t| t.id.clone()))
    }

    /// When the playing track changed since `was`, asks for motion. A commit no press caused
    /// (a load finishing, the audio reaching the end, a media key) lands at once otherwise, and
    /// the queue and the backdrop would jump to the new track.
    pub fn animate_if_moved(&self, tx: &mut Tx, was: &(Option<usize>, Option<String>)) {
        if &self.track_mark() != was && !self.settings.reduce_motion {
            tx.animated();
        }
    }

    pub fn tr(&self, s: S) -> &'static str {
        s.get(self.lang)
    }

    pub fn pal(&self) -> Pal {
        Pal::new(theme::accent_srgb(self.settings.accent, self.palette.accent))
    }

    /// Whether the light theme is active (either explicitly or via the system theme).
    pub fn is_light(&self) -> bool {
        match self.settings.theme {
            ThemeMode::Dark => false,
            ThemeMode::Light => true,
            ThemeMode::System => self.sys_light,
        }
    }

    /// Opens or closes the Now Playing page and records the transition in navigation history.
    ///
    /// The open player is one step on top of the page under it. Closing it steps back off it, so
    /// forward opens it again. It never leaves an "open player" entry in the back history:
    /// otherwise back would reopen the player after every close and the pages behind it could
    /// not be reached.
    pub fn set_np(&mut self, open: bool) {
        let open = open && self.current().is_some();
        self.menu_open = false;
        self.tmenu_open = false;
        self.close_volume();
        self.blur_edits();
        if self.np_open != open {
            let under = (self.page.clone(), false);
            let over = (self.page.clone(), true);
            if open {
                if self.forward.last() == Some(&over) {
                    self.forward.pop();
                } else {
                    self.forward.clear();
                }
                self.history.push(under);
                if self.history.len() > 40 {
                    self.history.remove(0);
                }
            } else if self.history.last() == Some(&under) {
                self.history.pop();
                self.forward.push(over);
            }
            crate::diag_log::event("ui", if open { "np open" } else { "np close" });
            self.log_mem(if open { "np:open" } else { "np:close" });
        }
        self.np_open = open;
        if !open {
            // Drop Now Playing's 600x600 hero texture shortly after the collapse spring finishes.
            self.art_trim_at = Some(self.now + 0.4);
        }
        if self.settings.reduce_motion || crate::fx::cpu_mode() {
            self.np = super::Pop { t: if open { 1.0 } else { 0.0 }, v: 0.0 };
        }
    }

    pub fn toast(&mut self, text: impl Into<String>) {
        self.toast_seq += 1;
        let t = Toast { id: self.toast_seq, text: text.into(), born: self.now, error: false };
        self.toasts.push(t);
        if self.toasts.len() > 3 {
            self.toasts.remove(0);
        }
    }

    pub fn open_download_folder(&self) {
        let Some(path) = self.store.downloads_dir() else { return };
        open_folder(&path);
    }

    pub fn open_logs_folder(&self) {
        let Some(path) = self.store.logs_dir() else { return };
        let _ = std::fs::create_dir_all(&path);
        open_folder(&path);
    }

    /// Samples current memory and GPU/art counters into the local diagnostic log.
    pub fn log_mem(&self, reason: &str) {
        crate::diag_log::log_memory(reason, self.art.stats(), crate::fx::gpu_stats());
    }

    /// Reclaims offscreen covers, CPU shape caches, unused waveforms, and freed working-set pages
    /// when the main window hides to the tray or mini player.
    pub fn trim_hidden_memory(&mut self) {
        let keep = self.current().and_then(|t| State::cover_key(t.cover_template()));
        self.art.trim_hidden(keep.as_deref());
        crate::ui::blobs::clear_cache();
        if !crate::fx::shaders_enabled() {
            self.waveforms.clear();
        }
        if !self.settings.backdrop.on {
            self.backdrop = None;
        }
        crate::diag_log::trim_working_set();
    }
}

fn open_folder(path: &std::path::Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

impl State {
    /// Whether a track can be played or downloaded right now (available on Yandex, already
    /// downloaded offline, or backed by an active FkCensor replacement).
    pub fn track_playable(&self, track: &Track) -> bool {
        track.is_available() || matches!(self.downloads.get(&track.id), Some(DownloadState::Done)) || (self.settings.fkcensor && self.fkcensor.has_track(&track.id))
    }

    /// Whether `tracks` are downloaded as far as possible (at least one done, all others permanently unavailable).
    pub fn downloads_settled(&self, tracks: &[Track]) -> bool {
        let mut any = false;
        for t in tracks {
            match self.downloads.get(&t.id) {
                Some(DownloadState::Done) => any = true,
                Some(DownloadState::Failed) if self.download_errors.get(&t.id).is_some_and(|f| f.lasting()) => {}
                _ if !self.track_playable(t) => {}
                _ => return false,
            }
        }
        any
    }

    /// Localized reason why the download of track `id` failed, if it failed.
    pub fn download_why(&self, id: &str) -> Option<&'static str> {
        self.download_errors.get(id).map(|f| f.reason().get(self.lang))
    }

    pub(crate) fn wipe_downloads(&mut self) {
        self.downloads_gen = self.downloads_gen.wrapping_add(1);
        self.deleted_downloads.extend(self.downloads.keys().cloned());
        if let Some(store) = &self.download_store {
            let _ = store.clear();
        }
        self.downloads.clear();
        self.download_titles.clear();
        self.downloaded_tracks.clear();
        self.download_errors.clear();
        self.download_order.clear();
        self.download_activity = None;
    }

    pub fn clear_download_history(&mut self) {
        self.wipe_downloads();
        self.toast(self.tr(S::DownloadHistoryCleared));
    }

    pub fn error_toast(&mut self, e: &ym_api::Error) {
        #[cfg(feature = "tooling")]
        if matches!(self.backend, Backend::Fixture(_)) {
            return;
        }
        let text = net::describe(e, self.lang);
        self.toast(text);
        if let Some(t) = self.toasts.last_mut() {
            t.error = true;
        }
    }

    pub fn mark_settings_dirty(&mut self) {
        self.settings_dirty_at = Some(self.now);
    }

    fn sync_settings_fields(&mut self) {
        self.settings.lang = Some(self.lang.code().into());
        self.settings.volume = self.player.volume;
        self.settings.shuffle = self.player.shuffle;
        self.settings.repeat = self.player.repeat;
        self.settings.eq_enabled = self.player.eq.enabled;
        self.settings.eq_preamp = self.player.eq.preamp_db;
        self.settings.eq_gains = self.player.eq.gains_db;
        self.settings.wave = self.wave_choice.clone();
        self.settings.pinned = self.pinned_items.iter().map(PinnedItem::setting).collect();
    }

    pub fn save_settings_now(&mut self) {
        self.sync_settings_fields();
        self.store.save(&self.settings);
        self.settings_dirty_at = None;
        self.save_playback_now();
    }

    /// Saves current settings and queues an immediate process relaunch on the next poll turn.
    pub fn restart_now(&mut self) {
        self.save_settings_now();
        if self.headless {
            return;
        }
        self.restart_pending = true;
        if let Some(app) = &self.app {
            app.wake();
        }
    }

    pub fn save_settings_async(&mut self) {
        self.sync_settings_fields();
        self.store.save_settings_async(self.settings.clone());
        self.settings_dirty_at = None;
        self.save_playback_async();
    }

    /// Updates the lrc.red contact on the API client and reloads lyrics if its availability flipped.
    pub fn apply_lrc(&mut self) {
        let contact = self.settings.word_lyrics.then_some(APP_URL);
        if let Backend::Live(c) = &self.backend {
            c.set_lrcred_contact(contact);
        }
        let usable = contact.is_some();
        if usable != self.lrc_usable {
            self.lrc_usable = usable;
            self.lyrics.for_track = None;
            self.load_lyrics_for_current();
        }
    }

    pub fn set_quality(&mut self, quality: crate::store::QualityPref) {
        if self.settings.quality == quality {
            return;
        }
        self.settings.quality = quality;
        self.mark_settings_dirty();
        self.reload_current();
    }

    pub fn set_word_lyrics(&mut self, on: bool) {
        self.settings.word_lyrics = on;
        self.apply_lrc();
        self.mark_settings_dirty();
    }

    pub fn set_fkcensor(&mut self, on: bool) {
        if self.settings.fkcensor == on {
            return;
        }
        self.settings.fkcensor = on;
        if on && let Backend::Live(client) = &self.backend {
            self.refresh_fkcensor(client.clone());
        }
        self.reapply_fkcensor();
        self.reload_current();
        self.mark_settings_dirty();
    }

    fn reapply_fkcensor(&mut self) {
        if self.settings.fkcensor
            && let Load::Ready(tracks) = &mut self.library.tracks
        {
            self.fkcensor.patch_tracks(tracks);
        }
        match self.detail.page.clone() {
            Some(Page::Album(id)) => self.open_album(&id),
            Some(Page::Artist(id)) => self.open_artist(&id),
            _ => {}
        }
    }

    pub(super) fn refresh_fkcensor(&mut self, client: ym_api::Client) {
        self.fkcensor_checked = self.now;
        let fkcensor = self.fkcensor.clone();
        self.job(async move { fkcensor.refresh(&client).await }, |s, _tx, changed| {
            if changed && s.settings.fkcensor {
                s.reapply_fkcensor();
                s.reload_current();
            }
        });
    }

    pub fn set_lang(&mut self, lang: Lang) {
        self.lang = lang;
        if let Backend::Live(c) = &self.backend {
            c.set_language(lang.code());
        }
        self.mark_settings_dirty();
        if self.session != Session::LoggedOut {
            self.reload_content();
        }
    }

    /// Whether custom GPU shaders are active right now (off in CPU mode or when disabled in Settings).
    pub fn shaders_active(&self) -> bool {
        self.settings.shaders && !crate::fx::cpu_mode()
    }

    /// Whether the Home butterflies should animate this frame.
    pub fn bugs_alive(&self) -> bool {
        self.page == Page::Home
            && !self.np_open
            && self.session != Session::LoggedOut
            && self.settings.butterflies
            && !self.settings.reduce_motion
            && self.shaders_active()
    }

    /// Advances all per-frame state. Returns true when the view needs a rebuild.
    pub fn tick(&mut self, t: f64) -> bool {
        self.prev_now = self.now;
        self.now = t;
        self.sync_backdrop();
        if self.settings.fkcensor
            && t - self.fkcensor_checked >= 600.0
            && let Backend::Live(client) = &self.backend
        {
            self.refresh_fkcensor(client.clone());
        }
        if self.settings.theme == ThemeMode::System && t - self.sys_checked > 2.0 {
            self.sys_checked = t;
            self.sys_light = theme::system_light();
        }
        let raw_dt = (self.now - self.prev_now).max(0.0) as f32;
        let dt = raw_dt.min(0.25);
        // Waking from an idle window produces a large dt; step UI springs by one frame instead
        // of skipping past the start of an opening animation.
        let anim_dt = if raw_dt > 0.1 { 1.0 / 60.0 } else { raw_dt.min(0.05) };
        let mut changed = false;
        if self.vol_close_at.is_some_and(|at| self.now >= at) {
            self.vol_close_at = None;
            if !self.vol_button_hover && !self.vol_card_hover {
                self.vol_open = false;
                changed = true;
            }
        }
        changed |= self.tick_menu(anim_dt);
        let arc_diff = self.wave_arc_to - self.wave_arc;
        if arc_diff.abs() > 0.0005 || self.wave_arc_vel.abs() > 0.002 {
            if self.settings.reduce_motion || crate::fx::cpu_mode() {
                self.wave_arc = self.wave_arc_to;
                self.wave_arc_vel = 0.0;
            } else {
                let omega = std::f32::consts::TAU / 0.24;
                let zeta = 0.96;
                let steps = (anim_dt * 240.0).ceil().clamp(1.0, 24.0);
                let h = anim_dt / steps;
                for _ in 0..steps as u32 {
                    let accel = omega * omega * (self.wave_arc_to - self.wave_arc) - 2.0 * zeta * omega * self.wave_arc_vel;
                    self.wave_arc_vel += accel * h;
                    self.wave_arc += self.wave_arc_vel * h;
                }
                if (self.wave_arc_to - self.wave_arc).abs() <= 0.0005 && self.wave_arc_vel.abs() <= 0.005 {
                    self.wave_arc = self.wave_arc_to;
                    self.wave_arc_vel = 0.0;
                    let count = self.arc_wave_count().max(1) as f32;
                    if self.wave_arc.abs() > count * 100.0 {
                        let shift = (self.wave_arc / count).floor() * count;
                        self.wave_arc -= shift;
                        self.wave_arc_to -= shift;
                    }
                }
            }
            changed = true;
        }
        changed |= self.tick_player(dt);
        changed |= self.drive_art();
        if self.launch_play && matches!(self.backend, Backend::Live(_)) && self.boot_playback_done {
            self.launch_play = false;
            if self.current().is_some() {
                if !self.player.playing {
                    self.toggle_pause();
                }
            } else if self.player.queue.is_empty() {
                let id = self.wave_choice.clone();
                self.start_wave(&id);
            }
            changed = true;
        }
        if self.autoplay
            && let Load::Ready(tracks) = &self.library.tracks
            && !tracks.is_empty()
        {
            self.autoplay = false;
            let queue = tracks.clone();
            self.play_queue(queue, 0, Source::Manual);
            if std::env::var("GLINT_AUTOPLAY").is_ok_and(|v| v == "np") {
                self.set_np(true);
            }
        }
        self.sync_desktop();
        self.sync_mini();
        if self.page == Page::Search {
            self.drive_search();
        }
        let before = self.toasts.len();
        self.toasts.retain(|x| self.now - x.born < 3.4);
        changed |= before != self.toasts.len();
        if self.download_activity.is_some_and(|a| a.active == 0 && self.now - a.changed_at >= 3.8) {
            self.download_activity = None;
            changed = true;
        }
        let burst = self.now - self.like_burst < 1.3;
        if burst != self.burst_on {
            self.burst_on = burst;
            changed = true;
        }
        if let Some(at) = self.settings_dirty_at
            && self.now - at > 0.8
        {
            self.save_settings_async();
        }
        let since = self.now - self.playback_saved_at;
        if (self.playback_dirty && since > 1.0) || (self.player.playing && since > 15.0) {
            self.save_playback_async();
        }
        let pipes_gen = crate::fx::pipes_settled_gen();
        if pipes_gen != self.pipes_gen_seen {
            self.pipes_gen_seen = pipes_gen;
            if self.now - self.ws_trimmed_at >= 12.0 {
                self.startup_trim_at = Some(self.now + 1.5);
            }
        }
        if !self.menu_moving() && !self.is_resizing() && self.startup_trim_at.is_some_and(|at| self.now >= at) && (crate::fx::cpu_mode() || !self.shaders_active() || crate::fx::ready()) {
            self.startup_trim_at = None;
            crate::diag_log::trim_working_set();
            self.ws_trimmed_at = self.now;
            self.log_mem("startup:trim");
        }
        // Evict offscreen covers promptly once animations settle (mounted covers are never touched
        // so visible rows never flash). Only keep the active track's 600px cover while Now Playing
        // is actually open; never call `trim_working_set` mid-transition.
        let keep_covers = || -> Vec<String> {
            let mut v = Vec::with_capacity(2 + self.pinned_items.len());
            if let Some(tpl) = self.current().and_then(|t| t.cover_template()) {
                if let Some(card) = State::cover_key(Some(tpl)) {
                    v.push(card);
                }
                if self.np_open
                    && !crate::fx::cpu_mode()
                    && let Some(lg) = State::cover_key_px(Some(tpl), crate::art::COVER_LG)
                {
                    v.push(lg);
                }
            }
            for item in &self.pinned_items {
                if let Some(sm) = State::cover_key_px(item.cover.as_deref(), crate::art::COVER_SM) {
                    v.push(sm);
                }
            }
            v
        };
        let due_nav_trim = self.art_trim_at.is_some_and(|at| self.now >= at);
        let due_periodic_trim = self.now - self.art_trimmed_at >= 3.0;
        if !self.menu_moving() && !self.is_resizing() && (due_nav_trim || due_periodic_trim) {
            let keep = keep_covers();
            let refs: Vec<&str> = keep.iter().map(String::as_str).collect();
            let evicted = self.art.trim_offscreen(&refs);
            if evicted {
                self.art_evict_gen = self.art_evict_gen.wrapping_add(1);
            }
            if self.page != Page::Home || self.np_open {
                crate::ui::blobs::clear_cache();
            }
            if evicted && self.now - self.ws_trimmed_at >= 18.0 {
                crate::diag_log::trim_working_set();
                self.ws_trimmed_at = self.now;
            }
            self.art_trim_at = None;
            self.art_trimmed_at = self.now;
        }
        changed
    }
}
