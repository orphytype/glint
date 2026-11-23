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
                    s.detail.title = title;
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
        let key = format!("{uid}:{kind}");
        let mut had_local = false;
        if let Some(entry) = self.local_playlists.get_mut(&key) {
            let before = entry.local_tracks.len();
            entry.local_tracks.retain(|t| t.id != id);
            had_local = entry.local_tracks.len() != before;
            entry.order.retain(|oid| *oid != id);
            self.store.save_local_playlists(&self.local_playlists);
        }
        if id.starts_with("local-") && had_local {
            let text = self.tr(S::RemovedFromPlaylist);
            self.toast(text);
            return;
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

    /// Merges locally uploaded tracks and any saved custom ordering into `tracks`.
    pub(crate) fn apply_local_playlist(&mut self, uid: i64, kind: i64, tracks: &mut Vec<Track>) {
        let key = format!("{uid}:{kind}");
        let Some(data) = self.local_playlists.get(&key) else { return };
        for lt in data.local_tracks.iter().rev() {
            if !tracks.iter().any(|t| t.id == lt.id) {
                tracks.insert(0, lt.clone());
            }
            if let Some(ds) = &self.download_store
                && ds.find_audio(&lt.id, "Best", "original").is_some()
            {
                self.downloads.insert(lt.id.clone(), crate::app::DownloadState::Done);
            }
        }
        if !data.order.is_empty() {
            let mut ordered = Vec::with_capacity(tracks.len());
            for id in &data.order {
                if let Some(pos) = tracks.iter().position(|t| &t.id == id) {
                    ordered.push(tracks.remove(pos));
                }
            }
            ordered.append(tracks);
            *tracks = ordered;
        }
    }

    /// Persists the current order and moves the track on the server when the user owns the playlist.
    pub fn persist_playlist_move(&mut self, uid: i64, kind: i64, from: usize, to: usize) {
        if from == to || to >= self.detail.tracks.len() {
            return;
        }
        let key = format!("{uid}:{kind}");
        let entry = self.local_playlists.entry(key).or_default();
        entry.order = self.detail.tracks.iter().map(|t| t.id.clone()).collect();
        self.store.save_local_playlists(&self.local_playlists);

        let moved = self.detail.tracks[to].clone();
        if self.owns(uid) && kind != 3 && !moved.id.starts_with("local-") {
            let b = self.backend.clone();
            self.job(async move { b.playlist_move(uid, kind, from, to, &moved).await }, |s, _, res| {
                if let Err(e) = res {
                    s.error_toast(&e);
                }
            });
        }
    }

    /// Handles drag-to-reorder on a track row grip handle (`queue = false` for playlists, `true` for Queue).
    pub fn drag_row(&mut self, queue: bool, row_idx: usize, d: &mut rune::Drag, tx: &mut rune::Tx) {
        match d.phase {
            rune::Phase::Down => {
                self.row_drag = Some(crate::app::RowDrag { queue, start: row_idx, cur: row_idx, dy: 0.0 });
            }
            rune::Phase::Move | rune::Phase::Up => {
                let Some(mut drag) = self.row_drag.clone() else { return };
                if drag.queue != queue {
                    return;
                }
                drag.dy = d.dy;
                let len = if queue { self.player.queue.len() } else { self.detail.tracks.len() };
                if len > 1 {
                    let row_h = crate::ui::kit::ROW_H;
                    let raw = (drag.start as f32 + (d.dy / row_h).round()) as isize;
                    let target = raw.clamp(0, len as isize - 1) as usize;
                    if target != drag.cur && drag.cur < len {
                        if queue {
                            let item = self.player.queue.remove(drag.cur);
                            self.player.queue.insert(target, item);
                            let remap = |idx: usize| -> usize {
                                if idx == drag.cur {
                                    target
                                } else if drag.cur < idx && idx <= target {
                                    idx - 1
                                } else if target <= idx && idx < drag.cur {
                                    idx + 1
                                } else {
                                    idx
                                }
                            };
                            self.player.index = self.player.index.map(remap);
                            self.player.pending_index = self.player.pending_index.map(remap);
                            self.invalidate_next();
                            self.playback_dirty = true;
                        } else {
                            let item = self.detail.tracks.remove(drag.cur);
                            self.detail.tracks.insert(target, item);
                            self.detail.unsorted = None;
                        }
                        drag.cur = target;
                        tx.animated();
                    }
                }
                if d.phase == rune::Phase::Up {
                    self.row_drag = None;
                    if !queue
                        && drag.start != drag.cur
                        && let Page::Playlist(uid, kind) = self.page
                    {
                        self.persist_playlist_move(uid, kind, drag.start, drag.cur);
                    }
                } else {
                    self.row_drag = Some(drag);
                }
            }
        }
    }

    /// Loads one or more local audio files (`.mp3`, `.flac`, `.m4a`, `.wav`) into playlist `(uid, kind)`
    /// at the beginning of the list.
    pub fn upload_playlist_files(&mut self, uid: i64, kind: i64, paths: Vec<std::path::PathBuf>) {
        if paths.is_empty() {
            return;
        }
        let store = self.download_store.clone();
        let backend = self.backend.clone();
        let own = self.owns(uid) && kind != 3;
        self.job(
            async move {
                let mut loaded = Vec::new();
                for path in paths {
                    let p_read = path.clone();
                    let Ok(Ok(raw)) = tokio::task::spawn_blocking(move || std::fs::read(&p_read)).await else { continue };
                    if raw.is_empty() {
                        continue;
                    }
                    let ext = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(str::to_ascii_lowercase)
                        .filter(|e| matches!(e.as_str(), "mp3" | "flac" | "m4a" | "wav" | "ogg" | "aac"))
                        .unwrap_or_else(|| "mp3".to_owned());
                    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Track").to_owned();
                    let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("track.mp3").to_owned();
                    let (title, artist, album_title) = crate::audio_tags::read_tags(&raw, &stem);
                    let arc_bytes: std::sync::Arc<[u8]> = raw.into();
                    let dur_ms = ym_audio::probe_duration_ms(arc_bytes.clone(), &ext);
                    let mut hash = 0xcbf2_9ce4_8422_2325u64;
                    for b in filename.as_bytes().iter().chain(arc_bytes.iter().take(4096)) {
                        hash = (hash ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3);
                    }
                    hash ^= arc_bytes.len() as u64;
                    let mut track_id = format!("local-{hash:016x}");
                    if own
                        && let Ok(ugc_id) = backend.playlist_upload_ugc(uid, kind, &filename, &arc_bytes).await
                        && !ugc_id.trim().is_empty()
                    {
                        track_id = ugc_id;
                    }
                    let artists = if artist.trim().is_empty() {
                        Vec::new()
                    } else {
                        vec![ym_api::Artist { id: String::new(), name: artist, ..Default::default() }]
                    };
                    let albums = match album_title {
                        Some(al) => vec![ym_api::Album { id: String::new(), title: al, ..Default::default() }],
                        None => Vec::new(),
                    };
                    let track = Track {
                        id: track_id.clone(),
                        title,
                        artists,
                        albums,
                        duration_ms: dur_ms,
                        available: Some(true),
                        ..Default::default()
                    };
                    if let Some(ds) = &store {
                        let _ = ds.put_audio_with_ext(&format!("{track_id}-Best-original"), &arc_bytes, &ext);
                        let _ = ds.put(&crate::store::DownloadedTrack {
                            track: track.clone(),
                            lyrics: ym_api::Lyrics::default(),
                            ext: ext.clone(),
                        });
                    }
                    loaded.push(track);
                }
                loaded
            },
            move |s, _, loaded| {
                if loaded.is_empty() {
                    let msg = s.tr(S::TrackUploadFailed);
                    s.toast(msg);
                    if let Some(t) = s.toasts.last_mut() {
                        t.error = true;
                    }
                    return;
                }
                let key = format!("{uid}:{kind}");
                for t in loaded.into_iter().rev() {
                    s.downloads.insert(t.id.clone(), crate::app::DownloadState::Done);
                    s.download_titles.insert(t.id.clone(), t.full_title());
                    s.downloaded_tracks.insert(t.id.clone(), t.clone());
                    if !s.download_order.contains(&t.id) {
                        s.download_order.insert(0, t.id.clone());
                    }
                    {
                        let entry = s.local_playlists.entry(key.clone()).or_default();
                        entry.local_tracks.retain(|x| x.id != t.id);
                        entry.local_tracks.insert(0, t.clone());
                        if !entry.order.is_empty() {
                            entry.order.retain(|x| *x != t.id);
                            entry.order.insert(0, t.id.clone());
                        }
                    }
                    if s.page == Page::Playlist(uid, kind) {
                        s.detail.tracks.retain(|x| x.id != t.id);
                        s.push_detail_track(t);
                    }
                }
                s.store.save_local_playlists(&s.local_playlists);
                let msg = s.tr(S::TrackUploaded);
                s.toast(msg);
            },
        );
    }
}
