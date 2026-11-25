//! Search page: debounced query input, search & listening history, mood tiles, and categorized results.

use rune::{Cursor, El, TextStyle, Transition, Tx, col, el, icon, row, text_input};
use ym_api::{SearchResult, ShelfKind, WAVE_PRESETS};

use super::garden::garden_hover;
use super::home::shelf;
use super::shared::{
    GRID_GAP, album_card_lazy, artist_card_lazy, card_row_h, column, fit, heading_side, hscroll, hscroll_range, playlist_card_lazy, row_arrows, start_wave_with, track_list,
    virtual_grid, wave_hue, wave_title,
};
use crate::app::{Load, Page, SearchTab, Source, State};
use crate::i18n::{Lang, S};
use crate::theme as th;
use crate::ui::blobs::{self, Fill, Look};
use crate::ui::shapes::Ill;
use crate::ui::{
    icons,
    kit::{self, Hover as _, ListSrc, ROW_H, label},
};

const FIELD: TextStyle = TextStyle::new(15.0, 400.0).family(th::SANS).lh(20.0);
const SEARCH_H: f32 = 40.0;
const TOP_ROWS: usize = 4;
const SEARCH_TABS: [SearchTab; 5] = [SearchTab::All, SearchTab::Tracks, SearchTab::Artists, SearchTab::Albums, SearchTab::Playlists];

fn tab_name(tab: SearchTab, lang: Lang) -> &'static str {
    match tab {
        SearchTab::All => match lang {
            Lang::Ru => "Всё",
            Lang::En => "All",
        },
        SearchTab::Tracks => S::Tracks.get(lang),
        SearchTab::Albums => S::Albums.get(lang),
        SearchTab::Artists => S::Artists.get(lang),
        SearchTab::Playlists => S::Playlists.get(lang),
    }
}

