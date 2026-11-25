//! Loading Home, Library, and detail pages (album, playlist, artist).

use ym_api::{Album, ArtistPage, Playlist, Track};

use super::{Detail, DownloadState, FromName, LibTab, Load, Page, PlayingFrom, Source, State};
use crate::i18n::{Lang, S};

/// Returns `(display_name, is_yandex)` for a playlist owner, replacing raw `yamusic-*` system
/// logins with localized `"Яндекс Музыка"` / `"Yandex Music"`.
pub fn playlist_owner_display(owner: Option<&ym_api::Owner>, lang: Lang) -> (String, bool) {
    let Some(o) = owner else {
        return (String::new(), false);
    };
    let login = o.login.trim();
    let name = o.name.as_deref().map(str::trim).unwrap_or("");
    let is_ym = login.starts_with("yamusic-")
        || login.eq_ignore_ascii_case("music-blog")
        || login.eq_ignore_ascii_case("yandexmusic")
        || name.starts_with("yamusic-");
    if is_ym {
        return (S::YandexMusic.get(lang).to_owned(), true);
    }
    let label = if !name.is_empty() { name } else { login };
    (label.to_owned(), false)
}

/// Deterministic xorshift64* shuffle seeded per refresh so My Waves show fresh combinations.
fn shuffle_with_seed<T>(slice: &mut [T], mut seed: u64) {
    if slice.len() <= 1 {
        return;
    }
    if seed == 0 {
        seed = 0x9e37_79b9_7f4a_7c15;
    }
    for i in (1..slice.len()).rev() {
        seed ^= seed >> 12;
        seed ^= seed << 25;
        seed ^= seed >> 27;
        let r = seed.wrapping_mul(0x2545_f491_4f6c_dd1d);
        let j = (r as usize) % (i + 1);
        slice.swap(i, j);
    }
}

/// Deduplicates `/wheel/new` entries by station key while preserving Yandex Music's full list.
pub(super) fn filter_wheel_waves(items: Vec<ym_api::WheelItem>) -> Vec<ym_api::WheelItem> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for item in items {
        let key = item.key();
        if item.name.trim().is_empty() || !seen.insert(key) {
            continue;
        }
        out.push(item);
    }
    out
}

impl State {
    pub fn reload_content(&mut self) {
        if !matches!(self.home, Load::Ready(_)) {
            self.home = Load::Loading;
        }
        if !matches!(self.library.tracks, Load::Ready(_)) {
            self.library.tracks = Load::Loading;
        }
        if !matches!(self.library.playlists, Load::Ready(_)) {
            self.library.playlists = Load::Idle;
        }
        if !matches!(self.library.albums, Load::Ready(_)) {
            self.library.albums = Load::Idle;
        }
        if !matches!(self.library.artists, Load::Ready(_)) {
            self.library.artists = Load::Idle;
        }
        if !matches!(self.library.top_artists, Load::Ready(_)) {
            self.library.top_artists = Load::Idle;
        }
        if !matches!(self.library.liked_playlists, Load::Ready(_)) {
            self.library.liked_playlists = Load::Idle;
        }
        self.library.collections_fresh = false;
        let (b, uid) = (self.backend.clone(), self.account.as_ref().map_or(0, |a| a.uid));
        let (fc, fc_on) = (self.fkcensor.clone(), self.settings.fkcensor);
        let b2 = b.clone();
        self.job(async move { b2.home().await }, |s, _, res| match res {
            Ok(v) => {
                s.home = Load::Ready(v);
                s.save_home_snapshot();
            }
            Err(e) => {
                s.error_toast(&e);
                if !matches!(s.home, Load::Ready(_)) {
                    s.home = Load::Failed(e.to_string());
                }
            }
        });
        self.refresh_wheel_waves();
        self.job(async move { b.liked_tracks(uid, &fc, fc_on).await }, move |s, _, res| match res {
            Ok(v) => {
                s.liked = v.iter().map(|t| t.id.clone()).collect();
                s.library.tracks = Load::Ready(v);
                s.save_library_snapshot();
            }
            Err(_) if matches!(s.library.tracks, Load::Ready(_)) => {}
            Err(e) => {
                let offline: Vec<Track> =
                    s.download_order.iter().filter(|id| matches!(s.downloads.get(*id), Some(DownloadState::Done))).filter_map(|id| s.downloaded_tracks.get(id).cloned()).collect();
                if !offline.is_empty() {
                    s.liked = offline.iter().map(|t| t.id.clone()).collect();
                    s.library.tracks = Load::Ready(offline);
                    s.toast(s.tr(S::Offline));
                } else {
                    s.library.tracks = Load::Failed(e.to_string());
                }
            }
        });
        self.preload_library();
    }

