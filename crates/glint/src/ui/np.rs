//! The Now Playing page: one quiet player (cover, names, the seek line, the transport, the volume)
//! over a slow wash of the cover's colours. With the lyrics or the queue closed it sits in the
//! middle of the window; opening one slides it to the left to make room. The line being sung rests
//! a little above the middle of the window, about a third of the way down, and when the lyrics are
//! timed per word (lrc.red, or the demo) it fills in as the words go by.

use std::borrow::Cow;

use rune::{Cursor, El, GrowthSpec, ScrollSpec, Shape, Span, Transition, Tx, canvas, col, el, icon, row};
use ym_api::{LyricLine, LyricWord, LyricsSource, join_words};

use super::dock;
use super::icons;
use super::kit::{self, BTN, GLYPH, ListSrc, PAGE_PAD, ROW_H, TITLE_H, label};
use crate::app::{Load, NpTab, Popover, State};
use super::shapes::Ill;
use crate::fx;
use crate::i18n::S;
use crate::store::PlayerLayout;
use crate::theme::{self as th, Pal};

/// Where the sung line rests, as a fraction of the window's height.
pub const REST: f32 = 0.35;

/// How bright an unsung word is next to a sung one (the ink at this strength).
const UNSUNG: f32 = 0.46;

/// Colours for the characters of `words` (joined with their spaces) at `pos_ms`: each unit fills
/// from the unsung tint to full ink while it is sung, character by character, so a long held
/// word wipes across instead of switching on. Neighbouring characters in the same state share a
/// span.
pub fn sung_spans(words: &[LyricWord], pos_ms: u32) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    let mut at = 0usize;
    let mut last: Option<(f32, usize)> = None; // the lit value of the open span and its index
    for (i, w) in words.iter().enumerate() {
        let p = w.progress(pos_ms);
        let n = w.text.chars().count().max(1) as f32;
        for (k, (off, ch)) in w.text.char_indices().enumerate() {
            let lit = (p * n - k as f32).clamp(0.0, 1.0);
            let (from, to) = (at + off, at + off + ch.len_utf8());
            match last {
                Some((l, idx)) if (lit == 0.0 || lit == 1.0) && l == lit && spans[idx].end as usize == from => spans[idx].end = to as u32,
                _ => {
                    // ease the fill so the leading edge of the wipe reads as light, not as a ramp
                    let e = lit * lit * (3.0 - 2.0 * lit);
                    spans.push(Span::new(from, to).color(th::veil(UNSUNG + (1.0 - UNSUNG) * e)));
                    last = Some((lit, spans.len() - 1));
                }
            }
        }
        at += w.text.len();
        if w.space_after && i + 1 < words.len() {
            at += 1;
        }
    }
    spans
}

/// Whether the line being sung has word timing and the clock is still inside it (plus a moment),
/// so the view should be redrawn every frame.
pub fn wiping(s: &State) -> bool {
    let (Load::Ready(l), Some(i)) = (&s.lyrics.lyrics, s.lyrics.active) else { return false };
    let Some(line) = l.lines.get(i) else { return false };
    let Some(end) = line.words.last().map(|w| w.end_ms).max(line.backing.last().map(|w| w.end_ms)) else { return false };
    s.player.playing && s.player.pos_ms <= u64::from(end) + 400
}

/// "Lyrics by lrc.red", a link to the track's page there: its terms ask for the credit.
fn credit(s: &State) -> Option<El> {
    let Load::Ready(l) = &s.lyrics.lyrics else { return None };
    if l.source != LyricsSource::LrcRed {
        return None;
    }
    let url = l.link.clone().unwrap_or_else(|| ym_api::lrcred::HOME.to_owned());
    Some(
        row()
            .key("lrc-credit")
            .items_center()
            .gap(8.0)
            .min_h(40.0)
            .cursor(Cursor::Pointer)
            .press_scale(th::PRESS)
            .on_press_tx(move |_s: &mut State, tx: &mut Tx| {
                let _ = tx.open_url(&url);
            })
            .child(label(S::LyricsBy.get(s.lang), th::LABEL, th::text3()))
            .child(icon(&icons::EXTERNAL, 14.0).color(th::text3())),
    )
}

/// How far forward line `i` should be: the sung one all the way, the rest back (every line of
/// lyrics without timing is forward).
fn line_target(s: &State, i: usize) -> f32 {
    match &s.lyrics.lyrics {
        Load::Ready(l) if !l.synced => 1.0,
        _ => {
            if s.lyrics.active == Some(i) {
                1.0
            } else {
                0.0
            }
        }
    }
}

