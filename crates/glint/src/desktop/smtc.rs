//! The System Media Transport Controls: the media keys, the headset buttons and the "now playing"
//! card of the volume flyout and the lock screen.
//!
//! Every WinRT call happens on one worker thread of ours, in a multithreaded apartment where
//! blocking on an async call is fine. The thread first waits for the app's window to exist (the
//! controls are bound to a window), then applies what the UI thread sends it, in order. The
//! controls' events arrive on the thread pool and go out as [`Command`]s.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Media::{
    MediaPlaybackStatus, MediaPlaybackType, PlaybackPositionChangeRequestedEventArgs, SystemMediaTransportControls, SystemMediaTransportControlsButton,
    SystemMediaTransportControlsButtonPressedEventArgs, SystemMediaTransportControlsTimelineProperties,
};
use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream, RandomAccessStreamReference};
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::System::WinRT::{ISystemMediaTransportControlsInterop, RO_INIT_MULTITHREADED, RoInitialize};
use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GWL_EXSTYLE, GetWindow, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, HSTRING, factory};
use ym_api::Track;

use super::{Command, Status};

/// How long the worker keeps looking for the window before it gives up.
const FIND_FOR: Duration = Duration::from_secs(30);
/// The app's window is at least this big (its minimum size is 960 × 640); helper windows are not.
const MIN_WINDOW: i64 = 200;
/// The Windows identity shared by the process, installed shortcuts and the media session.
const APP_USER_MODEL_ID: &str = "Orphytype.Glint";

enum Msg {
    Track { id: String, title: String, artist: String, album: String },
    Cover { id: String, png: Vec<u8> },
    Status(Status),
    Buttons { next: bool },
    Timeline { pos_ms: u64, dur_ms: u64 },
    Clear,
}

/// The handle the UI thread keeps: a queue into the worker.
pub struct Smtc {
    tx: Option<Sender<Msg>>,
    worker: Option<JoinHandle<()>>,
}

impl Smtc {
    pub fn start(on_command: Arc<dyn Fn(Command) + Send + Sync>) -> Option<Smtc> {
        let (tx, rx) = mpsc::channel();
        match std::thread::Builder::new().name("rune-smtc".into()).spawn(move || run(rx, on_command)) {
            Ok(worker) => Some(Smtc { tx: Some(tx), worker: Some(worker) }),
            Err(e) => {
                dlog!("media controls thread failed: {e}");
                None
            }
        }
    }

    fn send(&self, msg: Msg) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(msg);
        }
    }

    pub fn track(&self, t: &Track) {
        self.send(Msg::Track { id: t.id.clone(), title: t.full_title(), artist: t.artists_line(), album: t.album().map(|a| a.title.clone()).unwrap_or_default() });
    }

    pub fn cover(&self, id: &str, png: Vec<u8>) {
        self.send(Msg::Cover { id: id.to_owned(), png });
    }

    pub fn status(&self, s: Status) {
        self.send(Msg::Status(s));
    }

    pub fn buttons(&self, next: bool) {
        self.send(Msg::Buttons { next });
    }

    pub fn timeline(&self, pos_ms: u64, dur_ms: u64) {
        self.send(Msg::Timeline { pos_ms, dur_ms });
    }

    pub fn clear(&self) {
        self.send(Msg::Clear);
    }
}

