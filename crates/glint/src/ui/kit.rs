//! The pieces every page is made of: type, buttons, tabs, covers, illustrations, track rows,
//! sliders, and the slots the compositor paints (liquid glass, progressive blur).
//!
//! The window is laid out the same way on every page, so nothing jumps: a rail down the left side
//! (the app's mark, the places, the profile), the page to its right, and the player bar along the
//! bottom of the page, always there (with nothing playing it offers My Wave).

use crate::fx::Slot as _;
use rune::{Align, Color, Cursor, Drag, El, Icon, Phase, PopupAnchor, ScrollSpec, SurfaceFit, TextStyle, Transition, Tx, col, el, icon, row, text};
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
/// How tall the window's own minimize, maximize and close buttons are drawn on Windows (Rune
/// draws them at the height the titlebar is declared with; nothing else there reads it). Shorter
/// than the band, flush with the window's top-right corner as Windows draws its own, so they sit
/// as light as the rest of the band. Elsewhere the band's height, which centres macOS's buttons.
pub const CAPTION_H: f32 = if cfg!(target_os = "windows") { 44.0 } else { TITLE_H };
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
pub const PAGE_MAX: f32 = 960.0;
/// The one size of an icon button's hit area, and of its glyph.
pub const BTN: f32 = 40.0;
pub const GLYPH: f32 = 22.0;

// ---- hover under a surface ---------------------------------------------------------------------

