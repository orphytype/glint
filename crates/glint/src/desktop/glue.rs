//! The seam between the app and the desktop shell: starts it once the window is up, feeds it the
//! player's state every frame, and carries out what the media keys and the tray ask for.

use std::time::Instant;

use super::{Command, Desktop, Playback};
use crate::app::{Source, State};
use crate::store::Repeat;

impl State {
    /// Starts the tray and the media session on the first frame, when the window exists. Not in
    /// headless runs: they have no window to attach to and must not put icons in anyone's tray.
    fn ensure_desktop(&mut self) {
        if self.headless || self.desktop.is_some() {
            return;
        }
        let Some(app) = self.app.clone() else { return };
        self.desktop = Some(Desktop::start(self.lang, self.settings.show_tray, move |command| match command {
            Command::Show => app.open(),
            Command::Quit => app.quit(),
            other => app.update(move |s, _| s.on_desktop(other)),
        }));
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
            has_next: p.index.is_some_and(|i| i + 1 < p.queue.len()) || p.repeat == Repeat::All || matches!(p.source, Source::Wave { .. }),
        };
        let Some(desktop) = self.desktop.as_mut() else { return };
        if desktop.sync(&view, self.lang, Instant::now()) {
            self.send_card_cover();
        }
    }

    /// Fetches the playing track's cover once more, small and as a PNG, for the media card. (The
    /// pictures in the window live on the GPU side and cannot be read back.)
    fn send_card_cover(&self) {
        let Some(track) = self.current() else {
            // Nothing is playing: the next same track must fetch its cover again.
            *self.card_cover.borrow_mut() = None;
            return;
        };
        let Some(key) = State::cover_key(track.cover_template(), 300) else { return };
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
        self.job(
            async move {
                let img = backend.image(&key, 300).await?;
                tokio::task::spawn_blocking(move || super::cover_png(img.width, img.height, img.rgba, 300)).await.ok().flatten()
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

    pub fn set_close_to_tray(&mut self, on: bool) {
        self.settings.close_to_tray = on;
        self.mark_settings_dirty();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Session;
    use crate::store::Store;

    fn state() -> State {
        let mut s = State::new(None, Store::none(), true);
        s.backend = crate::net::Backend::demo();
        s.session = Session::Demo;
        s
    }

    fn playing(n: usize) -> State {
        let mut s = state();
        let tracks = crate::demo::Catalog::new().tracks.into_iter().take(n).collect();
        s.play_queue(tracks, 0, Source::Manual);
        s.player.loading = false;
        s
    }

    #[test]
    fn play_and_pause_are_not_toggles() {
        let mut s = playing(3);
        assert!(s.player.playing);
        s.on_desktop(Command::Play);
        assert!(s.player.playing, "Play while playing does nothing");
        s.on_desktop(Command::Pause);
        assert!(!s.player.playing);
        s.on_desktop(Command::Pause);
        assert!(!s.player.playing, "Pause while paused does nothing");
        s.on_desktop(Command::Play);
        assert!(s.player.playing);
        s.on_desktop(Command::Toggle);
        assert!(!s.player.playing);
        s.on_desktop(Command::Toggle);
        assert!(s.player.playing);
    }

    #[test]
    fn next_and_previous_move_through_the_queue() {
        let mut s = playing(3);
        s.on_desktop(Command::Next);
        assert_eq!(s.player.index, Some(1));
        s.on_desktop(Command::Next);
        assert_eq!(s.player.index, Some(2));
        s.on_desktop(Command::Next);
        assert_eq!(s.player.index, Some(2), "the end of the queue stays put");
        s.player.pos_ms = 0;
        s.on_desktop(Command::Prev);
        assert_eq!(s.player.index, Some(1));
    }

    #[test]
    fn stop_pauses_and_rewinds_but_keeps_the_queue() {
        let mut s = playing(3);
        s.player.pos_ms = 42_000;
        s.player.dur_ms = 200_000;
        s.on_desktop(Command::Stop);
        assert!(!s.player.playing);
        assert_eq!(s.player.pos_ms, 0);
        assert_eq!(s.player.queue.len(), 3);
        assert_eq!(s.player.index, Some(0));
    }

    #[test]
    fn seeking_from_the_flyout_stays_inside_the_track() {
        let mut s = playing(2);
        s.player.dur_ms = 200_000;
        s.on_desktop(Command::Seek(90_000));
        assert_eq!(s.player.pos_ms, 90_000);
        s.on_desktop(Command::Seek(9_999_999));
        assert_eq!(s.player.pos_ms, 199_999, "never past the end");
    }

    #[test]
    fn commands_with_nothing_loaded_do_nothing() {
        let mut s = state();
        for c in [Command::Next, Command::Prev, Command::Stop, Command::Seek(5_000), Command::Pause, Command::Play, Command::Toggle] {
            s.on_desktop(c);
        }
        assert!(s.current().is_none());
        assert_eq!((s.player.pos_ms, s.player.playing), (0, false));
    }

    #[test]
    fn play_with_a_queue_but_no_current_track_starts_it() {
        let mut s = state();
        s.player.queue = crate::demo::Catalog::new().tracks.into_iter().take(2).collect();
        s.on_desktop(Command::Play);
        assert_eq!(s.player.index, Some(0));
        assert!(s.player.playing);
    }

    #[test]
    fn close_to_tray_needs_a_tray_that_exists() {
        let mut s = state();
        assert!(!s.keeps_running_when_closed(), "off by default");
        s.settings.close_to_tray = true;
        assert!(!s.keeps_running_when_closed(), "no tray (headless), so closing must quit");
    }

    #[test]
    fn the_desktop_never_starts_in_a_headless_run() {
        let mut s = state();
        s.sync_desktop();
        assert!(s.desktop.is_none());
    }

    #[test]
    fn tray_settings_are_saved() {
        let mut s = state();
        assert!(s.settings.show_tray && !s.settings.close_to_tray, "the defaults");
        s.set_close_to_tray(true);
        s.set_show_tray(false);
        assert!(s.settings.close_to_tray && !s.settings.show_tray);
        assert!(s.settings_dirty_at.is_some());
    }
}
