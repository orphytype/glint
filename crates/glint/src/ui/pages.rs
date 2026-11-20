//! Home, Search, Library and the album / playlist / artist page.

use super::kit::Hover as _;
use crate::fx::Slot as _;
use std::borrow::Cow;

use rune::{Cursor, El, GrowthSpec, TextStyle, Transition, Tx, col, el, icon, row, text_input};
use ym_api::{Album, Artist, Playlist, Shelf, ShelfItem, ShelfKind, Track, WAVE_PRESETS};

use super::blobs::{self, Family, Fill, Look, Rng};
use super::icons;
use super::kit::{self, ListSrc, PAGE_MAX, PAGE_PAD, RAIL_W, ROW_H, label};
use super::shapes::Ill;
use crate::app::{LibTab, Load, Page, SearchTab, Sheet, Source, State};
use crate::i18n::S;
use crate::theme::{self as th, Pal};

/// The search field's type: big, light, no box.
const FIELD: TextStyle = TextStyle::new(15.0, 400.0).family(th::SANS).lh(20.0);
/// The search field's height (the page's), and the collection's (as tall as the tabs beside it).
const SEARCH_H: f32 = 44.0;
const LIB_CTL_H: f32 = 36.0;

pub fn page(s: &State, w: f32, h: f32) -> El {
    match &s.page {
        Page::Home | Page::Settings => home(s, w, h),
        Page::Search => search(s, w),
        Page::Library => library(s, w),
        Page::Album(_) | Page::Playlist(..) | Page::Artist(_) => detail(s, w),
    }
}

// ---- shared pieces -----------------------------------------------------------------------------------

thread_local! {
    /// Where each row of cards is scrolled to, `[offset, max, viewport]` in points, as RUNE last
    /// reported it (it cannot change state from there, so the arrows read it when pressed).
    static ROWS: std::cell::RefCell<std::collections::HashMap<String, [f32; 3]>> = std::cell::RefCell::new(std::collections::HashMap::new());
    /// Where an arrow sent each row, until it gets there: the view runs while the row glides
    /// (a pointer moving, say) and should show the arrows as they will be, not as they are midway.
    static GOING: std::cell::RefCell<std::collections::HashMap<String, f32>> = std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Where row `key` is, or is on its way to: `[offset, max, viewport]`.
fn row_at(key: &str) -> Option<[f32; 3]> {
    let [off, max, view] = ROWS.with_borrow(|m| m.get(key).copied())?;
    let off = GOING.with_borrow(|m| m.get(key).copied()).unwrap_or(off);
    Some([off, max, view])
}

/// The gap between the cards of a row.
const CARD_GAP: f32 = 20.0;

/// A horizontally scrolling row of cards.
pub(crate) fn hscroll(key: String, cards: Vec<El>) -> El {
    let watch = key.clone();
    kit::wheel_to_page(
        row()
            .key(key)
            .w_full()
            .gap(CARD_GAP)
            .py(6.0)
            .scroll_x()
            .scroll_edge_fade(30.0)
            .on_scroll_offset(move |p: rune::ScrollPos| {
                ROWS.with_borrow_mut(|m| {
                    m.insert(watch.clone(), [p.offset[0], p.max[0], p.viewport[0]]);
                });
                // Arrived (or near enough to call it): the reports speak for themselves again.
                GOING.with_borrow_mut(|m| {
                    if m.get(&watch).is_some_and(|to| (to - p.offset[0]).abs() < 1.0) {
                        m.remove(&watch);
                    }
                });
            })
            .children(cards),
    )
}

/// Two arrows for a row's heading that page row `key` by whole cards, or nothing when its `n` cards
/// of `card` points fit the page `w` wide. The one at its end is faint and takes no press.
pub(crate) fn row_arrows(s: &State, key: &str, n: usize, card: f32, w: f32) -> Option<El> {
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
            .w(34.0)
            .h(34.0)
            .radius(17.0)
            .center()
            .fill(th::veil(0.06))
            .child(icon(ic, 16.0).color(if on { th::text() } else { th::text().alpha(0.28) }));
        if !on {
            return b.disabled(true);
        }
        b.cursor(Cursor::Pointer).hov(th::veil(0.12)).press(th::veil(0.16)).press_scale(th::PRESS).on_press_tx(move |_s: &mut State, tx: &mut Tx| {
            let [off, max, view] = row_at(&row_key).unwrap_or([0.0, f32::MAX, pitch]);
            // As many whole cards as the row shows, landing on a card's edge.
            let k = ((view + CARD_GAP) / pitch).floor().max(1.0);
            let to = (((off / pitch).round() + dir * k) * pitch).clamp(0.0, max);
            tx.scroll_to(row_key.clone(), to, 0.0);
            // The view runs again right after this press, before the row has moved: it should
            // already show the arrows as they will be where the row lands.
            GOING.with_borrow_mut(|m| {
                m.insert(row_key.clone(), to);
            });
        })
    };
    Some(row().gap(8.0).items_center().child(arrow(-1.0)).child(arrow(1.0)))
}

/// A heading's right side: the row's arrows, then a "show all" link, either of which may be missing.
fn heading_side(arrows: Option<El>, more: Option<El>) -> Option<El> {
    match (arrows, more) {
        (None, m) => m,
        (a, None) => a,
        (Some(a), Some(m)) => Some(row().gap(16.0).items_center().child(m).child(a)),
    }
}

/// The space between cards in a grid or row; `fit` sizes cards for it, so the two must agree.
const GRID_GAP: f32 = 16.0;

fn grid(cards: Vec<El>) -> El {
    row().w_full().flex_wrap().gap(GRID_GAP).children(cards)
}

/// How many columns of at least `min` points fit in the page, and how wide each then is, so a
/// grid fills its row instead of leaving a ragged gap on the right.
fn fit(w: f32, min: f32, gap: f32, max_cols: usize) -> (usize, f32) {
    let avail = ((w - RAIL_W).min(PAGE_MAX) - 2.0 * PAGE_PAD).max(min);
    let cols = (((avail + gap) / (min + gap)).floor() as usize).clamp(1, max_cols);
    (cols, ((avail - gap * (cols as f32 - 1.0)) / cols as f32).floor())
}

/// What a card says under its name, the same three lines on every card: who made it (each a
/// link to their page) or plain words, then a quiet line of facts (what it is, the year, how
/// long). The name and who made it slide through their line under the pointer when they don't
/// fit; the facts stand still, apart from them, so the year is always in its place.
#[derive(Default)]
struct CardText {
    artists: Vec<Artist>,
    /// Said in place of the artists, when there are none.
    by: String,
    facts: String,
    explicit: bool,
}

/// What a card's play button does; the rest of the card opens the thing.
type OnPlay = Option<Box<dyn Fn(&mut State)>>;

/// The corner of a card's cover.
const CARD_R: f32 = 10.0;

