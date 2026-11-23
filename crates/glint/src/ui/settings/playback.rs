//! Playback quality, Now Playing backdrop, lyrics appearance, and 10-band equalizer tabs.

use rune::{Cursor, Drag, El, Tx, col, el, icon, row};
use ym_audio::{BANDS, MAX_GAIN_DB};

use super::{ROW_PAD, group, item, item_below, segmented, toggle, words};
use crate::app::{Popover, State};
use crate::i18n::{Lang, S};
use crate::store::{PlayerLayout, QualityPref};
use crate::theme::{self as th, veil};
use crate::ui::{
    kit::{self, Hover as _, label},
    menu, np_icons,
};

const LEVEL_W: f32 = 200.0;
const EQ_H: f32 = 132.0;
const EQ_PAD: f32 = 10.0;
const EQ_W: f32 = 36.0;
const HZ_LABELS: [&str; BANDS] = ["31.5", "63", "125", "250", "500", "1k", "2k", "4k", "8k", "16k"];

pub(super) fn playback(s: &State, k: &rune_kit::Kit) -> Vec<El> {
    let lang = s.lang;
    let qs = [QualityPref::Saver, QualityPref::Balanced, QualityPref::Best];
    let mut q = row().key("set-quality").padding(3.0).gap(2.0).radius(9.0).fill(veil(0.05)).border(1.0, veil(0.08)).items_center();
    for quality in qs {
        let on = s.settings.quality == quality;
        let text = np_icons::quality_label(quality).get(lang);
        let ink = if on { th::text() } else { th::text2() };
        let mut cell = row()
            .key(format!("set-q-{quality:?}"))
            .role("radio")
            .label(text)
            .checked(on)
            .h(28.0)
            .px(10.0)
            .gap(6.0)
            .radius(6.5)
            .items_center()
            .cursor(Cursor::Pointer)
            .hov(veil(if on { 0.0 } else { 0.05 }))
            .press(veil(0.10))
            .on_press(move |s: &mut State| s.set_quality(quality))
            .child(icon(np_icons::quality_icon(quality), 16.0).color(ink))
            .child(label(text, th::BODY_M, ink));
        if on {
            cell = cell.fill(if th::light() { th::pop() } else { veil(0.14) });
            if th::light() {
                cell = cell.border(1.0, veil(0.10));
            }
        }
        q = q.child(cell);
    }
    let layouts = [PlayerLayout::Auto, PlayerLayout::Comfortable, PlayerLayout::Compact, PlayerLayout::Cover];
    let layout = segmented(
        k,
        &[S::PlayerLayoutAuto.get(lang), S::PlayerLayoutComfortable.get(lang), S::PlayerLayoutCompact.get(lang), S::PlayerLayoutCover.get(lang)],
        layouts.iter().position(|l| *l == s.settings.player_layout).unwrap_or(0),
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
    let mut screen = vec![item_below(S::PlayerLayout.get(lang), None, layout)];
    for (i, (name, opts)) in menu::stage_rows().into_iter().enumerate() {
        screen.push(item(name.get(lang), None, look_choice(s, k, i, opts)));
    }
    screen.extend(backdrop_rows(s, k));
    let reset = if menu::look_changed(s) {
        kit::btn_small("set-look-reset", S::StageReset.get(lang), None).on_press(menu::reset_look)
    } else {
        kit::btn_small_disabled("set-look-reset", S::StageReset.get(lang), None)
    };
    screen.push(item(S::StageLook.get(lang), Some(S::StageResetNote.get(lang)), reset));
    vec![
        group(
            "set-sound",
            Some(S::SetSound.get(lang)),
            vec![
                item_below(S::Quality.get(lang), Some(S::QualityNote.get(lang)), q),
                item(S::FckCensor.get(lang), Some(S::FckCensorNote.get(lang)), censor),
                item(S::PlayOnLaunch.get(lang), None, launch),
            ],
            None,
        ),
        group("set-screen", Some(S::SetPlayerScreen.get(lang)), screen, None),
    ]
}

fn look_choice(s: &State, k: &rune_kit::Kit, i: usize, opts: [S; 3]) -> El {
    let names = opts.map(|o| o.get(s.lang));
    segmented(k, &names, usize::from(menu::stage_value(s, i)), move |s, v| menu::set_stage_value(s, i, v as u8))
}

fn backdrop_rows(s: &State, k: &rune_kit::Kit) -> Vec<El> {
    let lang = s.lang;
    let look = s.settings.backdrop;
    let pick = |key: &str, name: &str| kit::btn_small(key.to_owned(), name, None).on_press_tx(|s: &mut State, tx: &mut Tx| s.pick_backdrop(tx));
    if !look.on {
        return vec![item(S::Backdrop.get(lang), None, pick("bd-pick", S::BdPick.get(lang)))];
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
        item(S::Backdrop.get(lang), None, buttons),
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

fn level(key: &str, value: f32, set: impl Fn(&mut State, f32) + 'static) -> El {
    level_w(key, value, LEVEL_W, set)
}

/// Horizontal 0..1 level slider at a custom width, shared with the player's look panel.
pub(in crate::ui) fn level_w(key: &str, value: f32, lw: f32, set: impl Fn(&mut State, f32) + 'static) -> El {
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
        .child(el().absolute((lw * v - 8.0).clamp(0.0, lw - 16.0), 4.0).w(16.0).h(16.0).radius(8.0).fill(ink))
}

pub(super) fn lyrics(s: &State, k: &rune_kit::Kit) -> Vec<El> {
    let lang = s.lang;
    let word_lyrics = toggle(k, s.settings.word_lyrics, S::WordLyrics.get(lang), |s| s.set_word_lyrics(!s.settings.word_lyrics));
    let font = s.settings.lyric_look.font;
    let faces = segmented(k, &["Plex Mono", "Commissioner", "Cormorant"], usize::from(font), |s, i| {
        s.settings.lyric_look.font = i as u8;
        s.mark_settings_dirty();
    });
    let sample = label(S::LyricsFontSample.get(lang), th::lyric_face(th::LYRIC_CARD, font), th::text()).key("set-lyrics-sample").w_full().wrap();
    let face =
        col().w_full().px(ROW_PAD).py(14.0).gap(12.0).child(words(S::LyricsFont.get(lang), None)).child(row().w_full().child(faces)).child(sample);
    let mut rows = vec![face];
    for (j, (name, opts)) in menu::lyric_rows().into_iter().enumerate() {
        rows.push(item(name.get(lang), None, look_choice(s, k, 1 + j, opts)));
    }
    rows.push(item(S::WordLyrics.get(lang), None, word_lyrics));
    vec![group("set-lyrics", None, rows, None)]
}

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

fn eq_band(s: &State, i: usize) -> El {
    let g = s.player.eq.gains_db[i];
    let enabled = s.player.eq.enabled;
    let usable = EQ_H - 2.0 * EQ_PAD;
    let knob_y = EQ_PAD + (0.5 - g / (2.0 * MAX_GAIN_DB)) * usable;
    let mid_y = EQ_PAD + 0.5 * usable;
    let (top, height) = if knob_y < mid_y { (knob_y, mid_y - knob_y) } else { (mid_y, knob_y - mid_y) };
    let color = if enabled { th::text() } else { th::text3().alpha(0.55) };
    let rail = veil(if enabled { 0.14 } else { 0.07 });
    let tick = veil(if enabled { 0.26 } else { 0.13 });
    let value = if g.abs() < 0.05 { "0".to_owned() } else { format!("{g:+.1}") };
    let mx = EQ_W * 0.5;
    let hz = HZ_LABELS[i];

    let slider = el()
        .key(format!("eq-{i}"))
        .role("slider")
        .label(format!("{hz} Hz"))
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
        .child(el().absolute(mx - 1.0, EQ_PAD).w(2.0).h(usable).radius(1.0).fill(rail))
        .child(el().absolute(mx - 8.0, mid_y - 0.5).w(16.0).h(1.0).fill(tick))
        .child(el().absolute(mx - 1.5, top).w(3.0).h(height.max(0.5)).radius(1.5).fill(color))
        .child(el().absolute(mx - 6.0, knob_y - 6.0).w(12.0).h(12.0).radius(6.0).fill(color));

    col()
        .key(format!("eq-col-{i}"))
        .w(EQ_W)
        .gap(4.0)
        .items_center()
        .child(label(value, th::MONO, if enabled { th::text2() } else { th::text3().alpha(0.55) }))
        .child(slider)
        .child(label(hz, th::MONO, if enabled { th::text3() } else { th::text3().alpha(0.55) }))
}

pub(super) fn equalizer(s: &State, k: &rune_kit::Kit) -> Vec<El> {
    let lang = s.lang;
    let enabled = s.player.eq.enabled;
    let current = s.player.eq.preset_name().map_or_else(|| "—".to_owned(), |n| preset_label(n, lang));
    let open = s.popover_open && matches!(s.popover, Some(Popover::EqPreset { .. }));
    let presets = kit::btn_select("eq-preset", &current, 160.0, open).on_press(|s: &mut State| s.open_popover(Popover::EqPreset { anchor: "eq-preset".into() }));
    let reset = kit::btn_small("eq-reset", S::EqReset.get(lang), None).on_press(|s: &mut State| {
        s.player.eq.gains_db = [0.0; BANDS];
        s.player.eq.preamp_db = 0.0;
        s.apply_eq();
    });
    let bands = row().w_full().px(ROW_PAD - 4.0).pt(14.0).pb(12.0).justify_between().children((0..BANDS).map(|i| eq_band(s, i)));
    let on = toggle(k, enabled, S::EqEnabled.get(lang), |s| {
        s.player.eq.enabled = !s.player.eq.enabled;
        s.apply_eq();
    });
    vec![
        group("set-eq", None, vec![item(S::EqEnabled.get(lang), None, on), item(S::EqPreset.get(lang), None, row().gap(8.0).items_center().child(presets).child(reset))], None),
        group("set-eq-bands", Some(S::EqBands.get(lang)), vec![bands], None),
    ]
}
