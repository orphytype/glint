//! Covers, rolling titles, My Wave cards/buttons/glyph, and artist link lines.

use std::sync::OnceLock;

use crate::fx::Slot as _;
use rune::{Color, Cursor, El, SurfaceFit, TextStyle, Transition, Tx, col, el, icon, row};
use ym_api::{Artist, Track};

use super::marquee::marquee;
use super::{Hover as _, label, under};
use crate::app::{Page, Popover, Source, State, Station, WaveChoice};
use crate::fx;
use crate::i18n::S;
use crate::theme::{self as th, veil};
use crate::ui::{blobs, icons};

pub fn cover_border() -> Color {
    veil(if th::light() { 0.12 } else { 0.15 })
}

fn cover_px(s: &State, template: Option<&str>, px: u32, pt: f32, radius: f32) -> El {
    let base = el().w(pt).h(pt).radius(radius).fill(veil(0.06)).transition(Transition::none());
    let edge = el().absolute(0.0, 0.0).w_full().h_full().radius(radius).border(1.0, cover_border());
    match State::cover_key_px(template, px) {
        Some(key) => base.child(picture(s, &key, "img", [radius; 4])).child(edge),
        None => base.center().child(icon(&icons::NOTE, pt * 0.42).color(th::text3())).child(edge),
    }
}

/// Cover art loaded at 200x200 for cards, grids, and detail page headers.
pub fn cover(s: &State, template: Option<&str>, pt: f32, radius: f32) -> El {
    cover_px(s, template, crate::art::COVER_CARD, pt, radius)
}

/// Cover art placeholder box (same visual geometry as `cover` without requesting a download/decode).
pub fn cover_placeholder(pt: f32, radius: f32) -> El {
    let base = el().w(pt).h(pt).radius(radius).fill(veil(0.06)).transition(Transition::none());
    let edge = el().absolute(0.0, 0.0).w_full().h_full().radius(radius).border(1.0, cover_border());
    base.center().child(icon(&icons::NOTE, pt * 0.42).color(th::text3())).child(edge)
}

/// Cover art loaded at 100x100 for track lists, queues, small avatars, and pins.
pub fn cover_sm(s: &State, template: Option<&str>, pt: f32, radius: f32) -> El {
    cover_px(s, template, crate::art::COVER_SM, pt, radius)
}

/// Borderless cover tile for collages framed by one outer border.
pub fn cover_tile(s: &State, template: Option<&str>, pt: f32, corners: [f32; 4]) -> El {
    let base = el().w(pt).h(pt).corners(corners).fill(veil(0.06)).transition(Transition::none());
    match State::cover_key(template) {
        Some(key) => base.child(picture(s, &key, "img", corners)),
        None => base.center().child(icon(&icons::NOTE, pt * 0.42).color(th::text3())),
    }
}

fn picture(s: &State, key: &str, name: &str, corners: [f32; 4]) -> El {
    let px = s.art.pixels(key);
    el().key(format!("{name}-{key}")).absolute(0.0, 0.0).w_full().h_full().corners(corners).pixels_fit(&px, SurfaceFit::Fill).transition(Transition::none())
}

pub fn picture_url(s: &State, url: &str, radius: f32) -> El {
    picture(s, url, "pic", [radius; 4])
}

/// Playing track cover at 100x100 for the mini-player dock (`pt <= 64`) or 200x200 on Home.
/// Renders immediately with `Transition::none()` when ready so opening/closing Now Playing never
/// fades the cover in from opacity 0 (no grey blink); falls back to the previous cover while decoding.
pub fn playing_cover(s: &State, t: &Track, pt: f32, radius: f32, track_key: &str) -> El {
    let base = el().w(pt).h(pt).radius(radius).fill(veil(0.06)).transition(Transition::none());
    let edge = el().absolute(0.0, 0.0).w_full().h_full().radius(radius).border(1.0, cover_border());
    let px = if pt <= 64.0 { crate::art::COVER_SM } else { crate::art::COVER_CARD };
    let Some(key) = State::cover_key_px(t.cover_template(), px) else {
        return base.center().child(icon(&icons::NOTE, pt * 0.42).color(th::text3())).child(edge);
    };
    let pic = if s.art.ready(&key) {
        picture(s, &key, "play", [radius; 4])
    } else {
        let _ = s.art.pixels(&key);
        match s.palette_key.as_deref().filter(|h| s.art.ready(h)) {
            Some(hold) => picture(s, hold, "play-hold", [radius; 4]),
            None => picture(s, &key, "play", [radius; 4]),
        }
    };
    base.child(pic)
        .child(edge)
        .child(el().key(track_key.to_owned()).absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).fx(fx::COVER_MARK))
}

