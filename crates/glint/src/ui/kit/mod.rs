//! Shared UI primitives: layout constants, hover suppression, compositor slots, controls, media,
//! track rows, and marquee scrolling.

mod controls;
mod marquee;
mod media;
mod rows;

use std::cell::Cell;

use crate::fx::Slot as _;
use rune::{Align, Color, El, ScrollSpec, TextStyle, Transition, Tx, col, el, row, text};

use super::shapes::{self, Ill};
use crate::app::{Load, State};
use crate::fx::{self, Glass};
use crate::i18n::S;
use crate::theme::{self as th, Pal};

pub use controls::{
    BTN_H, FIELD_R, MENU_ROW, Weight, bar_play_btn, bone_fill, btn_primary, btn_select, btn_small, btn_soft, button_bone, button_width, ctl_radius, field_surface, ibtn, menu_row,
    more_link, np_play_btn, pill_icon, plus_badge, quiet_btn, quiet_off, quiet_toggle, round, skip_btn, slider, sort_button, tab_body, tab_pill_w, tab_pill_with_w, tab_track,
    text_bone,
};
pub use marquee::{marquee, marquee_hover, marquee_on};
pub use media::{artist_line_width, artists_row_in_width, cover, cover_border, cover_tile, picture_url, playing_cover, roll, track_title, wave_button, wave_card, wave_glyph};
pub use rows::{ListSrc, ROW_COVER, ROW_COVER_R, ROW_H, ROW_PAD, explicit_badge, np_like_button, queue_row, track_row};

pub const TITLE_H: f32 = 60.0;
pub const CAPTION_H: f32 = if cfg!(target_os = "windows") { 44.0 } else { TITLE_H };
pub const RAIL_W: f32 = 60.0;
pub const CHROME: f32 = 40.0;
pub const CHROME_INSET: f32 = (TITLE_H - CHROME) * 0.5;
pub const BAR_H: f32 = 72.0;
pub const BAR_GAP: f32 = 14.0;
pub const DOCK_ZONE: f32 = BAR_H + BAR_GAP + 48.0;
pub const PAGE_PAD: f32 = 36.0;
pub const PAGE_MAX: f32 = 960.0;
pub const BTN: f32 = 40.0;
pub const GLYPH: f32 = 22.0;

thread_local! {
    static BAR_ON: Cell<bool> = const { Cell::new(false) };
    static UNDER: Cell<bool> = const { Cell::new(false) };
}

pub fn set_bar_on(on: bool) {
    BAR_ON.with(|c| c.set(on));
}

pub fn foot_room() -> f32 {
    if BAR_ON.with(Cell::get) { DOCK_ZONE } else { 56.0 }
}

pub fn set_under(on: bool) {
    UNDER.with(|u| u.set(on));
}

pub fn under() -> bool {
    UNDER.with(Cell::get)
}

/// Runs `f` with `under` temporarily set to `on` (used by virtualized row builders).
pub fn built_under<T>(on: bool, f: impl FnOnce() -> T) -> T {
    let was = under();
    set_under(on);
    let out = f();
    set_under(was);
    out
}

/// Hover helpers that suppress visual hover styling while the pointer is over a floating bar or menu.
pub trait Hover {
    fn hov(self, c: Color) -> Self;
    fn hov_group(self) -> Self;
    fn gop(self, rest: f32, hovered: f32) -> Self;
    fn gcol(self, c: Color) -> Self;
}

