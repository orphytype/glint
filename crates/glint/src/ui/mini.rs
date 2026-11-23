//! Floating system mini-player window: always-on-top, customizable layouts (`Compact`,
//! `Standard`, `Cover`), adjustable opacity, and minimal resource consumption (0 FPS when idle,
//! 1 Hz timeline updates during playback, no background shader passes, and GPU surface released
//! when closed).

use std::hash::{Hash, Hasher};

use rune::{
    AppHandle, Color, Cursor, Drag, El, Icon, Phase, Pixels, SurfaceFit, TextStyle, Transition, Tx,
    WheelEvent, col, el, icon, row,
};

use super::icons;
use super::kit::{Hover as _, label};
use super::np_icons as ni;
use crate::app::State;
use crate::i18n::{Lang, S};
use crate::player::fmt_time;
use crate::store::{MiniStyle, Repeat};
use crate::theme as th;

const MINI_TITLE: TextStyle = TextStyle::new(13.5, 620.0).family(th::SANS).lh(17.0);
const MINI_SUB: TextStyle = TextStyle::new(12.0, 450.0).family(th::SANS).lh(15.0);
const MINI_TIME: TextStyle = TextStyle::new(10.5, 500.0).family(th::SANS).lh(13.0).features("'tnum' 1");

/// State of the secondary `"mini"` window, synchronized from the main window's [`State`].
pub struct MiniState {
    pub main_app: Option<AppHandle<State>>,
    pub win: Option<rune::WindowHandle>,
    pub lang: Lang,
    pub style: MiniStyle,
    pub always_on_top: bool,
    pub opacity: f32,
    pub light: bool,
    pub accent: Color,
    pub tint: [f32; 3],
    pub track_id: String,
    pub title: String,
    pub artist: String,
    pub cover: Option<Pixels>,
    pub playing: bool,
    pub liked: bool,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub volume: f32,
    pub pos_ms: u64,
    pub dur_ms: u64,
    pub can_prev: bool,
    pub can_next: bool,
    pub scrubbing: bool,
}

impl Default for MiniState {
    fn default() -> Self {
        Self {
            main_app: None,
            win: None,
            lang: Lang::Ru,
            style: MiniStyle::Standard,
            always_on_top: true,
            opacity: 0.95,
            light: false,
            accent: th::rgb(0xffbe55),
            tint: [0.10, 0.10, 0.12],
            track_id: String::new(),
            title: String::new(),
            artist: String::new(),
            cover: None,
            playing: false,
            liked: false,
            shuffle: false,
            repeat: Repeat::Off,
            volume: 0.8,
            pos_ms: 0,
            dur_ms: 0,
            can_prev: false,
            can_next: false,
            scrubbing: false,
        }
    }
}

impl MiniState {
    fn send(&self, f: impl FnOnce(&mut State) + Send + 'static) {
        if let Some(app) = &self.main_app {
            app.update(move |s, _tx| {
                f(s);
                s.sync_mini();
            });
        }
    }
}

impl State {
    /// Opens or closes the floating mini-player window. Opening it hides the main window;
    /// closing it brings the main window back, unless the app lives in the tray.
    pub fn set_mini_player(&mut self, open: bool) {
        if self.headless || self.mini_open == open {
            return;
        }
        self.mini_open = open;
        self.mini_sync_key = 0;
        if open {
            self.sync_mini();
            if let Some(win) = &self.mini_win {
                win.resize(self.settings.mini_style.size());
                win.set_always_on_top(self.settings.mini_always_on_top);
                // The window is made again on every open: put it back where it was left.
                match self.settings.mini_pos {
                    Some(pos) if mini_pos_on_screen(pos) => win.set_position(pos),
                    _ => win.place(
                        rune::DisplayTarget::Primary,
                        rune::OverlayPlacement::Edge { edge: rune::ScreenEdge::Bottom, margin: 72.0, align: rune::Align::End, inset: 24.0 },
                    ),
                }
                win.open();
            }
            if let Some(app) = &self.app
                && app.is_visible()
            {
                self.remax = crate::desktop::place::hold(app);
                app.hide();
            }
        } else {
            if let Some(win) = self.mini_win.clone() {
                if let Some(p) = win.placement() {
                    self.settings.mini_pos = Some(p.position);
                    self.mark_settings_dirty();
                }
                win.close();
            }
            if let Some(app) = &self.app
                && !app.is_visible()
                && !self.keeps_running_when_closed()
            {
                app.open();
            }
        }
    }

