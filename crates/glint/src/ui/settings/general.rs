//! General (account, appearance, graphics, system) and Shortcuts pages of Settings.

use rune::{Color, Cursor, El, Tx, col, el, row};

use super::{GROUP_R, ROW_PAD, figure, found_mark, group, item, item_enabled, item_key, segmented, toggle, toggle_enabled};
use crate::app::{Session, State};
use crate::i18n::{Lang, S};
use crate::keys::{self, Act};
use crate::store::ThemeMode;
use crate::theme::{self as th, ACCENTS, veil};
use crate::ui::{
    dock, icons,
    kit::{self, Hover as _, label},
};

fn account_card(s: &State) -> El {
    let lang = s.lang;
    let (name, login) = match &s.account {
        Some(a) => {
            let shown = if a.display_name.is_empty() { a.login.clone() } else { a.display_name.clone() };
            (shown, Some(a.login.clone()))
        }
        None => (String::new(), None),
    };
    let first = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    let mut title = row().gap(8.0).items_center().child(label(name, th::H3, th::text()).truncate().min_w(0.0));
    if s.account.as_ref().is_some_and(|a| a.has_plus) && s.session == Session::Live {
        title = title.child(kit::plus_badge(lang));
    }
    let mut info = col().grow(1.0).min_w(0.0).gap(2.0).child(title);
    if let Some(login) = login {
        let peek = s.id_peek;
        let shown = if peek { login } else { "•".repeat(login.chars().count().clamp(6, 10)) };
        info = info.child(
            label(shown, th::SMALL, if peek { th::text2() } else { th::text3() })
                .key("set-login")
                .truncate()
                .min_w(0.0)
                .cursor(Cursor::Default)
                .help(if peek { "" } else { S::IdHidden.get(lang) })
                .on_hover_change(|s: &mut State, on: bool, _tx: &mut Tx| s.id_peek = on),
        );
    }
    row()
        .key("set-account")
        .w_full()
        .px(ROW_PAD)
        .py(12.0)
        .gap(13.0)
        .items_center()
        .radius(GROUP_R)
        .fill(veil(0.028))
        .border(1.0, veil(0.055))
        .children(found_mark("set-account"))
        .child(dock::account_picture(s, 44.0, &first))
        .child(info)
        .child(kit::btn_small("set-signout", S::SignOut.get(lang), Some(&icons::SIGN_OUT)).on_press(|s: &mut State| s.sign_out()))
}

fn swatch(s: &State, i: usize) -> El {
    let on = s.settings.accent == i;
    let (name, hex) = ACCENTS[i];
    let dot = el().w(16.0).h(16.0).radius(8.0);
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
        .w(24.0)
        .h(24.0)
        .radius(12.0)
        .center()
        .cursor(Cursor::Pointer)
        .press_scale(th::PRESS)
        .border(1.5, if on { th::text().alpha(0.85) } else { Color::TRANSPARENT })
        .on_press(move |s: &mut State| {
            s.settings.accent = i;
            s.mark_settings_dirty();
        })
        .child(dot)
}

