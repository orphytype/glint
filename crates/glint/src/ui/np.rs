//! The Now Playing page: one quiet player (cover, names, the seek line, the transport, the volume)
//! over a slow wash of the cover's colours. With the lyrics or the queue closed it sits in the
//! middle of the window; opening one slides it to the left to make room. The line being sung rests
//! a little above the middle of the window, about a third of the way down, and when the lyrics are
//! timed per word (lrc.red) it fills in as the words go by.

use super::kit::Hover as _;
use crate::fx::Slot as _;
use std::borrow::Cow;

use rune::{Cursor, El, GrowthSpec, ScrollSpec, Shape, Span, Transition, Tx, canvas, col, el, icon, row};
use ym_api::{LyricLine, LyricWord, LyricsSource, join_words};

use super::dock;
use super::icons;
use super::kit::{self, BTN, PAGE_PAD, ROW_H, TITLE_H, label};
use super::np_icons;
use super::shapes::Ill;
use crate::app::{Load, NpTab, Popover, State};
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
    s.player.playing && s.lyric_ms() <= end + 400
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

/// The player's glyphs: the track's own buttons by the title, the modes and the panels, and the
/// way back and on; with the hit areas round them.
const SMALL_GLYPH: f32 = 19.0;
const SMALL_BTN: f32 = 32.0;
const MODE_GLYPH: f32 = 19.0;
const MODE_BTN: f32 = 34.0;
const SKIP_GLYPH: f32 = 24.0;
/// The room between the way back, play and on.
const TRANSPORT_GAP: f32 = 18.0;
/// From a mode (shuffle, repeat) to the way back or on beside it.
const MODE_GAP: f32 = 22.0;

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

/// Steps each lyric line toward where it should be: smoothly, or at once with reduced motion.
/// Returns whether any line is still moving.
pub fn step_lines(s: &mut State, dt: f32) -> bool {
    let n = match &s.lyrics.lyrics {
        Load::Ready(l) => l.lines.len(),
        _ => 0,
    };
    if s.ly_e.len() != n {
        s.ly_e = (0..n).map(|i| line_target(s, i)).collect();
        return false;
    }
    let smooth = !s.settings.reduce_motion;
    let mut moving = false;
    for i in 0..n {
        let to = line_target(s, i);
        let e = s.ly_e[i];
        if (e - to).abs() < 0.002 {
            s.ly_e[i] = to;
            continue;
        }
        s.ly_e[i] = if smooth { e + (to - e) * (1.0 - (-dt / 0.13).exp()) } else { to };
        moving = true;
    }
    moving
}

