//! Now Playing overlay: centered player column, animated cover backdrop, and optional lyrics or
//! queue side panel.

mod count;
mod lyrics;
mod queue;

use crate::fx::Slot as _;
use rune::{El, Transition, col, el, row};

use super::dock;
use super::kit::{self, BTN, PAGE_PAD, TITLE_H, label};
use super::np_icons;
use super::shapes::Ill;
use crate::app::{NpTab, State, Station, WaveChoice};
use crate::fx;
use crate::i18n::S;
use crate::store::{PlayerLayout, Repeat};
use crate::theme::{self as th, Pal};

pub use count::{count_start, counting};
pub use lyrics::wiping;
pub use queue::queue_offset;

const SMALL_GLYPH: f32 = 18.0;
const SMALL_BTN: f32 = 34.0;
const MODE_GLYPH: f32 = 18.0;
const MODE_BTN: f32 = 34.0;
const SKIP_GLYPH: f32 = 24.0;
const TRANSPORT_GAP: f32 = 18.0;

const PLAYER_REST: f32 = 20.0 + 58.0 + 14.0 + 22.0 + 16.0 + 56.0 + 12.0 + 40.0 + 16.0 + 58.0;
const PLAYER_COMPACT_REST: f32 = 20.0 + 58.0 + 14.0 + 22.0 + 16.0 + 56.0 + 12.0 + 40.0;

const CONTROLS_W: f32 = 360.0;
const CONTROLS_MAX: f32 = 440.0;
const COVER_MIN: f32 = 96.0;

const CONTROLS_MIN: f32 = 280.0;
const QUEUE_MAX: f32 = 760.0;
const LYRICS_MAX: f32 = QUEUE_MAX;

/// Deterministic seek-bar pattern seed from a track id.
pub fn seed_of(id: &str) -> f32 {
    (id.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(u32::from(b))) % 997) as f32 + 0.5
}

/// How the open player arranges its cover and controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Standard,
    Compact,
    Cover,
}

pub fn layout(s: &State, h: f32) -> Layout {
    match s.settings.player_layout {
        PlayerLayout::Auto if h < 660.0 => Layout::Compact,
        PlayerLayout::Auto | PlayerLayout::Comfortable => Layout::Standard,
        PlayerLayout::Compact => Layout::Compact,
        PlayerLayout::Cover => Layout::Cover,
    }
}

/// Whether the My Wave card moves out of the column into the player menu.
pub fn compact_player(s: &State, h: f32) -> bool {
    layout(s, h) != Layout::Standard
}

fn cover_max(shape: Layout) -> f32 {
    match shape {
        Layout::Standard => 440.0,
        Layout::Compact => 260.0,
        Layout::Cover => 640.0,
    }
}

pub fn cover_side(s: &State, w: f32, h: f32) -> f32 {
    let shape = layout(s, h);
    let rest = if shape == Layout::Standard { PLAYER_REST } else { PLAYER_COMPACT_REST };
    (h - TITLE_H - 48.0 - rest).clamp(COVER_MIN, cover_max(shape)).min(w - 2.0 * PAGE_PAD).round()
}

fn controls_w(cover: f32) -> f32 {
    cover.clamp(CONTROLS_W, CONTROLS_MAX)
}

/// Computes `(player_x, player_w, panel_x, panel_w)` for the open player in a `w` x `h` window.
pub fn columns(s: &State, w: f32, h: f32, panel: bool) -> (f32, f32, f32, f32) {
    if !panel {
        let side = cover_side(s, w, h).max(CONTROLS_W).min(w - 2.0 * PAGE_PAD);
        return ((w - side) * 0.5, side, w, 0.0);
    }
    let gap = (w * 0.042).clamp(32.0, 56.0);
    let avail = (w - 2.0 * PAGE_PAD - gap).max(480.0);
    let widest = cover_max(layout(s, h)).max(CONTROLS_MAX);
    let max_player = (avail * 0.44).clamp(CONTROLS_MIN, widest);
    let side = cover_side(s, w, h).max(CONTROLS_W).min(max_player).min((avail - 260.0).max(CONTROLS_MIN));
    let panel_w = (avail - side).clamp(240.0, QUEUE_MAX);
    let total = side + gap + panel_w;
    let x = ((w - total) * 0.5).max(PAGE_PAD).round();
    let panel_x = (x + side + gap).round();
    (x, side, panel_x, panel_w)
}

