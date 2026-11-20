//! The menus: the track menu (a right click on a row, or its "..." button) and the profile menu.
//!
//! Both are one flat card, small and quiet: no shadow, no blur, a hairline for an edge. A menu does
//! not appear, it comes out of whatever opened it. The card starts as that button's own box (a
//! point, for a right click) and grows into its place on a spring with a touch of overshoot while
//! the entries fade in one after another once there is room for them; closing runs the same road
//! back into the button. All of it is drawn from one number per menu (`State::menu_t`,
//! `State::tmenu`, stepped every frame by `tick_menu`), so a menu pressed again halfway reverses
//! from where it is.

use super::kit::Hover as _;
use std::borrow::Cow;

use rune::{Cursor, Cx, El, Icon, PopupAnchor, Transition, col, el, icon, row};

use super::icons;
use super::kit::{self, MENU_ROW, label, menu_row};
use super::np;
use crate::app::{DownloadState, Page, PinnedItem, Popover, Session, Sheet, Source, State, Station, TrackMenu};
use crate::i18n::S;
use crate::store::QualityPref;
use crate::theme as th;

/// The card's width; its height follows from the entries.
const W: f32 = 248.0;
/// The card's corner radius, and the padding inside it. A row's radius is the card's less this.
const RADIUS: f32 = 12.0;
const PAD: f32 = 4.0;
/// Room kept between a card and the window's edge, and between a card and its button.
const EDGE: f32 = 12.0;
const GAP: f32 = 6.0;
/// A divider's height (its line and the room around it), and the profile menu's head.
const DIVIDER: f32 = 9.0;
const HEAD: f32 = 44.0;

enum Entry {
    Row(El),
    Divider,
    Head(El),
    /// Anything else, of a known height.
    Block(El, f32),
}

impl Entry {
    fn height(&self) -> f32 {
        match self {
            Entry::Row(_) => MENU_ROW,
            Entry::Divider => DIVIDER,
            Entry::Head(_) => HEAD,
            Entry::Block(_, h) => *h,
        }
    }
}

/// How tall the card is for these entries.
fn height(entries: &[Entry]) -> f32 {
    2.0 * PAD + entries.iter().map(Entry::height).sum::<f32>()
}

fn divider() -> El {
    el().w_full().h(1.0).my((DIVIDER - 1.0) * 0.5).fill(th::hairline())
}

/// Where a menu starts and where it ends up, each as `[x, y, w, h]`, and the corner radius it
/// starts with (the button's own).
struct Flow {
    from: [f32; 4],
    to: [f32; 4],
    from_radius: f32,
}

/// Where a card `card_h` tall goes in a window `w` by `h`, and where it comes from.
fn place(cx: &Cx<State>, anchor: &PopupAnchor, card_h: f32, w: f32, h: f32) -> Flow {
    place_w(cx, anchor, W, card_h, w, h)
}

/// [`place`] for a card `card_w` wide.
fn place_w(cx: &Cx<State>, anchor: &PopupAnchor, card_w: f32, card_h: f32, w: f32, h: f32) -> Flow {
    let clamp_x = |x: f32| x.clamp(EDGE, (w - card_w - EDGE).max(EDGE));
    match anchor {
        // A click: the corner sits at the pointer, and turns around when it would run out of room.
        // The card starts as that point.
        PopupAnchor::Point(x, y) => {
            let left = if x + card_w + EDGE > w { x - card_w } else { *x };
            let top = if y + card_h + EDGE <= h { *y } else { y - card_h };
            let (left, top) = (clamp_x(left), top.max(EDGE));
            // The point is where the card's nearest corner is, so the card grows away from it.
            let (px, py) = (if left >= *x { left } else { left + card_w }, if top >= *y { top } else { top + card_h });
            Flow { from: [px, py, 0.0, 0.0], to: [left, top, card_w, card_h], from_radius: 0.0 }
        }
        // A button: the card comes out of it, with the right edges level, under it or (when there
        // is no room below) above it.
        PopupAnchor::Key(key) => match cx.window_layout(key) {
            // A button on the rail: the card opens to its right, level with the button (its top
            // with the top of one high up, its foot with the foot of one low down).
            Some([bx, by, bw, bh]) if bx + bw <= super::kit::RAIL_W => {
                let top = if by < h * 0.5 { by } else { by + bh - card_h };
                let top = top.clamp(EDGE, (h - card_h - EDGE).max(EDGE));
                Flow { from: [bx, by, bw, bh], to: [bx + bw + 2.0 * GAP + 8.0, top, card_w, card_h], from_radius: bh.min(bw) * 0.5 }
            }
            Some([bx, by, bw, bh]) => {
                let down = by + bh + GAP + card_h + EDGE <= h;
                let top = if down { by + bh + GAP } else { by - GAP - card_h };
                Flow { from: [bx, by, bw, bh], to: [clamp_x(bx + bw - card_w), top.max(EDGE), card_w, card_h], from_radius: bh.min(bw) * 0.5 }
            }
            None => {
                let (x, y) = (clamp_x((w - card_w) * 0.5), ((h - card_h) * 0.5).max(EDGE));
                Flow { from: [x + card_w * 0.5, y + card_h * 0.5, 0.0, 0.0], to: [x, y, card_w, card_h], from_radius: 0.0 }
            }
        },
    }
}

/// A menu at openness `t` (0 shut, 1 open; the spring may pass 1 a little): a sheet behind it that
/// closes it while it is open, and the card. `close` is what a press outside (or a right click
/// there) does.
fn flow_menu(name: &'static str, t: f32, open: bool, flow: &Flow, entries: Vec<Entry>, close: fn(&mut State)) -> El {
    let e = t.max(0.0);
    let lerp = |a: f32, b: f32| a + (b - a) * e;
    let [fx, fy, fw, fh] = flow.from;
    let [tx, ty, tw, th_] = flow.to;
    let (x, y) = (lerp(fx, tx), lerp(fy, ty));
    let (w, h) = (lerp(fw, tw).max(0.0), lerp(fh, th_).max(0.0));
    let radius = lerp(flow.from_radius, RADIUS).min(w.min(h) * 0.5);
    // The surface comes up quickly, so the card does not flash over the button it grows from.
    let skin = (t * 6.0).clamp(0.0, 1.0);

    // The entries keep their place on the screen while the card grows around them, and each fades
    // in a little after the one above it.
    let mut items = col().key(format!("{name}-items")).absolute(tx - x, ty - y).w(tw).h(th_).padding(PAD).transition(Transition::none());
    for (i, entry) in entries.into_iter().enumerate() {
        let a = ((t - 0.16 - 0.04 * i as f32) / 0.40).clamp(0.0, 1.0);
        let a = a * a * (3.0 - 2.0 * a);
        items = items.child(match entry {
            Entry::Row(r) => r.opacity(a),
            Entry::Divider => divider().opacity(a),
            Entry::Head(hd) => hd.opacity(a),
            Entry::Block(b, _) => b.opacity(a),
        });
    }
    let card = el()
        .key(format!("{name}-card"))
        .absolute(x, y)
        .w(w)
        .h(h)
        .radius(radius)
        .clip_rounded(radius)
        .border(1.0, th::veil(0.10 * skin))
        // Frosted: the page under it shows through, blurred, so it reads as floating over it.
        .children(kit::frost(radius, skin))
        .transition(Transition::none())
        .child(items);
    // While open the card is modal: what is under it takes no press, hover or wheel, a press on
    // the card's own padding stays on it, and one beside it (or Escape) closes it.
    let card = if open { card.modal(true, move |s: &mut State| close(s)) } else { card };

    let mut layer = el().key(format!("{name}-layer")).absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).instant_changes();
    layer = layer.child(kit::snapshot().key(format!("{name}-snap")));
    // The sheet only catches presses while the menu is open or opening; a right click on it closes
    // the menu too, so the next one starts from nothing.
    if open {
        layer = layer.child(
            el().key(format!("{name}-scrim")).absolute(0.0, 0.0).w_full().h_full().on_press(move |s: &mut State| close(s)).on_secondary_press(move |s: &mut State, _, _| close(s)),
        );
    }
    layer.child(card)
}

// ---- the track menu ----------------------------------------------------------------------------------

/// A track's page on music.yandex.ru.
pub fn track_link(t: &ym_api::Track) -> String {
    match t.albums.first() {
        Some(a) => format!("https://music.yandex.ru/album/{}/track/{}", a.id, t.id),
        None => format!("https://music.yandex.ru/track/{}", t.id),
    }
}

/// "Artist — Title", as it would be written down.
pub fn track_line(t: &ym_api::Track) -> String {
    let who = t.artists_line();
    if who.is_empty() { t.full_title() } else { format!("{who} — {}", t.full_title()) }
}

