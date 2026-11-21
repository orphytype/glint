//! Rich popover panels: Now Playing look (lyrics/effects/backdrop) and My Wave tuning.

use rune::{Cursor, El, Transition, col, el, row};

use super::{EDGE, Entry, PAD};
use crate::app::State;
use crate::i18n::S;
use crate::store::{BackdropLook, LyricLook, StageLook, WaveLook, WaveTune};
use crate::theme as th;
use crate::ui::{
    icons,
    kit::{self, Hover as _, label},
    np, settings, wave_title,
};

pub(super) const STAGE_W: f32 = 440.0;
const STAGE_MIN: f32 = 260.0;
const STAGE_INLINE: f32 = 400.0;
const STAGE_ROW: f32 = 46.0;
const PICK_W: f32 = 228.0;
const PICK_H: f32 = 32.0;
const PICK_PAD: f32 = 3.0;
const PICK_SEG_MIN: f32 = 58.0;
const STAGE_CAPTION: f32 = 30.0;
const STAGE_AFTER: f32 = 8.0;
const STAGE_HEAD: f32 = 56.0;
const STAGE_MARGIN: f32 = 2.0 * EDGE + 2.0 * PAD + 8.0;
const PICK_SPRING: rune::Spring = rune::Spring::new(0.34, 0.12);
const LYRIC_FACES: [&str; 3] = ["Plex Mono", "Commissioner", "Cormorant"];
const WIDE_PICK: f32 = STAGE_W - 2.0 * PAD - 8.0 - 12.0 - 7.0;

pub fn stage_rows() -> [(S, [S; 3]); 1] {
    [(S::Effects, [S::StageOff, S::StageCalm, S::StageLively])]
}

pub fn lyric_rows() -> [(S, [S; 3]); 2] {
    [(S::LyricSize, [S::SizeSmall, S::SizeMedium, S::SizeLarge]), (S::LyricAlign, [S::AlignLeft, S::AlignCenter, S::AlignRight])]
}

pub fn stage_value(s: &State, i: usize) -> u8 {
    let (look, ly) = (&s.settings.stage, &s.settings.lyric_look);
    [look.effects, ly.size, ly.align][i]
}

pub fn set_stage_value(s: &mut State, i: usize, v: u8) {
    let (l, ly) = (&mut s.settings.stage, &mut s.settings.lyric_look);
    match i {
        0 => l.effects = v,
        1 => ly.size = v,
        _ => ly.align = v,
    }
    s.mark_settings_dirty();
}

pub fn look_changed(s: &State) -> bool {
    let bd_rest = BackdropLook { rev: s.settings.backdrop.rev, ..Default::default() };
    s.settings.stage != StageLook::default() || s.settings.lyric_look != LyricLook::default() || s.settings.backdrop != bd_rest
}

pub fn reset_look(s: &mut State) {
    s.settings.stage = StageLook::default();
    s.settings.lyric_look = LyricLook::default();
    if s.settings.backdrop.on {
        s.drop_backdrop();
    }
    s.settings.backdrop = BackdropLook { rev: s.settings.backdrop.rev, ..Default::default() };
    s.mark_settings_dirty();
}

#[derive(Clone, Copy)]
enum Face {
    Word,
    Size,
    Align,
    Font,
    Serif,
}

fn face(f: Face, j: usize, name: &str, on: bool) -> El {
    let ink = if on { th::text() } else { th::text2() };
    match f {
        Face::Word => label(name.to_owned(), th::SMALL, ink).truncate(),
        Face::Serif => label(name.to_owned(), th::wave_words(16.0), ink).truncate(),
        Face::Size => {
            let px = [11.0, 14.0, 17.5][j.min(2)];
            label("Aa".to_owned(), rune::TextStyle::new(px, 600.0).family(th::SANS).lh(20.0), ink)
        }
        Face::Font => {
            let mut st = th::lyric_face(th::LYRIC_CARD, j as u8);
            st.size *= 0.66;
            st.line_height = 22.0;
            label("Aa".to_owned(), st, ink)
        }
        Face::Align => {
            let bars = col().w(16.0).gap(2.5);
            let bars = match j {
                0 => bars.items_start(),
                1 => bars.items_center(),
                _ => bars.items_end(),
            };
            bars.children([16.0, 9.0, 12.5].map(|w| el().w(w).h(2.0).radius(1.0).fill(ink)))
        }
    }
}

