//! The pieces every page is made of: type, buttons, tabs, covers, illustrations, track rows,
//! sliders, and the slots the compositor paints (contour lines, liquid glass, progressive blur).
//!
//! The window is laid out the same way on every page, so nothing jumps: a rail down the left side
//! (the app's mark, the places, the profile), the page to its right, and the player bar along the
//! bottom of the page, always there (with nothing playing it offers My Wave).

use rune::{Align, Color, Cursor, Drag, El, Icon, Phase, PopupAnchor, ScrollSpec, SurfaceFit, TextCase, TextStyle, Transition, Tx, col, el, icon, row, text};
use ym_api::Track;

use super::icons;
use super::shapes::{self, Ill};
use crate::app::{DownloadState, Load, Source, State};
use crate::fx::{self, Glass};
use crate::i18n::S;
use crate::theme::{self as th, Pal, veil};

/// The title bar's height; the page scrolls under it. It holds the way back and the window controls.
/// The round buttons of the chrome (the profile, back) sit in it as far from its top as the
/// profile is from the window's left edge: `(TITLE_H - CHROME) / 2 == (RAIL_W - CHROME) / 2`.
pub const TITLE_H: f32 = 60.0;
/// The rail down the left side of the window.
pub const RAIL_W: f32 = 60.0;
/// The round buttons of the window's chrome: the profile, back, out of the player.
pub const CHROME: f32 = 40.0;
/// How far a chrome button is from the window's top (and the profile from its left).
pub const CHROME_INSET: f32 = (TITLE_H - CHROME) * 0.5;
/// The player bar: its height and the room around it.
pub const BAR_H: f32 = 72.0;
pub const BAR_GAP: f32 = 14.0;
/// Room kept under a page so its last row can scroll clear of the bar.
pub const DOCK_ZONE: f32 = BAR_H + BAR_GAP + 48.0;

thread_local! {
    /// Whether the player bar is on screen in the view being built (it is only while a track is
    /// loaded), so a page keeps room under its last row for it only then.
    static BAR_ON: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Called once per view, before the page is built.
pub fn set_bar_on(on: bool) {
    BAR_ON.with(|c| c.set(on));
}

/// The room under a page's last row.
pub fn foot_room() -> f32 {
    if BAR_ON.with(std::cell::Cell::get) { DOCK_ZONE } else { 56.0 }
}
/// A page's side padding and widest column.
pub const PAGE_PAD: f32 = 36.0;
pub const PAGE_MAX: f32 = 1240.0;
/// The one size of an icon button's hit area, and of its glyph.
pub const BTN: f32 = 40.0;
pub const GLYPH: f32 = 22.0;

pub fn label(s: impl Into<String>, style: TextStyle, color: Color) -> El {
    text(s).font(style).color(color)
}

/// Small mono capitals: section names, kinds, counts.
pub fn caps(s: impl Into<String>, color: Color) -> El {
    text(s).font(th::LABEL).color(color).text_case(TextCase::Uppercase)
}

// ---- effect slots ------------------------------------------------------------------------------------

/// Where the compositor copies the frame so far, once, for every blur after it (the glass and the
/// veils). It goes after the page and before the first of them, and it draws nothing; it has no fade
/// of its own, since a slot inside a fading layer is skipped.
pub fn snapshot() -> El {
    el().key("snapshot").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).effect(fx::SNAP).effect_backdrop(true)
}

/// A pane of liquid glass: an empty box the compositor paints over whatever is behind it (the
/// frame as `snapshot` saw it, so it belongs after the page and not inside it).
///
/// It has no enter or exit fade of its own: every element fades in over 0.2 s by default, and a
/// slot inside a fading layer is skipped by the compositor, so the glass (and its blur) would only
/// show once the fade had settled. Whatever holds a pane has to stay out of a fade as well; the
/// pane is faded through its own strength (`effect_data`: the first number is how much of it is hidden).
pub fn glass(kind: Glass, radius: f32) -> El {
    el().absolute(0.0, 0.0).w_full().h_full().radius(radius).effect(fx::glass(kind)).transition(Transition::none())
}

/// A progressive blur over the top `h` points of the window.
pub fn veil_top(h: f32) -> El {
    el().key("veil-top").absolute(0.0, 0.0).w_full().h(h).transition(Transition::none()).effect(fx::VEIL).effect_data([h, 0.0, 20.0, 0.38])
}

/// A progressive blur over the bottom `h` points of the window.
pub fn veil_bottom(h: f32) -> El {
    el().key("veil-bottom").pin(Some(0.0), None, Some(0.0), Some(0.0)).h(h).transition(Transition::none()).effect(fx::VEIL).effect_data([0.0, h, 20.0, 0.38])
}

// ---- controls ----------------------------------------------------------------------------------------

/// A round icon button. `size` is the hit area, `glyph` the icon.
pub fn ibtn(key: impl Into<String>, ic: &'static Icon, glyph: f32, size: f32, color: Color, name: &str) -> El {
    row()
        .key(key)
        .role("button")
        .label(name)
        .help(name)
        .w(size)
        .h(size)
        .radius(size * 0.5)
        .center()
        .cursor(Cursor::Pointer)
        .hover(veil(0.08))
        .press(veil(0.14))
        .press_scale(th::PRESS)
        .child(icon(ic, glyph).color(color))
}

/// The play / pause button: the bare glyph, no disc. `size` is the hit area.
pub fn play_btn(s: &State, size: f32, glyph: f32) -> El {
    let g = if s.player.loading {
        icon(&icons::SPINNER, glyph * 0.8).color(th::text()).looping(rune::LoopSpec::rotation(0.9))
    } else {
        icon(if s.player.playing { &icons::PAUSE } else { &icons::PLAY }, glyph).color(th::text())
    };
    row()
        .key(format!("play-{}", size as u32))
        .role("button")
        .label(if s.player.playing { S::Pause.get(s.lang) } else { S::Play.get(s.lang) })
        .help(if s.player.playing { S::Pause.get(s.lang) } else { S::Play.get(s.lang) })
        .w(size)
        .h(size)
        .radius(size * 0.5)
        .center()
        .cursor(Cursor::Pointer)
        .hover(veil(0.08))
        .press(veil(0.14))
        .press_scale(th::PRESS)
        .on_press(|s: &mut State| s.toggle_pause())
        .child(g)
}

// ---- buttons -----------------------------------------------------------------------------------------
//
// One family, two weights, one height. Every button is `BTN_H` tall (or square), drawn as
// squircles (`surface_sq`), quiet and flat, like the fields:
//
// - solid: the ink as a filled block with a faint shadow (one per screen: the main action);
// - soft: a tonal block, the ink at a few percent inside a hairline, for the actions beside it
//   (Shuffle, Retry, the demo);
// - round: either of the two as a square holding a glyph (play beside a title, "...", close).
//
// Hover lifts the surface a step, a press darkens it and gives a little (`th::PRESS`).

