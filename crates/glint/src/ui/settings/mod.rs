//! Minimalist Settings dialog shell, 6-tab navigation, instant search, and shared form controls.

mod general;
mod playback;
mod storage;

use std::cell::RefCell;
use std::sync::OnceLock;

use rune::{Align, Color, Cursor, El, Transition, Tx, col, el, row, text_input};
use rune_kit::{ControlSize, SegmentedControl, Switch};

use super::ds;
use super::icons;
use super::kit::{self, Hover as _, label};
use super::menu;
use crate::app::State;
use crate::i18n::S;
use crate::keys;
use crate::theme::{self as th, veil};

pub(super) use playback::level_w;
pub(crate) use playback::preset_label;

/// Pages of Settings, in sidebar order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SetTab {
    #[default]
    General,
    Sound,
    Player,
    Keys,
    Storage,
    About,
}

const TABS: [SetTab; 6] = [SetTab::General, SetTab::Sound, SetTab::Player, SetTab::Keys, SetTab::Storage, SetTab::About];

impl SetTab {
    fn name(self) -> S {
        match self {
            SetTab::General => S::SetGeneral,
            SetTab::Sound => S::SetSound,
            SetTab::Player => S::SetPlayer,
            SetTab::Keys => S::SetKeys,
            SetTab::Storage => S::Storage,
            SetTab::About => S::AboutApp,
        }
    }

    fn starts_set(self) -> bool {
        matches!(self, SetTab::Storage)
    }
}

impl State {
    pub fn open_settings(&mut self) {
        self.menu_open = false;
        self.close_popover();
        self.blur_edits();
        self.settings_open = true;
        if self.settings.reduce_motion || crate::fx::cpu_mode() {
            self.settings_pop = crate::app::Pop { t: 1.0, v: 0.0 };
        }
        crate::diag_log::event("ui", &format!("settings open tab={:?}", self.settings_tab));
        self.log_mem("settings:open");
    }

    pub fn close_settings(&mut self) {
        self.settings_open = false;
        if self.settings.reduce_motion || crate::fx::cpu_mode() {
            self.settings_pop = crate::app::Pop::default();
        }
        self.settings_find.clear();
        self.settings_find.focused = false;
        self.settings_found = None;
        self.rebind = None;
        self.close_popover();
        crate::diag_log::event("ui", "settings close");
        self.log_mem("settings:close");
    }
}

pub(super) const PANE_PAD: f32 = 24.0;
pub(super) const ROW_PAD: f32 = 14.0;
pub(super) const GROUP_R: f32 = 10.0;

const TILE: u32 = 0x121212;
const PAPER: u32 = 0xf4f2ee;

