//! Shared page components: horizontal carousels, card grids, media cards, virtualized track
//! lists, and My Wave helpers.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;

use rune::{Cursor, El, GrowthSpec, Tx, col, el, icon, row};
use ym_api::{Album, Artist, Playlist, ShelfItem, Track};

use crate::app::{CardMenu, Load, Page, PinnedItem, Popover, Source, State, Station};
use crate::i18n::S;
use crate::theme as th;
use crate::ui::icons;
use crate::ui::kit::{self, Hover as _, ListSrc, PAGE_MAX, PAGE_PAD, RAIL_W, ROW_H, label};

pub(super) const CARD_GAP: f32 = 20.0;
pub(super) const GRID_GAP: f32 = 16.0;
pub(super) const CARD_R: f32 = 14.0;

thread_local! {
    static ROWS: RefCell<HashMap<String, [f32; 3]>> = RefCell::new(HashMap::new());
    static GOING: RefCell<HashMap<String, f32>> = RefCell::new(HashMap::new());
    static VROWS: RefCell<HashMap<String, std::ops::Range<usize>>> = RefCell::new(HashMap::new());
}

pub(super) fn visible_row_window(key: &str, pad: usize, fallback_end: usize) -> std::ops::Range<usize> {
    VROWS.with_borrow(|m| match m.get(key) {
        Some(r) => r.start.saturating_sub(pad)..r.end.saturating_add(pad),
        None => 0..fallback_end,
    })
}

pub(super) fn record_visible_rows(key: &str, range: std::ops::Range<usize>) -> bool {
    VROWS.with_borrow_mut(|m| {
        let changed = m.get(key).is_none_or(|prev| *prev != range);
        if changed {
            m.insert(key.to_owned(), range);
        }
        changed
    })
}

fn row_at(key: &str) -> Option<[f32; 3]> {
    let [off, max, view] = ROWS.with_borrow(|m| m.get(key).copied())?;
    let off = GOING.with_borrow(|m| m.get(key).copied()).unwrap_or(off);
    Some([off, max, view])
}

pub(super) fn column(w: f32) -> f32 {
    ((w - RAIL_W).min(PAGE_MAX) - 2.0 * PAGE_PAD).max(320.0)
}

/// Visible card range `[start..end)` for a horizontal shelf, keeping a 1-card buffer on both
/// sides for smooth scrolling without decoding far-offscreen covers into RAM.
pub(super) fn hscroll_range(key: &str, n: usize, card: f32, w: f32) -> std::ops::Range<usize> {
    let pitch = (card + CARD_GAP).max(1.0);
    let avail = column(w);
    let [off, _, view] = row_at(key).unwrap_or([0.0, 0.0, avail]);
    let view = if view > 1.0 { view } else { avail };
    let first = ((off / pitch).floor() as isize - 1).max(0) as usize;
    let last = (((off + view) / pitch).ceil() as usize + 1).min(n);
    first.min(n)..last.max(first.min(n))
}

pub(super) fn hscroll(s: &State, key: String, card: f32, cards: Vec<El>) -> El {
    let watch = key.clone();
    let app = s.app.clone();
    let pitch = (card + CARD_GAP).max(1.0);
    kit::wheel_to_page(
        row()
            .key(key)
            .w_full()
            .gap(CARD_GAP)
            .py(6.0)
            .scroll_x()
            // Cards fade out at the edges while more of the row is hidden behind them.
            .scroll_edge_fade(36.0)
            .on_scroll_offset(move |p: rune::ScrollPos| {
                let mut bucket_changed = false;
                ROWS.with_borrow_mut(|m| {
                    let prev_bucket = m.get(&watch).map_or(0, |old| (old[0] / pitch).floor() as i32);
                    let next_bucket = (p.offset[0] / pitch).floor() as i32;
                    bucket_changed = prev_bucket != next_bucket;
                    m.insert(watch.clone(), [p.offset[0], p.max[0], p.viewport[0]]);
                });
                GOING.with_borrow_mut(|m| {
                    if m.get(&watch).is_some_and(|to| (to - p.offset[0]).abs() < 1.0) {
                        m.remove(&watch);
                    }
                });
                if bucket_changed
                    && let Some(app) = &app
                {
                    app.wake();
                }
            })
            .children(cards),
    )
}

