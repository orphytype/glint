//! The window. Back to front: the page backdrop, the current page (beside the rail), the frame
//! snapshot (the one copy of the frame that all the blur is made from), the progressive blur at the
//! top and bottom of the page, the rail, the title bar (the way back), the player bar, the menus
//! and the toasts. The Now Playing page covers the rail and the bar with its own player; while it
//! comes or goes it is drawn over them, on its own spring (`State::np`).

use crate::fx::Slot as _;
use rune::{Cursor, Cx, El, Transition, el, icon, row};

use super::icons;
use super::kit::{self, BAR_GAP, BAR_H, RAIL_W, TITLE_H, label};
use super::{dock, login, np, pages};
use crate::app::{Session, State};
use crate::fx::{self, Glass};
use crate::i18n::S;
use crate::theme::{self as th};
use rune::col;

/// Where the backdrop and the page paint among the window's layers: under everything else, by
/// order and not only by place in the tree. A page that is leaving (another tab was opened) keeps
/// fading for a moment, and RUNE moves a leaving element to the end of the tree. Without an order
/// the old page painted over the player bar until it had faded.
const PAINT_FIELD: i16 = -2;
const PAINT_PAGE: i16 = -1;

pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    // The look follows the setting before anything below reads a colour.
    th::set_light(s.is_light());
    cx.reduced_motion(s.settings.reduce_motion);
    cx.foreground(th::text());
    cx.caption_color(th::text2());
    let measured = cx.layout_size("app");
    let [w, h] = measured.unwrap_or([1280.0, 800.0]);
    if let Some(size) = measured {
        let prev = s.win_size.replace(size);
        if prev != [0.0, 0.0] && ((prev[0] - w).abs() > 0.5 || (prev[1] - h).abs() > 0.5) {
            if !s.is_resizing()
                && let Some(app) = &s.app
            {
                app.wake();
            }
            s.resized_at.set(s.now);
        }
    }
    fx::set_resizing(s.is_resizing());
    if let Some([bw, _]) = cx.layout_size("vol-track") {
        s.vol_w.set(bw);
    }
    s.win_w.set(w);
    // Until the GPU takes over, the launch screen covers the whole window, so the page under it is
    // left out. Its turned mood names would stop the CPU from painting the first frame, and its
    // skeleton shimmer would hold back the hand-over to the GPU.
    if !s.headless
        && !fx::gpu_up()
        && let Some(launch) = super::splash::view(s)
    {
        return col().key("app").w_full().h_full().fill(th::bg()).child(launch);
    }
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
    // What the pointer is on, of the surfaces that float over the page: the page (and, under the
    // volume card, the bar) is built taking no hover while it is on one.
    let on_card = logged_in && !np_open && s.vol_visible() && s.vol_card_hover;
    let on_surface = s.over_titlebar || (bar_on && !np_full && s.over_bar) || on_card;
    kit::set_under(on_surface);

    // Hover washes and the group styles they drive come up over a moment instead of at once.
    let mut app = col()
        .key("app")
        .w_full()
        .h_full()
        .fill(th::bg())
        .hover_motion_within(rune::Spring::new(0.16, 0.0), rune::Spring::new(0.24, 0.0))
        .child(el().key("fx-pulse").absolute(0.0, 0.0).size(0.0, 0.0).live_offset(&s.fx_pulse).transition(Transition::none()))
        .child(el().key("field").absolute(0.0, 0.0).w_full().h_full().fx(fx::FIELD).paint_order(PAINT_FIELD));
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
            let page = page.paint_order(PAINT_PAGE);
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
        kit::set_under(on_card);
        app = app.child(dock::bar(s, w));
        kit::set_under(on_surface);
    }
    if np_moving {
        app = app.child(np::page(s, w, h, np_t, np_open));
    }
    kit::set_under(false);
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
    // A menu opened from Settings goes over its dialog; every other one under it.
    let over_settings = matches!(s.popover, Some(crate::app::Popover::EqPreset { .. }));
    if logged_in && s.popover_visible() && !over_settings {
        app = app.child(super::menu::popover(s, cx, w, h));
    }
    if logged_in && s.sheet_visible() {
        app = app.child(super::details::sheet(s, cx, w, h));
    }
    if s.download_confirm.is_some() {
        app = app.child(super::menu::download_confirm(s, w, h));
    }
    if s.delete_download_confirm.is_some() {
        app = app.child(super::menu::delete_download_confirm(s, w, h));
    }
    if logged_in && s.name_dialog.is_some() {
        app = app.child(super::menu::name_dialog(s, w));
    }
    if logged_in && s.delete_playlist_confirm.is_some() {
        app = app.child(super::menu::delete_playlist_confirm(s, w));
    }
    if logged_in && s.settings_open {
        app = app.child(super::settings::modal(s, w, h));
    }
    if logged_in && s.popover_visible() && over_settings {
        app = app.child(super::menu::popover(s, cx, w, h));
    }
    let app = app.child(island(s, w));
    // The launch screen covers everything while the app starts under it.
    match super::splash::view(s) {
        Some(launch) => app.child(launch),
        None => app,
    }
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
    let mut r = row()
        .key("titlebar")
        .absolute(left, 0.0)
        .w_full()
        .h(TITLE_H)
        .pl(pad + lead)
        .pr(16.0 + trail)
        .items_center()
        .window_drag()
        // While the pointer is on it, the page scrolled under it takes no hover (`kit::set_under`).
        .on_hover_change(|s: &mut State, on: bool, _tx: &mut rune::Tx| s.over_titlebar = on);
    if logged_in && !np {
        // Back and forward, always there; one with nowhere to go is faint and takes no press.
        // (Over the player, the way out is its own element, `np_close`.)
        let back = dock::nav_chip("back", &icons::CHEVRON_LEFT, S::Back.get(s.lang), s.can_back());
        let fwd = dock::nav_chip("forward", &icons::CHEVRON_RIGHT, S::Forward.get(s.lang), s.can_forward());
        r = r
            .child(if s.can_back() { back.on_press(|s: &mut State| s.back()) } else { back })
            .child(if s.can_forward() { fwd.on_press(|s: &mut State| s.go_forward()) } else { fwd }.ml(8.0));
    }
    // A new version waits beside the window's controls.
    if logged_in && let Some(pill) = dock::update_pill(s) {
        // Kept well clear of the window's buttons: right beside them it read as touching "minimise".
        r = r.child(el().grow(1.0)).child(pill.mr(16.0));
    }
    r
}

