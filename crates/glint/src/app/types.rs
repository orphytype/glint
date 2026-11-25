//! Navigation pages, loading states, download status, and detail/sort types.

use rune::TextEdit;
use ym_api::{Album, Artist, SearchResult, Track};

use crate::i18n::S;
use crate::store::PinnedSetting;

#[derive(Clone, Debug, PartialEq)]
pub enum Page {
    Home,
    Explore,
    Search,
    Library,
    Album(String),
    Playlist(i64, i64),
    Artist(String),
}

/// A page kept in the navigation rail alongside its display metadata so the rail can show its
/// cover after leaving the page. Keyed by `Page`, which keeps pinning idempotent.
#[derive(Clone, Debug, PartialEq)]
pub struct PinnedItem {
    pub page: Page,
    pub title: String,
    pub cover: Option<String>,
    pub artist: bool,
}

impl PinnedItem {
    pub fn key(&self) -> String {
        self.page.key()
    }

    pub(super) fn from_setting(p: &PinnedSetting) -> Option<Self> {
        let page = match p.kind.as_str() {
            "album" => Page::Album(p.id.clone()),
            "artist" => Page::Artist(p.id.clone()),
            "playlist" => Page::Playlist(p.id.parse().ok()?, p.secondary),
            _ => return None,
        };
        Some(Self { artist: matches!(&page, Page::Artist(_)), page, title: p.title.clone(), cover: p.cover.clone() })
    }

    pub(super) fn setting(&self) -> PinnedSetting {
        let (kind, id, secondary) = match &self.page {
            Page::Album(id) => ("album", id.clone(), 0),
            Page::Artist(id) => ("artist", id.clone(), 0),
            Page::Playlist(uid, kind) => ("playlist", uid.to_string(), *kind),
            _ => return PinnedSetting::default(),
        };
        PinnedSetting { kind: kind.to_owned(), id, secondary, title: self.title.clone(), cover: self.cover.clone() }
    }
}

impl Page {
    pub fn key(&self) -> String {
        match self {
            Page::Home => "home".into(),
            Page::Explore => "explore".into(),
            Page::Search => "search".into(),
            Page::Library => "library".into(),
            Page::Album(id) => format!("album-{id}"),
            Page::Playlist(u, k) => format!("playlist-{u}-{k}"),
            Page::Artist(id) => format!("artist-{id}"),
        }
    }

    /// Which rail item lights up.
    pub fn rail(&self) -> Page {
        match self {
            Page::Album(_) | Page::Playlist(..) | Page::Artist(_) => Page::Library,
            other => other.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub enum Load<T> {
    #[default]
    Idle,
    Loading,
    Ready(T),
    Failed(String),
}

impl<T> Load<T> {
    pub fn get(&self) -> Option<&T> {
        match self {
            Load::Ready(v) => Some(v),
            _ => None,
        }
    }

    pub fn is_loading(&self) -> bool {
        matches!(self, Load::Loading)
    }
}

/// A track's download state, kept in the UI so the row can explain what a click did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DownloadState {
    Downloading,
    Done,
    Failed,
}

/// Why a track could not be downloaded. Transient reasons can be retried; lasting ones stay with
/// the track so a page can treat the rest as done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadFail {
    Offline,
    /// The service has no file for it (no rights, taken down, not for this account).
    Unavailable,
    Region,
    SignIn,
    Storage,
    Service,
}

impl DownloadFail {
    pub fn of(e: &ym_api::Error) -> DownloadFail {
        match e {
            ym_api::Error::Network(_) => DownloadFail::Offline,
            ym_api::Error::RegionRestricted => DownloadFail::Region,
            ym_api::Error::Unauthorized => DownloadFail::SignIn,
            ym_api::Error::Http { status, .. } if *status == 429 || *status >= 500 => DownloadFail::Service,
            ym_api::Error::Http { .. } | ym_api::Error::Api(_) | ym_api::Error::Parse(_) => DownloadFail::Unavailable,
        }
    }

    /// Trying again will not help.
    pub fn lasting(self) -> bool {
        matches!(self, DownloadFail::Unavailable | DownloadFail::Region)
    }

