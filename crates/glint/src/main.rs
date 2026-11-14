//! Glint: a Yandex Music client on the RUNE UI framework.

// A release build is a Windows GUI program, so starting it opens no console window. (A debug build
// keeps its console, where the log lines and the headless tools print.) Started from a terminal it
// still prints there, see `desktop::attach_parent_console`.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod art;
mod audio_tags;
mod bugs;
mod fkcensor;
mod demo;
mod desktop;
mod fx;
mod genre;
mod i18n;
mod net;
mod palette;
mod player;
mod scenes;
mod store;
mod theme;
mod ui;
mod update;

use std::time::Duration;

use rune::{Key, KeyEvent, Tx};

use app::{Load, NpTab, Page, State};
use store::Store;

fn main() {
    #[cfg(windows)]
    desktop::attach_parent_console();
    let headless = app::is_headless();
    if !headless {
        #[cfg(windows)]
        desktop::set_app_id();
        // The Wayland app id and X11 class, which match the window to `glint.desktop` and its icon.
        #[cfg(target_os = "linux")]
        rune::set_app_id("glint");
        desktop::set_window_icon();
    }
    let spec = rune::AppSpec {
        min_size: Some((960.0, 640.0)),
        scenes: scenes::SCENES,
        poll: Some(poll),
        on_key: Some(on_key),
        ..rune::AppSpec::new("Glint", (1320.0, 820.0), || State::new(None, Store::none(), true), ui::view)
    };
    // Where the window was last closed, if that is still on a display.
    let saved = if headless { None } else { Store::open().load_window().filter(desktop::place::on_screen) };
    let mut app = rune::App::new(spec);
    if let Some(w) = saved {
        app = app
            .window_options(rune::WindowOptions { position: Some(w.position), physical_size: Some(w.size), ..Default::default() })
            .maximized(w.maximized);
    }
    let app = app
        .titlebar(rune::Titlebar::new(ui::kit::TITLE_H))
        .on_window(desktop::place::keep_window)
        .compositor(fx::Gfx::new())
        .srgb_blend(true)
        // Closing the window only hides it; whether that ends the app is decided in `on_close`.
        .keep_running()
        .on_close(|s: &mut State, tx: &mut Tx| {
            s.save_settings_now();
            desktop::place::save(&s.store);
            if !s.keeps_running_when_closed() {
                tx.quit();
            }
        })
        .on_quit(|s: &mut State, _tx: &mut Tx| {
            s.save_settings_now();
            desktop::place::save(&s.store);
            s.shutdown_desktop();
        });
    let handle = app.handle();
    app.init(move || {
        let store = if headless { Store::none() } else { Store::open() };
        let mut state = State::new(Some(handle), store, headless);
        if !headless {
            state.boot();
        }
        state
    })
    .run();
}

/// Runs at the start of every frame: advances the player and turns what it found into requests.
fn poll(s: &mut State, t: f64, tx: &mut Tx) -> bool {
    let mut changed = s.tick(t);

    // A hidden window (closed to the tray) is only woken when asked: keep the player's clock, the
    // media card and the end-of-track logic ticking while something plays.
    if s.player.playing || s.player.loading {
        tx.wake_after(std::time::Duration::from_millis(250));
    }
    // The page's own wheel scrolling eases toward where the wheel sent it.
    let dt = (s.now - s.prev_now).clamp(0.0, 0.05) as f32;
    // The butterflies on Home live while Home shows.
    if s.bugs_alive() {
        if s.swarm.seed != s.home_seed {
            s.swarm = bugs::Swarm::new(s.home_seed);
            changed = true;
        }
        let scroll = app::page_scroll("home").map_or(0.0, |p| p[0]);
        let geom = s.bug_geom.borrow().clone();
        if let Some(next) = s.swarm.step(s.now, dt, &geom, scroll) {
            tx.wake_after(next.max(Duration::from_millis(33)));
        }
    }
    let reduce = s.settings.reduce_motion;
    if let Some(y) = s.glide.step(dt, reduce) {
        tx.scroll_to_instant(s.glide.key.clone(), 0.0, y);
        tx.wake_after(Duration::from_millis(8));
    }
    // The profile menu opens and closes on a spring: a step every frame until it is still.
    if s.menu_moving() {
        tx.wake_after(Duration::from_millis(16));
        changed = true;
    }
    // The sung line comes forward and the one before steps back, frame by frame.
    if ui::np::step_lines(s, dt) && s.np_open && s.np_panel() == NpTab::Lyrics {
        tx.wake_after(Duration::from_millis(16));
        changed = true;
    }
    // The line being sung fills in word by word: redraw every frame while it does.
    if s.np_open && s.np_panel() == NpTab::Lyrics && (ui::np::wiping(s) || ui::np::counting(s)) {
        tx.wake_after(Duration::from_millis(16));
        changed = true;
    }

    // Keep the lyric that is being sung in the middle of its panel.
    let showing = s.np_open && s.np_panel() == NpTab::Lyrics && s.current().is_some();
    if showing != s.lyrics_showing {
        s.lyrics_showing = showing;
        if showing {
            s.scroll_lyrics = true;
        }
    }
    if showing && s.scroll_lyrics {
        s.scroll_lyrics = false;
        // Before the first line has started (a seek back to the intro, say) nothing is being sung
        // yet, and the list goes back to the top so the first line waits at its resting place.
        // With a count-in, the note above the lines is what waits there.
        let counted = matches!(&s.lyrics.lyrics, Load::Ready(l) if ui::np::count_start(l).is_some());
        let target = match s.lyrics.active {
            Some(i) => format!("ly-{i}"),
            None if counted => "ly-note".to_owned(),
            None => "ly-0".to_owned(),
        };
        // The clock moved the line, not a press, and RUNE lands such a change at once: ask for
        // the glide (unless the look says the lines change at once).
        if s.settings.lyric_look.anim > 0 && !s.settings.reduce_motion {
            tx.animated();
        }
        tx.scroll_center_in("np-lyrics", target);
    }

    if s.focus_search {
        s.focus_search = false;
        if s.page == Page::Search {
            tx.focus("search-input", false);
        }
    }
    changed
}