    /// Closes the mini player and opens the main window.
    pub fn expand_mini_player(&mut self) {
        self.set_mini_player(false);
        if let Some(app) = &self.app {
            app.open();
        }
    }

    pub fn toggle_mini_player(&mut self) {
        self.set_mini_player(!self.mini_open);
    }

    pub fn set_mini_style(&mut self, style: MiniStyle) {
        if self.settings.mini_style == style {
            return;
        }
        self.settings.mini_style = style;
        self.mark_settings_dirty();
        if let Some(win) = &self.mini_win {
            win.resize(style.size());
        }
        self.mini_sync_key = 0;
        self.sync_mini();
    }

    pub fn set_mini_always_on_top(&mut self, on: bool) {
        self.settings.mini_always_on_top = on;
        self.mark_settings_dirty();
        if let Some(win) = &self.mini_win {
            win.set_always_on_top(on);
        }
        self.mini_sync_key = 0;
        self.sync_mini();
    }

    pub fn set_mini_opacity(&mut self, opacity: f32) {
        self.settings.mini_opacity = opacity.clamp(0.65, 1.0);
        self.mark_settings_dirty();
        self.mini_sync_key = 0;
        self.sync_mini();
    }

    /// Synchronizes playback and theme state to the mini-player window only when a visible value
    /// changed (at most once per second for the playback clock).
    pub fn sync_mini(&mut self) {
        if !self.mini_open || self.headless {
            return;
        }
        let Some(mini_app) = self.mini_app.clone() else { return };
        let cur = self.current();
        let track_id_owned = cur.map(|t| t.id.clone()).unwrap_or_default();
        let title = cur.map(ym_api::Track::full_title).unwrap_or_default();
        let artist = cur.map(ym_api::Track::artists_line).unwrap_or_default();
        let cover_px = cur
            .and_then(|t| State::cover_key(t.cover_template()))
            .map(|k| self.art.pixels(&k));
        let has_cover = cover_px.is_some();
        let pos_sec = self.player.pos_ms / 1000;
        let dur_sec = self.player.dur_ms / 1000;
        let liked = !track_id_owned.is_empty() && self.is_liked(&track_id_owned);
        let light = self.is_light();
        let accent = self.pal().accent;
        let vol_q = (self.player.volume.clamp(0.0, 1.0) * 100.0).round() as u8;
        let op_q = (self.settings.mini_opacity.clamp(0.65, 1.0) * 100.0).round() as u8;
        let can_prev = self.can_prev();
        let can_next = self.can_next();

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        track_id_owned.hash(&mut hasher);
        has_cover.hash(&mut hasher);
        self.player.playing.hash(&mut hasher);
        pos_sec.hash(&mut hasher);
        dur_sec.hash(&mut hasher);
        liked.hash(&mut hasher);
        self.player.shuffle.hash(&mut hasher);
        (self.player.repeat as u8).hash(&mut hasher);
        vol_q.hash(&mut hasher);
        (self.settings.mini_style as u8).hash(&mut hasher);
        self.settings.mini_always_on_top.hash(&mut hasher);
        op_q.hash(&mut hasher);
        light.hash(&mut hasher);
        self.settings.accent.hash(&mut hasher);
        self.palette.accent.map(f32::to_bits).hash(&mut hasher);
        can_prev.hash(&mut hasher);
        can_next.hash(&mut hasher);
        (self.lang as u8).hash(&mut hasher);
        let key = hasher.finish();
        if key == self.mini_sync_key {
            return;
        }
        self.mini_sync_key = key;

        let lang = self.lang;
        let style = self.settings.mini_style;
        let always_on_top = self.settings.mini_always_on_top;
        let opacity = self.settings.mini_opacity;
        let playing = self.player.playing;
        let shuffle = self.player.shuffle;
        let repeat = self.player.repeat;
        let volume = self.player.volume;
        let pos_ms = self.player.pos_ms;
        let dur_ms = self.player.dur_ms;
        let tint = self.palette.colors[0];
        let main_app = self.app.clone();
        let win = self.mini_win.clone();

        mini_app.update(move |m, _tx| {
            m.main_app = main_app;
            m.win = win;
            m.lang = lang;
            m.style = style;
            m.always_on_top = always_on_top;
            m.opacity = opacity;
            m.light = light;
            m.accent = accent;
            m.tint = tint;
            m.track_id = track_id_owned;
            m.title = title;
            m.artist = artist;
            m.cover = cover_px;
            m.playing = playing;
            m.liked = liked;
            m.shuffle = shuffle;
            m.repeat = repeat;
            m.volume = volume;
            if !m.scrubbing {
                m.pos_ms = pos_ms;
            }
            m.dur_ms = dur_ms;
            m.can_prev = can_prev;
            m.can_next = can_next;
        });
    }
}