pub(super) fn search(s: &State, w: f32) -> El {
    let p = s.pal();
    let lang = s.lang;
    let has_text = !s.search.edit.text.is_empty();
    let input = text_input(&s.search.edit, FIELD)
        .key("search-input")
        .single_line()
        .grow(1.0)
        .color(th::text())
        .placeholder(S::SearchHint.get(lang), th::text3())
        .caret(s.caret_color(s.search.edit.focused, p.accent), p.accent.alpha(0.30))
        .on_edit(|s: &mut State| &mut s.search.edit)
        .on_submit(|s: &mut State, _tx: &mut Tx| {
            let t = s.search.edit.text.clone();
            s.run_search(t);
            s.remember_search();
        });
    let clear = has_text.then(|| {
        kit::search_clear("search-clear", lang).on_press(|s: &mut State| {
            s.search.edit.clear();
            s.run_search(String::new());
            s.focus_search = true;
        })
    });
    let field = kit::search_field("search-field", SEARCH_H, input, clear).w_full();
    let head = col().w_full().gap(8.0).child(kit::rise(kit::page_title(S::Search.get(lang), "").mt(8.0), 0)).child(kit::rise(field, 1));
    let mut body = col().gap(20.0).child(head);
    if !has_text {
        if !s.settings.search_history.is_empty() {
            body = body.child(kit::rise(history(s), 2));
        }
        if !s.listening_history.is_empty() {
            let count = s.listening_history.len().min(8);
            body = body.child(kit::rise(
                col()
                    .gap(10.0)
                    .child(kit::heading("sh-listening-h", S::ListeningHistory.get(lang), Some(clear_link("sh-listening-clear", lang, |s: &mut State| s.clear_listening_history()))))
                    .child(track_list("sh-listening", count, ListSrc::History)),
                3,
            ));
        }
        body = body.child(suggestions(s, w));
        return kit::scroll_page("search", body);
    }

    let counts = s.search_view().map(|r| [None, Some(r.tracks.results.len()), Some(r.artists.results.len()), Some(r.albums.results.len()), Some(r.playlists.results.len())]);
    let tabs = SEARCH_TABS
        .iter()
        .enumerate()
        .map(|(i, &tab)| {
            let n = counts.and_then(|c| c[i]).filter(|n| *n > 0);
            let name = tab_name(tab, lang);
            let tw = kit::tab_pill_w(name, n);
            (kit::tab_pill_with_w(format!("stab-{tab:?}"), name, n, s.search.tab == tab, tw).on_press(move |s: &mut State| s.set_search_tab(tab)), tw)
        })
        .collect();
    let at = SEARCH_TABS.iter().position(|t| *t == s.search.tab).unwrap_or(0);
    body = body.child(kit::rise(row().w_full().child(kit::tab_track("stabs", tabs, at)), 2));

    let (cols, card) = fit(w, 144.0, GRID_GAP, 8);
    let card = card.min(196.0);
    let tab = s.search.tab;
    let kind_loading = tab != SearchTab::All && s.search.more.is_loading();
    let content: El = match (&s.search.results, s.search_view()) {
        (Load::Failed(_), None) => kit::list_state(&s.search.results, s).unwrap_or_else(el),
        (Load::Loading, None) => search_skeleton(w),
        (_, Some(r)) => {
            let empty = match tab {
                SearchTab::All => r.is_empty(),
                SearchTab::Tracks => r.tracks.results.is_empty(),
                SearchTab::Albums => r.albums.results.is_empty(),
                SearchTab::Artists => r.artists.results.is_empty(),
                SearchTab::Playlists => r.playlists.results.is_empty(),
            };
            if empty && kind_loading {
                search_skeleton(w)
            } else if empty {
                kit::empty(&p, Ill::Lens, S::NoResults.get(lang))
            } else if let Some(other) = &s.search.relaid {
                col()
                    .w_full()
                    .gap(20.0)
                    .child(label(S::ShowingFor.get(lang).replace("{}", other), th::BODY, th::text2()).key("sr-relaid").truncate())
                    .child(results(s, r, tab, w, cols, card))
            } else {
                results(s, r, tab, w, cols, card)
            }
        }
        _ => el(),
    };
    body = body.child(kit::tab_body(format!("sr-tab-{tab:?}"), content));
    kit::scroll_page("search", body)
}

