//! Lyrics orchestration across lrc.red, Yandex Music and LRCLIB.

use std::sync::PoisonError;
use std::time::Duration;

use reqwest::header::USER_AGENT;
use serde::Deserialize;
use serde_json::Value;

use super::parse::{https, seg, unix_millis};
use super::{APP_UA, Client, LRCRED_SEEN_MAX, MAX_TEXT_BYTES, read_limited};
use crate::lrc::{Lyrics, LyricsSource};
use crate::models::{Track, lossy_vec, strip_nulls};
use crate::{Result, cover, lrcred, sign};

const LRCLIB: &str = "https://lrclib.net/api";
const LRCRED: &str = lrcred::HOME;

impl Client {
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
        if let Some(seen) = self.inner.lrcred_seen.lock().unwrap_or_else(PoisonError::into_inner).get(track.id.as_str()) {
            return seen.clone();
        }
        let found = self.lrcred_lookup(track, &ua).await;
        // A transport failure is not an answer: only remember what lrc.red actually said.
        if let Ok(found) = &found {
            let mut seen = self.inner.lrcred_seen.lock().unwrap_or_else(PoisonError::into_inner);
            if seen.len() >= LRCRED_SEEN_MAX {
                seen.clear();
            }
            seen.insert(track.id.clone(), found.clone());
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
                v.get_mut("hits").map(Value::take)
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
        let target = TrackNorm::of(track);

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
            && target.matches_lrclib(&item)
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
        Ok(best_lrclib_match(&items, &target))
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

/// Pre-normalized target track fields so search hits do not re-normalize the query track per hit.
struct TrackNorm {
    title: String,
    artists: Vec<String>,
    duration_secs: Option<f64>,
}

impl TrackNorm {
    fn of(track: &Track) -> Self {
        Self {
            title: norm(&track.title),
            artists: track.artists.iter().map(|a| norm(&a.name)).collect(),
            duration_secs: track.duration_ms.filter(|d| *d > 0).map(|d| d as f64 / 1000.0),
        }
    }

    fn matches_title(&self, candidate: &str) -> bool {
        similar_norm(&self.title, &norm(candidate))
    }

    fn matches_artist(&self, candidate: &str) -> bool {
        if self.artists.is_empty() || candidate.is_empty() {
            return true;
        }
        let other = norm(candidate);
        self.artists.iter().any(|a| similar_norm(a, &other))
    }

    fn matches_lrclib(&self, item: &LrclibItem) -> bool {
        if let (Some(theirs), Some(ours)) = (item.duration.filter(|d| *d > 0.0), self.duration_secs)
            && (theirs - ours).abs() > 4.5
        {
            return false;
        }
        if !self.matches_title(item.track_name.as_deref().unwrap_or("")) {
            return false;
        }
        self.matches_artist(item.artist_name.as_deref().unwrap_or(""))
    }
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

fn similar_norm(a: &str, b: &str) -> bool {
    !a.is_empty() && !b.is_empty() && (a == b || (a.chars().count() >= 4 && b.contains(a)) || (b.chars().count() >= 4 && a.contains(b)))
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
    let target = TrackNorm::of(track);
    let gap = |h: &lrcred::Hit| match (h.duration, target.duration_secs) {
        (Some(t), Some(o)) => (t - o).abs(),
        _ => 0.0,
    };
    hits.iter()
        .filter(|h| lrcred::valid_isrc(&h.isrc))
        .filter(|h| target.matches_title(&h.title))
        .filter(|h| target.matches_artist(&h.artist))
        .filter(|h| h.duration.filter(|d| *d > 0.0).is_none() || target.duration_secs.is_none() || gap(h) <= 4.5)
        .min_by(|a, b| gap(a).total_cmp(&gap(b)))
}

/// Among search hits, the matching one with synced lyrics if any, else any matching one.
fn best_lrclib_match(items: &[LrclibItem], target: &TrackNorm) -> Option<Lyrics> {
    let mut plain: Option<Lyrics> = None;
    for item in items.iter().filter(|i| target.matches_lrclib(i)) {
        match lyrics_from_item(item) {
            Some(l) if l.synced || l.instrumental => return Some(l),
            Some(l) if plain.is_none() => plain = Some(l),
            _ => {}
        }
    }
    plain
}
