//! Library page: Liked tracks header, collection tabs, live search filter, and card grids.

use rune::{El, col, el, icon, row, text_input};

use super::detail::{HEAD_COVER_R, HEAD_GAP, HEAD_SIDE, HEAD_SIDE_S, HEAD_STEP_BUTTONS, HEAD_STEP_BY};
use super::shared::{GRID_GAP, album_card, artist_card, column, fit, grid, play_button, playlist_card, track_list};
use crate::app::{LibTab, Load, Page, Popover, State, Station, TrackSort};
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
    let mut art = el().key("lib-pic").w(side).h(side).radius(radius).fill(th::veil(0.08));
    let mut count = 0;
    if let Some(tracks) = s.library.tracks.get() {
        let tile_corners = [[radius, 0.0, 0.0, 0.0], [0.0, radius, 0.0, 0.0], [0.0, 0.0, 0.0, radius], [0.0, 0.0, radius, 0.0]];
        for (i, track) in tracks.iter().take(4).enumerate() {
            let x = if i % 2 == 0 { 0.0 } else { cell };
            let y = if i < 2 { 0.0 } else { cell };
            let corners = tile_corners[i];
            art = art.child(el().absolute(x, y).w(cell + 1.0).h(cell + 1.0).corners(corners).fill(th::srgb(s.palette.colors[i])).child(kit::cover_tile(
                s,
                track.cover_template(),
                cell + 1.0,
                corners,
            )));
            count += 1;
        }
    }
    if count == 0 {
        art = art
            .child(el().absolute(0.0, 0.0).w_full().h_full().radius(radius).fill(s.pal().accent.alpha(0.24)))
            .child(row().absolute(0.0, 0.0).w_full().h_full().center().child(icon(&icons::HEART_FILL, side * 0.34).color(th::text())));
    }
    art.child(el().absolute(0.0, 0.0).w_full().h_full().radius(radius).fill(th::rgba(0x000000, 0.10)).border(1.0, kit::cover_border()))
}

