//! The window. Back to front: the page backdrop, the current page (beside the rail), the frame
//! snapshot (the one copy of the frame that all the blur is made from), the progressive blur at the
//! top and bottom of the page, the rail, the title bar (the way back), the player bar, the menus
//! and the toasts. The Now Playing page covers the rail and the bar with its own player; while it
//! comes or goes it is drawn over them, on its own spring (`State::np`).

use rune::{Cursor, Cx, El, Transition, el, icon, row};

use super::icons;
use super::kit::{self, BAR_GAP, BAR_H, RAIL_W, TITLE_H, label};
use super::{dock, login, np, pages};
use crate::app::{Session, State};
use crate::fx::{self, Glass};
use crate::i18n::S;
use crate::theme::{self as th};
use rune::col;

pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    // The look follows the setting before anything below reads a colour.
    th::set_light(s.is_light());
    cx.reduced_motion(s.settings.reduce_motion);
    cx.foreground(th::text());
    cx.caption_color(th::text2());
    let [w, h] = cx.layout_size("app").unwrap_or([1280.0, 800.0]);
    if let Some([bw, _]) = cx.layout_size("seek-track") {
        s.bar_w.set(bw);
    }
    if let Some([bw, _]) = cx.layout_size("vol-track") {
        s.vol_w.set(bw);
    }
    s.win_w.set(w);
    if s.bugs_alive() {
        measure_bugs(s, cx, h);
    }
    let ins = cx.chrome_insets();
    s.art.begin_view();
    let logged_in = s.session != Session::LoggedOut;
    let np_open = s.np_open && s.current().is_some();
    // The player comes over the window on a spring of its own (`State::np`): while it moves, it is
    // drawn over the page, the rail and the bar, which stay put underneath; once it covers the
    // window it is all there is.
    let np_t = if s.current().is_some() { s.np.t } else { 0.0 };
    let np_full = np_open && np_t >= 1.0;
    let np_moving = logged_in && !np_full && (np_open || np_t > 0.0);
    // The player bar is only there while something plays.
    let bar_on = logged_in && s.current().is_some();
    kit::set_bar_on(bar_on);

    // Hover washes and the group styles they drive come up over a moment instead of at once.
    let mut app = col()
        .key("app")
        .w_full()
        .h_full()
        .fill(th::bg())
        .hover_motion_within(rune::Spring::new(0.16, 0.0), rune::Spring::new(0.24, 0.0)).child(el().key("field").absolute(0.0, 0.0).w_full().h_full().effect(fx::FIELD));
    if logged_in {
        if np_full {
            // The player's own soft edges grow in once it covers the window (they blur the player,
            // so they can only be drawn once it is all there is).
            let v = s.np_veil.t;
            app = app
                .child(np::page(s, w, h, 1.0, true))
                .child(kit::snapshot())
                .child(kit::veil_top((TITLE_H + 20.0) * v).key("np-veil-top").motion(rune::Spring::NONE))
                .child(kit::veil_bottom(48.0 * v).key("np-veil-bottom").motion(rune::Spring::NONE));
        } else {
            // Under a player that is coming over, the page leaves at once when it is covered (its
            // own fade out would play on top of the player).
            let page = if np_open { pages::page(s, w, h).transition(Transition::none()) } else { pages::page(s, w, h) };
            app = app
                .child(page)
                .child(kit::snapshot())
                .child(kit::veil_top(TITLE_H + 24.0))
                .child(kit::veil_bottom(if bar_on { BAR_H + BAR_GAP + 28.0 } else { 40.0 }))
                .child(dock::rail(s, w));
        }
    } else {
        app = app.child(kit::snapshot()).child(login::page(s));
    }
    if bar_on && !np_full {
        app = app.child(dock::bar(s, w));
    }
    if np_moving {
        app = app.child(np::page(s, w, h, np_t, np_open));
    }
    app = app.child(titlebar(s, ins.leading, ins.trailing, logged_in, np_open));
    if logged_in && (np_full || np_moving) {
        app = app.child(np_close(s, ins.leading, if np_open { np_t } else { 0.0 }));
        if np_open {
            app = app.child(np_source(s, w, h, np_t));
        }
    }
    if logged_in && s.menu_visible() {
        app = app.child(super::menu::profile(s, cx, w, h));
    }
    if logged_in && !np_open && s.vol_visible() {
        app = app.child(super::menu::volume(s, cx, w, h));
    }
    if logged_in && s.track_menu_visible() {
        app = app.child(super::menu::overlay(s, cx, w, h));
    }
    if logged_in && s.popover_visible() {
        app = app.child(super::menu::popover(s, cx, w, h));
    }
    if logged_in && s.lightbox_visible() {
        app = app.child(super::menu::lightbox(s, cx, w, h));
    }
    if logged_in && s.sheet_visible() {
        app = app.child(super::menu::sheet(s, w, h));
    }
    if s.download_confirm.is_some() {
        app = app.child(super::menu::download_confirm(s, w, h));
    }
    if s.delete_download_confirm.is_some() {
        app = app.child(super::menu::delete_download_confirm(s, w, h));
    }
    app = app.child(download_activity(s));
    app.child(toasts(s, np_open))
}

