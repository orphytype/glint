//! Account, library, catalog entities, likes and search endpoints.

use std::collections::HashMap;
use std::time::Duration;

use reqwest::header::{AUTHORIZATION, USER_AGENT};
use serde::Deserialize;
use serde_json::Value;

use super::parse::{as_i64, from_value, id_pairs, seg, unwrap_items_newest_first};
use super::{APP_UA, BATCH, Client, Likeable, MAX_TEXT_BYTES, SearchKind, read_limited};
use crate::models::{Account, Album, Artist, ArtistPage, Credit, Playlist, SearchResult, Track, TrackRef, donation_url, lossy_vec};
use crate::{Error, Result};

/// Yandex ID's "who am I" endpoint: the account's picture (the music API does not give it).
const PASSPORT_INFO: &str = "https://login.yandex.ru/info?format=json";

impl Client {
    /// Who the token belongs to. Doubles as the "is this token any good?" check.
    pub async fn account(&self) -> Result<Account> {
        let v = self.get("/account/status", &[]).await?;
        let acct = v.get("account").ok_or_else(|| Error::Parse("account/status has no account".into()))?;
        let uid = as_i64(acct.get("uid"));
        if uid == 0 {
            return Err(Error::Unauthorized);
        }
        let text = |k: &str| acct.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
        let login = text("login").unwrap_or_default();
        let display_name = text("displayName").or_else(|| text("fullName")).unwrap_or_else(|| login.clone());
        let has_plus = v.pointer("/plus/hasPlus").and_then(Value::as_bool).unwrap_or(false);
        Ok(Account { uid, login, display_name, has_plus })
    }

    /// The liked tracks as bare references, newest first. Hydrate pages of them with
    /// [`Client::tracks`].
    pub async fn liked_track_refs(&self, uid: i64) -> Result<Vec<TrackRef>> {
        let mut v = self.get(&format!("/users/{uid}/likes/tracks"), &[]).await?;
        let raw = v.pointer_mut("/library/tracks").map(Value::take).unwrap_or(Value::Null);
        let mut refs: Vec<TrackRef> = lossy_vec(raw);
        // Timestamps are ISO-8601 strings, so they sort as text; an unstamped list keeps its order.
        refs.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        Ok(refs)
    }

    pub async fn liked_albums(&self, uid: i64) -> Result<Vec<Album>> {
        let v = self.get(&format!("/users/{uid}/likes/albums"), &[("rich", "true")]).await?;
        Ok(unwrap_items_newest_first(v, "album"))
    }

    pub async fn liked_artists(&self, uid: i64) -> Result<Vec<Artist>> {
        let v = self.get(&format!("/users/{uid}/likes/artists"), &[("with-timestamps", "true")]).await?;
        Ok(unwrap_items_newest_first(v, "artist"))
    }

    pub async fn playlists(&self, uid: i64) -> Result<Vec<Playlist>> {
        let v = self.get(&format!("/users/{uid}/playlists/list"), &[]).await?;
        let mut list: Vec<Playlist> = lossy_vec(v);
        if list.iter().any(|p| p.modified.is_some() || p.created.is_some()) {
            list.sort_by(|a, b| b.modified.as_deref().or(b.created.as_deref()).cmp(&a.modified.as_deref().or(a.created.as_deref())));
        }
        Ok(list)
    }

    /// A playlist with all its tracks resolved.
    pub async fn playlist(&self, uid: i64, kind: i64) -> Result<Playlist> {
        let v = self.get(&format!("/users/{uid}/playlists/{kind}"), &[("rich-tracks", "true")]).await?;
        let mut playlist: Playlist = from_value(v, "playlist")?;
        if playlist.uid == 0 {
            playlist.uid = uid;
        }
        let missing: Vec<String> = playlist.tracks.iter().filter(|t| t.track.is_none() && !t.id.is_empty()).map(|t| t.id.clone()).collect();
        if !missing.is_empty() {
            let resolved: HashMap<String, Track> = self.tracks(&missing).await?.into_iter().map(|t| (t.id.clone(), t)).collect();
            for entry in &mut playlist.tracks {
                if entry.track.is_none() {
                    entry.track = resolved.get(entry.id.split(':').next().unwrap_or("")).cloned();
                }
            }
        }
        Ok(playlist)
    }

    pub async fn album(&self, id: &str) -> Result<Album> {
        let v = self.get(&format!("/albums/{}/with-tracks", seg(id)?), &[]).await?;
        from_value(v, "album")
    }

    pub async fn albums(&self, ids: &[String]) -> Result<Vec<Album>> {
        let mut out = Vec::new();
        for chunk in ids.chunks(BATCH) {
            let form = id_pairs("album-ids", chunk)?;
            out.extend(lossy_vec::<Album>(self.post_form("/albums", &form).await?));
        }
        Ok(out)
    }

    /// Every track of an artist (the first `limit`), most popular first.
    pub async fn artist_tracks(&self, id: &str, limit: u32) -> Result<Vec<Track>> {
        let size = limit.clamp(1, 500).to_string();
        let mut v = self.get(&format!("/artists/{}/tracks", seg(id)?), &[("page", "0"), ("page-size", size.as_str())]).await?;
        let raw = v.get_mut("tracks").map(Value::take).unwrap_or(Value::Null);
        Ok(lossy_vec(raw))
    }

