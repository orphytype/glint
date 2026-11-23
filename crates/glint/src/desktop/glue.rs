//! The seam between the app and the desktop shell: starts it once the window is up, feeds it the
//! player's state every frame, and carries out what the media keys and the tray ask for.

use std::time::Instant;

use super::{Command, Desktop, Playback};
use crate::app::State;

/// Seconds after launch the tray and the media session are started.
pub const DESKTOP_AFTER_S: f64 = 0.4;
impl State {
    /// Starts the tray and the media session on the first frame, when the window exists. Not in
    /// headless runs: they have no window to attach to and must not put icons in anyone's tray.
    fn ensure_desktop(&mut self) {
        if self.headless || self.desktop.is_some() {
            return;
        }
        // The tray icon and the media session come up just after the window does: made with the
        // first frame they held it back ~60 ms. Something playing can't wait for them.
        if self.now < DESKTOP_AFTER_S && self.current().is_none() {
            return;
        }
        let Some(app) = self.app.clone() else { return };
        self.desktop = Some(Desktop::start(self.lang, self.settings.show_tray, move |command| match command {
            Command::Show => app.open(),
            Command::Quit => app.quit(),
            other => app.update(move |s, tx| {
                let was = s.track_mark();
                s.on_desktop(other);
                s.animate_if_moved(tx, &was);
            }),
        }));
    }

    /// Whether the tray and the media session are still to be started (`poll` wakes for them).
    pub fn desktop_pending(&self) -> bool {
        !self.headless && self.desktop.is_none()
    }

    /// Pushes the player's state to the tray and the media card. Cheap enough for every frame: it
    /// compares against what it sent last and talks to the OS only about what changed.
    pub fn sync_desktop(&mut self) {
        if self.headless {
            return;
        }
        self.ensure_desktop();
        let p = &self.player;
        let view = Playback {
            track: p.index.and_then(|i| p.queue.get(i)),
            playing: p.playing,
            loading: p.loading,
            pos_ms: p.pos_ms,
            dur_ms: p.dur_ms,
            // What `next()` would do: a following track, the queue's start again, or a fresh batch.
            has_next: self.can_next(),
            has_prev: self.can_prev(),
            liked: p.index.and_then(|i| p.queue.get(i)).is_some_and(|t| self.is_liked(&t.id)),
            shuffle: p.shuffle,
            repeat: p.repeat,
            muted: p.volume < 0.001,
        };
        let Some(desktop) = self.desktop.as_mut() else { return };
        if desktop.sync(&view, self.lang, Instant::now()) {
            self.send_card_cover();
        }
    }

    /// Sends the playing track's cover to the OS media card as PNG, reusing the RAM or disk cache
    /// whenever the 300x300 cover is already loaded so it is never fetched twice.
    fn send_card_cover(&self) {
        let Some(track) = self.current() else {
            // Nothing is playing: the next same track must send its cover again.
            *self.card_cover.borrow_mut() = None;
            return;
        };
        let Some(key) = State::cover_key(track.cover_template()) else { return };
        let id = track.id.clone();
        // The card asks once per track: no need to fetch and encode the same picture again.
        {
            let mut asked = self.card_cover.borrow_mut();
            if asked.as_deref() == Some(id.as_str()) {
                return;
            }
            *asked = Some(id.clone());
        }
        let backend = self.backend.clone();
        let art = self.art.clone();
        let disk = self.cache.clone();
        self.job(
            async move {
                let from_ram = art.encoded(&key);
                let disk_read = disk.clone();
                let key_disk = key.clone();
                let local = tokio::task::spawn_blocking(move || from_ram.or_else(|| disk_read.as_ref().and_then(|d| d.get(&key_disk)))).await.ok().flatten();
                let img = match local {
                    Some(raw) => tokio::task::spawn_blocking(move || crate::art::decode(&raw, crate::art::COVER_PX)).await.ok().flatten(),
                    None => match backend.image_bytes(&key).await {
                        Some(raw) => {
                            let disk_put = disk.clone();
                            let art_put = art.clone();
                            let key_put = key.clone();
                            tokio::task::spawn_blocking(move || {
                                let img = crate::art::decode(&raw, crate::art::COVER_PX)?;
                                if let Some(d) = disk_put {
                                    let _ = d.put(&key_put, &raw);
                                }
                                art_put.store_encoded(&key_put, raw);
                                Some(img)
                            })
                            .await
                            .ok()
                            .flatten()
                        }
                        None => backend.image(&key, crate::art::COVER_PX).await,
                    },
                }?;
                tokio::task::spawn_blocking(move || super::cover_png(img.width, img.height, img.rgba, crate::art::COVER_PX)).await.ok().flatten()
            },
            move |s, _, png| {
                if let (Some(png), Some(desktop)) = (png, s.desktop.as_ref()) {
                    desktop.set_cover(&id, png);
                }
            },
        );
    }

    /// Carries out what the media keys, the headset or the tray menu asked for.
    pub fn on_desktop(&mut self, command: Command) {
        let loaded = self.current().is_some();
        match command {
            Command::Play if !self.player.playing => self.toggle_pause(),
            Command::Pause if self.player.playing => self.toggle_pause(),
            Command::Toggle => self.toggle_pause(),
            Command::Next if loaded => self.next(),
            Command::Prev if loaded => self.prev(),
            // "Stop" keeps the queue: it pauses and goes back to the start.
            Command::Stop if loaded => {
                if self.player.playing {
                    self.toggle_pause();
                }
                self.seek_ms(0);
            }
            Command::Like if loaded => {
                if let Some(t) = self.current().cloned() {
                    self.toggle_like(&t);
                }
            }
            Command::Shuffle => self.toggle_shuffle(),
            Command::Repeat => self.cycle_repeat(),
            Command::Mute => self.toggle_mute(),
            Command::MiniPlayer => self.toggle_mini_player(),
            Command::Seek(ms) if loaded => {
                let end = self.player.dur_ms.saturating_sub(1);
                self.seek_ms(ms.min(end));
            }
            _ => {}
        }
        // The window may be hidden, so no frame is coming to do this.
        self.sync_desktop();
    }

    /// Takes the tray icon and the media card down. Called as the app quits.
    pub fn shutdown_desktop(&mut self) {
        if let Some(desktop) = self.desktop.take() {
            desktop.shutdown();
        }
    }

    /// Whether the close button should leave the app running in the tray instead of quitting.
    pub fn keeps_running_when_closed(&self) -> bool {
        self.settings.close_to_tray && self.desktop.as_ref().is_some_and(Desktop::tray_alive)
    }

    pub fn set_show_tray(&mut self, on: bool) {
        self.settings.show_tray = on;
        if let Some(desktop) = self.desktop.as_mut() {
            desktop.set_tray(on);
        }
        self.mark_settings_dirty();
    }

    pub fn set_autostart(&mut self, on: bool) {
        self.settings.autostart = on;
        crate::autostart::apply(on);
        self.mark_settings_dirty();
    }

    pub fn set_close_to_tray(&mut self, on: bool) {
        self.settings.close_to_tray = on;
        self.mark_settings_dirty();
    }
}
