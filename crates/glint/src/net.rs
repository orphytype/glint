//! Everything that talks to the outside world lives behind [`Backend`]. Its methods are `async` and run on a private tokio runtime; the UI never
//! blocks on them, it hands over a closure that applies the result on the UI thread.

use std::future::Future;
use std::sync::{Arc, OnceLock};

use tokio::sync::Semaphore;
use ym_api::{Account, Album, Artist, ArtistPage, Client, Lyrics, PlayReport, Playlist, RotorBatch, SearchKind, SearchResult, Shelf, Track, Visibility, WaveEvent};

use crate::art::{self, Decoded};
#[cfg(feature = "tooling")]
use crate::fixtures::{self, Catalog};
use crate::store::DownloadStore;

pub type Result<T> = std::result::Result<T, ym_api::Error>;

/// The runtime every background job runs on.
pub fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        let cap = if crate::fx::cpu_mode() { 2 } else { 4 };
        let workers = std::thread::available_parallelism().map_or(2, std::num::NonZero::get).clamp(2, cap);
        tokio::runtime::Builder::new_multi_thread().worker_threads(workers).enable_all().thread_name("glint-net").build().expect("tokio runtime")
    })
}

pub fn spawn<T: Send + 'static>(fut: impl Future<Output = T> + Send + 'static) -> tokio::task::JoinHandle<T> {
    runtime().spawn(fut)
}

#[derive(Clone, Default)]
pub enum Backend {
    #[default]
    None,
    /// The made-up catalogue the headless scenes draw from.
    #[cfg(feature = "tooling")]
    Fixture(Arc<Catalog>),
    Live(Client),
}

/// Container/codec family of an audio stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioCodec {
    Flac,
    Aac,
    Mp3,
    Other,
}

impl AudioCodec {
    pub fn from_codec(codec: &str) -> AudioCodec {
        let c = codec.to_ascii_lowercase();
        if c.contains("flac") {
            AudioCodec::Flac
        } else if c.contains("aac") || c.contains("m4a") || c.contains("mp4") {
            AudioCodec::Aac
        } else if c.contains("mp3") {
            AudioCodec::Mp3
        } else {
            AudioCodec::Other
        }
    }

    pub fn from_ext(ext: &str) -> AudioCodec {
        match ext {
            "flac" => AudioCodec::Flac,
            "m4a" => AudioCodec::Aac,
            "mp3" => AudioCodec::Mp3,
            _ => AudioCodec::Other,
        }
    }

    pub fn name(self) -> Option<&'static str> {
        match self {
            AudioCodec::Flac => Some("FLAC"),
            AudioCodec::Aac => Some("AAC"),
            AudioCodec::Mp3 => Some("MP3"),
            AudioCodec::Other => None,
        }
    }
}

/// Format and average bitrate of a track stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamQuality {
    pub codec: AudioCodec,
    /// Average bitrate in kbps (`0` when unknown).
    pub bitrate_kbps: u32,
}

impl StreamQuality {
    pub fn format_full(self, lang: crate::i18n::Lang) -> String {
        let kbps = crate::i18n::S::Kbps.get(lang);
        match (self.codec.name(), self.bitrate_kbps) {
            (Some(c), b) if b > 0 => format!("{c} · {b} {kbps}"),
            (Some(c), _) => c.to_owned(),
            (None, b) if b > 0 => format!("{b} {kbps}"),
            _ => String::new(),
        }
    }

    pub fn format_compact(self, lang: crate::i18n::Lang) -> String {
        let kbps = crate::i18n::S::Kbps.get(lang);
        match (self.codec, self.bitrate_kbps) {
            (AudioCodec::Flac, b) if b > 0 => format!("FLAC {b}"),
            (AudioCodec::Flac, _) => "FLAC".to_owned(),
            (_, b) if b > 0 => format!("{b} {kbps}"),
            _ => String::new(),
        }
    }

    /// True when `self` offers meaningfully higher fidelity than `other`.
    pub fn is_higher_than(self, other: StreamQuality) -> bool {
        (self.codec == AudioCodec::Flac && other.codec != AudioCodec::Flac) || self.bitrate_kbps > other.bitrate_kbps.saturating_add(24)
    }
}

#[cfg(feature = "tooling")]
pub fn fixture_max_quality(track: &Track) -> StreamQuality {
    let h = track.id.bytes().fold(2166136261u32, |a, b| (a ^ u32::from(b)).wrapping_mul(16777619));
    if h % 3 != 0 {
        StreamQuality { codec: AudioCodec::Flac, bitrate_kbps: 840 + (h % 180) }
    } else {
        StreamQuality { codec: AudioCodec::Mp3, bitrate_kbps: 320 }
    }
}

#[cfg(feature = "tooling")]
pub fn fixture_playing_quality(track: &Track, pref: crate::store::QualityPref) -> StreamQuality {
    match pref {
        crate::store::QualityPref::Saver => StreamQuality { codec: AudioCodec::Aac, bitrate_kbps: 64 },
        crate::store::QualityPref::Balanced => StreamQuality { codec: AudioCodec::Mp3, bitrate_kbps: 192 },
        crate::store::QualityPref::Best => fixture_max_quality(track),
    }
}

/// Disk-backed audio file staged for offline download tagging (zero full-file RAM buffer).
pub struct DownloadedAudioFile {
    pub path: std::path::PathBuf,
    pub ext: &'static str,
    /// FckCensor's uncensored version of the track, not Yandex's own.
    pub clean: bool,
    /// True when `path` is a newly created temporary file that the caller should move or delete.
    pub is_temp: bool,
    /// True when the file is an ISO BMFF (`m4a`) container holding lossless FLAC frames (`flac-mp4`).
    pub is_mp4_flac: bool,
}

/// Disk-backed playback audio handle (64 KiB read buffer in RAM instead of holding whole FLAC/MP3
/// files in memory).
pub struct StreamAudio {
    pub data: ym_audio::AudioData,
    pub ext: &'static str,
    pub quality: StreamQuality,
    pub from_disk: bool,
    pub clean: bool,
}

fn temp_stream_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("glint-stream")
}

