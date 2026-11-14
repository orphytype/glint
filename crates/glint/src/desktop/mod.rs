//! What Glint shows outside its window: the media keys and the "now playing" card (the System
//! Media Transport Controls, SMTC, on Windows; MPRIS over D-Bus on Linux) and a tray icon.
//!
//! Neither lives on the UI thread. The tray owns a thread (RUNE's `Tray`), and the media session
//! runs on one of ours, because its calls can block and its button events arrive elsewhere. So nothing
//! here touches the player: a press becomes a [`Command`] that the app queues onto the UI thread,
//! and the UI thread pushes what the OS should show with [`Desktop::sync`], which sends only what
//! changed since the last push.

use std::sync::{Arc, OnceLock, mpsc};
use std::time::{Duration, Instant};

use rune::{Tray, TrayEvent, TrayIcon, TrayItem, TraySpec};
use ym_api::Track;

use crate::i18n::{Lang, S};

/// Diagnostics for the desktop shell on stderr, only when `GLINT_DEBUG` is set.
macro_rules! dlog {
    ($($t:tt)*) => {
        if $crate::desktop::debug_on() {
            eprintln!("[desktop] {}", format_args!($($t)*));
        }
    };
}

mod glue;
mod icon;
pub mod place;
#[cfg(target_os = "linux")]
mod mpris;
#[cfg(windows)]
mod smtc;

/// The platform's media session: one type with the same few calls on each OS.
#[cfg(target_os = "linux")]
use mpris::Mpris as Session;
#[cfg(windows)]
use smtc::Smtc as Session;

pub use icon::cover_png;
#[cfg(windows)]
pub use smtc::set_app_id;

/// Gives RUNE's windows the orb as their icon: the title bar, the taskbar button and Alt+Tab.
/// Call it before the app runs, since each window takes the icon as it opens. (The exe's own icon,
/// for Explorer and the Start menu, is embedded by `build.rs`.)
pub fn set_window_icon() {
    let Some(png) = icon::window_icon() else { return };
    let started = Instant::now();
    match rune::set_app_icon(png) {
        Ok(()) => dlog!("window icon set in {:?}", started.elapsed()),
        Err(e) => dlog!("window icon: {e}"),
    }
}

/// A release build is a GUI program and has no console of its own, so what it prints (`--help`,
/// the headless tools, `GLINT_DEBUG`) would go nowhere. When it was started from a terminal and
/// its output is not already redirected, this joins that terminal. Started from Explorer or the
/// Start menu there is no terminal to join, and nothing opens, which is the point.
#[cfg(windows)]
pub fn attach_parent_console() {
    use std::os::windows::io::IntoRawHandle;

    use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle};

    let missing = |h: HANDLE| h.is_null() || h == INVALID_HANDLE_VALUE;
    // SAFETY: plain Win32 calls with valid arguments; the handle given to `SetStdHandle` is
    // leaked on purpose, as it has to stay open for the life of the process.
    unsafe {
        let (out, err) = (GetStdHandle(STD_OUTPUT_HANDLE), GetStdHandle(STD_ERROR_HANDLE));
        if !missing(out) && !missing(err) {
            return; // a pipe or a file already, as when a script runs it
        }
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            return; // no terminal started it
        }
        for (slot, now) in [(STD_OUTPUT_HANDLE, out), (STD_ERROR_HANDLE, err)] {
            if missing(now)
                && let Ok(conout) = std::fs::OpenOptions::new().write(true).open("CONOUT$")
            {
                SetStdHandle(slot, conout.into_raw_handle() as HANDLE);
            }
        }
    }
}

pub fn debug_on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("GLINT_DEBUG").is_some())
}

/// Something the user asked of the player from outside the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Play,
    Pause,
    Toggle,
    Next,
    Prev,
    Stop,
    /// Jump to a position (milliseconds), from the scrubber of the media flyout.
    Seek(u64),
    /// Bring the window back.
    Show,
    Quit,
}

/// What the player is doing, as far as the media card is concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Status {
    /// Nothing is loaded: the card goes away and the media keys go to another app.
    #[default]
    Closed,
    /// A track is being fetched or decoded.
    Changing,
    Paused,
    Playing,
}

/// The player, reduced to what the OS surfaces show.
#[derive(Clone, Copy)]
pub struct Playback<'a> {
    pub track: Option<&'a Track>,
    pub playing: bool,
    pub loading: bool,
    pub pos_ms: u64,
    pub dur_ms: u64,
    pub has_next: bool,
}