/// Steps each lyric line toward where it should be: at once, smoothly, or on a soft spring (as
/// chosen in the player's look). Returns whether any line is still moving.
pub fn step_lines(s: &mut State, dt: f32) -> bool {
    let n = match &s.lyrics.lyrics {
        Load::Ready(l) => l.lines.len(),
        _ => 0,
    };
    if s.ly_e.len() != n {
        s.ly_e = (0..n).map(|i| line_target(s, i)).collect();
        s.ly_v = vec![0.0; n];
        return false;
    }
    let anim = if s.settings.reduce_motion { 0 } else { s.settings.lyric_look.anim };
    let mut moving = false;
    for i in 0..n {
        let to = line_target(s, i);
        let (e, v) = (s.ly_e[i], s.ly_v[i]);
        if (e - to).abs() < 0.002 && v.abs() < 0.01 {
            if e != to {
                (s.ly_e[i], s.ly_v[i]) = (to, 0.0);
            }
            continue;
        }
        match anim {
            0 => (s.ly_e[i], s.ly_v[i]) = (to, 0.0),
            1 => {
                s.ly_e[i] = e + (to - e) * (1.0 - (-dt / 0.13).exp());
                s.ly_v[i] = 0.0;
            }
            _ => {
                // A soft spring with a little overshoot: the line leans in, then settles.
                let (k, c) = (210.0, 17.0);
                let v = v + (k * (to - e) - c * v) * dt;
                (s.ly_e[i], s.ly_v[i]) = (e + v * dt, v);
            }
        }
        moving = true;
    }
    moving
}

/// One lyric line: the main vocal, with the background vocals under it in the quieter italic.
/// `e` is how far forward it is: the sung line is full size and full ink, the others a little
/// smaller and quieter (`back` is how quiet).
#[allow(clippy::too_many_arguments)]
fn lyric_line(i: usize, line: &LyricLine, style: rune::TextStyle, e: f32, back: f32, active: bool, pos_ms: u32, synced: bool, align: u8) -> El {
    let (side, ox) = match align {
        1 => (rune::Align::Center, 0.5),
        2 => (rune::Align::End, 1.0),
        _ => (rune::Align::Start, 0.0),
    };
    let alpha = back + (1.0 - back) * e.clamp(0.0, 1.0);
    let timed = !line.words.is_empty();
    let main = if timed { join_words(&line.words) } else { line.text.clone() };
    // A line is dimmed through its text colour, not a group opacity: that would make every line
    // an offscreen layer of its own, a pass each, on every frame the page moves.
    let mut text = label(main, style, th::veil(alpha)).key(format!("ly-{i}")).w_full().wrap().align(side);
    if timed && active {
        text = text.spans(sung_spans(&line.words, pos_ms));
    }
    let mut block = col().w_full().gap(6.0).child(text);
    // Smaller about the side the words stand on, so they stay where they start.
    block = block.origin(ox, 0.5).scale(0.92 + 0.08 * e).scale_motion(rune::Anim::INSTANT);
    if !line.backing.is_empty() {
        let mut back = label(line.backing_text(), th::ASIDE, th::veil(0.64 * alpha)).w_full().wrap().align(side);
        if active {
            back = back.spans(sung_spans(&line.backing, pos_ms));
        }
        block = block.child(back);
    }
    if synced && let Some(ms) = line.time_ms {
        block = block.cursor(Cursor::Pointer).on_press(move |s: &mut State| s.seek_ms(u64::from(ms)));
    }
    block
}

// ---- the count-in ------------------------------------------------------------------------------------

/// How long before the first line the count starts, and how long each figure takes to become the
/// next.
const COUNT_MS: i64 = 3000;
const MORPH_MS: f32 = 420.0;
/// How many points each figure is drawn through (all the same, so any one can become any other).
const COUNT_PTS: usize = 72;
/// The count's box, in points.
const COUNT_W: f32 = 52.0;
const COUNT_H: f32 = 60.0;

/// When the first sung line of timed lyrics starts, if that is late enough for a count-in (more
/// than three seconds in; earlier ones start without one).
pub fn count_start(l: &ym_api::Lyrics) -> Option<u32> {
    if !l.synced {
        return None;
    }
    let first = l.lines.iter().find(|x| !x.text.trim().is_empty() || !x.words.is_empty())?;
    let ms = first.words.first().map(|w| w.start_ms).or(first.time_ms)?;
    (i64::from(ms) > COUNT_MS).then_some(ms)
}

/// Whether the count-in is on its way (so the view is drawn every frame while it morphs).
pub fn counting(s: &State) -> bool {
    let Load::Ready(l) = &s.lyrics.lyrics else { return false };
    let Some(first) = count_start(l) else { return false };
    let left = i64::from(first) - s.player.pos_ms.min(u64::from(u32::MAX)) as i64;
    s.player.playing && left <= COUNT_MS + 50 && left > -(GRAIN_S * 1000.0) as i64 - 100
}

/// The figures, each one unbroken stroke in a unit box: a note, 3, 2, 1.
fn figure(k: usize) -> Vec<[f32; 2]> {
    match k {
        0 => {
            // The flag, down the stem, round the head.
            let mut p = vec![[0.86, 0.42], [0.80, 0.27], [0.68, 0.16], [0.58, 0.05], [0.58, 0.30], [0.58, 0.55], [0.58, 0.79]];
            for i in 1..=24 {
                let a = std::f32::consts::TAU * i as f32 / 24.0;
                p.push([0.40 + 0.18 * a.cos(), 0.80 + 0.14 * a.sin()]);
            }
            p
        }
        1 => vec![
            [0.18, 0.18], [0.30, 0.07], [0.50, 0.03], [0.70, 0.08], [0.80, 0.22], [0.76, 0.36], [0.62, 0.45], [0.46, 0.48],
            [0.62, 0.51], [0.80, 0.62], [0.84, 0.78], [0.74, 0.92], [0.52, 0.98], [0.30, 0.95], [0.16, 0.84],
        ],
        2 => vec![[0.18, 0.24], [0.28, 0.09], [0.48, 0.02], [0.68, 0.06], [0.80, 0.20], [0.78, 0.36], [0.62, 0.54], [0.40, 0.75], [0.18, 0.97], [0.52, 0.97], [0.86, 0.97]],
        _ => vec![[0.26, 0.24], [0.42, 0.13], [0.58, 0.02], [0.58, 0.35], [0.58, 0.67], [0.58, 0.98]],
    }
}

