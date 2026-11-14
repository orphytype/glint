//! MPRIS, the Linux media session: the media keys, the "now playing" card of GNOME, KDE and the
//! other desktops, and `playerctl`.
//!
//! It serves `org.mpris.MediaPlayer2` and `org.mpris.MediaPlayer2.Player` on the session bus, from
//! a thread of ours with its own small tokio runtime (zbus needs one, and the UI thread must never
//! wait on the bus). The UI thread sends what changed; method calls from the desktop go out as
//! [`Command`]s. With no session bus (a bare X server, a container) there is simply no session.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use ym_api::Track;
use zbus::object_server::SignalEmitter;
use zbus::{Connection, ObjectPath, OwnedObjectPath, OwnedVariant, Str, Variant};

use super::{Command, Status};

const BUS_NAME: &str = "org.mpris.MediaPlayer2.glint";
const PATH: &str = "/org/mpris/MediaPlayer2";
/// How long setting up the session may take before the app goes on without one.
const SETUP_FOR: Duration = Duration::from_secs(3);
/// What `mpris:trackid` says when nothing is loaded, as the spec asks.
const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";

type OnCommand = Arc<dyn Fn(Command) + Send + Sync>;

enum Msg {
    Track(Meta),
    Status(Status),
    Buttons { next: bool },
    Timeline { pos_ms: u64, dur_ms: u64 },
    Clear,
}

/// The track, as the card shows it.
#[derive(Clone, Debug, Default, PartialEq)]
struct Meta {
    id: String,
    title: String,
    artists: Vec<String>,
    album: String,
    art: Option<String>,
}

/// What the two interfaces report, shared between them and the thread that updates it.
#[derive(Debug)]
struct Model {
    meta: Option<Meta>,
    status: Status,
    can_next: bool,
    dur_ms: u64,
    /// The position last pushed and when; the spec has clients extrapolate while playing, but
    /// `Position` is read on demand, so it is extrapolated here the same way.
    pos_ms: u64,
    pos_at: Instant,
}

impl Model {
    fn position_ms(&self, now: Instant) -> u64 {
        let ran = if self.status == Status::Playing { now.saturating_duration_since(self.pos_at).as_millis() as u64 } else { 0 };
        let at = self.pos_ms + ran;
        if self.dur_ms > 0 { at.min(self.dur_ms) } else { at }
    }
}

type Shared = Arc<Mutex<Model>>;

/// The handle the UI thread keeps: a queue into the bus thread.
pub struct Mpris {
    tx: Option<UnboundedSender<Msg>>,
    worker: Option<JoinHandle<()>>,
}

