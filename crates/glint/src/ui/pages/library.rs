//! Library page: Yandex-style Liked header, genre/mood filter pills, collection tabs, live search
//! filter, and card grids.

use std::collections::HashMap;

use rune::{Cursor, El, col, el, icon, row, text_input};
use ym_api::Track;

use super::detail::{HEAD_COVER_R, HEAD_GAP, HEAD_SIDE, HEAD_SIDE_S, HEAD_STEP_BUTTONS, HEAD_STEP_BY};
use super::shared::{GRID_GAP, album_card_lazy, artist_card_lazy, card_row_h, column, fit, playlist_card_lazy, track_list, virtual_grid};
use crate::app::{AlbumSort, ArtistSort, DiscKind, LibTab, Load, Page, PinnedItem, PlaylistSort, Popover, State, TrackSort};
use crate::i18n::{Lang, S};
use crate::theme as th;
use crate::ui::shapes::Ill;
use crate::ui::{
    icons,
    kit::{self, Hover as _, ListSrc, label},
};

const LIB_CTL_H: f32 = 36.0;
const YANDEX_YELLOW: u32 = 0xffdc4e;
const YANDEX_YELLOW_HOV: u32 = 0xffe474;

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

fn track_genre_bucket(track: &Track) -> Option<String> {
    let raw = track.albums.iter().find_map(|a| a.genre.as_deref()).filter(|g| !g.trim().is_empty())?;
    let b = crate::genre::bucket(raw);
    (!b.is_empty()).then_some(b)
}

fn track_matches_mood(track: &Track, mood: &str) -> bool {
    if let Some(p) = track.track_parameters {
        let energy = p.energy.unwrap_or(0.5);
        let bpm = p.bpm.unwrap_or(110.0);
        let hue = p.hue.unwrap_or(120.0);
        return match mood {
            "energetic" => energy >= 0.66 || bpm >= 126.0,
            "fun" => energy >= 0.50 && ((30.0..=150.0).contains(&hue) || bpm >= 112.0),
            "sad" => energy <= 0.48 && ((185.0..=295.0).contains(&hue) || bpm <= 98.0),
            "calm" => energy <= 0.42 || bpm <= 90.0,
            _ => true,
        };
    }
    let g = track.albums.iter().find_map(|a| a.genre.as_deref()).unwrap_or("").to_ascii_lowercase();
    match mood {
        "energetic" => matches!(g.as_str(), "dnb" | "dubstep" | "phonk" | "edmgenre" | "dance" | "punk" | "hardcore" | "metal" | "sport" | "techno" | "house"),
        "fun" => matches!(g.as_str(), "pop" | "ruspop" | "foreignpop" | "dance" | "disco" | "funk" | "kpop" | "hyperpop" | "eurodance"),
        "sad" => matches!(g.as_str(), "emo" | "indie" | "local-indie" | "postrock" | "blues" | "romances" | "bard" | "alternative"),
        "calm" => matches!(g.as_str(), "ambient" | "lofi" | "lounge" | "relax" | "classical" | "classicalmusic" | "jazz" | "newage" | "meditation"),
        _ => true,
    }
}

/// Evaluates whether `track` matches the selected Liked filter chip (`""`, `"mood:*"`, or `"genre:*"`).
pub(super) fn track_matches_filter(track: &Track, filter: &str) -> bool {
    if filter.is_empty() {
        return true;
    }
    if let Some(mood) = filter.strip_prefix("mood:") {
        return track_matches_mood(track, mood);
    }
    if let Some(genre) = filter.strip_prefix("genre:") {
        return track_genre_bucket(track).as_deref() == Some(genre);
    }
    true
}