impl Drop for Smtc {
    /// Closing the queue ends the worker, which takes the card down; wait for that.
    fn drop(&mut self) {
        self.tx = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run(rx: Receiver<Msg>, on_command: Arc<dyn Fn(Command) + Send + Sync>) {
    // SAFETY: Initialises WinRT for this thread, which nothing else has touched yet.
    if let Err(e) = unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
        dlog!("RoInitialize: {e}");
    }
    let mut waiting: Vec<Msg> = Vec::new();
    let give_up = Instant::now() + FIND_FOR;
    let mut next_look = Instant::now();
    let mut session = loop {
        let now = Instant::now();
        if now >= next_look {
            if let Some(hwnd) = find_window() {
                match Session::attach(hwnd, &on_command) {
                    Ok(s) => break s,
                    Err(e) => {
                        dlog!("attaching the media controls failed: {e}");
                        return;
                    }
                }
            }
            if now > give_up {
                dlog!("no window to attach the media controls to");
                return;
            }
            next_look = now + Duration::from_millis(200);
        }
        match rx.recv_timeout(next_look.saturating_duration_since(Instant::now())) {
            Ok(msg) => queue_up(&mut waiting, msg),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    };
    for msg in waiting {
        session.apply(msg);
    }
    while let Ok(msg) = rx.recv() {
        session.apply(msg);
    }
}

/// Keeps what a late-attaching session needs to catch up: the latest of each kind, and no cover
/// or timeline of a track that has since been replaced.
fn queue_up(waiting: &mut Vec<Msg>, msg: Msg) {
    let kind = std::mem::discriminant(&msg);
    waiting.retain(|m| std::mem::discriminant(m) != kind);
    if matches!(msg, Msg::Track { .. } | Msg::Clear) {
        waiting.retain(|m| !matches!(m, Msg::Cover { .. } | Msg::Timeline { .. } | Msg::Track { .. } | Msg::Clear));
    }
    waiting.push(msg);
}

/// Names this process for the shell, so the media card and the taskbar say "Glint" instead of
/// "glint.exe". Has to run before the first window exists.
pub fn set_app_id() {
    // SAFETY: A plain call with a static, NUL-terminated string.
    if let Err(e) = unsafe { SetCurrentProcessExplicitAppUserModelID(windows::core::w!("Orphytype.Glint")) } {
        dlog!("setting the app id failed: {e}");
    }
}

/// The window the controls attach to: the biggest visible, unowned, top-level window of this
/// process. (Not just any visible one: winit keeps a zero-sized visible window of its own.)
fn find_window() -> Option<HWND> {
    struct Search {
        pid: u32,
        best: Option<(i64, HWND)>,
    }
    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: `lparam` is the `Search` below, which outlives the enumeration. None of these
        // calls sends a message to another thread's window (so they cannot wait on the UI thread).
        unsafe {
            let search = &mut *(lparam.0 as *mut Search);
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let owned = GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.is_invalid());
            let tool = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0;
            let mut rect = RECT::default();
            if pid == search.pid && IsWindowVisible(hwnd).as_bool() && !owned && !tool && GetWindowRect(hwnd, &mut rect).is_ok() {
                let (w, h) = (i64::from(rect.right - rect.left), i64::from(rect.bottom - rect.top));
                if w >= MIN_WINDOW && h >= MIN_WINDOW && search.best.is_none_or(|(area, _)| w * h > area) {
                    search.best = Some((w * h, hwnd));
                }
            }
        }
        BOOL(1)
    }
    // SAFETY: A plain window enumeration; the callback only reads.
    let mut search = Search { pid: unsafe { GetCurrentProcessId() }, best: None };
    unsafe {
        let _ = EnumWindows(Some(visit), LPARAM(&mut search as *mut Search as isize));
    }
    search.best.map(|(_, hwnd)| hwnd)
}

/// The controls, attached to the window, and the event registrations to undo on the way out.
struct Session {
    controls: SystemMediaTransportControls,
    pressed: i64,
    seek: Option<i64>,
    /// The track the card shows, so a cover that arrives late for another one is ignored.
    current: Option<String>,
}

impl Session {
    fn attach(hwnd: HWND, on_command: &Arc<dyn Fn(Command) + Send + Sync>) -> windows::core::Result<Session> {
        let interop = factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>()?;
        // SAFETY: `hwnd` is a live top-level window of this process.
        let controls: SystemMediaTransportControls = unsafe { interop.GetForWindow(hwnd)? };
        // Off until there is something to play, so the keys keep working for other apps meanwhile.
        controls.SetIsEnabled(false)?;
        controls.SetIsPlayEnabled(true)?;
        controls.SetIsPauseEnabled(true)?;
        controls.SetIsStopEnabled(true)?;
        controls.SetIsNextEnabled(true)?;
        controls.SetIsPreviousEnabled(true)?;

        let cb = on_command.clone();
        let pressed = controls.ButtonPressed(&TypedEventHandler::<SystemMediaTransportControls, SystemMediaTransportControlsButtonPressedEventArgs>::new(move |_, args| {
            if let Some(args) = args.as_ref()
                && let Ok(button) = args.Button()
            {
                dlog!("media button {button:?}");
                if let Some(command) = command_for(button) {
                    cb(command);
                }
            }
            Ok(())
        }))?;
        let cb = on_command.clone();
        let seek = controls
            .PlaybackPositionChangeRequested(&TypedEventHandler::<SystemMediaTransportControls, PlaybackPositionChangeRequestedEventArgs>::new(move |_, args| {
                if let Some(args) = args.as_ref()
                    && let Ok(at) = args.RequestedPlaybackPosition()
                {
                    dlog!("seek requested to {} ms", at.Duration / 10_000);
                    cb(Command::Seek(millis(at)));
                }
                Ok(())
            }))
            .ok();
        dlog!("media controls attached to window {:?}", hwnd.0);
        Ok(Session { controls, pressed, seek, current: None })
    }

