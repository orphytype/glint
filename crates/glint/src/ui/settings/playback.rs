//! Sound (quality, FckCensor, autoplay, collapsible 10-band equalizer) and Player (Now Playing
//! layout, backdrop, lyrics, and Mini Player) pages of Settings.

use rune::{Cursor, Drag, El, Tx, col, el, icon, row};
use ym_audio::{BANDS, MAX_GAIN_DB};

use super::{ROW_PAD, figure, group, item, item_below, segmented, toggle};
use crate::app::{Popover, State};
use crate::i18n::{Lang, S};
use crate::store::{MiniStyle, PlayerLayout, QualityPref};
use crate::theme::{self as th, veil};
use crate::ui::{
    icons,
    kit::{self, Hover as _, label},
    menu,
};

const LEVEL_W: f32 = 160.0;
const EQ_H: f32 = 122.0;
const EQ_PAD: f32 = 10.0;
const EQ_W: f32 = 34.0;
const HZ_LABELS: [&str; BANDS] = ["31.5", "63", "125", "250", "500", "1k", "2k", "4k", "8k", "16k"];

pub(super) fn sound(s: &State, k: &rune_kit::Kit) -> Vec<El> {
    let lang = s.lang;
    let qs = [QualityPref::Saver, QualityPref::Balanced, QualityPref::Best];
    let q = segmented(
        k,
        &[S::QualitySaver.get(lang), S::QualityBalanced.get(lang), S::QualityBest.get(lang)],
        qs.iter().position(|q| *q == s.settings.quality).unwrap_or(2),
        move |s, i| s.set_quality(qs[i]),
    );
    let censor = toggle(k, s.settings.fkcensor, S::FckCensor.get(lang), |s| s.set_fkcensor(!s.settings.fkcensor));
    let launch = toggle(k, s.settings.play_on_launch, S::PlayOnLaunch.get(lang), |s| {
        s.settings.play_on_launch = !s.settings.play_on_launch;
        s.mark_settings_dirty();
    });
    vec![
        group(
            "set-sound",
            None,
            vec![
                item(S::Quality.get(lang), None, q),
                item(S::FckCensor.get(lang), Some(S::FckCensorNote.get(lang)), censor),
                item(S::PlayOnLaunch.get(lang), None, launch),
            ],
            None,
        ),
        equalizer(s, k),
    ]
}

