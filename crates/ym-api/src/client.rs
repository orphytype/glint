//! The HTTP client. Every network call the app makes lives in this file, so "what does this app
//! send where" is answered by reading one module:
//!
//! * the OAuth token is attached to requests for `api.music.yandex.net` (see [`Client::api`])
//!   and to one request to Yandex's own sign-in service, `login.yandex.ru/info`, which says which
//!   picture the account uses (see [`Client::avatar`]); audio, lyrics, artwork, LRCLIB and
//!   lrc.red requests never carry it;
//! * lrc.red is asked only when the person has given a contact for its User-Agent (its terms
//!   require one); the track's title and artist go in the search query, nothing else does;
//! * redirects are followed only to Yandex hosts, `lrclib.net` and `lrc.red`, and URLs that came
//!   out of a response are checked against [`cover::is_trusted_host`] before they are fetched;
//! * bodies are size-limited, ids that end up in a URL path are validated;
//! * responses are parsed leniently: the service omits fields freely, so one odd element never
//!   costs the whole page.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::header::{ACCEPT_LANGUAGE, AUTHORIZATION, HeaderValue, USER_AGENT};
use reqwest::{Method, RequestBuilder};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::lrc::{Lyrics, LyricsSource};
use crate::lrcred;
use crate::models::*;
use crate::{Error, Result, cover, sign};

const API: &str = "https://api.music.yandex.net";
/// Yandex ID's "who am I" endpoint: the account's picture (the music API does not give it).
const PASSPORT_INFO: &str = "https://login.yandex.ru/info?format=json";
const LRCLIB: &str = "https://lrclib.net/api";
const LRCRED: &str = lrcred::HOME;
/// What the service expects its desktop client to call itself; every open-source client sends it.
const CLIENT_ID: &str = "YandexMusicDesktopAppWindows/5.29.1";
const APP_UA: &str = concat!("Glint/", env!("CARGO_PKG_VERSION"), " (unofficial client)");

const MAX_JSON_BYTES: u64 = 64 << 20;
const MAX_AUDIO_BYTES: u64 = 256 << 20;
const MAX_IMAGE_BYTES: u64 = 16 << 20;
const MAX_TEXT_BYTES: u64 = 4 << 20;
/// How many ids one batch request carries.
const BATCH: usize = 100;

// ---------------------------------------------------------------------------------------------
// Small public types

/// How much audio quality to ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Quality {
    /// The smallest files (about 64 kbps AAC).
    Saver,
    /// Up to 192 kbps.
    Balanced,
    /// The best the service offers (320 kbps MP3).
    #[default]
    Best,
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

/// What besides a track can be liked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Likeable {
    Album,
    Artist,
    Playlist,
}

impl Likeable {
    /// The path segment and the form field the likes endpoints take.
    fn names(self) -> (&'static str, &'static str) {
        match self {
            Likeable::Album => ("albums", "album-ids"),
            Likeable::Artist => ("artists", "artist-ids"),
            Likeable::Playlist => ("playlists", "playlist-ids"),
        }
    }
}

impl SearchKind {
    fn as_str(self) -> &'static str {
        match self {
            SearchKind::All => "all",
            SearchKind::Track => "track",
            SearchKind::Album => "album",
            SearchKind::Artist => "artist",
            SearchKind::Playlist => "playlist",
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
    fn as_str(self) -> &'static str {
        match self {
            WaveEvent::RadioStarted => "radioStarted",
            WaveEvent::TrackStarted => "trackStarted",
            WaveEvent::TrackFinished => "trackFinished",
            WaveEvent::Skip => "skip",
        }
    }
}

// ---------------------------------------------------------------------------------------------

struct Inner {
    http: reqwest::Client,
    token: RwLock<Option<String>>,
    lang: RwLock<String>,
    /// The User-Agent for lrc.red; `None` keeps lrc.red switched off.
    lrcred_ua: RwLock<Option<String>>,
    /// What lrc.red said for a track id this session, so a track is asked about once.
    lrcred_seen: Mutex<HashMap<String, Option<Lyrics>>>,
}

/// How many lrc.red answers to remember.
const LRCRED_SEEN_MAX: usize = 64;

/// Cheap to clone; clones share the connection pool, the token and the language.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

