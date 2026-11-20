//! Discovery: charts, similar tracks and artists, an artist's whole discography.

use serde_json::Value;

use crate::client::{from_value, seg};
use crate::models::{Album, Artist, Playlist, Track, lossy_vec};
use crate::{Client, Result};

/// One row of a chart.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChartEntry {
    pub position: u32,
    /// `"up"`, `"down"`, `"same"` or `"new"`.
    pub progress: String,
    /// How many places it moved since the last chart.
    pub shift: i32,
    pub track: Track,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Chart {
    pub title: String,
    pub description: Option<String>,
    pub entries: Vec<ChartEntry>,
}

impl Client {
    /// The chart. `option` picks a regional one (`"world"`, `"russia"`); empty for the default.
    pub async fn chart_page(&self, option: &str) -> Result<Chart> {
        let path = if option.is_empty() { "/landing3/chart".to_owned() } else { format!("/landing3/chart/{}", seg(option)?) };
        let v = self.get(&path, &[]).await?;
        Ok(parse_chart(v))
    }

    /// Tracks like this one.
    pub async fn similar_tracks(&self, track_id: &str) -> Result<Vec<Track>> {
        let mut v = self.get(&format!("/tracks/{}/similar", seg(track_id)?), &[]).await?;
        Ok(lossy_vec(v.get_mut("similarTracks").map(Value::take).unwrap_or(Value::Null)))
    }

    /// The artist's own albums, newest first, one page (up to `size`).
    pub async fn artist_albums(&self, artist_id: &str, page: u32, size: u32) -> Result<Vec<Album>> {
        let (page, size) = (page.to_string(), size.clamp(1, 100).to_string());
        let mut v = self.get(&format!("/artists/{}/direct-albums", seg(artist_id)?), &[("page", &page), ("page-size", &size), ("sort-by", "year")]).await?;
        Ok(lossy_vec(v.get_mut("albums").map(Value::take).unwrap_or(Value::Null)))
    }

    pub async fn similar_artists(&self, artist_id: &str) -> Result<Vec<Artist>> {
        let mut v = self.get(&format!("/artists/{}/similar", seg(artist_id)?), &[]).await?;
        Ok(lossy_vec(v.get_mut("similarArtists").map(Value::take).unwrap_or(Value::Null)))
    }

    /// Playlists the service suggests to add tracks from, for a playlist the account owns.
    pub async fn playlist_recommendations(&self, uid: i64, kind: i64) -> Result<Vec<Track>> {
        let mut v = self.get(&format!("/users/{uid}/playlists/{kind}/recommendations"), &[]).await?;
        Ok(lossy_vec(v.get_mut("tracks").map(Value::take).unwrap_or(Value::Null)))
    }

    /// Several playlists by `"uid:kind"`.
    pub async fn playlists_by_key(&self, keys: &[String]) -> Result<Vec<Playlist>> {
        let joined = keys.iter().map(|k| seg(k).map(str::to_owned)).collect::<Result<Vec<_>>>()?.join(",");
        let v = self.post_form("/playlists/list", &[("playlistIds", joined.as_str())]).await?;
        Ok(lossy_vec(v))
    }

    /// A track's full record (with its lyrics flags), for when only an id is known.
    pub async fn track(&self, id: &str) -> Result<Track> {
        let v = self.get(&format!("/tracks/{}", seg(id)?), &[]).await?;
        let mut list: Vec<Track> = lossy_vec(v);
        if list.is_empty() { Err(crate::Error::Parse("no such track".into())) } else { Ok(list.swap_remove(0)) }
    }

    /// An album without its tracks (cheaper than [`Client::album`]).
    pub async fn album_brief(&self, id: &str) -> Result<Album> {
        let v = self.get(&format!("/albums/{}", seg(id)?), &[]).await?;
        from_value(v, "album")
    }
}

pub(crate) fn parse_chart(v: Value) -> Chart {
    let title = v.pointer("/chart/title").or_else(|| v.get("title")).and_then(Value::as_str).unwrap_or_default().to_owned();
    let description = v.get("chartDescription").and_then(Value::as_str).map(str::to_owned).filter(|s| !s.is_empty());
    let entries = v
        .pointer("/chart/tracks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let track: Track = serde_json::from_value(item.get("track")?.clone()).ok()?;
            let chart = item.get("chart");
            let num = |k: &str| chart.and_then(|c| c.get(k)).and_then(Value::as_i64).unwrap_or(0);
            let progress = chart.and_then(|c| c.get("progress")).and_then(Value::as_str).unwrap_or("same").to_owned();
            Some(ChartEntry { position: num("position").max(0) as u32, progress, shift: num("shift") as i32, track })
        })
        .enumerate()
        .map(|(i, mut e)| {
            if e.position == 0 {
                e.position = i as u32 + 1;
            }
            e
        })
        .collect();
    Chart { title, description, entries }
}