/// Shortcuts: keys no text field wants.
fn on_key(s: &mut State, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let typing = matches!(tx.focused(), Some("search-input" | "token" | "lib-search"));
    let ctrl = ev.mods.cmd || ev.mods.ctrl;
    if matches!(ev.key, Key::Escape) {
        if s.sheet_open {
            s.sheet_open = false;
            return true;
        }
        if s.lightbox_open {
            s.lightbox_open = false;
            return true;
        }
        if s.popover_open {
            s.close_popover();
            return true;
        }
        if s.tmenu_open {
            s.close_track_menu();
            return true;
        }
        if s.menu_open {
            s.menu_open = false;
            return true;
        }
        if s.np_open {
            s.set_np(false);
            return true;
        }
        if !s.history.is_empty() && !typing {
            s.back();
            return true;
        }
        return false;
    }
    if s.session == app::Session::LoggedOut {
        return false;
    }
    // Ctrl+K / Ctrl+F / Ctrl+L (also on a Russian layout, where the key arrives as its US twin).
    if let Key::Command(c) = &ev.key {
        if matches!(c, 'k' | 'f' | 'l') {
            s.go(Page::Search);
            s.focus_search = true;
            return true;
        }
        return false;
    }
    if typing {
        return false;
    }
    let has_track = s.current().is_some();
    // Alt+Left / Alt+Right: back and forward through the pages.
    if ev.mods.alt && matches!(ev.key, Key::Left | Key::Right) {
        if matches!(ev.key, Key::Left) {
            s.back();
        } else {
            s.go_forward();
        }
        return true;
    }
    match &ev.key {
        Key::Text(t) if t == " " => {
            if has_track {
                s.toggle_pause();
                return true;
            }
            false
        }
        Key::Left | Key::Right if has_track => {
            let forward = matches!(ev.key, Key::Right);
            if ctrl {
                if forward { s.next() } else { s.prev() }
            } else {
                let delta: i64 = if forward { 5_000 } else { -5_000 };
                let end = s.player.dur_ms.saturating_sub(1) as i64;
                s.seek_ms((s.player.pos_ms as i64 + delta).clamp(0, end.max(0)) as u64);
            }
            true
        }
        Key::Up | Key::Down => {
            let step = if matches!(ev.key, Key::Up) { 0.05 } else { -0.05 };
            s.set_volume((s.player.volume + step).clamp(0.0, 1.0));
            true
        }
        Key::Text(t) => match t.to_lowercase().as_str() {
            "l" | "д" if has_track => {
                if let Some(track) = s.current().cloned() {
                    let liking = !s.is_liked(&track.id);
                    s.toggle_like(&track);
                    if liking {
                        s.like_burst = s.now;
                        s.burst_track = track.id;
                        s.burst_on = true;
                    }
                }
                true
            }
            "m" | "ь" => {
                s.toggle_mute();
                true
            }
            "f" | "а" if has_track => {
                s.set_np(!s.np_open);
                true
            }
            _ => false,
        },
        _ => false,
    }
}