pub(super) fn general(s: &State, k: &rune_kit::Kit) -> Vec<El> {
    let lang = s.lang;
    let modes = [ThemeMode::Dark, ThemeMode::Light, ThemeMode::System];
    let theme =
        segmented(k, &[S::ThemeDark.get(lang), S::ThemeLight.get(lang), S::ThemeSystem.get(lang)], modes.iter().position(|m| *m == s.settings.theme).unwrap_or(0), move |s, i| {
            s.settings.theme = modes[i];
            s.mark_settings_dirty();
        });
    let swatches = row().gap(3.0).items_center().children((0..ACCENTS.len()).map(|i| swatch(s, i)));
    let langs = [Lang::En, Lang::Ru];
    let language = segmented(k, &["English", "Русский"], langs.iter().position(|l| *l == s.lang).unwrap_or(0), move |s, i| s.set_lang(langs[i]));

    let cpu_running = crate::fx::cpu_mode();
    let shaders_on = s.settings.shaders && !cpu_running;
    let shaders = toggle_enabled(k, shaders_on, !cpu_running, S::Shaders.get(lang), |s| {
        if !crate::fx::cpu_mode() {
            s.settings.shaders = !s.settings.shaders;
            if !s.settings.shaders {
                crate::fx::release_gpu();
            }
            s.mark_settings_dirty();
        }
    });
    let cpu = toggle(k, s.settings.cpu_mode, S::CpuMode.get(lang), |s| {
        s.settings.cpu_mode = !s.settings.cpu_mode;
        s.restart_now();
    });
    let motion = toggle(k, s.settings.reduce_motion, S::ReduceMotion.get(lang), |s| {
        s.settings.reduce_motion = !s.settings.reduce_motion;
        s.mark_settings_dirty();
    });

    let tray = s.settings.show_tray;
    let auto = s.settings.autostart;
    let mut sys_rows = vec![item(S::Autostart.get(lang), None, toggle(k, auto, S::Autostart.get(lang), |s| s.set_autostart(!s.settings.autostart)))];
    if auto && tray {
        sys_rows.push(item(
            S::AutostartHidden.get(lang),
            None,
            toggle(k, s.settings.autostart_hidden, S::AutostartHidden.get(lang), |s| {
                s.settings.autostart_hidden = !s.settings.autostart_hidden;
                s.mark_settings_dirty();
            }),
        ));
    }
    sys_rows.push(item(S::TrayIcon.get(lang), None, toggle(k, tray, S::TrayIcon.get(lang), |s| s.set_show_tray(!s.settings.show_tray))));
    if tray {
        sys_rows.push(item(
            S::CloseToTray.get(lang),
            None,
            toggle(k, s.settings.close_to_tray, S::CloseToTray.get(lang), |s| s.set_close_to_tray(!s.settings.close_to_tray)),
        ));
    }

    vec![
        account_card(s),
        group(
            "set-appearance",
            Some(S::SetAppearance.get(lang)),
            vec![
                item(S::Theme.get(lang), None, theme),
                item(S::Accent.get(lang), None, swatches),
                item(S::Language.get(lang), None, language),
            ],
            None,
        ),
        group(
            "set-graphics",
            Some(S::SetGraphics.get(lang)),
            vec![
                item_enabled(S::Shaders.get(lang), None, shaders, !cpu_running),
                item(S::CpuMode.get(lang), Some(S::CpuModeNote.get(lang)), cpu),
                item(S::ReduceMotion.get(lang), None, motion),
            ],
            None,
        ),
        group("set-system", Some(S::SetSystem.get(lang)), sys_rows, None),
    ]
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

fn key_cap(text: &str) -> El {
    row().h(22.0).min_w(22.0).px(6.5).radius(5.5).center().fill(veil(0.065)).border(1.0, veil(0.09)).child(label(text.to_owned(), th::LABEL, th::text2()))
}

fn shortcut_row(s: &State, act: Act) -> El {
    let lang = s.lang;
    let waiting = s.rebind == Some(act);
    let name = act.name().get(lang);
    let mut caps = row().gap(6.0).items_center();
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
            caps = caps.child(row().gap(3.5).items_center().children(chord.iter().map(|c| key_cap(c))));
        }
    }
    let button = row()
        .key(format!("key-{}", act.id()))
        .role("button")
        .label(name)
        .h(30.0)
        .px(7.0)
        .radius(7.5)
        .items_center()
        .cursor(Cursor::Pointer)
        .fill(if waiting { veil(0.10) } else { Color::TRANSPARENT })
        .border(1.0, if waiting { th::text().alpha(0.4) } else { Color::TRANSPARENT })
        .hov(veil(0.055))
        .press(veil(0.095))
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.blur();
            s.rebind = if s.rebind == Some(act) { None } else { Some(act) };
        })
        .child(caps);
    let key = item_key(name);
    row()
        .key(key.clone())
        .w_full()
        .h(40.0)
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
            .h(40.0)
            .px(ROW_PAD)
            .gap(12.0)
            .items_center()
            .children(found_mark(&back))
            .child(label(S::ScBack.get(lang).to_owned(), th::BODY, th::text()).grow(1.0).min_w(0.0))
            .child(key_cap("Esc")),
    );
    vec![group("set-keys", None, rows, Some(S::KeyHint.get(lang)))]
}
