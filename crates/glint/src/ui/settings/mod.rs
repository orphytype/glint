//! Settings dialog shell, tab navigation, and shared form building blocks.

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
use crate::app::State;
use crate::i18n::S;
use crate::theme::{self as th, veil};

pub(super) use playback::level_w;
pub(crate) use playback::preset_label;

/// Pages of Settings, in sidebar order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SetTab {
    #[default]
    Account,
    Look,
    Sound,
    Player,
    Mini,
    Startup,
    Keys,
    Storage,
    About,
}

const TABS: [SetTab; 9] =
    [SetTab::Account, SetTab::Look, SetTab::Sound, SetTab::Player, SetTab::Mini, SetTab::Startup, SetTab::Keys, SetTab::Storage, SetTab::About];

impl SetTab {
    fn icon(self) -> &'static rune::Icon {
        match self {
            SetTab::Account => &icons::USER,
            SetTab::Look => &icons::SPARKLE,
            SetTab::Sound => &icons::EQ,
            SetTab::Player => &icons::STAGE,
            SetTab::Mini => &icons::MINI_PLAYER,
            SetTab::Startup => &icons::WINDOW,
            SetTab::Keys => &icons::KEYBOARD,
            SetTab::Storage => &icons::DRIVE,
            SetTab::About => &icons::INFO,
        }
    }

    fn hue(self) -> u32 {
        match self {
            SetTab::Account | SetTab::Storage => 0x8e8e93,
            SetTab::Look => 0x5e5ce6,
            SetTab::Sound => 0xff375f,
            SetTab::Player => 0xff9f0a,
            SetTab::Mini => 0x0a84ff,
            SetTab::Startup => 0x30b158,
            SetTab::Keys => 0xaf52de,
            SetTab::About => 0x636366,
        }
    }

    fn name(self) -> S {
        match self {
            SetTab::Account => S::Account,
            SetTab::Look => S::SetLook,
            SetTab::Sound => S::SetSound,
            SetTab::Player => S::SetPlayerScreen,
            SetTab::Mini => S::MiniPlayer,
            SetTab::Startup => S::SetStartup,
            SetTab::Keys => S::SetKeys,
            SetTab::Storage => S::Storage,
            SetTab::About => S::AboutApp,
        }
    }

    /// A small gap above it in the sidebar: it starts a new set of pages.
    fn starts_set(self) -> bool {
        matches!(self, SetTab::Look | SetTab::Startup | SetTab::About)
    }
}

impl State {
    pub fn open_settings(&mut self) {
        self.menu_open = false;
        self.close_popover();
        self.settings_open = true;
        crate::diag_log::event("ui", &format!("settings open tab={:?}", self.settings_tab));
        self.log_mem("settings:open");
    }

    pub fn close_settings(&mut self) {
        self.settings_open = false;
        self.settings_find.clear();
        // Clearing keeps the field's focus; without this its caret shows the next time Settings
        // opens, in a field nobody clicked.
        self.settings_find.focused = false;
        self.settings_found = None;
        self.rebind = None;
        self.close_popover();
        crate::diag_log::event("ui", "settings close");
        self.log_mem("settings:close");
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
    let ch = 680.0_f32.min(h - 96.0).max(320.0);
    let narrow = cw < 700.0;

    FOUND.with_borrow_mut(|f| *f = s.settings_found.as_ref().map(|(key, _)| (key.clone(), s.pal().accent)));

    let mut nav = col().key("set-nav").w(if narrow { 60.0 } else { 224.0 }).h_full().corners([18.0, 0.0, 0.0, 18.0]).padding(10.0).gap(2.0).fill(veil(0.025));
    nav = nav.pt(if narrow { 56.0 } else { 10.0 });
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
        if t.starts_set() {
            nav = nav.child(el().h(8.0));
        }
        nav = nav.child(item);
    }