    /// Fetches fresh personalized My Waves for the Home arc wheel (`/wheel/new`).
    pub fn refresh_wheel_waves(&mut self) {
        let show_bones = self.wheel_waves.is_empty();
        self.refresh_wheel_waves_for(&[], show_bones);
    }

    /// Fetches fresh personalized My Waves, showing arc skeletons while loading when `show_bones`
    /// is true (e.g. when switching waves or resetting via the pill close button).
    pub fn refresh_wheel_waves_for(&mut self, seeds: &[String], show_bones: bool) {
        self.wheel_gen = self.wheel_gen.wrapping_add(1);
        let wheel_gen = self.wheel_gen;
        if show_bones {
            // Remember the waves currently visible on screen so the refreshed lineup puts fresh
            // vibes and artists at the front even when `/wheel/new` returns overlapping pools.
            let n = self.wheel_waves.len();
            if n > 0 {
                let base = self.wave_arc.round() as isize;
                for off in -3isize..=3 {
                    let idx = (base + off).rem_euclid(n as isize) as usize;
                    let key = self.wheel_waves[idx].key();
                    if key != "onyourwave" && !self.wheel_seen.contains(&key) {
                        self.wheel_seen.push(key);
                    }
                }
                if self.wheel_seen.len() > 24 {
                    let drop_n = self.wheel_seen.len() - 24;
                    self.wheel_seen.drain(..drop_n);
                }
            }
            self.wheel_loading = true;
            self.wave_arc = 0.0;
            self.wave_arc_to = 0.0;
            self.wave_arc_vel = 0.0;
        }
        let b = self.backend.clone();
        let seeds = seeds.to_vec();
        self.job(async move { b.wheel(&seeds).await }, move |s, _, res| {
            if s.wheel_gen != wheel_gen {
                return;
            }
            s.wheel_loading = false;
            if let Ok(items) = res {
                let mut filtered = filter_wheel_waves(items);
                if !filtered.is_empty() {
                    if show_bones {
                        let mix_seed = wheel_gen.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ s.now.to_bits() ^ s.home_seed;
                        let active = s.wave_choice.clone();
                        let seen: std::collections::HashSet<&str> = s.wheel_seen.iter().map(String::as_str).collect();
                        let mut accent_item: Option<ym_api::WheelItem> = None;
                        let mut fresh_items = Vec::new();
                        let mut repeat_items = Vec::new();
                        for it in filtered.drain(..) {
                            if it.control_accent {
                                accent_item = Some(it);
                                continue;
                            }
                            let key = it.key();
                            if active != "onyourwave" && key == active {
                                repeat_items.push(it);
                            } else if !seen.contains(key.as_str()) {
                                fresh_items.push(it);
                            } else {
                                repeat_items.push(it);
                            }
                        }
                        // If Yandex returned the exact same artist waves we just showed, pull fresh
                        // unseen artists from the user's own collection so artist waves rotate too.
                        let fresh_artists = fresh_items.iter().filter(|w| w.round).count();
                        if fresh_artists < 2 {
                            let existing: std::collections::HashSet<String> = fresh_items
                                .iter()
                                .chain(repeat_items.iter())
                                .map(ym_api::WheelItem::key)
                                .collect();
                            let mut pool: Vec<ym_api::Artist> = Vec::new();
                            if let Some(ar) = s.library.artists.get() {
                                pool.extend(ar.iter().cloned());
                            }
                            if let Some(top) = s.library.top_artists.get() {
                                for ta in top {
                                    if !pool.iter().any(|x| x.id == ta.artist.id) {
                                        pool.push(ta.artist.clone());
                                    }
                                }
                            }
                            pool.retain(|a| {
                                !a.id.is_empty()
                                    && !a.name.is_empty()
                                    && !a.is_various()
                                    && !seen.contains(format!("artist:{}", a.id).as_str())
                                    && !existing.contains(&format!("artist:{}", a.id))
                                    && format!("artist:{}", a.id) != active
                            });
                            shuffle_with_seed(&mut pool, mix_seed ^ 0xa5a5_5a5a);
                            let desc = match s.lang {
                                Lang::Ru => "В стиле артиста",
                                Lang::En => "Artist style",
                            };
                            for a in pool.into_iter().take(3 - fresh_artists.min(3)) {
                                fresh_items.push(ym_api::WheelItem {
                                    name: a.name.clone(),
                                    description: desc.to_owned(),
                                    cover: a.cover_template().map(str::to_owned),
                                    round: true,
                                    control_accent: false,
                                    color: None,
                                    seeds: vec![format!("artist:{}", a.id)],
                                });
                            }
                        }
                        shuffle_with_seed(&mut fresh_items, mix_seed);
                        shuffle_with_seed(&mut repeat_items, mix_seed ^ 0x517c_c1b7_2722_0a95);
                        let mut out = fresh_items;
                        out.extend(repeat_items);
                        if let Some(acc) = accent_item {
                            let at = 2.min(out.len());
                            out.insert(at, acc);
                        }
                        filtered = out;
                    }
                    s.wheel_waves = filtered;
                    s.save_home_snapshot();
                }
            }
        });
    }

