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
    BTN_H, FIELD_R, MENU_ROW, bar_play_btn, bone_fill, btn_primary, btn_select, btn_small, btn_small_disabled, btn_soft, btn_soft_icon, button_bone, button_group, button_width,
    field_icon_btn, field_surface, ibtn, menu_row, more_link, np_play_btn, plus_badge, quiet_btn, quiet_off, quiet_toggle, search_clear, search_field, seg_button,
    seg_icon, seg_sort, skip_btn, sort_button, sort_button_sm, swap_glyph, tab_body, tab_pill_w, tab_pill_with_w, tab_track, text_bone,
};
pub use marquee::{marquee, marquee_hover, marquee_on};
pub use media::{
    artist_line_width, artists_row_in_width, cover, cover_border, cover_placeholder, cover_sm, cover_tile, picture_url, playing_cover, playing_cover_lg, roll, seg_wave,
    source_glyph, track_title, wave_card_alpha, wave_glyph, wheel_art,
};
pub use rows::{ListSrc, ROW_COVER, ROW_COVER_R, ROW_H, ROW_PAD, ROW_TIME_INSET, bleed, explicit_badge, np_like_button, track_bones, track_fields, queue_row, track_row, track_row_lazy};

pub const TITLE_H: f32 = 60.0;
pub const CAPTION_H: f32 = if cfg!(target_os = "windows") { 44.0 } else { TITLE_H };
pub const RAIL_W: f32 = 60.0;
pub const CHROME: f32 = 40.0;
pub const CHROME_INSET: f32 = (TITLE_H - CHROME) * 0.5;
pub const BAR_H: f32 = 72.0;
pub const BAR_GAP: f32 = 14.0;
pub const DOCK_ZONE: f32 = BAR_H + BAR_GAP + 48.0;
pub const PAGE_PAD: f32 = 32.0;
pub const PAGE_MAX: f32 = 960.0;
pub const BTN: f32 = 40.0;

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

