//! Collection hub ("Коллекция") and sub-views matching Yandex Music: Liked tracks preview and
//! full view with genre/mood filters, top listened artists with hours and rank trend, own/liked
//! playlists, favorite albums, downloaded tracks, and dislikes.

use std::borrow::Cow;
use std::collections::HashMap;

use rune::{Align, Cursor, El, GrowthSpec, TextStyle, Transition, col, el, icon, row, text_input};
use ym_api::{Artist, TopArtist, Track};

use super::detail::{HEAD_COVER_R, HEAD_GAP, HEAD_SIDE, HEAD_SIDE_S, HEAD_STEP_BUTTONS, HEAD_STEP_BY};
use super::shared::{
    CARD_GAP, CARD_R, GRID_GAP, album_card_lazy, artist_card_lazy, card_row_h, column, fit, hscroll, hscroll_range, playlist_card_lazy, record_visible_rows, row_arrows, track_list,
    virtual_grid, visible_row_window,
};
use crate::app::{AlbumSort, ArtistSort, DiscKind, DislikeTab, DownloadState, LibTab, Load, Page, PinnedItem, PlaylistSort, PlaylistTab, Popover, State, TrackSort};
use crate::i18n::{Lang, S};
use crate::theme as th;
use crate::ui::shapes::Ill;
use crate::ui::{
    icons,
    kit::{self, Hover as _, ListSrc, label},
};

const LIB_CTL_H: f32 = 36.0;
const COMPACT_ROW_H: f32 = 52.0;
const COMPACT_COVER: f32 = 40.0;

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

fn liked_filter_chips(tracks: &[Track], lang: Lang) -> Vec<(String, String)> {
    let mut chips: Vec<(String, String)> = vec![(String::new(), S::KindAll.get(lang).to_owned())];
    if tracks.iter().any(|t| track_matches_mood(t, "energetic")) {
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
                    s.reset_scroll_key("library");
                })
                .child(label(text, th::BODY_M, if on { th::text() } else { th::text().alpha(0.90) }));
            if on {
                pill = pill.fill(th::veil(if th::light() { 0.12 } else { 0.15 })).border(1.5, th::text().alpha(0.82));
            } else {
                pill = pill.fill(th::veil(0.08)).hov(th::veil(0.14)).press(th::veil(0.18));
            }
            pill
        })
        .collect();
    Some(kit::wheel_to_page(row().key("lib-filters").w_full().gap(8.0).py(2.0).scroll_x().scroll_edge_fade(28.0).children(pills)))
}

fn format_listen_hours(seconds: u64, lang: Lang) -> String {
    let half_hours = (seconds as f64 / 1800.0).round() as u64;
    if half_hours == 0 {
        let mins = (seconds / 60).max(1);
        return match lang {
            Lang::Ru => format!("{mins} мин"),
            Lang::En => format!("{mins} min"),
        };
    }
    let whole = half_hours / 2;
    if half_hours % 2 == 1 {
        return match lang {
            Lang::Ru => format!("{whole},5 часа"),
            Lang::En => format!("{whole}.5 h"),
        };
    }
    match lang {
        Lang::Ru => {
            let word = match (whole % 10, whole % 100) {
                (1, n) if n != 11 => "час",
                (2..=4, n) if !(12..=14).contains(&n) => "часа",
                _ => "часов",
            };
            format!("{whole} {word}")
        }
        Lang::En => format!("{whole} h"),
    }
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
    let own = s.queue_from(&Page::Library) && s.playing_from.as_ref().is_some_and(|f| f.kind == S::Liked);
    let (name, ic) = match (own, s.player.playing) {
        (true, true) => (S::Pause, &icons::PAUSE),
        (true, false) => (S::Resume, &icons::PLAY),
        (false, _) => (S::Play, &icons::PLAY),
    };
    let text = name.get(s.lang);
    let ink = th::on_solid();
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
        .fill(th::solid())
        .hov(th::solid().alpha(0.88))
        .on_press(|s: &mut State| {
            if s.queue_from(&Page::Library) && s.playing_from.as_ref().is_some_and(|f| f.kind == S::Liked) {
                s.toggle_pause();
            } else {
                let list = s.list(ListSrc::Liked);
                s.play_all(list, false);
            }
        })
        .child(icon(ic, 15.0).color(ink))
        .child(label(text, th::BODY_M, ink))
}

fn sub_back_btn(lang: Lang) -> El {
    row()
        .key("lib-sub-back")
        .role("button")
        .label(S::Back.get(lang))
        .help(S::Back.get(lang))
        .w(34.0)
        .h(34.0)
        .radius(17.0)
        .center()
        .fill(th::veil(0.08))
        .hov(th::veil(0.15))
        .press(th::veil(0.20))
        .cursor(Cursor::Pointer)
        .press_scale(th::PRESS)
        .on_press(|s: &mut State| s.load_library_tab(LibTab::Overview))
        .child(icon(&icons::CHEVRON_LEFT, 16.0).color(th::text()))
}

fn sub_title_row(lang: Lang, title: &str, subtitle: Option<String>) -> El {
    let mut text = col().grow(1.0).min_w(0.0).gap(3.0).child(label(title.to_owned(), th::TITLE, th::text()).truncate().min_w(0.0));
    if let Some(sub) = subtitle {
        text = text.child(label(sub, th::SMALL, th::text3()).truncate().min_w(0.0));
    }
    row().key("lib-sub-head").w_full().mt(8.0).gap(12.0).items_center().child(sub_back_btn(lang)).child(text)
}