pub(super) fn row_arrows(s: &State, key: &str, n: usize, card: f32, w: f32) -> Option<El> {
    let avail = (w - RAIL_W).min(PAGE_MAX) - 2.0 * PAGE_PAD;
    let content = n as f32 * card + n.saturating_sub(1) as f32 * CARD_GAP;
    if n == 0 || content <= avail + 1.0 {
        return None;
    }
    let [off, max, _] = row_at(key).unwrap_or([0.0, content - avail, avail]);
    let pitch = card + CARD_GAP;
    let arrow = |dir: f32| {
        let on = if dir < 0.0 { off > 1.0 } else { off < max - 1.0 };
        let (ic, name) = if dir < 0.0 { (&icons::CHEVRON_LEFT, S::ScrollPrev) } else { (&icons::CHEVRON_RIGHT, S::ScrollNext) };
        let row_key = key.to_owned();
        let b = row()
            .key(format!("{key}-arrow-{}", if dir < 0.0 { "prev" } else { "next" }))
            .role("button")
            .label(name.get(s.lang))
            .help(name.get(s.lang))
            .w(30.0)
            .h(30.0)
            .radius(15.0)
            .center()
            .fill(th::veil(if on { 0.06 } else { 0.035 }))
            .border(1.0, th::veil(if on { 0.07 } else { 0.035 }))
            .child(icon(ic, 15.0).color(if on { th::text() } else { th::text().alpha(0.24) }));
        if !on {
            return b.disabled(true);
        }
        b.cursor(Cursor::Pointer).hov(th::veil(0.12)).press(th::veil(0.16)).press_scale(th::PRESS).on_press_tx(move |_s: &mut State, tx: &mut Tx| {
            let [off, max, view] = row_at(&row_key).unwrap_or([0.0, f32::MAX, pitch]);
            let k = ((view + CARD_GAP) / pitch).floor().max(1.0);
            let to = (((off / pitch).round() + dir * k) * pitch).clamp(0.0, max);
            tx.scroll_to(row_key.clone(), to, 0.0);
            GOING.with_borrow_mut(|m| {
                m.insert(row_key.clone(), to);
            });
        })
    };
    Some(row().gap(6.0).items_center().child(arrow(-1.0)).child(arrow(1.0)))
}

pub(super) fn heading_side(arrows: Option<El>, more: Option<El>) -> Option<El> {
    match (arrows, more) {
        (None, m) => m,
        (a, None) => a,
        (Some(a), Some(m)) => Some(row().gap(16.0).items_center().child(m).child(a)),
    }
}

/// Fixed row height for virtualized card grids (`artist` cards have 2 text lines; albums/playlists
/// have 3 text lines).
pub(super) fn card_row_h(size: f32, artist: bool) -> f32 {
    if artist { size + 61.0 } else { size + 84.0 }
}

/// Virtualized card grid: mounts only rows near the viewport and decodes cover art only for
/// visible rows plus a 1-row scroll buffer above and below.
pub(super) fn virtual_grid<F>(key: impl Into<String>, n: usize, cols: usize, row_h: f32, build_item: F) -> El
where
    F: Fn(&State, usize, bool) -> El + 'static,
{
    let cols = cols.max(1);
    let rows = n.div_ceil(cols);
    let under = kit::under();
    let key: String = key.into();
    let row_prefix = key.clone();
    let range_key = key.clone();
    let watch_key = key.clone();
    col()
        .key(key)
        .w_full()
        .growth(GrowthSpec::default())
        .no_scroll_anchor()
        .virtual_rows(
            rows,
            row_h,
            move |_s: &State, r: usize| Cow::Owned(format!("{row_prefix}-r{r}")),
            move |s: &State, r: usize| {
                let load_art = visible_row_window(&range_key, 1, 4).contains(&r);
                let start = r * cols;
                let end = (start + cols).min(n);
                kit::built_under(under, || row().w_full().h(row_h).pb(GRID_GAP).gap(GRID_GAP).children((start..end).map(|i| build_item(s, i, load_art))))
            },
        )
        .on_visible_rows(move |_s: &mut State, range: std::ops::Range<usize>| {
            VROWS.with_borrow_mut(|m| {
                let changed = m.get(&watch_key).is_none_or(|prev| *prev != range);
                if changed {
                    m.insert(watch_key.clone(), range);
                }
                changed
            })
        })
}

