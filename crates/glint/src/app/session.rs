//! Authentication, startup boot sequence, snapshots, and account avatar loading.

use rune::TextEdit;
use ym_api::Account;

use super::{APP_URL, Detail, DownloadState, Library, Load, Page, SearchUi, Session, State};
use crate::i18n::S;
use crate::net::Backend;
use crate::store::{DownloadedTrack, HomeSnapshot, LibrarySnapshot, PlaybackSnapshot, Store};

impl State {
    /// Signs in with a pasted or restored token.
    pub fn sign_in(&mut self, token: String, remember: bool) {
        self.boot_gen = self.boot_gen.wrapping_add(1);
        let Ok(client) = ym_api::Client::new() else {
            self.login.error = Some(S::Offline);
            return;
        };
        client.set_token(Some(token.clone()));
        client.set_language(self.lang.code());
        client.set_lrcred_contact(self.settings.word_lyrics.then_some(APP_URL));
        self.lrc_usable = self.settings.word_lyrics;
        self.login.busy = true;
        self.login.error = None;
        let client_for_fkcensor = client.clone();
        let backend = Backend::Live(client);
        let b2 = backend.clone();
        self.job(async move { b2.account().await }, move |s, _tx, res| {
            s.login.busy = false;
            match res {
                Ok(acc) => {
                    if remember {
                        s.store.save_token(Some(&token));
                    }
                    s.backend = backend;
                    s.session = Session::Live;
                    s.account = Some(acc);
                    if s.settings.fkcensor {
                        s.refresh_fkcensor(client_for_fkcensor);
                    }
                    s.fetch_avatar();
                    s.login.token = TextEdit::new("");
                    s.reload_content();
                    s.mark_settings_dirty();
                }
                Err(ym_api::Error::Unauthorized) => {
                    s.store.save_token(None);
                    s.login.error = Some(S::TokenRejected);
                }
                Err(e) => {
                    if matches!(e, ym_api::Error::Network(_)) && !s.downloaded_tracks.is_empty() {
                        s.backend = backend;
                        s.session = Session::Live;
                        s.account = Some(Account { uid: 0, login: String::new(), display_name: String::new(), has_plus: false });
                        s.login.token = TextEdit::new("");
                        s.login.error = None;
                        s.reload_content();
                        s.toast(S::Offline.get(s.lang));
                    } else {
                        s.login.error = Some(match e {
                            ym_api::Error::Network(_) => S::Offline,
                            _ => S::TokenRejected,
                        });
                    }
                }
            }
        });
    }

    fn attach_boot_client(&mut self, token: String, client: ym_api::Client) {
        client.set_token(Some(token));
        client.set_language(self.lang.code());
        client.set_lrcred_contact(self.settings.word_lyrics.then_some(APP_URL));
        self.lrc_usable = self.settings.word_lyrics;
        self.login.error = None;
        let fkcensor_client = client.clone();
        self.backend = Backend::Live(client);
        self.session = Session::Live;
        if let Some(path) = self.store.library_snapshot_path() {
            self.job(async move { tokio::task::spawn_blocking(move || Store::read_library_snapshot(&path)).await.ok().flatten() }, |s, _, snap| {
                let Some(snap) = snap else { return };
                if !matches!(s.library.tracks, Load::Ready(_)) && !snap.liked.is_empty() {
                    s.liked = snap.liked.iter().map(|t| t.id.clone()).collect();
                    s.library.tracks = Load::Ready(snap.liked);
                }
                if !matches!(s.library.playlists, Load::Ready(_)) && !snap.playlists.is_empty() {
                    s.library.playlists = Load::Ready(snap.playlists);
                }
                if !matches!(s.library.albums, Load::Ready(_)) && !snap.albums.is_empty() {
                    s.library.albums = Load::Ready(snap.albums);
                }
                if !matches!(s.library.artists, Load::Ready(_)) && !snap.artists.is_empty() {
                    s.library.artists = Load::Ready(snap.artists);
                }
                if s.library.liked_cover.is_none() {
                    s.library.liked_cover = snap.liked_cover;
                }
            });
        }
        let known = self.account.is_some();
        if known {
            self.reload_content();
        }
        if self.settings.fkcensor {
            self.refresh_fkcensor(fkcensor_client);
        }
        let b = self.backend.clone();
        self.job(async move { b.account().await }, move |s, _, res| match res {
            Ok(acc) => {
                let changed = s.account.as_ref().is_none_or(|a| a.uid != acc.uid);
                s.account = Some(acc);
                s.save_home_snapshot();
                s.fetch_avatar();
                if changed {
                    s.reload_content();
                }
            }
            Err(ym_api::Error::Unauthorized) => {
                s.reset_session();
                s.login.error = Some(S::TokenRejected);
            }
            Err(e) => {
                if !known {
                    s.account = Some(Account { uid: 0, login: String::new(), display_name: String::new(), has_plus: false });
                    s.reload_content();
                }
                s.error_toast(&e);
            }
        });
    }

    fn finish_boot_session(&mut self, boot_gen: u64, session: Option<(String, Result<ym_api::Client, ym_api::Error>)>) {
        if self.boot_gen != boot_gen || self.session != Session::Live || !matches!(self.backend, Backend::None) {
            return;
        }
        match session {
            Some((token, Ok(client))) => {
                self.attach_boot_client(token, client);
                self.auto_check_updates();
            }
            Some((_, Err(_))) => {
                self.login.error = Some(S::Offline);
                self.auto_check_updates();
            }
            None => {
                self.session = Session::LoggedOut;
                self.account = None;
                self.avatar = None;
                self.home = Load::Idle;
                self.launch_play = false;
                self.auto_check_updates();
            }
        }
    }

