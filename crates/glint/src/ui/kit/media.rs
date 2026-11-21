//! Covers, rolling titles, My Wave cards/buttons/glyph, and artist link lines.

use std::sync::OnceLock;

use crate::fx::Slot as _;
use rune::{Color, Cursor, El, SurfaceFit, TextStyle, Transition, Tx, col, el, icon, row};
use ym_api::{Artist, Track};

use super::controls::ctl_radius;
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

/// Cover art loaded at the single 300x300 size so every view shares one cached image.
pub fn cover(s: &State, template: Option<&str>, pt: f32, radius: f32) -> El {
    let base = el().w(pt).h(pt).radius(radius).clip_rounded(radius).fill(veil(0.06)).transition(Transition::none());
    let edge = el().absolute(0.0, 0.0).w_full().h_full().radius(radius).border(1.0, cover_border());
    match State::cover_key(template) {
        Some(key) => base.child(picture(s, &key, "img")).child(edge),
        None => base.center().child(icon(&icons::NOTE, pt * 0.42).color(th::text3())).child(edge),
    }
}

/// Borderless cover tile for collages framed by one outer border.
pub fn cover_tile(s: &State, template: Option<&str>, pt: f32) -> El {
    let base = el().w(pt).h(pt).fill(veil(0.06)).transition(Transition::none());
    match State::cover_key(template) {
        Some(key) => base.child(picture(s, &key, "img")),
        None => base.center().child(icon(&icons::NOTE, pt * 0.42).color(th::text3())),
    }
}

fn picture(s: &State, key: &str, name: &str) -> El {
    let px = s.art.pixels(key);
    el().key(format!("{name}-{key}")).absolute(0.0, 0.0).w_full().h_full().pixels_fit(&px, SurfaceFit::Fill).transition(Transition::none())
}

pub fn picture_url(s: &State, url: &str) -> El {
    picture(s, url, "pic")
}

/// Playing track cover with previous-cover hold while decoding and a zero-draw stage marker.
pub fn playing_cover(s: &State, t: &Track, pt: f32, radius: f32, track_key: &str) -> El {
    let base = el().w(pt).h(pt).radius(radius).clip_rounded(radius).fill(veil(0.06)).transition(Transition::none());
    let edge = el().absolute(0.0, 0.0).w_full().h_full().radius(radius).border(1.0, cover_border());
    let Some(key) = State::cover_key(t.cover_template()) else {
        return base.center().child(icon(&icons::NOTE, pt * 0.42).color(th::text3())).child(edge);
    };
    let mut c = base;
    if !s.art.ready(&key)
        && let Some(prev) = s.palette_key.as_deref()
        && prev != key
    {
        c = c.child(picture(s, prev, "img-prev"));
    }
    c.child(picture(s, &key, "img")).child(edge).child(el().key(track_key.to_owned()).absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).fx(fx::COVER_MARK))
}

