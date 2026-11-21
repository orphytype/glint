//! Library page: Liked tracks header, collection tabs, live search filter, and card grids.

use rune::{Cursor, El, col, el, icon, row, text_input};

use super::detail::{HEAD_COVER_R, HEAD_GAP, HEAD_SIDE, HEAD_SIDE_S, HEAD_STEP_BUTTONS, HEAD_STEP_BY, HEAD_STEP_TITLE};
use super::shared::{GRID_GAP, album_card, artist_card, column, fit, grid, play_button, playlist_card, track_list};
use crate::app::{LibTab, Load, Page, Popover, State, Station};
use crate::i18n::S;
use crate::theme as th;
use crate::ui::shapes::Ill;
use crate::ui::{
    icons,
    kit::{self, ListSrc, label},
};

const LIB_CTL_H: f32 = 36.0;

fn liked_playlist_cover(s: &State, side: f32, radius: f32) -> El {
    if let Some(tpl) = s.library.liked_cover.as_deref() {
        return el().key("lib-pic").w(side).h(side).radius(radius).child(kit::cover(s, Some(tpl), side, radius));
    }
    let cell = side * 0.5;
    let mut art = el().key("lib-pic").w(side).h(side).radius(radius).clip_rounded(radius).fill(th::veil(0.08));
    let mut count = 0;
    if let Some(tracks) = s.library.tracks.get() {
        for (i, track) in tracks.iter().take(4).enumerate() {
            let x = if i % 2 == 0 { 0.0 } else { cell };
            let y = if i < 2 { 0.0 } else { cell };
            art = art.child(el().absolute(x, y).w(cell + 1.0).h(cell + 1.0).fill(th::srgb(s.palette.colors[i])).child(kit::cover_tile(s, track.cover_template(), cell + 1.0)));
            count += 1;
        }
    }
    if count == 0 {
        art = art
            .child(el().absolute(0.0, 0.0).w_full().h_full().fill(s.pal().accent.alpha(0.24)))
            .child(row().absolute(0.0, 0.0).w_full().h_full().center().child(icon(&icons::HEART_FILL, side * 0.34).color(th::text())));
    }
    art.child(el().absolute(0.0, 0.0).w_full().h_full().radius(radius).fill(th::rgba(0x000000, 0.10)).border(1.0, kit::cover_border()))
}