/// The height of every button with words in it, and its corners.
pub const BTN_H: f32 = 40.0;
/// A text field's corners (a field is a soft button that takes words).
pub const FIELD_R: f32 = 10.0;

/// The surface of a text field: the soft button's, brighter at its edge while it has the caret.
pub fn field_surface(base: El) -> El {
    base.radius(FIELD_R).fill(veil(0.05)).border(1.0, veil(0.11)).hover(veil(0.03))
}

/// How a button carries itself.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    Solid,
    Soft,
}

/// A squircle plate `w` by `h` behind a button: its fill and, when given, a hairline edge.
///
/// Drawn by RUNE itself with its continuous corner curve (which eases into the sides over about
/// 1.5 times its radius), not as a vector shape.
pub fn squircle_plate(w: f32, h: f32, r: f32, fill: Color, edge: Option<Color>) -> El {
    let p = el().absolute(0.0, 0.0).w(w).h(h).radius(sq_radius(r)).continuous_corners().fill(fill);
    match edge {
        Some(e) => p.border(1.0, e),
        None => p,
    }
}

/// [`squircle_plate`] that fills whatever it sits in, so a button given another width (`w_full`
/// in a card) is painted all the way across, not at the width its words measured.
fn squircle_fill(r: f32, fill: Color, edge: Option<Color>) -> El {
    let p = el().absolute(0.0, 0.0).w_full().h_full().radius(sq_radius(r)).continuous_corners().fill(fill);
    match edge {
        Some(e) => p.border(1.0, e),
        None => p,
    }
}

/// The radius that, with continuous corners, draws a squircle about `r` across at each corner.
pub fn sq_radius(r: f32) -> f32 {
    (r * 0.66).round()
}

/// The corner of a squircle button `h` tall: rounder than a circular corner would be, as the
/// superellipse draws it smaller at the same size.
pub fn squircle_r(h: f32) -> f32 {
    (h * 0.46).round()
}

/// The surface of a button of `weight`, `w` by `h`, as a squircle: the plate, a lighter one that
/// comes up under the pointer, and the shadow of the solid kind.
fn surface_sq(base: El, weight: Weight, w: f32, h: f32) -> El {
    let r = squircle_r(h);
    let b = base.w(w).h(h).radius(r * 0.62).cursor(Cursor::Pointer).press_scale(th::PRESS).hover_group();
    let (fill, edge, lift) = match weight {
        Weight::Solid => (th::solid(), None, th::on_solid().alpha(0.10)),
        Weight::Soft => (veil(0.06), Some(veil(0.10)), veil(0.06)),
    };
    let b = if weight == Weight::Solid { b.shadow(2.0, 8.0, th::rgba(0x000000, if th::light() { 0.10 } else { 0.30 })) } else { b };
    b.child(squircle_fill(r, fill, edge)).child(squircle_fill(r, lift, None).group_opacity(0.0, 1.0))
}

/// A fully rounded surface for primary and secondary actions that carry text.
fn surface_pill(base: El, weight: Weight, w: f32, h: f32) -> El {
    let radius = h * 0.5;
    let b = base.w(w).h(h).radius(radius).clip_rounded(radius).cursor(Cursor::Pointer).press_scale(th::PRESS).hover_group();
    let (fill, edge, lift) = match weight {
        Weight::Solid => (th::solid(), None, th::on_solid().alpha(0.10)),
        Weight::Soft => (veil(0.06), Some(veil(0.10)), veil(0.06)),
    };
    let b = if weight == Weight::Solid { b.shadow(2.0, 8.0, th::rgba(0x000000, if th::light() { 0.10 } else { 0.30 })) } else { b };
    let plate = el().absolute(0.0, 0.0).w_full().h_full().radius(radius).fill(fill);
    let edge_plate = edge.map(|color| el().absolute(0.0, 0.0).w_full().h_full().radius(radius).border(1.0, color));
    b.child(plate).children(edge_plate).child(el().absolute(0.0, 0.0).w_full().h_full().radius(radius).fill(lift).group_opacity(0.0, 1.0))
}

/// The colour of words and glyphs on a button of `weight`.
fn ink_on(weight: Weight) -> Color {
    match weight {
        Weight::Solid => th::on_solid(),
        Weight::Soft => th::text(),
    }
}

/// A pill with words (and a glyph before them).
pub fn button(key: impl Into<String>, weight: Weight, name: &str, ic: Option<&'static Icon>) -> El {
    let ink = ink_on(weight);
    let tw = rune::text::measure(name, &[], th::BTN, None).0.ceil();
    let w = if ic.is_some() { 14.0 + 15.0 + 8.0 + 1.0 + tw + 18.0 } else { 18.0 + tw + 18.0 };
    let mut r = surface_pill(row().key(key).role("button").label(name).gap(8.0).center(), weight, w + 2.0, BTN_H);
    r = if ic.is_some() { r.pl(14.0).pr(18.0) } else { r.px(18.0) };
    if let Some(i) = ic {
        // Nudged 1pt right: the play triangle reads off-centre against the words otherwise.
        r = r.child(icon(i, 15.0).color(ink).ml(1.0));
    }
    r.child(text(name.to_owned()).font(th::BTN).color(ink))
}

/// The main button: the solid pill.
pub fn btn_primary(key: impl Into<String>, name: &str, ic: Option<&'static Icon>) -> El {
    button(key, Weight::Solid, name, ic)
}

/// The button beside the main one: the soft pill.
pub fn btn_soft(key: impl Into<String>, name: &str, ic: Option<&'static Icon>) -> El {
    button(key, Weight::Soft, name, ic)
}

/// A small soft button, 32 tall, for actions inside a settings card (sign out, reset, clear).
pub fn btn_small(key: impl Into<String>, name: &str, ic: Option<&'static Icon>) -> El {
    let tw = rune::text::measure(name, &[], th::BODY_M, None).0.ceil();
    let w = if ic.is_some() { 10.0 + 15.0 + 7.0 + tw + 12.0 } else { 12.0 + tw + 12.0 };
    let mut r = surface_sq(row().key(key).role("button").label(name).gap(7.0).center(), Weight::Soft, w + 2.0, 32.0);
    r = if ic.is_some() { r.pl(10.0).pr(12.0) } else { r.px(12.0) };
    if let Some(i) = ic {
        r = r.child(icon(i, 15.0).color(th::text()));
    }
    r.child(text(name.to_owned()).font(th::BODY_M).color(th::text()))
}

