//! Context menus, popovers, confirmation dialogs, and the dock volume card.
//!
//! Each menu springs out of its anchor (a button rect or click point) driven by one `Pop` spring
//! per surface so reversing mid-flight never jumps.

mod dialogs;
mod panels;
mod popovers;
mod track;

use rune::{Cx, El, Icon, PopupAnchor, Transition, col, el, row};

use super::icons;
use super::kit::{self, MENU_ROW, label, menu_row};
use super::np;
use crate::app::{Popover, Session, State};
use crate::i18n::S;
use crate::theme as th;

pub use dialogs::{delete_download_confirm, delete_playlist_confirm, download_confirm, name_dialog};
pub use panels::{look_changed, lyric_rows, reset_look, set_stage_value, stage_rows, stage_value};
pub use track::{overlay, track_link};

pub(super) const W: f32 = 248.0;
const RADIUS: f32 = 12.0;
pub(super) const PAD: f32 = 4.0;
pub(super) const EDGE: f32 = 12.0;
const GAP: f32 = 6.0;
const DIVIDER: f32 = 9.0;
pub(super) const HEAD: f32 = 44.0;

pub(super) enum Entry {
    Row(El),
    Divider,
    Head(El),
    Block(El, f32),
}

impl Entry {
    fn height(&self) -> f32 {
        match self {
            Entry::Row(_) => MENU_ROW,
            Entry::Divider => DIVIDER,
            Entry::Head(_) => HEAD,
            Entry::Block(_, h) => *h,
        }
    }
}

pub(super) fn height(entries: &[Entry]) -> f32 {
    2.0 * PAD + entries.iter().map(Entry::height).sum::<f32>()
}

fn divider() -> El {
    el().w_full().h(1.0).my((DIVIDER - 1.0) * 0.5).fill(th::hairline())
}

pub(super) struct Flow {
    from: [f32; 4],
    pub(super) to: [f32; 4],
    from_radius: f32,
}

pub(super) fn place(cx: &Cx<State>, anchor: &PopupAnchor, card_h: f32, w: f32, h: f32) -> Flow {
    place_w(cx, anchor, W, card_h, w, h)
}

pub(super) fn place_w(cx: &Cx<State>, anchor: &PopupAnchor, card_w: f32, card_h: f32, w: f32, h: f32) -> Flow {
    let clamp_x = |x: f32| x.clamp(EDGE, (w - card_w - EDGE).max(EDGE));
    match anchor {
        PopupAnchor::Point(x, y) => {
            let left = if x + card_w + EDGE > w { x - card_w } else { *x };
            let top = if y + card_h + EDGE <= h { *y } else { y - card_h };
            let (left, top) = (clamp_x(left), top.max(EDGE));
            let (px, py) = (if left >= *x { left } else { left + card_w }, if top >= *y { top } else { top + card_h });
            Flow { from: [px, py, 0.0, 0.0], to: [left, top, card_w, card_h], from_radius: 0.0 }
        }
        PopupAnchor::Key(key) => match cx.window_layout(key) {
            Some([bx, by, bw, bh]) if bx + bw <= kit::RAIL_W => {
                let top = if by < h * 0.5 { by } else { by + bh - card_h };
                let top = top.clamp(EDGE, (h - card_h - EDGE).max(EDGE));
                Flow { from: [bx, by, bw, bh], to: [bx + bw + 2.0 * GAP + 8.0, top, card_w, card_h], from_radius: bh.min(bw) * 0.5 }
            }
            Some([bx, by, bw, bh]) => {
                let down = by + bh + GAP + card_h + EDGE <= h;
                let top = if down { by + bh + GAP } else { by - GAP - card_h };
                Flow { from: [bx, by, bw, bh], to: [clamp_x(bx + bw - card_w), top.max(EDGE), card_w, card_h], from_radius: bh.min(bw) * 0.5 }
            }
            None => {
                let (x, y) = (clamp_x((w - card_w) * 0.5), ((h - card_h) * 0.5).max(EDGE));
                Flow { from: [x + card_w * 0.5, y + card_h * 0.5, 0.0, 0.0], to: [x, y, card_w, card_h], from_radius: 0.0 }
            }
        },
    }
}

