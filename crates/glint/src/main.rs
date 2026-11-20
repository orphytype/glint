//! Glint: a Yandex Music client on the RUNE UI framework.

// A release build is a Windows GUI program, so starting it opens no console window. (A debug build
// keeps its console, where the log lines and the headless tools print.) Started from a terminal it
// still prints there, see `desktop::attach_parent_console`.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod art;
mod audio_tags;
mod autostart;
mod backdrop;
mod bugs;
mod desktop;
#[cfg(feature = "tooling")]
mod fixtures;
mod fkcensor;
mod fx;
mod genre;
mod i18n;
mod keys;
mod mark;
mod net;
mod palette;
mod player;
mod playlists;
#[cfg(feature = "tooling")]
mod scenes;
mod store;
mod theme;
mod ui;
mod update;
mod wordmark;

use std::time::Duration;

use rune::{Key, KeyEvent, Tx};

use app::{Load, NpTab, Page, State};
use store::Store;

/// Names the running Glint to a later launch (the pipe on Windows, the socket on Linux).
const INSTANCE_ID: &str = "io.github.orphytype.glint";

static APP_FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/Commissioner[FLAR,VOLM,slnt,wght].ttf"),
    include_bytes!("../assets/fonts/Cormorant[wght].ttf"),
    include_bytes!("../assets/fonts/Cormorant-Italic[wght].ttf"),
];