fn section_chevron(s: &State, key: &'static str) -> El {
    let hovered = !kit::under() && s.lib_head_hover == Some(key);
    icon(&icons::CHEVRON_RIGHT, 17.0)
        .key(format!("{key}-chv"))
        .color(if hovered { th::text() } else { th::text3() })
        .mt(1.0)
        .offset(if hovered { 5.0 } else { 0.0 }, 0.0)
        .animated_changes()
        .motion(rune::Spring::new(0.20, 0.0))
}

fn section_link(s: &State, key: &'static str, title: &str, target: LibTab, extra_right: Option<El>) -> El {
    let btn = row()
        .key(key)
        .role("button")
        .label(title)
        .gap(6.0)
        .py(4.0)
        .pr(10.0)
        .items_center()
        .cursor(Cursor::Pointer)
        .press_scale(0.99)
        .on_hover_change(move |s: &mut State, over: bool, _tx: &mut rune::Tx| {
            if over {
                s.lib_head_hover = Some(key);
            } else if s.lib_head_hover == Some(key) {
                s.lib_head_hover = None;
            }
        })
        .on_press(move |s: &mut State| {
            s.lib_head_hover = None;
            s.load_library_tab(target);
        })
        .child(label(title.to_owned(), th::SECTION, th::text()))
        .child(section_chevron(s, key));
    let mut r = row().w_full().h(36.0).items_center().gap(12.0).child(btn).child(el().grow(1.0));
    if let Some(right) = extra_right {
        r = r.child(right);
    }
    r
}

fn small_pill(key: &'static str, text: &str, on: bool, on_press: fn(&mut State)) -> El {
    let mut b = row()
        .key(key)
        .role("tab")
        .label(text)
        .checked(on)
        .h(34.0)
        .px(14.0)
        .radius(17.0)
        .center()
        .cursor(Cursor::Pointer)
        .press_scale(th::PRESS)
        .on_press(on_press)
        .child(label(text.to_owned(), th::BODY_M, if on { th::text() } else { th::text2() }));
    b = if on {
        b.fill(th::veil(if th::light() { 0.12 } else { 0.16 }))
    } else {
        b.fill(rune::Color::TRANSPARENT).hov(th::veil(0.08))
    };
    b
}

/// Compact 52pt track row used in the 2-column × 4-row Collection previews (Liked & Downloads).
fn compact_track_row(s: &State, t: &Track, src: ListSrc, key_prefix: &str, idx: usize) -> El {
    let lang = s.lang;
    let playing = s.current().is_some_and(|c| c.id == t.id);
    let ink = if playing { s.pal().accent } else { th::text() };
    let liked = s.is_liked(&t.id);
    let dl_done = matches!(s.downloads.get(&t.id), Some(DownloadState::Done));
    let id = t.id.clone();
    let t_like = t.clone();
    let title_style = TextStyle::new(14.5, 520.0).family(th::SANS).lh(19.0);
    let artist_style = TextStyle::new(12.5, 420.0).family(th::SANS).lh(16.0);

    let play_ic = if playing && s.player.playing { &icons::PAUSE } else { &icons::PLAY };
    let cover = el()
        .w(COMPACT_COVER)
        .h(COMPACT_COVER)
        .radius(7.0)
        .child(kit::cover_sm(s, t.cover_template(), COMPACT_COVER, 7.0))
        .child(
            row()
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .radius(7.0)
                .center()
                .fill(th::rgba(0x000000, 0.45))
                .gop(if playing { 0.85 } else { 0.0 }, 1.0)
                .child(icon(play_ic, 16.0).color(th::rgb(0xffffff))),
        );

    let mut title_row = row().w_full().min_w(0.0).gap(5.0).items_center().child(label(t.full_title(), title_style, ink).truncate().min_w(0.0));
    if t.is_explicit() {
        title_row = title_row.child(kit::explicit_badge(s));
    }
    let meta = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(1.0)
        .justify_center()
        .child(title_row)
        .child(label(t.artists_line(), artist_style, th::text2()).truncate().min_w(0.0));

    let dl_mark = dl_done.then(|| {
        row()
            .w(24.0)
            .h(24.0)
            .center()
            .child(icon(&icons::DOWNLOAD, 14.0).color(th::rgb(0x4bd27e)))
    });

    let heart_col = if liked { th::text2() } else { th::text3() };
    let heart = row()
        .key(format!("{key_prefix}-heart-{idx}-{}", t.id))
        .role("button")
        .label(if liked { S::Unlike.get(lang) } else { S::Like.get(lang) })
        .w(30.0)
        .h(30.0)
        .radius(15.0)
        .center()
        .cursor(Cursor::Pointer)
        .hov(th::veil(0.08))
        .press_scale(th::PRESS)
        .on_press(move |s: &mut State| s.toggle_like(&t_like))
        .child(icon(if liked { &icons::HEART_FILL } else { &icons::HEART }, 15.0).color(heart_col));

    let dur = label(crate::player::fmt_time(t.duration_ms.unwrap_or(0)), th::TIME, th::text3()).w(40.0).align(Align::End);

    row()
        .key(format!("{key_prefix}-row-{idx}-{}", t.id))
        .role("button")
        .label(t.full_title())
        .w_full()
        .h(COMPACT_ROW_H)
        .px(6.0)
        .gap(10.0)
        .radius(10.0)
        .items_center()
        .cursor(Cursor::Pointer)
        .hov_group()
        .fill(if playing { th::veil(0.07) } else { rune::Color::TRANSPARENT })
        .hov(th::veil(0.06))
        .press(th::veil(0.10))
        .on_press(move |s: &mut State| s.play_from_list(src, &id))
        .child(cover)
        .child(meta)
        .children(dl_mark)
        .child(heart)
        .child(dur)
}

