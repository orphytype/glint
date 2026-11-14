//! The Settings page. One narrow column in the middle of the page: a head with a butterfly of its
//! own, then sections, each a name with a small dithered shape (as the shelves on Home have) over
//! a soft rounded card of a few rows: a name (and, where it is needed, a short note) on the left
//! and the control on the right. The controls are RUNE's own kit (`ds.rs`): switch, segmented
//! control, select and button, small and flat.

use rune::{Align, Color, Cursor, Drag, El, Transition, Tx, col, el, row};
use rune_kit::{ControlSize, SegmentedControl, Select, SelectAppearance, Switch};
use ym_audio::{BAND_HZ, MAX_GAIN_DB, PRESETS};

use super::blobs::{self, Family, Fill, Look, Rng};
use super::ds;
use super::icons;
use super::kit::{self, label};
use crate::app::{Session, State};
use crate::i18n::{Lang, S};
use crate::store::{Effects, PlayerLayout, QualityPref, Repeat, ThemeMode};
use crate::theme::{self as th, ACCENTS, veil};

pub fn settings(s: &State, _w: f32) -> El {
    let k = ds::kit(s);
    let mut layer: Vec<El> = Vec::new();
    let lang = s.lang;
    let column = col()
        .key("settings-col")
        .gap(18.0)
        .w_full()
        .max_w(680.0)
        .child(head(s))
        .child(account(s))
        .child(section(s, "set-look", &icons::SPARKLE, S::SetLook.get(lang), appearance(s, &k)))
        .child(section(s, "set-play", &icons::NOTE, S::SetPlayback.get(lang), playback(s, &k)))
        .child(section(s, "set-lyrics", &icons::LYRICS, S::Lyrics.get(lang), lyrics(s, &k)))
        .child(section(s, "set-desk", &icons::WINDOW, S::SetDesktop.get(lang), desktop(s, &k)))
        .child(section(s, "set-eq", &icons::EQ, S::Equalizer.get(lang), equalizer(s, &k, &mut layer)))
        .child(section(s, "set-cache", &icons::DRIVE, S::Cache.get(lang), cache(s)))
        .child(section(s, "set-update", &icons::DOWNLOAD, S::Updates.get(lang), updates(s, &k)))
        .child(about(s));
    // The column sits in the middle of the page, whatever the window's width.
    let body = row().w_full().justify_center().child(column);
    // The page fades like the others; a list that is open hangs beside the page, not inside its scroll.
    el().key("settings")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .transition(Transition::opacity().springs(th::LEAVE))
        .child(kit::scroll_page("settings-page", body))
        .children(layer)
}

// ---- building blocks ---------------------------------------------------------------------------------

/// The head: the page's name with a butterfly resting beside it.
fn head(s: &State) -> El {
    let mut rng = Rng::new(s.home_seed ^ 0xb077_e4f1);
    let fly = blobs::shape(format!("set-fly-{}", s.home_seed), Family::Butterfly, rng.seed(), Fill::Hue(rng.range(0.0, 360.0)), Look::default(), 58.0, 58.0);
    row()
        .key("set-head")
        .w_full()
        .items_center()
        .gap(12.0)
        .pt(4.0)
        .pb(6.0)
        .child(label(S::Settings.get(s.lang).to_owned(), th::TITLE, th::text()))
        .child(el().grow(1.0))
        .child(fly)
}

/// A section: one plate with its name at the top (a small tile with its icon in the accent beside
/// it) and its rows under it, a hairline between each two.
fn section(s: &State, key: &str, ic: &'static rune::Icon, name: &str, rows: Vec<El>) -> El {
    let p = s.pal();
    let tile = row().w(30.0).h(30.0).radius(9.0).center().fill(p.accent.alpha(0.16)).child(rune::icon(ic, 17.0).color(p.accent));
    let top = row().w_full().h(54.0).gap(12.0).items_center().child(tile).child(label(name.to_owned(), th::BODY_M, th::text()));
    let mut body = col().w_full();
    for (i, r) in rows.into_iter().enumerate() {
        if i > 0 {
            body = body.child(el().w_full().h(1.0).fill(veil(0.06)));
        }
        body = body.child(r);
    }
    col()
        .key(key.to_owned())
        .w_full()
        .px(18.0)
        .pt(6.0)
        .pb(8.0)
        .radius(18.0)
        .fill(veil(0.035))
        .border(1.0, veil(0.07))
        .child(top)
        .child(el().w_full().h(1.0).fill(veil(0.06)))
        .child(body)
}