pub(super) fn fit(w: f32, min: f32, gap: f32, max_cols: usize) -> (usize, f32) {
    let avail = ((w - RAIL_W).min(PAGE_MAX) - 2.0 * PAGE_PAD).max(min);
    let cols = (((avail + gap) / (min + gap)).floor() as usize).clamp(1, max_cols);
    (cols, ((avail - gap * (cols as f32 - 1.0)) / cols as f32).floor())
}

#[derive(Default)]
struct CardText {
    artists: Vec<Artist>,
    by: String,
    facts: String,
    explicit: bool,
}

type OnPlay = Option<Box<dyn Fn(&mut State)>>;

struct CardOverlay {
    page: Page,
    title: String,
    cover: Option<String>,
    like_key: Option<String>,
    link: Option<String>,
    station: Option<Station>,
}

struct CardSpec<'a> {
    key: String,
    tpl: Option<&'a str>,
    title: String,
    text: CardText,
    size: f32,
    round: bool,
    load_art: bool,
    overlay: Option<CardOverlay>,
}

fn card_circle_btn(key: String, ic: &'static rune::Icon, color: rune::Color, tip: &str, size: f32, glyph: f32) -> El {
    let r = size * 0.5;
    row()
        .key(key)
        .role("button")
        .label(tip)
        .help(tip)
        .w(size)
        .h(size)
        .radius(r)
        .center()
        .cursor(Cursor::Pointer)
        .press_scale(0.92)
        .child(el().absolute(0.0, 0.0).w_full().h_full().radius(r).fill(th::rgba(0x000000, 0.64)).hov(th::rgba(0x000000, 0.82)).gop(0.0, 1.0))
        .child(icon(ic, glyph).color(color).gop(0.0, 1.0))
}