/// `pts` smoothed (Catmull-Rom) and spaced evenly along its length into `n` points.
fn resample(pts: &[[f32; 2]], n: usize) -> Vec<[f32; 2]> {
    let mut dense = Vec::new();
    for i in 0..pts.len().saturating_sub(1) {
        let p0 = pts[i.saturating_sub(1)];
        let (p1, p2) = (pts[i], pts[i + 1]);
        let p3 = pts[(i + 2).min(pts.len() - 1)];
        for j in 0..8 {
            let t = j as f32 / 8.0;
            let (t2, t3) = (t * t, t * t * t);
            let c = |a: f32, b: f32, c: f32, d: f32| 0.5 * (2.0 * b + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3);
            dense.push([c(p0[0], p1[0], p2[0], p3[0]), c(p0[1], p1[1], p2[1], p3[1])]);
        }
    }
    dense.push(*pts.last().unwrap_or(&[0.0, 0.0]));
    let mut acc = vec![0.0f32];
    for w in dense.windows(2) {
        let d = ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt();
        acc.push(acc.last().copied().unwrap_or(0.0) + d);
    }
    let total = acc.last().copied().unwrap_or(0.0).max(1e-6);
    let mut out = Vec::with_capacity(n);
    let mut k = 0;
    for i in 0..n {
        let want = total * i as f32 / (n - 1) as f32;
        while k + 1 < acc.len() - 1 && acc[k + 1] < want {
            k += 1;
        }
        let span = (acc[k + 1] - acc[k]).max(1e-6);
        let f = ((want - acc[k]) / span).clamp(0.0, 1.0);
        out.push([dense[k][0] + (dense[k + 1][0] - dense[k][0]) * f, dense[k][1] + (dense[k + 1][1] - dense[k][1]) * f]);
    }
    out
}

/// The note above timed lyrics at `pos` ms, the first line starting at `first` ms: the note until
/// three seconds are left, then 3, 2 and 1, each figure flowing into the next (the 1 stands on a
/// foot). When the first line starts, the 1 falls apart into grains that drop onto the words
/// under it, bounce a little, lie there a moment and fade. It keeps its room after that, so the
/// lines do not move. `align`: where the lines stand (0 left, 1 middle, 2 right).
fn count_note(first: u32, pos: u32, w: f32, align: u8) -> El {
    let since = COUNT_MS - (i64::from(first) - i64::from(pos)); // ms into the count
    let x0 = match align {
        1 => ((w - COUNT_W) * 0.5).floor(),
        2 => w - COUNT_W,
        _ => -2.0,
    };
    let mut wrap = el().key("ly-note").w_full().h(COUNT_H).transition(Transition::none());
    let ink = th::text();
    if since >= COUNT_MS {
        // The grains: every point of the 1 and its foot, each dropping on its own from where it
        // was, the top ones a moment later than the foot (it crumbles from below).
        let gone = (since - COUNT_MS) as f32 / 1000.0;
        if gone >= GRAIN_S {
            return wrap;
        }
        let pts = one_points();
        let floor = COUNT_H + 20.0;
        let room = floor + 24.0;
        let mut shape = Shape::new();
        for (i, p) in pts.iter().enumerate() {
            let h = |k: u32| ((i as u32).wrapping_mul(2_654_435_761).wrapping_add(k.wrapping_mul(40_503)) % 1000) as f32 / 1000.0;
            let start = (1.0 - p[1]) * 0.10 + h(1) * 0.06;
            let age = gone - start;
            let (mut x, mut y) = (pad_x(p[0]), pad_y(p[1]));
            let size = 2.2 + 1.4 * h(2);
            if age > 0.0 {
                // A little burst outward, then gravity; each bounce keeps a third of the speed.
                let (mut vx, mut vy) = ((h(3) - 0.5) * 90.0 + (p[0] - 0.55) * 60.0, -(25.0 + 70.0 * h(4)));
                let land = floor + 4.0 * h(5) - size;
                let mut t = 0.0;
                let dt = 1.0 / 240.0;
                while t < age {
                    vy += 1500.0 * dt;
                    x += vx * dt;
                    y += vy * dt;
                    if y > land {
                        y = land;
                        vy = -vy * 0.32;
                        vx *= 0.6;
                        if vy.abs() < 18.0 {
                            vy = 0.0;
                        }
                    }
                    t += dt;
                }
            }
            let fade = 1.0 - ((gone - 0.85) / (GRAIN_S - 0.85)).clamp(0.0, 1.0);
            let a = 0.92 * fade;
            if a <= 0.01 {
                continue;
            }
            let r = size * 0.5;
            let dot: Vec<[f32; 2]> = (0..6).map(|k| {
                let ang = std::f32::consts::TAU * k as f32 / 6.0;
                [40.0 + x + r * ang.cos(), y + r * ang.sin()]
            }).collect();
            shape = shape.fill(dot, ink.alpha(a));
        }
        let span = COUNT_W + 80.0;
        let layer = canvas(shape.view_box(span, room)).key("ly-note-grains").absolute(x0 - 40.0, 0.0).w(span).h(room).transition(Transition::none()).motion(rune::Spring::NONE);
        return wrap.child(layer);
    }
    let (from, to, t) = if since < 0 {
        (0, 0, 0.0)
    } else {
        let k = ((since / 1000) as usize).min(3);
        let local = (since - k as i64 * 1000) as f32;
        let t = (local / MORPH_MS).clamp(0.0, 1.0);
        let t = t * t * (3.0 - 2.0 * t);
        if k >= 3 { (3, 3, 0.0) } else { (k, k + 1, t) }
    };
    let a = resample(&figure(from), COUNT_PTS);
    let b = resample(&figure(to), COUNT_PTS);
    let pts: Vec<[f32; 2]> = a
        .iter()
        .zip(&b)
        .map(|(p, q)| [pad_x(p[0] + (q[0] - p[0]) * t), pad_y(p[1] + (q[1] - p[1]) * t)])
        .collect();
    let mut shape = Shape::new().stroke_with(pts, false, STROKE, rune::LineCap::Round, ink.alpha(0.92));
    // The 1's foot comes in as the 2 becomes it.
    let foot = if to == 3 { t } else if from == 3 { 1.0 } else { 0.0 };
    if foot > 0.0 {
        let (l, r) = (0.58 - 0.26 * foot, 0.58 + 0.26 * foot);
        shape = shape.stroke_with(vec![[pad_x(l), pad_y(0.98)], [pad_x(r), pad_y(0.98)]], false, STROKE, rune::LineCap::Round, ink.alpha(0.92 * foot));
    }
    wrap = wrap.child(canvas(shape.view_box(COUNT_W, COUNT_H)).key("ly-note-fig").absolute(x0, 0.0).w(COUNT_W).h(COUNT_H).transition(Transition::none()).motion(rune::Spring::NONE));
    wrap
}