/// A name (and an optional short note) on the left, the control on the right.
fn setting(name: &str, note: Option<&str>, control: El) -> El {
    let mut text = col().grow(1.0).min_w(0.0).gap(2.0).child(label(name.to_owned(), th::BODY, th::text()));
    if let Some(n) = note {
        text = text.child(label(n.to_owned(), th::SMALL, th::text3()).wrap());
    }
    row().w_full().min_h(56.0).py(8.0).gap(20.0).items_center().child(text).child(control)
}

/// A segmented control over `names`, `current` lit; `pick` gets the index.
fn segmented(k: &rune_kit::Kit, names: &[&str], current: usize, pick: impl Fn(&mut State, usize) + Clone + 'static) -> El {
    SegmentedControl::new(names, Some(current)).fit().on_pick(move |s: &mut State, i: usize, _tx: &mut Tx| pick(s, i)).build(k)
}

fn toggle(k: &rune_kit::Kit, on: bool, name: &str, f: impl Fn(&mut State) + 'static) -> El {
    Switch::new(on).label(name).hide_label().control_size(ControlSize::Small).on_toggle(f).build(k)
}

// ---- account -----------------------------------------------------------------------------------------

/// Who is signed in, on a plate of its own at the top: the picture large, the name, the Plus badge,
/// and the way out.
fn account(s: &State) -> El {
    let lang = s.lang;
    let (name, note) = match (&s.session, &s.account) {
        (Session::Demo, _) => (S::DemoMode.get(lang).to_owned(), S::DemoNote.get(lang).to_owned()),
        (_, Some(a)) => {
            let shown = if a.display_name.is_empty() { a.login.clone() } else { a.display_name.clone() };
            (shown, a.login.clone())
        }
        _ => (String::new(), String::new()),
    };
    let first = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    let avatar = super::dock::account_picture(s, 60.0, &first);
    let mut title = row().gap(10.0).items_center().child(label(name, th::H3, th::text()).truncate().min_w(0.0));
    if s.account.as_ref().is_some_and(|a| a.has_plus) && s.session == Session::Live {
        title = title.child(kit::plus_badge(lang));
    }
    row()
        .key("set-account")
        .w_full()
        .min_h(96.0)
        .px(20.0)
        .py(18.0)
        .gap(16.0)
        .items_center()
        .radius(20.0)
        .fill(veil(0.045))
        .border(1.0, veil(0.08))
        .child(avatar)
        .child(col().grow(1.0).min_w(0.0).gap(3.0).child(title).child(label(note, th::SMALL, th::text3()).wrap().max_lines(2).min_w(0.0)))
        .child(kit::btn_small("set-signout", S::SignOut.get(lang), Some(&icons::SIGN_OUT)).on_press(|s: &mut State| s.sign_out()))
}

// ---- appearance --------------------------------------------------------------------------------------

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
        .w(28.0)
        .h(28.0)
        .radius(14.0)
        .center()
        .cursor(Cursor::Pointer)
        .press_scale(th::PRESS)
        .border(1.5, if on { th::text() } else { Color::TRANSPARENT })
        .on_press(move |s: &mut State| {
            s.settings.accent = i;
            s.mark_settings_dirty();
        })
        .child(dot)
}

