//! The menus: the track menu (a right click on a row, or its "..." button) and the profile menu.
//!
//! Both are one flat card, small and quiet: no shadow, no blur, a hairline for an edge. A menu does
//! not appear, it comes out of whatever opened it. The card starts as that button's own box (a
//! point, for a right click) and grows into its place on a spring with a touch of overshoot while
//! the entries fade in one after another once there is room for them; closing runs the same road
//! back into the button. All of it is drawn from one number per menu (`State::menu_t`,
//! `State::tmenu`, stepped every frame by `tick_menu`), so a menu pressed again halfway reverses
//! from where it is.

use rune::{Cursor, Cx, El, Icon, PopupAnchor, Transition, col, el, icon, row};

use super::icons;
use super::kit::{self, MENU_ROW, label, menu_row};
use super::np;
use crate::app::{DownloadState, Page, PinnedItem, Popover, Session, Sheet, Source, State, Station, TrackMenu};
use crate::i18n::S;
use crate::fx;
use crate::store::QualityPref;
use crate::theme as th;

/// The card's width; its height follows from the entries.
const W: f32 = 208.0;
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
        .fill(th::pop().alpha(skin))
        .border(1.0, th::veil(0.10 * skin))
        .transition(Transition::none())
        .child(items);
    // While open the card is modal: what is under it takes no press, hover or wheel, a press on
    // the card's own padding stays on it, and one beside it (or Escape) closes it.
    let card = if open { card.modal(true, move |s: &mut State| close(s)) } else { card };

    let mut layer = el().key(format!("{name}-layer")).absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).instant_changes();
    // The sheet only catches presses while the menu is open or opening; a right click on it closes
    // the menu too, so the next one starts from nothing.
    if open {
        layer = layer.child(
            el().key(format!("{name}-scrim"))
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .on_press(move |s: &mut State| close(s))
                .on_secondary_press(move |s: &mut State, _, _| close(s)),
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
            // Only what is still to come can be dropped from the queue.
            if Some(i) > s.player.index {
                out.push(Entry::Row(row_of("tm-remove", &icons::TRASH, S::RemoveFromQueue.get(lang)).on_press(move |s: &mut State| {
                    s.close_track_menu();
                    s.remove_from_queue(i);
                })));
            }
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
        out.push(Entry::Divider);
    }

    // The player has its own heart beside the title.
    if !m.player {
        let liked = s.is_liked(&t.id);
        let tr = t.clone();
        out.push(Entry::Row(row_of("tm-like", if liked { &icons::HEART_FILL } else { &icons::HEART }, if liked { S::Unlike.get(lang) } else { S::Like.get(lang) }).on_press(
            move |s: &mut State| {
                s.close_track_menu();
                s.toggle_like(&tr);
            },
        )));
    } else if np::compact_player(s, h) {
        let station = Station::Track(t.id.clone());
        let name = t.full_title();
        out.push(Entry::Row(row_of("tm-wave", &icons::NOTE, S::WaveByTrack.get(lang)).on_press(move |s: &mut State| {
            s.close_track_menu();
            s.start_station(&station, &name);
        })));
    }
    if m.player {
        if !out.is_empty() {
            out.push(Entry::Divider);
        }
        out.push(Entry::Row(row_of("tm-quality", &icons::EQ, S::Quality.get(lang)).on_press(move |s: &mut State| {
            s.open_popover(crate::app::Popover::Quality { anchor: "np-more".into() });
        })));
    }

    // The way to where it comes from, unless that page is the one open (and not hidden under the
    // player).
    let here = |p: &Page| s.page == *p && !s.np_open;
    let album = t.albums.first().map(|a| a.id.clone()).filter(|id| !here(&Page::Album(id.clone())));
    let artist = t.artists.first().map(|a| a.id.clone()).filter(|id| !here(&Page::Artist(id.clone())));
    if (album.is_some() || artist.is_some()) && !out.is_empty() {
        out.push(Entry::Divider);
    }
    if let Some(id) = album {
        out.push(Entry::Row(row_of("tm-album", &icons::DISC, S::GoToAlbum.get(lang)).on_press(move |s: &mut State| s.go(Page::Album(id.clone())))));
    }
    if let Some(id) = artist {
        out.push(Entry::Row(row_of("tm-artist", &icons::USER, S::GoToArtist.get(lang)).on_press(move |s: &mut State| s.go(Page::Artist(id.clone())))));
    }

    if !out.is_empty() {
        out.push(Entry::Divider);
    }
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
    out.push(Entry::Row(row_of("tm-share", &icons::SHARE, S::Share.get(lang)).on_press_tx(move |s: &mut State, tx: &mut rune::Tx| {
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
    let tr = t.clone();
    out.push(Entry::Row(row_of("tm-details", &icons::INFO, S::Details.get(lang)).on_press(move |s: &mut State| s.open_sheet(Sheet::Track(tr.clone())))));
    out
}

pub fn overlay(s: &State, cx: &Cx<State>, w: f32, h: f32) -> El {
    let Some(m) = &s.track_menu else { return el() };
    let list = track_entries(s, m, h);
    let flow = place(cx, &m.anchor, height(&list), w, h);
    flow_menu("tmenu", s.tmenu.t, s.tmenu_open, &flow, list, |s| s.close_track_menu())
}

pub fn download_confirm(s: &State, w: f32, h: f32) -> El {
    let count = s.download_confirm.as_ref().map_or(0, Vec::len);
    let lang = s.lang;
    el()
        .key("download-confirm")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .fill(th::rgba(0x000000, 0.48))
        .on_press(|s: &mut State| s.cancel_download())
        .child(
            col()
                .key("download-confirm-card")
                .absolute((w - 380.0) * 0.5, (h - 190.0) * 0.5)
                .w(380.0_f32.min(w - 32.0))
                .padding(24.0)
                .gap(14.0)
                .radius(22.0)
                .fill(th::bg())
                .border(1.0, th::hairline())
                .on_press(|_s: &mut State| {})
                .child(label(S::DownloadConfirm.get(lang), th::H2, th::text()))
                .child(label(lang.download_confirm_note(count), th::BODY, th::text2()).wrap())
                .child(
                    row()
                        .w_full()
                        .gap(10.0)
                        .justify_end()
                        .child(kit::button("download-cancel", kit::Weight::Soft, S::Close.get(lang), None).on_press(|s: &mut State| s.cancel_download()))
                        .child(kit::btn_primary("download-accept", S::DownloadAll.get(lang), Some(&icons::DOWNLOAD)).on_press(|s: &mut State| s.confirm_download())),
                ),
        )
}

pub fn delete_download_confirm(s: &State, w: f32, h: f32) -> El {
    let lang = s.lang;
    el()
        .key("delete-download-confirm")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .fill(th::rgba(0x000000, 0.48))
        .on_press(|s: &mut State| s.cancel_delete_download())
        .child(
            col()
                .key("delete-download-confirm-card")
                .absolute((w - 380.0) * 0.5, (h - 190.0) * 0.5)
                .w(380.0_f32.min(w - 32.0))
                .padding(24.0)
                .gap(14.0)
                .radius(22.0)
                .fill(th::bg())
                .border(1.0, th::hairline())
                .on_press(|_s: &mut State| {})
                .child(label(S::DeleteDownloadConfirm.get(lang), th::H2, th::text()))
                .child(label(S::DeleteDownloadNote.get(lang), th::BODY, th::text2()).wrap())
                .child(
                    row()
                        .w_full()
                        .gap(10.0)
                        .justify_end()
                        .child(kit::button("delete-download-cancel", kit::Weight::Soft, S::Close.get(lang), None).on_press(|s: &mut State| s.cancel_delete_download()))
                        .child(kit::btn_primary("delete-download-accept", S::DeleteDownload.get(lang), Some(&icons::TRASH)).on_press(|s: &mut State| s.confirm_delete_download())),
                ),
        )
}

// ---- popovers ----------------------------------------------------------------------------------------

/// The artists of a track, each with their picture: pressing one opens their page.
fn artist_entries(s: &State, artists: &[ym_api::Artist]) -> Vec<Entry> {
    artists
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let id = a.id.clone();
            Entry::Block(
                row()
                    .key(format!("pa-{i}"))
                    .role("link")
                    .label(a.name.as_str())
                    .w_full()
                    .h(ARTIST_ROW)
                    .px(8.0)
                    .gap(12.0)
                    .items_center()
                    .radius(8.0)
                    .cursor(Cursor::Pointer)
                    .transition(Transition::none())
                    .hover(th::veil(0.06))
                    .press(th::veil(0.10))
                    .hover_group()
                    .on_press(move |s: &mut State| {
                        s.close_popover();
                        s.go(Page::Artist(id.clone()));
                    })
                    .child(kit::avatar(s, a, 32.0))
                    .child(label(a.name.clone(), th::BODY, th::text2()).group_color(th::text()).truncate().min_w(0.0)),
                ARTIST_ROW,
            )
        })
        .collect()
}

const ARTIST_ROW: f32 = 44.0;
const ABOUT_W: f32 = 300.0;

/// What the open page is about: each fact's name small above it.
fn about_entries(s: &State) -> Vec<Entry> {
    let lang = s.lang;
    let inner = ABOUT_W - 2.0 * PAD - 24.0;
    s.detail
        .facts
        .iter()
        .map(|(name, value)| {
            // A rough count of the lines the value wraps to (the card's height has to be known).
            let chars = value.chars().count() as f32;
            let lines = (chars * 7.4 / inner).ceil().clamp(1.0, 7.0);
            let h = 12.0 + 16.0 + 4.0 + lines * 20.0 + 6.0;
            Entry::Block(
                col()
                    .w_full()
                    .h(h)
                    .px(12.0)
                    .pt(12.0)
                    .gap(4.0)
                    .transition(Transition::none())
                    .child(kit::caps(name.get(lang), th::text3()))
                    .child(label(value.clone(), th::BODY, th::text()).w_full().wrap().max_lines(7)),
                h,
            )
        })
        .collect()
}

/// What can be done with the open album, playlist or artist: its tracks into the queue, the way
/// to its artist, sharing it, opening it on the web, and what it is about.
fn action_entries(s: &State) -> Vec<Entry> {
    let lang = s.lang;
    let d = &s.detail;
    let mut out: Vec<Entry> = Vec::new();
    if !d.tracks.is_empty() {
        out.push(Entry::Row(menu_row("da-play", &icons::PLAY, S::Play.get(lang), false).on_press(|s: &mut State| {
            s.close_popover();
            let list = s.detail.tracks.clone();
            s.play_all(list, false);
        })));
        out.push(Entry::Row(menu_row("da-shuffle", &icons::SHUFFLE, S::Shuffle.get(lang), false).on_press(|s: &mut State| {
            s.close_popover();
            let list = s.detail.tracks.clone();
            s.play_all(list, true);
        })));
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
        if !out.is_empty() {
            out.push(Entry::Divider);
        }
        let copy = link.clone();
        out.push(Entry::Row(menu_row("da-share", &icons::SHARE, S::CopyLink.get(lang), false).on_press_tx(move |s: &mut State, tx: &mut rune::Tx| {
            s.close_popover();
            tx.copy(copy.clone());
            let text = s.tr(S::LinkCopied);
            s.toast(text);
        })));
        out.push(Entry::Row(menu_row("da-web", &icons::EXTERNAL, S::OpenInBrowser.get(lang), false).on_press_tx(move |s: &mut State, tx: &mut rune::Tx| {
            s.close_popover();
            let _ = tx.open_url(&link);
        })));
    }
    if !d.facts.is_empty() {
        if !out.is_empty() {
            out.push(Entry::Divider);
        }
        // The facts slide in on the side sheet.
        out.push(Entry::Row(menu_row("da-about", &icons::INFO, S::Details.get(lang), false).on_press(|s: &mut State| s.open_sheet(Sheet::Page))));
    }
    out
}

/// The compact context menu for a pinned cover in the rail.
fn pinned_entries(s: &State, item: &PinnedItem) -> Vec<Entry> {
    let lang = s.lang;
    let page = item.page.clone();
    let key = item.key();
    vec![
        Entry::Row(menu_row("pinned-open", &icons::EXTERNAL, S::Open.get(lang), false).on_press(move |s: &mut State| {
            s.close_popover();
            s.go(page.clone());
        })),
        Entry::Row(menu_row("pinned-remove", &icons::TRASH, S::Unpinned.get(lang), false).on_press(move |s: &mut State| s.remove_pinned(&key))),
    ]
}

/// A small download manager view. Downloads are run in order by the background worker, while this
/// list gives every queued item a stable status instead of reducing a batch to one toast.
fn download_entries(s: &State) -> Vec<Entry> {
    if s.downloads.is_empty() {
        return vec![
            download_head(s),
            Entry::Block(label(S::Nothing.get(s.lang), th::BODY, th::text2()).px(12.0).py(10.0), 42.0),
        ];
    }
    let mut ids: Vec<&String> = s.downloads.keys().collect();
    ids.sort_unstable_by(|a, b| {
        s.download_titles
            .get(*a)
            .cmp(&s.download_titles.get(*b))
            .then_with(|| a.cmp(b))
    });
    let rows = ids
        .into_iter()
        .filter_map(|id| {
            let state = s.downloads.get(id).copied()?;
            let title = s.download_titles.get(id).cloned().unwrap_or_else(|| id.clone());
            let downloaded = s.download_store.as_ref().and_then(|store| store.get(id)).map(|item| item.track);
            let status = match state {
                DownloadState::Downloading => S::DownloadInProgress,
                DownloadState::Done => S::DownloadDone,
                DownloadState::Failed => S::DownloadFailed,
            };
            let glyph_icon = match state {
                DownloadState::Downloading => &icons::SPINNER,
                DownloadState::Done => &icons::DOWNLOAD,
                DownloadState::Failed => &icons::DOWNLOAD,
            };
            let color = match state {
                DownloadState::Downloading => th::text2(),
                DownloadState::Done => th::rgb(0x4bd27e),
                DownloadState::Failed => th::rgb(0xf18a8a),
            };
            let glyph = if matches!(state, DownloadState::Downloading) {
                icon(glyph_icon, 12.0).color(color).looping(rune::LoopSpec::rotation(0.9))
            } else {
                icon(glyph_icon, 12.0).color(color)
            };
            let mut row = row()
                .key(format!("download-manager-{id}"))
                .role("button")
                .label(title.as_str())
                .w_full()
                .h(44.0)
                .px(10.0)
                .gap(10.0)
                .items_center();
            if let Some(track) = downloaded.clone() {
                let open_track = track.clone();
                row = row
                    .cursor(Cursor::Pointer)
                    .hover(th::veil(0.05))
                    .on_press(move |s: &mut State| {
                        s.close_popover();
                        s.open_sheet(Sheet::Track(open_track.clone()));
                    })
                    .child(kit::cover(s, track.cover_template(), 32.0, 8.0));
            }
            row = row.child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(1.0)
                    .child(label(title, th::BODY_M, th::text()).truncate())
                    .child(rune::row().gap(4.0).items_center().child(glyph).child(label(status.get(s.lang), th::SMALL, th::text3()))),
            );
            if matches!(state, DownloadState::Done)
                && let Some(track) = downloaded
            {
                row = row.child(kit::ibtn(format!("download-delete-{id}"), &icons::TRASH, 17.0, 30.0, th::text3(), S::DeleteDownload.get(s.lang)).on_press(move |s: &mut State| s.ask_delete_download(track.clone())));
            }
            Some(row)
        })
        .collect::<Vec<_>>();
    // Keep the popover inside the window and make a large queue wheel-scrollable instead of
    // letting hundreds of rows run underneath the player bar.
    const LIST_H: f32 = 220.0;
    let list_h = (rows.len() as f32 * 44.0).min(LIST_H);
    vec![
        download_head(s),
        Entry::Block(
            col()
                .key("download-manager-list")
                .w_full()
                .h(list_h)
                .scroll(rune::ScrollSpec { stick_bottom: false })
                .children(rows),
            list_h,
        ),
    ]
}

fn download_head(s: &State) -> Entry {
    let folder = kit::ibtn("download-folder", &icons::FOLDER, 17.0, 30.0, th::text2(), S::OpenDownloads.get(s.lang)).on_press(|s: &mut State| s.open_download_folder());
    let clear = kit::ibtn("download-clear", &icons::TRASH, 17.0, 30.0, th::text2(), S::ClearDownloadHistory.get(s.lang)).on_press(|s: &mut State| s.clear_download_history());
    Entry::Head(
        row()
            .h(HEAD)
            .w_full()
            .px(10.0)
            .items_center()
            .child(label(S::DownloadManager.get(s.lang), th::SECTION, th::text()))
            .child(el().grow(1.0))
            .child(folder)
            .child(clear),
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
                    .hover(th::veil(0.06))
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
    [
        (S::StageFlash, [S::StageOff, S::StageSoft, S::StageBright]),
        (S::StageReact, [S::StageCalm, S::StageLively, S::StageWild]),
    ]
}

fn quality_entries(s: &State) -> Vec<Entry> {
    [
        (QualityPref::Saver, S::QualitySaver),
        (QualityPref::Balanced, S::QualityBalanced),
        (QualityPref::Best, S::QualityBest),
    ]
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
                .hover(th::veil(0.06))
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

/// The lyrics' four settings, after the two player-background settings.
pub fn lyric_rows() -> [(S, [S; 3]); 4] {
    [
        (S::LyricSize, [S::Smaller, S::Normal, S::Larger]),
        (S::LyricAnim, [S::AnimNone, S::AnimSmooth, S::AnimSpring]),
        (S::LyricFocus, [S::FocusLight, S::FocusMid, S::FocusStrong]),
        (S::LyricAlign, [S::AlignLeft, S::AlignCenter, S::AlignRight]),
    ]
}

/// The look's setting `i` (as [`stage_rows`] orders them, then [`lyric_rows`] from 2).
pub fn stage_value(s: &State, i: usize) -> u8 {
    let (look, ly) = (&s.settings.stage, &s.settings.lyric_look);
    [look.flash, look.react, ly.size, ly.anim, ly.focus, ly.align][i]
}

pub fn set_stage_value(s: &mut State, i: usize, v: u8) {
    let (l, ly) = (&mut s.settings.stage, &mut s.settings.lyric_look);
    match i {
        0 => l.flash = v,
        1 => l.react = v,
        2 => ly.size = v,
        3 => ly.anim = v,
        4 => ly.focus = v,
        _ => ly.align = v,
    }
    s.mark_settings_dirty();
}

/// Three choices side by side in one soft track, the one picked lit.
pub fn stage_picker(key: &str, i: usize, names: [&str; 3], current: u8) -> El {
    let mut track = row().key(key.to_owned()).w_full().h(32.0).padding(3.0).gap(2.0).radius(10.0).fill(th::veil(0.05));
    for (j, name) in names.into_iter().enumerate() {
        let on = current as usize == j;
        track = track.child(
            row()
                .key(format!("{key}-{j}"))
                .role("radio")
                .label(name)
                .checked(on)
                .grow(1.0)
                .h_full()
                .radius(7.0)
                .center()
                .cursor(Cursor::Pointer)
                .fill(if on { th::veil(0.14) } else { rune::Color::TRANSPARENT })
                .hover(th::veil(if on { 0.16 } else { 0.06 }))
                .press(th::veil(0.18))
                .on_press(move |s: &mut State| set_stage_value(s, i, j as u8))
                .child(label(name.to_owned(), th::SMALL, if on { th::text() } else { th::text2() }).truncate()),
        );
    }
    track
}

/// The look's popover: a head with the reset, then each setting's name over its three choices.
fn stage_entries(s: &State) -> Vec<Entry> {
    let lang = s.lang;
    let changed = s.settings.stage != crate::store::StageLook::default() || s.settings.lyric_look != crate::store::LyricLook::default();
    let mut reset = row()
        .key("sl-reset")
        .role("button")
        .label(S::StageReset.get(lang))
        .h(26.0)
        .px(10.0)
        .radius(13.0)
        .center()
        .transition(Transition::none());
    if changed {
        reset = reset
            .cursor(Cursor::Pointer)
            .hover(th::veil(0.08))
            .press(th::veil(0.12))
            .on_press(|s: &mut State| {
                s.settings.stage = crate::store::StageLook::default();
                s.settings.lyric_look = crate::store::LyricLook::default();
                s.mark_settings_dirty();
            });
    }
    let reset = reset.child(label(S::StageReset.get(lang).to_owned(), th::SMALL, if changed { th::text2() } else { th::text3().alpha(0.5) }));
    let head = row()
        .w_full()
        .h(40.0)
        .px(10.0)
        .gap(10.0)
        .items_center()
        .child(rune::icon(&icons::STAGE, 18.0).color(th::text2()))
        .child(label(S::StageLook.get(lang).to_owned(), th::BODY_M, th::text()).grow(1.0))
        .child(reset);
    // Two columns: the picture behind the player on the left, the lyrics on the right.
    let setting = |i: usize, name: S, opts: [S; 3]| {
        let names = opts.map(|o| o.get(lang));
        col()
            .w_full()
            .h(STAGE_ROW)
            .pt(4.0)
            .gap(6.0)
            .child(label(name.get(lang).to_owned(), th::SMALL, th::text3()))
            .child(stage_picker(&format!("sl-{i}"), i, names, stage_value(s, i)))
    };
    let column = |title: S, rows: Vec<El>| col().grow(1.0).min_w(0.0).child(row().h(26.0).items_center().child(kit::caps(title.get(lang), th::text2()))).children(rows);
    let left = column(S::StageBack, stage_rows().into_iter().enumerate().map(|(i, (n, o))| setting(i, n, o)).collect());
    let right = column(S::LyricText, lyric_rows().into_iter().enumerate().map(|(i, (n, o))| setting(2 + i, n, o)).collect());
    let body_h = 26.0 + 4.0 * STAGE_ROW + 8.0;
    let body = row().w_full().h(body_h).px(10.0).gap(18.0).items_start().child(left).child(el().w(1.0).h(body_h - 14.0).mt(6.0).fill(th::hairline())).child(right);
    vec![Entry::Block(head, 40.0), Entry::Divider, Entry::Block(body, body_h)]
}

const STAGE_ROW: f32 = 62.0;

/// The popover that is open (or closing): a card that grows out of its button like the menus.
pub fn popover(s: &State, cx: &Cx<State>, w: f32, h: f32) -> El {
    let Some(p) = &s.popover else { return el() };
    let (list, width) = match p {
        Popover::Artists { artists, .. } => (artist_entries(s, artists), 260.0),
        Popover::About { .. } => (about_entries(s), ABOUT_W),
        Popover::Actions { .. } => (action_entries(s), 248.0),
        Popover::Waves { choices, .. } => (wave_entries(s, choices), 300.0),
        Popover::Quality { .. } => (quality_entries(s), 220.0),
        Popover::Stage { .. } => (stage_entries(s), 580.0),
        Popover::Pinned { item, .. } => (pinned_entries(s, item), 208.0),
        Popover::Downloads { .. } => (download_entries(s), 300.0),
    };
    let flow = place_w(cx, &PopupAnchor::Key(p.anchor().into()), width, height(&list), w, h);
    flow_menu("popover", s.pop.t, s.popover_open, &flow, list, |s| s.close_popover())
}

// ---- the details drawer ------------------------------------------------------------------------------

/// The drawer's width at most, and how far below the title bar it stops at its tallest.
const DRAWER_W: f32 = 560.0;
const DRAWER_TOP: f32 = 24.0;
const DRAWER_PAD: f32 = 22.0;
/// A fact's tile, a tile with a chart, and the chart in it.
const TILE_H: f32 = 58.0;
const CHART_TILE_H: f32 = 76.0;
const CHART_H: f32 = 44.0;
/// The picture at the head.
const PIC: f32 = 84.0;
const TILE_GAP: f32 = 8.0;

/// A tile's plate: the same soft fill for every tile.
fn tile_plate(key: String, w: f32) -> El {
    col().key(key).w(w).min_w(0.0).px(14.0).radius(14.0).fill(th::veil(0.045)).transition(Transition::none())
}

/// One fact, as a tile: its name in small capitals over its value; a value that leads somewhere
/// (an album, an artist) is a link.
fn fact_tile(key: String, name: &str, value: El, w: f32) -> El {
    tile_plate(key, w).min_h(TILE_H).py(10.0).gap(4.0).child(kit::caps(name, th::text3()).truncate().min_w(0.0)).child(value)
}

/// A number with its picture: the name, the value, and under them a small bar chart printed in a
/// dithered gradient (`fx::CHART`) that rises as the drawer comes up.
fn chart_tile(key: String, name: &str, value: String, note: Option<String>, chart: [f32; 4], bars: usize, w: f32) -> El {
    let mut words = col().grow(1.0).min_w(0.0).gap(2.0).child(kit::caps(name, th::text3()).truncate().min_w(0.0)).child(label(value, th::H2, th::text()).truncate().min_w(0.0));
    if let Some(note) = note {
        words = words.child(label(note, th::SMALL, th::text3()).truncate().min_w(0.0));
    }
    // The chart as wide as its bars want (14 points each), at most a little over half the tile.
    let chart_w = (bars as f32 * 14.0).clamp(42.0, ((w - 28.0) * 0.56).floor());
    let mut tile = row().key(key.clone()).w(w).min_w(0.0).h(CHART_TILE_H).px(14.0).gap(12.0).items_center().radius(14.0).fill(th::veil(0.045)).transition(Transition::none());
    tile = tile.child(words).child(el().key(format!("{key}-chart")).w(chart_w).h(CHART_H).transition(Transition::none()).effect(fx::CHART).effect_data(chart));
    tile
}

/// A value that opens `page` when pressed, lighting up under the pointer.
fn fact_link(key: String, text: String, page: Page) -> El {
    row()
        .key(key)
        .role("link")
        .label(text.as_str())
        .cursor(Cursor::Pointer)
        .hover_group()
        .w_full()
        .min_w(0.0)
        .on_press(move |s: &mut State| {
            s.sheet_open = false;
            s.go(page.clone());
        })
        .child(label(text, th::BODY_M, th::text().alpha(0.86)).group_color(th::text()).truncate().min_w(0.0))
}

/// A round icon button of the drawer; what it does is its tooltip.
fn sheet_button(key: &'static str, ic: &'static Icon, name: &str) -> El {
    row()
        .key(key)
        .role("button")
        .label(name)
        .help(name)
        .w(34.0)
        .h(34.0)
        .radius(17.0)
        .center()
        .cursor(Cursor::Pointer)
        .fill(th::veil(0.06))
        .hover(th::veil(0.11))
        .press(th::veil(0.15))
        .press_scale(th::PRESS)
        .child(rune::icon(ic, 16.0).color(th::text2()))
}

/// `values` squeezed into at most `n` bars, each the mean of its share.
fn squeeze(values: &[f32], n: usize) -> Vec<f32> {
    let len = values.len();
    if len <= n {
        return values.to_vec();
    }
    (0..n)
        .map(|i| {
            let (a, b) = (i * len / n, ((i + 1) * len / n).max(i * len / n + 1));
            values[a..b].iter().sum::<f32>() / (b - a) as f32
        })
        .collect()
}

/// How many of `years` fall in each span between the first and the last (a bar a year while
/// they fit, wider spans after), with that first and last; `None` for fewer than two years.
fn year_counts(years: &[i32]) -> Option<(Vec<f32>, i32, i32)> {
    let (lo, hi) = (*years.iter().min()?, *years.iter().max()?);
    if hi <= lo {
        return None;
    }
    let span = (hi - lo + 1) as usize;
    let n = span.min(18);
    let mut bins = vec![0.0f32; n];
    for y in years {
        bins[((y - lo) as usize * n / span).min(n - 1)] += 1.0;
    }
    Some((bins, lo, hi))
}

/// Details, in a drawer that rises from the bottom of the window over the page, which goes soft
/// and a little dark behind it. Its handle can be pulled down to close it; a press beside it, its
/// X or Escape close it too.
pub fn sheet(s: &State, w: f32, h: f32) -> El {
    let Some(what) = &s.sheet else { return el() };
    let lang = s.lang;
    let t = s.sheet_pop.t.clamp(0.0, 1.2);
    let a = t.clamp(0.0, 1.0);
    let dw = DRAWER_W.min(w - 32.0);
    let inner = dw - 2.0 * DRAWER_PAD;
    let mut layer = el().key("sheet").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).instant_changes();
    layer = layer
        .child(el().key("sheet-snap").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).effect(fx::SNAP).effect_backdrop(true))
        .child(el().key("sheet-blur").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).effect(fx::VEIL).effect_data([-a, 0.0, 18.0, 0.0]))
        .child(el().key("sheet-dim").absolute(0.0, 0.0).w_full().h_full().fill(th::rgba(0x000000, 0.30 * a)).transition(Transition::none()));
    if s.sheet_open {
        layer = layer.child(el().key("sheet-scrim").absolute(0.0, 0.0).w_full().h_full().on_press(|s: &mut State| s.sheet_open = false));
    }

    // What it shows: what kind of thing, its picture and name with who made it, the numbers that
    // have a picture (track lengths, release years), the other facts, then what it is about.
    struct Fact {
        name: S,
        value: String,
        page: Option<Page>,
    }
    struct Chart {
        name: S,
        value: String,
        note: Option<String>,
        values: Vec<f32>,
        /// Lengths, not counts: scaled from the shortest, so tracks of near one length differ.
        from_min: bool,
    }
    struct Head {
        kind: S,
        cover: Option<String>,
        title: String,
        artists: Vec<ym_api::Artist>,
        sub: String,
        facts: Vec<Fact>,
        charts: Vec<Chart>,
        about: Option<String>,
        link: Option<String>,
        line: String,
        round: bool,
    }
    let fact = |name: S, value: String, page: Option<Page>| Fact { name, value, page };
    let lengths = |tracks: &[ym_api::Track]| -> Vec<f32> { squeeze(&tracks.iter().filter_map(|t| t.duration_ms).map(|ms| ms as f32).collect::<Vec<_>>(), 18) };
    let Head { kind, cover, title, artists, sub, facts, charts, about, link, line, round } = match what {
        Sheet::Track(tr) => {
            let mut f = Vec::new();
            if let Some(al) = tr.albums.first() {
                f.push(fact(S::Album, al.title.clone(), Some(Page::Album(al.id.clone()))));
                if let Some(y) = al.year {
                    f.push(fact(S::Year, y.to_string(), None));
                }
                if let Some(g) = al.genre.as_deref().filter(|g| !g.is_empty()) {
                    f.push(fact(S::Genre, crate::genre::name(g, lang), None));
                }
            }
            if let Some(ms) = tr.duration_ms {
                f.push(fact(S::Duration, lang.duration(ms / 1000), None));
            }
            if tr.is_explicit() {
                f.push(fact(S::Explicit, S::Yes.get(lang).to_owned(), None));
            }
            Head {
                kind: S::Track,
                cover: tr.cover_template().map(str::to_owned),
                title: tr.full_title(),
                artists: tr.artists.clone(),
                sub: String::new(),
                facts: f,
                charts: Vec::new(),
                about: None,
                link: Some(track_link(tr)),
                line: track_line(tr),
                round: false,
            }
        }
        Sheet::Page => {
            let d = &s.detail;
            let mut f = Vec::new();
            let mut charts = Vec::new();
            let mut about = None;
            let value_of = |name: S| d.facts.iter().find(|(n, _)| *n == name).map(|(_, v)| v.clone());
            if d.is_artist {
                // Their releases over the years; the rest as plain facts.
                let years: Vec<i32> = d.albums.iter().filter_map(|al| al.year).collect();
                if let (Some(n), Some((bins, lo, hi))) = (value_of(S::Albums), year_counts(&years)) {
                    charts.push(Chart { name: S::Albums, value: n, note: Some(format!("{lo}–{hi}")), values: bins, from_min: false });
                }
            } else {
                // The length of every track, in order; for a playlist also the years they are from.
                let lens = lengths(&d.tracks);
                if let Some(total) = value_of(S::Duration).filter(|_| lens.len() >= 2) {
                    let note = value_of(S::Tracks).and_then(|n| n.parse::<usize>().ok()).map(|n| lang.tracks(n));
                    charts.push(Chart { name: S::Duration, value: total, note, values: lens, from_min: true });
                }
                if matches!(d.page, Some(Page::Playlist(..))) {
                    let years: Vec<i32> = d.tracks.iter().filter_map(|t| t.album().and_then(|al| al.year)).collect();
                    if let Some((bins, lo, hi)) = year_counts(&years) {
                        charts.push(Chart { name: S::Year, value: format!("{lo}–{hi}"), note: None, values: bins, from_min: false });
                    }
                }
            }
            let charted = |n: S| charts.iter().any(|c| c.name == n) || (n == S::Tracks && charts.iter().any(|c| c.name == S::Duration));
            for (n, v) in &d.facts {
                if *n == S::Description {
                    about = Some(v.clone());
                } else if !charted(*n) {
                    f.push(fact(*n, v.clone(), None));
                }
            }
            let kind = match d.page {
                Some(Page::Album(_)) => S::Album,
                Some(Page::Playlist(..)) => S::Playlist,
                _ => S::Artist,
            };
            let line = if d.subtitle.is_empty() { d.title.clone() } else { format!("{} — {}", d.subtitle, d.title) };
            Head {
                kind,
                cover: d.cover.clone(),
                title: d.title.clone(),
                artists: d.artists.clone(),
                sub: d.subtitle.clone(),
                facts: f,
                charts,
                about,
                link: d.link.clone(),
                line,
                round: d.is_artist,
            }
        }
    };

    // The handle: a short pill at the top; pulled down far enough, the drawer closes.
    let grip = row()
        .key("sheet-grip")
        .w_full()
        .h(20.0)
        .center()
        .cursor(Cursor::Grab)
        .on_drag(|s: &mut State, d: &mut rune::Drag, _tx: &mut rune::Tx| {
            if d.phase == rune::Phase::Up && d.dy > 48.0 {
                s.sheet_open = false;
            }
        })
        .child(el().w(36.0).h(4.0).radius(2.0).fill(th::veil(0.20)));

    let pic_el = if round {
        kit::cover(s, cover.as_deref(), PIC, PIC * 0.5)
    } else {
        el().w(PIC).h(PIC).radius(14.0).shadow(8.0, 22.0, th::rgba(0x000000, 0.30)).child(kit::cover(s, cover.as_deref(), PIC, 14.0))
    };

    // What can be done, as icons whose tooltips say it: the link copied, the name copied, the
    // page opened on the web.
    let mut actions = row().key("sheet-actions").gap(8.0).mt(8.0);
    if let Some(link) = link.clone() {
        actions = actions.child(sheet_button("sheet-link", &icons::SHARE, S::CopyLink.get(lang)).on_press_tx(move |s: &mut State, tx: &mut rune::Tx| {
            tx.copy(link.clone());
            let text = s.tr(S::LinkCopied);
            s.toast(text);
        }));
    }
    actions = actions.child(sheet_button("sheet-copy", &icons::COPY, S::CopyTitle.get(lang)).on_press_tx(move |s: &mut State, tx: &mut rune::Tx| {
        tx.copy(line.clone());
        let text = s.tr(S::TitleCopied);
        s.toast(text);
    }));
    if let Some(link) = link {
        actions = actions.child(sheet_button("sheet-web", &icons::EXTERNAL, S::OpenInBrowser.get(lang)).on_press_tx(move |_s: &mut State, tx: &mut rune::Tx| {
            let _ = tx.open_url(&link);
        }));
    }
    if matches!(what, Sheet::Page) && !s.detail.tracks.is_empty() {
        let tracks = s.detail.tracks.clone();
        actions = actions.child(sheet_button("sheet-download", &icons::DOWNLOAD, S::DownloadAll.get(lang)).on_press(move |s: &mut State| s.ask_download(tracks.clone())));
    }

    let mut names = col().grow(1.0).min_w(0.0).gap(3.0).child(kit::caps(kind.get(lang), th::text3())).child(label(title, th::H2, th::text()).wrap().max_lines(2));
    if !artists.is_empty() {
        names = names.child(kit::artists_row_in_width(s, &artists, th::BODY, false, "sheet-art", th::text2(), (inner - PIC - 56.0).max(80.0)));
    } else if !sub.is_empty() {
        names = names.child(label(sub, th::BODY, th::text2()).truncate());
    }
    names = names.child(actions);
    let close = sheet_button("sheet-close", &icons::CLOSE, S::Close.get(lang)).on_press(|s: &mut State| s.sheet_open = false);
    let head = row().key("sheet-head").w_full().gap(16.0).pr(40.0).items_center().child(pic_el).child(names);

    // The charts two across (one when there is one), then the facts three across (two in a
    // narrow window), all in tiles of one look.
    let hue = super::blobs::avatar_seed(&s.page.key()).1;
    let risen = ((t - 0.35) / 0.65).clamp(0.0, 1.0);
    let n_charts = charts.len();
    let chart_w = if n_charts > 1 { ((inner - TILE_GAP) / 2.0).floor() } else { inner };
    let mut chart_row = row().key("sheet-charts").w_full().gap(TILE_GAP);
    for (i, c) in charts.into_iter().enumerate() {
        let data = fx::chart_data(&c.values, c.from_min, hue + 48.0 * i as f32, risen);
        chart_row = chart_row.child(chart_tile(format!("sheet-c-{i}"), c.name.get(lang), c.value, c.note, data, c.values.len(), chart_w));
    }
    let n_facts = facts.len();
    // Rows of even length: four facts two by two, otherwise three across (two when narrow).
    let cols = if n_facts == 4 || inner < 440.0 { 2 } else { 3 }.min(n_facts.max(1));
    let tile_w = ((inner - TILE_GAP * (cols as f32 - 1.0)) / cols as f32).floor();
    let mut tiles = row().key("sheet-facts").w_full().gap(TILE_GAP).flex_wrap();
    for (i, f) in facts.into_iter().enumerate() {
        let value = match f.page {
            Some(page) => fact_link(format!("sheet-fl-{i}"), f.value, page),
            None => label(f.value, th::BODY_M, th::text()).truncate().min_w(0.0),
        };
        tiles = tiles.child(fact_tile(format!("sheet-f-{i}"), f.name.get(lang), value, tile_w));
    }

    let has_about = about.is_some();
    let mut body = col().key("sheet-body").w_full().gap(16.0).child(head);
    let mut stats = col().key("sheet-stats").w_full().gap(TILE_GAP);
    if n_charts > 0 {
        stats = stats.child(chart_row);
    }
    if n_facts > 0 {
        stats = stats.child(tiles);
    }
    if n_charts + n_facts > 0 {
        body = body.child(stats);
    }
    if let Some(text) = about {
        body = body.child(label(text, th::BODY, th::text2()).wrap().max_lines(8));
    }

    // As tall as it needs (a guess from what it holds), never above the title bar's line.
    let rows = n_facts.div_ceil(cols);
    let stats_h = if n_charts > 0 { CHART_TILE_H + TILE_GAP } else { 0.0 } + rows as f32 * (TILE_H + TILE_GAP);
    let want = 20.0 + PIC.max(112.0) + 16.0 + stats_h + if has_about { 176.0 } else { 0.0 } + DRAWER_PAD + 8.0;
    let dh = want.min(h - kit::TITLE_H - DRAWER_TOP).max(240.0);
    let x = (w - dw) * 0.5;
    // It rises from below the window; overshooting a little, it lifts a touch more and its foot
    // runs on under the window's edge, so no gap opens under it.
    let y = h - dh * t;
    let panel = col()
        .key("sheet-panel")
        .absolute(x, y)
        .w(dw)
        .h(dh + 40.0)
        .radius(24.0)
        .clip_rounded(24.0)
        .fill(th::pop())
        .border(1.0, th::veil(0.10))
        .shadow(30.0, 50.0, th::rgba(0x000000, 0.36 * a))
        .px(DRAWER_PAD)
        .pt(2.0)
        .pb(40.0 + DRAWER_PAD)
        .transition(Transition::none())
        // A press on the drawer itself stays on it.
        .on_press(|_s: &mut State| {})
        .child(grip)
        .child(close.absolute(dw - DRAWER_PAD - 30.0, 16.0))
        .child(col().key("sheet-scroll").w_full().grow(1.0).min_h(0.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(body));
    let panel = if s.sheet_open { panel.modal(true, |s: &mut State| s.sheet_open = false) } else { panel };
    layer.child(panel)
}

// ---- a cover, large --------------------------------------------------------------------------------

/// A cover opened large in the middle of the window over a dark sheet. It grows out of the cover
/// that was pressed and goes back into it; a press anywhere (or Escape) closes it.
pub fn lightbox(s: &State, cx: &Cx<State>, w: f32, h: f32) -> El {
    let Some((tpl, from_key)) = &s.lightbox else { return el() };
    let t = s.light.t.clamp(0.0, 1.0);
    let side = (w.min(h) - 140.0).clamp(220.0, 760.0);
    let to = [(w - side) * 0.5, (h - side) * 0.5, side, side];
    // While the cover is up here its place on the page holds an empty slot of the same size
    // (`<key>-gap`), so there is one picture, not two; it flies back into that slot.
    let from_rect = cx.window_layout(from_key).or_else(|| cx.window_layout(&format!("{from_key}-gap")));
    let (from, from_r) = match from_rect {
        Some(r) => (r, r[2].min(r[3]) * 0.5),
        None => ([w * 0.5 - 40.0, h * 0.5 - 40.0, 80.0, 80.0], 20.0),
    };
    // The round artist picture keeps a little of its roundness on the way; a square cover stays
    // rounded like the others.
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    let (x, y, cw, ch) = (lerp(from[0], to[0]), lerp(from[1], to[1]), lerp(from[2], to[2]), lerp(from[3], to[3]));
    let round = s.detail.is_artist;
    let from_r = if round { from_r } else { 20.0_f32.min(from_r) };
    let to_r = if round { side * 0.5 } else { 22.0 };
    let r = lerp(from_r, to_r).min(cw.min(ch) * 0.5);
    let pic = if round && from_key == "detail-cover" {
        // An artist's picture is their shape (nearly a circle) on the page: the same shape, drawn
        // by the compositor, opens out into a rounded square as it grows, and folds back on the
        // way home, so nothing jumps between the two.
        let e = t * t * (3.0 - 2.0 * t);
        let (seed, hue) = super::blobs::avatar_seed(&s.page.key());
        let corner = lerp(cw * 0.5, cw * 0.04);
        el().key("lightbox-pic")
            .w(cw)
            .h(ch)
            .radius(corner)
            .absolute(x, y)
            .shadow(18.0 * t, 60.0, th::rgba(0x000000, 0.45 * t))
            .transition(Transition::none())
            .child(super::blobs::avatar("lightbox-avatar", seed, super::blobs::Fill::Picture(hue), super::blobs::Look::default(), e, cw).absolute(0.0, 0.0))
    } else {
        kit::cover_px_at(s, Some(tpl.as_str()), side, r, 1000)
            .key("lightbox-pic")
            .w(cw)
            .h(ch)
            .radius(r)
            .clip_rounded(r)
            .absolute(x, y)
            .shadow(18.0 * t, 60.0, th::rgba(0x000000, 0.45 * t))
            .transition(Transition::none())
    };
    let mut layer = el().key("lightbox").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).instant_changes();
    // Behind the picture the window goes soft and dark: a fresh copy of the frame (so the rail and
    // the bar blur too, not only the page), a veil over all of it that blurs as the picture comes
    // up, and a dark wash on top.
    layer = layer
        .child(el().key("lightbox-snap").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).effect(fx::SNAP).effect_backdrop(true))
        .child(el().key("lightbox-blur").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).effect(fx::VEIL).effect_data([-t, 0.0, 28.0, 0.0]))
        .child(el().key("lightbox-sheet").absolute(0.0, 0.0).w_full().h_full().fill(th::rgba(0x000000, 0.46 * t)).transition(Transition::none()));
    if s.lightbox_open {
        layer = layer.child(
            el().key("lightbox-hit")
                .role("button")
                .label(crate::i18n::S::Close.get(s.lang))
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .cursor(Cursor::Pointer)
                .on_press(|s: &mut State| s.lightbox_open = false),
        );
    }
    // The way out, beside the picture's top corner when there is room for it, else in the
    // window's top right corner under the title bar. It comes in a beat after the picture.
    let btn = 44.0;
    let (bx, by) = if to[0] >= btn + 40.0 { (to[0] + side + 18.0, to[1]) } else { (w - btn - 20.0, kit::TITLE_H + 8.0) };
    let a = ((t - 0.35) / 0.65).clamp(0.0, 1.0);
    let close = row()
        .key("lightbox-close")
        .role("button")
        .label(crate::i18n::S::Close.get(s.lang))
        .help(crate::i18n::S::Close.get(s.lang))
        .absolute(bx, by + (1.0 - a) * 8.0)
        .w(btn)
        .h(btn)
        .radius(btn * 0.5)
        .center()
        .cursor(Cursor::Pointer)
        .fill(th::rgba(0xffffff, 0.12 * a))
        .border(1.0, th::rgba(0xffffff, 0.16 * a))
        .hover(th::rgba(0xffffff, 0.22 * a))
        .press(th::rgba(0xffffff, 0.30 * a))
        .press_scale(th::PRESS)
        .transition(Transition::none())
        .on_press(|s: &mut State| s.lightbox_open = false)
        .child(rune::icon(&super::icons::CLOSE, 20.0).color(th::rgba(0xffffff, 0.92 * a)));
    layer.child(pic).child(close)
}

