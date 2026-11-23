//! Shared page components: horizontal carousels, card grids, media cards, virtualized track
//! lists, and My Wave helpers.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;

use rune::{Cursor, El, GrowthSpec, Transition, Tx, col, el, icon, row};
use ym_api::{Album, Artist, Playlist, ShelfItem, Track};

use crate::app::{Load, Page, Source, State};
use crate::i18n::S;
use crate::theme as th;
use crate::ui::blobs::{self, Family};
use crate::ui::icons;
use crate::ui::kit::{self, Hover as _, ListSrc, PAGE_MAX, PAGE_PAD, RAIL_W, ROW_H, label};

pub(super) const CARD_GAP: f32 = 20.0;
pub(super) const GRID_GAP: f32 = 16.0;
pub(super) const CARD_R: f32 = 14.0;

thread_local! {
    static ROWS: RefCell<HashMap<String, [f32; 3]>> = RefCell::new(HashMap::new());
    static GOING: RefCell<HashMap<String, f32>> = RefCell::new(HashMap::new());
}

fn row_at(key: &str) -> Option<[f32; 3]> {
    let [off, max, view] = ROWS.with_borrow(|m| m.get(key).copied())?;
    let off = GOING.with_borrow(|m| m.get(key).copied()).unwrap_or(off);
    Some([off, max, view])
}

pub(super) fn column(w: f32) -> f32 {
    ((w - RAIL_W).min(PAGE_MAX) - 2.0 * PAGE_PAD).max(320.0)
}