/// One lyric line: the main vocal, with the background vocals under it in the quieter italic.
/// `e` is how far forward it is: the sung line is full size and full ink, the others a little
/// smaller and quieter (`back` is how quiet).
#[allow(clippy::too_many_arguments)]
fn lyric_line(i: usize, line: &LyricLine, style: rune::TextStyle, e: f32, back: f32, active: bool, pos_ms: u32, synced: bool, align: u8, font: u8) -> El {
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
        let mut back = label(line.backing_text(), th::lyric_face(th::ASIDE, font), th::veil(0.64 * alpha)).w_full().wrap().align(side);
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
const MORPH_MS: f32 = 600.0;
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
    let left = i64::from(first) - i64::from(s.lyric_ms());
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
            [0.18, 0.18],
            [0.30, 0.07],
            [0.50, 0.03],
            [0.70, 0.08],
            [0.80, 0.22],
            [0.76, 0.36],
            [0.62, 0.45],
            [0.46, 0.48],
            [0.62, 0.51],
            [0.80, 0.62],
            [0.84, 0.78],
            [0.74, 0.92],
            [0.52, 0.98],
            [0.30, 0.95],
            [0.16, 0.84],
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
            let dot: Vec<[f32; 2]> = (0..6)
                .map(|k| {
                    let ang = std::f32::consts::TAU * k as f32 / 6.0;
                    [40.0 + x + r * ang.cos(), y + r * ang.sin()]
                })
                .collect();
            shape = shape.fill(dot, ink.alpha(a));
        }
        let span = COUNT_W + 80.0;
        let layer = canvas(shape.view_box(span, room)).key("ly-note-grains").absolute(x0 - 40.0, 0.0).w(span).h(room).transition(Transition::none()).motion(rune::Spring::NONE);
        return wrap.child(layer);
    }
    let (from, to, local) = if since < 0 {
        (0, 0, f32::INFINITY)
    } else {
        let k = ((since / 1000) as usize).min(2);
        (k, k + 1, (since - k as i64 * 1000) as f32)
    };
    let tt = local / MORPH_MS;
    // Each new figure lands like a beat: it comes in a little small and springs to its size, its
    // stroke heavy for a moment and thinning out.
    let pop = 0.86 + 0.14 * settle(local / 520.0);
    let weight = STROKE * (1.0 + 0.45 * (-local / 240.0).exp());
    let a = resample(&figure(from), COUNT_PTS);
    let b = resample(&figure(to), COUNT_PTS);
    let last = (COUNT_PTS - 1) as f32;
    let place = |x: f32, y: f32| [pad_x(0.5 + (x - 0.5) * pop) + RING_PAD, pad_y(0.5 + (y - 0.5) * pop) + RING_PAD];
    // The new figure flows along the stroke from its start to its end: each point sets off a
    // little after the one before it, swings out on a curve rather than a straight line, and
    // settles with a touch of overshoot.
    let pts: Vec<[f32; 2]> = a
        .iter()
        .zip(&b)
        .enumerate()
        .map(|(i, (p, q))| {
            let u = ((tt - 0.32 * i as f32 / last) / 0.68).clamp(0.0, 1.0);
            let e = settle(u);
            let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
            let swing = 0.24 * (std::f32::consts::PI * u).sin();
            place(p[0] + dx * e - dy * swing, p[1] + dy * e + dx * swing)
        })
        .collect();
    let mut shape = Shape::new().stroke_with(pts, false, weight, rune::LineCap::Round, ink.alpha(0.92));
    // The 1's foot comes in as the 2 becomes it.
    let foot = if to == 3 { settle(tt).max(0.0) } else { 0.0 };
    if foot > 0.0 {
        let (l, r) = (0.58 - 0.26 * foot, 0.58 + 0.26 * foot);
        shape = shape.stroke_with(vec![place(l, 0.98), place(r, 0.98)], false, weight, rune::LineCap::Round, ink.alpha(0.92 * foot.min(1.0)));
    }
    // On each beat a ring goes out from the figure and fades, like a struck note.
    let ring = local / 650.0;
    if ring < 1.0 {
        let grow = 1.0 - (1.0 - ring).powi(3);
        let rad = 24.0 + 16.0 * grow;
        let (cx, cy) = (RING_PAD + COUNT_W * 0.5, RING_PAD + COUNT_H * 0.5);
        let circle: Vec<[f32; 2]> = (0..48)
            .map(|k| {
                let ang = std::f32::consts::TAU * k as f32 / 48.0;
                [cx + rad * ang.cos(), cy + rad * 1.08 * ang.sin()]
            })
            .collect();
        shape = shape.stroke_with(circle, true, 1.6, rune::LineCap::Round, ink.alpha(0.30 * (1.0 - ring).powi(2)));
    }
    let (bw, bh) = (COUNT_W + 2.0 * RING_PAD, COUNT_H + 2.0 * RING_PAD);
    wrap = wrap.child(canvas(shape.view_box(bw, bh)).key("ly-note-fig").absolute(x0 - RING_PAD, -RING_PAD).w(bw).h(bh).transition(Transition::none()).motion(rune::Spring::NONE));
    wrap
}

/// Room around the count for the ring each beat sends out, in points.
const RING_PAD: f32 = 18.0;

/// A damped spring from 0 to 1 over `u` (0..1 of its run): quick out, a touch past, settled.
fn settle(u: f32) -> f32 {
    if u <= 0.0 {
        0.0
    } else if u >= 1.0 {
        1.0
    } else {
        1.0 - (-6.0 * u).exp() * (7.0 * u).cos()
    }
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

/// How the open player arranges itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// The cover as large as the height allows up to a point, the controls, and My Wave by the
    /// track under them.
    Standard,
    /// A small cover and the controls, so the lyrics or the queue beside them get the room.
    Compact,
    /// The cover as large as the window allows, the controls under it; nothing else.
    Cover,
}