    fn finish_boot_playback(&mut self, boot_gen: u64, snap: Option<PlaybackSnapshot>) {
        if self.boot_gen != boot_gen || self.session == Session::LoggedOut {
            return;
        }
        self.boot_playback_done = true;
        if let Some(snap) = snap {
            self.restore_playback(snap);
        }
    }

    fn apply_loaded_downloads(&mut self, downloads_gen: u64, items: Vec<DownloadedTrack>) {
        if self.downloads_gen != downloads_gen {
            return;
        }
        for item in items {
            let id = item.track.id.clone();
            if self.downloads.contains_key(&id) || self.deleted_downloads.contains(&id) {
                continue;
            }
            self.downloads.insert(id.clone(), DownloadState::Done);
            self.download_titles.insert(id.clone(), item.track.full_title());
            self.download_order.push(id.clone());
            self.downloaded_tracks.insert(id, item.track);
        }
        self.deleted_downloads.clear();
    }

    /// Saves the account and home shelves for the next launch.
    pub fn save_home_snapshot(&self) {
        if self.session != Session::Live {
            return;
        }
        let home = match &self.home {
            Load::Ready(h) => h.clone(),
            _ => Vec::new(),
        };
        self.store.save_snapshot("home.json", HomeSnapshot { account: self.account.clone(), avatar: self.avatar.clone(), home });
    }

    /// Saves liked tracks, playlists, albums, and artists for the next launch.
    pub fn save_library_snapshot(&self) {
        if self.session != Session::Live {
            return;
        }
        let liked = match &self.library.tracks {
            Load::Ready(t) => t.clone(),
            _ => return,
        };
        let playlists = match &self.library.playlists {
            Load::Ready(p) => p.clone(),
            _ => Vec::new(),
        };
        let albums = self.library.albums.get().cloned().unwrap_or_default();
        let artists = self.library.artists.get().cloned().unwrap_or_default();
        let liked_cover = self.library.liked_cover.clone();
        self.store.save_snapshot("library.json", LibrarySnapshot { liked, playlists, albums, artists, liked_cover });
    }

    /// Tries the pasted text as a token; shows an error if it is invalid.
    pub fn submit_token(&mut self) {
        match ym_api::auth::extract_token(&self.login.token.text) {
            Some(t) => self.sign_in(t, true),
            None => self.login.error = Some(S::BadToken),
        }
    }

    /// Clears all live session data, stops playback, and removes saved token and snapshots.
    fn reset_session(&mut self) {
        self.menu_open = false;
        self.close_volume();
        self.stop_all();
        self.store.save_token(None);
        self.store.clear_snapshots();
        self.backend = Backend::None;
        self.session = Session::LoggedOut;
        self.account = None;
        self.avatar = None;
        self.home = Load::Idle;
        self.liked.clear();
        self.liked_items.clear();
        self.likes_loaded = false;
        self.library = Library::default();
        self.detail = Detail::default();
        self.search = SearchUi::default();
        self.history.clear();
        self.forward.clear();
        self.playing_from = None;
        self.page = Page::Home;
        self.np_open = false;
        self.launch_play = false;
        self.track_max_quality.clear();
        self.max_quality_jobs.clear();
    }

    pub fn sign_out(&mut self) {
        self.boot_gen = self.boot_gen.wrapping_add(1);
        self.reset_session();
        self.mark_settings_dirty();
        let text = self.tr(S::SignedOut);
        self.toast(text);
    }

    /// Restores last session on frame 1 from the home snapshot while decrypting the token,
    /// opening the client, reading playback/downloads, and sweeping caches on a worker thread.
    pub fn boot(&mut self) {
        self.launch = Some(Default::default());
        let has_token = self.store.has_token();
        if has_token {
            self.session = Session::Live;
            self.lrc_usable = self.settings.word_lyrics;
            let snap = self.store.load_home_snapshot().unwrap_or_default();
            self.account = snap.account;
            self.avatar = snap.avatar;
            if !snap.home.is_empty() {
                self.home = Load::Ready(snap.home);
            }
            self.launch_play = !self.headless && self.settings.play_on_launch;
        }
        let Some(handle) = self.app.clone() else { return };
        let store = self.store.clone();
        let download_store = self.download_store.clone();
        let cache = self.cache.clone();
        let legacy = self.store.legacy_downloads_dir();
        let autostart = self.settings.autostart;
        let boot_gen = self.boot_gen;
        let downloads_gen = self.downloads_gen;
        let _ = std::thread::Builder::new().name("glint-boot".into()).spawn(move || {
            if has_token {
                let session = store.load_token().map(|token| {
                    let client = ym_api::Client::new();
                    (token, client)
                });
                let h = handle.clone();
                h.update(move |s, _tx| s.finish_boot_session(boot_gen, session));
                let playback = store.load_playback();
                let h = handle.clone();
                h.update(move |s, _tx| s.finish_boot_playback(boot_gen, playback));
            } else {
                handle.update(|s, _tx| {
                    s.boot_playback_done = true;
                    s.auto_check_updates();
                });
            }
            if let Some(ds) = download_store {
                if let Some(legacy) = legacy {
                    ds.adopt(&legacy);
                }
                let items = ds.tracks();
                if !items.is_empty() {
                    handle.update(move |s, _tx| s.apply_loaded_downloads(downloads_gen, items));
                }
            }
            if let Some(c) = cache {
                c.sweep_tmp();
            }
            glint_update::cleanup();
            if autostart {
                crate::autostart::apply(true);
            }
        });
    }

    /// Asks Yandex ID for the account's avatar URL.
    pub fn fetch_avatar(&mut self) {
        let b = self.backend.clone();
        self.job(async move { b.avatar().await }, |s, _, url| {
            if url.is_some() && url != s.avatar {
                s.avatar = url;
                s.save_home_snapshot();
            }
        });
    }
}
