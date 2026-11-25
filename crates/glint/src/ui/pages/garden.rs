//! Home's My Wave layout: a fluid procedural wave backdrop behind a centered, responsive
//! two-column composition holding the hero player stage on the left and Yandex Music's
//! infinitely looping personalized wave wheel on the right.

use crate::fx::Slot as _;
use rune::{Align, Cursor, Drag, El, Phase, TextStyle, Transition, Tx, col, el, icon, row};

use super::shared::{start_wave_with, wave_hue, wave_title};
use crate::app::{FromName, Page, PlayingFrom, State};
use crate::i18n::S;
use crate::theme as th;
use crate::ui::blobs::{self, Fill, Look, Rng};
use crate::ui::{
    dock, icons,
    kit::{self, Hover as _, label},
    np_icons as ni,
};

/// Converts an `[r, g, b]` tint into a hue angle in `0..360` for the fluid wave backdrop.
fn rgb_hue([r, g, b]: [u8; 3]) -> f32 {
    let (rf, gf, bf) = (f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0);
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let d = max - min;
    if d < 1e-4 {
        return 222.0;
    }
    let h = if (max - rf).abs() < 1e-5 {
        ((gf - bf) / d).rem_euclid(6.0)
    } else if (max - gf).abs() < 1e-5 {
        (bf - rf) / d + 2.0
    } else {
        (rf - gf) / d + 4.0
    };
    (h * 60.0).rem_euclid(360.0)
}