impl Client {
    pub fn new() -> Result<Client> {
        // rustls needs its crypto chosen once per process (Windows uses SChannel instead).
        #[cfg(not(windows))]
        {
            static PROVIDER: std::sync::Once = std::sync::Once::new();
            PROVIDER.call_once(|| {
                let _ = rustls::crypto::ring::default_provider().install_default();
            });
        }
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30))
            .pool_idle_timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                let next = attempt.url();
                let trusted = cover::is_trusted_host(next.as_str()) || matches!(next.host_str(), Some("lrclib.net" | "lrc.red" | "raw.githubusercontent.com"));
                if attempt.previous().len() >= 5 {
                    attempt.error("too many redirects")
                } else if trusted {
                    attempt.follow()
                } else {
                    attempt.stop()
                }
            }))
            .build()?;
        Ok(Client {
            inner: Arc::new(Inner { http, token: RwLock::new(None), lang: RwLock::new("ru".into()), lrcred_ua: RwLock::new(None), lrcred_seen: Mutex::new(HashMap::new()) }),
        })
    }

    /// Sets (or clears) the OAuth token. Pass it through [`crate::auth::extract_token`] first.
    pub fn set_token(&self, token: Option<String>) {
        *self.inner.token.write().unwrap_or_else(PoisonError::into_inner) = token.filter(|t| !t.is_empty());
    }

    pub fn has_token(&self) -> bool {
        self.token().is_some()
    }

    /// `"ru"` or `"en"`: the language the service writes playlist titles and the like in.
    pub fn set_language(&self, lang: &str) {
        *self.inner.lang.write().unwrap_or_else(PoisonError::into_inner) = lang.to_owned();
    }

    /// Turns word-by-word lyrics from lrc.red on or off. Its terms ask every client to send a
    /// User-Agent with a way to reach whoever runs it, so it stays off until `contact` (a link, an
    /// address) is given; an unusable contact switches it off too.
    pub fn set_lrcred_contact(&self, contact: Option<&str>) {
        let ua = contact.and_then(|c| lrcred::user_agent(env!("CARGO_PKG_VERSION"), c));
        *self.inner.lrcred_ua.write().unwrap_or_else(PoisonError::into_inner) = ua;
        self.inner.lrcred_seen.lock().unwrap_or_else(PoisonError::into_inner).clear();
    }

    pub fn lrcred_enabled(&self) -> bool {
        self.inner.lrcred_ua.read().unwrap_or_else(PoisonError::into_inner).is_some()
    }

    fn token(&self) -> Option<String> {
        self.inner.token.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// A request to the music API: the only place the token is attached.
    fn api(&self, method: Method, path: &str) -> RequestBuilder {
        let lang = self.inner.lang.read().unwrap_or_else(PoisonError::into_inner).clone();
        let mut rb = self
            .inner
            .http
            .request(method, format!("{API}{path}"))
            .header(USER_AGENT, CLIENT_ID)
            .header("X-Yandex-Music-Client", CLIENT_ID)
            .header(ACCEPT_LANGUAGE, lang)
            .timeout(Duration::from_secs(25));
        if let Some(token) = self.token()
            && let Ok(mut value) = HeaderValue::from_str(&format!("OAuth {token}"))
        {
            value.set_sensitive(true);
            rb = rb.header(AUTHORIZATION, value);
        }
        rb
    }

    async fn call_once(&self, rb: RequestBuilder) -> Result<Value> {
        let resp = rb.send().await?;
        let status = resp.status().as_u16();
        let body = read_limited(resp, MAX_JSON_BYTES, |_, _| {}).await?;
        parse_envelope(status, &String::from_utf8_lossy(&body))
    }

    /// Sends the request and returns the envelope's `result`; transient failures get one retry.
    async fn call(&self, rb: RequestBuilder) -> Result<Value> {
        let again = rb.try_clone();
        match self.call_once(rb).await {
            Err(e) if e.is_transient() => match again {
                Some(rb) => {
                    tokio::time::sleep(Duration::from_millis(400)).await;
                    self.call_once(rb).await
                }
                None => Err(e),
            },
            other => other,
        }
    }

    async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        let rb = self.api(Method::GET, path);
        self.call(if query.is_empty() { rb } else { rb.query(query) }).await
    }

    async fn post_form(&self, path: &str, form: &[(&str, &str)]) -> Result<Value> {
        self.call(self.api(Method::POST, path).form(form)).await
    }

    /// A plain GET of a (checked) absolute URL, without credentials.
    async fn fetch(&self, url: &str, max: u64, timeout_secs: u64, mut progress: impl FnMut(u64, Option<u64>)) -> Result<Vec<u8>> {
        let resp = self.inner.http.get(url).header(USER_AGENT, CLIENT_ID).timeout(Duration::from_secs(timeout_secs)).send().await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(Error::Http { status: status.as_u16(), body: String::new() });
        }
        read_limited(resp, max, &mut progress).await
    }

    async fn fetch_text(&self, url: &str) -> Result<String> {
        let bytes = self.fetch(url, MAX_TEXT_BYTES, 20, |_, _| {}).await?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    // -----------------------------------------------------------------------------------------
    // Account and library

    /// Who the token belongs to. Doubles as the "is this token any good?" check.
    pub async fn account(&self) -> Result<Account> {
        let v = self.get("/account/status", &[]).await?;
        let acct = v.get("account").ok_or_else(|| Error::Parse("account/status has no account".into()))?;
        let uid = as_i64(acct.get("uid"));
        if uid == 0 {
            return Err(Error::Unauthorized);
        }
        let text = |k: &str| acct.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
        let login = text("login").unwrap_or_default();
        let display_name = text("displayName").or_else(|| text("fullName")).unwrap_or_else(|| login.clone());
        let has_plus = v.pointer("/plus/hasPlus").and_then(Value::as_bool).unwrap_or(false);
        Ok(Account { uid, login, display_name, has_plus })
    }

    /// The liked tracks as bare references, newest first. Hydrate pages of them with
    /// [`Client::tracks`].
    pub async fn liked_track_refs(&self, uid: i64) -> Result<Vec<TrackRef>> {
        let mut v = self.get(&format!("/users/{uid}/likes/tracks"), &[]).await?;
        let raw = v.pointer_mut("/library/tracks").map(Value::take).unwrap_or(Value::Null);
        let mut refs: Vec<TrackRef> = lossy_vec(raw);
        // Timestamps are ISO-8601 strings, so they sort as text; an unstamped list keeps its order.
        refs.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        Ok(refs)
    }

    pub async fn liked_albums(&self, uid: i64) -> Result<Vec<Album>> {
        let v = self.get(&format!("/users/{uid}/likes/albums"), &[("rich", "true")]).await?;
        Ok(unwrap_items(v, "album"))
    }

    pub async fn liked_artists(&self, uid: i64) -> Result<Vec<Artist>> {
        let v = self.get(&format!("/users/{uid}/likes/artists"), &[("with-timestamps", "false")]).await?;
        Ok(unwrap_items(v, "artist"))
    }

    pub async fn playlists(&self, uid: i64) -> Result<Vec<Playlist>> {
        let v = self.get(&format!("/users/{uid}/playlists/list"), &[]).await?;
        Ok(lossy_vec(v))
    }

    /// A playlist with all its tracks resolved.
    pub async fn playlist(&self, uid: i64, kind: i64) -> Result<Playlist> {
        let v = self.get(&format!("/users/{uid}/playlists/{kind}"), &[("rich-tracks", "true")]).await?;
        let mut playlist: Playlist = from_value(v, "playlist")?;
        if playlist.uid == 0 {
            playlist.uid = uid;
        }
        let missing: Vec<String> = playlist.tracks.iter().filter(|t| t.track.is_none() && !t.id.is_empty()).map(|t| t.id.clone()).collect();
        if !missing.is_empty() {
            let resolved: HashMap<String, Track> = self.tracks(&missing).await?.into_iter().map(|t| (t.id.clone(), t)).collect();
            for entry in &mut playlist.tracks {
                if entry.track.is_none() {
                    entry.track = resolved.get(entry.id.split(':').next().unwrap_or("")).cloned();
                }
            }
        }
        Ok(playlist)
    }

    pub async fn album(&self, id: &str) -> Result<Album> {
        let v = self.get(&format!("/albums/{}/with-tracks", seg(id)?), &[]).await?;
        from_value(v, "album")
    }

    pub async fn albums(&self, ids: &[String]) -> Result<Vec<Album>> {
        let mut out = Vec::new();
        for chunk in ids.chunks(BATCH) {
            let pairs = id_pairs("album-ids", chunk)?;
            let form: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
            out.extend(lossy_vec::<Album>(self.post_form("/albums", &form).await?));
        }
        Ok(out)
    }

    /// Every track of an artist (the first `limit`), most popular first.
    pub async fn artist_tracks(&self, id: &str, limit: u32) -> Result<Vec<Track>> {
        let size = limit.clamp(1, 500).to_string();
        let mut v = self.get(&format!("/artists/{}/tracks", seg(id)?), &[("page", "0"), ("page-size", size.as_str())]).await?;
        let raw = v.get_mut("tracks").map(Value::take).unwrap_or(Value::Null);
        Ok(lossy_vec(raw))
    }

    /// The picture of the signed-in account, as a URL on Yandex's avatar CDN; `None` when the
    /// account has none (or the service will not say). The token goes to Yandex ID here, the
    /// service that issued it, and to nothing else.
    pub async fn avatar(&self) -> Result<Option<String>> {
        let Some(token) = self.token() else { return Ok(None) };
        let Ok(mut value) = HeaderValue::from_str(&format!("OAuth {token}")) else { return Ok(None) };
        value.set_sensitive(true);
        let resp = self.inner.http.get(PASSPORT_INFO).header(USER_AGENT, APP_UA).header(AUTHORIZATION, value).timeout(Duration::from_secs(15)).send().await?;
        if !resp.status().is_success() {
            return Ok(None);
        }
        let body = read_limited(resp, MAX_TEXT_BYTES, |_, _| {}).await?;
        let v: Value = serde_json::from_slice(&body).map_err(|e| Error::Parse(e.to_string()))?;
        Ok(avatar_url(&v))
    }

    pub async fn artist(&self, id: &str) -> Result<ArtistPage> {
        let v = self.get(&format!("/artists/{}/brief-info", seg(id)?), &[]).await?;
        from_value(v, "artist page")
    }

    /// Full track objects for ids (`"123"` or `"123:456"`), in the order asked for.
    pub async fn tracks(&self, ids: &[String]) -> Result<Vec<Track>> {
        let mut out: Vec<Track> = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(BATCH) {
            let pairs = id_pairs("track-ids", chunk)?;
            let form: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
            out.extend(lossy_vec::<Track>(self.post_form("/tracks", &form).await?));
        }
        let order: HashMap<&str, usize> = ids.iter().enumerate().rev().map(|(i, id)| (id.split(':').next().unwrap_or(id), i)).collect();
        out.sort_by_key(|t| order.get(t.id.as_str()).copied().unwrap_or(usize::MAX));
        Ok(out)
    }

    pub async fn like_tracks(&self, uid: i64, ids: &[String]) -> Result<()> {
        self.track_list_action(&format!("/users/{uid}/likes/tracks/add-multiple"), ids).await
    }

    pub async fn unlike_tracks(&self, uid: i64, ids: &[String]) -> Result<()> {
        self.track_list_action(&format!("/users/{uid}/likes/tracks/remove"), ids).await
    }

    pub async fn dislike_tracks(&self, uid: i64, ids: &[String]) -> Result<()> {
        self.track_list_action(&format!("/users/{uid}/dislikes/tracks/add-multiple"), ids).await
    }

    /// Likes (or, with `like` false, unlikes) an album, an artist or a playlist (`"uid:kind"`).
    pub async fn set_item_like(&self, uid: i64, what: Likeable, id: &str, like: bool) -> Result<()> {
        let (plural, key) = what.names();
        let path = format!("/users/{uid}/likes/{plural}/{}", if like { "add-multiple" } else { "remove" });
        let id = if what == Likeable::Playlist { id.to_owned() } else { seg(id)?.to_owned() };
        self.post_form(&path, &[(key, id.as_str())]).await.map(drop)
    }

    /// The playlists the account liked, each as `"uid:kind"`.
    pub async fn liked_playlist_ids(&self, uid: i64) -> Result<Vec<String>> {
        let v = self.get(&format!("/users/{uid}/likes/playlists"), &[]).await?;
        let lists: Vec<Playlist> = unwrap_items(v, "playlist");
        Ok(lists.into_iter().map(|p| format!("{}:{}", p.uid, p.kind)).collect())
    }

    async fn track_list_action(&self, path: &str, ids: &[String]) -> Result<()> {
        let pairs = id_pairs("track-ids", ids)?;
        let form: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.post_form(path, &form).await.map(drop)
    }

    // -----------------------------------------------------------------------------------------
    // Search

    pub async fn search(&self, text: &str, kind: SearchKind, page: u32) -> Result<SearchResult> {
        let page = page.to_string();
        let v = self.get("/search", &[("text", text), ("type", kind.as_str()), ("page", &page), ("nocorrect", "false"), ("playlist-in-best", "true")]).await?;
        from_value(v, "search results")
    }

    pub async fn suggest(&self, part: &str) -> Result<Vec<String>> {
        let v = self.get("/search/suggest", &[("part", part)]).await?;
        Ok(v.get("suggestions").and_then(Value::as_array).into_iter().flatten().filter_map(|s| s.as_str().map(str::to_owned)).collect())
    }

    // -----------------------------------------------------------------------------------------
    // Home

    /// The home-page shelves. Each shelf is fetched independently; one failing does not hide the
    /// others, and an error is returned only when nothing at all came back.
    pub async fn home(&self) -> Result<Vec<Shelf>> {
        let (personal, editorial, releases, chart) = tokio::join!(self.personal_playlists(), self.editorial_playlists(), self.new_releases(), self.chart());
        let mut shelves = Vec::new();
        let mut first_err: Option<Error> = None;
        let mut add = |kind: ShelfKind, result: Result<Vec<ShelfItem>>| match result {
            Ok(items) if !items.is_empty() => shelves.push(Shelf { kind, items }),
            Ok(_) => {}
            Err(e) => {
                first_err.get_or_insert(e);
            }
        };
        add(ShelfKind::Personal, personal.map(|v| v.into_iter().map(ShelfItem::Playlist).collect()));
        add(ShelfKind::Editorial, editorial.map(|v| v.into_iter().map(ShelfItem::Playlist).collect()));
        add(ShelfKind::NewReleases, releases.map(|v| v.into_iter().map(ShelfItem::Album).collect()));
        add(ShelfKind::Chart, chart.map(|v| v.into_iter().map(ShelfItem::Track).collect()));
        match first_err {
            Some(e) if shelves.is_empty() => Err(e),
            _ => Ok(shelves),
        }
    }

    async fn personal_playlists(&self) -> Result<Vec<Playlist>> {
        match self.get("/feed", &[]).await {
            Ok(v) => {
                let found = generated_playlists(&v);
                if !found.is_empty() {
                    return Ok(found);
                }
            }
            Err(Error::Unauthorized) => return Err(Error::Unauthorized),
            Err(_) => {}
        }
        let v = self.get("/landing3", &[("blocks", "personal-playlists")]).await?;
        Ok(landing_playlists(&v))
    }

    async fn editorial_playlists(&self) -> Result<Vec<Playlist>> {
        let v = self.get("/landing3", &[("blocks", "new-playlists")]).await?;
        Ok(landing_playlists(&v))
    }

    async fn new_releases(&self) -> Result<Vec<Album>> {
        if let Ok(v) = self.get("/landing3/new-releases", &[]).await {
            let ids: Vec<String> = v.get("newReleases").and_then(Value::as_array).into_iter().flatten().take(40).map(as_id).filter(|s| !s.is_empty()).collect();
            if !ids.is_empty()
                && let Ok(albums) = self.albums(&ids).await
                && !albums.is_empty()
            {
                return Ok(albums);
            }
        }
        let v = self.get("/landing3", &[("blocks", "new-releases")]).await?;
        Ok(landing_albums(&v))
    }

    async fn chart(&self) -> Result<Vec<Track>> {
        let v = self.get("/landing3", &[("blocks", "chart")]).await?;
        Ok(chart_tracks(&v))
    }

    // -----------------------------------------------------------------------------------------
    // Wave

    /// The next batch of a Wave station (`"user:onyourwave"`). Pass the id of the last track you
    /// were given as `queue` to continue the stream.
    pub async fn wave_tracks(&self, station: &str, seeds: &[&str], queue: Option<&str>) -> Result<RotorBatch> {
        let mut query: Vec<(&str, &str)> = seeds.iter().map(|s| ("seeds", *s)).collect();
        if let Some(q) = queue.filter(|q| !q.is_empty()) {
            query.push(("queue", q));
        }
        let v = self.get(&format!("/rotor/station/{}/tracks", seg(station)?), &query).await?;
        Ok(parse_rotor(v))
    }

    /// Tells the Wave what you did, which is how it learns your taste. Best-effort.
    pub async fn wave_feedback(&self, station: &str, event: WaveEvent, track_id: Option<&str>, batch_id: Option<&str>, played_secs: Option<f64>) -> Result<()> {
        let mut body = json!({ "type": event.as_str(), "timestamp": iso8601(unix_millis()) });
        let map = body.as_object_mut().expect("json! object");
        if let Some(t) = track_id {
            map.insert("trackId".into(), json!(t));
        }
        if let Some(b) = batch_id {
            map.insert("batchId".into(), json!(b));
        }
        if let Some(s) = played_secs {
            map.insert("totalPlayedSeconds".into(), json!(s));
        }
        self.call(self.api(Method::POST, &format!("/rotor/station/{}/feedback", seg(station)?)).json(&body)).await.map(drop)
    }

    // -----------------------------------------------------------------------------------------
    // Audio, artwork, lyrics

    /// Resolves the audio URL(s) for a track.
    pub async fn stream(&self, track_id: &str, quality: Quality) -> Result<StreamInfo> {
        let v = self.get(&format!("/tracks/{}/download-info", seg(track_id)?), &[]).await?;
        let infos: Vec<DownloadInfo> = lossy_vec(v);
        let pick = pick_download_info(&infos, quality).ok_or(Error::RegionRestricted)?;
        let info_url = https(&pick.download_info_url);
        if !cover::is_trusted_host(&info_url) {
            return Err(Error::Parse("download-info points outside Yandex".into()));
        }
        let xml = self.fetch_text(&info_url).await?;
        let parsed = sign::parse_download_xml(&xml)?;
        Ok(StreamInfo { urls: sign::STREAM_SALTS.iter().map(|salt| sign::direct_url(&parsed, salt)).collect(), codec: pick.codec.clone(), bitrate_kbps: pick.bitrate_in_kbps })
    }

    /// Downloads the audio file. `progress(downloaded, total)` is called as chunks arrive.
    pub async fn fetch_audio(&self, info: &StreamInfo, mut progress: impl FnMut(u64, Option<u64>)) -> Result<Vec<u8>> {
        let mut last = Error::Api("no stream URL".into());
        for url in &info.urls {
            if !cover::is_trusted_host(url) {
                continue;
            }
            match self.fetch(url, MAX_AUDIO_BYTES, 120, &mut progress).await {
                Ok(bytes) => return Ok(bytes),
                // A wrong signature is a 403: the next salt may be the right one.
                Err(e @ Error::Http { .. }) => last = e,
                Err(e) => return Err(e),
            }
        }
        Err(last)
    }

    /// Loads the public FkCensorData map without sending the account token.
    pub async fn fkcensor_replacements(&self) -> Result<HashMap<String, crate::fkcensor::Replacement>> {
        crate::fkcensor::fetch_list(&self.inner.http).await
    }

    /// Downloads a replacement audio file from the pinned FkCensorData host.
    pub async fn fetch_fkcensor_audio(&self, url: &str) -> Result<Vec<u8>> {
        if !url.starts_with("https://raw.githubusercontent.com/") {
            return Err(Error::Parse("replacement audio points outside GitHub raw".into()));
        }
        self.fetch(url, MAX_AUDIO_BYTES, 120, |_, _| {}).await
    }

    /// Fetches artwork (from Yandex hosts only).
    pub async fn fetch_image(&self, url: &str) -> Result<Vec<u8>> {
        if !cover::is_trusted_host(url) {
            return Err(Error::Api("refusing to fetch an image from an unexpected host".into()));
        }
        self.fetch(url, MAX_IMAGE_BYTES, 20, |_, _| {}).await
    }

    /// Lyrics: word-timed ones from lrc.red when that is switched on, then the service's own
    /// (when signed in), then LRCLIB. `Ok` with an empty [`Lyrics`] means "none exist".
    pub async fn lyrics(&self, track: &Track) -> Result<Lyrics> {
        if let Some(found) = self.lrcred_lyrics(track).await.filter(|l| !l.is_empty()) {
            return Ok(found);
        }
        if self.has_token()
            && !track.id.is_empty()
            && let Ok(Some(found)) = self.yandex_lyrics(&track.id).await
        {
            return Ok(found);
        }
        Ok(self.lrclib(track).await?.unwrap_or_default())
    }

    /// lrc.red's word-timed lyrics, or `None` (off, no match, or any failure: the other sources
    /// are the fallback, so nothing here is worth an error).
    async fn lrcred_lyrics(&self, track: &Track) -> Option<Lyrics> {
        let ua = self.inner.lrcred_ua.read().unwrap_or_else(PoisonError::into_inner).clone()?;
        if track.title.is_empty() || track.id.is_empty() {
            return None;
        }
        let key = track.id.clone();
        if let Some(seen) = self.inner.lrcred_seen.lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
            return seen.clone();
        }
        let found = self.lrcred_lookup(track, &ua).await;
        // A transport failure is not an answer: only remember what lrc.red actually said.
        if let Ok(found) = &found {
            let mut seen = self.inner.lrcred_seen.lock().unwrap_or_else(PoisonError::into_inner);
            if seen.len() >= LRCRED_SEEN_MAX {
                seen.clear();
            }
            seen.insert(key, found.clone());
        }
        found.ok().flatten()
    }

    async fn lrcred_lookup(&self, track: &Track, ua: &str) -> Result<Option<Lyrics>> {
        let q = format!("{} {}", track.title, track.primary_artist()).trim().to_owned();
        let Some(body) = self.lrcred_get(ua, "/search.json", &[("q", q.as_str())]).await? else { return Ok(None) };
        let hits: Vec<lrcred::Hit> = serde_json::from_slice::<Value>(&body)
            .ok()
            .and_then(|mut v| {
                strip_nulls(&mut v);
                v.get("hits").cloned()
            })
            .map(lossy_vec)
            .unwrap_or_default();
        let Some(hit) = best_lrcred_hit(&hits, track) else { return Ok(None) };
        let Some(ttml) = self.lrcred_get(ua, &format!("/s/{}.ttml", hit.isrc), &[]).await? else { return Ok(None) };
        Ok(lrcred::parse_ttml(&String::from_utf8_lossy(&ttml)).map(|l| Lyrics { link: Some(lrcred::page_url(&hit.isrc)), ..l }))
    }

    /// `Ok(None)` for 404 and the like; errors only for transport failures.
    async fn lrcred_get(&self, ua: &str, path: &str, query: &[(&str, &str)]) -> Result<Option<Vec<u8>>> {
        let resp = self.inner.http.get(format!("{LRCRED}{path}")).query(query).header(USER_AGENT, ua).timeout(Duration::from_secs(15)).send().await?;
        if !resp.status().is_success() {
            return Ok(None);
        }
        Ok(Some(read_limited(resp, MAX_TEXT_BYTES, |_, _| {}).await?))
    }

    async fn yandex_lyrics(&self, track_id: &str) -> Result<Option<Lyrics>> {
        let ts = unix_millis() / 1000;
        let signature = sign::lyrics_sign(track_id, ts);
        let ts = ts.to_string();
        let v = self.get(&format!("/tracks/{}/lyrics", seg(track_id)?), &[("format", "LRC"), ("timeStamp", &ts), ("sign", &signature)]).await?;
        let Some(url) = v.get("downloadUrl").and_then(Value::as_str).map(https) else { return Ok(None) };
        if !cover::is_trusted_host(&url) {
            return Ok(None);
        }
        let parsed = Lyrics::from_lrc(&self.fetch_text(&url).await?, LyricsSource::Yandex);
        Ok((!parsed.is_empty()).then_some(parsed))
    }

    async fn lrclib(&self, track: &Track) -> Result<Option<Lyrics>> {
        let title = track.title.as_str();
        let artist = track.primary_artist();
        if title.is_empty() {
            return Ok(None);
        }
        let secs = track.duration_ms.map(|ms| (ms / 1000).to_string());
        let album = track.album().map(|a| a.title.as_str()).unwrap_or("");

        // 1. The exact lookup.
        let mut query: Vec<(&str, &str)> = vec![("track_name", title)];
        if !artist.is_empty() {
            query.push(("artist_name", artist));
        }
        if !album.is_empty() {
            query.push(("album_name", album));
        }
        if let Some(s) = secs.as_deref() {
            query.push(("duration", s));
        }
        if let Some(item) = self.lrclib_get("/get", &query).await?.and_then(|v| serde_json::from_value::<LrclibItem>(v).ok())
            && item_matches(&item, track)
            && let Some(found) = lyrics_from_item(&item)
        {
            return Ok(Some(found));
        }

        // 2. A search, best candidate first.
        let mut query = vec![("track_name", title)];
        if !artist.is_empty() {
            query.push(("artist_name", artist));
        }
        let items = self.lrclib_get("/search", &query).await?.map(lossy_vec::<LrclibItem>).unwrap_or_default();
        Ok(best_lrclib_match(items, track))
    }

    /// `Ok(None)` for 404 / unparsable; errors only for transport failures.
    async fn lrclib_get(&self, path: &str, query: &[(&str, &str)]) -> Result<Option<Value>> {
        let resp = self.inner.http.get(format!("{LRCLIB}{path}")).query(query).header(USER_AGENT, APP_UA).timeout(Duration::from_secs(15)).send().await?;
        if !resp.status().is_success() {
            return Ok(None);
        }
        let body = read_limited(resp, MAX_TEXT_BYTES, |_, _| {}).await?;
        Ok(serde_json::from_slice::<Value>(&body).ok().map(|mut v| {
            strip_nulls(&mut v);
            v
        }))
    }
}

