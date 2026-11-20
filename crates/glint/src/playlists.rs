//! The account's own playlists: making one, naming it again, deleting it, and putting tracks in
//! and taking them out. Each change goes to the service in the background and shows here at once
//! (the library, the open page), with a toast when the service has taken it.

use rune::TextEdit;
use ym_api::{Playlist, Track};

use crate::app::{Load, Page, State};
use crate::i18n::S;

/// What the name dialog is for.
#[derive(Clone, Debug)]
pub enum NamePurpose {
    /// A new playlist, and the track to put in it once it exists.
    Create {
        add: Option<Box<Track>>,
    },
    Rename {
        uid: i64,
        kind: i64,
    },
}

/// A playlist's own page's key: whose it is and which of theirs.
pub type Owned = (i64, i64);

impl State {
    /// The signed-in account's uid (0 when there is none yet).
    pub fn uid(&self) -> i64 {
        self.account.as_ref().map_or(0, |a| a.uid)
    }

    /// Whether the account can edit this playlist: its own, and not one the service makes for it.
    pub fn owns(&self, uid: i64) -> bool {
        uid != 0 && uid == self.uid()
    }

    /// The account's own playlists as far as they are known (fetched again if they are only
    /// last time's), for "Add to playlist".
    pub fn own_playlists(&self) -> Vec<Playlist> {
        let uid = self.uid();
        match &self.library.playlists {
            Load::Ready(v) => v.iter().filter(|p| p.owner_uid() == uid && p.generated_playlist_type.is_none()).cloned().collect(),
            _ => Vec::new(),
        }
    }

    /// Makes sure the own playlists are fresh before a list of them shows.
    pub fn want_playlists(&mut self) {
        if !self.library.playlists_fresh && !self.library.playlists.is_loading() {
            self.refresh_playlists();
        }
    }

    // ---- the name dialog ----------------------------------------------------------------------

    pub fn ask_new_playlist(&mut self, add: Option<Track>) {
        self.close_track_menu();
        self.close_popover();
        self.name_edit = TextEdit::new("");
        self.name_dialog = Some(NamePurpose::Create { add: add.map(Box::new) });
    }

    pub fn ask_rename_playlist(&mut self, uid: i64, kind: i64) {
        self.close_popover();
        self.name_edit = TextEdit::new(&self.detail.title);
        self.name_dialog = Some(NamePurpose::Rename { uid, kind });
    }

    pub fn close_name_dialog(&mut self) {
        self.name_dialog = None;
    }

    /// What the dialog's button does with the name typed.
    pub fn submit_name(&mut self) {
        let name = self.name_edit.text.trim().to_owned();
        if name.is_empty() {
            return;
        }
        match self.name_dialog.take() {
            Some(NamePurpose::Create { add }) => self.create_playlist(name, add.map(|t| *t)),
            Some(NamePurpose::Rename { uid, kind }) => self.rename_playlist(uid, kind, name),
            None => {}
        }
    }

    // ---- changes ------------------------------------------------------------------------------

    pub fn create_playlist(&mut self, title: String, add: Option<Track>) {
        let (b, uid) = (self.backend.clone(), self.uid());
        self.job(async move { b.playlist_create(uid, &title).await }, move |s, _, res| match res {
            Ok(p) => {
                let (pu, pk, title) = (p.owner_uid(), p.kind, p.title.clone());
                if let Load::Ready(v) = &mut s.library.playlists {
                    v.insert(0, p);
                } else {
                    s.library.playlists = Load::Ready(vec![p]);
                }
                s.save_library_snapshot();
                match &add {
                    Some(t) => s.add_to_playlist(pu, pk, &title, t),
                    None => {
                        let text = s.tr(S::PlaylistCreated);
                        s.toast(text);
                    }
                }
            }
            Err(e) => s.error_toast(&e),
        });
    }