/// Playing track hero cover loaded at 600x600 ONLY when Now Playing is open and settled (200x200
/// in CPU mode). Uses `Transition::none()` on both the 200px underlayer and the decoded 600px
/// upgrade so there is ZERO opacity-0 grey flash on open, upgrade, or collapse.
pub fn playing_cover_lg(s: &State, t: &Track, pt: f32, radius: f32, track_key: &str, settled: bool) -> El {
    if fx::cpu_mode() {
        return playing_cover(s, t, pt, radius, track_key);
    }
    let base = el().w(pt).h(pt).radius(radius).fill(veil(0.06)).transition(Transition::none());
    let edge = el().absolute(0.0, 0.0).w_full().h_full().radius(radius).border(1.0, cover_border());
    let Some(key_lg) = State::cover_key_px(t.cover_template(), crate::art::COVER_LG) else {
        return base.center().child(icon(&icons::NOTE, pt * 0.42).color(th::text3())).child(edge);
    };
    let lg_ready = s.art.ready(&key_lg);
    if !lg_ready && s.np_open && settled {
        // Schedule the 600px decode only after Now Playing settles so opening never hitches.
        let _ = s.art.pixels(&key_lg);
        s.art.watch(&key_lg);
    }
    let pic = if lg_ready {
        picture(s, &key_lg, "np-lg", [radius; 4])
    } else if let Some(card) = State::cover_key(t.cover_template()) {
        if s.art.ready(&card) {
            picture(s, &card, "np-card", [radius; 4])
        } else {
            let _ = s.art.pixels(&card);
            match s.palette_key.as_deref().filter(|h| s.art.ready(h)) {
                Some(hold) => picture(s, hold, "np-hold", [radius; 4]),
                None => picture(s, &card, "np-card", [radius; 4]),
            }
        }
    } else {
        return base.center().child(icon(&icons::NOTE, pt * 0.42).color(th::text3())).child(edge);
    };
    base.child(pic)
        .child(edge)
        .child(el().key(track_key.to_owned()).absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).fx(fx::COVER_MARK))
}

pub fn roll(name: &str, id: &str, line: El) -> El {
    let enter = Transition::opacity().combined(Transition::offset(0.0, 6.0));
    let exit = Transition::opacity().combined(Transition::offset(0.0, -6.0));
    el().key(format!("{name}-{id}")).w_full().min_w(0.0).animated_changes().transition(Transition::asymmetric(enter, exit).springs(th::ROLL)).child(line)
}

