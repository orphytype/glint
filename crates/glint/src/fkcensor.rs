//! Runtime catalog of FkCensorData clean-track audio replacements, deleted album tracklists,
//! artist insertions, metadata overrides, and synced LRC lyrics.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use ym_api::fkcensor::{AlbumMeta, ArtistRef, Catalog, Replacement};
use ym_api::{Album, Artist, ArtistPage, SearchResult, Track};

/// Synthetic FkCensor track IDs start at 600_000_000_000_000; lower numeric IDs exist on Yandex.
const SYNTHETIC_TRACK_ID_FLOOR: u64 = 600_000_000_000_000;

#[derive(Clone, Default)]
pub struct FkCensor {
    catalog: Arc<RwLock<Catalog>>,
    loaded: Arc<AtomicBool>,
    load_lock: Arc<tokio::sync::Mutex<()>>,
}

impl FkCensor {
    /// Ensures the remote catalog has been fetched at least once before serving a request.
    pub async fn ensure_loaded(&self, client: &ym_api::Client) {
        if self.loaded.load(Ordering::Acquire) {
            return;
        }
        let _guard = self.load_lock.lock().await;
        if self.loaded.load(Ordering::Acquire) {
            return;
        }
        let _ = self.refresh(client).await;
    }

    pub async fn refresh(&self, client: &ym_api::Client) -> bool {
        let Ok(next) = client.fkcensor_catalog().await else { return false };
        let Ok(mut current) = self.catalog.write() else { return false };
        self.loaded.store(true, Ordering::Release);
        if *current == next {
            return false;
        }
        *current = next;
        true
    }

    pub fn replacement(&self, track_id: &str) -> Option<Replacement> {
        self.catalog.read().ok()?.replacements.get(track_id).cloned()
    }

    pub fn has_track(&self, track_id: &str) -> bool {
        self.catalog.read().ok().is_some_and(|c| c.replacements.contains_key(track_id))
    }

    pub fn lyrics_url(&self, track_id: &str) -> Option<String> {
        self.catalog.read().ok()?.tracks.get(track_id)?.lyrics_url.clone()
    }

    /// Patches metadata and availability on a slice of tracks (Liked, playlists, search, waves).
    pub fn patch_tracks(&self, tracks: &mut [Track]) {
        let Ok(cat) = self.catalog.read() else { return };
        for track in tracks {
            apply_track(&cat, track, None, false);
        }
    }

    /// Merges FkCensorData metadata and missing/deleted tracks into an album fetched from Yandex
    /// (or synthesizes the album completely when Yandex returns 404/error for a deleted album).
    pub async fn merge_album(&self, client: &ym_api::Client, id: &str, ym_album: Option<Album>) -> Option<Album> {
        self.ensure_loaded(client).await;
        let fc_album = self.catalog.read().ok().and_then(|c| c.albums.get(id).cloned());
        let Some(fc_album) = fc_album else {
            let mut album = ym_album?;
            let stub = album_stub(&album);
            if let Ok(cat) = self.catalog.read() {
                for vol in &mut album.volumes {
                    for track in vol {
                        apply_track(&cat, track, Some(&stub), false);
                    }
                }
            }
            return Some(album);
        };

        let mut album = ym_album.unwrap_or_else(|| Album { id: id.to_owned(), ..Default::default() });
        if album.id.is_empty() {
            album.id = id.to_owned();
        }
        apply_album_meta(&mut album, &fc_album);

        let mut by_id: HashMap<String, Track> = HashMap::new();
        let mut ym_order: Vec<String> = Vec::new();
        for vol in std::mem::take(&mut album.volumes) {
            for t in vol {
                ym_order.push(t.id.clone());
                by_id.insert(t.id.clone(), t);
            }
        }

        // Hydrate deleted real Yandex track IDs that are listed in FkCensor's volume list but
        // stripped from Yandex's `/albums/{id}/with-tracks` response.
        let missing_real: Vec<String> = fc_album.volumes.iter().flatten().filter(|tid| !by_id.contains_key(tid.as_str()) && is_real_ym_track_id(tid)).cloned().collect();
        if !missing_real.is_empty()
            && let Ok(fetched) = client.tracks(&missing_real).await
        {
            for t in fetched {
                by_id.entry(t.id.clone()).or_insert(t);
            }
        }

        let stub = album_stub(&album);
        let Ok(cat) = self.catalog.read() else { return Some(album) };
        if fc_album.volumes.is_empty() {
            let mut vol = Vec::with_capacity(ym_order.len());
            for tid in ym_order {
                if let Some(mut t) = by_id.remove(&tid) {
                    apply_track(&cat, &mut t, Some(&stub), fc_album.replace_artists_in_volumes);
                    vol.push(t);
                }
            }
            if !vol.is_empty() {
                album.volumes.push(vol);
            }
        } else {
            let mut seen = HashSet::new();
            for vol_ids in &fc_album.volumes {
                let mut vol = Vec::with_capacity(vol_ids.len());
                for tid in vol_ids {
                    if !seen.insert(tid.clone()) {
                        continue;
                    }
                    let base = by_id.remove(tid);
                    vol.push(build_track(&cat, tid, base, Some(&stub), fc_album.replace_artists_in_volumes));
                }
                if !vol.is_empty() {
                    album.volumes.push(vol);
                }
            }
            // Keep any extra tracks Yandex still returned that were not listed in FkCensor.
            let mut extras = Vec::new();
            for tid in ym_order {
                if let Some(mut t) = by_id.remove(&tid) {
                    apply_track(&cat, &mut t, Some(&stub), fc_album.replace_artists_in_volumes);
                    extras.push(t);
                }
            }
            if !extras.is_empty() {
                if let Some(last) = album.volumes.last_mut() {
                    last.extend(extras);
                } else {
                    album.volumes.push(extras);
                }
            }
        }
        let count: usize = album.volumes.iter().map(Vec::len).sum();
        if count > 0 {
            album.track_count = Some(count as u32);
        }
        Some(album)
    }