/// A cover card with its name and [`CardText`] below; a play button rises on hover. Pressing the
/// play button plays (`play`), pressing anywhere else runs `on` (opens the page). A round card
/// (an artist) has its lines in the middle.
#[allow(clippy::too_many_arguments)]
fn media_card(
    s: &State,
    key: String,
    tpl: Option<&str>,
    title: String,
    text: CardText,
    size: f32,
    round: bool,
    play: OnPlay,
    on: impl Fn(&mut State) + 'static,
) -> El {
    let radius = if round { size * 0.5 } else { CARD_R };
    let mut art = el().w(size).h(size).radius(radius).child(kit::cover(s, tpl, size, radius));
    // A veil over the picture and a play button both come up under the pointer.
    art = art.child(el().absolute(0.0, 0.0).w_full().h_full().radius(radius).fill(th::rgba(0x000000, 0.18)).gop(0.0, 1.0));
    if let Some(play) = play {
        // The play button rises into the corner under the pointer.
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
                .fill(th::rgb(0xffffff))
                .shadow(4.0, 14.0, th::rgba(0x000000, 0.30))
                .gop(0.0, 1.0)
                .child(icon(&icons::PLAY, 17.0).color(th::rgb(0x0a0a0a)).ml(1.0)),
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

/// Facts joined into one quiet line.
fn facts_line(parts: impl IntoIterator<Item = String>) -> String {
    parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" · ")
}

fn playlist_card(s: &State, pl: &Playlist, size: f32) -> El {
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
    media_card(s, format!("pl-{}", pl.key()), pl.cover_template(), pl.title.clone(), text, size, false, play, move |s: &mut State| {
        s.go_preview(page.clone(), &preview)
    })
}

fn album_card(s: &State, a: &Album, size: f32) -> El {
    let (id, play_id, name) = (a.id.clone(), a.id.clone(), a.title.clone());
    let kind = crate::app::release_kind_name(a.kind.as_deref()).get(s.lang).to_owned();
    let facts = facts_line([kind, a.year.map(|y| y.to_string()).unwrap_or_default()]);
    let play: OnPlay = Some(Box::new(move |s: &mut State| s.play_album(&play_id, &name)));
    let page = Page::Album(id);
    let preview = Preview { title: a.title.clone(), cover: a.cover_template().map(str::to_owned), artists: a.artists.clone(), year: a.year };
    let text = CardText { artists: a.artists.clone(), facts, ..CardText::default() };
    media_card(s, format!("al-{}", a.id), a.cover_template(), a.title.clone(), text, size, false, play, move |s: &mut State| {
        s.go_preview(page.clone(), &preview)
    })
}

fn artist_card(s: &State, a: &Artist, size: f32) -> El {
    let id = a.id.clone();
    let name = a.name.clone();
    let text = CardText { by: S::Artist.get(s.lang).to_owned(), ..CardText::default() };
    media_card(s, format!("ar-{}", a.id), a.cover_template(), a.name.clone(), text, size, true, None, move |s: &mut State| {
        s.go_preview(Page::Artist(id.clone()), &Preview { title: name.clone(), cover: None, artists: Vec::new(), year: None })
    })
}

/// A track on a shelf: its play button plays it (the shelf is the queue), the rest of the card
/// opens its album (or plays it, when it has none).
fn track_card(s: &State, t: &Track, size: f32, shelf: usize) -> El {
    let (id, play_id) = (t.id.clone(), t.id.clone());
    let album = t.albums.first().cloned();
    let play: OnPlay = Some(Box::new(move |s: &mut State| s.play_shelf_track(shelf, &play_id)));
    let year = album.as_ref().and_then(|a| a.year).map(|y| y.to_string()).unwrap_or_default();
    let length = t.duration_ms.filter(|ms| *ms > 0).map(crate::player::fmt_time).unwrap_or_default();
    let text = CardText { artists: t.artists.clone(), facts: facts_line([year, length]), explicit: t.is_explicit(), ..CardText::default() };
    media_card(s, format!("tr-{shelf}-{}", t.id), t.cover_template(), t.full_title(), text, size, false, play, move |s: &mut State| match &album {
        Some(al) => {
            let preview = Preview { title: al.title.clone(), cover: al.cover_template().map(str::to_owned), artists: al.artists.clone(), year: al.year };
            s.go_preview(Page::Album(al.id.clone()), &preview)
        }
        None => s.play_shelf_track(shelf, &id),
    })
}

/// What a card already knows about the page it opens: the page shows it at once (the name, the
/// cover, who made it) while the rest comes.
#[derive(Clone)]
pub(crate) struct Preview {
    pub title: String,
    pub cover: Option<String>,
    pub artists: Vec<Artist>,
    pub year: Option<i32>,
}

impl State {
    /// Opens `page`, which shows what `p` knows of it until the rest has come.
    pub(crate) fn go_preview(&mut self, page: Page, p: &Preview) {
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

    /// Whether the queue in the player was started from `page`: by its play button or one of its
    /// rows. A track of the page that plays in a queue from somewhere else doesn't count.
    pub(crate) fn queue_from(&self, page: &Page) -> bool {
        self.current().is_some() && self.playing_from.as_ref().is_some_and(|f| !f.wave && f.page.as_ref() == Some(page))
    }
}

/// A page's play button. While the queue in the player came from `page`, it pauses and resumes
/// that queue where it is ("Pause", "Resume"). Otherwise it is "Play" and `start` plays the page's
/// tracks from the first.
fn play_button(s: &State, key: &str, page: Option<Page>, small: bool, start: fn(&mut State)) -> El {
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

// ---- wave presets ------------------------------------------------------------------------------------

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

/// A wave's name with the knobs it was tuned to beyond what it is ("Favorite · Calm").
fn wave_name_tuned(s: &State, id: &str) -> String {
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

fn start_wave_with(s: &mut State, id: &str) {
    s.wave_choice = id.to_owned();
    s.mark_settings_dirty();
    s.start_wave(id);
}

/// The moods offered under My Wave, in order.
const MOODS: [&str; 7] = ["favorite", "discover", "popular", "energetic", "calm", "fun", "sad"];

/// A hue for a preset: the colour its map is drawn in while nothing is playing.
fn wave_hue(id: &str) -> f32 {
    // The moods on Home sit side by side, so each has a hue well apart from the others.
    match id {
        "favorite" => 348.0,
        "discover" => 205.0,
        "popular" => 40.0,
        "energetic" => 14.0,
        "calm" => 172.0,
        "fun" => 280.0,
        "sad" => 228.0,
        _ => (id.bytes().fold(11u32, |a, b| a.wrapping_mul(37).wrapping_add(b as u32)) % 360) as f32,
    }
}

/// A mood's name set in its picture's room for words (`Family::room`: across a heart's lobes, in
/// a moon's thick part...): on one line, or on two when that lets it be larger, as large as the
/// room allows. It needs no compositor, so it stands in its place from the first frame.
fn name_on(name: &str, family: Family, side: f32, color: rune::Color) -> El {
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
            let size = (probe * (bw / w.max(1.0)).min(bh / h.max(1.0))).floor().clamp(12.0, 22.0);
            (t, size)
        })
        .fold((name.to_owned(), 0.0f32), |best, o| if o.1 > best.1 + 1.0 { o } else { best });
    // The room's middle, and a box around it wide enough for the words to stay centred on it.
    let (mx, my) = (side * 0.5 + cx * r, side * 0.5 + cy * r);
    let half = side * 0.5;
    el().absolute(mx - half, my - half)
        .w(side)
        .h(side)
        .center()
        .transition(Transition::none())
        .child(label(text, th::name_on(size), color).align(rune::Align::Center).max_w(bw.max(side * 0.6)).rotate(family.tilt()))
}

/// The width of the page's column for a window `w` wide.
fn column(w: f32) -> f32 {
    ((w - RAIL_W).min(PAGE_MAX) - 2.0 * PAGE_PAD).max(320.0)
}

// ---- home --------------------------------------------------------------------------------------------

/// Home: a greeting, then a garden of shapes, My Wave the big one (its button in the middle) and
/// each mood a shape of its own in its own colour, with loose marks drifting in the gaps; then the
/// shelves. The shapes are drawn from `State::home_seed`, so they are new every time the app
/// starts, and the button beside the greeting rolls them again.
fn home(s: &State, w: f32, _h: f32) -> El {
    let cw = column(w);
    let mut top = col().key("home-top").w_full().gap(40.0).transition(Transition::none()).child(kit::rise(greeting(s), 0)).child(garden(s, cw));
    // Not while the player is still leaving: they would fly over it.
    if s.bugs_alive() && s.swarm.seed == s.home_seed && s.np.t <= 0.0 {
        top = top.child(bug_layer(s));
    }
    let mut body = col().gap(40.0).child(top);
    match &s.home {
        Load::Ready(shelves) => {
            // The fullest shelf first (new releases fill a row to its end; the personal one
            // often doesn't), then down from there.
            let mut order: Vec<usize> = (0..shelves.len()).collect();
            order.sort_by_key(|&i| (std::cmp::Reverse(shelves[i].items.len()), shelves[i].kind != ShelfKind::NewReleases));
            let mut shown = 0;
            for (i, sh) in order.into_iter().map(|i| (i, &shelves[i])) {
                if sh.items.is_empty() {
                    continue;
                }
                shown += 1;
                // Only a fade: a shelf comes up exactly where its bones stood, so nothing moves.
                body = body.child(shelf(s, "", i, sh, w).transition(Transition::opacity().springs(th::RISE)).enter_delay(0.05 * shown.min(4) as f32));
            }
        }
        Load::Failed(_) => {
            if let Some(e) = kit::list_state(&s.home, s, 0) {
                body = body.child(e);
            }
        }
        // Still coming: two shelves of bones in the very places the first two will take.
        _ => {
            for i in 0..2 {
                body = body.child(shelf_bones(w, i));
            }
        }
    }
    kit::scroll_page_live("home", body)
}

/// "Good evening, Name" and a line under it; on the right the button that re-rolls the shapes.
fn greeting(s: &State) -> El {
    let lang = s.lang;
    let hour = crate::app::local_hour();
    let hello = match hour {
        5..=11 => S::GoodMorning,
        12..=17 => S::GoodAfternoon,
        18..=23 => S::GoodEvening,
        _ => S::GoodNight,
    };
    // A mood for the time of day, one press from playing.
    let (lead, mood) = match hour {
        5..=11 => (S::SuggestMorning, "energetic"),
        12..=17 => (S::SuggestDay, "discover"),
        18..=23 => (S::SuggestEvening, "calm"),
        _ => (S::SuggestNight, "sad"),
    };
    let (open, close) = if lang == crate::i18n::Lang::Ru { ("«", "»") } else { ("\u{201c}", "\u{201d}") };
    let playing_it = super::dock::wave_playing(s) && s.wave_choice == mood;
    let hint = format!("{} {open}{}{close}", lead.get(lang), wave_title(mood).get(lang));
    let suggest = row()
        .key("home-suggest")
        .role("button")
        .label(&hint)
        .gap(6.0)
        .items_center()
        .cursor(Cursor::Pointer)
        .hov_group()
        .press_scale(0.98)
        .opacity(if playing_it { 0.0 } else { 1.0 })
        .motion(rune::Spring::new(0.3, 0.0))
        .on_press(move |s: &mut State| start_wave_with(s, mood))
        .child(label(hint.clone(), th::BODY, th::text2()))
        .child(icon(&icons::PLAY, 11.0).color(th::text2()).gop(0.5, 1.0));
    let name = super::dock::account_name(s);
    let first = name.split_whitespace().next().unwrap_or("").to_owned();
    let title = if first.is_empty() { hello.get(lang).to_owned() } else { format!("{}, {first}", hello.get(lang)) };
    row()
        .key("home-head")
        .w_full()
        .items_center()
        .gap(16.0)
        .mt(20.0)
        .child(col().grow(1.0).min_w(0.0).gap(6.0).child(row().key("home-title").child(label(title, th::TITLE, th::text()).max_lines(2))).child(suggest))
        .child(
            kit::round("home-reroll", kit::Weight::Soft, &icons::SPARKLE, kit::BTN_H, 18.0, S::RerollShapes.get(lang))
                .on_press(|s: &mut State| s.home_seed = s.home_seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)),
        )
}

/// The keys of what the butterflies keep clear of and land on, and whether each is a shape.
pub fn bug_spots(s: &State) -> Vec<(String, bool)> {
    let seed = s.home_seed;
    let mut v = vec![("home-title".to_owned(), false), ("home-reroll".to_owned(), false), (format!("hm-wave-shape-{seed}"), true)];
    v.extend(MOODS.iter().map(|id| (format!("hm-mood-{id}-{seed}"), true)));
    v
}

/// The butterflies over the top of Home: each a small square the compositor draws one in, moved
/// and sized by the simulation's live values (`bugs.rs`); a press on one sends it off.
fn bug_layer(s: &State) -> El {
    let mut layer = el().key("bug-layer").pin(Some(-24.0), Some(-24.0), Some(-24.0), Some(-24.0)).transition(Transition::none());
    for (i, b) in s.swarm.bugs.iter().enumerate() {
        let side = b.boxed();
        layer = layer.child(
            el().key(format!("bug-{}-{i}", s.swarm.seed))
                .absolute(0.0, 0.0)
                .size(side, side)
                .live_offset(&b.off)
                .live_scale(&b.scale)
                .transition(Transition::none())
                .fx(crate::fx::BUTTERFLY)
                .fx_data([i as f32, b.seed, b.hue, 0.0])
                .on_press_tx(move |s: &mut State, tx: &mut Tx| {
                    s.swarm.scare(i, s.now);
                    tx.wake_after(std::time::Duration::from_millis(16));
                }),
        );
    }
    layer
}

/// What the pointer is over in the garden (a mood's id, or "wave"), kept in `State::garden_hover`.
fn garden_hover(id: &'static str) -> impl Fn(&mut State, bool, &mut Tx) + 'static {
    move |s: &mut State, on: bool, _tx: &mut Tx| {
        if on {
            s.garden_hover = Some(id);
        } else if s.garden_hover == Some(id) {
            s.garden_hover = None;
        }
    }
}

/// The garden of shapes at the head of Home, `cw` wide: My Wave the big one, filled with the
/// playing cover as a dithered print; each mood a shape of another family; butterflies flying
/// slow loops behind them.
fn garden(s: &State, cw: f32) -> El {
    let lang = s.lang;
    let seed = s.home_seed;
    let mut rng = Rng::new(seed);
    let gap = 18.0;
    let (big, bx, cell, spots, total_h) = garden_layout(cw, gap);
    let hover = if kit::under() { None } else { s.garden_hover };

    let mut g = el().key("garden").w_full().h(total_h).transition(Transition::none());

    // My Wave: the big shape, the name on it with its play button.
    let now = super::dock::wave_playing(s);
    let playing = now && s.player.playing;
    let hue = wave_hue(&s.wave_choice);
    // The cover fills it only while My Wave is what plays: a track played from anywhere else
    // leaves it in its mood's colour, so the two are never mistaken for each other. Once it holds
    // a wave track's cover it keeps it while the next one loads (another mood, the next track),
    // and the new one washes over it once it is in: never the mood's colour in between.
    let wave_cover_ready = s.current().and_then(|t| State::cover_key(t.cover_template(), 400)).is_some_and(|key| s.palette_key.as_deref() == Some(key.as_str()));
    let fill = {
        let mut wc = s.wave_cover.borrow_mut();
        if !now {
            *wc = (false, None);
        } else if wave_cover_ready && !wc.0 {
            *wc = (true, s.palette_key.clone());
        }
        match &*wc {
            (true, first) if *first == s.palette_key => Fill::CoverNow(hue),
            (true, _) => Fill::Cover(hue),
            _ => Fill::Hue(hue),
        }
    };
    let look = Look { hovered: hover == Some("wave"), ..Look::default() };
    let wave_seed = rng.seed();
    let wave_shape = blobs::wave(format!("hm-wave-shape-{seed}"), wave_seed, fill, look, s.settings.wave_look, big).absolute(0.0, 0.0);
    // Its knobs and look, from a quiet button in the box's corner (outside the shape, whatever
    // its form).
    let tune_open = s.popover_open && matches!(s.popover, Some(crate::app::Popover::WaveTune { .. }));
    let tune = kit::quiet_toggle("hm-wave-tune", &icons::TUNE, 18.0, 34.0, tune_open, S::WaveTune.get(lang))
        .absolute(big - 38.0, 4.0)
        .on_press(|s: &mut State| s.open_popover(crate::app::Popover::WaveTune { anchor: "hm-wave-tune".into() }));
    let go = move |s: &mut State| {
        if super::dock::wave_playing(s) {
            s.toggle_pause();
        } else {
            let id = s.wave_choice.clone();
            start_wave_with(s, &id);
        }
    };
    // Under the name: what it plays now, or which mood it will start with (and what it was tuned
    // to, if anything).
    let mood = wave_name_tuned(s, &s.wave_choice);
    let mood = mood.as_str();
    let (line, line_id) = match (now, s.current()) {
        (true, Some(t)) => {
            let l = format!("{} · {}\n{} — {}", S::WaveNowPlaying.get(lang), mood, t.full_title(), t.artists_line());
            (l.clone(), l)
        }
        _ => {
            let l = format!("{}: {}", S::Mood.get(lang), mood);
            (l.clone(), l)
        }
    };
    // The name is the button: the glyph (as tall as the capitals) and the words.
    // As big as the liquid's middle allows, but never cut short: the name shrinks to fit beside
    // the glyph.
    let wave_name = S::WaveTitle.get(lang);
    let room_w = big * 0.62;
    let mut title_size = (big * 0.085).clamp(22.0, 34.0);
    for _ in 0..2 {
        let room = room_w - 26.0 - 4.0 - (title_size * 0.95).round();
        let (tw, _) = rune::text::measure(wave_name, &[], th::name_on(title_size), None);
        if tw > room {
            title_size = (title_size * room / tw).floor().max(16.0);
        }
    }
    let glyph = (title_size * 0.95).round();
    let play_icon = icon(if playing { &icons::PAUSE } else { &icons::PLAY }, glyph).color(th::text()).gop(0.86, 1.0);
    let words = col()
        .absolute((big - room_w) * 0.5, 0.0)
        .w(room_w)
        .h(big)
        .justify_center()
        .items_center()
        .gap(4.0)
        .transition(Transition::none())
        .child(
            row()
                .key("hero-go")
                .role("button")
                .label(if playing { S::Pause.get(lang) } else { S::Play.get(lang) })
                .max_w(room_w)
                .pl(8.0)
                .pr(18.0)
                .py(4.0)
                .gap(4.0)
                .radius(14.0)
                .items_center()
                .cursor(Cursor::Pointer)
                // No wash under the pointer: the glyph brightens and the words give on a press.
                .hov_group()
                .press_scale(0.96)
                .on_press(go)
                .child(play_icon)
                .child(label(wave_name, th::name_on(title_size), th::text())),
        )
        .child(kit::roll(
            "hero-line",
            &line_id,
            row().w_full().justify_center().child(label(line, th::wave_words(16.0), th::text2()).align(rune::Align::Center).max_lines(2).wrap().max_w(room_w)),
        ));
    g = g.child(
        el().key("hm-wave")
            .absolute(bx, 0.0)
            .w(big)
            .h(big)
            .transition(Transition::none())
            .on_hover_change(garden_hover("wave"))
            .on_pointer_move_any(move |p: rune::PointerEvent, _tx: &mut Tx| {
                let r = blobs::radius(Family::Liquid, big);
                crate::fx::set_wave_pointer((p.x - big * 0.5) / r, (p.y - big * 0.5) / r);
            })
            .children(shape_bone("hm-wave-bone", Family::Liquid, wave_seed, s.settings.wave_look.form, hue, big))
            .child(wave_shape)
            .child(kit::rise(words, 1))
            .child(tune),
    );

    // The moods, each a shape of its own family, a little apart from the grid and its own size.
    let fg = th::text();
    for (i, id) in MOODS.into_iter().enumerate() {
        // A little larger than the cell: a picture sits inside its box (room for the ring).
        let size = cell * rng.range(0.94, 1.14);
        let (jx, jy) = (rng.range(-8.0, 8.0), rng.range(-8.0, 8.0));
        let x = spots[i][0] + (cell - size) * 0.5 + jx;
        let y = spots[i][1] + (cell - size) * 0.5 + jy;
        // The ring marks the mood My Wave is playing, and only while it plays.
        let on = now && s.wave_choice == id;
        let name = wave_title(id).get(lang);
        let look = Look { chosen: on, hovered: hover == Some(id), order: 1 + i as u32 };
        let shape_seed = rng.seed();
        let family = blobs::mood_family(id);
        let tile = el()
            .key(format!("hm-mood-{id}-{seed}"))
            .role("button")
            .label(name)
            .checked(on)
            .absolute(x, y)
            .w(size)
            .h(size)
            .cursor(Cursor::Pointer)
            .press_scale(0.95)
            .transition(Transition::none())
            .on_hover_change(garden_hover(id))
            .on_press(move |s: &mut State| start_wave_with(s, id))
            .children(shape_bone(&format!("hm-mood-bone-{id}"), family, shape_seed, 0, wave_hue(id), size))
            .child(blobs::shape(format!("hm-mood-shape-{id}-{seed}"), family, shape_seed, Fill::Hue(wave_hue(id)), look, size, size).absolute(0.0, 0.0))
            .child(name_on(name, family, size, fg));
        g = g.child(tile);
    }
    g
}

/// A shape before the compositor is up to draw it (the window's first frames): its very outline
/// for this seed, size and form (`outline`), filled with the colour it mostly shows. It fades as
/// the shape comes up over it in the same place, so nothing jumps.
fn shape_bone(key: &str, family: Family, seed: f32, form: u8, hue: f32, side: f32) -> Option<El> {
    if crate::fx::ready() {
        return None;
    }
    let points = super::outline::outline(family, form, seed, side);
    let bone = el().key(key.to_owned()).transition(Transition::opacity().springs(th::SOFT));
    if points.len() < 3 {
        // Nothing traced (it never should): the circle it is drawn about.
        let d = 2.0 * blobs::radius(family, side);
        return Some(bone.absolute((side - d) * 0.5, (side - d) * 0.5).w(d).h(d).radius(d * 0.5).fill(kit::bone_fill()));
    }
    Some(bone.absolute(0.0, 0.0).w(side).h(side).shape(rune::Shape::new().fill(points, super::outline::color(hue))))
}

/// Where the garden's shapes stand in a column `cw` wide: the big one's side and left edge, the
/// moods' cell and each mood's cell corner, and the garden's height.
///
/// Wide enough, the moods are a grid of two rows to the right of the big shape. Narrower, the big
/// one takes two by two cells of one grid across the column and the moods flow around it (beside
/// it, then under it); a row left short is spread across its width rather than leaving a hole,
/// and the grid is chosen so no mood is left alone in a row.
fn garden_layout(cw: f32, gap: f32) -> (f32, f32, f32, Vec<[f32; 2]>, f32) {
    let n = MOODS.len();
    const MIN: f32 = 140.0;
    let beside_min = 4.0 * MIN + 3.0 * gap;
    let big = (cw * 0.42).clamp(260.0, 420.0).min(cw - 32.0 - beside_min);
    if big >= 260.0 {
        let (mx, mw) = (big + 32.0, cw - big - 32.0);
        let cols = (((mw + gap) / (MIN + gap)).floor() as usize).clamp(4, 4);
        let cell = ((mw - gap * (cols as f32 - 1.0)) / cols as f32).min(186.0);
        let rows = n.div_ceil(cols);
        let grid_h = rows as f32 * cell + (rows as f32 - 1.0) * gap;
        let grid_w = cols as f32 * cell + (cols as f32 - 1.0) * gap;
        let (gx0, my) = (mx + (mw - grid_w) * 0.5, ((big - grid_h) * 0.5).max(0.0));
        let spots = (0..n)
            .map(|i| {
                let (r, c) = (i / cols, i % cols);
                let in_row = (n - r * cols).min(cols);
                let shift = (cols - in_row) as f32 * (cell + gap) * 0.5;
                [gx0 + shift + c as f32 * (cell + gap), my + r as f32 * (cell + gap)]
            })
            .collect();
        return (big, 0.0, cell, spots, big.max(grid_h));
    }
    let lonely = |c: usize| {
        let beside = 2 * (c - 2);
        beside < n && (n - beside) % c == 1
    };
    let cell_of = |c: usize| (cw - gap * (c as f32 - 1.0)) / c as f32;
    // Cells near 150 points; a column more or less if that one would leave a mood alone.
    let near = (((cw + gap) / (150.0 + gap)).round() as usize).clamp(3, 6);
    let cols = [near, near + 1, near.saturating_sub(1)].into_iter().find(|&c| (3..=6).contains(&c) && cell_of(c) >= 108.0 && !lonely(c)).unwrap_or(near);
    let cell = ((cw - gap * (cols as f32 - 1.0)) / cols as f32).min(200.0);
    let big = 2.0 * cell + gap;
    let grid_w = cols as f32 * cell + (cols as f32 - 1.0) * gap;
    let x0 = (cw - grid_w) * 0.5;
    // The cells in reading order, past the big one's four.
    let mut cells: Vec<(usize, usize)> = Vec::new();
    let mut r = 0;
    while cells.len() < n {
        for c in if r < 2 { 2 } else { 0 }..cols {
            if cells.len() < n {
                cells.push((r, c));
            }
        }
        r += 1;
    }
    let spots = cells
        .iter()
        .map(|&(r, c)| {
            let first = if r < 2 { 2 } else { 0 };
            let in_row = cells.iter().filter(|x| x.0 == r).count();
            let slots = cols - first;
            let y = r as f32 * (cell + gap);
            if in_row == slots {
                [x0 + c as f32 * (cell + gap), y]
            } else {
                // Spread evenly across the row's width.
                let left = x0 + first as f32 * (cell + gap);
                let width = slots as f32 * cell + (slots as f32 - 1.0) * gap;
                let space = (width - in_row as f32 * cell) / (in_row as f32 + 1.0);
                let j = (c - first) as f32;
                [left + space * (j + 1.0) + j * cell, y]
            }
        })
        .collect();
    let rows = r.max(2);
    (big, x0, cell, spots, rows as f32 * cell + (rows as f32 - 1.0) * gap)
}

/// A shelf before it has come: its mark and name, and as many cards as the row shows, each the
/// cover's place and two lines, all of a shelf's own sizes (`shelf`, `media_card`).
fn shelf_bones(w: f32, i: usize) -> El {
    let (_, size) = fit(w, 144.0, GRID_GAP, 7);
    let size = size.min(200.0);
    let n = (((column(w) + CARD_GAP) / (size + CARD_GAP)).floor() as usize).max(1);
    // The mark: the circle its pebble is drawn about.
    let d = 2.0 * blobs::radius(Family::Heart, 36.0);
    let mark = el().w(36.0).h(36.0).center().child(el().w(d).h(d).radius(d * 0.5).fill(kit::bone_fill()));
    let head = row().key(format!("bone-shelf-head-{i}")).w_full().items_center().gap(10.0).child(mark).child(kit::text_bone(
        format!("bone-shelf-name-{i}"),
        th::H1.line_height,
        14.0,
        150.0 - 30.0 * i as f32,
    ));
    let mut cards = row().key(format!("bone-shelf-row-{i}")).w_full().gap(CARD_GAP).py(6.0);
    for j in 0..n {
        cards = cards.child(
            col().key(format!("bone-card-{i}-{j}")).w(size).gap(10.0).child(el().w(size).h(size).radius(CARD_R).fill(kit::bone_fill())).child(
                col()
                    .w(size)
                    .gap(2.0)
                    .child(kit::text_bone(format!("bone-card-t-{i}-{j}"), th::BODY_M.line_height, 10.0, size * (0.78 - 0.07 * (j % 3) as f32)))
                    .child(kit::text_bone(format!("bone-card-s-{i}-{j}"), th::SMALL.line_height, 8.0, size * (0.5 - 0.06 * (j % 2) as f32)))
                    .child(kit::text_bone(format!("bone-card-f-{i}-{j}"), th::TIME.line_height, 7.0, size * 0.3).mt(2.0)),
            ),
        );
    }
    col().key(format!("bone-shelf-{i}")).w_full().gap(16.0).transition(Transition::opacity().springs(th::RISE)).child(head).child(cards)
}

fn shelf(s: &State, p: &str, i: usize, sh: &Shelf, w: f32) -> El {
    let lang = s.lang;
    let title = match sh.kind {
        ShelfKind::Personal => S::ShelfPersonal,
        ShelfKind::Editorial => S::ShelfEditorial,
        ShelfKind::NewReleases => S::ShelfNew,
        ShelfKind::Chart => S::ShelfChart,
    };
    // Cards are sized so a whole number of them fills the column.
    let (_, size) = fit(w, 144.0, GRID_GAP, 7);
    let size = size.min(200.0);
    let cards: Vec<El> = sh
        .items
        .iter()
        .map(|it| match it {
            ShelfItem::Playlist(pl) => playlist_card(s, pl, size),
            ShelfItem::Album(a) => album_card(s, a, size),
            ShelfItem::Track(t) => track_card(s, t, size, i),
        })
        .collect();
    let head = row()
        .key(format!("{p}shelf-head-{i}"))
        .w_full()
        .items_center()
        .gap(10.0)
        .child({
            // A small picture of what the shelf is before its name: a heart for what is yours, a
            // star for the editors' picks, a sun for what is new, a crown for the chart.
            let mut rng = Rng::new(s.home_seed ^ (i as u64 + 1).wrapping_mul(0x51_7cc1_b727_220a));
            let (family, hue) = match sh.kind {
                ShelfKind::Personal => (Family::Heart, 348.0),
                ShelfKind::Editorial => (Family::Star, 205.0),
                ShelfKind::NewReleases => (Family::Sun, 42.0),
                ShelfKind::Chart => (Family::Crown, 16.0),
            };
            blobs::shape(format!("{p}shelf-mark-{i}-{}", s.home_seed), family, rng.seed(), Fill::Hue(hue), Look::default(), 36.0, 36.0)
        })
        .child(label(title.get(lang).to_owned(), th::H1, th::text()).truncate().min_w(0.0));
    let row_key = format!("{p}shelf-row-{i}");
    let head = match row_arrows(s, &row_key, sh.items.len(), size, w) {
        Some(a) => head.child(el().grow(1.0)).child(a),
        None => head,
    };
    col().key(format!("{p}shelf-{i}")).gap(16.0).child(head).child(hscroll(row_key, cards))
}

// ---- search ------------------------------------------------------------------------------------------

/// The categories of results, in the order their chips stand.
const SEARCH_TABS: [SearchTab; 5] = [SearchTab::All, SearchTab::Tracks, SearchTab::Artists, SearchTab::Albums, SearchTab::Playlists];

fn tab_name(tab: SearchTab, lang: crate::i18n::Lang) -> &'static str {
    match tab {
        SearchTab::All => all_label(lang),
        SearchTab::Tracks => S::Tracks.get(lang),
        SearchTab::Albums => S::Albums.get(lang),
        SearchTab::Artists => S::Artists.get(lang),
        SearchTab::Playlists => S::Playlists.get(lang),
    }
}

/// Search: a title and the field. With nothing typed: what was searched for lately, then the moods.
/// While searching: a chip for each category (always all of them, so the row never jumps), then the
/// results of the one picked. "All" leads with the best match as a large card beside the first
/// tracks, then a row each of artists, albums and playlists; any other category is searched on
/// its own and lists many more of its kind.
fn search(s: &State, w: f32) -> El {
    let p = s.pal();
    let lang = s.lang;
    let has_text = !s.search.edit.text.is_empty();
    let input = text_input(&s.search.edit, FIELD)
        .key("search-input")
        .single_line()
        .grow(1.0)
        .color(th::text())
        .placeholder(S::SearchHint.get(lang), th::text3())
        .caret(p.accent, p.accent.alpha(0.30))
        .on_edit(|s: &mut State| &mut s.search.edit)
        .on_submit(|s: &mut State, _tx: &mut Tx| {
            let t = s.search.edit.text.clone();
            s.run_search(t);
            s.remember_search();
        });
    let mut field = kit::field_surface(row())
        .key("search-field")
        .w_full()
        .h(SEARCH_H)
        .pl(14.0)
        .pr(6.0)
        .gap(10.0)
        .radius(kit::ctl_radius(SEARCH_H))
        .items_center()
        .child(icon(&icons::SEARCH, 18.0).color(th::text3()))
        .child(input);
    if has_text {
        field = field.child(kit::ibtn("search-clear", &icons::CLOSE, 15.0, 32.0, th::text2(), S::ClearSearch.get(lang)).on_press(|s: &mut State| {
            s.search.edit.clear();
            s.run_search(String::new());
            s.focus_search = true;
        }));
    }
    // The field sits close under the title; the blocks below keep the page's usual gap.
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

    // The categories, always all five while there is text: a press searches that kind alone.
    let counts = s.search_view().map(|r| [None, Some(r.tracks.results.len()), Some(r.artists.results.len()), Some(r.albums.results.len()), Some(r.playlists.results.len())]);
    let tabs = SEARCH_TABS
        .iter()
        .enumerate()
        .map(|(i, &tab)| {
            let n = counts.and_then(|c| c[i]).filter(|n| *n > 0);
            let name = tab_name(tab, lang);
            (kit::tab_pill(format!("stab-{tab:?}"), name, n, s.search.tab == tab).on_press(move |s: &mut State| s.set_search_tab(tab)), kit::tab_pill_w(name, n))
        })
        .collect();
    let at = SEARCH_TABS.iter().position(|t| *t == s.search.tab).unwrap_or(0);
    body = body.child(kit::rise(row().w_full().child(kit::tab_track("stabs", tabs, at)), 2));

    let (_, card) = fit(w, 144.0, GRID_GAP, 8);
    let card = card.min(196.0);
    let tab = s.search.tab;
    // The kind picked is still being searched on its own: what "All" found of it stands in.
    let kind_loading = tab != SearchTab::All && s.search.more.is_loading();
    let content: El = match (&s.search.results, s.search_view()) {
        (Load::Idle | Load::Loading, None) => el(),
        (Load::Failed(_), None) => kit::list_state(&s.search.results, s, 0).unwrap_or_else(el),
        (_, Some(r)) => {
            let empty = match tab {
                SearchTab::All => r.is_empty(),
                SearchTab::Tracks => r.tracks.results.is_empty(),
                SearchTab::Albums => r.albums.results.is_empty(),
                SearchTab::Artists => r.artists.results.is_empty(),
                SearchTab::Playlists => r.playlists.results.is_empty(),
            };
            if empty && kind_loading {
                kit::list_state(&Load::<()>::Loading, s, 6).unwrap_or_else(el)
            } else if empty {
                kit::empty(&p, Ill::Lens, S::NoResults.get(lang))
            } else {
                results(s, r, tab, w, card)
            }
        }
        _ => el(),
    };
    body = body.child(kit::tab_body(format!("sr-tab-{tab:?}"), content));
    kit::scroll_page("search", body)
}

/// The results of the category `tab`, from `r`.
fn results(s: &State, r: &ym_api::SearchResult, tab: SearchTab, w: f32, card: f32) -> El {
    let lang = s.lang;
    let to = |tab: SearchTab| move |s: &mut State| s.set_search_tab(tab);
    let mut tb = col().w_full().gap(44.0);
    match tab {
        SearchTab::All => {
            if let Some(top) = top_row(s, r, w) {
                tb = tb.child(kit::rise(top, 3));
            }
            if !r.artists.results.is_empty() {
                let cards: Vec<El> = r.artists.results.iter().take(14).map(|a| artist_card(s, a, (card * 0.86).floor())).collect();
                let more = kit::more_link("sr-artists-more", S::ShowAll.get(lang)).on_press(to(SearchTab::Artists));
                let side = heading_side(row_arrows(s, "sr-artists", cards.len(), (card * 0.86).floor(), w), Some(more));
                tb = tb.child(kit::rise(col().gap(14.0).child(kit::heading("sr-artists-h", S::Artists.get(lang), side)).child(hscroll("sr-artists".into(), cards)), 4));
            }
            if !r.albums.results.is_empty() {
                let cards: Vec<El> = r.albums.results.iter().take(14).map(|a| album_card(s, a, card)).collect();
                let more = kit::more_link("sr-albums-more", S::ShowAll.get(lang)).on_press(to(SearchTab::Albums));
                let side = heading_side(row_arrows(s, "sr-albums", cards.len(), card, w), Some(more));
                tb = tb.child(kit::rise(col().gap(14.0).child(kit::heading("sr-albums-h", S::Albums.get(lang), side)).child(hscroll("sr-albums".into(), cards)), 5));
            }
            if !r.playlists.results.is_empty() {
                let cards: Vec<El> = r.playlists.results.iter().take(14).map(|a| playlist_card(s, a, card)).collect();
                let more = kit::more_link("sr-playlists-more", S::ShowAll.get(lang)).on_press(to(SearchTab::Playlists));
                let side = heading_side(row_arrows(s, "sr-playlists", cards.len(), card, w), Some(more));
                tb = tb.child(kit::rise(col().gap(14.0).child(kit::heading("sr-playlists-h", S::Playlists.get(lang), side)).child(hscroll("sr-playlists".into(), cards)), 6));
            }
        }
        SearchTab::Tracks => tb = tb.child(track_list("sr-rows", r.tracks.results.len(), ListSrc::Search)),
        SearchTab::Albums => tb = tb.child(grid(r.albums.results.iter().take(60).map(|a| album_card(s, a, card)).collect())),
        SearchTab::Artists => tb = tb.child(grid(r.artists.results.iter().take(60).map(|a| artist_card(s, a, card)).collect())),
        SearchTab::Playlists => tb = tb.child(grid(r.playlists.results.iter().take(60).map(|a| playlist_card(s, a, card)).collect())),
    }
    tb
}

/// A quiet "clear history" beside a heading.
fn clear_link(key: &str, lang: crate::i18n::Lang, on: impl Fn(&mut State) + 'static) -> El {
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

/// What was searched for lately: a small name and a way to forget them all on one line, and under
/// it a quiet pill each (a press searches it again, its cross forgets it).
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
                .transition(Transition::opacity().combined(Transition::scale(0.9)).springs(th::SOFT))
                .on_press(move |s: &mut State| s.search_again(&again))
                .child(icon(&icons::CLOCK, 14.0).color(th::text3()).gcol(th::text2()))
                .child(label(q.clone(), th::BODY, th::text2()).gcol(th::text()).truncate().max_w(240.0))
                .child(forget),
        );
    }
    let head = row().w_full().h(24.0).items_center().child(kit::caps(S::RecentSearches.get(lang), th::text3())).child(el().grow(1.0)).child(clear);
    col().key("sh").gap(10.0).child(head).child(pills)
}