pub fn track_title(s: &State, t: &Track, key: &'static str, style: TextStyle, width: f32) -> El {
    let name = t.full_title();
    let hover = s.title_hover.filter(|(k, _)| *k == key && !under()).map(|(_, since)| since);
    let album = t.albums.first().map(|a| a.id.clone());
    let name_w = rune::text::measure(&name, &[], style, None).0.ceil();
    let long = name_w > width + 0.5;
    let hover = if long { None } else { hover };
    let base = if hover.is_some() { th::text().alpha(0.62) } else { th::text() };
    let line = if long {
        let n2 = name.clone();
        marquee(format!("{key}-mq"), width, name_w, true, s.settings.reduce_motion, move || label(n2.clone(), style, base))
    } else {
        label(name.clone(), style, base).truncate().max_w(width)
    };
    let mut r = el().key(key).max_w(width).min_w(0.0).child(line);
    let Some(id) = album else { return r };
    r = r
        .role("link")
        .label(name.as_str())
        .cursor(Cursor::Pointer)
        .on_hover_change(move |s: &mut State, on: bool, _tx: &mut Tx| {
            if on {
                if s.title_hover.is_none_or(|(k, _)| k != key) {
                    s.title_hover = Some((key, s.now));
                }
            } else if s.title_hover.is_some_and(|(k, _)| k == key) {
                s.title_hover = None;
            }
        })
        .on_press(move |s: &mut State| {
            s.title_hover = None;
            s.go(Page::Album(id.clone()));
        });
    if let Some(since) = hover.filter(|_| !s.settings.reduce_motion) {
        let band = 64.0;
        let speed = 300.0;
        let travel = width + 2.0 * band + speed * 0.7;
        let p = f64::from(travel / speed);
        let delay = {
            let d = (-(s.now - since) / p).rem_euclid(1.0) * p;
            if d > p - 1e-6 { 0.0 } else { d }
        };
        let spec = |a: f32, b: f32| rune::LoopSpec { delay, ..rune::LoopSpec::slide(p, [a, 0.0], [b, 0.0]).easing(rune::Easing::LINEAR) };
        let (x0, x1) = (-band, travel - band);
        let lit_color = if th::light() { th::rgba(0x000000, 1.0) } else { th::rgba(0xffffff, 1.0) };
        let lit = el().absolute(0.0, 0.0).w(band).h_full().looping(spec(-x0 / band, -x1 / band)).child(label(name, style, lit_color).truncate().w(width).absolute(0.0, 0.0));
        let sheen = el().key(format!("{key}-ray")).absolute(0.0, 0.0).w(band).h_full().clip_fade(band * 0.5, 0.0, band * 0.5, 0.0).looping(spec(x0 / band, x1 / band)).child(lit);
        r = r.child(sheen);
    }
    r
}

#[expect(clippy::too_many_arguments, reason = "explicit wave card configuration and transition alpha")]
pub fn wave_card_alpha(s: &State, key: &str, caption: &str, name: &str, station: Station, w: Option<f32>, more: Option<Vec<WaveChoice>>, alpha: f32) -> El {
    let a = alpha.clamp(0.0, 1.0);
    let playing = matches!(&s.player.source, Source::Wave { preset, .. } if station.is(preset)) && s.current().is_some();
    let on = playing && s.player.playing;
    let seed = (key.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(u32::from(b))) % 997) as f32 + 0.37;
    let hovered = !under() && s.wave_hover.as_deref() == Some(key);
    let owner = key.to_owned();
    let wave_name = name.to_owned();
    let go = col()
        .key(format!("{key}-go"))
        .role("button")
        .label(caption)
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .pl(14.0)
        .pr(if more.is_some() { 10.0 } else { 18.0 })
        .gap(1.0)
        .justify_center()
        .corners(if more.is_some() { [12.0, 0.0, 0.0, 12.0] } else { [12.0; 4] })
        .cursor(Cursor::Pointer)
        .hov(veil(0.04 * a))
        .press(veil(0.10 * a))
        .transition(Transition::none())
        .on_press(move |s: &mut State| {
            if playing {
                s.toggle_pause();
            } else {
                s.start_station(&station, &wave_name);
            }
        })
        .child(label(caption.to_owned(), th::SMALL, th::text2().alpha(a)).truncate())
        .child(
            row()
                .gap(6.0)
                .items_center()
                .min_w(0.0)
                .child(icon(if on { &icons::PAUSE } else { &icons::PLAY }, 16.0).color(th::text().alpha(a)).ml(-3.0))
                .child(label(name.to_owned(), th::H3, th::text().alpha(a)).truncate().min_w(0.0)),
        );
    let mut c = row()
        .key(key.to_owned())
        .h(58.0)
        .items_center()
        .radius(12.0)
        .transition(Transition::none())
        .on_hover_change(move |s: &mut State, over: bool, _tx: &mut Tx| {
            if over {
                s.wave_hover = Some(owner.clone());
            } else if s.wave_hover.as_deref() == Some(owner.as_str()) {
                s.wave_hover = None;
            }
        })
        .child(blobs::ribbon_alpha(format!("{key}-ribbon"), seed, s.palette.wave, hovered, playing, 12.0, a))
        // A shade over the GPU ribbon shader so the words and menu arrow stay readable over glowing ribbons.
        .children(fx::ribbon_ready().then(|| el().absolute(0.0, 0.0).w_full().h_full().radius(12.0).fill(th::bg().alpha(0.32 * a))))
        .child(go);
    if let Some(choices) = more {
        let anchor = format!("{key}-more");
        let open = s.popover_open && matches!(&s.popover, Some(Popover::Waves { anchor: a, .. }) if *a == anchor);
        let a2 = anchor.clone();
        c = c.child(el().w(1.0).h(24.0).fill(veil(0.14 * a))).child(
            row()
                .key(anchor)
                .role("button")
                .label(S::MoreWaves.get(s.lang))
                .help(S::MoreWaves.get(s.lang))
                .checked(open)
                .w(44.0)
                .h_full()
                .corners([0.0, 12.0, 12.0, 0.0])
                .center()
                .cursor(Cursor::Pointer)
                .hov(veil(0.06 * a))
                .press(veil(0.12 * a))
                .transition(Transition::none())
                .on_press(move |s: &mut State| s.open_popover(Popover::Waves { choices: choices.clone(), anchor: a2.clone() }))
                .child(super::chevron(17.0, open, th::text().alpha(a))),
        );
    }
    match w {
        Some(w) => c.w(w),
        None => c.min_w(200.0).max_w(320.0),
    }
}