// ---------------------------------------------------------------------------------------------
// Pure helpers (everything below is unit-tested without a network)

async fn read_limited(mut resp: reqwest::Response, max: u64, mut progress: impl FnMut(u64, Option<u64>)) -> Result<Vec<u8>> {
    let total = resp.content_length();
    if total.is_some_and(|t| t > max) {
        return Err(Error::Api("response is too large".into()));
    }
    let mut buf = Vec::with_capacity(total.unwrap_or(64 << 10).min(32 << 20) as usize);
    while let Some(chunk) = resp.chunk().await? {
        if buf.len() as u64 + chunk.len() as u64 > max {
            return Err(Error::Api("response is too large".into()));
        }
        buf.extend_from_slice(&chunk);
        progress(buf.len() as u64, total);
    }
    Ok(buf)
}

/// Ids that end up inside a URL path must be plain.
fn seg(id: &str) -> Result<&str> {
    let plain = !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b':'));
    if plain { Ok(id) } else { Err(Error::Parse(format!("unexpected id {id:?}"))) }
}

fn id_pairs(key: &'static str, ids: &[String]) -> Result<Vec<(&'static str, String)>> {
    ids.iter().map(|id| seg(id).map(|id| (key, id.to_owned()))).collect()
}

fn https(url: &str) -> String {
    match url.strip_prefix("http://") {
        Some(rest) => format!("https://{rest}"),
        None if url.starts_with("//") => format!("https:{url}"),
        None => url.to_owned(),
    }
}