    pub fn reason(self) -> S {
        match self {
            DownloadFail::Offline => S::DlWhyOffline,
            DownloadFail::Unavailable => S::DlWhyUnavailable,
            DownloadFail::Region => S::DlWhyRegion,
            DownloadFail::SignIn => S::DlWhySignIn,
            DownloadFail::Storage => S::DlWhyStorage,
            DownloadFail::Service => S::DlWhyService,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LibTab {
    #[default]
    Tracks,
    Playlists,
    Albums,
    Artists,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SearchTab {
    #[default]
    All,
    Tracks,
    Albums,
    Artists,
    Playlists,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum NpTab {
    /// Neither side panel: the player alone, in the middle of the window.
    #[default]
    Closed,
    Lyrics,
    Queue,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub enum Session {
    #[default]
    LoggedOut,
    Live,
}

#[derive(Default)]
pub struct LoginUi {
    pub token: TextEdit,
    pub busy: bool,
    pub error: Option<S>,
    /// Show the pasted token instead of dots.
    pub reveal: bool,
}

#[derive(Default)]
pub struct SearchUi {
    pub edit: TextEdit,
    /// The text the current results are for.
    pub query: String,
    /// The query as if typed on the other keyboard layout, when the query as typed found nothing
    /// and this found something. The results are for this text then.
    pub relaid: Option<String>,
    /// The text last seen in the box, and when it changed (for the debounce).
    pub seen: String,
    pub changed_at: f64,
    pub results: Load<SearchResult>,
    pub tab: SearchTab,
    pub generation: u64,
    /// One kind searched on its own (a chip other than "All" picked): more of that kind than
    /// "All" brings back. `more_for` says which text and which kind it is for.
    pub more: Load<SearchResult>,
    pub more_for: Option<(String, SearchTab)>,
}

#[derive(Default, Clone)]
pub struct Detail {
    pub page: Option<Page>,
    pub title: String,
    pub subtitle: String,
    /// Cover template (as the API gives it).
    pub cover: Option<String>,
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub similar: Vec<Artist>,
    pub state: Load<()>,
    pub generation: u64,
    pub is_artist: bool,
    /// Facts shown in the details modal (genres, year, track count, length, description).
    pub facts: Vec<(S, String)>,
    /// An artist's full track list once requested, and whether the list shows all or popular.
    pub all_tracks: Load<Vec<Track>>,
    pub showing_all: bool,
    pub popular: Vec<Track>,
    /// Collection like key (`"album:ID"`, `"artist:ID"`, `"playlist:UID:KIND"`), when likeable.
    pub like_key: Option<String>,
    /// Page URL on music.yandex.ru.
    pub link: Option<String>,
    /// An album's first artist id (for the menu).
    pub artist: Option<String>,
    /// An album's artists, each a link in the page header.
    pub artists: Vec<Artist>,
    pub year: Option<i32>,
    pub monthly_listeners: Option<u64>,
    /// Change in monthly listeners compared to the 30 days before.
    pub listeners_delta: Option<i64>,
    /// Artist photo templates, first being the primary photo.
    pub photos: Vec<String>,
    /// Artist playlist scanner link on band.link.
    pub bandlink: Option<ym_api::OutLink>,
    pub labels: Vec<String>,
    pub likes: Option<u64>,
    /// Artist social and website links, plus optional donation link.
    pub links: Vec<ym_api::ArtistLink>,
    pub donation: Option<String>,
    /// API release kind (`"single"`, `"compilation"`, or none for an album).
    pub release_kind: Option<String>,
    /// Track ordering and the original order kept while another sort is active.
    pub sort: TrackSort,
    pub unsorted: Option<Vec<Track>>,
    /// Playlist search filter text.
    pub filter: TextEdit,
    /// What the filter finds (indices into `tracks`), and the query, sort and list it is for.
    pub shown: std::cell::RefCell<Vec<usize>>,
    pub shown_key: std::cell::RefCell<(String, TrackSort, usize, usize)>,
    /// Artist discography sort order and release-type filter.
    pub disc_sort: DiscSort,
    pub disc_kind: DiscKind,
}

/// The order of a page's tracks.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackSort {
    /// As the release or playlist has them.
    #[default]
    Original,
    Title,
    Artist,
    Album,
    Longest,
    Shortest,
}

impl TrackSort {
    pub const ALL: [TrackSort; 6] = [TrackSort::Original, TrackSort::Title, TrackSort::Artist, TrackSort::Album, TrackSort::Longest, TrackSort::Shortest];

    pub fn name(self) -> S {
        match self {
            TrackSort::Original => S::SortOriginal,
            TrackSort::Title => S::SortTitle,
            TrackSort::Artist => S::SortArtist,
            TrackSort::Album => S::SortAlbum,
            TrackSort::Longest => S::SortLongest,
            TrackSort::Shortest => S::SortShortest,
        }
    }

    /// Sorts `tracks` into this order; ties keep their original order.
    pub fn apply(self, tracks: &[Track]) -> Vec<Track> {
        let mut v = tracks.to_vec();
        let low = |s: &str| s.to_lowercase();
        match self {
            TrackSort::Original => {}
            TrackSort::Title => v.sort_by_cached_key(|t| low(&t.full_title())),
            TrackSort::Artist => v.sort_by_cached_key(|t| (low(&t.artists_line()), low(&t.full_title()))),
            TrackSort::Album => v.sort_by_cached_key(|t| t.albums.first().map(|a| low(&a.title)).unwrap_or_default()),
            TrackSort::Longest => v.sort_by_key(|t| std::cmp::Reverse(t.duration_ms.unwrap_or(0))),
            TrackSort::Shortest => v.sort_by_key(|t| t.duration_ms.unwrap_or(u64::MAX)),
        }
        v
    }

    /// Sorts `indices` into `tracks` by this order; ties keep their relative order.
    pub fn sort_indices(self, tracks: &[Track], indices: &mut [usize]) {
        let low = |s: &str| s.to_lowercase();
        match self {
            TrackSort::Original => {}
            TrackSort::Title => indices.sort_by_cached_key(|&i| low(&tracks[i].full_title())),
            TrackSort::Artist => indices.sort_by_cached_key(|&i| (low(&tracks[i].artists_line()), low(&tracks[i].full_title()))),
            TrackSort::Album => indices.sort_by_cached_key(|&i| tracks[i].albums.first().map(|a| low(&a.title)).unwrap_or_default()),
            TrackSort::Longest => indices.sort_by_key(|&i| std::cmp::Reverse(tracks[i].duration_ms.unwrap_or(0))),
            TrackSort::Shortest => indices.sort_by_key(|&i| tracks[i].duration_ms.unwrap_or(u64::MAX)),
        }
    }
}

/// The order of playlists in Library (default: most recently added/updated first).
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaylistSort {
    #[default]
    Recent,
    Title,
    MostTracks,
    Longest,
}

impl PlaylistSort {
    pub const ALL: [PlaylistSort; 4] = [PlaylistSort::Recent, PlaylistSort::Title, PlaylistSort::MostTracks, PlaylistSort::Longest];

    pub fn name(self) -> S {
        match self {
            PlaylistSort::Recent => S::SortRecent,
            PlaylistSort::Title => S::SortTitle,
            PlaylistSort::MostTracks => S::SortMostTracks,
            PlaylistSort::Longest => S::SortLongest,
        }
    }

    pub fn sort_indices(self, items: &[ym_api::Playlist], indices: &mut [usize]) {
        match self {
            PlaylistSort::Recent => {}
            PlaylistSort::Title => indices.sort_by_cached_key(|&i| items[i].title.to_lowercase()),
            PlaylistSort::MostTracks => indices.sort_by_key(|&i| std::cmp::Reverse(items[i].track_count.unwrap_or(0))),
            PlaylistSort::Longest => indices.sort_by_key(|&i| std::cmp::Reverse(items[i].duration_ms.unwrap_or(0))),
        }
    }
}

/// The order of liked albums in Library (default: most recently added to favorites first).
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlbumSort {
    #[default]
    Recent,
    Newest,
    Oldest,
    Title,
    Artist,
}

impl AlbumSort {
    pub const ALL: [AlbumSort; 5] = [AlbumSort::Recent, AlbumSort::Newest, AlbumSort::Oldest, AlbumSort::Title, AlbumSort::Artist];

    pub fn name(self) -> S {
        match self {
            AlbumSort::Recent => S::SortRecent,
            AlbumSort::Newest => S::SortNewest,
            AlbumSort::Oldest => S::SortOldest,
            AlbumSort::Title => S::SortTitle,
            AlbumSort::Artist => S::SortArtist,
        }
    }

    pub fn sort_indices(self, items: &[Album], indices: &mut [usize]) {
        match self {
            AlbumSort::Recent => {}
            AlbumSort::Newest => indices.sort_by(|&a, &b| {
                let (xa, xb) = (&items[a], &items[b]);
                (xb.year.unwrap_or(0), xb.release_date.as_deref().unwrap_or("")).cmp(&(xa.year.unwrap_or(0), xa.release_date.as_deref().unwrap_or("")))
            }),
            AlbumSort::Oldest => indices.sort_by(|&a, &b| {
                let (xa, xb) = (&items[a], &items[b]);
                (xa.year.unwrap_or(i32::MAX), xa.release_date.as_deref().unwrap_or("")).cmp(&(xb.year.unwrap_or(i32::MAX), xb.release_date.as_deref().unwrap_or("")))
            }),
            AlbumSort::Title => indices.sort_by_cached_key(|&i| items[i].title.to_lowercase()),
            AlbumSort::Artist => indices.sort_by_cached_key(|&i| (items[i].artists_line().to_lowercase(), items[i].title.to_lowercase())),
        }
    }
}

/// The order of liked artists in Library (default: most recently added to favorites first).
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtistSort {
    #[default]
    Recent,
    Name,
    NameDesc,
}

impl ArtistSort {
    pub const ALL: [ArtistSort; 3] = [ArtistSort::Recent, ArtistSort::Name, ArtistSort::NameDesc];

    pub fn name(self) -> S {
        match self {
            ArtistSort::Recent => S::SortRecent,
            ArtistSort::Name => S::SortNameAsc,
            ArtistSort::NameDesc => S::SortNameDesc,
        }
    }

    pub fn sort_indices(self, items: &[Artist], indices: &mut [usize]) {
        match self {
            ArtistSort::Recent => {}
            ArtistSort::Name => indices.sort_by_cached_key(|&i| items[i].name.to_lowercase()),
            ArtistSort::NameDesc => indices.sort_by_cached_key(|&i| std::cmp::Reverse(items[i].name.to_lowercase())),
        }
    }
}

/// The order of an artist's releases.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscSort {
    #[default]
    Newest,
    Oldest,
    Title,
}

impl DiscSort {
    pub const ALL: [DiscSort; 3] = [DiscSort::Newest, DiscSort::Oldest, DiscSort::Title];

    pub fn name(self) -> S {
        match self {
            DiscSort::Newest => S::SortNewest,
            DiscSort::Oldest => S::SortOldest,
            DiscSort::Title => S::SortTitle,
        }
    }
}

/// Which of an artist's releases are shown.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscKind {
    #[default]
    All,
    Albums,
    Singles,
    Compilations,
}

impl DiscKind {
    pub const ALL: [DiscKind; 4] = [DiscKind::All, DiscKind::Albums, DiscKind::Singles, DiscKind::Compilations];

    pub fn name(self) -> S {
        match self {
            DiscKind::All => S::KindAll,
            DiscKind::Albums => S::Albums,
            DiscKind::Singles => S::KindSingles,
            DiscKind::Compilations => S::KindCompilations,
        }
    }

    /// Whether a release of the API's `kind` belongs to this bucket.
    pub fn holds(self, kind: Option<&str>) -> bool {
        let k = kind.unwrap_or("");
        match self {
            DiscKind::All => true,
            DiscKind::Albums => !matches!(k, "single" | "compilation"),
            DiscKind::Singles => k == "single",
            DiscKind::Compilations => k == "compilation",
        }
    }
}

/// What a release is called: album, single, or compilation.
pub fn release_kind_name(kind: Option<&str>) -> S {
    match kind {
        Some("single") => S::Single,
        Some("compilation") => S::Compilation,
        _ => S::Album,
    }
}

/// Identifies a release as single, compilation, or album, falling back to `track_count == 1`
/// when Yandex Music omits `type: "single"` on an embedded album object.
pub fn album_kind_name(al: &Album) -> S {
    match al.kind.as_deref() {
        Some("single") => S::Single,
        Some("compilation") => S::Compilation,
        _ if al.track_count == Some(1) => S::Single,
        _ => S::Album,
    }
}