    /// Fetches the account's playlists again while keeping visible ones on screen.
    pub fn refresh_playlists(&mut self) {
        let (b, uid) = (self.backend.clone(), self.account.as_ref().map_or(0, |a| a.uid));
        if !matches!(self.library.playlists, Load::Ready(_)) {
            self.library.playlists = Load::Loading;
        }
        self.job(async move { b.playlists(uid).await }, |s, _, res| match res {
            Ok(v) => {
                s.library.playlists = Load::Ready(v);
                s.library.playlists_fresh = true;
                s.save_library_snapshot();
            }
            Err(e) if !matches!(s.library.playlists, Load::Ready(_)) => s.library.playlists = Load::Failed(e.to_string()),
            Err(_) => {}
        });
    }

    pub fn load_library_tab(&mut self, tab: LibTab) {
        if self.library.tab != tab {
            self.art.bump_page();
            self.art_trim_at = Some(self.now + 1.2);
            self.library.filter.clear();
            self.reset_scroll_key("library");
        }
        self.library.tab = tab;
        self.preload_library();
        if tab == LibTab::Dislikes {
            self.load_dislikes();
        }
    }

    /// Fetches disliked tracks and artists lazily when opening the Dislikes view.
    pub fn load_dislikes(&mut self) {
        let Some(uid) = self.account.as_ref().map(|a| a.uid) else { return };
        let b = self.backend.clone();
        if matches!(self.library.disliked_tracks, Load::Idle | Load::Failed(_)) {
            self.library.disliked_tracks = Load::Loading;
            let b2 = b.clone();
            self.job(async move { b2.disliked_tracks(uid).await }, |s, _, res| match res {
                Ok(v) => s.library.disliked_tracks = Load::Ready(v),
                Err(e) => s.library.disliked_tracks = Load::Failed(e.to_string()),
            });
        }
        if matches!(self.library.disliked_artists, Load::Idle | Load::Failed(_)) {
            self.library.disliked_artists = Load::Loading;
            self.job(async move { b.disliked_artists(uid).await }, |s, _, res| match res {
                Ok(v) => s.library.disliked_artists = Load::Ready(v),
                Err(e) => s.library.disliked_artists = Load::Failed(e.to_string()),
            });
        }
    }

    /// Removes a track from the user's dislikes list.
    pub fn undislike_track(&mut self, track: &Track) {
        let id = track.id.clone();
        if let Load::Ready(list) = &mut self.library.disliked_tracks {
            list.retain(|t| t.id != id);
        }
        let (b, uid, t) = (self.backend.clone(), self.account.as_ref().map_or(0, |a| a.uid), track.clone());
        let text = self.tr(S::RemovedFromDislikes);
        self.toast(text);
        self.job(async move { b.undislike_track(uid, &t).await }, move |s, _, res| {
            if let Err(e) = res {
                s.library.disliked_tracks = Load::Idle;
                s.error_toast(&e);
            }
        });
    }