/// The menu of a track, made for where it was opened. Four groups, each only with what makes sense
/// there: playing it (left out in the player, which has its own buttons; the playing track pauses
/// or resumes instead of starting again; a queued one is jumped to), keeping it, going to where it
/// comes from (not to the page that is already open), and passing it on (link, title, details).
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

    // Playing it: now, next, last, or its wave. The player has its own transport, so these are
    // left out there (but for its wave in a compact player, which has no wave button).
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
            // With nothing playing, "next" and "last" are the same as playing it.
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
    // Only what is still to come can be dropped from the queue; it goes with the queue's own.
    if !m.player
        && let Some(i) = m.queue_index
        && Some(i) > s.player.index
    {
        out.push(Entry::Row(row_of("tm-remove", &icons::CLOSE, S::RemoveFromQueue.get(lang)).on_press(move |s: &mut State| {
            s.close_track_menu();
            s.remove_from_queue(i);
        })));
    }

    // Keeping it: liked (the player has its own heart beside the title), into one of the
    // account's own playlists (the menu turns into the list of them), or out of the one open.
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

    // The way to where it comes from, unless that page is the one open (and not hidden under the
    // player). With several artists, the way to each of them.
    let here = |p: &Page| s.page == *p && !s.np_open;
    let album = t.albums.first().map(|a| a.id.clone()).filter(|id| !here(&Page::Album(id.clone())));
    let artists: Vec<ym_api::Artist> = t.artists.iter().filter(|a| !a.id.is_empty() && !here(&Page::Artist(a.id.clone()))).cloned().collect();
    if album.is_some() || !artists.is_empty() {
        out.push(Entry::Divider);
    }
    if let Some(id) = album {
        out.push(Entry::Row(row_of("tm-album", &icons::DISC, S::GoToAlbum.get(lang)).on_press(move |s: &mut State| s.go(Page::Album(id.clone())))));
    }
    for (i, a) in artists.into_iter().take(3).enumerate() {
        let id = a.id.clone();
        let text = if i == 0 && t.artists.len() == 1 { S::GoToArtist.get(lang).to_owned() } else { a.name.clone() };
        out.push(Entry::Row(menu_row(format!("tm-artist-{i}"), &icons::USER, &text, false).on_press(move |s: &mut State| s.go(Page::Artist(id.clone())))));
    }

    // On the device, and out of the app: a link to it, its name.
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
    out.push(Entry::Row(row_of("tm-share", &icons::EXTERNAL, S::CopyLink.get(lang)).on_press_tx(move |s: &mut State, tx: &mut rune::Tx| {
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

    // Last, what it is: its details, and in the player the quality it plays in.
    out.push(Entry::Divider);
    if m.player {
        // The list opens out of the same button the menu did: the dots of the open player or the
        // dots of the bar.
        let anchor = match &m.anchor {
            PopupAnchor::Key(key) => key.to_string(),
            _ => "np-more".to_owned(),
        };
        out.push(Entry::Row(row_of("tm-quality", &icons::EQ, S::Quality.get(lang)).on_press(move |s: &mut State| {
            s.open_popover(crate::app::Popover::Quality { anchor: anchor.clone() });
        })));
    }
    let tr = t.clone();
    out.push(Entry::Row(row_of("tm-details", &icons::INFO, S::Details.get(lang)).on_press(move |s: &mut State| s.open_sheet(Sheet::Track(Box::new(tr.clone()))))));
    out
}

/// The track menu turned into the account's own playlists: one to put the track in, a new one
/// for it, or back to the menu.
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
    // A long list stays a menu: the ten most recent, as the service lists them.
    for p in own.into_iter().take(10) {
        let (uid, kind, title) = (p.owner_uid(), p.kind, p.title.clone());
        let tr = t.clone();
        out.push(Entry::Row(menu_row(format!("tm-pl-{uid}-{kind}"), &icons::NOTE, &p.title, false).on_press(move |s: &mut State| s.add_to_playlist(uid, kind, &title, &tr))));
    }
    out
}

pub fn overlay(s: &State, cx: &Cx<State>, w: f32, h: f32) -> El {
    let Some(m) = &s.track_menu else { return el() };
    let list = track_entries(s, m, h);
    let flow = place(cx, &m.anchor, height(&list), w, h);
    flow_menu("tmenu", s.tmenu.t, s.tmenu_open, &flow, list, |s| s.close_track_menu())
}

pub fn download_confirm(s: &State, w: f32, _h: f32) -> El {
    let count = s.download_confirm.as_ref().map_or(0, Vec::len);
    let lang = s.lang;
    kit::dialog(
        "download-confirm",
        w,
        S::DownloadConfirm.get(lang),
        vec![label(lang.download_confirm_note(count), th::BODY, th::text2()).wrap()],
        vec![
            kit::btn_soft("download-cancel", S::Cancel.get(lang), None).on_press(|s: &mut State| s.cancel_download()),
            kit::btn_primary("download-accept", S::DownloadAll.get(lang), Some(&icons::DOWNLOAD)).on_press(|s: &mut State| s.confirm_download()),
        ],
        |s| s.cancel_download(),
    )
}

pub fn delete_download_confirm(s: &State, w: f32, _h: f32) -> El {
    let lang = s.lang;
    let batch = s.delete_download_confirm.as_ref().is_some_and(|tracks| tracks.len() > 1);
    let title = if batch { S::DeleteDownloadsConfirm } else { S::DeleteDownloadConfirm };
    let note = if batch { S::DeleteDownloadsNote } else { S::DeleteDownloadNote };
    kit::dialog(
        "delete-download-confirm",
        w,
        title.get(lang),
        vec![label(note.get(lang), th::BODY, th::text2()).wrap()],
        vec![
            kit::btn_soft("delete-download-cancel", S::Cancel.get(lang), None).on_press(|s: &mut State| s.cancel_delete_download()),
            kit::btn_primary("delete-download-accept", S::DeleteDownload.get(lang), Some(&icons::TRASH)).on_press(|s: &mut State| s.confirm_delete_download()),
        ],
        |s| s.cancel_delete_download(),
    )
}

/// Naming a playlist: a new one, or one of the account's own again.
pub fn name_dialog(s: &State, w: f32) -> El {
    let lang = s.lang;
    let creating = matches!(s.name_dialog, Some(crate::playlists::NamePurpose::Create { .. }));
    let (title, action) = if creating { (S::NewPlaylist, S::Create) } else { (S::RenamePlaylist, S::Save) };
    let p = s.pal();
    let field = rune::text_input(&s.name_edit, th::BODY)
        .key("name-field")
        .single_line()
        .grow(1.0)
        .min_w(0.0)
        .label(S::PlaylistName.get(lang))
        .color(th::text())
        .placeholder(S::PlaylistName.get(lang), th::text3())
        .caret(p.accent, p.accent.alpha(0.30))
        .on_edit(|s: &mut State| &mut s.name_edit)
        .on_submit(|s: &mut State, _tx: &mut rune::Tx| s.submit_name());
    let empty = s.name_edit.text.trim().is_empty();
    kit::dialog(
        "name-dialog",
        w,
        title.get(lang),
        vec![kit::field_surface(row().key("name-field-box").w_full().h(kit::BTN_H).px(14.0).items_center()).child(field)],
        vec![
            kit::btn_soft("name-cancel", S::Cancel.get(lang), None).on_press(|s: &mut State| s.close_name_dialog()),
            kit::btn_primary("name-accept", action.get(lang), None).opacity(if empty { 0.45 } else { 1.0 }).on_press(|s: &mut State| s.submit_name()),
        ],
        |s| s.close_name_dialog(),
    )
}

/// Deleting one of the account's playlists, once more to be sure.
pub fn delete_playlist_confirm(s: &State, w: f32) -> El {
    let lang = s.lang;
    kit::dialog(
        "delete-playlist",
        w,
        S::DeletePlaylist.get(lang),
        vec![label(S::DeletePlaylistNote.get(lang), th::BODY, th::text2()).wrap()],
        vec![
            kit::btn_soft("delete-playlist-cancel", S::Cancel.get(lang), None).on_press(|s: &mut State| s.delete_playlist_confirm = None),
            kit::btn_primary("delete-playlist-accept", S::DeletePlaylist.get(lang), Some(&icons::TRASH)).on_press(|s: &mut State| s.delete_playlist()),
        ],
        |s| s.delete_playlist_confirm = None,
    )
}

// ---- popovers ----------------------------------------------------------------------------------------

/// What can be done with the liked tracks beyond the buttons beside it (play, shuffle, their wave,
/// download): put them in the queue, let some downloads go, share them. No search: its field
/// stands over the list already.
fn liked_entries(s: &State) -> Vec<Entry> {
    use super::kit::ListSrc;
    let lang = s.lang;
    let tracks = s.library.tracks.get().cloned().unwrap_or_default();
    let mut out: Vec<Entry> = Vec::new();
    if !tracks.is_empty() {
        out.push(Entry::Row(menu_row("la-next", &icons::QUEUE_NEXT, S::PlayAllNext.get(lang), false).on_press(|s: &mut State| {
            s.close_popover();
            let list = s.list(ListSrc::Liked);
            s.queue_many(list, true);
        })));
        out.push(Entry::Row(menu_row("la-queue", &icons::QUEUE_ADD, S::AddToQueue.get(lang), false).on_press(|s: &mut State| {
            s.close_popover();
            let list = s.list(ListSrc::Liked);
            s.queue_many(list, false);
        })));
        // The button beside the list downloads them all, or deletes them once all are in; some
        // in and some not, this is the way to let those go.
        let kept = tracks.iter().filter(|t| matches!(s.downloads.get(&t.id), Some(DownloadState::Done))).count();
        if kept > 0 && !s.downloads_settled(&tracks) {
            out.push(Entry::Divider);
            out.push(Entry::Row(menu_row("la-undownload", &icons::TRASH, S::DeleteDownload.get(lang), false).on_press(|s: &mut State| {
                s.close_popover();
                let list: Vec<_> =
                    s.library.tracks.get().cloned().unwrap_or_default().into_iter().filter(|t| matches!(s.downloads.get(&t.id), Some(DownloadState::Done))).collect();
                s.ask_delete_downloads(list);
            })));
        }
    }
    if let Some(link) = s.liked_link() {
        if !out.is_empty() {
            out.push(Entry::Divider);
        }
        out.push(Entry::Row(menu_row("la-share", &icons::EXTERNAL, S::CopyLink.get(lang), false).on_press_tx(move |s: &mut State, tx: &mut rune::Tx| {
            s.close_popover();
            tx.copy(link.clone());
            let text = s.tr(S::LinkCopied);
            s.toast(text);
        })));
    }
    out
}

/// What can be done with the open album, playlist or artist beyond the buttons on its page (play,
/// shuffle, its wave, pin, download): its tracks into the queue, keeping it and the way to its
/// artist, sharing it, a playlist of one's own renamed or deleted, and what it is about.
fn action_entries(s: &State) -> Vec<Entry> {
    let lang = s.lang;
    let d = &s.detail;
    let mut out: Vec<Entry> = Vec::new();
    let group = |out: &mut Vec<Entry>| {
        if !out.is_empty() && !matches!(out.last(), Some(Entry::Divider)) {
            out.push(Entry::Divider);
        }
    };
    if !d.tracks.is_empty() {
        out.push(Entry::Row(menu_row("da-next", &icons::QUEUE_NEXT, S::PlayAllNext.get(lang), false).on_press(|s: &mut State| {
            s.close_popover();
            let list = s.detail.tracks.clone();
            s.queue_many(list, true);
        })));
        out.push(Entry::Row(menu_row("da-queue", &icons::QUEUE_ADD, S::AddToQueue.get(lang), false).on_press(|s: &mut State| {
            s.close_popover();
            let list = s.detail.tracks.clone();
            s.queue_many(list, false);
        })));
    }
    if d.like_key.is_some() || d.artist.is_some() {
        group(&mut out);
    }
    if let Some(key) = &d.like_key {
        let liked = s.is_item_liked(key);
        let (ic, name) = if liked { (&icons::HEART_FILL, S::RemoveFromCollection) } else { (&icons::HEART, S::AddToCollection) };
        out.push(Entry::Row(menu_row("da-like", ic, name.get(lang), false).on_press(|s: &mut State| {
            s.close_popover();
            s.toggle_detail_like();
        })));
    }
    if let Some(id) = d.artist.clone() {
        out.push(Entry::Row(menu_row("da-artist", &icons::USER, S::GoToArtist.get(lang), false).on_press(move |s: &mut State| {
            s.close_popover();
            s.go(Page::Artist(id.clone()));
        })));
    }
    if let Some(link) = d.link.clone() {
        group(&mut out);
        out.push(Entry::Row(menu_row("da-share", &icons::EXTERNAL, S::CopyLink.get(lang), false).on_press_tx(move |s: &mut State, tx: &mut rune::Tx| {
            s.close_popover();
            tx.copy(link.clone());
            let text = s.tr(S::LinkCopied);
            s.toast(text);
        })));
    }
    if let Some((uid, kind)) = s.open_own_playlist() {
        group(&mut out);
        out.push(Entry::Row(menu_row("da-rename", &icons::EDIT, S::Rename.get(lang), false).on_press_tx(move |s: &mut State, tx: &mut rune::Tx| {
            s.ask_rename_playlist(uid, kind);
            tx.focus_when_shown("name-field", false);
        })));
        out.push(Entry::Row(menu_row("da-delete", &icons::TRASH, S::DeletePlaylist.get(lang), false).on_press(move |s: &mut State| s.ask_delete_playlist(uid, kind))));
    }
    let artist_info = d.is_artist && (!d.links.is_empty() || d.donation.is_some());
    if !d.facts.is_empty() || artist_info {
        group(&mut out);
        // The facts (an artist's: listeners, what they are about, their pages) in the details dialog.
        let name = if d.is_artist { S::AboutArtist } else { S::Details };
        out.push(Entry::Row(menu_row("da-about", &icons::INFO, name.get(lang), false).on_press(|s: &mut State| s.open_sheet(Sheet::Page))));
    }
    if let Some(url) = d.donation.clone().filter(|_| d.is_artist) {
        out.push(Entry::Row(menu_row("da-donate", &icons::HEART, S::SupportArtist.get(lang), false).on_press_tx(move |s: &mut State, tx: &mut rune::Tx| {
            s.close_popover();
            let _ = tx.open_url(&url);
        })));
    }
    out
}

/// The compact context menu for a pinned cover in the rail.
fn pinned_entries(s: &State, item: &PinnedItem) -> Vec<Entry> {
    let lang = s.lang;
    let page = item.page.clone();
    let key = item.key();
    vec![
        Entry::Row(menu_row("pinned-open", &icons::CHEVRON_RIGHT, S::Open.get(lang), false).on_press(move |s: &mut State| {
            s.close_popover();
            s.go(page.clone());
        })),
        Entry::Row(menu_row("pinned-remove", &icons::TRASH, S::Unpinned.get(lang), false).on_press(move |s: &mut State| s.remove_pinned(&key))),
    ]
}

/// A small download manager view. Downloads are run in order by the background worker, while this
/// list gives every queued item a stable status instead of reducing a batch to one toast.
const DOWNLOAD_ROW_H: f32 = 44.0;

fn download_entries(s: &State) -> Vec<Entry> {
    if s.downloads.is_empty() {
        return vec![download_head(s), Entry::Block(label(S::Nothing.get(s.lang), th::BODY, th::text2()).px(12.0).py(10.0), 42.0)];
    }
    let mut ids: Vec<String> = s.download_order.iter().filter(|id| s.downloads.contains_key(*id)).cloned().collect();
    for id in s.downloads.keys() {
        if !ids.iter().any(|known| known == id) {
            ids.push(id.clone());
        }
    }
    // Keep the popover inside the window and only build rows that are visible in the wheel viewport.
    const LIST_H: f32 = 220.0;
    let list_h = (ids.len() as f32 * DOWNLOAD_ROW_H).min(LIST_H);
    let row_ids = ids.clone();
    let rows = col().key("download-manager-list").w_full().growth(rune::GrowthSpec::default()).virtual_rows(
        ids.len(),
        DOWNLOAD_ROW_H,
        move |_: &State, i| Cow::Owned(row_ids[i].clone()),
        move |s: &State, i| download_row(s, &ids[i]),
    );
    vec![download_head(s), Entry::Block(col().key("download-manager-scroll").w_full().h(list_h).scroll(rune::ScrollSpec { stick_bottom: false }).child(rows), list_h)]
}

fn download_row(s: &State, id: &str) -> El {
    let state = match s.downloads.get(id).copied() {
        Some(state) => state,
        None => return el().h(DOWNLOAD_ROW_H),
    };
    let title = s.download_titles.get(id).cloned().unwrap_or_else(|| id.to_owned());
    let downloaded = matches!(state, DownloadState::Done).then(|| s.downloaded_tracks.get(id).cloned()).flatten();
    let status = match state {
        DownloadState::Downloading => S::DownloadInProgress,
        DownloadState::Done => S::DownloadDone,
        DownloadState::Failed => S::DownloadFailed,
    };
    // A failed one says why under its title.
    let why = matches!(state, DownloadState::Failed).then(|| s.download_why(id)).flatten();
    let status_text = match why {
        Some(why) => format!("{} · {why}", status.get(s.lang)),
        None => status.get(s.lang).to_owned(),
    };
    let glyph_icon = match state {
        DownloadState::Downloading => &icons::SPINNER,
        DownloadState::Failed if s.download_errors.get(id).is_some_and(|f| f.lasting()) => &icons::ALERT,
        DownloadState::Done | DownloadState::Failed => &icons::DOWNLOAD,
    };
    let color = match state {
        DownloadState::Downloading => th::text2(),
        DownloadState::Done => th::rgb(0x4bd27e),
        DownloadState::Failed => th::rgb(0xf18a8a),
    };
    let glyph =
        if matches!(state, DownloadState::Downloading) { icon(glyph_icon, 12.0).color(color).looping(rune::LoopSpec::rotation(0.9)) } else { icon(glyph_icon, 12.0).color(color) };
    let mut row = row().key(format!("download-manager-{id}")).role("button").label(title.as_str()).w_full().h(DOWNLOAD_ROW_H).px(10.0).gap(10.0).items_center();
    if let Some(track) = downloaded.clone() {
        let open_track = track.clone();
        let destination = track.albums.first().map(|album| Page::Album(album.id.clone())).or_else(|| track.artists.first().map(|artist| Page::Artist(artist.id.clone())));
        row = row
            .cursor(Cursor::Pointer)
            .hov(th::veil(0.05))
            .on_press(move |s: &mut State| {
                s.close_popover();
                if let Some(page) = destination.clone() {
                    s.go(page);
                } else {
                    s.open_sheet(Sheet::Track(Box::new(open_track.clone())));
                }
            })
            .child(kit::cover(s, track.cover_template(), 32.0, 8.0));
    }
    row = row.child(
        col()
            .grow(1.0)
            .min_w(0.0)
            .gap(1.0)
            .child(label(title, th::BODY_M, th::text()).truncate())
            .child(rune::row().gap(4.0).items_center().child(glyph).child(label(status_text, th::SMALL, th::text3()).truncate().min_w(0.0)).min_w(0.0)),
    );
    if matches!(state, DownloadState::Done)
        && let Some(track) = downloaded
    {
        row = row.child(
            kit::ibtn(format!("download-delete-{id}"), &icons::TRASH, 17.0, 30.0, th::text3(), S::DeleteDownload.get(s.lang))
                .on_press(move |s: &mut State| s.ask_delete_download(track.clone())),
        );
    }
    row
}

fn download_head(s: &State) -> Entry {
    let folder = kit::ibtn("download-folder", &icons::FOLDER, 17.0, 30.0, th::text2(), S::OpenDownloads.get(s.lang)).on_press(|s: &mut State| s.open_download_folder());
    let clear = kit::ibtn("download-clear", &icons::TRASH, 17.0, 30.0, th::text2(), S::ClearDownloadHistory.get(s.lang)).on_press(|s: &mut State| s.clear_download_history());
    Entry::Head(
        row().h(HEAD).w_full().px(10.0).items_center().child(label(S::DownloadManager.get(s.lang), th::SECTION, th::text())).child(el().grow(1.0)).child(folder).child(clear),
    )
}

/// A wave card's list: each wave a plain row, what it is by and the thing's name on the right
/// (the one playing lit).
fn wave_entries(s: &State, choices: &[crate::app::WaveChoice]) -> Vec<Entry> {
    choices
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let station = c.station.clone();
            let wname = c.name.clone();
            let on = matches!(&s.player.source, crate::app::Source::Wave { preset, .. } if station.is(preset)) && s.current().is_some();
            let ic = match station {
                crate::app::Station::Track(_) => &icons::NOTE,
                crate::app::Station::Artist(_) => &icons::USER,
                crate::app::Station::Album(_) => &icons::DISC,
                crate::app::Station::Collection => &icons::HEART,
            };
            Entry::Row(
                row()
                    .key(format!("pw-{i}"))
                    .role("button")
                    .label(c.kind.as_str())
                    .checked(on)
                    .w_full()
                    .h(kit::MENU_ROW)
                    .px(10.0)
                    .gap(10.0)
                    .items_center()
                    .radius(8.0)
                    .cursor(Cursor::Pointer)
                    .transition(Transition::none())
                    .fill(if on { th::veil(0.08) } else { rune::Color::TRANSPARENT })
                    .hov(th::veil(0.06))
                    .press(th::veil(0.10))
                    .on_press(move |s: &mut State| {
                        s.close_popover();
                        s.start_station(&station, &wname);
                    })
                    .child(rune::icon(ic, 18.0).color(if on { th::text() } else { th::text2() }))
                    .child(label(c.kind.clone(), th::BODY, th::text()))
                    .child(el().grow(1.0))
                    .child(label(c.name.clone(), th::SMALL, th::text2()).truncate().max_w(150.0)),
            )
        })
        .collect()
}