pub(super) fn flow_menu(name: &'static str, t: f32, open: bool, flow: &Flow, entries: Vec<Entry>, close: fn(&mut State)) -> El {
    let e = t.max(0.0);
    let lerp = |a: f32, b: f32| a + (b - a) * e;
    let [fx, fy, fw, fh] = flow.from;
    let [tx, ty, tw, th_] = flow.to;
    let (x, y) = (lerp(fx, tx), lerp(fy, ty));
    let (w, h) = (lerp(fw, tw).max(0.0), lerp(fh, th_).max(0.0));
    let radius = lerp(flow.from_radius, RADIUS).min(w.min(h) * 0.5);
    let skin = (t * 6.0).clamp(0.0, 1.0);

    let mut items = col().key(format!("{name}-items")).absolute(tx - x, ty - y).w(tw).h(th_).padding(PAD).transition(Transition::none());
    for (i, entry) in entries.into_iter().enumerate() {
        let a = ((t - 0.16 - 0.04 * i as f32) / 0.40).clamp(0.0, 1.0);
        let a = a * a * (3.0 - 2.0 * a);
        items = items.child(match entry {
            Entry::Row(r) => r.opacity(a),
            Entry::Divider => divider().opacity(a),
            Entry::Head(hd) => hd.opacity(a),
            Entry::Block(b, _) => b.opacity(a),
        });
    }
    let card = el()
        .key(format!("{name}-card"))
        .absolute(x, y)
        .w(w)
        .h(h)
        .radius(radius)
        .clip_rounded(radius)
        .border(1.0, th::veil(0.10 * skin))
        .children(kit::frost(radius, skin))
        .transition(Transition::none())
        .child(items);
    let card = if open { card.modal(true, move |s: &mut State| close(s)) } else { card };

    let mut layer = el().key(format!("{name}-layer")).absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).instant_changes();
    layer = layer.child(kit::snapshot().key(format!("{name}-snap")));
    if open {
        layer = layer.child(
            el().key(format!("{name}-scrim")).absolute(0.0, 0.0).w_full().h_full().on_press(move |s: &mut State| close(s)).on_secondary_press(move |s: &mut State, _, _| close(s)),
        );
    }
    layer.child(card)
}

/// Renders the active popover card, anchored to its triggering control.
pub fn popover(s: &State, cx: &Cx<State>, w: f32, h: f32) -> El {
    let Some(p) = &s.popover else { return el() };
    let (list, width) = match p {
        Popover::Actions { .. } => (popovers::action_entries(s), 248.0),
        Popover::Waves { choices, .. } => (popovers::wave_entries(s, choices), 300.0),
        Popover::Quality { .. } => (popovers::quality_entries(s), 220.0),
        Popover::Stage { .. } => {
            let pw = panels::stage_w(s, w, h);
            (panels::stage_entries(s, pw, h), pw)
        }
        Popover::Pinned { item, .. } => (popovers::pinned_entries(s, item), 208.0),
        Popover::Downloads { .. } => (popovers::download_entries(s), 300.0),
        Popover::Liked { .. } => (popovers::liked_entries(s), 260.0),
        Popover::EqPreset { .. } => (popovers::eq_entries(s), 200.0),
        Popover::TrackSort { .. } => (popovers::track_sort_entries(s), 220.0),
        Popover::DiscSort { .. } => (popovers::disc_sort_entries(s), 220.0),
        Popover::WaveTune { .. } => (panels::wave_tune_entries(s, h), panels::STAGE_W),
    };
    let mut flow = place_w(cx, &PopupAnchor::Key(p.anchor().into()), width, height(&list), w, h);
    if matches!(p, Popover::Stage { .. }) {
        let (cover_x, cover) = np::cover_span(s, w, h);
        flow.to[0] = (cover_x + (cover - width) * 0.5).clamp(EDGE, (w - width - EDGE).max(EDGE));
    }
    flow_menu("popover", s.pop.t, s.popover_open, &flow, list, State::close_popover)
}

const VOL_W: f32 = 64.0;
const VOL_H: f32 = 184.0;
const VOL_NOTCHES: usize = 15;
const VOL_TOP: f32 = 34.0;
const VOL_BOTTOM: f32 = 12.0;