fn main() {
    rune::marks::mark("glint.main");
    #[cfg(windows)]
    desktop::attach_parent_console();
    let headless = app::is_headless();
    // One Glint per person: a second launch asks the first to come forward (out of the tray, up
    // from behind other windows) and ends. A run with its own data folder is a test copy and is
    // left to run beside the real one.
    if !headless && std::env::var_os("GLINT_DATA_DIR").is_none() && !rune::single_instance(INSTANCE_ID) {
        return;
    }
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
        #[cfg(feature = "tooling")]
        scenes: scenes::SCENES,
        fonts: APP_FONTS,
        families: ("Commissioner", "IBM Plex Mono"),
        poll: Some(poll),
        on_key: Some(on_key),
        ..rune::AppSpec::new("Glint", (1320.0, 820.0), || State::new(None, Store::none(), true), ui::view)
    };
    // Where the window was last closed, if that is still on a display.
    let store = (!headless).then(Store::open);
    let saved = store.as_ref().and_then(|s| s.load_window()).filter(desktop::place::on_screen);
    // Started at login with "start in the tray" on: the window waits until the tray opens it.
    let hidden = store.as_ref().is_some_and(|s| {
        let set = s.load();
        autostart::started_at_login() && set.autostart && set.autostart_hidden && set.show_tray
    });
    let mut app = rune::App::new(spec);
    let mut options = rune::WindowOptions { visible: !hidden, ..Default::default() };
    if let Some(w) = &saved {
        (options.position, options.physical_size) = (Some(w.position), Some(w.size));
    }
    app = app.window_options(options);
    if let Some(w) = saved {
        app = app.maximized(w.maximized);
    }
    let app = app
        .titlebar(rune::Titlebar::new(ui::kit::CAPTION_H))
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
            rune::release_instance();
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
    startup_report(tx);
    // The compositor came up: build the view again, with its effect slots this time.
    {
        use std::sync::atomic::{AtomicBool, Ordering};
        static SLOTS: AtomicBool = AtomicBool::new(false);
        if fx::ready() && !SLOTS.swap(true, Ordering::Relaxed) {
            changed = true;
        }
    }
    // The launch screen moves every frame until it has faded out.
    if ui::splash::step(s) {
        tx.wake_after(Duration::from_millis(16));
        changed = true;
    } else if ui::splash::waiting(s) {
        // Started in the tray: the launch screen begins once the window is shown.
        tx.wake_after(Duration::from_millis(250));
    }

    // A hidden window (closed to the tray) is only woken when asked: keep the player's clock, the
    // media card and the end-of-track logic ticking while something plays.
    if s.desktop_pending() {
        tx.wake_after(Duration::from_secs_f64((desktop::glue::DESKTOP_AFTER_S - s.now).max(0.0) + 0.02));
    }
    if s.player.playing || s.player.loading {
        tx.wake_after(std::time::Duration::from_millis(250));
    }
    // The page's own wheel scrolling eases toward where the wheel sent it.
    let dt = (s.now - s.prev_now).clamp(0.0, 0.05) as f32;
    // Passive shader effects (stage glow, garden, seek bars) and butterflies tick at 30 Hz instead
    // of spinning at display refresh rate; input, scrolling and UI transitions still render at full
    // rate. Touching `fx_pulse` wakes a repaint without rebuilding the view or layout.
    const FX_STEP: f64 = 1.0 / 30.0;
    let visible = s.app.as_ref().is_none_or(|app| app.is_visible());
    let fx_on = visible && fx::wants_tick();
    let bugs_on = visible && s.bugs_alive();
    if bugs_on && s.swarm.seed != s.home_seed {
        s.swarm = bugs::Swarm::new(s.home_seed);
        s.bugs_at = s.now;
        changed = true;
    }
    if fx_on || bugs_on {
        if fx_on {
            fx::arm_tick();
        }
        if s.now >= s.fx_next {
            let bug_dt = (s.now - s.bugs_at).clamp(0.0, 0.1) as f32;
            s.bugs_at = s.now;
            let mut bug_wait = None;
            if bugs_on {
                let scroll = app::page_scroll("home").map_or(0.0, |p| p[0]);
                let geom = s.bug_geom.borrow().clone();
                bug_wait = s.swarm.step(s.now, bug_dt, &geom, scroll);
            }
            let wait = if fx_on { Duration::from_secs_f64(FX_STEP) } else { bug_wait.map_or(Duration::from_secs_f64(FX_STEP), |d| d.max(Duration::from_secs_f64(FX_STEP))) };
            s.fx_next = if fx_on && s.fx_next > 0.0 && s.now - s.fx_next < FX_STEP { s.fx_next + FX_STEP } else { s.now + wait.as_secs_f64() };
            tx.wake_after(Duration::from_secs_f64((s.fx_next - s.now).max(0.004)));
            if fx_on {
                s.fx_phase = !s.fx_phase;
                s.fx_pulse.set([0.0, if s.fx_phase { 1.0 } else { 0.0 }]);
            }
        } else {
            tx.wake_after(Duration::from_secs_f64((s.fx_next - s.now).max(0.004)));
        }
    }
    let reduce = s.settings.reduce_motion;
    // A page come back to goes to where it was left, once it is tall enough for that (a list
    // that loads again grows in a moment); after a while it settles for as far as it goes.
    if let Some((key, y, until)) = s.scroll_restore.clone() {
        let late = s.now > until;
        match app::page_scroll(&key) {
            Some([offset, _]) if (offset - y).abs() < 1.0 => s.scroll_restore = None,
            Some([_, max]) if max >= y - 0.5 || (late && max > 0.0) => tx.scroll_to_instant(key, 0.0, y.min(max)),
            _ => {}
        }
        if late {
            s.scroll_restore = None;
        } else {
            tx.wake_after(Duration::from_millis(16));
        }
    }
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

    // The queue opens with the track before the playing one at its top, and follows as another
    // track starts (gliding there; at once as it opens).
    if s.np_open && s.np_panel() == NpTab::Queue && !s.player.queue.is_empty() {
        let cur = s.player.pending_index.or(s.player.index).unwrap_or(0);
        if s.queue_placed != Some(cur) {
            let y = ui::np::queue_offset(cur);
            if s.queue_placed.is_none() {
                tx.scroll_to_y("np-queue", y);
            } else {
                tx.scroll_to("np-queue", 0.0, y);
            }
            s.queue_placed = Some(cur);
            changed = true;
        }
    } else {
        s.queue_placed = None;
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
        // the glide (unless motion is reduced).
        if !s.settings.reduce_motion {
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

/// Shortcuts: keys no text field wants (`keys.rs` has what each one does).
fn on_key(s: &mut State, ev: &KeyEvent, tx: &mut Tx) -> bool {
    // Settings is waiting for the new key of an action.
    if let Some(act) = s.rebind {
        return s.record_key(act, ev, tx);
    }
    let typing = matches!(tx.focused(), Some("search-input" | "token" | "lib-search"));
    if matches!(ev.key, Key::Escape) {
        // A menu opened from Settings closes before Settings does.
        if s.popover_open && s.settings_open {
            s.close_popover();
            return true;
        }
        if s.settings_open {
            s.close_settings();
            return true;
        }
        if s.sheet_open {
            s.sheet_open = false;
            return true;
        }
        if s.popover_open {
            s.close_popover();
            return true;
        }
        if s.vol_open {
            s.vol_open = false;
            s.vol_button_hover = false;
            s.vol_card_hover = false;
            s.vol_close_at = None;
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
    s.answer_key(ev, tx, typing)
}

/// With `GLINT_STARTUP` set, prints how long each launch step took once the first frame is on
/// screen (from `main`), and with `GLINT_STARTUP=exit` quits there: the launch-time check.
fn startup_report(tx: &mut Tx) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static DONE: AtomicBool = AtomicBool::new(false);
    if DONE.load(Ordering::Relaxed) {
        return;
    }
    let Ok(mode) = std::env::var("GLINT_STARTUP") else {
        DONE.store(true, Ordering::Relaxed);
        return;
    };
    let marks = rune::marks::marks();
    let Some(start) = marks.iter().find(|(n, _)| *n == "glint.main").map(|(_, t)| *t) else { return };
    if !marks.iter().any(|(n, _)| *n == "frame.first.presented") {
        tx.wake_after(Duration::from_millis(1));
        return;
    }
    DONE.store(true, Ordering::Relaxed);
    for (name, at) in &marks {
        eprintln!("{:>8.1} ms  {name}", at.duration_since(start).as_secs_f64() * 1000.0);
    }
    for (name, v) in rune::marks::metrics() {
        eprintln!("{v:>12}  {name}");
    }
    if mode == "exit" {
        tx.quit();
    }
}