/// Compact semantic icon for source badges in Now Playing and on Home.
pub fn source_glyph(from: &crate::app::PlayingFrom, color: Color, size: f32) -> El {
    if from.wave {
        return wave_glyph(size + 2.0, color);
    }
    let ic = if matches!(from.name, crate::app::FromName::Word(S::ListeningHistory)) {
        &icons::CLOCK
    } else {
        match from.kind {
            S::Album | S::Single | S::Compilation => &icons::DISC,
            S::Artist => &icons::USER,
            S::Liked => &icons::HEART_FILL,
            S::Search => &icons::SEARCH,
            _ => &icons::QUEUE,
        }
    };
    icon(ic, size).color(color)
}

/// My Wave glyph at `side` points, reusing cached unit curve points.
pub fn wave_glyph(side: f32, color: Color) -> El {
    static WAVE_CURVE: OnceLock<Vec<[f32; 2]>> = OnceLock::new();
    let unit = WAVE_CURVE.get_or_init(|| {
        use std::f32::consts::{PI, TAU};
        let mut a = Vec::with_capacity(58);
        let (s0, s1, s2) = ([0.08f32, 0.80f32], [0.30f32, 0.80f32], [0.30f32, 0.46f32]);
        for i in 0..=12 {
            let t = i as f32 / 12.0;
            let u = 1.0 - t;
            a.push([u * u * s0[0] + 2.0 * u * t * s1[0] + t * t * s2[0], u * u * s0[1] + 2.0 * u * t * s1[1] + t * t * s2[1]]);
        }
        let (cx, cy) = (0.57f32, 0.46f32);
        for i in 1..=44 {
            let u = i as f32 / 44.0;
            let ang = PI + TAU * 1.12 * u;
            let r = 0.27 * (1.0 - 0.72 * u);
            a.push([cx + r * ang.cos(), cy + r * ang.sin()]);
        }
        a
    });
    let a: Vec<[f32; 2]> = unit.iter().map(|&[x, y]| [x * side, y * side]).collect();
    let water = vec![[0.50 * side, 0.84 * side], [0.92 * side, 0.84 * side]];
    let w = (side * 0.085).max(1.6);
    rune::canvas(rune::Shape::new().stroke_with(a, false, w, rune::LineCap::Round, color).stroke_with(water, false, w, rune::LineCap::Round, color).view_box(side, side))
        .w(side)
        .h(side)
        .transition(Transition::none())
}

