//! Buttons, tabs, sliders, menu rows, and skeleton placeholders.

use rune::{Color, Cursor, El, Icon, Transition, el, icon, row, text};

use super::{Hover as _, caps, label};
use crate::app::State;
use crate::i18n::{Lang, S};
use crate::theme::{self as th, veil};
use crate::ui::{icons, np_icons};

pub const BTN_H: f32 = 36.0;
pub const FIELD_R: f32 = 10.0;
pub const MENU_ROW: f32 = 29.0;
const MENU_GLYPH: f32 = 15.0;
const TAB_SPRING: rune::Spring = rune::Spring::new(0.26, 0.08);
const TAB_PILL_H: f32 = 28.0;

pub fn ctl_radius(h: f32) -> f32 {
    (h * 0.28).round().clamp(6.0, 12.0)
}

pub fn ibtn(key: impl Into<String>, ic: &'static Icon, glyph: f32, size: f32, color: Color, name: &str) -> El {
    ibtn_with(key, ic, glyph, size, color, name, false)
}

/// Icon button whose glyph fades in with the surrounding hover group without isolating the row.
pub(super) fn ibtn_gop(key: impl Into<String>, ic: &'static Icon, glyph: f32, size: f32, color: Color, name: &str) -> El {
    ibtn_with(key, ic, glyph, size, color, name, true)
}

fn ibtn_with(key: impl Into<String>, ic: &'static Icon, glyph: f32, size: f32, color: Color, name: &str, hover_only: bool) -> El {
    let mut g = icon(ic, glyph).color(color);
    let r = ctl_radius(size);
    let mut b = row()
        .key(key)
        .role("button")
        .label(name)
        .help(name)
        .w(size)
        .h(size)
        .radius(r)
        .center()
        .cursor(Cursor::Pointer)
        .press(veil(0.12))
        .press_scale(th::PRESS);
    if hover_only {
        g = g.gop(0.0, 1.0);
        // Put `.hov` on a leaf sibling so the button doesn't become a hover group owner and hide
        // the row's hover from the glyph.
        b = b.child(el().key("hov").absolute(0.0, 0.0).w_full().h_full().radius(r).hov(veil(0.07)));
    } else {
        b = b.hov(veil(0.07));
    }
    b.child(g)
}

pub(super) fn quiet_halo(glyph: f32) -> El {
    let d = ((glyph + 14.0) * 0.5).round() * 2.0;
    el().absolute(0.0, 0.0).w_full().h_full().center().child(el().w(d).h(d).radius(d * 0.5).fill(veil(0.08)).gop(0.0, 1.0))
}

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

pub fn quiet_toggle(key: impl Into<String>, ic: &'static Icon, glyph: f32, size: f32, on: bool, name: &str) -> El {
    quiet_btn(key, ic, glyph, size, if on { th::text() } else { th::text3() }, name).checked(on)
}

pub fn quiet_off(key: impl Into<String>, ic: &'static Icon, glyph: f32, size: f32, name: &str) -> El {
    row().key(key).role("button").label(name).help(name).disabled(true).w(size).h(size).center().child(icon(ic, glyph).color(veil(0.16)))
}

pub fn skip_btn(s: &State, forward: bool, key: &str, ic: &'static Icon, glyph: f32, size: f32, color: Color) -> El {
    let name = if forward { S::Next } else { S::Previous }.get(s.lang);
    let on = if forward { s.can_next() } else { s.can_prev() };
    if !on {
        return quiet_off(key.to_owned(), ic, glyph, size, name);
    }
    quiet_btn(key.to_owned(), ic, glyph, size, color, name).on_press(move |s: &mut State| if forward { s.next() } else { s.prev() })
}

pub fn swap_glyph(key: String, g: El, box_size: f32, glyph_size: f32, pops: bool) -> El {
    let pop = rune::SpringPair::new(rune::Spring::new(0.34, 0.3), rune::Spring::new(0.1, 0.0));
    let enter = if pops { Transition::scale(0.6).allow_small_scale().combined(Transition::opacity()) } else { Transition::opacity() };
    let off = (box_size - glyph_size) * 0.5;
    g.key(key).absolute(off, off).animated_changes().transition(Transition::asymmetric(enter, Transition::opacity()).springs(pop))
}