/// How long the grains of the 1 take to fall, lie and fade, in seconds.
const GRAIN_S: f32 = 1.7;
const STROKE: f32 = 4.2;

fn pad_x(x: f32) -> f32 {
    let pad = 5.0;
    pad + x * (COUNT_W - 2.0 * pad)
}

fn pad_y(y: f32) -> f32 {
    let pad = 5.0;
    pad + y * (COUNT_H - 2.0 * pad)
}

/// The points the 1 crumbles into, in its unit box: along its stroke and its foot.
fn one_points() -> Vec<[f32; 2]> {
    let mut p = resample(&figure(3), 30);
    for i in 0..=10 {
        p.push([0.32 + 0.52 * i as f32 / 10.0, 0.98]);
    }
    p
}

/// A song's part ("Verse 2", "Chorus") in the app's language; one we do not know stays as it is.
pub fn part_name(part: &str, lang: crate::i18n::Lang) -> String {
    if lang != crate::i18n::Lang::Ru {
        return part.to_owned();
    }
    let p = part.trim().trim_start_matches('[').trim_end_matches(']').trim();
    // "Verse 2: Name" keeps its number and what follows.
    let lower = p.to_lowercase();
    const PARTS: [(&str, &str); 17] = [
        ("pre-chorus", "Предприпев"),
        ("prechorus", "Предприпев"),
        ("post-chorus", "Постприпев"),
        ("postchorus", "Постприпев"),
        ("chorus", "Припев"),
        ("verse", "Куплет"),
        ("bridge", "Бридж"),
        ("intro", "Вступление"),
        ("outro", "Концовка"),
        ("hook", "Хук"),
        ("refrain", "Рефрен"),
        ("interlude", "Интерлюдия"),
        ("instrumental", "Инструментал"),
        ("break", "Брейк"),
        ("drop", "Дроп"),
        ("skit", "Скит"),
        ("spoken", "Речитатив"),
    ];
    for (en, ru) in PARTS {
        if let Some(rest) = lower.strip_prefix(en) {
            if rest.chars().next().is_some_and(|c| c.is_alphabetic()) {
                continue;
            }
            return format!("{ru}{}", &p[en.len()..]);
        }
    }
    p.to_owned()
}

/// A number from a track's id that picks the pattern of its bars, the same one every time.
pub fn seed_of(id: &str) -> f32 {
    (id.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(u32::from(b))) % 997) as f32 + 0.5
}

/// What the player column is made of, top to bottom, besides the cover: the names, the seek line,
/// the times, the transport and the volume row, with the gaps between them.
const PLAYER_REST: f32 = 20.0 + 58.0 + 14.0 + 22.0 + 16.0 + 56.0 + 12.0 + 40.0 + 16.0 + 56.0;
/// The same stack with the wave card moved into the player menu.
const PLAYER_COMPACT_REST: f32 = 20.0 + 58.0 + 14.0 + 22.0 + 16.0 + 56.0 + 12.0 + 40.0;

