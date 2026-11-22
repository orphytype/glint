//! The signed `get-file-info` endpoint: what the current desktop client streams from, and the only
//! route to lossless (FLAC) audio. Requests carry an HMAC signature over the timestamp, the track,
//! the quality and the accepted codecs; the answer is a plain (unencrypted, `raw` transport) URL.

use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;

use crate::client::{FILE_INFO_CLIENT, from_value, https, seg};
use crate::{Client, Error, Quality, Result, StreamInfo, cover, sign};

/// Codecs this player decodes, best first. FLAC in an MP4 box is left out: not every decoder build
/// reads it, and the service always offers raw FLAC alongside it.
const CODECS: [&str; 3] = ["flac", "aac", "mp3"];
const TRANSPORT: &str = "raw";

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct FileInfo {
    quality: String,
    codec: String,
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
        let id = seg(track_id)?.split(':').next().unwrap_or(track_id).to_owned();
        let ts = crate::client::unix_millis() / 1000;
        let q = quality.file_info_name();
        let sign = sign::file_info_sign(ts, &id, q, &CODECS, TRANSPORT);
        let ts = ts.to_string();
        let codecs = CODECS.join(",");
        let rb = self.api(Method::GET, "/get-file-info").header("X-Yandex-Music-Client", FILE_INFO_CLIENT).query(&[
            ("ts", ts.as_str()),
            ("trackId", id.as_str()),
            ("quality", q),
            ("codecs", codecs.as_str()),
            ("transports", TRANSPORT),
            ("sign", sign.as_str()),
        ]);
        let mut v = self.call(rb).await?;
        let info: FileInfo = from_value(v.get_mut("downloadInfo").map(Value::take).unwrap_or(Value::Null), "file info")?;
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