/// Builds the horizontal Liked filter chips matching Yandex Music: `Всё`, `Бодрое`, top genres in
/// the user's liked tracks (`Рэп и хип-хоп`, `Поп`, `Альтернатива`, ...), plus `Весёлое`,
/// `Грустное`, `Спокойное`.
fn liked_filter_chips(tracks: &[Track], lang: Lang) -> Vec<(String, String)> {
    let mut chips: Vec<(String, String)> = vec![(String::new(), S::KindAll.get(lang).to_owned())];
    let has_energetic = tracks.iter().any(|t| track_matches_mood(t, "energetic"));
    if has_energetic {
        chips.push(("mood:energetic".into(), S::WaveEnergetic.get(lang).to_owned()));
    }
    let mut counts: HashMap<String, usize> = HashMap::new();
    for track in tracks {
        if let Some(bucket) = track_genre_bucket(track) {
            *counts.entry(bucket).or_insert(0) += 1;
        }
    }
    let mut genres: Vec<(String, usize)> = counts.into_iter().collect();
    genres.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    // Place the top 3 genres right after "Бодрое" (matching Yandex Music's bar), then the remaining
    // moods, then additional genres in the user's library.
    for (g, _) in genres.iter().take(3) {
        chips.push((format!("genre:{g}"), crate::genre::name(g, lang)));
    }
    for (mood, key) in [("fun", S::WaveFun), ("sad", S::WaveSad), ("calm", S::WaveCalm)] {
        if tracks.iter().any(|t| track_matches_mood(t, mood)) {
            chips.push((format!("mood:{mood}"), key.get(lang).to_owned()));
        }
    }
    for (g, _) in genres.iter().skip(3).take(6) {
        chips.push((format!("genre:{g}"), crate::genre::name(g, lang)));
    }
    chips
}

fn filter_bar(s: &State, tracks: &[Track]) -> Option<El> {
    let chips = liked_filter_chips(tracks, s.lang);
    if chips.len() <= 1 {
        return None;
    }
    let active = s.library.track_filter.as_str();
    let pills: Vec<El> = chips
        .into_iter()
        .map(|(id, text)| {
            let on = active == id;
            let key = if id.is_empty() { "lib-flt-all".to_owned() } else { format!("lib-flt-{id}") };
            let click_id = id;
            let mut pill = row()
                .key(key)
                .role("radio")
                .label(text.as_str())
                .checked(on)
                .h(36.0)
                .px(16.0)
                .radius(18.0)
                .center()
                .cursor(Cursor::Pointer)
                .press_scale(th::PRESS)
                .on_press(move |s: &mut State| {
                    if !click_id.is_empty() && s.library.track_filter == click_id {
                        s.library.track_filter.clear();
                    } else {
                        s.library.track_filter.clone_from(&click_id);
                    }
                })
                .child(label(text, th::BODY_M, if on { th::text() } else { th::text().alpha(0.90) }));
            if on {
                pill = pill
                    .fill(if th::light() { th::veil(0.08) } else { th::rgba(0x141414, 0.96) })
                    .border(2.0, th::rgb(YANDEX_YELLOW));
            } else {
                pill = pill.fill(th::veil(0.08)).hov(th::veil(0.14)).press(th::veil(0.18));
            }
            pill
        })
        .collect();
    Some(kit::wheel_to_page(row().key("lib-filters").w_full().gap(8.0).py(2.0).scroll_x().scroll_edge_fade(28.0).children(pills)))
}

fn liked_circle_btn(key: &'static str, ic: &'static rune::Icon, color: rune::Color, tip: &str, active: bool) -> El {
    row()
        .key(key)
        .role("button")
        .label(tip)
        .help(tip)
        .w(40.0)
        .h(40.0)
        .radius(20.0)
        .center()
        .cursor(Cursor::Pointer)
        .press_scale(th::PRESS)
        .checked(active)
        .fill(th::veil(if active { 0.14 } else { 0.08 }))
        .hov(th::veil(0.15))
        .press(th::veil(0.20))
        .child(icon(ic, 18.0).color(color))
}

