//! Settings, in a dialog over the window. On the left the sections, each with a small coloured
//! tile (the account with its picture); on the right the one picked: its name, then its settings
//! in groups, each group a plate of rows. A row is a name with a sentence under it that says what
//! it does, and the control on the right (or, for a wide one, under the words). A group may have
//! a small heading above it and a note below. The controls are RUNE's own kit (`ds.rs`).

use super::kit::Hover as _;
use rune::{Align, Color, Cursor, Drag, El, Icon, Transition, Tx, col, el, row};
use rune_kit::{ControlSize, SegmentedControl, Switch};
use ym_audio::{BAND_HZ, MAX_GAIN_DB};

use super::ds;
use super::icons;
use super::kit::{self, label};
use crate::app::{Session, State};
use crate::i18n::{Lang, S};
use crate::keys::{self, Act};
use crate::store::{Effects, PlayerLayout, QualityPref, ThemeMode};
use crate::theme::{self as th, ACCENTS, veil};

/// The sections of Settings, in the order the column lists them.
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

    /// The colour of the section's tile.
    fn hue(self) -> u32 {
        match self {
            SetTab::Account => 0x8e8e93,
            SetTab::Look => 0x5e5ce6,
            SetTab::Playback => 0xff375f,
            SetTab::Lyrics => 0xff9f0a,
            SetTab::Keys => 0xaf52de,
            SetTab::Desktop => 0x0a84ff,
            SetTab::Equalizer => 0x30b158,
            SetTab::Cache => 0x8e8e93,
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

/// The settings dialog over a window `w` by `h`.
pub fn modal(s: &State, w: f32, h: f32) -> El {
    let k = ds::kit(s);
    let lang = s.lang;
    let tab = s.settings_tab;
    let cw = 900.0_f32.min(w - 48.0).max(320.0);
    let ch = 640.0_f32.min(h - 96.0).max(320.0);
    let narrow = cw < 700.0;

    // The sections, each a tile and a name; the one open sits on a plate.
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
        // A little air between the account and the rest, as between groups.
        if t == SetTab::Look {
            nav = nav.child(el().h(8.0));
        }
        nav = nav.child(item);
    }

    // The section open: its name, then its groups.
    let groups: Vec<El> = match tab {
        SetTab::Account => account(s),
        SetTab::Look => appearance(s, &k),
        SetTab::Playback => playback(s, &k),
        SetTab::Lyrics => lyrics(s, &k),
        SetTab::Keys => shortcuts(s),
        SetTab::Desktop => desktop(s, &k),
        SetTab::Equalizer => equalizer(s, &k),
        SetTab::Cache => cache(s),
        SetTab::Updates => updates(s, &k),
        SetTab::About => about(s),
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

// ---- building blocks ---------------------------------------------------------------------------------

/// The space at the sides of the open section.
const PANE_PAD: f32 = 28.0;
/// The inner side space of a group's rows.
const ROW_PAD: f32 = 16.0;
const GROUP_R: f32 = 12.0;

/// A section's mark in the column: a small tile of its colour with its glyph in white, or for the
/// account, its picture.
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

/// A group of rows on one plate, a hairline between each two (from where the words start), with
/// a small heading above it and a note below it when they are given.
fn group(key: &str, title: Option<&str>, rows: Vec<El>, foot: Option<&str>) -> El {
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

/// The words of a row: its name and the sentence under it.
fn words(name: &str, note: Option<&str>) -> El {
    let mut text = col().grow(1.0).min_w(0.0).gap(3.0).child(label(name.to_owned(), th::BODY_M, th::text()));
    if let Some(n) = note {
        text = text.child(label(n.to_owned(), th::SMALL, th::text2()).wrap().align(Align::Start));
    }
    text
}

/// A row: the words on the left, the control on the right.
fn item(name: &str, note: Option<&str>, control: El) -> El {
    row().w_full().min_h(54.0).px(ROW_PAD).py(12.0).gap(24.0).items_center().child(words(name, note)).child(control)
}

/// A row whose control is too wide to sit beside its words: under them instead.
fn item_below(name: &str, note: Option<&str>, control: El) -> El {
    col().w_full().px(ROW_PAD).py(14.0).gap(12.0).child(words(name, note)).child(row().w_full().child(control))
}

/// A segmented control over `names`, `current` lit; `pick` gets the index.
fn segmented(k: &rune_kit::Kit, names: &[&str], current: usize, pick: impl Fn(&mut State, usize) + Clone + 'static) -> El {
    SegmentedControl::new(names, Some(current)).fit().on_pick(move |s: &mut State, i: usize, _tx: &mut Tx| pick(s, i)).build(k)
}

fn toggle(k: &rune_kit::Kit, on: bool, name: &str, f: impl Fn(&mut State) + 'static) -> El {
    Switch::new(on).label(name).hide_label().control_size(ControlSize::Small).on_toggle(f).build(k)
}

/// The red of what cannot be undone.
fn danger_ink() -> Color {
    th::rgb(if th::light() { 0xd93a2b } else { 0xff6b5e })
}

/// A small button for what deletes something: red words on a breath of red.
fn danger_btn(key: &str, name: &str) -> El {
    let red = danger_ink();
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

/// A quiet figure beside a control (a size, a count).
fn figure(text: String) -> El {
    label(text, th::TIME, th::text3())
}

/// Glint's app icon, `size` points: the light pebble with its glint cut out, on a dark tile
/// (`crate::mark`; the icon files are drawn from the same outlines).
fn app_mark(size: f32) -> El {
    let (tile, paper) = (th::rgb(TILE), th::rgb(PAPER));
    // The mark takes 78% of the tile, in the middle, as in the icon files.
    let k = size * 0.78 / crate::mark::GRID;
    let at = size * 0.11;
    let place = |pts: Vec<[f32; 2]>| pts.into_iter().map(|[x, y]| [at + x * k, at + y * k]).collect::<Vec<_>>();
    let shape = rune::Shape::new().fill(place(crate::mark::pebble(160)), paper).fill(place(crate::mark::glint(160)), tile);
    el().w(size)
        .h(size)
        .radius(size * 0.27)
        .fill(tile)
        .border(1.0, Color::WHITE.alpha(0.08))
        .child(rune::canvas(shape).absolute(0.0, 0.0).w(size).h(size))
}

/// The app icon's tile and the pebble on it (`brand/brand.json`: ink and paper).
const TILE: u32 = 0x121212;
const PAPER: u32 = 0xf4f2ee;

// ---- account -----------------------------------------------------------------------------------------

/// Who is signed in, large on a plate of its own, then what Glint remembers of what was done.
fn account(s: &State) -> Vec<El> {
    let lang = s.lang;
    let (name, note, login) = match &s.account {
        Some(a) => {
            let shown = if a.display_name.is_empty() { a.login.clone() } else { a.display_name.clone() };
            (shown, S::SignedInAs.get(lang).to_owned(), Some(a.login.clone()))
        }
        None => (String::new(), String::new(), None),
    };
    // The Yandex ID is private: blurred, and read only while the pointer rests on it.
    let mut note = row().w_full().gap(5.0).items_center().min_w(0.0).child(label(note, th::SMALL, th::text2()).wrap().max_lines(2).shrink().min_w(0.0));
    if let Some(login) = login {
        let peek = s.id_peek;
        note = note.child(label("·".to_owned(), th::SMALL, th::text3())).child(
            label(login, th::SMALL, th::text2())
                .key("set-login")
                .truncate()
                .min_w(0.0)
                .blur(if peek { 0.0 } else { 4.0 })
                .opacity(if peek { 1.0 } else { 0.8 })
                .motion(rune::Spring::new(0.22, 0.0))
                .cursor(Cursor::Default)
                .help(if peek { "" } else { S::IdHidden.get(lang) })
                .on_hover_change(|s: &mut State, on: bool, _tx: &mut Tx| s.id_peek = on),
        );
    }
    let first = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    let mut title = row().gap(10.0).items_center().child(label(name, th::H1, th::text()).truncate().min_w(0.0));
    if s.account.as_ref().is_some_and(|a| a.has_plus) && s.session == Session::Live {
        title = title.child(kit::plus_badge(lang));
    }
    let who = row()
        .key("set-account")
        .w_full()
        .px(18.0)
        .py(18.0)
        .gap(16.0)
        .items_center()
        .radius(GROUP_R)
        .fill(veil(0.035))
        .border(1.0, veil(0.065))
        .child(super::dock::account_picture(s, 60.0, &first))
        .child(col().grow(1.0).min_w(0.0).gap(4.0).child(title).child(note))
        .child(kit::btn_small("set-signout", S::SignOut.get(lang), Some(&icons::SIGN_OUT)).on_press(|s: &mut State| s.sign_out()));

    let clear = |key: &str, empty: bool, f: fn(&mut State)| -> El {
        if empty {
            return figure(S::HistoryEmpty.get(lang).to_owned());
        }
        kit::btn_small(key.to_owned(), S::ClearShort.get(lang), None).on_press(move |s: &mut State| {
            f(s);
            let text = s.tr(S::HistoryCleared);
            s.toast(text);
        })
    };
    let searches = s.settings.search_history.len();
    let played = s.listening_history.len();
    let history = group(
        "set-history",
        Some(S::SetHistory.get(lang)),
        vec![
            item(S::SearchHistory.get(lang), Some(S::SearchHistoryNote.get(lang)), clear("set-clear-searches", searches == 0, State::clear_search_history)),
            item(S::ListeningHistory.get(lang), Some(S::ListeningHistoryNote.get(lang)), clear("set-clear-played", played == 0, State::clear_listening_history)),
        ],
        None,
    );
    vec![who, history]
}

// ---- appearance --------------------------------------------------------------------------------------

fn swatch(s: &State, i: usize) -> El {
    let on = s.settings.accent == i;
    let (name, hex) = ACCENTS[i];
    let dot = el().w(18.0).h(18.0).radius(9.0);
    let dot = if i == 0 {
        dot.linear_gradient(0.9, [(0.0, th::rgb(0x9b7bff)), (0.35, th::rgb(0x4cc6ff)), (0.68, th::rgb(0x2fe3b2)), (1.0, th::rgb(0xff5c96))])
    } else if i == ACCENTS.len() - 1 {
        dot.fill(th::text())
    } else {
        dot.fill(th::rgb(hex))
    };
    let label_text = if i == 0 { S::AccentAdaptive.get(s.lang) } else { name };
    el().key(format!("accent-{i}"))
        .role("button")
        .label(label_text)
        .help(label_text)
        .checked(on)
        .w(28.0)
        .h(28.0)
        .radius(14.0)
        .center()
        .cursor(Cursor::Pointer)
        .press_scale(th::PRESS)
        .border(2.0, if on { th::text().alpha(0.85) } else { Color::TRANSPARENT })
        .on_press(move |s: &mut State| {
            s.settings.accent = i;
            s.mark_settings_dirty();
        })
        .child(dot)
}

fn appearance(s: &State, k: &rune_kit::Kit) -> Vec<El> {
    let lang = s.lang;
    let modes = [ThemeMode::Dark, ThemeMode::Light, ThemeMode::System];
    let theme =
        segmented(k, &[S::ThemeDark.get(lang), S::ThemeLight.get(lang), S::ThemeSystem.get(lang)], modes.iter().position(|m| *m == s.settings.theme).unwrap_or(0), move |s, i| {
            s.settings.theme = modes[i];
            s.mark_settings_dirty();
        });
    let swatches = row().gap(4.0).items_center().children((0..ACCENTS.len()).map(|i| swatch(s, i)));
    let levels = [Effects::Full, Effects::Light, Effects::Off];
    let fx = segmented(
        k,
        &[S::EffectsFull.get(lang), S::EffectsLight.get(lang), S::EffectsOff.get(lang)],
        levels.iter().position(|e| *e == s.settings.effects).unwrap_or(0),
        move |s, i| {
            s.settings.effects = levels[i];
            s.mark_settings_dirty();
        },
    );
    let langs = [Lang::En, Lang::Ru];
    let language = segmented(k, &["English", "Русский"], langs.iter().position(|l| *l == s.lang).unwrap_or(0), move |s, i| s.set_lang(langs[i]));
    let motion = toggle(k, s.settings.reduce_motion, S::ReduceMotion.get(lang), |s| {
        s.settings.reduce_motion = !s.settings.reduce_motion;
        s.mark_settings_dirty();
    });
    let bugs = toggle(k, s.settings.butterflies, S::Butterflies.get(lang), |s| {
        s.settings.butterflies = !s.settings.butterflies;
        s.mark_settings_dirty();
    });
    vec![
        group("set-colour", None, vec![item(S::Theme.get(lang), None, theme), item(S::Accent.get(lang), Some(S::AccentNote.get(lang)), swatches)], None),
        group(
            "set-motion",
            None,
            vec![
                item(S::Effects.get(lang), Some(S::EffectsNote.get(lang)), fx),
                item(S::Butterflies.get(lang), Some(S::ButterfliesNote.get(lang)), bugs),
                item(S::ReduceMotion.get(lang), Some(S::ReduceMotionNote.get(lang)), motion),
            ],
            None,
        ),
        group("set-language", None, vec![item(S::Language.get(lang), None, language)], None),
    ]
}

// ---- playback ----------------------------------------------------------------------------------------

/// How the music sounds and how the screen that plays it looks. (Shuffle and repeat are the
/// player's own buttons: they have no place here.)
fn playback(s: &State, k: &rune_kit::Kit) -> Vec<El> {
    let lang = s.lang;
    let qs = [QualityPref::Saver, QualityPref::Balanced, QualityPref::Best, QualityPref::Lossless];
    let q = segmented(
        k,
        &[S::QualitySaver.get(lang), S::QualityBalanced.get(lang), S::QualityBest.get(lang), S::QualityLossless.get(lang)],
        qs.iter().position(|q| *q == s.settings.quality).unwrap_or(0),
        move |s, i| {
            s.settings.quality = qs[i];
            s.mark_settings_dirty();
        },
    );
    let layouts = [PlayerLayout::Auto, PlayerLayout::Comfortable, PlayerLayout::Compact, PlayerLayout::Cover];
    let layout = segmented(
        k,
        &[S::PlayerLayoutAuto.get(lang), S::PlayerLayoutComfortable.get(lang), S::PlayerLayoutCompact.get(lang), S::PlayerLayoutCover.get(lang)],
        layouts.iter().position(|layout| *layout == s.settings.player_layout).unwrap_or(0),
        move |s, i| {
            s.settings.player_layout = layouts[i];
            s.mark_settings_dirty();
        },
    );
    let censor = toggle(k, s.settings.fkcensor, S::FckCensor.get(lang), |s| s.set_fkcensor(!s.settings.fkcensor));
    let launch = toggle(k, s.settings.play_on_launch, S::PlayOnLaunch.get(lang), |s| {
        s.settings.play_on_launch = !s.settings.play_on_launch;
        s.mark_settings_dirty();
    });
    let mut screen = vec![item_below(S::PlayerLayout.get(lang), Some(S::PlayerLayoutNote.get(lang)), layout)];
    // The flash and the reaction, as the player's look panel has them.
    for (i, (name, opts)) in super::menu::stage_rows().into_iter().enumerate() {
        screen.push(item(name.get(lang), None, look_choice(s, k, i, opts)));
    }
    screen.extend(backdrop_rows(s, k));
    // The way back to the defaults, as in the panel: it covers the lyrics' look too.
    let changed = super::menu::look_changed(s);
    let reset = kit::btn_small("set-look-reset", S::StageReset.get(lang), None);
    let reset = if changed { reset.on_press(super::menu::reset_look) } else { reset.disabled(true).opacity(0.45) };
    screen.push(item(S::StageLook.get(lang), Some(S::StageResetNote.get(lang)), reset));
    vec![
        group(
            "set-sound",
            Some(S::SetSound.get(lang)),
            vec![
                item_below(S::Quality.get(lang), Some(S::QualityNote.get(lang)), q),
                item(S::FckCensor.get(lang), Some(S::FckCensorNote.get(lang)), censor),
                item(S::PlayOnLaunch.get(lang), Some(S::PlayOnLaunchNote.get(lang)), launch),
            ],
            None,
        ),
        group("set-screen", Some(S::SetPlayerScreen.get(lang)), screen, None),
    ]
}

/// Setting `i` of the player's look (as `menu::stage_value` numbers them) as a segmented control
/// over `opts`.
fn look_choice(s: &State, k: &rune_kit::Kit, i: usize, opts: [S; 3]) -> El {
    let names = opts.map(|o| o.get(s.lang));
    segmented(k, &names, usize::from(super::menu::stage_value(s, i)), move |s, v| super::menu::set_stage_value(s, i, v as u8))
}

/// The picture behind the player: choosing it, and once there is one, how soft and dark it is
/// and the effect over it.
fn backdrop_rows(s: &State, k: &rune_kit::Kit) -> Vec<El> {
    let lang = s.lang;
    let look = s.settings.backdrop;
    let pick = |key: &str, name: &str| kit::btn_small(key.to_owned(), name, None).on_press_tx(|s: &mut State, tx: &mut Tx| s.pick_backdrop(tx));
    if !look.on {
        return vec![item(S::Backdrop.get(lang), Some(S::BackdropNote.get(lang)), pick("bd-pick", S::BdPick.get(lang)))];
    }
    let buttons = row()
        .gap(8.0)
        .items_center()
        .child(pick("bd-change", S::BdChange.get(lang)))
        .child(kit::btn_small("bd-remove", S::BdRemove.get(lang), None).on_press(|s: &mut State| s.drop_backdrop()));
    let effect = segmented(k, &crate::backdrop::EFFECTS.map(|e| e.get(lang)), usize::from(look.effect), |s, i| {
        s.settings.backdrop.effect = i as u8;
        s.mark_settings_dirty();
    });
    vec![
        item(S::Backdrop.get(lang), Some(S::BackdropNote.get(lang)), buttons),
        item(
            S::BdBlur.get(lang),
            None,
            level("bd-blur", look.blur, |s, f| {
                s.settings.backdrop.blur = f;
                s.mark_settings_dirty();
            }),
        ),
        item(
            S::BdDim.get(lang),
            None,
            level("bd-dim", look.dim, |s, f| {
                s.settings.backdrop.dim = f;
                s.mark_settings_dirty();
            }),
        ),
        item_below(S::BdEffect.get(lang), None, effect),
    ]
}

/// The width of a level's track.
const LEVEL_W: f32 = 200.0;

/// A level from 0 to 1, set by pressing or dragging anywhere along its track.
fn level(key: &str, value: f32, set: impl Fn(&mut State, f32) + 'static) -> El {
    level_w(key, value, LEVEL_W, set)
}

/// The same level `lw` points wide (the player's look panel has its own width for it).
pub(super) fn level_w(key: &str, value: f32, lw: f32, set: impl Fn(&mut State, f32) + 'static) -> El {
    let v = value.clamp(0.0, 1.0);
    let ink = th::text();
    el().key(key.to_owned())
        .role("slider")
        .w(lw)
        .h(24.0)
        .cursor(Cursor::Pointer)
        .on_drag(move |s: &mut State, d: &mut Drag, _tx: &mut Tx| {
            let f = if d.w > 0.0 { (d.x / d.w).clamp(0.0, 1.0) } else { 0.0 };
            set(s, (f * 100.0).round() / 100.0);
        })
        .child(el().absolute(0.0, 10.0).w(lw).h(4.0).radius(2.0).fill(veil(0.12)))
        .child(el().absolute(0.0, 10.0).w((lw * v).max(4.0)).h(4.0).radius(2.0).fill(ink))
        .child(el().absolute((lw * v - 8.0).clamp(0.0, lw - 16.0), 4.0).w(16.0).h(16.0).radius(8.0).fill(Color::WHITE).shadow(1.0, 4.0, th::rgba(0x000000, 0.3)))
}

// ---- lyrics ------------------------------------------------------------------------------------------

/// How the lyrics look (the same settings as in the player's look panel), and word-by-word lyrics
/// from lrc.red.
fn lyrics(s: &State, k: &rune_kit::Kit) -> Vec<El> {
    let lang = s.lang;
    let word_lyrics = toggle(k, s.settings.word_lyrics, S::WordLyrics.get(lang), |s| s.set_word_lyrics(!s.settings.word_lyrics));
    let font = s.settings.lyric_look.font;
    let faces = segmented(k, &["Plex Mono", "Commissioner", "Cormorant"], usize::from(font), |s, i| {
        s.settings.lyric_look.font = i as u8;
        s.mark_settings_dirty();
    });
    // A line in the picked face under the choice, so it is seen before the player is opened.
    let sample = label(S::LyricsFontSample.get(lang), th::lyric_face(th::LYRIC_CARD, font), th::text()).key("set-lyrics-sample").w_full().wrap();
    let face =
        col().w_full().px(ROW_PAD).py(14.0).gap(12.0).child(words(S::LyricsFont.get(lang), Some(S::LyricsFontNote.get(lang)))).child(row().w_full().child(faces)).child(sample);
    let mut rows = vec![face];
    for (j, (name, opts)) in super::menu::lyric_rows().into_iter().enumerate() {
        rows.push(item(name.get(lang), None, look_choice(s, k, 2 + j, opts)));
    }
    rows.push(item(S::WordLyrics.get(lang), Some(S::WordLyricsNote.get(lang)), word_lyrics));
    vec![group("set-lyrics", None, rows, None)]
}

// ---- shortcuts ---------------------------------------------------------------------------------------

/// A key as it is drawn on the keyboard.
fn key_cap(text: &str) -> El {
    row().h(24.0).min_w(24.0).px(7.0).radius(6.0).center().fill(veil(0.07)).border(1.0, veil(0.10)).child(label(text.to_owned(), th::LABEL, th::text2()))
}

/// A row of the list: the action, and the keys it answers to as a button that waits for the new
/// key when pressed.
fn shortcut_row(s: &State, act: Act) -> El {
    let lang = s.lang;
    let waiting = s.rebind == Some(act);
    let name = act.name().get(lang);
    let mut caps = row().gap(8.0).items_center();
    if waiting {
        caps = caps.child(label(S::KeyPress.get(lang).to_owned(), th::SMALL, th::text()));
    } else {
        let all = keys::keycaps(&s.settings, act);
        if all.is_empty() {
            caps = caps.child(label(S::KeyNone.get(lang).to_owned(), th::SMALL, th::text3()));
        }
        for (i, chord) in all.iter().enumerate() {
            if i > 0 {
                caps = caps.child(label("/".to_owned(), th::SMALL, th::text3()));
            }
            caps = caps.child(row().gap(4.0).items_center().children(chord.iter().map(|c| key_cap(c))));
        }
    }
    let button = row()
        .key(format!("key-{}", act.id()))
        .role("button")
        .label(name)
        .h(34.0)
        .px(8.0)
        .radius(9.0)
        .items_center()
        .cursor(Cursor::Pointer)
        .fill(if waiting { veil(0.10) } else { Color::TRANSPARENT })
        .border(1.0, if waiting { th::text().alpha(0.4) } else { Color::TRANSPARENT })
        .hov(veil(0.06))
        .press(veil(0.10))
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            // A focused control would take Space and Enter before the recorder hears them.
            tx.blur();
            s.rebind = if s.rebind == Some(act) { None } else { Some(act) };
        })
        .child(caps);
    row().w_full().h(48.0).px(ROW_PAD).gap(12.0).items_center().child(label(name.to_owned(), th::BODY, th::text()).grow(1.0).min_w(0.0)).child(button)
}

/// What each key does, and a way to give an action another key. Esc is the one that stays.
fn shortcuts(s: &State) -> Vec<El> {
    let lang = s.lang;
    let mut rows: Vec<El> = Act::ALL.into_iter().map(|act| shortcut_row(s, act)).collect();
    rows.push(row().w_full().h(48.0).px(ROW_PAD).gap(12.0).items_center().child(label(S::ScBack.get(lang).to_owned(), th::BODY, th::text()).grow(1.0).min_w(0.0)).child(key_cap("Esc")));
    let reset = kit::btn_small("keys-reset", S::StageReset.get(lang), None);
    let reset = if keys::changed(&s.settings) { reset.on_press(|s: &mut State| s.reset_keys()) } else { reset.disabled(true).opacity(0.45) };
    vec![
        group("set-keys", None, rows, Some(S::KeyHint.get(lang))),
        group("set-keys-reset", None, vec![item(S::KeysDefault.get(lang), None, reset)], None),
    ]
}

// ---- windows -----------------------------------------------------------------------------------------

/// Starting with the computer, the tray icon, and closing into it.
fn desktop(s: &State, k: &rune_kit::Kit) -> Vec<El> {
    let lang = s.lang;
    let tray = s.settings.show_tray;
    let close = s.settings.close_to_tray && tray;
    let auto = s.settings.autostart;
    let hidden = s.settings.autostart_hidden && tray;
    vec![
        group(
            "set-autostart",
            None,
            vec![
                item(S::Autostart.get(lang), None, toggle(k, auto, S::Autostart.get(lang), |s| s.set_autostart(!s.settings.autostart))),
                // Waiting in a tray that isn't there would hide the app: dimmed until both are on.
                item(
                    S::AutostartHidden.get(lang),
                    Some(S::AutostartHiddenNote.get(lang)),
                    toggle(k, hidden, S::AutostartHidden.get(lang), |s| {
                        if s.settings.show_tray && s.settings.autostart {
                            s.settings.autostart_hidden = !s.settings.autostart_hidden;
                            s.mark_settings_dirty();
                        }
                    }),
                )
                .opacity(if tray && auto { 1.0 } else { 0.4 }),
            ],
            None,
        ),
        group(
            "set-desktop",
            None,
            vec![
                item(S::TrayIcon.get(lang), Some(S::TrayIconNote.get(lang)), toggle(k, tray, S::TrayIcon.get(lang), |s| s.set_show_tray(!s.settings.show_tray))),
                // Closing to a tray that isn't there would lose the window: dimmed until the icon is on.
                item(
                    S::CloseToTray.get(lang),
                    Some(S::CloseToTrayNote.get(lang)),
                    toggle(k, close, S::CloseToTray.get(lang), |s| {
                        if s.settings.show_tray {
                            s.set_close_to_tray(!s.settings.close_to_tray);
                        }
                    }),
                )
                .opacity(if tray { 1.0 } else { 0.4 }),
            ],
            None,
        ),
    ]
}

// ---- equaliser ---------------------------------------------------------------------------------------

const EQ_H: f32 = 132.0;
const EQ_PAD: f32 = 10.0;
const EQ_W: f32 = 36.0;

pub(crate) fn preset_label(name: &str, lang: Lang) -> String {
    match (name, lang) {
        ("flat", _) => S::Flat.get(lang).to_owned(),
        ("bass", _) => S::BassBoost.get(lang).to_owned(),
        ("treble", _) => S::Bright.get(lang).to_owned(),
        ("vocal", _) => S::Vocal.get(lang).to_owned(),
        ("rock", Lang::Ru) => "Рок".to_owned(),
        ("pop", Lang::Ru) => "Поп".to_owned(),
        ("electronic", Lang::Ru) => "Электроника".to_owned(),
        ("acoustic", Lang::Ru) => "Акустика".to_owned(),
        ("classical", Lang::Ru) => "Классика".to_owned(),
        ("loudness", Lang::Ru) => "Громкость".to_owned(),
        (other, _) => {
            let mut c = other.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
    }
}

fn hz_label(hz: f32) -> String {
    if hz >= 1000.0 {
        let k = hz / 1000.0;
        if k.fract() == 0.0 { format!("{}k", k as u32) } else { format!("{k}k") }
    } else if hz.fract() == 0.0 {
        format!("{}", hz as u32)
    } else {
        format!("{hz}")
    }
}

/// One vertical band slider: drag anywhere in the column to set the gain.
fn eq_band(s: &State, i: usize) -> El {
    let g = s.player.eq.gains_db[i];
    let enabled = s.player.eq.enabled;
    let usable = EQ_H - 2.0 * EQ_PAD;
    let knob_y = EQ_PAD + (0.5 - g / (2.0 * MAX_GAIN_DB)) * usable;
    let mid_y = EQ_PAD + 0.5 * usable;
    let (top, height) = if knob_y < mid_y { (knob_y, mid_y - knob_y) } else { (mid_y, knob_y - mid_y) };
    let color = if enabled { th::text() } else { th::text3() };
    let value = if g.abs() < 0.05 { "0".to_owned() } else { format!("{g:+.1}") };
    let mx = EQ_W * 0.5;

    let slider = el()
        .key(format!("eq-{i}"))
        .role("slider")
        .label(format!("{} Hz", hz_label(BAND_HZ[i])))
        .w(EQ_W)
        .h(EQ_H)
        .cursor(Cursor::Pointer)
        .on_drag(move |s: &mut State, d: &mut Drag, _tx: &mut Tx| {
            let f = ((d.y - EQ_PAD) / usable).clamp(0.0, 1.0);
            let mut gain = ((0.5 - f) * 2.0 * MAX_GAIN_DB * 2.0).round() / 2.0;
            if gain.abs() <= 0.5 {
                gain = 0.0;
            }
            if (s.player.eq.gains_db[i] - gain).abs() > f32::EPSILON {
                s.player.eq.gains_db[i] = gain;
                s.apply_eq();
            }
        })
        .child(el().absolute(mx - 1.0, EQ_PAD).w(2.0).h(usable).radius(1.0).fill(veil(0.14)))
        .child(el().absolute(mx - 8.0, mid_y - 0.5).w(16.0).h(1.0).fill(veil(0.26)))
        .child(el().absolute(mx - 1.5, top).w(3.0).h(height.max(0.5)).radius(1.5).fill(color))
        .child(el().absolute(mx - 6.0, knob_y - 6.0).w(12.0).h(12.0).radius(6.0).fill(color));

    col().key(format!("eq-col-{i}")).w(EQ_W).gap(4.0).items_center().child(label(value, th::MONO, if enabled { th::text2() } else { th::text3() })).child(slider).child(label(
        hz_label(BAND_HZ[i]),
        th::MONO,
        th::text3(),
    ))
}

fn equalizer(s: &State, k: &rune_kit::Kit) -> Vec<El> {
    let lang = s.lang;
    let enabled = s.player.eq.enabled;
    let current = s.player.eq.preset_name().map_or_else(|| "—".to_owned(), |n| preset_label(n, lang));
    let open = s.popover_open && matches!(s.popover, Some(crate::app::Popover::EqPreset { .. }));
    let presets = kit::btn_select("eq-preset", &current, 160.0, open).on_press(|s: &mut State| s.open_popover(crate::app::Popover::EqPreset { anchor: "eq-preset".into() }));
    let reset = kit::btn_small("eq-reset", S::EqReset.get(lang), None).on_press(|s: &mut State| {
        s.player.eq.gains_db = [0.0; 10];
        s.player.eq.preamp_db = 0.0;
        s.apply_eq();
    });
    let bands = row().w_full().px(ROW_PAD - 4.0).pt(14.0).pb(12.0).justify_between().opacity(if enabled { 1.0 } else { 0.45 }).children((0..BAND_HZ.len()).map(|i| eq_band(s, i)));
    let on = toggle(k, enabled, S::EqEnabled.get(lang), |s| {
        s.player.eq.enabled = !s.player.eq.enabled;
        s.apply_eq();
    });
    vec![
        group(
            "set-eq",
            None,
            vec![
                item(S::EqEnabled.get(lang), None, on),
                item(S::EqPreset.get(lang), None, row().gap(8.0).items_center().child(presets).child(reset)),
            ],
            None,
        ),
        group("set-eq-bands", Some(S::EqBands.get(lang)), vec![bands], None),
    ]
}

// ---- storage -----------------------------------------------------------------------------------------

/// `bytes` as people read a size, in the units and with the decimal mark of `lang`.
fn size(bytes: u64, lang: Lang) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let ru = lang == Lang::Ru;
    let b = bytes as f64;
    let (n, unit) = if b >= GB {
        (format!("{:.1}", b / GB), if ru { "ГБ" } else { "GB" })
    } else if b >= MB {
        (format!("{:.1}", b / MB), if ru { "МБ" } else { "MB" })
    } else if b >= KB {
        (format!("{:.0}", b / KB), if ru { "КБ" } else { "KB" })
    } else {
        (bytes.to_string(), if ru { "Б" } else { "B" })
    };
    let n = if ru { n.replace('.', ",") } else { n };
    format!("{n}\u{a0}{unit}")
}

/// The total on the storage plate: large, in the interface face, its figures of one width.
const TOTAL: rune::TextStyle = rune::TextStyle::new(30.0, 600.0).family(th::SANS).tracking(-0.6).lh(36.0).features("'tnum' 1");

/// The colours of the three kinds of data on the bar.
const STORE_HUES: [u32; 3] = [0x5e5ce6, 0x30b158, 0xff9f0a];

/// Deletes what Glint saved for offline: the downloaded tracks with their lyrics, and the covers.
fn delete_content(s: &mut State) {
    if let Some(d) = &s.cache {
        let _ = d.clear();
    }
    if let Some(d) = &s.download_store {
        let _ = d.clear();
    }
    s.disk_usage.set((0, 0, 0, f64::NEG_INFINITY));
    s.downloads.clear();
    s.download_titles.clear();
    s.download_activity = None;
}

/// How much Glint keeps on the disk, as one bar of its three kinds, and the one button that
/// deletes it.
fn cache(s: &State) -> Vec<El> {
    let lang = s.lang;
    // The figures come from walking whole folders, which is far too slow to repeat on every
    // build of the page. A few seconds of staleness is invisible; every clear resets the entry.
    let (images, audio, metadata) = {
        let (images, audio, metadata, at) = s.disk_usage.get();
        if s.now - at < 5.0 {
            (images, audio, metadata)
        } else {
            let images = s.cache.as_ref().map_or(0, |d| d.total_bytes());
            let (audio, metadata) = s.download_store.as_ref().map_or((0, 0), |d| d.usage());
            s.disk_usage.set((images, audio, metadata, s.now));
            (images, audio, metadata)
        }
    };
    let total = images + audio + metadata;
    let kinds = [(S::CacheImages, images), (S::CacheAudio, audio), (S::CacheMetadata, metadata)];

    let mut bar = row().key("store-bar").w_full().h(10.0).radius(5.0).clip_rounded(5.0).gap(2.0).fill(veil(0.07));
    if total > 0 {
        for (i, (_, bytes)) in kinds.iter().enumerate().filter(|(_, (_, b))| *b > 0) {
            bar = bar.child(el().key(format!("store-bar-{i}")).h_full().min_w(4.0).grow(*bytes as f32 / total as f32).fill(th::rgb(STORE_HUES[i])));
        }
    }
    let legend = row().w_full().gap(18.0).flex_wrap().children(kinds.iter().enumerate().map(|(i, (name, bytes))| {
        row()
            .gap(7.0)
            .items_center()
            .child(el().w(8.0).h(8.0).radius(4.0).fill(th::rgb(STORE_HUES[i])))
            .child(label(name.get(lang).to_owned(), th::SMALL, th::text2()))
            .child(figure(size(*bytes, lang)))
    }));
    let used = col()
        .key("store-used")
        .w_full()
        .padding(18.0)
        .gap(12.0)
        .radius(GROUP_R)
        .fill(veil(0.035))
        .border(1.0, veil(0.065))
        .child(col().gap(2.0).child(label(S::StorageUsed.get(lang).to_owned(), th::SMALL, th::text3())).child(label(size(total, lang), TOTAL, th::text())))
        .child(bar)
        .child(legend);

    let delete = if total > 0 {
        danger_btn("content-delete", S::DeleteContent.get(lang)).on_press(|s: &mut State| {
            delete_content(s);
            let text = s.tr(S::ContentDeleted);
            s.toast(text);
        })
    } else {
        figure(S::HistoryEmpty.get(lang).to_owned())
    };
    vec![used, group("set-store", None, vec![item(S::StorageContent.get(lang), Some(S::StorageContentNote.get(lang)), delete)], None)]
}

// ---- updates -----------------------------------------------------------------------------------------

/// The version that runs and whether there is a newer one, then whether to look by itself.
fn updates(s: &State, k: &rune_kit::Kit) -> Vec<El> {
    use crate::update::Ui;
    let lang = s.lang;
    let check = || kit::btn_small("update-check", S::CheckNow.get(lang), None).on_press(|s: &mut State| s.check_updates(true));
    let page = |url: String| {
        kit::btn_small("update-page", S::UpdateReleasePage.get(lang), Some(&icons::EXTERNAL)).on_press_tx(move |_s: &mut State, tx: &mut Tx| {
            let _ = tx.open_url(&url);
        })
    };
    let installable = s.update_method.can_install();
    let (status, note, control): (String, Option<&str>, El) = match &s.update {
        Ui::Idle => (S::UpdateIdleNote.get(lang).to_owned(), None, check()),
        Ui::Checking => (S::UpdateChecking.get(lang).to_owned(), None, check().opacity(0.5)),
        Ui::Latest => (S::UpToDate.get(lang).to_owned(), None, check()),
        Ui::Failed(why) => (why.get(lang).to_owned(), None, check()),
        Ui::Available(r) => {
            let mut buttons = row().gap(8.0).items_center().child(page(r.page_url.clone()));
            if installable && r.asset.is_some() {
                buttons = buttons.child(kit::btn_small("update-install", S::UpdateInstall.get(lang), Some(&icons::DOWNLOAD)).on_press(|s: &mut State| s.install_update()));
            }
            let note = (!installable).then(|| S::UpdateManual.get(lang));
            (format!("{} {}", S::UpdateAvailable.get(lang), r.version), note, buttons)
        }
        Ui::Downloading { release, done, total } => {
            let pct = if *total > 0 { (done * 100 / total).min(100) } else { 0 };
            (format!("{} {} · {pct}%", S::UpdateDownloading.get(lang), release.version), None, kit::btn_small("update-wait", S::UpdateInstall.get(lang), None).opacity(0.5))
        }
        Ui::Installing => (S::UpdateInstalling.get(lang).to_owned(), None, el()),
    };
    let mut says = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(3.0)
        .child(label(format!("Glint {}", env!("CARGO_PKG_VERSION")), th::H2, th::text()))
        .child(label(status, th::SMALL, th::text2()).wrap().align(Align::Start));
    if let Some(n) = note {
        says = says.child(label(n.to_owned(), th::SMALL, th::text3()).wrap().align(Align::Start));
    }
    let now = row().key("set-version").w_full().px(ROW_PAD).py(16.0).gap(16.0).items_center().child(app_mark(44.0)).child(says).child(control);
    let auto = toggle(k, s.settings.auto_update, S::AutoUpdate.get(lang), |s| {
        s.settings.auto_update = !s.settings.auto_update;
        s.mark_settings_dirty();
    });
    vec![group("set-update", None, vec![now], None), group("set-update-auto", None, vec![item(S::AutoUpdate.get(lang), Some(S::AutoUpdateNote.get(lang)), auto)], None)]
}

// ---- about -------------------------------------------------------------------------------------------

const REPO: &str = "https://github.com/orphytype/glint";

/// A row that opens a page in the browser.
fn link_row(key: &str, ic: &'static Icon, name: &str, url: &'static str) -> El {
    row()
        .key(key.to_owned())
        .role("link")
        .label(name)
        .w_full()
        .h(48.0)
        .px(ROW_PAD)
        .gap(12.0)
        .items_center()
        .cursor(Cursor::Pointer)
        .hov(veil(0.04))
        .press(veil(0.07))
        .hov_group()
        .on_press_tx(move |_s: &mut State, tx: &mut Tx| {
            let _ = tx.open_url(url);
        })
        .child(rune::icon(ic, 16.0).color(th::text2()))
        .child(label(name.to_owned(), th::BODY_M, th::text()).grow(1.0).min_w(0.0))
        .child(rune::icon(&icons::EXTERNAL, 14.0).color(th::text3()).gcol(th::text2()))
}

/// What Glint is and where it lives.
fn about(s: &State) -> Vec<El> {
    let lang = s.lang;
    let version = row().h(22.0).px(9.0).radius(11.0).center().fill(veil(0.07)).child(label(format!("v{}", env!("CARGO_PKG_VERSION")), th::MONO, th::text2()));
    let hero = col()
        .key("set-about")
        .w_full()
        .pt(12.0)
        .pb(4.0)
        .gap(10.0)
        .items_center()
        .child(app_mark(72.0))
        .child(row().gap(10.0).items_center().mt(6.0).child(label("Glint".to_owned(), th::H0, th::text())).child(version))
        .child(label(S::AboutText.get(lang).to_owned(), th::SMALL, th::text2()).wrap().align(Align::Center).max_w(420.0));
    let links = group(
        "set-links",
        Some(S::Links.get(lang)),
        vec![
            link_row("set-source", &icons::CODE, S::SourceCode.get(lang), REPO),
            link_row("set-issues", &icons::ALERT, S::ReportIssue.get(lang), "https://github.com/orphytype/glint/issues"),
            link_row("set-license", &icons::INFO, S::License.get(lang), "https://github.com/orphytype/glint/blob/main/LICENSE"),
        ],
        Some(S::Credits.get(lang)),
    );
    vec![hero, links]
}