impl Playback<'_> {
    pub fn status(&self) -> Status {
        match (self.track, self.loading, self.playing) {
            (None, ..) => Status::Closed,
            (_, true, _) => Status::Changing,
            (_, _, true) => Status::Playing,
            _ => Status::Paused,
        }
    }
}

/// The position the media card was last told, and when: Windows extrapolates it while playing, so
/// it only needs telling again when something other than time moved it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Timeline {
    pos_ms: u64,
    dur_ms: u64,
    at: Instant,
}

/// What the OS surfaces were last told.
#[derive(Clone, Debug)]
struct Sent {
    track: Option<String>,
    status: Status,
    can_next: bool,
    timeline: Option<Timeline>,
    lang: Lang,
    playing: bool,
    tip: String,
}

impl Sent {
    fn new(lang: Lang) -> Sent {
        Sent { track: None, status: Status::Closed, can_next: false, timeline: None, lang, playing: false, tip: tooltip(None) }
    }

    /// Notes that `plan` was pushed for `now`.
    fn record(&mut self, plan: Plan, now: &Playback<'_>, lang: Lang, at: Instant) {
        if plan.meta {
            self.track = now.track.map(|t| t.id.clone());
            self.tip = tooltip(now.track);
            self.timeline = None;
        }
        self.status = now.status();
        self.can_next = now.has_next;
        if plan.timeline {
            self.timeline = Some(Timeline { pos_ms: now.pos_ms, dur_ms: now.dur_ms, at });
        }
        if plan.tray {
            self.lang = lang;
            self.playing = now.playing;
        }
    }
}

/// What has to be pushed to bring the OS surfaces up to date.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Plan {
    /// A different track (or none): title, artist, album, and a new cover to fetch.
    meta: bool,
    status: bool,
    buttons: bool,
    timeline: bool,
    /// The tray's tooltip and menu.
    tray: bool,
}

/// How far the position may stray from "playing at normal speed" before it counts as a seek.
const SEEK_SLACK_MS: u64 = 1500;

fn plan(sent: &Sent, now: &Playback<'_>, lang: Lang, at: Instant) -> Plan {
    let meta = sent.track.as_deref() != now.track.map(|t| t.id.as_str());
    let status = now.status();
    let timeline = now.track.is_some()
        && now.dur_ms > 0
        && match sent.timeline {
            None => true,
            Some(tl) => meta || status != sent.status || tl.dur_ms != now.dur_ms || drifted(tl, now.pos_ms, status == Status::Playing, at),
        };
    Plan {
        meta,
        status: status != sent.status,
        buttons: now.track.is_some() && (meta || sent.can_next != now.has_next),
        timeline,
        tray: meta || lang != sent.lang || now.playing != sent.playing,
    }
}

/// The position is somewhere other than where playing since `tl` would have put it.
fn drifted(tl: Timeline, pos_ms: u64, playing: bool, at: Instant) -> bool {
    let elapsed = if playing { at.saturating_duration_since(tl.at).as_millis() as u64 } else { 0 };
    pos_ms.abs_diff(tl.pos_ms + elapsed) > SEEK_SLACK_MS
}

// ---- the tray ---------------------------------------------------------------------------------

const ID_SHOW: &str = "show";
const ID_TOGGLE: &str = "toggle";
const ID_PREV: &str = "prev";
const ID_NEXT: &str = "next";
const ID_QUIT: &str = "quit";

fn menu(lang: Lang, has_track: bool, playing: bool) -> Vec<TrayItem> {
    let toggle = if playing { S::Pause } else { S::Play };
    vec![
        TrayItem::action(ID_SHOW, S::TrayShow.get(lang)),
        TrayItem::Separator,
        TrayItem::action(ID_TOGGLE, toggle.get(lang)).enabled(has_track),
        TrayItem::action(ID_PREV, S::Previous.get(lang)).enabled(has_track),
        TrayItem::action(ID_NEXT, S::Next.get(lang)).enabled(has_track),
        TrayItem::Separator,
        TrayItem::action(ID_QUIT, S::TrayQuit.get(lang)),
    ]
}

fn command_for_menu(id: &str) -> Option<Command> {
    Some(match id {
        ID_SHOW => Command::Show,
        ID_TOGGLE => Command::Toggle,
        ID_PREV => Command::Prev,
        ID_NEXT => Command::Next,
        ID_QUIT => Command::Quit,
        _ => return None,
    })
}

