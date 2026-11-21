//! Settings dialog shell, tab navigation, and shared form building blocks.

mod general;
mod playback;
mod storage;

use std::sync::OnceLock;

use rune::{Align, Color, Cursor, El, Transition, Tx, col, el, row};
use rune_kit::{ControlSize, SegmentedControl, Switch};

use super::ds;
use super::icons;
use super::kit::{self, Hover as _, label};
use crate::app::State;
use crate::i18n::S;
use crate::theme::{self as th, veil};

pub(super) use playback::level_w;
pub(crate) use playback::preset_label;

/// Sections of Settings, in sidebar display order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SetTab {
    #[default]
    Account,
    Look,
    Playback,
    Lyrics,
    Keys,
    Desktop,
    Equalizer,
    Cache,
    Updates,
    About,
}

const TABS: [SetTab; 10] =
    [SetTab::Account, SetTab::Look, SetTab::Playback, SetTab::Lyrics, SetTab::Keys, SetTab::Desktop, SetTab::Equalizer, SetTab::Cache, SetTab::Updates, SetTab::About];

impl SetTab {
    fn icon(self) -> &'static rune::Icon {
        match self {
            SetTab::Account => &icons::USER,
            SetTab::Look => &icons::SPARKLE,
            SetTab::Playback => &icons::NOTE,
            SetTab::Lyrics => &icons::LYRICS,
            SetTab::Keys => &icons::KEYBOARD,
            SetTab::Desktop => &icons::WINDOW,
            SetTab::Equalizer => &icons::EQ,
            SetTab::Cache => &icons::DRIVE,
            SetTab::Updates => &icons::DOWNLOAD,
            SetTab::About => &icons::INFO,
        }
    }

    fn hue(self) -> u32 {
        match self {
            SetTab::Account | SetTab::Cache => 0x8e8e93,
            SetTab::Look => 0x5e5ce6,
            SetTab::Playback => 0xff375f,
            SetTab::Lyrics => 0xff9f0a,
            SetTab::Keys => 0xaf52de,
            SetTab::Desktop => 0x0a84ff,
            SetTab::Equalizer => 0x30b158,
            SetTab::Updates => 0x32ade6,
            SetTab::About => 0x636366,
        }
    }

    fn name(self) -> S {
        match self {
            SetTab::Account => S::Account,
            SetTab::Look => S::SetLook,
            SetTab::Playback => S::SetPlayback,
            SetTab::Lyrics => S::Lyrics,
            SetTab::Keys => S::SetKeys,
            SetTab::Desktop => S::SetDesktop,
            SetTab::Equalizer => S::Equalizer,
            SetTab::Cache => S::Storage,
            SetTab::Updates => S::Updates,
            SetTab::About => S::AboutApp,
        }
    }
}

impl State {
    pub fn open_settings(&mut self) {
        self.menu_open = false;
        self.close_popover();
        self.settings_open = true;
    }

    pub fn close_settings(&mut self) {
        self.settings_open = false;
        self.rebind = None;
        self.close_popover();
    }
}

pub(super) const PANE_PAD: f32 = 28.0;
pub(super) const ROW_PAD: f32 = 16.0;
pub(super) const GROUP_R: f32 = 12.0;

const TILE: u32 = 0x121212;
const PAPER: u32 = 0xf4f2ee;