pub fn compact_player(s: &State, h: f32) -> bool {
    match s.settings.player_layout {
        PlayerLayout::Auto => h < 660.0,
        PlayerLayout::Compact => true,
        PlayerLayout::Comfortable => false,
    }
}

/// Where the player's columns stand in a `w` x `h` window: the cover column's left edge and width,
/// and the side panel's left edge and width (`panel`: whether lyrics or the queue are open).
pub fn columns(s: &State, w: f32, h: f32, panel: bool) -> (f32, f32, f32, f32) {
    let compact = compact_player(s, h);
    let rest = if compact { PLAYER_COMPACT_REST } else { PLAYER_REST };
    let side = if compact {
        (h - TITLE_H - 56.0 - rest).clamp(160.0, 200.0)
    } else {
        (h - TITLE_H - 56.0 - rest).clamp(220.0, 372.0)
    }
    .min(w - 2.0 * PAGE_PAD);
    let gap = 72.0;
    let available = (w - 2.0 * PAGE_PAD - side - gap).max(0.0);
    // On a narrow window keep the lyrics/queue readable instead of letting them consume the
    // whole right half. The smaller panel leaves more room for the player while it stays near
    // the middle of the window.
    let compact = w < 1100.0;
    let panel_w = if compact {
        (available * 0.52).clamp(180.0, 420.0).min(available)
    } else {
        available.clamp(320.0, 620.0)
    };
    if panel {
        let x0 = ((w - (side + gap + panel_w)) * 0.5).max(PAGE_PAD);
        (x0, side, x0 + side + gap, panel_w)
    } else {
        ((w - side) * 0.5, side, w, panel_w)
    }
}