pub fn np_play_btn(s: &State, size: f32, glyph: f32) -> El {
    play_btn_with(s, size, glyph, &np_icons::PLAY, &np_icons::PAUSE, "np-play", true)
}

pub fn bar_play_btn(s: &State, size: f32, glyph: f32) -> El {
    play_btn_with(s, size, glyph, &np_icons::PLAY, &np_icons::PAUSE, "play", true)
}

fn play_btn_with(s: &State, size: f32, glyph: f32, play: &'static Icon, pause: &'static Icon, key: &str, quiet: bool) -> El {
    let (g, glyph_side, state) = if s.player.loading {
        let side = glyph * 0.8;
        (super::spin(icon(&icons::SPINNER, side).color(th::text())), side, "load")
    } else if s.player.playing {
        (icon(pause, glyph).color(th::text()), glyph, "pause")
    } else {
        (icon(play, glyph).color(th::text()), glyph, "play")
    };
    let g = swap_glyph(format!("{key}-{}-{state}", size as u32), g, size, glyph_side, true);
    let label_str = if s.player.playing { S::Pause.get(s.lang) } else { S::Play.get(s.lang) };
    let b = row().key(format!("{key}-{}", size as u32)).role("button").label(label_str).help(label_str).w(size).h(size).radius(size * 0.5).center().cursor(Cursor::Pointer);
    let b = if quiet { b.hov_group().child(quiet_halo(glyph)) } else { b.hov(veil(0.08)).press(veil(0.14)) };
    b.press_scale(th::PRESS).on_press(|s: &mut State| s.toggle_pause()).child(g)
}

pub fn field_surface(base: El) -> El {
    base.radius(FIELD_R).fill(veil(0.04)).border(1.0, veil(0.09)).hov(veil(0.02))
}

/// A search box: the same surface and corners as the buttons beside it, a magnifier, the input,
/// and a clear button once there is text. Pages differ only in height.
pub fn search_field(key: impl Into<String>, h: f32, input: El, clear: Option<El>) -> El {
    let big = h > BTN_H;
    let r = ctl_radius(h);
    field_surface(row())
        .key(key)
        .h(h)
        .pl(if big { 14.0 } else { 11.0 })
        .pr(4.0)
        .gap(if big { 10.0 } else { 8.0 })
        .radius(r)
        .clip_rounded(r)
        .items_center()
        .child(icon(&icons::SEARCH, if big { 18.0 } else { 16.0 }).color(th::text3()))
        .child(row().grow(1.0).min_w(0.0).h_full().items_center().clip_rounded(0.0).child(input))
        .children(clear)
}

/// The clear button inside a `search_field`.
pub fn search_clear(key: impl Into<String>, lang: Lang) -> El {
    ibtn(key, &icons::CLOSE, 14.0, 28.0, th::text2(), S::ClearSearch.get(lang))
}

/// A square button on the same surface as a search field, for the controls next to one.
pub fn field_icon_btn(key: impl Into<String>, ic: &'static Icon, name: &str) -> El {
    field_surface(row())
        .key(key)
        .role("button")
        .label(name)
        .help(name)
        .w(BTN_H)
        .h(BTN_H)
        .radius(ctl_radius(BTN_H))
        .center()
        .cursor(Cursor::Pointer)
        .press_scale(th::PRESS)
        .press(veil(0.10))
        .child(icon(ic, 18.0).color(th::text()))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    Solid,
    Soft,
}

fn surface_pill(base: El, weight: Weight, w: f32, h: f32) -> El {
    let radius = ctl_radius(h);
    let b = base.w(w).h(h).radius(radius).cursor(Cursor::Pointer).press_scale(th::PRESS).hov_group();
    let (fill, edge, lift) = match weight {
        Weight::Solid => (th::solid(), None, th::on_solid().alpha(0.10)),
        Weight::Soft => (veil(0.05), Some(veil(0.08)), veil(0.05)),
    };
    let plate = el().absolute(0.0, 0.0).w_full().h_full().radius(radius).fill(fill);
    let edge_plate = edge.map(|color| el().absolute(0.0, 0.0).w_full().h_full().radius(radius).border(1.0, color));
    b.child(plate).children(edge_plate).child(el().absolute(0.0, 0.0).w_full().h_full().radius(radius).fill(lift).gop(0.0, 1.0))
}