fn media_card(s: &State, spec: CardSpec<'_>, play: OnPlay, on: impl Fn(&mut State) + 'static) -> El {
    let CardSpec { key, tpl, title, text, size, round, load_art, overlay } = spec;
    let lang = s.lang;
    let radius = if round { size * 0.5 } else { CARD_R };
    let cover_el = if load_art { kit::cover(s, tpl, size, radius) } else { kit::cover_placeholder(size, radius) };
    let mut art = el().w(size).h(size).radius(radius).child(cover_el);
    art = art.child(el().absolute(0.0, 0.0).w_full().h_full().radius(radius).fill(th::rgba(0x000000, 0.28)).gop(0.0, 1.0));
    if let Some(ov) = overlay {
        let pinned = s.is_page_pinned(&ov.page);
        let pin_item = PinnedItem { page: ov.page.clone(), title: ov.title.clone(), cover: ov.cover.clone(), artist: false };
        let pin_btn = card_circle_btn(
            format!("{key}-pin"),
            if pinned { &icons::PIN_OFF } else { &icons::PIN },
            if pinned { th::rgb(0xffdc4e) } else { th::rgb(0xffffff) },
            if pinned { S::Unpin.get(lang) } else { S::Pin.get(lang) },
            34.0,
            16.0,
        )
        .absolute(size - 42.0, 8.0)
        .on_press(move |s: &mut State| s.toggle_pin_item(pin_item.clone()));
        art = art.child(pin_btn);

        if let Some(play) = play {
            let own_playing = s.queue_from(&ov.page) && s.player.playing;
            let page_play = ov.page.clone();
            let play_tip = if own_playing { S::Pause.get(lang) } else { S::Play.get(lang) };
            let play_ic = if own_playing { &icons::PAUSE } else { &icons::PLAY };
            art = art.child(
                row()
                    .key(format!("{key}-play"))
                    .role("button")
                    .label(play_tip)
                    .help(play_tip)
                    .cursor(Cursor::Pointer)
                    .press_scale(0.92)
                    .on_press(move |s: &mut State| {
                        if s.queue_from(&page_play) {
                            s.toggle_pause();
                        } else {
                            play(s);
                        }
                    })
                    .absolute(8.0, size - 50.0)
                    .w(42.0)
                    .h(42.0)
                    .radius(21.0)
                    .center()
                    .child(el().absolute(0.0, 0.0).w_full().h_full().radius(21.0).fill(th::rgb(0xededed)).hov(th::rgb(0xffffff)).gop(0.0, 1.0))
                    .child(icon(play_ic, 18.0).color(th::rgb(0x0a0a0a)).gop(0.0, 1.0)),
            );
        }

        let has_like = ov.like_key.is_some();
        let more_key = format!("{key}-more");
        let more_anchor = more_key.clone();
        let card_menu = CardMenu { page: ov.page, title: ov.title, cover: ov.cover, like_key: ov.like_key.clone(), link: ov.link, station: ov.station };
        let more_x = if has_like { size - 82.0 } else { size - 42.0 };
        let more_btn = card_circle_btn(more_key, &icons::MORE, th::rgb(0xffffff), S::More.get(lang), 34.0, 17.0)
            .absolute(more_x, size - 44.0)
            .on_press(move |s: &mut State| s.open_popover(Popover::Card { item: card_menu.clone(), anchor: more_anchor.clone() }));
        art = art.child(more_btn);

        if let Some(like_key) = ov.like_key {
            let liked = s.is_item_liked(&like_key);
            let heart_col = if liked { s.pal().accent } else { th::rgb(0xffffff) };
            let heart_ic = if liked { &icons::HEART_FILL } else { &icons::HEART };
            let heart_tip = if liked { S::RemoveFromCollection.get(lang) } else { S::AddToCollection.get(lang) };
            let heart_btn = card_circle_btn(format!("{key}-like"), heart_ic, heart_col, heart_tip, 34.0, 16.0)
                .absolute(size - 42.0, size - 44.0)
                .on_press(move |s: &mut State| s.toggle_item_like(&like_key));
            art = art.child(heart_btn);
        }
    } else if let Some(play) = play {
        art = art.child(
            row()
                .key(format!("{key}-play"))
                .role("button")
                .label(S::Play.get(lang))
                .help(S::Play.get(lang))
                .cursor(Cursor::Pointer)
                .press_scale(0.92)
                .on_press(play)
                .absolute(8.0, size - 50.0)
                .w(42.0)
                .h(42.0)
                .radius(21.0)
                .center()
                .child(el().absolute(0.0, 0.0).w_full().h_full().radius(21.0).fill(th::rgb(0xededed)).hov(th::rgb(0xffffff)).gop(0.0, 1.0))
                .child(icon(&icons::PLAY, 18.0).color(th::rgb(0x0a0a0a)).gop(0.0, 1.0)),
        );
    }
    let reduce = s.settings.reduce_motion;
    let moving = kit::marquee_on(s, &key);
    let badge_w = if text.explicit && !round { 21.0 } else { 0.0 };
    let title_room = (size - badge_w).max(32.0);
    let name_w = rune::text::measure(&title, &[], th::CARD_NAME, None).0.ceil();
    let name = title.clone();
    let mut lines = col().w(size).min_w(0.0).gap(1.0);
    if round {
        lines = lines.items_center();
    }
    let title_el = kit::marquee(format!("{key}-name"), title_room, name_w, moving, reduce, move || label(name.clone(), th::CARD_NAME, th::text()));
    if text.explicit && !round {
        lines = lines.child(row().w(size).min_w(0.0).gap(6.0).items_center().justify_between().child(title_el).child(kit::explicit_badge(s)));
    } else {
        lines = lines.child(title_el);
    }
    if !text.artists.is_empty() {
        lines = lines.child(kit::artists_row_in_width(s, &text.artists, th::CARD_BY, false, &format!("{key}-ar"), th::text2(), size, moving));
    } else if !text.by.is_empty() {
        lines = lines.child(label(text.by, th::CARD_BY, th::text2()).truncate().max_w(size).min_w(0.0));
    }
    if !text.facts.is_empty() {
        lines = lines.child(label(text.facts, th::TIME, th::text3()).truncate().max_w(size).min_w(0.0).mt(2.0));
    }
    let mut card = col()
        .key(key.clone())
        .role("button")
        .label(title)
        .w(size)
        .gap(10.0)
        .cursor(Cursor::Pointer)
        .hov_group()
        .press_scale(0.98)
        .on_hover_change(kit::marquee_hover(key))
        .on_press(on);
    if round {
        card = card.items_center();
    }
    card.child(art).child(lines)
}