pub fn roll(name: &str, id: &str, line: El) -> El {
    let enter = Transition::opacity().combined(Transition::offset(0.0, 9.0));
    let leave = Transition::opacity().combined(Transition::offset(0.0, -7.0));
    el().key(format!("{name}-{id}")).w_full().min_w(0.0).transition(Transition::asymmetric(enter, leave).springs(th::RISE)).child(line)
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

pub fn wave_card(s: &State, key: &str, caption: &str, name: &str, station: Station, w: Option<f32>, more: Option<Vec<WaveChoice>>) -> El {
    let playing = matches!(&s.player.source, Source::Wave { preset, .. } if station.is(preset)) && s.current().is_some();
    let on = playing && s.player.playing;
    let seed = (key.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(u32::from(b))) % 997) as f32 + 0.37;
    let hovered = !under() && s.wave_hover.as_deref() == Some(key);
    let [hue, sat, light] = s.palette.wave;
    let owner = key.to_owned();
    let wave_name = name.to_owned();
    let go = col()
        .key(format!("{key}-go"))
        .role("button")
        .label(caption)
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .pl(16.0)
        .pr(if more.is_some() { 12.0 } else { 20.0 })
        .gap(2.0)
        .justify_center()
        .cursor(Cursor::Pointer)
        .hov(veil(0.04))
        .press(veil(0.10))
        .transition(Transition::none())
        .on_press(move |s: &mut State| {
            if playing {
                s.toggle_pause();
            } else {
                s.start_station(&station, &wave_name);
            }
        })
        .child(label(caption.to_owned(), th::SMALL, th::text2()).truncate())
        .child(
            row()
                .gap(7.0)
                .items_center()
                .min_w(0.0)
                .child(icon(if on { &icons::PAUSE } else { &icons::PLAY }, 24.0).color(th::text()).ml(-3.0).mt(-3.0))
                .child(label(name.to_owned(), th::H3, th::text()).truncate().min_w(0.0)),
        );
    let mut c = row()
        .key(key.to_owned())
        .h(66.0)
        .items_center()
        .radius(14.0)
        .clip_rounded(14.0)
        .transition(Transition::none())
        .on_hover_change(move |s: &mut State, over: bool, _tx: &mut Tx| {
            if over {
                s.wave_hover = Some(owner.clone());
            } else if s.wave_hover.as_deref() == Some(owner.as_str()) {
                s.wave_hover = None;
            }
        })
        .child(blobs::ribbon(format!("{key}-ribbon"), seed, hue, sat, light, hovered, playing))
        .child(go);
    if let Some(choices) = more {
        let anchor = format!("{key}-more");
        let open = s.popover_open && matches!(&s.popover, Some(Popover::Waves { anchor: a, .. }) if *a == anchor);
        let a2 = anchor.clone();
        c = c.child(el().w(1.0).h(28.0).fill(th::text().alpha(0.18))).child(
            row()
                .key(anchor)
                .role("button")
                .label(S::MoreWaves.get(s.lang))
                .help(S::MoreWaves.get(s.lang))
                .checked(open)
                .w(46.0)
                .h_full()
                .center()
                .cursor(Cursor::Pointer)
                .hov(veil(0.06))
                .press(veil(0.12))
                .transition(Transition::none())
                .on_press(move |s: &mut State| s.open_popover(Popover::Waves { choices: choices.clone(), anchor: a2.clone() }))
                .child(icon(&icons::CHEVRON_DOWN, 18.0).color(th::text()).rotate(if open { std::f32::consts::PI } else { 0.0 })),
        );
    }
    match w {
        Some(w) => c.w(w),
        None => c.min_w(200.0).max_w(320.0),
    }
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

pub fn wave_button(s: &State, key: &str, caption: &str, name: &str, station: Station, size: f32) -> El {
    let playing = matches!(&s.player.source, Source::Wave { preset, .. } if station.is(preset)) && s.current().is_some();
    let on = playing && s.player.playing;
    let seed = (key.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(u32::from(b))) % 997) as f32 + 0.37;
    let hovered = !under() && s.wave_hover.as_deref() == Some(key);
    let [hue, sat, light] = s.palette.wave;
    let r = ctl_radius(size);
    let owner = key.to_owned();
    let wave_name = name.to_owned();
    let tip = if on { format!("{caption} · {}", S::Pause.get(s.lang)) } else { format!("{caption}: {name}") };
    let mut b = row()
        .key(key.to_owned())
        .role("button")
        .label(caption)
        .help(tip)
        .checked(playing)
        .w(size)
        .h(size)
        .radius(r)
        .clip_rounded(r)
        .center()
        .cursor(Cursor::Pointer)
        .press_scale(th::PRESS)
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
        .child(blobs::ribbon(format!("{key}-ribbon"), seed, hue, sat, light, hovered, playing));
    b = b.child(if on { icon(&icons::PAUSE, (size * 0.38).round()).color(th::text()) } else { wave_glyph((size * 0.60).round(), th::text()) });
    let edge = if playing { th::text().alpha(0.55) } else { veil(0.14) };
    b.child(el().absolute(0.0, 0.0).w_full().h_full().radius(r).border(1.0, edge))
}

fn avatar(s: &State, a: &Artist, size: f32) -> El {
    if a.cover_template().is_some() {
        return cover(s, a.cover_template(), size, size * 0.5);
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