/// Removes stale temporary audio stream files left by a previous session.
pub fn sweep_temp_streams() {
    let dir = temp_stream_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    for entry in entries.flatten() {
        let _ = std::fs::remove_file(entry.path());
    }
}

/// How many leading bytes to pull before starting playback (enough to sniff the container and
/// prime the decoder header without waiting for the rest of the file).
const STREAM_PROBE_BYTES: usize = 32 * 1024;
/// Maximum consecutive network retries while streaming a track.
const STREAM_RETRIES: u32 = 4;
/// Hard byte cap per streamed track (4 GiB, matching `ym_api`).
const MAX_STREAM_BYTES: u64 = 4 << 30;

#[derive(Clone)]
enum StreamSource {
    Yandex { track_id: String, quality: ym_api::Quality, info: ym_api::StreamInfo },
    FkCensor { url: String },
}

impl StreamSource {
    async fn open(&mut self, client: &Client, from: u64, refresh_url: bool) -> Result<ym_api::AudioResponse> {
        match self {
            StreamSource::Yandex { track_id, quality, info } => {
                if refresh_url
                    && let Ok(fresh) = client.stream(track_id, *quality).await
                {
                    *info = fresh;
                }
                match client.open_audio(info, from).await {
                    Ok(resp) => Ok(resp),
                    Err(ym_api::Error::Http { status: 401 | 403 | 410, .. }) if !refresh_url => {
                        *info = client.stream(track_id, *quality).await?;
                        client.open_audio(info, from).await
                    }
                    Err(e) => Err(e),
                }
            }
            StreamSource::FkCensor { url } => client.open_fkcensor_audio(url, from).await,
        }
    }
}

/// Opens a progressive on-disk stream, pulls the first ~32 KiB so the codec and bitrate can be
/// identified right away, and spawns a background tokio task to transfer the rest while playing.
async fn start_progressive_stream(
    client: Client,
    mut source: StreamSource,
    mut resp: ym_api::AudioResponse,
) -> Result<(ym_audio::AudioData, u64, Vec<u8>)> {
    let total_hint = resp.total_len;
    let (handle, writer) = ym_audio::StreamBuf::create(total_hint, resp.accepts_ranges).map_err(|e| ym_api::Error::Api(e.to_string()))?;
    let mut head = Vec::with_capacity(4096);
    let mut offset = 0u64;
    let mut eof = false;

    while offset < STREAM_PROBE_BYTES as u64 {
        match resp.next_chunk().await? {
            Some(chunk) => {
                if offset.saturating_add(chunk.len() as u64) > MAX_STREAM_BYTES {
                    writer.fail("response is too large".into());
                    return Err(ym_api::Error::Api("response is too large".into()));
                }
                if head.len() < 4096 {
                    let take = (4096 - head.len()).min(chunk.len());
                    head.extend_from_slice(&chunk[..take]);
                }
                writer.write(offset, &chunk).map_err(|e| ym_api::Error::Api(e.to_string()))?;
                offset += chunk.len() as u64;
            }
            None => {
                eof = true;
                break;
            }
        }
    }

    let reported_len = total_hint.unwrap_or(offset);
    if eof {
        writer.reached_eof(offset);
        return Ok((ym_audio::AudioData::stream(handle), offset, head));
    }

    let notify = Arc::new(tokio::sync::Notify::new());
    {
        let n = notify.clone();
        writer.set_waker(move || n.notify_one());
    }

    spawn(async move {
        let mut active: Option<ym_api::AudioResponse> = Some(resp);
        let mut pos = offset;
        let mut failures = 0u32;

        loop {
            if writer.is_aborted() {
                return;
            }
            if let Some(target) = writer.take_seek_request() {
                active = None;
                pos = target;
                failures = 0;
                match source.open(&client, target, false).await {
                    Ok(next) => {
                        if target > 0 && !next.partial {
                            // Server ignored Range and restarted from byte 0.
                            writer.set_seekable(false);
                            pos = 0;
                        }
                        if let Some(len) = next.total_len {
                            writer.set_len(len);
                        }
                        active = Some(next);
                    }
                    Err(e) => {
                        if writer.is_aborted() {
                            return;
                        }
                        failures += 1;
                        if failures > STREAM_RETRIES {
                            writer.fail(e.to_string());
                            return;
                        }
                    }
                }
                continue;
            }

            let Some(cur) = active.as_mut() else {
                // Parked after finishing a jumped segment or waiting to retry.
                if failures > 0 {
                    let delay = std::time::Duration::from_secs(1u64 << (failures - 1).min(3));
                    tokio::select! {
                        () = tokio::time::sleep(delay) => {}
                        () = notify.notified() => {
                            if writer.is_aborted() {
                                return;
                            }
                            if writer.has_seek_request() {
                                continue;
                            }
                        }
                    }
                    match source.open(&client, pos, true).await {
                        Ok(next) => {
                            if pos > 0 && !next.partial {
                                writer.set_seekable(false);
                                pos = 0;
                            }
                            if let Some(len) = next.total_len {
                                writer.set_len(len);
                            }
                            active = Some(next);
                        }
                        Err(e) => {
                            failures += 1;
                            if failures > STREAM_RETRIES {
                                writer.fail(e.to_string());
                                return;
                            }
                        }
                    }
                } else {
                    notify.notified().await;
                }
                continue;
            };

            let chunk = tokio::select! {
                biased;
                () = notify.notified() => {
                    if writer.is_aborted() {
                        return;
                    }
                    continue;
                }
                res = cur.next_chunk() => res,
            };

            match chunk {
                Ok(Some(bytes)) => {
                    if pos.saturating_add(bytes.len() as u64) > MAX_STREAM_BYTES {
                        writer.fail("response is too large".into());
                        return;
                    }
                    if let Err(e) = writer.write(pos, &bytes) {
                        writer.fail(e.to_string());
                        return;
                    }
                    pos += bytes.len() as u64;
                    failures = 0;
                }
                Ok(None) => {
                    active = None;
                    if writer.reached_eof(pos) {
                        return;
                    }
                    // Earlier bytes were skipped by a seek jump; stay parked until needed or cancelled.
                }
                Err(e) => {
                    active = None;
                    if writer.is_aborted() {
                        return;
                    }
                    failures += 1;
                    if failures > STREAM_RETRIES {
                        writer.fail(e.to_string());
                        return;
                    }
                }
            }
        }
    });

    Ok((ym_audio::AudioData::stream(handle), reported_len, head))
}

