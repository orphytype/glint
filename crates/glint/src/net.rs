//! Everything that talks to the outside world (or pretends to, in demo mode) lives behind
//! [`Backend`]. Its methods are `async` and run on a private tokio runtime; the UI never
//! blocks on them, it hands over a closure that applies the result on the UI thread.

use std::future::Future;
use std::sync::{Arc, OnceLock};

use tokio::sync::Semaphore;
use ym_api::{Account, Album, Artist, ArtistPage, Client, Lyrics, Playlist, RotorBatch, SearchKind, SearchResult, Shelf, Track, WaveEvent};

use crate::art::{self, Decoded};
use crate::demo::{self, Catalog};
use crate::store::DownloadStore;

pub type Result<T> = std::result::Result<T, ym_api::Error>;

/// The runtime every background job runs on.
pub fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| tokio::runtime::Builder::new_multi_thread().worker_threads(3).enable_all().thread_name("glint-net").build().expect("tokio runtime"))
}

pub fn spawn<T: Send + 'static>(fut: impl Future<Output = T> + Send + 'static) -> tokio::task::JoinHandle<T> {
    runtime().spawn(fut)
}

#[derive(Clone, Default)]
pub enum Backend {
    #[default]
    None,
    Demo(Arc<Catalog>),
    Live(Client),
}

/// What `audio` hands back: the file and the container it most likely is.
pub struct Audio {
    pub bytes: Arc<[u8]>,
    pub ext: &'static str,
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
        [b'I', b'D', b'3', ..] => "mp3",
        // An MPEG audio frame: eleven sync bits set.
        [0xff, b, ..] if b & 0xe0 == 0xe0 => "mp3",
        // Unknown: most of what the service serves under an MP3 URL really is MP3.
        _ => "mp3",
    }
}