/// A square `size` across holding a glyph `glyph` big, its corners like a button's.
pub fn round(key: impl Into<String>, weight: Weight, ic: &'static Icon, size: f32, glyph: f32, name: &str) -> El {
    surface_sq(row().key(key).role("button").label(name).help(name).center(), weight, size, size).child(icon(ic, glyph).color(ink_on(weight)))
}

/// A fully rounded icon button for page headers. Its width may be the button height for a circular
/// control or a little wider when a compact label is added beside the icon.
pub fn pill_icon(key: impl Into<String>, weight: Weight, ic: &'static Icon, width: f32, glyph: f32, name: &str) -> El {
    let radius = BTN_H * 0.5;
    let (fill, edge, ink) = match weight {
        Weight::Solid => (th::solid(), None, th::on_solid()),
        Weight::Soft => (veil(0.06), Some(veil(0.10)), th::text()),
    };
    let mut b = row()
        .key(key)
        .role("button")
        .label(name)
        .help(name)
        .w(width)
        .h(BTN_H)
        .radius(radius)
        .clip_rounded(radius)
        .center()
        .cursor(Cursor::Pointer)
        .press_scale(th::PRESS)
        .fill(fill)
        .hover(veil(if weight == Weight::Solid { 0.08 } else { 0.06 }))
        .press(veil(if weight == Weight::Solid { 0.14 } else { 0.10 }))
        .child(icon(ic, glyph).color(ink));
    if let Some(edge) = edge {
        b = b.border(1.0, edge);
    }
    b
}

/// A quiet link that leads further ("All", "Show all"): words and a small chevron, brighter under
/// the pointer.
pub fn more_link(key: impl Into<String>, name: &str) -> El {
    row()
        .key(key)
        .role("link")
        .label(name)
        .h(30.0)
        .pl(12.0)
        .pr(6.0)
        .gap(2.0)
        .radius(15.0)
        .items_center()
        .cursor(Cursor::Pointer)
        .hover(veil(0.06))
        .press(veil(0.10))
        .hover_group()
        .child(label(name.to_owned(), th::BODY_M, th::text2()).group_color(th::text()))
        .child(icon(&icons::CHEVRON_RIGHT, 16.0).color(th::text3()).group_color(th::text2()))
}

/// The Plus badge: a small pill in the four colours of the adaptive accent, a sparkle and the word
/// in white, with a soft light along its top edge.
pub fn plus_badge(lang: crate::i18n::Lang) -> El {
    row()
        .key("plus-badge")
        .h(20.0)
        .pl(6.0)
        .pr(8.0)
        .gap(4.0)
        .radius(10.0)
        .items_center()
        .linear_gradient(0.35, [(0.0, th::rgb(0x8a6bff)), (0.38, th::rgb(0x4cb8ff)), (0.70, th::rgb(0x27d6a8)), (1.0, th::rgb(0xff5c9a))])
        .border(1.0, th::rgba(0xffffff, 0.22))
        .child(icon(&icons::SPARKLE, 12.0).color(th::rgb(0xffffff)))
        .child(caps(crate::i18n::S::Plus.get(lang), th::rgb(0xffffff)))
}

/// What a tab of chips shows, keyed by the tab: the one that is left fades out quickly, and the new
/// one rises in once it has mostly gone (two lists fading across each other read as a mess).
pub fn tab_body(key: impl Into<String>, body: El) -> El {
    let enter = Transition::opacity().combined(Transition::offset(0.0, 10.0));
    let springs = rune::SpringPair::new(rune::Spring::new(0.26, 0.0), rune::Spring::new(0.07, 0.0));
    el().key(key).w_full().transition(Transition::asymmetric(enter, Transition::opacity()).springs(springs)).enter_delay(0.07).child(body)
}

/// One tab of a [`tab_track`]: its name and, when known, how many there are; the one picked is
/// the solid pill.
pub fn tab_pill(key: impl Into<String>, name: &str, count: Option<usize>, on: bool) -> El {
    let ink = if on { th::on_solid() } else { th::text2() };
    let mut r = row()
        .key(key)
        .role("tab")
        .label(name)
        .checked(on)
        .h(34.0)
        .px(14.0)
        .gap(7.0)
        .radius(10.0)
        .items_center()
        .cursor(Cursor::Pointer)
        .press_scale(th::PRESS)
        .fill(if on { th::solid() } else { Color::TRANSPARENT })
        .hover(if on { th::on_solid().alpha(0.06) } else { veil(0.06) })
        .press(if on { th::on_solid().alpha(0.12) } else { veil(0.10) })
        .child(label(name.to_owned(), th::BODY_M, ink));
    if let Some(n) = count {
        r = r.child(label(n.to_string(), th::SMALL, ink.alpha(0.55)));
    }
    r
}

/// Tabs side by side on one soft track.
pub fn tab_track(key: impl Into<String>, tabs: Vec<El>) -> El {
    row().key(key).h(42.0).padding(4.0).gap(2.0).radius(13.0).items_center().fill(veil(0.045)).border(1.0, veil(0.07)).children(tabs)
}

// ---- pictures ----------------------------------------------------------------------------------------

/// A cover picture. `pt` is its edge in points; the sharpness requested follows the size.
pub fn cover(s: &State, template: Option<&str>, pt: f32, radius: f32) -> El {
    let base = el().w(pt).h(pt).radius(radius).clip_rounded(radius).fill(veil(0.06));
    let edge = el().absolute(0.0, 0.0).w_full().h_full().radius(radius).border(1.0, th::hairline());
    match State::cover_key(template, cover_px(pt)) {
        Some(key) => base.child(picture(s, &key, "img")).child(edge),
        None => base.center().child(icon(&icons::NOTE, pt * 0.42).color(th::text3())).child(edge),
    }
}

/// [`cover`] asked for at `px` pixels (a cover shown large).
pub fn cover_px_at(s: &State, template: Option<&str>, pt: f32, radius: f32, px: u32) -> El {
    let base = el().w(pt).h(pt).radius(radius).clip_rounded(radius).fill(veil(0.06));
    match State::cover_key(template, px) {
        // Until the large copy is in, the one the page already has stands in for it.
        Some(key) => {
            let small = State::cover_key(template, cover_px(232.0)).filter(|k| *k != key && s.art.ready(k));
            let mut b = base;
            if let Some(k) = small {
                b = b.child(picture(s, &k, "img-small"));
            }
            b.child(picture(s, &key, "img"))
        }
        None => base.center().child(icon(&icons::NOTE, pt * 0.42).color(th::text3())),
    }
}

