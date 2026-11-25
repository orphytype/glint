//! Lyrics orchestration across lrc.red, Spicy Lyrics, Yandex Music, Musixmatch and LRCLIB.

use std::sync::PoisonError;
use std::time::{Duration, Instant};

use reqwest::header::{AUTHORIZATION, COOKIE, USER_AGENT};
use serde::Deserialize;
use serde_json::Value;

use super::parse::{https, seg, unix_millis};
use super::{APP_UA, Client, LRCRED_SEEN_MAX, MAX_TEXT_BYTES, read_limited};
use crate::lrc::{LyricLine, LyricWord, Lyrics, LyricsSource, join_words};
use crate::models::{Track, lossy_vec, strip_nulls};
use crate::{Result, cover, lrcred, sign};

const LRCLIB: &str = "https://lrclib.net/api";
const LRCRED: &str = lrcred::HOME;
const SPICY_API: &str = "https://api.spicylyrics.org";
const MXM_API: &str = "https://apic-desktop.musixmatch.com/ws/1.1";
const MXM_APP_ID: &str = "web-desktop-app-v1.0";
const MXM_COOKIE: &str = "x-mxm-token-guid=";

impl Client {
    /// Fetches lyrics across all configured providers, preferring word-synced, then line-synced,
    /// then plain text:
    /// 1. lrc.red (word-synced TTML when enabled)
    /// 2. Spicy Lyrics (`api.spicylyrics.org`: syllable-synced or line-synced)
    /// 3. Yandex Music (`LRC` signed endpoint, with `TEXT` kept as a plain fallback)
    /// 4. Musixmatch (`macro.subtitles.get`: richsync word-synced, LRC subtitles, or plain)
    /// 5. LRCLIB (LRC synced or plain)
    pub async fn lyrics(&self, track: &Track) -> Result<Lyrics> {
        let mut best_synced: Option<Lyrics> = None;
        let mut best_plain: Option<Lyrics> = None;

        // 1. lrc.red (word-synced)
        if let Some(found) = self.lrcred_lyrics(track).await
            && let Some(done) = accept_lyrics(found, &mut best_synced, &mut best_plain)
        {
            return Ok(done);
        }

        // 2. Spicy Lyrics (syllable/word-synced or line-synced)
        if let Some(found) = self.spicy_lyrics(track).await
            && let Some(done) = accept_lyrics(found, &mut best_synced, &mut best_plain)
        {
            return Ok(done);
        }

        // 3. Yandex Music (synced LRC first; keep plain TEXT only as fallback)
        if self.has_token()
            && !track.id.is_empty()
            && let Ok(Some(found)) = self.yandex_lyrics(&track.id, "LRC").await
            && let Some(done) = accept_lyrics(found, &mut best_synced, &mut best_plain)
        {
            return Ok(done);
        }

        // 4. Musixmatch (richsync word-synced, LRC synced, or plain)
        // Even if Yandex already gave line-synced LRC, Musixmatch richsync can upgrade it to
        // word-synced; if we already have synced lyrics from Yandex, keep the probe brief.
        let mxm_timeout = if best_synced.is_some() { Duration::from_secs(3) } else { Duration::from_secs(8) };
        if let Ok(Some(found)) = tokio::time::timeout(mxm_timeout, self.musixmatch_lyrics(track)).await
            && let Some(done) = accept_lyrics(found, &mut best_synced, &mut best_plain)
        {
            return Ok(done);
        }

        if let Some(synced) = best_synced {
            return Ok(synced);
        }

        // 5. LRCLIB (line-synced or plain)
        if let Ok(Some(found)) = self.lrclib(track).await {
            if let Some(done) = accept_lyrics(found, &mut best_synced, &mut best_plain) {
                return Ok(done);
            }
            if let Some(synced) = best_synced {
                return Ok(synced);
            }
        }

        // 6. Yandex Music plain TEXT fallback
        if best_plain.is_none()
            && self.has_token()
            && !track.id.is_empty()
            && let Ok(Some(found)) = self.yandex_lyrics(&track.id, "TEXT").await
        {
            let _ = accept_lyrics(found, &mut best_synced, &mut best_plain);
        }

        Ok(best_plain.unwrap_or_default())
    }

    // ---- lrc.red ----------------------------------------------------------------------------------------

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

    // ---- Spicy Lyrics (developers.spicylyrics.org) ------------------------------------------------------