/// Renders the settings dialog centered over a `w` x `h` window.
pub fn modal(s: &State, w: f32, h: f32) -> El {
    let k = ds::kit(s);
    let lang = s.lang;
    let tab = s.settings_tab;
    let cw = 900.0_f32.min(w - 48.0).max(320.0);
    let ch = 640.0_f32.min(h - 96.0).max(320.0);
    let narrow = cw < 700.0;

    let mut nav = col().key("set-nav").w(if narrow { 60.0 } else { 224.0 }).h_full().padding(10.0).gap(2.0).fill(veil(0.025));
    nav = nav.pt(if narrow { 56.0 } else { 18.0 });
    for t in TABS {
        let on = t == tab;
        let mut item = row()
            .key(format!("set-tab-{t:?}"))
            .role("tab")
            .label(t.name().get(lang))
            .checked(on)
            .w_full()
            .h(36.0)
            .px(if narrow { 0.0 } else { 8.0 })
            .gap(11.0)
            .radius(9.0)
            .items_center()
            .cursor(Cursor::Pointer)
            .fill(if on { veil(0.085) } else { Color::TRANSPARENT })
            .hov(veil(0.05))
            .press(veil(0.09))
            .on_press(move |s: &mut State| {
                s.settings_tab = t;
                s.rebind = None;
                s.close_popover();
            })
            .child(tab_tile(s, t));
        if narrow {
            item = item.justify_center().help(t.name().get(lang));
        } else {
            item = item.child(label(t.name().get(lang).to_owned(), if on { th::BODY_M } else { th::BODY }, if on { th::text() } else { th::text2() }).truncate().min_w(0.0));
        }
        if t == SetTab::Look {
            nav = nav.child(el().h(8.0));
        }
        nav = nav.child(item);
    }

    let groups: Vec<El> = match tab {
        SetTab::Account => general::account(s),
        SetTab::Look => general::appearance(s, &k),
        SetTab::Playback => playback::playback(s, &k),
        SetTab::Lyrics => playback::lyrics(s, &k),
        SetTab::Keys => general::shortcuts(s),
        SetTab::Desktop => general::desktop(s, &k),
        SetTab::Equalizer => playback::equalizer(s, &k),
        SetTab::Cache => storage::cache(s),
        SetTab::Updates => storage::updates(s, &k),
        SetTab::About => storage::about(s),
    };
    let list =
        col().key(format!("set-groups-{tab:?}")).w_full().gap(26.0).transition(Transition::opacity().combined(Transition::offset(0.0, 6.0)).springs(th::RISE)).children(groups);
    let close = kit::ibtn("set-close", &icons::CLOSE, 16.0, 32.0, th::text2(), S::Close.get(lang)).on_press(|s: &mut State| s.close_settings());
    let head = row()
        .key("set-head")
        .w_full()
        .h(64.0)
        .pl(PANE_PAD)
        .pr(14.0)
        .items_center()
        .child(label(tab.name().get(lang).to_owned(), th::H1, th::text()).grow(1.0).min_w(0.0).truncate())
        .child(close);
    let pane = col().key("set-pane").grow(1.0).min_w(0.0).h_full().child(head).child(
        col()
            .key("set-scroll")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .scroll(rune::ScrollSpec { stick_bottom: false })
            .scroll_edge_fade(16.0)
            .child(col().w_full().px(PANE_PAD).pt(4.0).pb(PANE_PAD).child(list)),
    );
    let card = row()
        .key("set-card")
        .role("dialog")
        .label(S::Settings.get(lang))
        .w(cw)
        .h(ch)
        .radius(18.0)
        .clip_rounded(18.0)
        .fill(th::pop())
        .border(1.0, th::hairline())
        .shadow(18.0, 56.0, th::rgba(0x000000, if th::light() { 0.16 } else { 0.5 }))
        .transition(Transition::opacity().combined(Transition::scale(0.96)).combined(Transition::offset(0.0, 8.0)).springs(th::RISE))
        .modal(true, |s: &mut State| s.close_settings())
        .on_press(|_s: &mut State| {})
        .child(nav)
        .child(el().w(1.0).h_full().fill(th::hairline()))
        .child(pane);
    el().key("settings-modal")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .center()
        .fill(th::rgba(0x000000, if th::light() { 0.24 } else { 0.56 }))
        .transition(Transition::opacity().springs(th::RISE))
        .on_press(|s: &mut State| s.close_settings())
        .child(card)
}

fn tab_tile(s: &State, t: SetTab) -> El {
    if t == SetTab::Account {
        let first = s
            .account
            .as_ref()
            .map(|a| if a.display_name.is_empty() { &a.login } else { &a.display_name })
            .and_then(|n| n.chars().next())
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default();
        return super::dock::account_picture(s, 24.0, &first);
    }
    row().w(24.0).h(24.0).radius(7.0).center().fill(th::rgb(t.hue())).child(rune::icon(t.icon(), 14.0).color(Color::WHITE))
}