/// Renders the settings dialog centered over a `w` x `h` window.
pub fn modal(s: &State, w: f32, h: f32) -> El {
    let k = ds::kit(s);
    let lang = s.lang;
    let tab = s.settings_tab;
    let cw = 828.0_f32.min(w - 48.0).max(320.0);
    let ch = 608.0_f32.min(h - 88.0).max(320.0);
    let narrow = cw < 680.0;

    FOUND.with_borrow_mut(|f| *f = s.settings_found.as_ref().map(|(key, _)| (key.clone(), s.pal().accent)));

    let mut nav = col()
        .key("set-nav")
        .w(if narrow { 56.0 } else { 204.0 })
        .h_full()
        .corners([16.0, 0.0, 0.0, 16.0])
        .padding(10.0)
        .gap(2.0)
        .fill(veil(0.02))
        .focusable(true)
        .tab_index(-1);
    nav = nav.pt(if narrow { 52.0 } else { 10.0 });
    let finding = !narrow && !s.settings_find.text.trim().is_empty();
    if !narrow {
        nav = nav.child(find_field(s)).child(el().h(8.0));
    }
    if finding {
        nav = nav.child(found_list(s));
    }
    for t in TABS.into_iter().filter(|_| !finding) {
        let on = t == tab;
        let mut item = row()
            .key(format!("set-tab-{t:?}"))
            .role("tab")
            .label(t.name().get(lang))
            .checked(on)
            .w_full()
            .h(34.0)
            .px(if narrow { 0.0 } else { 9.0 })
            .gap(10.0)
            .radius(8.0)
            .items_center()
            .cursor(Cursor::Pointer)
            .hov(veil(if on { 0.0 } else { 0.045 }))
            .press(veil(0.08))
            .press_scale(0.985)
            .on_press(move |s: &mut State| {
                s.settings_tab = t;
                s.settings_find.focused = false;
                s.rebind = None;
                s.close_popover();
            });
        if on {
            item = item.child(
                el().key("set-tab-pill")
                    .matched("set-tab-pill")
                    .matched_in_place()
                    .animated_changes()
                    .motion(th::SLIDE)
                    .absolute(0.0, 0.0)
                    .w_full()
                    .h_full()
                    .radius(8.0)
                    .fill(veil(0.08)),
            );
        }
        item = item.child(tab_tile(t, on));
        if narrow {
            item = item.justify_center().help(t.name().get(lang));
        } else {
            item = item.child(label(t.name().get(lang).to_owned(), if on { th::BODY_M } else { th::BODY }, if on { th::text() } else { th::text2() }).truncate().min_w(0.0));
        }
        if t.starts_set() {
            nav = nav.child(row().w_full().h(13.0).px(8.0).items_center().child(el().w_full().h(1.0).fill(th::hairline())));
        }
        nav = nav.child(item);
    }

    let groups: Vec<El> = match tab {
        SetTab::General => general::general(s, &k),
        SetTab::Sound => playback::sound(s, &k),
        SetTab::Player => playback::player(s, &k),
        SetTab::Keys => general::shortcuts(s),
        SetTab::Storage => storage::storage(s),
        SetTab::About => storage::about(s, &k),
    };
    let cpu = crate::fx::cpu_mode();
    let list = col()
        .key(format!("set-groups-{tab:?}"))
        .w_full()
        .gap(20.0)
        .animated_changes()
        .transition(if cpu { Transition::none() } else { Transition::asymmetric(Transition::offset(0.0, 6.0), Transition::none()).springs(th::RISE) })
        .children(groups);

    let reset_action = match tab {
        SetTab::Player if menu::look_changed(s) => Some(ghost_action("set-look-reset", S::StageReset.get(lang), menu::reset_look)),
        SetTab::Keys if keys::changed(&s.settings) => Some(ghost_action("keys-reset", S::StageReset.get(lang), State::reset_keys)),
        _ => None,
    };
    let close = kit::ibtn("set-close", &icons::CLOSE, 15.0, 30.0, th::text2(), S::Close.get(lang)).on_press(|s: &mut State| s.close_settings());
    let head = row()
        .key("set-head")
        .w_full()
        .h(50.0)
        .pl(PANE_PAD)
        .pr(12.0)
        .gap(8.0)
        .items_center()
        .child(label(tab.name().get(lang).to_owned(), th::H3, th::text()).grow(1.0).min_w(0.0).truncate())
        .children(reset_action)
        .child(close);
    let scroll = col()
        .key(format!("set-scroll-{tab:?}"))
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(rune::ScrollSpec { stick_bottom: false })
        .child(col().w_full().px(PANE_PAD).py(20.0).child(list));
    let pane = col()
        .key("set-pane")
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .focusable(true)
        .tab_index(-1)
        .autofocus()
        .child(head)
        .child(el().w_full().h(1.0).fill(th::hairline()))
        .child(scroll);

    let t = if s.headless && s.settings_open && s.settings_pop.t == 0.0 { 1.0 } else { s.settings_pop.t.clamp(0.0, 1.0) };
    let e = t * t * (3.0 - 2.0 * t);
    let mut card = row()
        .key("set-card")
        .role("dialog")
        .label(S::Settings.get(lang))
        .w(cw)
        .h(ch)
        .radius(16.0)
        .fill(th::pop())
        .border(1.0, veil(0.11))
        .offset(0.0, (1.0 - e) * 14.0)
        .scale(0.96 + 0.04 * e)
        .transition(Transition::none())
        .animated_changes()
        .on_press(|_s: &mut State| {})
        .child(nav.instant_changes())
        .child(el().w(1.0).h_full().fill(th::hairline()))
        .child(pane.instant_changes());
    if e < 0.999 {
        card = card.opacity(e);
    }
    if s.settings_open {
        card = card.modal(true, |s: &mut State| s.close_settings());
    }
    let mut backdrop = el()
        .key("settings-modal")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .center()
        .fill(th::rgba(0x000000, (if th::light() { 0.22 } else { 0.54 }) * e))
        .transition(Transition::none())
        .instant_changes();
    if s.settings_open {
        backdrop = backdrop.on_press(|s: &mut State| s.close_settings());
    }
    backdrop.child(card)
}

fn ghost_action(key: &str, text: &str, f: fn(&mut State)) -> El {
    row()
        .key(key.to_owned())
        .role("button")
        .label(text)
        .h(26.0)
        .px(10.0)
        .radius(7.0)
        .center()
        .cursor(Cursor::Pointer)
        .hov(veil(0.06))
        .press(veil(0.11))
        .press_scale(th::PRESS)
        .on_press(f)
        .child(label(text.to_owned(), th::SMALL, th::text2()))
}

