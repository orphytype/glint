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

pub mod glue;
mod icon;
#[cfg(target_os = "linux")]
mod mpris;
pub mod place;
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

/// Gives RUNE's windows the app icon: the title bar, the taskbar button and Alt+Tab.
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
    /// Like the playing track, or take the like back (the tray menu).
    Like,
    /// Shuffle on or off (the tray menu).
    Shuffle,
    /// The next repeat mode (the tray menu).
    Repeat,
    /// Sound off, or back on (the tray menu).
    Mute,
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
    pub has_prev: bool,
    /// The playing track is in the user's likes.
    pub liked: bool,
    pub shuffle: bool,
    pub repeat: crate::store::Repeat,
    pub muted: bool,
}

impl Playback<'_> {
    /// What the tray menu shows of this.
    fn menu(&self) -> MenuState {
        MenuState {
            has_track: self.track.is_some(),
            playing: self.playing,
            has_next: self.has_next,
            has_prev: self.has_prev,
            liked: self.liked,
            shuffle: self.shuffle,
            repeat: self.repeat,
            muted: self.muted,
        }
    }

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
    menu: MenuState,
    tip: String,
    /// The playing track as the tray menu's first line.
    head: String,
}

impl Sent {
    fn new(lang: Lang) -> Sent {
        Sent { track: None, status: Status::Closed, can_next: false, timeline: None, lang, menu: MenuState::default(), tip: tooltip(None), head: heading(None) }
    }

    /// Notes that `plan` was pushed for `now`.
    fn record(&mut self, plan: Plan, now: &Playback<'_>, lang: Lang, at: Instant) {
        if plan.meta {
            self.track = now.track.map(|t| t.id.clone());
            self.tip = tooltip(now.track);
            self.head = heading(now.track);
            self.timeline = None;
        }
        self.status = now.status();
        self.can_next = now.has_next;
        if plan.timeline {
            self.timeline = Some(Timeline { pos_ms: now.pos_ms, dur_ms: now.dur_ms, at });
        }
        if plan.tray {
            self.lang = lang;
            self.menu = now.menu();
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
        tray: meta || lang != sent.lang || now.menu() != sent.menu,
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
const ID_HEAD: &str = "head";
const ID_LIKE: &str = "like";
const ID_SHUFFLE: &str = "shuffle";
const ID_REPEAT: &str = "repeat";
const ID_MUTE: &str = "mute";

/// What the tray menu depends on besides the language and the track's name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
struct MenuState {
    has_track: bool,
    playing: bool,
    has_next: bool,
    has_prev: bool,
    liked: bool,
    shuffle: bool,
    repeat: crate::store::Repeat,
    muted: bool,
}

/// The tray menu: what plays (a line that can't be picked), the playback, the track's like and
/// the modes, then the window and the way out. The modes say what they are set to now; picking
/// one moves it on.
fn menu(lang: Lang, head: &str, m: MenuState) -> Vec<TrayItem> {
    use crate::store::Repeat;
    let toggle = if m.playing { S::TrayPause } else { S::TrayPlay };
    let like = if m.liked { S::TrayUnlike } else { S::TrayLike };
    let shuffle = if m.shuffle { S::TrayShuffleOn } else { S::TrayShuffleOff };
    let repeat = match m.repeat {
        Repeat::Off => S::TrayRepeatOff,
        Repeat::All => S::TrayRepeatAll,
        Repeat::One => S::TrayRepeatOne,
    };
    let mute = if m.muted { S::TrayUnmute } else { S::TrayMute };
    let head = if m.has_track { menu_label(head) } else { S::TrayNothing.get(lang).to_owned() };
    vec![
        TrayItem::action(ID_HEAD, head).enabled(false),
        TrayItem::Separator,
        TrayItem::action(ID_TOGGLE, toggle.get(lang)).enabled(m.has_track),
        TrayItem::action(ID_PREV, S::TrayPrev.get(lang)).enabled(m.has_track && m.has_prev),
        TrayItem::action(ID_NEXT, S::TrayNext.get(lang)).enabled(m.has_track && m.has_next),
        TrayItem::Separator,
        TrayItem::action(ID_LIKE, like.get(lang)).enabled(m.has_track),
        TrayItem::action(ID_SHUFFLE, shuffle.get(lang)),
        TrayItem::action(ID_REPEAT, repeat.get(lang)),
        TrayItem::action(ID_MUTE, mute.get(lang)),
        TrayItem::Separator,
        TrayItem::action(ID_SHOW, S::TrayShow.get(lang)),
        TrayItem::action(ID_QUIT, S::TrayQuit.get(lang)),
    ]
}

/// A track's name as a menu line: the character that marks a menu's access key (`&` on Windows,
/// `_` in a D-Bus menu) doubled, so "Salt & Pepper" keeps its ampersand.
fn menu_label(text: &str) -> String {
    let mark = if cfg!(windows) { "&" } else { "_" };
    text.replace(mark, &mark.repeat(2))
}

/// The longest first line of the tray menu, in UTF-16 units. The menu is as wide as its longest
/// line, so a long name would stretch it far past its other lines.
const HEADING_MAX: usize = 34;

/// "Title — Artist" for the tray menu's first line, cut to [`HEADING_MAX`].
fn heading(track: Option<&Track>) -> String {
    let Some(t) = track else { return String::new() };
    let artists = t.artists_line();
    let text = if artists.is_empty() { t.full_title() } else { format!("{} — {artists}", t.full_title()) };
    clip_utf16(&text, HEADING_MAX)
}

fn command_for_menu(id: &str) -> Option<Command> {
    Some(match id {
        ID_SHOW => Command::Show,
        ID_TOGGLE => Command::Toggle,
        ID_PREV => Command::Prev,
        ID_NEXT => Command::Next,
        ID_LIKE => Command::Like,
        ID_SHUFFLE => Command::Shuffle,
        ID_REPEAT => Command::Repeat,
        ID_MUTE => Command::Mute,
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
        let icon = TrayIcon::rgba(icon::TRAY_PX, icon::TRAY_PX, icon::app_icon(icon::TRAY_PX));
        let spec = TraySpec::new("glint", "Glint", icon).tooltip(self.sent.tip.clone()).menu(menu(self.sent.lang, &self.sent.head, self.sent.menu));
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
            tray.set_menu(menu(lang, &self.sent.head, self.sent.menu));
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