thread_local! {
    static UNDER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Says whether what the view builds from here on lies under the surface the pointer is on (the
/// bar, the title bar, the volume card). RUNE lights an element's hover wherever the pointer is
/// inside it, whatever is drawn over it, so the rows under the bar lit up through it. Built under
/// a surface the pointer is on, an element looks the same hovered or not ([`Hover`]).
pub fn set_under(on: bool) {
    UNDER.with(|u| u.set(on));
}

/// Whether the element being built lies under the surface the pointer is on.
pub fn under() -> bool {
    UNDER.with(std::cell::Cell::get)
}

/// Builds `f` with `set_under(on)` in force. A virtual list's rows are built after the view
/// returns, so they take the value that held where the list stands (read it with [`under`]).
pub fn built_under<T>(on: bool, f: impl FnOnce() -> T) -> T {
    let was = under();
    set_under(on);
    let out = f();
    set_under(was);
    out
}

/// Hover washes and hover-group styles that show nothing under the surface the pointer is on.
/// The hover itself stays (only invisible), so RUNE keeps following it and nothing is left lit
/// when the surface lets go.
pub trait Hover {
    fn hov(self, c: Color) -> Self;
    fn hov_group(self) -> Self;
    /// `group_opacity`.
    fn gop(self, rest: f32, hovered: f32) -> Self;
    /// `group_color`.
    fn gcol(self, c: Color) -> Self;
}

impl Hover for El {
    fn hov(self, c: Color) -> El {
        // A wash RUNE still tracks, too faint to see.
        self.hover(if under() { c.alpha(0.001) } else { c })
    }

    fn hov_group(self) -> El {
        self.hover_group()
    }

    fn gop(self, rest: f32, hovered: f32) -> El {
        self.group_opacity(rest, if under() { rest } else { hovered })
    }

    fn gcol(self, c: Color) -> El {
        if under() { self } else { self.group_color(c) }
    }
}

pub fn label(s: impl Into<String>, style: TextStyle, color: Color) -> El {
    text(s).font(style).color(color)
}

/// A small quiet label: section names, kinds, counts (in sentence case, as the rest of the UI).
pub fn caps(s: impl Into<String>, color: Color) -> El {
    let s: String = s.into();
    let mut c = s.chars();
    let s = match c.next() {
        Some(f) => f.to_uppercase().chain(c.flat_map(char::to_lowercase)).collect(),
        None => s,
    };
    text(s).font(th::LABEL).color(color)
}

// ---- effect slots ------------------------------------------------------------------------------------

/// Where the compositor copies the frame so far, once, for every blur after it (the glass and the
/// veils). It goes after the page and before the first of them, and it draws nothing; it has no fade
/// of its own, since a slot inside a fading layer is skipped.
pub fn snapshot() -> El {
    el().key("snapshot").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).fx(fx::SNAP).fx_backdrop(true)
}

/// A pane of liquid glass: an empty box the compositor paints over whatever is behind it (the
/// frame as `snapshot` saw it, so it belongs after the page and not inside it).
///
/// It has no enter or exit fade of its own: every element fades in over 0.2 s by default, and a
/// slot inside a fading layer is skipped by the compositor, so the glass (and its blur) would only
/// show once the fade had settled. Whatever holds a pane has to stay out of a fade as well; the
/// pane is faded through its own strength (`effect_data`: the first number is how much of it is hidden).
pub fn glass(kind: Glass, radius: f32) -> El {
    let pane = el().absolute(0.0, 0.0).w_full().h_full().radius(radius).transition(Transition::none());
    if fx::ready() {
        pane.effect(fx::glass(kind))
    } else {
        // The window's first frames, before the compositor is up: a plain tint where the glass goes.
        pane.fill(th::veil(0.06)).border(1.0, th::veil(0.08))
    }
}

/// The inside of a frosted card (a menu, a popover) `radius` round, `skin` (0..1) of the way in:
/// the compositor's soft glass over the page as the last `snapshot` saw it, then a film of the
/// card's own colour so its words stay easy to read. Before the compositor is up, and with the
/// effects off, the film is solid.
pub fn frost(radius: f32, skin: f32) -> [El; 2] {
    let film = if fx::ready() { th::FROST } else { 1.0 };
    [
        glass(Glass::Menu, radius).fx_data([1.0 - skin, 0.0, 0.0, 0.0]),
        el().absolute(0.0, 0.0).w_full().h_full().radius(radius).fill(th::pop().alpha(film * skin)).transition(Transition::none()),
    ]
}

/// A progressive blur over the top `h` points of the window.
pub fn veil_top(h: f32) -> El {
    el().key("veil-top").absolute(0.0, 0.0).w_full().h(h).transition(Transition::none()).fx(fx::VEIL).fx_data([h, 0.0, 20.0, 0.38])
}

/// A progressive blur over the bottom `h` points of the window.
pub fn veil_bottom(h: f32) -> El {
    el().key("veil-bottom").pin(Some(0.0), None, Some(0.0), Some(0.0)).h(h).transition(Transition::none()).fx(fx::VEIL).fx_data([0.0, h, 20.0, 0.38])
}

// ---- controls ----------------------------------------------------------------------------------------

/// The corner of a control `h` tall: a rounded rectangle, never a pill (10 at the usual size).
pub fn ctl_radius(h: f32) -> f32 {
    (h * 0.28).round().clamp(6.0, 12.0)
}

/// An icon button: a rounded square that lights up under the pointer. `size` is the hit area,
/// `glyph` the icon.
pub fn ibtn(key: impl Into<String>, ic: &'static Icon, glyph: f32, size: f32, color: Color, name: &str) -> El {
    row()
        .key(key)
        .role("button")
        .label(name)
        .help(name)
        .w(size)
        .h(size)
        .radius(ctl_radius(size))
        .center()
        .cursor(Cursor::Pointer)
        .hov(veil(0.07))
        .press(veil(0.12))
        .press_scale(th::PRESS)
        .child(icon(ic, glyph).color(color))
}

/// The soft disc under a quiet button's glyph that shows under the pointer: only a little wider
/// than the glyph, so a row of them never reads as a row of plates.
fn quiet_halo(glyph: f32) -> El {
    let d = (glyph + 14.0).round();
    el().absolute(0.0, 0.0).w_full().h_full().center().child(el().w(d).h(d).radius(d * 0.5).fill(veil(0.08)).gop(0.0, 1.0))
}

/// An icon button of the open player: no plate, the glyph brightens under the pointer over a
/// small disc. `size` is the hit area, which stays generous.
pub fn quiet_btn(key: impl Into<String>, ic: &'static Icon, glyph: f32, size: f32, color: Color, name: &str) -> El {
    row()
        .key(key)
        .role("button")
        .label(name)
        .help(name)
        .w(size)
        .h(size)
        .center()
        .cursor(Cursor::Pointer)
        .hov_group()
        .press_scale(th::PRESS)
        .child(quiet_halo(glyph))
        .child(icon(ic, glyph).color(color).gcol(th::text()))
}

/// A quiet toggle: faint while off, in full ink while on. No dot or mark under it: the glyph
/// turning white is the whole signal (shuffle, repeat, the lyrics, the queue, their look).
pub fn quiet_toggle(key: impl Into<String>, ic: &'static Icon, glyph: f32, size: f32, on: bool, name: &str) -> El {
    quiet_btn(key, ic, glyph, size, if on { th::text() } else { th::text3() }, name).checked(on)
}

/// A quiet toggle that cannot be pressed now (no lyrics for this track): fainter still, and it
/// says why.
pub fn quiet_off(key: impl Into<String>, ic: &'static Icon, glyph: f32, size: f32, name: &str) -> El {
    row().key(key).role("button").label(name).help(name).disabled(true).w(size).h(size).center().child(icon(ic, glyph).color(veil(0.16)))
}

/// "Previous" or "next": a quiet button, or, when there is nowhere to go, a faint one that can't
/// be pressed (the same size and key, so nothing moves when it comes back).
pub fn skip_btn(s: &State, forward: bool, key: &str, ic: &'static Icon, glyph: f32, size: f32, color: Color) -> El {
    let name = if forward { S::Next } else { S::Previous }.get(s.lang);
    let on = if forward { s.can_next() } else { s.can_prev() };
    if !on {
        return quiet_off(key.to_owned(), ic, glyph, size, name);
    }
    quiet_btn(key.to_owned(), ic, glyph, size, color, name).on_press(move |s: &mut State| if forward { s.next() } else { s.prev() })
}

/// A glyph that stands for a state (play or pause, liked or not), keyed by it: the new one pops in
/// from a little smaller on a lively spring while the old one fades, so the change reads as a
/// press answered rather than a picture swapped.
fn swap_glyph(key: String, g: El, pops: bool) -> El {
    let pop = rune::SpringPair::new(rune::Spring::new(0.34, 0.3), rune::Spring::new(0.1, 0.0));
    let enter = if pops { Transition::scale(0.6).allow_small_scale().combined(Transition::opacity()) } else { Transition::opacity() };
    el().key(key).absolute(0.0, 0.0).w_full().h_full().center().transition(Transition::asymmetric(enter, Transition::opacity()).springs(pop)).child(g)
}

/// The open player's play / pause: its own glyphs, and under the pointer a disc a little wider
/// than the glyph rather than one filling the hit area.
pub fn np_play_btn(s: &State, size: f32, glyph: f32) -> El {
    play_btn_with(s, size, glyph, &super::np_icons::PLAY, &super::np_icons::PAUSE, "np-play", true)
}

/// The bar's play / pause: the open player's glyphs and its quiet disc, at the bar's size.
pub fn bar_play_btn(s: &State, size: f32, glyph: f32) -> El {
    play_btn_with(s, size, glyph, &super::np_icons::PLAY, &super::np_icons::PAUSE, "play", true)
}

fn play_btn_with(s: &State, size: f32, glyph: f32, play: &'static Icon, pause: &'static Icon, key: &str, quiet: bool) -> El {
    let (g, state) = if s.player.loading {
        (icon(&icons::SPINNER, glyph * 0.8).color(th::text()).looping(rune::LoopSpec::rotation(0.9)), "load")
    } else if s.player.playing {
        (icon(pause, glyph).color(th::text()), "pause")
    } else {
        (icon(play, glyph).color(th::text()), "play")
    };
    let g = swap_glyph(format!("{key}-{}-{state}", size as u32), g, true);
    let b = row()
        .key(format!("{key}-{}", size as u32))
        .role("button")
        .label(if s.player.playing { S::Pause.get(s.lang) } else { S::Play.get(s.lang) })
        .help(if s.player.playing { S::Pause.get(s.lang) } else { S::Play.get(s.lang) })
        .w(size)
        .h(size)
        .radius(size * 0.5)
        .center()
        .cursor(Cursor::Pointer);
    let b = if quiet { b.hov_group().child(quiet_halo(glyph)) } else { b.hov(veil(0.08)).press(veil(0.14)) };
    b.press_scale(th::PRESS).on_press(|s: &mut State| s.toggle_pause()).child(g)
}

// ---- buttons -----------------------------------------------------------------------------------------
//
// One family, two weights, one height. Every button is `BTN_H` tall (or square), a rounded
// rectangle (`ctl_radius`), quiet and flat, like the fields:
//
// - solid: the ink as a filled block with a faint shadow (one per screen: the main action);
// - soft: a tonal block, the ink at a few percent inside a hairline, for the actions beside it
//   (Shuffle, Retry);
// - round: either of the two as a square holding a glyph (play beside a title, "...", close).
//
// Hover lifts the surface a step, a press darkens it and gives a little (`th::PRESS`).

/// The height of every button with words in it, and its corners.
pub const BTN_H: f32 = 36.0;
/// A text field's corners (a field is a soft button that takes words).
pub const FIELD_R: f32 = 10.0;

/// The surface of a text field: the soft button's, brighter at its edge while it has the caret.
pub fn field_surface(base: El) -> El {
    base.radius(FIELD_R).fill(veil(0.04)).border(1.0, veil(0.09)).hov(veil(0.02))
}

/// How a button carries itself.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    Solid,
    Soft,
}