    async fn spicy_lyrics(&self, track: &Track) -> Option<Lyrics> {
        let title = track.title.trim();
        let artist = track.primary_artist().trim();
        if title.is_empty() {
            return None;
        }
        let target = TrackNorm::of(track);
        let key = self.inner.spicy_key.read().unwrap_or_else(PoisonError::into_inner).clone();

        // 1. Track search on Spicy Lyrics v1 API to resolve a track ID (Spotify/ISRC/Apple).
        let mut query = vec![("title", title)];
        if !artist.is_empty() {
            query.push(("artist", artist));
        }
        let search_val = self.spicy_get("/v1/tracks/search", &query, key.as_deref()).await?;
        let track_id = pick_spicy_track_id(&search_val, &target)?;

        // 2. Fetch syllable or line-synced lyrics for the matched track ID.
        if let Some(body) = self.spicy_get(&format!("/v1/lyrics/{track_id}"), &[], key.as_deref()).await
            && let Some(parsed) = parse_spicy_payload(&body)
        {
            return Some(parsed);
        }

        // 3. Fallback to Spicy Lyrics batch `/query` route for public client responses.
        let batch = self.spicy_query_id(&track_id, key.as_deref()).await?;
        parse_spicy_payload(&batch)
    }

    async fn spicy_get(&self, path: &str, query: &[(&str, &str)], key: Option<&str>) -> Option<Value> {
        let mut rb = self.inner.http.get(format!("{SPICY_API}{path}")).query(query).header(USER_AGENT, APP_UA).timeout(Duration::from_secs(7));
        if let Some(k) = key {
            rb = rb.header(AUTHORIZATION, format!("Bearer {k}"));
        }
        let resp = rb.send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let body = read_limited(resp, MAX_TEXT_BYTES, |_, _| {}).await.ok()?;
        serde_json::from_slice::<Value>(&body).ok()
    }

    async fn spicy_query_id(&self, track_id: &str, key: Option<&str>) -> Option<Value> {
        let payload = serde_json::json!({
            "queries": [{ "operation": "lyrics", "variables": { "id": track_id } }],
            "client": { "version": env!("CARGO_PKG_VERSION") }
        });
        let mut rb = self
            .inner
            .http
            .post(format!("{SPICY_API}/query"))
            .header(USER_AGENT, APP_UA)
            .header("X-mode", "2")
            .json(&payload)
            .timeout(Duration::from_secs(7));
        if let Some(k) = key {
            rb = rb.header(AUTHORIZATION, format!("Bearer {k}"));
        }
        let resp = rb.send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let body = read_limited(resp, MAX_TEXT_BYTES, |_, _| {}).await.ok()?;
        let val = serde_json::from_slice::<Value>(&body).ok()?;
        val.pointer("/queries/0/result/responseData").cloned().or(Some(val))
    }

    // ---- Yandex Music -----------------------------------------------------------------------------------

    async fn yandex_lyrics(&self, track_id: &str, format: &str) -> Result<Option<Lyrics>> {
        let ts = unix_millis() / 1000;
        let signature = sign::lyrics_sign(track_id, ts);
        let ts = ts.to_string();
        let v = self.get(&format!("/tracks/{}/lyrics", seg(track_id)?), &[("format", format), ("timeStamp", &ts), ("sign", &signature)]).await?;
        let Some(url) = v.get("downloadUrl").and_then(Value::as_str).map(https) else { return Ok(None) };
        if !cover::is_trusted_host(&url) {
            return Ok(None);
        }
        let raw = self.fetch_text(&url).await?;
        let parsed = Lyrics::from_lrc(&raw, LyricsSource::Yandex);
        Ok((!parsed.is_empty()).then_some(parsed))
    }

    // ---- Musixmatch -------------------------------------------------------------------------------------

    async fn musixmatch_token(&self) -> Option<String> {
        {
            let guard = self.inner.mxm_token.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some((tok, exp)) = guard.as_ref()
                && Instant::now() < *exp
            {
                return Some(tok.clone());
            }
        }
        let resp = self
            .inner
            .http
            .get(format!("{MXM_API}/token.get"))
            .query(&[("app_id", MXM_APP_ID), ("format", "json")])
            .header(USER_AGENT, APP_UA)
            .header(COOKIE, MXM_COOKIE)
            .timeout(Duration::from_secs(6))
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let bytes = read_limited(resp, MAX_TEXT_BYTES, |_, _| {}).await.ok()?;
        let val: Value = serde_json::from_slice(&bytes).ok()?;
        if val.pointer("/message/header/status_code").and_then(Value::as_i64) != Some(200) {
            return None;
        }
        let tok = val.pointer("/message/body/user_token").and_then(Value::as_str)?.trim().to_owned();
        if tok.is_empty() || tok.contains("UpgradeOnly") {
            return None;
        }
        *self.inner.mxm_token.lock().unwrap_or_else(PoisonError::into_inner) = Some((tok.clone(), Instant::now() + Duration::from_secs(12 * 60)));
        Some(tok)
    }

