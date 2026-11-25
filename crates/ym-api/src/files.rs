//! The signed file-info endpoint: what the current desktop client streams from, and the only route
//! to lossless (FLAC) audio. Requests carry an HMAC signature over the timestamp, the track, the
//! quality and the accepted codecs; the answer is a plain (unencrypted, `raw` transport) URL.
//!
//! The service moved it from `get-file-info` to `resources/v2/file-info`. The query and the
//! signature are the same. The new one answers with the file info itself, where the old one
//! wrapped it in `downloadInfo`. The old path still answers, so it stays as the fallback.

use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;

use crate::client::{from_value, https, seg};
use crate::{Client, Error, Quality, Result, StreamInfo, cover, sign};

/// Codecs requested for lossless playback, best first. Unencrypted lossless on Yandex Music is
/// served primarily as `flac-mp4` (FLAC frames inside an ISO BMFF container), with raw `flac` and
/// lossy formats as fallbacks.
const CODECS_BEST: [&str; 5] = ["flac-mp4", "flac", "aac-mp4", "aac", "mp3"];
const CODECS_LOSSY: [&str; 3] = ["aac-mp4", "aac", "mp3"];
const TRANSPORT: &str = "raw";

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
}

impl Client {
    /// A playable URL for `track_id` from the signed file-info endpoint. Encrypted renditions
    /// (`encraw`) are refused; we ask for `raw` only.
    pub async fn file_info(&self, track_id: &str, quality: Quality) -> Result<StreamInfo> {
        match self.file_info_at("/get-file-info", track_id, quality).await {
            Ok(info) => Ok(info),
            Err(Error::Unauthorized) => Err(Error::Unauthorized),
            Err(_) => self.file_info_at("/resources/v2/file-info", track_id, quality).await,
        }
    }

    async fn file_info_at(&self, path: &str, track_id: &str, quality: Quality) -> Result<StreamInfo> {
        let id = seg(track_id)?.split(':').next().unwrap_or(track_id).to_owned();
        let ts = crate::client::unix_millis() / 1000;
        let q = quality.file_info_name();
        let codecs_list: &[&str] = if quality == Quality::Best { &CODECS_BEST } else { &CODECS_LOSSY };
        let sign = sign::file_info_sign(ts, &id, q, codecs_list, TRANSPORT);
        let ts = ts.to_string();
        let codecs = codecs_list.join(",");
        let rb = self.desktop_api(Method::GET, path).query(&[
            ("ts", ts.as_str()),
            ("trackId", id.as_str()),
            ("quality", q),
            ("codecs", codecs.as_str()),
            ("transports", TRANSPORT),
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
    if info.transport != TRANSPORT && !info.transport.is_empty() {
        return Err(Error::Api(format!("unsupported transport {}", info.transport)));
    }
    let mut urls: Vec<String> = info.urls.iter().chain(std::iter::once(&info.url)).filter(|u| !u.is_empty()).map(|u| https(u)).filter(|u| cover::is_trusted_host(u)).collect();
    urls.dedup();
    if urls.is_empty() {
        return Err(Error::Parse("file info has no usable URL".into()));
    }
    let codec = if info.codec.is_empty() { info.quality } else { info.codec };
    Ok(StreamInfo { urls, codec, bitrate_kbps: info.bitrate, size_bytes: info.size })
}