    fn apply(&mut self, msg: Msg) {
        if let Err(e) = self.try_apply(msg) {
            dlog!("media controls: {e}");
        }
    }

    fn try_apply(&mut self, msg: Msg) -> windows::core::Result<()> {
        let c = &self.controls;
        match msg {
            Msg::Clear => {
                self.current = None;
                c.SetPlaybackStatus(MediaPlaybackStatus::Closed)?;
                let u = c.DisplayUpdater()?;
                u.ClearAll()?;
                u.Update()?;
                c.SetIsEnabled(false)?;
            }
            Msg::Track { id, title, artist, album } => {
                self.current = Some(id);
                c.SetIsEnabled(true)?;
                let u = c.DisplayUpdater()?;
                u.ClearAll()?;
                if let Err(e) = u.SetAppMediaId(&HSTRING::from(APP_USER_MODEL_ID)) {
                    dlog!("setting the media app id failed: {e}");
                }
                u.SetType(MediaPlaybackType::Music)?;
                let music = u.MusicProperties()?;
                music.SetTitle(&HSTRING::from(title))?;
                music.SetArtist(&HSTRING::from(artist))?;
                music.SetAlbumTitle(&HSTRING::from(album))?;
                u.Update()?;
            }
            Msg::Cover { id, png } => {
                if self.current.as_deref() != Some(id.as_str()) {
                    return Ok(());
                }
                let stream = png_stream(&png)?;
                let u = c.DisplayUpdater()?;
                u.SetThumbnail(&RandomAccessStreamReference::CreateFromStream(&stream)?)?;
                u.Update()?;
            }
            Msg::Status(s) => c.SetPlaybackStatus(playback_status(s))?,
            Msg::Buttons { next } => c.SetIsNextEnabled(next)?,
            Msg::Timeline { pos_ms, dur_ms } => {
                let t = SystemMediaTransportControlsTimelineProperties::new()?;
                t.SetStartTime(span(0))?;
                t.SetEndTime(span(dur_ms))?;
                t.SetMinSeekTime(span(0))?;
                t.SetMaxSeekTime(span(dur_ms))?;
                t.SetPosition(span(pos_ms.min(dur_ms)))?;
                c.UpdateTimelineProperties(&t)?;
            }
        }
        Ok(())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let c = &self.controls;
        let _ = c.RemoveButtonPressed(self.pressed);
        if let Some(token) = self.seek {
            let _ = c.RemovePlaybackPositionChangeRequested(token);
        }
        if let Ok(u) = c.DisplayUpdater() {
            let _ = u.ClearAll();
            let _ = u.Update();
        }
        let _ = c.SetPlaybackStatus(MediaPlaybackStatus::Closed);
        let _ = c.SetIsEnabled(false);
        dlog!("media controls detached");
    }
}

/// The picture as an in-memory stream, which is what a card's thumbnail is made from.
fn png_stream(png: &[u8]) -> windows::core::Result<InMemoryRandomAccessStream> {
    let stream = InMemoryRandomAccessStream::new()?;
    let writer = DataWriter::CreateDataWriter(&stream)?;
    writer.WriteBytes(png)?;
    writer.StoreAsync()?.join()?;
    // The writer would close the stream with it; the thumbnail still needs it.
    writer.DetachStream()?;
    stream.Seek(0)?;
    Ok(stream)
}