fn appearance(s: &State, k: &rune_kit::Kit) -> Vec<El> {
    let lang = s.lang;
    let modes = [ThemeMode::Dark, ThemeMode::Light, ThemeMode::System];
    let theme = segmented(k, &[S::ThemeDark.get(lang), S::ThemeLight.get(lang), S::ThemeSystem.get(lang)], modes.iter().position(|m| *m == s.settings.theme).unwrap_or(0), move |s, i| {
        s.settings.theme = modes[i];
        s.mark_settings_dirty();
    });
    let swatches = row().gap(2.0).items_center().children((0..ACCENTS.len()).map(|i| swatch(s, i)));
    let levels = [Effects::Full, Effects::Light, Effects::Off];
    let fx = segmented(k, &[S::EffectsFull.get(lang), S::EffectsLight.get(lang), S::EffectsOff.get(lang)], levels.iter().position(|e| *e == s.settings.effects).unwrap_or(0), move |s, i| {
        s.settings.effects = levels[i];
        s.mark_settings_dirty();
    });
    let langs = [Lang::En, Lang::Ru];
    let language = segmented(k, &["English", "Русский"], langs.iter().position(|l| *l == s.lang).unwrap_or(0), move |s, i| s.set_lang(langs[i]));
    vec![
        setting(S::Theme.get(lang), None, theme),
        setting(S::Accent.get(lang), None, swatches),
        setting(S::Effects.get(lang), Some(S::EffectsNote.get(lang)), fx),
        setting(
            S::ReduceMotion.get(lang),
            None,
            toggle(k, s.settings.reduce_motion, S::ReduceMotion.get(lang), |s| {
                s.settings.reduce_motion = !s.settings.reduce_motion;
                s.mark_settings_dirty();
            }),
        ),
        setting(S::Language.get(lang), None, language),
    ]
}

// ---- playback ----------------------------------------------------------------------------------------

fn playback(s: &State, k: &rune_kit::Kit) -> Vec<El> {
    let lang = s.lang;
    let layouts = [PlayerLayout::Auto, PlayerLayout::Compact, PlayerLayout::Comfortable];
    let layout = segmented(
        k,
        &[S::PlayerLayoutAuto.get(lang), S::PlayerLayoutCompact.get(lang), S::PlayerLayoutComfortable.get(lang)],
        layouts.iter().position(|layout| *layout == s.settings.player_layout).unwrap_or(0),
        move |s, i| {
            s.settings.player_layout = layouts[i];
            s.mark_settings_dirty();
        },
    );
    let qs = [QualityPref::Saver, QualityPref::Balanced, QualityPref::Best];
    let q = segmented(k, &[S::QualitySaver.get(lang), S::QualityBalanced.get(lang), S::QualityBest.get(lang)], qs.iter().position(|q| *q == s.settings.quality).unwrap_or(0), move |s, i| {
        s.settings.quality = qs[i];
        s.mark_settings_dirty();
    });
    let repeat = [Repeat::Off, Repeat::All, Repeat::One];
    let repeat_names = [S::RepeatOff.get(lang), S::RepeatAll.get(lang), S::RepeatOne.get(lang)];
    let repeat_control = segmented(k, &repeat_names, repeat.iter().position(|r| *r == s.player.repeat).unwrap_or(0), move |s, i| {
        s.player.repeat = repeat[i];
        s.mark_settings_dirty();
    });
    vec![
        setting(S::PlayerLayout.get(lang), None, layout),
        setting(S::Quality.get(lang), None, q),
        setting(S::Shuffle.get(lang), None, toggle(k, s.player.shuffle, S::Shuffle.get(lang), |s| s.toggle_shuffle())),
        setting(S::Repeat.get(lang), None, repeat_control),
    ]
}

// ---- lyrics ------------------------------------------------------------------------------------------

/// Word-by-word lyrics from lrc.red.
fn lyrics(s: &State, k: &rune_kit::Kit) -> Vec<El> {
    let lang = s.lang;
    let on = s.settings.word_lyrics;
    let mut rows: Vec<El> = super::menu::lyric_rows()
        .into_iter()
        .enumerate()
        .map(|(i, (name, opts))| {
            let names = opts.map(|o| o.get(lang));
            setting(name.get(lang), None, segmented(k, &names, super::menu::stage_value(s, 2 + i) as usize, move |s, j| super::menu::set_stage_value(s, 2 + i, j as u8)))
        })
        .collect();
    rows.push(setting(
        S::WordLyrics.get(lang),
        Some(S::WordLyricsNote.get(lang)),
        toggle(k, on, S::WordLyrics.get(lang), |s| s.set_word_lyrics(!s.settings.word_lyrics)),
    ));
    rows.push(setting(
        S::FckCensor.get(lang),
        Some(S::FckCensorNote.get(lang)),
        toggle(k, s.settings.fkcensor, S::FckCensor.get(lang), |s| s.set_fkcensor(!s.settings.fkcensor)),
    ));
    rows
}

