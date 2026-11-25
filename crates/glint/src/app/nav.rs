//! Page navigation, history stack, scroll memory, pinning, sorting, and collection likes.

use ym_api::{Album, Track};

use super::{DiscSort, Load, Page, PinnedItem, Session, State, TrackSort, clear_scroll, page_scroll};
use crate::i18n::S;

impl State {
    pub fn go(&mut self, page: Page) {
        if self.page == Page::Search && page != Page::Search {
            self.remember_search();
        }
        self.menu_open = false;
        self.tmenu_open = false;
        self.close_volume();
        self.blur_edits();
        self.sheet_open = false;
        if self.page == page && !self.np_open {
            return;
        }
        self.history.push((self.page.clone(), self.np_open));
        if self.history.len() > 40 {
            self.history.remove(0);
        }
        self.forward.clear();
        self.np_open = false;
        if self.settings.reduce_motion || crate::fx::cpu_mode() {
            self.np = super::Pop::default();
        }
        self.glide.active = false;
        if self.page != page {
            self.keep_scroll(&page);
            self.page = page.clone();
            self.art_trim_at = Some(self.now + 0.5);
            self.enter(page);
        }
    }

    /// Remembers the current scroll offset and schedules restoration for `to`.
    fn keep_scroll(&mut self, to: &Page) {
        let from = self.page.key();
        if let Some([offset, _]) = page_scroll(&from) {
            self.scroll_memory.insert(from, offset);
        }
        let key = to.key();
        clear_scroll(&key);
        self.scroll_restore = self.scroll_memory.get(&key).copied().filter(|y| *y > 1.0).map(|y| (key, y, self.now + 4.0));
    }

    /// Whether there is a page or player state to go back to (or an artist's full track list to collapse).
    pub fn can_back(&self) -> bool {
        !self.history.is_empty() || (!self.np_open && self.detail.showing_all && matches!(self.page, Page::Artist(_)))
    }

    pub fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    pub fn back(&mut self) {
        if !self.np_open && self.detail.showing_all && matches!(self.page, Page::Artist(_)) {
            self.show_all_artist_tracks(false);
            return;
        }
        while let Some((p, np)) = self.history.pop() {
            let want_np = np && self.current().is_some();
            if p == self.page && want_np == self.np_open {
                continue;
            }
            self.forward.push((self.page.clone(), self.np_open));
            self.leave_to(p, want_np);
            break;
        }
    }

    pub fn go_forward(&mut self) {
        while let Some((p, np)) = self.forward.pop() {
            let want_np = np && self.current().is_some();
            if p == self.page && want_np == self.np_open {
                continue;
            }
            self.history.push((self.page.clone(), self.np_open));
            self.leave_to(p, want_np);
            break;
        }
    }

    /// Switches to `(p, np)` during back/forward navigation without pushing to history.
    fn leave_to(&mut self, p: Page, np: bool) {
        if self.page == Page::Search && p != Page::Search {
            self.remember_search();
        }
        self.menu_open = false;
        self.tmenu_open = false;
        self.close_volume();
        self.blur_edits();
        if self.np_open != np {
            crate::diag_log::event("ui", if np { "np open" } else { "np close" });
            self.log_mem(if np { "np:open" } else { "np:close" });
            self.np_open = np;
            if self.settings.reduce_motion || crate::fx::cpu_mode() {
                self.np = super::Pop { t: if np { 1.0 } else { 0.0 }, v: 0.0 };
            }
        }
        self.glide.active = false;
        let same = self.page == p;
        if !same {
            self.keep_scroll(&p);
        }
        self.page = p.clone();
        if !same {
            self.enter(p);
        }
    }

    /// Triggers initial data loading when entering a page.
    fn enter(&mut self, page: Page) {
        let kind = match &page {
            Page::Home => "Home",
            Page::Explore => "Explore",
            Page::Library => "Library",
            Page::Search => "Search",
            Page::Album(_) => "Album",
            Page::Playlist(_, _) => "Playlist",
            Page::Artist(_) => "Artist",
        };
        crate::diag_log::event("nav", &format!("page={kind}"));
        self.log_mem(&format!("nav:{kind}"));
        self.art.bump_page();
        self.art_trim_at = Some(self.now + 0.5);
        match page {
            Page::Library => self.load_library_tab(self.library.tab),
            Page::Album(id) => self.open_album(&id),
            Page::Playlist(uid, kind) => self.open_playlist(uid, kind),
            Page::Artist(id) => self.open_artist(&id),
            _ => {}
        }
        self.ensure_likes();
    }

    /// Fetches liked album, artist, and playlist keys once per session.
    pub fn ensure_likes(&mut self) {
        if self.likes_loaded || self.session == Session::LoggedOut {
            return;
        }
        self.likes_loaded = true;
        let (b, uid) = (self.backend.clone(), self.account.as_ref().map_or(0, |a| a.uid));
        self.job(async move { b.liked_items(uid).await }, |s, _, res| match res {
            Ok(keys) => s.liked_items.extend(keys),
            Err(_) => s.likes_loaded = false,
        });
    }

    pub fn is_item_liked(&self, key: &str) -> bool {
        self.liked_items.contains(key)
    }