/// Where the butterflies' layer on Home is, and what in it they keep clear of and land on, from the
/// last layout (`bugs::Geom`): all in the layer's points, with the page's scroll at that moment.
fn measure_bugs(s: &State, cx: &mut Cx<State>, h: f32) {
    let Some(layer) = cx.window_layout("bug-layer") else { return };
    let spots = pages::bug_spots(s)
        .into_iter()
        .filter_map(|(key, round)| {
            let r = cx.window_layout(&key)?;
            let id = key.bytes().fold(0xcbf2_9ce4_8422_2325u64, |a, b| (a ^ b as u64).wrapping_mul(0x100_0000_01b3));
            Some(crate::bugs::Spot { id, rect: [r[0] - layer[0], r[1] - layer[1], r[2], r[3]], round })
        })
        .collect();
    let scroll = crate::app::page_scroll("home").map_or(0.0, |p| p[0]);
    *s.bug_geom.borrow_mut() = crate::bugs::Geom { layer, scroll, view: [TITLE_H, h - kit::foot_room() + 24.0], spots };
}

/// The top band of the page: the way back (or out of the player) on the left, the window controls
/// on the right, and the rest of it drags the window.
fn titlebar(s: &State, lead: f32, trail: f32, logged_in: bool, np: bool) -> El {
    let left = if logged_in && !np { RAIL_W } else { 0.0 };
    // Over a page the way back lines up with the page's left edge; over the player, with the window's.
    let pad = if left > 0.0 { kit::PAGE_PAD - 4.0 } else { kit::CHROME_INSET };
    let mut r = row().key("titlebar").absolute(left, 0.0).w_full().h(TITLE_H).pl(pad + lead).pr(16.0 + trail).items_center().window_drag();
    if logged_in && !np {
        // Back and forward, always there; one with nowhere to go is faint and takes no press.
        // (Over the player, the way out is its own element, `np_close`.)
        let back = dock::nav_chip("back", &icons::CHEVRON_LEFT, S::Back.get(s.lang), s.can_back());
        let fwd = dock::nav_chip("forward", &icons::CHEVRON_RIGHT, S::Forward.get(s.lang), s.can_forward());
        r = r
            .child(if s.can_back() { back.on_press(|s: &mut State| s.back()) } else { back })
            .child(if s.can_forward() { fwd.on_press(|s: &mut State| s.go_forward()) } else { fwd }.ml(8.0));
    }
    r
}

/// Where what plays came from, at the top of the open player: "My Wave · Calm", "Album ·
/// Name"; pressing it leads there. `t`: how far the player is open.
fn np_source(s: &State, w: f32, h: f32, t: f32) -> El {
    let Some(from) = &s.playing_from else { return el() };
    let t = t.clamp(0.0, 1.0);
    let text = from.line(s.lang);
    let (x, side, _, _) = super::np::columns(s, w, h, s.np_panel() != crate::app::NpTab::Closed);
    // Keep the source badge inside the cover column. The icon and padding consume 56 px,
    // leaving the rest for the single-line title; the label itself truncates at that width.
    let max_text = (side - 56.0).max(0.0);
    let tw = rune::text::measure(&text, &[], th::SMALL, None).0.ceil().min(max_text);
    let pw = 56.0 + tw;
    let glyph = if from.wave {
        kit::wave_glyph(18.0, th::text().alpha(t))
    } else {
        icon(&icons::QUEUE, 16.0).color(th::text2().alpha(t)).w(18.0)
    };
    let page = from.page.clone();
    let cx = x + side * 0.5;
    let mut pill = row()
        .key("np-source")
        .role(if page.is_some() { "link" } else { "note" })
        .label(text.as_str())
        // Over the cover's column, so lines of lyrics or the queue never run under it.
        .absolute((cx - pw * 0.5).clamp(8.0, (w - pw - 8.0).max(8.0)), kit::CHROME_INSET + 3.0)
        .w(pw)
        .max_w(side)
        .h(34.0)
        .pl(14.0)
        .pr(16.0)
        .gap(8.0)
        .radius(17.0)
        .items_center()
        .no_window_drag()
        .fill(th::veil(0.06 * t))
        .border(1.0, th::veil(0.08 * t))
        .transition(Transition::none())
        .motion(rune::Spring::NONE)
        .child(glyph)
        .child(label(text, th::SMALL, th::text2().alpha(t)).truncate().max_w(max_text));
    if let Some(p) = page {
        pill = pill.cursor(Cursor::Pointer).hover(th::veil(0.10)).press_scale(th::PRESS).on_press(move |s: &mut State| s.go(p.clone()));
    }
    pill
}