fn pick_lines(width: f32, n: usize) -> usize {
    if (width - 2.0 * PICK_PAD) / n.max(1) as f32 >= PICK_SEG_MIN { 1 } else { 2 }
}

fn pick_h(width: f32, n: usize) -> f32 {
    pick_lines(width, n) as f32 * (PICK_H - 2.0 * PICK_PAD) + 2.0 * PICK_PAD
}

fn picker(key: &str, width: f32, names: &[&str], current: usize, look: Face, i: usize, set: fn(&mut State, usize, usize)) -> El {
    let n = names.len().max(1);
    let per = n.div_ceil(pick_lines(width, n));
    let seg = (width - 2.0 * PICK_PAD) / per as f32;
    let cell = PICK_H - 2.0 * PICK_PAD;
    let height = pick_h(width, n);
    let at = current.min(n - 1);
    let thumb_fill = if th::light() { th::rgb(0xffffff) } else { th::veil(0.15) };
    let mut thumb = el()
        .key(format!("{key}-thumb"))
        .absolute(PICK_PAD + (at % per) as f32 * seg, PICK_PAD + (at / per) as f32 * cell)
        .w(seg)
        .h(cell)
        .radius(7.5)
        .fill(thumb_fill)
        .motion(PICK_SPRING);
    if th::light() {
        thumb = thumb.shadow(1.0, 4.0, th::rgba(0x000000, 0.12));
    }
    let mut choices = col().absolute(PICK_PAD, PICK_PAD).w(width - 2.0 * PICK_PAD).h(height - 2.0 * PICK_PAD);
    let mut line = row().w_full().h(cell);
    for (j, &name) in names.iter().enumerate() {
        let on = at == j;
        let mut c = row()
            .key(format!("{key}-{j}"))
            .role("radio")
            .label(name)
            .checked(on)
            .w(seg)
            .h_full()
            .px(4.0)
            .radius(7.5)
            .center()
            .cursor(Cursor::Pointer)
            .hov(th::veil(if on { 0.0 } else { 0.05 }))
            .on_press(move |s: &mut State| set(s, i, j))
            .child(face(look, j, name, on));
        if !matches!(look, Face::Word | Face::Serif) {
            c = c.help(name);
        }
        line = line.child(c);
        if (j + 1) % per == 0 || j + 1 == names.len() {
            choices = choices.child(line);
            line = row().w_full().h(cell);
        }
    }
    el().key(key.to_owned()).w(width).h(height).radius(10.0).fill(th::veil(0.06)).child(thumb).child(choices)
}

fn look_row(name: &str, control: El) -> (El, f32) {
    (
        row().w_full().h(STAGE_ROW).pl(12.0).pr(7.0).gap(12.0).items_center().child(label(name.to_owned(), th::BODY, th::text()).truncate().grow(1.0).min_w(0.0)).child(control),
        STAGE_ROW,
    )
}

fn look_row_below(name: &str, control: El, ctl_h: f32) -> (El, f32) {
    let h = STAGE_ROW - 10.0 + ctl_h + 10.0;
    (col().w_full().h(h).pl(12.0).pr(7.0).child(row().w_full().h(STAGE_ROW - 10.0).items_center().child(label(name.to_owned(), th::BODY, th::text()).truncate())).child(control), h)
}

fn full_pick(pw: f32) -> f32 {
    pw - 2.0 * PAD - 8.0 - 12.0 - 7.0
}

struct LookSpec<'a> {
    name: &'a str,
    key: &'a str,
    names: &'a [&'a str],
    current: usize,
    look: Face,
}

fn look_line(pw: f32, spec: LookSpec<'_>, i: usize, set: fn(&mut State, usize, usize)) -> (El, f32) {
    if pw >= STAGE_INLINE {
        return look_row(spec.name, picker(spec.key, PICK_W, spec.names, spec.current, spec.look, i, set));
    }
    let full = full_pick(pw);
    look_row_below(spec.name, picker(spec.key, full, spec.names, spec.current, spec.look, i, set), pick_h(full, spec.names.len()))
}