fn compact_track_grid(s: &State, tracks: &[Track], src: ListSrc, key_prefix: &str, cw: f32) -> El {
    let take_n = tracks.len().min(8);
    if cw < 660.0 || take_n <= 4 {
        let mut c = col().key(format!("{key_prefix}-grid")).w_full().gap(2.0);
        for (i, t) in tracks.iter().take(take_n).enumerate() {
            c = c.child(compact_track_row(s, t, src, key_prefix, i));
        }
        return c;
    }
    let mut left = col().grow(1.0).min_w(0.0).gap(2.0);
    let mut right = col().grow(1.0).min_w(0.0).gap(2.0);
    for (i, t) in tracks.iter().take(4).enumerate() {
        left = left.child(compact_track_row(s, t, src, key_prefix, i));
    }
    for (i, t) in tracks.iter().skip(4).take(4).enumerate() {
        right = right.child(compact_track_row(s, t, src, key_prefix, i + 4));
    }
    row().key(format!("{key_prefix}-grid")).w_full().gap(20.0).child(left).child(right)
}

/// Top listened artist card with circular portrait, rank, trend icon, artist name, and hours.
fn top_artist_card(s: &State, ta: &TopArtist, size: f32, load: bool) -> El {
    let lang = s.lang;
    let id = ta.artist.id.clone();
    let r = size * 0.5;
    let pic = if load {
        kit::cover(s, ta.artist.cover_template(), size, r)
    } else {
        kit::cover_placeholder(size, r)
    };
    let art = el()
        .w(size)
        .h(size)
        .radius(r)
        .child(pic)
        .child(el().absolute(0.0, 0.0).w_full().h_full().radius(r).fill(th::rgba(0x000000, 0.16)).gop(0.0, 1.0));

    let trend = if ta.position == 1 {
        icon(&icons::SHELF_CHART, 12.0).color(th::rgb(0xffd24c))
    } else {
        match ta.progress.as_str() {
            "up" => icon(&icons::TREND_UP, 9.5).color(th::rgb(0x36c770)),
            "down" => icon(&icons::TREND_DOWN, 9.5).color(th::rgb(0xf05252)),
            _ => label("—", th::TIME, th::text3()),
        }
    };
    let rank_row = row()
        .gap(5.0)
        .items_center()
        .justify_center()
        .child(label(ta.position.to_string(), TextStyle::new(13.0, 620.0).family(th::SANS).lh(17.0), th::text()))
        .child(trend);

    let hours = format_listen_hours(ta.seconds, lang);
    col()
        .key(format!("top-ar-{}", ta.artist.id))
        .role("button")
        .label(ta.artist.name.as_str())
        .w(size)
        .gap(10.0)
        .items_center()
        .cursor(Cursor::Pointer)
        .hov_group()
        .press_scale(0.98)
        .on_press(move |s: &mut State| s.go(Page::Artist(id.clone())))
        .child(art)
        .child(
            col()
                .w(size)
                .gap(2.0)
                .items_center()
                .child(rank_row)
                .child(label(ta.artist.name.clone(), th::CARD_NAME, th::text()).truncate().max_w(size).align(Align::Center))
                .child(label(hours, th::TIME, th::text3()).truncate().max_w(size).align(Align::Center)),
        )
}

fn new_playlist_card(lang: Lang, size: f32) -> El {
    let title = S::NewPlaylist.get(lang);
    col()
        .key("lib-new-pl-card")
        .role("button")
        .label(title)
        .w(size)
        .gap(10.0)
        .cursor(Cursor::Pointer)
        .press_scale(0.98)
        .on_press(|s: &mut State| s.ask_new_playlist(None))
        .child(
            row()
                .w(size)
                .h(size)
                .radius(CARD_R)
                .center()
                .fill(th::veil(0.08))
                .hov(th::veil(0.13))
                .press(th::veil(0.17))
                .child(icon(&icons::PLUS, 30.0).color(th::text2())),
        )
        .child(label(title.to_owned(), th::CARD_NAME, th::text()).truncate().max_w(size))
}