/// A bright colour of hue `h` (degrees), as the shapes are drawn in.
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

/// Whether My Wave in the mood `id` is what plays now.
fn mood_playing(s: &State, id: &str) -> bool {
    s.current().is_some() && matches!(&s.player.source, Source::Wave { preset, .. } if preset == id)
}

/// A mood as a tile: a dark plate with a breath of the mood's colour along it, its name on the
/// left and, whole on the right, the same liquid pebble the mood has on Home, which comes alive
/// under the pointer. The mood that plays now has a border in its own colour, and pressing it
/// pauses and resumes it instead of starting it again. (The mood merely picked on Home, which
/// may not play, is not marked: a white ring there read as a broken border.)
fn mood_tile(s: &State, id: &'static str, w: f32, h: f32) -> El {
    let on = mood_playing(s, id);
    let name = wave_title(id).get(s.lang);
    let hue = wave_hue(id);
    let seed = (id.bytes().fold(5u32, |a, b| a.wrapping_mul(33).wrapping_add(b as u32)) % 997) as f32 + 0.5;
    let hot = !kit::under() && s.garden_hover == Some(id);
    let side = (h * 0.78).floor();
    let tint = |a: f32| hue_color(hue).alpha(a);
    // What a press does, shown under the name while the pointer is on it.
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
        .linear_gradient(0.0, [(0.0, tint(0.05)), (1.0, tint(if hot { 0.24 } else { 0.15 }))])
        .border(1.0, tint(if on { 0.7 } else { 0.22 }))
        .hov_group()
        .transition(Transition::none())
        .on_hover_change(garden_hover(id))
        .on_press(move |s: &mut State| if mood_playing(s, id) { s.toggle_pause() } else { start_wave_with(s, id) })
        .child(art)
        .child(col().gap(3.0).max_w(w - side - 30.0).child(label(name.to_owned(), th::name_on(17.0), th::text()).truncate()).child(
            row().gap(5.0).items_center().gop(0.0, 1.0).child(icon(hint_icon, 11.0).color(th::text2())).child(label(hint.get(s.lang).to_owned(), th::SMALL, th::text2())),
        ))
}

