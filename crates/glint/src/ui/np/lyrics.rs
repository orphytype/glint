//! Synced and plain lyrics panel, word-by-word fill spans, and line emphasis stepping.

use std::borrow::Cow;
use std::cell::RefCell;
use std::hash::{Hash, Hasher};

use rune::{Cursor, El, ScrollSpec, Span, Tx, col, el, icon, row};
use ym_api::{LyricLine, LyricWord, LyricsSource, join_words};

use super::count::{COUNT_W, TargetLine, count_note, count_start};
use super::notice;
use crate::app::{Load, State};
use crate::i18n::{Lang, S};
use crate::theme::{self as th, Pal};
use crate::ui::icons;
use crate::ui::kit::{self, label};
use crate::ui::shapes::Ill;

/// Where the sung line rests, as a fraction of the window's height.
pub const REST: f32 = 0.35;

/// Unsung word opacity next to sung words.
const UNSUNG: f32 = 0.46;

/// Builds quantized character-level color spans for word-synced lines so held words wipe smoothly
/// while merging consecutive characters of equal brightness to minimize text-shaping churn.
pub fn sung_spans(words: &[LyricWord], pos_ms: u32) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    let mut at = 0usize;
    let mut last: Option<(u8, usize)> = None;
    for (i, w) in words.iter().enumerate() {
        let p = w.progress(pos_ms);
        let n = w.text.chars().count().max(1) as f32;
        for (k, (off, ch)) in w.text.char_indices().enumerate() {
            let lit = (p * n - k as f32).clamp(0.0, 1.0);
            let e = lit * lit * (3.0 - 2.0 * lit);
            let step = (e * 8.0).round() as u8;
            let (from, to) = (at + off, at + off + ch.len_utf8());
            match last {
                Some((prev_step, idx)) if prev_step == step && spans[idx].end as usize == from => spans[idx].end = to as u32,
                _ => {
                    let q = f32::from(step) / 8.0;
                    spans.push(Span::new(from, to).color(th::veil(UNSUNG + (1.0 - UNSUNG) * q)));
                    last = Some((step, spans.len() - 1));
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

/// Whether the active lyric line has word timing and playback is still inside it.
pub fn wiping(s: &State) -> bool {
    let (Load::Ready(l), Some(i)) = (&s.lyrics.lyrics, s.lyrics.active) else { return false };
    let Some(line) = l.lines.get(i) else { return false };
    let Some(end) = line.words.last().map(|w| w.end_ms).max(line.backing.last().map(|w| w.end_ms)) else { return false };
    s.player.playing && s.lyric_ms() <= end + 400
}

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


/// Translates common English song section headings ("Verse 2", "Chorus") into Russian when needed.
pub fn part_name(part: &str, lang: Lang) -> String {
    if lang != Lang::Ru {
        return part.to_owned();
    }
    let p = part.trim().trim_start_matches('[').trim_end_matches(']').trim();
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
            if rest.chars().next().is_some_and(char::is_alphabetic) {
                continue;
            }
            return format!("{ru}{}", &p[en.len()..]);
        }
    }
    p.to_owned()
}

struct LineCtx {
    style: rune::TextStyle,
    pos_ms: u32,
    synced: bool,
    align: u8,
    font: u8,
    reduce: bool,
    cpu: bool,
    resizing: bool,
    hover: bool,
}

/// Computes progressive per-line opacity matching Yandex Music's synced lyrics view:
/// - active line: full brightness (`1.0`);
/// - upcoming lines below: progressively dimmer with each step (`+1`, `+2`, `+3`, ...);
/// - past lines above: hidden normally (`0.0`), and softly visible on hover (`-1`, `-2`, ...).
fn stepped_opacity(delta: isize, hover: bool) -> f32 {
    if delta == 0 {
        1.0
    } else if delta > 0 {
        match delta {
            1 => 0.42,
            2 => 0.25,
            3 => 0.15,
            4 => 0.09,
            _ => 0.05,
        }
    } else if hover {
        match -delta {
            1 => 0.30,
            2 => 0.19,
            3 => 0.11,
            4 => 0.07,
            _ => 0.04,
        }
    } else {
        0.0
    }
}

fn lyric_line(i: usize, line: &LyricLine, active: bool, delta: isize, near: bool, ctx: &LineCtx) -> El {
    let (side, ox) = match ctx.align {
        1 => (rune::Align::Center, 0.5),
        2 => (rune::Align::End, 1.0),
        _ => (rune::Align::Start, 0.0),
    };
    let timed = !line.words.is_empty();
    let main = if timed { join_words(&line.words) } else { line.text.clone() };
    let target_opacity = if !ctx.synced { 0.95 } else { stepped_opacity(delta, ctx.hover) };
    // Only animate lines close to the active line on GPU; CPU mode skips scale transforms &
    // spring allocations completely so lyrics stay ultra-lightweight.
    let anim: rune::Anim = if ctx.reduce || ctx.cpu || ctx.resizing || !near {
        rune::Anim::INSTANT
    } else {
        rune::Spring::new(0.28, 0.0).into()
    };
    // Apply opacity on leaf text elements (never on the `col()` container) so RUNE paints every
    // lyric line directly in the main pass instead of allocating an offscreen `Rgba16Float` layer
    // per line.
    let mut text = label(main, ctx.style, th::veil(1.0))
        .key(format!("ly-{i}"))
        .w_full()
        .wrap()
        .align(side)
        .opacity(target_opacity)
        .opacity_motion(anim)
        .motion(rune::Spring::NONE);
    if timed && active && !ctx.reduce {
        text = text.spans(sung_spans(&line.words, ctx.pos_ms));
    }
    let mut block = col().key(format!("ly-row-{i}")).w_full().gap(6.0).transition(rune::Transition::none()).motion(rune::Spring::NONE).child(text);
    if !ctx.cpu {
        let target_scale = if !ctx.synced || active { 1.0 } else { 0.92 };
        block = block.animated_changes().origin(ox, 0.5).scale(target_scale).scale_motion(anim);
    }
    if !line.backing.is_empty() {
        let mut back = label(line.backing_text(), th::lyric_face(th::ASIDE, ctx.font), th::veil(0.64))
            .w_full()
            .wrap()
            .align(side)
            .opacity(target_opacity)
            .opacity_motion(anim)
            .motion(rune::Spring::NONE);
        if active && !ctx.reduce {
            back = back.spans(sung_spans(&line.backing, ctx.pos_ms));
        }
        block = block.child(back);
    }
    if ctx.synced
        && let Some(ms) = line.time_ms
    {
        block = block.cursor(Cursor::Pointer).on_press(move |s: &mut State| s.seek_ms(u64::from(ms)));
    }
    block
}

/// Unconstrained lyrics width, cached by lyrics generation and typography so window resizes
/// clamp without re-measuring every line.
fn lyrics_block_width(lyrics_gen: u64, l: &ym_api::Lyrics, style: rune::TextStyle, font: u8, max_w: f32, has_credit: bool, lang: Lang) -> f32 {
    thread_local! {
        static CACHE: RefCell<Option<(u64, f32)>> = const { RefCell::new(None) };
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    lyrics_gen.hash(&mut hasher);
    (l.lines.as_ptr() as usize).hash(&mut hasher);
    l.lines.len().hash(&mut hasher);
    l.synced.hash(&mut hasher);
    style.size.to_bits().hash(&mut hasher);
    font.hash(&mut hasher);
    has_credit.hash(&mut hasher);
    (lang as u8).hash(&mut hasher);
    let key = hasher.finish();
    if let Some(w) = CACHE.with_borrow(|c| c.and_then(|(k, w)| (k == key).then_some(w))) {
        return w.clamp(COUNT_W.min(max_w), max_w);
    }
    let mut widest = if count_start(l).is_some() { COUNT_W } else { 0.0 };
    if has_credit {
        widest = widest.max(rune::text::measure(S::LyricsBy.get(lang), &[], th::LABEL, None).0 + 22.0);
    }
    let aside_style = th::lyric_face(th::ASIDE, font);
    for line in &l.lines {
        let text: Cow<'_, str> = if !line.words.is_empty() { Cow::Owned(join_words(&line.words)) } else { Cow::Borrowed(&line.text) };
        if !text.trim().is_empty() {
            widest = widest.max(rune::text::measure(&text, &[], style, None).0);
        }
        if !line.backing.is_empty() {
            widest = widest.max(rune::text::measure(&line.backing_text(), &[], aside_style, None).0);
        }
        if l.synced
            && let Some(part) = line.part.as_deref()
        {
            widest = widest.max(rune::text::measure(&part_name(part, lang), &[], th::LABEL, None).0);
        }
    }
    let raw = widest.ceil() + 2.0;
    CACHE.with_borrow_mut(|c| *c = Some((key, raw)));
    raw.clamp(COUNT_W.min(max_w), max_w)
}

pub(super) fn lyrics_view(s: &State, p: &Pal, x: f32, w: f32, h: f32) -> El {
    let lang = s.lang;
    match &s.lyrics.lyrics {
        Load::Idle | Load::Loading => el(),
        Load::Failed(_) => notice(p, x, w, h, Ill::Mic, S::NoLyrics.get(lang)),
        Load::Ready(l) if l.instrumental => notice(p, x, w, h, Ill::Tape, S::Instrumental.get(lang)),
        Load::Ready(l) if !l.has_words() => notice(p, x, w, h, Ill::Mic, S::NoLyrics.get(lang)),
        Load::Ready(l) => {
            let active = if l.synced { s.lyrics.active } else { None };
            let vp_h = h * (2.0 * (1.0 - REST));
            let vp_top = h * REST - vp_h * 0.5;
            let pos = s.lyric_ms();
            let look = s.settings.lyric_look;
            let style = th::lyric_face(
                match (look.size, w < 480.0) {
                    (0, _) => th::LYRIC_XS,
                    (1, _) | (_, true) => th::LYRIC_S,
                    (_, false) => th::LYRIC,
                },
                look.font,
            );
            let credit_el = credit(s);
            let block_w = lyrics_block_width(s.lyrics_gen, l, style, look.font, w, credit_el.is_some(), lang);
            let mut lines = col().w(block_w).gap(20.0).pt(vp_h * 0.5).pb(vp_h * 0.5).motion(rune::Spring::NONE);
            if let Some(first) = count_start(l) {
                let mut empty_before = 0usize;
                let mut first_line_idx = None;
                for (i, line) in l.lines.iter().enumerate() {
                    if line.text.trim().is_empty() && line.words.is_empty() {
                        empty_before += 1;
                    } else {
                        first_line_idx = Some(i);
                        break;
                    }
                }
                let top_y = 38.0 + 20.0 + empty_before as f32 * 40.0;
                let (target_text, target_style): (Cow<'_, str>, rune::TextStyle) = match first_line_idx.and_then(|i| l.lines.get(i)) {
                    Some(line) if l.synced && line.part.is_some() => {
                        let name = part_name(line.part.as_deref().unwrap_or_default(), lang);
                        (Cow::Owned(name), th::LABEL)
                    }
                    Some(line) => {
                        let main = if !line.words.is_empty() { Cow::Owned(join_words(&line.words)) } else { Cow::Borrowed(line.text.as_str()) };
                        (main, style)
                    }
                    None => (Cow::Borrowed(""), style),
                };
                lines = lines.child(count_note(first, pos, block_w, look.align, s.settings.reduce_motion, TargetLine { text: &target_text, style: target_style, font: look.font, top_y }));
            }
            let cpu = crate::fx::cpu_mode();
            let pos_bucket = pos / if cpu { 80 } else { 55 };
            let hover = s.lyrics.hover;
            let ctx = LineCtx { style, pos_ms: pos, synced: l.synced, align: look.align, font: look.font, reduce: s.settings.reduce_motion, cpu, resizing: s.is_resizing(), hover };
            for (i, line) in l.lines.iter().enumerate() {
                if line.text.trim().is_empty() && line.words.is_empty() {
                    lines = lines.child(el().key(format!("ly-{i}")).h(20.0));
                    continue;
                }
                let delta: isize = match active {
                    Some(a) => i as isize - a as isize,
                    None => i as isize + 1,
                };
                if l.synced
                    && let Some(part) = line.part.as_deref()
                {
                    let name = part_name(part, lang);
                    let part_op = stepped_opacity(delta, hover);
                    let caps = kit::caps(name.as_str(), th::text3())
                        .w_full()
                        .opacity(part_op)
                        .align(match look.align {
                            1 => rune::Align::Center,
                            2 => rune::Align::End,
                            _ => rune::Align::Start,
                        });
                    lines = lines.child(caps.mt(if i == 0 { 0.0 } else { 18.0 }));
                }
                let is_active = l.synced && active == Some(i);
                let near = active.is_none_or(|a| i.abs_diff(a) <= 6);
                let row_el = if is_active && !ctx.reduce && (!line.words.is_empty() || !line.backing.is_empty()) {
                    let version = (s.lyrics_gen, pos_bucket, look.size, look.align, look.font, ctx.cpu, ctx.resizing, th::light());
                    rune::memo(format!("ly-memo-{i}"), version, || lyric_line(i, line, true, 0, true, &ctx))
                } else {
                    let delta_bucket = delta.clamp(-5, 5);
                    let flags = u8::from(is_active)
                        | (u8::from(hover) << 1)
                        | (u8::from(near) << 2)
                        | (u8::from(l.synced) << 3)
                        | (u8::from(ctx.reduce) << 4)
                        | (u8::from(ctx.cpu) << 5)
                        | (u8::from(ctx.resizing) << 6)
                        | (u8::from(th::light()) << 7);
                    let version = (s.lyrics_gen, delta_bucket, flags, look.size, look.align, look.font);
                    rune::memo(format!("ly-memo-{i}"), version, || lyric_line(i, line, is_active, delta, near, &ctx))
                };
                lines = lines.child(row_el);
            }
            if let Some(c) = credit_el {
                let row_c = match look.align {
                    1 => row().w_full().justify_center(),
                    2 => row().w_full().justify_end(),
                    _ => row().w_full().justify_start(),
                };
                lines = lines.child(row_c.child(c).mt(12.0));
            }
            col()
                .key("np-lyrics")
                .absolute(x, vp_top)
                .w(w)
                .h(vp_h)
                .motion(rune::Spring::NONE)
                .on_hover_change(|s: &mut State, on: bool, _tx: &mut Tx| s.lyrics.hover = on)
                .scroll(ScrollSpec { stick_bottom: false })
                .scroll_edge_fade(56.0)
                .child(lines)
        }
    }
}