/// The shape picked in Settings; "automatic" is the standard one, or the compact one in a low
/// window.
pub fn layout(s: &State, h: f32) -> Layout {
    match s.settings.player_layout {
        PlayerLayout::Auto if h < 660.0 => Layout::Compact,
        PlayerLayout::Auto | PlayerLayout::Comfortable => Layout::Standard,
        PlayerLayout::Compact => Layout::Compact,
        PlayerLayout::Cover => Layout::Cover,
    }
}

/// Whether My Wave by the track has left the column for the player's menu.
pub fn compact_player(s: &State, h: f32) -> bool {
    layout(s, h) != Layout::Standard
}

/// The player's controls are this wide whatever the window; only the cover gives way. Under a
/// larger cover they grow with it, up to `CONTROLS_MAX`.
const CONTROLS_W: f32 = 360.0;
const CONTROLS_MAX: f32 = 440.0;
const COVER_MIN: f32 = 96.0;

/// The cover's largest size in each shape.
fn cover_max(shape: Layout) -> f32 {
    match shape {
        Layout::Standard => 440.0,
        Layout::Compact => 260.0,
        Layout::Cover => 640.0,
    }
}

/// How large the cover is in a `w` x `h` window: what the height leaves over the controls, so it
/// follows the window smoothly, every point of the way.
pub fn cover_side(s: &State, w: f32, h: f32) -> f32 {
    let shape = layout(s, h);
    let rest = if shape == Layout::Standard { PLAYER_REST } else { PLAYER_COMPACT_REST };
    (h - TITLE_H - 48.0 - rest).clamp(COVER_MIN, cover_max(shape)).min(w - 2.0 * PAGE_PAD).round()
}

/// How wide the controls under a cover of `cover` points are.
fn controls_w(cover: f32) -> f32 {
    cover.clamp(CONTROLS_W, CONTROLS_MAX)
}

/// The player's share of the window while the lyrics or the queue are open: a third of it (the
/// words get the other two), never less than its controls need.
const PLAYER_SHARE: f32 = 1.0 / 3.0;
const PLAYER_MIN: f32 = 300.0;
/// The narrowest the controls go in a small window (the seven buttons of the transport still fit).
const CONTROLS_MIN: f32 = 280.0;
/// From the player's third to the words.
const PANEL_GAP: f32 = 20.0;
/// The widest the lyrics and the queue grow (past these a line stops reading as one).
const LYRICS_MAX: f32 = 1280.0;
const QUEUE_MAX: f32 = 760.0;

/// Where the player's columns stand in a `w` x `h` window: the player column's left edge and width
/// (the controls' width, or the cover's when it is wider), and the side panel's left edge and
/// width (`panel`: whether lyrics or the queue are open). With a panel the player keeps the left
/// third of the window, centred in it, and the panel takes the rest, so the words are wide in any
/// window. In a window wider than the queue needs, the player and the panel move right together
/// until the space left of the cover equals the space right of the queue. The lyrics then reach
/// further right than the queue, but the player stands in the same place for both.
pub fn columns(s: &State, w: f32, h: f32, panel: bool) -> (f32, f32, f32, f32) {
    if !panel {
        let side = cover_side(s, w, h).max(CONTROLS_W).min(w - 2.0 * PAGE_PAD);
        return ((w - side) * 0.5, side, w, 0.0);
    }
    let third = (w * PLAYER_SHARE).max(PLAYER_MIN).min(w * 0.5);
    let pad = (third * 0.09).clamp(16.0, 56.0);
    let widest = cover_max(layout(s, h)).max(CONTROLS_MAX);
    // The column keeps as much room on its left as the panel keeps on its right, narrowing for it
    // in a small window rather than creeping up to the edge.
    let side = (third - 2.0 * pad).clamp(CONTROLS_MIN, widest).min(cover_side(s, w, h).max(CONTROLS_W)).min(third - PAGE_PAD - 4.0);
    let x = ((third - side) * 0.5).max(PAGE_PAD);
    let panel_x = third + PANEL_GAP;
    let panel_w = (w - panel_x - PAGE_PAD).max(200.0);
    if panel_w <= QUEUE_MAX {
        return (x, side, panel_x, panel_w);
    }
    let gap = panel_x - x - side;
    let shift = ((w - side - gap - QUEUE_MAX) * 0.5 - x).max(0.0);
    (x + shift, side, panel_x + shift, panel_w - shift)
}