fn overview(s: &State, w: f32) -> El {
    let lang = s.lang;
    let cw = column(w);
    let (_, card) = fit(w, 152.0, GRID_GAP, 6);
    let card = card.min(196.0);
    let ar_card = card.min(184.0);
    let mut body = col().gap(34.0).child(kit::rise(kit::page_title(S::Collection.get(lang), "").mt(8.0), 0));

    // ---- 1. Liked tracks preview ---------------------------------------------------------------
    let liked_count = s.library.tracks.get().map(Vec::len);
    let liked_head = row()
        .key("lib-ov-liked-h")
        .role("button")
        .label(S::Liked.get(lang))
        .gap(14.0)
        .py(4.0)
        .pr(14.0)
        .items_center()
        .cursor(Cursor::Pointer)
        .press_scale(0.99)
        .on_hover_change(|s: &mut State, over: bool, _tx: &mut rune::Tx| {
            if over {
                s.lib_head_hover = Some("lib-ov-liked-h");
            } else if s.lib_head_hover == Some("lib-ov-liked-h") {
                s.lib_head_hover = None;
            }
        })
        .on_press(|s: &mut State| {
            s.lib_head_hover = None;
            s.load_library_tab(LibTab::Tracks);
        })
        .child(liked_playlist_cover(s, 52.0, 11.0))
        .child(
            col()
                .gap(2.0)
                .child(
                    row()
                        .gap(6.0)
                        .items_center()
                        .child(label(S::Liked.get(lang).to_owned(), th::SECTION, th::text()))
                        .child(section_chevron(s, "lib-ov-liked-h")),
                )
                .children(liked_count.map(|n| label(lang.tracks(n), th::SMALL, th::text3()))),
        );
    let mut liked_sec = col().key("lib-ov-liked").w_full().gap(12.0).child(row().w_full().child(liked_head));
    match &s.library.tracks {
        Load::Ready(v) if !v.is_empty() => {
            liked_sec = liked_sec.child(compact_track_grid(s, v, ListSrc::Liked, "ov-liked", cw));
        }
        Load::Loading => {
            liked_sec = liked_sec.child(kit::track_bones("ov-liked-bones", 4, true));
        }
        _ => {}
    }
    body = body.child(kit::rise(liked_sec, 1));

    // ---- 2. Favorite artists (monthly top with hours, fallback to liked artists) ---------------
    let top = s.library.top_artists.get().filter(|v| !v.is_empty());
    let liked_ar = s.library.artists.get().filter(|v| !v.is_empty());
    if let Some(list) = top {
        let row_key = "lib-ov-top-ar".to_owned();
        let vis = hscroll_range(&row_key, list.len(), ar_card, w);
        let arrows = row_arrows(s, &row_key, list.len(), ar_card, w);
        let cards: Vec<El> = list.iter().enumerate().map(|(i, ta)| top_artist_card(s, ta, ar_card, vis.contains(&i))).collect();
        body = body.child(kit::rise(
            col()
                .key("lib-ov-artists")
                .w_full()
                .gap(14.0)
                .child(section_link(s, "lib-ov-artists-h", S::FavoriteArtists.get(lang), LibTab::Artists, arrows))
                .child(hscroll(s, row_key, ar_card, cards)),
            2,
        ));
    } else if let Some(list) = liked_ar {
        let row_key = "lib-ov-liked-ar".to_owned();
        let vis = hscroll_range(&row_key, list.len(), ar_card, w);
        let arrows = row_arrows(s, &row_key, list.len(), ar_card, w);
        let cards: Vec<El> = list.iter().enumerate().map(|(i, a)| artist_card_lazy(s, a, ar_card, vis.contains(&i))).collect();
        body = body.child(kit::rise(
            col()
                .key("lib-ov-artists")
                .w_full()
                .gap(14.0)
                .child(section_link(s, "lib-ov-artists-h", S::FavoriteArtists.get(lang), LibTab::Artists, arrows))
                .child(hscroll(s, row_key, ar_card, cards)),
            2,
        ));
    } else if s.library.top_artists.is_loading() || s.library.artists.is_loading() {
        let mut bones = row().w_full().gap(CARD_GAP);
        for i in 0..5 {
            bones = bones.child(
                col()
                    .key(format!("ov-ar-bone-{i}"))
                    .w(ar_card)
                    .gap(10.0)
                    .items_center()
                    .child(el().w(ar_card).h(ar_card).radius(ar_card * 0.5).fill(kit::bone_fill()))
                    .child(el().w(ar_card * 0.55).h(10.0).radius(5.0).fill(kit::bone_fill())),
            );
        }
        body = body.child(
            col()
                .key("lib-ov-artists")
                .w_full()
                .gap(14.0)
                .child(section_link(s, "lib-ov-artists-h", S::FavoriteArtists.get(lang), LibTab::Artists, None))
                .child(bones),
        );
    }

    // ---- 3. My playlists (Created by you / You liked) ------------------------------------------
    let pl_tab = s.library.playlist_tab;
    let pl_pills = row()
        .key("lib-ov-pl-tabs")
        .gap(8.0)
        .items_center()
        .child(small_pill("lib-pl-own", S::PlaylistsCreated.get(lang), pl_tab == PlaylistTab::Own, |s| {
            s.library.playlist_tab = PlaylistTab::Own;
        }))
        .child(small_pill("lib-pl-liked", S::PlaylistsLiked.get(lang), pl_tab == PlaylistTab::Liked, |s| {
            s.library.playlist_tab = PlaylistTab::Liked;
        }));
    let pl_list = match pl_tab {
        PlaylistTab::Own => s.library.playlists.get().map(Vec::as_slice).unwrap_or(&[]),
        PlaylistTab::Liked => s.library.liked_playlists.get().map(Vec::as_slice).unwrap_or(&[]),
    };
    let pl_row_key = match pl_tab {
        PlaylistTab::Own => "lib-ov-pl-own".to_owned(),
        PlaylistTab::Liked => "lib-ov-pl-liked".to_owned(),
    };
    let extra_new = usize::from(pl_tab == PlaylistTab::Own);
    let total_pl_cards = pl_list.len() + extra_new;
    let pl_vis = hscroll_range(&pl_row_key, total_pl_cards, card, w);
    let pl_arrows = row_arrows(s, &pl_row_key, total_pl_cards, card, w);
    let mut pl_cards: Vec<El> = Vec::with_capacity(total_pl_cards);
    if pl_tab == PlaylistTab::Own {
        pl_cards.push(new_playlist_card(lang, card));
    }
    for (i, pl) in pl_list.iter().enumerate() {
        pl_cards.push(playlist_card_lazy(s, pl, card, pl_vis.contains(&(i + extra_new))));
    }
    let pl_body = if pl_cards.is_empty() {
        row().h(72.0).items_center().pl(4.0).child(label(S::Nothing.get(lang).to_owned(), th::BODY, th::text3()))
    } else {
        hscroll(s, pl_row_key, card, pl_cards)
    };
    body = body.child(kit::rise(
        col()
            .key("lib-ov-playlists")
            .w_full()
            .gap(12.0)
            .child(section_link(s, "lib-ov-pl-h", S::MyPlaylists.get(lang), LibTab::Playlists, pl_arrows))
            .child(pl_pills)
            .child(pl_body),
        3,
    ));

    // ---- 4. Favorite albums --------------------------------------------------------------------
    if let Some(albums) = s.library.albums.get().filter(|v| !v.is_empty()) {
        let row_key = "lib-ov-albums".to_owned();
        let vis = hscroll_range(&row_key, albums.len(), card, w);
        let arrows = row_arrows(s, &row_key, albums.len(), card, w);
        let cards: Vec<El> = albums.iter().enumerate().map(|(i, a)| album_card_lazy(s, a, card, vis.contains(&i))).collect();
        body = body.child(kit::rise(
            col()
                .key("lib-ov-al")
                .w_full()
                .gap(14.0)
                .child(section_link(s, "lib-ov-al-h", S::FavoriteAlbums.get(lang), LibTab::Albums, arrows))
                .child(hscroll(s, row_key, card, cards)),
            3,
        ));
    }

    // ---- 5. Downloaded tracks ------------------------------------------------------------------
    let dl_total = s
        .download_order
        .iter()
        .filter(|id| matches!(s.downloads.get(*id), Some(DownloadState::Done)) && s.downloaded_tracks.contains_key(*id))
        .count();
    if dl_total > 0 {
        let dl_preview: Vec<Track> = s
            .download_order
            .iter()
            .filter(|id| matches!(s.downloads.get(*id), Some(DownloadState::Done)))
            .filter_map(|id| s.downloaded_tracks.get(id))
            .take(8)
            .cloned()
            .collect();
        let dl_head = col()
            .gap(2.0)
            .child(section_link(s, "lib-ov-dl-h", S::DownloadedTracks.get(lang), LibTab::Downloads, None))
            .child(label(lang.tracks(dl_total), th::SMALL, th::text3()));
        body = body.child(kit::rise(
            col()
                .key("lib-ov-downloads")
                .w_full()
                .gap(10.0)
                .child(dl_head)
                .child(compact_track_grid(s, &dl_preview, ListSrc::Downloads, "ov-dl", cw)),
            3,
        ));
    }

    // ---- 6. My dislikes ------------------------------------------------------------------------
    body = body.child(kit::rise(
        section_link(s, "lib-ov-dislikes-h", S::MyDislikes.get(lang), LibTab::Dislikes, None),
        3,
    ));

    kit::scroll_page_live("library", body)
}