/// Whether a saved mini-player position is still on a display (a monitor unplugged since).
fn mini_pos_on_screen(position: (i32, i32)) -> bool {
    crate::desktop::place::on_screen(&crate::store::SavedWindow { position, size: (160, 60), maximized: false })
}

fn mini_wheel(m: &mut MiniState, e: WheelEvent, _tx: &mut Tx) -> bool {
    if e.dy == 0.0 {
        return false;
    }
    let step = if e.precise { -e.dy * 0.004 } else { -e.dy.signum() * 0.05 };
    let next = (m.volume + step).clamp(0.0, 1.0);
    m.volume = next;
    m.send(move |s| s.set_volume(next));
    true
}

fn surface_bg(m: &MiniState) -> Color {
    let a = m.opacity.clamp(0.65, 1.0);
    if m.light {
        let [r, g, b] = m.tint;
        th::srgb([(0.95 + r * 0.04).min(1.0), (0.95 + g * 0.04).min(1.0), (0.96 + b * 0.04).min(1.0)]).alpha(a)
    } else {
        let [r, g, b] = m.tint;
        th::srgb([(0.08 + r * 0.14).min(0.22), (0.08 + g * 0.14).min(0.22), (0.10 + b * 0.14).min(0.24)]).alpha(a)
    }
}

fn ink1(m: &MiniState) -> Color {
    if m.light { th::rgb(0x161618) } else { th::rgb(0xf4f4f6) }
}

fn ink2(m: &MiniState) -> Color {
    if m.light { th::rgba(0x161618, 0.68) } else { th::rgba(0xf4f4f6, 0.68) }
}

fn ink3(m: &MiniState) -> Color {
    if m.light { th::rgba(0x161618, 0.42) } else { th::rgba(0xf4f4f6, 0.42) }
}

fn veil_c(m: &MiniState, a: f32) -> Color {
    if m.light { th::rgba(0x000000, a * 0.75) } else { th::rgba(0xffffff, a) }
}

fn mini_ibtn(
    m: &MiniState,
    key: &'static str,
    ic: &'static Icon,
    glyph: f32,
    size: f32,
    color: Color,
    tip: &str,
) -> El {
    row()
        .key(key)
        .role("button")
        .label(tip)
        .help(tip)
        .w(size)
        .h(size)
        .radius(size * 0.5)
        .center()
        .cursor(Cursor::Pointer)
        .no_window_drag()
        .hov(veil_c(m, 0.08))
        .press(veil_c(m, 0.14))
        .press_scale(th::PRESS)
        .child(icon(ic, glyph).color(color))
}