/// The moods in an even grid (four across, two in a narrow page), then what Home has to offer
/// from the chart and the new releases.
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
    // The chart and the new releases from Home, so there is music here before anything is typed.
    if let Load::Ready(shelves) = &s.home {
        let picks = [ShelfKind::Chart, ShelfKind::NewReleases];
        for (n, kind) in picks.iter().enumerate() {
            if let Some((i, sh)) = shelves.iter().enumerate().find(|(_, sh)| sh.kind == *kind && !sh.items.is_empty()) {
                out = out.child(kit::rise(shelf(s, "sr-", i, sh, w), 5 + n));
            }
        }
    }
    out
}

fn all_label(lang: crate::i18n::Lang) -> &'static str {
    match lang {
        crate::i18n::Lang::Ru => "Всё",
        crate::i18n::Lang::En => "All",
    }
}

/// How many track rows sit beside the best match.
const TOP_ROWS: usize = 4;

/// The head of "All": the best match as a large card, and beside it (under it, in a narrow window)
/// the first tracks with a way to all of them.
fn top_row(s: &State, r: &ym_api::SearchResult, w: f32) -> Option<El> {
    let lang = s.lang;
    let tracks = &r.tracks.results;
    let best = top_card(s, r);
    if best.is_none() && tracks.is_empty() {
        return None;
    }
    let mut list = col().key("sr-top-tracks").grow(1.0).min_w(0.0).gap(10.0);
    if !tracks.is_empty() {
        let more = (tracks.len() > TOP_ROWS).then(|| kit::more_link("sr-tracks-more", S::ShowAll.get(lang)).on_press(|s: &mut State| s.search.tab = SearchTab::Tracks));
        let rows = tracks.iter().take(TOP_ROWS).enumerate().map(|(i, t)| kit::track_row(s, t, i + 1, ListSrc::Search)).collect::<Vec<_>>();
        list = list.child(kit::list_heading("sr-tracks-h", S::Tracks.get(lang), more)).child(col().w_full().gap(2.0).children(rows));
    }
    let wide = column(w) >= 820.0;
    let mut r = if wide { row().items_start().gap(28.0) } else { col().gap(28.0) };
    r = r.key("sr-top").w_full();
    if let Some(b) = best {
        let card_w = if wide { (column(w) * 0.38).clamp(300.0, 420.0) } else { column(w) };
        r = r.child(col().w(card_w).gap(10.0).child(kit::heading("sr-best-h", S::TopResult.get(lang), None)).child(b.w_full()));
    }
    Some(r.child(list))
}

