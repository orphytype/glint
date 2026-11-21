//! Home shelves and legacy Wave station endpoints.

use reqwest::Method;
use serde::Deserialize;
use serde_json::{Value, json};

use super::parse::{as_id, iso8601, seg, unix_millis};
use super::{Client, WaveEvent};
use crate::models::{Album, Playlist, RotorBatch, Shelf, ShelfItem, ShelfKind, Track};
use crate::{Error, Result};

impl Client {
    /// The home-page shelves. Each shelf is fetched independently; one failing does not hide the
    /// others, and an error is returned only when nothing at all came back.
    pub async fn home(&self) -> Result<Vec<Shelf>> {
        let (personal, editorial, releases, chart) = tokio::join!(self.personal_playlists(), self.editorial_playlists(), self.new_releases(), self.chart());
        let mut shelves = Vec::new();
        let mut first_err: Option<Error> = None;
        let mut add = |kind: ShelfKind, result: Result<Vec<ShelfItem>>| match result {
            Ok(items) if !items.is_empty() => shelves.push(Shelf { kind, items }),
            Ok(_) => {}
            Err(e) => {
                first_err.get_or_insert(e);
            }
        };
        add(ShelfKind::Personal, personal.map(|v| v.into_iter().map(ShelfItem::Playlist).collect()));
        add(ShelfKind::Editorial, editorial.map(|v| v.into_iter().map(ShelfItem::Playlist).collect()));
        add(ShelfKind::NewReleases, releases.map(|v| v.into_iter().map(ShelfItem::Album).collect()));
        add(ShelfKind::Chart, chart.map(|v| v.into_iter().map(ShelfItem::Track).collect()));
        match first_err {
            Some(e) if shelves.is_empty() => Err(e),
            _ => Ok(shelves),
        }
    }

    async fn personal_playlists(&self) -> Result<Vec<Playlist>> {
        match self.get("/feed", &[]).await {
            Ok(v) => {
                let found = generated_playlists(&v);
                if !found.is_empty() {
                    return Ok(found);
                }
            }
            Err(Error::Unauthorized) => return Err(Error::Unauthorized),
            Err(_) => {}
        }
        let v = self.get("/landing3", &[("blocks", "personal-playlists")]).await?;
        Ok(landing_playlists(&v))
    }

    async fn editorial_playlists(&self) -> Result<Vec<Playlist>> {
        let v = self.get("/landing3", &[("blocks", "new-playlists")]).await?;
        Ok(landing_playlists(&v))
    }

    async fn new_releases(&self) -> Result<Vec<Album>> {
        if let Ok(v) = self.get("/landing3/new-releases", &[]).await {
            let ids: Vec<String> = v.get("newReleases").and_then(Value::as_array).into_iter().flatten().take(40).map(as_id).filter(|s| !s.is_empty()).collect();
            if !ids.is_empty()
                && let Ok(albums) = self.albums(&ids).await
                && !albums.is_empty()
            {
                return Ok(albums);
            }
        }
        let v = self.get("/landing3", &[("blocks", "new-releases")]).await?;
        Ok(landing_albums(&v))
    }

    async fn chart(&self) -> Result<Vec<Track>> {
        let v = self.get("/landing3", &[("blocks", "chart")]).await?;
        Ok(chart_tracks(&v))
    }

    /// The next batch of a Wave station (`"user:onyourwave"`). Pass the id of the last track you
    /// were given as `queue` to continue the stream.
    pub async fn wave_tracks(&self, station: &str, seeds: &[&str], queue: Option<&str>) -> Result<RotorBatch> {
        let mut query: Vec<(&str, &str)> = seeds.iter().map(|s| ("seeds", *s)).collect();
        if let Some(q) = queue.filter(|q| !q.is_empty()) {
            query.push(("queue", q));
        }
        let v = self.get(&format!("/rotor/station/{}/tracks", seg(station)?), &query).await?;
        Ok(parse_rotor(v))
    }

    /// Tells the Wave what you did, which is how it learns your taste. Best-effort.
    pub async fn wave_feedback(&self, station: &str, event: WaveEvent, track_id: Option<&str>, batch_id: Option<&str>, played_secs: Option<f64>) -> Result<()> {
        let mut body = json!({ "type": event.as_str(), "timestamp": iso8601(unix_millis()) });
        let map = body.as_object_mut().expect("json! object");
        if let Some(t) = track_id {
            map.insert("trackId".into(), json!(t));
        }
        if let Some(b) = batch_id {
            map.insert("batchId".into(), json!(b));
        }
        if let Some(s) = played_secs {
            map.insert("totalPlayedSeconds".into(), json!(s));
        }
        self.call(self.api(Method::POST, &format!("/rotor/station/{}/feedback", seg(station)?)).json(&body)).await.map(drop)
    }
}

/// `result.blocks[].entities[].data`, with the extra `data` wrapper some blocks add peeled off.
fn landing_entities(result: &Value) -> impl Iterator<Item = &Value> {
    result
        .get("blocks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|block| block.get("entities").and_then(Value::as_array))
        .flatten()
        .filter_map(|entity| entity.get("data"))
        .map(|data| data.get("data").filter(|inner| inner.is_object()).unwrap_or(data))
}

fn landing_playlists(result: &Value) -> Vec<Playlist> {
    landing_entities(result).filter_map(|d| Playlist::deserialize(d).ok()).filter(|p| !p.title.is_empty()).collect()
}

fn landing_albums(result: &Value) -> Vec<Album> {
    landing_entities(result).filter_map(|d| Album::deserialize(d).ok()).filter(|a| !a.id.is_empty()).collect()
}

fn chart_tracks(result: &Value) -> Vec<Track> {
    // Chart entities carry `{position, track}`; the unwrapping above must not descend into it.
    result
        .get("blocks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|block| block.get("entities").and_then(Value::as_array))
        .flatten()
        .filter_map(|entity| entity.pointer("/data/track"))
        .filter_map(|t| Track::deserialize(t).ok())
        .filter(|t| !t.id.is_empty())
        .collect()
}

/// `/feed`'s `generatedPlaylists[].data`.
fn generated_playlists(result: &Value) -> Vec<Playlist> {
    result
        .get("generatedPlaylists")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|g| g.get("data"))
        .filter_map(|d| Playlist::deserialize(d).ok())
        .filter(|p| !p.title.is_empty())
        .collect()
}

fn parse_rotor(mut result: Value) -> RotorBatch {
    let batch_id = result.get("batchId").and_then(Value::as_str).map(str::to_owned);
    let sequence = result.get_mut("sequence").map(Value::take).unwrap_or(Value::Null);
    let tracks = match sequence {
        Value::Array(items) => items
            .into_iter()
            .filter_map(|mut item| item.get_mut("track").map(Value::take))
            .filter_map(|t| serde_json::from_value::<Track>(t).ok())
            .filter(|t| !t.id.is_empty())
            .collect(),
        _ => Vec::new(),
    };
    RotorBatch { batch_id, tracks }
}