fn mini_play_btn(m: &MiniState, size: f32, glyph: f32) -> El {
    let tip = if m.playing { S::Pause.get(m.lang) } else { S::Play.get(m.lang) };
    let ic = if m.playing { &ni::PAUSE } else { &ni::PLAY };
    let bg = if m.light { th::rgb(0x161618) } else { th::rgb(0xffffff) };
    let fg = if m.light { th::rgb(0xffffff) } else { th::rgb(0x111113) };
    row()
        .key("mini-play")
        .role("button")
        .label(tip)
        .help(tip)
        .w(size)
        .h(size)
        .radius(size * 0.5)
        .center()
        .cursor(Cursor::Pointer)
        .no_window_drag()
        .fill(bg)
        .hov(bg.alpha(0.90))
        .press_scale(th::PRESS)
        .on_press(|m: &mut MiniState| {
            m.playing = !m.playing;
            m.send(State::toggle_pause);
        })
        .child(icon(ic, glyph).color(fg))
}

fn mini_cover(m: &MiniState, side: f32, radius: f32) -> El {
    let mut box_el = el()
        .key("mini-cover")
        .w(side)
        .h(side)
        .radius(radius)
        .fill(veil_c(m, 0.08))
        .border(1.0, veil_c(m, 0.10));
    if let Some(px) = &m.cover {
        box_el = box_el.child(el().w_full().h_full().radius(radius).pixels_fit(px, SurfaceFit::Fill));
    } else {
        box_el = box_el.child(
            row()
                .w_full()
                .h_full()
                .center()
                .child(icon(&icons::NOTE, (side * 0.42).clamp(16.0, 42.0)).color(ink3(m))),
        );
    }
    box_el
}

fn mini_seek_bar(m: &MiniState, w: f32, show_times: bool) -> El {
    let frac = if m.dur_ms > 0 { (m.pos_ms as f32 / m.dur_ms as f32).clamp(0.0, 1.0) } else { 0.0 };
    let fill_w = if frac > 0.002 { (w * frac).max(3.0) } else { 0.0 };
    let thumb_x = (w * frac - 5.0).clamp(0.0, (w - 10.0).max(0.0));
    let track = el()
        .key("mini-seek")
        .role("slider")
        .w(w)
        .h(14.0)
        .cursor(Cursor::Pointer)
        .no_window_drag()
        .hov_group()
        .instant_changes()
        .on_drag(|m: &mut MiniState, d: &mut Drag, _tx: &mut Tx| {
            let f = if d.w > 0.0 { (d.x / d.w).clamp(0.0, 1.0) } else { 0.0 };
            let up = d.phase == Phase::Up;
            m.scrubbing = !up;
            m.pos_ms = (f as f64 * m.dur_ms as f64) as u64;
            if up {
                m.send(move |s| s.seek_frac(f));
            }
        })
        .child(el().absolute(0.0, 5.5).w(w).h(3.0).radius(1.5).fill(veil_c(m, 0.16)))
        .child(el().absolute(0.0, 5.5).w(fill_w).h(3.0).radius(1.5).fill(m.accent))
        .child(el().absolute(thumb_x, 2.0).w(10.0).h(10.0).radius(5.0).fill(ink1(m)).gop(0.0, 1.0));
    if !show_times {
        return track;
    }
    let cur_str = fmt_time(m.pos_ms);
    let rem_ms = m.dur_ms.saturating_sub(m.pos_ms);
    let rem_str = format!("-{}", fmt_time(rem_ms));
    col()
        .w(w)
        .gap(1.0)
        .child(track)
        .child(
            row()
                .w(w)
                .justify_between()
                .items_center()
                .child(label(cur_str, MINI_TIME, ink3(m)))
                .child(label(rem_str, MINI_TIME, ink3(m))),
        )
}