    /// Removes an artist from the user's dislikes list.
    pub fn undislike_artist(&mut self, artist: &ym_api::Artist) {
        let id = artist.id.clone();
        if let Load::Ready(list) = &mut self.library.disliked_artists {
            list.retain(|a| a.id != id);
        }
        let (b, uid) = (self.backend.clone(), self.account.as_ref().map_or(0, |a| a.uid));
        let text = self.tr(S::RemovedFromDislikes);
        self.toast(text);
        self.job(async move { b.undislike_artist(uid, &id).await }, move |s, _, res| {
            if let Err(e) = res {
                s.library.disliked_artists = Load::Idle;
                s.error_toast(&e);
            }
        });
    }

    /// Completed downloaded tracks in newest-first order.
    pub fn downloaded_list(&self) -> Vec<Track> {
        self.download_order
            .iter()
            .filter(|id| matches!(self.downloads.get(*id), Some(DownloadState::Done)))
            .filter_map(|id| self.downloaded_tracks.get(id).cloned())
            .collect()
    }

    /// Ensures the cached virtual-list slice of downloaded tracks matches the current filter query
    /// and returns its length (O(1) on cache hit).
    pub fn ensure_downloaded_shown(&self) -> usize {
        let q = if self.library.tab == LibTab::Downloads { self.library.filter.text.trim().to_owned() } else { String::new() };
        let key = (self.download_order.len(), self.downloaded_tracks.len(), q.clone());
        let mut cached_key = self.library.downloaded_key.borrow_mut();
        let mut cached = self.library.downloaded_shown.borrow_mut();
        if *cached_key != key {
            let all = self.downloaded_list();
            *cached = if q.is_empty() {
                all
            } else {
                match crate::find::Query::new(&q) {
                    None => all,
                    Some(qr) => crate::find::best(all.iter().map(|t| {
                        let f = crate::ui::kit::track_fields(t);
                        qr.tier(&f.iter().map(String::as_str).collect::<Vec<_>>())
                    }))
                    .into_iter()
                    .filter_map(|i| all.get(i).cloned())
                    .collect(),
                }
            };
            *cached_key = key;
        }
        cached.len()
    }

    /// Preloads playlists, liked playlists, liked albums, liked artists, top artists, and the Liked cover.
    pub fn preload_library(&mut self) {
        let Some(uid) = self.account.as_ref().map(|a| a.uid) else { return };
        if !self.library.playlists_fresh && !self.library.playlists.is_loading() {
            self.refresh_playlists();
        }
        if self.library.collections_fresh {
            return;
        }
        self.library.collections_fresh = true;
        let b = self.backend.clone();
        if !matches!(self.library.albums, Load::Ready(_)) {
            self.library.albums = Load::Loading;
        }
        let b2 = b.clone();
        self.job(async move { b2.liked_albums(uid).await }, |s, _, res| match res {
            Ok(v) => {
                s.library.albums = Load::Ready(v);
                s.save_library_snapshot();
            }
            Err(e) => {
                s.library.collections_fresh = false;
                if !matches!(s.library.albums, Load::Ready(_)) {
                    s.library.albums = Load::Failed(e.to_string());
                }
            }
        });
        if !matches!(self.library.artists, Load::Ready(_)) {
            self.library.artists = Load::Loading;
        }
        let b3 = b.clone();
        self.job(async move { b3.liked_artists(uid).await }, |s, _, res| match res {
            Ok(v) => {
                s.library.artists = Load::Ready(v);
                s.save_library_snapshot();
            }
            Err(e) => {
                s.library.collections_fresh = false;
                if !matches!(s.library.artists, Load::Ready(_)) {
                    s.library.artists = Load::Failed(e.to_string());
                }
            }
        });
        if !matches!(self.library.top_artists, Load::Ready(_)) {
            self.library.top_artists = Load::Loading;
        }
        let b4 = b.clone();
        self.job(async move { b4.top_artists(15).await }, |s, _, res| match res {
            Ok(v) => {
                s.library.top_artists = Load::Ready(v);
                s.save_library_snapshot();
            }
            Err(_) => {
                if !matches!(s.library.top_artists, Load::Ready(_)) {
                    s.library.top_artists = Load::Ready(Vec::new());
                }
            }
        });
        if !matches!(self.library.liked_playlists, Load::Ready(_)) {
            self.library.liked_playlists = Load::Loading;
        }
        let b5 = b.clone();
        self.job(async move { b5.liked_playlists(uid).await }, |s, _, res| match res {
            Ok(v) => {
                s.library.liked_playlists = Load::Ready(v);
                s.save_library_snapshot();
            }
            Err(_) => {
                if !matches!(s.library.liked_playlists, Load::Ready(_)) {
                    s.library.liked_playlists = Load::Ready(Vec::new());
                }
            }
        });
        self.job(async move { b.liked_cover(uid).await }, |s, _, res| {
            if let Ok(Some(tpl)) = res
                && s.library.liked_cover.as_deref() != Some(tpl.as_str())
            {
                s.library.liked_cover = Some(tpl);
                s.save_library_snapshot();
            }
        });
    }

