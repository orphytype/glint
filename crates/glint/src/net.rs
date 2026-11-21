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
        let workers = std::thread::available_parallelism().map_or(2, std::num::NonZero::get).clamp(2, 4);
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

/// What `audio` hands back: the file and the container it most likely is.
pub struct Audio {
    pub bytes: Arc<[u8]>,
    pub ext: &'static str,
    /// FckCensor's uncensored version of the track, not Yandex's own.
    pub clean: bool,
}

/// The container an audio file really is, from its first bytes. Cached downloads have no codec
/// description to go by, and files served as MP3 are sometimes FLAC or AAC inside.
fn sniff_ext(bytes: &[u8]) -> &'static str {
    match bytes {
        [b'f', b'L', b'a', b'C', ..] => "flac",
        [b'O', b'g', b'g', b'S', ..] => "ogg",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'A', b'V', b'E', ..] => "wav",
        // ISO base media (`ftyp` at offset 4): M4A/AAC in practice.
        [_, _, _, _, b'f', b't', b'y', b'p', ..] => "m4a",
        // ID3 tag, MPEG sync frame, or unknown container: Yandex serves MP3 by default.
        _ => "mp3",
    }
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

    pub async fn liked_tracks(&self, uid: i64) -> Result<Vec<Track>> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => Ok(c.liked_tracks()),
            Backend::Live(c) => {
                let refs = c.liked_track_refs(uid).await?;
                let ids: Vec<String> = refs.iter().map(|r| r.id.clone()).collect();
                c.tracks(&ids).await
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

    pub async fn album(&self, id: &str) -> Result<Album> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => c.album(id).ok_or(ym_api::Error::Api("album not found".into())),
            Backend::Live(c) => c.album(id).await,
            Backend::None => Err(no_session()),
        }
    }

    pub async fn playlist(&self, uid: i64, kind: i64) -> Result<Playlist> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => c.playlist(kind).ok_or(ym_api::Error::Api("playlist not found".into())),
            Backend::Live(c) => c.playlist(uid, kind).await,
            Backend::None => Err(no_session()),
        }
    }

    /// All of an artist's tracks (up to the API's 500-track page), most popular first.
    pub async fn artist_tracks(&self, id: &str) -> Result<Vec<Track>> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => Ok(c.artist_tracks(id)),
            Backend::Live(c) => c.artist_tracks(id, 500).await,
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

    pub async fn artist(&self, id: &str) -> Result<ArtistPage> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => c.artist(id).ok_or(ym_api::Error::Api("artist not found".into())),
            Backend::Live(c) => c.artist(id).await,
            Backend::None => Err(no_session()),
        }
    }

    pub async fn search(&self, text: &str, kind: SearchKind) -> Result<SearchResult> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => Ok(c.search(text)),
            Backend::Live(c) => c.search(text, kind, 0).await,
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

    /// The next batch of a wave station. `after` is the track the previous batch ended on.
    /// A batch of a wave: one of My Wave's presets by its id, a station seeded by one thing
    /// (`"track:ID"`, `"artist:ID"`, `"album:ID"`), or `"collection"` (My Wave on what one likes).
    ///
    /// `knobs` are the wave's own settings, for one of My Wave's (`WaveTune::applies`). My Wave's
    /// station plays as the account's knobs are set, whatever seeds it is asked with: a wave
    /// that named only its own knob ("Favorite") played on in the mood the last one left there
    /// ("Sad"). So its first batch sets all three on the account first, as the service's own
    /// apps do, and asks with all three.
    #[cfg_attr(not(feature = "tooling"), allow(unused_variables))]
    pub async fn wave(&self, preset: &str, knobs: Option<ym_api::WaveSettings>, after: Option<&str>, round: u64) -> Result<RotorBatch> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => Ok(RotorBatch { batch_id: Some(format!("fixture-{round}")), tracks: c.wave(preset, round) }),
            Backend::Live(c) => {
                if let Some(k) = knobs {
                    if after.is_none() {
                        // Best-effort: the seeds below still ask for the same.
                        let _ = c.set_wave_settings(k).await;
                    }
                    let seeds: Vec<String> = k.seeds().into_iter().skip(1).collect();
                    let seeds: Vec<&str> = seeds.iter().map(String::as_str).collect();
                    return c.wave_tracks("user:onyourwave", &seeds, after).await;
                }
                let (station, seeds) = station_of(preset);
                c.wave_tracks(station, seeds, after).await
            }
            Backend::None => Err(no_session()),
        }
    }

    pub async fn wave_feedback(&self, preset: &str, event: WaveEvent, track_id: &str, batch_id: Option<&str>, played_secs: f64) {
        if let Backend::Live(c) = self {
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

    /// Adds `track` at the end of one of the account's playlists. Edits quote the playlist's
    /// revision, so it is read fresh first.
    pub async fn playlist_add(&self, uid: i64, kind: i64, track: &Track) -> Result<()> {
        match self {
            Backend::Live(c) => {
                let list = c.playlist(uid, kind).await?;
                let album = track.albums.first().map(|a| a.id.clone()).unwrap_or_default();
                let at = list.track_count.map_or(list.tracks.len(), |n| n as usize);
                c.playlist_insert(uid, kind, list.revision, at, &[(track.id.clone(), album)]).await.map(drop)
            }
            #[cfg(feature = "tooling")]
            Backend::Fixture(_) => Ok(()),
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

    pub async fn lyrics(&self, track: &Track) -> Result<Lyrics> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(c) => Ok(c.lyrics(track)),
            Backend::Live(c) => c.lyrics(track).await,
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

    /// The audio file of `track`, from the downloaded-track store when it is there. Playback reads
    /// existing downloads without adding ordinary listening to the offline library. `exact`: only a
    /// copy in `quality` counts (a download in a new quality must fetch it, not copy an old one).
    pub async fn audio(
        &self,
        track: &Track,
        quality: ym_api::Quality,
        downloads: Option<Arc<DownloadStore>>,
        fkcensor: &crate::fkcensor::FkCensor,
        fkcensor_on: bool,
        exact: bool,
    ) -> Result<Audio> {
        match self {
            #[cfg(feature = "tooling")]
            Backend::Fixture(_) => Err(ym_api::Error::Api("the scene fixtures have no audio".into())),
            Backend::Live(c) => {
                let replacement = fkcensor_on.then(|| fkcensor.replacement(&track.id)).flatten();
                let q = format!("{quality:?}");
                // A downloaded copy in any quality plays before anything is asked of the network,
                // so downloads play offline whatever the quality setting is now.
                let find = |variant: &str| {
                    let disk = downloads.as_ref()?;
                    if exact { disk.get_audio(&format!("{}-{q}-{variant}", track.id)) } else { disk.find_audio(&track.id, &q, variant) }
                };
                if replacement.is_some()
                    && let Some(bytes) = find("replacement")
                {
                    let ext = sniff_ext(&bytes);
                    return Ok(Audio { bytes, ext, clean: true });
                }
                let local = find("original");
                if replacement.is_none()
                    && let Some(bytes) = local.clone()
                {
                    let ext = sniff_ext(&bytes);
                    return Ok(Audio { bytes, ext, clean: false });
                }
                // With the original on disk, the clean version gets only a short wait: offline,
                // the download must not sit behind a connection that never comes.
                let wait = if local.is_some() { std::time::Duration::from_secs(4) } else { std::time::Duration::from_secs(120) };
                if let Some(replacement) = replacement
                    && let Ok(Ok(data)) = tokio::time::timeout(wait, c.fetch_fkcensor_audio(&replacement.url)).await
                {
                    let bytes: Arc<[u8]> = data.into();
                    let ext = sniff_ext(&bytes);
                    return Ok(Audio { bytes, ext, clean: true });
                }
                if let Some(bytes) = local {
                    let ext = sniff_ext(&bytes);
                    return Ok(Audio { bytes, ext, clean: false });
                }
                let info = c.stream(&track.id, quality).await?;
                let data = c.fetch_audio(&info, |_, _| {}).await?;
                let bytes: Arc<[u8]> = data.into();
                // The codec id is usually right; the bytes catch a server that names MP3 but
                // serves FLAC or AAC.
                let ext = if info.codec.contains("flac") {
                    "flac"
                } else if info.codec.contains("aac") {
                    "m4a"
                } else {
                    sniff_ext(&bytes)
                };
                Ok(Audio { bytes, ext, clean: false })
            }
            Backend::None => Err(no_session()),
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
        let max_side = max_side.min(art::COVER_PX);
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