/// The way out of the player, at the window's top left. It is glass, which cannot take a group
/// fade, so it comes and goes with the player through its own `hide` (`t`: how far the player is
/// open).
fn np_close(s: &State, lead: f32, t: f32) -> El {
    let t = t.clamp(0.0, 1.0);
    let c = dock::CHIP;
    row()
        .key("np-close")
        .role("button")
        .label(S::Close.get(s.lang))
        .help(S::Close.get(s.lang))
        .absolute(kit::CHROME_INSET + lead, kit::CHROME_INSET)
        .w(c)
        .h(c)
        .radius(c * 0.5)
        .center()
        .cursor(Cursor::Pointer)
        .no_window_drag()
        .press_scale(th::PRESS)
        .transition(Transition::none())
        .motion(rune::Spring::NONE)
        .child(kit::glass(Glass::Chip, c * 0.5).effect_data([1.0 - t, 0.0, 0.0, 0.0]))
        .child(icon(&icons::CHEVRON_DOWN, kit::GLYPH).color(th::text().alpha(t)))
        .on_press(|s: &mut State| s.set_np(false))
}

fn toasts(s: &State, np: bool) -> El {
    let lift = if np || s.current().is_none() { 28.0 } else { BAR_H + BAR_GAP + 16.0 };
    let left = if np || s.session == Session::LoggedOut { 0.0 } else { RAIL_W };
    let mut c = col().key("toasts").pin(Some(left), None, Some(0.0), Some(lift)).items_center().gap(8.0);
    for t in &s.toasts {
        c = c.child(
            row()
                .key(format!("toast-{}", t.id))
                .px(20.0)
                .h(44.0)
                .radius(22.0)
                .items_center()
                .fill(th::pop())
                .border(1.0, th::hairline())
                .transition(Transition::opacity().combined(Transition::offset(0.0, 16.0)).combined(Transition::scale(0.96)).springs(th::SOFT))
                .child(label(t.text.clone(), th::BODY_M, th::text())),
        );
    }
    c
}

/// A single compact task surface. It stays in one place while a batch downloads, then changes its
/// label to the completed count so a user never has to reconcile several transient notifications.
fn download_activity(s: &State) -> El {
    let Some(activity) = s.download_activity else { return el().key("download-activity-empty") };
    let age = s.now - activity.changed_at;
    if activity.active == 0 && age > 3.8 {
        return el().key("download-activity-empty");
    }
    let (text, icon_color) = if activity.active > 0 {
        (format!("{}: {}", S::DownloadInProgress.get(s.lang), activity.active), th::text())
    } else if activity.failed > 0 {
        (format!("{}: {}", S::DownloadFailed.get(s.lang), activity.failed), th::rgb(0xd56b61))
    } else {
        (format!("{}: {}", S::DownloadDone.get(s.lang), activity.done), th::rgb(0x3dbf75))
    };
    let glyph = if activity.active > 0 {
        icon(&icons::SPINNER, 15.0).color(icon_color).looping(rune::LoopSpec::rotation(0.9))
    } else {
        icon(&icons::DOWNLOAD, 15.0).color(icon_color)
    };
    row()
        .key("download-activity")
        .absolute(0.0, TITLE_H + 8.0)
        .w_full()
        .h(38.0)
        .justify_center()
        .transition(Transition::opacity().combined(Transition::offset(0.0, -8.0)).combined(Transition::scale(0.96)).springs(th::SOFT))
        .child(row().h(36.0).px(16.0).gap(8.0).items_center().radius(18.0).fill(th::pop()).border(1.0, th::hairline()).child(glyph).child(label(text, th::BODY_M, th::text())))
}
