//! Track context menu (row right-click / "..." button and player menu) and playlist picker.

use rune::{Cx, El, Icon, PopupAnchor, el, row};

use super::{Entry, flow_menu, height, place};
use crate::app::{DownloadState, Page, Popover, Sheet, Source, State, Station, TrackMenu};
use crate::i18n::S;
use crate::theme as th;
use crate::ui::{
    icons,
    kit::{MENU_ROW, label, menu_row},
    np, np_icons,
};

/// Yandex Music URL for a track.
pub fn track_link(t: &ym_api::Track) -> String {
    match t.albums.first() {
        Some(a) => format!("https://music.yandex.ru/album/{}/track/{}", a.id, t.id),
        None => format!("https://music.yandex.ru/track/{}", t.id),
    }
}

/// "Artist — Title" string for clipboard copy.
pub fn track_line(t: &ym_api::Track) -> String {
    let who = t.artists_line();
    if who.is_empty() { t.full_title() } else { format!("{who} — {}", t.full_title()) }
}

fn track_entries(s: &State, m: &TrackMenu, h: f32) -> Vec<Entry> {
    if m.choosing {
        return playlist_choices(s, &m.track);
    }
    let lang = s.lang;
    let t = &m.track;
    let mut out: Vec<Entry> = Vec::new();
    let row_of = |key: &'static str, ic: &'static Icon, text: &str| menu_row(key, ic, text, false);
    let is_current = match m.queue_index {
        Some(i) => s.player.index == Some(i),
        None => s.current().is_some_and(|c| c.id == t.id),
    };

    if !m.player {
        if is_current {
            let (ic, name) = if s.player.playing { (&icons::PAUSE, S::Pause) } else { (&icons::PLAY, S::Resume) };
            out.push(Entry::Row(row_of("tm-pause", ic, name.get(lang)).on_press(|s: &mut State| {
                s.close_track_menu();
                s.toggle_pause();
            })));
        } else if let Some(i) = m.queue_index {
            out.push(Entry::Row(row_of("tm-play", &icons::PLAY, S::Play.get(lang)).on_press(move |s: &mut State| {
                s.close_track_menu();
                s.play_index(i);
            })));
        } else {
            let tr = t.clone();
            out.push(Entry::Row(row_of("tm-play", &icons::PLAY, S::Play.get(lang)).on_press(move |s: &mut State| {
                s.close_track_menu();
                s.play_queue(vec![tr.clone()], 0, Source::Manual);
            })));
            if s.current().is_some() {
                let tr = t.clone();
                out.push(Entry::Row(row_of("tm-next", &icons::QUEUE_NEXT, S::PlayNext.get(lang)).on_press(move |s: &mut State| {
                    s.close_track_menu();
                    s.queue_next(tr.clone());
                })));
                let tr = t.clone();
                out.push(Entry::Row(row_of("tm-queue", &icons::QUEUE_ADD, S::AddToQueue.get(lang)).on_press(move |s: &mut State| {
                    s.close_track_menu();
                    s.queue_last(tr.clone());
                })));
            }
        }
    }
    if !m.player || np::compact_player(s, h) {
        let station = Station::Track(t.id.clone());
        let name = t.full_title();
        out.push(Entry::Row(row_of("tm-wave", &icons::SPARKLE, S::WaveByTrack.get(lang)).on_press(move |s: &mut State| {
            s.close_track_menu();
            s.start_station(&station, &name);
        })));
    }
    if !m.player
        && let Some(i) = m.queue_index
        && Some(i) > s.player.index
    {
        out.push(Entry::Row(row_of("tm-remove", &icons::CLOSE, S::RemoveFromQueue.get(lang)).on_press(move |s: &mut State| {
            s.close_track_menu();
            s.remove_from_queue(i);
        })));
    }

    if !out.is_empty() {
        out.push(Entry::Divider);
    }
    if !m.player {
        let liked = s.is_liked(&t.id);
        let tr = t.clone();
        out.push(Entry::Row(row_of("tm-like", if liked { &icons::HEART_FILL } else { &icons::HEART }, if liked { S::Unlike.get(lang) } else { S::Like.get(lang) }).on_press(
            move |s: &mut State| {
                s.close_track_menu();
                s.toggle_like(&tr);
            },
        )));
    } else {
        let bad = t.clone();
        out.push(Entry::Row(row_of("tm-dislike", &icons::DISLIKE, S::Dislike.get(lang)).on_press(move |s: &mut State| {
            s.close_track_menu();
            s.dislike(&bad);
        })));
    }
    out.push(Entry::Row(row_of("tm-add-pl", &icons::PLAYLIST_ADD, S::AddToPlaylist.get(lang)).on_press(|s: &mut State| {
        s.want_playlists();
        if let Some(m) = &mut s.track_menu {
            m.choosing = true;
        }
    })));
    if !m.player
        && let Some((uid, kind)) = s.open_own_playlist()
        && s.detail.tracks.iter().any(|x| x.id == t.id)
    {
        let tr = t.clone();
        out.push(Entry::Row(row_of("tm-rm-pl", &icons::TRASH, S::RemoveFromPlaylist.get(lang)).on_press(move |s: &mut State| s.remove_from_playlist(uid, kind, &tr))));
    }

    let here = |p: &Page| s.page == *p && !s.np_open;
    let album = t.albums.first().map(|a| a.id.clone()).filter(|id| !here(&Page::Album(id.clone())));
    let artists: Vec<&ym_api::Artist> = t.artists.iter().filter(|a| !a.id.is_empty() && !here(&Page::Artist(a.id.clone()))).take(3).collect();
    if album.is_some() || !artists.is_empty() {
        out.push(Entry::Divider);
    }
    if let Some(id) = album {
        out.push(Entry::Row(row_of("tm-album", &icons::DISC, S::GoToAlbum.get(lang)).on_press(move |s: &mut State| s.go(Page::Album(id.clone())))));
    }
    for (i, a) in artists.into_iter().enumerate() {
        let id = a.id.clone();
        let text = if i == 0 && t.artists.len() == 1 { S::GoToArtist.get(lang) } else { a.name.as_str() };
        out.push(Entry::Row(menu_row(format!("tm-artist-{i}"), &icons::USER, text, false).on_press(move |s: &mut State| s.go(Page::Artist(id.clone())))));
    }

    out.push(Entry::Divider);
    if matches!(s.downloads.get(&t.id), Some(DownloadState::Done)) {
        let download = t.clone();
        out.push(Entry::Row(row_of("tm-delete-download", &icons::TRASH, S::DeleteDownload.get(lang)).on_press(move |s: &mut State| {
            s.close_track_menu();
            s.ask_delete_download(download.clone());
        })));
    } else {
        let download = t.clone();
        out.push(Entry::Row(row_of("tm-download", &icons::DOWNLOAD, S::Download.get(lang)).on_press(move |s: &mut State| {
            s.close_track_menu();
            s.ask_download(vec![download.clone()]);
        })));
    }
    let link = track_link(t);
    out.push(Entry::Row(row_of("tm-share", &icons::COPY, S::CopyLink.get(lang)).on_press_tx(move |s: &mut State, tx: &mut rune::Tx| {
        s.close_track_menu();
        tx.copy(link.clone());
        let text = s.tr(S::LinkCopied);
        s.toast(text);
    })));
    let line = track_line(t);
    out.push(Entry::Row(row_of("tm-copy", &icons::COPY, S::CopyTitle.get(lang)).on_press_tx(move |s: &mut State, tx: &mut rune::Tx| {
        s.close_track_menu();
        tx.copy(line.clone());
        let text = s.tr(S::TitleCopied);
        s.toast(text);
    })));

    out.push(Entry::Divider);
    if m.player && s.np_open {
        let anchor = match &m.anchor {
            PopupAnchor::Key(key) => key.clone(),
            _ => "np-more".to_owned(),
        };
        let look_anchor = anchor.clone();
        out.push(Entry::Row(row_of("tm-quality", np_icons::quality_icon(s.settings.quality), S::Quality.get(lang)).on_press(move |s: &mut State| {
            s.open_popover(Popover::Quality { anchor: anchor.clone() });
        })));
        out.push(Entry::Row(row_of("tm-look", &np_icons::LOOK, S::StageLook.get(lang)).on_press(move |s: &mut State| {
            s.open_popover(Popover::Stage { anchor: look_anchor.clone() });
        })));
    }
    if m.player {
        out.push(Entry::Row(row_of("tm-mini", &np_icons::MINI, S::MiniPlayer.get(lang)).on_press(|s: &mut State| {
            s.close_track_menu();
            s.toggle_mini_player();
        })));
    }
    let tr = t.clone();
    out.push(Entry::Row(row_of("tm-details", &icons::INFO, S::Details.get(lang)).on_press(move |s: &mut State| s.open_sheet(Sheet::Track(Box::new(tr.clone()))))));
    out
}

