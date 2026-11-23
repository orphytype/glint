//! Account, look, mini player, startup and tray, and shortcut pages of Settings.

use rune::{Color, Cursor, El, Tx, col, el, row};

use super::{GROUP_R, ROW_PAD, figure, found_mark, group, item, item_enabled, item_key, level_w, segmented, toggle, toggle_enabled};
use crate::app::{Session, State};
use crate::i18n::{Lang, S};
use crate::keys::{self, Act};
use crate::store::{Effects, MiniStyle, ThemeMode};
use crate::theme::{self as th, ACCENTS, veil};
use crate::ui::{
    dock, icons,
    kit::{self, Hover as _, label},
};

pub(super) fn account(s: &State) -> Vec<El> {
    let lang = s.lang;
    let (name, note, login) = match &s.account {
        Some(a) => {
            let shown = if a.display_name.is_empty() { a.login.clone() } else { a.display_name.clone() };
            (shown, S::SignedInAs.get(lang).to_owned(), Some(a.login.clone()))
        }
        None => (String::new(), String::new(), None),
    };
    let mut note = row().w_full().gap(5.0).items_center().min_w(0.0).child(label(note, th::SMALL, th::text2()).wrap().max_lines(2).shrink().min_w(0.0));
    if let Some(login) = login {
        let peek = s.id_peek;
        let shown = if peek { login } else { "•".repeat(login.chars().count().clamp(5, 10)) };
        note = note.child(label("·".to_owned(), th::SMALL, th::text3())).child(
            label(shown, th::SMALL, if peek { th::text2() } else { th::text3() })
                .key("set-login")
                .truncate()
                .min_w(0.0)
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
        .children(found_mark("set-account"))
        .child(dock::account_picture(s, 60.0, &first))
        .child(col().grow(1.0).min_w(0.0).gap(4.0).child(title).child(note))
        .child(kit::btn_small("set-signout", S::SignOut.get(lang), Some(&icons::SIGN_OUT)).on_press(|s: &mut State| s.sign_out()));

    vec![who]
}

/// Search and listening history, on the Storage page.
pub(super) fn history(s: &State) -> El {
    let lang = s.lang;
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
    group(
        "set-history",
        Some(S::SetHistory.get(lang)),
        vec![
            item(S::SearchHistory.get(lang), None, clear("set-clear-searches", s.settings.search_history.is_empty(), State::clear_search_history)),
            item(S::ListeningHistory.get(lang), None, clear("set-clear-played", s.listening_history.is_empty(), State::clear_listening_history)),
        ],
        None,
    )
}

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

pub(super) fn appearance(s: &State, k: &rune_kit::Kit) -> Vec<El> {
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
        group("set-colour", None, vec![item(S::Theme.get(lang), None, theme), item(S::Accent.get(lang), None, swatches)], None),
        group(
            "set-motion",
            None,
            vec![
                item(S::Effects.get(lang), Some(S::EffectsNote.get(lang)), fx),
                item(S::Butterflies.get(lang), None, bugs),
                item(S::ReduceMotion.get(lang), None, motion),
            ],
            None,
        ),
        group("set-language", None, vec![item(S::Language.get(lang), None, language)], None),
    ]
}

fn key_cap(text: &str) -> El {
    row().h(24.0).min_w(24.0).px(7.0).radius(6.0).center().fill(veil(0.07)).border(1.0, veil(0.10)).child(label(text.to_owned(), th::LABEL, th::text2()))
}

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
            tx.blur();
            s.rebind = if s.rebind == Some(act) { None } else { Some(act) };
        })
        .child(caps);
    let key = item_key(name);
    row()
        .key(key.clone())
        .w_full()
        .h(48.0)
        .px(ROW_PAD)
        .gap(12.0)
        .items_center()
        .children(found_mark(&key))
        .child(label(name.to_owned(), th::BODY, th::text()).grow(1.0).min_w(0.0))
        .child(button)
}

pub(super) fn shortcuts(s: &State) -> Vec<El> {
    let lang = s.lang;
    let mut rows: Vec<El> = Act::ALL.into_iter().map(|act| shortcut_row(s, act)).collect();
    let back = item_key(S::ScBack.get(lang));
    rows.push(
        row()
            .key(back.clone())
            .w_full()
            .h(48.0)
            .px(ROW_PAD)
            .gap(12.0)
            .items_center()
            .children(found_mark(&back))
            .child(label(S::ScBack.get(lang).to_owned(), th::BODY, th::text()).grow(1.0).min_w(0.0))
            .child(key_cap("Esc")),
    );
    let reset = if keys::changed(&s.settings) {
        kit::btn_small("keys-reset", S::StageReset.get(lang), None).on_press(|s: &mut State| s.reset_keys())
    } else {
        kit::btn_small_disabled("keys-reset", S::StageReset.get(lang), None)
    };
    vec![group("set-keys", None, rows, Some(S::KeyHint.get(lang))), group("set-keys-reset", None, vec![item(S::KeysDefault.get(lang), None, reset)], None)]
}