/// The player page. `come` is how far it has come over the window (0..1: its backdrop fades over
/// the page underneath by it); `front` is whether its column and panel are there (they leave as
/// soon as it starts to close, the cover flying back to the bar).
pub fn page(s: &State, w: f32, h: f32, come: f32, front: bool) -> El {
    let Some(t) = s.current() else { return el() };
    let p = s.pal();
    let lang = s.lang;
    // A track without lyrics keeps the player in the middle, as if they had been closed.
    let tab = s.np_panel();
    let panel = tab != NpTab::Closed;

    // The cover is as large as the window's height allows, and the column is centred in what is
    // left under the title bar.
    // (The bottom 56 points are under the progressive blur, so the column stays out of them.)
    let compact = compact_player(s, h);
    let (x, side, panel_x, panel_w) = columns(s, w, h, panel);
    let rest = if compact { PLAYER_COMPACT_REST } else { PLAYER_REST };
    let total_h = side + rest;
    let top = TITLE_H + ((h - TITLE_H - 56.0 - total_h) * 0.5).max(0.0);

    // Each artist with their picture; the name lights up under the pointer and opens their page.
    let artist = kit::artist_line_width(s, t, th::BODY, true, "np-art", side);
    // The title with the buttons beside it; under it the artists, across the whole column (their
    // pictures and a "2 more" need the room).
    let button_count = if compact || side < 300.0 { 2.0 } else { 3.0 };
    let mut head = row()
        .w(side + (BTN - GLYPH) * 0.5)
        .h(32.0)
        .items_center()
        .child(col().grow(1.0).min_w(0.0).child(kit::roll("np-title", &t.full_title(), kit::track_title(s, t, "np-title-link", th::H2, (side - button_count * BTN).max(60.0)))));
    // The compact player has less vertical room, so keep its title row focused on the essential
    // actions and move dislike into the track menu instead.
    if !compact {
        let dt = t.clone();
        head = head.child(kit::ibtn("dislike", &icons::DISLIKE, GLYPH, BTN, th::text2(), S::Dislike.get(lang)).on_press(move |s: &mut State| s.dislike(&dt)));
    }
    head = head.child(kit::like_button(s, t, BTN));
    if compact || side >= 300.0 {
        head =
            head.child(kit::ibtn("np-more", &icons::MORE, GLYPH, BTN, th::text2(), S::More.get(lang)).on_press(|s: &mut State| s.open_player_menu(rune::PopupAnchor::Key("np-more".into()))));
    }
    let info = col().w(side).h(58.0).gap(4.0).child(head).child(kit::roll("np-artist", &t.artists_line(), artist));

    let times = row()
        .w_full()
        .h(22.0)
        .items_center()
        .justify_between()
        .child(label("0:00", th::MONO, th::text3()).live_text(&s.lives.clock))
        .child(label("-0:00", th::MONO, th::text3()).live_text(&s.lives.remaining));
    let transport = row()
        .w_full()
        .h(56.0)
        .gap(4.0)
        .items_center()
        .justify_center()
        .child(dock::shuffle_btn(s))
        .child(kit::ibtn("prev", &icons::PREV, GLYPH, BTN, th::text(), S::Previous.get(lang)).on_press(|s: &mut State| s.prev()))
        .child(kit::play_btn(s, 56.0, 34.0))
        .child(kit::ibtn("next", &icons::NEXT, GLYPH, BTN, th::text(), S::Next.get(lang)).on_press(|s: &mut State| s.next()))
        .child(dock::repeat_btn(s));
    let v = s.player.volume;
    let vol_icon = if v < 0.01 {
        &icons::VOLUME_MUTE
    } else if v < 0.5 {
        &icons::VOLUME_LOW
    } else {
        &icons::VOLUME
    };
    let panel_toggle = |key: &'static str, ic: &'static rune::Icon, name: &str, which: NpTab| {
        let on = tab == which;
        dock::toggle(key, ic, on, name).on_press(move |s: &mut State| s.np_tab = if s.np_panel() == which { NpTab::Closed } else { which })
    };
    // No lyrics for this track: the button is there but off, and says why.
    let lyrics_btn = if s.lyrics_missing() {
        dock::toggle_off("np-lyrics-btn", &icons::LYRICS, S::NoLyricsShort.get(lang))
    } else {
        panel_toggle("np-lyrics-btn", &icons::LYRICS, S::Lyrics.get(lang), NpTab::Lyrics)
    };
    let utility = row()
        .w(side + (BTN - GLYPH))
        .ml(-(BTN - GLYPH) * 0.5)
        .h(40.0)
        .items_center()
        .child(kit::ibtn("mute", vol_icon, GLYPH, BTN, th::text2(), S::Mute.get(lang)).on_press(|s: &mut State| s.toggle_mute()))
        .child(row().grow(1.0).min_w(0.0).mr(12.0).child(kit::slider("vol", &s.lives.volume, 3.0, th::text(), |s: &mut State, f: f32, _| s.set_volume(f))))
        .child(lyrics_btn)
        .child(panel_toggle("np-queue-btn", &icons::QUEUE, S::Queue.get(lang), NpTab::Queue));

    // My Wave by this track; its arrow lists the others (by each artist, by the release).
    let mut choices = vec![crate::app::WaveChoice { station: crate::app::Station::Track(t.id.clone()), kind: S::ByTrack.get(lang).to_owned(), name: t.full_title() }];
    for a in t.artists.iter().take(3) {
        choices.push(crate::app::WaveChoice { station: crate::app::Station::Artist(a.id.clone()), kind: S::ByArtist.get(lang).to_owned(), name: a.name.clone() });
    }
    if let Some(al) = t.albums.first() {
        choices.push(crate::app::WaveChoice { station: crate::app::Station::Album(al.id.clone()), kind: S::ByRelease.get(lang).to_owned(), name: al.title.clone() });
    }
    let waves = kit::wave_card(s, "np-wave", S::WaveByTrack.get(lang), &t.full_title(), crate::app::Station::Track(t.id.clone()), Some(side), Some(choices));

    let mut left = col()
        .key("np-left")
        .absolute(x, top)
        .w(side)
        // No fade on the way in: the bars are drawn by the compositor, which skips what sits in a
        // fading group (they fade up on their own). On the way out it fades quickly.
        .transition(Transition::opacity().springs(th::LEAVE))
        // It slides aside for the lyrics or the queue on a calm spring of its own.
        .motion(th::SLIDE)
        .child(kit::playing_cover(s, t, side, 28.0, "np-cover-morph", seed_of(&t.id) % 13.0).matched("np-cover").motion(th::FLIGHT))
        // The names and the controls rise in after the cover has started to come over; the seek
        // bars are drawn by the compositor, so they fade up on their own (see `BarMem` in fx.rs).
        .child(kit::rise(info.mt(20.0), 1))
        .child(row().w_full().mt(10.0).child(dock::seek_bars("seek", seed_of(&t.id), 36.0)))
        .child(kit::rise(times, 2))
        // The times are small and at the sides, so the transport sits right under their row: the
        // play button is as far from the bars as the volume row is from it.
        .child(kit::rise(transport.mt(-8.0), 2))
        .child(kit::rise(utility.mt(12.0), 3));
    if !compact {
        left = left.child(kit::rise(waves.mt(16.0), 4));
    }

    let body = match tab {
        NpTab::Closed => el(),
        NpTab::Lyrics => lyrics_view(s, &p, panel_x, panel_w, h),
        NpTab::Queue => queue_view(s, &p, panel_x, panel_w, h),
    };
    let come = come.clamp(0.0, 1.0);
    // The picture behind the player: the cover's colours, a burst of light from under it and the
    // rays that leave it (`fx::STAGE`). The slot is the whole window, and the burst starts at the
    // cover's middle: where the column stands, which the shader glides to as
    // the column slides aside for the lyrics.
    let stage = el()
        .key("np-stage")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .transition(Transition::none())
        .effect(fx::STAGE)
        .effect_data([
            come,
            seed_of(&t.id) % 100.0 + 100.0 * side.round(),
            (x + side * 0.5) / w.max(1.0),
            (top + side * 0.5) / h.max(1.0),
        ]);
    let mut root = el()
        .key("np")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .transition(Transition::none())
        // Its own canvas, so it covers what is underneath whatever the effects. It comes and goes
        // by `come`, never by a group fade (which would hide the compositor's slots), and takes
        // each step at once (`come` is already a spring).
        .child(el().key("np-canvas").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).fill(th::bg().alpha(come)).motion(rune::Spring::NONE))
        // The stage is over that canvas and under the column: the burst comes out from under the
        // cover, and the words stand over them.
        .child(stage);
    if !front {
        return root;
    }
    root = root.child(left)
        // The side panel waits for the player to slide over, then eases in from the right.
        .child(
            el().key(format!("np-body-{tab:?}"))
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .transition(Transition::opacity().combined(Transition::offset(32.0, 0.0)).springs(th::SOFT))
                .enter_delay(0.14)
                .child(body),
        )
        // The corner button that opens the look of the picture behind the player.
        .child(look_button(s, w, h));
    root
}