fn ink_on(weight: Weight) -> Color {
    match weight {
        Weight::Solid => th::on_solid(),
        Weight::Soft => th::text(),
    }
}

pub fn button(key: impl Into<String>, weight: Weight, name: &str, ic: Option<&'static Icon>) -> El {
    let ink = ink_on(weight);
    let mut r = surface_pill(row().key(key).role("button").label(name).gap(8.0).center(), weight, button_width(name, ic.is_some()), BTN_H);
    r = if ic.is_some() { r.pl(12.0).pr(16.0) } else { r.px(16.0) };
    if let Some(i) = ic {
        r = r.child(icon(i, 15.0).color(ink).ml(1.0));
    }
    r.child(text(name.to_owned()).font(th::BTN).color(ink))
}

pub fn button_width(name: &str, glyph: bool) -> f32 {
    let tw = super::measure_w(name, th::BTN).ceil();
    (if glyph { 12.0 + 15.0 + 8.0 + 1.0 + tw + 16.0 } else { 16.0 + tw + 16.0 }) + 2.0
}

pub fn bone_fill() -> Color {
    veil(0.05)
}

pub fn text_bone(key: impl Into<String>, line: f32, ink: f32, w: f32) -> El {
    row().key(key).w_full().max_w(w).h(line).items_center().child(el().grow(1.0).h(ink).radius(ink * 0.5).fill(bone_fill()))
}

pub fn button_bone(key: impl Into<String>, w: f32) -> El {
    el().key(key).w(w).h(BTN_H).radius(ctl_radius(BTN_H)).fill(bone_fill())
}

pub fn btn_primary(key: impl Into<String>, name: &str, ic: Option<&'static Icon>) -> El {
    button(key, Weight::Solid, name, ic)
}

pub fn btn_soft(key: impl Into<String>, name: &str, ic: Option<&'static Icon>) -> El {
    button(key, Weight::Soft, name, ic)
}

/// A square soft button with only an icon, as tall as the other buttons.
pub fn btn_soft_icon(key: impl Into<String>, ic: &'static Icon, name: &str) -> El {
    surface_pill(row().key(key).role("button").label(name).help(name).center(), Weight::Soft, BTN_H, BTN_H).child(icon(ic, 16.0).color(th::text()))
}

pub fn btn_small(key: impl Into<String>, name: &str, ic: Option<&'static Icon>) -> El {
    let tw = super::measure_w(name, th::BODY_M).ceil();
    let w = if ic.is_some() { 10.0 + 15.0 + 7.0 + tw + 12.0 } else { 12.0 + tw + 12.0 };
    let mut r = surface_pill(row().key(key).role("button").label(name).gap(7.0).center(), Weight::Soft, w + 2.0, 32.0);
    r = if ic.is_some() { r.pl(10.0).pr(12.0) } else { r.px(12.0) };
    if let Some(i) = ic {
        r = r.child(icon(i, 15.0).color(th::text()));
    }
    r.child(text(name.to_owned()).font(th::BODY_M).color(th::text()))
}

/// Disabled companion to `btn_small` styled with muted colors instead of group opacity.
pub fn btn_small_disabled(key: impl Into<String>, name: &str, ic: Option<&'static Icon>) -> El {
    let tw = super::measure_w(name, th::BODY_M).ceil();
    let w = if ic.is_some() { 10.0 + 15.0 + 7.0 + tw + 12.0 } else { 12.0 + tw + 12.0 };
    let radius = ctl_radius(32.0);
    let ink = th::text().alpha(0.42);
    let mut r = row().key(key).role("button").label(name).disabled(true).w(w + 2.0).h(32.0).gap(7.0).center().radius(radius).fill(veil(0.025)).border(1.0, veil(0.045));
    r = if ic.is_some() { r.pl(10.0).pr(12.0) } else { r.px(12.0) };
    if let Some(i) = ic {
        r = r.child(icon(i, 15.0).color(ink));
    }
    r.child(text(name.to_owned()).font(th::BODY_M).color(ink))
}