/// One of the look's settings: which it is, and its three choices.
pub fn stage_rows() -> [(S, [S; 3]); 2] {
    [(S::StageFlash, [S::StageOff, S::StageSoft, S::StageBright]), (S::StageReact, [S::StageOff, S::StageCalm, S::StageLively])]
}

/// The equaliser's presets, the one in use ticked.
fn eq_entries(s: &State) -> Vec<Entry> {
    let current = s.player.eq.preset_name();
    ym_audio::PRESETS
        .iter()
        .enumerate()
        .map(|(n, (name, gains))| {
            let on = current == Some(*name);
            let text = super::settings::preset_label(name, s.lang);
            let gains = *gains;
            Entry::Row(
                row()
                    .key(format!("eq-preset-{name}"))
                    .role("radio")
                    .label(text.as_str())
                    .checked(on)
                    .w_full()
                    .h(kit::MENU_ROW)
                    .pl(10.0)
                    .pr(8.0)
                    .gap(10.0)
                    .items_center()
                    .radius(8.0)
                    .cursor(Cursor::Pointer)
                    .hov(th::veil(0.06))
                    .press(th::veil(0.10))
                    .on_press(move |s: &mut State| {
                        s.player.eq.gains_db = gains;
                        s.eq_preset = n;
                        s.apply_eq();
                        s.close_popover();
                    })
                    .child(label(text, th::BODY, if on { th::text() } else { th::text2() }).grow(1.0).min_w(0.0).truncate())
                    .children(on.then(|| rune::icon(&icons::CHECK, 15.0).color(th::text()))),
            )
        })
        .collect()
}