fn as_i64(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

fn as_id(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn from_value<T: DeserializeOwned>(v: Value, what: &str) -> Result<T> {
    serde_json::from_value(v).map_err(|e| Error::Parse(format!("{what}: {e}")))
}

/// List items that may or may not be wrapped (`[{"album": {..}}, ..]` vs `[{..}, ..]`).
/// The picture URL in a Yandex ID `info` answer, if the account has set one.
fn avatar_url(v: &Value) -> Option<String> {
    if v.get("is_avatar_empty").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    let id = v.get("default_avatar_id").and_then(Value::as_str)?.trim();
    // An id is a couple of path segments of letters, digits, '-' and '_' ("1234/abc-5").
    if id.is_empty() || id.len() > 200 || !id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '/')) || id.contains("//") {
        return None;
    }
    Some(format!("https://avatars.yandex.net/get-yapic/{id}/islands-200"))
}

fn unwrap_items<T: DeserializeOwned>(v: Value, key: &str) -> Vec<T> {
    let Value::Array(items) = v else { return Vec::new() };
    items
        .into_iter()
        .filter_map(|mut item| {
            let inner = item.get_mut(key).filter(|x| x.is_object()).map(Value::take);
            serde_json::from_value(inner.unwrap_or(item)).ok()
        })
        .collect()
}