/// The pixel size a cover of `pt` points is asked for at.
fn cover_px(pt: f32) -> u32 {
    ((pt * 2.0) as u32).clamp(100, 700)
}

/// A picture over its cover's placeholder, once it has landed. The one that lands while it is on
/// screen fades in; one that was already there when its view was built is simply shown.
/// A picture that replaces another crossfades with it (each picture is its own element, keyed
/// by what it shows).
fn picture(s: &State, key: &str, name: &str) -> El {
    let px = s.art.pixels(key);
    if !s.art.ready(key) {
        return el();
    }
    let enter = if s.art.fresh(key) { Transition::opacity() } else { Transition::none() };
    el().key(format!("{name}-{key}"))
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .pixels_fit(&px, SurfaceFit::Fill)
        .transition(Transition::asymmetric(enter, Transition::opacity()).springs(th::RISE))
}

/// A picture at a URL (one the art cache takes), filling its parent, once it has landed.
pub fn picture_url(s: &State, url: &str) -> El {
    picture(s, url, "pic")
}

/// The cover of the playing track, as the player and the bar show it. It never goes blank when
/// the track changes: the copy the colours are taken from (always decoded before the morph
/// starts) lies under the sharp one, and the morph (`fx::ARTMORPH`) is laid over both.
pub fn playing_cover(s: &State, t: &ym_api::Track, pt: f32, radius: f32, morph_key: &str, seed: f32) -> El {
    let template = t.cover_template();
    // No fade of its own: it flies between the bar and the player (`matched`), and a cover that
    // lingered while fading out would keep its id taken, so the flight would not happen.
    let base = el().w(pt).h(pt).radius(radius).clip_rounded(radius).fill(veil(0.06)).transition(Transition::none());
    let edge = el().absolute(0.0, 0.0).w_full().h_full().radius(radius).border(1.0, th::hairline());
    let (Some(under), Some(sharp)) = (State::cover_key(template, 400), State::cover_key(template, cover_px(pt))) else {
        return base.center().child(icon(&icons::NOTE, pt * 0.42).color(th::text3())).child(edge);
    };
    // Underneath: the copy the colours come from. Until the new track's is decoded that is still
    // the last track's (`palette_key` moves only once it is), so the cover holds the old picture
    // rather than going blank, and the new one crossfades in over it.
    let under = s.palette_key.clone().unwrap_or(under);
    let mut c = base.child(picture(s, &under, "img"));
    if sharp != under {
        c = c.child(picture(s, &sharp, "img"));
    }
    c.child(edge).child(
        el().key(morph_key.to_owned())
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .radius(radius)
            .transition(Transition::none())
            .effect(fx::ARTMORPH)
            .effect_data([seed, 0.0, 0.0, 0.0]),
    )
}

/// Words that roll when they change: the old ones lift away as they fade, the new ones come up
/// from just below. `id` is what is shown (the words themselves), so a line that stays the same
/// from one track to the next (the same artist) holds still; `name` keeps two lines apart.
pub fn roll(name: &str, id: &str, line: El) -> El {
    let enter = Transition::opacity().combined(Transition::offset(0.0, 9.0));
    let leave = Transition::opacity().combined(Transition::offset(0.0, -7.0));
    el().key(format!("{name}-{id}")).w_full().min_w(0.0).transition(Transition::asymmetric(enter, leave).springs(th::RISE)).child(line)
}

/// The playing track's title, up to `width` wide: a link to its release. Under the pointer it dims a
/// little and a ray of light runs across it from left to right, again and again: a narrow window
/// with soft edges slides over the words while a bright copy of them inside slides back by as
/// much, so the copy stays on the words.
pub fn track_title(s: &State, t: &Track, key: &'static str, style: TextStyle, width: f32) -> El {
    let name = t.full_title();
    let hover = s.title_hover.filter(|(k, _)| *k == key).map(|(_, since)| since);
    let album = t.albums.first().map(|a| a.id.clone());
    let base = if hover.is_some() { th::text().alpha(0.62) } else { th::text() };
    let mut r = el().key(key).max_w(width).min_w(0.0).child(label(name.clone(), style, base).truncate().max_w(width));
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
            s.go(crate::app::Page::Album(id.clone()));
        });
    if let Some(since) = hover.filter(|_| !s.settings.reduce_motion) {
        // The ray is `band` wide and runs past both ends, then rests a moment off the end.
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
        let lit = el()
            .absolute(0.0, 0.0)
            .w(band)
            .h_full()
            .looping(spec(-x0 / band, -x1 / band))
            .child(label(name, style, lit_color).truncate().w(width).absolute(0.0, 0.0));
        let sheen = el().key(format!("{key}-ray")).absolute(0.0, 0.0).w(band).h_full().clip_fade(band * 0.5, 0.0, band * 0.5, 0.0).looping(spec(x0 / band, x1 / band)).child(lit);
        r = r.child(sheen);
    }
    r
}

/// A wave seeded by one thing, as a card: a grey printed tile, what it is small on top ("My Wave
/// by track") and under it the thing itself after a play glyph (pause while that wave plays).
/// `w`: its width, or `None` to fit its words.
///
/// With `more`, a thin upright line and an arrow close its right side: the arrow lists those
/// waves (`Popover::Waves`), each a plain row.
pub fn wave_card(s: &State, key: &str, caption: &str, name: &str, station: crate::app::Station, w: Option<f32>, more: Option<Vec<crate::app::WaveChoice>>) -> El {
    let playing = matches!(&s.player.source, crate::app::Source::Wave { preset, .. } if station.is(preset)) && s.current().is_some();
    let on = playing && s.player.playing;
    let seed = (key.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32)) % 997) as f32 + 0.37;
    let hovered = s.wave_hover.as_deref() == Some(key);
    // The cover's own colour: its hue, and how colourful and how light it really is.
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
        .hover(veil(0.04))
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
            // The glyph is half as large again as the words' line and sits a little above its
            // middle, level with the capitals rather than the line box.
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
        .child(super::blobs::ribbon(format!("{key}-ribbon"), seed, hue, sat, light, hovered, playing))
        .child(go);
    if let Some(choices) = more {
        let anchor = format!("{key}-more");
        let open = s.popover_open && matches!(&s.popover, Some(crate::app::Popover::Waves { anchor: a, .. }) if *a == anchor);
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
                .hover(veil(0.06))
                .press(veil(0.12))
                .transition(Transition::none())
                .on_press(move |s: &mut State| s.open_popover(crate::app::Popover::Waves { choices: choices.clone(), anchor: a2.clone() }))
                .child(icon(&icons::CHEVRON_DOWN, 18.0).color(th::text()).rotate(if open { std::f32::consts::PI } else { 0.0 })),
        );
    }
    c = match w {
        Some(w) => c.w(w),
        None => c.min_w(200.0).max_w(320.0),
    };
    c
}

