//! Loading Home, Library, and detail pages (album, playlist, artist).

use ym_api::{Album, ArtistPage, Playlist, Track};

use super::{Detail, DownloadState, FromName, LibTab, Load, Page, PlayingFrom, Source, State};
use crate::i18n::S;

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
        self.library.collections_fresh = false;
        let (b, uid) = (self.backend.clone(), self.account.as_ref().map_or(0, |a| a.uid));
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
        self.job(async move { b.liked_tracks(uid).await }, move |s, _, res| match res {
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
        self.library.tab = tab;
        self.preload_library();
    }

    /// Preloads playlists, liked albums, liked artists, and the Liked cover so every tab shows its count.
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
        self.detail.like_key = Some(format!("album:{}", a.id));
        self.detail.link = Some(format!("https://music.yandex.ru/album/{}", a.id));
        self.detail.artist = a.artists.first().map(|r| r.id.clone()).filter(|id| !id.is_empty());
        self.detail.subtitle = a.artists_line();
        self.detail.cover = a.cover_template().map(str::to_owned);
        self.detail.tracks = a.tracks().cloned().collect();
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
        self.detail.artists = a.artists;
        self.detail.release_kind = a.kind.filter(|k| !k.is_empty());
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
        self.detail.tracks = p.resolved_tracks().cloned().collect();
        let mut facts = self.length_facts();
        if let Some(d) = p.description.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
            facts.push((S::Description, d.to_owned()));
        }
        self.detail.title = p.title;
        self.detail.subtitle = p.owner.and_then(|o| o.name.or(Some(o.login))).unwrap_or_default();
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
        let (b, id) = (self.backend.clone(), id.to_owned());
        self.job(async move { b.album(&id).await }, move |s, _, res| {
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
        let b = self.backend.clone();
        self.job(async move { b.playlist(uid, kind).await }, move |s, _, res| {
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
        let (b, id, title) = (self.backend.clone(), id.to_owned(), title.to_owned());
        self.job(async move { b.album(&id).await }, move |s, _, res| match res {
            Ok(a) => {
                let tracks: Vec<Track> = a.tracks().cloned().collect();
                s.play_list_from(tracks, S::Album, title, Page::Album(a.id));
            }
            Err(e) => s.error_toast(&e),
        });
    }

    /// Plays a playlist from its first track without opening its page.
    pub fn play_playlist(&mut self, uid: i64, kind: i64, title: &str) {
        let (b, title) = (self.backend.clone(), title.to_owned());
        self.job(async move { b.playlist(uid, kind).await }, move |s, _, res| match res {
            Ok(p) => {
                let tracks: Vec<Track> = p.resolved_tracks().cloned().collect();
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
        let (b, id) = (self.backend.clone(), id.to_owned());
        self.job(async move { b.artist(&id).await }, move |s, _, res| {
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
        let (b, generation) = (self.backend.clone(), self.detail.generation);
        self.job(async move { b.artist_tracks(&id).await }, move |s, _, res| {
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