pub(super) fn hscroll(key: String, cards: Vec<El>) -> El {
    let watch = key.clone();
    kit::wheel_to_page(
        row()
            .key(key)
            .w_full()
            .gap(CARD_GAP)
            .py(6.0)
            .scroll_x()
            .on_scroll_offset(move |p: rune::ScrollPos| {
                ROWS.with_borrow_mut(|m| {
                    m.insert(watch.clone(), [p.offset[0], p.max[0], p.viewport[0]]);
                });
                GOING.with_borrow_mut(|m| {
                    if m.get(&watch).is_some_and(|to| (to - p.offset[0]).abs() < 1.0) {
                        m.remove(&watch);
                    }
                });
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

pub(super) fn grid(cards: Vec<El>) -> El {
    row().w_full().flex_wrap().gap(GRID_GAP).children(cards)
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

struct CardSpec<'a> {
    key: String,
    tpl: Option<&'a str>,
    title: String,
    text: CardText,
    size: f32,
    round: bool,
}

fn media_card(s: &State, spec: CardSpec<'_>, play: OnPlay, on: impl Fn(&mut State) + 'static) -> El {
    let CardSpec { key, tpl, title, text, size, round } = spec;
    let radius = if round { size * 0.5 } else { CARD_R };
    let mut art = el().w(size).h(size).radius(radius).child(kit::cover(s, tpl, size, radius));
    art = art.child(el().absolute(0.0, 0.0).w_full().h_full().radius(radius).fill(th::rgba(0x000000, 0.18)).gop(0.0, 1.0));
    if let Some(play) = play {
        art = art.child(
            row()
                .key(format!("{key}-play"))
                .role("button")
                .label(S::Play.get(s.lang))
                .help(S::Play.get(s.lang))
                .cursor(Cursor::Pointer)
                .press_scale(0.92)
                .on_press(play)
                .absolute(size - 50.0, size - 50.0)
                .w(40.0)
                .h(40.0)
                .radius(20.0)
                .center()
                .child(el().absolute(0.0, 0.0).w_full().h_full().radius(20.0).fill(th::rgb(0xffffff)).gop(0.0, 1.0))
                .child(icon(&icons::PLAY, 17.0).color(th::rgb(0x0a0a0a)).ml(1.0).gop(0.0, 1.0)),
        );
    }
    let reduce = s.settings.reduce_motion;
    let moving = kit::marquee_on(s, &key);
    let name_w = rune::text::measure(&title, &[], th::BODY_M, None).0.ceil();
    let name = title.clone();
    let mut lines = col().w(size).min_w(0.0).gap(1.0);
    if round {
        lines = lines.items_center();
    }
    lines = lines.child(kit::marquee(format!("{key}-name"), size, name_w, moving, reduce, move || label(name.clone(), th::BODY_M, th::text())));
    if !text.artists.is_empty() {
        lines = lines.child(kit::artists_row_in_width(s, &text.artists, th::SMALL, false, &format!("{key}-ar"), th::text2(), size, moving));
    } else if !text.by.is_empty() {
        lines = lines.child(label(text.by, th::SMALL, th::text2()).truncate().max_w(size).min_w(0.0));
    }
    if !text.facts.is_empty() || text.explicit {
        let mut facts = row().key(format!("{key}-facts")).max_w(size).min_w(0.0).mt(2.0).gap(6.0).items_center();
        if text.explicit {
            facts = facts.child(kit::explicit_badge(s));
        }
        if !text.facts.is_empty() {
            facts = facts.child(label(text.facts, th::TIME, th::text3()).truncate().min_w(0.0).shrink());
        }
        lines = lines.child(facts);
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

pub(super) fn playlist_card(s: &State, pl: &Playlist, size: f32) -> El {
    let (uid, kind) = (pl.owner_uid(), pl.kind);
    let lang = s.lang;
    let by = pl
        .owner
        .as_ref()
        .and_then(|o| o.name.clone().filter(|n| !n.is_empty()).or_else(|| Some(o.login.clone())))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| S::Playlist.get(lang).to_owned());
    let facts = facts_line([
        pl.track_count.map(|n| lang.tracks(n as usize)).unwrap_or_default(),
        pl.duration_ms.filter(|ms| *ms >= 60_000).map(|ms| lang.duration(ms / 1000)).unwrap_or_default(),
    ]);
    let name = pl.title.clone();
    let play: OnPlay = Some(Box::new(move |s: &mut State| s.play_playlist(uid, kind, &name)));
    let page = Page::Playlist(uid, kind);
    let preview = Preview { title: pl.title.clone(), cover: pl.cover_template().map(str::to_owned), artists: Vec::new(), year: None };
    let text = CardText { by, facts, ..CardText::default() };
    media_card(s, CardSpec { key: format!("pl-{}", pl.key()), tpl: pl.cover_template(), title: pl.title.clone(), text, size, round: false }, play, move |s: &mut State| {
        s.go_preview(page.clone(), &preview)
    })
}

pub(super) fn album_card(s: &State, a: &Album, size: f32) -> El {
    let (id, play_id, name) = (a.id.clone(), a.id.clone(), a.title.clone());
    let kind = crate::app::release_kind_name(a.kind.as_deref()).get(s.lang).to_owned();
    let facts = facts_line([kind, a.year.map(|y| y.to_string()).unwrap_or_default()]);
    let play: OnPlay = Some(Box::new(move |s: &mut State| s.play_album(&play_id, &name)));
    let page = Page::Album(id);
    let preview = Preview { title: a.title.clone(), cover: a.cover_template().map(str::to_owned), artists: a.artists.clone(), year: a.year };
    let text = CardText { artists: a.artists.clone(), facts, ..CardText::default() };
    media_card(s, CardSpec { key: format!("al-{}", a.id), tpl: a.cover_template(), title: a.title.clone(), text, size, round: false }, play, move |s: &mut State| {
        s.go_preview(page.clone(), &preview)
    })
}

pub(super) fn artist_card(s: &State, a: &Artist, size: f32) -> El {
    let id = a.id.clone();
    let preview = Preview { title: a.name.clone(), cover: a.cover_template().map(str::to_owned), artists: Vec::new(), year: None };
    let text = CardText { by: S::Artist.get(s.lang).to_owned(), ..CardText::default() };
    media_card(s, CardSpec { key: format!("ar-{}", a.id), tpl: a.cover_template(), title: a.name.clone(), text, size, round: true }, None, move |s: &mut State| {
        s.go_preview(Page::Artist(id.clone()), &preview)
    })
}

pub(super) fn track_card(s: &State, t: &Track, size: f32, shelf: usize) -> El {
    let (id, play_id) = (t.id.clone(), t.id.clone());
    let album = t.albums.first().cloned();
    let play: OnPlay = Some(Box::new(move |s: &mut State| s.play_shelf_track(shelf, &play_id)));
    let year = album.as_ref().and_then(|a| a.year).map(|y| y.to_string()).unwrap_or_default();
    let length = t.duration_ms.filter(|ms| *ms > 0).map(crate::player::fmt_time).unwrap_or_default();
    let text = CardText { artists: t.artists.clone(), facts: facts_line([year, length]), explicit: t.is_explicit(), ..CardText::default() };
    media_card(s, CardSpec { key: format!("tr-{shelf}-{}", t.id), tpl: t.cover_template(), title: t.full_title(), text, size, round: false }, play, move |s: &mut State| {
        match &album {
            Some(al) => {
                let preview = Preview { title: al.title.clone(), cover: al.cover_template().map(str::to_owned), artists: al.artists.clone(), year: al.year };
                s.go_preview(Page::Album(al.id.clone()), &preview)
            }
            None => s.play_shelf_track(shelf, &id),
        }
    })
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
        self.playing_from = self.list_from(if self.page == Page::Library { ListSrc::Liked } else { ListSrc::Detail });
    }

    pub(super) fn queue_from(&self, page: &Page) -> bool {
        self.current().is_some() && self.playing_from.as_ref().is_some_and(|f| !f.wave && f.page.as_ref() == Some(page))
    }

    fn list_track(&self, src: ListSrc, i: usize) -> Option<&Track> {
        match src {
            ListSrc::Liked => {
                let v = self.library.tracks.get()?;
                if self.library.filter.text.trim().is_empty() && self.library.sort == crate::app::TrackSort::Original {
                    v.get(i)
                } else {
                    self.library.shown.borrow().get(i).and_then(|&j| v.get(j))
                }
            }
            ListSrc::Detail => {
                let q = self.detail.filter.text.trim().to_lowercase();
                if q.is_empty() || !matches!(self.page, Page::Playlist(..)) {
                    self.detail.tracks.get(i)
                } else {
                    self.detail.tracks.iter().filter(|t| self.detail_track_matches(t, &q)).nth(i)
                }
            }
            ListSrc::Search => self.search_view().and_then(|r| r.tracks.results.get(i)),
            ListSrc::Queue => self.player.queue.get(i),
            ListSrc::History => self.listening_history.get(i),
        }
    }

    fn list_id(&self, src: ListSrc, i: usize) -> String {
        match self.list_track(src, i) {
            Some(t) => {
                let occ = (0..i).filter(|&j| self.list_track(src, j).is_some_and(|o| o.id == t.id)).count();
                format!("{src:?}-{}-{occ}", t.id)
            }
            None => format!("{src:?}{i}"),
        }
    }
}

pub(super) fn play_button(s: &State, key: &str, page: Option<Page>, small: bool, start: fn(&mut State)) -> El {
    let own = page.as_ref().is_some_and(|p| s.queue_from(p));
    let (name, ic) = match (own, s.player.playing) {
        (true, true) => (S::Pause, &icons::PAUSE),
        (true, false) => (S::Resume, &icons::PLAY),
        (false, _) => (S::Play, &icons::PLAY),
    };
    let name = name.get(s.lang);
    let b = if small { kit::btn_small(key, name, Some(ic)) } else { kit::btn_primary(key, name, Some(ic)) };
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
    col().key(key).w_full().growth(GrowthSpec::default()).virtual_rows(
        n,
        ROW_H,
        move |s: &State, i: usize| Cow::Owned(s.list_id(src, i)),
        move |s: &State, i: usize| match s.list_track(src, i) {
            Some(t) => kit::built_under(under, || kit::track_row(s, t, i + 1, src)),
            None => el().h(ROW_H),
        },
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

pub(super) fn wave_name_tuned(s: &State, id: &str) -> String {
    let lang = s.lang;
    let mut out = wave_title(id).get(lang).to_owned();
    if !crate::store::WaveTune::applies(id) {
        return out;
    }
    let (t, base) = (s.wave_tune(id), crate::store::WaveTune::of(id));
    let knobs = [
        (t.diversity, base.diversity, &[S::WaveAnyOne, S::WaveFavorite, S::WaveDiscover, S::WavePopular][..]),
        (t.mood, base.mood, &[S::WaveAnyMood, S::WaveEnergetic, S::WaveFun, S::WaveCalm, S::WaveSad][..]),
        (t.language, base.language, &[S::WaveAnyOne, S::WaveRussian, S::WaveForeign, S::WaveNoWords][..]),
    ];
    for (now, was, names) in knobs {
        if now != was
            && now != 0
            && let Some(n) = names.get(usize::from(now))
        {
            out.push_str(" · ");
            out.push_str(n.get(lang));
        }
    }
    out
}

pub(super) fn start_wave_with(s: &mut State, id: &str) {
    s.wave_choice = id.to_owned();
    s.mark_settings_dirty();
    s.start_wave(id);
}

pub(super) fn wave_hue(id: &str) -> f32 {
    match id {
        "favorite" => 348.0,
        "discover" => 205.0,
        "popular" => 40.0,
        "energetic" => 14.0,
        "calm" => 172.0,
        "fun" => 280.0,
        "sad" => 228.0,
        _ => (id.bytes().fold(11u32, |a, b| a.wrapping_mul(37).wrapping_add(u32::from(b))) % 360) as f32,
    }
}

pub(super) fn name_on(name: &str, family: Family, side: f32, color: rune::Color) -> El {
    let r = blobs::radius(family, side);
    let [cx, cy, rw, rh] = family.room();
    let (bw, bh) = (r * rw, r * rh);
    let mut options = vec![name.to_owned()];
    let mid = name.chars().count() / 2;
    if let Some((i, _)) = name.char_indices().filter(|(_, ch)| *ch == ' ').min_by_key(|(i, _)| name[..*i].chars().count().abs_diff(mid)) {
        options.push(format!("{}\n{}", &name[..i], &name[i + 1..]));
    }
    let probe = 20.0;
    let (text, size) = options
        .into_iter()
        .map(|t| {
            let (w, h) = rune::text::measure(&t, &[], th::name_on(probe), None);
            let size = (probe * (bw / w.max(1.0)).min(bh / h.max(1.0))).floor().clamp(11.0, 22.0);
            (t, size)
        })
        .fold((name.to_owned(), 0.0f32), |best, o| if o.1 > best.1 + 1.0 { o } else { best });
    let (mx, my) = (side * 0.5 + cx * r, side * 0.5 + cy * r);
    let half = side * 0.5;
    el().absolute(mx - half, my - half)
        .w(side)
        .h(side)
        .center()
        .transition(Transition::none())
        .child(label(text, th::name_on(size), color).align(rune::Align::Center).max_w(bw.max(side * 0.6)).rotate(family.tilt()))
}