/// How far the look button of the open player stands from the window's corner.
const LOOK_INSET: f32 = 62.0;

/// The button in the corner of the open player that opens its look: the flash and reaction of the
/// picture behind it, and how the lyrics are drawn.
fn look_button(s: &State, w: f32, h: f32) -> El {
    kit::ibtn("np-look", &icons::STAGE, 24.0, 40.0, th::text3(), S::StageLook.get(s.lang))
        .absolute(w - LOOK_INSET, h - LOOK_INSET)
        .on_press(|s: &mut State| s.open_popover(Popover::Stage { anchor: "np-look".into() }))
}

/// A message with its illustration, in the right-hand column.
fn notice(s: &State, p: &Pal, x: f32, w: f32, h: f32, art: Ill, msg: &str) -> El {
    let _ = s;
    col().absolute(x, 0.0).w(w).h(h * 0.78).justify_center().child(kit::empty(p, art, msg))
}

fn lyrics_view(s: &State, p: &Pal, x: f32, w: f32, h: f32) -> El {
    let lang = s.lang;
    match &s.lyrics.lyrics {
        Load::Idle | Load::Loading => {
            let mut c = col().absolute(x, h * REST - 120.0).w(w).gap(26.0);
            for i in 0..5 {
                c = c.child(kit::skeleton((w * 0.5 + ((i * 97) % 180) as f32).min(w), 30.0, 15.0));
            }
            c.child(label(S::LyricsLoading.get(lang), th::SMALL, th::text3()))
        }
        Load::Failed(_) => notice(s, p, x, w, h, Ill::Mic, S::NoLyrics.get(lang)),
        Load::Ready(l) if l.instrumental => notice(s, p, x, w, h, Ill::Tape, S::Instrumental.get(lang)),
        Load::Ready(l) if !l.has_words() => notice(s, p, x, w, h, Ill::Mic, S::NoLyrics.get(lang)),
        Load::Ready(l) => {
            let active = if l.synced { s.lyrics.active } else { None };
            // The scroll viewport is taller than the window and starts above it, so that its middle
            // (where a sung line is centred) sits `REST` of the way down the window.
            let vp_h = h * (2.0 * (1.0 - REST));
            let vp_top = h * REST - vp_h * 0.5;
            let mut lines = col().w_full().gap(20.0).pt(vp_h * 0.5).pb(vp_h * 0.5);
            let pos = s.player.pos_ms.min(u64::from(u32::MAX)) as u32;
            // Timed lyrics that start a while in: a note waits above them and counts the last three
            // seconds down, 3, 2, 1, turning from one figure into the next.
            let look = s.settings.lyric_look;
            if let Some(first) = count_start(l) {
                lines = lines.child(count_note(first, pos, w, look.align));
            }
            let style = match (look.size, w < 620.0) {
                (0, _) => th::LYRIC_S,
                (1, true) => th::LYRIC_S,
                (1, false) => th::LYRIC,
                (_, true) => th::LYRIC,
                (_, false) => th::LYRIC_L,
            };
            let back = [0.46, 0.30, 0.17][look.focus.min(2) as usize];
            for (i, line) in l.lines.iter().enumerate() {
                if line.text.trim().is_empty() && line.words.is_empty() {
                    lines = lines.child(el().key(format!("ly-{i}")).h(20.0));
                    continue;
                }
                // A section opens with its name in small capitals.
                if l.synced
                    && let Some(part) = line.part.as_deref()
                {
                    let name = part_name(part, lang);
                    let caps = kit::caps(name.as_str(), th::text3()).w_full().align(match look.align {
                        1 => rune::Align::Center,
                        2 => rune::Align::End,
                        _ => rune::Align::Start,
                    });
                    lines = lines.child(caps.mt(if i == 0 { 0.0 } else { 18.0 }));
                }
                let is_active = l.synced && active == Some(i);
                let e = if l.synced { s.ly_e.get(i).copied().unwrap_or(if is_active { 1.0 } else { 0.0 }) } else { 0.95 };
                lines = lines.child(lyric_line(i, line, style, e, back, is_active, pos, l.synced, look.align));
            }
            // Where the lyrics came from closes them (lrc.red's terms ask for the credit).
            if let Some(c) = credit(s) {
                lines = lines.child(c.mt(12.0));
            }
            col().key("np-lyrics").absolute(x, vp_top).w(w).h(vp_h).scroll(ScrollSpec { stick_bottom: false }).child(lines)
        }
    }
}