/// One choice of a list of them: its name, a tick when it is the one picked.
fn choice_row(key: String, text: &str, on: bool, pick: impl Fn(&mut State) + 'static) -> Entry {
    Entry::Row(
        row()
            .key(key)
            .role("radio")
            .label(text)
            .checked(on)
            .w_full()
            .h(kit::MENU_ROW)
            .pl(10.0)
            .pr(8.0)
            .gap(10.0)
            .items_center()
            .radius(8.0)
            .cursor(Cursor::Pointer)
            .hov(th::veil(0.06))
            .press(th::veil(0.10))
            .on_press(move |s: &mut State| {
                pick(s);
                s.close_popover();
            })
            .child(label(text.to_owned(), th::BODY, if on { th::text() } else { th::text2() }).grow(1.0).min_w(0.0).truncate())
            .children(on.then(|| rune::icon(&icons::CHECK, 15.0).color(th::text()))),
    )
}

/// A quiet name over a group of choices.
fn choice_caption(key: &str, text: &str) -> Entry {
    Entry::Block(row().key(key.to_owned()).w_full().h(28.0).px(10.0).items_end().pb(4.0).child(kit::caps(text, th::text3())), 28.0)
}

/// The orders the open page's tracks can be listed in.
fn track_sort_entries(s: &State) -> Vec<Entry> {
    let now = s.detail.sort;
    crate::app::TrackSort::ALL
        .into_iter()
        // A release's tracks are all from it: no point in ordering them by album.
        .filter(|o| *o != crate::app::TrackSort::Album || !matches!(s.detail.page, Some(Page::Album(_))))
        .map(|o| choice_row(format!("sort-{o:?}"), o.name().get(s.lang), o == now, move |s: &mut State| s.set_track_sort(o)))
        .collect()
}

