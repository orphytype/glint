//! Small public request/response types exposed by [`crate::Client`].

/// How much audio quality to ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Quality {
    /// The smallest files (about 64 kbps AAC).
    Saver,
    /// Up to 192 kbps.
    Balanced,
    /// The highest rendition available for the track: FLAC when offered, else the best lossy stream.
    #[default]
    Best,
}

impl Quality {
    /// The `quality` the signed `get-file-info` endpoint takes.
    pub(crate) fn file_info_name(self) -> &'static str {
        match self {
            Quality::Saver => "nq",
            Quality::Balanced => "hq",
            Quality::Best => "lossless",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum SearchKind {
    #[default]
    All,
    Track,
    Album,
    Artist,
    Playlist,
}

impl SearchKind {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            SearchKind::All => "all",
            SearchKind::Track => "track",
            SearchKind::Album => "album",
            SearchKind::Artist => "artist",
            SearchKind::Playlist => "playlist",
        }
    }
}

/// What besides a track can be liked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Likeable {
    Album,
    Artist,
    Playlist,
}

impl Likeable {
    /// The path segment and the form field the likes endpoints take.
    pub(super) fn names(self) -> (&'static str, &'static str) {
        match self {
            Likeable::Album => ("albums", "album-ids"),
            Likeable::Artist => ("artists", "artist-ids"),
            Likeable::Playlist => ("playlists", "playlist-ids"),
        }
    }
}

/// Where to fetch a track's audio. One URL per signing salt: try them in order, the CDN answers
/// 403 to a wrong signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamInfo {
    pub urls: Vec<String>,
    pub codec: String,
    pub bitrate_kbps: u32,
    pub size_bytes: u64,
}

/// What happened, for the Wave's feedback channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveEvent {
    RadioStarted,
    TrackStarted,
    TrackFinished,
    Skip,
}

impl WaveEvent {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            WaveEvent::RadioStarted => "radioStarted",
            WaveEvent::TrackStarted => "trackStarted",
            WaveEvent::TrackFinished => "trackFinished",
            WaveEvent::Skip => "skip",
        }
    }
}
