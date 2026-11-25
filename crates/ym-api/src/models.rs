//! Domain models. They deserialize straight from the API's JSON (every field is defaulted: the
//! service omits things freely) and are also what the offline demo backend constructs.

use serde::{Deserialize, Deserializer, Serialize};

use crate::cover;

// ---------------------------------------------------------------------------------------------
// Lenient scalar deserializers: ids arrive as numbers or strings depending on the endpoint.

#[derive(Deserialize)]
#[serde(untagged)]
enum Loose {
    S(String),
    I(i64),
    U(u64),
    F(f64),
    B(bool),
}

fn loose_string(v: Loose) -> String {
    match v {
        Loose::S(s) => s,
        Loose::I(i) => i.to_string(),
        Loose::U(u) => u.to_string(),
        Loose::F(f) => format!("{f}"),
        Loose::B(b) => b.to_string(),
    }
}

pub(crate) fn de_id<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(Option::<Loose>::deserialize(d)?.map(loose_string).unwrap_or_default())
}

pub(crate) fn de_opt_id<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(Option::<Loose>::deserialize(d)?.map(loose_string).filter(|s| !s.is_empty()))
}

pub(crate) fn de_i64<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    Ok(match Option::<Loose>::deserialize(d)? {
        Some(Loose::I(i)) => i,
        Some(Loose::U(u)) => u as i64,
        Some(Loose::F(f)) => f as i64,
        Some(Loose::S(s)) => s.trim().parse().unwrap_or(0),
        _ => 0,
    })
}

fn de_vec_lossy<'de, D: Deserializer<'de>, T: serde::de::DeserializeOwned>(d: D) -> Result<Vec<T>, D::Error> {
    // One malformed element must not throw away the whole page of results.
    let raw = Option::<Vec<serde_json::Value>>::deserialize(d)?.unwrap_or_default();
    Ok(raw.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect())
}

fn de_tracks<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Track>, D::Error> {
    de_vec_lossy(d)
}
fn de_albums<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Album>, D::Error> {
    de_vec_lossy(d)
}
fn de_artists<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Artist>, D::Error> {
    de_vec_lossy(d)
}
fn de_playlist_tracks<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<PlaylistTrack>, D::Error> {
    de_vec_lossy(d)
}
fn de_volumes<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Vec<Track>>, D::Error> {
    let raw = Option::<Vec<Vec<serde_json::Value>>>::deserialize(d)?.unwrap_or_default();
    Ok(raw.into_iter().map(|v| v.into_iter().filter_map(|t| serde_json::from_value(t).ok()).collect()).collect())
}

// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ImageRef {
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub uri: Option<String>,
    pub items_uri: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Artist {
    #[serde(deserialize_with = "de_id")]
    pub id: String,
    pub name: String,
    pub cover: Option<ImageRef>,
    pub og_image: Option<String>,
    pub various: bool,
    pub genres: Vec<String>,
    pub stats: Option<ArtistStats>,
    /// What the service says about them (the artist page's "about").
    pub description: Option<ArtistDescription>,
    /// Their own pages: the official site and their social networks.
    pub links: Vec<ArtistLink>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ArtistStats {
    pub last_month_listeners: Option<u64>,
    /// How many more (or, below zero, fewer) than in the 30 days before.
    pub last_month_listeners_delta: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ArtistDescription {
    pub text: String,
    pub uri: Option<String>,
}

/// One of an artist's own pages.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ArtistLink {
    pub title: String,
    pub href: String,
    /// `"official"`, `"social"`, and (for a way to support them) anything with "donat" in it.
    #[serde(rename = "type")]
    pub kind: String,
    /// `"vk"`, `"telegram"`, `"youtube"`, `"twitter"`...
    pub social_network: Option<String>,
}

impl ArtistLink {
    /// Whether the link is a way to give the artist money.
    pub fn is_donation(&self) -> bool {
        let k = self.kind.to_ascii_lowercase();
        let h = self.href.to_ascii_lowercase();
        k.contains("donat") || ["boosty.to", "donationalerts.", "patreon.com", "donatty.", "dalink.to", "tips.yandex", "cloudtips."].iter().any(|d| h.contains(d))
    }
}

impl Artist {
    pub fn cover_template(&self) -> Option<&str> {
        self.cover.as_ref().and_then(|c| c.uri.as_deref()).or(self.og_image.as_deref())
    }

    pub fn cover_url(&self, size: u32) -> Option<String> {
        self.cover_template().map(|t| cover::url(t, size))
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct LyricsInfo {
    pub has_available_sync_lyrics: bool,
    pub has_available_text_lyrics: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Track {
    #[serde(deserialize_with = "de_id")]
    pub id: String,
    #[serde(deserialize_with = "de_opt_id")]
    pub real_id: Option<String>,
    pub title: String,
    pub version: Option<String>,
    pub available: Option<bool>,
    pub duration_ms: Option<u64>,
    #[serde(deserialize_with = "de_artists")]
    pub artists: Vec<Artist>,
    #[serde(deserialize_with = "de_albums")]
    pub albums: Vec<Album>,
    pub cover_uri: Option<String>,
    pub og_image: Option<String>,
    pub lyrics_info: Option<LyricsInfo>,
    pub content_warning: Option<String>,
}

impl Track {
    /// `"Title (Version)"`.
    pub fn full_title(&self) -> String {
        match self.version.as_deref().filter(|v| !v.is_empty()) {
            Some(v) => format!("{} ({v})", self.title),
            None => self.title.clone(),
        }
    }

    /// `"Artist, Artist"`.
    pub fn artists_line(&self) -> String {
        self.artists.iter().map(|a| a.name.as_str()).filter(|n| !n.is_empty()).collect::<Vec<_>>().join(", ")
    }

    pub fn primary_artist(&self) -> &str {
        self.artists.first().map_or("", |a| a.name.as_str())
    }

    pub fn duration_secs(&self) -> f32 {
        self.duration_ms.unwrap_or(0) as f32 / 1000.0
    }

    pub fn is_available(&self) -> bool {
        self.available.unwrap_or(true)
    }

    pub fn is_explicit(&self) -> bool {
        self.content_warning.as_deref() == Some("explicit")
    }

    pub fn album(&self) -> Option<&Album> {
        self.albums.first()
    }

    pub fn cover_template(&self) -> Option<&str> {
        self.cover_uri.as_deref().or(self.og_image.as_deref()).or_else(|| self.albums.first().and_then(|a| a.cover_template()))
    }

    pub fn cover_url(&self, size: u32) -> Option<String> {
        self.cover_template().map(|t| cover::url(t, size))
    }

    /// The id the batch endpoints want: `track:album`.
    pub fn compound_id(&self) -> String {
        match self.albums.first() {
            Some(a) if !a.id.is_empty() => format!("{}:{}", self.id, a.id),
            _ => self.id.clone(),
        }
    }

    pub fn has_synced_lyrics(&self) -> bool {
        self.lyrics_info.as_ref().is_some_and(|l| l.has_available_sync_lyrics)
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Album {
    #[serde(deserialize_with = "de_id")]
    pub id: String,
    pub title: String,
    pub version: Option<String>,
    pub year: Option<i32>,
    pub release_date: Option<String>,
    pub cover_uri: Option<String>,
    pub og_image: Option<String>,
    pub genre: Option<String>,
    pub track_count: Option<u32>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    #[serde(deserialize_with = "de_artists")]
    pub artists: Vec<Artist>,
    #[serde(deserialize_with = "de_volumes")]
    pub volumes: Vec<Vec<Track>>,
    /// The labels that put it out, by name (the API gives objects or bare names).
    #[serde(deserialize_with = "de_labels")]
    pub labels: Vec<String>,
    pub likes_count: Option<u64>,
}

/// A label list: `[{"id": 1, "name": "x"}]` or `["x"]`; anything else is skipped.
fn de_labels<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    let items = v.as_array().cloned().unwrap_or_default();
    Ok(items
        .iter()
        .filter_map(|l| l.as_str().map(str::to_owned).or_else(|| l.get("name").and_then(serde_json::Value::as_str).map(str::to_owned)))
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty() && n != "-")
        .collect())
}

/// One line of a track's credits, as the service words it ("Автор музыки": "...").
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Credit {
    pub title: String,
    pub value: String,
}

/// A link out to a page about the artist (band.link's list of the playlists they are in).
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct OutLink {
    pub title: String,
    pub subtitle: Option<String>,
    pub url: String,
}

impl Album {
    pub fn artists_line(&self) -> String {
        self.artists.iter().map(|a| a.name.as_str()).filter(|n| !n.is_empty()).collect::<Vec<_>>().join(", ")
    }

    pub fn cover_template(&self) -> Option<&str> {
        self.cover_uri.as_deref().or(self.og_image.as_deref())
    }

    pub fn cover_url(&self, size: u32) -> Option<String> {
        self.cover_template().map(|t| cover::url(t, size))
    }

    /// Every track of every volume, in order.
    pub fn tracks(&self) -> impl Iterator<Item = &Track> {
        self.volumes.iter().flatten()
    }

    pub fn subtitle(&self) -> String {
        let mut parts = vec![self.artists_line()];
        if let Some(y) = self.year {
            parts.push(y.to_string());
        }
        parts.retain(|p| !p.is_empty());
        parts.join(" · ")
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Owner {
    #[serde(deserialize_with = "de_i64")]
    pub uid: i64,
    pub login: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PlaylistTrack {
    #[serde(deserialize_with = "de_id")]
    pub id: String,
    pub track: Option<Track>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Playlist {
    #[serde(deserialize_with = "de_i64")]
    pub uid: i64,
    #[serde(deserialize_with = "de_i64")]
    pub kind: i64,
    pub title: String,
    pub description: Option<String>,
    pub track_count: Option<u32>,
    pub duration_ms: Option<u64>,
    pub cover: Option<ImageRef>,
    pub og_image: Option<String>,
    pub owner: Option<Owner>,
    #[serde(deserialize_with = "de_playlist_tracks")]
    pub tracks: Vec<PlaylistTrack>,
    pub generated_playlist_type: Option<String>,
    /// The edit counter; track edits must quote the current one.
    #[serde(deserialize_with = "de_i64")]
    pub revision: i64,
    /// `"public"` or `"private"`.
    pub visibility: Option<String>,
    pub modified: Option<String>,
    pub created: Option<String>,
}

impl Playlist {
    /// Stable key for lists and caches.
    pub fn key(&self) -> String {
        let uid = if self.uid != 0 { self.uid } else { self.owner.as_ref().map_or(0, |o| o.uid) };
        format!("{uid}:{}", self.kind)
    }

    pub fn owner_uid(&self) -> i64 {
        if self.uid != 0 { self.uid } else { self.owner.as_ref().map_or(0, |o| o.uid) }
    }

    pub fn cover_template(&self) -> Option<&str> {
        self.cover.as_ref().and_then(|c| c.uri.as_deref().or_else(|| c.items_uri.first().map(String::as_str))).or(self.og_image.as_deref())
    }

    pub fn cover_url(&self, size: u32) -> Option<String> {
        self.cover_template().map(|t| cover::url(t, size))
    }

    pub fn resolved_tracks(&self) -> impl Iterator<Item = &Track> {
        self.tracks.iter().filter_map(|t| t.track.as_ref())
    }
}

/// One page of search hits (built by the lossy section deserializers below).
#[derive(Debug, Clone)]
pub struct Page<T> {
    pub total: u32,
    pub per_page: u32,
    pub results: Vec<T>,
}

impl<T> Default for Page<T> {
    fn default() -> Self {
        Page { total: 0, per_page: 0, results: Vec::new() }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchResult {
    pub text: String,
    #[serde(deserialize_with = "de_tracks_page")]
    pub tracks: Page<Track>,
    #[serde(deserialize_with = "de_albums_page")]
    pub albums: Page<Album>,
    #[serde(deserialize_with = "de_artists_page")]
    pub artists: Page<Artist>,
    #[serde(deserialize_with = "de_playlists_page")]
    pub playlists: Page<Playlist>,
}

macro_rules! lossy_page {
    ($name:ident, $t:ty) => {
        fn $name<'de, D: Deserializer<'de>>(d: D) -> Result<Page<$t>, D::Error> {
            #[derive(Deserialize, Default)]
            #[serde(rename_all = "camelCase", default)]
            struct Raw {
                total: u32,
                per_page: u32,
                results: Vec<serde_json::Value>,
            }
            let raw = Option::<Raw>::deserialize(d)?.unwrap_or_default();
            Ok(Page { total: raw.total, per_page: raw.per_page, results: raw.results.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect() })
        }
    };
}
lossy_page!(de_tracks_page, Track);
lossy_page!(de_albums_page, Album);
lossy_page!(de_artists_page, Artist);
lossy_page!(de_playlists_page, Playlist);

impl SearchResult {
    pub fn is_empty(&self) -> bool {
        self.tracks.results.is_empty() && self.albums.results.is_empty() && self.artists.results.is_empty() && self.playlists.results.is_empty()
    }
}

/// Everything the artist page needs.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ArtistPage {
    pub artist: Artist,
    #[serde(deserialize_with = "de_tracks")]
    pub popular_tracks: Vec<Track>,
    #[serde(deserialize_with = "de_albums")]
    pub albums: Vec<Album>,
    #[serde(deserialize_with = "de_artists")]
    pub similar_artists: Vec<Artist>,
    /// The listeners over the last month (the brief info keeps them beside the artist, not in it).
    pub stats: Option<ArtistStats>,
    /// Their photos (the first is the one they are shown by).
    pub all_covers: Vec<ImageRef>,
    /// The playlists they are in, as band.link lists them.
    pub bandlink_scanner_link: Option<OutLink>,
    /// A way to support the artist, when they have one set up.
    #[serde(skip)]
    pub donation: Option<String>,
}

impl ArtistPage {
    /// How many listened to them over the last month, wherever the answer put it.
    pub fn listeners(&self) -> Option<u64> {
        self.stats.as_ref().and_then(|s| s.last_month_listeners).or_else(|| self.artist.stats.as_ref().and_then(|s| s.last_month_listeners))
    }

    /// How the listeners changed against the 30 days before.
    pub fn listeners_delta(&self) -> Option<i64> {
        self.stats.as_ref().and_then(|s| s.last_month_listeners_delta).or_else(|| self.artist.stats.as_ref().and_then(|s| s.last_month_listeners_delta))
    }

    /// The photos' templates, each once.
    pub fn photos(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for c in &self.all_covers {
            if let Some(u) = c.uri.as_deref().filter(|u| !u.is_empty())
                && !out.iter().any(|o| o == u)
            {
                out.push(u.to_owned());
            }
        }
        out
    }
}

/// The link to support the artist in a brief-info answer: a `donation` object (beside the artist
/// or in it) with a URL in it, or one of their links that is one.
pub(crate) fn donation_url(v: &serde_json::Value, artist: &Artist) -> Option<String> {
    let url_in = |d: &serde_json::Value| -> Option<String> {
        if let Some(s) = d.as_str() {
            return s.starts_with("http").then(|| s.to_owned());
        }
        ["url", "href", "link", "uri"].iter().find_map(|k| d.get(*k).and_then(serde_json::Value::as_str)).filter(|s| s.starts_with("http")).map(str::to_owned)
    };
    for d in [v.get("donation"), v.pointer("/artist/donation"), v.get("donations"), v.pointer("/artist/donations")].into_iter().flatten() {
        if let Some(u) = url_in(d).or_else(|| d.as_array().and_then(|a| a.iter().find_map(url_in))) {
            return Some(u);
        }
    }
    artist.links.iter().find(|l| l.is_donation()).map(|l| l.href.clone())
}

/// The signed-in account.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Account {
    pub uid: i64,
    pub login: String,
    pub display_name: String,
    pub has_plus: bool,
}

/// One entry of the liked-tracks list: just ids until hydrated with [`crate::Client::tracks`].
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct TrackRef {
    #[serde(deserialize_with = "de_id")]
    pub id: String,
    #[serde(deserialize_with = "de_opt_id")]
    pub album_id: Option<String>,
    pub timestamp: Option<String>,
}

/// One audio rendition offered for a track.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct DownloadInfo {
    pub codec: String,
    pub bitrate_in_kbps: u32,
    pub gain: bool,
    pub preview: bool,
    pub download_info_url: String,
    pub direct: bool,
}

/// One batch of the infinite "Wave" stream.
#[derive(Debug, Clone, Default)]
pub struct RotorBatch {
    pub batch_id: Option<String>,
    pub tracks: Vec<Track>,
}

/// Which landing-page shelf a [`Shelf`] is. The UI owns the (localised) titles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum ShelfKind {
    /// "Playlist of the day", "Premiere", "Deja vu" ...
    Personal,
    /// Curated playlists.
    Editorial,
    NewReleases,
    Chart,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Shelf {
    pub kind: ShelfKind,
    pub items: Vec<ShelfItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum ShelfItem {
    Playlist(Playlist),
    Album(Album),
    Track(Track),
}

/// Deserializes every element of a JSON array that parses, dropping those that do not.
pub(crate) fn lossy_vec<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> Vec<T> {
    match v {
        serde_json::Value::Array(items) => items.into_iter().filter_map(|i| serde_json::from_value(i).ok()).collect(),
        _ => Vec::new(),
    }
}

/// Removes every `null` object member, so that `#[serde(default)]` treats "null" exactly like
/// "absent" (a plain `String` field would otherwise reject `"title": null` and the whole item
/// would be lost). Applied once to every response.
pub(crate) fn strip_nulls(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(map) => {
            map.retain(|_, member| !member.is_null());
            map.values_mut().for_each(strip_nulls);
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(strip_nulls),
        _ => {}
    }
}