/// "Title — Artist", short enough for the tray tooltip (Windows keeps 127 UTF-16 units).
fn tooltip(track: Option<&Track>) -> String {
    let Some(t) = track else { return "Glint".into() };
    let artists = t.artists_line();
    let text = if artists.is_empty() { t.full_title() } else { format!("{} — {artists}", t.full_title()) };
    clip_utf16(&text, 120)
}

/// `s` cut to at most `max` UTF-16 units, with an ellipsis when something was cut. Cuts between
/// characters, never inside a surrogate pair.
fn clip_utf16(s: &str, max: usize) -> String {
    if s.encode_utf16().count() <= max {
        return s.to_owned();
    }
    let mut out = String::new();
    let mut units = 0;
    for ch in s.chars() {
        if units + ch.len_utf16() > max - 1 {
            break;
        }
        out.push(ch);
        units += ch.len_utf16();
    }
    format!("{}…", out.trim_end())
}

// ---- the whole -------------------------------------------------------------------------------

/// The tray icon and the media session.
pub struct Desktop {
    on_command: Arc<dyn Fn(Command) + Send + Sync>,
    tray: Option<Tray>,
    #[cfg(any(windows, target_os = "linux"))]
    session: Option<Session>,
    sent: Sent,
}

impl Desktop {
    /// Starts the media session and, when `want_tray`, adds the tray icon. `on_command` runs on
    /// other threads: keep it short and hand the work to the UI thread.
    pub fn start(lang: Lang, want_tray: bool, on_command: impl Fn(Command) + Send + Sync + 'static) -> Desktop {
        let on_command: Arc<dyn Fn(Command) + Send + Sync> = Arc::new(on_command);
        let mut d = Desktop {
            #[cfg(any(windows, target_os = "linux"))]
            session: Session::start(on_command.clone()),
            on_command,
            tray: None,
            sent: Sent::new(lang),
        };
        d.set_tray(want_tray);
        d
    }

    /// Shows or removes the tray icon.
    pub fn set_tray(&mut self, on: bool) {
        match (on, self.tray.is_some()) {
            (true, false) => self.tray = self.make_tray(),
            (false, true) => {
                self.tray = None;
                dlog!("tray icon removed");
            }
            _ => {}
        }
    }

    /// Whether the tray icon is really there (adding it can fail, and then closing the window
    /// must not hide the app somewhere unreachable).
    pub fn tray_alive(&self) -> bool {
        self.tray.is_some()
    }

    fn make_tray(&self) -> Option<Tray> {
        let cb = self.on_command.clone();
        let icon = TrayIcon::rgba(icon::TRAY_PX, icon::TRAY_PX, icon::orb(icon::TRAY_PX));
        let spec = TraySpec::new("glint", "Glint", icon).tooltip(self.sent.tip.clone()).menu(menu(self.sent.lang, self.sent.track.is_some(), self.sent.playing));
        let made = Tray::new(spec, move |event| {
            let command = match event {
                TrayEvent::Activate { .. } => Some(Command::Show),
                TrayEvent::Menu(id) => command_for_menu(&id),
                _ => None,
            };
            if let Some(c) = command {
                cb(c);
            }
        });
        match made {
            Ok(tray) => {
                dlog!("tray icon added");
                Some(tray)
            }
            Err(e) => {
                dlog!("tray icon failed: {e}");
                None
            }
        }
    }

    /// Brings the tray and the media card up to date with `now`. Returns true when `now` is a new
    /// track, whose cover the media card is still missing.
    pub fn sync(&mut self, now: &Playback<'_>, lang: Lang, at: Instant) -> bool {
        let plan = plan(&self.sent, now, lang, at);
        if plan == Plan::default() {
            return false;
        }
        #[cfg(any(windows, target_os = "linux"))]
        if let Some(session) = &self.session {
            if plan.meta {
                match now.track {
                    Some(t) => session.track(t),
                    None => session.clear(),
                }
            }
            if plan.status {
                session.status(now.status());
            }
            if plan.buttons {
                session.buttons(now.has_next);
            }
            if plan.timeline {
                session.timeline(now.pos_ms, now.dur_ms);
            }
        }
        self.sent.record(plan, now, lang, at);
        if plan.tray
            && let Some(tray) = &self.tray
        {
            tray.set_menu(menu(lang, now.track.is_some(), now.playing));
            tray.set_tooltip(self.sent.tip.clone());
        }
        // Only the Windows card takes a picture; MPRIS gets the cover by URL with the track.
        cfg!(windows) && plan.meta && now.track.is_some()
    }

