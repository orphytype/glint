//! Request signing and stream-URL construction.
//!
//! Yandex serves audio through "download-info" XML documents. The final CDN URL is
//! `https://{host}/get-mp3/{md5}/{ts}{path}` where `md5 = MD5(salt ‖ path[1..] ‖ s)`.
//! Lyrics requests are signed with an HMAC-SHA256 over `trackId ‖ timestamp`.
//!
//! The salt and key below are public constants shipped in every open-source client of the
//! service; they are not credentials.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use hmac::{Hmac, KeyInit, Mac};
use md5::{Digest, Md5};
use sha2::Sha256;

use crate::{Error, Result};

/// Salts for the signed `get-mp3` URL: what the current desktop client uses first, then the
/// long-standing one. The downloader tries them in order when the CDN answers 403.
pub const STREAM_SALTS: [&str; 2] = ["XDXFnebAaaWaytaSession", "XGRlBW9FXlekgbPrRHuSiA"];

/// HMAC key for signed lyrics requests.
const LYRICS_SIGN_KEY: &[u8] = b"kzqU4XhfCaY6B6JTHODeq5";

/// The four fields of a `<download-info>` document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadXml {
    pub host: String,
    pub path: String,
    pub ts: String,
    pub s: String,
}

fn xml_text(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    let raw = xml[start..end].trim();
    Some(raw.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'"))
}

pub fn parse_download_xml(xml: &str) -> Result<DownloadXml> {
    let get = |n: &str| xml_text(xml, n).filter(|v| !v.is_empty());
    match (get("host"), get("path"), get("ts"), get("s")) {
        (Some(host), Some(path), Some(ts), Some(s)) => {
            // The host is interpolated into a URL: refuse anything that is not a plain hostname.
            if !host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-') {
                return Err(Error::Parse("download-info host is not a hostname".into()));
            }
            Ok(DownloadXml { host, path, ts, s })
        }
        _ => Err(Error::Parse("download-info is missing host, path, ts or s".into())),
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

pub(crate) fn md5_hex(data: &[u8]) -> String {
    let mut h = Md5::new();
    h.update(data);
    hex(h.finalize().as_slice())
}

/// The signed direct URL for `salt`.
pub fn direct_url(info: &DownloadXml, salt: &str) -> String {
    let trimmed = info.path.strip_prefix('/').unwrap_or(&info.path);
    let sign = md5_hex(format!("{salt}{trimmed}{}", info.s).as_bytes());
    format!("https://{}/get-mp3/{sign}/{}/{trimmed}", info.host, info.ts)
}

fn hmac_sha256(key: &[u8], msg: &[u8]) -> Vec<u8> {
    // HMAC accepts keys of any length, so this cannot fail.
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes any key length");
    mac.update(msg);
    mac.finalize().into_bytes().to_vec()
}

/// Base64 HMAC-SHA256 of `trackId ‖ timestamp` for the signed lyrics endpoint.
pub fn lyrics_sign(track_id: &str, unix_ts: u64) -> String {
    STANDARD.encode(hmac_sha256(LYRICS_SIGN_KEY, format!("{track_id}{unix_ts}").as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_known_answer() {
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[test]
    fn hmac_sha256_rfc4231_case_2() {
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(hex(&mac), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
    }

    #[test]
    fn lyrics_sign_matches_the_protocol_vector() {
        assert_eq!(lyrics_sign("123", 1_700_000_000), "iZLECmQ6t14zZKM0V43n2bkNXeVaphhMIDWN2DqVghY=");
    }

    const XML: &str = "<?xml version='1.0' encoding='utf-8'?>\n<download-info><host>s208vla.storage.yandex.net</host><path>/rmusic/u-0/abc123/data-0.mp3</path><ts>0005a1b2c3d4e5f6</ts><region>-1</region><s>deadbeef</s></download-info>";

    #[test]
    fn parses_download_info() {
        let i = parse_download_xml(XML).unwrap();
        assert_eq!(i.host, "s208vla.storage.yandex.net");
        assert_eq!(i.path, "/rmusic/u-0/abc123/data-0.mp3");
        assert_eq!(i.ts, "0005a1b2c3d4e5f6");
        assert_eq!(i.s, "deadbeef");
    }

    #[test]
    fn direct_url_has_signature_and_normalised_path() {
        let i = parse_download_xml(XML).unwrap();
        let url = direct_url(&i, STREAM_SALTS[0]);
        let want = md5_hex(format!("{}rmusic/u-0/abc123/data-0.mp3deadbeef", STREAM_SALTS[0]).as_bytes());
        assert_eq!(url, format!("https://s208vla.storage.yandex.net/get-mp3/{want}/0005a1b2c3d4e5f6/rmusic/u-0/abc123/data-0.mp3"));
        // A path without the leading slash gives the same URL.
        let j = DownloadXml { path: i.path.trim_start_matches('/').into(), ..i.clone() };
        assert_eq!(direct_url(&j, STREAM_SALTS[0]), url);
        // The other salt gives a different signature.
        assert_ne!(direct_url(&i, STREAM_SALTS[1]), url);
    }

    #[test]
    fn rejects_incomplete_or_hostile_documents() {
        assert!(parse_download_xml("<download-info><host>a</host></download-info>").is_err());
        assert!(parse_download_xml("not xml").is_err());
        let evil = XML.replace("s208vla.storage.yandex.net", "evil.com/x?y=");
        assert!(parse_download_xml(&evil).is_err());
    }
}