    /// Injects missing FkCensor albums and tracks into an artist's page and patches existing items.
    pub async fn merge_artist(&self, client: &ym_api::Client, artist_id: &str, page: &mut ArtistPage) {
        self.ensure_loaded(client).await;
        let (matching_albums, popular_ids, insertions) = {
            let Ok(cat) = self.catalog.read() else { return };
            let albums: Vec<AlbumMeta> = cat.albums.values().filter(|a| a.artists.iter().any(|ar| ar.id == artist_id)).cloned().collect();
            let pop = cat.artist_popular.get(artist_id).cloned().unwrap_or_default();
            let ins = cat.artist_insertions.get(artist_id).cloned().unwrap_or_default();
            (albums, pop, ins)
        };

        let existing_album_ids: HashSet<String> = page.albums.iter().map(|a| a.id.clone()).collect();
        if let Ok(cat) = self.catalog.read() {
            for a in &mut page.albums {
                if let Some(meta) = cat.albums.get(&a.id) {
                    apply_album_meta(a, meta);
                }
            }
        }
        let mut new_albums: Vec<Album> = matching_albums.iter().filter(|a| !existing_album_ids.contains(&a.id)).map(album_card_from_meta).collect();
        new_albums.sort_by(|a, b| b.release_date.cmp(&a.release_date).then_with(|| b.year.cmp(&a.year)));
        for a in new_albums {
            page.albums.push(a);
        }

        let existing_track_ids: HashSet<String> = page.popular_tracks.iter().map(|t| t.id.clone()).collect();
        let mut needed: Vec<String> = Vec::new();
        for tid in popular_ids.iter().chain(insertions.iter().map(|i| &i.track_id)) {
            if !existing_track_ids.contains(tid) && !needed.contains(tid) {
                needed.push(tid.clone());
            }
        }
        let missing_real: Vec<String> = needed.iter().filter(|tid| is_real_ym_track_id(tid)).cloned().collect();
        let mut hydrated: HashMap<String, Track> = HashMap::new();
        if !missing_real.is_empty()
            && let Ok(fetched) = client.tracks(&missing_real).await
        {
            for t in fetched {
                hydrated.insert(t.id.clone(), t);
            }
        }

        let Ok(cat) = self.catalog.read() else { return };
        for t in &mut page.popular_tracks {
            let stub = album_for_track(&cat, &t.id);
            apply_track(&cat, t, stub.as_ref(), false);
        }
        let mut seen: HashSet<String> = page.popular_tracks.iter().map(|t| t.id.clone()).collect();
        for tid in popular_ids {
            if seen.insert(tid.clone()) {
                let stub = album_for_track(&cat, &tid);
                page.popular_tracks.push(build_track(&cat, &tid, hydrated.remove(&tid), stub.as_ref(), false));
            }
        }
        for ins in insertions {
            if seen.insert(ins.track_id.clone()) {
                let stub = album_for_track(&cat, &ins.track_id);
                let track = build_track(&cat, &ins.track_id, hydrated.remove(&ins.track_id), stub.as_ref(), false);
                if ins.index >= 0 && (ins.index as usize) <= page.popular_tracks.len() {
                    page.popular_tracks.insert(ins.index as usize, track);
                } else {
                    page.popular_tracks.push(track);
                }
            }
        }
    }