fn look_group(key: &str, caption: El, rows: Vec<(El, f32)>) -> (El, f32) {
    let n = rows.len();
    let mut plate = col().key(format!("{key}-plate")).w_full().radius(12.0).fill(th::veil(0.045)).border(1.0, th::veil(0.06));
    let mut plate_h = 0.0;
    for (k, (r, rh)) in rows.into_iter().enumerate() {
        if k > 0 {
            plate = plate.child(row().w_full().h(1.0).pl(12.0).child(el().grow(1.0).h(1.0).fill(th::veil(0.06))));
        }
        plate = plate.child(r);
        plate_h += rh;
    }
    plate_h += n.saturating_sub(1) as f32;
    let h = STAGE_CAPTION + plate_h + STAGE_AFTER;
    let g = col().key(key.to_owned()).w_full().h(h).px(4.0).child(row().h(STAGE_CAPTION).pl(8.0).pb(7.0).items_end().child(caption)).child(plate);
    (g, h)
}

fn set_look(s: &mut State, i: usize, v: usize) {
    set_stage_value(s, i, v as u8);
}

fn set_face(s: &mut State, _: usize, v: usize) {
    s.settings.lyric_look.font = v as u8;
    s.mark_settings_dirty();
}

fn set_effect(s: &mut State, _: usize, v: usize) {
    s.settings.backdrop.effect = v as u8;
    s.mark_settings_dirty();
}

fn look_back(s: &State, pw: f32) -> (El, f32) {
    let lang = s.lang;
    let mut rows = Vec::new();
    for (i, (name, opts)) in stage_rows().into_iter().enumerate() {
        let names = opts.map(|o| o.get(lang));
        rows.push(look_line(pw, LookSpec { name: name.get(lang), key: &format!("sl-{i}"), names: &names, current: usize::from(stage_value(s, i)), look: Face::Word }, i, set_look));
    }
    let bd = s.settings.backdrop;
    let pick = |key: &str, name: &str| kit::btn_small(key.to_owned(), name, None).on_press_tx(|s: &mut State, tx: &mut rune::Tx| s.pick_backdrop(tx));
    let picture = if bd.on {
        row()
            .gap(6.0)
            .items_center()
            .child(pick("sl-bd-change", S::BdChange.get(lang)))
            .child(kit::btn_small("sl-bd-remove", S::BdRemove.get(lang), None).on_press(|s: &mut State| s.drop_backdrop()))
    } else {
        row().child(pick("sl-bd-pick", S::BdPick.get(lang)))
    };
    let wide = pw >= STAGE_INLINE;
    rows.push(if wide { look_row(S::BdPicture.get(lang), picture) } else { look_row_below(S::BdPicture.get(lang), picture, 32.0) });
    if bd.on {
        let lw = if wide { PICK_W - 8.0 } else { (pw - 141.0).max(80.0) };
        rows.push(look_row(
            S::BdBlur.get(lang),
            settings::level_w("sl-bd-blur", bd.blur, lw, |s, f| {
                s.settings.backdrop.blur = f;
                s.mark_settings_dirty();
            })
            .mr(4.0),
        ));
        rows.push(look_row(
            S::BdDim.get(lang),
            settings::level_w("sl-bd-dim", bd.dim, lw, |s, f| {
                s.settings.backdrop.dim = f;
                s.mark_settings_dirty();
            })
            .mr(4.0),
        ));
        let names = crate::backdrop::EFFECTS.map(|e| e.get(lang));
        let full = full_pick(pw);
        rows.push(look_row_below(S::BdEffect.get(lang), picker("sl-bd-effect", full, &names, usize::from(bd.effect), Face::Word, 0, set_effect), pick_h(full, names.len())));
    }
    look_group("sl-back", kit::caps(S::StageBack.get(s.lang), th::text3()), rows)
}