    /// Toggles the collection like for an album (`album:{id}`), playlist (`playlist:{uid}:{kind}`),
    /// or artist (`artist:{id}`).
    pub fn toggle_item_like(&mut self, key: &str) {
        let key = key.to_owned();
        let like = !self.liked_items.contains(&key);
        let (what, id) = match key.split_once(':') {
            Some(("album", id)) => (ym_api::Likeable::Album, id.to_owned()),
            Some(("artist", id)) => (ym_api::Likeable::Artist, id.to_owned()),
            Some(("playlist", id)) => (ym_api::Likeable::Playlist, id.to_owned()),
            _ => return,
        };
        if like {
            self.liked_items.insert(key.clone());
        } else {
            self.liked_items.remove(&key);
        }
        match what {
            ym_api::Likeable::Album => self.library.albums = Load::Idle,
            ym_api::Likeable::Artist => self.library.artists = Load::Idle,
            ym_api::Likeable::Playlist => self.library.playlists = Load::Idle,
        }
        let text = self.tr(if like { S::AddedToCollection } else { S::RemovedFromCollection });
        self.toast(text);
        let (b, uid) = (self.backend.clone(), self.account.as_ref().map_or(0, |a| a.uid));
        self.job(async move { b.set_item_like(uid, what, &id, like).await }, move |s, _, res| {
            if let Err(e) = res {
                if like {
                    s.liked_items.remove(&key);
                } else {
                    s.liked_items.insert(key);
                }
                s.error_toast(&e);
            }
        });
    }

    /// Toggles the collection like for the currently open album, playlist, or artist.
    pub fn toggle_detail_like(&mut self) {
        let Some(key) = self.detail.like_key.clone() else { return };
        self.toggle_item_like(&key);
    }

    /// Whether `page` is currently pinned in the navigation rail.
    pub fn is_page_pinned(&self, page: &Page) -> bool {
        self.pinned_items.iter().any(|item| item.page == *page)
    }

    /// Whether the open detail page is pinned in the navigation rail.
    pub fn detail_is_pinned(&self) -> bool {
        self.detail.page.as_ref().is_some_and(|page| self.is_page_pinned(page))
    }

    /// Sorts the open page's tracks while preserving original order to restore later.
    pub fn set_track_sort(&mut self, sort: TrackSort) {
        let d = &mut self.detail;
        d.sort = sort;
        if sort == TrackSort::Original {
            if let Some(base) = d.unsorted.take() {
                d.tracks = base;
            }
            return;
        }
        let base = d.unsorted.get_or_insert_with(|| d.tracks.clone());
        d.tracks = sort.apply(base);
    }

    /// Sorts Liked tracks in Library without mutating the source list.
    pub fn set_library_sort(&mut self, sort: TrackSort) {
        self.library.sort = sort;
    }

    /// Re-applies the active sort after replacing the open page's track list.
    pub(crate) fn detail_tracks_changed(&mut self) {
        self.detail.unsorted = None;
        let sort = self.detail.sort;
        if sort != TrackSort::Original {
            self.set_track_sort(sort);
        }
    }

    /// Inserts a newly added track at the start of the open playlist.
    pub(crate) fn push_detail_track(&mut self, t: Track) {
        if let Some(base) = &mut self.detail.unsorted {
            base.insert(0, t);
            let sort = self.detail.sort;
            self.set_track_sort(sort);
        } else {
            self.detail.tracks.insert(0, t);
        }
    }

    /// Returns the open artist's releases filtered by kind and sorted by the selected order.
    pub fn shown_albums(&self) -> Vec<&Album> {
        let d = &self.detail;
        let mut v: Vec<&Album> = d.albums.iter().filter(|a| d.disc_kind.holds(a.kind.as_deref())).collect();
        match d.disc_sort {
            DiscSort::Newest => {
                v.sort_by(|a, b| (b.year.unwrap_or(0), b.release_date.as_deref().unwrap_or("")).cmp(&(a.year.unwrap_or(0), a.release_date.as_deref().unwrap_or(""))))
            }
            DiscSort::Oldest => {
                v.sort_by(|a, b| (a.year.unwrap_or(i32::MAX), a.release_date.as_deref().unwrap_or("")).cmp(&(b.year.unwrap_or(i32::MAX), b.release_date.as_deref().unwrap_or(""))))
            }
            DiscSort::Title => v.sort_by_cached_key(|a| a.title.to_lowercase()),
        }
        v
    }

    /// Pins `item` in the navigation rail, or removes it if that page is already pinned.
    pub fn toggle_pin_item(&mut self, item: PinnedItem) {
        let key = item.key();
        if let Some(i) = self.pinned_items.iter().position(|it| it.key() == key) {
            self.pinned_items.remove(i);
            let text = self.tr(S::Unpinned);
            self.toast(text);
            self.mark_settings_dirty();
            return;
        }
        self.pinned_items.push(item);
        let text = self.tr(S::Pinned);
        self.toast(text);
        self.mark_settings_dirty();
    }

    pub fn toggle_detail_pin(&mut self) {
        let Some(page) = self.detail.page.clone() else { return };
        if self.is_page_pinned(&page) {
            self.toggle_pin_item(PinnedItem { page, title: String::new(), cover: None, artist: false });
            return;
        }
        if self.detail.title.is_empty() || self.detail.state.is_loading() {
            return;
        }
        self.toggle_pin_item(PinnedItem { page, title: self.detail.title.clone(), cover: self.detail.cover.clone(), artist: self.detail.is_artist });
    }

    /// Removes a pinned item by its stable page key.
    pub fn remove_pinned(&mut self, key: &str) {
        let before = self.pinned_items.len();
        self.pinned_items.retain(|item| item.key() != key);
        if self.pinned_items.len() != before {
            self.close_popover();
            let text = self.tr(S::Unpinned);
            self.toast(text);
            self.mark_settings_dirty();
        }
    }
}