/// The surface of a button of `weight`, `w` by `h`, as a squircle: the plate, a lighter one that
/// comes up under the pointer, and the shadow of the solid kind.
fn surface_sq(base: El, weight: Weight, w: f32, h: f32) -> El {
    surface_pill(base, weight, w, h)
}

/// A fully rounded surface for primary and secondary actions that carry text.
fn surface_pill(base: El, weight: Weight, w: f32, h: f32) -> El {
    let radius = ctl_radius(h);
    let b = base.w(w).h(h).radius(radius).clip_rounded(radius).cursor(Cursor::Pointer).press_scale(th::PRESS).hov_group();
    let (fill, edge, lift) = match weight {
        Weight::Solid => (th::solid(), None, th::on_solid().alpha(0.10)),
        Weight::Soft => (veil(0.05), Some(veil(0.08)), veil(0.05)),
    };
    let plate = el().absolute(0.0, 0.0).w_full().h_full().radius(radius).fill(fill);
    let edge_plate = edge.map(|color| el().absolute(0.0, 0.0).w_full().h_full().radius(radius).border(1.0, color));
    b.child(plate).children(edge_plate).child(el().absolute(0.0, 0.0).w_full().h_full().radius(radius).fill(lift).gop(0.0, 1.0))
}

/// The colour of words and glyphs on a button of `weight`.
fn ink_on(weight: Weight) -> Color {
    match weight {
        Weight::Solid => th::on_solid(),
        Weight::Soft => th::text(),
    }
}

/// A button with words (and a glyph before them).
pub fn button(key: impl Into<String>, weight: Weight, name: &str, ic: Option<&'static Icon>) -> El {
    let ink = ink_on(weight);
    let mut r = surface_pill(row().key(key).role("button").label(name).gap(8.0).center(), weight, button_width(name, ic.is_some()), BTN_H);
    r = if ic.is_some() { r.pl(12.0).pr(16.0) } else { r.px(16.0) };
    if let Some(i) = ic {
        // Nudged 1pt right: the play triangle reads off-centre against the words otherwise.
        r = r.child(icon(i, 15.0).color(ink).ml(1.0));
    }
    r.child(text(name.to_owned()).font(th::BTN).color(ink))
}

/// How wide [`button`] is with this name (and a glyph before it, or not).
pub fn button_width(name: &str, glyph: bool) -> f32 {
    let tw = rune::text::measure(name, &[], th::BTN, None).0.ceil();
    (if glyph { 12.0 + 15.0 + 8.0 + 1.0 + tw + 16.0 } else { 16.0 + tw + 16.0 }) + 2.0
}

// ---- what is not here yet ----------------------------------------------------------------------------

/// The ink of anything not here yet: a picture's place, a line's bar, a button's shape.
pub fn bone_fill() -> Color {
    veil(0.05)
}

/// A line of text not here yet: a box exactly as tall as the line it stands for (so nothing moves
/// when the words come), up to `w` wide, with a bar about as tall as the letters in its middle.
pub fn text_bone(key: impl Into<String>, line: f32, ink: f32, w: f32) -> El {
    row().key(key).w_full().max_w(w).h(line).items_center().child(el().grow(1.0).h(ink).radius(ink * 0.5).fill(bone_fill()))
}