    /// The picture of the signed-in account, as a URL on Yandex's avatar CDN; `None` when the
    /// account has none (or the service will not say). The token goes to Yandex ID here, the
    /// service that issued it, and to nothing else.
    pub async fn avatar(&self) -> Result<Option<String>> {
        let Some(value) = self.auth_header() else { return Ok(None) };
        let resp = self.inner.http.get(PASSPORT_INFO).header(USER_AGENT, APP_UA).header(AUTHORIZATION, value).timeout(Duration::from_secs(15)).send().await?;
        if !resp.status().is_success() {
            return Ok(None);
        }
        let body = read_limited(resp, MAX_TEXT_BYTES, |_, _| {}).await?;
        let v: Value = serde_json::from_slice(&body).map_err(|e| Error::Parse(e.to_string()))?;
        Ok(avatar_url(&v))
    }

    pub async fn artist(&self, id: &str) -> Result<ArtistPage> {
        let v = self.get(&format!("/artists/{}/brief-info", seg(id)?), &[]).await?;
        let mut page = ArtistPage::deserialize(&v).map_err(|e| Error::Parse(format!("artist page: {e}")))?;
        page.donation = donation_url(&v, &page.artist);
        Ok(page)
    }

    /// Who made a track, as the service credits them: performers, the authors of the music and
    /// the words, the label, where the recording comes from.
    pub async fn track_credits(&self, id: &str) -> Result<Vec<Credit>> {
        let base = id.split(':').next().unwrap_or(id);
        let mut v = self.get(&format!("/tracks/{}/credits", seg(base)?), &[]).await?;
        let list = v.get_mut("credits").map(Value::take).unwrap_or_else(|| Value::Array(Vec::new()));
        let mut credits: Vec<Credit> = from_value(list, "credits")?;
        credits.retain(|c| !c.title.trim().is_empty() && !c.value.trim().is_empty());
        Ok(credits)
    }

    /// Full track objects for ids (`"123"` or `"123:456"`), in the order asked for.
    pub async fn tracks(&self, ids: &[String]) -> Result<Vec<Track>> {
        let mut out: Vec<Track> = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(BATCH) {
            let form = id_pairs("track-ids", chunk)?;
            out.extend(lossy_vec::<Track>(self.post_form("/tracks", &form).await?));
        }
        let order: HashMap<&str, usize> = ids.iter().enumerate().rev().map(|(i, id)| (id.split(':').next().unwrap_or(id), i)).collect();
        out.sort_by_key(|t| order.get(t.id.as_str()).copied().unwrap_or(usize::MAX));
        Ok(out)
    }

    pub async fn like_tracks(&self, uid: i64, ids: &[String]) -> Result<()> {
        self.track_list_action(&format!("/users/{uid}/likes/tracks/add-multiple"), ids).await
    }

    pub async fn unlike_tracks(&self, uid: i64, ids: &[String]) -> Result<()> {
        self.track_list_action(&format!("/users/{uid}/likes/tracks/remove"), ids).await
    }

    pub async fn dislike_tracks(&self, uid: i64, ids: &[String]) -> Result<()> {
        self.track_list_action(&format!("/users/{uid}/dislikes/tracks/add-multiple"), ids).await
    }

    /// Likes (or, with `like` false, unlikes) an album, an artist or a playlist (`"uid:kind"`).
    pub async fn set_item_like(&self, uid: i64, what: Likeable, id: &str, like: bool) -> Result<()> {
        let (plural, key) = what.names();
        let path = format!("/users/{uid}/likes/{plural}/{}", if like { "add-multiple" } else { "remove" });
        let id = if what == Likeable::Playlist { id } else { seg(id)? };
        self.post_form(&path, &[(key, id)]).await.map(drop)
    }

    /// The playlists the account liked, each as `"uid:kind"`.
    pub async fn liked_playlist_ids(&self, uid: i64) -> Result<Vec<String>> {
        let v = self.get(&format!("/users/{uid}/likes/playlists"), &[]).await?;
        let lists: Vec<Playlist> = unwrap_items_newest_first(v, "playlist");
        Ok(lists.into_iter().map(|p| format!("{}:{}", p.uid, p.kind)).collect())
    }

    async fn track_list_action(&self, path: &str, ids: &[String]) -> Result<()> {
        let form = id_pairs("track-ids", ids)?;
        self.post_form(path, &form).await.map(drop)
    }

    pub async fn search(&self, text: &str, kind: SearchKind, page: u32) -> Result<SearchResult> {
        let page = page.to_string();
        let v = self.get("/search", &[("text", text), ("type", kind.as_str()), ("page", &page), ("nocorrect", "false"), ("playlist-in-best", "true")]).await?;
        from_value(v, "search results")
    }

    pub async fn suggest(&self, part: &str) -> Result<Vec<String>> {
        let v = self.get("/search/suggest", &[("part", part)]).await?;
        Ok(v.get("suggestions").and_then(Value::as_array).into_iter().flatten().filter_map(|s| s.as_str().map(str::to_owned)).collect())
    }
}

/// The picture URL in a Yandex ID `info` answer, if the account has set one.
fn avatar_url(v: &Value) -> Option<String> {
    if v.get("is_avatar_empty").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    let id = v.get("default_avatar_id").and_then(Value::as_str)?.trim();
    // An id is a couple of path segments of letters, digits, '-' and '_' ("1234/abc-5").
    if id.is_empty() || id.len() > 200 || !id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '/')) || id.contains("//") {
        return None;
    }
    Some(format!("https://avatars.yandex.net/get-yapic/{id}/islands-200"))
}