fn disliked_track_row(s: &State, t: &Track, idx: usize, load_art: bool) -> El {
    let lang = s.lang;
    let playing = s.current().is_some_and(|c| c.id == t.id);
    let ink = if playing { s.pal().accent } else { th::text() };
    let dl_done = matches!(s.downloads.get(&t.id), Some(DownloadState::Done));
    let id = t.id.clone();
    let t_rm = t.clone();
    let cover = if load_art {
        kit::cover_sm(s, t.cover_template(), kit::ROW_COVER, kit::ROW_COVER_R)
    } else {
        kit::cover_placeholder(kit::ROW_COVER, kit::ROW_COVER_R)
    };
    let mut title_row = row().w_full().min_w(0.0).gap(6.0).items_center().child(label(t.full_title(), th::BODY_M, ink).truncate().min_w(0.0));
    if t.is_explicit() {
        title_row = title_row.child(kit::explicit_badge(s));
    }
    let meta = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(2.0)
        .justify_center()
        .child(title_row)
        .child(label(t.artists_line(), th::SMALL, th::text2()).truncate().min_w(0.0));
    let dl_mark = dl_done.then(|| row().w(28.0).h(28.0).center().child(icon(&icons::DOWNLOAD, 15.0).color(th::rgb(0x4bd27e))));
    let remove = row()
        .key(format!("dis-rm-t-{idx}-{}", t.id))
        .role("button")
        .label(S::RemoveDislike.get(lang))
        .help(S::RemoveDislike.get(lang))
        .w(34.0)
        .h(34.0)
        .radius(17.0)
        .center()
        .cursor(Cursor::Pointer)
        .hov(th::veil(0.10))
        .press_scale(th::PRESS)
        .on_press(move |s: &mut State| s.undislike_track(&t_rm))
        .child(icon(&icons::CLOSE, 15.0).color(th::text2()));
    let dur = label(crate::player::fmt_time(t.duration_ms.unwrap_or(0)), th::TIME, th::text3()).w(44.0).align(Align::End);
    row()
        .key(format!("dis-t-{idx}-{}", t.id))
        .role("button")
        .label(t.full_title())
        .w_full()
        .h(kit::ROW_H)
        .px(kit::ROW_PAD)
        .gap(14.0)
        .radius(12.0)
        .items_center()
        .cursor(Cursor::Pointer)
        .fill(if playing { th::veil(0.07) } else { rune::Color::TRANSPARENT })
        .hov(th::veil(0.06))
        .press(th::veil(0.10))
        .on_press(move |s: &mut State| s.play_from_list(ListSrc::Dislikes, &id))
        .child(cover)
        .child(meta)
        .children(dl_mark)
        .child(remove)
        .child(dur)
}