thread_local! {
    static FOUND: RefCell<Option<(String, Color)>> = const { RefCell::new(None) };
}

/// The key of a setting's row, made from its name.
pub(crate) fn item_key(name: &str) -> String {
    format!("set-item-{name}")
}

/// A soft highlight under the row opened from Settings search.
pub(super) fn found_mark(key: &str) -> Option<El> {
    let accent = FOUND.with_borrow(|f| f.as_ref().filter(|(k, _)| k == key).map(|(_, c)| *c))?;
    Some(
        el().key(format!("{key}-found"))
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .radius(GROUP_R - 1.0)
            .fill(accent.alpha(0.16))
            .animated_changes()
            .transition(Transition::opacity().springs(th::SWAP)),
    )
}

struct Entry {
    tab: SetTab,
    name: S,
    also: Option<S>,
    key: Option<String>,
}

fn entries(s: &State) -> Vec<Entry> {
    let lang = s.lang;
    let page = |tab: SetTab| Entry { tab, name: tab.name(), also: None, key: None };
    let set = |tab: SetTab, name: S, also: Option<S>| Entry { tab, name, also, key: Some(item_key(name.get(lang))) };
    let at = |tab: SetTab, name: S, key: &str| Entry { tab, name, also: None, key: Some(key.to_owned()) };
    let mut v = vec![
        page(SetTab::General),
        at(SetTab::General, S::Account, "set-account"),
        at(SetTab::General, S::SignOut, "set-account"),
        set(SetTab::General, S::Theme, Some(S::SetAppearance)),
        set(SetTab::General, S::Accent, Some(S::SetAppearance)),
        set(SetTab::General, S::Language, Some(S::SetAppearance)),
        set(SetTab::General, S::Shaders, Some(S::SetGraphics)),
        set(SetTab::General, S::Butterflies, Some(S::SetGraphics)),
        set(SetTab::General, S::CpuMode, Some(S::CpuModeNote)),
        set(SetTab::General, S::ReduceMotion, Some(S::SetGraphics)),
        set(SetTab::General, S::Autostart, Some(S::SetSystem)),
        set(SetTab::General, S::TrayIcon, Some(S::SetSystem)),
    ];
    if s.settings.butterflies && s.settings.shaders && !crate::fx::cpu_mode() && !s.settings.reduce_motion {
        v.push(set(SetTab::General, S::ButterflyCount, Some(S::SetGraphics)));
    }
    if s.settings.show_tray {
        v.push(set(SetTab::General, S::CloseToTray, Some(S::SetSystem)));
        if s.settings.autostart {
            v.push(set(SetTab::General, S::AutostartHidden, Some(S::SetSystem)));
        }
    }
    v.extend([
        page(SetTab::Sound),
        set(SetTab::Sound, S::Quality, None),
        set(SetTab::Sound, S::FckCensor, Some(S::FckCensorNote)),
        set(SetTab::Sound, S::PlayOnLaunch, None),
        set(SetTab::Sound, S::EqEnabled, Some(S::Equalizer)),
    ]);
    if s.player.eq.enabled {
        v.push(set(SetTab::Sound, S::EqPreset, Some(S::Equalizer)));
    }
    v.extend([
        page(SetTab::Player),
        set(SetTab::Player, S::PlayerLayout, Some(S::SetPlayerScreen)),
        set(SetTab::Player, S::Effects, Some(S::SetPlayerScreen)),
        set(SetTab::Player, S::Backdrop, Some(S::SetPlayerScreen)),
    ]);
    if s.settings.backdrop.on {
        v.extend([
            set(SetTab::Player, S::BdBlur, Some(S::SetPlayerScreen)),
            set(SetTab::Player, S::BdDim, Some(S::SetPlayerScreen)),
            set(SetTab::Player, S::BdEffect, Some(S::SetPlayerScreen)),
        ]);
    }
    v.extend([
        set(SetTab::Player, S::WordLyrics, Some(S::SetLyrics)),
        set(SetTab::Player, S::LyricsFont, Some(S::SetLyrics)),
        set(SetTab::Player, S::LyricSize, Some(S::SetLyrics)),
        set(SetTab::Player, S::LyricAlign, Some(S::SetLyrics)),
        set(SetTab::Player, S::MiniStyle, Some(S::MiniPlayer)),
        set(SetTab::Player, S::MiniAlwaysOnTop, Some(S::MiniPlayer)),
        set(SetTab::Player, S::MiniOpacity, Some(S::MiniPlayer)),
    ]);
    if s.settings.show_tray && s.settings.close_to_tray {
        v.push(set(SetTab::Player, S::MiniOnClose, Some(S::MiniPlayer)));
    }
    v.push(page(SetTab::Keys));
    v.extend(keys::Act::ALL.into_iter().map(|a| set(SetTab::Keys, a.name(), Some(S::SetKeys))));
    v.extend([
        set(SetTab::Keys, S::ScBack, Some(S::SetKeys)),
        page(SetTab::Storage),
        at(SetTab::Storage, S::StorageUsed, "store-used"),
        set(SetTab::Storage, S::SearchHistory, Some(S::SetHistory)),
        set(SetTab::Storage, S::ListeningHistory, Some(S::SetHistory)),
        page(SetTab::About),
        at(SetTab::About, S::Updates, "set-version"),
        set(SetTab::About, S::AutoUpdate, Some(S::Updates)),
        set(SetTab::About, S::DiagLogs, Some(S::Diagnostics)),
        at(SetTab::About, S::SourceCode, "set-source"),
        at(SetTab::About, S::ReportIssue, "set-issues"),
        at(SetTab::About, S::License, "set-license"),
    ]);
    v
}