/// The orders an artist's releases can be in, and which kinds of them are shown.
fn disc_sort_entries(s: &State) -> Vec<Entry> {
    let lang = s.lang;
    let (now, kind) = (s.detail.disc_sort, s.detail.disc_kind);
    let mut v = vec![choice_caption("disc-sort-cap", S::SortBy.get(lang))];
    v.extend(crate::app::DiscSort::ALL.into_iter().map(|o| choice_row(format!("disc-{o:?}"), o.name().get(lang), o == now, move |s: &mut State| s.detail.disc_sort = o)));
    v.push(Entry::Divider);
    v.push(choice_caption("disc-kind-cap", S::ShowKinds.get(lang)));
    // Only the kinds the artist has (and all of them together).
    let albums = &s.detail.albums;
    v.extend(
        crate::app::DiscKind::ALL
            .into_iter()
            .filter(|k| *k == crate::app::DiscKind::All || *k == kind || albums.iter().any(|a| k.holds(a.kind.as_deref())))
            .map(|k| choice_row(format!("disc-kind-{k:?}"), k.name().get(lang), k == kind, move |s: &mut State| s.detail.disc_kind = k)),
    );
    v
}

fn quality_entries(s: &State) -> Vec<Entry> {
    [(QualityPref::Saver, S::QualitySaver), (QualityPref::Balanced, S::QualityBalanced), (QualityPref::Best, S::QualityBest), (QualityPref::Lossless, S::QualityLossless)]
        .into_iter()
        .map(|(quality, text)| {
            let selected = s.settings.quality == quality;
            Entry::Row(
                row()
                    .key(format!("quality-{quality:?}"))
                    .role("radio")
                    .label(text.get(s.lang))
                    .checked(selected)
                    .w_full()
                    .h(kit::MENU_ROW)
                    .px(10.0)
                    .items_center()
                    .radius(8.0)
                    .cursor(Cursor::Pointer)
                    .fill(if selected { th::veil(0.08) } else { rune::Color::TRANSPARENT })
                    .hov(th::veil(0.06))
                    .press(th::veil(0.10))
                    .on_press(move |s: &mut State| {
                        s.settings.quality = quality;
                        s.mark_settings_dirty();
                        s.close_popover();
                    })
                    .child(label(text.get(s.lang), th::BODY, if selected { th::text() } else { th::text2() })),
            )
        })
        .collect()
}

/// The lyrics' two settings, after the two player-background settings.
pub fn lyric_rows() -> [(S, [S; 3]); 2] {
    [(S::LyricSize, [S::SizeSmall, S::SizeMedium, S::SizeLarge]), (S::LyricAlign, [S::AlignLeft, S::AlignCenter, S::AlignRight])]
}

/// The look's setting `i` (as [`stage_rows`] orders them, then [`lyric_rows`] from 2).
pub fn stage_value(s: &State, i: usize) -> u8 {
    let (look, ly) = (&s.settings.stage, &s.settings.lyric_look);
    [look.flash, look.react, ly.size, ly.align][i]
}

pub fn set_stage_value(s: &mut State, i: usize, v: u8) {
    let (l, ly) = (&mut s.settings.stage, &mut s.settings.lyric_look);
    match i {
        0 => l.flash = v,
        1 => l.react = v,
        2 => ly.size = v,
        _ => ly.align = v,
    }
    s.mark_settings_dirty();
}

/// The look panel: its width (My Wave's panel keeps it; the player's look is as wide as the
/// cover, never under `STAGE_MIN`, and from `STAGE_INLINE` its choices stand beside their names),
/// a setting's line, the width of a setting's choices, and the small heading over each group with
/// the room under the group.
const STAGE_W: f32 = 440.0;
const STAGE_MIN: f32 = 260.0;
const STAGE_INLINE: f32 = 400.0;
const STAGE_ROW: f32 = 46.0;
const PICK_W: f32 = 228.0;
const PICK_H: f32 = 32.0;
const PICK_PAD: f32 = 3.0;
/// The narrowest a choice gets before the picker breaks into two lines.
const PICK_SEG_MIN: f32 = 58.0;
const STAGE_CAPTION: f32 = 30.0;
const STAGE_AFTER: f32 = 8.0;
/// The head of the panel, and how far its card keeps from the window's top and foot.
const STAGE_HEAD: f32 = 56.0;
const STAGE_MARGIN: f32 = 2.0 * EDGE + 2.0 * PAD + 8.0;
/// How the lit choice slides to the one pressed.
const PICK_SPRING: rune::Spring = rune::Spring::new(0.34, 0.12);
/// The faces the lyrics can be set in, as Settings names them.
const LYRIC_FACES: [&str; 3] = ["Plex Mono", "Commissioner", "Cormorant"];

/// What a choice shows: its name, or a small picture of what it does (the name is then its tooltip).
#[derive(Clone, Copy)]
enum Face {
    Word,
    /// The letter at one of three sizes.
    Size,
    /// Three lines standing left, in the middle or right.
    Align,
    /// The letters in each face the lyrics can be set in.
    Font,
    /// The name, in Cormorant (My Wave's knobs).
    Serif,
}

/// Choice `j` of a picker drawn as `f`.
fn face(f: Face, j: usize, name: &str, on: bool) -> El {
    let ink = if on { th::text() } else { th::text2() };
    match f {
        Face::Word => label(name.to_owned(), th::SMALL, ink).truncate(),
        Face::Serif => label(name.to_owned(), th::wave_words(16.0), ink).truncate(),
        Face::Size => {
            let px = [11.0, 14.0, 17.5][j.min(2)];
            label("Aa".to_owned(), rune::TextStyle::new(px, 600.0).family(th::SANS).lh(20.0), ink)
        }
        Face::Font => {
            let mut st = th::lyric_face(th::LYRIC_CARD, j as u8);
            st.size *= 0.66;
            st.line_height = 22.0;
            label("Aa".to_owned(), st, ink)
        }
        Face::Align => {
            let bars = col().w(16.0).gap(2.5);
            let bars = match j {
                0 => bars.items_start(),
                1 => bars.items_center(),
                _ => bars.items_end(),
            };
            bars.children([16.0, 9.0, 12.5].map(|w| el().w(w).h(2.0).radius(1.0).fill(ink)))
        }
    }
}

/// How many lines `n` choices take in a picker `width` wide: one, or two when one would crowd them.
fn pick_lines(width: f32, n: usize) -> usize {
    if (width - 2.0 * PICK_PAD) / n.max(1) as f32 >= PICK_SEG_MIN { 1 } else { 2 }
}

/// How tall a picker of `n` choices `width` wide is.
fn pick_h(width: f32, n: usize) -> f32 {
    pick_lines(width, n) as f32 * (PICK_H - 2.0 * PICK_PAD) + 2.0 * PICK_PAD
}

/// Choices in one soft track `width` wide (in two lines when one would crowd them); the lit one
/// sits on a pill that slides to whichever is pressed, which calls `set(state, i, choice)`.
#[allow(clippy::too_many_arguments)]
fn picker(key: &str, width: f32, names: &[String], current: usize, look: Face, i: usize, set: fn(&mut State, usize, usize)) -> El {
    let n = names.len().max(1);
    let per = n.div_ceil(pick_lines(width, n));
    let seg = (width - 2.0 * PICK_PAD) / per as f32;
    let cell = PICK_H - 2.0 * PICK_PAD;
    let height = pick_h(width, n);
    let at = current.min(n - 1);
    let thumb_fill = if th::light() { th::rgb(0xffffff) } else { th::veil(0.15) };
    let mut thumb = el()
        .key(format!("{key}-thumb"))
        .absolute(PICK_PAD + (at % per) as f32 * seg, PICK_PAD + (at / per) as f32 * cell)
        .w(seg)
        .h(cell)
        .radius(7.5)
        .fill(thumb_fill)
        .motion(PICK_SPRING);
    if th::light() {
        thumb = thumb.shadow(1.0, 4.0, th::rgba(0x000000, 0.12));
    }
    let mut choices = col().absolute(PICK_PAD, PICK_PAD).w(width - 2.0 * PICK_PAD).h(height - 2.0 * PICK_PAD);
    let mut line = row().w_full().h(cell);
    for (j, name) in names.iter().enumerate() {
        let on = at == j;
        let mut c = row()
            .key(format!("{key}-{j}"))
            .role("radio")
            .label(name.as_str())
            .checked(on)
            .w(seg)
            .h_full()
            .px(4.0)
            .radius(7.5)
            .center()
            .cursor(Cursor::Pointer)
            .hov(th::veil(if on { 0.0 } else { 0.05 }))
            .on_press(move |s: &mut State| set(s, i, j))
            .child(face(look, j, name, on));
        if !matches!(look, Face::Word | Face::Serif) {
            c = c.help(name.as_str());
        }
        line = line.child(c);
        if (j + 1) % per == 0 || j + 1 == names.len() {
            choices = choices.child(line);
            line = row().w_full().h(cell);
        }
    }
    el().key(key.to_owned()).w(width).h(height).radius(10.0).fill(th::veil(0.06)).child(thumb).child(choices)
}