fn look_text(s: &State, pw: f32) -> (El, f32) {
    let lang = s.lang;
    let mut rows = vec![look_line(
        pw,
        LookSpec { name: S::LyricFace.get(lang), key: "sl-face", names: &LYRIC_FACES, current: usize::from(s.settings.lyric_look.font), look: Face::Font },
        0,
        set_face,
    )];
    for (k, (name, opts)) in lyric_rows().into_iter().enumerate() {
        let i = 1 + k;
        let names = opts.map(|o| o.get(lang));
        let look = match i {
            1 => Face::Size,
            2 => Face::Align,
            _ => Face::Word,
        };
        rows.push(look_line(pw, LookSpec { name: name.get(lang), key: &format!("sl-{i}"), names: &names, current: usize::from(stage_value(s, i)), look }, i, set_look));
    }
    look_group("sl-text", kit::caps(S::LyricText.get(s.lang), th::text3()), rows)
}

pub(super) fn stage_w(s: &State, w: f32, h: f32) -> f32 {
    let (_, cover) = np::cover_span(s, w, h);
    cover.max(STAGE_MIN).min(w - 2.0 * EDGE)
}

pub(super) fn stage_entries(s: &State, pw: f32, win_h: f32) -> Vec<Entry> {
    let lang = s.lang;
    let changed = look_changed(s);
    let mut reset = row().key("sl-reset").role("button").label(S::StageReset.get(lang)).h(28.0).px(12.0).radius(14.0).center().transition(Transition::none());
    reset = if changed {
        reset.cursor(Cursor::Pointer).fill(th::veil(0.07)).hov(th::veil(0.11)).press(th::veil(0.15)).press_scale(th::PRESS).on_press(reset_look)
    } else {
        reset.disabled(true)
    };
    let reset = reset.child(label(S::StageReset.get(lang).to_owned(), th::SMALL, if changed { th::text() } else { th::text3().alpha(0.6) }));
    let head = row()
        .w_full()
        .h(STAGE_HEAD)
        .pl(8.0)
        .pr(6.0)
        .gap(12.0)
        .items_center()
        .child(row().w(34.0).h(34.0).radius(10.0).center().fill(th::veil(0.07)).child(rune::icon(&icons::STAGE, 18.0).color(th::text())))
        .child(label(S::StageLook.get(lang).to_owned(), th::BODY_M, th::text()).truncate().grow(1.0).min_w(0.0))
        .child(reset);
    let (text, text_h) = look_text(s, pw);
    let (back, back_h) = look_back(s, pw);
    let body_h = text_h + back_h;
    let room = (win_h - STAGE_MARGIN - STAGE_HEAD).max(160.0);
    let body = col().w_full().child(text).child(back);
    if body_h <= room {
        return vec![Entry::Block(head, STAGE_HEAD), Entry::Block(body.h(body_h), body_h)];
    }
    let scroll = col().key("sl-scroll").w_full().h(room).scroll(rune::ScrollSpec { stick_bottom: false }).scroll_edge_fade(12.0).child(body);
    vec![Entry::Block(head, STAGE_HEAD), Entry::Block(scroll, room)]
}

fn tune_row(name: &str, control: El) -> (El, f32) {
    let h = STAGE_ROW - 10.0 + PICK_H + 10.0;
    (
        col()
            .w_full()
            .h(h)
            .pl(12.0)
            .pr(7.0)
            .child(row().w_full().h(STAGE_ROW - 10.0).items_center().child(label(name.to_owned(), th::wave_words(18.0), th::text()).truncate()))
            .child(control),
        h,
    )
}

fn set_wave_knob(s: &mut State, i: usize, v: usize) {
    let v = v as u8;
    match i {
        3 => s.settings.wave_look.form = v,
        4 => s.settings.wave_look.print = v,
        _ => {
            let id = s.wave_choice.clone();
            let mut t = s.wave_tune(&id);
            match i {
                0 => t.diversity = v,
                1 => t.mood = v,
                _ => t.language = v,
            }
            s.set_wave_tune(&id, t);
            return;
        }
    }
    s.mark_settings_dirty();
}

