//! FkCensorData catalog: clean-track audio replacements, deleted album tracklists, artist
//! insertions, metadata overrides, and synced LRC lyrics.

use std::collections::HashMap;

use serde_json::Value;

const LIST_URL: &str = "https://raw.githubusercontent.com/fcensor-data/FkCensorData/refs/heads/main/list_v2.json";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Replacement {
    pub url: String,
    pub duration_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArtistRef {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrackMeta {
    pub title: Option<String>,
    pub artists: Vec<ArtistRef>,
    pub cover_uri: Option<String>,
    pub duration_ms: Option<u64>,
    pub lyrics_url: Option<String>,
    pub insertion_index: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AlbumMeta {
    pub id: String,
    pub title: Option<String>,
    pub artists: Vec<ArtistRef>,
    pub cover_uri: Option<String>,
    pub release_date: Option<String>,
    pub volumes: Vec<Vec<String>>,
    pub insertion_index: Option<i64>,
    pub replace_artists_in_volumes: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArtistTrackInsert {
    pub index: i64,
    pub track_id: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    pub replacements: HashMap<String, Replacement>,
    pub tracks: HashMap<String, TrackMeta>,
    pub albums: HashMap<String, AlbumMeta>,
    pub artist_popular: HashMap<String, Vec<String>>,
    pub artist_insertions: HashMap<String, Vec<ArtistTrackInsert>>,
}

/// Parses the public FkCensorData `list_v2.json` into audio replacements, track/album metadata,
/// deleted album tracklists, and artist insertions.
pub fn parse_catalog(json: &str) -> Option<Catalog> {
    let root: Value = serde_json::from_str(json).ok()?;
    let sources = root.get("sources")?.as_array()?;
    let mut catalog = Catalog::default();

    for source in sources {
        if let Some(list) = source.get("tracksStorages").and_then(Value::as_array) {
            for storage in list.iter().rev() {
                parse_storage(storage, &mut catalog.replacements);
            }
        }
        if let Some(tracks) = source.get("tracks").and_then(Value::as_object) {
            for (id, value) in tracks {
                if let Some(meta) = parse_track_meta(value) {
                    catalog.tracks.entry(id.clone()).or_insert(meta);
                }
            }
        }
        if let Some(albums) = source.get("albums").and_then(Value::as_object) {
            for (id, value) in albums {
                if let Some(meta) = parse_album_meta(id, value) {
                    catalog.albums.entry(id.clone()).or_insert(meta);
                }
            }
        }
        if let Some(artists) = source.get("artists").and_then(Value::as_object) {
            for (id, value) in artists {
                if let Some(popular) = value.pointer("/__fckCensor/popular").and_then(Value::as_array) {
                    let ids: Vec<String> = popular.iter().filter_map(value_id).collect();
                    if !ids.is_empty() {
                        catalog.artist_popular.entry(id.clone()).or_insert(ids);
                    }
                }
            }
        }
        if let Some(insertions) = source.get("artistsInsertions").and_then(Value::as_object) {
            for (id, value) in insertions {
                if let Some(list) = value.get("tracks").and_then(Value::as_array) {
                    let items: Vec<ArtistTrackInsert> = list
                        .iter()
                        .filter_map(|item| {
                            let track_id = item.get("releaseId").and_then(value_id)?;
                            let index = item.get("index").and_then(Value::as_i64).unwrap_or(-1);
                            Some(ArtistTrackInsert { index, track_id })
                        })
                        .collect();
                    if !items.is_empty() {
                        catalog.artist_insertions.entry(id.clone()).or_insert(items);
                    }
                }
            }
        }
    }

    (!catalog.replacements.is_empty()).then_some(catalog)
}

fn parse_storage(storage: &Value, out: &mut HashMap<String, Replacement>) {
    let template = storage.get("urlTemplate").and_then(Value::as_str);
    if let Some(tracks) = storage.get("tracks").and_then(Value::as_object) {
        for (id, value) in tracks {
            if out.contains_key(id) {
                continue;
            }
            let (url, duration_ms) = storage_url(template, value);
            if let Some(url) = url {
                out.insert(id.clone(), Replacement { url, duration_ms });
            }
        }
    }
    if let Some(ids) = storage.get("trackIds").and_then(Value::as_array) {
        let Some(template) = template else { return };
        for item in ids {
            let Some(id) = item.get("id").and_then(value_id).or_else(|| value_id(item)) else { continue };
            if out.contains_key(&id) {
                continue;
            }
            let mut raw = template.replace("%%", &id);
            if let Some(v) = item.get("v").and_then(value_id) {
                let sep = if raw.contains('?') { '&' } else { '?' };
                raw.push(sep);
                raw.push_str("v=");
                raw.push_str(&v);
            }
            let Some(url) = normalize_audio_url(&raw) else { continue };
            let duration_ms = item.get("durationMs").and_then(Value::as_u64);
            out.insert(id, Replacement { url, duration_ms });
        }
    }
}

fn parse_artists(value: Option<&Value>) -> Vec<ArtistRef> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let id = a.get("id").and_then(value_id).unwrap_or_default();
            let name = a.get("name").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())?.to_owned();
            Some(ArtistRef { id, name })
        })
        .collect()
}

