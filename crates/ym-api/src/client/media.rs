//! Audio stream resolution, downloads, FkCensor replacements and artwork fetching.

use std::collections::HashMap;

use super::parse::{https, seg};
use super::{Client, MAX_AUDIO_BYTES, MAX_IMAGE_BYTES, Quality, StreamInfo};
use crate::models::{DownloadInfo, lossy_vec};
use crate::{Error, Result, cover, sign};

impl Client {
    /// Resolves the audio URL(s) for a track. Lossless (and the plain-file renditions the
    /// current desktop client streams) come from the signed `get-file-info` endpoint; when that
    /// refuses, the long-standing `download-info` route answers instead.
    pub async fn stream(&self, track_id: &str, quality: Quality) -> Result<StreamInfo> {
        if quality == Quality::Lossless {
            match self.file_info(track_id, quality).await {
                Ok(info) => return Ok(info),
                Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                Err(_) => {}
            }
        }
        self.legacy_stream(track_id, quality).await
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
        Quality::Best | Quality::Lossless => usable.into_iter().max_by_key(|i| (i.bitrate_in_kbps, std::cmp::Reverse(rank(&i.codec)))),
        Quality::Balanced => {
            const CAP: u32 = 192;
            let under = usable.iter().copied().filter(|i| i.bitrate_in_kbps <= CAP).max_by_key(|i| (i.bitrate_in_kbps, std::cmp::Reverse(rank(&i.codec))));
            under.or_else(|| usable.into_iter().min_by_key(|i| (i.bitrate_in_kbps, rank(&i.codec))))
        }
    }
}