pub(super) fn library(s: &State, w: f32) -> El {
    let p = s.pal();
    let lang = s.lang;
    let tab = s.library.tab;
    let q = s.library.filter.text.trim().to_owned();
    let query = crate::find::Query::new(&q);
    // Everything when there is no query; otherwise what the query finds, closest matches only.
    let pick = |n: usize, fields: &dyn Fn(usize) -> Vec<String>| -> Vec<usize> {
        match &query {
            None => (0..n).collect(),
            Some(qr) => crate::find::best((0..n).map(|i| {
                let f = fields(i);
                qr.tier(&f.iter().map(String::as_str).collect::<Vec<_>>())
            })),
        }
    };
    let count = |n: Option<usize>| n.filter(|n| *n > 0);
    let tabs = [
        (LibTab::Tracks, S::Tracks, count(s.library.tracks.get().map(Vec::len))),
        (LibTab::Playlists, S::Playlists, count(s.library.playlists.get().map(Vec::len))),
        (LibTab::Albums, S::Albums, count(s.library.albums.get().map(Vec::len))),
        (LibTab::Artists, S::Artists, count(s.library.artists.get().map(Vec::len))),
    ];
    let at = tabs.iter().position(|(t, ..)| *t == tab).unwrap_or(0);
    let tab_items: Vec<(El, f32)> = tabs
        .into_iter()
        .map(|(t, name, n)| {
            let label_text = name.get(lang);
            let tw = kit::tab_pill_w(label_text, n);
            (kit::tab_pill_with_w(format!("ltab-{t:?}"), label_text, n, tab == t, tw).on_press(move |s: &mut State| s.load_library_tab(t)), tw)
        })
        .collect();
    let tabs_w = 8.0 + tab_items.iter().map(|(_, tw)| *tw).sum::<f32>() + 2.0 * tab_items.len().saturating_sub(1) as f32;
    let tabs = kit::tab_track("ltabs", tab_items, at);
    let input = text_input(&s.library.filter, th::BODY)
        .key("lib-search")
        .single_line()
        .grow(1.0)
        .min_w(0.0)
        .color(th::text())
        .placeholder(S::SearchCollection.get(lang), th::text3())
        .caret(p.accent, p.accent.alpha(0.30))
        .on_edit(|s: &mut State| &mut s.library.filter);
    let clear = (!q.is_empty()).then(|| kit::search_clear("lib-search-clear", lang).on_press(|s: &mut State| s.library.filter.clear()));
    let field = kit::search_field("lib-search-field", LIB_CTL_H, input, clear);
    let cw = column(w);
    let new_pl = (tab == LibTab::Playlists).then(|| {
        kit::field_icon_btn("lib-new-pl", &icons::PLUS, S::NewPlaylist.get(lang)).on_press_tx(|s: &mut State, tx: &mut rune::Tx| {
                s.ask_new_playlist(None);
                tx.focus_when_shown("name-field", false);
            })
    });
    let sort_now =
        (tab == LibTab::Tracks && s.library.tracks.get().is_some_and(|v| v.len() > 2)).then(|| (s.library.sort != TrackSort::Original).then(|| s.library.sort.name().get(lang)));
    let sort_w = sort_now.map_or(0.0, kit::sort_button_w);
    let sort_btn = sort_now.map(|now| {
        let open = s.popover_open && matches!(s.popover, Some(Popover::LibSort { .. }));
        kit::sort_button("lib-sort", s, now, open).on_press(|s: &mut State| s.open_popover(Popover::LibSort { anchor: "lib-sort".into() }))
    });
    let right_extras = (if sort_btn.is_some() { sort_w + 8.0 } else { 0.0 }) + (if new_pl.is_some() { LIB_CTL_H + 8.0 } else { 0.0 });
    let avail_field = cw - tabs_w - 12.0 - right_extras;
    let bar = if avail_field >= 140.0 {
        let right = row().gap(8.0).items_center().children(sort_btn).children(new_pl).child(field.w(avail_field.min(240.0)));
        row().key("lib-bar").w_full().gap(12.0).items_center().justify_between().child(tabs).child(right)
    } else {
        col().key("lib-bar").w_full().gap(12.0).child(tabs).child(row().w_full().gap(8.0).items_center().child(field.grow(1.0).min_w(0.0)).children(sort_btn).children(new_pl))
    };

    let head = if tab == LibTab::Tracks {
        let liked = s.library.tracks.get().filter(|v| !v.is_empty());
        let lead = match liked {
            Some(v) => {
                let secs = v.iter().filter_map(|t| t.duration_ms).sum::<u64>() / 1000;
                format!("{} · {} · {}", S::Playlist.get(lang), lang.tracks(v.len()), lang.duration(secs))
            }
            None => S::Playlist.get(lang).to_owned(),
        };
        let side = if cw >= 640.0 { HEAD_SIDE } else { HEAD_SIDE_S };
        let pic = liked_playlist_cover(s, side, HEAD_COVER_R);
        let mut meta = col()
            .key("lib-meta")
            .grow(1.0)
            .min_w(0.0)
            .child(label(S::Liked.get(lang).to_owned(), th::RELEASE, th::text()))
            .child(label(lead, th::SMALL, th::text3()).mt(HEAD_STEP_BY));
        if liked.is_some() {
            let more_on = s.popover_open && matches!(s.popover, Some(Popover::Liked { .. }));
            let buttons = row()
                .key("lib-buttons")
                .gap(8.0)
                .items_center()
                .mt(HEAD_STEP_BUTTONS)
                .child(play_button(s, "lib-play", Some(Page::Library), false, |s| {
                    let list = s.list(ListSrc::Liked);
                    s.play_all(list, false);
                }))
                .child(kit::button_group(
                    "lib-group",
                    vec![
                        kit::seg_icon("lib-shuffle", &icons::SHUFFLE, S::Shuffle.get(lang)).on_press(|s: &mut State| {
                            let list = s.list(ListSrc::Liked);
                            s.play_all(list, true);
                        }),
                        kit::seg_wave(s, "lib-wave", S::WaveByCollection.get(lang), S::LikedTracks.get(lang), Station::Collection),
                        {
                            let more = kit::seg_icon("lib-more", &icons::MORE, S::More.get(lang))
                                .checked(more_on)
                                .on_press(|s: &mut State| s.open_popover(Popover::Liked { anchor: "lib-more".into() }));
                            if more_on { more.fill(th::veil(0.09)) } else { more }
                        },
                    ],
                ));
            meta = meta.child(buttons);
        }
        let hero = if cw < 640.0 {
            col().key("lib-head").w_full().gap(18.0).child(pic).child(meta.w_full())
        } else {
            row().key("lib-head").w_full().gap(HEAD_GAP).items_center().child(pic).child(meta)
        };
        hero.mt(12.0)
    } else {
        kit::page_title(S::Collection.get(lang), "").key("lib-head-compact").mt(8.0)
    };
    let body = col().gap(if tab == LibTab::Tracks { 32.0 } else { 20.0 }).child(kit::rise(head, 0)).child(kit::rise(bar, 1));
    let (_, card) = fit(w, 144.0, GRID_GAP, 8);
    let nothing = |art: Ill| kit::empty(&p, art, S::Nothing.get(lang));
    let content = match tab {
        LibTab::Tracks => match &s.library.tracks {
            Load::Ready(v) if v.is_empty() => nothing(Ill::Shelf),
            Load::Ready(v) => {
                let sort = s.library.sort;
                let n = if q.is_empty() && sort == TrackSort::Original {
                    v.len()
                } else {
                    let key = (q, sort, v.as_ptr() as usize, v.len());
                    let mut shown = s.library.shown.borrow_mut();
                    let mut cached_key = s.library.shown_key.borrow_mut();
                    if *cached_key != key {
                        *shown = pick(v.len(), &|i| kit::track_fields(&v[i]));
                        sort.sort_indices(v, &mut shown);
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
            Load::Ready(v) => grid(pick(v.len(), &|i| vec![v[i].title.clone()]).into_iter().map(|i| playlist_card(s, &v[i], card)).collect()),
            other => kit::list_state(other, s).unwrap_or_else(el),
        },
        LibTab::Albums => match &s.library.albums {
            Load::Ready(v) if v.is_empty() => nothing(Ill::Shelf),
            Load::Ready(v) => grid(pick(v.len(), &|i| vec![v[i].title.clone(), v[i].artists_line()]).into_iter().map(|i| album_card(s, &v[i], card)).collect()),
            other => kit::list_state(other, s).unwrap_or_else(el),
        },
        LibTab::Artists => match &s.library.artists {
            Load::Ready(v) if v.is_empty() => nothing(Ill::Shelf),
            Load::Ready(v) => grid(pick(v.len(), &|i| vec![v[i].name.clone()]).into_iter().map(|i| artist_card(s, &v[i], card)).collect()),
            other => kit::list_state(other, s).unwrap_or_else(el),
        },
    };
    kit::scroll_page("library", body.child(kit::tab_body(format!("lib-tab-{tab:?}"), content)))
}