fn truncate(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_owned(),
    }
}

/// Unwraps the `{"result": …, "error": …}` envelope, mapping failures to [`Error`]s.
pub(crate) fn parse_envelope(status: u16, body: &str) -> Result<Value> {
    let mut parsed: Option<Value> = serde_json::from_str(body).ok();
    let (name, message) = match parsed.as_ref().and_then(|v| v.get("error")).filter(|e| !e.is_null()) {
        Some(e) => {
            let get = |k: &str| e.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
            // Some failures put a bare string here.
            if let Some(s) = e.as_str() { (String::new(), s.to_owned()) } else { (get("name").to_lowercase(), get("message")) }
        }
        None => (String::new(), String::new()),
    };
    let has_error = !name.is_empty() || !message.is_empty();

    let authy = ["session-expired", "unauthorized", "not-auth", "no-auth", "oauth", "token"].iter().any(|k| name.contains(k));
    if status == 401 || authy {
        return Err(Error::Unauthorized);
    }
    if status == 451 || ["region", "geo", "country"].iter().any(|k| name.contains(k)) {
        return Err(Error::RegionRestricted);
    }
    if !(200..300).contains(&status) {
        let detail = if message.is_empty() { body } else { message.as_str() };
        return Err(Error::Http { status, body: truncate(detail.trim(), 200) });
    }
    if has_error {
        return Err(Error::Api(if message.is_empty() { name } else { message }));
    }
    let mut v = parsed.take().ok_or_else(|| Error::Parse("the response is not JSON".into()))?;
    match v.get_mut("result") {
        Some(result) => {
            let mut result = result.take();
            strip_nulls(&mut result);
            Ok(result)
        }
        None => Err(Error::Api("the response has no result".into())),
    }
}

/// Which audio rendition to download. Previews are never picked; formats we may not be able to
/// decode (HE-AAC) only when nothing else is offered; at equal bitrate MP3 wins over AAC.
pub(crate) fn pick_download_info(infos: &[DownloadInfo], quality: Quality) -> Option<&DownloadInfo> {
    let rank = |codec: &str| match codec {
        "flac" => 0u8,
        "mp3" => 1,
        "aac" => 2,
        _ => 3,
    };
    let mut usable: Vec<&DownloadInfo> = infos.iter().filter(|i| !i.preview && !i.download_info_url.is_empty()).collect();
    if usable.iter().any(|i| rank(&i.codec) < 3) {
        usable.retain(|i| rank(&i.codec) < 3);
    }
    match quality {
        Quality::Saver => usable.into_iter().min_by_key(|i| (i.bitrate_in_kbps, rank(&i.codec))),
        Quality::Best => usable.into_iter().max_by_key(|i| (i.bitrate_in_kbps, std::cmp::Reverse(rank(&i.codec)))),
        Quality::Balanced => {
            const CAP: u32 = 192;
            let under = usable.iter().copied().filter(|i| i.bitrate_in_kbps <= CAP).max_by_key(|i| (i.bitrate_in_kbps, std::cmp::Reverse(rank(&i.codec))));
            under.or_else(|| usable.into_iter().min_by_key(|i| (i.bitrate_in_kbps, rank(&i.codec))))
        }
    }
}

