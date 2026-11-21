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
use crate::app::{NpTab, Popover, State, Station, WaveChoice};
use crate::fx;
use crate::i18n::S;
use crate::store::{PlayerLayout, Repeat};
use crate::theme::{self as th, Pal};

pub use count::{count_start, counting};
pub use lyrics::{step_lines, wiping};
pub use queue::queue_offset;

const SMALL_GLYPH: f32 = 19.0;
const SMALL_BTN: f32 = 34.0;
const MODE_GLYPH: f32 = 19.0;
const MODE_BTN: f32 = 34.0;
const SKIP_GLYPH: f32 = 24.0;
const TRANSPORT_GAP: f32 = 18.0;

const PLAYER_REST: f32 = 20.0 + 58.0 + 14.0 + 22.0 + 16.0 + 56.0 + 12.0 + 40.0 + 16.0 + 56.0;
const PLAYER_COMPACT_REST: f32 = 20.0 + 58.0 + 14.0 + 22.0 + 16.0 + 56.0 + 12.0 + 40.0;

const CONTROLS_W: f32 = 360.0;
const CONTROLS_MAX: f32 = 440.0;
const COVER_MIN: f32 = 96.0;

const PLAYER_SHARE: f32 = 1.0 / 3.0;
const PLAYER_MIN: f32 = 300.0;
const CONTROLS_MIN: f32 = 280.0;
const PANEL_GAP: f32 = 20.0;
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
    let third = (w * PLAYER_SHARE).max(PLAYER_MIN).min(w * 0.5);
    let pad = (third * 0.09).clamp(16.0, 56.0);
    let widest = cover_max(layout(s, h)).max(CONTROLS_MAX);
    let side = (third - 2.0 * pad).clamp(CONTROLS_MIN, widest).min(cover_side(s, w, h).max(CONTROLS_W)).min(third - PAGE_PAD - 4.0);
    let x = ((third - side) * 0.5).max(PAGE_PAD);
    // Keep at least as much space between the player and the side panel as on the player's left.
    let panel_x = (third + PANEL_GAP).max(x + side + x);
    let panel_w = (w - panel_x - PAGE_PAD).max(200.0);
    if panel_w <= QUEUE_MAX {
        return (x, side, panel_x, panel_w);
    }
    let gap = panel_x - x - side;
    let shift = ((w - side - gap - QUEUE_MAX) * 0.5 - x).max(0.0);
    (x + shift, side, panel_x + shift, panel_w - shift)
}

/// Returns `(cover_x, cover_side)` across a `w` x `h` window.
pub fn cover_span(s: &State, w: f32, h: f32) -> (f32, f32) {
    let (x, side, _, _) = columns(s, w, h, s.np_panel() != NpTab::Closed);
    let cover = cover_side(s, w, h).min(side);
    (x + (side - cover) * 0.5, cover)
}