/// My Wave's own glyph, `side` points square: a wave rising from the left, breaking over its crest
/// and curling into itself, one stroke, with the water running on under it.
pub fn wave_glyph(side: f32, color: Color) -> El {
    use std::f32::consts::{PI, TAU};
    let p = |x: f32, y: f32| [x * side, y * side];
    let mut a = Vec::with_capacity(60);
    // The swell: a quadratic curve from low on the left up to the crest's left side.
    let (s0, s1, s2) = ([0.08f32, 0.80f32], [0.30f32, 0.80f32], [0.30f32, 0.46f32]);
    for i in 0..=12 {
        let t = i as f32 / 12.0;
        let u = 1.0 - t;
        a.push(p(u * u * s0[0] + 2.0 * u * t * s1[0] + t * t * s2[0], u * u * s0[1] + 2.0 * u * t * s1[1] + t * t * s2[1]));
    }
    // Over the top and round into the curl, tighter as it goes.
    let (cx, cy) = (0.57f32, 0.46f32);
    let n = 44;
    for i in 1..=n {
        let u = i as f32 / n as f32;
        let ang = PI + TAU * 1.12 * u;
        let r = 0.27 * (1.0 - 0.72 * u);
        a.push(p(cx + r * ang.cos(), cy + r * ang.sin()));
    }
    let water = vec![p(0.50, 0.84), p(0.92, 0.84)];
    let w = (side * 0.085).max(1.6);
    rune::canvas(rune::Shape::new().stroke_with(a, false, w, rune::LineCap::Round, color).stroke_with(water, false, w, rune::LineCap::Round, color).view_box(side, side))
        .w(side)
        .h(side)
        .transition(Transition::none())
}

/// My Wave by one thing as a fully rounded button `size` across: the cover's colour in printed ribbons
/// with the wave's glyph on them (a pause while that wave plays, then it also wears a ring). The
/// tooltip says which wave it is.
pub fn wave_button(s: &State, key: &str, caption: &str, name: &str, station: crate::app::Station, size: f32) -> El {
    let playing = matches!(&s.player.source, crate::app::Source::Wave { preset, .. } if station.is(preset)) && s.current().is_some();
    let on = playing && s.player.playing;
    let seed = (key.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32)) % 997) as f32 + 0.37;
    let hovered = s.wave_hover.as_deref() == Some(key);
    let [hue, sat, light] = s.palette.wave;
    let r = size * 0.5;
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
        .child(super::blobs::ribbon(format!("{key}-ribbon"), seed, hue, sat, light, hovered, playing));
    b = b.child(if on { icon(&icons::PAUSE, (size * 0.38).round()).color(th::text()) } else { wave_glyph((size * 0.64).round(), th::text()) });
    if playing {
        b = b.child(el().absolute(0.0, 0.0).w_full().h_full().radius(r).border(1.0, th::text().alpha(0.55)));
    }
    b
}

/// A placeholder block for content that is still loading: a quiet grey, and now and then a soft
/// ray of light passing over it from the top left to the bottom right. The ray is one for the
/// whole window (`fs_pattern`'s skeleton mode), so every block it crosses lights up in turn.
pub fn skeleton(w: f32, h: f32, radius: f32) -> El {
    el().w(w)
        .h(h)
        .radius(radius)
        .fill(veil(0.07))
        .child(el().effect(fx::PATTERN).effect_data([-1.0, 0.0, 0.0, 0.0]).absolute(0.0, 0.0).w_full().h_full().radius(radius).transition(Transition::none()))
}

/// An empty or waiting state: its illustration drawing itself on, and a line under it.
pub fn empty(p: &Pal, art: Ill, msg: &str) -> El {
    col().w_full().py(40.0).gap(20.0).items_center().child(shapes::ill(art, 176.0, p.accent, 0.08)).child(label(msg.to_owned(), th::LEAD, th::text2()).align(Align::Center))
}

// ---- page furniture ----------------------------------------------------------------------------------

/// The title of a page (Search, Collection) with a quiet line under it.
pub fn page_title(title: &str, lead: &str) -> El {
    let mut c = col().gap(6.0).child(label(title.to_owned(), th::TITLE, th::text()).max_lines(2));
    if !lead.is_empty() {
        c = c.child(label(lead.to_owned(), th::LEAD, th::text2()));
    }
    c
}

/// A section's head: its name in sentence case, and on the right whatever leads further (a
/// [`more_link`]) when there is one.
pub fn heading(key: impl Into<String>, title: &str, more: Option<El>) -> El {
    let mut r = row().key(key).w_full().h(32.0).items_center().gap(12.0).child(label(title.to_owned(), th::SECTION, th::text()).truncate().min_w(0.0)).child(el().grow(1.0));
    if let Some(m) = more {
        r = r.child(m);
    }
    r
}

/// [`heading`] over a list of track rows: inset like the rows' contents, so the name starts where
/// their cover does.
pub fn list_heading(key: impl Into<String>, title: &str, more: Option<El>) -> El {
    heading(key, title, more).pl(12.0).pr(4.0)
}

/// A scrolling page that runs under the title bar and the bar, beside the rail, centred in its
/// column. Its content
/// rises a little as it fades up; the page it replaces fades out at the same time.
pub fn scroll_page(key: impl Into<String>, body: El) -> El {
    page_shell(key, body.transition(Transition::opacity().combined(Transition::offset(0.0, 12.0)).springs(th::RISE)))
}

/// [`scroll_page`] for a page that holds compositor slots (the flowing colour, the spheres): a
/// slot inside a fading group is skipped until the fade is over, so the body does not fade, and the
/// blocks of the page that hold no slot come in by [`rise`] on their own.
pub fn scroll_page_live(key: impl Into<String>, body: El) -> El {
    page_shell(key, body.transition(Transition::none()))
}