pub(super) fn facts_line(parts: impl IntoIterator<Item = String>) -> String {
    parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" · ")
}

pub(super) fn playlist_card_lazy(s: &State, pl: &Playlist, size: f32, load_art: bool) -> El {
    let (uid, kind) = (pl.owner_uid(), pl.kind);
    let lang = s.lang;
    let (owner_label, is_ym) = crate::app::playlist_owner_display(pl.owner.as_ref(), lang);
    let by = if owner_label.is_empty() { S::Playlist.get(lang).to_owned() } else { owner_label };
    let facts = if is_ym {
        String::new()
    } else {
        facts_line([
            pl.track_count.map(|n| lang.tracks(n as usize)).unwrap_or_default(),
            pl.duration_ms.filter(|ms| *ms >= 60_000).map(|ms| lang.duration(ms / 1000)).unwrap_or_default(),
        ])
    };
    let name = pl.title.clone();
    let play: OnPlay = Some(Box::new(move |s: &mut State| s.play_playlist(uid, kind, &name)));
    let page = Page::Playlist(uid, kind);
    let own = s.account.as_ref().is_some_and(|a| a.uid == uid);
    let owner_slug = pl.owner.as_ref().map(|o| o.login.as_str()).filter(|l| !l.is_empty()).map_or_else(|| uid.to_string(), str::to_owned);
    let overlay = Some(CardOverlay {
        page: page.clone(),
        title: pl.title.clone(),
        cover: pl.cover_template().map(str::to_owned),
        like_key: (!own).then(|| format!("playlist:{uid}:{kind}")),
        link: Some(format!("https://music.yandex.ru/users/{owner_slug}/playlists/{kind}")),
        station: None,
    });
    let preview = Preview { title: pl.title.clone(), cover: pl.cover_template().map(str::to_owned), artists: Vec::new(), year: None };
    let text = CardText { by, facts, ..CardText::default() };
    media_card(
        s,
        CardSpec { key: format!("pl-{}", pl.key()), tpl: pl.cover_template(), title: pl.title.clone(), text, size, round: false, load_art, overlay },
        play,
        move |s: &mut State| s.go_preview(page.clone(), &preview),
    )
}

pub(super) fn album_card_lazy(s: &State, a: &Album, size: f32, load_art: bool) -> El {
    let (id, play_id, name) = (a.id.clone(), a.id.clone(), a.title.clone());
    let had_various = a.artists.iter().any(Artist::is_various);
    let real_artists: Vec<Artist> = a.artists.iter().filter(|ar| !ar.is_various()).cloned().collect();
    let effective_kind = a.kind.as_deref().or(had_various.then_some("compilation"));
    let kind = crate::app::release_kind_name(effective_kind).get(s.lang).to_owned();
    let facts = facts_line([a.year.map(|y| y.to_string()).unwrap_or_default(), kind]);
    let play: OnPlay = Some(Box::new(move |s: &mut State| s.play_album(&play_id, &name)));
    let page = Page::Album(id.clone());
    let overlay = Some(CardOverlay {
        page: page.clone(),
        title: a.title.clone(),
        cover: a.cover_template().map(str::to_owned),
        like_key: Some(format!("album:{id}")),
        link: Some(format!("https://music.yandex.ru/album/{id}")),
        station: Some(Station::Album(id)),
    });
    let preview = Preview { title: a.title.clone(), cover: a.cover_template().map(str::to_owned), artists: real_artists.clone(), year: a.year };
    let text = CardText { artists: real_artists, facts, explicit: a.is_explicit(), ..CardText::default() };
    media_card(
        s,
        CardSpec { key: format!("al-{}", a.id), tpl: a.cover_template(), title: a.title.clone(), text, size, round: false, load_art, overlay },
        play,
        move |s: &mut State| s.go_preview(page.clone(), &preview),
    )
}

pub(super) fn artist_card_lazy(s: &State, a: &Artist, size: f32, load_art: bool) -> El {
    let id = a.id.clone();
    let preview = Preview { title: a.name.clone(), cover: a.cover_template().map(str::to_owned), artists: Vec::new(), year: None };
    let text = CardText { by: S::Artist.get(s.lang).to_owned(), ..CardText::default() };
    media_card(s, CardSpec { key: format!("ar-{}", a.id), tpl: a.cover_template(), title: a.name.clone(), text, size, round: true, load_art, overlay: None }, None, move |s: &mut State| {
        s.go_preview(Page::Artist(id.clone()), &preview)
    })
}