    pub fn rename_playlist(&mut self, uid: i64, kind: i64, title: String) {
        let b = self.backend.clone();
        let t2 = title.clone();
        self.job(async move { b.playlist_rename(uid, kind, &t2).await }, move |s, _, res| match res {
            Ok(()) => {
                if let Load::Ready(v) = &mut s.library.playlists
                    && let Some(p) = v.iter_mut().find(|p| p.owner_uid() == uid && p.kind == kind)
                {
                    p.title = title.clone();
                }
                if s.page == Page::Playlist(uid, kind) {
                    s.detail.title = title.clone();
                }
                s.save_library_snapshot();
                let text = s.tr(S::PlaylistRenamed);
                s.toast(text);
            }
            Err(e) => s.error_toast(&e),
        });
    }

    pub fn ask_delete_playlist(&mut self, uid: i64, kind: i64) {
        self.close_popover();
        self.delete_playlist_confirm = Some((uid, kind));
    }

    pub fn delete_playlist(&mut self) {
        let Some((uid, kind)) = self.delete_playlist_confirm.take() else { return };
        let b = self.backend.clone();
        self.job(async move { b.playlist_delete(uid, kind).await }, move |s, _, res| match res {
            Ok(()) => {
                if let Load::Ready(v) = &mut s.library.playlists {
                    v.retain(|p| !(p.owner_uid() == uid && p.kind == kind));
                }
                s.save_library_snapshot();
                if s.page == Page::Playlist(uid, kind) {
                    s.back();
                }
                let text = s.tr(S::PlaylistDeleted);
                s.toast(text);
            }
            Err(e) => s.error_toast(&e),
        });
    }

    pub fn add_to_playlist(&mut self, uid: i64, kind: i64, title: &str, track: &Track) {
        self.close_track_menu();
        let (b, t) = (self.backend.clone(), track.clone());
        let title = title.to_owned();
        self.job(async move { b.playlist_add(uid, kind, &t).await.map(|()| t) }, move |s, _, res| match res {
            Ok(t) => {
                if let Load::Ready(v) = &mut s.library.playlists
                    && let Some(p) = v.iter_mut().find(|p| p.owner_uid() == uid && p.kind == kind)
                {
                    p.track_count = Some(p.track_count.unwrap_or(0) + 1);
                }
                if s.page == Page::Playlist(uid, kind) {
                    s.push_detail_track(t);
                }
                let text = format!("{} «{title}»", s.tr(S::AddedToPlaylist));
                s.toast(text);
            }
            Err(e) => s.error_toast(&e),
        });
    }

    pub fn remove_from_playlist(&mut self, uid: i64, kind: i64, track: &Track) {
        self.close_track_menu();
        let (b, id) = (self.backend.clone(), track.id.clone());
        // Off the page at once; the service follows.
        if self.page == Page::Playlist(uid, kind)
            && let Some(at) = self.detail.tracks.iter().position(|t| t.id == id)
        {
            self.detail.tracks.remove(at);
        }
        self.job(async move { b.playlist_remove(uid, kind, &id).await }, move |s, _, res| match res {
            Ok(()) => {
                if let Load::Ready(v) = &mut s.library.playlists
                    && let Some(p) = v.iter_mut().find(|p| p.owner_uid() == uid && p.kind == kind)
                {
                    p.track_count = p.track_count.map(|n| n.saturating_sub(1));
                }
                let text = s.tr(S::RemovedFromPlaylist);
                s.toast(text);
            }
            Err(e) => {
                s.error_toast(&e);
                if s.page == Page::Playlist(uid, kind) {
                    s.open_playlist(uid, kind);
                }
            }
        });
    }

    /// The open page's playlist, when it's one of the account's own.
    pub fn open_own_playlist(&self) -> Option<Owned> {
        match self.page {
            Page::Playlist(uid, kind) if self.owns(uid) && kind != 3 => Some((uid, kind)),
            _ => None,
        }
    }
}
