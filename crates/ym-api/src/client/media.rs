//! Audio stream resolution, downloads, FkCensor replacements and artwork fetching.

use super::parse::{https, seg};
use super::{CLIENT_ID, Client, MAX_AUDIO_BYTES, MAX_IMAGE_BYTES, Quality, StreamInfo};
use crate::models::{DownloadInfo, lossy_vec};
use crate::{Error, Result, cover, sign};

/// An open streaming audio HTTP response whose body can be read chunk-by-chunk as it arrives.
pub struct AudioResponse {
    resp: reqwest::Response,
    /// Total file size in bytes when known from `Content-Range`, `Content-Length`, or metadata.
    pub total_len: Option<u64>,
    /// True when the server signaled HTTP Range support (or answered with `206 Partial Content`).
    pub accepts_ranges: bool,
    /// True when the server answered `206 Partial Content` for a non-zero `from` offset.
    pub partial: bool,
}

impl AudioResponse {
    /// Reads the next chunk from the network, returning `Ok(None)` at EOF.
    pub async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>> {
        Ok(self.resp.chunk().await?.map(|b| b.to_vec()))
    }
}

impl Client {
    /// Resolves the audio URL(s) for a track. `Quality::Best` asks the signed `get-file-info`
    /// endpoint for FLAC (`flac-mp4`/`flac`) or the best available stream, falling back to
    /// `download-info` when that fails or offers a lower lossy bitrate.
    pub async fn stream(&self, track_id: &str, quality: Quality) -> Result<StreamInfo> {
        if quality == Quality::Best {
            match self.file_info(track_id, quality).await {
                Ok(info) if info.codec.to_ascii_lowercase().contains("flac") || info.bitrate_kbps >= 320 => return Ok(info),
                Ok(info) => {
                    return match self.legacy_stream(track_id, quality).await {
                        Ok(legacy) if legacy.codec.to_ascii_lowercase().contains("flac") || legacy.bitrate_kbps > info.bitrate_kbps => Ok(legacy),
                        _ => Ok(info),
                    };
                }
                Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                Err(_) => {}
            }
        }
        match self.legacy_stream(track_id, quality).await {
            Ok(info) => Ok(info),
            Err(Error::Unauthorized) => Err(Error::Unauthorized),
            Err(legacy_err) => match self.file_info(track_id, quality).await {
                Ok(info) => Ok(info),
                Err(_) => Err(legacy_err),
            },
        }
    }

    async fn legacy_stream(&self, track_id: &str, quality: Quality) -> Result<StreamInfo> {
        let v = self.get(&format!("/tracks/{}/download-info", seg(track_id)?), &[]).await?;
        let infos: Vec<DownloadInfo> = lossy_vec(v);
        let pick = pick_download_info(&infos, quality).ok_or(Error::RegionRestricted)?;
        let info_url = https(&pick.download_info_url);
        if !cover::is_trusted_host(&info_url) {
            return Err(Error::Parse("download-info points outside Yandex".into()));
        }
        let xml = self.fetch_text(&info_url).await?;
        let parsed = sign::parse_download_xml(&xml)?;
        Ok(StreamInfo {
            urls: sign::STREAM_SALTS.iter().map(|salt| sign::direct_url(&parsed, salt)).collect(),
            codec: pick.codec.clone(),
            bitrate_kbps: pick.bitrate_in_kbps,
            size_bytes: 0,
        })
    }

    /// Returns the audio file size in bytes from stream metadata, HTTP HEAD, or a 1-byte range probe.
    pub async fn audio_size(&self, info: &StreamInfo) -> Option<u64> {
        if info.size_bytes > 0 {
            return Some(info.size_bytes);
        }
        let timeout = std::time::Duration::from_secs(6);
        for url in &info.urls {
            if !cover::is_trusted_host(url) {
                continue;
            }
            if let Ok(resp) = self.inner.http.head(url).timeout(timeout).send().await
                && resp.status().is_success()
                && let Some(len) = resp.content_length().filter(|&n| n > 0)
            {
                return Some(len);
            }
            if let Ok(resp) = self.inner.http.get(url).header(reqwest::header::RANGE, "bytes=0-0").timeout(timeout).send().await
                && resp.status().is_success()
                && let Some(len) = content_range_total(&resp)
            {
                return Some(len);
            }
        }
        None
    }

    /// Downloads the audio file. `progress(downloaded, total)` is called as chunks arrive.
    pub async fn fetch_audio(&self, info: &StreamInfo, mut progress: impl FnMut(u64, Option<u64>)) -> Result<Vec<u8>> {
        let mut last = Error::Api("no stream URL".into());
        for url in &info.urls {
            if !cover::is_trusted_host(url) {
                continue;
            }
            match self.fetch(url, MAX_AUDIO_BYTES, None, &mut progress).await {
                Ok(bytes) => return Ok(bytes),
                // A wrong signature is a 403: the next salt may be the right one.
                Err(e @ Error::Http { .. }) => last = e,
                Err(e) => return Err(e),
            }
        }
        Err(last)
    }