    async fn musixmatch_lyrics(&self, track: &Track) -> Option<Lyrics> {
        let title = track.title.trim();
        let artist = track.primary_artist().trim();
        if title.is_empty() {
            return None;
        }
        let token = self.musixmatch_token().await?;
        let secs = track.duration_ms.filter(|&ms| ms > 0).map(|ms| (ms / 1000).to_string());
        let album = track.album().map(|a| a.title.trim()).unwrap_or("");
        let mut query = vec![
            ("format", "json"),
            ("namespace", "lyrics_richsynched"),
            ("subtitle_format", "lrc"),
            ("app_id", MXM_APP_ID),
            ("usertoken", token.as_str()),
            ("q_track", title),
        ];
        if !artist.is_empty() {
            query.push(("q_artist", artist));
        }
        if !album.is_empty() {
            query.push(("q_album", album));
        }
        if let Some(s) = secs.as_deref() {
            query.push(("q_duration", s));
            query.push(("f_subtitle_length", s));
            query.push(("f_subtitle_length_max_deviation", "6"));
        }
        let resp = self
            .inner
            .http
            .get(format!("{MXM_API}/macro.subtitles.get"))
            .query(&query)
            .header(USER_AGENT, APP_UA)
            .header(COOKIE, MXM_COOKIE)
            .timeout(Duration::from_secs(7))
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let bytes = read_limited(resp, MAX_TEXT_BYTES, |_, _| {}).await.ok()?;
        let val: Value = serde_json::from_slice(&bytes).ok()?;
        let code = val.pointer("/message/header/status_code").and_then(Value::as_i64).unwrap_or(0);
        if code == 401 {
            // Token expired or rate-limited: clear cache and fall back to next source.
            *self.inner.mxm_token.lock().unwrap_or_else(PoisonError::into_inner) = None;
            return None;
        }
        if code != 200 {
            return None;
        }
        let calls = val.pointer("/message/body/macro_calls")?;
        // Verify matched track metadata before trusting lyrics.
        let target = TrackNorm::of(track);
        if let Some(tr) = calls.pointer("/matcher.track.get/message/body/track") {
            let m_title = tr.get("track_name").and_then(Value::as_str).unwrap_or("");
            let m_artist = tr.get("artist_name").and_then(Value::as_str).unwrap_or("");
            let m_dur = tr.get("track_length").and_then(Value::as_f64).filter(|&d| d > 0.0);
            if !target.matches_title(m_title) || !target.matches_artist(m_artist) {
                return None;
            }
            if let (Some(theirs), Some(ours)) = (m_dur, target.duration_secs)
                && (theirs - ours).abs() > 6.5
            {
                return None;
            }
            if tr.get("instrumental").and_then(Value::as_i64) == Some(1) {
                return Some(Lyrics { source: LyricsSource::Musixmatch, ..Lyrics::instrumental() });
            }
        }
        // 1. Word-synced richsync JSON
        if let Some(rich_str) = calls.pointer("/track.richsync.get/message/body/richsync/richsync_body").and_then(Value::as_str)
            && let Some(parsed) = parse_musixmatch_richsync(rich_str)
        {
            return Some(parsed);
        }
        // 2. Line-synced LRC subtitles
        if let Some(lrc_str) = calls.pointer("/track.subtitles.get/message/body/subtitle_list/0/subtitle/subtitle_body").and_then(Value::as_str) {
            let parsed = Lyrics::from_lrc(lrc_str, LyricsSource::Musixmatch);
            if parsed.synced && !parsed.is_empty() {
                return Some(parsed);
            }
        }
        // 3. Plain lyrics
        if let Some(plain_str) = calls.pointer("/track.lyrics.get/message/body/lyrics/lyrics_body").and_then(Value::as_str) {
            let cleaned: String = plain_str.lines().take_while(|l| !l.contains("******* This Lyrics is NOT for Commercial use")).collect::<Vec<_>>().join("\n");
            let parsed = Lyrics::from_lrc(&cleaned, LyricsSource::Musixmatch);
            if !parsed.is_empty() {
                return Some(parsed);
            }
        }
        None
    }