// ---- the volume card ---------------------------------------------------------------------------------

const VOL_W: f32 = 64.0;
const VOL_H: f32 = 212.0;
/// The notches of the level, bottom to top; the top of the card holds the number.
const VOL_NOTCHES: usize = 15;
const VOL_TOP: f32 = 38.0;
const VOL_BOTTOM: f32 = 14.0;

/// The volume card: it grows out of the speaker button like the menus do, and its level is a stack of
/// notches that grow wider toward the top. They rise into place one after another from the bottom
/// while it opens, then fill up to the level; drag anywhere on them to set it. The notch at the
/// level is lit by how far into it the level is, so the stack moves smoothly rather than in steps.
pub fn volume(s: &State, cx: &Cx<State>, w: f32, h: f32) -> El {
    let (t, open) = (s.vol.t, s.vol_open);
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
        field = field.child(
            el().absolute(nx, ny)
                .w(width * (0.5 + 0.5 * a))
                .h(4.0)
                .radius(2.0)
                .fill(th::veil((0.16 + 0.84 * lit) * a))
                .transition(Transition::none()),
        );
    }
    let pct = (v * 100.0).round() as i32;
    let number = row()
        .absolute(to[0] - x, to[1] - y + 12.0)
        .w(VOL_W)
        .h(18.0)
        .center()
        .transition(Transition::none())
        .child(label(format!("{pct}"), th::MONO, th::text2()).opacity(((t - 0.15) / 0.3).clamp(0.0, 1.0)));
    let card = el()
        .key("vol-card")
        .absolute(x, y)
        .w(cw)
        .h(ch)
        .radius(radius)
        .clip_rounded(radius)
        .fill(th::pop().alpha(skin))
        .border(1.0, th::veil(0.10 * skin))
        .transition(Transition::none())
        .on_hover_change(|s: &mut State, over: bool, _tx: &mut rune::Tx| {
            if over {
                s.vol_close_at = None;
            } else {
                s.vol_close_at = Some(s.now + 0.18);
            }
        })
        .child(number)
        .child(field);
    let card = if open {
        card.modal(true, |s: &mut State| {
            s.vol_open = false;
            s.vol_close_at = None;
        })
    } else {
        card
    };
    let mut layer = el().key("vol-layer").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).instant_changes();
    if open {
        layer = layer.child(
            el().key("vol-scrim")
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .on_press(|s: &mut State| {
                    s.vol_open = false;
                    s.vol_close_at = None;
                })
                .on_secondary_press(|s: &mut State, _, _| {
                    s.vol_open = false;
                    s.vol_close_at = None;
                }),
        );
    }
    layer.child(card)
}