pub(super) fn track_card_lazy(s: &State, t: &Track, size: f32, shelf: usize, load_art: bool) -> El {
    let (id, play_id) = (t.id.clone(), t.id.clone());
    let album = t.albums.first().cloned();
    let play: OnPlay = Some(Box::new(move |s: &mut State| s.play_shelf_track(shelf, &play_id)));
    let year = album.as_ref().and_then(|a| a.year).map(|y| y.to_string()).unwrap_or_default();
    let length = t.duration_ms.filter(|ms| *ms > 0).map(crate::player::fmt_time).unwrap_or_default();
    let text = CardText { artists: t.artists.clone(), facts: facts_line([year, length]), explicit: t.is_explicit(), ..CardText::default() };
    media_card(
        s,
        CardSpec { key: format!("tr-{shelf}-{}", t.id), tpl: t.cover_template(), title: t.full_title(), text, size, round: false, load_art, overlay: None },
        play,
        move |s: &mut State| match &album {
            Some(al) => {
                let preview = Preview { title: al.title.clone(), cover: al.cover_template().map(str::to_owned), artists: al.artists.clone(), year: al.year };
                s.go_preview(Page::Album(al.id.clone()), &preview)
            }
            None => s.play_shelf_track(shelf, &id),
        },
    )
}

#[derive(Clone)]
pub(super) struct Preview {
    pub title: String,
    pub cover: Option<String>,
    pub artists: Vec<Artist>,
    pub year: Option<i32>,
}

impl State {
    pub(super) fn go_preview(&mut self, page: Page, p: &Preview) {
        self.go(page.clone());
        let d = &mut self.detail;
        if d.page.as_ref() == Some(&page) && !matches!(d.state, Load::Ready(_)) {
            if d.title.is_empty() {
                d.title.clone_from(&p.title);
            }
            if d.cover.is_none() {
                d.cover.clone_from(&p.cover);
            }
            if d.artists.is_empty() {
                d.artists.clone_from(&p.artists);
            }
            if d.year.is_none() {
                d.year = p.year;
            }
        }
    }

    fn play_shelf_track(&mut self, shelf: usize, id: &str) {
        let Load::Ready(shelves) = &self.home else { return };
        let Some(sh) = shelves.get(shelf) else { return };
        let tracks: Vec<Track> = sh.items.iter().filter_map(|i| if let ShelfItem::Track(t) = i { Some(t.clone()) } else { None }).collect();
        if let Some(i) = tracks.iter().position(|t| t.id == id) {
            self.play_queue(tracks, i, Source::Manual);
        }
    }

    pub(crate) fn play_all(&mut self, tracks: Vec<Track>, shuffle: bool) {
        if tracks.is_empty() {
            return;
        }
        if shuffle != self.player.shuffle {
            self.player.shuffle = shuffle;
            self.mark_settings_dirty();
        }
        let start = if shuffle { (self.now.to_bits() as usize) % tracks.len() } else { 0 };
        self.play_queue(tracks, start, Source::Manual);
        let src = if self.page == Page::Library {
            match self.library.tab {
                crate::app::LibTab::Downloads => ListSrc::Downloads,
                crate::app::LibTab::Dislikes => ListSrc::Dislikes,
                _ => ListSrc::Liked,
            }
        } else {
            ListSrc::Detail
        };
        self.playing_from = self.list_from(src);
    }

    pub(super) fn queue_from(&self, page: &Page) -> bool {
        self.current().is_some() && self.playing_from.as_ref().is_some_and(|f| !f.wave && f.page.as_ref() == Some(page))
    }