pub fn btn_select(key: impl Into<String>, current: &str, w: f32, open: bool) -> El {
    surface_pill(row().key(key).role("button").label(current).pl(12.0).pr(9.0).gap(8.0).items_center(), Weight::Soft, w, 32.0)
        .child(text(current.to_owned()).font(th::BODY_M).color(th::text()).truncate().grow(1.0).min_w(0.0))
        .child(super::chevron(15.0, open, th::text3()).gcol(th::text2()).motion(th::FLIGHT))
}

/// Buttons that belong together, joined into one: a shared surface, as tall as a button, with a
/// thin line between each two. Each part is one of the `seg_*` buttons below.
pub fn button_group(key: impl Into<String>, parts: Vec<El>) -> El {
    let r = ctl_radius(BTN_H);
    let n = parts.len();
    let mut g = row().key(key).h(BTN_H).radius(r).fill(veil(0.05)).border(1.0, veil(0.08)).items_center();
    for (i, part) in parts.into_iter().enumerate() {
        if i > 0 {
            g = g.child(el().w(1.0).h(BTN_H * 0.5).fill(veil(0.10)));
        }
        let (first, last) = (i == 0, i + 1 == n);
        // The parts light up on hover; at the group's ends they follow its rounded corners.
        let corners = [if first { r } else { 0.0 }, if last { r } else { 0.0 }, if last { r } else { 0.0 }, if first { r } else { 0.0 }];
        g = g.child(part.corners(corners));
    }
    g
}

/// A part of a `button_group` with only an icon.
pub fn seg_icon(key: impl Into<String>, ic: &'static Icon, name: &str) -> El {
    seg(row().key(key).role("button").label(name).help(name).w(BTN_H).center()).child(icon(ic, 18.0).color(th::text()))
}

/// A part of a `button_group` with words, and an icon before them.
pub fn seg_button(key: impl Into<String>, name: &str, ic: Option<&'static Icon>) -> El {
    let mut b = seg(row().key(key).role("button").label(name).gap(8.0).items_center());
    b = if ic.is_some() { b.pl(12.0).pr(14.0) } else { b.px(14.0) };
    if let Some(i) = ic {
        b = b.child(icon(i, 15.0).color(th::text()));
    }
    b.child(text(name.to_owned()).font(th::BTN).color(th::text()))
}

/// What every part of a group shares: its height, the pointer, and a light on hover and press.
fn seg(base: El) -> El {
    base.h(BTN_H).cursor(Cursor::Pointer).hov(veil(0.05)).press(veil(0.09)).hov_group()
}

/// The sort button as a part of a `button_group`: the icon, and the order once it isn't the
/// original one.
pub fn seg_sort(key: &str, s: &State, now: Option<&str>, open: bool) -> El {
    let name = S::SortBy.get(s.lang);
    let mut b = seg(row().key(key.to_owned()).role("button").label(name).help(name).checked(open).w(sort_button_w(now)).gap(SORT_GAP).center())
        .child(icon(&icons::SORT, SORT_GLYPH).color(if now.is_some() || open { th::text() } else { th::text2() }).gcol(th::text()));
    if open {
        b = b.fill(veil(0.09));
    }
    if let Some(n) = now {
        b = b.child(label(n.to_owned(), th::BODY_M, th::text2()).gcol(th::text()));
    }
    b
}

/// Compact 30pt round sort pill matching horizontal carousel arrows (`row_arrows`).
pub fn sort_button_sm(key: &str, s: &State, now: Option<&str>, open: bool) -> El {
    const H: f32 = 30.0;
    const GLYPH: f32 = 15.0;
    const GAP: f32 = 5.0;
    const PAD: f32 = 10.0;
    let name = S::SortBy.get(s.lang);
    let w = match now {
        None => H,
        Some(n) => PAD * 2.0 + GLYPH + GAP + text::measure(n, &[], th::SMALL, None).0.ceil(),
    };
    let mut b = row()
        .key(key.to_owned())
        .role("button")
        .label(name)
        .help(name)
        .checked(open)
        .w(w)
        .h(H)
        .gap(GAP)
        .radius(H * 0.5)
        .center()
        .fill(veil(if open { 0.12 } else { 0.06 }))
        .border(1.0, veil(if open { 0.14 } else { 0.07 }))
        .cursor(Cursor::Pointer)
        .hov(veil(0.12))
        .press(veil(0.16))
        .press_scale(th::PRESS)
        .hov_group()
        .child(icon(&icons::SORT, GLYPH).color(if now.is_some() || open { th::text() } else { th::text2() }).gcol(th::text()));
    if let Some(n) = now {
        b = b.child(label(n.to_owned(), th::SMALL, th::text2()).gcol(th::text()));
    }
    b
}