    let groups: Vec<El> = match tab {
        SetTab::Account => general::account(s),
        SetTab::Look => general::appearance(s, &k),
        SetTab::Sound => playback::sound(s, &k),
        SetTab::Player => playback::player(s, &k),
        SetTab::Mini => general::mini(s, &k),
        SetTab::Startup => general::startup(s, &k),
        SetTab::Keys => general::shortcuts(s),
        SetTab::Storage => storage::storage(s),
        SetTab::About => storage::about(s, &k),
    };
    let list = col()
        .key(format!("set-groups-{tab:?}"))
        .w_full()
        .gap(26.0)
        .animated_changes()
        .transition(Transition::asymmetric(Transition::offset(0.0, 6.0), Transition::none()).springs(th::RISE))
        .children(groups);
    let close = kit::ibtn("set-close", &icons::CLOSE, 16.0, 32.0, th::text2(), S::Close.get(lang)).on_press(|s: &mut State| s.close_settings());
    let head = row()
        .key("set-head")
        .w_full()
        // As tall as twice the sidebar's first row sits from the top: the page name lines up with it.
        .h(56.0)
        .pl(PANE_PAD)
        .pr(14.0)
        .items_center()
        .child(label(tab.name().get(lang).to_owned(), th::BODY_M, th::text()).grow(1.0).min_w(0.0).truncate())
        .child(close);
    let scroll = col()
        .key(format!("set-scroll-{tab:?}"))
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(rune::ScrollSpec { stick_bottom: false })
        .child(col().w_full().padding(PANE_PAD).child(list));
    let pane = col().key("set-pane").grow(1.0).min_w(0.0).h_full().child(head).child(el().w_full().h(1.0).fill(th::hairline())).child(scroll);
    let card = row()
        .key("set-card")
        .role("dialog")
        .label(S::Settings.get(lang))
        .w(cw)
        .h(ch)
        .radius(18.0)
        .fill(th::pop())
        .border(2.0, veil(0.12))
        .transition(kit::pop_in(0.97, 6.0, th::RISE))
        .animated_changes()
        .modal(true, |s: &mut State| s.close_settings())
        .on_press(|_s: &mut State| {})
        .child(nav.instant_changes())
        .child(el().w(1.0).h_full().fill(th::hairline()))
        .child(pane.instant_changes());
    el().key("settings-modal")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .center()
        .fill(th::rgba(0x000000, if th::light() { 0.24 } else { 0.56 }))
        .transition(Transition::none())
        .instant_changes()
        .on_press(|s: &mut State| s.close_settings())
        .child(card)
}

thread_local! {
    /// The row a search result just opened, and the accent to light it with. Set as the dialog
    /// is built, so the rows below can tell.
    static FOUND: RefCell<Option<(String, Color)>> = const { RefCell::new(None) };
}

/// The key of a setting's row, made from its name.
pub(crate) fn item_key(name: &str) -> String {
    format!("set-item-{name}")
}

/// A soft plate under the row a search result opened. It fades out on its own.
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

/// One thing the Settings search can find: a page, or a setting on it.
struct Entry {
    tab: SetTab,
    name: S,
    /// Searched too, not shown: the setting's note, or the group it is in.
    also: Option<S>,
    /// The row to scroll to and light up; None for a page.
    key: Option<String>,
}

/// Everything the Settings search looks through, in sidebar order.
fn entries(s: &State) -> Vec<Entry> {
    let lang = s.lang;
    let page = |tab: SetTab| Entry { tab, name: tab.name(), also: None, key: None };
    let set = |tab: SetTab, name: S, also: Option<S>| Entry { tab, name, also, key: Some(item_key(name.get(lang))) };
    let at = |tab: SetTab, name: S, key: &str| Entry { tab, name, also: None, key: Some(key.to_owned()) };
    let mut v = vec![
        page(SetTab::Account),
        at(SetTab::Account, S::SignOut, "set-account"),
        page(SetTab::Look),
        set(SetTab::Look, S::Theme, None),
        set(SetTab::Look, S::Accent, None),
        set(SetTab::Look, S::Effects, Some(S::EffectsNote)),
        set(SetTab::Look, S::Butterflies, None),
        set(SetTab::Look, S::ReduceMotion, None),
        set(SetTab::Look, S::Language, None),
        page(SetTab::Sound),
        set(SetTab::Sound, S::Quality, Some(S::QualityNote)),
        set(SetTab::Sound, S::FckCensor, Some(S::FckCensorNote)),
        set(SetTab::Sound, S::PlayOnLaunch, None),
        set(SetTab::Sound, S::EqEnabled, Some(S::Equalizer)),
        set(SetTab::Sound, S::EqPreset, Some(S::Equalizer)),
        page(SetTab::Player),
        set(SetTab::Player, S::PlayerLayout, None),
        set(SetTab::Player, S::Effects, None),
        set(SetTab::Player, S::Backdrop, Some(S::SetBackdrop)),
    ];
    if s.settings.backdrop.on {
        v.extend([
            set(SetTab::Player, S::BdBlur, Some(S::SetBackdrop)),
            set(SetTab::Player, S::BdDim, Some(S::SetBackdrop)),
            set(SetTab::Player, S::BdEffect, Some(S::SetBackdrop)),
        ]);
    }
    v.extend([
        set(SetTab::Player, S::LyricsFont, Some(S::SetLyrics)),
        set(SetTab::Player, S::LyricSize, Some(S::SetLyrics)),
        set(SetTab::Player, S::LyricAlign, Some(S::SetLyrics)),
        set(SetTab::Player, S::WordLyrics, Some(S::SetLyrics)),
        set(SetTab::Player, S::StageLook, Some(S::StageResetNote)),
        page(SetTab::Mini),
        set(SetTab::Mini, S::MiniShow, None),
        set(SetTab::Mini, S::MiniStyle, None),
        set(SetTab::Mini, S::MiniAlwaysOnTop, None),
        set(SetTab::Mini, S::MiniOpacity, None),
        set(SetTab::Mini, S::MiniOnClose, None),
        page(SetTab::Startup),
        set(SetTab::Startup, S::Autostart, Some(S::SetLaunch)),
        set(SetTab::Startup, S::AutostartHidden, Some(S::SetLaunch)),
        set(SetTab::Startup, S::TrayIcon, Some(S::SetTray)),
        set(SetTab::Startup, S::CloseToTray, Some(S::CloseToTrayNote)),
        page(SetTab::Keys),
    ]);
    v.extend(crate::keys::Act::ALL.into_iter().map(|a| set(SetTab::Keys, a.name(), Some(S::SetKeys))));
    v.extend([
        set(SetTab::Keys, S::ScBack, Some(S::SetKeys)),
        set(SetTab::Keys, S::KeysDefault, Some(S::SetKeys)),
        page(SetTab::Storage),
        set(SetTab::Storage, S::StorageContent, Some(S::StorageContentNote)),
        set(SetTab::Storage, S::SearchHistory, None),
        set(SetTab::Storage, S::ListeningHistory, None),
        page(SetTab::About),
        at(SetTab::About, S::Updates, "set-version"),
        set(SetTab::About, S::AutoUpdate, Some(S::Updates)),
        set(SetTab::About, S::DiagLogs, Some(S::DiagLogsNote)),
        set(SetTab::About, S::DiagLogFiles, Some(S::Diagnostics)),
        at(SetTab::About, S::SourceCode, "set-source"),
        at(SetTab::About, S::ReportIssue, "set-issues"),
        at(SetTab::About, S::License, "set-license"),
    ]);
    v
}