pub(super) fn library(s: &State, w: f32) -> El {
    let p = s.pal();
    let lang = s.lang;
    let tab = s.library.tab;
    let q = s.library.filter.text.trim().to_lowercase();
    let matches_q = |hay: &str| hay.to_lowercase().contains(&q);
    let count = |n: Option<usize>| n.filter(|n| *n > 0);
    let tabs = [
        (LibTab::Tracks, S::Tracks, count(s.library.tracks.get().map(Vec::len))),
        (LibTab::Playlists, S::Playlists, count(s.library.playlists.get().map(Vec::len))),
        (LibTab::Albums, S::Albums, count(s.library.albums.get().map(Vec::len))),
        (LibTab::Artists, S::Artists, count(s.library.artists.get().map(Vec::len))),
    ];
    let at = tabs.iter().position(|(t, ..)| *t == tab).unwrap_or(0);
    let tabs = kit::tab_track(
        "ltabs",
        tabs.into_iter()
            .map(|(t, name, n)| {
                let label_text = name.get(lang);
                let tw = kit::tab_pill_w(label_text, n);
                (kit::tab_pill_with_w(format!("ltab-{t:?}"), label_text, n, tab == t, tw).on_press(move |s: &mut State| s.load_library_tab(t)), tw)
            })
            .collect(),
        at,
    );
    let input = text_input(&s.library.filter, th::BODY)
        .key("lib-search")
        .single_line()
        .grow(1.0)
        .min_w(0.0)
        .color(th::text())
        .placeholder(S::SearchCollection.get(lang), th::text3())
        .caret(p.accent, p.accent.alpha(0.30))
        .on_edit(|s: &mut State| &mut s.library.filter);
    let mut field = kit::field_surface(row())
        .key("lib-search-field")
        .w(240.0)
        .h(LIB_CTL_H)
        .pl(10.0)
        .pr(4.0)
        .gap(8.0)
        .radius(kit::ctl_radius(LIB_CTL_H))
        .items_center()
        .child(icon(&icons::SEARCH, 16.0).color(th::text3()))
        .child(input);
    if !q.is_empty() {
        field = field.child(kit::ibtn("lib-search-clear", &icons::CLOSE, 14.0, 28.0, th::text2(), S::ClearSearch.get(lang)).on_press(|s: &mut State| s.library.filter.clear()));
    }
    let cw = column(w);
    let new_pl = (tab == LibTab::Playlists).then(|| {
        kit::field_surface(row())
            .key("lib-new-pl")
            .role("button")
            .label(S::NewPlaylist.get(lang))
            .help(S::NewPlaylist.get(lang))
            .w(LIB_CTL_H)
            .h(LIB_CTL_H)
            .radius(kit::ctl_radius(LIB_CTL_H))
            .center()
            .cursor(Cursor::Pointer)
            .press_scale(th::PRESS)
            .press(th::veil(0.10))
            .child(icon(&icons::PLUS, 18.0).color(th::text()))
            .on_press_tx(|s: &mut State, tx: &mut rune::Tx| {
                s.ask_new_playlist(None);
                tx.focus_when_shown("name-field", false);
            })
    });
    let bar = if cw >= 700.0 {
        row().key("lib-bar").w_full().gap(16.0).items_center().child(tabs).child(el().grow(1.0)).children(new_pl).child(field)
    } else {
        col().key("lib-bar").w_full().gap(12.0).child(tabs).child(row().w_full().gap(10.0).items_center().child(field.grow(1.0).min_w(0.0)).children(new_pl))
    };

    let liked = s.library.tracks.get().filter(|v| !v.is_empty());
    let lead = match liked {
        Some(v) => {
            let secs = v.iter().filter_map(|t| t.duration_ms).sum::<u64>() / 1000;
            format!("{} · {}", lang.tracks(v.len()), lang.duration(secs))
        }
        None => String::new(),
    };
    let side = if cw >= 640.0 { HEAD_SIDE } else { HEAD_SIDE_S };
    let pic = liked_playlist_cover(s, side, HEAD_COVER_R);
    let mut meta = col()
        .key("lib-meta")
        .grow(1.0)
        .min_w(0.0)
        .child(kit::caps(S::Playlist.get(lang), th::text3()))
        .child(label(S::Liked.get(lang).to_owned(), th::TITLE_D, th::text()).mt(HEAD_STEP_TITLE));
    if !lead.is_empty() {
        meta = meta.child(label(lead, th::BYLINE, th::text3()).mt(HEAD_STEP_BY));
    }
    if let Some(list) = liked {
        let more_on = s.popover_open && matches!(s.popover, Some(Popover::Liked { .. }));
        let settled = s.downloads_settled(list);
        let (dl_icon, dl_name) = if settled { (&icons::TRASH, S::DeleteDownload) } else { (&icons::DOWNLOAD, S::DownloadAllLiked) };
        let buttons = row()
            .key("lib-buttons")
            .gap(10.0)
            .items_center()
            .mt(HEAD_STEP_BUTTONS)
            .child(play_button(s, "lib-play", Some(Page::Library), false, |s| {
                let list = s.list(ListSrc::Liked);
                s.play_all(list, false);
            }))
            .child(kit::round("lib-shuffle", kit::Weight::Soft, &icons::SHUFFLE, kit::BTN_H, 20.0, S::Shuffle.get(lang)).on_press(|s: &mut State| {
                let list = s.list(ListSrc::Liked);
                s.play_all(list, true);
            }))
            .child(kit::wave_button(s, "lib-wave", S::WaveByCollection.get(lang), S::LikedTracks.get(lang), Station::Collection, kit::BTN_H))
            .child(
                kit::pill_icon("lib-more", kit::Weight::Soft, &icons::MORE, kit::BTN_H, 20.0, S::More.get(lang))
                    .checked(more_on)
                    .on_press(|s: &mut State| s.open_popover(Popover::Liked { anchor: "lib-more".into() })),
            )
            .child(kit::pill_icon("lib-download", kit::Weight::Soft, dl_icon, kit::BTN_H, 20.0, dl_name.get(lang)).on_press(move |s: &mut State| {
                let list = s.library.tracks.get().cloned().unwrap_or_default();
                if settled {
                    s.ask_delete_downloads(list);
                } else {
                    s.ask_download(list);
                }
            }));
        meta = meta.child(buttons);
    }
    let head = if cw < 640.0 {
        col().key("lib-head").w_full().gap(18.0).child(pic).child(meta.w_full())
    } else {
        row().key("lib-head").w_full().gap(HEAD_GAP).items_center().child(pic).child(meta)
    };
    let body = col().gap(32.0).child(kit::rise(head.mt(12.0), 0)).child(kit::rise(bar, 1));
    let (_, card) = fit(w, 144.0, GRID_GAP, 8);
    let nothing = |art: Ill| kit::empty(&p, art, S::Nothing.get(lang));
    let content = match tab {
        LibTab::Tracks => match &s.library.tracks {
            Load::Ready(v) if v.is_empty() => nothing(Ill::Shelf),
            Load::Ready(v) => {
                let n = if q.is_empty() {
                    v.len()
                } else {
                    let key = (q.clone(), v.as_ptr() as usize, v.len());
                    let mut shown = s.library.shown.borrow_mut();
                    let mut cached_key = s.library.shown_key.borrow_mut();
                    if *cached_key != key {
                        *shown = (0..v.len())
                            .filter(|&i| {
                                let t = &v[i];
                                matches_q(&t.title) || matches_q(&t.artists_line()) || t.albums.first().is_some_and(|a| matches_q(&a.title))
                            })
                            .collect();
                        *cached_key = key;
                    }
                    shown.len()
                };
                if n == 0 { nothing(Ill::Lens) } else { track_list("lib-rows", n, ListSrc::Liked) }
            }
            other => kit::list_state(other, s).unwrap_or_else(el),
        },
        LibTab::Playlists => match &s.library.playlists {
            Load::Ready(v) if v.is_empty() => nothing(Ill::Shelf),
            Load::Ready(v) => grid(v.iter().filter(|x| q.is_empty() || matches_q(&x.title)).map(|x| playlist_card(s, x, card)).collect()),
            other => kit::list_state(other, s).unwrap_or_else(el),
        },
        LibTab::Albums => match &s.library.albums {
            Load::Ready(v) if v.is_empty() => nothing(Ill::Shelf),
            Load::Ready(v) => grid(v.iter().filter(|x| q.is_empty() || matches_q(&x.title) || matches_q(&x.artists_line())).map(|x| album_card(s, x, card)).collect()),
            other => kit::list_state(other, s).unwrap_or_else(el),
        },
        LibTab::Artists => match &s.library.artists {
            Load::Ready(v) if v.is_empty() => nothing(Ill::Shelf),
            Load::Ready(v) => grid(v.iter().filter(|x| q.is_empty() || matches_q(&x.name)).map(|x| artist_card(s, x, card)).collect()),
            other => kit::list_state(other, s).unwrap_or_else(el),
        },
    };
    kit::scroll_page("library", body.child(kit::tab_body(format!("lib-tab-{tab:?}"), content)))
}