// ---- the profile menu --------------------------------------------------------------------------------

/// The menu of the profile button: who is signed in, My Wave, Settings and the way out.
pub fn profile(s: &State, cx: &Cx<State>, w: f32, h: f32) -> El {
    let lang = s.lang;
    let name = super::dock::account_name(s);
    let sub = s.account.as_ref().map(|a| a.login.clone()).unwrap_or_default();
    let plus = s.account.as_ref().is_some_and(|a| a.has_plus) && s.session == Session::Live;
    let initial = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    let head = row()
        .w_full()
        .h(HEAD)
        .px(10.0)
        .gap(10.0)
        .items_center()
        .child(super::dock::account_picture(s, 28.0, &initial))
        .child(
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
        Entry::Row(entry("menu-settings", &icons::SETTINGS, S::Settings.get(lang), s.page == Page::Settings).on_press(|s: &mut State| s.go(Page::Settings))),
        Entry::Divider,
        Entry::Row(entry("menu-out", &icons::SIGN_OUT, S::SignOut.get(lang), false).on_press(|s: &mut State| s.sign_out())),
    ];
    let flow = place(cx, &PopupAnchor::Key("profile".into()), height(&list), w, h);
    flow_menu("menu", s.menu_t, s.menu_open, &flow, list, |s| s.menu_open = false)
}