fn playlist_choices(s: &State, t: &ym_api::Track) -> Vec<Entry> {
    let lang = s.lang;
    let mut out = vec![Entry::Row(menu_row("tm-pl-back", &icons::CHEVRON_LEFT, S::AddToPlaylist.get(lang), false).on_press(|s: &mut State| {
        if let Some(m) = &mut s.track_menu {
            m.choosing = false;
        }
    }))];
    out.push(Entry::Divider);
    let tr = t.clone();
    out.push(Entry::Row(menu_row("tm-pl-new", &icons::PLUS, S::NewPlaylist.get(lang), false).on_press_tx(move |s: &mut State, tx: &mut rune::Tx| {
        s.ask_new_playlist(Some(tr.clone()));
        tx.focus_when_shown("name-field", false);
    })));
    let own = s.own_playlists();
    if own.is_empty() {
        out.push(Entry::Row(row().key("tm-pl-none").w_full().h(MENU_ROW).px(10.0).items_center().child(label(S::NoOwnPlaylists.get(lang), th::SMALL, th::text3()))));
    }
    for p in own.into_iter().take(10) {
        let (uid, kind, title) = (p.owner_uid(), p.kind, p.title.clone());
        let tr = t.clone();
        out.push(Entry::Row(menu_row(format!("tm-pl-{uid}-{kind}"), &icons::NOTE, &p.title, false).on_press(move |s: &mut State| s.add_to_playlist(uid, kind, &title, &tr))));
    }
    out
}

/// Renders the open or closing track context menu.
pub fn overlay(s: &State, cx: &Cx<State>, w: f32, h: f32) -> El {
    let Some(m) = &s.track_menu else { return el() };
    let list = track_entries(s, m, h);
    let flow = place(cx, &m.anchor, height(&list), w, h);
    flow_menu("tmenu", s.tmenu.t, s.tmenu_open, &flow, list, State::close_track_menu)
}