    /// Streams the audio file directly to `path` on disk without buffering the whole file in RAM.
    pub async fn fetch_audio_to_file(&self, info: &StreamInfo, path: &std::path::Path) -> Result<(u64, Vec<u8>)> {
        let mut last = Error::Api("no stream URL".into());
        for url in &info.urls {
            if !cover::is_trusted_host(url) {
                continue;
            }
            match self.fetch_to_file(url, MAX_AUDIO_BYTES, path).await {
                Ok(out) => return Ok(out),
                Err(e @ Error::Http { .. }) => last = e,
                Err(e) => return Err(e),
            }
        }
        Err(last)
    }

    /// Streams a replacement audio file from FkCensorData directly to `path` on disk.
    pub async fn fetch_fkcensor_audio_to_file(&self, url: &str, path: &std::path::Path) -> Result<(u64, Vec<u8>)> {
        if !url.starts_with("https://raw.githubusercontent.com/") {
            return Err(Error::Parse("replacement audio points outside GitHub raw".into()));
        }
        self.fetch_to_file(url, MAX_AUDIO_BYTES, path).await
    }

    /// Opens a streaming HTTP response for `info` starting at byte `from` without reading the body.
    pub async fn open_audio(&self, info: &StreamInfo, from: u64) -> Result<AudioResponse> {
        let mut last = Error::Api("no stream URL".into());
        let hint_len = (info.size_bytes > 0).then_some(info.size_bytes);
        for url in &info.urls {
            if !cover::is_trusted_host(url) {
                continue;
            }
            match self.open_url_from(url, from, hint_len).await {
                Ok(resp) => return Ok(resp),
                Err(e @ Error::Http { .. }) => last = e,
                Err(e) => return Err(e),
            }
        }
        Err(last)
    }

    /// Opens a streaming HTTP response for a replacement audio file from FkCensorData.
    pub async fn open_fkcensor_audio(&self, url: &str, from: u64) -> Result<AudioResponse> {
        if !url.starts_with("https://raw.githubusercontent.com/") {
            return Err(Error::Parse("replacement audio points outside GitHub raw".into()));
        }
        self.open_url_from(url, from, None).await
    }

    async fn open_url_from(&self, url: &str, from: u64, hint_len: Option<u64>) -> Result<AudioResponse> {
        let mut req = self.inner.http.get(url).header(reqwest::header::USER_AGENT, CLIENT_ID);
        if from > 0 {
            req = req.header(reqwest::header::RANGE, format!("bytes={from}-"));
        }
        let resp = req.send().await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(Error::Http { status: status.as_u16(), body: String::new() });
        }
        let partial = status == reqwest::StatusCode::PARTIAL_CONTENT;
        let total_len = if partial {
            content_range_total(&resp).or_else(|| resp.content_length().map(|n| from.saturating_add(n))).or(hint_len)
        } else {
            resp.content_length().filter(|&n| n > 0).or(hint_len)
        };
        if total_len.is_some_and(|t| t > MAX_AUDIO_BYTES) {
            return Err(Error::Api("response is too large".into()));
        }
        let accepts_ranges = partial
            || resp
                .headers()
                .get(reqwest::header::ACCEPT_RANGES)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.eq_ignore_ascii_case("bytes"))
            || (from == 0 && total_len.is_some());
        Ok(AudioResponse { resp, total_len, accepts_ranges, partial })
    }

    /// Loads the public FkCensorData catalog without sending the account token.
    pub async fn fkcensor_catalog(&self) -> Result<crate::fkcensor::Catalog> {
        crate::fkcensor::fetch_catalog(&self.inner.http).await
    }

    /// Downloads a replacement audio file from the pinned FkCensorData host.
    pub async fn fetch_fkcensor_audio(&self, url: &str) -> Result<Vec<u8>> {
        if !url.starts_with("https://raw.githubusercontent.com/") {
            return Err(Error::Parse("replacement audio points outside GitHub raw".into()));
        }
        self.fetch(url, MAX_AUDIO_BYTES, None, |_, _| {}).await
    }

    /// Fetches synced LRC lyrics hosted by FkCensorData.
    pub async fn fetch_fkcensor_lrc(&self, url: &str) -> Result<String> {
        if !url.starts_with("https://raw.githubusercontent.com/fcensor-data/") {
            return Err(Error::Parse("replacement lyrics point outside FkCensorData".into()));
        }
        self.fetch_text(url).await
    }

    /// Fetches artwork from trusted Yandex or FkCensorData cover hosts.
    pub async fn fetch_image(&self, url: &str) -> Result<Vec<u8>> {
        if !cover::is_trusted_image_host(url) {
            return Err(Error::Api("refusing to fetch an image from an unexpected host".into()));
        }
        self.fetch(url, MAX_IMAGE_BYTES, Some(20), |_, _| {}).await
    }
}

/// Which audio rendition to download. Previews are never picked; formats we may not be able to
/// decode (HE-AAC) only when nothing else is offered; at equal bitrate MP3 wins over AAC.
fn pick_download_info(infos: &[DownloadInfo], quality: Quality) -> Option<&DownloadInfo> {
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

fn content_range_total(resp: &reqwest::Response) -> Option<u64> {
    let val = resp.headers().get(reqwest::header::CONTENT_RANGE)?.to_str().ok()?;
    let (_, total) = val.rsplit_once('/')?;
    total.trim().parse::<u64>().ok().filter(|&n| n > 0)
}