pub(super) fn player(s: &State, k: &rune_kit::Kit) -> Vec<El> {
    let lang = s.lang;
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
    let mut screen = vec![item(S::PlayerLayout.get(lang), None, layout)];
    if s.shaders_active() {
        let stage_fx = toggle(k, s.settings.stage.effects > 0, S::Effects.get(lang), |s| {
            s.settings.stage.effects = u8::from(s.settings.stage.effects == 0);
            s.mark_settings_dirty();
        });
        screen.push(item(S::Effects.get(lang), None, stage_fx));
    }
    screen.extend(backdrop_rows(s, k));

    vec![
        group("set-screen", Some(S::SetPlayerScreen.get(lang)), screen, None),
        lyrics(s, k),
        mini_section(s, k),
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
    let remove = row()
        .key("bd-remove")
        .role("button")
        .label(S::BdRemove.get(lang))
        .help(S::BdRemove.get(lang))
        .w(28.0)
        .h(28.0)
        .radius(7.0)
        .center()
        .fill(veil(0.06))
        .cursor(Cursor::Pointer)
        .hov(veil(0.11))
        .press(veil(0.15))
        .press_scale(th::PRESS)
        .on_press(|s: &mut State| s.drop_backdrop())
        .child(icon(&icons::CLOSE, 13.0).color(th::text2()));
    let buttons = row().gap(6.0).items_center().child(pick("bd-change", S::BdChange.get(lang))).child(remove);
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
        .h(22.0)
        .cursor(Cursor::Pointer)
        .on_drag(move |s: &mut State, d: &mut Drag, _tx: &mut Tx| {
            let f = if d.w > 0.0 { (d.x / d.w).clamp(0.0, 1.0) } else { 0.0 };
            set(s, (f * 100.0).round() / 100.0);
        })
        .child(el().absolute(0.0, 9.5).w(lw).h(3.0).radius(1.5).fill(veil(0.12)))
        .child(el().absolute(0.0, 9.5).w((lw * v).max(3.0)).h(3.0).radius(1.5).fill(ink))
        .child(el().absolute((lw * v - 6.5).clamp(0.0, lw - 13.0), 4.5).w(13.0).h(13.0).radius(6.5).fill(ink))
}

fn lyrics(s: &State, k: &rune_kit::Kit) -> El {
    let lang = s.lang;
    let word_lyrics = toggle(k, s.settings.word_lyrics, S::WordLyrics.get(lang), |s| s.set_word_lyrics(!s.settings.word_lyrics));
    let font = s.settings.lyric_look.font;
    let faces = segmented(k, &["Commissioner", "Mustica Pro", "Cormorant"], usize::from(font), |s, i| {
        s.settings.lyric_look.font = i as u8;
        s.mark_settings_dirty();
    });
    let mut rows = vec![
        item(S::WordLyrics.get(lang), None, word_lyrics),
        item(S::LyricsFont.get(lang), None, faces),
    ];
    for (j, (name, opts)) in menu::lyric_rows().into_iter().enumerate() {
        rows.push(item(name.get(lang), None, look_choice(s, k, 1 + j, opts)));
    }
    group("set-lyrics", Some(S::SetLyrics.get(lang)), rows, None)
}

fn mini_section(s: &State, k: &rune_kit::Kit) -> El {
    let lang = s.lang;
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
    let op_ctl = row().gap(10.0).items_center().child(figure(op_pct)).child(level_w("set-mini-opacity", op_frac, 110.0, |s, f| {
        s.set_mini_opacity(0.65 + f * 0.35);
    }));
    let mut rows = vec![
        item(S::MiniStyle.get(lang), None, mini_style_ctl),
        item(
            S::MiniAlwaysOnTop.get(lang),
            None,
            toggle(k, s.settings.mini_always_on_top, S::MiniAlwaysOnTop.get(lang), |s| s.set_mini_always_on_top(!s.settings.mini_always_on_top)),
        ),
        item(S::MiniOpacity.get(lang), None, op_ctl),
    ];
    if s.settings.show_tray && s.settings.close_to_tray {
        rows.push(item(
            S::MiniOnClose.get(lang),
            None,
            toggle(k, s.settings.mini_on_close, S::MiniOnClose.get(lang), |s| {
                s.settings.mini_on_close = !s.settings.mini_on_close;
                s.mark_settings_dirty();
            }),
        ));
    }
    group("set-mini", Some(S::MiniPlayer.get(lang)), rows, None)
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
    let usable = EQ_H - 2.0 * EQ_PAD;
    let knob_y = EQ_PAD + (0.5 - g / (2.0 * MAX_GAIN_DB)) * usable;
    let mid_y = EQ_PAD + 0.5 * usable;
    let (top, height) = if knob_y < mid_y { (knob_y, mid_y - knob_y) } else { (mid_y, knob_y - mid_y) };
    let color = th::text();
    let rail = veil(0.12);
    let tick = veil(0.22);
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
        .child(el().absolute(mx - 6.0, mid_y - 0.5).w(12.0).h(1.0).fill(tick))
        .child(el().absolute(mx - 1.5, top).w(3.0).h(height.max(0.5)).radius(1.5).fill(color))
        .child(el().absolute(mx - 5.5, knob_y - 5.5).w(11.0).h(11.0).radius(5.5).fill(color));

    col()
        .key(format!("eq-col-{i}"))
        .w(EQ_W)
        .gap(3.0)
        .items_center()
        .child(label(value, th::MONO, th::text2()))
        .child(slider)
        .child(label(hz, th::MONO, th::text3()))
}

fn equalizer(s: &State, k: &rune_kit::Kit) -> El {
    let lang = s.lang;
    let enabled = s.player.eq.enabled;
    let on = toggle(k, enabled, S::EqEnabled.get(lang), |s| {
        s.player.eq.enabled = !s.player.eq.enabled;
        s.apply_eq();
    });
    let mut rows = vec![item(S::EqEnabled.get(lang), None, on)];
    if enabled {
        let current = s.player.eq.preset_name().map_or_else(|| "—".to_owned(), |n| preset_label(n, lang));
        let open = s.popover_open && matches!(s.popover, Some(Popover::EqPreset { .. }));
        let presets = kit::btn_select("eq-preset", &current, 152.0, open).on_press(|s: &mut State| s.open_popover(Popover::EqPreset { anchor: "eq-preset".into() }));
        let reset = kit::btn_small("eq-reset", S::EqReset.get(lang), None).on_press(|s: &mut State| {
            s.player.eq.gains_db = [0.0; BANDS];
            s.player.eq.preamp_db = 0.0;
            s.apply_eq();
        });
        let bands = row().w_full().px(ROW_PAD - 4.0).pt(12.0).pb(10.0).justify_between().children((0..BANDS).map(|i| eq_band(s, i)));
        rows.push(item(S::EqPreset.get(lang), None, row().gap(8.0).items_center().child(presets).child(reset)));
        rows.push(bands);
    }
    group("set-eq", Some(S::Equalizer.get(lang)), rows, None)
}
