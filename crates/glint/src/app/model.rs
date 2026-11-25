//! Domain models for playback, library, overlays, and live bindings.

use std::cell::RefCell;

use rune::{Live, LiveText, PopupAnchor, TextEdit};
use ym_api::{Album, Artist, Lyrics, Playlist, Track};
use ym_audio::{Engine, EqSettings};

use super::types::{AlbumSort, ArtistSort, DiscKind, LibTab, Load, Page, PinnedItem, PlaylistSort, TrackSort};
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
    /// Short, scannable source label for pills in Now Playing and on Home: omits verbose prefixes
    /// (since the pill shows a matching kind icon) so the badge never outgrows the track title.
    pub fn line(&self, lang: Lang) -> String {
        let name = match &self.name {
            FromName::Text(t) => t.trim(),
            FromName::Word(w) => w.get(lang).trim(),
        };
        if self.wave {
            if self.kind == S::WaveTitle {
                let title = S::WaveTitle.get(lang);
                if name.is_empty() || name.eq_ignore_ascii_case(title) || name == S::WaveOnYour.get(lang) {
                    return title.to_owned();
                }
                return format!("{} · {name}", S::WaveShort.get(lang));
            }
            if self.kind == S::WaveByCollection {
                return format!("{} · {}", S::WaveShort.get(lang), S::Collection.get(lang));
            }
            if !name.is_empty() {
                return format!("{} · {name}", S::WaveShort.get(lang));
            }
            return self.kind.get(lang).to_owned();
        }
        if self.kind == S::Liked {
            return S::Liked.get(lang).to_owned();
        }
        if matches!(self.name, FromName::Word(S::ListeningHistory)) {
            return S::HistoryShort.get(lang).to_owned();
        }
        if self.kind == S::Single {
            return if name.is_empty() { S::Single.get(lang).to_owned() } else { format!("{} · {name}", S::Single.get(lang)) };
        }
        if !name.is_empty() {
            return name.to_owned();
        }
        self.kind.get(lang).to_owned()
    }

    /// Context-aware variant for pills near the playing track: when playing a single whose name
    /// matches the track title, shows just "Single" instead of repeating the track title twice.
    pub fn line_for_track(&self, lang: Lang, track: Option<&Track>) -> String {
        if !self.wave && self.kind == S::Single {
            let name = match &self.name {
                FromName::Text(t) => t.trim(),
                FromName::Word(w) => w.get(lang).trim(),
            };
            if let Some(t) = track
                && (name.is_empty() || name.eq_ignore_ascii_case(t.title.trim()) || name.eq_ignore_ascii_case(t.full_title().trim()))
            {
                return S::Single.get(lang).to_owned();
            }
        }
        self.line(lang)
    }
}

#[derive(Default)]
pub struct Library {
    pub tab: LibTab,
    pub sort: TrackSort,
    pub playlist_sort: PlaylistSort,
    pub album_sort: AlbumSort,
    pub album_kind: DiscKind,
    pub artist_sort: ArtistSort,
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
    pub shuffle: bool,
    pub repeat: Repeat,
    pub volume: f32,
    /// Last non-zero volume before muting, restored on unmute in the player and tray.
    pub unmuted_volume: f32,
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
    /// Format and bitrate of the audio currently loaded in the engine.
    pub current_audio: Option<crate::net::StreamQuality>,
    /// Format and bitrate of the prefetched next track (`next`).
    pub next_audio: Option<crate::net::StreamQuality>,
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
    /// Native seek-bar fill rect `[x, y, w, h]` for the dock when shaders are off.
    pub seek_dock: Live<[f32; 4]>,
    /// Native seek-bar thumb rect `[x, y, w, h]` for the dock when shaders are off.
    pub seek_dock_thumb: Live<[f32; 4]>,
    /// Native seek-bar fill rect `[x, y, w, h]` for Now Playing when shaders are off.
    pub seek_np: Live<[f32; 4]>,
    /// Native seek-bar thumb rect `[x, y, w, h]` for Now Playing when shaders are off.
    pub seek_np_thumb: Live<[f32; 4]>,
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

/// Active track row drag-and-drop reorder state.
#[derive(Clone, Debug, PartialEq)]
pub struct RowDrag {
    /// Whether dragging in the play queue (`true`) or the open playlist (`false`).
    pub queue: bool,
    /// Row index where the drag started.
    pub start: usize,
    /// Current index of the dragged track after live swaps.
    pub cur: usize,
    /// Total vertical pointer displacement (`Drag.dy`).
    pub dy: f32,
}