impl Mpris {
    pub fn start(on_command: OnCommand) -> Option<Mpris> {
        let (tx, mut rx) = unbounded_channel::<Msg>();
        let (ready, started) = mpsc::sync_channel::<bool>(1);
        let worker = std::thread::Builder::new()
            .name("glint-mpris".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                    Ok(r) => r,
                    Err(e) => {
                        dlog!("media session runtime: {e}");
                        let _ = ready.send(false);
                        return;
                    }
                };
                runtime.block_on(async move {
                    let model: Shared = Arc::new(Mutex::new(Model { meta: None, status: Status::Closed, can_next: false, dur_ms: 0, pos_ms: 0, pos_at: Instant::now() }));
                    let setup = zbus::connection::Builder::session()
                        .name(BUS_NAME)
                        .serve_at(PATH, Root { on_command: on_command.clone() })
                        .serve_at(PATH, Player { model: model.clone(), on_command });
                    let conn = match tokio::time::timeout(SETUP_FOR, setup.build()).await {
                        Ok(Ok(conn)) => conn,
                        Ok(Err(e)) => {
                            dlog!("media session: {e}");
                            let _ = ready.send(false);
                            return;
                        }
                        Err(_) => {
                            dlog!("media session: the session bus timed out");
                            let _ = ready.send(false);
                            return;
                        }
                    };
                    let _ = ready.send(true);
                    dlog!("media session on the bus as {BUS_NAME}");
                    while let Some(msg) = rx.recv().await {
                        apply(&conn, &model, msg).await;
                    }
                    let _ = conn.close().await;
                    dlog!("media session closed");
                });
            });
        let worker = match worker {
            Ok(w) => w,
            Err(e) => {
                dlog!("media session thread failed: {e}");
                return None;
            }
        };
        if started.recv().unwrap_or(false) {
            Some(Mpris { tx: Some(tx), worker: Some(worker) })
        } else {
            let _ = worker.join();
            None
        }
    }

    fn send(&self, msg: Msg) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(msg);
        }
    }

    pub fn track(&self, t: &Track) {
        self.send(Msg::Track(Meta {
            id: t.id.clone(),
            title: t.full_title(),
            artists: t.artists.iter().map(|a| a.name.clone()).filter(|n| !n.is_empty()).collect(),
            album: t.album().map(|a| a.title.clone()).unwrap_or_default(),
            // The desktop fetches the cover itself, by URL (the PNG the Windows card takes is not used).
            art: t.cover_url(400),
        }));
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

impl Drop for Mpris {
    /// Closing the queue ends the thread, which leaves the bus; wait for that.
    fn drop(&mut self) {
        self.tx = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Applies one change to the model and tells the bus what it changed.
async fn apply(conn: &Connection, model: &Shared, msg: Msg) {
    #[derive(Default)]
    struct Changed {
        meta: bool,
        status: bool,
        buttons: bool,
        seeked: Option<i64>,
    }
    let mut changed = Changed::default();
    {
        let mut m = model.lock().unwrap();
        match msg {
            Msg::Track(meta) => {
                m.meta = Some(meta);
                m.dur_ms = 0;
                m.pos_ms = 0;
                m.pos_at = Instant::now();
                changed.meta = true;
                changed.buttons = true;
            }
            Msg::Status(s) => {
                // Keep the extrapolated position when pausing, so it does not jump back.
                let now = Instant::now();
                m.pos_ms = m.position_ms(now);
                m.pos_at = now;
                m.status = s;
                changed.status = true;
                changed.buttons = true;
            }
            Msg::Buttons { next } => {
                m.can_next = next;
                changed.buttons = true;
            }
            Msg::Timeline { pos_ms, dur_ms } => {
                let length_changed = m.dur_ms != dur_ms;
                m.pos_ms = pos_ms;
                m.pos_at = Instant::now();
                m.dur_ms = dur_ms;
                changed.meta = length_changed;
                changed.seeked = Some(micros(pos_ms));
            }
            Msg::Clear => {
                m.meta = None;
                m.status = Status::Closed;
                m.dur_ms = 0;
                m.pos_ms = 0;
                changed.meta = true;
                changed.status = true;
                changed.buttons = true;
            }
        }
    }
    let Ok(iface) = conn.object_server().interface::<_, Player>(PATH).await else { return };
    let emitter = iface.signal_emitter();
    let player = iface.get().await;
    if changed.meta {
        let _ = player.metadata_changed(emitter).await;
    }
    if changed.status {
        let _ = player.playback_status_changed(emitter).await;
    }
    if changed.buttons {
        let _ = player.can_go_next_changed(emitter).await;
        let _ = player.can_go_previous_changed(emitter).await;
        let _ = player.can_play_changed(emitter).await;
        let _ = player.can_pause_changed(emitter).await;
        let _ = player.can_seek_changed(emitter).await;
    }
    if let Some(at) = changed.seeked {
        let _ = Player::seeked(emitter, at).await;
    }
}

// ---- org.mpris.MediaPlayer2 -------------------------------------------------------------------

struct Root {
    on_command: OnCommand,
}

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {
        (self.on_command)(Command::Show);
    }
    fn quit(&self) {
        (self.on_command)(Command::Quit);
    }
    #[zbus(property)]
    fn can_quit(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn identity(&self) -> &str {
        "Glint"
    }
    /// The name of the `.desktop` file, without the extension, so the card shows the app's icon.
    #[zbus(property)]
    fn desktop_entry(&self) -> &str {
        "glint"
    }
    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        Vec::new()
    }
    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        Vec::new()
    }
}

// ---- org.mpris.MediaPlayer2.Player ------------------------------------------------------------

struct Player {
    model: Shared,
    on_command: OnCommand,
}

impl Player {
    fn has_track(&self) -> bool {
        self.model.lock().unwrap().meta.is_some()
    }
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn next(&self) {
        (self.on_command)(Command::Next);
    }
    fn previous(&self) {
        (self.on_command)(Command::Prev);
    }
    fn pause(&self) {
        (self.on_command)(Command::Pause);
    }
    fn play_pause(&self) {
        (self.on_command)(Command::Toggle);
    }
    fn stop(&self) {
        (self.on_command)(Command::Stop);
    }
    fn play(&self) {
        (self.on_command)(Command::Play);
    }
    /// Moves by `offset` microseconds from where the track is now.
    fn seek(&self, offset: i64) {
        let (at, dur) = {
            let m = self.model.lock().unwrap();
            if m.meta.is_none() {
                return;
            }
            (m.position_ms(Instant::now()), m.dur_ms)
        };
        if let Some(to) = seek_target(at, dur, offset) {
            (self.on_command)(Command::Seek(to));
        } else {
            // Past the end, as the spec says, is the next track.
            (self.on_command)(Command::Next);
        }
    }
    /// Jumps to `position` microseconds, if `track_id` is still the track playing.
    fn set_position(&self, track_id: OwnedObjectPath, position: i64) {
        let dur = {
            let m = self.model.lock().unwrap();
            match &m.meta {
                Some(meta) if track_path(&meta.id) == track_id.as_str() => m.dur_ms,
                _ => return,
            }
        };
        if position >= 0 && (dur == 0 || millis(position) <= dur) {
            (self.on_command)(Command::Seek(millis(position)));
        }
    }
    fn open_uri(&self, _uri: String) {}

    #[zbus(property)]
    fn playback_status(&self) -> &'static str {
        status_name(self.model.lock().unwrap().status)
    }
    #[zbus(property)]
    fn loop_status(&self) -> &str {
        "None"
    }
    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn shuffle(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedVariant> {
        let m = self.model.lock().unwrap();
        metadata(m.meta.as_ref(), m.dur_ms)
    }
    #[zbus(property)]
    fn volume(&self) -> f64 {
        1.0
    }
    /// Read on demand, never signalled (the spec says so: clients extrapolate, and `Seeked` tells
    /// them when that stopped holding).
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        micros(self.model.lock().unwrap().position_ms(Instant::now()))
    }
    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        let m = self.model.lock().unwrap();
        m.meta.is_some() && m.can_next
    }
    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        self.has_track()
    }
    #[zbus(property)]
    fn can_play(&self) -> bool {
        self.has_track()
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        self.has_track()
    }
    #[zbus(property)]
    fn can_seek(&self) -> bool {
        let m = self.model.lock().unwrap();
        m.meta.is_some() && m.dur_ms > 0
    }
    #[zbus(property(emits_changed_signal = "const"))]
    fn can_control(&self) -> bool {
        true
    }
    #[zbus(signal)]
    async fn seeked(emitter: &SignalEmitter<'_>, position: i64) -> zbus::Result<()>;
}