fn queue_view(s: &State, p: &Pal, x: f32, w: f32, h: f32) -> El {
    let lang = s.lang;
    let n = s.player.queue.len();
    if n == 0 {
        return notice(s, p, x, w, h, Ill::Tape, S::QueueEmpty.get(lang));
    }
    // The name and the count line up with the cover and duration cell in each row.
    let head = row()
        .w_full()
        .pl(12.0)
        .pr(12.0 + (BTN - GLYPH) * 0.5)
        .items_center()
        .gap(18.0)
        .child(kit::caps(S::Queue.get(lang), th::text2()))
        .child(el().grow(1.0))
        .child(kit::caps(lang.tracks(n), th::text3()));
    let rows = col().key("np-queue-rows").w_full().growth(GrowthSpec::default()).virtual_rows(
        n,
        ROW_H,
        |s: &State, i: usize| Cow::Owned(format!("q{i}-{}", s.player.queue.get(i).map_or("", |t| t.id.as_str()))),
        |s: &State, i: usize| match s.player.queue.get(i) {
            Some(t) => kit::track_row(s, t, i + 1, ListSrc::Queue),
            None => el().h(ROW_H),
        },
    );
    col().key("np-queue").absolute(x, 0.0).w(w).h(h).scroll(ScrollSpec { stick_bottom: false }).child(col().w_full().pt(TITLE_H + 24.0).pb(72.0).gap(18.0).child(head).child(rows))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{PlayerLayout, Store};

    #[test]
    fn the_count_in_waits_only_for_lyrics_that_start_late() {
        let late = ym_api::Lyrics::from_lrc("[00:12.00]one
[00:15.00]two", LyricsSource::Lrclib);
        let soon = ym_api::Lyrics::from_lrc("[00:02.50]one
[00:05.00]two", LyricsSource::Lrclib);
        let plain = ym_api::Lyrics::from_lrc("one
two", LyricsSource::Lrclib);
        assert_eq!(count_start(&late), Some(12_000));
        assert_eq!(count_start(&soon), None, "starts within three seconds");
        assert_eq!(count_start(&plain), None, "not timed");
    }

    #[test]
    fn compact_player_tracks_height_and_preference() {
        let mut s = State::new(None, Store::none(), true);
        assert!(compact_player(&s, 659.0));
        assert!(!compact_player(&s, 660.0));

        s.settings.player_layout = PlayerLayout::Compact;
        assert!(compact_player(&s, 820.0));
        s.settings.player_layout = PlayerLayout::Comfortable;
        assert!(!compact_player(&s, 640.0));
    }

    #[test]
    fn every_count_figure_is_drawn_through_the_same_number_of_points() {
        for k in 0..4 {
            let p = resample(&figure(k), COUNT_PTS);
            assert_eq!(p.len(), COUNT_PTS);
            assert!(p.iter().all(|q| (-0.1..=1.1).contains(&q[0]) && (-0.1..=1.1).contains(&q[1])));
        }
    }

    fn w(text: &str, start_ms: u32, end_ms: u32, space_after: bool) -> LyricWord {
        LyricWord { start_ms, end_ms, text: text.into(), space_after }
    }

    #[test]
    fn spans_follow_the_words_and_never_split_a_character() {
        let words = [w("Hel", 0, 100, false), w("lo", 100, 200, true), w("мир", 200, 300, false)];
        let text = join_words(&words);
        for pos in [0, 50, 150, 250, 999] {
            let spans = sung_spans(&words, pos);
            for s in &spans {
                assert!(s.start < s.end && s.end as usize <= text.len(), "pos {pos}");
                assert!(text.is_char_boundary(s.start as usize) && text.is_char_boundary(s.end as usize), "pos {pos}: {s:?}");
            }
            assert!(spans.windows(2).all(|p| p[0].end <= p[1].start), "pos {pos}: spans overlap");
        }
    }

    #[test]
    fn a_word_fills_in_character_by_character() {
        let words = [w("Hel", 0, 100, false), w("lo", 100, 200, true), w("мир", 200, 300, false)];
        // Before the line and after it, neighbouring characters share a span: a word with its
        // syllables, then the next word.
        assert_eq!(sung_spans(&words, 0).len(), 2);
        assert_eq!(sung_spans(&words, 999).len(), 2);
        // Halfway through "lo": "Hel" and the "l" are sung, the "o" is not yet.
        let mid = sung_spans(&words, 150);
        assert_eq!((mid[0].start, mid[0].end), (0, 4));
        assert!(mid.len() >= 3);
    }

    #[test]
    fn a_line_is_only_redrawn_per_frame_while_it_is_being_sung() {
        let line = LyricLine {
            time_ms: Some(1_000),
            end_ms: Some(2_000),
            words: vec![w("a", 1_000, 1_500, true), w("b", 1_500, 2_000, false)],
            text: "a b".into(),
            ..LyricLine::default()
        };
        let mut s = State::new(None, crate::store::Store::none(), true);
        s.lyrics.lyrics = Load::Ready(ym_api::Lyrics { lines: vec![line], synced: true, word_synced: true, ..ym_api::Lyrics::default() });
        s.lyrics.active = Some(0);
        s.player.playing = true;
        s.player.pos_ms = 1_200;
        assert!(wiping(&s));
        s.player.pos_ms = 2_500;
        assert!(!wiping(&s), "well past the line's last word");
        s.player.pos_ms = 1_200;
        s.player.playing = false;
        assert!(!wiping(&s), "paused");
    }
}