fn disliked_artist_row(s: &State, a: &Artist, idx: usize, load_art: bool) -> El {
    let lang = s.lang;
    let id = a.id.clone();
    let a_rm = a.clone();
    let cover = if load_art {
        kit::cover_sm(s, a.cover_template(), kit::ROW_COVER, kit::ROW_COVER * 0.5)
    } else {
        kit::cover_placeholder(kit::ROW_COVER, kit::ROW_COVER * 0.5)
    };
    let remove = row()
        .key(format!("dis-rm-a-{idx}-{}", a.id))
        .role("button")
        .label(S::RemoveDislike.get(lang))
        .help(S::RemoveDislike.get(lang))
        .w(34.0)
        .h(34.0)
        .radius(17.0)
        .center()
        .cursor(Cursor::Pointer)
        .hov(th::veil(0.10))
        .press_scale(th::PRESS)
        .on_press(move |s: &mut State| s.undislike_artist(&a_rm))
        .child(icon(&icons::CLOSE, 15.0).color(th::text2()));
    row()
        .key(format!("dis-a-{idx}-{}", a.id))
        .role("button")
        .label(a.name.as_str())
        .w_full()
        .h(kit::ROW_H)
        .px(kit::ROW_PAD)
        .gap(14.0)
        .radius(12.0)
        .items_center()
        .cursor(Cursor::Pointer)
        .hov(th::veil(0.06))
        .press(th::veil(0.10))
        .on_press(move |s: &mut State| s.go(Page::Artist(id.clone())))
        .child(cover)
        .child(label(a.name.clone(), th::BODY_M, th::text()).grow(1.0).min_w(0.0).truncate())
        .child(remove)
}

fn dislikes_view(s: &State) -> El {
    let lang = s.lang;
    let p = s.pal();
    let dtab = s.library.dislike_tab;
    let t_count = s.library.disliked_tracks.get().map(Vec::len);
    let a_count = s.library.disliked_artists.get().map(Vec::len);
    let t_label = match t_count {
        Some(n) => format!("{} · {n}", S::Tracks.get(lang)),
        None => S::Tracks.get(lang).to_owned(),
    };
    let a_label = match a_count {
        Some(n) => format!("{} · {n}", S::Artists.get(lang)),
        None => S::Artists.get(lang).to_owned(),
    };
    let tabs = row()
        .key("lib-dis-tabs")
        .gap(8.0)
        .items_center()
        .child(small_pill("lib-dis-tracks", &t_label, dtab == DislikeTab::Tracks, |s| {
            s.library.dislike_tab = DislikeTab::Tracks;
            s.reset_scroll_key("library");
        }))
        .child(small_pill("lib-dis-artists", &a_label, dtab == DislikeTab::Artists, |s| {
            s.library.dislike_tab = DislikeTab::Artists;
            s.reset_scroll_key("library");
        }));
    let content = match dtab {
        DislikeTab::Tracks => match &s.library.disliked_tracks {
            Load::Ready(v) if v.is_empty() => kit::empty(&p, Ill::Shelf, S::Nothing.get(lang)),
            Load::Ready(v) => {
                let n = v.len();
                let key = "lib-dis-t-rows";
                kit::bleed(
                    col()
                        .key(key)
                        .growth(GrowthSpec::default())
                        .no_scroll_anchor()
                        .virtual_rows(
                            n,
                            kit::ROW_H,
                            move |s: &State, i: usize| {
                                let id = s.library.disliked_tracks.get().and_then(|v| v.get(i)).map_or("", |t| t.id.as_str());
                                Cow::Owned(format!("dis-t-{id}-{i}"))
                            },
                            move |s: &State, i: usize| {
                                let load = visible_row_window(key, 4, 16).contains(&i);
                                match s.library.disliked_tracks.get().and_then(|v| v.get(i)) {
                                    Some(t) => disliked_track_row(s, t, i, load),
                                    None => el().h(kit::ROW_H),
                                }
                            },
                        )
                        .on_visible_rows(move |_s: &mut State, range: std::ops::Range<usize>| record_visible_rows(key, range)),
                )
            }
            other => kit::list_state(other, s).unwrap_or_else(|| kit::track_bones("lib-dis-bones", 8, true)),
        },
        DislikeTab::Artists => match &s.library.disliked_artists {
            Load::Ready(v) if v.is_empty() => kit::empty(&p, Ill::Shelf, S::Nothing.get(lang)),
            Load::Ready(v) => {
                let n = v.len();
                let key = "lib-dis-a-rows";
                kit::bleed(
                    col()
                        .key(key)
                        .growth(GrowthSpec::default())
                        .no_scroll_anchor()
                        .virtual_rows(
                            n,
                            kit::ROW_H,
                            move |s: &State, i: usize| {
                                let id = s.library.disliked_artists.get().and_then(|v| v.get(i)).map_or("", |a| a.id.as_str());
                                Cow::Owned(format!("dis-a-{id}-{i}"))
                            },
                            move |s: &State, i: usize| {
                                let load = visible_row_window(key, 4, 16).contains(&i);
                                match s.library.disliked_artists.get().and_then(|v| v.get(i)) {
                                    Some(a) => disliked_artist_row(s, a, i, load),
                                    None => el().h(kit::ROW_H),
                                }
                            },
                        )
                        .on_visible_rows(move |_s: &mut State, range: std::ops::Range<usize>| record_visible_rows(key, range)),
                )
            }
            other => kit::list_state(other, s).unwrap_or_else(|| kit::track_bones("lib-dis-a-bones", 5, true)),
        },
    };
    let body = col()
        .gap(20.0)
        .child(kit::rise(sub_title_row(lang, S::MyDislikes.get(lang), None), 0))
        .child(kit::rise(tabs, 1))
        .child(content);
    kit::scroll_page_live("library", body)
}