fn results(s: &State, r: &SearchResult, tab: SearchTab, w: f32, cols: usize, card: f32) -> El {
    let lang = s.lang;
    let to = |tab: SearchTab| move |s: &mut State| s.set_search_tab(tab);
    let mut tb = col().w_full().gap(32.0);
    match tab {
        SearchTab::All => {
            if let Some(top) = top_row(s, r, w) {
                tb = tb.child(kit::rise(top, 3));
            }
            if !r.artists.results.is_empty() {
                let ar_size = (card * 0.86).floor();
                let n = r.artists.results.len().min(14);
                let vis = hscroll_range("sr-artists", n, ar_size, w);
                let cards: Vec<El> = r.artists.results.iter().take(n).enumerate().map(|(idx, a)| artist_card_lazy(s, a, ar_size, vis.contains(&idx))).collect();
                let more = kit::more_link("sr-artists-more", S::ShowAll.get(lang)).on_press(to(SearchTab::Artists));
                let side = heading_side(row_arrows(s, "sr-artists", cards.len(), ar_size, w), Some(more));
                tb = tb.child(kit::rise(col().gap(14.0).child(kit::heading("sr-artists-h", S::Artists.get(lang), side)).child(hscroll(s, "sr-artists".into(), ar_size, cards)), 4));
            }
            if !r.albums.results.is_empty() {
                let n = r.albums.results.len().min(14);
                let vis = hscroll_range("sr-albums", n, card, w);
                let cards: Vec<El> = r.albums.results.iter().take(n).enumerate().map(|(idx, a)| album_card_lazy(s, a, card, vis.contains(&idx))).collect();
                let more = kit::more_link("sr-albums-more", S::ShowAll.get(lang)).on_press(to(SearchTab::Albums));
                let side = heading_side(row_arrows(s, "sr-albums", cards.len(), card, w), Some(more));
                tb = tb.child(kit::rise(col().gap(14.0).child(kit::heading("sr-albums-h", S::Albums.get(lang), side)).child(hscroll(s, "sr-albums".into(), card, cards)), 5));
            }
            if !r.playlists.results.is_empty() {
                let n = r.playlists.results.len().min(14);
                let vis = hscroll_range("sr-playlists", n, card, w);
                let cards: Vec<El> = r.playlists.results.iter().take(n).enumerate().map(|(idx, a)| playlist_card_lazy(s, a, card, vis.contains(&idx))).collect();
                let more = kit::more_link("sr-playlists-more", S::ShowAll.get(lang)).on_press(to(SearchTab::Playlists));
                let side = heading_side(row_arrows(s, "sr-playlists", cards.len(), card, w), Some(more));
                tb = tb.child(kit::rise(col().gap(14.0).child(kit::heading("sr-playlists-h", S::Playlists.get(lang), side)).child(hscroll(s, "sr-playlists".into(), card, cards)), 6));
            }
        }
        SearchTab::Tracks => tb = tb.child(track_list("sr-rows", r.tracks.results.len(), ListSrc::Search)),
        SearchTab::Albums => {
            let n = r.albums.results.len().min(60);
            tb = tb.child(virtual_grid(format!("sr-al-grid-{cols}"), n, cols, card_row_h(card, false), move |s, pos, load_art| {
                s.search_view().and_then(|r| r.albums.results.get(pos)).map_or_else(el, |a| album_card_lazy(s, a, card, load_art))
            }));
        }
        SearchTab::Artists => {
            let n = r.artists.results.len().min(60);
            tb = tb.child(virtual_grid(format!("sr-ar-grid-{cols}"), n, cols, card_row_h(card, true), move |s, pos, load_art| {
                s.search_view().and_then(|r| r.artists.results.get(pos)).map_or_else(el, |a| artist_card_lazy(s, a, card, load_art))
            }));
        }
        SearchTab::Playlists => {
            let n = r.playlists.results.len().min(60);
            tb = tb.child(virtual_grid(format!("sr-pl-grid-{cols}"), n, cols, card_row_h(card, false), move |s, pos, load_art| {
                s.search_view().and_then(|r| r.playlists.results.get(pos)).map_or_else(el, |p| playlist_card_lazy(s, p, card, load_art))
            }));
        }
    }
    tb
}

fn clear_link(key: &str, lang: Lang, on: impl Fn(&mut State) + 'static) -> El {
    row()
        .key(key)
        .role("button")
        .label(S::ClearHistory.get(lang))
        .h(24.0)
        .px(8.0)
        .radius(12.0)
        .items_center()
        .cursor(Cursor::Pointer)
        .hov(th::veil(0.06))
        .press(th::veil(0.10))
        .on_press(on)
        .child(label(S::ClearHistory.get(lang).to_owned(), th::SMALL, th::text3()))
}