impl Hover for El {
    fn hov(self, c: Color) -> El {
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

/// Small sentence-case label for section headings, metadata kinds, and badges.
pub fn caps(s: impl Into<String>, color: Color) -> El {
    let s: String = s.into();
    let mut c = s.chars();
    let s = match c.next() {
        Some(f) => f.to_uppercase().chain(c.flat_map(char::to_lowercase)).collect(),
        None => s,
    };
    text(s).font(th::LABEL).color(color)
}

// ---- compositor slots --------------------------------------------------------------------------------

pub fn snapshot() -> El {
    fx::snap_slot(el().key("snapshot").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()))
}

pub fn glass(kind: Glass, radius: f32) -> El {
    let pane = el().absolute(0.0, 0.0).w_full().h_full().radius(radius).transition(Transition::none());
    if fx::glass_enabled() { pane.effect(fx::glass(kind)) } else { pane.fill(th::veil(0.06)).border(1.0, th::veil(0.08)) }
}

pub fn frost(radius: f32, skin: f32) -> [El; 2] {
    let film = if fx::glass_enabled() { th::FROST } else { 1.0 };
    [
        glass(Glass::Menu, radius).fx_data([1.0 - skin, 0.0, 0.0, 0.0]),
        el().absolute(0.0, 0.0).w_full().h_full().radius(radius).fill(th::pop().alpha(film * skin)).transition(Transition::none()),
    ]
}

pub fn veil_top(h: f32) -> El {
    fx::veil_slot(el().key("veil-top").absolute(0.0, 0.0).w_full().h(h).transition(Transition::none()), [h, 0.0, 20.0, 0.38])
}

pub fn veil_bottom(h: f32) -> El {
    fx::veil_slot(el().key("veil-bottom").pin(Some(0.0), None, Some(0.0), Some(0.0)).h(h).transition(Transition::none()), [0.0, h, 20.0, 0.38])
}

// ---- page furniture & dialogs ------------------------------------------------------------------------

pub fn empty(p: &Pal, art: Ill, msg: &str) -> El {
    col().w_full().py(40.0).gap(16.0).items_center().child(shapes::ill(art, 140.0, p.accent, 0.08)).child(label(msg.to_owned(), th::BODY, th::text2()).align(Align::Center))
}

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

pub fn page_title(title: &str, lead: &str) -> El {
    let mut c = col().gap(6.0).child(label(title.to_owned(), th::TITLE, th::text()).max_lines(2));
    if !lead.is_empty() {
        c = c.child(label(lead.to_owned(), th::LEAD, th::text2()));
    }
    c
}

pub fn heading(key: impl Into<String>, title: &str, more: Option<El>) -> El {
    let mut r = row().key(key).w_full().h(32.0).items_center().gap(12.0).child(label(title.to_owned(), th::SECTION, th::text()).truncate().min_w(0.0)).child(el().grow(1.0));
    if let Some(m) = more {
        r = r.child(m);
    }
    r
}

pub fn list_heading(key: impl Into<String>, title: &str, more: Option<El>) -> El {
    heading(key, title, more).pl(8.0).pr(4.0)
}

pub fn scroll_page(key: impl Into<String>, body: El) -> El {
    page_shell(key, body.transition(Transition::opacity().combined(Transition::offset(0.0, 14.0)).combined(Transition::scale(0.99)).springs(th::PAGE_IN)))
}

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
        .on_wheel(move |s: &mut State, e: rune::WheelEvent, tx: &mut Tx| {
            s.scroll_restore = None;
            if e.precise || e.mods.ctrl || e.mods.cmd || e.dy == 0.0 || e.dx.abs() > e.dy.abs() {
                s.glide.active = false;
                return false;
            }
            let (now, reduce) = (s.now, s.settings.reduce_motion);
            s.glide.wheel_to(&wheel_key, e.dy, now, reduce, tx);
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

pub fn wheel_to_page(e: El) -> El {
    e.on_wheel(|s: &mut State, ev: rune::WheelEvent, tx: &mut Tx| {
        if ev.precise || ev.mods.ctrl || ev.mods.cmd || ev.mods.shift || ev.dy == 0.0 || ev.dx.abs() > ev.dy.abs() {
            return false;
        }
        let (now, reduce) = (s.now, s.settings.reduce_motion);
        s.glide.wheel_page(ev.dy, now, reduce, tx)
    })
}

pub fn rise(e: El, i: usize) -> El {
    e.transition(Transition::opacity().combined(Transition::offset(0.0, 12.0)).springs(th::PAGE_IN)).enter_delay(0.04 * i.min(6) as f32)
}

pub fn list_state<T>(l: &Load<T>, s: &State) -> Option<El> {
    match l {
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
        Load::Idle | Load::Loading | Load::Ready(_) => None,
    }
}