fn page_shell(key: impl Into<String>, body: El) -> El {
    let key: String = key.into();
    let wheel_key = key.clone();
    let watch_key = key.clone();
    crate::app::PAGE_KEY.with_borrow_mut(|k| k.clone_from(&key));
    col()
        .key(key)
        .pin(Some(RAIL_W), Some(0.0), Some(0.0), Some(0.0))
        .scroll(ScrollSpec { stick_bottom: false })
        // A notched mouse wheel scrolls the page on its own glide (`WheelGlide`); a trackpad's
        // precise deltas, and Ctrl (zoom), go to RUNE as usual.
        .on_wheel(move |s: &mut State, e: rune::WheelEvent, _tx: &mut Tx| {
            if e.precise || e.mods.ctrl || e.mods.cmd || e.dy == 0.0 || e.dx.abs() > e.dy.abs() {
                return false;
            }
            s.glide.wheel(&wheel_key, e.dy);
            true
        })
        .on_scroll_offset(move |p: rune::ScrollPos| crate::app::PAGE_SCROLL.with_borrow_mut(|m| {
            m.insert(watch_key.clone(), [p.offset[1], p.max[1]]);
        }))
        .transition(Transition::opacity().springs(th::LEAVE))
        .child(
        row().w_full().justify_center().transition(Transition::none()).child(body.w_full().max_w(PAGE_MAX).px(PAGE_PAD).pt(TITLE_H + 8.0).pb(foot_room()).gap(44.0)),
    )
}

/// Lets a notched wheel turned over `e` (a row that scrolls sideways, which RUNE treats as a scroll
/// view of its own and so never hands the wheel to the page) glide the page on screen instead.
/// Shift, a sideways wheel and a trackpad's precise deltas still scroll `e` itself.
pub fn wheel_to_page(e: El) -> El {
    e.on_wheel(|s: &mut State, ev: rune::WheelEvent, _tx: &mut Tx| {
        if ev.precise || ev.mods.ctrl || ev.mods.cmd || ev.mods.shift || ev.dy == 0.0 || ev.dx.abs() > ev.dy.abs() {
            return false;
        }
        s.glide.wheel_page(ev.dy)
    })
}

/// How a block comes in when its page opens: it rises a little as it fades up, after `i` short
/// beats (so a page's blocks arrive one after another, not all at once).
pub fn rise(e: El, i: usize) -> El {
    e.transition(Transition::opacity().combined(Transition::offset(0.0, 14.0)).springs(th::RISE)).enter_delay(0.04 * i.min(6) as f32)
}

// ---- lists -------------------------------------------------------------------------------------------

/// Which list of tracks a row belongs to, so a press plays the right queue without every row
/// carrying a copy of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListSrc {
    Liked,
    Detail,
    Search,
    Queue,
    History,
}

impl State {
    pub fn list(&self, src: ListSrc) -> Vec<Track> {
        match src {
            ListSrc::Liked => {
                let all = self.library.tracks.get().cloned().unwrap_or_default();
                if self.library.filter.text.trim().is_empty() {
                    all
                } else {
                    self.library.shown.borrow().iter().filter_map(|&i| all.get(i).cloned()).collect()
                }
            }
            ListSrc::Detail => self.detail.tracks.clone(),
            ListSrc::Search => self.search_view().map(|r| r.tracks.results.clone()).unwrap_or_default(),
            ListSrc::Queue => self.player.queue.clone(),
            ListSrc::History => self.listening_history.clone(),
        }
    }

    pub fn play_from_list(&mut self, src: ListSrc, id: &str) {
        if src == ListSrc::Queue {
            if let Some(i) = self.player.queue.iter().position(|t| t.id == id) {
                self.play_index(i);
            }
            return;
        }
        if src == ListSrc::Search {
            self.remember_search();
        }
        let list = self.list(src);
        if let Some(i) = list.iter().position(|t| t.id == id) {
            self.play_queue(list, i, Source::Manual);
            self.playing_from = self.list_from(src);
        }
    }

    /// What a list is, as the player names where its tracks came from.
    pub fn list_from(&self, src: ListSrc) -> Option<crate::app::PlayingFrom> {
        use crate::app::{FromName, Page};
        let (kind, name, page) = match src {
            ListSrc::Liked => (S::Liked, FromName::Word(S::LikedTracks), Some(Page::Library)),
            ListSrc::Detail => {
                let page = self.detail.page.clone()?;
                let kind = match page {
                    Page::Album(_) => S::Album,
                    Page::Playlist(..) => S::Playlist,
                    _ => S::Artist,
                };
                (kind, FromName::Text(self.detail.title.clone()), Some(page))
            }
            ListSrc::Search => (S::Search, FromName::Text(self.search.query.clone()), Some(Page::Search)),
            ListSrc::History => (S::Search, FromName::Word(S::ListeningHistory), Some(Page::Search)),
            ListSrc::Queue => return self.playing_from.clone(),
        };
        Some(crate::app::PlayingFrom { wave: false, kind, name, page })
    }
}

/// An entry of a menu card's height, and its glyph's size.
pub const MENU_ROW: f32 = 34.0;
const MENU_GLYPH: f32 = 18.0;

/// An entry of a menu card: a small glyph and a name on a row that lights up under the pointer.
/// (Menus fade their entries themselves, so a row has no enter or exit of its own.)
pub fn menu_row(key: impl Into<String>, ic: &'static Icon, text: &str, on: bool) -> El {
    row()
        .key(key)
        .role("button")
        .label(text)
        .w_full()
        .h(MENU_ROW)
        .px(10.0)
        .gap(10.0)
        .items_center()
        .radius(8.0)
        .cursor(Cursor::Pointer)
        .transition(Transition::none())
        .fill(if on { veil(0.08) } else { Color::TRANSPARENT })
        .hover(veil(0.06))
        .press(veil(0.10))
        .child(icon(ic, MENU_GLYPH).color(if on { th::text() } else { th::text2() }))
        .child(label(text.to_owned(), th::BODY, th::text()))
}

/// The right-click / "more" menu of a track, opened from a point or from the element with a key.
/// `queue_index` is the track's slot when the row belongs to the queue (it adds "remove").
pub fn open_track_menu(s: &mut State, track: &Track, queue_index: Option<usize>, anchor: impl Into<PopupAnchor>) {
    s.open_track_menu(track.clone(), queue_index, anchor.into());
}

pub fn like_button(s: &State, track: &Track, size: f32) -> El {
    let on = s.is_liked(&track.id);
    let p = s.pal();
    let t = track.clone();
    let mut b = ibtn(
        format!("like-{}", track.id),
        if on { &icons::HEART_FILL } else { &icons::HEART },
        GLYPH,
        size,
        if on { p.accent } else { th::text2() },
        if on { S::Unlike.get(s.lang) } else { S::Like.get(s.lang) },
    )
    .checked(on)
    .on_press(move |s: &mut State| {
        let liking = !s.is_liked(&t.id);
        s.toggle_like(&t);
        if liking {
            s.like_burst = s.now;
            s.burst_track = t.id.clone();
            s.burst_on = true;
        }
    });
    if s.burst_on && s.burst_track == track.id {
        b = b.child(
            el().key("burst").effect(fx::SPARKLES).effect_data([s.like_burst as f32, 16.0, 7.0, 0.0]).effect_bleed(46.0).absolute(0.0, 0.0).w_full().h_full().radius(size * 0.5),
        );
    }
    b
}