    /// URL of the account's Liked playlist on Yandex Music.
    pub fn liked_link(&self) -> Option<String> {
        let login = self.account.as_ref().map(|a| a.login.trim()).filter(|l| !l.is_empty())?;
        Some(format!("https://music.yandex.ru/users/{login}/playlists/3"))
    }

    pub(crate) fn begin_detail(&mut self, page: Page) -> u64 {
        let is_artist = matches!(page, Page::Artist(_));
        self.detail = Detail { page: Some(page), generation: self.detail.generation + 1, state: Load::Loading, is_artist, ..Detail::default() };
        self.detail.generation
    }

    pub(crate) fn show_album(&mut self, a: Album) {
        let had_various = a.artists.iter().any(ym_api::Artist::is_various);
        self.detail.cover = a.cover_template().map(str::to_owned);
        self.detail.tracks = a.tracks().cloned().collect();
        let real_artists: Vec<ym_api::Artist> = a.artists.into_iter().filter(|r| !r.is_various()).collect();
        self.detail.like_key = Some(format!("album:{}", a.id));
        self.detail.link = Some(format!("https://music.yandex.ru/album/{}", a.id));
        self.detail.artist = real_artists.first().map(|r| r.id.clone()).filter(|id| !id.is_empty());
        self.detail.subtitle = real_artists.iter().map(|r| r.name.as_str()).filter(|n| !n.is_empty()).collect::<Vec<_>>().join(", ");
        let mut facts = Vec::new();
        if let Some(y) = a.year {
            facts.push((S::Year, y.to_string()));
        }
        if let Some(g) = a.genre.as_deref().filter(|g| !g.is_empty()) {
            facts.push((S::Genre, crate::genre::name(g, self.lang)));
        }
        if let Some(date) = a.release_date.as_deref().filter(|d| !d.is_empty()) {
            facts.push((S::ReleaseDate, crate::i18n::release_date(date, self.lang)));
        }
        if let Some(version) = a.version.as_deref().filter(|v| !v.is_empty()) {
            facts.push((S::Version, version.to_owned()));
        }
        facts.extend(self.length_facts());
        self.detail.artists = real_artists;
        self.detail.release_kind = a.kind.filter(|k| !k.is_empty()).or_else(|| had_various.then(|| "compilation".to_owned()));
        self.detail.year = a.year;
        self.detail.labels = a.labels;
        self.detail.likes = a.likes_count.filter(|n| *n > 0);
        self.detail.title = a.title;
        self.detail.facts = facts;
        self.detail.state = Load::Ready(());
    }