/// Renders the dock's vertical notched volume card.
pub fn volume(s: &State, cx: &Cx<State>, w: f32, h: f32) -> El {
    let t = s.vol.t;
    let (from, from_radius) = match cx.window_layout("vol-btn") {
        Some(r) => (r, r[2].min(r[3]) * 0.5),
        None => ([w * 0.5, h * 0.5, 0.0, 0.0], 0.0),
    };
    let cx_mid = from[0] + from[2] * 0.5;
    let to = [(cx_mid - VOL_W * 0.5).clamp(EDGE, (w - VOL_W - EDGE).max(EDGE)), (from[1] - GAP - VOL_H).max(EDGE), VOL_W, VOL_H];
    let e = t.max(0.0);
    let lerp = |a: f32, b: f32| a + (b - a) * e;
    let (x, y) = (lerp(from[0], to[0]), lerp(from[1], to[1]));
    let (cw, ch) = (lerp(from[2], to[2]).max(0.0), lerp(from[3], to[3]).max(0.0));
    let radius = lerp(from_radius, 20.0).min(cw.min(ch) * 0.5);
    let skin = (t * 6.0).clamp(0.0, 1.0);

    let v = s.player.volume.clamp(0.0, 1.0);
    let field_h = VOL_H - VOL_TOP - VOL_BOTTOM;
    let pitch = field_h / VOL_NOTCHES as f32;
    let mut field = el()
        .key("vol-field")
        .role("slider")
        .label(S::Volume.get(s.lang))
        .absolute(to[0] - x, to[1] - y + VOL_TOP)
        .w(VOL_W)
        .h(field_h)
        .cursor(rune::Cursor::Pointer)
        .transition(Transition::none())
        .on_drag(|s: &mut State, d: &mut rune::Drag, _tx: &mut rune::Tx| {
            let f = if d.h > 0.0 { 1.0 - d.y / d.h } else { 0.0 };
            let f = ((f - 0.015) / 0.97).clamp(0.0, 1.0);
            s.set_volume(f);
        });
    for i in 0..VOL_NOTCHES {
        let lit = (v * VOL_NOTCHES as f32 - i as f32).clamp(0.0, 1.0);
        let a = ((t - 0.06 - 0.022 * i as f32) / 0.30).clamp(0.0, 1.0);
        let a = a * a * (3.0 - 2.0 * a);
        let width = 14.0 + 20.0 * i as f32 / (VOL_NOTCHES - 1) as f32;
        let nx = (VOL_W - width * (0.5 + 0.5 * a)) * 0.5;
        let ny = field_h - (i as f32 + 0.5) * pitch - 2.0 + (1.0 - a) * 10.0;
        field = field.child(el().absolute(nx, ny).w(width * (0.5 + 0.5 * a)).h(4.0).radius(2.0).fill(th::veil((0.16 + 0.84 * lit) * a)).transition(Transition::none()));
    }
    let pct = (v * 100.0).round() as i32;
    let number = row()
        .absolute(to[0] - x, to[1] - y + 12.0)
        .w(VOL_W)
        .h(18.0)
        .center()
        .transition(Transition::none())
        .child(label(format!("{pct}"), th::MONO, th::text2()).opacity(((t - 0.15) / 0.3).clamp(0.0, 1.0)));
    let card = el()
        .key("vol-card")
        .absolute(x, y)
        .w(cw)
        .h(ch)
        .radius(radius)
        .clip_rounded(radius)
        .border(1.0, th::veil(0.10 * skin))
        .children(kit::frost(radius, skin))
        .transition(Transition::none())
        .on_press(|_s: &mut State| {})
        .on_hover_change(|s: &mut State, over: bool, _tx: &mut rune::Tx| {
            s.vol_card_hover = over;
            if over {
                s.vol_open = true;
                s.vol_close_at = None;
            } else {
                s.vol_close_at = Some(s.now + 0.28);
            }
        })
        .child(number)
        .child(field);
    el().key("vol-layer").absolute(0.0, 0.0).w(0.0).h(0.0).transition(Transition::none()).child(kit::snapshot().key("vol-snap").w(w).h(h)).child(card)
}

/// Renders the profile menu anchored to the rail avatar button.
pub fn profile(s: &State, cx: &Cx<State>, w: f32, h: f32) -> El {
    let lang = s.lang;
    let name = super::dock::account_name(s);
    let sub = s.account.as_ref().map(|a| a.login.clone()).unwrap_or_default();
    let plus = s.account.as_ref().is_some_and(|a| a.has_plus) && s.session == Session::Live;
    let initial = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    let head = row().w_full().h(HEAD).px(10.0).gap(10.0).items_center().child(super::dock::account_picture(s, 28.0, &initial)).child(
        col()
            .grow(1.0)
            .min_w(0.0)
            .child(row().gap(6.0).items_center().min_w(0.0).child(label(name, th::BODY_M, th::text()).truncate().min_w(0.0)).children(plus.then(|| kit::plus_badge(lang))))
            .child(label(sub, th::SMALL, th::text3()).truncate().min_w(0.0)),
    );
    let entry = |key: &'static str, ic: &'static Icon, text: &str, on: bool| menu_row(key, ic, text, on);
    let list = vec![
        Entry::Head(head),
        Entry::Divider,
        Entry::Row(entry("menu-settings", &icons::SETTINGS, S::Settings.get(lang), s.settings_open).on_press(|s: &mut State| s.open_settings())),
        Entry::Divider,
        Entry::Row(entry("menu-out", &icons::SIGN_OUT, S::SignOut.get(lang), false).on_press(|s: &mut State| s.sign_out())),
    ];
    let flow = place(cx, &PopupAnchor::Key("profile".into()), height(&list), w, h);
    flow_menu("menu", s.menu.t, s.menu_open, &flow, list, |s| s.menu_open = false)
}