    // ---- LRCLIB -----------------------------------------------------------------------------------------

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

// ---- Spicy Lyrics parsing ---------------------------------------------------------------------------

fn accept_lyrics(cand: Lyrics, best_synced: &mut Option<Lyrics>, best_plain: &mut Option<Lyrics>) -> Option<Lyrics> {
    if cand.is_empty() {
        return None;
    }
    if cand.word_synced {
        return Some(cand);
    }
    if cand.synced || cand.instrumental {
        if best_synced.is_none() {
            *best_synced = Some(cand);
        }
    } else if best_plain.is_none() {
        *best_plain = Some(cand);
    }
    None
}

fn pick_spicy_track_id(val: &Value, target: &TrackNorm) -> Option<String> {
    let arr = val
        .as_array()
        .or_else(|| val.get("tracks").and_then(Value::as_array))
        .or_else(|| val.get("results").and_then(Value::as_array))
        .or_else(|| val.pointer("/Body/tracks").and_then(Value::as_array))
        .or_else(|| val.pointer("/Body/results").and_then(Value::as_array))
        .or_else(|| val.get("data").and_then(Value::as_array))?;
    let mut best: Option<(f64, String)> = None;
    for item in arr {
        let title = item.get("title").or_else(|| item.get("name")).and_then(Value::as_str).unwrap_or("");
        if !target.matches_title(title) {
            continue;
        }
        let artist_ok = if let Some(a) = item.get("artist").and_then(Value::as_str) {
            target.matches_artist(a)
        } else if let Some(list) = item.get("artists").and_then(Value::as_array) {
            list.iter().any(|x| {
                let s = x.as_str().or_else(|| x.get("name").and_then(Value::as_str)).unwrap_or("");
                target.matches_artist(s)
            })
        } else {
            true
        };
        if !artist_ok {
            continue;
        }
        let dur = item
            .get("duration")
            .and_then(Value::as_f64)
            .map(|d| if d > 10_000.0 { d / 1000.0 } else { d })
            .or_else(|| item.get("durationMs").or_else(|| item.get("duration_ms")).and_then(Value::as_f64).map(|ms| ms / 1000.0));
        let gap = match (dur.filter(|&d| d > 0.0), target.duration_secs) {
            (Some(t), Some(o)) => {
                let g = (t - o).abs();
                if g > 5.5 {
                    continue;
                }
                g
            }
            _ => 0.0,
        };
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .or_else(|| item.pointer("/ids/spotify").and_then(Value::as_str))
            .or_else(|| item.pointer("/ids/isrc").and_then(Value::as_str))
            .or_else(|| item.get("spotifyId").and_then(Value::as_str))
            .or_else(|| item.get("isrc").and_then(Value::as_str))
            .map(str::trim)
            .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
        if let Some(id) = id
            && best.as_ref().is_none_or(|(g, _)| gap < *g)
        {
            best = Some((gap, id.to_owned()));
        }
    }
    best.map(|(_, id)| id)
}

fn sec_to_ms(v: Option<&Value>) -> Option<u32> {
    let f = v?.as_f64()?;
    if !f.is_finite() || f < 0.0 {
        return None;
    }
    Some((f * 1000.0).round() as u32)
}

fn parse_spicy_words(arr: &[Value]) -> Vec<LyricWord> {
    let mut words = Vec::with_capacity(arr.len());
    for item in arr {
        let Some(text) = item.get("Text").or_else(|| item.get("text")).and_then(Value::as_str) else { continue };
        let Some(start_ms) = sec_to_ms(item.get("StartTime").or_else(|| item.get("startTime"))) else { continue };
        let end_ms = sec_to_ms(item.get("EndTime").or_else(|| item.get("endTime"))).unwrap_or(start_ms);
        let part = item.get("IsPartOfWord").or_else(|| item.get("isPartOfWord")).and_then(Value::as_bool).unwrap_or(false);
        let clean = text.trim_end_matches('-');
        if clean.is_empty() {
            continue;
        }
        words.push(LyricWord { start_ms, end_ms: end_ms.max(start_ms), text: clean.to_owned(), space_after: !part && !text.ends_with('-') });
    }
    words
}

fn parse_spicy_payload(val: &Value) -> Option<Lyrics> {
    let body = val.get("Body").or_else(|| val.get("body")).unwrap_or(val);
    let kind = body.get("Type").or_else(|| body.get("type")).and_then(Value::as_str).unwrap_or("");
    let content = body.get("Content").or_else(|| body.get("content")).and_then(Value::as_array)?;
    let mut lines: Vec<LyricLine> = Vec::with_capacity(content.len());
    let mut word_synced = false;
    let mut synced = false;

    for item in content {
        let lead = item.get("Lead").or_else(|| item.get("lead"));
        let syl_arr = lead
            .and_then(|l| l.get("Syllables").or_else(|| l.get("syllables")))
            .or_else(|| item.get("Syllables").or_else(|| item.get("syllables")))
            .and_then(Value::as_array);
        if let Some(arr) = syl_arr
            && !arr.is_empty()
        {
            let words = parse_spicy_words(arr);
            if !words.is_empty() {
                let start_ms = lead
                    .and_then(|l| sec_to_ms(l.get("StartTime").or_else(|| l.get("startTime"))))
                    .or_else(|| sec_to_ms(item.get("StartTime").or_else(|| item.get("startTime"))))
                    .or_else(|| words.first().map(|w| w.start_ms));
                let end_ms = lead
                    .and_then(|l| sec_to_ms(l.get("EndTime").or_else(|| l.get("endTime"))))
                    .or_else(|| sec_to_ms(item.get("EndTime").or_else(|| item.get("endTime"))))
                    .or_else(|| words.last().map(|w| w.end_ms));
                let backing = item
                    .get("Background")
                    .or_else(|| item.get("background"))
                    .and_then(Value::as_array)
                    .map(|bg| {
                        bg.iter()
                            .flat_map(|b| b.get("Syllables").or_else(|| b.get("syllables")).and_then(Value::as_array).into_iter().flat_map(|a| parse_spicy_words(a)))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let text = join_words(&words);
                word_synced = true;
                synced = true;
                lines.push(LyricLine { time_ms: start_ms, end_ms, text, words, backing, part: None });
                continue;
            }
        }
        let text = item.get("Text").or_else(|| item.get("text")).and_then(Value::as_str).unwrap_or("").trim();
        let start_ms = sec_to_ms(item.get("StartTime").or_else(|| item.get("startTime")));
        let end_ms = sec_to_ms(item.get("EndTime").or_else(|| item.get("endTime")));
        if start_ms.is_some() {
            synced = true;
        }
        if !text.is_empty() || start_ms.is_some() {
            lines.push(LyricLine { time_ms: start_ms, end_ms, text: text.to_owned(), ..LyricLine::default() });
        }
    }

    if kind.eq_ignore_ascii_case("Static") {
        synced = false;
        word_synced = false;
    }
    if synced {
        lines.sort_by_key(|l| l.time_ms);
    }
    let out = Lyrics { lines, synced, word_synced, instrumental: false, source: LyricsSource::SpicyLyrics, link: None };
    (!out.is_empty()).then_some(out)
}

// ---- Musixmatch richsync parsing --------------------------------------------------------------------

#[derive(Deserialize)]
struct MxmRichLine {
    ts: f64,
    te: f64,
    #[serde(default)]
    l: Vec<MxmRichChar>,
    #[serde(default)]
    x: String,
}

#[derive(Deserialize)]
struct MxmRichChar {
    c: String,
    o: f64,
}

fn parse_musixmatch_richsync(raw: &str) -> Option<Lyrics> {
    let entries: Vec<MxmRichLine> = serde_json::from_str(raw).ok()?;
    let mut lines = Vec::with_capacity(entries.len());
    for entry in entries {
        let start_ms = (entry.ts.max(0.0) * 1000.0).round() as u32;
        let end_ms = (entry.te.max(entry.ts) * 1000.0).round() as u32;
        let mut words: Vec<LyricWord> = Vec::new();
        for (i, ch) in entry.l.iter().enumerate() {
            let trimmed = ch.c.trim();
            if trimmed.is_empty() {
                continue;
            }
            let w_start = ((entry.ts + ch.o).max(0.0) * 1000.0).round() as u32;
            let w_end = entry
                .l
                .get(i + 1)
                .map(|n| ((entry.ts + n.o).max(0.0) * 1000.0).round() as u32)
                .unwrap_or(end_ms)
                .max(w_start);
            let space_after = entry.l.get(i + 1).is_some_and(|n| n.c == " " || n.c.starts_with(' ')) || ch.c.ends_with(' ');
            words.push(LyricWord { start_ms: w_start, end_ms: w_end, text: trimmed.to_owned(), space_after });
        }
        let text = if !entry.x.trim().is_empty() { entry.x.trim().to_owned() } else { join_words(&words) };
        lines.push(LyricLine { time_ms: Some(start_ms), end_ms: Some(end_ms), text, words, backing: Vec::new(), part: None });
    }
    let word_synced = lines.iter().any(|l| !l.words.is_empty());
    lines.sort_by_key(|l| l.time_ms);
    let out = Lyrics { lines, synced: true, word_synced, instrumental: false, source: LyricsSource::Musixmatch, link: None };
    (!out.is_empty()).then_some(out)
}

// ---- Shared track normalization & LRCLIB ------------------------------------------------------------

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