/// The best match: an artist whose name is what was typed, else the first track.
fn top_card(s: &State, r: &ym_api::SearchResult) -> Option<El> {
    let lang = s.lang;
    let q = s.search.query.to_lowercase();
    let artist = r.artists.results.first().filter(|a| !q.is_empty() && a.name.to_lowercase().contains(&q));
    // As tall as the track rows beside it, so the two end level.
    let rows = r.tracks.results.len().min(TOP_ROWS) as f32;
    let h = (rows * (ROW_H + 2.0) - 2.0).max(210.0);
    let base = |key: String, name: &str| {
        col()
            .key(key)
            .role("button")
            .label(name)
            .h(h)
            .padding(24.0)
            .gap(16.0)
            .justify_end()
            .radius(24.0)
            .cursor(Cursor::Pointer)
            .fill(th::veil(0.05))
            .border_gradient(1.0, th::veil(0.10), th::veil(0.02))
            .hov(th::veil(0.035))
            .press(th::veil(0.07))
            .hov_group()
    };
    let kind_line = |kind: &str, sub: &str| {
        let mut r = row().gap(10.0).items_center().min_w(0.0).child(row().h(24.0).px(10.0).radius(12.0).items_center().fill(th::veil(0.09)).child(label(
            kind.to_owned(),
            th::SMALL,
            th::text(),
        )));
        if !sub.is_empty() {
            r = r.child(label(sub.to_owned(), th::BODY, th::text2()).truncate().min_w(0.0));
        }
        r
    };
    let play = |key: &str| kit::round(key.to_owned(), kit::Weight::Solid, &icons::PLAY, 52.0, 22.0, S::Play.get(lang)).absolute(0.0, 0.0).gop(0.0, 1.0);
    if let Some(a) = artist {
        let id = a.id.clone();
        let card = base(format!("sr-best-ar-{}", a.id), &a.name)
            .on_press(move |s: &mut State| s.go(Page::Artist(id.clone())))
            .child(kit::cover(s, a.cover_template(), 104.0, 52.0))
            .child(col().gap(10.0).min_w(0.0).child(label(a.name.clone(), th::H0, th::text()).truncate().min_w(0.0)).child(kind_line(S::Artist.get(lang), "")));
        return Some(card);
    }
    let t = r.tracks.results.first()?;
    let (id, id2) = (t.id.clone(), t.id.clone());
    let corner = el().absolute(0.0, 0.0).w_full().h_full().child(
        el().absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .child(row().pin(None, None, Some(0.0), Some(0.0)).w(52.0).h(52.0).child(play("sr-best-play").on_press(move |s: &mut State| s.play_from_list(ListSrc::Search, &id2)))),
    );
    let card = base(format!("sr-best-tr-{}", t.id), &t.full_title())
        .on_press(move |s: &mut State| s.play_from_list(ListSrc::Search, &id))
        .child(kit::cover(s, t.cover_template(), 104.0, 16.0))
        .child(el().w_full().child(corner).child(
            col().gap(10.0).min_w(0.0).pr(64.0).child(label(t.full_title(), th::H0, th::text()).truncate().min_w(0.0)).child(kind_line(S::Track.get(lang), &t.artists_line())),
        ));
    Some(card)
}

