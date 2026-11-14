//! Home, Search, Library and the album / playlist / artist page.

use std::borrow::Cow;

use rune::{Cursor, El, GrowthSpec, TextStyle, Transition, Tx, col, el, icon, row, text_input};
use ym_api::{Album, Artist, Playlist, Shelf, ShelfItem, ShelfKind, Track, WAVE_PRESETS};

use super::icons;
use super::kit::{self, ListSrc, PAGE_MAX, PAGE_PAD, RAIL_W, ROW_H, label};
use super::blobs::{self, Family, Fill, Look, Rng};
use super::shapes::Ill;
use crate::app::{LibTab, Load, Page, SearchTab, Source, State};
use crate::i18n::S;
use crate::theme::{self as th, Pal};

/// The search field's type: big, light, no box.
const FIELD: TextStyle = TextStyle::new(20.0, 300.0).family(th::SANS).tracking(-0.3).lh(28.0);

pub fn page(s: &State, w: f32, h: f32) -> El {
    match &s.page {
        Page::Home => home(s, w, h),
        Page::Search => search(s, w),
        Page::Library => library(s, w),
        Page::Settings => super::settings::settings(s, w - RAIL_W),
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
fn hscroll(key: String, cards: Vec<El>) -> El {
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
fn row_arrows(s: &State, key: &str, n: usize, card: f32, w: f32) -> Option<El> {
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
        b.cursor(Cursor::Pointer).hover(th::veil(0.12)).press(th::veil(0.16)).press_scale(th::PRESS).on_press_tx(move |_s: &mut State, tx: &mut Tx| {
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

fn grid(cards: Vec<El>) -> El {
    row().w_full().flex_wrap().gap(20.0).children(cards)
}

/// How many columns of at least `min` points fit in the page, and how wide each then is, so a
/// grid fills its row instead of leaving a ragged gap on the right.
fn fit(w: f32, min: f32, gap: f32, max_cols: usize) -> (usize, f32) {
    let avail = ((w - RAIL_W).min(PAGE_MAX) - 2.0 * PAGE_PAD).max(min);
    let cols = (((avail + gap) / (min + gap)).floor() as usize).clamp(1, max_cols);
    (cols, ((avail - gap * (cols as f32 - 1.0)) / cols as f32).floor())
}

/// What a card says under its name: plain words, or the artists (each opens their page) with
/// words after them (" · 2026").
enum Sub {
    Text(String),
    Artists(Vec<Artist>, String),
}

/// What a card's play button does; the rest of the card opens the thing.
type OnPlay = Option<Box<dyn Fn(&mut State)>>;

/// A cover card with a title and a line below; a play button rises on hover. Pressing the play
/// button plays (`play`), pressing anywhere else runs `on` (opens the page).
#[allow(clippy::too_many_arguments)]
fn media_card(s: &State, key: String, tpl: Option<&str>, title: String, sub: Sub, size: f32, round: bool, play: OnPlay, on: impl Fn(&mut State) + 'static) -> El {
    let radius = if round { size * 0.5 } else { 14.0 };
    let mut art = el().w(size).h(size).radius(radius).child(kit::cover(s, tpl, size, radius));
    // A veil over the picture and a play disc both come up under the pointer.
    art = art.child(el().absolute(0.0, 0.0).w_full().h_full().radius(radius).fill(th::rgba(0x000000, 0.22)).group_opacity(0.0, 1.0));
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
                .absolute(size - 60.0, size - 60.0)
                .w(48.0)
                .h(48.0)
                .radius(24.0)
                .center()
                .fill(th::rgb(0xffffff))
                .border_gradient(1.0, th::rgba(0xffffff, 0.0), th::rgba(0x000000, 0.12))
                .shadow(6.0, 18.0, th::rgba(0x000000, 0.35))
                .group_opacity(0.0, 1.0)
                .child(icon(&icons::PLAY, 20.0).color(th::rgb(0x0a0a0a))),
        );
    }
    let mut text_col = col().w(size).gap(2.0).child(label(title.clone(), th::BODY_M, th::text()).truncate().min_w(0.0));
    match sub {
        Sub::Text(sub) if !sub.is_empty() => text_col = text_col.child(label(sub, th::SMALL, th::text2()).truncate().min_w(0.0)),
        Sub::Text(_) => {}
        Sub::Artists(artists, after) if !artists.is_empty() => {
            let after_w = if after.is_empty() { 0.0 } else { rune::text::measure(&after, &[], th::SMALL, None).0.ceil() + 4.0 };
            let mut line = row().w(size).min_w(0.0).items_center().child(kit::artists_row_in_width(s, &artists, th::SMALL, false, &format!("{key}-ar"), th::text2(), (size - after_w).max(40.0)).shrink());
            if !after.is_empty() {
                line = line.child(label(after, th::SMALL, th::text2()).truncate().min_w(0.0));
            }
            text_col = text_col.child(line);
        }
        Sub::Artists(_, after) => text_col = text_col.child(label(after.trim_start_matches(" · ").to_owned(), th::SMALL, th::text2()).truncate().min_w(0.0)),
    }
    col().key(key).role("button").label(title).w(size).gap(12.0).cursor(Cursor::Pointer).hover_group().press_scale(th::PRESS).on_press(on).child(art).child(text_col)
}

fn playlist_card(s: &State, pl: &Playlist, size: f32) -> El {
    let (uid, kind) = (pl.owner_uid(), pl.kind);
    let sub = pl.track_count.map(|n| s.lang.tracks(n as usize)).unwrap_or_default();
    let name = pl.title.clone();
    let play: OnPlay = Some(Box::new(move |s: &mut State| s.play_playlist(uid, kind, &name)));
    media_card(s, format!("pl-{}", pl.key()), pl.cover_template(), pl.title.clone(), Sub::Text(sub), size, false, play, move |s: &mut State| s.go(Page::Playlist(uid, kind)))
}

fn album_card(s: &State, a: &Album, size: f32) -> El {
    let (id, play_id, name) = (a.id.clone(), a.id.clone(), a.title.clone());
    let year = a.year.map(|y| format!(" · {y}")).unwrap_or_default();
    let play: OnPlay = Some(Box::new(move |s: &mut State| s.play_album(&play_id, &name)));
    media_card(s, format!("al-{}", a.id), a.cover_template(), a.title.clone(), Sub::Artists(a.artists.clone(), year), size, false, play, move |s: &mut State| {
        s.go(Page::Album(id.clone()))
    })
}

fn artist_card(s: &State, a: &Artist, size: f32) -> El {
    let id = a.id.clone();
    media_card(s, format!("ar-{}", a.id), a.cover_template(), a.name.clone(), Sub::Text(S::Artist.get(s.lang).to_owned()), size, true, None, move |s: &mut State| {
        s.go(Page::Artist(id.clone()))
    })
}

/// A track on a shelf: its play button plays it (the shelf is the queue), the rest of the card
/// opens its album (or plays it, when it has none).
fn track_card(s: &State, t: &Track, size: f32, shelf: usize) -> El {
    let (id, play_id) = (t.id.clone(), t.id.clone());
    let album = t.albums.first().map(|a| a.id.clone());
    let play: OnPlay = Some(Box::new(move |s: &mut State| s.play_shelf_track(shelf, &play_id)));
    media_card(s, format!("tr-{shelf}-{}", t.id), t.cover_template(), t.full_title(), Sub::Artists(t.artists.clone(), String::new()), size, false, play, move |s: &mut State| match &album {
        Some(al) => s.go(Page::Album(al.clone())),
        None => s.play_shelf_track(shelf, &id),
    })
}

impl State {
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

/// A mood's name written by hand into its shape, as a designer would letter it: in the shape's
/// own room for words (`Family::room`), leaning with it, on one line or two (whichever lets it be
/// bigger), as large as that room allows.
fn written_in(name: &str, family: Family, seed: f32, side: f32, color: rune::Color) -> El {
    let r = blobs::radius(family, side);
    let a = blobs::lean(seed);
    let mut lines = vec![name.to_owned()];
    // Two lines, broken at the space nearest the middle.
    let mid = name.chars().count() / 2;
    if let Some((i, _)) = name.char_indices().filter(|(_, ch)| *ch == ' ').min_by_key(|(i, _)| name[..*i].chars().count().abs_diff(mid)) {
        lines.push(format!("{}\n{}", &name[..i], &name[i + 1..]));
    }
    // Every room with every way of breaking the name; the one that lets it be biggest wins (a
    // room up the shape only when that is clearly bigger: across reads easier).
    let probe = 20.0;
    let mut best: Option<(String, f32, [f32; 5])> = None;
    for (n, room) in family.rooms().iter().enumerate() {
        let (bw, bh) = (room[2] * r, room[3] * r);
        for t in &lines {
            let (w, h) = rune::text::measure(t, &[], th::hand(probe), None);
            // The script's loops may reach past its lines a little.
            let k = (bw / w.max(1.0)).min(bh / (h * 0.78).max(1.0));
            let size = (probe * k).floor().min(34.0);
            let score = if n == 0 { size } else { size * 0.8 };
            if best.as_ref().is_none_or(|b| score > if b.2 == family.rooms()[0] { b.1 } else { b.1 * 0.8 }) {
                best = Some((t.clone(), size, *room));
            }
        }
    }
    let (text, size, [cx, cy, _, _, turn]) = best.unwrap_or_else(|| (name.to_owned(), 14.0, [0.0; 5]));
    let size = size.max(11.0);
    let (c, s) = (a.cos(), a.sin());
    let (ox, oy) = (r * (c * cx - s * cy), r * (s * cx + c * cy));
    let (w, h) = rune::text::measure(&text, &[], th::hand(size), None);
    el().absolute(side * 0.5 + ox - w * 0.5, side * 0.5 + oy - h * 0.5)
        .w(w + 2.0)
        .h(h)
        .rotate(a + turn)
        .child(label(text, th::hand(size), color).align(rune::Align::Center).w_full())
}

/// The shape each mood is drawn as: a picture of the mood.
fn mood_family(id: &str) -> Family {
    match id {
        "favorite" => Family::Heart,
        "discover" => Family::Star,
        "popular" => Family::Flame,
        "energetic" => Family::Bolt,
        "calm" => Family::Moon,
        "fun" => Family::Sun,
        "sad" => Family::Drop,
        _ => Family::Waves,
    }
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
                body = body.child(kit::rise(shelf(s, "", i, sh, w), 3 + shown));
            }
        }
        Load::Failed(_) => {
            if let Some(e) = kit::list_state(&s.home, s, 0) {
                body = body.child(e);
            }
        }
        _ => {
            body = body.child(shelf_skeleton("sk-a")).child(shelf_skeleton("sk-b"));
        }
    }
    kit::scroll_page_live("home", body)
}

/// "Good evening, Name" and a line under it; on the right the button that re-rolls the shapes.
fn greeting(s: &State) -> El {
    let lang = s.lang;
    let hello = match crate::app::local_hour() {
        5..=11 => S::GoodMorning,
        12..=17 => S::GoodAfternoon,
        18..=23 => S::GoodEvening,
        _ => S::GoodNight,
    };
    let name = super::dock::account_name(s);
    let first = name.split_whitespace().next().unwrap_or("").to_owned();
    let title = if first.is_empty() { hello.get(lang).to_owned() } else { format!("{}, {first}", hello.get(lang)) };
    row()
        .key("home-head")
        .w_full()
        .items_center()
        .gap(16.0)
        .mt(20.0)
        .child(row().key("home-title").grow(1.0).min_w(0.0).child(label(title, th::TITLE, th::text()).max_lines(2)))
        .child(
            kit::round("home-reroll", kit::Weight::Soft, &icons::SPARKLE, 44.0, 20.0, S::Shuffle.get(lang)).on_press(|s: &mut State| s.home_seed = s.home_seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)),
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
                .effect(crate::fx::BUTTERFLY)
                .effect_data([i as f32, b.seed, b.hue, 0.0])
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
    // The big one is a family the words can sit across; each mood is a picture of itself.
    let wave_family = blobs::SOLID[rng.pick(blobs::SOLID.len())];
    let hover = s.garden_hover;

    let mut g = el().key("garden").w_full().h(total_h).transition(Transition::none());

    // My Wave: the big shape, the name on it with its play button.
    let now = super::dock::wave_playing(s);
    let playing = now && s.player.playing;
    let hue = wave_hue(&s.wave_choice);
    // The cover fills it only while My Wave is what plays: a track played from anywhere else
    // leaves it in its mood's colour, so the two are never mistaken for each other.
    let wave_cover_ready = s.current().and_then(|t| State::cover_key(t.cover_template(), 400)).is_some_and(|key| s.palette_key.as_deref() == Some(key.as_str()));
    let fill = if now && wave_cover_ready { Fill::Cover(hue) } else { Fill::Hue(hue) };
    let look = Look { hovered: hover == Some("wave"), ..Look::default() };
    let wave_shape = blobs::shape(format!("hm-wave-shape-{seed}"), wave_family, rng.seed(), fill, look, big, big).absolute(0.0, 0.0);
    let go = move |s: &mut State| {
        if super::dock::wave_playing(s) {
            s.toggle_pause();
        } else {
            let id = s.wave_choice.clone();
            start_wave_with(s, &id);
        }
    };
    // Under the name: what it plays now, or which mood it will start with.
    let mood = wave_title(&s.wave_choice).get(lang);
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
    // The name is the button: the glyph (as tall as the script's capitals) and the handwritten
    // words, with a soft wash under the pointer.
    // As big as the shape allows, but never cut short: the name shrinks to fit beside the glyph.
    let wave_name = S::WaveTitle.get(lang);
    let mut title_size = (big * 0.105).clamp(28.0, 44.0);
    for _ in 0..2 {
        let room = big * 0.76 - 26.0 - 4.0 - (title_size * 1.15).round();
        let (tw, _) = rune::text::measure(wave_name, &[], th::hand(title_size), None);
        if tw > room {
            title_size = (title_size * room / tw).floor().max(18.0);
        }
    }
    let glyph = (title_size * 1.15).round();
    let play_icon = icon(if playing { &icons::PAUSE } else { &icons::PLAY }, glyph).color(th::text()).group_opacity(0.86, 1.0);
    let words = col()
        .absolute(big * 0.12, 0.0)
        .w(big * 0.76)
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
                .max_w(big * 0.76)
                .pl(8.0)
                .pr(18.0)
                .py(4.0)
                .gap(4.0)
                .radius(14.0)
                .items_center()
                .cursor(Cursor::Pointer)
                // A dark wash reads over any cover, light or dark.
                .hover(th::rgba(0x000000, 0.24))
                .press(th::rgba(0x000000, 0.34))
                .hover_group()
                .press_scale(0.96)
                .on_press(go)
                .child(play_icon)
                .child(label(wave_name, th::hand(title_size), th::text())),
        )
        .child(kit::roll("hero-line", &line_id, row().w_full().justify_center().child(label(line, th::SMALL, th::text2()).align(rune::Align::Center).max_lines(2).wrap().max_w(big * 0.62))));
    g = g.child(
        el().key("hm-wave")
            .absolute(bx, 0.0)
            .w(big)
            .h(big)
            .transition(Transition::none())
            .on_hover_change(garden_hover("wave"))
            .child(wave_shape)
            .child(kit::rise(words, 1)),
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
        let family = mood_family(id);
        let shape_seed = rng.seed();
        g = g.child(
            el().key(format!("hm-mood-{id}-{seed}"))
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
                .child(blobs::shape(format!("hm-mood-shape-{id}-{seed}"), family, shape_seed, Fill::Hue(wave_hue(id)), look, size, size).absolute(0.0, 0.0))
                .child(written_in(name, family, shape_seed, size, fg).transition(Transition::opacity().springs(th::RISE)).enter_delay(0.12 + 0.04 * i as f32)),
        );
    }
    g
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

fn shelf_skeleton(key: &str) -> El {
    let cards = (0..6)
        .map(|i| col().key(format!("{key}-{i}")).gap(14.0).child(kit::skeleton(180.0, 180.0, 16.0)).child(kit::skeleton(140.0, 12.0, 6.0)).child(kit::skeleton(90.0, 10.0, 5.0)))
        .collect();
    col().gap(16.0).child(kit::skeleton(150.0, 14.0, 7.0)).child(hscroll(format!("{key}-row"), cards))
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
    let (_, size) = fit(w, 168.0, 20.0, 7);
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
            // star for the editors' picks, a sun for what is new, a flame for the chart.
            let mut rng = Rng::new(s.home_seed ^ (i as u64 + 1).wrapping_mul(0x51_7cc1_b727_220a));
            let (family, hue) = match sh.kind {
                ShelfKind::Personal => (Family::Heart, 348.0),
                ShelfKind::Editorial => (Family::Star, 205.0),
                ShelfKind::NewReleases => (Family::Sun, 42.0),
                ShelfKind::Chart => (Family::Flame, 16.0),
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
        .h(58.0)
        .pl(20.0)
        .pr(10.0)
        .gap(14.0)
        .radius(18.0)
        .items_center()
        .child(icon(&icons::SEARCH, 22.0).color(th::text2()))
        .child(input);
    if has_text {
        field = field.child(kit::round("search-clear", kit::Weight::Soft, &icons::CLOSE, 36.0, 16.0, S::ClearSearch.get(lang)).on_press(|s: &mut State| {
            s.search.edit.clear();
            s.run_search(String::new());
            s.focus_search = true;
        }));
    }
    let mut body = col().gap(24.0).child(kit::rise(kit::page_title(S::Search.get(lang), "").mt(12.0), 0)).child(kit::rise(field, 1));
    if !has_text {
        if !s.settings.search_history.is_empty() {
            body = body.child(kit::rise(history(s), 2));
        }
        if !s.listening_history.is_empty() {
            let count = s.listening_history.len().min(8);
            body = body.child(kit::rise(col().gap(10.0).child(kit::heading("sh-listening-h", S::ListeningHistory.get(lang), None)).child(track_list("sh-listening", count, ListSrc::History)), 3));
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
            kit::tab_pill(format!("stab-{tab:?}"), tab_name(tab, lang), n, s.search.tab == tab).on_press(move |s: &mut State| s.set_search_tab(tab))
        })
        .collect();
    body = body.child(kit::rise(row().w_full().child(kit::tab_track("stabs", tabs)), 2));

    let (_, card) = fit(w, 168.0, 20.0, 8);
    let card = card.min(196.0);
    let tab = s.search.tab;
    // The kind picked is still being searched on its own: what "All" found of it stands in.
    let kind_loading = tab != SearchTab::All && s.search.more.is_loading();
    let content: El = match (&s.search.results, s.search_view()) {
        (Load::Idle, _) => kit::empty(&p, Ill::Lens, S::Loading.get(lang)),
        (Load::Loading, None) => kit::list_state(&s.search.results, s, 6).unwrap_or_else(el),
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

/// What was searched for lately: a small name and a way to forget them all on one line, and under
/// it a quiet pill each (a press searches it again, its cross forgets it).
fn history(s: &State) -> El {
    let lang = s.lang;
    let clear = row()
        .key("sh-clear")
        .role("button")
        .label(S::ClearHistory.get(lang))
        .h(24.0)
        .px(8.0)
        .radius(12.0)
        .items_center()
        .cursor(Cursor::Pointer)
        .hover(th::veil(0.06))
        .press(th::veil(0.10))
        .on_press(|s: &mut State| s.clear_search_history())
        .child(label(S::ClearHistory.get(lang).to_owned(), th::SMALL, th::text3()));
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
            .group_opacity(0.0, 1.0)
            .hover(th::veil(0.10))
            .press(th::veil(0.16))
            .on_press(move |s: &mut State| s.forget_search(&gone))
            .child(icon(&icons::CLOSE, 12.0).color(th::text3()));
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
                .hover(th::veil(0.05))
                .press(th::veil(0.09))
                .hover_group()
                .transition(Transition::opacity().combined(Transition::scale(0.9)).springs(th::SOFT))
                .on_press(move |s: &mut State| s.search_again(&again))
                .child(icon(&icons::CLOCK, 14.0).color(th::text3()).group_color(th::text2()))
                .child(label(q.clone(), th::BODY, th::text2()).group_color(th::text()).truncate().max_w(240.0))
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

/// A mood as a tile: a dark plate with a breath of the mood's colour along it, its name on the
/// left and, whole on the right, the same dithered shape the mood has on Home, which comes alive
/// under the pointer. The one My Wave plays wears a ring.
fn mood_tile(s: &State, id: &'static str, w: f32, h: f32) -> El {
    let on = s.wave_choice == id;
    let name = wave_title(id).get(s.lang);
    let hue = wave_hue(id);
    let seed = (id.bytes().fold(5u32, |a, b| a.wrapping_mul(33).wrapping_add(b as u32)) % 997) as f32 + 0.5;
    let hot = s.garden_hover == Some(id);
    let side = (h * 0.84).floor();
    let tint = |a: f32| hue_color(hue).alpha(a);
    let art = blobs::shape(format!("wt-shape-{id}"), mood_family(id), seed, Fill::Hue(hue), Look { chosen: on, hovered: hot, order: 0 }, side, side)
        .absolute(w - side - 10.0, (h - side) * 0.5);
    row()
        .key(format!("wt-{id}"))
        .role("button")
        .label(name)
        .checked(on)
        .w(w)
        .h(h)
        .pl(18.0)
        .items_center()
        .radius(18.0)
        .cursor(Cursor::Pointer)
        .press_scale(th::PRESS)
        .linear_gradient(0.0, [(0.0, tint(0.05)), (1.0, tint(if hot { 0.24 } else { 0.15 }))])
        .border(if on { 2.0 } else { 1.0 }, if on { th::text() } else { tint(0.22) })
        .hover_group()
        .transition(Transition::none())
        .on_hover_change(garden_hover(id))
        .on_press(move |s: &mut State| start_wave_with(s, id))
        .child(art)
        .child(
            col()
                .gap(3.0)
                .max_w(w - side - 30.0)
                .child(label(name.to_owned(), th::H3, th::text()).truncate())
                .child(row().gap(5.0).items_center().group_opacity(0.0, 1.0).child(icon(&icons::PLAY, 11.0).color(th::text2())).child(label(S::Listen.get(s.lang).to_owned(), th::SMALL, th::text2()))),
        )
}

/// The moods in an even grid (four across, two in a narrow page), then what Home has to offer
/// from the chart and the new releases.
fn suggestions(s: &State, w: f32) -> El {
    let lang = s.lang;
    let cw = column(w);
    let gap = 12.0;
    let cols = if cw >= 640.0 { 4.0 } else { 2.0 };
    let tile = ((cw - gap * (cols - 1.0)) / cols).floor();
    let mut tiles = row().key("sugg-tiles").w_full().gap(gap).flex_wrap().transition(Transition::none());
    for (i, preset) in WAVE_PRESETS.iter().enumerate().take(8) {
        tiles = tiles.child(kit::rise(mood_tile(s, preset.id, tile, 92.0), 3 + i / 4));
    }
    let mut out = col()
        .key("sugg")
        .gap(36.0)
        .mt(4.0)
        .transition(Transition::none())
        .child(col().gap(14.0).child(kit::rise(kit::heading("sugg-h", S::Browse.get(lang), None), 2)).child(tiles));
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
    let h = (rows * (ROW_H + 2.0) - 2.0).max(236.0);
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
            .hover(th::veil(0.035))
            .press(th::veil(0.07))
            .hover_group()
    };
    let kind_line = |kind: &str, sub: &str| {
        let mut r = row().gap(10.0).items_center().min_w(0.0).child(row().h(24.0).px(10.0).radius(12.0).items_center().fill(th::veil(0.09)).child(label(kind.to_owned(), th::SMALL, th::text())));
        if !sub.is_empty() {
            r = r.child(label(sub.to_owned(), th::BODY, th::text2()).truncate().min_w(0.0));
        }
        r
    };
    let play = |key: &str| kit::round(key.to_owned(), kit::Weight::Solid, &icons::PLAY, 52.0, 22.0, S::Play.get(lang)).absolute(0.0, 0.0).group_opacity(0.0, 1.0);
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
        el().absolute(0.0, 0.0).w_full().h_full().child(
            row().pin(None, None, Some(0.0), Some(0.0)).w(52.0).h(52.0).child(play("sr-best-play").on_press(move |s: &mut State| s.play_from_list(ListSrc::Search, &id2))),
        ),
    );
    let card = base(format!("sr-best-tr-{}", t.id), &t.full_title())
        .on_press(move |s: &mut State| s.play_from_list(ListSrc::Search, &id))
        .child(kit::cover(s, t.cover_template(), 104.0, 16.0))
        .child(
            el().w_full().child(corner).child(
                col().gap(10.0).min_w(0.0).pr(64.0).child(label(t.full_title(), th::H0, th::text()).truncate().min_w(0.0)).child(kind_line(S::Track.get(lang), &t.artists_line())),
            ),
        );
    Some(card)
}

// ---- library -----------------------------------------------------------------------------------------

/// The collection: its title, a chip for each kind, then what was liked of that kind. Liked tracks
/// open with a banner (the heart, how many there are and how long they play, play and shuffle);
/// the other kinds are grids of cards.
fn liked_playlist_cover(s: &State, side: f32, radius: f32) -> El {
    let cell = side * 0.5;
    let mut art = el().key("lib-pic").w(side).h(side).radius(radius).clip_rounded(radius).fill(th::veil(0.08));
    let mut count = 0;
    if let Some(tracks) = s.library.tracks.get() {
        for (i, track) in tracks.iter().take(4).enumerate() {
            let x = if i % 2 == 0 { 0.0 } else { cell };
            let y = if i < 2 { 0.0 } else { cell };
            art = art.child(
                el()
                    .absolute(x, y)
                    .w(cell + 1.0)
                    .h(cell + 1.0)
                    .fill(th::srgb(s.palette.colors[i]))
                    .child(kit::cover(s, track.cover_template(), cell + 1.0, 0.0)),
            );
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
    let tabs = kit::tab_track("ltabs", tabs.into_iter().map(|(t, name, n)| kit::tab_pill(format!("ltab-{t:?}"), name.get(lang), n, tab == t).on_press(move |s: &mut State| s.load_library_tab(t))).collect());
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
    let mut field = kit::field_surface(row()).key("lib-search-field").w(260.0).h(42.0).pl(12.0).pr(4.0).gap(8.0).radius(13.0).items_center().child(icon(&icons::SEARCH, 18.0).color(th::text3())).child(input);
    if !q.is_empty() {
        field = field.child(kit::ibtn("lib-search-clear", &icons::CLOSE, 16.0, 32.0, th::text2(), S::ClearSearch.get(lang)).on_press(|s: &mut State| s.library.filter.clear()));
    }
    let cw = column(w);
    let bar = if cw >= 700.0 {
        row().key("lib-bar").w_full().gap(16.0).items_center().child(tabs).child(el().grow(1.0)).child(field)
    } else {
        col().key("lib-bar").w_full().gap(12.0).child(tabs).child(field.w_full())
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
    let side = if cw >= 640.0 { 150.0 } else { 104.0 };
    let r = kit::squircle_r(side) * 0.5;
    let pic = liked_playlist_cover(s, side, r);
    let mut meta = col().key("lib-meta").grow(1.0).min_w(0.0).gap(8.0).child(kit::caps(S::Playlist.get(lang), th::text3())).child(label(S::Liked.get(lang).to_owned(), th::TITLE_D, th::text()));
    if !lead.is_empty() {
        meta = meta.child(label(lead, th::BYLINE, th::text3()));
    }
    if liked.is_some() {
        let buttons = row()
            .key("lib-buttons")
            .gap(10.0)
            .items_center()
            .mt(16.0)
            .child(kit::btn_primary("lib-play", S::Play.get(lang), Some(&icons::PLAY)).on_press(|s: &mut State| {
                let list = s.list(ListSrc::Liked);
                s.play_all(list, false);
            }))
            .child(kit::round("lib-shuffle", kit::Weight::Soft, &icons::SHUFFLE, kit::BTN_H, 20.0, S::Shuffle.get(lang)).on_press(|s: &mut State| {
                let list = s.list(ListSrc::Liked);
                s.play_all(list, true);
            }))
            .child(kit::wave_button(s, "lib-wave", S::WaveByCollection.get(lang), S::LikedTracks.get(lang), crate::app::Station::Collection, kit::BTN_H));
        let buttons = buttons.child(kit::pill_icon("lib-download", kit::Weight::Soft, &icons::DOWNLOAD, kit::BTN_H, 20.0, S::DownloadAllLiked.get(lang)).on_press(|s: &mut State| {
            let list = s.library.tracks.get().cloned().unwrap_or_default();
            s.ask_download(list);
        }));
        meta = meta.child(buttons);
    }
    let head = if cw < 640.0 {
        col().key("lib-head").w_full().gap(18.0).child(pic).child(meta.w_full())
    } else {
        row().key("lib-head").w_full().gap(28.0).items_start().child(pic).child(meta)
    };
    let body = col().gap(32.0).child(kit::rise(head.mt(12.0), 0)).child(kit::rise(bar, 1));
    let (_, card) = fit(w, 168.0, 20.0, 8);
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
            other => card_skeletons(other, s, card),
        },
        LibTab::Albums => match &s.library.albums {
            Load::Ready(v) if v.is_empty() => nothing(Ill::Shelf),
            Load::Ready(v) => grid(v.iter().filter(|x| has(&[x.title.as_str(), &x.artists_line()])).map(|x| album_card(s, x, card)).collect()),
            other => card_skeletons(other, s, card),
        },
        LibTab::Artists => match &s.library.artists {
            Load::Ready(v) if v.is_empty() => nothing(Ill::Shelf),
            Load::Ready(v) => grid(v.iter().filter(|x| has(&[x.name.as_str()])).map(|x| artist_card(s, x, card)).collect()),
            other => card_skeletons(other, s, card),
        },
    };
    kit::scroll_page("library", body.child(kit::tab_body(format!("lib-tab-{tab:?}"), content)))
}

/// A grid of placeholder cards while it loads, or what went wrong.
fn card_skeletons<T>(state: &Load<T>, s: &State, card: f32) -> El {
    if matches!(state, Load::Failed(_))
        && let Some(e) = kit::list_state(state, s, 0)
    {
        return e;
    }
    grid((0..8).map(|i| col().key(format!("sk-card-{i}")).gap(12.0).child(kit::skeleton(card, card, 16.0)).child(kit::skeleton(card * 0.7, 12.0, 6.0))).collect())
}

fn track_list(key: &'static str, n: usize, src: ListSrc) -> El {
    col().key(key).w_full().growth(GrowthSpec::default()).virtual_rows(
        n,
        ROW_H,
        move |s: &State, i: usize| Cow::Owned(s.list_id(src, i)),
        move |s: &State, i: usize| match s.list_track(src, i) {
            Some(t) => kit::track_row(s, t, i + 1, src),
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

/// The side of the cover (or the artist's shape) at the head of the page.
const HEAD_SIDE: f32 = 184.0;


/// The picture at the head of the page: the cover, or the artist in a shape of their own (a blob
/// that slowly moves, as the moods on Home do). Pressing it opens it large.
fn head_picture(s: &State, key: &str) -> El {
    let lang = s.lang;
    let d = &s.detail;
    let side = HEAD_SIDE;
    let radius = if d.is_artist { side * 0.5 } else { 18.0 };
    if d.state.is_loading() || d.page.is_none() {
        return kit::skeleton(side, side, radius);
    }
    // While the cover is opened large it is not here as well: its place holds an empty slot it
    // flies out of and back into (`menu::lightbox`), so there is never a second picture.
    let lifted = s.lightbox_visible() && s.lightbox.as_ref().is_some_and(|(_, from)| from == "detail-cover");
    if lifted {
        return el().key("detail-cover-gap").w(side).h(side).radius(radius).fill(th::veil(0.05)).transition(Transition::none());
    }
    let Some(tpl) = d.cover.clone() else { return kit::cover(s, None, side, radius) };
    let base = el()
        .key("detail-cover")
        .role("button")
        .label(S::OpenCover.get(lang))
        .help(S::OpenCover.get(lang))
        .w(side)
        .h(side)
        .cursor(Cursor::Pointer)
        .transition(Transition::none());
    if d.is_artist {
        // The picture is drawn by the shape's shader, which reads it from the art cache: large
        // enough for the photo viewer, which opens this same shape out into a rounded square.
        let pic = State::cover_key(Some(&tpl), 1000);
        if let Some(k) = &pic {
            let _ = s.art.pixels(k);
            let _ = s.art.full(k);
        }
        s.pic_key.replace(pic);
        let (seed, hue) = blobs::avatar_seed(key);
        let look = Look { hovered: s.garden_hover == Some("detail"), ..Look::default() };
        return base
            .on_hover_change(garden_hover("detail"))
            .on_press(move |s: &mut State| s.open_lightbox(tpl.clone(), "detail-cover"))
            .child(blobs::avatar(format!("detail-blob-{key}"), seed, Fill::Picture(hue), look, 0.0, side).absolute(0.0, 0.0));
    }
    base.radius(radius)
        .press_scale(0.98)
        .hover_group()
        .shadow(12.0, 34.0, th::rgba(0x000000, if th::light() { 0.14 } else { 0.40 }))
        .on_press(move |s: &mut State| s.open_lightbox(tpl.clone(), "detail-cover"))
        .child(kit::cover(s, d.cover.as_deref(), side, radius))
        .child(el().absolute(0.0, 0.0).w_full().h_full().radius(radius).fill(th::rgba(0xffffff, 0.06)).group_opacity(0.0, 1.0))
}

/// What it is, under its tracks: the genre, how many tracks and how long (an artist: their genres,
/// albums and similar artists), each a quiet name over its value, then a playlist's description.
/// The year is at the head, beside who made it.
fn detail_facts(s: &State) -> Option<El> {
    let d = &s.detail;
    let mut cells = row().key("d-facts").w_full().gap(40.0).flex_wrap();
    let mut any = false;
    let mut about = None;
    for (i, (name, value)) in d.facts.iter().enumerate() {
        match name {
            S::Year => continue,
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

/// The compact page header keeps the primary actions visible. Secondary actions live in the
/// three-dot menu so release and artist headers keep the same order at every width. Downloads stay
/// beside that menu because they are the one frequent action that should not be hidden.
/// The tracks are read from the state when a button is pressed, not copied into every build of
/// the page (an artist's whole list, twice, on every cover that landed).
fn head_buttons(s: &State) -> El {
    let lang = s.lang;
    let d = &s.detail;
    let mut buttons = row().key("d-buttons").gap(10.0).items_center().flex_wrap();
    // My Wave by the artist or by the release is the first action. A playlist has no station
    // seed, so its equivalent is the ordinary play-all action.
    let wave = match &d.page {
        Some(Page::Artist(id)) => Some(kit::wave_button(s, "d-wave", S::WaveByArtist.get(lang), &d.title, crate::app::Station::Artist(id.clone()), kit::BTN_H)),
        Some(Page::Album(id)) => Some(kit::wave_button(s, "d-wave", S::WaveByRelease.get(lang), &d.title, crate::app::Station::Album(id.clone()), kit::BTN_H)),
        _ => None,
    };
    if !d.tracks.is_empty() {
        buttons = buttons.child(kit::btn_primary("d-play", S::Play.get(lang), Some(&icons::PLAY)).on_press(|s: &mut State| {
            let list = s.detail.tracks.clone();
            s.play_all(list, false);
        }));
    }
    if let Some(w) = wave {
        buttons = buttons.child(w);
    }
    if d.page.is_some() {
        let pin_on = s.detail_is_pinned();
        let pin_name = if pin_on { S::Pinned } else { S::Pin };
        let pin_icon = if pin_on { &icons::PIN_OFF } else { &icons::PIN };
        let mut pin = kit::pill_icon("d-pin", kit::Weight::Soft, pin_icon, kit::BTN_H, 20.0, pin_name.get(lang)).checked(pin_on);
        if pin_on {
            pin = pin.fill(th::veil(0.14));
        }
        buttons = buttons.child(
            pin.on_press(|s: &mut State| s.toggle_detail_pin()),
        );
    }
    let on = s.popover_open && matches!(s.popover, Some(crate::app::Popover::Actions { .. } | crate::app::Popover::About { .. }));
    buttons = buttons.child(
        kit::pill_icon("d-more", kit::Weight::Soft, &icons::MORE, kit::BTN_H, 20.0, S::More.get(lang))
            .checked(on)
            .on_press(|s: &mut State| s.open_popover(crate::app::Popover::Actions { anchor: "d-more".into() })),
    );
    if !d.tracks.is_empty() {
        let list = d.tracks.clone();
        buttons = buttons.child(kit::pill_icon("d-download", kit::Weight::Soft, &icons::DOWNLOAD, kit::BTN_H, 20.0, S::DownloadAll.get(lang)).on_press(move |s: &mut State| s.ask_download(list.clone())));
    }
    buttons
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

    if !all_mode {
        let mut meta = col().key("d-meta").grow(1.0).min_w(0.0).gap(8.0).child(kit::caps(kind.get(lang), th::text3())).child(
            label(if d.title.is_empty() { "…".to_owned() } else { d.title.clone() }, th::TITLE_D, th::text()).max_lines(2).w_full(),
        );
        // Who made it, each a link to their page, and the year beside them.
        let mut by = row().key("d-by").w_full().min_w(0.0).gap(10.0).items_center();
        let mut has_by = false;
        if !d.artists.is_empty() {
            by = by.child(kit::artists_row_in_width(s, &d.artists, th::BYLINE, false, "d-art", th::text().alpha(0.88), (w - 260.0).max(180.0)).shrink());
            has_by = true;
        } else if !d.subtitle.is_empty() {
            by = by.child(label(d.subtitle.clone(), th::BYLINE, th::text().alpha(0.88)).truncate().min_w(0.0));
            has_by = true;
        }
        if let Some(listeners) = d.monthly_listeners {
            if has_by {
                by = by.child(label("·", th::BYLINE, th::text3()));
            }
            by = by.child(label(format_listeners(listeners, lang), th::BYLINE, th::text3()).truncate().min_w(0.0));
            has_by = true;
        }
        if let Some(y) = d.year {
            if has_by {
                by = by.child(label("·", th::BYLINE, th::text3()));
            }
            by = by.child(label(y.to_string(), th::BYLINE, th::text3()));
            has_by = true;
        }
        if has_by {
            meta = meta.child(by);
        }
        if !d.tracks.is_empty() || d.like_key.is_some() || !d.facts.is_empty() {
            meta = meta.child(head_buttons(s).mt(16.0));
        }
        let wave: Option<El> = None;
        // The head folds away (it shrinks back and fades) when "All" is pressed, and the list
        // under it rises into its place.
        let head = if column(w) < 700.0 {
            col().key("detail-head").w_full().mt(8.0).gap(18.0).transition(Transition::asymmetric(Transition::none(), Transition::opacity().combined(Transition::scale(0.94)).combined(Transition::offset(0.0, -16.0))).springs(th::RISE)).child(head_picture(s, &key)).child(kit::rise(meta.w_full().min_w(0.0), 1)).children(wave)
        } else {
            row().key("detail-head").w_full().mt(8.0).gap(32.0).items_center().transition(Transition::asymmetric(Transition::none(), Transition::opacity().combined(Transition::scale(0.94)).combined(Transition::offset(0.0, -16.0))).springs(th::RISE)).child(head_picture(s, &key)).child(kit::rise(meta.grow(1.0).min_w(0.0), 1)).children(wave)
        };
        body = body.child(head);
    }

    match &d.state {
        Load::Ready(()) => {
            let (_, card) = fit(w, 168.0, 20.0, 8);
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
                        .child(kit::btn_small("d-all-play", S::Play.get(lang), Some(&icons::PLAY)).on_press(|s: &mut State| {
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
                    let more = d.is_artist.then(|| {
                        let mut m = kit::more_link("d-all", S::All.get(lang)).on_press_tx(|s: &mut State, tx: &mut Tx| {
                            s.show_all_artist_tracks(true);
                            tx.scroll_to_y(s.page.key(), 0.0);
                        });
                        if d.all_tracks.is_loading() {
                            m = m.child(icon(&icons::SPINNER, 14.0).color(th::text3()).looping(rune::LoopSpec::rotation(0.9)));
                        }
                        m
                    });
                    col().key("d-tracks").gap(10.0).child(kit::list_heading("d-tracks-h", title.get(lang), more)).child(list)
                };
                body = body.child(kit::rise(section, 2));
            }
            if !all_mode && let Some(f) = detail_facts(s) {
                body = body.child(kit::rise(f, 3));
            }
            if !all_mode && !d.albums.is_empty() {
                body = body.child(
                    col()
                        .key("d-albums")
                        .gap(14.0)
                        .transition(Transition::opacity().springs(th::RISE))
                        .child(kit::heading("d-albums-h", S::Discography.get(lang), row_arrows(s, "detail-albums", d.albums.len(), card, w)))
                        .child(hscroll("detail-albums".into(), d.albums.iter().map(|a| album_card(s, a, card)).collect())),
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

pub(crate) fn format_listeners(n: u64, lang: crate::i18n::Lang) -> String {
    let value = if n >= 1_000_000 { format!("{:.1}M", n as f64 / 1_000_000.0) } else if n >= 1_000 { format!("{:.1}K", n as f64 / 1_000.0) } else { n.to_string() };
    format!("{value} {}", S::MonthlyListeners.get(lang).to_lowercase())
}
