//! Details modal for a track, album, playlist, or artist. Morphs out of the source cover and
//! supports pull-down dismissal, credits, artist bio, links, photos, and release facts.

mod links;
mod sections;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use rune::{Align, Cursor, Cx, Drag, El, Phase, Transition, Tx, col, el, row};

use super::icons;
use super::kit::{self, Hover as _, label};
use super::menu::track_link;
use crate::app::{Page, Sheet, State};
use crate::fx;
use crate::i18n::{Lang, S};
use crate::theme as th;

const MAX_W: f32 = 880.0;
const MAX_H: f32 = 620.0;
const PAD: f32 = 28.0;
const RADIUS: f32 = 26.0;
const COVER_R: f32 = 18.0;
const ROUND: f32 = 34.0;
const SECTION_GAP: f32 = 28.0;

type Spot = ([f32; 4], f32);
type Tap = Rc<dyn Fn(&mut State, &mut Tx)>;

thread_local! {
    /// Last known source cover rect and radius for `sheet_seq` so closing always returns there.
    static FROM: RefCell<(u64, Option<Spot>)> = const { RefCell::new((0, None)) };
}

fn source_now(s: &State, cx: &Cx<State>, what: &Sheet) -> Option<Spot> {
    match what {
        Sheet::Track(t) => {
            let playing = s.current().is_some_and(|c| c.id == t.id);
            if playing
                && s.np_open
                && let Some(r) = cx.window_layout("np-cover")
            {
                return Some((r, 28.0));
            }
            if let Some(r) = cx.window_layout(&format!("row-{}", t.id)) {
                let c = kit::ROW_COVER;
                return Some(([r[0] + kit::ROW_PAD, r[1] + (r[3] - c) * 0.5, c, c], kit::ROW_COVER_R));
            }
            if playing {
                return cx.window_layout("bar-cover").map(|r| (r, 10.0));
            }
            None
        }
        Sheet::Page => cx.window_layout("detail-cover").map(|r| (r, if s.detail.is_artist { r[2] * 0.5 } else { super::pages::HEAD_COVER_R })),
    }
}

fn source(s: &State, cx: &Cx<State>, what: &Sheet) -> Option<Spot> {
    let now = source_now(s, cx, what);
    FROM.with_borrow_mut(|(seq, last)| {
        if *seq != s.sheet_seq {
            *seq = s.sheet_seq;
            *last = None;
        }
        if now.is_some() {
            *last = now;
        }
        *last
    })
}

fn rubber(x: f32, d: f32) -> f32 {
    (1.0 - 1.0 / (0.55 * x / d + 1.0)) * d
}

