//! Home's My Wave shape garden, mood tiles, skeleton outlines, and butterfly overlay.

use crate::fx::Slot as _;
use rune::{Cursor, El, Transition, Tx, col, el, icon, row};

use super::shared::{name_on, start_wave_with, wave_hue, wave_name_tuned, wave_title};
use crate::app::{Popover, State};
use crate::i18n::S;
use crate::theme as th;
use crate::ui::blobs::{self, Family, Fill, Look, Rng};
use crate::ui::{
    dock, icons,
    kit::{self, Hover as _, label},
    outline,
};

pub(super) const MOODS: [&str; 7] = ["favorite", "discover", "popular", "energetic", "calm", "fun", "sad"];

/// Layout keys that butterflies perch on or steer around.
pub fn bug_spots(s: &State) -> Vec<(String, bool)> {
    let seed = s.home_seed;
    let mut v = vec![("home-title".to_owned(), false), ("home-reroll".to_owned(), false), (format!("hm-wave-shape-{seed}"), true)];
    v.extend(MOODS.iter().map(|id| (format!("hm-mood-{id}-{seed}"), true)));
    v
}

pub(super) fn bug_layer(s: &State) -> El {
    let mut layer = el().key("bug-layer").pin(Some(-24.0), Some(-24.0), Some(-24.0), Some(-24.0)).transition(Transition::none());
    for (i, b) in s.swarm.bugs.iter().enumerate() {
        let side = b.boxed();
        layer = layer.child(
            el().key(format!("bug-{}-{i}", s.swarm.seed))
                .absolute(0.0, 0.0)
                .size(side, side)
                .live_offset(&b.off)
                .live_scale(&b.scale)
                .transition(Transition::none())
                .fx(crate::fx::BUTTERFLY)
                .fx_data([i as f32, b.seed, b.hue, 0.0])
                .on_press_tx(move |s: &mut State, tx: &mut Tx| {
                    s.swarm.scare(i, s.now);
                    tx.wake_after(std::time::Duration::from_millis(16));
                }),
        );
    }
    layer
}

