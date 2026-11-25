//! The signed file-info endpoint: what the official desktop and mobile clients stream from, and
//! the route to lossless (FLAC) audio. Requests carry an HMAC signature over the timestamp, the
//! track, the quality, the accepted codecs, and the transport (`encraw` or `raw`).
//!
//! Yandex delivers most lossless `flac-mp4` streams via `encraw` (AES-128-CTR encrypted with a
//! per-response hex `key` and zero IV) and rejects or downgrades unencrypted `raw` on many tracks.
//! We request `encraw` first with the full lossless codec list, falling back to `raw` and the
//! v2 path when needed.

use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;

use crate::aes_ctr::parse_hex_key;
use crate::client::{from_value, https, seg};
use crate::{Client, Error, Quality, Result, StreamInfo, cover, sign};

/// Codecs requested for lossless playback, matching Yandex's official desktop/mobile priority.
const CODECS_BEST: [&str; 7] = ["flac", "flac-mp4", "aac-mp4", "he-aac-mp4", "aac", "he-aac", "mp3"];
const CODECS_LOSSY: [&str; 5] = ["aac-mp4", "he-aac-mp4", "aac", "he-aac", "mp3"];
const TRANSPORT_ENCRAW: &str = "encraw";
const TRANSPORT_RAW: &str = "raw";
/// Mobile client header matching `MarshalX/yandex-music-api`, used as a fallback if the desktop
/// header is rejected or returns a downgraded stream on `/get-file-info`.
const MOBILE_CLIENT: &str = "YandexMusicAndroid/24023621";

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct FileInfo {
    quality: String,
    codec: String,
    #[serde(alias = "bitrateInKbps")]
    bitrate: u32,
    size: u64,
    transport: String,
    url: String,
    urls: Vec<String>,
    key: Option<String>,
}

impl Client {
    /// A playable URL for `track_id` from the signed file-info endpoint. Requests `encraw`
    /// (AES-128-CTR) first so lossless `flac-mp4` is available on the full catalog, falling back
    /// to mobile headers, unencrypted `raw`, and `/resources/v2/file-info` when needed.
    pub async fn file_info(&self, track_id: &str, quality: Quality) -> Result<StreamInfo> {
        let mut last = Error::Api("file info unavailable".into());
        let mut best_lossy: Option<StreamInfo> = None;
        for (path, transport, mobile) in [
            ("/get-file-info", TRANSPORT_ENCRAW, false),
            ("/get-file-info", TRANSPORT_ENCRAW, true),
            ("/get-file-info", TRANSPORT_RAW, false),
            ("/resources/v2/file-info", TRANSPORT_ENCRAW, false),
            ("/resources/v2/file-info", TRANSPORT_RAW, false),
        ] {
            match self.file_info_at(path, track_id, quality, transport, mobile).await {
                Ok(info) => {
                    let is_flac = info.codec.to_ascii_lowercase().contains("flac");
                    if quality != Quality::Best || is_flac {
                        return Ok(info);
                    }
                    // Keep the best lossy rendition seen so far, and try the mobile header once
                    // in case the desktop client returned a downgraded stream.
                    let replace = best_lossy.as_ref().is_none_or(|prev| info.bitrate_kbps > prev.bitrate_kbps);
                    if replace {
                        best_lossy = Some(info);
                    }
                    if mobile {
                        break;
                    }
                }
                Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                Err(e) => last = e,
            }
        }
        best_lossy.ok_or(last)
    }

    async fn file_info_at(&self, path: &str, track_id: &str, quality: Quality, transport: &str, mobile: bool) -> Result<StreamInfo> {
        let id = seg(track_id)?.split(':').next().unwrap_or(track_id).to_owned();
        let ts = crate::client::unix_millis() / 1000;
        let q = quality.file_info_name();
        let codecs_list: &[&str] = if quality == Quality::Best { &CODECS_BEST } else { &CODECS_LOSSY };
        let sign = sign::file_info_sign(ts, &id, q, codecs_list, transport);
        let ts = ts.to_string();
        let codecs = codecs_list.join(",");
        let rb = if mobile {
            self.api(Method::GET, path).header("X-Yandex-Music-Client", MOBILE_CLIENT)
        } else {
            self.desktop_api(Method::GET, path)
        }
        .query(&[
            ("ts", ts.as_str()),
            ("trackId", id.as_str()),
            ("quality", q),
            ("codecs", codecs.as_str()),
            ("transports", transport),
            ("sign", sign.as_str()),
        ]);
        let mut v = self.call(rb).await?;
        // The old path wraps it in `downloadInfo`; the new one answers with it as it is.
        let body = match ["fileInfo", "downloadInfo"].iter().find_map(|k| v.get_mut(*k).map(Value::take)) {
            Some(inner) => inner,
            None => v,
        };
        let info: FileInfo = from_value(body, "file info")?;
        stream_from(info)
    }
}

fn stream_from(info: FileInfo) -> Result<StreamInfo> {
    let key = match info.transport.as_str() {
        "" | TRANSPORT_RAW => None,
        TRANSPORT_ENCRAW => {
            let hex = info.key.as_deref().ok_or_else(|| Error::Parse("encraw file info is missing key".into()))?;
            let parsed = parse_hex_key(hex).ok_or_else(|| Error::Parse("encraw key is not 16 hex bytes".into()))?;
            Some(parsed)
        }
        other => return Err(Error::Api(format!("unsupported transport {other}"))),
    };
    let mut urls: Vec<String> = info.urls.iter().chain(std::iter::once(&info.url)).filter(|u| !u.is_empty()).map(|u| https(u)).filter(|u| cover::is_trusted_host(u)).collect();
    urls.dedup();
    if urls.is_empty() {
        return Err(Error::Parse("file info has no usable URL".into()));
    }
    let codec = if info.codec.is_empty() { info.quality } else { info.codec };
    Ok(StreamInfo { urls, codec, bitrate_kbps: info.bitrate, size_bytes: info.size, key })
}
