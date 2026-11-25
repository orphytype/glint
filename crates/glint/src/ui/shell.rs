//! The window. Back to front: the page backdrop, the current page (beside the rail), the frame
//! snapshot (the one copy of the frame that all the blur is made from), the progressive blur at the
//! bottom of the page, the rail, the title bar (the way back), the player bar, the menus
//! and the toasts. The Now Playing page covers the rail and the bar with its own player; while it
//! comes or goes it is drawn over them, on its own spring (`State::np`).

use crate::fx::Slot as _;
use rune::{Cursor, Cx, El, Transition, el, icon, row};

use super::icons;
use super::kit::{self, BAR_GAP, BAR_H, Hover as _, RAIL_W, TITLE_H, label};
use super::{dock, login, np, pages};
use crate::app::{Page, Session, State};
use crate::fx;
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
    fx::set_effects_off(!s.shaders_active());
    s.win_w.set(w);
    // Until the GPU takes over (only when shaders are active on GPU), the launch screen covers the
    // whole window so the page under it is left out. In CPU mode or with shaders off, draw at once.
    if !s.headless
        && !fx::cpu_mode()
        && fx::shaders_enabled()
        && !fx::gpu_up()
        && let Some(launch) = super::splash::view(s)
    {
        return col().key("app").w_full().h_full().fill(th::bg()).child(launch);
    }
    if s.bugs_alive() {
        measure_bugs(s, cx, h);
    }
    let ins = cx.chrome_insets();
    let logged_in = s.session != Session::LoggedOut;
    let np_open = s.np_open && s.current().is_some();
    let np_t = s.np.t.clamp(0.0, 1.0);
    let np_full = np_open && np_t >= 0.999;
    let np_moving = (np_t > 0.001 || s.np.v != 0.0) && s.current().is_some();
    // Pause `view_gen` while full-screen Now Playing hides the underlying page so its covers stay
    // cached and never reload or blink when collapsing Now Playing.
    if np_full {
        s.art.clear_watched();
    } else {
        s.art.begin_view();
    }
    // The player bar is shown while something plays, except on Home (which has its own inline player).
    let stage_home = s.page == Page::Home;
    let bar_on = logged_in && s.current().is_some() && !stage_home;
    let bar_vis = logged_in && s.bar_visible();
    kit::set_bar_on(bar_on);
    // What the pointer is on, of the surfaces that float over the page: the page (and, under the
    // volume card, the bar) is built taking no hover while it is on one.
    let on_card = logged_in && !np_open && s.vol_visible() && (s.vol_card_hover || s.vol_bridge_hover);
    let on_surface = s.over_titlebar || (bar_on && !np_open && s.over_bar) || on_card;
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
        if !np_full {
            // Under a player that is coming over, the page leaves at once when it is covered (its
            // own fade out would play on top of the player).
            let page = if np_open { pages::page(s, w, h).transition(Transition::none()) } else { pages::page(s, w, h) };
            let page = page.paint_order(PAINT_PAGE);
            // Skip allocating a full-window backdrop snapshot on Stage Home when no glass dock,
            // veil, or floating menu reads it (saves ~21-42 MiB on ultrawide displays).
            let need_snap = !stage_home || bar_vis || s.menu_visible() || s.vol_visible() || s.track_menu_visible() || s.popover_visible() || s.sheet_visible();
            app = app.child(page);
            if need_snap {
                app = app.child(kit::snapshot());
            }
            if !stage_home {
                app = app.child(kit::veil_bottom(if bar_on { BAR_H + BAR_GAP + 28.0 } else { 40.0 }));
            }
            app = app.child(kit::page_scrollbar(h)).child(dock::rail(s, w, ins.leading, np_t));
        }
        // Opening (or sliding off when returning to Home): bar sits beneath `np` so `np-cover` flies unobstructed.
        if bar_vis && (np_open || stage_home) && !np_full {
            kit::set_under(on_card);
            app = app.child(dock::bar(s, w, np_open, np_t));
            kit::set_under(on_surface);
        }
        // Mount Now Playing on the very same commit `np_open` flips true so `hm-cover`/`bar-cover`
        // leaves and `np-cover` enters together, letting `matched("np-cover")` fly smoothly on open.
        if np_open || np_moving {
            app = app.child(np::page(s, w, h, if np_full { 1.0 } else { np_t }, np_open, np_full));
        }
        // Take a post-Now-Playing backdrop snapshot only when a glass overlay (menu/popover/sheet)
        // is actually open on top of Now Playing, avoiding idle full-screen copies and blur pyramids.
        let np_overlay = s.menu_visible() || s.track_menu_visible() || s.popover_visible() || s.sheet_visible();
        if np_full && np_overlay {
            app = app.child(kit::snapshot());
        }
        // Closing (or browsing): bar sits above the fading `np` curtain so `bar-cover` flies cleanly
        // back down into the mini-player without being hidden behind `np-canvas`.
        if bar_vis && !np_open && !stage_home {
            kit::set_under(on_card);
            app = app.child(dock::bar(s, w, false, np_t));
            kit::set_under(on_surface);
        }
    } else {
        app = app.child(kit::snapshot()).child(login::page(s));
    }
    kit::set_under(false);
    app = app.child(titlebar(s, ins.leading, ins.trailing, logged_in, np_open, np_t));
    if logged_in && (np_open || np_moving) {
        let smooth = |x: f32| {
            let x = x.clamp(0.0, 1.0);
            x * x * (3.0 - 2.0 * x)
        };
        let chrome_a = if np_full {
            1.0
        } else if np_open {
            smooth((np_t - 0.14) / 0.62)
        } else {
            smooth((np_t - 0.30) / 0.60)
        };
        if chrome_a > 0.005 {
            app = app.child(np_close(s, ins.leading, chrome_a, np_open)).child(np_source(s, w, h, ins.leading, ins.trailing, chrome_a, np_open, np_full));
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
    if logged_in && s.settings_visible() {
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
/// on the right (or on the left on macOS), and the rest of it drags the window.
fn titlebar(s: &State, lead: f32, trail: f32, logged_in: bool, np: bool, np_t: f32) -> El {
    let left = if logged_in && !np { RAIL_W } else { 0.0 };
    // `lead` is measured from the window's left edge (`x = 0`), while `titlebar` starts at `left`.
    // Keep the back/forward arrows close to the rail on Windows/Linux and just right of the
    // traffic lights on macOS.
    let pl = if left > 0.0 {
        ((lead - left).max(0.0) + 12.0).max(14.0)
    } else {
        lead + kit::CHROME_INSET
    };
    let mut r = row()
        .key("titlebar")
        .pin(Some(left), Some(0.0), Some(0.0), None)
        .h(TITLE_H)
        .pl(pl)
        .pr(16.0 + trail)
        .items_center()
        .window_drag()
        // While the pointer is on it, the page scrolled under it takes no hover (`kit::set_under`).
        .on_hover_change(|s: &mut State, on: bool, _tx: &mut rune::Tx| s.over_titlebar = on);
    if logged_in && !np {
        // Back and forward, always there; one with nowhere to go is faint and takes no press.
        // Smoothly lift and settle back when Now Playing collapses.
        let flat = s.page == Page::Home || np_t > 0.01;
        let nav_vis = (1.0 - (np_t / 0.48).clamp(0.0, 1.0)).clamp(0.0, 1.0);
        let nav_vis = nav_vis * nav_vis * (3.0 - 2.0 * nav_vis);
        let back = dock::nav_chip("back", &icons::CHEVRON_LEFT, S::Back.get(s.lang), s.can_back(), flat);
        let fwd = dock::nav_chip("forward", &icons::CHEVRON_RIGHT, S::Forward.get(s.lang), s.can_forward(), flat);
        let mut arrows = row()
            .key("titlebar-nav")
            .items_center()
            .gap(6.0)
            .offset(0.0, -(1.0 - nav_vis) * 8.0)
            .child(if s.can_back() { back.on_press(|s: &mut State| s.back()) } else { back })
            .child(if s.can_forward() { fwd.on_press(|s: &mut State| s.go_forward()) } else { fwd });
        if nav_vis < 0.999 {
            arrows = arrows.opacity(nav_vis);
        }
        r = r.child(arrows);
    }
    // A new version waits beside the window's controls.
    if logged_in && let Some(pill) = dock::update_pill(s) {
        r = r.child(el().grow(1.0)).child(pill.mr(12.0));
    }
    r
}

/// Where what plays came from, at the top of the open player: "My Wave · Calm", "Album ·
/// Name"; pressing it leads there. It is glass, as tall as the way out beside it and in the same
/// colours, and like it comes and goes through its glass's own `hide` (`t`: how far the player is
/// open).
/// Unified cover-tinted frosted chip surface for Now Playing's collapse button and source pill.
/// Uses the exact same visual layers throughout opening and settled states so the button never
/// snaps colour late when Now Playing finishes opening.
fn chip_surface(s: &State, radius: f32, alpha: f32) -> El {
    let a = alpha.clamp(0.0, 1.0);
    let is_light = th::light();
    let base_fill = if is_light { th::bg().alpha(0.68 * a) } else { th::bg().alpha(0.32 * a) };
    let border = th::veil(if is_light { 0.12 } else { 0.15 } * a);
    let surface = el().absolute(0.0, 0.0).w_full().h_full().radius(radius).fill(base_fill).border(1.0, border).transition(Transition::none());
    match super::blobs::cover_tint(s.palette.wave, is_light, (if is_light { 0.10 } else { 0.18 }) * a) {
        Some(tint) => surface.child(el().absolute(0.0, 0.0).w_full().h_full().radius(radius).fill(tint)),
        None => surface.child(el().absolute(0.0, 0.0).w_full().h_full().radius(radius).fill(th::veil(if is_light { 0.04 } else { 0.06 } * a))),
    }
}

const NP_CLOSE_S: f32 = 30.0;
const NP_PILL_H: f32 = 28.0;
const NP_PILL_STYLE: rune::TextStyle = rune::TextStyle::new(12.0, 520.0).family(th::SANS).lh(16.0);

#[expect(clippy::too_many_arguments, reason = "explicit chrome layout, alpha, and interaction state")]
fn np_source(s: &State, w: f32, h: f32, lead: f32, trail: f32, t: f32, interactive: bool, settled: bool) -> El {
    let Some(from) = &s.playing_from else { return el() };
    let t = t.clamp(0.0, 1.0);
    let text = from.line_for_track(s.lang, s.current());
    let (cover_x, cover) = np::cover_span(s, w, h);
    let pill_y = ((TITLE_H - NP_PILL_H) * 0.5).round();
    // Stay centered over the cover, never touching `np-close` on the left, the window caption
    // controls on the right, or sticking out past the cover's edges.
    let min_x = 16.0 + lead + NP_CLOSE_S + 12.0;
    let win_right = (w - trail - 16.0).max(min_x + 80.0);
    let left_bound = min_x.max(cover_x);
    let right_bound = (cover_x + cover).min(win_right);
    let mut cx = cover_x + cover * 0.5;
    let mut half = (cx - left_bound).min(right_bound - cx);
    if half < 60.0 {
        cx = (min_x + win_right) * 0.5;
        half = (win_right - min_x) * 0.5;
    }
    let max_pw = (half * 2.0).clamp(80.0, 236.0);
    let overhead = 10.0 + 13.0 + 6.0 + 10.0;
    let max_text = (max_pw - overhead).max(0.0);
    let tw = kit::measure_w(&text, NP_PILL_STYLE).ceil().min(max_text);
    let pw = (overhead + tw + 2.0).min(max_pw);
    let glyph = kit::source_glyph(from, th::text().alpha(0.88 * t), 13.0);
    let page = from.page.clone();
    let px = (cx - pw * 0.5).round();
    let slide = if s.is_resizing() || !settled { rune::Spring::NONE } else { th::SLIDE };
    let mut pill = row()
        .key("np-source")
        .role(if page.is_some() { "link" } else { "note" })
        .label(text.as_str())
        .absolute(px, pill_y)
        .w(pw)
        .max_w(max_pw)
        .h(NP_PILL_H)
        .px(10.0)
        .gap(6.0)
        .radius(NP_PILL_H * 0.5)
        .offset(0.0, -(1.0 - t) * 10.0)
        .items_center()
        .no_window_drag()
        .transition(Transition::none())
        .animated_changes()
        .motion(slide)
        .child(chip_surface(s, NP_PILL_H * 0.5, t))
        .child(glyph)
        .child(label(text, NP_PILL_STYLE, th::text().alpha(0.92 * t)).truncate().grow(1.0).min_w(0.0));
    if interactive
        && let Some(p) = page
    {
        pill = pill.cursor(Cursor::Pointer).hov(th::veil(0.06)).press(th::veil(0.10)).press_scale(th::PRESS).on_press(move |s: &mut State| s.go(p.clone()));
    }
    pill
}

/// Compact collapse button aligned cleanly with the title bar and Now Playing source pill.
fn np_close(s: &State, lead: f32, t: f32, interactive: bool) -> El {
    let t = t.clamp(0.0, 1.0);
    let y = ((TITLE_H - NP_CLOSE_S) * 0.5).round();
    let mut btn = row()
        .key("np-close")
        .role("button")
        .label(S::Close.get(s.lang))
        .help(S::Close.get(s.lang))
        .absolute(16.0 + lead, y)
        .w(NP_CLOSE_S)
        .h(NP_CLOSE_S)
        .radius(NP_CLOSE_S * 0.5)
        .offset(0.0, -(1.0 - t) * 10.0)
        .center()
        .no_window_drag()
        .transition(Transition::none())
        .motion(rune::Spring::NONE)
        .child(chip_surface(s, NP_CLOSE_S * 0.5, t))
        .child(icon(&icons::CHEVRON_DOWN, 16.0).color(th::text().alpha(0.92 * t)));
    if interactive {
        btn = btn.cursor(Cursor::Pointer).hov(th::veil(0.07)).press(th::veil(0.12)).press_scale(th::PRESS).on_press(|s: &mut State| s.set_np(false));
    }
    btn
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
            glyph: kit::spin(icon(&icons::SPINNER, 16.0).color(th::rgb(0xffffff))),
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
        .transition(kit::pop_in(0.95, 0.0, rune::SpringPair::new(rune::Spring::new(0.26, 0.0), rune::Spring::new(0.14, 0.0))))
        .child(say.glyph)
        .child(label(say.text, th::BODY_M, th::rgb(0xf4f4f4)).truncate().max_w(max_text));
    let mut pill = row()
        .key("island-pill")
        .role("status")
        .w(iw)
        .h(ISLAND_H)
        .radius(ISLAND_H * 0.5)
        .fill(th::rgb(0x000000))
        .border(2.0, if th::light() { th::rgba(0x000000, 0.14) } else { th::rgba(0xffffff, 0.14) })
        .no_window_drag()
        .motion(ISLAND_SPRING)
        .transition(kit::pop_in(0.6, 0.0, ISLAND_IN).allow_small_scale())
        .child(line);
    if let Some(id) = say.toast {
        pill = pill.cursor(Cursor::Pointer).press_scale(th::PRESS).on_press(move |s: &mut State| s.toasts.retain(|t| t.id != id));
    }
    row().key("island").absolute(0.0, ((TITLE_H - ISLAND_H) * 0.5).round()).w_full().h(ISLAND_H).justify_center().transition(Transition::none()).child(pill)
}