fn read_file_head(path: &std::path::Path) -> Vec<u8> {
    use std::io::Read as _;
    let Ok(mut f) = std::fs::File::open(path) else { return Vec::new() };
    let mut buf = vec![0u8; 4096];
    let n = f.read(&mut buf).unwrap_or(0);
    buf.truncate(n);
    buf
}

/// Estimates average audio bitrate in kbps from the total file size and the file's leading header
/// bytes without loading the whole file into RAM.
fn estimate_bitrate_from_head(head: &[u8], total_len: u64, ext: &str, duration_ms: Option<u64>) -> u32 {
    let Some(dur_ms) = duration_ms.filter(|&d| d > 0) else { return 0 };
    let header_bytes: u64 = if ext == "flac" && head.starts_with(b"fLaC") {
        let mut off = 4usize;
        while off + 4 <= head.len() {
            let h = head[off];
            let len = ((head[off + 1] as usize) << 16) | ((head[off + 2] as usize) << 8) | (head[off + 3] as usize);
            off = off.saturating_add(4).saturating_add(len);
            if h & 0x80 != 0 {
                break;
            }
        }
        off as u64
    } else if head.len() > 10 && &head[0..3] == b"ID3" {
        ((u64::from(head[6] & 0x7f) << 21) | (u64::from(head[7] & 0x7f) << 14) | (u64::from(head[8] & 0x7f) << 7) | u64::from(head[9] & 0x7f)) + 10
    } else {
        0
    };
    let payload = if header_bytes < total_len { total_len - header_bytes } else { total_len };
    ((payload * 8 + dur_ms / 2) / dur_ms) as u32
}

/// The container an audio file really is, from its first bytes. Cached downloads have no codec
/// description to go by, and files served as MP3 are sometimes FLAC or AAC inside.
fn sniff_ext(bytes: &[u8]) -> &'static str {
    match bytes {
        [b'f', b'L', b'a', b'C', ..] => "flac",
        [b'O', b'g', b'g', b'S', ..] => "ogg",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'A', b'V', b'E', ..] => "wav",
        // ISO base media (`ftyp` at offset 4): MP4 container holding AAC or lossless FLAC (`flac-mp4`).
        [_, _, _, _, b'f', b't', b'y', b'p', ..] => "m4a",
        // ID3 tag, MPEG sync frame, or unknown container: Yandex serves MP3 by default.
        _ => "mp3",
    }
}

/// Container hint for Symphonia: prefers the actual byte magic so `flac-mp4` gets `"m4a"` instead
/// of `"flac"`, while falling back to the API codec string if the leading bytes are ambiguous.
fn container_ext(head: &[u8], codec: &str) -> &'static str {
    match head {
        [b'f', b'L', b'a', b'C', ..] => "flac",
        [b'O', b'g', b'g', b'S', ..] => "ogg",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'A', b'V', b'E', ..] => "wav",
        [_, _, _, _, b'f', b't', b'y', b'p', ..] => "m4a",
        _ => {
            let c = codec.to_ascii_lowercase();
            if c.contains("mp4") || c.contains("aac") {
                "m4a"
            } else if c.contains("flac") {
                "flac"
            } else {
                "mp3"
            }
        }
    }
}

fn local_codec(path: &std::path::Path, ext: &str) -> AudioCodec {
    if ext == "m4a" && crate::audio_tags::mp4_has_flac_path(path) {
        return AudioCodec::Flac;
    }
    AudioCodec::from_ext(ext)
}

fn limiter() -> &'static Semaphore {
    static S: OnceLock<Semaphore> = OnceLock::new();
    S.get_or_init(|| Semaphore::new(6))
}

/// Limits concurrent disk reads and image decodes so fast scrolling never floods weak CPUs or HDDs.
pub fn decode_limiter() -> &'static Semaphore {
    static S: OnceLock<Semaphore> = OnceLock::new();
    S.get_or_init(|| {
        let n = std::thread::available_parallelism().map_or(2, std::num::NonZero::get).clamp(2, 4);
        Semaphore::new(n)
    })
}

fn no_session() -> ym_api::Error {
    ym_api::Error::Unauthorized
}

/// Result of fetching one batch of a Wave station or Rotor session.
pub struct WaveBatch {
    pub session: Option<String>,
    pub batch: RotorBatch,
}

fn parse_hex_rgb(hex: &str) -> Option<[u8; 3]> {
    let hex = hex.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some([r, g, b])
}

/// The rotor station and seeds a wave id asks for (see [`Backend::wave`]).
fn station_of(id: &str) -> (&str, &'static [&'static str]) {
    if id == "collection" {
        return ("user:onyourwave", &["settingDiversity:favorite"]);
    }
    if let Some(p) = ym_api::wave::preset(id) {
        return (p.station, p.seeds);
    }
    if id.split_once(':').is_some_and(|(ty, tag)| !ty.is_empty() && !tag.is_empty()) {
        return (id, &[]);
    }
    (ym_api::WAVE_PRESETS[0].station, ym_api::WAVE_PRESETS[0].seeds)
}