fn found(s: &State) -> Vec<Entry> {
    let Some(q) = crate::find::Query::new(&s.settings_find.text) else { return Vec::new() };
    let lang = s.lang;
    let mut all: Vec<Option<Entry>> = entries(s).into_iter().map(Some).collect();
    let tiers = all.iter().flatten().map(|e| q.tier(&[e.name.get(lang)]).or_else(|| e.also.filter(|a| q.exact(&[a.get(lang)])).map(|_| 0)));
    crate::find::best(tiers.collect::<Vec<_>>()).into_iter().filter_map(|i| all[i].take()).collect()
}

fn open_found(s: &mut State, tab: SetTab, key: Option<String>, tx: &mut Tx) {
    s.settings_tab = tab;
    s.rebind = None;
    s.close_popover();
    s.settings_found = None;
    if let Some(key) = key {
        tx.scroll_center_in(format!("set-scroll-{tab:?}"), key.clone());
        s.settings_found = Some((key, s.now + 1.6));
    }
}

fn find_field(s: &State) -> El {
    let p = s.pal();
    let lang = s.lang;
    let input = text_input(&s.settings_find, th::BODY)
        .key("set-find")
        .single_line()
        .grow(1.0)
        .min_w(0.0)
        .color(th::text())
        .placeholder(S::FindSetting.get(lang), th::text3())
        .caret(s.caret_color(s.settings_find.focused, p.accent), p.accent.alpha(0.30))
        .on_edit(|s: &mut State| &mut s.settings_find)
        .on_submit(|s: &mut State, tx: &mut Tx| {
            if let Some(first) = found(s).into_iter().next() {
                open_found(s, first.tab, first.key, tx);
            }
        });
    let clear = (!s.settings_find.text.is_empty()).then(|| kit::search_clear("set-find-clear", lang).on_press(|s: &mut State| s.settings_find.clear()));
    kit::search_field("set-find-field", 32.0, input, clear).w_full()
}

fn found_list(s: &State) -> El {
    let lang = s.lang;
    let hits = found(s);
    if hits.is_empty() {
        return label(S::NoResults.get(lang).to_owned(), th::SMALL, th::text3()).px(8.0).pt(6.0);
    }
    let mut list = col().key("set-found").w_full().gap(2.0);
    for (i, e) in hits.into_iter().take(12).enumerate() {
        let on = e.key.is_none() && e.tab == s.settings_tab;
        let lit = e.key.is_some() && e.key == s.settings_found.as_ref().map(|(k, _)| k.clone());
        let name = e.name.get(lang);
        let mut words = col().grow(1.0).min_w(0.0).gap(1.0).child(label(name.to_owned(), th::BODY, if on || lit { th::text() } else { th::text2() }).truncate().min_w(0.0));
        if e.key.is_some() {
            words = words.child(label(e.tab.name().get(lang).to_owned(), th::SMALL, th::text3()).truncate().min_w(0.0));
        }
        let (tab, key) = (e.tab, e.key);
        list = list.child(
            row()
                .key(format!("set-hit-{i}"))
                .role("button")
                .label(name)
                .w_full()
                .min_h(34.0)
                .px(8.0)
                .py(5.0)
                .gap(10.0)
                .radius(8.0)
                .items_center()
                .cursor(Cursor::Pointer)
                .fill(if on || lit { veil(0.08) } else { Color::TRANSPARENT })
                .hov(veil(0.045))
                .press(veil(0.08))
                .on_press_tx(move |s: &mut State, tx: &mut Tx| open_found(s, tab, key.clone(), tx))
                .child(tab_tile(tab, on || lit))
                .child(words),
        );
    }
    list
}