// ---- windows -----------------------------------------------------------------------------------------

/// The tray icon. (The media keys need no setting: they simply work.)
fn desktop(s: &State, k: &rune_kit::Kit) -> Vec<El> {
    let lang = s.lang;
    let tray = s.settings.show_tray;
    let close = s.settings.close_to_tray && tray;
    vec![
        setting(S::TrayIcon.get(lang), Some(S::TrayIconNote.get(lang)), toggle(k, tray, S::TrayIcon.get(lang), |s| s.set_show_tray(!s.settings.show_tray))),
        // Closing to a tray that isn't there would lose the window: dimmed until the icon is on.
        setting(
            S::CloseToTray.get(lang),
            Some(S::CloseToTrayNote.get(lang)),
            toggle(k, close, S::CloseToTray.get(lang), |s| {
                if s.settings.show_tray {
                    s.set_close_to_tray(!s.settings.close_to_tray);
                }
            }),
        )
        .opacity(if tray { 1.0 } else { 0.4 }),
    ]
}

// ---- equaliser ---------------------------------------------------------------------------------------

const EQ_H: f32 = 132.0;
const EQ_PAD: f32 = 10.0;
const EQ_W: f32 = 36.0;

fn preset_label(name: &str, lang: Lang) -> String {
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

fn equalizer(s: &State, k: &rune_kit::Kit, layer: &mut Vec<El>) -> Vec<El> {
    let lang = s.lang;
    let enabled = s.player.eq.enabled;
    let names: Vec<String> = PRESETS.iter().map(|(name, _)| preset_label(name, lang)).collect();
    let current = s.player.eq.preset_name().map(|n| preset_label(n, lang));
    let presets = Select::new("eq-preset", &names)
        .width(150.0)
        .appearance(SelectAppearance::Field)
        .current(current.as_deref())
        .placeholder("—")
        .label(S::Equalizer.get(lang))
        .open(s.eq_open)
        .on_toggle(|s: &mut State| s.eq_open = !s.eq_open)
        .on_close(|s: &mut State| s.eq_open = false)
        .on_choose(|s: &mut State, v: &str, _tx: &mut Tx| {
            let lang = s.lang;
            s.eq_open = false;
            if let Some((n, (_, gains))) = PRESETS.iter().enumerate().find(|(_, (name, _))| preset_label(name, lang) == v) {
                s.player.eq.gains_db = *gains;
                s.eq_preset = n;
                s.apply_eq();
            }
        })
        .build(k, layer);
    let reset = kit::btn_small("eq-reset", S::EqReset.get(lang), None).on_press(|s: &mut State| {
        s.player.eq.gains_db = [0.0; 10];
        s.player.eq.preamp_db = 0.0;
        s.apply_eq();
    });
    let bands = row().w_full().justify_between().opacity(if enabled { 1.0 } else { 0.5 }).children((0..BAND_HZ.len()).map(|i| eq_band(s, i)));
    vec![
        setting(
            S::EqPreset.get(lang),
            None,
            row().gap(8.0).items_center().child(presets).child(reset).child(toggle(k, enabled, S::EqEnabled.get(lang), |s| {
                s.player.eq.enabled = !s.player.eq.enabled;
                s.apply_eq();
            })),
        ),
        bands.pt(4.0).pb(10.0),
    ]
}

// ---- cache -------------------------------------------------------------------------------------------

fn size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

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
    let stat = |text: String| label(text, th::MONO, th::text2());
    let clear_images = kit::btn_small("cache-clear-images", S::CacheClear.get(lang), Some(&icons::TRASH)).on_press(|s: &mut State| {
        if let Some(d) = &s.cache {
            let _ = d.clear();
        }
        s.disk_usage.set((0, 0, 0, f64::NEG_INFINITY));
        let text = s.tr(S::CacheCleared);
        s.toast(text);
    });
    let clear_downloads = kit::btn_small("downloads-clear", S::DownloadsClear.get(lang), Some(&icons::TRASH)).on_press(|s: &mut State| {
        if let Some(d) = &s.download_store {
            let _ = d.clear();
        }
        s.disk_usage.set((0, 0, 0, f64::NEG_INFINITY));
        s.downloads.clear();
        s.download_titles.clear();
        s.download_activity = None;
        let text = s.tr(S::DownloadsCleared);
        s.toast(text);
    });
    let clear_all = kit::btn_small("cache-clear-all", S::CacheClearAll.get(lang), Some(&icons::TRASH)).on_press(|s: &mut State| {
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
        let text = s.tr(S::CacheAllCleared);
        s.toast(text);
    });
    vec![
        setting(S::CacheImages.get(lang), None, row().gap(12.0).items_center().child(stat(size(images))).child(clear_images)),
        setting(S::CacheAudio.get(lang), None, stat(size(audio))),
        setting(S::CacheMetadata.get(lang), None, stat(size(metadata))),
        setting(S::CacheTotal.get(lang), None, row().gap(8.0).items_center().child(stat(size(total))).child(clear_downloads).child(clear_all)),
    ]
}