/// Where the cover stands across a `w` x `h` window: its left edge and its width.
pub fn cover_span(s: &State, w: f32, h: f32) -> (f32, f32) {
    let (x, side, _, _) = columns(s, w, h, s.np_panel() != NpTab::Closed);
    let cover = cover_side(s, w, h).min(side);
    (x + (side - cover) * 0.5, cover)
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
    // `side` is the column (the controls' width); `cover` is the picture over it, which alone
    // shrinks as the window does.
    let (x, side, panel_x, panel_w) = columns(s, w, h, panel);
    let cover = cover_side(s, w, h).min(side);
    let rest = if compact { PLAYER_COMPACT_REST } else { PLAYER_REST };
    let total_h = cover + rest;
    let top = TITLE_H + ((h - TITLE_H - 48.0 - total_h) * 0.5).max(0.0);
    // The controls under the cover: as wide as it, within reason, and centred under it.
    let ctl = controls_w(cover).min(side);

    // Each artist with their picture; the name lights up under the pointer and opens their page.
    let artist = kit::artist_line_width(s, t, th::BODY, true, "np-art", controls_w(cover).min(side));
    // The title with the track's own buttons beside it (not this one, the heart, the menu); under
    // it the artists, across the whole column (their pictures and a "2 more" need the room). The
    // buttons are bare glyphs, their right edge level with the controls' under them.
    let bad = t.clone();
    let mine = row()
        .items_center()
        .child(kit::quiet_btn("np-dislike", &np_icons::DISLIKE, SMALL_GLYPH, SMALL_BTN, th::text3(), S::Dislike.get(lang)).on_press(move |s: &mut State| s.dislike(&bad)))
        .child(kit::np_like_button(s, t, SMALL_BTN, SMALL_GLYPH))
        .child(
            kit::quiet_btn("np-more", &np_icons::MORE, SMALL_GLYPH, SMALL_BTN, th::text3(), S::More.get(lang))
                .on_press(|s: &mut State| s.open_player_menu(rune::PopupAnchor::Key("np-more".into()))),
        );
    let inset = (SMALL_BTN - SMALL_GLYPH) * 0.5;
    let head = row()
        .w(ctl + inset)
        .h(32.0)
        .items_center()
        .child(col().grow(1.0).min_w(0.0).child(kit::roll("np-title", &t.full_title(), kit::track_title(s, t, "np-title-link", th::H2, (ctl - 3.0 * SMALL_BTN).max(60.0)))))
        .child(mine);
    let info = col().w(ctl).h(58.0).gap(4.0).child(head).child(kit::roll("np-artist", &t.artists_line(), artist));

    let times = row()
        .w(ctl)
        .h(22.0)
        .items_center()
        .justify_between()
        .child(label("0:00", th::MONO, th::text3()).live_text(&s.lives.clock))
        .child(label("-0:00", th::MONO, th::text3()).live_text(&s.lives.remaining));
    // Five things on one line, centred: the way back and on with the play button between them,
    // and the two modes, faint, just outside them. Nothing has a plate until the pointer is on it.
    let repeat_icon = if s.player.repeat == crate::store::Repeat::One { &np_icons::REPEAT_ONE } else { &np_icons::REPEAT };
    let transport = row()
        .w(ctl + 2.0 * (MODE_BTN - MODE_GLYPH) * 0.5)
        .h(56.0)
        .items_center()
        .justify_center()
        .gap(MODE_GAP)
        .child(kit::quiet_toggle("np-shuffle", &np_icons::SHUFFLE, MODE_GLYPH, MODE_BTN, s.player.shuffle, S::Shuffle.get(lang)).on_press(|s: &mut State| s.toggle_shuffle()))
        .child(
            // The way back, play and on sit closer together than the modes at the ends.
            row()
                .items_center()
                .gap(TRANSPORT_GAP)
                .child(kit::skip_btn(s, false, "np-prev", &np_icons::PREV, SKIP_GLYPH, BTN + 4.0, th::text()))
                .child(kit::np_play_btn(s, 60.0, 38.0))
                .child(kit::skip_btn(s, true, "np-next", &np_icons::NEXT, SKIP_GLYPH, BTN + 4.0, th::text())),
        )
        .child(
            kit::quiet_toggle("np-repeat", repeat_icon, MODE_GLYPH, MODE_BTN, s.player.repeat != crate::store::Repeat::Off, S::Repeat.get(lang))
                .on_press(|s: &mut State| s.cycle_repeat()),
        );
    let v = s.player.volume;
    let vol_icon = if v < 0.01 {
        &np_icons::VOLUME_MUTE
    } else if v < 0.5 {
        &np_icons::VOLUME_LOW
    } else {
        &np_icons::VOLUME
    };
    let panel_toggle = |key: &'static str, ic: &'static rune::Icon, name: &str, which: NpTab| {
        let on = tab == which;
        kit::quiet_toggle(key, ic, MODE_GLYPH, MODE_BTN, on, name).on_press(move |s: &mut State| s.np_tab = if s.np_panel() == which { NpTab::Closed } else { which })
    };
    // No lyrics for this track: the button is there but off, and says why.
    let lyrics_btn = if s.lyrics_missing() {
        kit::quiet_off("np-lyrics-btn", &np_icons::LYRICS, MODE_GLYPH, MODE_BTN, S::NoLyricsShort.get(lang))
    } else {
        panel_toggle("np-lyrics-btn", &np_icons::LYRICS, S::Lyrics.get(lang), NpTab::Lyrics)
    };
    // The volume, then what opens beside the player: the words, the queue and their look.
    let edge = (MODE_BTN - MODE_GLYPH) * 0.5;
    let utility = row()
        .w(ctl + 2.0 * edge)
        .h(40.0)
        .items_center()
        .child(kit::quiet_btn("np-mute", vol_icon, MODE_GLYPH, MODE_BTN, th::text3(), S::Mute.get(lang)).on_press(|s: &mut State| s.toggle_mute()))
        .child(row().grow(1.0).min_w(0.0).ml(4.0).mr(16.0).child(kit::slider("vol", &s.lives.volume, 3.0, th::text2(), |s: &mut State, f: f32, _| s.set_volume(f))))
        .child(lyrics_btn)
        .child(panel_toggle("np-queue-btn", &np_icons::QUEUE, S::Queue.get(lang), NpTab::Queue))
        .child(look_button(s));

    // My Wave by this track; its arrow lists the others (by each artist, by the release).
    let mut choices = vec![crate::app::WaveChoice { station: crate::app::Station::Track(t.id.clone()), kind: S::ByTrack.get(lang).to_owned(), name: t.full_title() }];
    for a in t.artists.iter().take(3) {
        choices.push(crate::app::WaveChoice { station: crate::app::Station::Artist(a.id.clone()), kind: S::ByArtist.get(lang).to_owned(), name: a.name.clone() });
    }
    if let Some(al) = t.albums.first() {
        choices.push(crate::app::WaveChoice { station: crate::app::Station::Album(al.id.clone()), kind: S::ByRelease.get(lang).to_owned(), name: al.title.clone() });
    }
    let waves = kit::wave_card(s, "np-wave", S::WaveByTrack.get(lang), &t.full_title(), crate::app::Station::Track(t.id.clone()), Some(ctl), Some(choices));

    let mut left = col()
        .key("np-left")
        .absolute(x, top)
        .w(side)
        // No fade on the way in: the bars are drawn by the compositor, which skips what sits in a
        // fading group (they fade up on their own). On the way out it fades quickly.
        .items_center()
        .transition(Transition::opacity().springs(th::LEAVE))
        // It slides aside for the lyrics or the queue on a calm spring of its own.
        .motion(th::SLIDE)
        .child(
            row()
                .key("np-cover-place")
                .w_full()
                .h(cover)
                .justify_center()
                // The cover follows the window on a soft spring rather than snapping with it.
                .motion(th::SLIDE)
                .child(kit::playing_cover(s, t, cover, (cover * 0.075).clamp(14.0, 28.0), "np-cover-morph", seed_of(&t.id) % 13.0).matched("np-cover").motion(th::FLIGHT)),
        )
        // The names and the controls rise in after the cover has started to come over; the seek
        // bars are drawn by the compositor, so they fade up on their own (see `BarMem` in fx.rs).
        .child(kit::rise(info.mt(20.0), 1))
        .child(row().w(ctl).mt(10.0).child(dock::seek_bars("seek", seed_of(&t.id), 36.0)))
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
        NpTab::Lyrics => lyrics_view(s, &p, panel_x, panel_w.min(LYRICS_MAX), h),
        NpTab::Queue => queue_view(s, &p, panel_x, panel_w.min(QUEUE_MAX), h),
    };
    let come = come.clamp(0.0, 1.0);
    // The picture behind the player: the cover's colours, a burst of light from under it and the
    // rays that leave it (`fx::STAGE`). The slot is the whole window, and the burst starts at the
    // cover's middle: where the column stands, which the shader glides to as
    // the column slides aside for the lyrics.
    let stage = el().key("np-stage").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).fx(fx::STAGE).fx_data([
        come,
        seed_of(&t.id) % 100.0 + 100.0 * side.round(),
        (x + side * 0.5) / w.max(1.0),
        (top + cover * 0.5) / h.max(1.0),
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
    root = root
        .child(left)
        // The side panel waits for the player to slide over, then eases in from the right.
        .child(
            el().key(format!("np-body-{tab:?}"))
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .transition(Transition::opacity().combined(Transition::offset(32.0, 0.0)).springs(th::SOFT))
                .enter_delay(0.14)
                .child(body),
        );
    root
}

/// The button by the volume that opens the player's look: the font and size of the words, how
/// they move, and the picture behind the player, all changed in view.
fn look_button(s: &State) -> El {
    let open = s.popover_open && matches!(s.popover, Some(Popover::Stage { .. }));
    kit::quiet_toggle("np-look", &np_icons::LOOK, MODE_GLYPH, MODE_BTN, open, S::StageLook.get(s.lang))
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
        // The words come in when they arrive.
        Load::Idle | Load::Loading => el(),
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
            let pos = s.lyric_ms();
            // Timed lyrics that start a while in: a note waits above them and counts the last three
            // seconds down, 3, 2, 1, turning from one figure into the next.
            let look = s.settings.lyric_look;
            if let Some(first) = count_start(l) {
                lines = lines.child(count_note(first, pos, w, look.align));
            }
            let style = th::lyric_face(
                match (look.size, w < 480.0) {
                    (0, true) => th::LYRIC_XS,
                    (0, false) => th::LYRIC_S,
                    (1, _) => th::LYRIC_S,
                    (_, true) => th::LYRIC_S,
                    (_, false) => th::LYRIC,
                },
                look.font,
            );
            // How quiet the lines that are not being sung are.
            let back = 0.30;
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
                lines = lines.child(lyric_line(i, line, style, e, back, is_active, pos, l.synced, look.align, look.font));
            }
            // Where the lyrics came from closes them (lrc.red's terms ask for the credit).
            if let Some(c) = credit(s) {
                lines = lines.child(c.mt(12.0));
            }
            col().key("np-lyrics").absolute(x, vp_top).w(w).h(vp_h).scroll(ScrollSpec { stick_bottom: false }).child(lines)
        }
    }
}