fn status_name(s: Status) -> &'static str {
    match s {
        Status::Playing => "Playing",
        // A track being fetched is about to play; "Paused" keeps the card up meanwhile.
        Status::Paused | Status::Changing => "Paused",
        Status::Closed => "Stopped",
    }
}

/// The object path naming a track: the spec wants one per track, and Yandex ids are digits with
/// at most a colon (`123:456`), so anything else becomes an underscore.
fn track_path(id: &str) -> String {
    let safe: String = id.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    format!("/org/glint/track/t{safe}")
}

fn metadata(meta: Option<&Meta>, dur_ms: u64) -> HashMap<String, OwnedVariant> {
    let mut map = HashMap::new();
    let Some(meta) = meta else {
        if let Ok(path) = ObjectPath::try_from(NO_TRACK) {
            map.insert("mpris:trackid".into(), OwnedVariant::from(path));
        }
        return map;
    };
    if let Ok(path) = ObjectPath::try_from(track_path(&meta.id)) {
        map.insert("mpris:trackid".into(), OwnedVariant::from(path));
    }
    map.insert("xesam:title".into(), OwnedVariant::from(Str::from(meta.title.clone())));
    if let Ok(v) = OwnedVariant::try_from(Variant::from(meta.artists.clone())) {
        map.insert("xesam:artist".into(), v);
    }
    if !meta.album.is_empty() {
        map.insert("xesam:album".into(), OwnedVariant::from(Str::from(meta.album.clone())));
    }
    if dur_ms > 0 {
        map.insert("mpris:length".into(), OwnedVariant::from(micros(dur_ms)));
    }
    if let Some(art) = &meta.art {
        map.insert("mpris:artUrl".into(), OwnedVariant::from(Str::from(art.clone())));
    }
    map
}

