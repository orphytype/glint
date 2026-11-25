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
mod find;
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
    include_bytes!("../assets/fonts/MusticaPro-SemiBold.otf"),
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
    // Where the window was last closed, if that is still on a display.
    let store = if headless { Store::none() } else { Store::open() };
    let settings = store.load();
    let cpu_mode = !headless && (settings.cpu_mode || std::env::var_os("GLINT_CPU").is_some_and(|v| v != "0" && !v.is_empty()));
    fx::set_cpu_mode(cpu_mode);
    fx::set_effects_off(cpu_mode || !settings.shaders);
    rune::set_gpu_memory_policy(rune::GpuMemoryPolicy::Manual { block_size: (4 << 20)..(4 << 20) });
    if cpu_mode {
        #[cfg(any(windows, target_os = "linux"))]
        if std::env::var_os("RUNE_CPU_FIRST").is_none() {
            // SAFETY: called once on the main thread during startup before any worker threads spawn.
            unsafe { std::env::set_var("RUNE_CPU_FIRST", "only") };
        }
    } else {
        rune::prewarm_gpu();
    }
    if !headless {
        net::sweep_temp_streams();
        diag_log::init(store.logs_dir(), settings.logging);
    }
    let spec = rune::AppSpec {
        min_size: Some((840.0, 560.0)),
        #[cfg(feature = "tooling")]
        scenes: scenes::SCENES,
        fonts: APP_FONTS,
        families: ("Mustica Pro", "IBM Plex Mono"),
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
    let mini_size = settings.mini_style.size();
    let mini_spec = rune::AppSpec {
        min_size: Some((0.0, 0.0)),
        fonts: APP_FONTS,
        families: ("Mustica Pro", "IBM Plex Mono"),
        on_key: Some(ui::mini::on_key),
        ..rune::AppSpec::new("Glint Mini", mini_size, ui::mini::MiniState::default, ui::mini::view)
    };
    let mini_sub = rune::App::new(mini_spec)
        .windowless()
        .release_gpu_when_closed()
        .srgb_blend(true);
    let (mini_handle, mini_win) = if !headless {
        let mini_opts = rune::WindowOptions {
            // Transparent windows bypass RUNE's CPU-only renderer and spin up a full GPU device,
            // so keep the mini player opaque in CPU mode to stay 100% CPU-rendered and low-RAM.
            transparent: !cpu_mode,
            borderless: true,
            always_on_top: settings.mini_always_on_top,
            visible: false,
            resizable: false,
            min_size: Some((0.0, 0.0)),
            position: settings.mini_pos,
            ..Default::default()
        };
        let (h, w) = app.window(rune::WindowSpec::from_app("mini", mini_sub).options(mini_opts));
        (Some(h), Some(w))
    } else {
        (None, None)
    };
    let app = app
        .titlebar(rune::Titlebar::new(ui::kit::CAPTION_H))
        .on_window(desktop::place::keep_window)
        .on_focus(on_focus)
        .on_rest(|s: &mut State, _tx: &mut Tx| {
            let mut keep = Vec::with_capacity(2 + s.pinned_items.len());
            if let Some(tpl) = s.current().and_then(|t| t.cover_template()) {
                if let Some(md) = State::cover_key(Some(tpl)) {
                    keep.push(md);
                }
                if !fx::cpu_mode()
                    && let Some(lg) = State::cover_key_px(Some(tpl), art::COVER_LG)
                {
                    keep.push(lg);
                }
            }
            for item in &s.pinned_items {
                if let Some(sm) = State::cover_key_px(item.cover.as_deref(), art::COVER_SM) {
                    keep.push(sm);
                }
            }
            let refs: Vec<&str> = keep.iter().map(String::as_str).collect();
            let _ = s.art.trim_offscreen(&refs);
            if s.page != Page::Home || s.np_open {
                ui::blobs::clear_cache();
            }
            if s.now - s.ws_trimmed_at >= 20.0 {
                diag_log::trim_working_set();
                s.ws_trimmed_at = s.now;
            }
            s.log_mem("rest");
            false
        });
    let app = if cpu_mode { app } else { app.compositor(fx::Gfx::new()) };
    let app = app
        .srgb_blend(true)
        // Closing the window only hides it; whether that ends the app is decided in `on_close`.
        .keep_running()
        // A hidden window holds no GPU memory: the tray and the mini player don't pay for it.
        .release_gpu_when_closed()
        .on_close(|s: &mut State, tx: &mut Tx| {
            if let Some(win) = &s.mini_win
                && let Some(p) = win.placement()
            {
                s.settings.mini_pos = Some(p.position);
            }
            s.save_settings_now();
            desktop::place::save(&s.store);
            if let Some(app) = &s.app {
                s.remax = desktop::place::hold(app);
            }
            fx::release_gpu();
            s.trim_hidden_memory();
            // With the mini player open the app goes on in it.
            if s.mini_open {
                return;
            }
            if !s.keeps_running_when_closed() {
                tx.quit();
            } else if s.settings.mini_on_close && !s.mini_open {
                s.set_mini_player(true);
            }
        })
        .on_quit(|s: &mut State, _tx: &mut Tx| {
            diag_log::event("app", "quit");
            if let Some(win) = &s.mini_win
                && let Some(p) = win.placement()
            {
                s.settings.mini_pos = Some(p.position);
            }
            s.save_settings_now();
            desktop::place::save(&s.store);
            s.shutdown_desktop();
            rune::release_instance();
        });
    let handle = app.handle();
    app.init(move || {
        let mut state = State::new(Some(handle), store, settings, headless);
        state.mini_app = mini_handle;
        state.mini_win = mini_win;
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
    if s.restart_pending && !s.headless {
        s.restart_pending = false;
        if let Some(win) = &s.mini_win
            && let Some(p) = win.placement()
        {
            s.settings.mini_pos = Some(p.position);
        }
        s.save_settings_now();
        desktop::place::save(&s.store);
        s.shutdown_desktop();
        rune::release_instance();
        if let Ok(exe) = std::env::current_exe() {
            let mut cmd = std::process::Command::new(exe);
            cmd.env_remove("RUNE_CPU_FIRST");
            let _ = cmd.spawn();
        }
        tx.quit();
        return false;
    }
    let was = s.track_mark();
    let mut changed = s.tick(t);
    s.animate_if_moved(tx, &was);
    // The queue's headings come back once the tracks have passed them.
    if let Some(until) = s.queue_passing {
        if s.now >= until {
            s.queue_passing = None;
            changed = true;
        } else {
            tx.wake_after(Duration::from_secs_f64(until - s.now));
        }
    }
    // A setting found from the Settings search stays lit for a moment.
    if let Some((_, until)) = &s.settings_found {
        if s.now >= *until {
            s.settings_found = None;
            changed = true;
        } else {
            tx.wake_after(Duration::from_secs_f64(until - s.now));
        }
    }
    startup_report(tx);
    // The compositor tiers came up: rebuild the view with glass or shape slots as each finishes.
    {
        use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
        static GPU: AtomicBool = AtomicBool::new(false);
        static TIERS: AtomicU8 = AtomicU8::new(0);
        if !GPU.load(Ordering::Relaxed) && fx::gpu_up() && !GPU.swap(true, Ordering::Relaxed) {
            diag_log::write_system(&s.settings, s.update_method, s.win_size.get());
            s.startup_trim_at = Some(s.now + 3.0);
            s.log_mem("gpu:ready");
            changed = true;
        }
        let tiers = u8::from(fx::glass_ready())
            | (u8::from(fx::wave_ready()) << 1)
            | (u8::from(fx::mood_ready()) << 2)
            | (u8::from(fx::ribbon_ready()) << 3)
            | (u8::from(fx::ready()) << 4)
            | (u8::from(fx::stage_ready()) << 5)
            | (u8::from(fx::bars_ready()) << 6);
        if TIERS.swap(tiers, Ordering::Relaxed) != tiers {
            changed = true;
        }
    }
    let visible = s.headless || s.app.as_ref().is_none_or(rune::AppHandle::is_visible);
    {
        use std::sync::atomic::{AtomicBool, Ordering};
        static WAS_VISIBLE: AtomicBool = AtomicBool::new(true);
        if WAS_VISIBLE.swap(visible, Ordering::Relaxed) && !visible {
            s.trim_hidden_memory();
        }
    }
    if !visible {
        fx::release_gpu();
    } else if s.remax
        && !s.headless
        && let Some(maximized) = desktop::place::maximized()
    {
        s.remax = false;
        if !maximized {
            tx.toggle_maximized();
        }
    }
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
        // Short safety wake while the CPU splash hands over to the GPU so readiness is never missed.
        tx.wake_after(Duration::from_millis(120));
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
    // Compositor shaders run at 24 Hz when only the My Wave ribbon is active, else 30 Hz.
    // Word-by-word lyrics rebuild on a lighter 18 Hz (GPU) / 12 Hz (CPU) cadence.
    let fx_step = fx::tick_step_s();
    let lyric_step = if fx::cpu_mode() { 1.0 / 12.0 } else { 1.0 / 18.0 };
    let resizing = s.is_resizing();
    // Rebuild the glass once the last size holds, even on a paused or otherwise idle page.
    if resizing {
        tx.wake_after(Duration::from_secs_f64(s.resize_settles_in()));
    } else if s.resized_at.replace(f64::NEG_INFINITY).is_finite() {
        changed = true;
    }
    // Restore a remembered scroll offset on back/forward, or snap back to the top when `y == 0.0`.
    if let Some((key, y, until)) = s.scroll_restore.as_ref() {
        let (y, late) = (*y, s.now > *until);
        let mut clear = late;
        match app::page_scroll(key) {
            Some([offset, _]) if (offset - y).abs() < 1.0 => clear = true,
            Some([offset, _]) if y <= 0.5 && offset >= 1.0 => tx.scroll_to_instant(key.as_str(), 0.0, 0.0),
            Some([_, max]) if max >= y - 0.5 || (late && max > 0.0) => tx.scroll_to_instant(key.as_str(), 0.0, y.min(max)),
            None if y <= 0.5 => tx.scroll_to_instant(key.as_str(), 0.0, 0.0),
            _ => {}
        }
        if clear {
            s.scroll_restore = None;
        } else if !visible {
            tx.wake_after(Duration::from_millis(50));
        }
    }
    // Menus, the open player, and the details sheet step on springs at full display refresh rate.
    let menu_moving = s.menu_moving();
    if menu_moving {
        if !visible {
            tx.wake_after(Duration::from_millis(50));
        }
        changed = true;
    }
    let _ = dt;
    s.ui_animating = visible && (splash_moving || menu_moving || s.scroll_restore.is_some());

    // Blink the focused text input's caret on a standard 530 ms cadence; resets solid on every edit
    // or caret movement, and schedules zero wakes when no text input is focused.
    const CARET_HALF_S: f64 = 0.53;
    if visible && !s.headless {
        if let Some(sig) = s.focused_edit_sig() {
            if sig != s.caret_sig {
                s.caret_sig = sig;
                s.caret_since = s.now;
                if !s.caret_visible {
                    s.caret_visible = true;
                    changed = true;
                }
                tx.wake_after(Duration::from_secs_f64(CARET_HALF_S));
            } else {
                let elapsed = (s.now - s.caret_since).max(0.0);
                let phase = ((elapsed / CARET_HALF_S).floor() as u64).is_multiple_of(2);
                if phase != s.caret_visible {
                    s.caret_visible = phase;
                    changed = true;
                }
                let rem = CARET_HALF_S - (elapsed % CARET_HALF_S);
                tx.wake_after(Duration::from_secs_f64(rem.max(0.02)));
            }
        } else if s.caret_sig != 0 || !s.caret_visible {
            s.caret_sig = 0;
            if !s.caret_visible {
                s.caret_visible = true;
                changed = true;
            }
        }
    }

    let fx_on = visible && fx::wants_tick();
    let bugs_on = visible && s.bugs_alive();
    let wiping_now = s.np_open && s.np_panel() == NpTab::Lyrics && (ui::np::wiping(s) || ui::np::counting(s));
    let want_bugs = usize::from(s.settings.butterfly_count.clamp(1, 10));
    if bugs_on && (s.swarm.seed != s.home_seed || s.swarm.bugs.len() != want_bugs) {
        s.swarm = bugs::Swarm::new(s.home_seed, want_bugs);
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
                use std::sync::atomic::{AtomicU32, Ordering};
                static LAST_LYRIC_BUCKET: AtomicU32 = AtomicU32::new(u32::MAX);
                let div = if fx::cpu_mode() { 80 } else { 55 };
                let bucket = s.lyric_ms() / div;
                if LAST_LYRIC_BUCKET.swap(bucket, Ordering::Relaxed) != bucket || ui::np::counting(s) {
                    changed = true;
                }
            }
            let step = if fx_on {
                fx_step
            } else if wiping_now {
                lyric_step
            } else {
                fx_step
            };
            let wait = if fx_on || wiping_now {
                Duration::from_secs_f64(step)
            } else {
                bug_wait.map_or(Duration::from_secs_f64(step), |d| d.max(Duration::from_secs_f64(step)))
            };
            s.fx_next = if (fx_on || wiping_now) && s.fx_next > 0.0 && s.now - s.fx_next < step { s.fx_next + step } else { s.now + wait.as_secs_f64() };
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
        let cur = s.player.index.unwrap_or(0);
        if s.queue_placed != Some(cur) {
            let y = ui::np::queue_offset(cur);
            if s.queue_placed.is_none() {
                tx.scroll_to_y("np-queue", y);
            } else {
                // On the rows' spring, so the headings hold still while the tracks pass them.
                if !s.settings.reduce_motion {
                    tx.animated();
                }
                tx.scroll_to_with("np-queue", 0.0, y, theme::SLIDE);
                s.queue_passing = Some(s.now + 0.26);
            }
            s.queue_placed = Some(cur);
            changed = true;
        }
    } else {
        s.queue_placed = None;
    }

    // Keep the lyric that is being sung in the middle of its panel, pausing auto-scroll for 4 s
    // whenever the user scrolls manually and smoothly returning focus once they stop.
    let showing = s.np_open && s.np_panel() == NpTab::Lyrics && s.current().is_some();
    if showing != s.lyrics_showing {
        s.lyrics_showing = showing;
        s.lyrics.user_scroll_at = None;
        if showing {
            s.scroll_lyrics = true;
        }
    }
    if showing && let Some(at) = s.lyrics.user_scroll_at {
        let elapsed = s.now - at;
        if elapsed >= 4.0 {
            s.lyrics.user_scroll_at = None;
            s.scroll_lyrics = true;
            changed = true;
        } else {
            tx.wake_after(Duration::from_secs_f64((4.0 - elapsed).max(0.05)));
        }
    }
    if showing && s.scroll_lyrics && s.lyrics.user_scroll_at.is_none() {
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
    let typing = matches!(tx.focused(), Some("search-input" | "token" | "lib-search" | "pl-search"));
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