fn parse_track_meta(value: &Value) -> Option<TrackMeta> {
    let obj = value.as_object()?;
    let title = obj.get("title").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
    let artists = parse_artists(obj.get("artists"));
    let cover_uri = obj.get("coverUri").and_then(Value::as_str).and_then(normalize_cover_uri);
    let duration_ms = obj.get("durationMs").and_then(Value::as_u64);
    let fc = obj.get("__fckCensor");
    let lyrics_url = fc.and_then(|v| v.get("lyrics")).and_then(Value::as_str).and_then(normalize_data_url);
    let insertion_index = fc.and_then(|v| v.get("insertionIndex")).and_then(Value::as_i64);
    Some(TrackMeta { title, artists, cover_uri, duration_ms, lyrics_url, insertion_index })
}

fn parse_album_meta(id: &str, value: &Value) -> Option<AlbumMeta> {
    let obj = value.as_object()?;
    let title = obj.get("title").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
    let artists = parse_artists(obj.get("artists"));
    let cover_uri = obj.get("coverUri").and_then(Value::as_str).and_then(normalize_cover_uri);
    let release_date = obj.get("releaseDate").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
    let volumes = obj
        .get("volumes")
        .and_then(Value::as_array)
        .map(|vols| {
            vols.iter()
                .filter_map(Value::as_array)
                .map(|vol| vol.iter().filter_map(|item| item.get("id").and_then(value_id).or_else(|| value_id(item))).collect::<Vec<String>>())
                .filter(|v| !v.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let fc = obj.get("__fckCensor");
    let insertion_index = fc.and_then(|v| v.get("insertionIndex")).and_then(Value::as_i64);
    let replace_artists_in_volumes = fc.and_then(|v| v.get("replaceArtistsInAlbumVolumes")).and_then(Value::as_bool).unwrap_or(false);
    Some(AlbumMeta { id: id.to_owned(), title, artists, cover_uri, release_date, volumes, insertion_index, replace_artists_in_volumes })
}

fn storage_url(template: Option<&str>, value: &Value) -> (Option<String>, Option<u64>) {
    let (raw, duration_ms) =
        if let Some(object) = value.as_object() { (object.get("url").and_then(Value::as_str), object.get("durationMs").and_then(Value::as_u64)) } else { (value.as_str(), None) };
    let raw = match raw {
        Some(raw) => raw,
        None if value.as_u64().is_some() => "",
        None => return (None, None),
    };
    let url = if raw.is_empty() {
        let Some(id) = value.as_u64() else { return (None, None) };
        let Some(template) = template else { return (None, None) };
        template.replace("%%", &id.to_string())
    } else if raw.contains("://") {
        raw.to_owned()
    } else {
        let Some(template) = template else { return (None, None) };
        template.replace("%%", raw)
    };
    (normalize_audio_url(&url), duration_ms)
}

fn value_id(value: &Value) -> Option<String> {
    value.as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).or_else(|| value.as_u64().map(|id| id.to_string()))
}

fn normalize_audio_url(url: &str) -> Option<String> {
    let url = if url.starts_with("https://") { url.to_owned() } else { format!("https://{url}") };
    (url.strip_prefix("https://")?.split('/').next()? == "raw.githubusercontent.com").then_some(url)
}

fn normalize_data_url(url: &str) -> Option<String> {
    let url = if url.starts_with("https://") { url.to_owned() } else { format!("https://{url}") };
    url.starts_with("https://raw.githubusercontent.com/fcensor-data/").then_some(url)
}

fn normalize_cover_uri(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let test = crate::cover::url(trimmed, 300);
    crate::cover::is_trusted_image_host(&test).then(|| trimmed.to_owned())
}

pub async fn fetch_catalog(http: &reqwest::Client) -> crate::Result<Catalog> {
    let response = http.get(LIST_URL).send().await?.error_for_status()?;
    let body = response.text().await?;
    parse_catalog(&body).ok_or_else(|| crate::Error::Parse("invalid FkCensorData list".into()))
}