    /// Patches an artist's full track list and appends any deleted/FkCensor-only tracks for that artist.
    pub async fn merge_artist_tracks(&self, client: &ym_api::Client, artist_id: &str, tracks: &mut Vec<Track>) {
        self.ensure_loaded(client).await;
        let extra_ids: Vec<String> = {
            let Ok(cat) = self.catalog.read() else { return };
            let mut ids = Vec::new();
            let mut seen: HashSet<String> = tracks.iter().map(|t| t.id.clone()).collect();
            if let Some(pop) = cat.artist_popular.get(artist_id) {
                for tid in pop {
                    if seen.insert(tid.clone()) {
                        ids.push(tid.clone());
                    }
                }
            }
            if let Some(ins) = cat.artist_insertions.get(artist_id) {
                for item in ins {
                    if seen.insert(item.track_id.clone()) {
                        ids.push(item.track_id.clone());
                    }
                }
            }
            for album in cat.albums.values().filter(|a| a.artists.iter().any(|ar| ar.id == artist_id)) {
                for tid in album.volumes.iter().flatten() {
                    if seen.insert(tid.clone()) {
                        ids.push(tid.clone());
                    }
                }
            }
            ids
        };
        let missing_real: Vec<String> = extra_ids.iter().filter(|tid| is_real_ym_track_id(tid)).cloned().collect();
        let mut hydrated: HashMap<String, Track> = HashMap::new();
        if !missing_real.is_empty()
            && let Ok(fetched) = client.tracks(&missing_real).await
        {
            for t in fetched {
                hydrated.insert(t.id.clone(), t);
            }
        }
        let Ok(cat) = self.catalog.read() else { return };
        for t in tracks.iter_mut() {
            let stub = album_for_track(&cat, &t.id);
            apply_track(&cat, t, stub.as_ref(), false);
        }
        for tid in extra_ids {
            let stub = album_for_track(&cat, &tid);
            tracks.push(build_track(&cat, &tid, hydrated.remove(&tid), stub.as_ref(), false));
        }
    }

    /// Patches search results and injects matching deleted FkCensor albums/tracks hidden by Yandex.
    pub fn merge_search(&self, query: &str, result: &mut SearchResult) {
        let Ok(cat) = self.catalog.read() else { return };
        for t in &mut result.tracks.results {
            let stub = album_for_track(&cat, &t.id);
            apply_track(&cat, t, stub.as_ref(), false);
        }
        for a in &mut result.albums.results {
            if let Some(meta) = cat.albums.get(&a.id) {
                apply_album_meta(a, meta);
            }
        }
        let q = query.trim().to_lowercase();
        if q.chars().count() < 2 {
            return;
        }
        let existing_albums: HashSet<String> = result.albums.results.iter().map(|a| a.id.clone()).collect();
        let mut added_albums = Vec::new();
        for meta in cat.albums.values() {
            if existing_albums.contains(&meta.id) {
                continue;
            }
            let title_hit = meta.title.as_deref().is_some_and(|t| t.to_lowercase().contains(&q));
            let artist_hit = meta.artists.iter().any(|a| a.name.to_lowercase().contains(&q));
            let combined_hit = meta
                .title
                .as_deref()
                .is_some_and(|t| meta.artists.iter().any(|a| format!("{} {t}", a.name).to_lowercase().contains(&q) || format!("{t} {}", a.name).to_lowercase().contains(&q)));
            if title_hit || artist_hit || combined_hit {
                added_albums.push((title_hit || combined_hit, album_card_from_meta(meta)));
            }
        }
        added_albums.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.release_date.cmp(&a.1.release_date)));
        for (strong, album) in added_albums.into_iter().take(10) {
            if strong {
                result.albums.results.insert(0, album);
            } else {
                result.albums.results.push(album);
            }
            result.albums.total = result.albums.total.max(result.albums.results.len() as u32);
        }

        let existing_tracks: HashSet<String> = result.tracks.results.iter().map(|t| t.id.clone()).collect();
        let mut added_tracks = Vec::new();
        for (tid, meta) in &cat.tracks {
            if existing_tracks.contains(tid) || !cat.replacements.contains_key(tid) {
                continue;
            }
            let title_hit = meta.title.as_deref().is_some_and(|t| t.to_lowercase().contains(&q));
            let artist_hit = meta.artists.iter().any(|a| a.name.to_lowercase().contains(&q));
            let combined_hit = meta
                .title
                .as_deref()
                .is_some_and(|t| meta.artists.iter().any(|a| format!("{} {t}", a.name).to_lowercase().contains(&q) || format!("{t} {}", a.name).to_lowercase().contains(&q)));
            if title_hit || artist_hit || combined_hit {
                let stub = album_for_track(&cat, tid);
                added_tracks.push((title_hit || combined_hit, build_track(&cat, tid, None, stub.as_ref(), false)));
            }
        }
        added_tracks.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.title.cmp(&b.1.title)));
        for (strong, track) in added_tracks.into_iter().take(15) {
            if strong {
                result.tracks.results.insert(0, track);
            } else {
                result.tracks.results.push(track);
            }
            result.tracks.total = result.tracks.total.max(result.tracks.results.len() as u32);
        }
    }
}

