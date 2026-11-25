//! Minimal floating popover panel for Now Playing look (lyrics, effects, and backdrop).

use rune::{Cursor, El, Transition, col, el, icon, row};

use super::{EDGE, Entry, PAD};
use crate::app::State;
use crate::i18n::S;
use crate::store::{BackdropLook, LyricLook, StageLook};
use crate::theme as th;
use crate::ui::{
    icons,
    kit::{self, Hover as _, label},
    np, settings,
};

const STAGE_MIN: f32 = 280.0;
const STAGE_MAX: f32 = 312.0;
const STAGE_ROW: f32 = 34.0;
const PICK_W: f32 = 144.0;
const PICK_H: f32 = 24.0;
const PICK_PAD: f32 = 2.0;
const PICK_SEG_MIN: f32 = 56.0;
const STAGE_CAPTION: f32 = 22.0;
const STAGE_HEAD: f32 = 34.0;
const STAGE_DIV: f32 = 9.0;
const STAGE_MARGIN: f32 = 2.0 * EDGE + 2.0 * PAD + 8.0;
const PICK_SPRING: rune::Spring = rune::Spring::new(0.34, 0.12);
const LYRIC_FACES: [&str; 3] = ["Plex Mono", "Commissioner", "Cormorant"];

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
}

fn face(f: Face, j: usize, name: &str, on: bool) -> El {
    let ink = if on { th::text() } else { th::text2() };
    match f {
        Face::Word => label(name.to_owned(), rune::TextStyle::new(11.5, if on { 520.0 } else { 420.0 }).family(th::SANS).lh(15.0), ink).truncate(),
        Face::Size => {
            let px = [10.5, 13.0, 16.0][j.min(2)];
            label("Aa".to_owned(), rune::TextStyle::new(px, 600.0).family(th::SANS).lh(18.0), ink)
        }
        Face::Font => {
            let mut st = th::lyric_face(th::LYRIC_CARD, j as u8);
            st.size *= 0.62;
            st.line_height = 20.0;
            label("Aa".to_owned(), st, ink)
        }
        Face::Align => {
            let bars = col().w(14.0).gap(2.0);
            let bars = match j {
                0 => bars.items_start(),
                1 => bars.items_center(),
                _ => bars.items_end(),
            };
            bars.children([14.0, 8.0, 11.0].map(|w| el().w(w).h(1.8).radius(0.9).fill(ink)))
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
    let per = if matches!(look, Face::Word) { n.div_ceil(pick_lines(width, n)) } else { n };
    let lines = n.div_ceil(per);
    let seg = (width - 2.0 * PICK_PAD) / per as f32;
    let cell = PICK_H - 2.0 * PICK_PAD;
    let height = lines as f32 * cell + 2.0 * PICK_PAD;
    let at = current.min(n - 1);
    let thumb_fill = if th::light() { th::rgb(0xffffff) } else { th::veil(0.14) };
    let mut thumb = el()
        .key(format!("{key}-thumb"))
        .absolute(PICK_PAD + (at % per) as f32 * seg, PICK_PAD + (at / per) as f32 * cell)
        .w(seg)
        .h(cell)
        .radius(5.5)
        .fill(thumb_fill)
        .motion(PICK_SPRING);
    if th::light() {
        thumb = thumb.border(1.0, th::veil(0.08));
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
            .px(3.0)
            .radius(5.5)
            .center()
            .cursor(Cursor::Pointer)
            .hov(th::veil(if on { 0.0 } else { 0.045 }))
            .on_press(move |s: &mut State| set(s, i, j))
            .child(face(look, j, name, on));
        if !matches!(look, Face::Word) {
            c = c.help(name);
        }
        line = line.child(c);
        if (j + 1) % per == 0 || j + 1 == names.len() {
            choices = choices.child(line);
            line = row().w_full().h(cell);
        }
    }
    el().key(key.to_owned()).w(width).h(height).radius(7.5).fill(th::veil(0.06)).child(thumb).child(choices)
}

fn mini_switch(key: &str, on: bool, accent: rune::Color, name: &str, toggle: fn(&mut State)) -> El {
    let w = 34.0;
    let h = 20.0;
    let knob = 14.0;
    let x = if on { w - knob - 3.0 } else { 3.0 };
    let track = if on { accent } else { th::veil(0.14) };
    let dot = th::rgb(0xffffff);
    el().key(key.to_owned())
        .role("switch")
        .label(name)
        .checked(on)
        .w(w)
        .h(h)
        .radius(h * 0.5)
        .fill(track)
        .cursor(Cursor::Pointer)
        .press_scale(0.95)
        .on_press(toggle)
        .child(el().key(format!("{key}-knob")).absolute(x, 3.0).w(knob).h(knob).radius(knob * 0.5).fill(dot).motion(PICK_SPRING))
}

fn inline_btn(key: &str, text: &str) -> El {
    row()
        .key(key.to_owned())
        .role("button")
        .label(text)
        .h(24.0)
        .px(10.0)
        .radius(6.5)
        .center()
        .fill(th::veil(0.07))
        .cursor(Cursor::Pointer)
        .hov(th::veil(0.12))
        .press(th::veil(0.16))
        .press_scale(th::PRESS)
        .child(label(text.to_owned(), th::SMALL, th::text()))
}

fn look_row(name: &str, control: El) -> (El, f32) {
    (
        row()
            .w_full()
            .h(STAGE_ROW)
            .pl(10.0)
            .pr(8.0)
            .gap(10.0)
            .items_center()
            .child(label(name.to_owned(), th::BODY, th::text()).truncate().grow(1.0).min_w(0.0))
            .child(control),
        STAGE_ROW,
    )
}

fn look_row_below(name: &str, control: El, ctl_h: f32) -> (El, f32) {
    let h = 24.0 + ctl_h + 8.0;
    (
        col()
            .w_full()
            .h(h)
            .pl(10.0)
            .pr(8.0)
            .child(row().w_full().h(24.0).items_center().child(label(name.to_owned(), th::BODY, th::text()).truncate()))
            .child(control),
        h,
    )
}

fn look_section(key: &str, caption: &str, rows: Vec<(El, f32)>) -> (El, f32) {
    let mut body = col()
        .key(key.to_owned())
        .w_full()
        .child(row().w_full().h(STAGE_CAPTION).pl(10.0).pb(2.0).items_end().child(kit::caps(caption, th::text3())));
    let mut h = STAGE_CAPTION;
    for (r, rh) in rows {
        body = body.child(r);
        h += rh;
    }
    (body.h(h), h)
}

fn section_divider() -> (El, f32) {
    (row().w_full().h(STAGE_DIV).px(8.0).items_center().child(el().w_full().h(1.0).fill(th::hairline())), STAGE_DIV)
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

fn toggle_effects(s: &mut State) {
    s.settings.stage.effects = u8::from(s.settings.stage.effects == 0);
    s.mark_settings_dirty();
}

fn look_text(s: &State) -> (El, f32) {
    let lang = s.lang;
    let mut rows = vec![look_row(
        S::LyricFace.get(lang),
        picker("sl-face", PICK_W, &LYRIC_FACES, usize::from(s.settings.lyric_look.font), Face::Font, 0, set_face),
    )];
    for (k, (name, opts)) in lyric_rows().into_iter().enumerate() {
        let i = 1 + k;
        let names = opts.map(|o| o.get(lang));
        let look = match i {
            1 => Face::Size,
            2 => Face::Align,
            _ => Face::Word,
        };
        rows.push(look_row(name.get(lang), picker(&format!("sl-{i}"), PICK_W, &names, usize::from(stage_value(s, i)), look, i, set_look)));
    }
    look_section("sl-text", S::LyricText.get(lang), rows)
}

fn look_back(s: &State, pw: f32) -> (El, f32) {
    let lang = s.lang;
    let mut rows = vec![look_row(
        S::Effects.get(lang),
        mini_switch("sl-0", s.settings.stage.effects > 0, s.pal().accent, S::Effects.get(lang), toggle_effects),
    )];
    let bd = s.settings.backdrop;
    let pick = |key: &str, name: &str| inline_btn(key, name).on_press_tx(|s: &mut State, tx: &mut rune::Tx| s.pick_backdrop(tx));
    let picture = if bd.on {
        row()
            .gap(4.0)
            .items_center()
            .child(pick("sl-bd-change", S::BdChange.get(lang)))
            .child(
                row()
                    .key("sl-bd-remove")
                    .role("button")
                    .label(S::BdRemove.get(lang))
                    .help(S::BdRemove.get(lang))
                    .w(24.0)
                    .h(24.0)
                    .radius(6.5)
                    .center()
                    .fill(th::veil(0.07))
                    .cursor(Cursor::Pointer)
                    .hov(th::veil(0.12))
                    .press(th::veil(0.16))
                    .press_scale(th::PRESS)
                    .on_press(|s: &mut State| s.drop_backdrop())
                    .child(icon(&icons::CLOSE, 12.0).color(th::text2())),
            )
    } else {
        pick("sl-bd-pick", S::BdPick.get(lang))
    };
    rows.push(look_row(S::BdPicture.get(lang), picture));
    if bd.on {
        let lw = PICK_W - 4.0;
        rows.push(look_row(
            S::BdBlur.get(lang),
            settings::level_w("sl-bd-blur", bd.blur, lw, |s, f| {
                s.settings.backdrop.blur = f;
                s.mark_settings_dirty();
            })
            .mr(2.0),
        ));
        rows.push(look_row(
            S::BdDim.get(lang),
            settings::level_w("sl-bd-dim", bd.dim, lw, |s, f| {
                s.settings.backdrop.dim = f;
                s.mark_settings_dirty();
            })
            .mr(2.0),
        ));
        let names = crate::backdrop::EFFECTS.map(|e| e.get(lang));
        let full = (pw - 2.0 * PAD - 18.0).max(PICK_W);
        rows.push(look_row_below(S::BdEffect.get(lang), picker("sl-bd-effect", full, &names, usize::from(bd.effect), Face::Word, 0, set_effect), pick_h(full, names.len())));
    }
    look_section("sl-back", S::StageBack.get(lang), rows)
}

pub(super) fn stage_w(s: &State, w: f32, h: f32) -> f32 {
    let (_, cover) = np::cover_span(s, w, h);
    cover.clamp(STAGE_MIN, STAGE_MAX).min(w - 2.0 * EDGE)
}

pub(super) fn stage_entries(s: &State, pw: f32, win_h: f32) -> Vec<Entry> {
    let lang = s.lang;
    let changed = look_changed(s);
    let reset = changed.then(|| {
        row()
            .key("sl-reset")
            .role("button")
            .label(S::StageReset.get(lang))
            .h(24.0)
            .px(8.0)
            .radius(6.0)
            .center()
            .cursor(Cursor::Pointer)
            .hov(th::veil(0.06))
            .press(th::veil(0.11))
            .press_scale(th::PRESS)
            .transition(Transition::none())
            .on_press(reset_look)
            .child(label(S::StageReset.get(lang).to_owned(), th::SMALL, th::text2()))
    });
    let head = row()
        .w_full()
        .h(STAGE_HEAD)
        .pl(10.0)
        .pr(6.0)
        .gap(8.0)
        .items_center()
        .child(label(S::StageLook.get(lang).to_owned(), th::BODY_M, th::text()).truncate().grow(1.0).min_w(0.0))
        .children(reset);
    let (div_top, div_top_h) = section_divider();
    let (text, text_h) = look_text(s);
    let (div_mid, div_mid_h) = section_divider();
    let (back, back_h) = look_back(s, pw);
    let body_h = div_top_h + text_h + div_mid_h + back_h + 2.0;
    let room = (win_h - STAGE_MARGIN - STAGE_HEAD).max(160.0);
    let body = col().w_full().pb(2.0).child(div_top).child(text).child(div_mid).child(back);
    if body_h <= room {
        return vec![Entry::Block(head, STAGE_HEAD), Entry::Block(body.h(body_h), body_h)];
    }
    let scroll = col().key("sl-scroll").w_full().h(room).scroll(rune::ScrollSpec { stick_bottom: false }).child(body);
    vec![Entry::Block(head, STAGE_HEAD), Entry::Block(scroll, room)]
}