pub(super) fn group(key: &str, title: Option<&str>, rows: Vec<El>, foot: Option<&str>) -> El {
    let mut plate = col().key(format!("{key}-plate")).w_full().radius(GROUP_R).clip_rounded(GROUP_R).fill(veil(0.035)).border(1.0, veil(0.065));
    for (i, r) in rows.into_iter().enumerate() {
        if i > 0 {
            plate = plate.child(row().w_full().h(1.0).pl(ROW_PAD).child(el().grow(1.0).h(1.0).fill(veil(0.07))));
        }
        plate = plate.child(r);
    }
    let mut g = col().key(key.to_owned()).w_full();
    if let Some(t) = title {
        g = g.child(label(t.to_owned(), th::LABEL, th::text3()).pl(ROW_PAD).pb(8.0));
    }
    g = g.child(plate);
    if let Some(f) = foot {
        g = g.child(label(f.to_owned(), th::SMALL, th::text3()).wrap().align(Align::Start).px(ROW_PAD).pt(8.0));
    }
    g
}

pub(super) fn words(name: &str, note: Option<&str>) -> El {
    let mut text = col().grow(1.0).min_w(0.0).gap(3.0).child(label(name.to_owned(), th::BODY_M, th::text()));
    if let Some(n) = note {
        text = text.child(label(n.to_owned(), th::SMALL, th::text2()).wrap().align(Align::Start));
    }
    text
}

pub(super) fn item(name: &str, note: Option<&str>, control: El) -> El {
    row().w_full().min_h(54.0).px(ROW_PAD).py(12.0).gap(24.0).items_center().child(words(name, note)).child(control)
}

pub(super) fn item_below(name: &str, note: Option<&str>, control: El) -> El {
    col().w_full().px(ROW_PAD).py(14.0).gap(12.0).child(words(name, note)).child(row().w_full().child(control))
}

pub(super) fn segmented(k: &rune_kit::Kit, names: &[&str], current: usize, pick: impl Fn(&mut State, usize) + Clone + 'static) -> El {
    SegmentedControl::new(names, Some(current)).fit().on_pick(move |s: &mut State, i: usize, _tx: &mut Tx| pick(s, i)).build(k)
}

pub(super) fn toggle(k: &rune_kit::Kit, on: bool, name: &str, f: impl Fn(&mut State) + 'static) -> El {
    Switch::new(on).label(name).hide_label().control_size(ControlSize::Small).on_toggle(f).build(k)
}

pub(super) fn danger_btn(key: &str, name: &str) -> El {
    let red = th::rgb(if th::light() { 0xd93a2b } else { 0xff6b5e });
    row()
        .key(key.to_owned())
        .role("button")
        .label(name)
        .h(32.0)
        .pl(10.0)
        .pr(12.0)
        .gap(7.0)
        .radius(kit::ctl_radius(32.0))
        .items_center()
        .cursor(Cursor::Pointer)
        .fill(red.alpha(0.12))
        .hov(red.alpha(0.18))
        .press(red.alpha(0.24))
        .press_scale(th::PRESS)
        .child(rune::icon(&icons::TRASH, 15.0).color(red))
        .child(label(name.to_owned(), th::BODY_M, red))
}

pub(super) fn figure(text: String) -> El {
    label(text, th::TIME, th::text3())
}

/// Glint's app icon tile at `size` points, reusing cached pebble and glint outlines.
pub(super) fn app_mark(size: f32) -> El {
    type MarkOutline = (Vec<[f32; 2]>, Vec<[f32; 2]>);
    static MARK: OnceLock<MarkOutline> = OnceLock::new();
    let (pebble, glint) = MARK.get_or_init(|| (crate::mark::pebble(160), crate::mark::glint(160)));
    let (tile, paper) = (th::rgb(TILE), th::rgb(PAPER));
    let k = size * 0.78 / crate::mark::GRID;
    let at = size * 0.11;
    let place = |pts: &[[f32; 2]]| pts.iter().map(|&[x, y]| [at + x * k, at + y * k]).collect::<Vec<_>>();
    let shape = rune::Shape::new().fill(place(pebble), paper).fill(place(glint), tile);
    el().w(size).h(size).radius(size * 0.27).fill(tile).border(1.0, Color::WHITE.alpha(0.08)).child(rune::canvas(shape).absolute(0.0, 0.0).w(size).h(size))
}