fn downloads_view(s: &State) -> El {
    let lang = s.lang;
    let p = s.pal();
    let total_done = s.download_order.iter().filter(|id| matches!(s.downloads.get(*id), Some(DownloadState::Done)) && s.downloaded_tracks.contains_key(*id)).count();
    let shown_n = s.ensure_downloaded_shown();
    let q = s.library.filter.text.trim().to_owned();
    let input = text_input(&s.library.filter, th::BODY)
        .key("lib-dl-search")
        .single_line()
        .grow(1.0)
        .min_w(0.0)
        .color(th::text())
        .placeholder(S::SearchTrack.get(lang), th::text3())
        .caret(s.caret_color(s.library.filter.focused, p.accent), p.accent.alpha(0.30))
        .on_edit(|s: &mut State| &mut s.library.filter);
    let clear = (!q.is_empty()).then(|| kit::search_clear("lib-dl-search-clear", lang).on_press(|s: &mut State| s.library.filter.clear()));
    let field = kit::search_field("lib-dl-search-field", LIB_CTL_H, input, clear);
    let mut actions = row().key("lib-dl-bar").w_full().gap(10.0).items_center();
    if total_done > 0 {
        let play_btn = kit::btn_primary("lib-dl-play", S::Play.get(lang), Some(&icons::PLAY)).on_press(|s: &mut State| {
            let list = s.list(ListSrc::Downloads);
            s.play_all(list, false);
        });
        let shuffle_btn = liked_circle_btn("lib-dl-shuffle", &icons::SHUFFLE, th::text2(), S::Shuffle.get(lang), false).on_press(|s: &mut State| {
            let list = s.list(ListSrc::Downloads);
            s.play_all(list, true);
        });
        let clear_btn = liked_circle_btn("lib-dl-delete", &icons::TRASH, th::text2(), S::DeleteContent.get(lang), false).on_press(|s: &mut State| {
            let list = s.downloaded_list();
            s.ask_delete_downloads(list);
        });
        actions = actions.child(play_btn).child(shuffle_btn).child(clear_btn).child(el().grow(1.0)).child(field.w(240.0));
    }
    let content = if total_done == 0 {
        kit::empty(&p, Ill::Shelf, S::Nothing.get(lang))
    } else if shown_n == 0 {
        kit::empty(&p, Ill::Lens, S::Nothing.get(lang))
    } else {
        track_list("lib-dl-rows", shown_n, ListSrc::Downloads)
    };
    let body = col()
        .gap(20.0)
        .child(kit::rise(sub_title_row(lang, S::DownloadedTracks.get(lang), Some(lang.tracks(total_done))), 0))
        .child(kit::rise(actions, 1))
        .child(content);
    kit::scroll_page_live("library", body)
}