pub(super) fn garden_hover(id: &'static str) -> impl Fn(&mut State, bool, &mut Tx) + 'static {
    move |s: &mut State, on: bool, _tx: &mut Tx| {
        if on {
            s.garden_hover = Some(id);
        } else if s.garden_hover == Some(id) {
            s.garden_hover = None;
        }
    }
}

pub(super) fn garden(s: &State, cw: f32) -> El {
    let lang = s.lang;
    let seed = s.home_seed;
    let mut rng = Rng::new(seed);
    let (big, bx, by, cell, spots, total_h) = garden_layout(cw);
    let hover = if kit::under() { None } else { s.garden_hover };

    let mut g = el().key("garden").w_full().h(total_h).transition(Transition::none());

    let now = dock::wave_playing(s);
    let playing = now && s.player.playing;
    let hue = wave_hue(&s.wave_choice);
    let wave_cover_ready = s.palette_key.is_some() && s.current().and_then(|t| t.cover_template()).is_some_and(|tpl| s.palette_tpl.as_deref() == Some(tpl));
    let fill = {
        let mut wc = s.wave_cover.borrow_mut();
        if !now {
            *wc = (false, None);
        } else if wave_cover_ready && !wc.0 {
            *wc = (true, s.palette_key.clone());
        }
        match &*wc {
            (true, first) if *first == s.palette_key => Fill::CoverNow(hue),
            (true, _) => Fill::Cover(hue),
            _ => Fill::Hue(hue),
        }
    };
    let look = Look { hovered: hover == Some("wave"), ..Look::default() };
    let wave_seed = rng.seed();
    let wave_shape = blobs::wave(format!("hm-wave-shape-{seed}"), wave_seed, fill, look, s.settings.wave_look, big).absolute(0.0, 0.0);
    let tune_open = s.popover_open && matches!(s.popover, Some(Popover::WaveTune { .. }));
    let tune = kit::quiet_toggle("hm-wave-tune", &icons::TUNE, 18.0, 34.0, tune_open, S::WaveTune.get(lang))
        .absolute((big * 0.80).min(big - 36.0), (big * 0.08).max(4.0))
        .on_press(|s: &mut State| s.open_popover(Popover::WaveTune { anchor: "hm-wave-tune".into() }));
    let go = move |s: &mut State| {
        if dock::wave_playing(s) {
            s.toggle_pause();
        } else {
            let id = s.wave_choice.clone();
            start_wave_with(s, &id);
        }
    };
    let mood = wave_name_tuned(s, &s.wave_choice);
    let line = match (now, s.current()) {
        (true, Some(t)) => format!("{} · {mood}\n{} — {}", S::WaveNowPlaying.get(lang), t.full_title(), t.artists_line()),
        _ => format!("{}: {mood}", S::Mood.get(lang)),
    };
    let wave_name = S::WaveTitle.get(lang);
    let room_w = big * 0.66;
    let mut title_size = (big * 0.088).clamp(18.0, 34.0);
    for _ in 0..2 {
        let room = room_w - 22.0 - 4.0 - (title_size * 0.95).round();
        let (tw, _) = rune::text::measure(wave_name, &[], th::name_on(title_size), None);
        if tw > room {
            title_size = (title_size * room / tw).floor().max(15.0);
        }
    }
    let glyph = (title_size * 0.95).round();
    let play_icon = icon(if playing { &icons::PAUSE } else { &icons::PLAY }, glyph).color(th::text()).gop(0.86, 1.0);
    let line_el = row().w_full().justify_center().child(label(line.clone(), th::SMALL, th::text().alpha(0.76)).align(rune::Align::Center).max_lines(2).wrap().max_w(room_w));
    let words = col()
        .absolute((big - room_w) * 0.5, 0.0)
        .w(room_w)
        .h(big)
        .justify_center()
        .items_center()
        .gap(2.0)
        .transition(Transition::none())
        .child(
            row()
                .key("hero-go")
                .role("button")
                .label(if playing { S::Pause.get(lang) } else { S::Play.get(lang) })
                .max_w(room_w)
                .pl(6.0)
                .pr(14.0)
                .py(4.0)
                .gap(4.0)
                .radius(14.0)
                .items_center()
                .cursor(Cursor::Pointer)
                .hov_group()
                .press_scale(0.96)
                .on_press(go)
                .child(play_icon)
                .child(label(wave_name, th::name_on(title_size), th::text())),
        )
        .child(kit::roll("hero-line", &line, line_el));
    g = g.child(
        el().key("hm-wave")
            .absolute(bx, by)
            .w(big)
            .h(big)
            .transition(Transition::none())
            .on_hover_change(garden_hover("wave"))
            .on_pointer_move_any(move |p: rune::PointerEvent, _tx: &mut Tx| {
                let r = blobs::radius(Family::Liquid, big);
                crate::fx::set_wave_pointer((p.x - big * 0.5) / r, (p.y - big * 0.5) / r);
            })
            .children(shape_bone("hm-wave-bone", Family::Liquid, wave_seed, s.settings.wave_look.form, big, 0, s.settings.reduce_motion))
            .child(wave_shape)
            .child(kit::rise(words, 1))
            .child(tune),
    );

    let fg = th::text();
    let calm = ((cell - 104.0) / 76.0).clamp(0.0, 1.0);
    let jit = 7.0 * calm;
    let lo = 1.0 - 0.05 * calm;
    let hi = 1.0 + 0.12 * calm;
    for (i, id) in MOODS.into_iter().enumerate() {
        let size = cell * rng.range(lo, hi);
        let (jx, jy) = (rng.range(-jit, jit), rng.range(-jit, jit));
        let x = spots[i][0] + (cell - size) * 0.5 + jx;
        let y = spots[i][1] + (cell - size) * 0.5 + jy;
        let on = now && s.wave_choice == id;
        let name = wave_title(id).get(lang);
        let look = Look { chosen: on, hovered: hover == Some(id), order: 1 + i as u32 };
        let shape_seed = rng.seed();
        let family = blobs::mood_family(id);
        let tile = el()
            .key(format!("hm-mood-{id}-{seed}"))
            .role("button")
            .label(name)
            .checked(on)
            .absolute(x, y)
            .w(size)
            .h(size)
            .cursor(Cursor::Pointer)
            .press_scale(0.95)
            .transition(Transition::none())
            .on_hover_change(garden_hover(id))
            .on_press(move |s: &mut State| start_wave_with(s, id))
            .children(shape_bone(&format!("hm-mood-bone-{id}"), family, shape_seed, 0, size, i + 1, s.settings.reduce_motion))
            .child(blobs::shape(format!("hm-mood-shape-{id}-{seed}"), family, shape_seed, Fill::Hue(wave_hue(id)), look, size, size).absolute(0.0, 0.0))
            .child(name_on(name, family, size, fg));
        g = g.child(tile);
    }
    g
}

fn shape_bone(key: &str, family: Family, seed: f32, form: u8, side: f32, order: usize, reduce: bool) -> Option<El> {
    if crate::fx::shapes_ready() {
        return None;
    }
    let points = outline::outline(family, form, seed, side);
    let bone = el().key(key.to_owned()).transition(Transition::opacity().springs(th::SOFT));
    let base = th::veil(if th::light() { 0.065 } else { 0.07 });
    let glow = th::veil(if th::light() { 0.085 } else { 0.095 });
    let animate = !reduce && (crate::fx::gpu_up() || crate::app::is_headless());
    let period = 1.65;
    let delay = order as f64 * 0.08;
    if points.len() < 3 {
        let d = 2.0 * blobs::radius(family, side);
        let mut fallback = bone.absolute((side - d) * 0.5, (side - d) * 0.5).w(d).h(d).radius(d * 0.5).fill(base);
        if animate {
            let band = (d * 0.65).max(48.0);
            let travel = d + band * 2.2;
            let (x0, x1) = (-band, travel - band);
            let spec = rune::LoopSpec { delay, ..rune::LoopSpec::slide(period, [x0 / band, 0.0], [x1 / band, 0.0]).easing(rune::Easing::LINEAR) };
            let sheen = el().key(format!("{key}-shimmer")).absolute(0.0, 0.0).w(band).h_full().clip_fade(band * 0.5, 0.0, band * 0.5, 0.0).fill(glow).looping(spec);
            fallback = fallback.clip_rounded(d * 0.5).child(sheen);
        }
        return Some(fallback);
    }
    let mut out = bone.absolute(0.0, 0.0).w(side).h(side).shape(rune::Shape::new().fill(points.clone(), base));
    if animate {
        let band = (side * 0.65).max(48.0);
        let travel = side + band * 2.2;
        let (x0, x1) = (-band, travel - band);
        let spec = |a: f32, b: f32| rune::LoopSpec { delay, ..rune::LoopSpec::slide(period, [a, 0.0], [b, 0.0]).easing(rune::Easing::LINEAR) };
        let hi_shape = el().absolute(0.0, 0.0).w(side).h(side).shape(rune::Shape::new().fill(points, glow));
        let lit = el().absolute(0.0, 0.0).w(band).h_full().looping(spec(-x0 / band, -x1 / band)).child(hi_shape);
        let sheen =
            el().key(format!("{key}-shimmer")).absolute(0.0, 0.0).w(band).h_full().clip_fade(band * 0.5, 0.0, band * 0.5, 0.0).looping(spec(x0 / band, x1 / band)).child(lit);
        out = out.child(sheen);
    }
    Some(out)
}

/// Computes `(big_size, big_x, big_y, cell_size, mood_positions, total_height)` cleanly across
/// wide desktop windows and compact small windows alike.
fn garden_layout(cw: f32) -> (f32, f32, f32, f32, [[f32; 2]; 7], f32) {
    const N: usize = MOODS.len();
    const COLS: usize = 4;
    let mut spots = [[0.0f32; 2]; N];
    if cw >= 640.0 {
        let t = ((cw - 640.0) / 360.0).clamp(0.0, 1.0);
        let gap = 10.0 + 8.0 * t;
        let sep = 14.0 + 18.0 * t;
        let min_cell = 104.0 + 36.0 * t;
        let beside_min = COLS as f32 * min_cell + (COLS as f32 - 1.0) * gap;
        let big = (cw * (0.35 + 0.05 * t)).clamp(216.0, 410.0).min(cw - sep - beside_min).max(204.0);
        let mw = (cw - big - sep).max(beside_min);
        let cell = ((mw - gap * (COLS as f32 - 1.0)) / COLS as f32).clamp(96.0, 186.0);
        let rows = N.div_ceil(COLS);
        let grid_w = COLS as f32 * cell + (COLS as f32 - 1.0) * gap;
        let grid_h = rows as f32 * cell + (rows as f32 - 1.0) * gap;
        let pair_w = big + sep + grid_w;
        let x0 = ((cw - pair_w) * 0.5).max(0.0);
        let total_h = big.max(grid_h);
        let by = (total_h - big) * 0.5;
        let gx0 = x0 + big + sep;
        let my = (total_h - grid_h) * 0.5;
        for (i, spot) in spots.iter_mut().enumerate() {
            let (r, c) = (i / COLS, i % COLS);
            let in_row = (N - r * COLS).min(COLS);
            let shift = (COLS - in_row) as f32 * (cell + gap) * 0.5;
            *spot = [gx0 + shift + c as f32 * (cell + gap), my + r as f32 * (cell + gap)];
        }
        return (big, x0, by, cell, spots, total_h);
    }
    // Ultra-narrow window: center My Wave cleanly above a balanced 4 + 3 mood grid.
    let gap = 10.0;
    let big = (cw * 0.52).clamp(200.0, 252.0);
    let cell = ((cw - gap * (COLS as f32 - 1.0)) / COLS as f32).clamp(84.0, 132.0);
    let rows = N.div_ceil(COLS);
    let grid_w = COLS as f32 * cell + (COLS as f32 - 1.0) * gap;
    let grid_h = rows as f32 * cell + (rows as f32 - 1.0) * gap;
    let gx0 = ((cw - grid_w) * 0.5).max(0.0);
    let my = big + 14.0;
    for (i, spot) in spots.iter_mut().enumerate() {
        let (r, c) = (i / COLS, i % COLS);
        let in_row = (N - r * COLS).min(COLS);
        let shift = (COLS - in_row) as f32 * (cell + gap) * 0.5;
        *spot = [gx0 + shift + c as f32 * (cell + gap), my + r as f32 * (cell + gap)];
    }
    (big, (cw - big) * 0.5, 0.0, cell, spots, my + grid_h)
}