// ---- library -----------------------------------------------------------------------------------------

/// The collection: its title, a chip for each kind, then what was liked of that kind. Liked tracks
/// open with a banner (the heart, how many there are and how long they play, play and shuffle);
/// the other kinds are grids of cards.
fn liked_playlist_cover(s: &State, side: f32, radius: f32) -> El {
    if let Some(tpl) = s.library.liked_cover.as_deref() {
        return el().key("lib-pic").w(side).h(side).radius(radius).shadow(12.0, 34.0, th::rgba(0x000000, if th::light() { 0.14 } else { 0.40 })).child(kit::cover(
            s,
            Some(tpl),
            side,
            radius,
        ));
    }
    let cell = side * 0.5;
    let mut art = el().key("lib-pic").w(side).h(side).radius(radius).clip_rounded(radius).fill(th::veil(0.08));
    let mut count = 0;
    if let Some(tracks) = s.library.tracks.get() {
        for (i, track) in tracks.iter().take(4).enumerate() {
            let x = if i % 2 == 0 { 0.0 } else { cell };
            let y = if i < 2 { 0.0 } else { cell };
            art = art.child(el().absolute(x, y).w(cell + 1.0).h(cell + 1.0).fill(th::srgb(s.palette.colors[i])).child(kit::cover(s, track.cover_template(), cell + 1.0, 0.0)));
            count += 1;
        }
    }
    if count == 0 {
        art = art
            .child(el().absolute(0.0, 0.0).w_full().h_full().fill(s.pal().accent.alpha(0.24)))
            .child(row().absolute(0.0, 0.0).w_full().h_full().center().child(icon(&icons::HEART_FILL, side * 0.34).color(th::text())));
    }
    art.child(el().absolute(0.0, 0.0).w_full().h_full().radius(radius).fill(th::rgba(0x000000, 0.12)))
}