pub const ROW_H: f32 = 60.0;

/// The playing track's marker: three bars that dance with the bass, the middle and the highs of
/// what plays, and settle into three still dots while it is paused. It is white, for sitting on
/// a cover.
pub fn marker(s: &State) -> El {
    el().key("playing-marker").w(22.0).h(22.0).transition(Transition::none()).effect(fx::MORPH).effect_data([if s.player.playing { 1.0 } else { 0.0 }, 1.0, 0.0, 0.0])
}

/// A list row's cover; the playing one is dimmed with the marker on it.
///
/// The cover is the same element whether or not its track plays. Building a different tree for the
/// playing row remounted the picture on every track change, and a remounted element fades in, so
/// the cover flashed bright, went blank and came back. Here the dim over it is always there and only
/// changes colour (which springs), and the marker, a compositor slot, comes without a fade of its own.
fn row_cover(s: &State, t: &Track, playing: bool) -> El {
    let dim = th::rgba(0x000000, if playing { 0.52 } else { 0.0 });
    let mut c = el()
        .key("rc")
        .w(44.0)
        .h(44.0)
        .transition(Transition::none())
        .child(cover(s, t.cover_template(), 44.0, 11.0))
        .child(el().key("rc-dim").absolute(0.0, 0.0).w_full().h_full().radius(11.0).transition(Transition::none()).fill(dim));
    if playing {
        c = c.child(row().key("rc-mark").absolute(0.0, 0.0).w_full().h_full().center().transition(Transition::none()).child(marker(s)));
    } else {
        c = c.child(
            row()
                .key("rc-play")
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .center()
                .group_opacity(0.0, 1.0)
                .child(icon(&icons::PLAY, 18.0).color(th::text())),
        );
    }
    c
}

fn download_action(s: &State, t: &Track) -> El {
    let state = s.downloads.get(&t.id).copied();
    let (ic, color, help) = match state {
        Some(DownloadState::Downloading) => (&icons::SPINNER, th::text2(), S::DownloadInProgress),
        Some(DownloadState::Done) => (&icons::DOWNLOAD, th::rgb(0x4bd27e), S::DeleteDownload),
        Some(DownloadState::Failed) => (&icons::DOWNLOAD, th::rgb(0xf18a8a), S::DownloadFailed),
        None => (&icons::DOWNLOAD, th::text3(), S::Download),
    };
    if matches!(state, Some(DownloadState::Downloading)) {
        return row()
            .key(format!("download-{}", t.id))
            .role("status")
            .label(help.get(s.lang))
            .help(help.get(s.lang))
            .w(BTN)
            .h(BTN)
            .radius(BTN * 0.5)
            .center()
            .child(icon(ic, 19.0).color(color).looping(rune::LoopSpec::rotation(0.9)));
    }
    let mut b = ibtn(format!("download-{}", t.id), ic, 19.0, BTN, color, help.get(s.lang));
    let track = t.clone();
    b = match state {
        Some(DownloadState::Done) => b.on_press(move |s: &mut State| s.ask_delete_download(track.clone())),
        Some(DownloadState::Downloading) => b,
        _ => b.on_press(move |s: &mut State| s.ask_download(vec![track.clone()])),
    };
    b
}

/// One row of a track list.
pub fn track_row(s: &State, t: &Track, n: usize, src: ListSrc) -> El {
    let playing = s.current().is_some_and(|c| c.id == t.id);
    let ok = t.is_available();
    let (id, id2, t2) = (t.id.clone(), t.id.clone(), t.clone());
    let qi = (src == ListSrc::Queue).then(|| n.saturating_sub(1));
    let ink = if ok { th::text() } else { th::text3() };
    // Each artist is a link of their own (their page), lighting up under the pointer.
    let mut meta = row().gap(8.0).items_center().min_w(0.0).child(artists_row(s, &t.artists, th::SMALL, false, &format!("row-{id}-ar")).shrink());
    if t.is_explicit() {
        meta = meta.child(row().px(5.0).h(16.0).radius(4.0).border(1.0, th::text3()).center().child(label("E", th::LABEL, th::text3())));
    }

    row()
        .key(format!("row-{id}"))
        .role("button")
        .label(t.full_title())
        .w_full()
        .h(ROW_H)
        .px(12.0)
        .gap(12.0)
        .items_center()
        .radius(16.0)
        .cursor(Cursor::Pointer)
        .fill(if playing { veil(0.06) } else { Color::TRANSPARENT })
        .hover(veil(0.05))
        .press(veil(0.09))
        .on_press(move |s: &mut State| s.play_from_list(src, &id))
        .on_secondary_press(move |s: &mut State, press, _tx: &mut Tx| open_track_menu(s, &t2, qi, press))
        .hover_group()
        .child(row_cover(s, t, playing))
        .child(col().grow(1.0).min_w(0.0).gap(2.0).child(label(t.full_title(), th::BODY_M, ink).truncate().min_w(0.0)).child(meta))
        // The heart comes up under the pointer; a liked track keeps it showing.
        .child(
            row()
                .key(format!("row-actions-{id2}"))
                .w(136.0)
                .h(BTN)
                .gap(4.0)
                .justify_end()
                .items_center()
                .child(download_action(s, t))
                .child({
                    let heart = like_button(s, t, BTN);
                    if s.is_liked(&t.id) { heart } else { heart.group_opacity(0.0, 1.0) }
                })
                .child({
            let t3 = t.clone();
            let time = label(crate::player::fmt_time(t.duration_ms.unwrap_or(0)), th::MONO, th::text3())
                .w_full()
                .h_full()
                .align(Align::Center)
                .group_opacity(1.0, 0.0);
            let more = ibtn(format!("more-{id2}"), &icons::MORE, GLYPH, BTN, th::text2(), S::More.get(s.lang))
                .absolute(0.0, 0.0)
                .group_opacity(0.0, 1.0)
                .on_press(move |s: &mut State| open_track_menu(s, &t3, qi, format!("more-{}", t3.id)));
            el()
                .key(format!("duration-{id2}"))
                .w(BTN)
                .h(BTN)
                .hover_group()
                .child(time)
                .child(more)
                }),
        )
}