fn tab_tile(t: SetTab, active: bool) -> El {
    row().w(22.0).h(22.0).center().child(super::shapes::settings_icon(t, active))
}

pub(super) fn group(key: &str, title: Option<&str>, rows: Vec<El>, foot: Option<&str>) -> El {
    let mut plate = col().key(format!("{key}-plate")).w_full().radius(GROUP_R).fill(veil(0.028)).border(1.0, veil(0.055)).instant_changes();
    for (i, r) in rows.into_iter().enumerate() {
        if i > 0 {
            plate = plate.child(row().w_full().h(1.0).pl(ROW_PAD).child(el().grow(1.0).h(1.0).fill(veil(0.055))));
        }
        plate = plate.child(r);
    }
    let mut g = col().key(key.to_owned()).w_full();
    if let Some(t) = title {
        g = g.child(label(t.to_owned(), th::LABEL, th::text3()).pl(4.0).pb(6.0));
    }
    g = g.child(plate);
    if let Some(f) = foot {
        g = g.child(label(f.to_owned(), th::SMALL, th::text3()).wrap().align(Align::Start).px(4.0).pt(7.0));
    }
    g
}

fn words_dim(name: &str, note: Option<&str>, dim: bool) -> El {
    let title_c = if dim { th::text().alpha(0.42) } else { th::text() };
    let note_c = if dim { th::text2().alpha(0.42) } else { th::text2() };
    let mut text = col().grow(1.0).min_w(0.0).gap(2.0).child(label(name.to_owned(), th::BODY, title_c));
    if let Some(n) = note {
        text = text.child(label(n.to_owned(), th::SMALL, note_c).wrap().align(Align::Start));
    }
    text
}

pub(super) fn item(name: &str, note: Option<&str>, control: El) -> El {
    item_enabled(name, note, control, true)
}

pub(super) fn item_enabled(name: &str, note: Option<&str>, control: El, enabled: bool) -> El {
    let key = item_key(name);
    row()
        .key(key.clone())
        .w_full()
        .min_h(46.0)
        .px(ROW_PAD)
        .py(9.0)
        .gap(16.0)
        .items_center()
        .children(found_mark(&key))
        .child(words_dim(name, note, !enabled))
        .child(control)
}

pub(super) fn item_below(name: &str, note: Option<&str>, control: El) -> El {
    let key = item_key(name);
    col()
        .key(key.clone())
        .w_full()
        .px(ROW_PAD)
        .py(11.0)
        .gap(9.0)
        .children(found_mark(&key))
        .child(words_dim(name, note, false))
        .child(row().w_full().child(control))
}

pub(super) fn segmented(k: &rune_kit::Kit, names: &[&str], current: usize, pick: impl Fn(&mut State, usize) + Clone + 'static) -> El {
    SegmentedControl::new(names, Some(current)).fit().on_pick(move |s: &mut State, i: usize, _tx: &mut Tx| pick(s, i)).build(k)
}

pub(super) fn toggle(k: &rune_kit::Kit, on: bool, name: &str, f: impl Fn(&mut State) + 'static) -> El {
    Switch::new(on).label(name).hide_label().control_size(ControlSize::Small).on_toggle(f).build(k)
}

pub(super) fn toggle_enabled(k: &rune_kit::Kit, on: bool, enabled: bool, name: &str, f: impl Fn(&mut State) + 'static) -> El {
    Switch::new(on).label(name).hide_label().control_size(ControlSize::Small).disabled(!enabled).on_toggle(f).build(k)
}

pub(super) fn danger_btn(key: &str, name: &str) -> El {
    let red = th::rgb(if th::light() { 0xd93a2b } else { 0xff6b5e });
    row()
        .key(key.to_owned())
        .role("button")
        .label(name)
        .h(28.0)
        .px(10.0)
        .gap(6.0)
        .radius(7.0)
        .items_center()
        .cursor(Cursor::Pointer)
        .fill(red.alpha(0.11))
        .hov(red.alpha(0.18))
        .press(red.alpha(0.24))
        .press_scale(th::PRESS)
        .child(rune::icon(&icons::TRASH, 13.0).color(red))
        .child(label(name.to_owned(), th::SMALL, red))
}

pub(super) fn figure(text: String) -> El {
    label(text, th::TIME, th::text3())
}

/// Glint's app icon tile at `size` points, reusing cached pebble and glint outlines.
pub(crate) fn app_mark(size: f32) -> El {
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