/// `"2023-11-14T22:13:20.000Z"` for a Unix time in milliseconds.
pub(crate) fn iso8601(unix_ms: u64) -> String {
    let secs = unix_ms / 1000;
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3600, rem % 3600 / 60, rem % 60, unix_ms % 1000)
}

fn unix_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

// -- Home-page shapes ---------------------------------------------------------------------------

/// `result.blocks[].entities[].data`, with the extra `data` wrapper some blocks add peeled off.
fn landing_entities(result: &Value) -> impl Iterator<Item = &Value> {
    result
        .get("blocks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|block| block.get("entities").and_then(Value::as_array))
        .flatten()
        .filter_map(|entity| entity.get("data"))
        .map(|data| data.get("data").filter(|inner| inner.is_object()).unwrap_or(data))
}

fn landing_playlists(result: &Value) -> Vec<Playlist> {
    landing_entities(result).filter_map(|d| serde_json::from_value(d.clone()).ok()).filter(|p: &Playlist| !p.title.is_empty()).collect()
}

fn landing_albums(result: &Value) -> Vec<Album> {
    landing_entities(result).filter_map(|d| serde_json::from_value(d.clone()).ok()).filter(|a: &Album| !a.id.is_empty()).collect()
}

fn chart_tracks(result: &Value) -> Vec<Track> {
    // Chart entities carry `{position, track}`; the unwrapping above must not descend into it.
    result
        .get("blocks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|block| block.get("entities").and_then(Value::as_array))
        .flatten()
        .filter_map(|entity| entity.pointer("/data/track"))
        .filter_map(|t| serde_json::from_value(t.clone()).ok())
        .filter(|t: &Track| !t.id.is_empty())
        .collect()
}

/// `/feed`'s `generatedPlaylists[].data`.
fn generated_playlists(result: &Value) -> Vec<Playlist> {
    result
        .get("generatedPlaylists")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|g| g.get("data"))
        .filter_map(|d| serde_json::from_value(d.clone()).ok())
        .filter(|p: &Playlist| !p.title.is_empty())
        .collect()
}

pub(crate) fn parse_rotor(mut result: Value) -> RotorBatch {
    let batch_id = result.get("batchId").and_then(Value::as_str).map(str::to_owned);
    let sequence = result.get_mut("sequence").map(Value::take).unwrap_or(Value::Null);
    let tracks = match sequence {
        Value::Array(items) => items
            .into_iter()
            .filter_map(|mut item| item.get_mut("track").map(Value::take))
            .filter_map(|t| serde_json::from_value::<Track>(t).ok())
            .filter(|t| !t.id.is_empty())
            .collect(),
        _ => Vec::new(),
    };
    RotorBatch { batch_id, tracks }
}

// -- LRCLIB -------------------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct LrclibItem {
    track_name: Option<String>,
    artist_name: Option<String>,
    duration: Option<f64>,
    instrumental: bool,
    plain_lyrics: Option<String>,
    synced_lyrics: Option<String>,
}

/// Lowercased letters and digits, with bracketed qualifiers ("(feat. X)", "[Remix]") dropped.
fn norm(s: &str) -> String {
    let (mut kept, mut all, mut depth) = (String::new(), String::new(), 0u32);
    for c in s.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            c if c.is_alphanumeric() => {
                all.extend(c.to_lowercase());
                if depth == 0 {
                    kept.extend(c.to_lowercase());
                }
            }
            _ => {}
        }
    }
    if kept.is_empty() { all } else { kept }
}

fn similar(a: &str, b: &str) -> bool {
    let (a, b) = (norm(a), norm(b));
    !a.is_empty() && !b.is_empty() && (a == b || (a.chars().count() >= 4 && b.contains(&a)) || (b.chars().count() >= 4 && a.contains(&b)))
}

/// Is this LRCLIB record plausibly the same recording? Duration within 4.5 s (when both sides
/// know it), same title, and one of our artists among theirs.
fn item_matches(item: &LrclibItem, track: &Track) -> bool {
    if let (Some(theirs), Some(ours)) = (item.duration.filter(|d| *d > 0.0), track.duration_ms.filter(|d| *d > 0))
        && (theirs - ours as f64 / 1000.0).abs() > 4.5
    {
        return false;
    }
    if !similar(&track.title, item.track_name.as_deref().unwrap_or("")) {
        return false;
    }
    let theirs = item.artist_name.as_deref().unwrap_or("");
    track.artists.is_empty() || theirs.is_empty() || track.artists.iter().any(|a| similar(&a.name, theirs))
}

fn lyrics_from_item(item: &LrclibItem) -> Option<Lyrics> {
    if item.instrumental {
        return Some(Lyrics { source: LyricsSource::Lrclib, ..Lyrics::instrumental() });
    }
    if let Some(src) = item.synced_lyrics.as_deref().filter(|s| !s.trim().is_empty()) {
        let parsed = Lyrics::from_lrc(src, LyricsSource::Lrclib);
        if parsed.synced {
            return Some(parsed);
        }
    }
    let plain = item.plain_lyrics.as_deref().filter(|s| !s.trim().is_empty())?;
    let parsed = Lyrics::from_lrc(plain, LyricsSource::Lrclib);
    (!parsed.is_empty()).then_some(parsed)
}

/// The lrc.red search hit that is plausibly this recording: same title, one of our artists, and a
/// duration within 4.5 s when both sides know it. The closest duration wins.
fn best_lrcred_hit<'a>(hits: &'a [lrcred::Hit], track: &Track) -> Option<&'a lrcred::Hit> {
    let ours = track.duration_ms.filter(|d| *d > 0).map(|d| d as f64 / 1000.0);
    let gap = |h: &lrcred::Hit| match (h.duration, ours) {
        (Some(t), Some(o)) => (t - o).abs(),
        _ => 0.0,
    };
    hits.iter()
        .filter(|h| lrcred::valid_isrc(&h.isrc))
        .filter(|h| similar(&track.title, &h.title))
        .filter(|h| track.artists.is_empty() || h.artist.is_empty() || track.artists.iter().any(|a| similar(&a.name, &h.artist)))
        .filter(|h| h.duration.filter(|d| *d > 0.0).is_none() || ours.is_none() || gap(h) <= 4.5)
        .min_by(|a, b| gap(a).total_cmp(&gap(b)))
}

/// Among search hits, the matching one with synced lyrics if any, else any matching one.
fn best_lrclib_match(items: Vec<LrclibItem>, track: &Track) -> Option<Lyrics> {
    let mut plain: Option<Lyrics> = None;
    for item in items.iter().filter(|i| item_matches(i, track)) {
        match lyrics_from_item(item) {
            Some(l) if l.synced || l.instrumental => return Some(l),
            Some(l) if plain.is_none() => plain = Some(l),
            _ => {}
        }
    }
    plain
}

// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {

    #[test]
    fn avatar_url_comes_from_the_default_avatar_id_only_when_set() {
        let v = json!({"default_avatar_id": "21377/abc-def", "is_avatar_empty": false});
        assert_eq!(avatar_url(&v).as_deref(), Some("https://avatars.yandex.net/get-yapic/21377/abc-def/islands-200"));
        assert_eq!(avatar_url(&json!({"default_avatar_id": "0/0-0", "is_avatar_empty": true})), None);
        assert_eq!(avatar_url(&json!({"default_avatar_id": "../../evil?x=1"})), None);
        assert_eq!(avatar_url(&json!({})), None);
    }
    use super::*;

    fn err(r: Result<Value>) -> Error {
        r.expect_err("expected an error")
    }

    #[test]
    fn envelope_returns_the_result() {
        let v = parse_envelope(200, r#"{"invocationInfo": {"hostname": "x"}, "result": {"a": 1}}"#).unwrap();
        assert_eq!(v["a"], 1);
    }

    #[test]
    fn envelope_maps_failures() {
        assert!(matches!(err(parse_envelope(401, "{}")), Error::Unauthorized));
        assert!(matches!(err(parse_envelope(200, r#"{"error": {"name": "session-expired", "message": "x"}}"#)), Error::Unauthorized));
        assert!(matches!(err(parse_envelope(451, "")), Error::RegionRestricted));
        match err(parse_envelope(200, r#"{"error": {"name": "validate", "message": "bad id"}}"#)) {
            Error::Api(m) => assert_eq!(m, "bad id"),
            other => panic!("{other:?}"),
        }
        let e = err(parse_envelope(502, "<html>bad gateway</html>"));
        assert!(matches!(e, Error::Http { status: 502, .. }));
        assert!(e.is_transient());
        assert!(!err(parse_envelope(404, r#"{"error": {"name": "not-found", "message": "no such album"}}"#)).is_transient());
        assert!(matches!(err(parse_envelope(200, "<html>")), Error::Parse(_)));
        assert!(matches!(err(parse_envelope(200, r#"{"invocationInfo": {}}"#)), Error::Api(_)));
    }

    #[test]
    fn envelope_treats_null_like_absent() {
        let v = parse_envelope(200, r#"{"result": [{"id": "1", "title": null, "artists": null}]}"#).unwrap();
        let tracks: Vec<Track> = lossy_vec(v);
        assert_eq!(tracks.len(), 1, "an explicit null must not cost us the track");
        assert_eq!(tracks[0].id, "1");
    }

    #[test]
    fn long_error_bodies_are_truncated() {
        match err(parse_envelope(500, &"x".repeat(5000))) {
            Error::Http { body, .. } => assert!(body.chars().count() <= 201),
            other => panic!("{other:?}"),
        }
    }

    fn info(codec: &str, kbps: u32) -> DownloadInfo {
        DownloadInfo { codec: codec.into(), bitrate_in_kbps: kbps, download_info_url: format!("https://storage.mds.yandex.net/{codec}{kbps}"), ..DownloadInfo::default() }
    }

    #[test]
    fn quality_picks_the_right_rendition() {
        let mut preview = info("mp3", 320);
        preview.preview = true;
        let infos = vec![info("aac", 64), info("he-aac", 64), info("mp3", 192), info("aac", 192), info("mp3", 320), preview];
        let pick = |q| pick_download_info(&infos, q).map(|i| (i.codec.as_str(), i.bitrate_in_kbps));
        assert_eq!(pick(Quality::Saver), Some(("aac", 64)));
        assert_eq!(pick(Quality::Balanced), Some(("mp3", 192)));
        assert_eq!(pick(Quality::Best), Some(("mp3", 320)));
        // Only HE-AAC on offer: better than nothing.
        let only = vec![info("he-aac", 64)];
        assert_eq!(pick_download_info(&only, Quality::Best).map(|i| i.codec.as_str()), Some("he-aac"));
        // Nothing under the cap: take the smallest above it.
        let high = vec![info("mp3", 256), info("mp3", 320)];
        assert_eq!(pick_download_info(&high, Quality::Balanced).map(|i| i.bitrate_in_kbps), Some(256));
        // A preview alone is not a track.
        let mut p = info("mp3", 128);
        p.preview = true;
        assert!(pick_download_info(&[p], Quality::Best).is_none());
        assert!(pick_download_info(&[], Quality::Best).is_none());
    }

    #[test]
    fn iso8601_formats_known_instants() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso8601(1_700_000_000_123), "2023-11-14T22:13:20.123Z");
        assert_eq!(iso8601(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(iso8601(4_102_444_799_999), "2099-12-31T23:59:59.999Z");
    }

    #[test]
    fn ids_in_paths_are_validated() {
        assert!(seg("12345").is_ok());
        assert!(seg("user:onyourwave").is_ok());
        assert!(seg("").is_err());
        assert!(seg("../../etc").is_err());
        assert!(seg("1/2").is_err());
        assert!(seg("1?x=2").is_err());
        assert!(id_pairs("track-ids", &["1".into(), "2:3".into()]).is_ok());
        assert!(id_pairs("track-ids", &["1".into(), "bad id".into()]).is_err());
    }

    #[test]
    fn urls_are_upgraded_to_https() {
        assert_eq!(https("http://a.yandex.net/x"), "https://a.yandex.net/x");
        assert_eq!(https("//a.yandex.net/x"), "https://a.yandex.net/x");
        assert_eq!(https("https://a.yandex.net/x"), "https://a.yandex.net/x");
    }

    #[test]
    fn rotor_batches_parse() {
        let v: Value = serde_json::from_str(
            r#"{"radioIsAvailable": true, "batchId": "b-1", "sequence": [
                {"type": "track", "liked": false, "track": {"id": "11", "title": "One", "artists": [{"id": 1, "name": "A"}]}},
                {"type": "track", "track": {"id": 22, "title": "Two"}},
                {"type": "other"},
                {"type": "track", "track": "garbage"}]}"#,
        )
        .unwrap();
        let batch = parse_rotor(v);
        assert_eq!(batch.batch_id.as_deref(), Some("b-1"));
        assert_eq!(batch.tracks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["11", "22"]);
    }

    #[test]
    fn landing_shapes_parse() {
        let personal: Value = serde_json::from_str(
            r#"{"blocks": [{"id": "personal-playlists", "entities": [
                {"type": "personal-playlist", "data": {"data": {"uid": 1, "kind": 5, "title": "Playlist of the day", "generatedPlaylistType": "playlistOfTheDay"}, "type": "playlistOfTheDay"}},
                {"type": "personal-playlist", "data": {"uid": 1, "kind": 6, "title": "Premiere"}},
                {"type": "x", "data": {"uid": 1, "kind": 7, "title": ""}}]}]}"#,
        )
        .unwrap();
        let p = landing_playlists(&personal);
        assert_eq!(p.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(), ["Playlist of the day", "Premiere"]);
        assert_eq!(p[0].generated_playlist_type.as_deref(), Some("playlistOfTheDay"));

        let releases: Value = serde_json::from_str(r#"{"blocks": [{"entities": [{"data": {"id": 9, "title": "New album", "artists": [{"id": 2, "name": "B"}]}}]}]}"#).unwrap();
        assert_eq!(landing_albums(&releases)[0].artists_line(), "B");

        let chart: Value =
            serde_json::from_str(r#"{"blocks": [{"entities": [{"data": {"position": 1, "track": {"id": "5", "title": "Hit"}}}, {"data": {"position": 2}}]}]}"#).unwrap();
        assert_eq!(chart_tracks(&chart).len(), 1);

        let feed: Value = serde_json::from_str(r#"{"generatedPlaylists": [{"type": "x", "data": {"uid": 3, "kind": 8, "title": "Deja vu"}}, {"data": "junk"}]}"#).unwrap();
        assert_eq!(generated_playlists(&feed).len(), 1);

        assert!(landing_playlists(&Value::Null).is_empty());
    }

    #[test]
    fn wrapped_and_bare_list_items_both_parse() {
        let wrapped: Value = serde_json::from_str(r#"[{"id": 1, "timestamp": "t", "album": {"id": "7", "title": "W"}}]"#).unwrap();
        assert_eq!(unwrap_items::<Album>(wrapped, "album")[0].title, "W");
        let bare: Value = serde_json::from_str(r#"[{"id": "8", "name": "Bare"}]"#).unwrap();
        assert_eq!(unwrap_items::<Artist>(bare, "artist")[0].name, "Bare");
    }

    fn track(title: &str, artist: &str, secs: u64) -> Track {
        let mut t: Track = serde_json::from_str(r#"{"id": "1", "title": "x"}"#).unwrap();
        t.title = title.into();
        t.artists = vec![Artist { id: "1".into(), name: artist.into(), ..Artist::default() }];
        t.duration_ms = Some(secs * 1000);
        t
    }

    fn item(title: &str, artist: &str, secs: f64, synced: Option<&str>, plain: Option<&str>) -> LrclibItem {
        LrclibItem {
            track_name: Some(title.into()),
            artist_name: Some(artist.into()),
            duration: Some(secs),
            instrumental: false,
            plain_lyrics: plain.map(Into::into),
            synced_lyrics: synced.map(Into::into),
        }
    }

    #[test]
    fn titles_compare_loosely() {
        assert!(similar("Song (feat. Someone)", "Song"));
        assert!(similar("SONG!", "song"));
        assert!(similar("Кино — Группа крови", "группа крови"));
        assert!(!similar("Song", "Other"));
        assert!(!similar("", ""));
        // A title that is only a bracket still compares.
        assert!(similar("(Intro)", "intro"));
    }

    #[test]
    fn lrclib_records_must_match_the_track() {
        let t = track("Song", "Artist", 200);
        assert!(item_matches(&item("Song", "Artist", 201.0, None, None), &t));
        assert!(!item_matches(&item("Song", "Artist", 230.0, None, None), &t), "too different in length");
        assert!(!item_matches(&item("Different", "Artist", 200.0, None, None), &t));
        assert!(!item_matches(&item("Song", "Somebody Else", 200.0, None, None), &t));
        assert!(item_matches(&item("Song", "Artist & Friend", 200.0, None, None), &t));
    }

    #[test]
    fn lyrics_are_built_from_lrclib_records() {
        let synced = lyrics_from_item(&item("S", "A", 1.0, Some("[00:01.00]hi"), Some("hi"))).unwrap();
        assert!(synced.synced && synced.source == LyricsSource::Lrclib);
        let plain = lyrics_from_item(&item("S", "A", 1.0, Some(""), Some("one\ntwo"))).unwrap();
        assert!(!plain.synced && plain.lines.len() == 2);
        let mut inst = item("S", "A", 1.0, None, None);
        inst.instrumental = true;
        assert!(lyrics_from_item(&inst).unwrap().instrumental);
        assert!(lyrics_from_item(&item("S", "A", 1.0, None, None)).is_none());
    }

    #[test]
    fn search_prefers_synced_lyrics() {
        let t = track("Song", "Artist", 200);
        let hits = vec![
            item("Song", "Artist", 200.0, None, Some("plain only")),
            item("Other", "Artist", 200.0, Some("[00:01.00]wrong song"), None),
            item("Song", "Artist", 199.0, Some("[00:01.00]right"), Some("right")),
        ];
        let best = best_lrclib_match(hits, &t).unwrap();
        assert!(best.synced);
        assert_eq!(best.lines[0].text, "right");
        let only_plain = best_lrclib_match(vec![item("Song", "Artist", 200.0, None, Some("plain only"))], &t).unwrap();
        assert!(!only_plain.synced);
        assert!(best_lrclib_match(vec![], &t).is_none());
    }

    fn hit(isrc: &str, title: &str, artist: &str, secs: f64) -> lrcred::Hit {
        lrcred::Hit { isrc: isrc.into(), title: title.into(), artist: artist.into(), duration: Some(secs) }
    }

    #[test]
    fn lrcred_hits_must_match_and_the_closest_length_wins() {
        let t = track("Song", "Artist", 200);
        let hits = vec![
            hit("AAAAAAAAAAAA", "Song", "Artist", 203.0),
            hit("BBBBBBBBBBBB", "Song", "Artist", 200.5),
            hit("CCCCCCCCCCCC", "Other", "Artist", 200.0),
            hit("DDDDDDDDDDDD", "Song", "Somebody Else", 200.0),
            hit("../..", "Song", "Artist", 200.0),
        ];
        assert_eq!(best_lrcred_hit(&hits, &t).map(|h| h.isrc.as_str()), Some("BBBBBBBBBBBB"));
        assert!(best_lrcred_hit(&[hit("EEEEEEEEEEEE", "Song", "Artist", 260.0)], &t).is_none(), "a different length is a different recording");
        assert!(best_lrcred_hit(&[], &t).is_none());
        // Without a length on either side the title and artist decide.
        let mut nodur = hit("FFFFFFFFFFFF", "Song", "Artist", 0.0);
        nodur.duration = None;
        assert!(best_lrcred_hit(&[nodur], &t).is_some());
    }

    #[test]
    fn lrcred_stays_off_without_a_contact() {
        let c = Client::new().unwrap();
        assert!(!c.lrcred_enabled());
        c.set_lrcred_contact(Some("   "));
        assert!(!c.lrcred_enabled());
        c.set_lrcred_contact(Some("https://example.org/me ; me@example.org"));
        assert!(c.lrcred_enabled());
        c.set_lrcred_contact(None);
        assert!(!c.lrcred_enabled());
        let t = track("Song", "Artist", 200);
        let found = tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(c.lrcred_lyrics(&t));
        assert!(found.is_none(), "no request is made while it is off");
    }

    #[test]
    fn client_builds_and_holds_a_token() {
        let c = Client::new().unwrap();
        assert!(!c.has_token());
        c.set_token(Some("y0_FAKE-token_for.tests_0123456789".into()));
        assert!(c.has_token());
        assert!(c.clone().has_token(), "clones share the token");
        c.set_token(Some(String::new()));
        assert!(!c.has_token());
        // The token is attached to API requests only.
        c.set_token(Some("y0_FAKE-token_for.tests_0123456789".into()));
        let req = c.api(Method::GET, "/account/status").build().unwrap();
        assert_eq!(req.url().host_str(), Some("api.music.yandex.net"));
        let auth = req.headers().get(AUTHORIZATION).expect("API requests carry the token");
        assert!(auth.is_sensitive());
        assert!(req.headers().get("X-Yandex-Music-Client").is_some());
        let other = c.inner.http.get("https://lrclib.net/api/get").build().unwrap();
        assert!(other.headers().get(AUTHORIZATION).is_none());
    }
}