/// A loading or failure stand-in for a whole list.
pub fn list_state<T>(l: &Load<T>, s: &State, rows: usize) -> Option<El> {
    match l {
        Load::Loading | Load::Idle => {
            let mut c = col().w_full().gap(8.0);
            for i in 0..rows {
                c = c.child(
                    row()
                        .key(format!("sk-{i}"))
                        .w_full()
                        .h(ROW_H)
                        .px(12.0)
                        .gap(14.0)
                        .items_center()
                        .child(skeleton(40.0, 40.0, 10.0))
                        .child(col().gap(8.0).child(skeleton(200.0 + (i % 3) as f32 * 40.0, 12.0, 6.0)).child(skeleton(120.0, 10.0, 5.0))),
                );
            }
            Some(c)
        }
        Load::Failed(_) => Some(
            col()
                .w_full()
                .py(32.0)
                .gap(20.0)
                .items_center()
                .child(shapes::ill(Ill::Unplugged, 176.0, s.pal().accent, 0.05))
                .child(label(S::Offline.get(s.lang), th::LEAD, th::text2()))
                .child(btn_soft("retry", S::Retry.get(s.lang), None).on_press(|s: &mut State| s.reload_content())),
        ),
        Load::Ready(_) => None,
    }
}

// ---- sliders -----------------------------------------------------------------------------------------

/// A horizontal slider: a hairline track, a live-bound fill. `fill` is the live rect of the fill;
/// `on` gets the fraction (0..1) and whether the drag has ended.
pub fn slider(key: impl Into<String>, fill: &rune::Live<[f32; 4]>, height: f32, color: Color, on: impl Fn(&mut State, f32, bool) + 'static) -> El {
    let key = key.into();
    let hit_h = height + 18.0;
    let drag = move |s: &mut State, d: &mut Drag, _tx: &mut Tx| {
        let f = if d.w > 0.0 { (d.x / d.w).clamp(0.0, 1.0) } else { 0.0 };
        on(s, f, d.phase == Phase::Up);
    };
    el().key(key.clone()).role("slider").w_full().h(hit_h).cursor(Cursor::Pointer).on_drag(drag).child(
        el().key(format!("{key}-track"))
            .absolute(0.0, (hit_h - height) * 0.5)
            .w_full()
            .h(height)
            .radius(height * 0.5)
            .fill(veil(0.16))
            .clip_rounded(height * 0.5)
            .child(el().h(height).radius(height * 0.5).fill(color).live_rect(fill)),
    )
}

// ---- artists ---------------------------------------------------------------------------------------

/// A round picture of an artist, `size` points across; their initial when there is no picture.
pub fn avatar(s: &State, a: &ym_api::Artist, size: f32) -> El {
    if a.cover_template().is_some() {
        return cover(s, a.cover_template(), size, size * 0.5);
    }
    let initial = a.name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    row().w(size).h(size).radius(size * 0.5).center().fill(veil(0.10)).child(label(initial, th::LABEL, th::text2()))
}

/// A track's artists constrained to a known line width. The row keeps as many names as fit,
/// then moves the rest behind one compact “more” control.
pub fn artist_line_width(s: &State, t: &Track, style: TextStyle, pictures: bool, key: &str, width: f32) -> El {
    artists_row_in_width(s, &t.artists, style, pictures, key, th::text2(), width).w_full()
}

/// `artists` as names that each open their page, in `style`.
pub fn artists_row(s: &State, artists: &[ym_api::Artist], style: TextStyle, pictures: bool, key: &str) -> El {
    artists_row_in(s, artists, style, pictures, key, th::text2())
}

/// [`artists_row`] with the names in `color` (they light up to the ink under the pointer).
pub fn artists_row_in(s: &State, artists: &[ym_api::Artist], style: TextStyle, pictures: bool, key: &str, color: Color) -> El {
    artists_row_in_width(s, artists, style, pictures, key, color, f32::INFINITY)
}

/// [`artists_row_in`] with a measured width. Three names get room in broad cards, while a tight
/// line shows one name and keeps the remaining count visible.
pub fn artists_row_in_width(s: &State, artists: &[ym_api::Artist], style: TextStyle, pictures: bool, key: &str, color: Color, width: f32) -> El {
    let n = artists.len();
    let shown = artist_count(s, artists, style, pictures, width);
    let mut r = row().min_w(0.0).items_center().gap(if pictures { 14.0 } else { 0.0 });
    for (i, a) in artists.iter().take(shown).enumerate() {
        let id = a.id.clone();
        let text = if !pictures && i + 1 < shown { format!("{}, ", a.name) } else { a.name.clone() };
        let mut item = row()
            .key(format!("{key}-a{i}"))
            .role("link")
            .label(a.name.as_str())
            .items_center()
            .gap(7.0)
            .shrink()
            .cursor(Cursor::Pointer)
            .hover_group()
            .on_press(move |s: &mut State| s.go(crate::app::Page::Artist(id.clone())));
        if pictures {
            item = item.child(avatar(s, a, 22.0));
        }
        r = r.child(item.child(label(text, style, color).group_color(th::text()).truncate().min_w(0.0)));
    }
    if n > shown {
        let remaining = n - shown;
        let more_text = more_artists_text(s, remaining, width);
        let more_key = format!("{key}-more");
        let anchor = more_key.clone();
        let artists = artists.to_vec();
        r = r.child(
            row()
                .key(more_key)
                .role("button")
                .label(s.lang.more_artists(remaining))
                .h(20.0)
                .px(10.0)
                .ml(if pictures { 0.0 } else { 8.0 })
                .radius(10.0)
                .items_center()
                .cursor(Cursor::Pointer)
                .fill(veil(0.08))
                .hover(veil(0.08))
                .press_scale(th::PRESS)
                .hover_group()
                .on_press(move |s: &mut State| s.open_popover(crate::app::Popover::Artists { artists: artists.clone(), anchor: anchor.clone() }))
                .child(label(more_text, th::SMALL, th::text().alpha(0.82)).group_color(th::text())),
        );
    }
    r
}

fn artist_count(s: &State, artists: &[ym_api::Artist], style: TextStyle, pictures: bool, width: f32) -> usize {
    let max = artists.len().min(3);
    if max == 0 || !width.is_finite() {
        return max;
    }
    for shown in (1..=max).rev() {
        let mut used = 0.0;
        for (i, artist) in artists.iter().take(shown).enumerate() {
            used += rune::text::measure(&artist.name, &[], style, None).0.ceil();
            if !pictures {
                used += 7.0;
                if i + 1 < shown {
                    used += 5.0;
                }
            } else {
                used += 29.0;
            }
        }
        if artists.len() > shown {
            used += rune::text::measure(&more_artists_text(s, artists.len() - shown, width), &[], th::SMALL, None).0.ceil() + 20.0;
            if !pictures {
                used += 8.0;
            }
        }
        if used <= width {
            return shown;
        }
    }
    1
}

fn more_artists_text(s: &State, count: usize, width: f32) -> String {
    if width.is_finite() && width < 260.0 { format!("+{count}") } else { s.lang.more_artists(count) }
}