impl Backend {
    pub async fn account(&self) -> Result<Account> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => Ok(c.account()),
            Backend::Live(c) => c.account().await,
            Backend::None => Err(no_session()),
        }
    }

    pub async fn home(&self) -> Result<Vec<Shelf>> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => Ok(c.home()),
            Backend::Live(c) => c.home().await,
            Backend::None => Err(no_session()),
        }
    }

    /// Additional personalized My Waves for the Home arc wheel (`POST /wheel/new`, falling back to
    /// `/rotor/stations/dashboard` if the wheel endpoint returns nothing).
    pub async fn wheel(&self, seeds: &[String]) -> Result<Vec<ym_api::WheelItem>> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => {
                let mut items = c.wheel();
                if !seeds.is_empty() && items.len() > 2 {
                    items.rotate_left(2);
                }
                Ok(items)
            }
            Backend::Live(c) => {
                if let Ok(items) = c.wheel_with_seeds(seeds).await
                    && !items.is_empty()
                {
                    return Ok(items);
                }
                let stations = c.stations_dashboard().await?;
                Ok(stations
                    .into_iter()
                    .filter(|st| !st.id.is_empty() && !st.name.is_empty())
                    .map(|st| ym_api::WheelItem {
                        name: st.name,
                        description: String::new(),
                        cover: st.icon,
                        round: st.id.starts_with("artist:"),
                        control_accent: st.id == "user:onyourwave",
                        color: st.color.as_deref().and_then(parse_hex_rgb),
                        seeds: vec![st.id],
                    })
                    .collect())
            }
            Backend::None => Err(no_session()),
        }
    }

    pub async fn liked_tracks(&self, uid: i64, fkcensor: &crate::fkcensor::FkCensor, fkcensor_on: bool) -> Result<Vec<Track>> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => Ok(c.liked_tracks()),
            Backend::Live(c) => {
                let refs = c.liked_track_refs(uid).await?;
                let ids: Vec<String> = refs.iter().map(|r| r.id.clone()).collect();
                let mut tracks = c.tracks(&ids).await?;
                if fkcensor_on {
                    fkcensor.ensure_loaded(c).await;
                    fkcensor.patch_tracks(&mut tracks);
                }
                Ok(tracks)
            }
            Backend::None => Err(no_session()),
        }
    }

    /// The cover Yandex gives the account's Liked playlist (kind 3), when it is a picture of its
    /// own rather than a mosaic of the tracks' covers; `None` leaves the app's own.
    pub async fn liked_cover(&self, uid: i64) -> Result<Option<String>> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(_) => Ok(None),
            Backend::Live(c) => {
                let list = c.playlists_by_key(&[format!("{uid}:3")]).await?;
                Ok(list.into_iter().next().and_then(|p| {
                    let own = p.cover.as_ref().filter(|c| c.kind.as_deref() != Some("mosaic")).and_then(|c| c.uri.clone());
                    own.or(p.og_image).filter(|t| !t.trim().is_empty())
                }))
            }
            Backend::None => Err(no_session()),
        }
    }

    pub async fn playlists(&self, uid: i64) -> Result<Vec<Playlist>> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => Ok(c
                .playlists
                .clone()
                .into_iter()
                .map(|mut p| {
                    p.tracks.clear();
                    p
                })
                .collect()),
            Backend::Live(c) => c.playlists(uid).await,
            Backend::None => Err(no_session()),
        }
    }

    pub async fn liked_albums(&self, uid: i64) -> Result<Vec<Album>> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => Ok(c
                .albums
                .iter()
                .step_by(3)
                .cloned()
                .map(|mut a| {
                    a.volumes.clear();
                    a
                })
                .collect()),
            Backend::Live(c) => c.liked_albums(uid).await,
            Backend::None => Err(no_session()),
        }
    }

    pub async fn liked_artists(&self, uid: i64) -> Result<Vec<Artist>> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => Ok(c.artists.clone()),
            Backend::Live(c) => c.liked_artists(uid).await,
            Backend::None => Err(no_session()),
        }
    }

    pub async fn top_artists(&self, count: u32) -> Result<Vec<ym_api::TopArtist>> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => Ok(c.top_artists()),
            Backend::Live(c) => c.top_artists(count).await,
            Backend::None => Err(no_session()),
        }
    }

    pub async fn liked_playlists(&self, uid: i64) -> Result<Vec<Playlist>> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => Ok(c.liked_playlists()),
            Backend::Live(c) => c.liked_playlists(uid).await,
            Backend::None => Err(no_session()),
        }
    }

    pub async fn disliked_tracks(&self, uid: i64) -> Result<Vec<Track>> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => Ok(c.disliked_tracks()),
            Backend::Live(c) => {
                let refs = c.disliked_track_refs(uid).await?;
                if refs.is_empty() {
                    return Ok(Vec::new());
                }
                let ids: Vec<String> = refs.iter().map(|r| r.id.clone()).collect();
                c.tracks(&ids).await
            }
            Backend::None => Err(no_session()),
        }
    }

    pub async fn undislike_track(&self, uid: i64, track: &Track) -> Result<()> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(_) => Ok(()),
            Backend::Live(c) => c.undislike_tracks(uid, &[track.compound_id()]).await,
            Backend::None => Err(no_session()),
        }
    }

    pub async fn disliked_artists(&self, uid: i64) -> Result<Vec<Artist>> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => Ok(c.disliked_artists()),
            Backend::Live(c) => c.disliked_artists(uid).await,
            Backend::None => Err(no_session()),
        }
    }

    pub async fn undislike_artist(&self, uid: i64, id: &str) -> Result<()> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(_) => Ok(()),
            Backend::Live(c) => c.undislike_artist(uid, id).await,
            Backend::None => Err(no_session()),
        }
    }

    pub async fn playlist_set_visibility(&self, uid: i64, kind: i64, private: bool) -> Result<()> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(_) => Ok(()),
            Backend::Live(c) => {
                let vis = if private { Visibility::Private } else { Visibility::Public };
                c.playlist_set_visibility(uid, kind, vis).await.map(drop)
            }
            Backend::None => Err(no_session()),
        }
    }

    pub async fn album(&self, id: &str, fkcensor: &crate::fkcensor::FkCensor, fkcensor_on: bool) -> Result<Album> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => c.album(id).ok_or(ym_api::Error::Api("album not found".into())),
            Backend::Live(c) => {
                let res = c.album(id).await;
                if !fkcensor_on {
                    return res;
                }
                match res {
                    Ok(album) => Ok(fkcensor.merge_album(c, id, Some(album)).await.unwrap_or_default()),
                    Err(e) => match fkcensor.merge_album(c, id, None).await {
                        Some(album) => Ok(album),
                        None => Err(e),
                    },
                }
            }
            Backend::None => Err(no_session()),
        }
    }

    pub async fn playlist(&self, uid: i64, kind: i64, fkcensor: &crate::fkcensor::FkCensor, fkcensor_on: bool) -> Result<Playlist> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => c.playlist(kind).ok_or(ym_api::Error::Api("playlist not found".into())),
            Backend::Live(c) => {
                let mut pl = c.playlist(uid, kind).await?;
                if fkcensor_on {
                    fkcensor.ensure_loaded(c).await;
                    for entry in &mut pl.tracks {
                        if let Some(track) = entry.track.as_mut() {
                            fkcensor.patch_tracks(std::slice::from_mut(track));
                        }
                    }
                }
                Ok(pl)
            }
            Backend::None => Err(no_session()),
        }
    }

    /// All of an artist's tracks (up to the API's 500-track page), most popular first.
    pub async fn artist_tracks(&self, id: &str, fkcensor: &crate::fkcensor::FkCensor, fkcensor_on: bool) -> Result<Vec<Track>> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => Ok(c.artist_tracks(id)),
            Backend::Live(c) => {
                let mut tracks = c.artist_tracks(id, 500).await?;
                if fkcensor_on {
                    fkcensor.merge_artist_tracks(c, id, &mut tracks).await;
                }
                Ok(tracks)
            }
            Backend::None => Err(no_session()),
        }
    }

    /// The URL of the account's picture, if it has one.
    pub async fn avatar(&self) -> Option<String> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(_) => Some(ym_api::cover::url(&fixtures::avatar_template(), 200)),
            Backend::Live(c) => c.avatar().await.ok().flatten(),
            Backend::None => None,
        }
    }

    pub async fn artist(&self, id: &str, fkcensor: &crate::fkcensor::FkCensor, fkcensor_on: bool) -> Result<ArtistPage> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => c.artist(id).ok_or(ym_api::Error::Api("artist not found".into())),
            Backend::Live(c) => {
                let mut page = c.artist(id).await?;
                if fkcensor_on {
                    fkcensor.merge_artist(c, id, &mut page).await;
                }
                Ok(page)
            }
            Backend::None => Err(no_session()),
        }
    }

    pub async fn search(&self, text: &str, kind: SearchKind, fkcensor: &crate::fkcensor::FkCensor, fkcensor_on: bool) -> Result<SearchResult> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => Ok(c.search(text)),
            Backend::Live(c) => {
                let mut res = c.search(text, kind, 0).await?;
                if fkcensor_on {
                    fkcensor.ensure_loaded(c).await;
                    fkcensor.merge_search(text, &mut res);
                }
                Ok(res)
            }
            Backend::None => Err(no_session()),
        }
    }

    /// Likes or unlikes an album, an artist or a playlist (`"uid:kind"`).
    pub async fn set_item_like(&self, uid: i64, what: ym_api::Likeable, id: &str, like: bool) -> Result<()> {
        match self {
            Backend::Live(c) => c.set_item_like(uid, what, id, like).await,
            _ => Ok(()),
        }
    }

    /// Every album, artist and playlist the account liked, as `Detail::like_key`s.
    pub async fn liked_items(&self, uid: i64) -> Result<Vec<String>> {
        let (albums, artists) = (self.liked_albums(uid).await?, self.liked_artists(uid).await?);
        let mut out: Vec<String> = albums.iter().map(|a| format!("album:{}", a.id)).chain(artists.iter().map(|a| format!("artist:{}", a.id))).collect();
        if let Backend::Live(c) = self {
            // An older account may have none of these: no list is no reason to fail the rest.
            out.extend(c.liked_playlist_ids(uid).await.unwrap_or_default().into_iter().map(|id| format!("playlist:{id}")));
        }
        Ok(out)
    }

    /// Fetches the next batch of a Wave station or Rotor session. Uses modern `/rotor/session/*`
    /// first (matching the desktop app and supporting all `/wheel/new` vibes), falling back to the
    /// legacy `/rotor/station/{id}/tracks` endpoint when needed.
    #[expect(clippy::too_many_arguments, reason = "wave session parameters for new/next batch resolution")]
    #[cfg_attr(not(feature = "tooling"), allow(unused_variables))]
    pub async fn wave(
        &self,
        preset: &str,
        extra_seeds: &[String],
        knobs: Option<ym_api::WaveSettings>,
        session: Option<&str>,
        queue: &[String],
        after: Option<&str>,
        round: u64,
    ) -> Result<WaveBatch> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => Ok(WaveBatch { session: None, batch: RotorBatch { batch_id: Some(format!("fixture-{round}")), tracks: c.wave(preset, round) } }),
            Backend::Live(c) => {
                if let Some(sid) = session.filter(|s| !s.is_empty())
                    && !queue.is_empty()
                    && let Ok(rs) = c.rotor_session_tracks(sid, queue).await
                    && !rs.tracks.is_empty()
                {
                    let next_sid = if rs.id.is_empty() { Some(sid.to_owned()) } else { Some(rs.id) };
                    return Ok(WaveBatch { session: next_sid, batch: RotorBatch { batch_id: rs.batch_id, tracks: rs.tracks } });
                }

                let seeds: Vec<String> = if let Some(k) = knobs {
                    if after.is_none() {
                        let _ = c.set_wave_settings(k).await;
                    }
                    k.seeds()
                } else if !extra_seeds.is_empty() {
                    extra_seeds.to_vec()
                } else {
                    let (station, st_seeds) = station_of(preset);
                    std::iter::once(station.to_owned()).chain(st_seeds.iter().map(|s| (*s).to_owned())).collect()
                };

                if let Ok(rs) = c.rotor_session_new(&seeds, queue).await
                    && !rs.tracks.is_empty()
                {
                    return Ok(WaveBatch {
                        session: (!rs.id.is_empty()).then_some(rs.id),
                        batch: RotorBatch { batch_id: rs.batch_id, tracks: rs.tracks },
                    });
                }

                if let Some(k) = knobs {
                    let tail: Vec<String> = k.seeds().into_iter().skip(1).collect();
                    let tail_ref: Vec<&str> = tail.iter().map(String::as_str).collect();
                    let batch = c.wave_tracks("user:onyourwave", &tail_ref, after).await?;
                    return Ok(WaveBatch { session: None, batch });
                }
                let (station, st_seeds) = station_of(preset);
                let batch = c.wave_tracks(station, st_seeds, after).await?;
                Ok(WaveBatch { session: None, batch })
            }
            Backend::None => Err(no_session()),
        }
    }

    pub async fn wave_feedback(&self, preset: &str, session: Option<&str>, event: WaveEvent, track_id: &str, batch_id: Option<&str>, played_secs: f64) {
        if let Backend::Live(c) = self {
            if let Some(sid) = session.filter(|s| !s.is_empty()) {
                let _ = c.rotor_session_feedback(sid, batch_id, event, Some(track_id), Some(played_secs)).await;
                return;
            }
            let (station, _) = station_of(preset);
            let _ = c.wave_feedback(station, event, Some(track_id), batch_id, Some(played_secs)).await;
        }
    }

    /// A new private playlist on the account.
    pub async fn playlist_create(&self, uid: i64, title: &str) -> Result<Playlist> {
        match self {
            Backend::Live(c) => c.playlist_create(uid, title, Visibility::Private).await,
            #[cfg(feature = "tooling")]
            Backend::Fixture(_) => Ok(Playlist { uid, kind: 9000 + i64::from(title.len() as u32), title: title.into(), track_count: Some(0), ..Default::default() }),
            Backend::None => Err(no_session()),
        }
    }

    pub async fn playlist_rename(&self, uid: i64, kind: i64, title: &str) -> Result<()> {
        match self {
            Backend::Live(c) => c.playlist_rename(uid, kind, title).await.map(drop),
            #[cfg(feature = "tooling")]
            Backend::Fixture(_) => Ok(()),
            Backend::None => Err(no_session()),
        }
    }

    pub async fn playlist_delete(&self, uid: i64, kind: i64) -> Result<()> {
        match self {
            Backend::Live(c) => c.playlist_delete(uid, kind).await,
            #[cfg(feature = "tooling")]
            Backend::Fixture(_) => Ok(()),
            Backend::None => Err(no_session()),
        }
    }

    /// Adds `track` at the start of one of the account's playlists. Edits quote the playlist's
    /// revision, so it is read fresh first.
    pub async fn playlist_add(&self, uid: i64, kind: i64, track: &Track) -> Result<()> {
        match self {
            Backend::Live(c) => {
                let list = c.playlist(uid, kind).await?;
                let album = track.albums.first().map(|a| a.id.clone()).unwrap_or_default();
                c.playlist_insert(uid, kind, list.revision, 0, &[(track.id.clone(), album)]).await.map(drop)
            }
            #[cfg(feature = "tooling")]
            Backend::Fixture(_) => Ok(()),
            Backend::None => Err(no_session()),
        }
    }

    /// Moves a track from `from` to `to` in one of the account's playlists.
    pub async fn playlist_move(&self, uid: i64, kind: i64, from: usize, to: usize, track: &Track) -> Result<()> {
        if from == to {
            return Ok(());
        }
        match self {
            Backend::Live(c) => {
                let list = c.playlist(uid, kind).await?;
                if from >= list.tracks.len() || to >= list.tracks.len() {
                    return Ok(());
                }
                let album = track.albums.first().map(|a| a.id.clone()).unwrap_or_default();
                c.playlist_move(uid, kind, list.revision, from, to, &(track.id.clone(), album)).await.map(drop)
            }
            #[cfg(feature = "tooling")]
            Backend::Fixture(_) => Ok(()),
            Backend::None => Err(no_session()),
        }
    }

    /// Uploads a local audio file into one of the account's playlists via the UGC loader.
    pub async fn playlist_upload_ugc(&self, uid: i64, kind: i64, filename: &str, audio: &[u8]) -> Result<String> {
        match self {
            Backend::Live(c) => c.playlist_upload_ugc(uid, kind, filename, audio).await,
            #[cfg(feature = "tooling")]
            Backend::Fixture(_) => Ok(String::new()),
            Backend::None => Err(no_session()),
        }
    }

    /// Takes the first `track_id` out of one of the account's playlists (found in a fresh copy,
    /// with its revision, so a list changed elsewhere meanwhile loses the right track).
    pub async fn playlist_remove(&self, uid: i64, kind: i64, track_id: &str) -> Result<()> {
        match self {
            Backend::Live(c) => {
                let list = c.playlist(uid, kind).await?;
                let Some(at) = list.tracks.iter().position(|t| t.id == track_id || t.track.as_ref().is_some_and(|t| t.id == track_id)) else { return Ok(()) };
                c.playlist_remove(uid, kind, list.revision, at, at + 1).await.map(drop)
            }
            #[cfg(feature = "tooling")]
            Backend::Fixture(_) => Ok(()),
            Backend::None => Err(no_session()),
        }
    }

    /// Tells the service a track was played, so the account's history and recommendations count
    /// it. Best effort, and nothing outside a live session.
    pub async fn report_play(&self, uid: i64, report: PlayReport) {
        if let Backend::Live(c) = self {
            let _ = c.play_audio(uid, &report).await;
        }
    }

    pub async fn credits(&self, track: &Track) -> Result<Vec<ym_api::Credit>> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => Ok(c.credits(track)),
            Backend::Live(c) => c.track_credits(&track.id).await,
            Backend::None => Err(no_session()),
        }
    }

    pub async fn lyrics(&self, track: &Track, fkcensor: &crate::fkcensor::FkCensor, fkcensor_on: bool) -> Result<Lyrics> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => Ok(c.lyrics(track)),
            Backend::Live(c) => {
                if fkcensor_on {
                    fkcensor.ensure_loaded(c).await;
                    if let Some(url) = fkcensor.lyrics_url(&track.id)
                        && let Ok(lrc) = c.fetch_fkcensor_lrc(&url).await
                    {
                        let parsed = Lyrics::from_lrc(&lrc, ym_api::LyricsSource::Yandex);
                        if parsed.has_words() {
                            return Ok(parsed);
                        }
                    }
                }
                c.lyrics(track).await
            }
            Backend::None => Err(no_session()),
        }
    }

    pub async fn set_like(&self, uid: i64, track: &Track, like: bool) -> Result<()> {
        match self {
            Backend::Live(c) => {
                let id = track.compound_id();
                if like { c.like_tracks(uid, &[id]).await } else { c.unlike_tracks(uid, &[id]).await }
            }
            _ => Ok(()),
        }
    }

    pub async fn set_dislike(&self, uid: i64, track: &Track) -> Result<()> {
        match self {
            Backend::Live(c) => c.dislike_tracks(uid, &[track.compound_id()]).await,
            _ => Ok(()),
        }
    }

    /// Streams the audio file of `track` directly to `tmp_path` on disk (or reuses an existing
    /// downloaded file path) without buffering whole FLAC/MP3 files in RAM.
    #[expect(clippy::too_many_arguments, reason = "download source, quality, cache, fkcensor, and staging path")]
    pub async fn download_audio_to_file(
        &self,
        track: &Track,
        quality: ym_api::Quality,
        downloads: Option<Arc<DownloadStore>>,
        fkcensor: &crate::fkcensor::FkCensor,
        fkcensor_on: bool,
        exact: bool,
        tmp_path: &std::path::Path,
    ) -> Result<DownloadedAudioFile> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(_) => Err(ym_api::Error::Api("the scene fixtures have no audio".into())),
            Backend::Live(c) => {
                if fkcensor_on {
                    fkcensor.ensure_loaded(c).await;
                }
                let replacement = fkcensor_on.then(|| fkcensor.replacement(&track.id)).flatten();
                let q = format!("{quality:?}");
                let find = |variant: &str| {
                    let disk = downloads.as_ref()?;
                    if exact { disk.get_audio_path(&format!("{}-{q}-{variant}", track.id)) } else { disk.find_audio_path(&track.id, &q, variant) }
                };
                if replacement.is_some()
                    && let Some((path, _)) = find("replacement")
                {
                    let head = read_file_head(&path);
                    let ext = sniff_ext(&head);
                    let is_mp4_flac = ext == "m4a" && crate::audio_tags::mp4_has_flac_path(&path);
                    return Ok(DownloadedAudioFile { path, ext, clean: true, is_temp: false, is_mp4_flac });
                }
                let local = find("original");
                if replacement.is_none()
                    && let Some((path, _)) = local.clone()
                {
                    let head = read_file_head(&path);
                    let ext = sniff_ext(&head);
                    let is_mp4_flac = ext == "m4a" && crate::audio_tags::mp4_has_flac_path(&path);
                    return Ok(DownloadedAudioFile { path, ext, clean: false, is_temp: false, is_mp4_flac });
                }
                if let Some(replacement) = replacement {
                    let fetched = if local.is_some() {
                        tokio::time::timeout(std::time::Duration::from_secs(4), c.fetch_fkcensor_audio_to_file(&replacement.url, tmp_path))
                            .await
                            .ok()
                            .and_then(Result::ok)
                    } else {
                        c.fetch_fkcensor_audio_to_file(&replacement.url, tmp_path).await.ok()
                    };
                    if let Some((_, head)) = fetched {
                        let ext = sniff_ext(&head);
                        let is_mp4_flac = ext == "m4a" && crate::audio_tags::mp4_has_flac_path(tmp_path);
                        return Ok(DownloadedAudioFile { path: tmp_path.to_path_buf(), ext, clean: true, is_temp: true, is_mp4_flac });
                    }
                }
                if let Some((path, _)) = local {
                    let head = read_file_head(&path);
                    let ext = sniff_ext(&head);
                    let is_mp4_flac = ext == "m4a" && crate::audio_tags::mp4_has_flac_path(&path);
                    return Ok(DownloadedAudioFile { path, ext, clean: false, is_temp: false, is_mp4_flac });
                }
                let info = c.stream(&track.id, quality).await?;
                let (_, head) = c.fetch_audio_to_file(&info, tmp_path).await?;
                let ext = container_ext(&head, &info.codec);
                let is_mp4_flac = ext == "m4a" && (info.codec.to_ascii_lowercase().contains("flac") || crate::audio_tags::mp4_has_flac_path(tmp_path));
                Ok(DownloadedAudioFile { path: tmp_path.to_path_buf(), ext, clean: false, is_temp: true, is_mp4_flac })
            }
            Backend::None => Err(no_session()),
        }
    }

    /// Opens a seekable disk-backed audio stream for playback so large FLAC/MP3 tracks never sit
    /// whole in RAM. Offline downloads open straight from disk; online streams write to a small
    /// temporary file deleted automatically when playback finishes.
    pub async fn audio_stream(
        &self,
        track: &Track,
        quality: ym_api::Quality,
        downloads: Option<Arc<DownloadStore>>,
        fkcensor: &crate::fkcensor::FkCensor,
        fkcensor_on: bool,
    ) -> Result<StreamAudio> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(_) => Err(ym_api::Error::Api("the scene fixtures have no audio".into())),
            Backend::Live(c) => {
                if fkcensor_on {
                    fkcensor.ensure_loaded(c).await;
                }
                let replacement = fkcensor_on.then(|| fkcensor.replacement(&track.id)).flatten();
                let q = format!("{quality:?}");
                let find_path = |variant: &str| {
                    let disk = downloads.as_ref()?;
                    disk.find_audio_path(&track.id, &q, variant)
                };
                if replacement.is_some()
                    && let Some((path, len)) = find_path("replacement")
                {
                    let head = read_file_head(&path);
                    let ext = sniff_ext(&head);
                    let codec = local_codec(&path, ext);
                    let quality = StreamQuality { codec, bitrate_kbps: estimate_bitrate_from_head(&head, len, ext, track.duration_ms) };
                    return Ok(StreamAudio { data: ym_audio::AudioData::file(path, len), ext, quality, from_disk: true, clean: true });
                }
                let local = find_path("original");
                if replacement.is_none()
                    && let Some((path, len)) = local.clone()
                {
                    let head = read_file_head(&path);
                    let ext = sniff_ext(&head);
                    let codec = local_codec(&path, ext);
                    let quality = StreamQuality { codec, bitrate_kbps: estimate_bitrate_from_head(&head, len, ext, track.duration_ms) };
                    return Ok(StreamAudio { data: ym_audio::AudioData::file(path, len), ext, quality, from_disk: true, clean: false });
                }
                if let Some(replacement) = replacement {
                    let opened = if local.is_some() {
                        tokio::time::timeout(std::time::Duration::from_secs(4), c.open_fkcensor_audio(&replacement.url, 0)).await.ok().and_then(Result::ok)
                    } else {
                        c.open_fkcensor_audio(&replacement.url, 0).await.ok()
                    };
                    if let Some(resp) = opened
                        && let Ok((data, len, head)) = start_progressive_stream(c.clone(), StreamSource::FkCensor { url: replacement.url }, resp).await
                    {
                        let ext = sniff_ext(&head);
                        let quality = StreamQuality { codec: AudioCodec::from_ext(ext), bitrate_kbps: estimate_bitrate_from_head(&head, len, ext, track.duration_ms) };
                        return Ok(StreamAudio { data, ext, quality, from_disk: false, clean: true });
                    }
                }
                if let Some((path, len)) = local {
                    let head = read_file_head(&path);
                    let ext = sniff_ext(&head);
                    let codec = local_codec(&path, ext);
                    let quality = StreamQuality { codec, bitrate_kbps: estimate_bitrate_from_head(&head, len, ext, track.duration_ms) };
                    return Ok(StreamAudio { data: ym_audio::AudioData::file(path, len), ext, quality, from_disk: true, clean: false });
                }
                let info = c.stream(&track.id, quality).await?;
                let resp = c.open_audio(&info, 0).await?;
                let source = StreamSource::Yandex { track_id: track.id.clone(), quality, info: info.clone() };
                let (data, len, head) = start_progressive_stream(c.clone(), source, resp).await?;
                // Container hint for Symphonia must match the actual byte wrapper (`m4a` for
                // `flac-mp4` / `aac-mp4`, `flac` for raw FLAC), while display codec reflects the
                // audio stream inside (`flac-mp4` -> FLAC).
                let ext = container_ext(&head, &info.codec);
                let codec = match AudioCodec::from_codec(&info.codec) {
                    AudioCodec::Other => AudioCodec::from_ext(ext),
                    c => c,
                };
                let est_kbps = estimate_bitrate_from_head(&head, len, ext, track.duration_ms);
                let bitrate_kbps = if codec == AudioCodec::Flac && est_kbps > 320 {
                    est_kbps
                } else if info.bitrate_kbps > 0 {
                    info.bitrate_kbps
                } else {
                    est_kbps
                };
                Ok(StreamAudio { data, ext, quality: StreamQuality { codec, bitrate_kbps }, from_disk: false, clean: false })
            }
            Backend::None => Err(no_session()),
        }
    }

    /// Probes the highest audio format and bitrate Yandex offers for `track` without downloading it.
    pub async fn max_quality(&self, track: &Track) -> Option<StreamQuality> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(_) => Some(fixture_max_quality(track)),
            Backend::Live(c) => {
                let info = c.stream(&track.id, ym_api::Quality::Best).await.ok()?;
                let codec = AudioCodec::from_codec(&info.codec);
                let bitrate_kbps = if codec == AudioCodec::Flac && (info.bitrate_kbps == 0 || info.bitrate_kbps <= 320) {
                    if let Some(dur_ms) = track.duration_ms.filter(|&d| d > 0) {
                        c.audio_size(&info).await.map_or(info.bitrate_kbps, |sz| ((sz * 8 + dur_ms / 2) / dur_ms) as u32)
                    } else {
                        info.bitrate_kbps
                    }
                } else if info.bitrate_kbps > 0 {
                    info.bitrate_kbps
                } else if let Some(dur_ms) = track.duration_ms.filter(|&d| d > 0) {
                    c.audio_size(&info).await.map_or(0, |sz| ((sz * 8 + dur_ms / 2) / dur_ms) as u32)
                } else {
                    0
                };
                Some(StreamQuality { codec, bitrate_kbps })
            }
            Backend::None => None,
        }
    }

    /// Raw image bytes from the CDN (JPEG/WebP/PNG), without decoding or re-encoding.
    pub async fn image_bytes(&self, url: &str) -> Option<Arc<[u8]>> {
        let _permit = limiter().acquire().await.ok()?;
        let Backend::Live(c) = self else { return None };
        let bytes = c.fetch_image(url).await.ok()?;
        Some(bytes.into())
    }

    /// A cover as pixels, for fixtures or callers that need decoded RGBA directly.
    pub async fn image(&self, url: &str, max_side: u32) -> Option<Decoded> {
        let max_side = max_side.min(art::COVER_LG);
        #[cfg(feature = "tooling")]
        if let Some((hue, style, seed, size)) = fixtures::parse_cover_url(url) {
            let _permit = decode_limiter().acquire().await.ok()?;
            return tokio::task::spawn_blocking(move || {
                let size = size.min(max_side);
                Decoded { width: size, height: size, rgba: fixtures::cover_rgba(hue, style, seed, size) }
            })
            .await
            .ok();
        }
        let bytes = self.image_bytes(url).await?;
        let _permit = decode_limiter().acquire().await.ok()?;
        tokio::task::spawn_blocking(move || art::decode(&bytes, max_side)).await.ok().flatten()
    }
}

/// A short, human reason for a failed request, in the user's language.
pub fn describe(e: &ym_api::Error, lang: crate::i18n::Lang) -> &'static str {
    use crate::i18n::S;
    match e {
        ym_api::Error::Network(_) => S::Offline.get(lang),
        ym_api::Error::RegionRestricted => S::NotAvailable.get(lang),
        ym_api::Error::Unauthorized => S::TokenRejected.get(lang),
        _ => S::PlaybackFailed.get(lang),
    }
}