fn mini_volume_slider(m: &MiniState, w: f32) -> El {
    let v = m.volume.clamp(0.0, 1.0);
    let fill_w = if v > 0.005 { (w * v).max(3.0) } else { 0.0 };
    let ic = if v < 0.01 {
        &ni::VOLUME_MUTE
    } else if v < 0.5 {
        &ni::VOLUME_LOW
    } else {
        &ni::VOLUME
    };
    let mute_btn = mini_ibtn(m, "mini-mute", ic, 14.0, 24.0, ink2(m), S::Mute.get(m.lang)).on_press(|m: &mut MiniState| {
        m.send(State::toggle_mute);
    });
    let bar = el()
        .key("mini-vol")
        .role("slider")
        .w(w)
        .h(18.0)
        .cursor(Cursor::Pointer)
        .no_window_drag()
        .instant_changes()
        .on_drag(|m: &mut MiniState, d: &mut Drag, _tx: &mut Tx| {
            let f = if d.w > 0.0 { (d.x / d.w).clamp(0.0, 1.0) } else { 0.0 };
            m.volume = f;
            m.send(move |s| s.set_volume(f));
        })
        .child(el().absolute(0.0, 7.5).w(w).h(3.0).radius(1.5).fill(veil_c(m, 0.16)))
        .child(el().absolute(0.0, 7.5).w(fill_w).h(3.0).radius(1.5).fill(ink2(m)));
    row().gap(2.0).items_center().child(mute_btn).child(bar)
}

fn chrome_buttons(m: &MiniState) -> El {
    let lang = m.lang;
    let pin_ic = if m.always_on_top { &icons::PIN } else { &icons::PIN_OFF };
    let pin_col = if m.always_on_top { m.accent } else { ink3(m) };
    let pin = mini_ibtn(m, "mini-pin", pin_ic, 13.0, 24.0, pin_col, S::MiniAlwaysOnTop.get(lang)).on_press(|m: &mut MiniState| {
        let next = !m.always_on_top;
        m.always_on_top = next;
        if let Some(win) = &m.win {
            win.set_always_on_top(next);
        }
        m.send(move |s| {
            s.settings.mini_always_on_top = next;
            s.mark_settings_dirty();
        });
    });
    let style_btn = mini_ibtn(m, "mini-style", &icons::WINDOW, 13.0, 24.0, ink3(m), S::MiniStyle.get(lang)).on_press(|m: &mut MiniState| {
        let next = m.style.next();
        m.style = next;
        m.send(move |s| s.set_mini_style(next));
    });
    let expand = mini_ibtn(m, "mini-expand", &icons::EXPAND, 13.0, 24.0, ink3(m), S::MiniExpand.get(lang))
        .on_press(|m: &mut MiniState| m.send(State::expand_mini_player));
    let close = mini_ibtn(m, "mini-close", &icons::CLOSE, 13.0, 24.0, ink3(m), S::Close.get(lang))
        .on_press(|m: &mut MiniState| m.send(|s| s.set_mini_player(false)));
    row().gap(1.0).items_center().child(pin).child(style_btn).child(expand).child(close)
}

fn title_texts(m: &MiniState, center: bool) -> (String, String) {
    if m.track_id.is_empty() {
        (S::TrayNothing.get(m.lang).to_owned(), "Glint".to_owned())
    } else {
        let _ = center;
        (m.title.clone(), m.artist.clone())
    }
}