/// A setting's line: its name on the left, its control on the right.
fn look_row(name: &str, control: El) -> (El, f32) {
    (
        row().w_full().h(STAGE_ROW).pl(12.0).pr(7.0).gap(12.0).items_center().child(label(name.to_owned(), th::BODY, th::text()).truncate().grow(1.0).min_w(0.0)).child(control),
        STAGE_ROW,
    )
}

/// A setting whose control needs the whole width: its name, and the control (`ctl_h` tall) under it.
fn look_row_below(name: &str, control: El, ctl_h: f32) -> (El, f32) {
    let h = STAGE_ROW - 10.0 + ctl_h + 10.0;
    (col().w_full().h(h).pl(12.0).pr(7.0).child(row().w_full().h(STAGE_ROW - 10.0).items_center().child(label(name.to_owned(), th::BODY, th::text()).truncate())).child(control), h)
}

/// The width a control under its name gets in a panel `pw` wide.
fn full_pick(pw: f32) -> f32 {
    pw - 2.0 * PAD - 8.0 - 12.0 - 7.0
}

/// The width a control under its name gets in My Wave's panel.
const WIDE_PICK: f32 = STAGE_W - 2.0 * PAD - 8.0 - 12.0 - 7.0;

/// A setting of the player's look in a panel `pw` wide: its choices beside its name in a wide
/// panel, under it in a narrow one.
#[allow(clippy::too_many_arguments)]
fn look_line(pw: f32, name: &str, key: &str, names: &[String], current: usize, look: Face, i: usize, set: fn(&mut State, usize, usize)) -> (El, f32) {
    if pw >= STAGE_INLINE {
        return look_row(name, picker(key, PICK_W, names, current, look, i, set));
    }
    let full = full_pick(pw);
    look_row_below(name, picker(key, full, names, current, look, i, set), pick_h(full, names.len()))
}

/// One group of the panel: its small heading, then its settings on one plate, a hairline between
/// each two. Returns the group and its height.
fn look_group(key: &str, caption: El, rows: Vec<(El, f32)>) -> (El, f32) {
    let n = rows.len();
    let mut plate = col().key(format!("{key}-plate")).w_full().radius(12.0).fill(th::veil(0.045)).border(1.0, th::veil(0.06));
    let mut plate_h = 0.0;
    for (k, (r, rh)) in rows.into_iter().enumerate() {
        if k > 0 {
            plate = plate.child(row().w_full().h(1.0).pl(12.0).child(el().grow(1.0).h(1.0).fill(th::veil(0.06))));
        }
        plate = plate.child(r);
        plate_h += rh;
    }
    plate_h += n.saturating_sub(1) as f32;
    let h = STAGE_CAPTION + plate_h + STAGE_AFTER;
    let g = col().key(key.to_owned()).w_full().h(h).px(4.0).child(row().h(STAGE_CAPTION).pl(8.0).pb(7.0).items_end().child(caption)).child(plate);
    (g, h)
}

fn set_look(s: &mut State, i: usize, v: usize) {
    set_stage_value(s, i, v as u8);
}

fn set_face(s: &mut State, _: usize, v: usize) {
    s.settings.lyric_look.font = v as u8;
    s.mark_settings_dirty();
}

fn set_effect(s: &mut State, _: usize, v: usize) {
    s.settings.backdrop.effect = v as u8;
    s.mark_settings_dirty();
}

/// The picture behind the player: the burst and how it moves, and the user's own picture with how
/// soft and dark it is and the effect over it, all seen at once behind the panel.
fn look_back(s: &State, pw: f32) -> (El, f32) {
    let lang = s.lang;
    let mut rows = Vec::new();
    for (i, (name, opts)) in stage_rows().into_iter().enumerate() {
        let names: Vec<String> = opts.iter().map(|o| o.get(lang).to_owned()).collect();
        rows.push(look_line(pw, name.get(lang), &format!("sl-{i}"), &names, usize::from(stage_value(s, i)), Face::Word, i, set_look));
    }
    let bd = s.settings.backdrop;
    let pick = |key: &str, name: &str| kit::btn_small(key.to_owned(), name, None).on_press_tx(|s: &mut State, tx: &mut rune::Tx| s.pick_backdrop(tx));
    let picture = if bd.on {
        row()
            .gap(6.0)
            .items_center()
            .child(pick("sl-bd-change", S::BdChange.get(lang)))
            .child(kit::btn_small("sl-bd-remove", S::BdRemove.get(lang), None).on_press(|s: &mut State| s.drop_backdrop()))
    } else {
        row().child(pick("sl-bd-pick", S::BdPick.get(lang)))
    };
    // In a narrow panel the buttons go under the name, and the levels are shorter.
    let wide = pw >= STAGE_INLINE;
    rows.push(if wide { look_row(S::BdPicture.get(lang), picture) } else { look_row_below(S::BdPicture.get(lang), picture, 32.0) });
    if bd.on {
        let lw = if wide { PICK_W - 8.0 } else { (pw - 141.0).max(80.0) };
        rows.push(look_row(
            S::BdBlur.get(lang),
            super::settings::level_w("sl-bd-blur", bd.blur, lw, |s, f| {
                s.settings.backdrop.blur = f;
                s.mark_settings_dirty();
            })
            .mr(4.0),
        ));
        rows.push(look_row(
            S::BdDim.get(lang),
            super::settings::level_w("sl-bd-dim", bd.dim, lw, |s, f| {
                s.settings.backdrop.dim = f;
                s.mark_settings_dirty();
            })
            .mr(4.0),
        ));
        let names: Vec<String> = crate::backdrop::EFFECTS.iter().map(|e| e.get(lang).to_owned()).collect();
        let full = full_pick(pw);
        rows.push(look_row_below(S::BdEffect.get(lang), picker("sl-bd-effect", full, &names, usize::from(bd.effect), Face::Word, 0, set_effect), pick_h(full, names.len())));
    }
    look_group("sl-back", kit::caps(S::StageBack.get(s.lang), th::text3()), rows)
}

/// The lyrics: the face they are set in, then how large, how they move and where they stand.
fn look_text(s: &State, pw: f32) -> (El, f32) {
    let lang = s.lang;
    let faces: Vec<String> = LYRIC_FACES.iter().map(|f| (*f).to_owned()).collect();
    let mut rows = vec![look_line(pw, S::LyricFace.get(lang), "sl-face", &faces, usize::from(s.settings.lyric_look.font), Face::Font, 0, set_face)];
    for (k, (name, opts)) in lyric_rows().into_iter().enumerate() {
        let i = 2 + k;
        let names: Vec<String> = opts.iter().map(|o| o.get(lang).to_owned()).collect();
        let look = match i {
            2 => Face::Size,
            3 => Face::Align,
            _ => Face::Word,
        };
        rows.push(look_line(pw, name.get(lang), &format!("sl-{i}"), &names, usize::from(stage_value(s, i)), look, i, set_look));
    }
    look_group("sl-text", kit::caps(S::LyricText.get(s.lang), th::text3()), rows)
}

/// Whether anything of the player's look is off its default: the stage, the lyrics (their font
/// too) or the picture behind the player. `rev` is not a look, so it is left out.
pub fn look_changed(s: &State) -> bool {
    let bd_rest = crate::store::BackdropLook { rev: s.settings.backdrop.rev, ..Default::default() };
    s.settings.stage != crate::store::StageLook::default() || s.settings.lyric_look != crate::store::LyricLook::default() || s.settings.backdrop != bd_rest
}

/// Puts all of the player's look back to its defaults. The user's own picture is dropped.
pub fn reset_look(s: &mut State) {
    s.settings.stage = crate::store::StageLook::default();
    s.settings.lyric_look = crate::store::LyricLook::default();
    if s.settings.backdrop.on {
        s.drop_backdrop();
    }
    s.settings.backdrop = crate::store::BackdropLook { rev: s.settings.backdrop.rev, ..Default::default() };
    s.mark_settings_dirty();
}

/// How wide the player's look panel is in a `w` x `h` window: as wide as the cover.
fn stage_w(s: &State, w: f32, h: f32) -> f32 {
    let (_, cover) = np::cover_span(s, w, h);
    cover.max(STAGE_MIN).min(w - 2.0 * EDGE)
}