fn history(s: &State) -> El {
    let lang = s.lang;
    let clear = clear_link("sh-clear", lang, |s: &mut State| s.clear_search_history());
    let mut pills = row().key("sh-pills").w_full().gap(8.0).flex_wrap();
    for (i, q) in s.settings.search_history.iter().enumerate() {
        let (again, gone) = (q.clone(), q.clone());
        let forget = row()
            .key(format!("sh-x-{i}"))
            .role("button")
            .label(S::Forget.get(lang))
            .help(S::Forget.get(lang))
            .w(22.0)
            .h(22.0)
            .radius(11.0)
            .center()
            .cursor(Cursor::Pointer)
            .hov_group()
            .hov(th::veil(0.10))
            .press(th::veil(0.16))
            .on_press(move |s: &mut State| s.forget_search(&gone))
            .child(icon(&icons::CLOSE, 12.0).color(th::text3()).gcol(th::text()));
        pills = pills.child(
            row()
                .key(format!("sh-{q}"))
                .role("button")
                .label(q.as_str())
                .h(34.0)
                .pl(12.0)
                .pr(6.0)
                .gap(7.0)
                .items_center()
                .radius(10.0)
                .cursor(Cursor::Pointer)
                .press_scale(th::PRESS)
                .fill(th::veil(0.04))
                .border(1.0, th::veil(0.07))
                .hov(th::veil(0.05))
                .press(th::veil(0.09))
                .hov_group()
                .transition(kit::pop_in(0.92, 0.0, th::SOFT))
                .on_press(move |s: &mut State| s.search_again(&again))
                .child(icon(&icons::CLOCK, 14.0).color(th::text3()).gcol(th::text2()))
                .child(label(q.clone(), th::BODY, th::text2()).gcol(th::text()).truncate().max_w(240.0))
                .child(forget),
        );
    }
    let head = row().w_full().h(24.0).items_center().child(kit::caps(S::RecentSearches.get(lang), th::text3())).child(el().grow(1.0)).child(clear);
    col().key("sh").gap(10.0).child(head).child(pills)
}

fn hue_color(h: f32) -> rune::Color {
    let (s, l) = (0.75f32, 0.55f32);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c * 0.5;
    rune::Color { r: r + m, g: g + m, b: b + m, a: 1.0 }
}

fn mood_playing(s: &State, id: &str) -> bool {
    s.current().is_some() && matches!(&s.player.source, Source::Wave { preset, .. } if preset == id)
}

fn mood_tile(s: &State, id: &'static str, w: f32, h: f32) -> El {
    let on = mood_playing(s, id);
    let name = wave_title(id).get(s.lang);
    let hue = wave_hue(id);
    let seed = (id.bytes().fold(5u32, |a, b| a.wrapping_mul(33).wrapping_add(u32::from(b))) % 997) as f32 + 0.5;
    let hot = !kit::under() && s.garden_hover == Some(id);
    let side = (h * 0.78).floor();
    let tint = |a: f32| hue_color(hue).alpha(a);
    let (hint_icon, hint) = match (on, s.player.playing) {
        (true, true) => (&icons::PAUSE, S::Pause),
        (true, false) => (&icons::PLAY, S::Resume),
        (false, _) => (&icons::PLAY, S::Listen),
    };
    let art = blobs::shape(format!("wt-shape-{id}"), blobs::mood_family(id), seed, Fill::Hue(hue), Look { chosen: false, hovered: hot, order: 0 }, side, side)
        .absolute(w - side - 10.0, (h - side) * 0.5);
    row()
        .key(format!("wt-{id}"))
        .role("button")
        .label(name)
        .checked(on)
        .w(w)
        .h(h)
        .pl(14.0)
        .items_center()
        .radius(12.0)
        .cursor(Cursor::Pointer)
        .press_scale(th::PRESS)
        .fill(th::veil(if hot { 0.075 } else { 0.045 }))
        .border(1.0, tint(if on { 0.55 } else if hot { 0.28 } else { 0.15 }))
        .hov_group()
        .transition(Transition::none())
        .on_hover_change(garden_hover(id))
        .on_press(move |s: &mut State| if mood_playing(s, id) { s.toggle_pause() } else { start_wave_with(s, id) })
        .child(el().absolute(0.0, 0.0).w_full().h_full().radius(12.0).fill(tint(if on { 0.14 } else if hot { 0.095 } else { 0.055 })))
        .child(art)
        .child(
            col().gap(3.0).max_w(w - side - 30.0).child(label(name.to_owned(), th::name_on(17.0), th::text()).truncate()).child(
                row()
                    .gap(5.0)
                    .items_center()
                    .child(icon(hint_icon, 11.0).color(th::text2()).gop(0.0, 1.0))
                    .child(label(hint.get(s.lang).to_owned(), th::SMALL, th::text2()).gop(0.0, 1.0)),
            ),
        )
}