    /// The cover of `track_id` as a PNG for the media card. It is dropped when that track is no
    /// longer the one playing by the time it arrives.
    pub fn set_cover(&self, track_id: &str, png: Vec<u8>) {
        #[cfg(windows)]
        if let Some(session) = &self.session {
            session.cover(track_id, png);
        }
        #[cfg(not(windows))]
        let _ = (track_id, png);
    }

    /// Takes the tray icon down and ends the media session, waiting a moment for both so no ghost
    /// icon is left behind when the process exits right after.
    pub fn shutdown(self) {
        let (done, wait) = mpsc::channel();
        let spawned = std::thread::Builder::new().name("desktop-shutdown".into()).spawn(move || {
            drop(self);
            let _ = done.send(());
        });
        if spawned.is_ok() {
            match wait.recv_timeout(Duration::from_millis(700)) {
                Ok(()) => dlog!("tray icon and media card taken down"),
                Err(_) => dlog!("tray icon and media card still going down after 700 ms; leaving them to the exit"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracks() -> Vec<Track> {
        crate::demo::Catalog::new().tracks.into_iter().take(3).collect()
    }

    fn view(track: Option<&Track>, playing: bool, pos_ms: u64, dur_ms: u64) -> Playback<'_> {
        Playback { track, playing, loading: false, pos_ms, dur_ms, has_next: true }
    }

    fn sent() -> Sent {
        Sent::new(Lang::En)
    }

    /// What `Desktop::sync` does, minus the real tray and session.
    fn record(s: &mut Sent, now: &Playback<'_>, lang: Lang, at: Instant) -> Plan {
        let p = plan(s, now, lang, at);
        s.record(p, now, lang, at);
        p
    }

    #[test]
    fn an_idle_player_pushes_nothing() {
        let t0 = Instant::now();
        assert_eq!(plan(&sent(), &view(None, false, 0, 0), Lang::En, t0), Plan::default());
    }

    #[test]
    fn a_new_track_pushes_everything_once_its_length_is_known() {
        let ts = tracks();
        let (mut s, t0) = (sent(), Instant::now());
        let loading = Playback { loading: true, ..view(Some(&ts[0]), true, 0, 0) };
        let p = record(&mut s, &loading, Lang::En, t0);
        assert_eq!(p, Plan { meta: true, status: true, buttons: true, timeline: false, tray: true }, "no timeline without a length");
        assert_eq!(loading.status(), Status::Changing);
        let p = record(&mut s, &view(Some(&ts[0]), true, 0, 200_000), Lang::En, t0);
        assert_eq!(p, Plan { meta: false, status: true, buttons: false, timeline: true, tray: false }, "playing, and the length arrived");
        assert_eq!(plan(&s, &view(Some(&ts[0]), true, 0, 200_000), Lang::En, t0), Plan::default(), "and then it is quiet");
    }

    #[test]
    fn time_passing_is_not_a_reason_to_push() {
        let ts = tracks();
        let (mut s, t0) = (sent(), Instant::now());
        record(&mut s, &view(Some(&ts[0]), true, 1000, 200_000), Lang::En, t0);
        for secs in 1..=60u64 {
            let at = t0 + Duration::from_secs(secs);
            let now = view(Some(&ts[0]), true, 1000 + secs * 1000 + 40, 200_000);
            assert_eq!(plan(&s, &now, Lang::En, at), Plan::default(), "after {secs} s of plain playing");
        }
    }

    #[test]
    fn a_seek_pushes_the_new_position() {
        let ts = tracks();
        let (mut s, t0) = (sent(), Instant::now());
        record(&mut s, &view(Some(&ts[0]), true, 10_000, 200_000), Lang::En, t0);
        let at = t0 + Duration::from_secs(2);
        let p = plan(&s, &view(Some(&ts[0]), true, 90_000, 200_000), Lang::En, at);
        assert!(p.timeline && !p.meta && !p.status, "{p:?}");
        let back = plan(&s, &view(Some(&ts[0]), true, 0, 200_000), Lang::En, at);
        assert!(back.timeline, "a restart counts as a seek too");
    }

    #[test]
    fn a_pause_does_not_extrapolate() {
        let ts = tracks();
        let (mut s, t0) = (sent(), Instant::now());
        record(&mut s, &view(Some(&ts[0]), false, 30_000, 200_000), Lang::En, t0);
        let later = t0 + Duration::from_secs(60);
        assert_eq!(plan(&s, &view(Some(&ts[0]), false, 30_000, 200_000), Lang::En, later), Plan::default(), "paused for a minute, same position");
        let p = plan(&s, &view(Some(&ts[0]), true, 30_000, 200_000), Lang::En, later);
        assert!(p.status && p.timeline && p.tray, "resuming tells Windows to start counting: {p:?}");
    }

    #[test]
    fn changing_track_clears_the_old_timeline() {
        let ts = tracks();
        let (mut s, t0) = (sent(), Instant::now());
        record(&mut s, &view(Some(&ts[0]), true, 50_000, 200_000), Lang::En, t0);
        let p = plan(&s, &view(Some(&ts[1]), true, 0, 180_000), Lang::En, t0);
        assert!(p.meta && p.buttons && p.timeline && p.tray, "{p:?}");
        let none = plan(&s, &view(None, false, 0, 0), Lang::En, t0);
        assert!(none.meta && none.status && !none.timeline && !none.buttons, "stopping removes the card: {none:?}");
    }

    #[test]
    fn the_next_button_follows_the_queue() {
        let ts = tracks();
        let (mut s, t0) = (sent(), Instant::now());
        record(&mut s, &view(Some(&ts[0]), true, 0, 200_000), Lang::En, t0);
        let last = Playback { has_next: false, ..view(Some(&ts[0]), true, 0, 200_000) };
        assert_eq!(plan(&s, &last, Lang::En, t0), Plan { buttons: true, ..Plan::default() });
    }

    #[test]
    fn the_tray_relabels_for_a_language_and_for_play_state() {
        let ts = tracks();
        let (mut s, t0) = (sent(), Instant::now());
        record(&mut s, &view(Some(&ts[0]), true, 0, 200_000), Lang::En, t0);
        assert!(plan(&s, &view(Some(&ts[0]), true, 0, 200_000), Lang::Ru, t0).tray);
        assert!(plan(&s, &view(Some(&ts[0]), false, 0, 200_000), Lang::En, t0).tray);
    }

    #[test]
    fn the_menu_follows_the_player() {
        let labels = |m: &[TrayItem]| -> Vec<(String, bool)> {
            m.iter()
                .filter_map(|i| match i {
                    TrayItem::Action { label, enabled, .. } => Some((label.clone(), *enabled)),
                    _ => None,
                })
                .collect()
        };
        let idle = labels(&menu(Lang::En, false, false));
        assert_eq!(idle.len(), 5);
        assert_eq!(idle[0], ("Open Glint".into(), true));
        assert_eq!(idle[1], ("Play".into(), false), "nothing to play");
        assert!(idle[4].1, "Quit always works");
        let playing = labels(&menu(Lang::En, true, true));
        assert_eq!(playing[1], ("Pause".into(), true));
        assert_eq!(labels(&menu(Lang::Ru, true, false))[1].0, "Слушать");
        for lang in [Lang::En, Lang::Ru] {
            assert!(labels(&menu(lang, true, true)).iter().all(|(l, _)| !l.is_empty()));
        }
    }

    #[test]
    fn every_menu_item_maps_to_a_command() {
        let items = menu(Lang::En, true, true);
        let ids: Vec<&str> = items
            .iter()
            .filter_map(|i| match i {
                TrayItem::Action { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            ids.iter().map(|id| command_for_menu(id)).collect::<Vec<_>>(),
            [Some(Command::Show), Some(Command::Toggle), Some(Command::Prev), Some(Command::Next), Some(Command::Quit)]
        );
        assert_eq!(command_for_menu("nope"), None);
    }

    #[test]
    fn the_tooltip_names_the_track_and_fits() {
        let ts = tracks();
        assert_eq!(tooltip(None), "Glint");
        let tip = tooltip(Some(&ts[0]));
        assert!(tip.contains(&ts[0].title) && tip.contains(" — "), "{tip}");

        let mut long = ts[0].clone();
        long.title = "Очень длинное название трека, которое не помещается ни в какую подсказку ".repeat(4);
        let tip = tooltip(Some(&long));
        assert!(tip.encode_utf16().count() <= 120 && tip.ends_with('…'), "{} units: {tip}", tip.encode_utf16().count());
    }

    #[test]
    fn clipping_never_splits_a_surrogate_pair() {
        let s = "🎵".repeat(100);
        for max in [5, 6, 7, 40, 119, 120] {
            let out = clip_utf16(&s, max);
            assert!(out.encode_utf16().count() <= max, "max {max}");
            assert!(out.ends_with('…'));
            assert!(out.chars().filter(|c| *c == '🎵').count() * 2 < max);
        }
        assert_eq!(clip_utf16("short", 120), "short");
        assert_eq!(clip_utf16("abcdef", 6), "abcdef", "exactly fitting stays whole");
        assert_eq!(clip_utf16("abcdefg", 6), "abcde…");
    }
}