pub(super) fn mini(s: &State, k: &rune_kit::Kit) -> Vec<El> {
    let lang = s.lang;
    let close = s.settings.close_to_tray && s.settings.show_tray;
    let mini_styles = [MiniStyle::Compact, MiniStyle::Standard, MiniStyle::Cover];
    let mini_style_idx = mini_styles.iter().position(|st| *st == s.settings.mini_style).unwrap_or(1);
    let mini_style_ctl = segmented(
        k,
        &[S::MiniCompact.get(lang), S::MiniStandard.get(lang), S::MiniCover.get(lang)],
        mini_style_idx,
        move |s, i| s.set_mini_style(mini_styles[i]),
    );
    let op_frac = ((s.settings.mini_opacity.clamp(0.65, 1.0) - 0.65) / 0.35).clamp(0.0, 1.0);
    let op_pct = format!("{}%", (s.settings.mini_opacity.clamp(0.65, 1.0) * 100.0).round() as i32);
    let op_ctl = row()
        .gap(10.0)
        .items_center()
        .child(figure(op_pct))
        .child(level_w("set-mini-opacity", op_frac, 110.0, |s, f| {
            s.set_mini_opacity(0.65 + f * 0.35);
        }));
    let on_close = toggle_enabled(k, s.settings.mini_on_close && close, close, S::MiniOnClose.get(lang), |s| {
        if s.settings.show_tray && s.settings.close_to_tray {
            s.settings.mini_on_close = !s.settings.mini_on_close;
            s.mark_settings_dirty();
        }
    });
    vec![
        group(
            "set-mini",
            None,
            vec![
                item(S::MiniShow.get(lang), None, toggle(k, s.mini_open, S::MiniShow.get(lang), State::toggle_mini_player)),
                item(S::MiniStyle.get(lang), None, mini_style_ctl),
                item(
                    S::MiniAlwaysOnTop.get(lang),
                    None,
                    toggle(k, s.settings.mini_always_on_top, S::MiniAlwaysOnTop.get(lang), |s| s.set_mini_always_on_top(!s.settings.mini_always_on_top)),
                ),
                item(S::MiniOpacity.get(lang), None, op_ctl),
            ],
            None,
        ),
        group(
            "set-mini-close",
            None,
            vec![item_enabled(S::MiniOnClose.get(lang), (!close).then(|| S::MiniOnCloseNeedsTray.get(lang)), on_close, close)],
            None,
        ),
    ]
}

pub(super) fn startup(s: &State, k: &rune_kit::Kit) -> Vec<El> {
    let lang = s.lang;
    let tray = s.settings.show_tray;
    let close = s.settings.close_to_tray && tray;
    let auto = s.settings.autostart;
    let hidden = s.settings.autostart_hidden && tray && auto;
    vec![
        group(
            "set-autostart",
            Some(S::SetLaunch.get(lang)),
            vec![
                item(S::Autostart.get(lang), None, toggle(k, auto, S::Autostart.get(lang), |s| s.set_autostart(!s.settings.autostart))),
                item_enabled(
                    S::AutostartHidden.get(lang),
                    None,
                    toggle_enabled(k, hidden, tray && auto, S::AutostartHidden.get(lang), |s| {
                        if s.settings.show_tray && s.settings.autostart {
                            s.settings.autostart_hidden = !s.settings.autostart_hidden;
                            s.mark_settings_dirty();
                        }
                    }),
                    tray && auto,
                ),
            ],
            None,
        ),
        group(
            "set-tray",
            Some(S::SetTray.get(lang)),
            vec![
                item(S::TrayIcon.get(lang), None, toggle(k, tray, S::TrayIcon.get(lang), |s| s.set_show_tray(!s.settings.show_tray))),
                item_enabled(
                    S::CloseToTray.get(lang),
                    Some(S::CloseToTrayNote.get(lang)),
                    toggle_enabled(k, close, tray, S::CloseToTray.get(lang), |s| {
                        if s.settings.show_tray {
                            s.set_close_to_tray(!s.settings.close_to_tray);
                        }
                    }),
                    tray,
                ),
            ],
            None,
        ),
    ]
}