pub(super) fn library(s: &State, w: f32) -> El {
    let tab = s.library.tab;
    match tab {
        LibTab::Overview => return overview(s, w),
        LibTab::Dislikes => return dislikes_view(s),
        LibTab::Downloads => return downloads_view(s),
        LibTab::Tracks | LibTab::Playlists | LibTab::Albums | LibTab::Artists => {}
    }

    let p = s.pal();
    let lang = s.lang;
    let cw = column(w);
    let q = s.library.filter.text.trim().to_owned();
    let query = crate::find::Query::new(&q);
    let pick = |n: usize, fields: &dyn Fn(usize) -> Vec<String>| -> Vec<usize> {
        match &query {
            None => (0..n).collect(),
            Some(qr) => crate::find::best((0..n).map(|i| {
                let f = fields(i);
                qr.tier(&f.iter().map(String::as_str).collect::<Vec<_>>())
            })),
        }
    };

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
    let new_pl = (tab == LibTab::Playlists).then(|| {
        kit::field_icon_btn("lib-new-pl", &icons::PLUS, S::NewPlaylist.get(lang)).on_press(|s: &mut State| {
            s.ask_new_playlist(None);
        })
    });
    let active_playlists = match s.library.playlist_tab {
        PlaylistTab::Own => &s.library.playlists,
        PlaylistTab::Liked => &s.library.liked_playlists,
    };
    let sort_now = match tab {
        LibTab::Tracks => (s.library.tracks.get().is_some_and(|v| v.len() > 1) || s.library.sort != TrackSort::Original)
            .then(|| (s.library.sort != TrackSort::Original).then(|| s.library.sort.name().get(lang))),
        LibTab::Playlists => (active_playlists.get().is_some_and(|v| v.len() > 1) || s.library.playlist_sort != PlaylistSort::Recent)
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
        LibTab::Overview | LibTab::Downloads | LibTab::Dislikes => None,
    };
    let sort_btn = sort_now.map(|now| {
        let open = s.popover_open && matches!(s.popover, Some(Popover::LibSort { .. }));
        kit::sort_button("lib-sort", s, now, open).on_press(|s: &mut State| s.open_popover(Popover::LibSort { anchor: "lib-sort".into() }))
    });

    let liked_slice = s.library.tracks.get().filter(|v| !v.is_empty());
    let head = if tab == LibTab::Tracks {
        let lead = liked_slice.map(|v| lang.tracks(v.len()));
        let side = if cw >= 640.0 { HEAD_SIDE } else { HEAD_SIDE_S };
        let pic = liked_playlist_cover(s, side, HEAD_COVER_R);
        let mut meta = col().key("lib-meta").grow(1.0).min_w(0.0).child(label(S::Liked.get(lang).to_owned(), th::RELEASE, th::text()).truncate().min_w(0.0));
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
                if pinned { th::text() } else { th::text2() },
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
        let title = match tab {
            LibTab::Playlists => S::MyPlaylists,
            LibTab::Albums => S::FavoriteAlbums,
            LibTab::Artists => S::FavoriteArtists,
            _ => S::Collection,
        };
        sub_title_row(lang, title.get(lang), None)
    };

    let bar = if tab == LibTab::Playlists {
        let pl_tab = s.library.playlist_tab;
        let pills = row()
            .gap(8.0)
            .items_center()
            .child(small_pill("lib-sub-pl-own", S::PlaylistsCreated.get(lang), pl_tab == PlaylistTab::Own, |s| {
                s.library.playlist_tab = PlaylistTab::Own;
            }))
            .child(small_pill("lib-sub-pl-liked", S::PlaylistsLiked.get(lang), pl_tab == PlaylistTab::Liked, |s| {
                s.library.playlist_tab = PlaylistTab::Liked;
            }));
        let right = row().gap(8.0).items_center().children(sort_btn).children(new_pl).child(field.w(240.0));
        row().key("lib-bar").w_full().gap(12.0).items_center().justify_between().child(pills).child(right)
    } else {
        row().key("lib-bar").w_full().gap(8.0).items_center().child(field.grow(1.0).min_w(0.0).max_w(320.0)).child(el().grow(1.0)).children(sort_btn).children(new_pl)
    };

    let chips = if tab == LibTab::Tracks { liked_slice.and_then(|v| filter_bar(s, v)) } else { None };
    let mut body = col().gap(20.0).child(kit::rise(head, 0)).child(kit::rise(bar, 1));
    if let Some(chips) = chips {
        body = body.child(kit::rise(chips, 2));
    }

    // On the Favorite Artists sub-view, show the monthly top-hours strip above all liked artists.
    if tab == LibTab::Artists
        && q.is_empty()
        && let Some(top) = s.library.top_artists.get().filter(|v| !v.is_empty())
    {
        let (_, ar_card) = fit(w, 152.0, GRID_GAP, 6);
        let ar_card = ar_card.min(184.0);
        let row_key = "lib-sub-top-ar".to_owned();
        let vis = hscroll_range(&row_key, top.len(), ar_card, w);
        let arrows = row_arrows(s, &row_key, top.len(), ar_card, w);
        let cards: Vec<El> = top.iter().enumerate().map(|(i, ta)| top_artist_card(s, ta, ar_card, vis.contains(&i))).collect();
        body = body.child(
            col()
                .key("lib-sub-top")
                .w_full()
                .gap(12.0)
                .transition(Transition::none())
                .child(kit::heading("lib-sub-top-h", S::FavoriteArtists.get(lang), arrows))
                .child(hscroll(s, row_key, ar_card, cards)),
        );
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
        LibTab::Playlists => match active_playlists {
            Load::Ready(v) if v.is_empty() => nothing(Ill::Shelf),
            Load::Ready(v) => {
                let mut shown = pick(v.len(), &|i| vec![v[i].title.clone()]);
                s.library.playlist_sort.sort_indices(v, &mut shown);
                if shown.is_empty() {
                    nothing(Ill::Lens)
                } else {
                    let row_h = card_row_h(card, false);
                    let list = v.clone();
                    virtual_grid("lib-pl-grid", shown.len(), cols, row_h, move |s, i, load| playlist_card_lazy(s, &list[shown[i]], card, load))
                }
            }
            other => kit::list_state(other, s).unwrap_or_else(el),
        },
        LibTab::Albums => match &s.library.albums {
            Load::Ready(v) if v.is_empty() => nothing(Ill::Shelf),
            Load::Ready(v) => {
                let kind = s.library.album_kind;
                let mut shown: Vec<usize> = pick(v.len(), &|i| vec![v[i].title.clone(), v[i].artists_line()])
                    .into_iter()
                    .filter(|&i| kind.holds(v[i].kind.as_deref()))
                    .collect();
                s.library.album_sort.sort_indices(v, &mut shown);
                if shown.is_empty() {
                    nothing(Ill::Lens)
                } else {
                    let row_h = card_row_h(card, false);
                    let list = v.clone();
                    virtual_grid("lib-al-grid", shown.len(), cols, row_h, move |s, i, load| album_card_lazy(s, &list[shown[i]], card, load))
                }
            }
            other => kit::list_state(other, s).unwrap_or_else(el),
        },
        LibTab::Artists => match &s.library.artists {
            Load::Ready(v) if v.is_empty() => nothing(Ill::Shelf),
            Load::Ready(v) => {
                let mut shown = pick(v.len(), &|i| vec![v[i].name.clone()]);
                s.library.artist_sort.sort_indices(v, &mut shown);
                if shown.is_empty() {
                    nothing(Ill::Lens)
                } else {
                    let row_h = card_row_h(card, true);
                    let list = v.clone();
                    virtual_grid("lib-ar-grid", shown.len(), cols, row_h, move |s, i, load| artist_card_lazy(s, &list[shown[i]], card, load))
                }
            }
            other => kit::list_state(other, s).unwrap_or_else(el),
        },
        LibTab::Overview | LibTab::Downloads | LibTab::Dislikes => el(),
    };
    kit::scroll_page_live("library", body.child(content))
}