fn suggestions(s: &State, w: f32) -> El {
    let lang = s.lang;
    let cw = column(w);
    let gap = 10.0;
    let cols = if cw >= 640.0 { 4.0 } else { 2.0 };
    let tile = ((cw - gap * (cols - 1.0)) / cols).floor();
    let mut tiles = row().key("sugg-tiles").w_full().gap(gap).flex_wrap().transition(Transition::none());
    for (i, preset) in WAVE_PRESETS.iter().enumerate().take(8) {
        tiles = tiles.child(kit::rise(mood_tile(s, preset.id, tile, 64.0), 3 + i / 4));
    }
    let mut out = col()
        .key("sugg")
        .gap(32.0)
        .mt(4.0)
        .transition(Transition::none())
        .child(col().gap(10.0).child(kit::rise(kit::heading("sugg-h", S::Browse.get(lang), None), 2)).child(tiles));
    if let Load::Ready(shelves) = &s.home {
        for (n, kind) in [ShelfKind::Chart, ShelfKind::NewReleases].iter().enumerate() {
            if let Some((i, sh)) = shelves.iter().enumerate().find(|(_, sh)| sh.kind == *kind && !sh.items.is_empty()) {
                out = out.child(kit::rise(shelf(s, "sr-", i, sh, w), 5 + n));
            }
        }
    }
    out
}

/// The best match as a wide card, then the first tracks under it: both span the column, so a
/// short list never leaves a tall empty card beside it.
fn top_row(s: &State, r: &SearchResult, _w: f32) -> Option<El> {
    let lang = s.lang;
    let tracks = &r.tracks.results;
    let best = top_card(s, r);
    if best.is_none() && tracks.is_empty() {
        return None;
    }
    let mut out = col().key("sr-top").w_full().gap(28.0);
    if let Some(b) = best {
        out = out.child(col().w_full().gap(14.0).child(kit::heading("sr-best-h", S::TopResult.get(lang), None)).child(b));
    }
    if !tracks.is_empty() {
        let more = (tracks.len() > TOP_ROWS).then(|| kit::more_link("sr-tracks-more", S::ShowAll.get(lang)).on_press(|s: &mut State| s.search.tab = SearchTab::Tracks));
        let rows = tracks.iter().take(TOP_ROWS).enumerate().map(|(i, t)| kit::track_row(s, t, i + 1, ListSrc::Search)).collect::<Vec<_>>();
        out = out.child(col().key("sr-top-tracks").w_full().gap(10.0).child(kit::list_heading("sr-tracks-h", S::Tracks.get(lang), more)).child(kit::bleed(col().gap(2.0).children(rows))));
    }
    Some(out)
}

const BEST_COVER: f32 = 112.0;
const BEST_PAD: f32 = 16.0;
const BEST_R: f32 = 20.0;
const BEST_PLAY: f32 = 48.0;

fn search_skeleton(_w: f32) -> El {
    let rows = (0..4)
        .map(|i| {
            row()
                .key(format!("sr-bone-{i}"))
                .w_full()
                .h(ROW_H)
                .px(kit::ROW_PAD)
                .gap(12.0)
                .items_center()
                .child(el().w(kit::ROW_COVER).h(kit::ROW_COVER).radius(kit::ROW_COVER_R).fill(kit::bone_fill()))
                .child(col().grow(1.0).gap(6.0).child(kit::text_bone(format!("sr-bone-t-{i}"), 16.0, 12.0, 220.0 - 20.0 * (i % 3) as f32)).child(kit::text_bone(
                    format!("sr-bone-a-{i}"),
                    14.0,
                    10.0,
                    130.0,
                )))
        })
        .collect::<Vec<_>>();
    let card = row()
        .key("sr-bone-card")
        .w_full()
        .padding(BEST_PAD)
        .gap(20.0)
        .items_center()
        .radius(BEST_R)
        .fill(th::veil(0.04))
        .child(el().w(BEST_COVER).h(BEST_COVER).radius(14.0).fill(kit::bone_fill()))
        .child(col().gap(10.0).child(kit::text_bone("sr-bone-s", 14.0, 10.0, 80.0)).child(kit::text_bone("sr-bone-h", 24.0, 18.0, 240.0)).child(kit::text_bone("sr-bone-a", 14.0, 10.0, 140.0)));
    let tracks = kit::bleed(col().gap(2.0).children(rows));
    el().key("sr-loading").w_full().child(col().w_full().gap(28.0).child(card).child(tracks))
}