    fn with_list_track<R>(&self, src: ListSrc, i: usize, f: impl FnOnce(&Track) -> R) -> Option<R> {
        match src {
            ListSrc::Liked => {
                let v = self.library.tracks.get()?;
                if self.library.filter.text.trim().is_empty() && self.library.track_filter.is_empty() && self.library.sort == crate::app::TrackSort::Original {
                    v.get(i).map(f)
                } else {
                    self.library.shown.borrow().get(i).and_then(|&j| v.get(j)).map(f)
                }
            }
            ListSrc::Downloads => {
                self.ensure_downloaded_shown();
                self.library.downloaded_shown.borrow().get(i).map(f)
            }
            ListSrc::Dislikes => self.library.disliked_tracks.get()?.get(i).map(f),
            ListSrc::Detail => match self.detail_shown() {
                None => self.detail.tracks.get(i).map(f),
                Some(shown) => shown.get(i).and_then(|&j| self.detail.tracks.get(j)).map(f),
            },
            ListSrc::Search => self.search_view().and_then(|r| r.tracks.results.get(i)).map(f),
            ListSrc::Queue => self.player.queue.get(i).map(f),
            ListSrc::History => self.listening_history.get(i).map(f),
        }
    }

    fn list_id(&self, src: ListSrc, i: usize) -> String {
        self.with_list_track(src, i, |t| format!("{src:?}-{}-{i}", t.id)).unwrap_or_else(|| format!("{src:?}-{i}"))
    }
}

/// Plays what the page lists, or pauses and resumes it once it is what plays. `part` makes it a
/// part of a `kit::button_group` instead of the white button.
pub(super) fn play_button(s: &State, key: &str, page: Option<Page>, part: bool, start: fn(&mut State)) -> El {
    let own = page.as_ref().is_some_and(|p| s.queue_from(p));
    let (name, ic) = match (own, s.player.playing) {
        (true, true) => (S::Pause, &icons::PAUSE),
        (true, false) => (S::Resume, &icons::PLAY),
        (false, _) => (S::Play, &icons::PLAY),
    };
    let name = name.get(s.lang);
    let b = if part { kit::seg_button(key, name, Some(ic)) } else { kit::btn_primary(key, name, Some(ic)) };
    b.on_press(move |s: &mut State| {
        if page.as_ref().is_some_and(|p| s.queue_from(p)) {
            s.toggle_pause();
        } else {
            start(s);
        }
    })
}

pub(super) fn track_list(key: &'static str, n: usize, src: ListSrc) -> El {
    let under = kit::under();
    kit::bleed(
        col()
            .key(key)
            .growth(GrowthSpec::default())
            .no_scroll_anchor()
            .virtual_rows(
                n,
                ROW_H,
                move |s: &State, i: usize| Cow::Owned(s.list_id(src, i)),
                move |s: &State, i: usize| {
                    let load_art = visible_row_window(key, 4, 16).contains(&i);
                    s.with_list_track(src, i, |t| kit::built_under(under, || kit::track_row_lazy(s, t, i + 1, src, load_art)))
                        .unwrap_or_else(|| el().h(ROW_H))
                },
            )
            .on_visible_rows(move |_s: &mut State, range: std::ops::Range<usize>| record_visible_rows(key, range)),
    )
}

pub fn wave_title(id: &str) -> S {
    match id {
        "favorite" => S::WaveFavorite,
        "discover" => S::WaveDiscover,
        "popular" => S::WavePopular,
        "energetic" => S::WaveEnergetic,
        "calm" => S::WaveCalm,
        "fun" => S::WaveFun,
        "sad" => S::WaveSad,
        "workbackground" => S::WaveWork,
        "without-words" => S::WaveNoWords,
        "russian" => S::WaveRussian,
        "not-russian" => S::WaveForeign,
        _ => S::WaveOnYour,
    }
}

pub(super) fn start_wave_with(s: &mut State, id: &str) {
    s.wave_choice = id.to_owned();
    s.mark_settings_dirty();
    s.start_wave(id);
}

pub(super) fn wave_hue(id: &str) -> f32 {
    match id {
        "onyourwave" => 222.0,
        "favorite" => 348.0,
        "discover" => 204.0,
        "popular" => 40.0,
        "energetic" => 18.0,
        "calm" => 182.0,
        "fun" => 162.0,
        "sad" => 232.0,
        "workbackground" => 216.0,
        "without-words" => 276.0,
        "russian" => 346.0,
        "not-russian" => 152.0,
        _ => (id.bytes().fold(11u32, |a, b| a.wrapping_mul(37).wrapping_add(u32::from(b))) % 360) as f32,
    }
}