/// A button not here yet, `w` wide and as tall as a button.
pub fn button_bone(key: impl Into<String>, w: f32) -> El {
    el().key(key).w(w).h(BTN_H).radius(ctl_radius(BTN_H)).fill(bone_fill())
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

/// A button that picks one of several things: what is picked on the left, a chevron on the right
/// (turned up while its menu is open). The height and look of [`btn_small`], so the two sit level.
pub fn btn_select(key: impl Into<String>, current: &str, w: f32, open: bool) -> El {
    surface_sq(row().key(key).role("button").label(current).pl(12.0).pr(9.0).gap(8.0).items_center(), Weight::Soft, w, 32.0)
        .child(text(current.to_owned()).font(th::BODY_M).color(th::text()).truncate().grow(1.0).min_w(0.0))
        .child(icon(&icons::CHEVRON_DOWN, 15.0).color(th::text3()).gcol(th::text2()).rotate(if open { std::f32::consts::PI } else { 0.0 }).motion(th::FLIGHT))
}

/// A square `size` across holding a glyph `glyph` big, its corners like a button's.
pub fn round(key: impl Into<String>, weight: Weight, ic: &'static Icon, size: f32, glyph: f32, name: &str) -> El {
    surface_sq(row().key(key).role("button").label(name).help(name).center(), weight, size, size).child(icon(ic, glyph).color(ink_on(weight)))
}

/// A fully rounded icon button for page headers. Its width may be the button height for a circular
/// control or a little wider when a compact label is added beside the icon.
pub fn pill_icon(key: impl Into<String>, weight: Weight, ic: &'static Icon, width: f32, glyph: f32, name: &str) -> El {
    let radius = ctl_radius(BTN_H);
    let (fill, edge, ink) = match weight {
        Weight::Solid => (th::solid(), None, th::on_solid()),
        Weight::Soft => (veil(0.05), Some(veil(0.08)), th::text()),
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
        .hov(veil(if weight == Weight::Solid { 0.08 } else { 0.05 }))
        .press(veil(if weight == Weight::Solid { 0.14 } else { 0.09 }))
        .child(icon(ic, glyph.min(18.0)).color(ink));
    if let Some(edge) = edge {
        b = b.border(1.0, edge);
    }
    b
}

/// A quiet link that leads further ("All", "Show all"): words and a small chevron, brighter under
/// the pointer.
/// The button that orders a list: the sort glyph, and the order picked when it is not the list's
/// own, lit while its menu is open.
pub fn sort_button(key: &str, s: &State, now: Option<&str>, open: bool) -> El {
    let name = S::SortBy.get(s.lang);
    let mut b = row()
        .key(key.to_owned())
        .role("button")
        .label(name)
        .help(name)
        .checked(open)
        .h(30.0)
        .px(if now.is_some() { 10.0 } else { 7.0 })
        .gap(6.0)
        .radius(8.0)
        .items_center()
        .cursor(Cursor::Pointer)
        .hov(veil(0.06))
        .press(veil(0.10))
        .hov_group()
        .child(icon(&icons::SORT, 17.0).color(if now.is_some() || open { th::text() } else { th::text3() }).gcol(th::text()));
    if open || now.is_some() {
        b = b.fill(veil(0.06));
    }
    if let Some(n) = now {
        b = b.child(label(n.to_owned(), th::BODY_M, th::text2()).gcol(th::text()));
    }
    b
}

pub fn more_link(key: impl Into<String>, name: &str) -> El {
    row()
        .key(key)
        .role("link")
        .label(name)
        .h(30.0)
        .pl(10.0)
        .pr(6.0)
        .gap(2.0)
        .radius(8.0)
        .items_center()
        .cursor(Cursor::Pointer)
        .hov(veil(0.06))
        .press(veil(0.10))
        .hov_group()
        .child(label(name.to_owned(), th::BODY_M, th::text2()).gcol(th::text()))
        .child(icon(&icons::CHEVRON_RIGHT, 16.0).color(th::text3()).gcol(th::text2()))
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

/// How a tab's plate slides from the one left to the one picked.
const TAB_SPRING: rune::Spring = rune::Spring::new(0.36, 0.12);
const TAB_PILL_H: f32 = 28.0;

/// How wide a [`tab_pill`] with this name (and count) is.
pub fn tab_pill_w(name: &str, count: Option<usize>) -> f32 {
    let mut w = 12.0 + rune::text::measure(name, &[], th::BODY_M, None).0.ceil() + 12.0;
    if let Some(n) = count {
        w += 6.0 + rune::text::measure(&n.to_string(), &[], th::SMALL, None).0.ceil();
    }
    w
}

/// One tab of a [`tab_track`]: its name and, when known, how many there are. The plate under
/// the picked one is the track's, and slides between them.
pub fn tab_pill(key: impl Into<String>, name: &str, count: Option<usize>, on: bool) -> El {
    let ink = if on { th::text() } else { th::text2() };
    let mut r = row()
        .key(key)
        .role("tab")
        .label(name)
        .checked(on)
        .w(tab_pill_w(name, count))
        .h(TAB_PILL_H)
        .px(12.0)
        .gap(6.0)
        .radius(7.0)
        .items_center()
        .cursor(Cursor::Pointer)
        .hov(veil(if on { 0.0 } else { 0.05 }))
        .press_scale(0.97)
        .child(label(name.to_owned(), th::BODY_M, ink));
    if let Some(n) = count {
        r = r.child(label(n.to_string(), th::SMALL, ink.alpha(0.55)));
    }
    r
}

/// Tabs side by side on one soft track: each a [`tab_pill`] with its width ([`tab_pill_w`]); the
/// picked one (`at`) sits on a plate that slides to whichever is pressed next.
pub fn tab_track(key: impl Into<String>, tabs: Vec<(El, f32)>, at: usize) -> El {
    let key: String = key.into();
    let at = at.min(tabs.len().saturating_sub(1));
    let x = 4.0 + tabs[..at].iter().map(|(_, w)| w + 2.0).sum::<f32>();
    let pw = tabs.get(at).map_or(0.0, |(_, w)| *w);
    let total = 8.0 + tabs.iter().map(|(_, w)| *w).sum::<f32>() + 2.0 * tabs.len().saturating_sub(1) as f32;
    let mut plate = el().key(format!("{key}-plate")).absolute(x, 4.0).w(pw).h(TAB_PILL_H).radius(7.0).fill(if th::light() { th::pop() } else { veil(0.10) }).motion(TAB_SPRING);
    if th::light() {
        plate = plate.shadow(1.0, 3.0, th::rgba(0x000000, 0.10));
    }
    let items = row().absolute(0.0, 0.0).h(36.0).px(4.0).gap(2.0).items_center().children(tabs.into_iter().map(|(e, _)| e));
    el().key(key).w(total).h(36.0).radius(10.0).fill(veil(0.04)).border(1.0, veil(0.07)).child(plate).child(items)
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
pub fn cover_px(pt: f32) -> u32 {
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
    c.child(edge)
        .child(el().key(morph_key.to_owned()).absolute(0.0, 0.0).w_full().h_full().radius(radius).transition(Transition::none()).fx(fx::ARTMORPH).fx_data([seed, 0.0, 0.0, 0.0]))
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
        let lit = el().absolute(0.0, 0.0).w(band).h_full().looping(spec(-x0 / band, -x1 / band)).child(label(name, style, lit_color).truncate().w(width).absolute(0.0, 0.0));
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
    let hovered = !under() && s.wave_hover.as_deref() == Some(key);
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
                .hov(veil(0.06))
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
    let hovered = !under() && s.wave_hover.as_deref() == Some(key);
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

/// An empty or waiting state: its illustration drawing itself on, and a line under it.
pub fn empty(p: &Pal, art: Ill, msg: &str) -> El {
    col().w_full().py(40.0).gap(16.0).items_center().child(shapes::ill(art, 140.0, p.accent, 0.08)).child(label(msg.to_owned(), th::BODY, th::text2()).align(Align::Center))
}

// ---- dialogs -----------------------------------------------------------------------------------------

/// A dialog over the window: the window dims, and a card comes up out of the middle, a little
/// small and rising into place, then goes the way it came. A press beside the card or Escape
/// dismisses it; the card's own presses stay on it. `body` sits between the title and the buttons
/// (the first button is the quiet one, the last the one the dialog is for).
pub fn dialog(key: &str, w: f32, title: &str, body: Vec<El>, buttons: Vec<El>, dismiss: fn(&mut State)) -> El {
    let width = 400.0_f32.min(w - 32.0);
    let card = col()
        .key(format!("{key}-card"))
        .role("dialog")
        .label(title.to_owned())
        .w(width)
        .padding(20.0)
        .gap(12.0)
        .radius(16.0)
        .fill(th::pop())
        .border(1.0, th::hairline())
        .shadow(16.0, 48.0, th::rgba(0x000000, if th::light() { 0.16 } else { 0.42 }))
        .transition(Transition::opacity().combined(Transition::scale(0.96)).combined(Transition::offset(0.0, 8.0)).springs(th::RISE))
        .modal(true, dismiss)
        .on_press(|_s: &mut State| {})
        .child(label(title.to_owned(), th::H2, th::text()).wrap())
        .children(body)
        .child(row().w_full().pt(6.0).gap(10.0).justify_end().children(buttons));
    el().key(key.to_owned())
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .center()
        .fill(th::rgba(0x000000, if th::light() { 0.24 } else { 0.52 }))
        .transition(Transition::opacity().springs(th::RISE))
        .on_press(dismiss)
        .child(card)
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
    heading(key, title, more).pl(8.0).pr(4.0)
}

/// A scrolling page that runs under the title bar and the bar, beside the rail, centred in its
/// column. Its content
/// rises a little as it fades up; the page it replaces fades out at the same time.
pub fn scroll_page(key: impl Into<String>, body: El) -> El {
    page_shell(key, body.transition(Transition::opacity().combined(Transition::offset(0.0, 14.0)).combined(Transition::scale(0.99)).springs(th::PAGE_IN)))
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
            // The user scrolling takes over from a page being put back where it was.
            s.scroll_restore = None;
            if e.precise || e.mods.ctrl || e.mods.cmd || e.dy == 0.0 || e.dx.abs() > e.dy.abs() {
                return false;
            }
            s.glide.wheel(&wheel_key, e.dy);
            true
        })
        .on_scroll_offset(move |p: rune::ScrollPos| {
            crate::app::PAGE_SCROLL.with_borrow_mut(|m| {
                m.insert(watch_key.clone(), [p.offset[1], p.max[1]]);
            })
        })
        .transition(Transition::opacity().springs(th::LEAVE))
        .child(row().w_full().justify_center().transition(Transition::none()).child(body.w_full().max_w(PAGE_MAX).px(PAGE_PAD).pt(TITLE_H + 8.0).pb(foot_room()).gap(44.0)))
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
    e.transition(Transition::opacity().combined(Transition::offset(0.0, 12.0)).springs(th::PAGE_IN)).enter_delay(0.04 * i.min(6) as f32)
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
                if self.library.filter.text.trim().is_empty() { all } else { self.library.shown.borrow().iter().filter_map(|&i| all.get(i).cloned()).collect() }
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
pub const MENU_ROW: f32 = 32.0;
const MENU_GLYPH: f32 = 16.0;

/// An entry of a menu card: a small glyph and a name on a row that lights up under the pointer.
/// (Menus fade their entries themselves, so a row has no enter or exit of its own.)
pub fn menu_row(key: impl Into<String>, ic: &'static Icon, text: &str, on: bool) -> El {
    row()
        .key(key)
        .role("button")
        .label(text)
        .w_full()
        .h(MENU_ROW)
        .px(8.0)
        .gap(10.0)
        .items_center()
        .radius(7.0)
        .cursor(Cursor::Pointer)
        .transition(Transition::none())
        .fill(if on { veil(0.08) } else { Color::TRANSPARENT })
        .hov(veil(0.06))
        .press(veil(0.10))
        .child(icon(ic, MENU_GLYPH).color(if on { th::text() } else { th::text2() }))
        .child(label(text.to_owned(), th::BODY, th::text()).truncate().min_w(0.0))
}

/// The right-click / "more" menu of a track, opened from a point or from the element with a key.
/// `queue_index` is the track's slot when the row belongs to the queue (it adds "remove").
pub fn open_track_menu(s: &mut State, track: &Track, queue_index: Option<usize>, anchor: impl Into<PopupAnchor>) {
    s.open_track_menu(track.clone(), queue_index, anchor.into());
}

/// The heart in a button `size` across: half of it, and never large.
pub fn heart_glyph(size: f32) -> f32 {
    (size * 0.48).round().clamp(14.0, 18.0)
}

pub fn like_button(s: &State, track: &Track, size: f32) -> El {
    like_button_with(s, track, size, heart_glyph(size), (&icons::HEART, &icons::HEART_FILL), false)
}

/// The open player's heart: its own glyph and the small disc under the pointer.
pub fn np_like_button(s: &State, track: &Track, size: f32, glyph: f32) -> El {
    like_button_with(s, track, size, glyph, (&super::np_icons::HEART, &super::np_icons::HEART_FILL), true)
}

fn like_button_with(s: &State, track: &Track, size: f32, glyph_px: f32, hearts: (&'static Icon, &'static Icon), quiet: bool) -> El {
    let on = s.is_liked(&track.id);
    let p = s.pal();
    let t = track.clone();
    let name = if on { S::Unlike.get(s.lang) } else { S::Like.get(s.lang) };
    let rest = if quiet { th::text3() } else { th::text2() };
    let mut glyph = icon(if on { hearts.1 } else { hearts.0 }, glyph_px).color(if on { p.accent } else { rest });
    if quiet && !on {
        glyph = glyph.gcol(th::text());
    }
    let mut b = row().key(format!("like-{}", track.id)).role("button").label(name).help(name).w(size).h(size).radius(ctl_radius(size)).cursor(Cursor::Pointer);
    b = if quiet { b.hov_group().child(quiet_halo(glyph_px)) } else { b.hov(veil(0.07)).press(veil(0.12)) };
    let mut b = b
        .press_scale(th::PRESS)
        // Only a heart just pressed pops (rows that scroll into view bring theirs in quietly).
        .child(swap_glyph(format!("like-{}-{on}", track.id), glyph, s.burst_on && s.burst_track == track.id))
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
        b = b.child(el().key("burst").fx(fx::SPARKLES).fx_data([s.like_burst as f32, 14.0, 6.0, 0.0]).fx_bleed(40.0).absolute(0.0, 0.0).w_full().h_full().radius(ctl_radius(size)));
    }
    b
}

pub const ROW_H: f32 = 56.0;
pub const ROW_PAD: f32 = 8.0;
const ROW_GAP: f32 = 12.0;
/// The small square buttons at a row's end, and their glyphs.
const ROW_BTN: f32 = 32.0;
const ROW_GLYPH: f32 = 16.0;
/// The length at a row's end: wide enough for "12:34", a little apart from the heart.
const ROW_TIME: f32 = 44.0;
const ROW_ACTIONS: f32 = ROW_BTN * 2.0 + 2.0 + 10.0 + ROW_TIME;
/// A row's cover.
pub const ROW_COVER: f32 = 44.0;
pub const ROW_COVER_R: f32 = 7.0;

/// The small "E" after an explicit track's name, the size of its lowercase letters, with what
/// it means under the pointer.
pub fn explicit_badge(s: &State) -> El {
    row()
        .key("explicit")
        .role("note")
        .label(S::Explicit.get(s.lang))
        .help(S::ExplicitHint.get(s.lang))
        .w(15.0)
        .h(14.0)
        .radius(3.5)
        .border(1.0, th::text3().alpha(0.7))
        .center()
        .child(label("E", TextStyle::new(9.0, 650.0).family(th::SANS).lh(9.0), th::text3()))
}

/// The mark after a track's name when FckCensor's uncensored version of it was loaded.
pub fn clean_badge(s: &State) -> El {
    let accent = s.pal().accent;
    row()
        .key("clean")
        .role("note")
        .label(S::CleanLoaded.get(s.lang))
        .help(S::CleanLoaded.get(s.lang))
        .w(18.0)
        .h(14.0)
        .radius(3.5)
        .fill(accent.alpha(0.16))
        .center()
        .child(icon(&icons::VOLUME, 11.0).color(accent))
}

/// A track's name in a row: as much of it as fits, then its marks (explicit, uncensored), which
/// are never cut off.
pub fn title_line(s: &State, t: &Track, ink: Color, width: f32, moving: bool) -> El {
    let name = t.full_title();
    let badges = 26.0 * (u8::from(t.is_explicit()) + u8::from(s.clean_tracks.contains(&t.id))) as f32;
    let name_w = rune::text::measure(&name, &[], th::BODY_M, None).0.ceil();
    let room = if width.is_finite() { (width - badges).max(40.0) } else { width };
    let line = marquee(format!("tl-{}", t.id), room, name_w, moving, s.settings.reduce_motion, move || label(name.clone(), th::BODY_M, ink));
    let mut r = row().min_w(0.0).gap(6.0).items_center().child(line.shrink());
    if t.is_explicit() {
        r = r.child(explicit_badge(s));
    }
    if s.clean_tracks.contains(&t.id) {
        r = r.child(clean_badge(s));
    }
    r
}

/// The playing track's marker: three bars that dance with the bass, the middle and the highs of
/// what plays, and settle into three still dots while it is paused. It is white, for sitting on
/// a cover.
pub fn marker(s: &State) -> El {
    el().key("playing-marker").w(22.0).h(22.0).transition(Transition::none()).fx(fx::MORPH).fx_data([if s.player.playing { 1.0 } else { 0.0 }, 1.0, 0.0, 0.0])
}

/// A list row's cover; the playing one is dimmed with the marker on it.
///
/// The cover is the same element whether or not its track plays. Building a different tree for the
/// playing row remounted the picture on every track change, and a remounted element fades in, so
/// the cover flashed bright, went blank and came back. Here the dim over it is always there and only
/// changes colour (which springs), and the marker, a compositor slot, comes without a fade of its own.
fn row_cover(s: &State, t: &Track, playing: bool, queue: bool) -> El {
    let dim = th::rgba(0x000000, if playing { 0.52 } else { 0.0 });
    let mut c = el()
        .key("rc")
        .w(ROW_COVER)
        .h(ROW_COVER)
        .transition(Transition::none())
        .child(cover(s, t.cover_template(), ROW_COVER, ROW_COVER_R))
        .child(el().key("rc-dim").absolute(0.0, 0.0).w_full().h_full().radius(ROW_COVER_R).transition(Transition::none()).fill(dim));
    if playing && queue {
        c = c.child(
            row()
                .key("rc-play-queue")
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .center()
                .transition(Transition::none())
                .child(icon(if s.player.playing { &icons::PAUSE } else { &icons::PLAY }, 16.0).color(th::rgb(0xffffff))),
        );
    } else if playing {
        c = c.child(row().key("rc-mark").absolute(0.0, 0.0).w_full().h_full().center().transition(Transition::none()).child(marker(s)));
    } else {
        c = c.child(
            row()
                .key("rc-play")
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .center()
                .fill(th::rgba(0x000000, 0.36))
                .radius(ROW_COVER_R)
                .gop(0.0, 1.0)
                .child(icon(&icons::PLAY, 16.0).color(th::rgb(0xffffff))),
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
    // A failed one says why; one the service will never give wears a warning, not an arrow that
    // invites another try.
    let lasting = s.download_errors.get(&t.id).is_some_and(|f| f.lasting());
    let ic = if matches!(state, Some(DownloadState::Failed)) && lasting { &icons::ALERT } else { ic };
    let help_text = match (state, s.download_why(&t.id)) {
        (Some(DownloadState::Failed), Some(why)) => format!("{}: {why}", help.get(s.lang)),
        _ => help.get(s.lang).to_owned(),
    };
    if matches!(state, Some(DownloadState::Downloading)) {
        return row()
            .key(format!("download-{}", t.id))
            .role("status")
            .label(help_text.as_str())
            .help(help_text.as_str())
            .w(ROW_BTN)
            .h(ROW_BTN)
            .center()
            .child(icon(ic, ROW_GLYPH).color(color).looping(rune::LoopSpec::rotation(0.9)));
    }
    let mut b = ibtn(format!("download-{}", t.id), ic, ROW_GLYPH, ROW_BTN, color, &help_text);
    // Only a downloaded (or failed) track shows its mark at rest; the others offer it under the pointer.
    if state.is_none() {
        b = b.gop(0.0, 1.0);
    }
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
    let qi = (src == ListSrc::Queue).then(|| s.player.queue.iter().position(|queued| queued.id == t.id).unwrap_or_else(|| n.saturating_sub(1)));
    track_row_at(s, t, src, qi)
}

/// The row of the track at `qi` in the queue. It is that place, not the track, that it stands
/// for: a press plays that place and only that place lights up as playing, so a track queued
/// twice is two rows that each do what they say.
pub fn queue_row(s: &State, t: &Track, qi: usize) -> El {
    track_row_at(s, t, ListSrc::Queue, Some(qi)).on_press(move |s: &mut State| s.play_index(qi))
}

fn track_row_at(s: &State, t: &Track, src: ListSrc, qi: Option<usize>) -> El {
    let queue = src == ListSrc::Queue;
    let playing = match (queue, qi) {
        (true, Some(i)) => s.player.index == Some(i),
        _ => s.current().is_some_and(|c| c.id == t.id),
    };
    let ok = t.is_available();
    let (id, id2, t2) = (t.id.clone(), t.id.clone(), t.clone());
    let ink = if ok { th::text() } else { th::text3() };
    // Each artist is a link of their own (their page), lighting up under the pointer; lines
    // that don't fit slide while the pointer is on the row.
    let hot = format!("row-{id}");
    let moving = marquee_on(s, &hot);
    let room_w = room(&hot);
    let meta = artists_row_in_width(s, &t.artists, th::SMALL, false, &format!("row-{id}-ar"), th::text2(), room_w, moving);

    row()
        .key(format!("row-{id}"))
        .role("button")
        .label(t.full_title())
        .w_full()
        .h(ROW_H)
        .px(ROW_PAD)
        .gap(ROW_GAP)
        .items_center()
        .radius(10.0)
        .cursor(Cursor::Pointer)
        .fill(if playing { veil(0.07) } else { Color::TRANSPARENT })
        .hov(veil(0.045))
        .press(veil(0.08))
        .on_press(move |s: &mut State| s.play_from_list(src, &id))
        .on_secondary_press(move |s: &mut State, press, _tx: &mut Tx| open_track_menu(s, &t2, qi, press))
        .hov_group()
        .on_hover_change(marquee_hover(hot.clone()))
        .child(row_cover(s, t, playing, queue))
        .child(measure_room(
            col().grow(1.0).min_w(0.0).h_full().justify_center().gap(2.0).child(title_line(s, t, if playing { s.pal().accent } else { ink }, room_w, moving)).child(meta),
            hot,
        ))
        // The heart comes up under the pointer; a liked track keeps it showing.
        .child(
            row()
                .key(format!("row-actions-{id2}"))
                .w(ROW_ACTIONS)
                .h(ROW_BTN)
                .gap(2.0)
                .justify_end()
                .items_center()
                .child(download_action(s, t))
                .child({
                    let heart = like_button(s, t, ROW_BTN);
                    if queue || s.is_liked(&t.id) { heart } else { heart.gop(0.0, 1.0) }
                })
                .child({
                    let t3 = t.clone();
                    let time = row().w_full().h_full().pr(6.0).items_center().justify_center().gop(1.0, 0.0).justify_end().child(label(
                        crate::player::fmt_time(t.duration_ms.unwrap_or(0)),
                        th::TIME,
                        th::text3(),
                    ));
                    let more = ibtn(format!("more-{id2}"), &icons::MORE, ROW_GLYPH + 2.0, ROW_BTN, th::text2(), S::More.get(s.lang))
                        // Its right edge where the time ends, so it keeps the row's inset as the time does.
                        .absolute(ROW_TIME - ROW_BTN, 0.0)
                        .gop(0.0, 1.0)
                        .on_press(move |s: &mut State| open_track_menu(s, &t3, qi, format!("more-{}", t3.id)));
                    el().key(format!("duration-{id2}")).w(ROW_TIME).h(ROW_BTN).ml(10.0).hov_group().child(time).child(more)
                }),
        )
}

/// What stands in for a whole list that isn't there: nothing while it loads (the list comes in
/// when it arrives, there's no waiting screen), or what went wrong.
pub fn list_state<T>(l: &Load<T>, s: &State, _rows: usize) -> Option<El> {
    match l {
        Load::Loading | Load::Idle => None,
        Load::Failed(_) => Some(
            col()
                .w_full()
                .py(32.0)
                .gap(20.0)
                .items_center()
                .child(shapes::ill(Ill::Unplugged, 140.0, s.pal().accent, 0.05))
                .child(label(S::Offline.get(s.lang), th::BODY, th::text2()))
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

/// A track's artists, `width` wide, sliding on their own when they don't fit (see [`marquee`]).
pub fn artist_line_width(s: &State, t: &Track, style: TextStyle, pictures: bool, key: &str, width: f32) -> El {
    artists_row_in_width(s, &t.artists, style, pictures, key, th::text2(), width, true)
}

/// Every one of `artists`, each a link to their page, with their pictures when `pictures`.
fn artists_all(s: &State, artists: &[ym_api::Artist], style: TextStyle, pictures: bool, key: &str, color: Color) -> El {
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
            .on_press(move |s: &mut State| s.go(crate::app::Page::Artist(id.clone())));
        if pictures {
            item = item.child(avatar(s, a, 22.0));
        }
        r = r.child(item.child(label(text, style, color).gcol(th::text())));
    }
    r
}

/// How wide [`artists_all`] draws `artists`.
pub fn artists_width(artists: &[ym_api::Artist], style: TextStyle, pictures: bool) -> f32 {
    let n = artists.len();
    artists
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let text = if !pictures && i + 1 < n { format!("{}, ", a.name) } else { a.name.clone() };
            rune::text::measure(&text, &[], style, None).0.ceil() + if pictures { 29.0 } else { 0.0 }
        })
        .sum::<f32>()
        + if pictures { 14.0 * n.saturating_sub(1) as f32 } else { 0.0 }
}

/// [`artists_row`] in `color`, `width` wide: all of them, sliding through the line when they
/// don't fit (on their own, or only while `moving`).
#[allow(clippy::too_many_arguments)]
pub fn artists_row_in_width(s: &State, artists: &[ym_api::Artist], style: TextStyle, pictures: bool, key: &str, color: Color, width: f32, moving: bool) -> El {
    let line_w = artists_width(artists, style, pictures);
    let reduce = s.settings.reduce_motion;
    marquee(format!("{key}-mq"), width, line_w, moving, reduce, || artists_all(s, artists, style, pictures, key, color))
}

// ---- marquee -----------------------------------------------------------------------------------------

/// The room between a sliding line's end and its next start.
const MARQUEE_GAP: f32 = 48.0;
/// How fast a sliding line goes, in points a second.
const MARQUEE_SPEED: f32 = 58.0;
/// How much of that speed a line keeps while the pointer is on it, so it is easier to read.
const MARQUEE_SLOW: f32 = 0.55;

thread_local! {
    /// The key of the sliding line the pointer is on (it goes slower), if any.
    static MARQUEE_UNDER: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// A line in a room `width` wide. When it is wider (`line_w`), it slides through the room, softly
/// and over and over: two copies of it end to end (`build` makes one) move left by one copy and
/// the gap, slowing to a near stop at each end, so it comes round without a jump. Each copy sits
/// in a slot exactly `line_w` wide, so the second one lands where the first started to the point
/// (a copy narrower than its measured width would leave the line short of the start, and it would
/// twitch back as the loop came round). Under the pointer it goes slower. The edges fade, the
/// left one only once the line is moving. `moving` false (a list row the pointer is
/// not on, or less motion asked for) leaves it at its start, faded out at the right edge. A line
/// that fits is just itself.
pub fn marquee(key: impl Into<String>, width: f32, line_w: f32, moving: bool, reduce: bool, build: impl Fn() -> El) -> El {
    let key: String = key.into();
    if !width.is_finite() {
        // Its room isn't measured yet (the first frame of a list): cut off at the room's end.
        return el().key(key).w_full().min_w(0.0).clip_fade(0.0, 0.0, 26.0, 0.0).child(row().child(build()));
    }
    if line_w <= width + 0.5 {
        return build();
    }
    let slide = moving && !reduce;
    if !slide {
        let strip = row().key(format!("{key}-strip")).w(line_w).items_center().child(build());
        return el().key(key).w(width).min_w(0.0).clip_fade(0.0, 0.0, 26.0, 0.0).child(strip);
    }
    // The left fade can't come and go with the slide, so it lies outside the line's room: the
    // clip reaches `LEAD` points to the left, the strip starts with as much empty space, and a
    // line at rest sits clear of the fade; only a line on its way out passes through it.
    const LEAD: f32 = 14.0;
    let travel = line_w + MARQUEE_GAP;
    let strip_w = LEAD + 2.0 * line_w + MARQUEE_GAP;
    // Slower under the pointer. The loop keeps its place when its speed changes (the slide is
    // retimed, not restarted), so the line doesn't jump as the pointer comes or goes.
    let slow = MARQUEE_UNDER.with_borrow(|k| k.as_deref() == Some(key.as_str()));
    let speed = if slow { MARQUEE_SPEED * MARQUEE_SLOW } else { MARQUEE_SPEED };
    let period = f64::from(travel / speed * 1.25 + 1.1);
    // The slide is in parts of the strip's own width. It gets going quickly and comes to rest
    // slowly, so the line is read from its start while it waits rather than while it creeps.
    let spec = rune::LoopSpec::slide(period, [0.0, 0.0], [-travel / strip_w, 0.0]).easing(rune::Easing::cubic(0.36, 0.0, 0.2, 1.0));
    let copy = || row().w(line_w).min_w(line_w).items_center().child(build());
    let strip = row().key(format!("{key}-strip")).w(strip_w).pl(LEAD).gap(MARQUEE_GAP).items_center().child(copy()).child(copy()).looping(spec);
    let hover_key = key.clone();
    el().key(key)
        .w(width + LEAD)
        .ml(-LEAD)
        .min_w(0.0)
        .clip_fade(LEAD, 0.0, 26.0, 0.0)
        .on_hover_change(move |_s: &mut State, on: bool, _tx: &mut Tx| {
            MARQUEE_UNDER.with_borrow_mut(|k| {
                if on {
                    *k = Some(hover_key.clone());
                } else if k.as_deref() == Some(hover_key.as_str()) {
                    *k = None;
                }
            });
        })
        .child(strip)
}

/// A list row's or a card's `key` as the one under the pointer (its lines slide), or not.
pub fn marquee_hover(key: String) -> impl Fn(&mut State, bool, &mut Tx) + 'static {
    move |s: &mut State, on: bool, _tx: &mut Tx| {
        if on {
            if s.marquee_hot.as_deref() != Some(key.as_str()) {
                s.marquee_hot = Some(key.clone());
            }
        } else if s.marquee_hot.as_deref() == Some(key.as_str()) {
            s.marquee_hot = None;
        }
    }
}

thread_local! {
    /// How wide the room for a list row's lines is, by the row's key (`measure_room`), as the
    /// last layout found it.
    static ROOM: std::cell::RefCell<std::collections::HashMap<String, f32>> = std::cell::RefCell::new(std::collections::HashMap::new());
}

/// The room for the lines of the row keyed `key`, once a layout has measured it.
pub fn room(key: &str) -> f32 {
    ROOM.with_borrow(|m| m.get(key).copied()).unwrap_or(f32::INFINITY)
}

/// Measures `e`'s width as the room for the lines of the row keyed `key`. Only a new width
/// builds the view again (a row moving in a scroll does not).
pub fn measure_room(e: El, key: String) -> El {
    e.on_layout_tx(move |_s: &mut State, r: [f32; 4], tx: &mut Tx| {
        let changed = ROOM.with_borrow_mut(|m| {
            let old = m.insert(key.clone(), r[2]);
            old.is_none_or(|o| (o - r[2]).abs() > 1.0)
        });
        if !changed {
            tx.live_only();
        }
    })
}

/// Whether `key` is the row or card under the pointer.
pub fn marquee_on(s: &State, key: &str) -> bool {
    s.marquee_hot.as_deref() == Some(key) && !under()
}