/// The top result: an artist whose name matches the query, or else the first track. Cover on the
/// left, what it is and its name beside it, and for a track a play button on the right.
fn top_card(s: &State, r: &SearchResult) -> Option<El> {
    let lang = s.lang;
    let q = crate::find::Query::new(s.search.relaid.as_deref().unwrap_or(&s.search.query));
    let artist = r.artists.results.first().filter(|a| q.as_ref().is_some_and(|q| q.matches(&[&a.name])));
    let base = |key: String, name: &str| {
        row()
            .key(key)
            .role("button")
            .label(name)
            .w_full()
            .padding(BEST_PAD)
            .gap(20.0)
            .items_center()
            .radius(BEST_R)
            .cursor(Cursor::Pointer)
            .fill(th::veil(0.045))
            .border(1.0, th::veil(0.08))
            .hov(th::veil(0.03))
            .press(th::veil(0.07))
            .hov_group()
    };
    let says = |kind: &str, title: String, sub: String| {
        let mut c = col().grow(1.0).min_w(0.0).gap(4.0).child(label(kind.to_owned(), th::LABEL, th::text3())).child(label(title, th::H1, th::text()).truncate().min_w(0.0));
        if !sub.is_empty() {
            c = c.child(label(sub, th::BODY, th::text2()).truncate().min_w(0.0));
        }
        c
    };
    if let Some(a) = artist {
        let id = a.id.clone();
        let card = base(format!("sr-best-ar-{}", a.id), &a.name)
            .on_press(move |s: &mut State| s.go(Page::Artist(id.clone())))
            .child(kit::cover(s, a.cover_template(), BEST_COVER, BEST_COVER * 0.5))
            .child(says(S::Artist.get(lang), a.name.clone(), String::new()))
            .child(icon(&icons::CHEVRON_RIGHT, 20.0).color(th::text3()).gcol(th::text()).mr(8.0));
        return Some(card);
    }
    let t = r.tracks.results.first()?;
    let (id, id2) = (t.id.clone(), t.id.clone());
    let title = t.full_title();
    let playing = s.current().is_some_and(|c| c.id == t.id) && s.player.playing;
    let play = row()
        .key("sr-best-play")
        .role("button")
        .label(S::Play.get(lang))
        .help(S::Play.get(lang))
        .w(BEST_PLAY)
        .h(BEST_PLAY)
        .radius(BEST_PLAY * 0.5)
        .center()
        .mr(8.0)
        .cursor(Cursor::Pointer)
        .press_scale(th::PRESS)
        .fill(th::solid())
        .on_press(move |s: &mut State| {
            if s.current().is_some_and(|c| c.id == id2) {
                s.toggle_pause();
            } else {
                s.play_from_list(ListSrc::Search, &id2);
            }
        })
        .child(icon(if playing { &icons::PAUSE } else { &icons::PLAY }, 20.0).color(th::on_solid()).ml(if playing { 0.0 } else { 2.0 }));
    let card = base(format!("sr-best-tr-{}", t.id), &title)
        .on_press(move |s: &mut State| s.play_from_list(ListSrc::Search, &id))
        .child(kit::cover(s, t.cover_template(), BEST_COVER, 14.0))
        .child(says(S::Track.get(lang), title.clone(), t.artists_line()))
        .child(play);
    Some(card)
}