fn library(s: &State, w: f32) -> El {
    let p = s.pal();
    let lang = s.lang;
    let tab = s.library.tab;
    let q = s.library.filter.text.trim().to_lowercase();
    let has = |hay: &[&str]| q.is_empty() || hay.iter().any(|h| h.to_lowercase().contains(&q));
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
                (kit::tab_pill(format!("ltab-{t:?}"), name.get(lang), n, tab == t).on_press(move |s: &mut State| s.load_library_tab(t)), kit::tab_pill_w(name.get(lang), n))
            })
            .collect(),
        at,
    );
    // The kinds on the left, the search on the right.
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
    // On the playlists, a new one of the account's own.
    // (Shaped as the tabs and the search beside it: as tall, the same corners.)
    let new_pl = (tab == LibTab::Playlists).then(|| {
        {
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
        }
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

    // The head, as an album's: a picture (the liked tracks' heart on the cover's colours), what
    // it is and how much it holds, and the buttons: play it all, shuffle, My Wave by it.
    let liked = s.library.tracks.get().filter(|v| !v.is_empty());
    let lead = match liked {
        Some(v) => {
            let secs = v.iter().filter_map(|t| t.duration_ms).sum::<u64>() / 1000;
            format!("{} · {}", lang.tracks(v.len()), lang.duration(secs))
        }
        None => String::new(),
    };
    let side = if cw >= 640.0 { HEAD_SIDE } else { 112.0 };
    let pic = liked_playlist_cover(s, side, 12.0);
    let mut meta = col()
        .key("lib-meta")
        .grow(1.0)
        .min_w(0.0)
        .child(kit::caps(S::Playlist.get(lang), th::text3()))
        .child(label(S::Liked.get(lang).to_owned(), th::TITLE_D, th::text()).mt(HEAD_STEP_TITLE));
    if !lead.is_empty() {
        meta = meta.child(label(lead, th::BYLINE, th::text3()).mt(HEAD_STEP_BY));
    }
    if liked.is_some() {
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
            .child(kit::wave_button(s, "lib-wave", S::WaveByCollection.get(lang), S::LikedTracks.get(lang), crate::app::Station::Collection, kit::BTN_H));
        let more_on = s.popover_open && matches!(s.popover, Some(crate::app::Popover::Liked { .. }));
        let buttons = buttons
            .child(
                kit::pill_icon("lib-more", kit::Weight::Soft, &icons::MORE, kit::BTN_H, 20.0, S::More.get(lang))
                    .checked(more_on)
                    .on_press(|s: &mut State| s.open_popover(crate::app::Popover::Liked { anchor: "lib-more".into() })),
            )
            .child({
                // Everything that can be had is on disk: the button deletes the downloads instead.
                let settled = s.library.tracks.get().is_some_and(|list| s.downloads_settled(list));
                let (ic, name) = if settled { (&icons::TRASH, S::DeleteDownload) } else { (&icons::DOWNLOAD, S::DownloadAllLiked) };
                kit::pill_icon("lib-download", kit::Weight::Soft, ic, kit::BTN_H, 20.0, name.get(lang)).on_press(move |s: &mut State| {
                    let list = s.library.tracks.get().cloned().unwrap_or_default();
                    if settled {
                        s.ask_delete_downloads(list);
                    } else {
                        s.ask_download(list);
                    }
                })
            });
        meta = meta.child(buttons);
    }
    let head = if cw < 640.0 {
        col().key("lib-head").w_full().gap(18.0).child(pic).child(meta.w_full())
    } else {
        row().key("lib-head").w_full().gap(HEAD_GAP).items_end().child(pic).child(meta)
    };
    let body = col().gap(32.0).child(kit::rise(head.mt(12.0), 0)).child(kit::rise(bar, 1));
    let (_, card) = fit(w, 144.0, GRID_GAP, 8);
    let nothing = |art: Ill| kit::empty(&p, art, S::Nothing.get(lang));
    let content = match tab {
        LibTab::Tracks => match &s.library.tracks {
            Load::Ready(v) if v.is_empty() => nothing(Ill::Shelf),
            Load::Ready(v) => {
                let shown: Vec<usize> = (0..v.len())
                    .filter(|&i| {
                        let t = &v[i];
                        has(&[t.title.as_str(), &t.artists_line(), t.albums.first().map_or("", |a| a.title.as_str())])
                    })
                    .collect();
                let n = shown.len();
                *s.library.shown.borrow_mut() = shown;
                if n == 0 { nothing(Ill::Lens) } else { track_list("lib-rows", n, ListSrc::Liked) }
            }
            other => kit::list_state(other, s, 8).unwrap_or_else(el),
        },
        LibTab::Playlists => match &s.library.playlists {
            Load::Ready(v) if v.is_empty() => nothing(Ill::Shelf),
            Load::Ready(v) => grid(v.iter().filter(|x| has(&[x.title.as_str()])).map(|x| playlist_card(s, x, card)).collect()),
            other => kit::list_state(other, s, 0).unwrap_or_else(el),
        },
        LibTab::Albums => match &s.library.albums {
            Load::Ready(v) if v.is_empty() => nothing(Ill::Shelf),
            Load::Ready(v) => grid(v.iter().filter(|x| has(&[x.title.as_str(), &x.artists_line()])).map(|x| album_card(s, x, card)).collect()),
            other => kit::list_state(other, s, 0).unwrap_or_else(el),
        },
        LibTab::Artists => match &s.library.artists {
            Load::Ready(v) if v.is_empty() => nothing(Ill::Shelf),
            Load::Ready(v) => grid(v.iter().filter(|x| has(&[x.name.as_str()])).map(|x| artist_card(s, x, card)).collect()),
            other => kit::list_state(other, s, 0).unwrap_or_else(el),
        },
    };
    kit::scroll_page("library", body.child(kit::tab_body(format!("lib-tab-{tab:?}"), content)))
}

fn track_list(key: &'static str, n: usize, src: ListSrc) -> El {
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

impl State {
    fn list_track(&self, src: ListSrc, i: usize) -> Option<&Track> {
        match src {
            ListSrc::Liked => {
                let v = self.library.tracks.get()?;
                if self.library.filter.text.trim().is_empty() { v.get(i) } else { self.library.shown.borrow().get(i).and_then(|&j| v.get(j)) }
            }
            ListSrc::Detail => self.detail.tracks.get(i),
            ListSrc::Search => self.search_view().and_then(|r| r.tracks.results.get(i)),
            ListSrc::Queue => self.player.queue.get(i),
            ListSrc::History => self.listening_history.get(i),
        }
    }

    fn list_id(&self, src: ListSrc, i: usize) -> String {
        match self.list_track(src, i) {
            Some(t) => format!("{:?}{i}-{}", src, t.id),
            None => format!("{src:?}{i}"),
        }
    }
}

// ---- album / playlist / artist -----------------------------------------------------------------------

/// The side of the picture at the head of the Liked page.
const HEAD_SIDE: f32 = 160.0;
/// The side of the cover (or the artist's shape) at the head of a release, a playlist or an
/// artist, in a wide window and in a narrow one.
const RELEASE_SIDE: f32 = 236.0;
const RELEASE_SIDE_S: f32 = 184.0;
/// The space over the name, over who made it and over the buttons in a head beside a large
/// picture. The lines carry room of their own above and below their letters (the name more than
/// the rest), so these differ for the steps to look the same: about 13 points between letters.
pub(crate) const HEAD_STEP_TITLE: f32 = 6.0;
pub(crate) const HEAD_STEP_BY: f32 = 6.0;
pub(crate) const HEAD_STEP_BUTTONS: f32 = 10.0;
/// The gap between the large picture and the words beside it.
pub(crate) const HEAD_GAP: f32 = 32.0;

/// The picture at the head of the page: the cover, or the artist in a shape of their own (a blob
/// that slowly moves, as the moods on Home do). Pressing it opens the details: the release's facts,
/// or what is known about the artist.
fn head_picture(s: &State, key: &str, side: f32) -> El {
    let lang = s.lang;
    let d = &s.detail;
    let radius = if d.is_artist { side * 0.5 } else { 12.0 };
    if d.page.is_none() || (d.state.is_loading() && (d.cover.is_none() || d.is_artist)) {
        // The picture comes in when the page does: until then, its place, quietly.
        return el().key("detail-cover-bone").w(side).h(side).radius(radius).fill(kit::bone_fill());
    }
    let Some(tpl) = d.cover.clone() else { return kit::cover(s, None, side, radius) };
    let about = if d.is_artist { S::AboutArtist } else { S::Details }.get(lang);
    let base = el()
        .key("detail-cover")
        .role("button")
        .label(about)
        .help(about)
        .w(side)
        .h(side)
        .cursor(Cursor::Pointer)
        .transition(Transition::none())
        .on_press(|s: &mut State| s.open_sheet(Sheet::Page));
    if d.is_artist {
        // The picture is drawn by the shape's shader, which reads it from the art cache. Until the
        // photo is in, its place stays grey (the shape would show a colour of its own).
        let pic = State::cover_key(Some(&tpl), 1000);
        if let Some(k) = &pic {
            let _ = s.art.pixels(k);
            let _ = s.art.full(k);
        }
        let landed = pic.as_ref().is_some_and(|k| s.art.ready(k));
        s.pic_key.replace(pic);
        if !landed {
            return el().key("detail-cover-bone").w(side).h(side).radius(radius).fill(kit::bone_fill());
        }
        let (seed, hue) = blobs::avatar_seed(key);
        return base.child(blobs::avatar(format!("detail-blob-{key}"), seed, Fill::Picture(hue), Look::default(), 0.0, side).absolute(0.0, 0.0));
    }
    base.radius(radius)
        .hov_group()
        .shadow(18.0, 48.0, th::rgba(0x000000, if th::light() { 0.18 } else { 0.45 }))
        .child(kit::cover(s, d.cover.as_deref(), side, radius))
        .child(el().absolute(0.0, 0.0).w_full().h_full().radius(radius).fill(th::rgba(0xffffff, 0.06)).gop(0.0, 1.0))
}

/// What it is, under its tracks: the genre, how many tracks and how long (an artist: their genres,
/// albums and similar artists), each a quiet name over its value, then a playlist's description.
/// The year, the genre, how many tracks and how long are at the head, in its line of facts.
fn detail_facts(s: &State) -> Option<El> {
    let d = &s.detail;
    let mut cells = row().key("d-facts").w_full().gap(40.0).flex_wrap();
    let mut any = false;
    let mut about = None;
    for (i, (name, value)) in d.facts.iter().enumerate() {
        match name {
            S::Year | S::Genre | S::Genres | S::Tracks | S::Duration | S::MonthlyListeners => continue,
            S::SimilarArtists => continue,
            S::Description => {
                about = Some(value.clone());
                continue;
            }
            _ => {}
        }
        any = true;
        cells = cells.child(
            col()
                .key(format!("d-fact-{i}"))
                .gap(5.0)
                .max_w(360.0)
                .child(kit::caps(name.get(s.lang), th::text3()))
                .child(label(value.clone(), th::BODY, th::text2()).wrap().max_lines(3)),
        );
    }
    if !any && about.is_none() {
        return None;
    }
    let mut c = col().key("d-about").w_full().gap(20.0).transition(Transition::opacity().springs(th::RISE)).child(el().w_full().h(1.0).fill(th::hairline()));
    if any {
        c = c.child(cells);
    }
    if let Some(text) = about {
        c = c.child(label(text, th::BODY, th::text2()).wrap().max_lines(12).max_w(720.0));
    }
    Some(c)
}

/// The picture at the head of the page, melted into its colours and laid across the top of the
/// page behind the head: as wide as the page, from the top of the window to a little under the
/// picture, where it has faded out into the page. `None` until the picture is in.
fn head_backdrop(s: &State, w: f32, side: f32) -> Option<El> {
    let d = &s.detail;
    let tpl = d.cover.as_deref()?;
    // The copy the head already asks for (the shape's for an artist, the cover's for the rest).
    let pic = if d.is_artist { State::cover_key(Some(tpl), 1000) } else { State::cover_key(Some(tpl), kit::cover_px(side)) }?;
    let px = s.art.soft(&pic)?;
    let page_w = (w - RAIL_W).max(0.0);
    // The head is in the page's middle column; the picture reaches out to the page's sides.
    let left = ((page_w - page_w.min(PAGE_MAX)) * 0.5).max(0.0);
    let h = kit::TITLE_H + 16.0 + side + 120.0;
    let strength = if th::light() { 0.40 } else { 0.52 };
    // Keyed by the cover, not by the copy's size: a key that changes as the window settles would
    // make it come in again. It comes in from the top, as a soft wipe down the page.
    Some(
        el().key(format!("d-backdrop-{tpl}"))
            .absolute(-left, 0.0)
            .w(page_w)
            .h(h)
            .clip_fade(0.0, 0.0, 0.0, h * 0.66)
            .transition(Transition::reveal(rune_motion::Edge::Top).combined(Transition::opacity()).springs(th::GLOW))
            .child(el().absolute(0.0, 0.0).w_full().h_full().opacity(strength).pixels_fit(&px, rune::SurfaceFit::Fill)),
    )
}

/// The quiet line of facts under the name at the head: what it is, its genre, the year, how many
/// tracks and how long (an artist: their genres and listeners). A release of one track leaves out
/// "1 track" and its length: the track's own row under the head shows how long it is.
fn head_facts(s: &State, kind: S) -> String {
    let lang = s.lang;
    let d = &s.detail;
    let one = d.facts.iter().any(|(n, v)| *n == S::Tracks && v.trim() == "1");
    let mut parts = vec![kind.get(lang).to_owned()];
    for (name, value) in &d.facts {
        match name {
            S::Genre | S::Genres => parts.push(value.clone()),
            S::Tracks if !one => parts.push(value.parse::<usize>().map_or_else(|_| value.clone(), |n| lang.tracks(n))),
            S::Duration if !one => parts.push(value.clone()),
            _ => {}
        }
    }
    if let Some(y) = d.year {
        // The year goes after the genre.
        let at = parts.len().min(2);
        parts.insert(at, y.to_string());
    }
    if let Some(n) = d.monthly_listeners {
        parts.push(format_listeners(n, lang));
    }
    facts_line(parts)
}

/// The head's buttons: play and shuffle (and My Wave by it) on the left, where the eye starts;
/// pin, download and more at the right end of the line.
/// The tracks are read from the state when a button is pressed, not copied into every build of
/// the page (an artist's whole list, twice, on every cover that landed).
fn head_buttons(s: &State) -> El {
    let lang = s.lang;
    let d = &s.detail;
    let mut first = row().gap(10.0).items_center();
    if !d.tracks.is_empty() {
        first = first
            .child(play_button(s, "d-play", d.page.clone(), false, |s| {
                let list = s.detail.tracks.clone();
                s.play_all(list, false);
            }))
            .child(kit::btn_soft("d-shuffle", S::Shuffle.get(lang), Some(&icons::SHUFFLE)).on_press(|s: &mut State| {
                let list = s.detail.tracks.clone();
                s.play_all(list, true);
            }));
    }
    // My Wave by the artist or by the release. A playlist has no station seed.
    let wave = match &d.page {
        Some(Page::Artist(id)) => Some(kit::wave_button(s, "d-wave", S::WaveByArtist.get(lang), &d.title, crate::app::Station::Artist(id.clone()), kit::BTN_H)),
        Some(Page::Album(id)) => Some(kit::wave_button(s, "d-wave", S::WaveByRelease.get(lang), &d.title, crate::app::Station::Album(id.clone()), kit::BTN_H)),
        _ => None,
    };
    if let Some(w) = wave {
        first = first.child(w);
    }
    let mut rest = row().gap(8.0).items_center();
    if d.page.is_some() {
        let pin_on = s.detail_is_pinned();
        let pin_name = if pin_on { S::Pinned } else { S::Pin };
        let pin_icon = if pin_on { &icons::PIN_OFF } else { &icons::PIN };
        let mut pin = kit::pill_icon("d-pin", kit::Weight::Soft, pin_icon, kit::BTN_H, 20.0, pin_name.get(lang)).checked(pin_on);
        if pin_on {
            pin = pin.fill(th::veil(0.14));
        }
        rest = rest.child(pin.on_press(|s: &mut State| s.toggle_detail_pin()));
    }
    let download_tracks = if d.is_artist {
        match &d.all_tracks {
            Load::Ready(tracks) => tracks.clone(),
            _ => Vec::new(),
        }
    } else {
        d.tracks.clone()
    };
    if !d.tracks.is_empty() {
        let downloaded = s.downloads_settled(&download_tracks);
        let (icon, label) = if downloaded { (&icons::TRASH, S::DeleteDownload) } else { (&icons::DOWNLOAD, S::DownloadAll) };
        rest = rest.child(kit::pill_icon("d-download", kit::Weight::Soft, icon, kit::BTN_H, 20.0, label.get(lang)).on_press(move |s: &mut State| {
            if downloaded {
                s.delete_detail_downloads();
            } else {
                s.download_detail();
            }
        }));
    }
    let on = s.popover_open && matches!(s.popover, Some(crate::app::Popover::Actions { .. }));
    rest = rest.child(
        kit::pill_icon("d-more", kit::Weight::Soft, &icons::MORE, kit::BTN_H, 20.0, S::More.get(lang))
            .checked(on)
            .on_press(|s: &mut State| s.open_popover(crate::app::Popover::Actions { anchor: "d-more".into() })),
    );
    row().key("d-buttons").w_full().gap(10.0).items_center().flex_wrap().child(first).child(el().grow(1.0)).child(rest)
}

/// The head's buttons before the page is here: the same shapes, in the same places, as the ones
/// it will most likely have (play, shuffle, the wave for an artist or a release; pin, download,
/// more).
fn head_button_bones(s: &State) -> El {
    let d = &s.detail;
    let mut first = row()
        .gap(10.0)
        .items_center()
        .child(kit::button_bone("d-play-bone", kit::button_width(S::Play.get(s.lang), true)))
        .child(kit::button_bone("d-shuffle-bone", kit::button_width(S::Shuffle.get(s.lang), true)));
    if matches!(d.page, Some(Page::Artist(_) | Page::Album(_))) {
        first = first.child(kit::button_bone("d-wave-bone", kit::BTN_H));
    }
    let rest = row()
        .gap(8.0)
        .items_center()
        .child(kit::button_bone("d-pin-bone", kit::BTN_H))
        .child(kit::button_bone("d-download-bone", kit::BTN_H))
        .child(kit::button_bone("d-more-bone", kit::BTN_H));
    row().key("d-buttons-bone").w_full().gap(10.0).items_center().flex_wrap().child(first).child(el().grow(1.0)).child(rest)
}

/// An album, a playlist or an artist: the picture with the name, the buttons beside it and the
/// tracks under them; for an artist then the albums and similar artists. "All" beside an
/// artist's popular tracks folds the head away and lists every track under a slim bar of its own.
fn detail(s: &State, w: f32) -> El {
    let p: Pal = s.pal();
    let lang = s.lang;
    let d = &s.detail;
    let key = s.page.key();
    if !d.is_artist {
        s.pic_key.replace(None);
    }
    let kind = match s.page {
        Page::Album(_) => S::Album,
        Page::Playlist(..) => S::Playlist,
        _ => S::Artist,
    };
    let all_mode = d.is_artist && d.showing_all;
    let mut body = col().gap(40.0);

    // A page that could not be had shows only why (and a way to try again): no head over it.
    let failed = matches!(d.state, Load::Failed(_));
    // Until the page is here its head holds bones in the very places the name, who made it and
    // the buttons will take, each as tall as what comes.
    let coming = !failed && !matches!(d.state, Load::Ready(_));
    if !all_mode && !failed {
        let wide = column(w) >= 700.0;
        let side = if wide { RELEASE_SIDE } else { RELEASE_SIDE_S };
        let kind = match s.page {
            Page::Album(_) => crate::app::release_kind_name(d.release_kind.as_deref()),
            _ => kind,
        };
        if let Some(b) = head_backdrop(s, w, side) {
            body = body.child(b);
        }
        let title_style = if wide { th::RELEASE } else { th::RELEASE_S };
        let name = if coming && d.title.is_empty() {
            kit::text_bone("d-title-bone", title_style.line_height, 26.0, 320.0)
        } else {
            label(d.title.clone(), title_style, th::text()).max_lines(2).w_full()
        };
        let mut meta = col().key("d-meta").min_w(0.0).child(name);
        // Who made it, each a link to their page with their picture (a playlist: whose it is).
        let by_w = if wide { (column(w) - side - HEAD_GAP).max(180.0) } else { column(w).max(180.0) };
        if !d.artists.is_empty() {
            meta = meta.child(el().key("d-by").w_full().min_w(0.0).mt(8.0).child(kit::artists_row_in_width(
                s,
                &d.artists,
                th::BYLINE_M,
                true,
                "d-art",
                th::text().alpha(0.9),
                by_w,
                true,
            )));
        } else if !d.subtitle.is_empty() {
            meta = meta.child(label(d.subtitle.clone(), th::BYLINE_M, th::text().alpha(0.9)).truncate().min_w(0.0).mt(6.0));
        }
        // What it is, its genre, the year, how many tracks and how long: quiet, on one line.
        if !coming {
            meta = meta.child(label(head_facts(s, kind), th::TIME, th::text3()).truncate().min_w(0.0).mt(8.0));
        } else {
            meta = meta.child(kit::text_bone("d-by-bone", th::BYLINE_M.line_height, 12.0, 200.0).mt(8.0));
        }
        if coming {
            meta = meta.child(head_button_bones(s).mt(22.0));
        } else if !d.tracks.is_empty() || d.like_key.is_some() || !d.facts.is_empty() {
            meta = meta.child(head_buttons(s).mt(22.0));
        }
        // The head folds away (it shrinks back and fades) when "All" is pressed, and the list
        // under it rises into its place.
        let fold = Transition::asymmetric(Transition::none(), Transition::opacity().combined(Transition::scale(0.94)).combined(Transition::offset(0.0, -16.0))).springs(th::RISE);
        let head = if wide {
            // The buttons stand level with the foot of the picture.
            row()
                .key("detail-head")
                .w_full()
                .mt(8.0)
                .gap(HEAD_GAP)
                .items_end()
                .transition(fold)
                .child(head_picture(s, &key, side))
                .child(kit::rise(meta.grow(1.0).min_w(0.0), 1))
        } else {
            col().key("detail-head").w_full().mt(8.0).gap(20.0).transition(fold).child(head_picture(s, &key, side)).child(kit::rise(meta.w_full().min_w(0.0), 1))
        };
        body = body.child(head);
    }

    match &d.state {
        Load::Ready(()) => {
            let (_, card) = fit(w, 144.0, GRID_GAP, 8);
            let card = card.min(196.0);
            if !d.tracks.is_empty() {
                let list = track_list("detail-rows", d.tracks.len(), ListSrc::Detail);
                let section = if all_mode {
                    // The same heading row as before (so nothing in the list jumps while the head
                    // folds away above it), now naming every track, with play and shuffle in it.
                    let count = match &d.all_tracks {
                        Load::Ready(v) => format!("{} · {}", d.title, lang.tracks(v.len())),
                        _ => d.title.clone(),
                    };
                    let heading = row()
                        .key("d-tracks-h")
                        .w_full()
                        .h(32.0)
                        .pl(12.0)
                        .pr(4.0)
                        .gap(12.0)
                        .items_center()
                        .child(label(S::AllTracks.get(lang), th::SECTION, th::text()))
                        .child(label(count, th::BODY, th::text3()).truncate().min_w(0.0))
                        .child(el().grow(1.0))
                        .child(track_sort_button(s))
                        .child(play_button(s, "d-all-play", d.page.clone(), true, |s| {
                            let list = s.detail.tracks.clone();
                            s.play_all(list, false);
                        }))
                        .child(kit::round("d-all-shuffle", kit::Weight::Soft, &icons::SHUFFLE, 32.0, 16.0, S::Shuffle.get(lang)).on_press(|s: &mut State| {
                            let list = s.detail.tracks.clone();
                            s.play_all(list, true);
                        }));
                    col().key("d-tracks").mt(16.0).gap(10.0).child(heading).child(list)
                } else {
                    let title = if d.is_artist { S::PopularTracks } else { S::Tracks };
                    // An artist's page lists the popular tracks; "All" beside them lists every one.
                    let all = d.is_artist.then(|| {
                        let mut m = kit::more_link("d-all", S::All.get(lang)).on_press_tx(|s: &mut State, tx: &mut Tx| {
                            s.show_all_artist_tracks(true);
                            tx.scroll_to_y(s.page.key(), 0.0);
                        });
                        if d.all_tracks.is_loading() {
                            m = m.child(icon(&icons::SPINNER, 14.0).color(th::text3()).looping(rune::LoopSpec::rotation(0.9)));
                        }
                        m
                    });
                    let sort = (d.tracks.len() > 2 && !d.is_artist).then(|| track_sort_button(s));
                    let more = heading_side(sort, all);
                    col().key("d-tracks").gap(10.0).child(kit::list_heading("d-tracks-h", title.get(lang), more)).child(list)
                };
                body = body.child(kit::rise(section, 2));
            }
            if !all_mode && let Some(f) = detail_facts(s) {
                body = body.child(kit::rise(f, 3));
            }
            if !all_mode && !d.albums.is_empty() {
                let shown = s.shown_albums();
                let open = s.popover_open && matches!(s.popover, Some(crate::app::Popover::DiscSort { .. }));
                let changed = d.disc_sort != crate::app::DiscSort::default() || d.disc_kind != crate::app::DiscKind::default();
                let now = changed.then(|| if d.disc_kind != crate::app::DiscKind::default() { d.disc_kind.name() } else { d.disc_sort.name() }.get(lang));
                let sort = kit::sort_button("d-disc-sort", s, now, open).on_press(|s: &mut State| s.open_popover(crate::app::Popover::DiscSort { anchor: "d-disc-sort".into() }));
                let releases = if shown.is_empty() {
                    row().key("d-albums-none").h(card * 0.5).items_center().pl(4.0).child(label(S::NoneOfKind.get(lang), th::BODY, th::text3()))
                } else {
                    // Its own key for each order and kind, so a new choice starts at the left.
                    hscroll(format!("detail-albums-{:?}-{:?}", d.disc_sort, d.disc_kind), shown.iter().map(|a| album_card(s, a, card)).collect())
                };
                let arrows = row_arrows(s, &format!("detail-albums-{:?}-{:?}", d.disc_sort, d.disc_kind), shown.len(), card, w);
                body = body.child(
                    col()
                        .key("d-albums")
                        .gap(14.0)
                        .transition(Transition::opacity().springs(th::RISE))
                        .child(kit::heading("d-albums-h", S::Discography.get(lang), heading_side(arrows, Some(sort))))
                        .child(releases),
                );
            }
            if !all_mode && !d.similar.is_empty() {
                body = body.child(
                    col()
                        .key("d-similar")
                        .gap(14.0)
                        .transition(Transition::opacity().springs(th::RISE))
                        .child(kit::heading("d-similar-h", S::SimilarArtists.get(lang), row_arrows(s, "detail-similar", d.similar.len(), (card * 0.86).floor(), w)))
                        .child(hscroll("detail-similar".into(), d.similar.iter().map(|a| artist_card(s, a, (card * 0.86).floor())).collect())),
                );
            }
            if d.tracks.is_empty() && d.albums.is_empty() {
                body = body.child(kit::empty(&p, Ill::Shelf, S::Nothing.get(lang)));
            }
        }
        Load::Failed(_) => {
            if let Some(e) = kit::list_state(&d.state, s, 0) {
                body = body.child(e);
            }
        }
        _ => {
            body = body.child(kit::list_state(&d.state, s, 7).unwrap_or_else(el));
        }
    }
    kit::scroll_page_live(key, body)
}

/// The button over a page's tracks that orders them.
fn track_sort_button(s: &State) -> El {
    let d = &s.detail;
    let open = s.popover_open && matches!(s.popover, Some(crate::app::Popover::TrackSort { .. }));
    let now = (d.sort != crate::app::TrackSort::Original).then(|| d.sort.name().get(s.lang));
    kit::sort_button("d-sort", s, now, open).on_press(|s: &mut State| s.open_popover(crate::app::Popover::TrackSort { anchor: "d-sort".into() }))
}

pub(crate) fn format_listeners(n: u64, lang: crate::i18n::Lang) -> String {
    format!("{} {}", crate::i18n::group_digits(n), S::ListenersPerMonth.get(lang))
}