pub(super) fn wave_tune_entries(s: &State, win_h: f32) -> Vec<Entry> {
    let lang = s.lang;
    let id = &s.wave_choice;
    let tune = s.wave_tune(id);
    let tunable = WaveTune::applies(id);
    let changed = (tunable && tune != WaveTune::of(id)) || s.settings.wave_look != WaveLook::default();
    let mut reset = row().key("wt-reset").role("button").label(S::StageReset.get(lang)).h(28.0).px(12.0).radius(14.0).center().transition(Transition::none());
    reset = if changed {
        reset.cursor(Cursor::Pointer).fill(th::veil(0.07)).hov(th::veil(0.11)).press(th::veil(0.15)).press_scale(th::PRESS).on_press(|s: &mut State| {
            let id = s.wave_choice.clone();
            s.settings.wave_look = WaveLook::default();
            s.mark_settings_dirty();
            s.set_wave_tune(&id, WaveTune::of(&id));
        })
    } else {
        reset.disabled(true)
    };
    let reset = reset.child(label(S::StageReset.get(lang).to_owned(), th::SMALL, if changed { th::text() } else { th::text3().alpha(0.6) }));
    let head = row()
        .w_full()
        .h(STAGE_HEAD)
        .pl(8.0)
        .pr(6.0)
        .gap(12.0)
        .items_center()
        .child(row().w(34.0).h(34.0).radius(10.0).center().fill(th::veil(0.07)).child(rune::icon(&icons::TUNE, 18.0).color(th::text())))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .child(label(S::WaveTitle.get(lang).to_owned(), th::wave_words(22.0), th::text()))
                .child(label(format!("{} · {}", S::WaveTuneOf.get(lang), wave_title(id).get(lang)), th::wave_words(15.0), th::text3()).truncate()),
        )
        .child(reset);
    let caption = |t: S| label(t.get(lang).to_owned(), th::wave_words(16.0), th::text3());
    let (knobs, knobs_h) = if tunable {
        let divs = [S::WaveAnyOne, S::WaveFavorite, S::WaveDiscover, S::WavePopular].map(|n| n.get(lang));
        let moods = [S::WaveAnyMood, S::WaveEnergetic, S::WaveFun, S::WaveCalm, S::WaveSad].map(|n| n.get(lang));
        let langs = [S::WaveAnyOne, S::WaveRussian, S::WaveForeign, S::WaveNoWords].map(|n| n.get(lang));
        let rows = vec![
            tune_row(S::WaveCharacter.get(lang), picker("wt-div", WIDE_PICK, &divs, usize::from(tune.diversity), Face::Serif, 0, set_wave_knob)),
            tune_row(S::Mood.get(lang), picker("wt-mood", WIDE_PICK, &moods, usize::from(tune.mood), Face::Serif, 1, set_wave_knob)),
            tune_row(S::WaveLanguage.get(lang), picker("wt-lang", WIDE_PICK, &langs, usize::from(tune.language), Face::Serif, 2, set_wave_knob)),
        ];
        look_group("wt-knobs", caption(S::WaveTitle), rows)
    } else {
        let note =
            (row().w_full().h(STAGE_ROW).pl(12.0).items_center().child(label(S::WaveTuneNone.get(lang).to_owned(), th::wave_words(16.0), th::text2()).truncate()), STAGE_ROW);
        look_group("wt-knobs", caption(S::WaveTitle), vec![note])
    };
    let look = s.settings.wave_look;
    let forms = [S::FormLiquid, S::FormFlower, S::FormPinwheel, S::FormCloud].map(|n| n.get(lang));
    let prints = [S::PrintSmooth, S::PrintDither, S::PrintDots, S::PrintLines].map(|n| n.get(lang));
    let (looks, looks_h) = look_group(
        "wt-look",
        caption(S::WaveLook),
        vec![
            tune_row(S::WaveForm.get(lang), picker("wt-form", WIDE_PICK, &forms, usize::from(look.form), Face::Serif, 3, set_wave_knob)),
            tune_row(S::WavePrint.get(lang), picker("wt-print", WIDE_PICK, &prints, usize::from(look.print), Face::Serif, 4, set_wave_knob)),
        ],
    );
    let body_h = knobs_h + looks_h;
    let room = (win_h - STAGE_MARGIN - STAGE_HEAD).max(160.0);
    let body = col().w_full().child(knobs).child(looks);
    if body_h <= room {
        return vec![Entry::Block(head, STAGE_HEAD), Entry::Block(body.h(body_h), body_h)];
    }
    let scroll = col().key("wt-scroll").w_full().h(room).scroll(rune::ScrollSpec { stick_bottom: false }).scroll_edge_fade(12.0).child(body);
    vec![Entry::Block(head, STAGE_HEAD), Entry::Block(scroll, room)]
}