/// Where a relative seek of `offset_us` from `at_ms` lands, or None when it runs past the end.
fn seek_target(at_ms: u64, dur_ms: u64, offset_us: i64) -> Option<u64> {
    let to = at_ms as i64 + offset_us / 1000;
    if dur_ms > 0 && to > dur_ms as i64 {
        return None;
    }
    Some(to.max(0) as u64)
}

/// MPRIS times are microseconds.
fn micros(ms: u64) -> i64 {
    i64::try_from(ms).unwrap_or(i64::MAX).saturating_mul(1000)
}

fn millis(us: i64) -> u64 {
    (us.max(0) / 1000) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_have_mpris_names() {
        assert_eq!(status_name(Status::Playing), "Playing");
        assert_eq!(status_name(Status::Paused), "Paused");
        assert_eq!(status_name(Status::Changing), "Paused");
        assert_eq!(status_name(Status::Closed), "Stopped");
    }

    #[test]
    fn track_paths_are_object_paths() {
        for id in ["123", "123:456", "a-b.c", ""] {
            let p = track_path(id);
            assert!(ObjectPath::try_from(p.as_str()).is_ok(), "{p}");
        }
        assert_ne!(track_path("1:2"), track_path("12"));
    }

    #[test]
    fn metadata_names_the_track() {
        let meta = Meta { id: "1:2".into(), title: "T".into(), artists: vec!["A".into(), "B".into()], album: "Al".into(), art: Some("https://x/y".into()) };
        let m = metadata(Some(&meta), 200_000);
        for key in ["mpris:trackid", "xesam:title", "xesam:artist", "xesam:album", "mpris:length", "mpris:artUrl"] {
            assert!(m.contains_key(key), "{key}");
        }
        assert_eq!(i64::try_from(m["mpris:length"].clone()).unwrap(), 200_000_000);
        let empty = metadata(None, 0);
        assert_eq!(empty.len(), 1, "only the no-track id");
    }

    #[test]
    fn seeks_stay_in_the_track() {
        assert_eq!(seek_target(10_000, 200_000, 5_000_000), Some(15_000));
        assert_eq!(seek_target(10_000, 200_000, -60_000_000), Some(0), "never before the start");
        assert_eq!(seek_target(190_000, 200_000, 20_000_000), None, "past the end is the next track");
        assert_eq!(seek_target(5_000, 0, 1_000_000), Some(6_000), "unknown length: no end to run past");
    }

    #[test]
    fn the_position_runs_only_while_playing() {
        let t0 = Instant::now();
        let mut m = Model { meta: None, status: Status::Playing, can_next: false, dur_ms: 10_000, pos_ms: 1_000, pos_at: t0 };
        assert_eq!(m.position_ms(t0 + Duration::from_secs(2)), 3_000);
        assert_eq!(m.position_ms(t0 + Duration::from_secs(60)), 10_000, "never past the end");
        m.status = Status::Paused;
        assert_eq!(m.position_ms(t0 + Duration::from_secs(2)), 1_000);
    }

    #[test]
    fn times_convert_both_ways() {
        assert_eq!(micros(1500), 1_500_000);
        assert_eq!(millis(micros(215_400)), 215_400);
        assert_eq!(millis(-5), 0);
    }
}