fn view_compact(m: &MiniState, w: f32, h: f32) -> El {
    let (title, artist) = title_texts(m, false);
    let frac = if m.dur_ms > 0 { (m.pos_ms as f32 / m.dur_ms as f32).clamp(0.0, 1.0) } else { 0.0 };
    let prog_w = ((w - 24.0) * frac).max(0.0);
    let prev = mini_ibtn(m, "mini-prev", &ni::PREV, 15.0, 28.0, if m.can_prev { ink1(m) } else { ink3(m) }, S::Previous.get(m.lang))
        .on_press(|m: &mut MiniState| m.send(State::prev));
    let next = mini_ibtn(m, "mini-next", &ni::NEXT, 15.0, 28.0, if m.can_next { ink1(m) } else { ink3(m) }, S::Next.get(m.lang))
        .on_press(|m: &mut MiniState| m.send(State::next));
    let heart_ic = if m.liked { &ni::HEART_FILL } else { &ni::HEART };
    let heart_col = if m.liked { m.accent } else { ink2(m) };
    let like = mini_ibtn(m, "mini-like", heart_ic, 14.0, 26.0, heart_col, S::Like.get(m.lang)).on_press(|m: &mut MiniState| {
        m.liked = !m.liked;
        m.send(|s| {
            if let Some(t) = s.current().cloned() {
                s.toggle_like(&t);
            }
        });
    });
    let style_btn = mini_ibtn(m, "mini-style", &icons::WINDOW, 13.0, 24.0, ink3(m), S::MiniStyle.get(m.lang)).on_press(|m: &mut MiniState| {
        let next = m.style.next();
        m.style = next;
        m.send(move |s| s.set_mini_style(next));
    });
    let close = mini_ibtn(m, "mini-close", &icons::CLOSE, 13.0, 24.0, ink3(m), S::Close.get(m.lang))
        .on_press(|m: &mut MiniState| m.send(|s| s.set_mini_player(false)));

    el().w(w)
        .h(h)
        .child(
            row()
                .w(w)
                .h(h)
                .px(10.0)
                .gap(8.0)
                .items_center()
                .child(mini_cover(m, 46.0, 9.0))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(1.0)
                        .child(label(title, MINI_TITLE, ink1(m)).truncate())
                        .child(label(artist, MINI_SUB, ink2(m)).truncate()),
                )
                .child(
                    row()
                        .gap(2.0)
                        .items_center()
                        .child(like)
                        .child(prev)
                        .child(mini_play_btn(m, 32.0, 17.0))
                        .child(next)
                        .child(style_btn)
                        .child(close),
                ),
        )
        .child(el().absolute(12.0, h - 3.0).w(w - 24.0).h(2.0).radius(1.0).fill(veil_c(m, 0.12)))
        .child(el().absolute(12.0, h - 3.0).w(prog_w).h(2.0).radius(1.0).fill(m.accent))
}

fn view_standard(m: &MiniState, w: f32, h: f32) -> El {
    let (title, artist) = title_texts(m, false);
    let prev = mini_ibtn(m, "mini-prev", &ni::PREV, 16.0, 30.0, if m.can_prev { ink1(m) } else { ink3(m) }, S::Previous.get(m.lang))
        .on_press(|m: &mut MiniState| m.send(State::prev));
    let next = mini_ibtn(m, "mini-next", &ni::NEXT, 16.0, 30.0, if m.can_next { ink1(m) } else { ink3(m) }, S::Next.get(m.lang))
        .on_press(|m: &mut MiniState| m.send(State::next));
    let heart_ic = if m.liked { &ni::HEART_FILL } else { &ni::HEART };
    let heart_col = if m.liked { m.accent } else { ink2(m) };
    let like = mini_ibtn(m, "mini-like", heart_ic, 15.0, 28.0, heart_col, S::Like.get(m.lang)).on_press(|m: &mut MiniState| {
        m.liked = !m.liked;
        m.send(|s| {
            if let Some(t) = s.current().cloned() {
                s.toggle_like(&t);
            }
        });
    });
    let shuf_col = if m.shuffle { m.accent } else { ink3(m) };
    let shuf = mini_ibtn(m, "mini-shuf", &ni::SHUFFLE, 14.0, 26.0, shuf_col, S::Shuffle.get(m.lang)).on_press(|m: &mut MiniState| {
        m.shuffle = !m.shuffle;
        m.send(State::toggle_shuffle);
    });
    let rep_ic = if m.repeat == Repeat::One { &ni::REPEAT_ONE } else { &ni::REPEAT };
    let rep_col = if m.repeat != Repeat::Off { m.accent } else { ink3(m) };
    let rep = mini_ibtn(m, "mini-rep", rep_ic, 14.0, 26.0, rep_col, S::Repeat.get(m.lang)).on_press(|m: &mut MiniState| {
        m.repeat = m.repeat.next();
        m.send(State::cycle_repeat);
    });

    let top = row()
        .w_full()
        .gap(10.0)
        .items_center()
        .child(mini_cover(m, 52.0, 10.0))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(2.0)
                .child(
                    row()
                        .w_full()
                        .items_center()
                        .gap(4.0)
                        .child(label(title, MINI_TITLE, ink1(m)).truncate().grow(1.0).min_w(0.0))
                        .child(chrome_buttons(m)),
                )
                .child(label(artist, MINI_SUB, ink2(m)).truncate()),
        );

    let controls = row()
        .w_full()
        .items_center()
        .justify_between()
        .child(row().gap(2.0).items_center().child(like).child(shuf).child(rep))
        .child(row().gap(4.0).items_center().child(prev).child(mini_play_btn(m, 34.0, 18.0)).child(next))
        .child(mini_volume_slider(m, 54.0));

    col()
        .w(w)
        .h(h)
        .px(12.0)
        .py(10.0)
        .justify_between()
        .child(top)
        .child(mini_seek_bar(m, w - 24.0, true))
        .child(controls)
}