/// Cached single-line text width measurement to avoid repeated shaping during commits.
pub fn measure_w(s: &str, style: TextStyle) -> f32 {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::hash::{Hash, Hasher};

    thread_local! {
        static CACHE: RefCell<HashMap<u64, f32>> = RefCell::new(HashMap::new());
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    style.family.hash(&mut h);
    style.size.to_bits().hash(&mut h);
    style.weight.to_bits().hash(&mut h);
    style.tracking.to_bits().hash(&mut h);
    style.line_height.to_bits().hash(&mut h);
    let key = h.finish();
    if let Some(w) = CACHE.with_borrow(|c| c.get(&key).copied()) {
        return w;
    }
    let w = text::measure(s, &[], style, None).0;
    CACHE.with_borrow_mut(|c| {
        if c.len() > 512 {
            c.clear();
        }
        c.insert(key, w);
    });
    w
}

/// Animates a loading spinner without creating a rotated layer in CPU mode (which would trigger
/// a GPU handover in RUNE's CPU renderer).
pub fn spin(e: El) -> El {
    if fx::cpu_mode() {
        e.looping(rune::LoopSpec::opacity(1.1, 0.0, 0.35, 1.0))
    } else {
        e.looping(rune::LoopSpec::rotation(0.9))
    }
}

/// A downward chevron that flips upward when `open`, swapping glyph instead of rotating in CPU mode.
pub fn chevron(size: f32, open: bool, color: Color) -> El {
    if fx::cpu_mode() {
        rune::icon(if open { &super::icons::CHEVRON_UP } else { &super::icons::CHEVRON_DOWN }, size).color(color)
    } else {
        rune::icon(&super::icons::CHEVRON_DOWN, size).color(color).rotate(if open { std::f32::consts::PI } else { 0.0 })
    }
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
    glass_alpha(kind, radius, 1.0)
}

pub fn glass_alpha(kind: Glass, radius: f32, alpha: f32) -> El {
    let alpha = alpha.clamp(0.0, 1.0);
    let pane = el().absolute(0.0, 0.0).w_full().h_full().radius(radius).transition(Transition::none());
    if fx::glass_enabled() {
        return pane.effect(fx::glass(kind)).fx_data([1.0 - alpha, 0.0, 0.0, 0.0]);
    }
    let (fill, edge, bw) = match kind {
        Glass::Dock | Glass::Card => (th::pop().alpha(alpha), th::veil(0.11 * alpha), 1.0),
        Glass::Chip => (th::pop().alpha((if th::light() { 0.82 } else { 0.72 }) * alpha), th::veil(0.12 * alpha), 1.0),
        Glass::Menu => (th::pop().alpha(alpha), th::veil(0.0), 0.0),
    };
    let pane = pane.fill(fill);
    if bw > 0.0 { pane.border(bw, edge) } else { pane }
}

pub fn frost(radius: f32, skin: f32) -> [El; 2] {
    let film = if fx::glass_enabled() { th::FROST } else { 1.0 };
    [
        glass_alpha(Glass::Menu, radius, skin),
        el().absolute(0.0, 0.0).w_full().h_full().radius(radius).fill(th::pop().alpha(film * skin)).border(1.0, th::veil(0.12 * skin)).transition(Transition::none()),
    ]
}

pub fn veil_bottom(h: f32) -> El {
    fx::veil_slot(el().key("veil-bottom").pin(Some(0.0), None, Some(0.0), Some(0.0)).h(h).transition(Transition::none()), [0.0, h, 20.0, 0.38])
}

// ---- page furniture & dialogs ------------------------------------------------------------------------

pub fn empty(p: &Pal, art: Ill, msg: &str) -> El {
    col().w_full().py(40.0).gap(16.0).items_center().child(shapes::ill(art, 140.0, p.accent, 0.08)).child(label(msg.to_owned(), th::BODY, th::text2()).align(Align::Center))
}

pub fn dialog(key: &str, w: f32, title: &str, body: Vec<El>, buttons: Vec<El>, dismiss: fn(&mut State)) -> El {
    dialog_head(key, w, title, None, body, buttons, dismiss)
}

pub fn dialog_head(key: &str, w: f32, title: &str, badge: Option<El>, body: Vec<El>, buttons: Vec<El>, dismiss: fn(&mut State)) -> El {
    let width = 428.0_f32.min(w - 32.0);
    let head = match badge {
        Some(b) => row()
            .w_full()
            .gap(10.0)
            .items_center()
            .justify_between()
            .child(label(title.to_owned(), th::H2, th::text()).shrink().min_w(0.0))
            .child(b),
        None => label(title.to_owned(), th::H2, th::text()).wrap(),
    };
    let card_tr = if fx::cpu_mode() {
        Transition::none()
    } else {
        Transition::asymmetric(
            Transition::scale(0.95).combined(Transition::offset(0.0, 10.0)).combined(Transition::opacity()),
            Transition::scale(0.97).combined(Transition::offset(0.0, 4.0)).combined(Transition::opacity()),
        )
        .springs(th::RISE)
    };
    let card = col()
        .key(format!("{key}-card"))
        .role("dialog")
        .label(title.to_owned())
        .w(width)
        .padding(20.0)
        .gap(12.0)
        .radius(16.0)
        .fill(th::pop())
        .border(1.0, th::veil(0.12))
        .transition(card_tr)
        .modal(true, dismiss)
        .on_press(|_s: &mut State| {})
        .child(head.focusable(true).tab_index(-1).autofocus())
        .children(body)
        .child(row().w_full().pt(6.0).gap(10.0).justify_end().children(buttons));
    let scrim_tr = if fx::cpu_mode() { Transition::none() } else { Transition::opacity().springs(th::RISE) };
    el().key(key.to_owned())
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .center()
        .fill(th::rgba(0x000000, if th::light() { 0.24 } else { 0.52 }))
        .transition(scrim_tr)
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
    let mut r = row().key(key).w_full().h(36.0).items_center().gap(12.0).child(label(title.to_owned(), th::SECTION, th::text()).truncate().min_w(0.0)).child(el().grow(1.0));
    if let Some(m) = more {
        r = r.child(m);
    }
    r
}

pub fn list_heading(key: impl Into<String>, title: &str, more: Option<El>) -> El {
    heading(key, title, more).pl(ROW_PAD)
}

pub fn scroll_page(key: impl Into<String>, body: El) -> El {
    page_shell(key, body.transition(Transition::asymmetric(Transition::offset(0.0, 10.0), Transition::none()).springs(th::PAGE_IN)))
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
        .on_scroll_offset(move |p: rune::ScrollPos| crate::app::report_scroll(&watch_key, p))
        .transition(Transition::none())
        .child(row().w_full().justify_center().transition(Transition::none()).child(body.w_full().max_w(PAGE_MAX).px(PAGE_PAD).pt(TITLE_H + 8.0).pb(foot_room()).gap(32.0)))
}

const SCROLLBAR_HIT_W: f32 = 14.0;
const SCROLLBAR_THUMB_W: f32 = 5.0;
const SCROLLBAR_RIGHT: f32 = 3.0;

/// Slim interactive vertical scrollbar bound to a scroll container's `on_scroll_offset` key.
/// Moves its thumb via `Live<[f32; 4]>` so scrolling never triggers a view rebuild.
fn scrollbar(scroll_key: &str, vp_h: f32, top_pad: f32, bottom_pad: f32) -> El {
    let track_h = (vp_h - top_pad - bottom_pad).max(40.0);
    let thumb_x = SCROLLBAR_HIT_W - SCROLLBAR_THUMB_W - SCROLLBAR_RIGHT;
    let live = crate::app::scroll_thumb(scroll_key, vp_h, track_h, thumb_x, SCROLLBAR_THUMB_W);
    let drag_key = scroll_key.to_owned();
    let thumb =
        el().key(format!("sb-thumb-{scroll_key}")).radius(SCROLLBAR_THUMB_W * 0.5).fill(th::veil(0.24)).gcol(th::veil(0.46)).live_rect(&live).transition(Transition::none());
    el().key(format!("sb-{scroll_key}"))
        .pin(None, Some(top_pad), Some(0.0), Some(bottom_pad))
        .w(SCROLLBAR_HIT_W)
        .hov_group()
        .transition(Transition::none())
        .on_drag(move |s: &mut State, d: &mut rune::Drag, tx: &mut Tx| {
            let Some([offset, max]) = crate::app::page_scroll(&drag_key) else { return };
            if max <= 2.0 || d.cancelled() {
                return;
            }
            let th = (d.h * (vp_h / (vp_h + max))).clamp(28.0, d.h);
            let span = (d.h - th).max(1.0);
            match d.phase {
                rune::Phase::Down => {
                    let thumb_y = (offset / max).clamp(0.0, 1.0) * span;
                    if d.y >= thumb_y - 4.0 && d.y <= thumb_y + th + 4.0 {
                        d.data = d.y - thumb_y;
                        s.glide.active = false;
                        s.scroll_restore = None;
                    } else {
                        d.data = -1.0;
                    }
                }
                rune::Phase::Move => {
                    if d.data < 0.0 {
                        return;
                    }
                    s.glide.active = false;
                    s.scroll_restore = None;
                    let frac = ((d.y - d.data) / span).clamp(0.0, 1.0);
                    tx.scroll_to_instant(&drag_key, 0.0, frac * max);
                }
                rune::Phase::Up => {}
            }
        })
        .child(thumb)
}

/// Interactive scrollbar for the active main page, mounted above the edge blur veils.
pub fn page_scrollbar(h: f32) -> El {
    let key = crate::app::PAGE_KEY.with_borrow(Clone::clone);
    if key.is_empty() {
        return el().key("page-sb-none");
    }
    scrollbar(&key, h, CAPTION_H + 8.0, 12.0)
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
    e.transition(Transition::asymmetric(Transition::offset(0.0, 10.0), Transition::none()).springs(th::PAGE_IN)).enter_delay(0.04 * i.min(6) as f32)
}

/// Scale/offset enter transition without opacity fade so modals and pills avoid offscreen layers.
pub fn pop_in(scale: f32, dy: f32, springs: rune::SpringPair) -> Transition {
    Transition {
        insertion: rune_motion::Effect { scale, offset: rune_motion::Vec2::new(0.0, dy), ..rune_motion::Effect::IDENTITY },
        removal: rune_motion::Effect::IDENTITY,
        pop_layout: true,
        springs,
    }
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