pub fn sort_button(key: &str, s: &State, now: Option<&str>, open: bool) -> El {
    let name = S::SortBy.get(s.lang);
    let mut b = field_surface(row())
        .key(key.to_owned())
        .role("button")
        .label(name)
        .help(name)
        .checked(open)
        .w(sort_button_w(now))
        .h(BTN_H)
        .gap(SORT_GAP)
        .radius(ctl_radius(BTN_H))
        .center()
        .cursor(Cursor::Pointer)
        .press(veil(0.10))
        .hov_group()
        .child(icon(&icons::SORT, SORT_GLYPH).color(if now.is_some() || open { th::text() } else { th::text2() }).gcol(th::text()));
    if open {
        b = b.fill(veil(0.09));
    }
    if let Some(n) = now {
        b = b.child(label(n.to_owned(), th::BODY_M, th::text2()).gcol(th::text()));
    }
    b
}

const SORT_GLYPH: f32 = 17.0;
const SORT_GAP: f32 = 6.0;
const SORT_PAD: f32 = 12.0;

/// How wide `sort_button` is: a square with only the icon, wider once it names the order.
pub fn sort_button_w(now: Option<&str>) -> f32 {
    match now {
        None => BTN_H,
        Some(n) => SORT_PAD * 2.0 + SORT_GLYPH + SORT_GAP + text::measure(n, &[], th::BODY_M, None).0.ceil(),
    }
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

pub fn plus_badge(lang: Lang) -> El {
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
        .child(caps(S::Plus.get(lang), th::rgb(0xffffff)))
}

pub fn tab_body(key: impl Into<String>, body: El) -> El {
    let springs = rune::SpringPair::new(rune::Spring::new(0.18, 0.0), rune::Spring::new(0.06, 0.0));
    el().key(key).w_full().transition(Transition::asymmetric(Transition::offset(0.0, 5.0), Transition::none()).springs(springs)).child(body)
}

pub fn tab_pill_w(name: &str, count: Option<usize>) -> f32 {
    let mut w = 24.0 + super::measure_w(name, th::BODY_M).ceil();
    if let Some(n) = count {
        w += 6.0 + super::measure_w(&n.to_string(), th::SMALL).ceil();
    }
    w
}

pub fn tab_pill_with_w(key: impl Into<String>, name: &str, count: Option<usize>, on: bool, width: f32) -> El {
    let ink = if on { th::text() } else { th::text2() };
    let mut r = row()
        .key(key)
        .role("tab")
        .label(name)
        .checked(on)
        .w(width)
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

pub fn tab_track(key: impl Into<String>, tabs: Vec<(El, f32)>, at: usize) -> El {
    let key: String = key.into();
    let at = at.min(tabs.len().saturating_sub(1));
    let x = 4.0 + tabs[..at].iter().map(|(_, w)| w + 2.0).sum::<f32>();
    let pw = tabs.get(at).map_or(0.0, |(_, w)| *w);
    let total = 8.0 + tabs.iter().map(|(_, w)| *w).sum::<f32>() + 2.0 * tabs.len().saturating_sub(1) as f32;
    let mut plate = el().key(format!("{key}-plate")).absolute(x, 4.0).w(pw).h(TAB_PILL_H).radius(7.0).fill(if th::light() { th::pop() } else { veil(0.10) }).motion(TAB_SPRING);
    if th::light() {
        plate = plate.border(1.0, veil(0.10));
    }
    let items = row().absolute(0.0, 0.0).h(36.0).px(4.0).gap(2.0).items_center().children(tabs.into_iter().map(|(e, _)| e));
    el().key(key).w(total).h(36.0).radius(10.0).fill(veil(0.04)).border(1.0, veil(0.07)).child(plate).child(items)
}

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
