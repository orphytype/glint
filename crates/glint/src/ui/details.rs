//! The card that tells about a track, an album, a playlist or an artist. It grows out of the
//! cover it is about: the cover leaves its row (or the player, or the page's head) and settles at
//! the top of the card's left side while the card opens around it; closing puts it back. The card
//! can be pulled down by its cover or its handle: a short pull springs back, a long one or a
//! flick closes it.
//!
//! It only tells: no play, no heart, no download (the player and the pages do those). Left: the
//! cover, what it is, its name and who made it. Right, as the service tells it: a track's credits
//! (who plays, who wrote the music and the words, the label) and the release it is from; an
//! artist's listeners this month and how that changed, their story, the playlists they are in,
//! their pages and photos, and artists like them; an album's or a playlist's facts.

use super::kit::Hover as _;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use rune::{Align, Color, Cursor, Cx, Drag, El, Phase, Transition, Tx, col, el, row};
use ym_api::Track;

use super::icons;
use super::kit::{self, label};
use super::menu::{link_title, track_link};
use crate::app::{Load, Page, Sheet, State};
use crate::fx::{self, Slot as _};
use crate::i18n::{Lang, S};
use crate::theme as th;

/// The card at most, its inner margin and its corners.
const MAX_W: f32 = 880.0;
const MAX_H: f32 = 620.0;
const PAD: f32 = 28.0;
const RADIUS: f32 = 26.0;
/// The cover's corners in the card (a square one; an artist's is round).
const COVER_R: f32 = 16.0;
/// The round buttons at the top right.
const ROUND: f32 = 34.0;
/// Room between the sections on the right.
const SECTION_GAP: f32 = 28.0;
/// A similar artist in the grid: the least room it takes, its picture and its height.
const SIM_MIN_W: f32 = 100.0;
const SIM_PIC: f32 = 68.0;
const SIM_H: f32 = SIM_PIC + 10.0 + 36.0;
const SIM_GAP: f32 = 10.0;
/// The photos: as many to a row, and the most shown.
const PHOTO_COLS: usize = 3;
const PHOTO_MAX: usize = 6;
/// A rise and a fall in listeners.
const UP: u32 = 0x4fd18b;
const DOWN: u32 = 0xf07c7c;

/// A cover on screen and its corner radius.
type Spot = ([f32; 4], f32);

thread_local! {
    /// Where the card of the sheet opened `seq`-th came from, as last seen (the thing may scroll
    /// or go while the card is up; it then goes back to where it was last seen).
    static FROM: RefCell<(u64, Option<Spot>)> = const { RefCell::new((0, None)) };
}

/// The cover the card is about, on screen now: the track's row, the player's or the bar's cover
/// of the playing track, or the head of the page. With its corner radius.
fn source_now(s: &State, cx: &Cx<State>, what: &Sheet) -> Option<Spot> {
    match what {
        Sheet::Track(t) => {
            let playing = s.current().is_some_and(|c| c.id == t.id);
            if playing
                && s.np_open
                && let Some(r) = cx.window_layout("np-cover-morph")
            {
                return Some((r, 28.0));
            }
            if let Some(r) = cx.window_layout(&format!("row-{}", t.id)) {
                let c = kit::ROW_COVER;
                return Some(([r[0] + kit::ROW_PAD, r[1] + (r[3] - c) * 0.5, c, c], kit::ROW_COVER_R));
            }
            if playing {
                return cx.window_layout("bar-cover").map(|r| (r, 10.0));
            }
            None
        }
        Sheet::Page => cx.window_layout("detail-cover").map(|r| {
            if !s.detail.is_artist {
                return (r, 16.0);
            }
            // An artist's circle is drawn by the shape's shader a little inside its box. The card
            // starts from the circle as it is seen, so the photo doesn't jump in size as it leaves
            // the page and as it lands back on it.
            let rad = super::blobs::radius(super::blobs::Family::Avatar, r[2]);
            let (cx, cy) = (r[0] + r[2] * 0.5, r[1] + r[3] * 0.5);
            ([cx - rad, cy - rad, 2.0 * rad, 2.0 * rad], rad)
        }),
    }
}

fn source(s: &State, cx: &Cx<State>, what: &Sheet) -> Option<Spot> {
    let now = source_now(s, cx, what);
    FROM.with_borrow_mut(|(seq, last)| {
        if *seq != s.sheet_seq {
            *seq = s.sheet_seq;
            *last = None;
        }
        if now.is_some() {
            *last = now;
        }
        *last
    })
}