/// The look's popover, `pw` wide: a head with what it is and the way back to the defaults, then
/// the lyrics and the picture behind the player, each a group of its settings. In a low window the
/// groups scroll under the head.
fn stage_entries(s: &State, pw: f32, win_h: f32) -> Vec<Entry> {
    let lang = s.lang;
    let changed = look_changed(s);
    let mut reset = row().key("sl-reset").role("button").label(S::StageReset.get(lang)).h(28.0).px(12.0).radius(14.0).center().transition(Transition::none());
    reset = if changed {
        reset.cursor(Cursor::Pointer).fill(th::veil(0.07)).hov(th::veil(0.11)).press(th::veil(0.15)).press_scale(th::PRESS).on_press(reset_look)
    } else {
        reset.disabled(true)
    };
    let reset = reset.child(label(S::StageReset.get(lang).to_owned(), th::SMALL, if changed { th::text() } else { th::text3().alpha(0.6) }));
    let head = row()
        .w_full()
        .h(STAGE_HEAD)
        .pl(8.0)
        .pr(6.0)
        .gap(12.0)
        .items_center()
        .child(row().w(34.0).h(34.0).radius(10.0).center().fill(th::veil(0.07)).child(rune::icon(&icons::STAGE, 18.0).color(th::text())))
        .child(label(S::StageLook.get(lang).to_owned(), th::BODY_M, th::text()).truncate().grow(1.0).min_w(0.0))
        .child(reset);
    // The words first: they are what the panel is opened for most, and they change in view.
    let (text, text_h) = look_text(s, pw);
    let (back, back_h) = look_back(s, pw);
    let body_h = text_h + back_h;
    let room = (win_h - STAGE_MARGIN - STAGE_HEAD).max(160.0);
    let body = col().w_full().child(text).child(back);
    if body_h <= room {
        return vec![Entry::Block(head, STAGE_HEAD), Entry::Block(body.h(body_h), body_h)];
    }
    let scroll = col().key("sl-scroll").w_full().h(room).scroll(rune::ScrollSpec { stick_bottom: false }).scroll_edge_fade(12.0).child(body);
    vec![Entry::Block(head, STAGE_HEAD), Entry::Block(scroll, room)]
}

// ---- My Wave's knobs and look ---------------------------------------------------------------------

/// A knob of My Wave: its name in Cormorant, and its choices under it across the panel.
fn tune_row(name: &str, control: El) -> (El, f32) {
    let h = STAGE_ROW - 10.0 + PICK_H + 10.0;
    (col().w_full().h(h).pl(12.0).pr(7.0).child(row().w_full().h(STAGE_ROW - 10.0).items_center().child(label(name.to_owned(), th::wave_words(18.0), th::text()).truncate())).child(control), h)
}

/// Knob `i` of the wave chosen on Home (0 character, 1 mood, 2 language) or of the look (3 form,
/// 4 print) set to choice `v`. A knob of the wave playing starts it again with it.
fn set_wave_knob(s: &mut State, i: usize, v: usize) {
    let v = v as u8;
    match i {
        3 => s.settings.wave_look.form = v,
        4 => s.settings.wave_look.print = v,
        _ => {
            let id = s.wave_choice.clone();
            let mut t = s.wave_tune(&id);
            match i {
                0 => t.diversity = v,
                1 => t.mood = v,
                _ => t.language = v,
            }
            s.set_wave_tune(&id, t);
            return;
        }
    }
    s.mark_settings_dirty();
}

/// The popover from My Wave's tune button: a head with the wave it tunes and the way back to how
/// that wave is, then the wave's three knobs (what it leans to, its mood, its language: each wave
/// keeps its own, so "Favorite" can be calm and stays so), then the look of the shape on Home.
/// Every word here is in Cormorant, as My Wave's name is.
fn wave_tune_entries(s: &State, win_h: f32) -> Vec<Entry> {
    use crate::store::{WaveLook, WaveTune};
    let lang = s.lang;
    let id = s.wave_choice.clone();
    let tune = s.wave_tune(&id);
    let tunable = WaveTune::applies(&id);
    let changed = (tunable && tune != WaveTune::of(&id)) || s.settings.wave_look != WaveLook::default();
    let mut reset = row().key("wt-reset").role("button").label(S::StageReset.get(lang)).h(28.0).px(12.0).radius(14.0).center().transition(Transition::none());
    reset = if changed {
        reset.cursor(Cursor::Pointer).fill(th::veil(0.07)).hov(th::veil(0.11)).press(th::veil(0.15)).press_scale(th::PRESS).on_press(|s: &mut State| {
            let id = s.wave_choice.clone();
            s.settings.wave_look = WaveLook::default();
            s.mark_settings_dirty();
            s.set_wave_tune(&id, WaveTune::of(&id));
        })
    } else {
        reset.disabled(true)
    };
    let reset = reset.child(label(S::StageReset.get(lang).to_owned(), th::SMALL, if changed { th::text() } else { th::text3().alpha(0.6) }));
    let head = row()
        .w_full()
        .h(STAGE_HEAD)
        .pl(8.0)
        .pr(6.0)
        .gap(12.0)
        .items_center()
        .child(row().w(34.0).h(34.0).radius(10.0).center().fill(th::veil(0.07)).child(rune::icon(&icons::TUNE, 18.0).color(th::text())))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .child(label(S::WaveTitle.get(lang).to_owned(), th::wave_words(22.0), th::text()))
                .child(label(format!("{} · {}", S::WaveTuneOf.get(lang), super::wave_title(&id).get(lang)), th::wave_words(15.0), th::text3()).truncate()),
        )
        .child(reset);
    let caption = |t: S| label(t.get(lang).to_owned(), th::wave_words(16.0), th::text3());
    let names = |list: &[S]| list.iter().map(|n| n.get(lang).to_owned()).collect::<Vec<String>>();
    let (knobs, knobs_h) = if tunable {
        let rows = vec![
            tune_row(
                S::WaveCharacter.get(lang),
                picker("wt-div", WIDE_PICK, &names(&[S::WaveAnyOne, S::WaveFavorite, S::WaveDiscover, S::WavePopular]), usize::from(tune.diversity), Face::Serif, 0, set_wave_knob),
            ),
            tune_row(
                S::Mood.get(lang),
                picker("wt-mood", WIDE_PICK, &names(&[S::WaveAnyMood, S::WaveEnergetic, S::WaveFun, S::WaveCalm, S::WaveSad]), usize::from(tune.mood), Face::Serif, 1, set_wave_knob),
            ),
            tune_row(
                S::WaveLanguage.get(lang),
                picker("wt-lang", WIDE_PICK, &names(&[S::WaveAnyOne, S::WaveRussian, S::WaveForeign, S::WaveNoWords]), usize::from(tune.language), Face::Serif, 2, set_wave_knob),
            ),
        ];
        look_group("wt-knobs", caption(S::WaveTitle), rows)
    } else {
        let note = (row().w_full().h(STAGE_ROW).pl(12.0).items_center().child(label(S::WaveTuneNone.get(lang).to_owned(), th::wave_words(16.0), th::text2()).truncate()), STAGE_ROW);
        look_group("wt-knobs", caption(S::WaveTitle), vec![note])
    };
    let look = s.settings.wave_look;
    let (looks, looks_h) = look_group(
        "wt-look",
        caption(S::WaveLook),
        vec![
            tune_row(
                S::WaveForm.get(lang),
                picker("wt-form", WIDE_PICK, &names(&[S::FormLiquid, S::FormFlower, S::FormPinwheel, S::FormCloud]), usize::from(look.form), Face::Serif, 3, set_wave_knob),
            ),
            tune_row(S::WavePrint.get(lang), picker("wt-print", WIDE_PICK, &names(&[S::PrintSmooth, S::PrintDither, S::PrintDots, S::PrintLines]), usize::from(look.print), Face::Serif, 4, set_wave_knob)),
        ],
    );
    let body_h = knobs_h + looks_h;
    let room = (win_h - STAGE_MARGIN - STAGE_HEAD).max(160.0);
    let body = col().w_full().child(knobs).child(looks);
    if body_h <= room {
        return vec![Entry::Block(head, STAGE_HEAD), Entry::Block(body.h(body_h), body_h)];
    }
    let scroll = col().key("wt-scroll").w_full().h(room).scroll(rune::ScrollSpec { stick_bottom: false }).scroll_edge_fade(12.0).child(body);
    vec![Entry::Block(head, STAGE_HEAD), Entry::Block(scroll, room)]
}