// ---- updates -----------------------------------------------------------------------------------------

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
    let (name, note, control) = match &s.update {
        Ui::Idle => (format!("{} · v{}", S::Updates.get(lang), env!("CARGO_PKG_VERSION")), None, check()),
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
    vec![
        setting(S::AutoUpdate.get(lang), Some(S::AutoUpdateNote.get(lang)), toggle(k, s.settings.auto_update, S::AutoUpdate.get(lang), |s| {
            s.settings.auto_update = !s.settings.auto_update;
            s.mark_settings_dirty();
        })),
        setting(&name, note, control),
    ]
}

// ---- about -------------------------------------------------------------------------------------------

fn about(s: &State) -> El {
    let lang = s.lang;
    let p = s.pal();
    let version = row().h(20.0).px(8.0).radius(10.0).center().fill(veil(0.07)).child(label(format!("v{}", env!("CARGO_PKG_VERSION")), th::MONO, th::text2()));
    let name = row().gap(10.0).items_center().child(label("Glint".to_owned(), th::H3, th::text())).child(version);
    let tile = row().w(30.0).h(30.0).radius(9.0).center().fill(p.accent.alpha(0.16)).child(rune::icon(&icons::INFO, 17.0).color(p.accent));
    let source = kit::btn_small("set-source", S::SourceCode.get(lang), Some(&icons::CODE)).on_press_tx(|_s: &mut State, tx: &mut Tx| {
        let _ = tx.open_url("https://github.com/orphytype/glint");
    });
    let top = row().w_full().gap(12.0).items_center().child(tile).child(name).child(el().grow(1.0)).child(source);
    let keys = col()
        .w_full()
        .gap(6.0)
        .child(kit::caps(S::Shortcuts.get(lang), th::text3()))
        .child(label(S::ShortcutsText.get(lang), th::SMALL, th::text2()).wrap().align(Align::Start));
    col()
        .key("set-about")
        .w_full()
        .px(18.0)
        .py(16.0)
        .gap(14.0)
        .radius(18.0)
        .fill(veil(0.035))
        .border(1.0, veil(0.07))
        .child(top)
        .child(col().w_full().gap(4.0).child(label(S::AboutText.get(lang), th::SMALL, th::text2()).wrap()).child(label(format!("{} · {}", S::Credits.get(lang), S::License.get(lang)), th::SMALL, th::text3()).wrap()))
        .child(el().w_full().h(1.0).fill(veil(0.06)))
        .child(keys)
}