    pub(crate) fn show_playlist(&mut self, p: Playlist) {
        let own = self.account.as_ref().is_some_and(|a| a.uid == p.uid);
        self.detail.like_key = (!own).then(|| format!("playlist:{}:{}", p.uid, p.kind));
        let owner = p.owner.as_ref().map(|o| o.login.as_str()).filter(|l| !l.is_empty()).map_or_else(|| p.uid.to_string(), str::to_owned);
        self.detail.link = Some(format!("https://music.yandex.ru/users/{owner}/playlists/{}", p.kind));
        self.detail.cover = p.cover_template().map(str::to_owned);
        let mut tracks: Vec<Track> = p.resolved_tracks().cloned().collect();
        self.apply_local_playlist(p.uid, p.kind, &mut tracks);
        self.detail.tracks = tracks;
        let mut facts = self.length_facts();
        if let Some(d) = p.description.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
            facts.push((S::Description, d.to_owned()));
        }
        self.detail.title = p.title;
        let (owner_label, is_ym) = playlist_owner_display(p.owner.as_ref(), self.lang);
        self.detail.subtitle = owner_label;
        self.detail.yandex_playlist = is_ym;
        self.detail.playlist_private = p.visibility.as_deref() == Some("private");
        self.detail.facts = facts;
        self.detail.state = Load::Ready(());
    }

    pub(crate) fn show_artist(&mut self, a: ArtistPage) {
        self.detail.like_key = Some(format!("artist:{}", a.artist.id));
        self.detail.link = Some(format!("https://music.yandex.ru/artist/{}", a.artist.id));
        let listeners = a.listeners();
        self.detail.monthly_listeners = listeners;
        self.detail.listeners_delta = a.listeners_delta();
        self.detail.photos = a.photos();
        self.detail.cover = a.artist.cover_template().map(str::to_owned).or_else(|| a.popular_tracks.first().and_then(|t| t.cover_template().map(str::to_owned)));
        let mut facts = Vec::new();
        if !a.artist.genres.is_empty() {
            facts.push((S::Genres, crate::genre::names(&a.artist.genres, self.lang)));
        }
        if !a.albums.is_empty() {
            facts.push((S::Albums, a.albums.len().to_string()));
        }
        if !a.similar_artists.is_empty() {
            facts.push((S::SimilarArtists, a.similar_artists.len().to_string()));
        }
        if let Some(listeners) = listeners {
            facts.push((S::MonthlyListeners, crate::i18n::group_digits(listeners)));
        }
        if let Some(text) = a.artist.description.as_ref().map(|d| d.text.trim()).filter(|t| !t.is_empty()) {
            facts.push((S::Description, text.to_owned()));
        }
        self.detail.title = a.artist.name;
        self.detail.bandlink = a.bandlink_scanner_link.filter(|l| l.url.starts_with("https://"));
        self.detail.links = a.artist.links.into_iter().filter(|l| !l.href.is_empty() && !l.is_donation()).collect();
        self.detail.donation = a.donation;
        self.detail.facts = facts;
        self.detail.popular = a.popular_tracks.clone();
        self.detail.tracks = a.popular_tracks;
        self.detail.albums = a.albums;
        self.detail.similar = a.similar_artists;
        self.detail.state = Load::Ready(());
    }

    /// Track count and total duration facts for the open page.
    fn length_facts(&self) -> Vec<(S, String)> {
        let t = &self.detail.tracks;
        if t.is_empty() {
            return Vec::new();
        }
        let ms: u64 = t.iter().filter_map(|t| t.duration_ms).sum();
        vec![(S::Tracks, t.len().to_string()), (S::Duration, self.lang.duration(ms / 1000))]
    }

    fn detail_failed(&mut self, e: &ym_api::Error) {
        self.detail.state = Load::Failed(e.to_string());
        self.error_toast(e);
    }

    pub fn open_album(&mut self, id: &str) {
        let generation = self.begin_detail(Page::Album(id.into()));
        let (b, id, fc, fc_on) = (self.backend.clone(), id.to_owned(), self.fkcensor.clone(), self.settings.fkcensor);
        self.job(async move { b.album(&id, &fc, fc_on).await }, move |s, _, res| {
            if s.detail.generation == generation {
                match res {
                    Ok(a) => s.show_album(a),
                    Err(e) => s.detail_failed(&e),
                }
            }
        });
    }

    pub fn open_playlist(&mut self, uid: i64, kind: i64) {
        let generation = self.begin_detail(Page::Playlist(uid, kind));
        let (b, fc, fc_on) = (self.backend.clone(), self.fkcensor.clone(), self.settings.fkcensor);
        self.job(async move { b.playlist(uid, kind, &fc, fc_on).await }, move |s, _, res| {
            if s.detail.generation == generation {
                match res {
                    Ok(p) => s.show_playlist(p),
                    Err(e) => s.detail_failed(&e),
                }
            }
        });
    }

    /// Plays an album from its first track without opening its page.
    pub fn play_album(&mut self, id: &str, title: &str) {
        let (b, id, title, fc, fc_on) = (self.backend.clone(), id.to_owned(), title.to_owned(), self.fkcensor.clone(), self.settings.fkcensor);
        self.job(async move { b.album(&id, &fc, fc_on).await }, move |s, _, res| match res {
            Ok(a) => {
                let tracks: Vec<Track> = a.tracks().cloned().collect();
                let k = crate::app::album_kind_name(&a);
                let kind = if k == S::Album && tracks.len() == 1 { S::Single } else { k };
                s.play_list_from(tracks, kind, title, Page::Album(a.id));
            }
            Err(e) => s.error_toast(&e),
        });
    }

    /// Plays a playlist from its first track without opening its page.
    pub fn play_playlist(&mut self, uid: i64, kind: i64, title: &str) {
        let (b, title, fc, fc_on) = (self.backend.clone(), title.to_owned(), self.fkcensor.clone(), self.settings.fkcensor);
        self.job(async move { b.playlist(uid, kind, &fc, fc_on).await }, move |s, _, res| match res {
            Ok(p) => {
                let mut tracks: Vec<Track> = p.resolved_tracks().cloned().collect();
                s.apply_local_playlist(uid, kind, &mut tracks);
                s.play_list_from(tracks, S::Playlist, title, Page::Playlist(uid, kind));
            }
            Err(e) => s.error_toast(&e),
        });
    }

    fn play_list_from(&mut self, tracks: Vec<Track>, kind: S, name: String, page: Page) {
        if tracks.is_empty() {
            return;
        }
        self.play_queue(tracks, 0, Source::Manual);
        self.playing_from = Some(PlayingFrom { wave: false, kind, name: FromName::Text(name), page: Some(page) });
    }

    pub fn open_artist(&mut self, id: &str) {
        let generation = self.begin_detail(Page::Artist(id.into()));
        let (b, id, fc, fc_on) = (self.backend.clone(), id.to_owned(), self.fkcensor.clone(), self.settings.fkcensor);
        self.job(async move { b.artist(&id, &fc, fc_on).await }, move |s, _, res| {
            if s.detail.generation == generation {
                match res {
                    Ok(a) => s.show_artist(a),
                    Err(e) => s.detail_failed(&e),
                }
            }
        });
    }

    /// Switches an artist page between popular tracks and the full track list (fetched on demand).
    pub fn show_all_artist_tracks(&mut self, all: bool) {
        let Some(Page::Artist(id)) = self.detail.page.clone() else { return };
        self.detail.showing_all = all;
        if !all {
            self.detail.tracks = self.detail.popular.clone();
            self.detail_tracks_changed();
            return;
        }
        if let Load::Ready(v) = &self.detail.all_tracks {
            self.detail.tracks = v.clone();
            self.detail_tracks_changed();
            return;
        }
        self.detail.all_tracks = Load::Loading;
        let (b, generation, fc, fc_on) = (self.backend.clone(), self.detail.generation, self.fkcensor.clone(), self.settings.fkcensor);
        self.job(async move { b.artist_tracks(&id, &fc, fc_on).await }, move |s, _, res| {
            if s.detail.generation != generation {
                return;
            }
            match res {
                Ok(v) => {
                    if s.detail.showing_all {
                        s.detail.tracks = v.clone();
                        s.detail_tracks_changed();
                    }
                    s.detail.all_tracks = Load::Ready(v);
                }
                Err(e) => {
                    s.detail.all_tracks = Load::Failed(e.to_string());
                    s.detail.showing_all = false;
                    s.error_toast(&e);
                }
            }
        });
    }
}