/// The queue, in its own order, which never changes by itself: what has played above (faint,
/// one of them in view as the panel opens, the rest a scroll up), then where the music comes
/// from with the playing track under it, then what comes next. Moving to another track moves the
/// headings, never the tracks.
fn queue_view(s: &State, p: &Pal, x: f32, w: f32, h: f32) -> El {
    let lang = s.lang;
    let n = s.player.queue.len();
    if n == 0 {
        return notice(s, p, x, w, h, Ill::Tape, S::QueueEmpty.get(lang));
    }
    let cur = s.player.pending_index.or(s.player.index).unwrap_or(0).min(n - 1);
    let after = n - cur - 1;
    // The list's items: the tracks before, the heading of what plays, the playing track, then
    // (when there is more) the heading of what comes next and those tracks.
    let total = cur + 2 + if after > 0 { 1 + after } else { 0 };
    let under = kit::under();
    let rows = col().key("np-queue-rows").w_full().growth(GrowthSpec::default()).virtual_rows(
        total,
        ROW_H,
        move |s: &State, i: usize| match queue_item(cur, i) {
            QueueItem::Track(q) => Cow::Owned(format!("q{q}-{}", s.player.queue.get(q).map_or("", |t| t.id.as_str()))),
            QueueItem::Now => Cow::Borrowed("q-now"),
            QueueItem::Next => Cow::Borrowed("q-next"),
        },
        move |s: &State, i: usize| {
            kit::built_under(under, || match queue_item(cur, i) {
                QueueItem::Track(q) => match s.player.queue.get(q) {
                    // What has played already steps back; it comes forward under the pointer.
                    Some(t) if q < cur => el().key(format!("qh-{q}")).w_full().hov_group().child(kit::queue_row(s, t, q).gop(0.5, 1.0)),
                    Some(t) => kit::queue_row(s, t, q),
                    None => el().h(ROW_H),
                },
                QueueItem::Now => now_heading(s),
                QueueItem::Next => {
                    let after = s.player.queue.len().saturating_sub(cur + 1);
                    row()
                        .key("q-next-h")
                        .w_full()
                        .h(52.0)
                        .px(kit::ROW_PAD)
                        .pt(18.0)
                        .items_center()
                        .child(label(S::UpNext.get(s.lang), th::H3, th::text()))
                        .child(el().grow(1.0))
                        .child(label(s.lang.tracks(after), th::SMALL, th::text3()))
                }
            })
        },
    );
    col()
        .key("np-queue")
        .absolute(x, 0.0)
        .w(w)
        .h(h)
        .scroll(ScrollSpec { stick_bottom: false })
        .scroll_edge_fade(28.0)
        .child(col().w_full().pt(QUEUE_TOP).pb((h * 0.5).max(72.0)).child(rows))
}