/// Pulling past where something can go: the further, the less it moves (`c` 0.55, over `d`).
fn rubber(x: f32, d: f32) -> f32 {
    (1.0 - 1.0 / (0.55 * x / d + 1.0)) * d
}

/// Where a flick at `v` points a second would coast to from here (deceleration 0.998 a ms).
fn project(v: f32) -> f32 {
    v / 1000.0 * 0.998 / (1.0 - 0.998)
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// How far a piece that starts coming in at `from` (of the opening, 0..1) has come at `p`: it
/// always arrives when the card does, so a later piece only comes in faster, never late.
fn arrival(p: f32, from: f32) -> f32 {
    smooth((p - from) / (1.0 - from).max(0.05))
}

/// What a tap on a handle does (a press that hardly moved).
type Tap = Rc<dyn Fn(&mut State, &mut Tx)>;

/// Makes `e` a handle the card is pulled down by; a tap on it does `tap`.
fn pull_handle(e: El, ch: f32, tap: Option<Tap>) -> El {
    let cursor = if tap.is_some() { Cursor::Pointer } else { Cursor::Grab };
    e.cursor(cursor).on_drag(move |s: &mut State, d: &mut Drag, tx: &mut Tx| match d.phase {
        Phase::Down => {
            // Caught mid-flight: it goes on from where it is.
            s.sheet_dragging = true;
            d.data = s.sheet_drag.t;
            s.sheet_drag.v = 0.0;
            s.sheet_drag_vel = 0.0;
            s.sheet_drag_mark = Some((Instant::now(), s.sheet_drag.t));
        }
        Phase::Move => {
            let raw = d.data + d.dy;
            let y = if raw >= 0.0 { raw } else { -rubber(-raw, ch) };
            let now = Instant::now();
            if let Some((t0, y0)) = s.sheet_drag_mark {
                let dt = now.duration_since(t0).as_secs_f32();
                if dt > 0.004 {
                    let v = (y - y0) / dt;
                    s.sheet_drag_vel = 0.6 * v + 0.4 * s.sheet_drag_vel;
                    s.sheet_drag_mark = Some((now, y));
                }
            }
            s.sheet_drag.t = y;
        }
        Phase::Up => {
            s.sheet_dragging = false;
            if !d.cancelled()
                && d.dx.abs() < 4.0
                && d.dy.abs() < 4.0
                && let Some(tap) = &tap
            {
                s.sheet_drag.v = 0.0;
                tap(s, tx);
                return;
            }
            // A pull that stopped a while ago carries no speed.
            let still = s.sheet_drag_mark.is_some_and(|(t0, _)| t0.elapsed().as_secs_f32() > 0.1);
            let v = if still || d.cancelled() { 0.0 } else { s.sheet_drag_vel };
            let landing = s.sheet_drag.t + project(v);
            if !d.cancelled() && d.dy > 6.0 && landing > ch * 0.24 {
                s.sheet_open = false;
            }
            s.sheet_drag.v = v;
        }
    })
}

/// Closes the card and opens `page` behind it.
fn go_to(s: &mut State, page: Page) {
    s.sheet_open = false;
    s.go(page);
}

/// A round button at the card's top right: a glyph on a soft disc; what it does is its tooltip.
fn round_btn(key: &str, ic: &'static rune::Icon, name: &str) -> El {
    row()
        .key(key.to_owned())
        .role("button")
        .label(name)
        .help(name)
        .w(ROUND)
        .h(ROUND)
        .radius(ROUND * 0.5)
        .center()
        .cursor(Cursor::Pointer)
        .fill(th::veil(0.07))
        .hov(th::veil(0.13))
        .press(th::veil(0.17))
        .press_scale(th::PRESS)
        .hov_group()
        .child(rune::icon(ic, 15.0).color(th::text2()).gcol(th::text()))
}

/// A section of the right side: its quiet name over what it holds.
fn section(key: &str, name: &str, body: El) -> El {
    col().key(key.to_owned()).w_full().gap(12.0).child(kit::caps(name, th::text3())).child(body)
}

/// One credit or fact: its name in the ink, its value under it in a softer one, as long as it
/// needs to be.
fn fact_line(key: String, name: &str, value: &str, width: f32) -> El {
    col().key(key).w(width).gap(3.0).child(label(name.to_owned(), th::BODY_M, th::text()).wrap().w(width)).child(label(value.to_owned(), th::BODY, th::text2()).wrap().w(width))
}

/// Lines in waiting, while the credits load.
fn bones(key: &str, n: usize, width: f32) -> El {
    let widths = [0.42, 0.78, 0.5, 0.66, 0.36];
    col().key(key.to_owned()).w(width).gap(10.0).children((0..n).map(|i| {
        el().w(width * widths[i % widths.len()]).h(if i % 2 == 0 { 14.0 } else { 12.0 }).radius(6.0).fill(th::veil(0.08)).looping(rune::LoopSpec::opacity(
            1.4,
            0.08 * i as f64,
            0.45,
            1.0,
        ))
    }))
}

/// A row that leads somewhere: a picture (or a glyph on a disc), a title and a quieter line, and
/// an arrow; the whole row is the button.
fn lead_row(key: &str, pic: El, title: String, sub: Option<String>, out: bool, width: f32) -> El {
    // The row's padding, the picture, two gaps and the arrow.
    let text_w = width - 20.0 - 52.0 - 24.0 - 20.0;
    let mut words = col().grow(1.0).min_w(0.0).gap(2.0).child(label(title, th::BODY_M, th::text()).truncate().w(text_w));
    if let Some(sub) = sub.filter(|x| !x.is_empty()) {
        words = words.child(label(sub, th::SMALL, th::text3()).truncate().w(text_w));
    }
    row()
        .key(key.to_owned())
        .role("link")
        .w(width)
        .px(10.0)
        .py(10.0)
        .gap(12.0)
        .items_center()
        .radius(16.0)
        .cursor(Cursor::Pointer)
        .fill(th::veil(0.045))
        .hov(th::veil(0.08))
        .press(th::veil(0.11))
        .press_scale(th::PRESS)
        .hov_group()
        .child(pic)
        .child(words)
        .child(row().w(20.0).h(20.0).center().child(rune::icon(if out { &icons::EXTERNAL } else { &icons::CHEVRON_RIGHT }, 16.0).color(th::text3()).gcol(th::text())))
}

/// A glyph on a coloured disc, the picture of a row that leads out of the app.
fn glyph_disc(ic: &'static rune::Icon, fill: Color) -> El {
    row().w(52.0).h(52.0).radius(26.0).center().fill(fill).child(rune::icon(ic, 20.0).color(th::rgb(0xffffff)))
}

// ---- a track ---------------------------------------------------------------------------------------

/// Who made the track, as the service credits them; while they load, lines in waiting; when the
/// service has none, its performers at least.
fn credits(s: &State, t: &Track, width: f32) -> El {
    let lang = s.lang;
    let mine = s.sheet_credits.0.as_deref() == Some(t.id.as_str());
    let lines: Vec<(String, String)> = match &s.sheet_credits.1 {
        Load::Ready(c) if mine && !c.is_empty() => c.iter().map(|c| (c.title.clone(), c.value.clone())).collect(),
        Load::Loading | Load::Idle if mine || s.sheet_credits.0.is_none() => return section("sheet-credits", S::SheetCredits.get(lang), bones("sheet-credits-bones", 6, width)),
        _ => vec![(S::Artist.get(lang).to_owned(), t.artists_line())],
    };
    let list = col().w(width).gap(14.0).children(lines.iter().enumerate().map(|(i, (n, v))| fact_line(format!("sheet-credit-{i}"), n, v, width)));
    section("sheet-credits", S::SheetCredits.get(lang), list)
}

/// The release the track is from, as a row that opens it.
fn release(s: &State, t: &Track, width: f32) -> Option<El> {
    let al = t.albums.first()?;
    let lang = s.lang;
    let mut sub = vec![S::Album.get(lang).to_owned()];
    if let Some(y) = al.year {
        sub.push(y.to_string());
    }
    if let Some(n) = al.track_count.filter(|n| *n > 1) {
        sub.push(format!("{n} {}", S::Tracks.get(lang).to_lowercase()));
    }
    let page = Page::Album(al.id.clone());
    let pic = el().w(52.0).h(52.0).radius(10.0).child(kit::cover(s, al.cover_template().or(t.cover_template()), 52.0, 10.0));
    let r = lead_row("sheet-release", pic, al.title.clone(), Some(sub.join(" · ")), false, width).on_press(move |s: &mut State| go_to(s, page.clone()));
    Some(section("sheet-release-sec", S::FromRelease.get(lang), r))
}

// ---- an artist -------------------------------------------------------------------------------------

/// The listeners this month, large, and how that changed against the 30 days before.
fn listeners(s: &State, width: f32) -> Option<El> {
    let n = s.detail.monthly_listeners?;
    let lang = s.lang;
    let mut c = col()
        .key("sheet-listeners")
        .w(width)
        .gap(6.0)
        .child(label(S::ListenersMonth.get(lang).to_owned(), th::BODY_M, th::text2()))
        .child(label(crate::i18n::group_digits(n), th::RELEASE, th::text()).key("sheet-listeners-n"));
    if let Some(d) = s.detail.listeners_delta.filter(|d| *d != 0) {
        let (ink, rest) = if d > 0 { (th::rgb(UP), S::ListenersMore) } else { (th::rgb(DOWN), S::ListenersFewer) };
        let num = crate::i18n::group_digits(d.unsigned_abs());
        let mut line = row().w(width).gap(4.0).flex_wrap();
        let by = S::ListenersBy.get(lang);
        if !by.is_empty() {
            line = line.child(label(by.to_owned(), th::BODY_M, th::text2()));
        }
        line = line.child(label(num, th::BODY_M, ink));
        for word in rest.get(lang).split(' ') {
            line = line.child(label(word.to_owned(), th::BODY_M, th::text2()));
        }
        c = c.child(line);
    }
    Some(c)
}

/// Where the artist is besides: the playlists they are in (band.link), their own pages, and a way
/// to support them.
fn elsewhere(s: &State, width: f32) -> Option<El> {
    let lang = s.lang;
    let mut c = col().w(width).gap(14.0);
    let mut any = false;
    if let Some(link) = s.detail.bandlink.clone() {
        let url = link.url.clone();
        let pic = glyph_disc(&icons::QUEUE, th::rgb(0xe8394a));
        c = c.child(lead_row("sheet-bandlink", pic, link.title.clone(), link.subtitle.clone(), true, width).help(link.url.as_str()).on_press_tx(
            move |_s: &mut State, tx: &mut Tx| {
                let _ = tx.open_url(&url);
            },
        ));
        any = true;
    }
    if !s.detail.links.is_empty() || s.detail.donation.is_some() {
        let mut pills = row().w(width).gap(8.0).flex_wrap();
        if let Some(url) = s.detail.donation.clone() {
            pills = pills.child(link_pill("sheet-donate", &icons::HEART_FILL, S::SupportArtist.get(lang).to_owned(), &url, true));
        }
        for (i, l) in s.detail.links.iter().enumerate() {
            pills = pills.child(link_pill(&format!("sheet-link-{i}"), &icons::EXTERNAL, link_title(l), &l.href, false));
        }
        c = c.child(pills);
        any = true;
    }
    any.then(|| section("sheet-elsewhere", S::ArtistLinks.get(lang), c))
}

/// One of the artist's pages as a small pill (the way to support them stands out).
fn link_pill(key: &str, ic: &'static rune::Icon, title: String, url: &str, accent: bool) -> El {
    let url = url.to_owned();
    let h = 34.0;
    row()
        .key(key.to_owned())
        .role("link")
        .label(title.as_str())
        .help(url.as_str())
        .h(h)
        .pl(11.0)
        .pr(13.0)
        .gap(7.0)
        .radius(h * 0.5)
        .items_center()
        .cursor(Cursor::Pointer)
        .fill(if accent { th::rgba(0xe8394a, 0.22) } else { th::veil(0.06) })
        .hov(if accent { th::rgba(0xe8394a, 0.32) } else { th::veil(0.1) })
        .press_scale(th::PRESS)
        .on_press_tx(move |_s: &mut State, tx: &mut Tx| {
            let _ = tx.open_url(&url);
        })
        .child(rune::icon(ic, 13.0).color(if accent { th::rgb(0xff8a95) } else { th::text2() }))
        .child(label(title, th::BODY_M, th::text()))
}

/// The artist's photos, three to a row.
fn photos(s: &State, width: f32) -> Option<El> {
    let list: Vec<&String> = s.detail.photos.iter().take(PHOTO_MAX).collect();
    if list.len() < 2 {
        return None;
    }
    let gap = 8.0;
    let cell = ((width - gap * (PHOTO_COLS as f32 - 1.0)) / PHOTO_COLS as f32).floor();
    let mut grid = row().key("sheet-photos").w(width).gap(gap).flex_wrap();
    for (i, tpl) in list.into_iter().enumerate() {
        grid = grid.child(el().key(format!("sheet-photo-{i}")).w(cell).h(cell).radius(12.0).child(kit::cover(s, Some(tpl.as_str()), cell, 12.0)));
    }
    Some(section("sheet-photos-sec", S::ArtistPhotos.get(s.lang), grid))
}

/// Artists like this one: pictures in a grid as wide as the room, names under them.
fn similar(s: &State, width: f32) -> Option<El> {
    if s.detail.similar.is_empty() {
        return None;
    }
    let cols = (((width + SIM_GAP) / (SIM_MIN_W + SIM_GAP)).floor() as usize).max(2);
    let cell = ((width - SIM_GAP * (cols as f32 - 1.0)) / cols as f32).floor();
    let mut grid = row().key("sheet-similar").w(width).gap(SIM_GAP).flex_wrap();
    for artist in s.detail.similar.iter().take(cols * 2) {
        let id = artist.id.clone();
        grid = grid.child(
            col()
                .key(format!("sheet-similar-{}", artist.id))
                .role("link")
                .label(artist.name.as_str())
                .w(cell)
                .h(SIM_H)
                .pt(2.0)
                .gap(10.0)
                .items_center()
                .radius(12.0)
                .cursor(Cursor::Pointer)
                .hov_group()
                .press_scale(th::PRESS)
                .on_press(move |s: &mut State| go_to(s, Page::Artist(id.clone())))
                .child(el().w(SIM_PIC).h(SIM_PIC).radius(SIM_PIC * 0.5).gop(0.88, 1.0).child(kit::cover(s, artist.cover_template(), SIM_PIC, SIM_PIC * 0.5)))
                .child(label(artist.name.clone(), th::SMALL, th::text2()).gcol(th::text()).wrap().max_lines(2).align(Align::Center).w(cell - 6.0)),
        );
    }
    Some(section("sheet-similar-sec", S::SimilarArtists.get(s.lang), grid))
}

// ---- an album or a playlist -----------------------------------------------------------------------

/// The page's facts, each a name over its value, two to a row.
fn page_facts(s: &State, width: f32) -> Option<El> {
    let lang = s.lang;
    let d = &s.detail;
    let mut facts: Vec<(String, String)> =
        d.facts.iter().filter(|(n, _)| !matches!(n, S::Description | S::SimilarArtists | S::MonthlyListeners)).map(|(n, v)| (n.get(lang).to_owned(), v.clone())).collect();
    if !d.labels.is_empty() {
        let name = if d.labels.len() > 1 { S::Labels } else { S::Label };
        facts.push((name.get(lang).to_owned(), d.labels.join(", ")));
    }
    if let Some(n) = d.likes {
        facts.push((S::LikesCount.get(lang).to_owned(), crate::i18n::group_digits(n)));
    }
    if facts.is_empty() {
        return None;
    }
    let gap = 16.0;
    let half = ((width - gap) * 0.5).floor();
    let grid = row().key("sheet-facts").w(width).gap(gap).flex_wrap().children(facts.iter().enumerate().map(|(i, (n, v))| fact_line(format!("sheet-fact-{i}"), n, v, half)));
    Some(section("sheet-facts-sec", S::Details.get(lang), grid))
}

/// What the service says about it (an artist's story, a playlist's description).
fn about(s: &State, width: f32) -> Option<El> {
    let text = s.detail.facts.iter().find(|(n, _)| *n == S::Description).map(|(_, v)| v.clone())?;
    let name = if s.detail.is_artist { S::AboutArtist } else { S::Description };
    Some(section("sheet-about-sec", name.get(s.lang), label(text, th::LEAD, th::text2()).wrap().align(Align::Start).w(width).key("sheet-about")))
}

// ---- the card ---------------------------------------------------------------------------------------

/// What the card is about, as the left side shows it.
struct Head {
    kind: S,
    cover: Option<String>,
    title: String,
    artists: Vec<ym_api::Artist>,
    sub: String,
    /// Small facts under the name (a year, a genre, a length).
    chips: Vec<String>,
    explicit: bool,
    link: Option<String>,
    round: bool,
    /// Where a tap on the cover leads.
    release: Option<Page>,
}

fn head_of(s: &State, what: &Sheet) -> Head {
    let lang = s.lang;
    match what {
        Sheet::Track(tr) => {
            let mut chips = Vec::new();
            let al = tr.albums.first();
            if let Some(y) = al.and_then(|a| a.year) {
                chips.push(y.to_string());
            }
            if let Some(g) = al.and_then(|a| a.genre.as_deref()).filter(|g| !g.is_empty()) {
                chips.push(crate::genre::name(g, lang));
            }
            if let Some(ms) = tr.duration_ms {
                chips.push(crate::player::fmt_time(ms));
            }
            Head {
                kind: S::Track,
                cover: tr.cover_template().map(str::to_owned),
                title: tr.full_title(),
                artists: tr.artists.clone(),
                sub: String::new(),
                chips,
                explicit: tr.is_explicit(),
                link: Some(track_link(tr)),
                round: false,
                release: al.map(|a| Page::Album(a.id.clone())),
            }
        }
        Sheet::Page => {
            let d = &s.detail;
            let kind = match d.page {
                Some(Page::Album(_)) => S::Album,
                Some(Page::Playlist(..)) => S::Playlist,
                _ => S::Artist,
            };
            let mut chips = Vec::new();
            if let Some(y) = d.year {
                chips.push(y.to_string());
            }
            Head {
                kind,
                cover: d.cover.clone(),
                title: d.title.clone(),
                artists: if d.is_artist { Vec::new() } else { d.artists.clone() },
                sub: if d.is_artist { String::new() } else { d.subtitle.clone() },
                chips,
                explicit: false,
                link: d.link.clone(),
                round: d.is_artist,
                release: None,
            }
        }
    }
}

/// The right side's sections, in the order they come in.
fn sections(s: &State, what: &Sheet, width: f32) -> Vec<El> {
    let mut out = Vec::new();
    match what {
        Sheet::Track(t) => {
            out.push(credits(s, t, width));
            out.extend(release(s, t, width));
        }
        Sheet::Page if s.detail.is_artist => {
            out.extend(listeners(s, width));
            out.extend(about(s, width));
            out.extend(elsewhere(s, width));
            out.extend(photos(s, width));
            out.extend(similar(s, width));
        }
        Sheet::Page => {
            out.extend(page_facts(s, width));
            out.extend(about(s, width));
        }
    }
    out
}

/// The small facts under the name, as quiet pills.
fn chips(lang: Lang, head: &Head) -> Option<El> {
    if head.chips.is_empty() && !head.explicit {
        return None;
    }
    let pill = |text: String| row().h(24.0).px(10.0).radius(12.0).items_center().fill(th::veil(0.07)).child(label(text, th::SMALL, th::text2()));
    let mut r = row().key("sheet-chips").gap(6.0).flex_wrap().justify_center().mt(12.0);
    for c in &head.chips {
        r = r.child(pill(c.clone()));
    }
    if head.explicit {
        r = r.child(pill(S::Explicit.get(lang).to_owned()));
    }
    Some(r)
}

/// The card, over the page.
pub fn sheet(s: &State, cx: &Cx<State>, w: f32, h: f32) -> El {
    let Some(what) = &s.sheet else { return el() };
    let lang = s.lang;
    let p = s.sheet_pop.t;
    let a = p.clamp(0.0, 1.0);
    let drag = s.sheet_drag.t;
    let cw = MAX_W.min(w - 64.0).max(360.0).round();
    let max_h = MAX_H.min(h - 2.0 * kit::TITLE_H).max(340.0).round();
    let head = head_of(s, what);

    // The left side: the cover's place, then what it is.
    let left_w = (cw * 0.38).clamp(250.0, 320.0).round();
    let inner = left_w - 2.0 * PAD;
    let cover_side = inner.min(max_h - 250.0).max(120.0).round();
    let names_top = PAD + 6.0 + cover_side + 20.0;
    // The right side, below the round buttons.
    let right_x = left_w + 4.0;
    let right_w = cw - right_x - PAD;
    let right_top = PAD + ROUND + 6.0;
    // As tall as what it holds (both sides as laid out last frame), up to the room there is; a
    // longer right side scrolls.
    let left_h = cx.layout_size("sheet-names").map_or(max_h, |[_, nh]| names_top + nh + PAD);
    let right_h = cx.layout_size("sheet-sections").map_or(0.0, |[_, bh]| right_top + bh + PAD);
    let ch = left_h.max(right_h).clamp(360.0, max_h).round();
    let dst = [((w - cw) * 0.5).round(), ((h - ch) * 0.5).round(), cw, ch];
    // How far it has been pulled down, 0..1: the card shrinks a little and the page clears.
    let pull = (drag.max(0.0) / (ch * 0.7)).min(1.0);

    // ---- the cover's colours tint the page behind and glow under the cover.
    let cover_key = State::cover_key(head.cover.as_deref(), 600);
    let pal = cover_key.as_deref().and_then(|k| s.art.palette(k));
    let deep = pal.map_or(th::rgb(0x000000), |p| th::srgb(p.colors[1]));
    let glow = pal.map_or(th::veil(0.0), |p| th::srgb(p.accent));

    let dim_a = a * (1.0 - 0.75 * pull);
    let mut layer = el().key("sheet").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).instant_changes();
    layer = layer
        .child(el().key("sheet-snap").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).fx(fx::SNAP).fx_backdrop(true))
        .child(el().key("sheet-blur").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).fx(fx::VEIL).fx_data([-dim_a, 0.0, 22.0, 0.0]))
        .child(
            el().key("sheet-dim")
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .transition(Transition::none())
                .fill(th::rgba(0x000000, (if th::light() { 0.18 } else { 0.42 }) * dim_a))
                .radial_gradient([0.5, 0.5], w.max(h) * 0.75, [(0.0, deep.alpha(0.5 * dim_a)), (1.0, deep.alpha(0.0))]),
        );
    if s.sheet_open {
        layer = layer.child(el().key("sheet-scrim").absolute(0.0, 0.0).w_full().h_full().on_press(|s: &mut State| s.sheet_open = false));
    }

    // ---- the morph: from the cover pressed (or a small card in the middle) to the card.
    let from = source(s, cx, what);
    let has_from = from.is_some();
    let (src, src_r) = from.unwrap_or(([dst[0] + cw * 0.5 - 70.0, dst[1] + ch * 0.5 - 70.0, 140.0, 140.0], 24.0));
    let lerp = |x: f32, y: f32| x + (y - x) * p;
    let rect = [lerp(src[0], dst[0]), lerp(src[1], dst[1]), lerp(src[2], dst[2]).max(1.0), lerp(src[3], dst[3]).max(1.0)];
    let radius = (src_r + (RADIUS - src_r) * a).min(rect[2].min(rect[3]) * 0.5);

    let cover_r = if head.round { cover_side * 0.5 } else { COVER_R };
    let slot = [dst[0] + (left_w - cover_side) * 0.5, dst[1] + PAD + 6.0, cover_side, cover_side];
    let cov = [lerp(src[0], slot[0]), lerp(src[1], slot[1]), lerp(src[2], slot[2]).max(1.0), lerp(src[3], slot[3]).max(1.0)];
    let cov_r = src_r + (cover_r - src_r) * a;

    // Everything but the cover is laid out where the card ends up and comes in once the card is
    // most of the way there: the left side first, then the sections one after another. Closing
    // runs it backwards (the last section goes first).
    let off = [dst[0] - rect[0], dst[1] - rect[1]];
    let left_in = arrival(p, 0.42);

    // ---- the left side: what it is, its name, who made it, the small facts.
    let mut names = col()
        .key("sheet-names")
        .absolute(PAD, names_top + (1.0 - left_in) * 12.0)
        .w(inner)
        .gap(4.0)
        .items_center()
        .opacity(left_in)
        .transition(Transition::none())
        .child(kit::caps(head.kind.get(lang), th::text3()).mb(2.0))
        .child(label(head.title.clone(), th::SHEET_TITLE, th::text()).wrap().max_lines(3).align(Align::Center).w(inner));
    if !head.artists.is_empty() {
        names = names.child(row().max_w(inner).justify_center().child(kit::artists_row_in_width(s, &head.artists, th::BYLINE_M, false, "sheet-art", th::text2(), inner, true)));
    } else if !head.sub.is_empty() {
        names = names.child(label(head.sub.clone(), th::BYLINE_M, th::text2()).truncate().max_w(inner));
    }
    names = names.children(chips(lang, &head));

    // ---- the right side: the sections, each coming in on its own.
    let list = sections(s, what, right_w);
    let n = list.len();
    let mut stack = col().key("sheet-sections").w(right_w).gap(SECTION_GAP).pb(4.0);
    for (i, sec) in list.into_iter().enumerate() {
        let e = arrival(p, 0.5 + 0.06 * i.min(5) as f32);
        stack = stack.child(el().key(format!("sheet-sec-{i}-of-{n}")).w(right_w).opacity(e).offset(0.0, (1.0 - e) * 18.0).transition(Transition::none()).child(sec));
    }
    let scroll = col()
        .key(format!("sheet-scroll-{}", s.sheet_seq))
        .absolute(right_x, right_top)
        .w(right_w + PAD)
        .h(ch - right_top)
        .scroll(rune::ScrollSpec { stick_bottom: false })
        .scroll_edge_fade(22.0)
        .child(col().w(right_w).pt(2.0).pb(PAD).child(stack));

    let buttons_in = arrival(p, 0.55);
    let close = round_btn("sheet-close", &icons::CLOSE, S::Close.get(lang)).absolute(cw - 18.0 - ROUND, 18.0).opacity(buttons_in).on_press(|s: &mut State| s.sheet_open = false);
    let share = head.link.clone().map(|link| {
        round_btn("sheet-link", &icons::EXTERNAL, S::CopyLink.get(lang)).absolute(cw - 18.0 - 2.0 * ROUND - 8.0, 18.0).opacity(buttons_in).on_press_tx(
            move |s: &mut State, tx: &mut Tx| {
                tx.copy(link.clone());
                let text = s.tr(S::LinkCopied);
                s.toast(text);
            },
        )
    });
    let grab = pull_handle(
        el().key("sheet-grab").absolute(cw * 0.5 - 60.0, 0.0).w(120.0).h(20.0).center().opacity(buttons_in).child(el().w(36.0).h(4.0).radius(2.0).fill(th::veil(0.2))),
        ch,
        None,
    );

    let content = el().key("sheet-content").absolute(off[0], off[1]).w(cw).h(ch).transition(Transition::none()).child(grab).child(names).child(scroll).children(share).child(close);

    // ---- the card's face, laid over the card as it is now (not where it ends up), so no corner
    // of the growing card is ever left bare: the menu's colour, the cover melted into its colours
    // under a wash that keeps the words readable, and the cover's light pooling around the cover.
    let face_a = if has_from { smooth(p / 0.3) } else { 1.0 };
    let melt = cover_key.as_deref().and_then(|k| s.art.soft(k)).map(|px| {
        let (strength, wash) = if th::light() { (0.5, 0.64) } else { (0.6, 0.6) };
        el().absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .child(el().absolute(0.0, 0.0).w_full().h_full().opacity(strength).pixels_fit(&px, rune::SurfaceFit::Fill))
            .child(el().absolute(0.0, 0.0).w_full().h_full().linear_gradient(std::f32::consts::PI, [(0.0, th::pop().alpha(wash * 0.72)), (1.0, th::pop().alpha(wash))]))
    });
    let glow_at = [((cov[0] + cov[2] * 0.5 - rect[0]) / rect[2]).clamp(0.0, 1.0), ((cov[1] + cov[3] * 0.5 - rect[1]) / rect[3]).clamp(0.0, 1.0)];
    let face = el().key("sheet-face").absolute(0.0, 0.0).w_full().h_full().fill(th::pop()).opacity(face_a).transition(Transition::none()).children(melt).child(
        el().absolute(0.0, 0.0).w_full().h_full().radial_gradient(
            glow_at,
            rect[2].max(rect[3]) * 0.6,
            [(0.0, glow.alpha(if th::light() { 0.12 } else { 0.2 })), (1.0, glow.alpha(0.0))],
        ),
    );

    // The cover, the one thing that stays through the move: drawn last, over the card.
    let rise = arrival(p, 0.3);
    let pic = el()
        .key("sheet-cover")
        .absolute(cov[0] - rect[0], cov[1] - rect[1])
        .w(cov[2])
        .h(cov[3])
        .radius(cov_r)
        .transition(Transition::none())
        .shadow(12.0, 34.0, if pal.is_some() { deep.alpha(0.55 * rise) } else { th::rgba(0x000000, 0.35 * rise) })
        .child(kit::cover_px_at(s, head.cover.as_deref(), cov[2].min(cov[3]), cov_r, 600));
    // A tap on the cover goes to the release it is from (on a page, back to the page itself).
    let tap: Tap = match head.release.clone() {
        Some(page) => Rc::new(move |s: &mut State, _tx: &mut Tx| go_to(s, page.clone())),
        None => Rc::new(|s: &mut State, _tx: &mut Tx| s.sheet_open = false),
    };
    let pic = pull_handle(pic.help(S::GoToAlbum.get(lang)), ch, Some(tap));

    let card_shadow = (if th::light() { 0.18 } else { 0.5 }) * face_a * (1.0 - pull);
    let card = el()
        .key("sheet-panel")
        .role("dialog")
        .label(head.kind.get(lang))
        .absolute(rect[0], rect[1] + drag)
        .w(rect[2])
        .h(rect[3])
        .radius(radius)
        .clip_rounded(radius)
        .scale(1.0 - 0.1 * pull)
        .opacity(if has_from { 1.0 } else { a })
        .transition(Transition::none())
        // Open, it follows a change of what it holds (the credits coming in) on a calm spring;
        // while it grows out of the cover the move itself places it.
        .motion(if p >= 0.999 && drag == 0.0 { th::SLIDE } else { rune::Spring::NONE })
        .on_press(|_s: &mut State| {})
        .border(1.0, th::veil(0.08 * face_a))
        .shadow(24.0, 64.0, th::rgba(0x000000, card_shadow))
        .child(face)
        .child(content)
        .child(pic);
    let card = if s.sheet_open { card.modal(true, |s: &mut State| s.sheet_open = false) } else { card };
    layer.child(card)
}