fn project(v: f32) -> f32 {
    v / 1000.0 * 0.998 / (1.0 - 0.998)
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn arrival(p: f32, from: f32) -> f32 {
    smooth((p - from) / (1.0 - from).max(0.05))
}

fn pull_handle(e: El, ch: f32, tap: Option<Tap>) -> El {
    let cursor = if tap.is_some() { Cursor::Pointer } else { Cursor::Grab };
    e.cursor(cursor).on_drag(move |s: &mut State, d: &mut Drag, tx: &mut Tx| match d.phase {
        Phase::Down => {
            s.sheet_dragging = true;
            d.data = s.sheet_drag.t;
            s.sheet_drag.v = 0.0;
            s.sheet_drag_vel = 0.0;
            s.sheet_drag_mark = Some((Instant::now(), s.sheet_drag.t));
        }
        Phase::Move => {
            let raw = d.data + d.dy;
            let y = if raw >= 0.0 { raw } else { -rubber(-raw, ch) };
            let now = Instant::now();
            if let Some((t0, y0)) = s.sheet_drag_mark {
                let dt = now.duration_since(t0).as_secs_f32();
                if dt > 0.004 {
                    let v = (y - y0) / dt;
                    s.sheet_drag_vel = 0.6 * v + 0.4 * s.sheet_drag_vel;
                    s.sheet_drag_mark = Some((now, y));
                }
            }
            s.sheet_drag.t = y;
        }
        Phase::Up => {
            s.sheet_dragging = false;
            if !d.cancelled()
                && d.dx.abs() < 4.0
                && d.dy.abs() < 4.0
                && let Some(tap) = &tap
            {
                s.sheet_drag.v = 0.0;
                tap(s, tx);
                return;
            }
            let still = s.sheet_drag_mark.is_some_and(|(t0, _)| t0.elapsed().as_secs_f32() > 0.1);
            let v = if still || d.cancelled() { 0.0 } else { s.sheet_drag_vel };
            let landing = s.sheet_drag.t + project(v);
            if !d.cancelled() && d.dy > 6.0 && landing > ch * 0.24 {
                s.sheet_open = false;
            }
            s.sheet_drag.v = v;
        }
    })
}

fn round_btn(key: &str, ic: &'static rune::Icon, name: &str) -> El {
    row()
        .key(key.to_owned())
        .role("button")
        .label(name)
        .help(name)
        .w(ROUND)
        .h(ROUND)
        .radius(ROUND * 0.5)
        .center()
        .cursor(Cursor::Pointer)
        .fill(th::veil(0.07))
        .hov(th::veil(0.13))
        .press(th::veil(0.17))
        .press_scale(th::PRESS)
        .hov_group()
        .child(rune::icon(ic, 15.0).color(th::text2()).gcol(th::text()))
}

struct Head {
    kind: S,
    cover: Option<String>,
    title: String,
    artists: Vec<ym_api::Artist>,
    sub: String,
    chips: Vec<String>,
    explicit: bool,
    link: Option<String>,
    round: bool,
    release: Option<Page>,
}

fn head_of(s: &State, what: &Sheet) -> Head {
    let lang = s.lang;
    match what {
        Sheet::Track(tr) => {
            let mut chips = Vec::new();
            let al = tr.albums.first();
            if let Some(y) = al.and_then(|a| a.year) {
                chips.push(y.to_string());
            }
            if let Some(g) = al.and_then(|a| a.genre.as_deref()).filter(|g| !g.is_empty()) {
                chips.push(crate::genre::name(g, lang));
            }
            if let Some(ms) = tr.duration_ms {
                chips.push(crate::player::fmt_time(ms));
            }
            Head {
                kind: S::Track,
                cover: tr.cover_template().map(str::to_owned),
                title: tr.full_title(),
                artists: tr.artists.clone(),
                sub: String::new(),
                chips,
                explicit: tr.is_explicit(),
                link: Some(track_link(tr)),
                round: false,
                release: al.map(|a| Page::Album(a.id.clone())),
            }
        }
        Sheet::Page => {
            let d = &s.detail;
            let kind = match d.page {
                Some(Page::Album(_)) => crate::app::release_kind_name(d.release_kind.as_deref()),
                Some(Page::Playlist(..)) => S::Playlist,
                _ => S::Artist,
            };
            let mut chips = Vec::new();
            if let Some(y) = d.year {
                chips.push(y.to_string());
            }
            for (name, value) in &d.facts {
                match name {
                    S::Genre if !value.trim().is_empty() => chips.push(value.trim().to_owned()),
                    S::Genres => chips.extend(value.split(',').map(str::trim).filter(|g| !g.is_empty()).map(str::to_owned)),
                    _ => {}
                }
            }
            Head {
                kind,
                cover: d.cover.clone(),
                title: d.title.clone(),
                artists: if d.is_artist { Vec::new() } else { d.artists.clone() },
                sub: if d.is_artist { String::new() } else { d.subtitle.clone() },
                chips,
                explicit: false,
                link: d.link.clone(),
                round: d.is_artist,
                release: None,
            }
        }
    }
}

fn chips(lang: Lang, mut items: Vec<String>, explicit: bool) -> Option<El> {
    if items.is_empty() && !explicit {
        return None;
    }
    if explicit {
        items.push(S::Explicit.get(lang).to_owned());
    }
    let pill = |text: String| row().h(24.0).px(10.0).radius(12.0).items_center().fill(th::veil(0.07)).child(label(text, th::SMALL, th::text2()));
    Some(row().key("sheet-chips").gap(6.0).flex_wrap().justify_center().mt(12.0).children(items.into_iter().map(pill)))
}

/// Renders the details modal over the page.
pub fn sheet(s: &State, cx: &Cx<State>, w: f32, h: f32) -> El {
    let Some(what) = &s.sheet else { return el() };
    let lang = s.lang;
    let p = s.sheet_pop.t;
    let a = p.clamp(0.0, 1.0);
    let drag = s.sheet_drag.t;
    let cw = MAX_W.min(w - 64.0).max(360.0).round();
    let max_h = MAX_H.min(h - 2.0 * kit::TITLE_H).max(340.0).round();
    let head = head_of(s, what);

    let left_w = (cw * 0.38).clamp(250.0, 320.0).round();
    let inner = left_w - 2.0 * PAD;
    let cover_side = inner.min(max_h - 250.0).max(120.0).round();
    let names_top = PAD + 6.0 + cover_side + 20.0;
    let right_x = left_w + 4.0;
    let right_w = cw - right_x - PAD;
    let right_top = PAD + ROUND + 6.0;
    let left_h = cx.layout_size("sheet-names").map_or(max_h, |[_, nh]| names_top + nh + PAD);
    let right_h = cx.layout_size("sheet-sections").map_or(0.0, |[_, bh]| right_top + bh + PAD);
    let ch = left_h.max(right_h).clamp(360.0, max_h).round();
    let dst = [((w - cw) * 0.5).round(), ((h - ch) * 0.5).round(), cw, ch];
    let pull = (drag.max(0.0) / (ch * 0.7)).min(1.0);

    let cover_key = State::cover_key(head.cover.as_deref());
    let pal = cover_key.as_deref().and_then(|k| s.art.palette(k));
    let deep = pal.map_or(th::rgb(0x000000), |p| th::srgb(p.colors[1]));
    let glow = pal.map_or(th::veil(0.0), |p| th::srgb(p.accent));

    let dim_a = a * (1.0 - 0.75 * pull);
    let mut layer = el().key("sheet").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).instant_changes();
    layer = layer
        .child(fx::snap_slot(el().key("sheet-snap").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none())))
        .child(fx::veil_slot(el().key("sheet-blur").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()), [-dim_a, 0.0, 22.0, 0.0]))
        .child(
            el().key("sheet-dim")
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .transition(Transition::none())
                .fill(th::rgba(0x000000, (if th::light() { 0.18 } else { 0.42 }) * dim_a))
                .radial_gradient([0.5, 0.5], w.max(h) * 0.75, [(0.0, deep.alpha(0.5 * dim_a)), (1.0, deep.alpha(0.0))]),
        );
    if s.sheet_open {
        layer = layer.child(el().key("sheet-scrim").absolute(0.0, 0.0).w_full().h_full().on_press(|s: &mut State| s.sheet_open = false));
    }

    let from = source(s, cx, what);
    let has_from = from.is_some();
    let (src, src_r) = from.unwrap_or(([dst[0] + cw * 0.5 - 70.0, dst[1] + ch * 0.5 - 70.0, 140.0, 140.0], 24.0));
    let lerp = |x: f32, y: f32| x + (y - x) * p;
    let rect = [lerp(src[0], dst[0]), lerp(src[1], dst[1]), lerp(src[2], dst[2]).max(1.0), lerp(src[3], dst[3]).max(1.0)];
    let radius = (src_r + (RADIUS - src_r) * a).min(rect[2].min(rect[3]) * 0.5);

    let cover_r = if head.round { cover_side * 0.5 } else { COVER_R };
    let slot = [dst[0] + (left_w - cover_side) * 0.5, dst[1] + PAD + 6.0, cover_side, cover_side];
    let cov = [lerp(src[0], slot[0]), lerp(src[1], slot[1]), lerp(src[2], slot[2]).max(1.0), lerp(src[3], slot[3]).max(1.0)];
    let cov_r = src_r + (cover_r - src_r) * a;

    let off = [dst[0] - rect[0], dst[1] - rect[1]];
    let left_in = arrival(p, 0.42);

    let mut names = col()
        .key("sheet-names")
        .absolute(PAD, names_top + (1.0 - left_in) * 12.0)
        .w(inner)
        .gap(4.0)
        .items_center()
        .opacity(left_in)
        .transition(Transition::none())
        .child(kit::caps(head.kind.get(lang), th::text3()).mb(2.0))
        .child(label(head.title, th::SHEET_TITLE, th::text()).wrap().max_lines(3).align(Align::Center).w(inner));
    if !head.artists.is_empty() {
        names = names.child(row().max_w(inner).justify_center().child(kit::artists_row_in_width(s, &head.artists, th::BYLINE_M, false, "sheet-art", th::text2(), inner, true)));
    } else if !head.sub.is_empty() {
        names = names.child(label(head.sub, th::BYLINE_M, th::text2()).truncate().max_w(inner));
    }
    names = names.children(chips(lang, head.chips, head.explicit));

    let list = sections::sections(s, what, right_w);
    let n = list.len();
    let mut stack = col().key("sheet-sections").w(right_w).gap(SECTION_GAP).pb(4.0);
    for (i, sec) in list.into_iter().enumerate() {
        let e = arrival(p, 0.5 + 0.06 * i.min(5) as f32);
        stack = stack.child(el().key(format!("sheet-sec-{i}-of-{n}")).w(right_w).opacity(e).offset(0.0, (1.0 - e) * 18.0).transition(Transition::none()).child(sec));
    }
    let scroll = col()
        .key(format!("sheet-scroll-{}", s.sheet_seq))
        .absolute(right_x, right_top)
        .w(right_w + PAD)
        .h(ch - right_top)
        .scroll(rune::ScrollSpec { stick_bottom: false })
        .scroll_edge_fade(22.0)
        .child(col().w(right_w).pt(2.0).pb(PAD).child(stack));

    let buttons_in = arrival(p, 0.55);
    let close = round_btn("sheet-close", &icons::CLOSE, S::Close.get(lang)).absolute(cw - 18.0 - ROUND, 18.0).opacity(buttons_in).on_press(|s: &mut State| s.sheet_open = false);
    let open_yandex = head.link.map(|link| {
        round_btn("sheet-link", &icons::EXTERNAL, S::OpenInYandexMusic.get(lang)).absolute(cw - 18.0 - 2.0 * ROUND - 8.0, 18.0).opacity(buttons_in).on_press_tx(
            move |_s: &mut State, tx: &mut Tx| {
                let _ = tx.open_url(&link);
            },
        )
    });
    let grab = pull_handle(
        el().key("sheet-grab").absolute(cw * 0.5 - 60.0, 0.0).w(120.0).h(20.0).center().opacity(buttons_in).child(el().w(36.0).h(4.0).radius(2.0).fill(th::veil(0.2))),
        ch,
        None,
    );

    let content =
        el().key("sheet-content").absolute(off[0], off[1]).w(cw).h(ch).transition(Transition::none()).child(grab).child(names).child(scroll).children(open_yandex).child(close);

    let face_a = if has_from { smooth(p / 0.3) } else { 1.0 };
    let melt = cover_key.as_deref().and_then(|k| s.art.soft(k)).map(|px| {
        let (strength, wash) = if th::light() { (0.5, 0.64) } else { (0.6, 0.6) };
        el().absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .child(el().absolute(0.0, 0.0).w_full().h_full().opacity(strength).pixels_fit(&px, rune::SurfaceFit::Fill))
            .child(el().absolute(0.0, 0.0).w_full().h_full().linear_gradient(std::f32::consts::PI, [(0.0, th::pop().alpha(wash * 0.72)), (1.0, th::pop().alpha(wash))]))
    });
    let glow_at = [((cov[0] + cov[2] * 0.5 - rect[0]) / rect[2]).clamp(0.0, 1.0), ((cov[1] + cov[3] * 0.5 - rect[1]) / rect[3]).clamp(0.0, 1.0)];
    let face = el().key("sheet-face").absolute(0.0, 0.0).w_full().h_full().fill(th::pop()).opacity(face_a).transition(Transition::none()).children(melt).child(
        el().absolute(0.0, 0.0).w_full().h_full().radial_gradient(
            glow_at,
            rect[2].max(rect[3]) * 0.6,
            [(0.0, glow.alpha(if th::light() { 0.12 } else { 0.2 })), (1.0, glow.alpha(0.0))],
        ),
    );

    let pic = el().key("sheet-cover").absolute(cov[0] - rect[0], cov[1] - rect[1]).w(cov[2]).h(cov[3]).radius(cov_r).transition(Transition::none()).child(kit::cover(
        s,
        head.cover.as_deref(),
        cov[2].min(cov[3]),
        cov_r,
    ));
    let tap: Tap = match head.release {
        Some(page) => Rc::new(move |s: &mut State, _tx: &mut Tx| sections::go_to(s, page.clone())),
        None => Rc::new(|s: &mut State, _tx: &mut Tx| s.sheet_open = false),
    };
    let pic = pull_handle(pic.help(S::GoToAlbum.get(lang)), ch, Some(tap));

    let card_shadow = (if th::light() { 0.18 } else { 0.5 }) * face_a * (1.0 - pull);
    let card = el()
        .key("sheet-panel")
        .role("dialog")
        .label(head.kind.get(lang))
        .absolute(rect[0], rect[1] + drag)
        .w(rect[2])
        .h(rect[3])
        .radius(radius)
        .clip_rounded(radius)
        .scale(1.0 - 0.1 * pull)
        .opacity(if has_from { 1.0 } else { a })
        .transition(Transition::none())
        .motion(if p >= 0.999 && drag == 0.0 { th::SLIDE } else { rune::Spring::NONE })
        .on_press(|_s: &mut State| {})
        .border(1.0, th::veil(0.08 * face_a))
        .shadow(24.0, 64.0, th::rgba(0x000000, card_shadow))
        .child(face)
        .child(content)
        .child(pic);
    let card = if s.sheet_open { card.modal(true, |s: &mut State| s.sheet_open = false) } else { card };
    layer.child(card)
}
