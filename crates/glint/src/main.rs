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
mod diag_log;
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

#[cfg(feature = "diag")]
#[global_allocator]
static ALLOC: procwatch::alloc::Counting = procwatch::alloc::Counting::system();

use std::time::Duration;

use rune::{Key, KeyEvent, Tx};

use app::{Load, NpTab, Page, State};
use store::Store;

/// Names the running Glint to a later launch (the pipe on Windows, the socket on Linux).
const INSTANCE_ID: &str = "io.github.orphytype.glint";

pub(crate) static APP_FONTS: &[&[u8]] = &[
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
    rune::prewarm_gpu();
    // Where the window was last closed, if that is still on a display.
    let store = if headless { Store::none() } else { Store::open() };
    let settings = store.load();
    if !headless {
        diag_log::init(store.logs_dir(), settings.logging);
    }
    let spec = rune::AppSpec {
        min_size: Some((840.0, 560.0)),
        #[cfg(feature = "tooling")]
        scenes: scenes::SCENES,
        fonts: APP_FONTS,
        families: ("Commissioner", "IBM Plex Mono"),
        poll: Some(poll),
        on_key: Some(on_key),
        ..rune::AppSpec::new("Glint", (1320.0, 820.0), || State::new(None, Store::none(), store::Settings::default(), true), ui::view)
    };
    let saved = (!headless).then(|| store.load_window()).flatten().filter(desktop::place::on_screen);
    // Started at login with "start in the tray" on: the window waits until the tray opens it.
    let hidden = !headless && autostart::started_at_login() && settings.autostart && settings.autostart_hidden && settings.show_tray;
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
        .on_focus(on_focus)
        .on_rest(|s: &mut State, _tx: &mut Tx| {
            s.art.trim_idle();
            s.log_mem("rest");
            false
        })
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
            diag_log::event("app", "quit");
            s.save_settings_now();
            desktop::place::save(&s.store);
            s.shutdown_desktop();
            rune::release_instance();
        });
    let handle = app.handle();
    app.init(move || {
        let mut state = State::new(Some(handle), store, settings, headless);
        if !headless {
            #[cfg(feature = "diag")]
            if let Ok(name) = std::env::var("GLINT_DIAG_SCENE")
                && let Some((_, apply)) = scenes::SCENES.iter().find(|(k, _)| *k == name)
            {
                apply(&mut state);
                state.player.sim_started = true;
            } else {
                state.boot();
            }
            #[cfg(not(feature = "diag"))]
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
    // The compositor tiers came up: rebuild the view with glass or shape slots as each finishes.
    {
        use std::sync::atomic::{AtomicBool, Ordering};
        static GPU: AtomicBool = AtomicBool::new(false);
        static GLASS: AtomicBool = AtomicBool::new(false);
        static SHAPES: AtomicBool = AtomicBool::new(false);
        static SLOTS: AtomicBool = AtomicBool::new(false);
        if !GPU.load(Ordering::Relaxed) && fx::gpu_up() && !GPU.swap(true, Ordering::Relaxed) {
            diag_log::write_system(&s.settings, s.update_method, s.win_size.get());
            s.log_mem("gpu:ready");
            changed = true;
        }
        if !GLASS.load(Ordering::Relaxed) && fx::glass_ready() && !GLASS.swap(true, Ordering::Relaxed) {
            changed = true;
        }
        if !SHAPES.load(Ordering::Relaxed) && fx::shapes_ready() && !SHAPES.swap(true, Ordering::Relaxed) {
            changed = true;
        }
        if !SLOTS.load(Ordering::Relaxed) && fx::ready() && !SLOTS.swap(true, Ordering::Relaxed) {
            changed = true;
        }
    }
    let visible = s.app.as_ref().is_none_or(rune::AppHandle::is_visible);
    // The launch screen moves every frame until it has faded out.
    let splash_moving = ui::splash::step(s);
    if splash_moving {
        if !visible {
            tx.wake_after(Duration::from_millis(50));
        }
        changed = true;
    } else if ui::splash::waiting(s) {
        // Started in the tray: the launch screen begins once the window is shown.
        tx.wake_after(Duration::from_millis(250));
    } else if ui::splash::awaiting_gpu(s) {
        // Safety wake if GPU init takes unusually long; far enough out to keep CPU handover quiet.
        tx.wake_after(Duration::from_secs(2));
    }

    // A hidden window (closed to the tray) is only woken when asked: keep the player's clock, the
    // media card and the end-of-track logic ticking while something plays.
    if s.desktop_pending() {
        tx.wake_after(Duration::from_secs_f64((desktop::glue::DESKTOP_AFTER_S - s.now).max(0.0) + 0.02));
    }
    if s.player.playing || s.player.loading {
        tx.wake_after(Duration::from_millis(250));
        // If Windows left the window marked covered after minimizing and restoring without mouse
        // or keyboard input, nudge the host once per second until frames resume.
        if visible
            && !s.headless
            && s.now - fx::last_draw_clock() > 1.0
            && s.now - s.restore_kick_at > 1.0
            && let Some(app) = &s.app
        {
            s.restore_kick_at = s.now;
            fx::disarm_tick();
            app.wake();
        }
    }
    let dt = (s.now - s.prev_now).clamp(0.0, 0.05) as f32;
    // Passive shader effects, butterflies, and word-by-word lyrics share one 30 Hz cadence.
    const FX_STEP: f64 = 1.0 / 30.0;
    let resizing = s.is_resizing();
    // Rebuild the glass once the last size holds, even on a paused or otherwise idle page.
    if resizing {
        tx.wake_after(Duration::from_secs_f64(s.resize_settles_in()));
    } else if s.resized_at.replace(f64::NEG_INFINITY).is_finite() {
        changed = true;
    }
    // A page come back to goes to where it was left, once it is tall enough for that (a list
    // that loads again grows in a moment); after a while it settles for as far as it goes.
    if let Some((key, y, until)) = s.scroll_restore.as_ref() {
        let (y, late) = (*y, s.now > *until);
        let mut clear = late;
        match app::page_scroll(key) {
            Some([offset, _]) if (offset - y).abs() < 1.0 => clear = true,
            Some([_, max]) if max >= y - 0.5 || (late && max > 0.0) => tx.scroll_to_instant(key.as_str(), 0.0, y.min(max)),
            _ => {}
        }
        if clear {
            s.scroll_restore = None;
        } else if !visible {
            tx.wake_after(Duration::from_millis(50));
        }
    }
    // Menus, the player page, and the details sheet step on springs at full display refresh rate.
    let menu_moving = s.menu_moving();
    if menu_moving {
        if !visible {
            tx.wake_after(Duration::from_millis(50));
        }
        changed = true;
    }
    // The sung line comes forward and the one before steps back, frame by frame.
    let lines_moving = !resizing && s.np_open && s.np_panel() == NpTab::Lyrics && ui::np::step_lines(s, dt);
    if lines_moving {
        if !visible {
            tx.wake_after(Duration::from_millis(50));
        }
        changed = true;
    }
    // Keep the compositor in continuous VSync-paced rendering while any UI spring moves,
    // instead of sleeping on poll timers (which clamp to 16.7 ms and drop to 30 FPS on Windows).
    s.ui_animating = visible && (splash_moving || menu_moving || lines_moving || s.scroll_restore.is_some());

    let fx_on = visible && fx::wants_tick();
    let bugs_on = visible && s.bugs_alive();
    let wiping_now = s.np_open && s.np_panel() == NpTab::Lyrics && (ui::np::wiping(s) || ui::np::counting(s));
    if bugs_on && s.swarm.seed != s.home_seed {
        s.swarm = bugs::Swarm::new(s.home_seed);
        s.bugs_at = s.now;
        changed = true;
    }
    if fx_on || bugs_on || wiping_now {
        if fx_on {
            fx::arm_tick();
        }
        if resizing {
            tx.wake_after(Duration::from_millis(50));
        } else if s.now >= s.fx_next {
            let bug_dt = (s.now - s.bugs_at).clamp(0.0, 0.1) as f32;
            s.bugs_at = s.now;
            let mut bug_wait = None;
            if bugs_on {
                let scroll = app::page_scroll("home").map_or(0.0, |p| p[0]);
                let geom = s.bug_geom.borrow();
                bug_wait = s.swarm.step(s.now, bug_dt, &geom, scroll);
            }
            if wiping_now {
                changed = true;
            }
            let wait =
                if fx_on || wiping_now { Duration::from_secs_f64(FX_STEP) } else { bug_wait.map_or(Duration::from_secs_f64(FX_STEP), |d| d.max(Duration::from_secs_f64(FX_STEP))) };
            s.fx_next = if (fx_on || wiping_now) && s.fx_next > 0.0 && s.now - s.fx_next < FX_STEP { s.fx_next + FX_STEP } else { s.now + wait.as_secs_f64() };
            if !s.ui_animating {
                tx.wake_after(Duration::from_secs_f64((s.fx_next - s.now).max(0.004)));
                if fx_on && !changed {
                    s.fx_phase = !s.fx_phase;
                    s.fx_pulse.set([0.0, if s.fx_phase { 1.0 } else { 0.0 }]);
                }
            }
        } else if !s.ui_animating {
            tx.wake_after(Duration::from_secs_f64((s.fx_next - s.now).max(0.004)));
        }
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

/// Restores live playback updates immediately when the window regains focus (e.g. after
/// restoring from minimized without hovering the cursor or pressing a key).
fn on_focus(s: &mut State, focused: bool, _tx: &mut Tx) {
    if !focused {
        return;
    }
    fx::disarm_tick();
    s.fx_next = 0.0;
    s.sync_lives();
    if (s.player.playing || s.player.loading)
        && let Some(app) = s.app.clone()
    {
        net::spawn(async move {
            tokio::time::sleep(Duration::from_millis(120)).await;
            app.wake();
        });
    }
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
            s.close_volume();
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