/// Where the queue's list starts below the panel's top.
pub const QUEUE_TOP: f32 = TITLE_H + 16.0;

/// How far down the queue is scrolled to show the track before `cur` at its top (the rest of
/// what has played is above it).
pub fn queue_offset(cur: usize) -> f32 {
    cur.saturating_sub(1) as f32 * ROW_H
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum QueueItem {
    Track(usize),
    Now,
    Next,
}

/// What the queue's list shows at `i` while the track at `cur` plays.
fn queue_item(cur: usize, i: usize) -> QueueItem {
    if i < cur {
        QueueItem::Track(i)
    } else if i == cur {
        QueueItem::Now
    } else if i == cur + 1 {
        QueueItem::Track(cur)
    } else if i == cur + 2 {
        QueueItem::Next
    } else {
        QueueItem::Track(i - 2)
    }
}

/// The heading over the playing track: that it plays and, large, where it comes from (a press
/// there leads to it).
fn now_heading(s: &State) -> El {
    let lang = s.lang;
    let (caption, name, page) = match &s.playing_from {
        Some(from) => {
            let name = match &from.name {
                crate::app::FromName::Text(t) => t.trim().to_owned(),
                crate::app::FromName::Word(w) => w.get(lang).to_owned(),
            };
            let kind = from.kind.get(lang);
            if name.is_empty() || name.to_lowercase() == kind.to_lowercase() {
                (S::NowPlaying.get(lang).to_owned(), kind.to_owned(), from.page.clone())
            } else {
                (format!("{} · {}", S::NowPlaying.get(lang), kind.to_lowercase()), name, from.page.clone())
            }
        }
        None => (String::new(), S::NowPlaying.get(lang).to_owned(), None),
    };
    let mut title = row().key("q-now-name").min_w(0.0).child(label(name, th::SHEET_TITLE, th::text()).truncate().min_w(0.0));
    if let Some(page) = page {
        title = title
            .role("link")
            .cursor(Cursor::Pointer)
            .hov_group()
            .on_press(move |s: &mut State| s.go(page.clone()))
            .child(icon(&icons::CHEVRON_RIGHT, 18.0).color(th::text3()).gcol(th::text()).ml(4.0));
    }
    let mut c = col().key("q-now-h").w_full().px(kit::ROW_PAD).pt(20.0).pb(10.0).gap(2.0);
    if !caption.is_empty() {
        c = c.child(label(caption, th::SMALL, th::text2()));
    }
    c.child(title.items_center())
}
