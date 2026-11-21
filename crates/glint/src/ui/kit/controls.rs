//! Buttons, tabs, sliders, menu rows, and skeleton placeholders.

use rune::{Color, Cursor, Drag, El, Icon, Phase, Transition, Tx, el, icon, row, text};

use super::{Hover as _, caps, label};
use crate::app::State;
use crate::i18n::{Lang, S};
use crate::theme::{self as th, veil};
use crate::ui::{icons, np_icons};

pub const BTN_H: f32 = 36.0;
pub const FIELD_R: f32 = 10.0;
pub const MENU_ROW: f32 = 32.0;
const MENU_GLYPH: f32 = 16.0;
const TAB_SPRING: rune::Spring = rune::Spring::new(0.36, 0.12);
const TAB_PILL_H: f32 = 28.0;

pub fn ctl_radius(h: f32) -> f32 {
    (h * 0.28).round().clamp(6.0, 12.0)
}

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

pub(super) fn quiet_halo(glyph: f32) -> El {
    let d = (glyph + 14.0).round();
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

pub(super) fn swap_glyph(key: String, g: El, pops: bool) -> El {
    let pop = rune::SpringPair::new(rune::Spring::new(0.34, 0.3), rune::Spring::new(0.1, 0.0));
    let enter = if pops { Transition::scale(0.6).allow_small_scale().combined(Transition::opacity()) } else { Transition::opacity() };
    el().key(key).absolute(0.0, 0.0).w_full().h_full().center().transition(Transition::asymmetric(enter, Transition::opacity()).springs(pop)).child(g)
}

pub fn np_play_btn(s: &State, size: f32, glyph: f32) -> El {
    play_btn_with(s, size, glyph, &np_icons::PLAY, &np_icons::PAUSE, "np-play", true)
}

pub fn bar_play_btn(s: &State, size: f32, glyph: f32) -> El {
    play_btn_with(s, size, glyph, &np_icons::PLAY, &np_icons::PAUSE, "play", true)
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
    let label_str = if s.player.playing { S::Pause.get(s.lang) } else { S::Play.get(s.lang) };
    let b = row().key(format!("{key}-{}", size as u32)).role("button").label(label_str).help(label_str).w(size).h(size).radius(size * 0.5).center().cursor(Cursor::Pointer);
    let b = if quiet { b.hov_group().child(quiet_halo(glyph)) } else { b.hov(veil(0.08)).press(veil(0.14)) };
    b.press_scale(th::PRESS).on_press(|s: &mut State| s.toggle_pause()).child(g)
}

pub fn field_surface(base: El) -> El {
    base.radius(FIELD_R).fill(veil(0.04)).border(1.0, veil(0.09)).hov(veil(0.02))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    Solid,
    Soft,
}

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
    let tw = text::measure(name, &[], th::BTN, None).0.ceil();
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

pub fn btn_small(key: impl Into<String>, name: &str, ic: Option<&'static Icon>) -> El {
    let tw = text::measure(name, &[], th::BODY_M, None).0.ceil();
    let w = if ic.is_some() { 10.0 + 15.0 + 7.0 + tw + 12.0 } else { 12.0 + tw + 12.0 };
    let mut r = surface_pill(row().key(key).role("button").label(name).gap(7.0).center(), Weight::Soft, w + 2.0, 32.0);
    r = if ic.is_some() { r.pl(10.0).pr(12.0) } else { r.px(12.0) };
    if let Some(i) = ic {
        r = r.child(icon(i, 15.0).color(th::text()));
    }
    r.child(text(name.to_owned()).font(th::BODY_M).color(th::text()))
}

pub fn btn_select(key: impl Into<String>, current: &str, w: f32, open: bool) -> El {
    surface_pill(row().key(key).role("button").label(current).pl(12.0).pr(9.0).gap(8.0).items_center(), Weight::Soft, w, 32.0)
        .child(text(current.to_owned()).font(th::BODY_M).color(th::text()).truncate().grow(1.0).min_w(0.0))
        .child(icon(&icons::CHEVRON_DOWN, 15.0).color(th::text3()).gcol(th::text2()).rotate(if open { std::f32::consts::PI } else { 0.0 }).motion(th::FLIGHT))
}

pub fn round(key: impl Into<String>, weight: Weight, ic: &'static Icon, size: f32, glyph: f32, name: &str) -> El {
    surface_pill(row().key(key).role("button").label(name).help(name).center(), weight, size, size).child(icon(ic, glyph).color(ink_on(weight)))
}

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
    let enter = Transition::opacity().combined(Transition::offset(0.0, 10.0));
    let springs = rune::SpringPair::new(rune::Spring::new(0.26, 0.0), rune::Spring::new(0.07, 0.0));
    el().key(key).w_full().transition(Transition::asymmetric(enter, Transition::opacity()).springs(springs)).enter_delay(0.07).child(body)
}

pub fn tab_pill_w(name: &str, count: Option<usize>) -> f32 {
    let mut w = 24.0 + text::measure(name, &[], th::BODY_M, None).0.ceil();
    if let Some(n) = count {
        w += 6.0 + text::measure(&n.to_string(), &[], th::SMALL, None).0.ceil();
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
        plate = plate.shadow(1.0, 3.0, th::rgba(0x000000, 0.10));
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