fn view_cover(m: &MiniState, w: f32, h: f32) -> El {
    let (title, artist) = title_texts(m, true);
    let prev = mini_ibtn(m, "mini-prev", &ni::PREV, 16.0, 30.0, if m.can_prev { ink1(m) } else { ink3(m) }, S::Previous.get(m.lang))
        .on_press(|m: &mut MiniState| m.send(State::prev));
    let next = mini_ibtn(m, "mini-next", &ni::NEXT, 16.0, 30.0, if m.can_next { ink1(m) } else { ink3(m) }, S::Next.get(m.lang))
        .on_press(|m: &mut MiniState| m.send(State::next));
    let heart_ic = if m.liked { &ni::HEART_FILL } else { &ni::HEART };
    let heart_col = if m.liked { m.accent } else { ink2(m) };
    let like = mini_ibtn(m, "mini-like", heart_ic, 15.0, 28.0, heart_col, S::Like.get(m.lang)).on_press(|m: &mut MiniState| {
        m.liked = !m.liked;
        m.send(|s| {
            if let Some(t) = s.current().cloned() {
                s.toggle_like(&t);
            }
        });
    });
    let rep_ic = if m.repeat == Repeat::One { &ni::REPEAT_ONE } else { &ni::REPEAT };
    let rep_col = if m.repeat != Repeat::Off { m.accent } else { ink3(m) };
    let rep = mini_ibtn(m, "mini-rep", rep_ic, 14.0, 28.0, rep_col, S::Repeat.get(m.lang)).on_press(|m: &mut MiniState| {
        m.repeat = m.repeat.next();
        m.send(State::cycle_repeat);
    });

    col()
        .w(w)
        .h(h)
        .px(14.0)
        .py(10.0)
        .items_center()
        .justify_between()
        .child(row().w_full().justify_between().items_center().child(label("Glint", MINI_TIME, ink3(m))).child(chrome_buttons(m)))
        .child(mini_cover(m, 140.0, 14.0))
        .child(
            col()
                .w_full()
                .items_center()
                .gap(2.0)
                .child(label(title, MINI_TITLE, ink1(m)).truncate().max_w(w - 28.0))
                .child(label(artist, MINI_SUB, ink2(m)).truncate().max_w(w - 28.0)),
        )
        .child(mini_seek_bar(m, w - 28.0, false))
        .child(
            row()
                .w_full()
                .items_center()
                .justify_between()
                .child(like)
                .child(row().gap(6.0).items_center().child(prev).child(mini_play_btn(m, 36.0, 19.0)).child(next))
                .child(rep),
        )
}

/// View function for the secondary `"mini"` window.
pub fn view(m: &MiniState, _cx: &mut rune::Cx<MiniState>) -> El {
    let (w, h) = m.style.size();
    let body = match m.style {
        MiniStyle::Compact => view_compact(m, w, h),
        MiniStyle::Standard => view_standard(m, w, h),
        MiniStyle::Cover => view_cover(m, w, h),
    };
    el().key("mini-root")
        .w(w)
        .h(h)
        .radius(16.0)
        .fill(surface_bg(m))
        .border(1.0, veil_c(m, 0.14))
        .window_drag()
        .transition(Transition::none())
        .on_wheel(mini_wheel)
        .child(body)
}