/// Where what plays came from, at the top of the open player: "My Wave · Calm", "Album ·
/// Name"; pressing it leads there. It is glass, as tall as the way out beside it and in the same
/// colours, and like it comes and goes through its glass's own `hide` (`t`: how far the player is
/// open).
fn np_source(s: &State, w: f32, h: f32, t: f32) -> El {
    let Some(from) = &s.playing_from else { return el() };
    let t = t.clamp(0.0, 1.0);
    let text = from.line(s.lang);
    let (x, side, _, _) = np::columns(s, w, h, s.np_panel() != crate::app::NpTab::Closed);
    // Keep the source badge inside the cover column. The icon and padding consume 56 px,
    // leaving the rest for the single-line title; the label itself truncates at that width.
    let max_text = (side - 56.0).max(0.0);
    let tw = rune::text::measure(&text, &[], th::SMALL, None).0.ceil().min(max_text);
    let pw = 56.0 + tw;
    let c = dock::CHIP;
    let glyph = if from.wave { kit::wave_glyph(18.0, th::text().alpha(t)) } else { icon(&icons::QUEUE, 16.0).color(th::text().alpha(t)).w(18.0) };
    let page = from.page.clone();
    let cx = x + side * 0.5;
    let mut pill = row()
        .key("np-source")
        .role(if page.is_some() { "link" } else { "note" })
        .label(text.as_str())
        // Over the cover's column, so lines of lyrics or the queue never run under it.
        .absolute((cx - pw * 0.5).clamp(8.0, (w - pw - 8.0).max(8.0)), kit::CHROME_INSET)
        .w(pw)
        .max_w(side)
        .h(c)
        .pl(14.0)
        .pr(16.0)
        .gap(8.0)
        .radius(c * 0.5)
        .items_center()
        .no_window_drag()
        .transition(Transition::none())
        .motion(rune::Spring::NONE)
        .child(kit::glass(Glass::Chip, c * 0.5).fx_data([1.0 - t, 0.0, 0.0, 0.0]))
        .child(glyph)
        .child(label(text, th::SMALL, th::text().alpha(t)).truncate().max_w(max_text));
    if let Some(p) = page {
        pill = pill.cursor(Cursor::Pointer).press_scale(th::PRESS).on_press(move |s: &mut State| s.go(p.clone()));
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
        .child(kit::glass(Glass::Chip, c * 0.5).fx_data([1.0 - t, 0.0, 0.0, 0.0]))
        .child(icon(&icons::CHEVRON_DOWN, kit::GLYPH).color(th::text().alpha(t)))
        .on_press(|s: &mut State| s.set_np(false))
}

// ---- the island ----------------------------------------------------------------------------------

/// The island's height, and how it springs: a little past its size and back, like a drop.
const ISLAND_H: f32 = 36.0;
const ISLAND_SPRING: rune::Spring = rune::Spring::new(0.5, 0.22);
const ISLAND_IN: rune::SpringPair = rune::SpringPair::new(rune::Spring::new(0.45, 0.26), rune::Spring::new(0.28, 0.0));

/// What the island says now: the newest note, else how the downloads are going.
struct IslandSay {
    key: String,
    glyph: El,
    text: String,
    toast: Option<u64>,
}

fn island_say(s: &State) -> Option<IslandSay> {
    let ok = th::rgb(0x3dd27f);
    let bad = th::rgb(0xff6b5e);
    if let Some(t) = s.toasts.last() {
        let glyph = if t.error { icon(&icons::ALERT, 16.0).color(bad) } else { icon(&icons::CHECK, 16.0).color(ok) };
        return Some(IslandSay { key: format!("island-t-{}", t.id), glyph, text: t.text.clone(), toast: Some(t.id) });
    }
    let activity = s.download_activity?;
    if activity.active == 0 && s.now - activity.changed_at > 3.8 {
        return None;
    }
    let lang = s.lang;
    Some(if activity.active > 0 {
        IslandSay {
            key: "island-dl".into(),
            glyph: icon(&icons::SPINNER, 16.0).color(th::rgb(0xffffff)).looping(rune::LoopSpec::rotation(0.9)),
            text: format!("{}: {}", S::DownloadInProgress.get(lang), activity.active),
            toast: None,
        }
    } else if activity.failed > 0 {
        IslandSay {
            key: "island-dl-failed".into(),
            glyph: icon(&icons::ALERT, 16.0).color(bad),
            text: format!("{}: {}", S::DownloadFailed.get(lang), activity.failed),
            toast: None,
        }
    } else {
        IslandSay { key: "island-dl-done".into(), glyph: icon(&icons::DOWNLOAD, 16.0).color(ok), text: format!("{}: {}", S::DownloadDone.get(lang), activity.done), toast: None }
    })
}

/// The notes of the app as one black island in the middle of the title bar. It swells out of a
/// point when there is something to say, springs to the width of each new line (the old line
/// fading as the new one comes in) and draws back into nothing when all is said. A press on a
/// note puts it away.
fn island(s: &State, w: f32) -> El {
    let Some(say) = island_say(s) else { return el().key("island-none") };
    let max_text = (w * 0.5 - 60.0).clamp(120.0, 420.0);
    let tw = rune::text::measure(&say.text, &[], th::BODY_M, None).0.ceil().min(max_text);
    let iw = (14.0 + 16.0 + 9.0 + tw + 18.0).max(132.0);
    let line = row()
        .key(say.key)
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .pl(14.0)
        .pr(18.0)
        .gap(9.0)
        .items_center()
        .justify_center()
        .transition(Transition::opacity().combined(Transition::scale(0.9)).springs(rune::SpringPair::new(rune::Spring::new(0.3, 0.0), rune::Spring::new(0.18, 0.0))))
        .child(say.glyph)
        .child(label(say.text, th::BODY_M, th::rgb(0xf4f4f4)).truncate().max_w(max_text));
    let mut pill = row()
        .key("island-pill")
        .role("status")
        .w(iw)
        .h(ISLAND_H)
        .radius(ISLAND_H * 0.5)
        .clip_rounded(ISLAND_H * 0.5)
        .fill(th::rgb(0x000000))
        .border(1.0, if th::light() { th::rgba(0x000000, 0.0) } else { th::rgba(0xffffff, 0.09) })
        .shadow(8.0, 24.0, th::rgba(0x000000, if th::light() { 0.22 } else { 0.40 }))
        .no_window_drag()
        .motion(ISLAND_SPRING)
        .transition(Transition::opacity().combined(Transition::scale(0.4)).springs(ISLAND_IN))
        .child(line);
    if let Some(id) = say.toast {
        pill = pill.cursor(Cursor::Pointer).press_scale(th::PRESS).on_press(move |s: &mut State| s.toasts.retain(|t| t.id != id));
    }
    row().key("island").absolute(0.0, ((TITLE_H - ISLAND_H) * 0.5).round()).w_full().h(ISLAND_H).justify_center().transition(Transition::none()).child(pill)
}