/// Renders the Now Playing overlay.
pub fn page(s: &State, w: f32, h: f32, come: f32, front: bool) -> El {
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

    let artist = kit::artist_line_width(s, t, th::BODY, true, "np-art", ctl);
    let mine = row().items_center().child(kit::np_like_button(s, t, SMALL_BTN, SMALL_GLYPH)).child(
        kit::quiet_btn("np-more", &np_icons::MORE_V, SMALL_GLYPH, SMALL_BTN, th::text3(), S::More.get(lang))
            .on_press(|s: &mut State| s.open_player_menu(rune::PopupAnchor::Key("np-more".into()))),
    );
    let inset = (SMALL_BTN - SMALL_GLYPH) * 0.5;
    let head = row()
        .w(ctl + inset)
        .h(34.0)
        .items_center()
        .child(col().grow(1.0).min_w(0.0).child(kit::roll("np-title", &full_title, kit::track_title(s, t, "np-title-link", th::H2, (ctl - 2.0 * SMALL_BTN).max(60.0)))))
        .child(mine);
    let info = col().w(ctl).h(58.0).gap(2.0).child(head).child(kit::roll("np-artist", &t.artists_line(), artist));

    let times = row()
        .w(ctl)
        .h(22.0)
        .items_center()
        .justify_between()
        .child(label("0:00", th::MONO, th::text3()).live_text(&s.lives.clock))
        .child(label("-0:00", th::MONO, th::text3()).live_text(&s.lives.remaining));
    let edge = (MODE_BTN - MODE_GLYPH) * 0.5;
    let align_inset = MODE_GLYPH * 0.5 + MODE_BTN;
    let min_span = 30.0 + TRANSPORT_GAP + (BTN + 4.0) + MODE_BTN * 0.5;
    let mode_gap = (ctl * 0.5 - align_inset - min_span).max(12.0);
    let repeat_icon = if s.player.repeat == Repeat::One { &np_icons::REPEAT_ONE } else { &np_icons::REPEAT };
    let transport = row()
        .w(ctl + 2.0 * edge)
        .h(56.0)
        .items_center()
        .justify_center()
        .gap(mode_gap)
        .child(kit::quiet_toggle("np-shuffle", &np_icons::SHUFFLE, MODE_GLYPH, MODE_BTN, s.player.shuffle, S::Shuffle.get(lang)).on_press(|s: &mut State| s.toggle_shuffle()))
        .child(
            row()
                .items_center()
                .gap(TRANSPORT_GAP)
                .child(kit::skip_btn(s, false, "np-prev", &np_icons::PREV, SKIP_GLYPH, BTN + 4.0, th::text()))
                .child(kit::np_play_btn(s, 60.0, 38.0))
                .child(kit::skip_btn(s, true, "np-next", &np_icons::NEXT, SKIP_GLYPH, BTN + 4.0, th::text())),
        )
        .child(kit::quiet_toggle("np-repeat", repeat_icon, MODE_GLYPH, MODE_BTN, s.player.repeat != Repeat::Off, S::Repeat.get(lang)).on_press(|s: &mut State| s.cycle_repeat()));
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
        kit::quiet_toggle(key, ic, MODE_GLYPH, MODE_BTN, on, name).on_press(move |s: &mut State| s.np_tab = if s.np_panel() == which { NpTab::Closed } else { which })
    };
    let lyrics_btn = if s.lyrics_missing() {
        kit::quiet_off("np-lyrics-btn", &np_icons::LYRICS, MODE_GLYPH, MODE_BTN, S::NoLyricsShort.get(lang))
    } else {
        panel_toggle("np-lyrics-btn", &np_icons::LYRICS, S::Lyrics.get(lang), NpTab::Lyrics)
    };
    let utility = row()
        .w(ctl + 2.0 * edge)
        .h(40.0)
        .items_center()
        .child(kit::quiet_btn("np-mute", vol_icon, MODE_GLYPH, MODE_BTN, th::text3(), S::Mute.get(lang)).on_press(|s: &mut State| s.toggle_mute()))
        .child(row().grow(1.0).min_w(0.0).ml(4.0).mr(16.0).child(kit::slider("vol", &s.lives.volume, 3.0, th::text2(), |s: &mut State, f: f32, _| s.set_volume(f))))
        .child(lyrics_btn)
        .child(panel_toggle("np-queue-btn", &np_icons::QUEUE, S::Queue.get(lang), NpTab::Queue))
        .child(look_button(s));

    let slide = if s.is_resizing() { rune::Spring::NONE } else { th::SLIDE };
    let mut left = col()
        .key("np-left")
        .absolute(x, top)
        .w(side)
        .items_center()
        .transition(Transition::opacity().springs(th::LEAVE))
        .motion(slide)
        .child(
            row()
                .key("np-cover-place")
                .w_full()
                .h(cover)
                .justify_center()
                .motion(slide)
                .child(kit::playing_cover(s, t, cover, (cover * 0.075).clamp(14.0, 28.0), "np-cover-mark").key("np-cover").matched("np-cover").motion(th::FLIGHT)),
        )
        .child(kit::rise(info.mt(20.0), 1))
        .child(row().w(ctl).mt(10.0).child(dock::seek_bars("seek", seed, 36.0)))
        .child(kit::rise(times, 2))
        .child(kit::rise(transport.mt(-8.0), 2))
        .child(kit::rise(utility.mt(12.0), 3));
    if !compact {
        let mut choices = vec![WaveChoice { station: Station::Track(t.id.clone()), kind: S::ByTrack.get(lang).to_owned(), name: full_title.clone() }];
        for a in t.artists.iter().take(3) {
            choices.push(WaveChoice { station: Station::Artist(a.id.clone()), kind: S::ByArtist.get(lang).to_owned(), name: a.name.clone() });
        }
        if let Some(al) = t.albums.first() {
            choices.push(WaveChoice { station: Station::Album(al.id.clone()), kind: S::ByRelease.get(lang).to_owned(), name: al.title.clone() });
        }
        let waves = kit::wave_card(s, "np-wave", S::WaveByTrack.get(lang), &full_title, Station::Track(t.id.clone()), Some(ctl), Some(choices));
        left = left.child(kit::rise(waves.mt(16.0), 4));
    }

    let body_w = panel_w.min(LYRICS_MAX);
    let body = match tab {
        NpTab::Closed => None,
        NpTab::Lyrics => Some(lyrics::lyrics_view(s, &p, 0.0, body_w, h)),
        NpTab::Queue => Some(queue::queue_view(s, &p, 0.0, body_w, h)),
    };
    let come = come.clamp(0.0, 1.0);
    let stage = el().key("np-stage").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).fx(fx::STAGE).fx_data([
        come,
        seed % 100.0 + 100.0 * side.round(),
        (x + side * 0.5) / w.max(1.0),
        (top + cover * 0.5) / h.max(1.0),
    ]);
    let mut root = el()
        .key("np")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .transition(Transition::none())
        .child(el().key("np-canvas").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).fill(th::bg().alpha(come)).motion(rune::Spring::NONE))
        .child(stage);
    if !front {
        return root;
    }
    root = root.child(left);
    if let Some(body) = body {
        root = root.child(
            el().key(format!("np-body-{tab:?}"))
                .absolute(panel_x, 0.0)
                .w(body_w)
                .h(h)
                .motion(rune::Spring::NONE)
                .transition(Transition::opacity().combined(Transition::offset(24.0, 0.0)).springs(th::SOFT))
                .enter_delay(0.14)
                .child(body),
        );
    }
    root
}

fn look_button(s: &State) -> El {
    let open = s.popover_open && matches!(s.popover, Some(Popover::Stage { .. }));
    kit::quiet_toggle("np-look", &np_icons::LOOK, MODE_GLYPH, MODE_BTN, open, S::StageLook.get(s.lang))
        .on_press(|s: &mut State| s.open_popover(Popover::Stage { anchor: "np-look".into() }))
}

pub(super) fn notice(p: &Pal, x: f32, w: f32, h: f32, art: Ill, msg: &str) -> El {
    col().absolute(x, 0.0).w(w).h(h * 0.78).justify_center().child(kit::empty(p, art, msg))
}
