//! Domain models for playback, library, overlays, and live bindings.

use std::cell::RefCell;

use rune::{Live, LiveText, PopupAnchor, TextEdit};
use ym_api::{Album, Artist, Lyrics, Playlist, Track};
use ym_audio::{Engine, EqSettings};

use super::types::{LibTab, Load, Page, PinnedItem, TrackSort};
use crate::i18n::{Lang, S};
use crate::store::Repeat;

/// Where what plays came from, as the player names it.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayingFrom {
    /// My Wave (a mood or a wave by something), or a list.
    pub wave: bool,
    /// Kind label ("Album", "My Wave by artist"), kept as a key so it follows language changes.
    pub kind: S,
    pub name: FromName,
    pub page: Option<Page>,
}

/// What the source badge calls what plays: raw text or a localized string key.
#[derive(Clone, Debug, PartialEq)]
pub enum FromName {
    Text(String),
    Word(S),
}

impl PlayingFrom {
    /// Formats "My Wave · Calm" or "Album · Name", dropping the name if it repeats the kind.
    pub fn line(&self, lang: Lang) -> String {
        let kind = self.kind.get(lang);
        let name = match &self.name {
            FromName::Text(t) => t.trim(),
            FromName::Word(w) => w.get(lang).trim(),
        };
        if name.is_empty() || name.to_lowercase() == kind.trim().to_lowercase() { kind.to_owned() } else { format!("{kind} · {name}") }
    }
}

#[derive(Default)]
pub struct Library {
    pub tab: LibTab,
    pub sort: TrackSort,
    /// Collection search query and cached matching/sorted indices for liked tracks.
    pub filter: TextEdit,
    pub shown: RefCell<Vec<usize>>,
    /// Cache key for `shown`: trimmed lowercase query, active sort, track slice pointer, and length.
    pub shown_key: RefCell<(String, TrackSort, usize, usize)>,
    pub tracks: Load<Vec<Track>>,
    pub playlists: Load<Vec<Playlist>>,
    pub albums: Load<Vec<Album>>,
    pub artists: Load<Vec<Artist>>,
    /// Whether `playlists` came from the service this session (not only from the snapshot).
    pub playlists_fresh: bool,
    /// Whether liked albums, artists, and the Liked cover came from the service this session.
    pub collections_fresh: bool,
    pub liked_cover: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Manual,
    Wave { preset: String, batch: Option<String>, round: u64 },
}

pub struct Player {
    pub engine: Option<Engine>,
    pub engine_failed: bool,
    pub queue: Vec<Track>,
    pub original: Vec<Track>,
    pub index: Option<usize>,
    /// Queue index whose audio is loading before it becomes the visible current track.
    pub pending_index: Option<usize>,
    pub pending_token: Option<u64>,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub volume: f32,
    /// The user wants sound (not paused).
    pub playing: bool,
    pub loading: bool,
    pub pos_ms: u64,
    pub dur_ms: u64,
    pub token: u64,
    pub token_seed: u64,
    pub next: Option<(u64, usize)>,
    pub next_loading: Option<u64>,
    pub source: Source,
    pub wave_fetching: bool,
    /// Incremented on every wave start so stale batches from replaced waves are ignored.
    pub wave_gen: u64,
    pub sim_started: bool,
    pub eq: EqSettings,
    /// Start position for the restored track when its audio first arrives.
    pub resume_ms: Option<u64>,
    /// A seek the engine has not caught up with yet, and when it was made.
    pub seek_hold: Option<(u64, f64)>,
}

#[derive(Default)]
pub struct LyricsUi {
    pub for_track: Option<String>,
    pub lyrics: Load<Lyrics>,
    pub active: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub id: u64,
    pub text: String,
    pub born: f64,
    /// Something went wrong (the island shows it in red).
    pub error: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DownloadActivity {
    pub total: usize,
    pub active: usize,
    pub done: usize,
    pub failed: usize,
    pub changed_at: f64,
}

/// Values that change every frame; bound to elements without re-running the view.
#[derive(Clone)]
pub struct Lives {
    pub clock: LiveText,
    pub remaining: LiveText,
    pub volume: Live<[f32; 4]>,
}

/// What a popover lists.
#[derive(Clone, Debug)]
pub enum Popover {
    /// Actions for the open album, playlist or artist ("..." beside its buttons).
    Actions { anchor: String },
    /// Waves offered by a wave card's arrow: by track, by artist, by release.
    Waves { choices: Vec<WaveChoice>, anchor: String },
    /// Audio quality for the next track load.
    Quality { anchor: String },
    /// Now Playing backdrop style (`Settings::stage`).
    Stage { anchor: String },
    /// Actions for a cover in the pinned-items rail.
    Pinned { item: PinnedItem, anchor: String },
    /// Download queue and completed/failed items.
    Downloads { anchor: String },
    /// Actions for the liked tracks collection.
    Liked { anchor: String },
    /// Equalizer presets, from Settings.
    EqPreset { anchor: String },
    /// Track sort order for the open page.
    TrackSort { anchor: String },
    /// Track sort order for Liked tracks in Library.
    LibSort { anchor: String },
    /// Release sort order and kind filter on an artist page.
    DiscSort { anchor: String },
    /// My Wave tuning knobs and shape style on Home.
    WaveTune { anchor: String },
    /// Available app update summary and install action from the title bar icon.
    Update { anchor: String },
}

/// One wave option in a wave card popover.
#[derive(Clone, Debug)]
pub struct WaveChoice {
    pub station: Station,
    pub kind: String,
    pub name: String,
}

impl Popover {
    pub fn anchor(&self) -> &str {
        match self {
            Popover::Actions { anchor }
            | Popover::Waves { anchor, .. }
            | Popover::Quality { anchor }
            | Popover::Stage { anchor }
            | Popover::Pinned { anchor, .. }
            | Popover::Downloads { anchor }
            | Popover::Liked { anchor }
            | Popover::EqPreset { anchor }
            | Popover::TrackSort { anchor }
            | Popover::LibSort { anchor }
            | Popover::DiscSort { anchor }
            | Popover::WaveTune { anchor }
            | Popover::Update { anchor } => anchor,
        }
    }
}

/// Context menu for a single track, anchored to a cursor point or element key.
#[derive(Clone, Debug)]
pub struct TrackMenu {
    pub track: Track,
    pub queue_index: Option<usize>,
    pub anchor: PopupAnchor,
    /// Opened from the player bar or Now Playing, which already expose play and queue controls.
    pub player: bool,
    /// Showing the account's playlists to pick a target for "Add to playlist".
    pub choosing: bool,
}

/// A wave seeded by a single track, artist, album, or the user's collection.
#[derive(Clone, Debug, PartialEq)]
pub enum Station {
    Track(String),
    Artist(String),
    Album(String),
    Collection,
}

impl Station {
    /// Its wave preset id for `net::Backend::wave`.
    pub fn id(&self) -> String {
        match self {
            Station::Track(id) => format!("track:{id}"),
            Station::Artist(id) => format!("artist:{id}"),
            Station::Album(id) => format!("album:{id}"),
            Station::Collection => "collection".to_owned(),
        }
    }

    /// Whether `preset` matches this station.
    pub fn is(&self, preset: &str) -> bool {
        self.id() == preset
    }
}

/// What the details drawer ("Details") displays.
#[derive(Clone, Debug)]
pub enum Sheet {
    Track(Box<Track>),
    /// The open album, playlist or artist (`State::detail`).
    Page,
}