/// The popover that is open (or closing): a card that grows out of its button like the menus.
pub fn popover(s: &State, cx: &Cx<State>, w: f32, h: f32) -> El {
    let Some(p) = &s.popover else { return el() };
    let (list, width) = match p {
        Popover::Actions { .. } => (action_entries(s), 248.0),
        Popover::Waves { choices, .. } => (wave_entries(s, choices), 300.0),
        Popover::Quality { .. } => (quality_entries(s), 220.0),
        Popover::Stage { .. } => {
            let pw = stage_w(s, w, h);
            (stage_entries(s, pw, h), pw)
        }
        Popover::Pinned { item, .. } => (pinned_entries(s, item), 208.0),
        Popover::Downloads { .. } => (download_entries(s), 300.0),
        Popover::Liked { .. } => (liked_entries(s), 260.0),
        Popover::EqPreset { .. } => (eq_entries(s), 200.0),
        Popover::TrackSort { .. } => (track_sort_entries(s), 220.0),
        Popover::DiscSort { .. } => (disc_sort_entries(s), 220.0),
        Popover::WaveTune { .. } => (wave_tune_entries(s, h), STAGE_W),
    };
    let mut flow = place_w(cx, &PopupAnchor::Key(p.anchor().into()), width, height(&list), w, h);
    // The player's look stands over the cover, its edges level with the cover's.
    if matches!(p, Popover::Stage { .. }) {
        let (cover_x, cover) = np::cover_span(s, w, h);
        flow.to[0] = (cover_x + (cover - width) * 0.5).clamp(EDGE, (w - width - EDGE).max(EDGE));
    }
    flow_menu("popover", s.pop.t, s.popover_open, &flow, list, |s| s.close_popover())
}

// ---- pieces of the details card (`details.rs`) ------------------------------------------------------------------------------

/// What an artist's link is called on its button: its own title, the network's name, or the site.
pub(super) fn link_title(l: &ym_api::ArtistLink) -> String {
    if !l.title.trim().is_empty() {
        return l.title.trim().to_owned();
    }
    if let Some(net) = l.social_network.as_deref().filter(|n| !n.is_empty()) {
        let mut c = net.chars();
        return c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default();
    }
    l.href.trim_start_matches("https://").trim_start_matches("http://").trim_start_matches("www.").split('/').next().unwrap_or("").to_owned()
}

// ---- the volume card ---------------------------------------------------------------------------------

const VOL_W: f32 = 64.0;
const VOL_H: f32 = 184.0;
/// The notches of the level, bottom to top; the top of the card holds the number.
const VOL_NOTCHES: usize = 15;
const VOL_TOP: f32 = 34.0;
const VOL_BOTTOM: f32 = 12.0;

/// The volume card: it grows out of the speaker button like the menus do, and its level is a stack of
/// notches that grow wider toward the top. They rise into place one after another from the bottom
/// while it opens, then fill up to the level; drag anywhere on them to set it. The notch at the
/// level is lit by how far into it the level is, so the stack moves smoothly rather than in steps.
pub fn volume(s: &State, cx: &Cx<State>, w: f32, h: f32) -> El {
    let t = s.vol.t;
    let to_y_floor = EDGE;
    let (from, from_radius) = match cx.window_layout("vol-btn") {
        Some(r) => (r, r[2].min(r[3]) * 0.5),
        None => ([w * 0.5, h * 0.5, 0.0, 0.0], 0.0),
    };
    let cx_mid = from[0] + from[2] * 0.5;
    let to = [(cx_mid - VOL_W * 0.5).clamp(EDGE, (w - VOL_W - EDGE).max(EDGE)), (from[1] - GAP - VOL_H).max(to_y_floor), VOL_W, VOL_H];
    let e = t.max(0.0);
    let lerp = |a: f32, b: f32| a + (b - a) * e;
    let (x, y) = (lerp(from[0], to[0]), lerp(from[1], to[1]));
    let (cw, ch) = (lerp(from[2], to[2]).max(0.0), lerp(from[3], to[3]).max(0.0));
    let radius = lerp(from_radius, 20.0).min(cw.min(ch) * 0.5);
    let skin = (t * 6.0).clamp(0.0, 1.0);

    let v = s.player.volume.clamp(0.0, 1.0);
    let field_h = VOL_H - VOL_TOP - VOL_BOTTOM;
    let pitch = field_h / VOL_NOTCHES as f32;
    let mut field = el()
        .key("vol-field")
        .role("slider")
        .label(S::Volume.get(s.lang))
        .absolute(to[0] - x, to[1] - y + VOL_TOP)
        .w(VOL_W)
        .h(field_h)
        .cursor(rune::Cursor::Pointer)
        .transition(Transition::none())
        .on_drag(|s: &mut State, d: &mut rune::Drag, _tx: &mut rune::Tx| {
            let f = if d.h > 0.0 { 1.0 - d.y / d.h } else { 0.0 };
            // The ends are easy to hit: a little past the first or last notch is nothing or full.
            let f = ((f - 0.015) / 0.97).clamp(0.0, 1.0);
            s.set_volume(f);
        });
    for i in 0..VOL_NOTCHES {
        let lit = (v * VOL_NOTCHES as f32 - i as f32).clamp(0.0, 1.0);
        // From the bottom: each notch rises into place a little after the one below it.
        let a = ((t - 0.06 - 0.022 * i as f32) / 0.30).clamp(0.0, 1.0);
        let a = a * a * (3.0 - 2.0 * a);
        let width = 14.0 + 20.0 * i as f32 / (VOL_NOTCHES - 1) as f32;
        let nx = (VOL_W - width * (0.5 + 0.5 * a)) * 0.5;
        let ny = field_h - (i as f32 + 0.5) * pitch - 2.0 + (1.0 - a) * 10.0;
        field = field.child(el().absolute(nx, ny).w(width * (0.5 + 0.5 * a)).h(4.0).radius(2.0).fill(th::veil((0.16 + 0.84 * lit) * a)).transition(Transition::none()));
    }
    let pct = (v * 100.0).round() as i32;
    let number = row()
        .absolute(to[0] - x, to[1] - y + 12.0)
        .w(VOL_W)
        .h(18.0)
        .center()
        .transition(Transition::none())
        .child(label(format!("{pct}"), th::MONO, th::text2()).opacity(((t - 0.15) / 0.3).clamp(0.0, 1.0)));
    // The frame as it is now (the bar under the card included), for the card's frost.
    let card = el()
        .key("vol-card")
        .absolute(x, y)
        .w(cw)
        .h(ch)
        .radius(radius)
        .clip_rounded(radius)
        .border(1.0, th::veil(0.10 * skin))
        // Frosted: the page under it shows through, blurred, so it reads as floating over it.
        .children(kit::frost(radius, skin))
        .transition(Transition::none())
        // A surface of its own: a press between the notches stops here, not on what is under it.
        .on_press(|_s: &mut State| {})
        .on_hover_change(|s: &mut State, over: bool, _tx: &mut rune::Tx| {
            s.vol_card_hover = over;
            if over {
                s.vol_open = true;
                s.vol_close_at = None;
            } else {
                s.vol_close_at = Some(s.now + 0.28);
            }
        })
        .child(number)
        .child(field);
    el().key("vol-layer").absolute(0.0, 0.0).w(0.0).h(0.0).transition(Transition::none()).child(kit::snapshot().key("vol-snap").w(w).h(h)).child(card)
}

// ---- the profile menu --------------------------------------------------------------------------------

/// The menu of the profile button: who is signed in, My Wave, Settings and the way out.
pub fn profile(s: &State, cx: &Cx<State>, w: f32, h: f32) -> El {
    let lang = s.lang;
    let name = super::dock::account_name(s);
    let sub = s.account.as_ref().map(|a| a.login.clone()).unwrap_or_default();
    let plus = s.account.as_ref().is_some_and(|a| a.has_plus) && s.session == Session::Live;
    let initial = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    let head = row().w_full().h(HEAD).px(10.0).gap(10.0).items_center().child(super::dock::account_picture(s, 28.0, &initial)).child(
        col()
            .grow(1.0)
            .min_w(0.0)
            .child(row().gap(6.0).items_center().min_w(0.0).child(label(name, th::BODY_M, th::text()).truncate().min_w(0.0)).children(plus.then(|| kit::plus_badge(lang))))
            .child(label(sub, th::SMALL, th::text3()).truncate().min_w(0.0)),
    );
    let entry = |key: &'static str, ic: &'static Icon, text: &str, on: bool| menu_row(key, ic, text, on);
    let list = vec![
        Entry::Head(head),
        Entry::Divider,
        Entry::Row(entry("menu-settings", &icons::SETTINGS, S::Settings.get(lang), s.settings_open).on_press(|s: &mut State| s.open_settings())),
        Entry::Divider,
        Entry::Row(entry("menu-out", &icons::SIGN_OUT, S::SignOut.get(lang), false).on_press(|s: &mut State| s.sign_out())),
    ];
    let flow = place(cx, &PopupAnchor::Key("profile".into()), height(&list), w, h);
    flow_menu("menu", s.menu_t, s.menu_open, &flow, list, |s| s.menu_open = false)
}