/// What the sidebar's search finds, closest matches first kept in sidebar order.
fn found(s: &State) -> Vec<Entry> {
    let Some(q) = crate::find::Query::new(&s.settings_find.text) else { return Vec::new() };
    let lang = s.lang;
    let mut all: Vec<Option<Entry>> = entries(s).into_iter().map(Some).collect();
    // The name may be near; a note or a group only counts when it holds the query as typed.
    let tiers = all.iter().flatten().map(|e| q.tier(&[e.name.get(lang)]).or_else(|| e.also.filter(|a| q.exact(&[a.get(lang)])).map(|_| 0)));
    crate::find::best(tiers.collect::<Vec<_>>()).into_iter().filter_map(|i| all[i].take()).collect()
}

/// Opens what a search result points at: its page, scrolled to its row, the row lit up.
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
        .caret(p.accent, p.accent.alpha(0.30))
        .on_edit(|s: &mut State| &mut s.settings_find)
        .on_submit(|s: &mut State, tx: &mut Tx| {
            if let Some(first) = found(s).into_iter().next() {
                open_found(s, first.tab, first.key, tx);
            }
        });
    let clear = (!s.settings_find.text.is_empty()).then(|| kit::search_clear("set-find-clear", lang).on_press(|s: &mut State| s.settings_find.clear()));
    kit::search_field("set-find-field", 34.0, input, clear).w_full()
}

/// The results in place of the page list: each setting's name, and the page it is on.
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
                .min_h(36.0)
                .px(8.0)
                .py(5.0)
                .gap(11.0)
                .radius(9.0)
                .items_center()
                .cursor(Cursor::Pointer)
                .fill(if on || lit { veil(0.085) } else { Color::TRANSPARENT })
                .hov(veil(0.05))
                .press(veil(0.09))
                .on_press_tx(move |s: &mut State, tx: &mut Tx| open_found(s, tab, key.clone(), tx))
                .child(tab_tile(s, tab))
                .child(words),
        );
    }
    list
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
    let mut plate = col().key(format!("{key}-plate")).w_full().radius(GROUP_R).fill(veil(0.035)).border(1.0, veil(0.065)).instant_changes();
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
    words_dim(name, note, false)
}

fn words_dim(name: &str, note: Option<&str>, dim: bool) -> El {
    let title_c = if dim { th::text().alpha(0.42) } else { th::text() };
    let note_c = if dim { th::text2().alpha(0.42) } else { th::text2() };
    let mut text = col().grow(1.0).min_w(0.0).gap(3.0).child(label(name.to_owned(), th::BODY_M, title_c));
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
        .min_h(54.0)
        .px(ROW_PAD)
        .py(12.0)
        .gap(24.0)
        .items_center()
        .children(found_mark(&key))
        .child(words_dim(name, note, !enabled))
        .child(control)
}

pub(super) fn toggle_enabled(k: &rune_kit::Kit, on: bool, enabled: bool, name: &str, f: impl Fn(&mut State) + 'static) -> El {
    Switch::new(on).label(name).hide_label().control_size(ControlSize::Small).disabled(!enabled).on_toggle(f).build(k)
}

pub(super) fn item_below(name: &str, note: Option<&str>, control: El) -> El {
    let key = item_key(name);
    col().key(key.clone()).w_full().px(ROW_PAD).py(14.0).gap(12.0).children(found_mark(&key)).child(words(name, note)).child(row().w_full().child(control))
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