fn limiter() -> &'static Semaphore {
    static S: OnceLock<Semaphore> = OnceLock::new();
    S.get_or_init(|| Semaphore::new(6))
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
    pub fn demo() -> Backend {
        Backend::Demo(Arc::new(Catalog::new()))
    }

    pub async fn account(&self) -> Result<Account> {
        match self {
            Backend::Demo(c) => Ok(c.account()),
            Backend::Live(c) => c.account().await,
            Backend::None => Err(no_session()),
        }
    }

    pub async fn home(&self) -> Result<Vec<Shelf>> {
        match self {
            Backend::Demo(c) => Ok(c.home()),
            Backend::Live(c) => c.home().await,
            Backend::None => Err(no_session()),
        }
    }

    pub async fn liked_tracks(&self, uid: i64) -> Result<Vec<Track>> {
        match self {
            Backend::Demo(c) => Ok(c.liked_tracks()),
            Backend::Live(c) => {
                let refs = c.liked_track_refs(uid).await?;
                let ids: Vec<String> = refs.iter().map(|r| r.id.clone()).collect();
                c.tracks(&ids).await
            }
            Backend::None => Err(no_session()),
        }
    }

    pub async fn playlists(&self, uid: i64) -> Result<Vec<Playlist>> {
        match self {
            Backend::Demo(c) => Ok(c
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
            Backend::Demo(c) => Ok(c
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
            Backend::Demo(c) => Ok(c.artists.clone()),
            Backend::Live(c) => c.liked_artists(uid).await,
            Backend::None => Err(no_session()),
        }
    }

    pub async fn album(&self, id: &str) -> Result<Album> {
        match self {
            Backend::Demo(c) => c.album(id).ok_or(ym_api::Error::Api("album not found".into())),
            Backend::Live(c) => c.album(id).await,
            Backend::None => Err(no_session()),
        }
    }

    pub async fn playlist(&self, uid: i64, kind: i64) -> Result<Playlist> {
        match self {
            Backend::Demo(c) => c.playlist(kind).ok_or(ym_api::Error::Api("playlist not found".into())),
            Backend::Live(c) => c.playlist(uid, kind).await,
            Backend::None => Err(no_session()),
        }
    }

    /// All of an artist's tracks (up to a few hundred), most popular first.
    pub async fn artist_tracks(&self, id: &str) -> Result<Vec<Track>> {
        match self {
            Backend::Demo(c) => Ok(c.artist_tracks(id)),
            Backend::Live(c) => c.artist_tracks(id, 300).await,
            Backend::None => Err(no_session()),
        }
    }

    /// The URL of the account's picture, if it has one.
    pub async fn avatar(&self) -> Option<String> {
        match self {
            Backend::Demo(_) => Some(ym_api::cover::url(&demo::avatar_template(), 200)),
            Backend::Live(c) => c.avatar().await.ok().flatten(),
            Backend::None => None,
        }
    }

    pub async fn artist(&self, id: &str) -> Result<ArtistPage> {
        match self {
            Backend::Demo(c) => c.artist(id).ok_or(ym_api::Error::Api("artist not found".into())),
            Backend::Live(c) => c.artist(id).await,
            Backend::None => Err(no_session()),
        }
    }

    pub async fn search(&self, text: &str, kind: SearchKind) -> Result<SearchResult> {
        match self {
            Backend::Demo(c) => Ok(c.search(text)),
            Backend::Live(c) => c.search(text, kind, 0).await,
            Backend::None => Err(no_session()),
        }
    }

    /// Likes or unlikes an album, an artist or a playlist (`"uid:kind"`). The demo keeps its likes
    /// in the app alone.
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
    pub async fn wave(&self, preset: &str, after: Option<&str>, round: u64) -> Result<RotorBatch> {
        match self {
            Backend::Demo(c) => Ok(RotorBatch { batch_id: Some(format!("demo-{round}")), tracks: c.wave(preset, round) }),
            Backend::Live(c) => {
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

    pub async fn lyrics(&self, track: &Track) -> Result<Lyrics> {
        match self {
            Backend::Demo(c) => Ok(c.lyrics(track)),
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
    /// existing downloads without adding ordinary listening to the offline library.
    pub async fn audio(&self, track: &Track, quality: ym_api::Quality, downloads: Option<std::sync::Arc<DownloadStore>>, fkcensor: &crate::fkcensor::FkCensor, fkcensor_on: bool, persist: bool) -> Result<Audio> {
        match self {
            Backend::Demo(_) => {
                let id = track.id.clone();
                let key = format!("{}-{:?}-original", track.id, quality);
                if let Some(store) = downloads.as_ref()
                    && let Some(bytes) = store.get_audio(&key)
                {
                    return Ok(Audio { bytes, ext: "wav" });
                }
                let bytes = tokio::task::spawn_blocking(move || demo::audio(&id)).await.ok().flatten().ok_or(ym_api::Error::Api("unknown demo track".into()))?;
                if persist && let Some(store) = downloads.as_ref() {
                    store.put_audio(&key, &bytes).map_err(|e| ym_api::Error::Api(format!("download storage: {e}")))?;
                }
                Ok(Audio { bytes, ext: "wav" })
            }
            Backend::Live(c) => {
                let replacement = fkcensor_on.then(|| fkcensor.replacement(&track.id)).flatten();
                let original_key = format!("{}-{:?}-original", track.id, quality);
                let replacement_key = format!("{}-{:?}-replacement", track.id, quality);
                if replacement.is_some()
                    && let Some(disk) = &downloads
                    && let Some(bytes) = disk.get_audio(&replacement_key)
                {
                    let ext = sniff_ext(&bytes);
                    return Ok(Audio { bytes, ext });
                }
                if let Some(replacement) = replacement
                    && let Ok(data) = c.fetch_fkcensor_audio(&replacement.url).await
                {
                    let bytes: std::sync::Arc<[u8]> = data.into();
                    let ext = sniff_ext(&bytes);
                    if persist && let Some(disk) = downloads.clone() {
                        let (k, b) = (replacement_key, bytes.clone());
                        tokio::task::spawn_blocking(move || disk.put_audio(&k, &b)).await.ok().and_then(|result| result.ok()).ok_or_else(|| ym_api::Error::Api("download storage failed".into()))?;
                    }
                    return Ok(Audio { bytes, ext });
                }
                if let Some(disk) = &downloads
                    && let Some(bytes) = disk.get_audio(&original_key)
                {
                    let ext = sniff_ext(&bytes);
                    return Ok(Audio { bytes, ext });
                }
                let info = c.stream(&track.id, quality).await?;
                let data = c.fetch_audio(&info, |_, _| {}).await?;
                let bytes: Arc<[u8]> = data.into();
                // The codec id is usually right; the bytes catch a server that names MP3 but
                // serves FLAC or AAC.
                let ext = if info.codec.contains("flac") { "flac" } else if info.codec.contains("aac") { "m4a" } else { sniff_ext(&bytes) };
                if persist && let Some(disk) = downloads {
                    let (k, b) = (original_key, bytes.clone());
                    tokio::task::spawn_blocking(move || disk.put_audio(&k, &b)).await.ok().and_then(|result| result.ok()).ok_or_else(|| ym_api::Error::Api("download storage failed".into()))?;
                }
                Ok(Audio { bytes, ext })
            }
            Backend::None => Err(no_session()),
        }
    }

    /// A cover as pixels: painted for demo covers, fetched and decoded for real ones.
    pub async fn image(&self, url: &str, max_side: u32) -> Option<Decoded> {
        let _permit = limiter().acquire().await.ok()?;
        if let Some((hue, style, seed, size)) = demo::parse_cover_url(url) {
            return tokio::task::spawn_blocking(move || {
                let size = size.min(max_side);
                Decoded { width: size, height: size, rgba: demo::cover_rgba(hue, style, seed, size) }
            })
            .await
            .ok();
        }
        let Backend::Live(c) = self else { return None };
        let bytes = c.fetch_image(url).await.ok()?;
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

#[cfg(test)]
mod tests {
    use super::sniff_ext;

    #[test]
    fn containers_are_recognised_from_their_bytes() {
        assert_eq!(sniff_ext(b"fLaC\x00\x00"), "flac");
        assert_eq!(sniff_ext(b"OggS\x00"), "ogg");
        assert_eq!(sniff_ext(b"RIFF\x24\x08\x00\x00WAVEfmt "), "wav");
        assert_eq!(sniff_ext(b"\x00\x00\x00\x20ftypM4A "), "m4a");
        assert_eq!(sniff_ext(b"ID3\x04\x00"), "mp3");
        assert_eq!(sniff_ext(&[0xff, 0xfb, 0x90, 0x00]), "mp3");
    }

    #[test]
    fn unknown_bytes_fall_back_to_mp3() {
        assert_eq!(sniff_ext(b""), "mp3");
        assert_eq!(sniff_ext(b"not audio at all"), "mp3");
        assert_eq!(sniff_ext(&[0xff, 0x00]), "mp3", "not an MPEG frame");
    }
}