fn is_real_ym_track_id(id: &str) -> bool {
    id.parse::<u64>().is_ok_and(|n| n > 0 && n < SYNTHETIC_TRACK_ID_FLOOR)
}

fn to_artist(a: &ArtistRef) -> Artist {
    Artist { id: a.id.clone(), name: a.name.clone(), ..Default::default() }
}

fn year_from_date(date: &str) -> Option<i32> {
    date.get(..4)?.parse::<i32>().ok()
}

fn apply_album_meta(album: &mut Album, meta: &AlbumMeta) {
    if let Some(title) = &meta.title {
        album.title = title.clone();
    }
    if !meta.artists.is_empty() {
        album.artists = meta.artists.iter().map(to_artist).collect();
    }
    if let Some(cover) = &meta.cover_uri {
        album.cover_uri = Some(cover.clone());
    }
    if let Some(date) = &meta.release_date {
        album.release_date = Some(date.clone());
        if album.year.is_none() {
            album.year = year_from_date(date);
        }
    }
    let count: usize = meta.volumes.iter().map(Vec::len).sum();
    if count > 0 && album.track_count.is_none() {
        album.track_count = Some(count as u32);
    }
}

fn album_card_from_meta(meta: &AlbumMeta) -> Album {
    let mut album = Album { id: meta.id.clone(), ..Default::default() };
    apply_album_meta(&mut album, meta);
    album
}

fn album_stub(album: &Album) -> Album {
    Album {
        id: album.id.clone(),
        title: album.title.clone(),
        version: album.version.clone(),
        year: album.year,
        release_date: album.release_date.clone(),
        cover_uri: album.cover_uri.clone(),
        og_image: album.og_image.clone(),
        genre: album.genre.clone(),
        track_count: album.track_count,
        kind: album.kind.clone(),
        artists: album.artists.clone(),
        volumes: Vec::new(),
        labels: Vec::new(),
        likes_count: album.likes_count,
    }
}

fn album_for_track(cat: &Catalog, track_id: &str) -> Option<Album> {
    cat.albums.values().find(|a| a.volumes.iter().flatten().any(|id| id == track_id)).map(album_card_from_meta)
}

fn apply_track(cat: &Catalog, track: &mut Track, album: Option<&Album>, replace_artists: bool) {
    if let Some(meta) = cat.tracks.get(&track.id) {
        if let Some(title) = &meta.title {
            track.title = title.clone();
        }
        if !meta.artists.is_empty() {
            track.artists = meta.artists.iter().map(to_artist).collect();
        }
        if let Some(cover) = &meta.cover_uri {
            track.cover_uri = Some(cover.clone());
        }
        if let Some(dur) = meta.duration_ms
            && track.duration_ms.unwrap_or(0) == 0
        {
            track.duration_ms = Some(dur);
        }
    }
    if let Some(rep) = cat.replacements.get(&track.id) {
        track.available = Some(true);
        if let Some(dur) = rep.duration_ms
            && track.duration_ms.unwrap_or(0) == 0
        {
            track.duration_ms = Some(dur);
        }
    }
    if let Some(album) = album {
        if (replace_artists || track.artists.is_empty()) && !album.artists.is_empty() {
            track.artists = album.artists.clone();
        }
        if track.cover_uri.is_none() && track.og_image.is_none() {
            track.cover_uri = album.cover_uri.clone().or_else(|| album.og_image.clone());
        }
        if track.albums.is_empty() {
            track.albums.push(album.clone());
        }
    }
}

fn build_track(cat: &Catalog, id: &str, base: Option<Track>, album: Option<&Album>, replace_artists: bool) -> Track {
    let mut track = base.unwrap_or_else(|| Track { id: id.to_owned(), title: id.to_owned(), ..Default::default() });
    if track.id.is_empty() {
        track.id = id.to_owned();
    }
    apply_track(cat, &mut track, album, replace_artists);
    track
}