fn command_for(button: SystemMediaTransportControlsButton) -> Option<Command> {
    Some(match button {
        SystemMediaTransportControlsButton::Play => Command::Play,
        SystemMediaTransportControlsButton::Pause => Command::Pause,
        SystemMediaTransportControlsButton::Stop => Command::Stop,
        SystemMediaTransportControlsButton::Next => Command::Next,
        SystemMediaTransportControlsButton::Previous => Command::Prev,
        _ => return None,
    })
}

fn playback_status(s: Status) -> MediaPlaybackStatus {
    match s {
        Status::Closed => MediaPlaybackStatus::Closed,
        Status::Changing => MediaPlaybackStatus::Changing,
        Status::Paused => MediaPlaybackStatus::Paused,
        Status::Playing => MediaPlaybackStatus::Playing,
    }
}

/// WinRT times are 100 ns ticks.
fn span(ms: u64) -> TimeSpan {
    TimeSpan { Duration: i64::try_from(ms).unwrap_or(i64::MAX).saturating_mul(10_000) }
}

fn millis(t: TimeSpan) -> u64 {
    (t.Duration.max(0) / 10_000) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_become_commands() {
        use SystemMediaTransportControlsButton as B;
        assert_eq!(command_for(B::Play), Some(Command::Play));
        assert_eq!(command_for(B::Pause), Some(Command::Pause));
        assert_eq!(command_for(B::Stop), Some(Command::Stop));
        assert_eq!(command_for(B::Next), Some(Command::Next));
        assert_eq!(command_for(B::Previous), Some(Command::Prev));
        for ignored in [B::Record, B::FastForward, B::Rewind, B::ChannelUp, B::ChannelDown] {
            assert_eq!(command_for(ignored), None, "{ignored:?}");
        }
    }

    #[test]
    fn statuses_map_one_to_one() {
        assert_eq!(playback_status(Status::Closed), MediaPlaybackStatus::Closed);
        assert_eq!(playback_status(Status::Changing), MediaPlaybackStatus::Changing);
        assert_eq!(playback_status(Status::Paused), MediaPlaybackStatus::Paused);
        assert_eq!(playback_status(Status::Playing), MediaPlaybackStatus::Playing);
    }

    #[test]
    fn times_convert_both_ways() {
        assert_eq!(span(1500).Duration, 15_000_000);
        assert_eq!(millis(span(215_400)), 215_400);
        assert_eq!(millis(TimeSpan { Duration: -5 }), 0, "never negative");
        assert_eq!(span(u64::MAX).Duration, i64::MAX, "saturates instead of wrapping");
    }

    #[test]
    fn a_late_session_catches_up_with_the_latest_of_each_kind() {
        let track = |id: &str| Msg::Track { id: id.into(), title: id.into(), artist: String::new(), album: String::new() };
        let mut waiting = Vec::new();
        queue_up(&mut waiting, track("a"));
        queue_up(&mut waiting, Msg::Cover { id: "a".into(), png: vec![1] });
        queue_up(&mut waiting, Msg::Status(Status::Playing));
        queue_up(&mut waiting, Msg::Timeline { pos_ms: 1, dur_ms: 10 });
        queue_up(&mut waiting, Msg::Status(Status::Paused));
        assert_eq!(waiting.len(), 4, "the older status is gone");
        queue_up(&mut waiting, track("b"));
        let kinds: Vec<&str> = waiting
            .iter()
            .map(|m| match m {
                Msg::Track { id, .. } if id == "b" => "track b",
                Msg::Status(Status::Paused) => "paused",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, ["paused", "track b"], "a new track drops the old cover and position");
        queue_up(&mut waiting, Msg::Clear);
        assert!(matches!(waiting.as_slice(), [Msg::Status(_), Msg::Clear]), "clearing replaces the track");
    }

    /// Needs WinRT (any Windows 10+ machine), not a window or a media session.
    #[test]
    fn a_thumbnail_stream_holds_the_picture() {
        // SAFETY: A fresh test thread; a second initialisation in the same mode is harmless.
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        let png = super::super::cover_png(8, 8, vec![200; 8 * 8 * 4], 300).unwrap();
        let stream = png_stream(&png).unwrap();
        assert_eq!(stream.Size().unwrap(), png.len() as u64);
        RandomAccessStreamReference::CreateFromStream(&stream).unwrap();
    }

    #[test]
    fn the_test_process_has_no_window_to_attach_to() {
        assert!(find_window().is_none());
    }
}