/// The wave button as a part of a `button_group`: the same moving picture, as a tile inside the
/// part, so it sits anywhere in the group.
pub fn seg_wave(s: &State, key: &str, caption: &str, name: &str, station: Station) -> El {
    const TILE: f32 = 26.0;
    const TILE_R: f32 = 7.0;
    let playing = matches!(&s.player.source, Source::Wave { preset, .. } if station.is(preset)) && s.current().is_some();
    let on = playing && s.player.playing;
    let seed = (key.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(u32::from(b))) % 997) as f32 + 0.37;
    let hovered = !under() && s.wave_hover.as_deref() == Some(key);
    let owner = key.to_owned();
    let wave_name = name.to_owned();
    let tip = if on { format!("{caption} · {}", S::Pause.get(s.lang)) } else { format!("{caption}: {name}") };
    let tile = row()
        .w(TILE)
        .h(TILE)
        .radius(TILE_R)
        .center()
        .child(blobs::ribbon(format!("{key}-ribbon"), seed, s.palette.wave, hovered, playing, TILE_R))
        .child(if on { icon(&icons::PAUSE, 11.0).color(th::text()) } else { wave_glyph(16.0, th::text()) })
        .child(el().absolute(0.0, 0.0).w_full().h_full().radius(TILE_R).border(1.0, if playing { th::text().alpha(0.55) } else { veil(0.14) }));
    row()
        .key(key.to_owned())
        .role("button")
        .label(caption)
        .help(tip)
        .checked(playing)
        .w(super::BTN_H)
        .h(super::BTN_H)
        .center()
        .cursor(Cursor::Pointer)
        .hov(veil(0.05))
        .press(veil(0.09))
        .transition(Transition::none())
        .on_hover_change(move |s: &mut State, over: bool, _tx: &mut Tx| {
            if over {
                s.wave_hover = Some(owner.clone());
            } else if s.wave_hover.as_deref() == Some(owner.as_str()) {
                s.wave_hover = None;
            }
        })
        .on_press(move |s: &mut State| {
            if playing {
                s.toggle_pause();
            } else {
                s.start_station(&station, &wave_name);
            }
        })
        .child(tile)
}

fn avatar(s: &State, a: &Artist, size: f32) -> El {
    if a.cover_template().is_some() {
        return cover_sm(s, a.cover_template(), size, size * 0.5);
    }
    let initial = a.name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    row().w(size).h(size).radius(size * 0.5).center().fill(veil(0.10)).child(label(initial, th::LABEL, th::text2()))
}

pub fn artist_line_width(s: &State, t: &Track, style: TextStyle, pictures: bool, key: &str, width: f32) -> El {
    artists_row_in_width(s, &t.artists, style, pictures, key, th::text2(), width, true)
}

fn artists_all(s: &State, artists: &[Artist], style: TextStyle, pictures: bool, key: &str, color: Color) -> El {
    let mut r = row().items_center().gap(if pictures { 14.0 } else { 0.0 });
    let n = artists.len();
    for (i, a) in artists.iter().enumerate() {
        let id = a.id.clone();
        let text = if !pictures && i + 1 < n { format!("{}, ", a.name) } else { a.name.clone() };
        let mut item = row()
            .key(format!("{key}-a{i}"))
            .role("link")
            .label(a.name.as_str())
            .items_center()
            .gap(7.0)
            .cursor(Cursor::Pointer)
            .hov_group()
            .on_press(move |s: &mut State| s.go(Page::Artist(id.clone())));
        if pictures {
            item = item.child(avatar(s, a, 22.0));
        }
        r = r.child(item.child(label(text, style, color).gcol(th::text())));
    }
    r
}

fn artists_width(artists: &[Artist], style: TextStyle, pictures: bool) -> f32 {
    let n = artists.len();
    if n == 0 {
        return 0.0;
    }
    let sep_w = if !pictures && n > 1 { rune::text::measure(", ", &[], style, None).0 } else { 0.0 };
    let base: f32 = artists
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let extra = if pictures {
                29.0
            } else if i + 1 < n {
                sep_w
            } else {
                0.0
            };
            (rune::text::measure(&a.name, &[], style, None).0 + extra).ceil()
        })
        .sum();
    base + if pictures { 14.0 * (n - 1) as f32 } else { 0.0 }
}

#[expect(clippy::too_many_arguments, reason = "explicit typography and marquee layout inputs")]
pub fn artists_row_in_width(s: &State, artists: &[Artist], style: TextStyle, pictures: bool, key: &str, color: Color, width: f32, moving: bool) -> El {
    let line_w = artists_width(artists, style, pictures);
    let reduce = s.settings.reduce_motion;
    marquee(format!("{key}-mq"), width, line_w, moving, reduce, || artists_all(s, artists, style, pictures, key, color))
}