/// Layout keys that butterflies perch on or steer around when enabled in Settings.
pub fn bug_spots(_s: &State) -> Vec<(String, bool)> {
    vec![
        ("hm-cover".to_owned(), false),
        ("hm-source".to_owned(), false),
        ("hm-control-bar".to_owned(), false),
        ("hm-track-title".to_owned(), false),
    ]
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

/// Rounds `v` to the nearest even integer within `[lo, hi]` so icon glyphs stay pixel-centered.
fn even(v: f32, lo: f32, hi: f32) -> f32 {
    ((v * 0.5).round() * 2.0).clamp(lo, hi)
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn recede_el(mut e: El, stay: f32, dy: f32) -> El {
    let stay = stay.clamp(0.0, 1.0);
    if stay < 0.999 {
        e = e.offset(0.0, (1.0 - stay) * dy).scale(0.97 + 0.03 * stay).opacity(stay);
    }
    e
}

/// Builds the sleek frosted top eyebrow pill on Home: golden My Wave badge when playing a My Wave
/// preset or idle, or a clickable source pill (Album, Single, Playlist, Artist, Liked, History)
/// when playing outside My Wave. Always housed in a subtle translucent pill so it stays crisp even
/// over bright yellow/light covers.
fn stage_eyebrow(s: &State, cur: Option<&ym_api::Track>, wave_on: bool, stage_w: f32, u: f32, colors: (rune::Color, rune::Color, rune::Color)) -> (El, f32) {
    let lang = s.lang;
    let (chip_bg, chip_border, cell_hov) = colors;
    let eyebrow_col = if th::light() { th::rgb(0x9e6300) } else { th::rgb(0xffdc4e) };
    let pill_h = even(28.0 * u, 24.0, 30.0);
    let pill_px = (16.0 * u).clamp(13.0, 18.0).round();
    let font_sz = (12.5 * u).clamp(11.0, 13.5);
    let glyph_sz = even(13.5 * u, 12.0, 14.0);
    let max_pill = (stage_w - 32.0).clamp(160.0, 300.0);

    let wave_pill = |text: String| {
        row()
            .key("hm-source")
            .h(pill_h)
            .max_w(max_pill)
            .px(pill_px)
            .gap(7.0)
            .radius(pill_h * 0.5)
            .items_center()
            .fill(chip_bg)
            .border(1.0, chip_border)
            .animated_changes()
            .motion(th::SLIDE)
            .child(kit::wave_glyph(glyph_sz + 1.0, eyebrow_col))
            .child(label(text, TextStyle::new(font_sz, 600.0).family(th::SANS).tracking(0.1).lh(pill_h - 8.0), eyebrow_col).truncate().min_w(0.0))
    };

    let Some(track) = cur else {
        return (wave_pill(S::WaveTitle.get(lang).to_owned()), pill_h);
    };
    if wave_on && s.playing_from.as_ref().is_none_or(|f| f.kind == S::WaveTitle) {
        let title = if s.wave_choice == "onyourwave" {
            S::WaveTitle.get(lang).to_owned()
        } else if let Some(w) = s.wheel_wave(&s.wave_choice) {
            format!("{} · {}", S::WaveShort.get(lang), w.name)
        } else {
            format!("{} · {}", S::WaveShort.get(lang), wave_title(&s.wave_choice).get(lang))
        };
        return (wave_pill(title), pill_h);
    }
    let fallback = || match track.albums.first() {
        Some(al) => {
            let mut kind = crate::app::album_kind_name(al);
            if kind == S::Album && al.track_count.is_none() && al.title.trim().eq_ignore_ascii_case(track.title.trim()) {
                kind = S::Single;
            }
            PlayingFrom { wave: false, kind, name: FromName::Text(al.title.clone()), page: Some(Page::Album(al.id.clone())) }
        }
        None => PlayingFrom { wave: false, kind: S::Track, name: FromName::Word(S::NowPlaying), page: None },
    };
    let from = s.playing_from.clone().unwrap_or_else(fallback);
    let text = from.line_for_track(lang, Some(track));
    let ink = if wave_on { eyebrow_col } else { th::text().alpha(0.90) };
    let glyph = kit::source_glyph(&from, if wave_on { eyebrow_col } else { th::text().alpha(0.80) }, glyph_sz);
    let page = from.page;
    let mut pill = row()
        .key("hm-source")
        .role(if page.is_some() { "link" } else { "note" })
        .label(text.as_str())
        .h(pill_h)
        .max_w(max_pill)
        .px(pill_px)
        .gap(7.0)
        .radius(pill_h * 0.5)
        .items_center()
        .fill(chip_bg)
        .border(1.0, chip_border)
        .animated_changes()
        .motion(th::SLIDE)
        .child(glyph)
        .child(label(text, TextStyle::new(font_sz, 580.0).family(th::SANS).lh(pill_h - 8.0), ink).truncate().min_w(0.0));
    if let Some(p) = page {
        pill = pill.cursor(Cursor::Pointer).hov(cell_hov).press_scale(0.97).on_press(move |s: &mut State| s.go(p.clone()));
    }
    (pill, pill_h)
}

pub(super) fn garden(s: &State, cw: f32, h: f32) -> El {
    let lang = s.lang;
    let light = th::light();
    let seed = s.home_seed;
    let mut rng = Rng::new(seed);
    let hover = if kit::under() { None } else { s.garden_hover };
    let page_w = cw.max(320.0);
    let stage_h = h.max(420.0);

    let wave_on = dock::wave_playing(s);
    let cur = s.current();
    let has_track = cur.is_some();
    let playing = has_track && s.player.playing;
    let selected_wheel = s.wheel_wave(&s.wave_choice);
    let mood_name = selected_wheel.map(|w| w.name.clone()).unwrap_or_else(|| wave_title(&s.wave_choice).get(lang).to_owned());

    let hue = if !crate::fx::wave_ready() && has_track && s.palette.wave[1] > 0.12 {
        s.palette.wave[0]
    } else if let Some(rgb) = selected_wheel.and_then(|w| w.color) {
        rgb_hue(rgb)
    } else {
        wave_hue(&s.wave_choice)
    };
    let wave_cover_ready = s.palette_key.is_some() && cur.and_then(|t| t.cover_template()).is_some_and(|tpl| s.palette_tpl.as_deref() == Some(tpl));
    let fill = {
        let mut wc = s.wave_cover.borrow_mut();
        if !has_track {
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
    let wave_seed = rng.seed();
    let look = Look { chosen: has_track, hovered: hover == Some("wave"), order: 0 };

    let go = move |s: &mut State| {
        if s.current().is_some() {
            s.toggle_pause();
        } else {
            let id = s.wave_choice.clone();
            start_wave_with(s, &id);
        }
    };

    // Always side-by-side (left hero player stage + right Cormorant arc wheel), centered cleanly
    // inside a generous max band on ultrawide displays and proportionally scaled down on compact windows.
    let content_w = page_w.min(1560.0);
    let content_x = ((page_w - content_w) * 0.5).max(0.0).round();
    let stage_w = (content_w * 0.62).clamp(280.0, 960.0).round();
    let arc_w = (content_w - stage_w).max(180.0);

    // Continuous proportional scale factors for left stage (`u`) and right arc (`arc_u`).
    let u = (stage_w / 720.0).min(stage_h / 760.0).clamp(0.58, 1.16);
    let arc_u = (arc_w / 420.0).min(stage_h / 760.0).clamp(0.60, 1.15);

    // Refined frosted surface tokens.
    let bar_bg = if light { th::rgb(0xffffff).alpha(0.84) } else { th::rgb(0x0c0e15).alpha(0.50) };
    let chip_bg = if light { th::rgb(0xffffff).alpha(0.74) } else { th::rgb(0x0c0e15).alpha(0.38) };
    let bar_border = if light { th::rgb(0x000000).alpha(0.08) } else { th::rgb(0xffffff).alpha(0.14) };
    let chip_border = if light { th::rgb(0x000000).alpha(0.07) } else { th::rgb(0xffffff).alpha(0.12) };
    let cell_hov = if light { th::rgb(0x000000).alpha(0.065) } else { th::rgb(0xffffff).alpha(0.12) };
    let div_col = if light { th::rgb(0x000000).alpha(0.08) } else { th::rgb(0xffffff).alpha(0.12) };

    let np_t = s.np.t.clamp(0.0, 1.0);
    let recede = smooth(np_t / 0.52);
    let stay = 1.0 - recede;

    let (eyebrow, eyebrow_h) = stage_eyebrow(s, cur, wave_on, stage_w, u, (chip_bg, chip_border, cell_hov));
    let eyebrow = recede_el(eyebrow, stay, -12.0);

    // Dynamic even-pixel control sizing with a compact play/pause circle and a slim seek bar
    // resting directly on the top edge of the control pill.
    let btn_s = even(34.0 * u, 28.0, 36.0);
    let play_s = even(32.0 * u, 28.0, 34.0);
    let play_ic = even(15.0 * u, 13.0, 16.0);
    let ic_sm = even(17.0 * u, 14.0, 18.0);
    let ic_md = even(18.0 * u, 15.0, 19.0);
    let bar_h = even(48.0 * u, 42.0, 50.0);
    let bar_px = (9.0 * u).clamp(6.0, 11.0).round();
    let div_mx = (5.0 * u).clamp(3.0, 7.0).round();
    let play_mx = (4.0 * u).clamp(2.0, 5.0).round();
    let bar_w = (2.0 * bar_px + 6.0 * btn_s + play_s + 2.0 * play_mx + 2.0 * (1.0 + 2.0 * div_mx) + 8.0 * 2.0).round();
    let seek_inset = (bar_h * 0.5).round();
    let seek_w = (bar_w - 2.0 * seek_inset).max(120.0).round();

    let cover_side = even(196.0 * u, 112.0, 224.0);
    let cover_r = (cover_side * 0.115).clamp(14.0, 24.0).round();
    let cover_mt = (16.0 * u).clamp(10.0, 20.0).round();
    let title_sz = (22.0 * u).clamp(15.5, 26.0).round();
    let title_lh = title_sz + 5.0;
    let title_mt = (18.0 * u).clamp(10.0, 22.0).round();
    let art_sz = (14.5 * u).clamp(12.0, 16.0);
    let art_lh = (art_sz + 5.0).round();
    let art_mt = (3.0 * u).clamp(2.0, 5.0).round();
    let bar_mt = (22.0 * u).clamp(14.0, 26.0).round();

    // Lock the wave ring dead center onto the album cover (left half of the centered band).
    let stage_cx = content_x + stage_w * 0.5;
    let cover_cy = if has_track {
        let above = eyebrow_h + cover_mt;
        let below = title_mt + title_lh + art_mt + art_lh + bar_mt + bar_h;
        stage_h * 0.5 + (above - below) * 0.5
    } else {
        stage_h * 0.5
    };
    let fx = (stage_cx / page_w).clamp(0.20, 0.65);
    let fy = (cover_cy / stage_h).clamp(0.28, 0.68);
    let min_w_frac = (stage_w / page_w).clamp(0.42, 0.78);
    crate::fx::set_wave_frame(fx, fy, min_w_frac, 1.02);

    let aura = blobs::wave_aura(format!("hm-wave-shape-{seed}"), wave_seed, fill, look, s.settings.wave_look, page_w, stage_h, 0.0).absolute(0.0, 0.0);

    let bar_icon = |key: &'static str, ic: &'static rune::Icon, glyph: f32, col: rune::Color, tip: &str| {
        row()
            .key(key)
            .role("button")
            .label(tip)
            .help(tip)
            .w(btn_s)
            .h(btn_s)
            .radius(btn_s * 0.5)
            .center()
            .cursor(Cursor::Pointer)
            .hov(cell_hov)
            .press_scale(th::PRESS)
            .child(icon(ic, glyph).color(col))
    };

    let vol = s.player.volume;
    let vol_ic = if vol < 0.01 {
        &ni::VOLUME_MUTE
    } else if vol < 0.5 {
        &ni::VOLUME_LOW
    } else {
        &ni::VOLUME
    };
    let vol_open = s.vol_open;
    let vol_btn = row()
        .key("hm-vol")
        .role("button")
        .label(S::Volume.get(lang))
        .help(S::Volume.get(lang))
        .checked(vol_open)
        .w(btn_s)
        .h(btn_s)
        .radius(btn_s * 0.5)
        .center()
        .fill(if vol_open { cell_hov } else { rune::Color::TRANSPARENT })
        .cursor(Cursor::Pointer)
        .hov(cell_hov)
        .press_scale(th::PRESS)
        .on_wheel(dock::volume_wheel)
        .on_hover_change(|s: &mut State, over: bool, _tx: &mut Tx| {
            s.vol_button_hover = over;
            if over {
                s.vol_open = true;
                s.vol_close_at = None;
            } else {
                s.vol_close_at = Some(s.now + 0.28);
            }
        })
        .on_press(State::toggle_mute)
        .child(icon(vol_ic, ic_sm).color(th::text().alpha(0.86)));

    let prev_btn = if s.can_prev() {
        bar_icon("hm-prev", &ni::PREV, ic_md, th::text().alpha(0.92), S::Previous.get(lang)).on_press(|s: &mut State| s.prev())
    } else {
        row().key("hm-prev").role("button").label(S::Previous.get(lang)).disabled(true).w(btn_s).h(btn_s).center().child(icon(&ni::PREV, ic_md).color(th::veil(0.22)))
    };
    let next_btn = if s.can_next() {
        bar_icon("hm-next", &ni::NEXT, ic_md, th::text().alpha(0.92), S::Next.get(lang)).on_press(|s: &mut State| s.next())
    } else {
        row().key("hm-next").role("button").label(S::Next.get(lang)).disabled(true).w(btn_s).h(btn_s).center().child(icon(&ni::NEXT, ic_md).color(th::veil(0.22)))
    };

    let play_bg = if light { th::rgb(0x141418).alpha(0.92) } else { th::rgb(0xffffff).alpha(0.95) };
    let play_fg = if light { th::rgb(0xffffff) } else { th::rgb(0x101218) };
    let (play_glyph_raw, play_state) = if has_track && s.player.loading {
        (kit::spin(icon(&icons::SPINNER, play_ic).color(play_fg)), "load")
    } else if playing {
        (icon(&ni::PAUSE, play_ic).color(play_fg), "pause")
    } else {
        (icon(&ni::PLAY, play_ic).color(play_fg), "play")
    };
    let play_glyph = if has_track {
        kit::swap_glyph(format!("hm-play-{}-{play_state}", play_s as u32), play_glyph_raw, play_s, play_ic, true)
    } else {
        play_glyph_raw
    };

    let divider = || el().w(1.0).h((14.0 * u).clamp(10.0, 16.0).round()).mx(div_mx).fill(div_col);

    // ---- left hero player stage ----------------------------------------------------------------
    let stage_enter = if crate::fx::cpu_mode() { Transition::none() } else { kit::pop_in(0.985, 12.0, th::PAGE_IN) };
    let mut stage_content = col()
        .key("hm-stage-body")
        .w_full()
        .h_full()
        .items_center()
        .justify_center()
        .transition(stage_enter)
        .child(eyebrow);

    if let Some(t) = cur {
        let cover = if s.np_open {
            el().key("hm-cover-gap").w(cover_side).h(cover_side)
        } else {
            let cov = el()
                .key("hm-cover")
                .role("button")
                .label(S::NowPlaying.get(lang))
                .w(cover_side)
                .h(cover_side)
                .radius(cover_r)
                .border(1.0, th::veil(if light { 0.12 } else { 0.22 }))
                .cursor(Cursor::Pointer)
                .press_scale(0.97)
                .transition(Transition::none())
                .on_press(|s: &mut State| s.set_np(true))
                .child(kit::playing_cover(s, t, cover_side, cover_r, "hm-cover-mark"));
            if crate::fx::cpu_mode() { cov } else { cov.matched("np-cover").animated_changes().motion(th::FLIGHT) }
        };

        let text_max_w = (stage_w - (48.0 * u).clamp(24.0, 64.0)).clamp(200.0, 460.0);
        let title_style = TextStyle::new(title_sz, 650.0).family(th::SANS).tracking(-0.35).lh(title_lh);
        let track_title = row()
            .key("hm-track-title")
            .role("button")
            .label(t.full_title())
            .max_w(text_max_w)
            .px(8.0)
            .py(1.0)
            .radius(10.0)
            .center()
            .cursor(Cursor::Pointer)
            .hov(th::veil(if light { 0.055 } else { 0.085 }))
            .press_scale(0.985)
            .transition(Transition::none())
            .on_press(|s: &mut State| s.set_np(true))
            .child(label(t.full_title(), title_style, th::text()).truncate().align(Align::Center).max_w(text_max_w - 16.0));
        let track_title = recede_el(row().key("hm-title-slot").w(text_max_w).justify_center().child(kit::roll("hm-title", &t.full_title(), row().w(text_max_w).justify_center().child(track_title))), stay, 14.0);

        let art_style = TextStyle::new(art_sz, 450.0).family(th::SANS).lh(art_lh);
        let artists_line = if t.artists.is_empty() {
            el().h(art_lh)
        } else {
            let raw_w = kit::measure_w(&t.artists_line(), art_style).ceil() + 4.0;
            let art_w = raw_w.min(text_max_w);
            let inner = row()
                .key(format!("hm-artists-{}", t.id))
                .w(text_max_w)
                .h(art_lh)
                .justify_center()
                .items_center()
                .transition(Transition::none())
                .child(kit::artists_row_in_width(s, &t.artists, art_style, false, "hm-art", th::text().alpha(0.76), art_w, true));
            recede_el(row().key("hm-art-slot").w(text_max_w).h(art_lh).justify_center().child(kit::roll("hm-art", &t.artists_line(), inner)), stay, 16.0)
        };

        let play_pill = row()
            .key("hero-go")
            .role("button")
            .label(if playing { S::Pause.get(lang) } else { S::Play.get(lang) })
            .w(play_s)
            .h(play_s)
            .radius(play_s * 0.5)
            .center()
            .fill(play_bg)
            .cursor(Cursor::Pointer)
            .press_scale(0.93)
            .on_press(go)
            .child(play_glyph);

        let bad = t.clone();
        let like_t = t.clone();
        let liked = s.is_liked(&t.id);
        let dislike_b = bar_icon("hm-dislike", &ni::DISLIKE, ic_sm, th::text().alpha(0.86), S::Dislike.get(lang)).on_press(move |s: &mut State| s.dislike(&bad));
        let like_glyph = kit::swap_glyph(
            format!("hm-like-{}-{liked}", t.id),
            icon(if liked { &ni::HEART_FILL } else { &ni::HEART }, ic_sm).color(if liked { th::rgb(0xff4f70) } else { th::text().alpha(0.86) }),
            btn_s,
            ic_sm,
            s.burst_on && s.burst_track == t.id,
        );
        let mut like_b = row()
            .key("hm-like")
            .role("button")
            .label(if liked { S::Unlike.get(lang) } else { S::Like.get(lang) })
            .help(if liked { S::Unlike.get(lang) } else { S::Like.get(lang) })
            .checked(liked)
            .w(btn_s)
            .h(btn_s)
            .radius(btn_s * 0.5)
            .center()
            .cursor(Cursor::Pointer)
            .hov(cell_hov)
            .press_scale(th::PRESS)
            .on_press(move |s: &mut State| {
                let liking = !s.is_liked(&like_t.id);
                s.toggle_like(&like_t);
                if liking {
                    s.like_burst = s.now;
                    s.burst_track = like_t.id.clone();
                    s.burst_on = true;
                }
            })
            .child(like_glyph);
        if s.burst_on && s.burst_track == t.id {
            like_b = like_b.child(
                el().key("hm-burst")
                    .fx(crate::fx::SPARKLES)
                    .fx_data([s.like_burst as f32, 14.0, 6.0, 0.0])
                    .fx_bleed(40.0)
                    .absolute(0.0, 0.0)
                    .w_full()
                    .h_full()
                    .radius(btn_s * 0.5),
            );
        }
        let more_b = bar_icon("hm-more", &ni::MORE, ic_sm, th::text().alpha(0.86), S::More.get(lang))
            .on_press(|s: &mut State| s.open_player_menu(rune::PopupAnchor::Key("hm-more".into())));

        let frac = if s.player.dur_ms > 0 { (s.player.pos_ms as f32 / s.player.dur_ms as f32).clamp(0.0, 1.0) } else { 0.0 };
        let (fill_live, thumb_live) = s.update_home_seek_live(seek_w, frac);
        let thumb_rest = if s.scrubbing { 1.0 } else { 0.0 };
        let seek_bar = el()
            .key("hm-seek")
            .role("slider")
            .absolute(seek_inset, -4.5)
            .w(seek_w)
            .h(12.0)
            .cursor(Cursor::Pointer)
            .transition(Transition::none())
            .hov_group()
            .on_drag(|s: &mut State, d: &mut Drag, _tx: &mut Tx| {
                let f = if d.w > 0.0 { (d.x / d.w).clamp(0.0, 1.0) } else { 0.0 };
                let up = d.phase == Phase::Up;
                s.scrubbing = !up;
                s.player.pos_ms = (f as f64 * s.player.dur_ms as f64) as u64;
                s.sync_lives();
                if up {
                    s.seek_frac(f);
                }
            })
            .child(
                el().key("hm-seek-track")
                    .absolute(0.0, 4.0)
                    .w(seek_w)
                    .h(2.5)
                    .radius(1.25)
                    .fill(th::veil(if light { 0.14 } else { 0.22 }))
                    .hov(th::veil(if light { 0.24 } else { 0.34 }))
                    .transition(Transition::none())
                    .child(el().key("hm-seek-fill").radius(1.25).fill(th::text().alpha(0.92)).live_rect(fill_live).transition(Transition::none())),
            )
            .child(el().key("hm-seek-thumb").radius(4.0).fill(th::text()).live_rect(thumb_live).gop(thumb_rest, 1.0).transition(Transition::none()));

        let control_pill = row()
            .key("hm-control-bar")
            .w(bar_w)
            .h(bar_h)
            .px(bar_px)
            .gap(2.0)
            .radius(bar_h * 0.5)
            .items_center()
            .justify_center()
            .fill(bar_bg)
            .border(1.0, bar_border)
            .child(vol_btn)
            .child(dislike_b)
            .child(divider())
            .child(prev_btn)
            .child(play_pill.mx(play_mx))
            .child(next_btn)
            .child(divider())
            .child(like_b)
            .child(more_b);

        let controls_wrap = recede_el(
            el().key("hm-controls-wrap").w(bar_w).h(bar_h).transition(Transition::none()).child(control_pill).child(seek_bar),
            stay,
            20.0,
        );

        stage_content = stage_content
            .child(cover.mt(cover_mt))
            .child(track_title.mt(title_mt))
            .child(artists_line.mt(art_mt))
            .child(controls_wrap.mt(bar_mt));
    } else {
        let idle_ic = even(72.0 * u, 48.0, 84.0);
        let idle_art = if let Some(w) = selected_wheel
            && w.cover.is_some()
        {
            let r = if w.round { idle_ic * 0.5 } else { 10.0 };
            kit::wheel_art(s, w.cover.as_deref(), idle_ic, r).mt((18.0 * u).round())
        } else {
            let tint = if light { th::rgb(0xc07a00) } else { th::rgb(0xffdc4e) };
            row().w(idle_ic).h(idle_ic).mt((18.0 * u).round()).center().child(kit::wave_glyph((idle_ic * 0.62).round(), tint))
        };
        let idle_sz = (34.0 * u).clamp(24.0, 42.0).round();
        let idle_style = TextStyle::new(idle_sz, 700.0).family(th::SANS).tracking(-0.4).lh(idle_sz + 8.0);
        let listen_pill = row()
            .key("hero-go")
            .role("button")
            .label(S::Listen.get(lang))
            .h(play_s)
            .px((20.0 * u).clamp(14.0, 24.0).round())
            .gap(8.0)
            .radius(play_s * 0.5)
            .center()
            .fill(play_bg)
            .cursor(Cursor::Pointer)
            .press_scale(0.95)
            .on_press(go)
            .child(play_glyph)
            .child(label(S::Listen.get(lang), TextStyle::new((14.5 * u).clamp(12.5, 16.0), 650.0).family(th::SANS).lh(20.0), play_fg));

        let control_bar = row()
            .key("hm-control-bar")
            .h(bar_h)
            .px(bar_px)
            .gap(2.0)
            .radius(bar_h * 0.5)
            .items_center()
            .justify_center()
            .fill(bar_bg)
            .border(1.0, bar_border)
            .child(vol_btn)
            .child(divider())
            .child(prev_btn)
            .child(listen_pill.mx(play_mx))
            .child(next_btn);

        stage_content = stage_content
            .child(idle_art)
            .child(label(mood_name, idle_style, th::text()).align(Align::Center).max_lines(2).max_w((stage_w - 40.0).max(200.0)).mt((8.0 * u).round()))
            .child(control_bar.mt((28.0 * u).clamp(18.0, 34.0).round()));
    }

    let stage = el().key("hm-stage").w(stage_w).h(stage_h).transition(Transition::none()).on_hover_change(garden_hover("wave")).child(stage_content);

    // ---- right infinitely-looping Yandex Music wave wheel --------------------------------------
    let ic_box = even(72.0 * arc_u, 48.0, 78.0);
    let pitch = (stage_h / 7.2).clamp(80.0, 102.0).round();
    let item_h = (ic_box + 8.0).max(60.0);
    let center_y = stage_h * 0.5;
    let arc_pos = s.wave_arc;
    let vis_span = (stage_h / (2.0 * pitch) + 1.35).max(3.5);
    let fade_edge = (stage_h * 0.14).clamp(56.0, 116.0).round();

    let arc_enter = if crate::fx::cpu_mode() {
        Transition::none()
    } else {
        Transition::asymmetric(Transition::offset(18.0, 0.0), Transition::none()).springs(th::PAGE_IN)
    };
    let mut arc = el()
        .key("hm-arc")
        .w(arc_w)
        .h(stage_h)
        .offset(26.0 * recede, 0.0)
        .clip_fade(0.0, fade_edge, 0.0, fade_edge)
        .transition(arc_enter)
        .on_wheel(move |s: &mut State, e: rune::WheelEvent, tx: &mut Tx| {
            if e.mods.ctrl || e.mods.cmd || e.dy == 0.0 || e.dx.abs() > e.dy.abs() {
                return false;
            }
            let delta = if e.precise { -e.dy / 64.0 } else { -(e.dy / 40.0).clamp(-1.5, 1.5) * 0.9 };
            s.wave_arc_to += delta;
            if s.settings.reduce_motion || crate::fx::cpu_mode() {
                s.wave_arc = s.wave_arc_to;
                s.wave_arc_vel = 0.0;
            } else {
                tx.wake_after(std::time::Duration::from_millis(16));
            }
            true
        });

    let pad_l = (arc_w * 0.06).clamp(8.0, 36.0).round();
    let curve = (32.0 * arc_u).clamp(14.0, 40.0);
    let item_gap = (16.0 * arc_u).clamp(11.0, 20.0).round();
    let accent_yellow = if light { th::rgb(0xc07a00) } else { th::rgb(0xffdc4e) };
    let total_waves = s.wheel_waves.len();
    if total_waves > 0 {
        let n = total_waves as isize;
        let base_slot = arc_pos.round() as isize;
        let half = vis_span.ceil() as isize;
        for offset in -half..=half {
            let slot = base_slot + offset;
            let rel = slot as f32 - arc_pos;
            if rel.abs() > vis_span {
                continue;
            }
            let idx = slot.rem_euclid(n) as usize;
            let w = &s.wheel_waves[idx];
            let norm = (rel / 3.4).clamp(-1.35, 1.35);
            // Gentle rightward bow cradling the left wave's circular silhouette.
            let x = (pad_l + curve * (1.0 - (norm * norm).min(1.0))).round();
            let y = (center_y + rel * pitch - item_h * 0.5).round();
            let focus = (1.0 - (rel.abs() / 3.4).min(1.0)).powf(1.15);
            let edge_op = (1.0 - (rel.abs() / vis_span).powf(2.4)).clamp(0.0, 1.0);
            let ic_side = ((ic_box - 6.0) + 6.0 * focus).round().max(42.0);
            let fs = (17.0 * arc_u).clamp(13.5, 19.0).round();
            let label_style = TextStyle::new(fs, 680.0).family(th::SANS).tracking(-0.2).lh(fs + 4.0);
            let row_w = (arc_w - x - 10.0).max(120.0);

            let key_id = w.key();
            let chosen = s.wave_choice == key_id && (wave_on || !has_track);
            let is_shake = w.control_accent;
            let icon_el = if w.cover.is_some() {
                let r = if w.round { ic_side * 0.5 } else { 10.0 };
                kit::wheel_art(s, w.cover.as_deref(), ic_side, r)
            } else {
                let tint = if is_shake {
                    accent_yellow
                } else {
                    match w.color {
                        Some([r, g, b]) => th::srgb([f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0]),
                        None => accent_yellow,
                    }
                };
                row().w(ic_side).h(ic_side).center().child(kit::wave_glyph((ic_side * 0.62).round(), tint))
            };

            let label_col = if is_shake {
                accent_yellow
            } else if chosen {
                th::text()
            } else {
                th::text().alpha(0.84 + 0.16 * focus)
            };
            let text_el = if !w.description.is_empty() {
                let desc_sz = (13.0 * arc_u).clamp(10.5, 14.0);
                let desc_style = TextStyle::new(desc_sz, 520.0).family(th::SANS).lh(desc_sz + 3.0);
                let desc_col = th::text().alpha(0.50);
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(2.0)
                    .justify_center()
                    .child(label(w.description.clone(), desc_style, desc_col).truncate().min_w(0.0))
                    .child(label(w.name.clone(), label_style, label_col).truncate().min_w(0.0))
            } else {
                el().grow(1.0).min_w(0.0).child(label(w.name.clone(), label_style, label_col).truncate().min_w(0.0))
            };

            let item_op = (if chosen { edge_op.max(0.65) } else { edge_op }) * stay;
            let click_id = key_id;
            let item = row()
                .key(format!("hm-arc-slot-{slot}"))
                .role("button")
                .label(w.name.as_str())
                .checked(chosen)
                .absolute(x, y)
                .w(row_w)
                .h(item_h)
                .px(4.0)
                .gap(item_gap)
                .items_center()
                .opacity(item_op)
                .cursor(Cursor::Pointer)
                .press_scale(0.98)
                .transition(Transition::none())
                .on_hover_change(garden_hover("arc"))
                .on_press_tx(move |s: &mut State, tx: &mut Tx| {
                    s.wave_arc_to = slot as f32;
                    if s.settings.reduce_motion || crate::fx::cpu_mode() {
                        s.wave_arc = s.wave_arc_to;
                        s.wave_arc_vel = 0.0;
                    } else {
                        tx.wake_after(std::time::Duration::from_millis(16));
                    }
                    if !is_shake && dock::wave_playing(s) && s.wave_choice == click_id {
                        s.toggle_pause();
                    } else {
                        start_wave_with(s, &click_id);
                    }
                })
                .child(el().w(ic_box).h(ic_box).center().child(icon_el))
                .child(text_el);

            arc = arc.child(item);
        }
    } else {
        let fs = (17.0 * arc_u).clamp(13.5, 19.0).round();
        let label_style = TextStyle::new(fs, 680.0).family(th::SANS).tracking(-0.2).lh(fs + 4.0);
        let fallback_item = row()
            .key("hm-arc-fallback")
            .role("button")
            .label(S::WaveTitle.get(lang))
            .absolute(pad_l + curve, (center_y - item_h * 0.5).round())
            .w((arc_w - pad_l - curve - 10.0).max(120.0))
            .h(item_h)
            .px(4.0)
            .gap(item_gap)
            .items_center()
            .opacity(stay)
            .cursor(Cursor::Pointer)
            .press_scale(0.98)
            .on_press(|s: &mut State| start_wave_with(s, "onyourwave"))
            .child(el().w(ic_box).h(ic_box).center().child(kit::wave_glyph((ic_box * 0.56).round(), accent_yellow)))
            .child(label(S::WaveTitle.get(lang), label_style, accent_yellow).truncate().min_w(0.0));
        arc = arc.child(fallback_item);
    }

    let foreground = row()
        .key("hm-band")
        .absolute(content_x, 0.0)
        .w(content_w)
        .h(stage_h)
        .items_center()
        .transition(Transition::none())
        .child(stage)
        .child(arc);

    let mini_corner = recede_el(
        row()
            .key("hm-mini")
            .role("button")
            .label(S::MiniPlayer.get(lang))
            .help(S::MiniPlayer.get(lang))
            .checked(s.mini_open)
            .pin(None, None, Some(20.0), Some(20.0))
            .w(38.0)
            .h(38.0)
            .radius(19.0)
            .center()
            .fill(chip_bg)
            .border(1.0, chip_border)
            .cursor(Cursor::Pointer)
            .hov(cell_hov)
            .press_scale(th::PRESS)
            .on_press(|s: &mut State| s.toggle_mini_player())
            .child(icon(&ni::MINI, 17.0).color(if s.mini_open { s.pal().accent } else { th::text().alpha(0.78) })),
        stay,
        14.0,
    );

    el().key("garden").w_full().h(stage_h).transition(Transition::none()).child(aura).child(foreground).child(mini_corner)
}