fn liked_play_pill(s: &State) -> El {
    let own = s.queue_from(&Page::Library);
    let (name, ic) = match (own, s.player.playing) {
        (true, true) => (S::Pause, &icons::PAUSE),
        (true, false) => (S::Resume, &icons::PLAY),
        (false, _) => (S::Play, &icons::PLAY),
    };
    let text = name.get(s.lang);
    let ink = th::rgb(0x111111);
    row()
        .key("lib-play")
        .role("button")
        .label(text)
        .help(text)
        .h(40.0)
        .px(22.0)
        .gap(8.0)
        .radius(20.0)
        .center()
        .cursor(Cursor::Pointer)
        .press_scale(th::PRESS)
        .fill(th::rgb(YANDEX_YELLOW))
        .hov(th::rgb(YANDEX_YELLOW_HOV))
        .on_press(|s: &mut State| {
            if s.queue_from(&Page::Library) {
                s.toggle_pause();
            } else {
                let list = s.list(ListSrc::Liked);
                s.play_all(list, false);
            }
        })
        .child(icon(ic, 15.0).color(ink))
        .child(label(text, th::BODY_M, ink))
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
        .placeholder(if tab == LibTab::Tracks { S::SearchTrack } else { S::SearchCollection }.get(lang), th::text3())
        .caret(s.caret_color(s.library.filter.focused, p.accent), p.accent.alpha(0.30))
        .on_edit(|s: &mut State| &mut s.library.filter);
    let clear = (!q.is_empty()).then(|| kit::search_clear("lib-search-clear", lang).on_press(|s: &mut State| s.library.filter.clear()));
    let field = kit::search_field("lib-search-field", LIB_CTL_H, input, clear);
    let cw = column(w);
    let new_pl = (tab == LibTab::Playlists).then(|| {
        kit::field_icon_btn("lib-new-pl", &icons::PLUS, S::NewPlaylist.get(lang)).on_press(|s: &mut State| {
            s.ask_new_playlist(None);
        })
    });
    let sort_now = match tab {
        LibTab::Tracks => (s.library.tracks.get().is_some_and(|v| v.len() > 1) || s.library.sort != TrackSort::Original)
            .then(|| (s.library.sort != TrackSort::Original).then(|| s.library.sort.name().get(lang))),
        LibTab::Playlists => (s.library.playlists.get().is_some_and(|v| v.len() > 1) || s.library.playlist_sort != PlaylistSort::Recent)
            .then(|| (s.library.playlist_sort != PlaylistSort::Recent).then(|| s.library.playlist_sort.name().get(lang))),
        LibTab::Albums => (s.library.albums.get().is_some_and(|v| v.len() > 1) || s.library.album_sort != AlbumSort::Recent || s.library.album_kind != DiscKind::All).then(|| {
            if s.library.album_kind != DiscKind::All {
                Some(s.library.album_kind.name().get(lang))
            } else if s.library.album_sort != AlbumSort::Recent {
                Some(s.library.album_sort.name().get(lang))
            } else {
                None
            }
        }),
        LibTab::Artists => (s.library.artists.get().is_some_and(|v| v.len() > 1) || s.library.artist_sort != ArtistSort::Recent)
            .then(|| (s.library.artist_sort != ArtistSort::Recent).then(|| s.library.artist_sort.name().get(lang))),
    };
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

    let liked_slice = s.library.tracks.get().filter(|v| !v.is_empty());
    let head = if tab == LibTab::Tracks {
        let lead = liked_slice.map(|v| lang.tracks(v.len()));
        let side = if cw >= 640.0 { HEAD_SIDE } else { HEAD_SIDE_S };
        let pic = liked_playlist_cover(s, side, HEAD_COVER_R);
        let mut meta = col().key("lib-meta").grow(1.0).min_w(0.0).child(label(S::Liked.get(lang).to_owned(), th::RELEASE, th::text()));
        if let Some(lead) = lead {
            meta = meta.child(label(lead, th::SMALL, th::text3()).mt(HEAD_STEP_BY));
        }
        if let Some(tracks) = liked_slice {
            let pinned = s.is_page_pinned(&Page::Library);
            let more_on = s.popover_open && matches!(s.popover, Some(Popover::Liked { .. }));
            let downloaded = s.downloads_settled(tracks);
            let pin_btn = liked_circle_btn(
                "lib-pin",
                if pinned { &icons::PIN_OFF } else { &icons::PIN },
                if pinned { th::rgb(YANDEX_YELLOW) } else { th::text2() },
                if pinned { S::Unpin.get(lang) } else { S::Pin.get(lang) },
                pinned,
            )
            .on_press(|s: &mut State| {
                let title = S::Liked.get(s.lang).to_owned();
                let cover = s.library.liked_cover.clone();
                s.toggle_pin_item(PinnedItem { page: Page::Library, title, cover, artist: false });
            });
            let more_btn = liked_circle_btn("lib-more", &icons::MORE, if more_on { th::text() } else { th::text2() }, S::More.get(lang), more_on)
                .on_press(|s: &mut State| s.open_popover(Popover::Liked { anchor: "lib-more".into() }));
            let dl_btn = liked_circle_btn(
                "lib-dl",
                if downloaded { &icons::CHECK } else { &icons::DOWNLOAD },
                if downloaded { th::rgb(0x4bd27e) } else { th::text2() },
                if downloaded { S::DownloadDone.get(lang) } else { S::DownloadAllLiked.get(lang) },
                false,
            )
            .on_press(move |s: &mut State| {
                let list = s.library.tracks.get().cloned().unwrap_or_default();
                if downloaded {
                    s.ask_delete_downloads(list);
                } else {
                    s.ask_download(list);
                }
            });
            let buttons = row().key("lib-buttons").gap(10.0).items_center().mt(HEAD_STEP_BUTTONS).child(liked_play_pill(s)).child(pin_btn).child(more_btn).child(dl_btn);
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
    let chips = if tab == LibTab::Tracks { liked_slice.and_then(|v| filter_bar(s, v)) } else { None };
    let mut body = col().gap(if tab == LibTab::Tracks { 22.0 } else { 20.0 }).child(kit::rise(head, 0)).child(kit::rise(bar, 1));
    if let Some(chips) = chips {
        body = body.child(kit::rise(chips, 2));
    }
    let (cols, card) = fit(w, 144.0, GRID_GAP, 8);
    let nothing = |art: Ill| kit::empty(&p, art, S::Nothing.get(lang));
    let content = match tab {
        LibTab::Tracks => match &s.library.tracks {
            Load::Ready(v) if v.is_empty() => nothing(Ill::Shelf),
            Load::Ready(v) => {
                let sort = s.library.sort;
                let flt = s.library.track_filter.clone();
                let n = if q.is_empty() && flt.is_empty() && sort == TrackSort::Original {
                    v.len()
                } else {
                    let key = (q, flt.clone(), sort, v.as_ptr() as usize, v.len());
                    let mut shown = s.library.shown.borrow_mut();
                    let mut cached_key = s.library.shown_key.borrow_mut();
                    if *cached_key != key {
                        let mut matched = pick(v.len(), &|i| kit::track_fields(&v[i]));
                        if !flt.is_empty() {
                            matched.retain(|&i| track_matches_filter(&v[i], &flt));
                        }
                        sort.sort_indices(v, &mut matched);
                        *shown = matched;
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
            Load::Ready(v) => {
                let mut shown = pick(v.len(), &|i| vec![v[i].title.clone()]);
                s.library.playlist_sort.sort_indices(v, &mut shown);
                let shown = std::sync::Arc::new(shown);
                if shown.is_empty() {
                    nothing(Ill::Lens)
                } else {
                    let n = shown.len();
                    virtual_grid(format!("lib-pl-grid-{cols}"), n, cols, card_row_h(card, false), move |s, pos, load_art| {
                        match s.library.playlists.get().and_then(|v| shown.get(pos).and_then(|&i| v.get(i))) {
                            Some(pl) => playlist_card_lazy(s, pl, card, load_art),
                            None => el().w(card),
                        }
                    })
                }
            }
            other => kit::list_state(other, s).unwrap_or_else(el),
        },
        LibTab::Albums => match &s.library.albums {
            Load::Ready(v) if v.is_empty() => nothing(Ill::Shelf),
            Load::Ready(v) => {
                let kind = s.library.album_kind;
                let mut shown = pick(v.len(), &|i| vec![v[i].title.clone(), v[i].artists_line()]);
                if kind != DiscKind::All {
                    shown.retain(|&i| kind.holds(v[i].kind.as_deref()));
                }
                s.library.album_sort.sort_indices(v, &mut shown);
                let shown = std::sync::Arc::new(shown);
                if shown.is_empty() {
                    nothing(Ill::Lens)
                } else {
                    let n = shown.len();
                    virtual_grid(format!("lib-al-grid-{cols}"), n, cols, card_row_h(card, false), move |s, pos, load_art| {
                        match s.library.albums.get().and_then(|v| shown.get(pos).and_then(|&i| v.get(i))) {
                            Some(a) => album_card_lazy(s, a, card, load_art),
                            None => el().w(card),
                        }
                    })
                }
            }
            other => kit::list_state(other, s).unwrap_or_else(el),
        },
        LibTab::Artists => match &s.library.artists {
            Load::Ready(v) if v.is_empty() => nothing(Ill::Shelf),
            Load::Ready(v) => {
                let mut shown = pick(v.len(), &|i| vec![v[i].name.clone()]);
                s.library.artist_sort.sort_indices(v, &mut shown);
                let shown = std::sync::Arc::new(shown);
                if shown.is_empty() {
                    nothing(Ill::Lens)
                } else {
                    let n = shown.len();
                    virtual_grid(format!("lib-ar-grid-{cols}"), n, cols, card_row_h(card, true), move |s, pos, load_art| {
                        match s.library.artists.get().and_then(|v| shown.get(pos).and_then(|&i| v.get(i))) {
                            Some(a) => artist_card_lazy(s, a, card, load_art),
                            None => el().w(card),
                        }
                    })
                }
            }
            other => kit::list_state(other, s).unwrap_or_else(el),
        },
    };
    kit::scroll_page("library", body.child(kit::tab_body(format!("lib-tab-{tab:?}"), content)))
}