/// Returns `(cover_x, cover_side)` across a `w` x `h` window.
pub fn cover_span(s: &State, w: f32, h: f32) -> (f32, f32) {
    let (x, side, _, _) = columns(s, w, h, s.np_panel() != NpTab::Closed);
    let cover = cover_side(s, w, h).min(side);
    (x + (side - cover) * 0.5, cover)
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn ease_out(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    1.0 - (1.0 - x).powi(3)
}

fn stagger_el(mut e: El, p: f32, dy: f32, scale_from: f32) -> El {
    let p = p.clamp(0.0, 1.0);
    if p < 0.999 {
        e = e.offset(0.0, (1.0 - p) * dy).opacity(p);
        if scale_from < 0.999 {
            e = e.scale(scale_from + (1.0 - scale_from) * p);
        }
    }
    e
}

/// Renders the Now Playing overlay.
pub fn page(s: &State, w: f32, h: f32, come: f32, front: bool, settled: bool) -> El {
    let Some(t) = s.current() else { return el() };
    let p = s.pal();
    let lang = s.lang;
    let tab = s.np_panel();
    let panel = tab != NpTab::Closed;

    let compact = compact_player(s, h);
    let (x, side, panel_x, panel_w) = columns(s, w, h, panel);
    let cover = cover_side(s, w, h).min(side);
    let rest = if compact { PLAYER_COMPACT_REST } else { PLAYER_REST };
    let top = TITLE_H + ((h - TITLE_H - 48.0 - (cover + rest)) * 0.5).max(0.0);
    let ctl = controls_w(cover).min(side);
    let full_title = t.full_title();
    let seed = seed_of(&t.id);

    let come = come.clamp(0.0, 1.0);
    let curtain = ease_out(come);
    let show_controls = front || come > 0.28;
    let (c_info, c_seek, c_ctrl, c_wave, p_anim) = if settled {
        (1.0, 1.0, 1.0, 1.0, 1.0)
    } else if front {
        (
            smooth((come - 0.12) / 0.56),
            smooth((come - 0.20) / 0.56),
            smooth((come - 0.26) / 0.56),
            smooth((come - 0.34) / 0.56),
            smooth((come - 0.18) / 0.64),
        )
    } else {
        (
            smooth((come - 0.30) / 0.62),
            smooth((come - 0.34) / 0.58),
            smooth((come - 0.38) / 0.54),
            smooth((come - 0.42) / 0.50),
            smooth((come - 0.32) / 0.60),
        )
    };

    let mut root = el().key("np").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).instant_changes();
    if fx::ready() {
        root = root.child(el().key("np-canvas").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).fill(th::bg().alpha(curtain)).motion(rune::Spring::NONE));
        // Bridge the first-ever Now Playing open while `stage.wgsl` finishes its one-time background
        // compile so expanding the player never flashes a flat black frame.
        if !fx::stage_ready()
            && let Some(stage_px) = State::cover_key(t.cover_template()).and_then(|k| s.art.soft_stage(&k, th::canvas(), th::light()))
        {
            root = root.child(
                el().key("np-stage-bridge").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).opacity(curtain).pixels_fit(&stage_px, rune::SurfaceFit::Fill),
            );
        }
        let stage = el().key("np-stage").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).fx(fx::STAGE).fx_data([
            curtain,
            seed % 100.0 + 100.0 * side.round(),
            (x + side * 0.5) / w.max(1.0),
            (top + cover * 0.5) / h.max(1.0),
        ]);
        root = root.child(stage);
    } else if s.settings.backdrop.on
        && let Some(bd) = &s.backdrop
    {
        let dim = s.settings.backdrop.dim.clamp(0.0, 1.0);
        root = root
            .child(el().key("np-stage").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).opacity(curtain).pixels_fit(&bd.pixels, rune::SurfaceFit::Fill))
            .child(el().key("np-stage-dim").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).fill(th::rgba(0x000000, dim * 0.85 * curtain)));
    } else if let Some(stage_px) = State::cover_key(t.cover_template()).and_then(|k| s.art.soft_stage(&k, th::canvas(), th::light())) {
        // Single opaque pre-blended backdrop quad: zero full-window alpha overdraw at rest, and
        // leaf opacity fades cleanly during open/close without allocating an offscreen layer.
        root = root.child(
            el().key("np-stage").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).opacity(curtain).pixels_fit(&stage_px, rune::SurfaceFit::Fill),
        );
    } else {
        root = root.child(el().key("np-stage").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).fill(th::bg().alpha(curtain)));
    }
    if !show_controls {
        return root;
    }

    let artist = kit::artist_line_width(s, t, th::BODY, true, "np-art", ctl);
    let mine = row().items_center().child(kit::np_like_button(s, t, SMALL_BTN, SMALL_GLYPH)).child(
        kit::quiet_btn("np-more", &np_icons::MORE, SMALL_GLYPH, SMALL_BTN, th::text3(), S::More.get(lang))
            .on_press(|s: &mut State| s.open_player_menu(rune::PopupAnchor::Key("np-more".into()))),
    );
    let inset = (SMALL_BTN - SMALL_GLYPH) * 0.5;
    let head = row()
        .w(ctl + inset)
        .h(34.0)
        .items_center()
        .child(col().grow(1.0).min_w(0.0).child(kit::roll("np-title", &full_title, kit::track_title(s, t, "np-title-link", th::H2, (ctl - 2.0 * SMALL_BTN).max(60.0)))))
        .child(mine);
    let info = stagger_el(col().w(ctl).h(58.0).gap(2.0).child(head).child(kit::roll("np-artist", &t.artists_line(), artist)), c_info, if front { 20.0 } else { 16.0 }, 0.975);

    let times = stagger_el(
        row()
            .w(ctl)
            .h(22.0)
            .items_center()
            .child(label("0:00", th::MONO, th::text3()).live_text(&s.lives.clock).w(52.0))
            .child(row().grow(1.0).h_full())
            .child(label("-0:00", th::MONO, th::text3()).live_text(&s.lives.remaining).w(52.0).align(rune::Align::End)),
        c_seek,
        if front { 22.0 } else { 18.0 },
        1.0,
    );
    let edge = (MODE_BTN - MODE_GLYPH) * 0.5;
    let repeat_icon = if s.player.repeat == Repeat::One { &np_icons::REPEAT_ONE } else { &np_icons::REPEAT };
    let transport = stagger_el(
        row()
            .w(ctl + 2.0 * edge)
            .h(56.0)
            .items_center()
            .justify_between()
            .child(kit::quiet_toggle("np-shuffle", &np_icons::SHUFFLE, MODE_GLYPH, MODE_BTN, s.player.shuffle, S::Shuffle.get(lang)).on_press(|s: &mut State| s.toggle_shuffle()))
            .child(
                row()
                    .items_center()
                    .gap(TRANSPORT_GAP)
                    .child(kit::skip_btn(s, false, "np-prev", &np_icons::PREV, SKIP_GLYPH, BTN + 4.0, th::text()))
                    .child(kit::np_play_btn(s, 60.0, 38.0))
                    .child(kit::skip_btn(s, true, "np-next", &np_icons::NEXT, SKIP_GLYPH, BTN + 4.0, th::text())),
            )
            .child(kit::quiet_toggle("np-repeat", repeat_icon, MODE_GLYPH, MODE_BTN, s.player.repeat != Repeat::Off, S::Repeat.get(lang)).on_press(|s: &mut State| s.cycle_repeat())),
        c_ctrl,
        if front { 26.0 } else { 20.0 },
        0.97,
    );
    let v = s.player.volume;
    let vol_icon = if v < 0.01 {
        &np_icons::VOLUME_MUTE
    } else if v < 0.5 {
        &np_icons::VOLUME_LOW
    } else {
        &np_icons::VOLUME
    };
    let panel_toggle = |key: &'static str, ic: &'static rune::Icon, name: &str, which: NpTab| {
        let on = tab == which;
        kit::quiet_toggle(key, ic, MODE_GLYPH, MODE_BTN, on, name).on_press(move |s: &mut State| {
            let next = if s.np_panel() == which { NpTab::Closed } else { which };
            s.set_np_tab(next);
        })
    };
    let lyrics_btn = if s.lyrics_missing() {
        kit::quiet_off("np-lyrics-btn", &np_icons::LYRICS, MODE_GLYPH, MODE_BTN, S::NoLyricsShort.get(lang))
    } else {
        panel_toggle("np-lyrics-btn", &np_icons::LYRICS, S::Lyrics.get(lang), NpTab::Lyrics)
    };
    let utility_w = ctl + 2.0 * edge;
    let vol_w = (utility_w - 4.0 * MODE_BTN - 12.0).max(72.0);
    let utility = stagger_el(
        row()
            .w(utility_w)
            .h(40.0)
            .items_center()
            .child(kit::quiet_btn("np-mute", vol_icon, MODE_GLYPH, MODE_BTN, th::text3(), S::Mute.get(lang)).on_wheel(dock::volume_wheel).on_press(State::toggle_mute))
            .child(row().ml(4.0).mr(8.0).items_center().child(dock::volume_slider(s, "np-vol", vol_w)))
            .child(dock::quality_btn_at(s, "np-quality", MODE_BTN, th::text3()))
            .child(lyrics_btn)
            .child(panel_toggle("np-queue-btn", &np_icons::QUEUE, S::Queue.get(lang), NpTab::Queue)),
        c_ctrl,
        if front { 28.0 } else { 22.0 },
        0.98,
    );

    let slide = if s.is_resizing() || !settled { rune::Spring::NONE } else { th::SLIDE };
    let cover_motion = if settled { slide } else { th::FLIGHT };
    // On collapse (`!front`), replace `np-cover` with a layout spacer so RUNE's `matched("np-cover")`
    // immediately flies the cover back to Home or the bottom dock while the controls fold down.
    let cover_slot = if front {
        let cov = kit::playing_cover_lg(s, t, cover, (cover * 0.075).clamp(14.0, 28.0), "np-cover-mark", settled).key("np-cover");
        if fx::cpu_mode() { cov } else { cov.matched("np-cover").animated_changes().motion(cover_motion) }
    } else {
        el().key("np-cover-gap").w(cover).h(cover)
    };
    let mut left = col()
        .key("np-left")
        .absolute(x, top)
        .w(side)
        .items_center()
        .transition(Transition::none())
        .animated_changes()
        .motion(slide)
        .child(row().key("np-cover-place").w_full().h(cover).justify_center().motion(slide).child(cover_slot))
        .child(info.mt(20.0))
        .child(row().w(ctl).mt(10.0).offset(0.0, (1.0 - c_seek) * if front { 22.0 } else { 18.0 }).instant_changes().child(dock::seek_bars_alpha(
            s,
            "seek",
            seed,
            ctl,
            36.0,
            false,
            c_seek,
        )))
        .child(times)
        .child(transport.mt(-8.0))
        .child(utility.mt(12.0));
    if !compact {
        let mut choices = vec![WaveChoice { station: Station::Track(t.id.clone()), kind: S::ByTrack.get(lang).to_owned(), name: full_title.clone() }];
        for a in t.artists.iter().take(3) {
            choices.push(WaveChoice { station: Station::Artist(a.id.clone()), kind: S::ByArtist.get(lang).to_owned(), name: a.name.clone() });
        }
        if let Some(al) = t.albums.first() {
            choices.push(WaveChoice { station: Station::Album(al.id.clone()), kind: S::ByRelease.get(lang).to_owned(), name: al.title.clone() });
        }
        let waves = kit::wave_card_alpha(s, "np-wave", S::WaveByTrack.get(lang), &full_title, Station::Track(t.id.clone()), Some(ctl), Some(choices), c_wave)
            .offset(0.0, (1.0 - c_wave) * if front { 30.0 } else { 24.0 });
        left = left.child(waves.mt(16.0));
    }
    root = root.child(left);

    let body_w = panel_w.min(LYRICS_MAX);
    let body = if front || p_anim > 0.02 {
        match tab {
            NpTab::Closed => None,
            NpTab::Lyrics => Some(lyrics::lyrics_view(s, &p, 0.0, body_w, h)),
            NpTab::Queue => Some(queue::queue_view(s, &p, 0.0, body_w, h)),
        }
    } else {
        None
    };
    if let Some(body) = body {
        let switching = s.np_tab_prev != NpTab::Closed && s.np_tab_prev != tab;
        let springs = rune::SpringPair { enter: rune::Spring::new(if switching { 0.26 } else { 0.28 }, 0.0), exit: rune::Spring::new(0.16, 0.0) };
        let enter = if switching { Transition::scale(0.985).combined(Transition::offset(0.0, 12.0)) } else { Transition::opacity().combined(Transition::offset(22.0, 0.0)) };
        let exit = Transition::scale(0.985).combined(Transition::offset(0.0, -6.0));
        let mut pane = el()
            .key(format!("np-body-{tab:?}"))
            .absolute(panel_x, 0.0)
            .w(body_w)
            .h(h)
            .offset((1.0 - p_anim) * if front { 28.0 } else { 20.0 }, 0.0)
            .animated_changes()
            .motion(slide)
            .transition(if settled { Transition::asymmetric(enter, exit).springs(springs) } else { Transition::none() });
        if p_anim < 0.999 {
            pane = pane.opacity(p_anim);
        }
        if settled && !switching {
            pane = pane.enter_delay(0.04);
        }
        root = root.child(pane.child(body.instant_changes()));
    }
    root
}

pub(super) fn notice(p: &Pal, x: f32, w: f32, h: f32, art: Ill, msg: &str) -> El {
    col().absolute(x, 0.0).w(w).h(h * 0.78).justify_center().child(kit::empty(p, art, msg))
}
