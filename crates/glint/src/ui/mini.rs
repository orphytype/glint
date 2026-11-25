//! Floating system mini-player window: always-on-top, customizable layouts (`Compact`,
//! `Standard`, `Cover`), adjustable opacity, cover-tinted ambient backdrop, hover-revealed chrome,
//! keyboard shortcuts, and minimal resource consumption (0 FPS when idle, 1 Hz timeline updates
//! during playback, no background shader passes, and GPU surface released when closed).

use std::hash::{Hash, Hasher};

use rune::{
    AppHandle, Color, Cursor, Drag, El, Icon, Key, KeyEvent, Phase, Pixels, SurfaceFit, TextStyle,
    Transition, Tx, WheelEvent, col, el, icon, row,
};

use super::kit::{Hover as _, label};
use super::np_icons as ni;
use crate::app::State;
use crate::i18n::{Lang, S};
use crate::player::fmt_time;
use crate::store::{MiniStyle, Repeat};
use crate::theme as th;

const MINI_TITLE: TextStyle = TextStyle::new(13.5, 620.0).family(th::SANS).lh(17.5);
const MINI_TITLE_LG: TextStyle = TextStyle::new(14.5, 640.0).family(th::SANS).lh(18.5);
const MINI_SUB: TextStyle = TextStyle::new(12.0, 460.0).family(th::SANS).lh(15.5);
const MINI_TIME: TextStyle = TextStyle::new(10.5, 500.0).family(th::SANS).lh(13.0).features("'tnum' 1");
const HEART_HEX: u32 = 0xff4f70;

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
    pub tint2: [f32; 3],
    pub track_id: String,
    pub title: String,
    pub artist: String,
    pub cover: Option<Pixels>,
    pub soft_cover: Option<Pixels>,
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
            tint2: [0.14, 0.14, 0.18],
            track_id: String::new(),
            title: String::new(),
            artist: String::new(),
            cover: None,
            soft_cover: None,
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

    fn toggle_pin(&mut self) {
        let next = !self.always_on_top;
        self.always_on_top = next;
        if let Some(win) = &self.win {
            win.set_always_on_top(next);
        }
        self.send(move |s| {
            s.settings.mini_always_on_top = next;
            s.mark_settings_dirty();
        });
    }

    fn cycle_style(&mut self) {
        let next = self.style.next();
        self.style = next;
        self.send(move |s| s.set_mini_style(next));
    }

    fn toggle_like(&mut self) {
        if self.track_id.is_empty() {
            return;
        }
        self.liked = !self.liked;
        self.send(|s| {
            if let Some(t) = s.current().cloned() {
                s.toggle_like(&t);
            }
        });
    }

    fn nudge_volume(&mut self, delta: f32) {
        let next = (self.volume + delta).clamp(0.0, 1.0);
        self.volume = next;
        self.send(move |s| s.set_volume(next));
    }

    fn nudge_seek(&mut self, delta_ms: i64) {
        if self.dur_ms == 0 {
            return;
        }
        let end = self.dur_ms.saturating_sub(1) as i64;
        let next = (self.pos_ms as i64 + delta_ms).clamp(0, end.max(0)) as u64;
        self.pos_ms = next;
        self.send(move |s| s.seek_ms(next));
    }
}

/// Keyboard shortcuts when the floating mini-player window has focus.
pub fn on_key(m: &mut MiniState, ev: &KeyEvent, _tx: &mut Tx) -> bool {
    let mod_held = ev.mods.ctrl || ev.mods.cmd;
    match &ev.key {
        Key::Escape => {
            m.send(State::expand_mini_player);
            true
        }
        Key::Left => {
            if mod_held {
                if m.can_prev {
                    m.send(State::prev);
                }
            } else {
                m.nudge_seek(-5_000);
            }
            true
        }
        Key::Right => {
            if mod_held {
                if m.can_next {
                    m.send(State::next);
                }
            } else {
                m.nudge_seek(5_000);
            }
            true
        }
        Key::Up => {
            m.nudge_volume(0.05);
            true
        }
        Key::Down => {
            m.nudge_volume(-0.05);
            true
        }
        Key::Tab => {
            m.cycle_style();
            true
        }
        Key::Text(t) if !mod_held && !ev.mods.alt => match t.as_str() {
            " " => {
                m.playing = !m.playing;
                m.send(State::toggle_pause);
                true
            }
            "m" | "M" | "ь" | "Ь" => {
                m.send(State::toggle_mute);
                true
            }
            "l" | "L" | "д" | "Д" => {
                m.toggle_like();
                true
            }
            "p" | "P" | "з" | "З" => {
                m.toggle_pin();
                true
            }
            "v" | "V" | "м" | "М" => {
                m.cycle_style();
                true
            }
            _ => false,
        },
        _ => false,
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
            self.trim_hidden_memory();
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
        let cov_key = cur.and_then(|t| State::cover_key(t.cover_template()));
        let cover_px = cov_key.as_deref().map(|k| self.art.pixels(k));
        let soft_px = cov_key.as_deref().and_then(|k| self.art.soft(k));
        let has_cover = cover_px.is_some();
        let has_soft = soft_px.is_some();
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
        has_soft.hash(&mut hasher);
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
        self.palette.colors[0].map(f32::to_bits).hash(&mut hasher);
        self.palette.colors[1].map(f32::to_bits).hash(&mut hasher);
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
        let tint2 = self.palette.colors[1];
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
            m.tint2 = tint2;
            m.track_id = track_id_owned;
            m.title = title;
            m.artist = artist;
            m.cover = cover_px;
            m.soft_cover = soft_px;
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
    m.nudge_volume(step);
    true
}

fn base_opacity(m: &MiniState) -> f32 {
    if crate::fx::cpu_mode() { 1.0 } else { m.opacity.clamp(0.65, 1.0) }
}

fn surface_rgb(m: &MiniState) -> Color {
    let [r, g, b] = m.tint;
    if m.light {
        th::srgb([(0.95 + r * 0.04).min(1.0), (0.95 + g * 0.04).min(1.0), (0.96 + b * 0.04).min(1.0)])
    } else {
        th::srgb([(0.065 + r * 0.13).min(0.20), (0.068 + g * 0.13).min(0.20), (0.082 + b * 0.13).min(0.22)])
    }
}

fn surface_bg(m: &MiniState) -> Color {
    surface_rgb(m).alpha(base_opacity(m))
}

fn ink1(m: &MiniState) -> Color {
    if m.light { th::rgb(0x141418) } else { th::rgb(0xf5f5f7) }
}

fn ink2(m: &MiniState) -> Color {
    if m.light { th::rgba(0x141418, 0.64) } else { th::rgba(0xf5f5f7, 0.66) }
}

fn ink3(m: &MiniState) -> Color {
    if m.light { th::rgba(0x141418, 0.40) } else { th::rgba(0xf5f5f7, 0.42) }
}

fn veil_c(m: &MiniState, a: f32) -> Color {
    if m.light { th::rgba(0x000000, a * 0.72) } else { th::rgba(0xffffff, a) }
}

/// Subtle cover-tinted ambient backdrop for `Compact` and `Standard` layouts (0 shaders, 0 FPS idle).
fn ambient_wash(m: &MiniState, w: f32, h: f32, corner: f32) -> El {
    let a = base_opacity(m);
    let glow = th::srgb(m.tint2);
    let mut layer = el().absolute(0.0, 0.0).w(w).h(h).radius(corner).transition(Transition::none());
    if let Some(px) = &m.soft_cover {
        let strength = if m.light { 0.16 } else { 0.34 };
        layer = layer.child(el().absolute(0.0, 0.0).w(w).h(h).radius(corner).opacity(strength * a).pixels_fit(px, SurfaceFit::Fill));
    }
    let wash = surface_rgb(m);
    let top_a = (if m.light { 0.62 } else { 0.54 }) * a;
    let bot_a = (if m.light { 0.86 } else { 0.80 }) * a;
    layer
        .child(
            el().absolute(0.0, 0.0)
                .w(w)
                .h(h)
                .radius(corner)
                .linear_gradient(std::f32::consts::PI, [(0.0, wash.alpha(top_a)), (1.0, wash.alpha(bot_a))]),
        )
        .child(
            el().absolute(0.0, 0.0)
                .w(w)
                .h(h)
                .radius(corner)
                .radial_gradient([0.14, 0.25], w.max(h) * 0.72, [(0.0, glow.alpha((if m.light { 0.14 } else { 0.22 }) * a)), (1.0, glow.alpha(0.0))]),
        )
}

fn mini_ibtn(m: &MiniState, key: &'static str, ic: &'static Icon, glyph: f32, size: f32, color: Color, tip: &str) -> El {
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
        .hov(veil_c(m, 0.09))
        .press(veil_c(m, 0.15))
        .press_scale(th::PRESS)
        .child(icon(ic, glyph).color(color))
}

fn mini_play_btn(m: &MiniState, size: f32, glyph: f32, over_art: bool) -> El {
    let tip = if m.playing { S::Pause.get(m.lang) } else { S::Play.get(m.lang) };
    let ic = if m.playing { &ni::PAUSE } else { &ni::PLAY };
    let (bg, fg) = if over_art || !m.light { (th::rgb(0xffffff), th::rgb(0x111114)) } else { (th::rgb(0x141418), th::rgb(0xffffff)) };
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
        .press_scale(0.94)
        .on_press(|m: &mut MiniState| {
            m.playing = !m.playing;
            m.send(State::toggle_pause);
        })
        .child(icon(ic, glyph).color(fg))
}

/// Clickable album cover thumbnail: opens the main window on click and shows an expand hint on hover.
fn mini_cover_btn(m: &MiniState, side: f32, radius: f32) -> El {
    let mut box_el = el()
        .key("mini-cover")
        .role("button")
        .label(S::MiniExpand.get(m.lang))
        .help(S::MiniExpand.get(m.lang))
        .w(side)
        .h(side)
        .radius(radius)
        .fill(veil_c(m, 0.08))
        .border(1.0, veil_c(m, 0.12))
        .cursor(Cursor::Pointer)
        .no_window_drag()
        .hov_group()
        .press_scale(0.96)
        .on_press(|m: &mut MiniState| m.send(State::expand_mini_player));
    if let Some(px) = &m.cover {
        box_el = box_el.child(el().w_full().h_full().radius(radius).pixels_fit(px, SurfaceFit::Fill));
    } else {
        box_el = box_el.child(row().w_full().h_full().center().child(icon(&ni::NOTE, (side * 0.42).clamp(16.0, 42.0)).color(ink3(m))));
    }
    box_el.child(
        row()
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .radius(radius)
            .center()
            .fill(th::rgba(0x0c0e14, 0.52))
            .gop(0.0, 1.0)
            .child(icon(&ni::EXPAND, (side * 0.34).clamp(14.0, 18.0)).color(th::rgb(0xffffff))),
    )
}

fn mini_seek_bar(m: &MiniState, w: f32, show_times: bool, dark_art: bool) -> El {
    let frac = if m.dur_ms > 0 { (m.pos_ms as f32 / m.dur_ms as f32).clamp(0.0, 1.0) } else { 0.0 };
    let fill_w = if frac > 0.002 { (w * frac).max(3.0) } else { 0.0 };
    let thumb_x = (w * frac - 5.0).clamp(0.0, (w - 10.0).max(0.0));
    let track_bg = if dark_art { th::rgba(0xffffff, 0.20) } else { veil_c(m, 0.15) };
    let fill_col = if dark_art { th::rgb(0xffffff) } else { m.accent };
    let thumb_col = if dark_art { th::rgb(0xffffff) } else { ink1(m) };
    let time_col = if dark_art { th::rgba(0xffffff, 0.58) } else { ink3(m) };

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
        .child(el().absolute(0.0, 5.5).w(w).h(3.0).radius(1.5).fill(track_bg))
        .child(el().absolute(0.0, 5.5).w(fill_w).h(3.0).radius(1.5).fill(fill_col))
        .child(el().absolute(thumb_x, 2.0).w(10.0).h(10.0).radius(5.0).fill(thumb_col).gop(0.0, 1.0));
    if !show_times {
        return track;
    }
    let cur_str = fmt_time(m.pos_ms);
    let rem_str = format!("-{}", fmt_time(m.dur_ms.saturating_sub(m.pos_ms)));
    col()
        .w(w)
        .gap(1.0)
        .child(track)
        .child(row().w(w).justify_between().items_center().child(label(cur_str, MINI_TIME, time_col)).child(label(rem_str, MINI_TIME, time_col)))
}

fn mini_volume_slider(m: &MiniState, w: f32) -> El {
    let v = m.volume.clamp(0.0, 1.0);
    let fill_w = if v > 0.005 { (w * v).max(3.0) } else { 0.0 };
    let thumb_x = (w * v - 4.0).clamp(0.0, (w - 8.0).max(0.0));
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
        .hov_group()
        .instant_changes()
        .on_drag(|m: &mut MiniState, d: &mut Drag, _tx: &mut Tx| {
            let f = if d.w > 0.0 { (d.x / d.w).clamp(0.0, 1.0) } else { 0.0 };
            m.volume = f;
            m.send(move |s| s.set_volume(f));
        })
        .child(el().absolute(0.0, 7.5).w(w).h(3.0).radius(1.5).fill(veil_c(m, 0.16)))
        .child(el().absolute(0.0, 7.5).w(fill_w).h(3.0).radius(1.5).fill(ink2(m)))
        .child(el().absolute(thumb_x, 5.0).w(8.0).h(8.0).radius(4.0).fill(ink1(m)).gop(0.0, 1.0));
    row().gap(2.0).items_center().child(mute_btn).child(bar)
}

/// Hover-revealed floating top-right chrome pill (`Pin`, `Layout`, `Expand`, `Close`).
fn chrome_pill(m: &MiniState, over_art: bool) -> El {
    let lang = m.lang;
    let (pill_bg, pill_edge, ic_idle, hov_col) = if over_art {
        (th::rgba(0x0c0e14, 0.72), th::rgba(0xffffff, 0.16), th::rgba(0xffffff, 0.82), th::rgba(0xffffff, 0.14))
    } else if m.light {
        (surface_rgb(m).alpha(0.95), th::rgba(0x000000, 0.10), th::rgba(0x141418, 0.62), th::rgba(0x000000, 0.07))
    } else {
        (surface_rgb(m).alpha(0.95), th::rgba(0xffffff, 0.13), th::rgba(0xffffff, 0.68), th::rgba(0xffffff, 0.10))
    };
    let pill_btn = |key: &'static str, ic: &'static Icon, col: Color, tip: &str| {
        row()
            .key(key)
            .role("button")
            .label(tip)
            .help(tip)
            .w(22.0)
            .h(22.0)
            .radius(11.0)
            .center()
            .cursor(Cursor::Pointer)
            .no_window_drag()
            .hov(hov_col)
            .press_scale(th::PRESS)
            .child(icon(ic, 13.0).color(col))
    };
    let pin_ic = if m.always_on_top { &ni::PIN } else { &ni::PIN_OFF };
    let pin_col = if m.always_on_top { m.accent } else { ic_idle };
    let pin = pill_btn("mini-pin", pin_ic, pin_col, S::MiniAlwaysOnTop.get(lang)).on_press(MiniState::toggle_pin);
    let style_btn = pill_btn("mini-style", &ni::LAYOUT, ic_idle, S::MiniStyle.get(lang)).on_press(MiniState::cycle_style);
    let expand = pill_btn("mini-expand", &ni::EXPAND, ic_idle, S::MiniExpand.get(lang)).on_press(|m: &mut MiniState| m.send(State::expand_mini_player));
    let close = pill_btn("mini-close", &ni::CLOSE, ic_idle, S::Close.get(lang)).on_press(|m: &mut MiniState| m.send(|s| s.set_mini_player(false)));

    row()
        .key("mini-chrome")
        .h(26.0)
        .px(3.0)
        .gap(1.0)
        .radius(13.0)
        .items_center()
        .fill(pill_bg)
        .border(1.0, pill_edge)
        .no_window_drag()
        .gop(0.0, 1.0)
        .child(pin)
        .child(style_btn)
        .child(expand)
        .child(close)
}

fn title_texts(m: &MiniState) -> (String, String) {
    if m.track_id.is_empty() {
        (S::TrayNothing.get(m.lang).to_owned(), "Glint".to_owned())
    } else {
        (m.title.clone(), m.artist.clone())
    }
}

fn view_compact(m: &MiniState, w: f32, h: f32, corner: f32) -> El {
    let (title, artist) = title_texts(m);
    let frac = if m.dur_ms > 0 { (m.pos_ms as f32 / m.dur_ms as f32).clamp(0.0, 1.0) } else { 0.0 };
    let track_w = w - 24.0;
    let prog_w = if frac > 0.002 { (track_w * frac).max(3.0) } else { 0.0 };
    let thumb_x = (track_w * frac - 3.5).clamp(0.0, (track_w - 7.0).max(0.0));

    let prev = mini_ibtn(m, "mini-prev", &ni::PREV, 15.0, 26.0, if m.can_prev { ink1(m) } else { ink3(m) }, S::Previous.get(m.lang))
        .on_press(|m: &mut MiniState| m.send(State::prev));
    let next = mini_ibtn(m, "mini-next", &ni::NEXT, 15.0, 26.0, if m.can_next { ink1(m) } else { ink3(m) }, S::Next.get(m.lang))
        .on_press(|m: &mut MiniState| m.send(State::next));
    let heart_ic = if m.liked { &ni::HEART_FILL } else { &ni::HEART };
    let heart_col = if m.liked { th::rgb(HEART_HEX) } else { ink2(m) };
    let like = mini_ibtn(m, "mini-like", heart_ic, 14.0, 26.0, heart_col, S::Like.get(m.lang)).on_press(MiniState::toggle_like);
    let style_btn = mini_ibtn(m, "mini-style", &ni::LAYOUT, 13.0, 22.0, ink2(m), S::MiniStyle.get(m.lang)).gop(0.28, 1.0).on_press(MiniState::cycle_style);
    let close = mini_ibtn(m, "mini-close", &ni::CLOSE, 13.0, 22.0, ink2(m), S::Close.get(m.lang))
        .gop(0.28, 1.0)
        .on_press(|m: &mut MiniState| m.send(|s| s.set_mini_player(false)));

    let scrub = el()
        .key("mini-seek")
        .role("slider")
        .absolute(12.0, h - 10.0)
        .w(track_w)
        .h(10.0)
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
        .child(el().absolute(0.0, 5.5).w(track_w).h(2.5).radius(1.25).fill(veil_c(m, 0.14)))
        .child(el().absolute(0.0, 5.5).w(prog_w).h(2.5).radius(1.25).fill(m.accent))
        .child(el().absolute(thumb_x, 3.0).w(7.0).h(7.0).radius(3.5).fill(ink1(m)).gop(0.0, 1.0));

    el().w(w)
        .h(h)
        .child(ambient_wash(m, w, h, corner))
        .child(
            row()
                .w(w)
                .h(h - 3.0)
                .px(10.0)
                .gap(9.0)
                .items_center()
                .child(mini_cover_btn(m, 46.0, 9.0))
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
                        .child(mini_play_btn(m, 32.0, 17.0, false))
                        .child(next)
                        .child(el().w(1.0).h(16.0).mx(3.0).fill(veil_c(m, 0.12)).gop(0.25, 1.0))
                        .child(style_btn)
                        .child(close),
                ),
        )
        .child(scrub)
}

fn view_standard(m: &MiniState, w: f32, h: f32, corner: f32) -> El {
    let (title, artist) = title_texts(m);
    let prev = mini_ibtn(m, "mini-prev", &ni::PREV, 16.0, 30.0, if m.can_prev { ink1(m) } else { ink3(m) }, S::Previous.get(m.lang))
        .on_press(|m: &mut MiniState| m.send(State::prev));
    let next = mini_ibtn(m, "mini-next", &ni::NEXT, 16.0, 30.0, if m.can_next { ink1(m) } else { ink3(m) }, S::Next.get(m.lang))
        .on_press(|m: &mut MiniState| m.send(State::next));
    let heart_ic = if m.liked { &ni::HEART_FILL } else { &ni::HEART };
    let heart_col = if m.liked { th::rgb(HEART_HEX) } else { ink2(m) };
    let like = mini_ibtn(m, "mini-like", heart_ic, 15.0, 28.0, heart_col, S::Like.get(m.lang)).on_press(MiniState::toggle_like);
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
        .gap(11.0)
        .items_center()
        .child(mini_cover_btn(m, 54.0, 10.0))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(2.0)
                .child(label(title, MINI_TITLE_LG, ink1(m)).truncate())
                .child(label(artist, MINI_SUB, ink2(m)).truncate()),
        );

    let controls = row()
        .w_full()
        .items_center()
        .justify_between()
        .child(row().gap(2.0).items_center().child(like).child(shuf).child(rep))
        .child(row().gap(4.0).items_center().child(prev).child(mini_play_btn(m, 36.0, 18.0, false)).child(next))
        .child(mini_volume_slider(m, 54.0));

    let pill_w = 97.0;
    el().w(w)
        .h(h)
        .child(ambient_wash(m, w, h, corner))
        .child(col().w(w).h(h).px(12.0).py(11.0).justify_between().child(top).child(mini_seek_bar(m, w - 24.0, true, false)).child(controls))
        .child(chrome_pill(m, false).absolute(w - pill_w - 8.0, 8.0))
}

fn view_cover(m: &MiniState, w: f32, h: f32, corner: f32) -> El {
    let (title, artist) = title_texts(m);
    let art_btn = |key: &'static str, ic: &'static Icon, glyph: f32, size: f32, color: Color, tip: &str| {
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
            .hov(th::rgba(0xffffff, 0.12))
            .press(th::rgba(0xffffff, 0.20))
            .press_scale(th::PRESS)
            .child(icon(ic, glyph).color(color))
    };

    let prev = art_btn(
        "mini-prev",
        &ni::PREV,
        16.0,
        30.0,
        if m.can_prev { th::rgb(0xffffff) } else { th::rgba(0xffffff, 0.36) },
        S::Previous.get(m.lang),
    )
    .on_press(|m: &mut MiniState| m.send(State::prev));
    let next = art_btn(
        "mini-next",
        &ni::NEXT,
        16.0,
        30.0,
        if m.can_next { th::rgb(0xffffff) } else { th::rgba(0xffffff, 0.36) },
        S::Next.get(m.lang),
    )
    .on_press(|m: &mut MiniState| m.send(State::next));
    let heart_ic = if m.liked { &ni::HEART_FILL } else { &ni::HEART };
    let heart_col = if m.liked { th::rgb(HEART_HEX) } else { th::rgba(0xffffff, 0.78) };
    let like = art_btn("mini-like", heart_ic, 16.0, 28.0, heart_col, S::Like.get(m.lang)).on_press(MiniState::toggle_like);
    let shuf_col = if m.shuffle { m.accent } else { th::rgba(0xffffff, 0.56) };
    let shuf = art_btn("mini-shuf", &ni::SHUFFLE, 14.0, 26.0, shuf_col, S::Shuffle.get(m.lang)).on_press(|m: &mut MiniState| {
        m.shuffle = !m.shuffle;
        m.send(State::toggle_shuffle);
    });
    let rep_ic = if m.repeat == Repeat::One { &ni::REPEAT_ONE } else { &ni::REPEAT };
    let rep_col = if m.repeat != Repeat::Off { m.accent } else { th::rgba(0xffffff, 0.56) };
    let rep = art_btn("mini-rep", rep_ic, 14.0, 26.0, rep_col, S::Repeat.get(m.lang)).on_press(|m: &mut MiniState| {
        m.repeat = m.repeat.next();
        m.send(State::cycle_repeat);
    });

    // Dark-tinted cover deck so white ink and album art stay razor-sharp on any cover palette.
    let [r, g, b] = m.tint;
    let deck = th::srgb([(0.055 + r * 0.14).min(0.18), (0.058 + g * 0.14).min(0.18), (0.072 + b * 0.14).min(0.20)]);
    let a = base_opacity(m);

    let art_layer = if let Some(px) = &m.cover {
        el().absolute(0.0, 0.0).w(w).h(w).pixels_fit(px, SurfaceFit::Fill)
    } else {
        row()
            .absolute(0.0, 0.0)
            .w(w)
            .h(w)
            .center()
            .pb(40.0)
            .fill(deck.alpha(a))
            .child(icon(&ni::NOTE, 48.0).color(th::rgba(0xffffff, 0.24)))
    };

    let fade = el().absolute(0.0, 110.0).w(w).h(h - 110.0).linear_gradient(
        std::f32::consts::PI,
        [
            (0.0, deck.alpha(0.0)),
            (0.32, deck.alpha(0.56 * a)),
            (0.62, deck.alpha(0.94 * a)),
            (1.0, deck.alpha(a)),
        ],
    );

    let info_row = row()
        .w_full()
        .gap(8.0)
        .items_center()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(1.0)
                .child(label(title, MINI_TITLE_LG, th::rgb(0xffffff)).truncate())
                .child(label(artist, MINI_SUB, th::rgba(0xffffff, 0.72)).truncate()),
        )
        .child(like);

    let controls = row()
        .w_full()
        .items_center()
        .justify_between()
        .child(shuf)
        .child(prev)
        .child(mini_play_btn(m, 38.0, 19.0, true))
        .child(next)
        .child(rep);

    let deck_box = col()
        .absolute(0.0, h - 126.0)
        .w(w)
        .h(126.0)
        .px(14.0)
        .pb(11.0)
        .pt(6.0)
        .justify_between()
        .child(info_row)
        .child(mini_seek_bar(m, w - 28.0, true, true))
        .child(controls);

    let pill_w = 97.0;
    let _ = corner;
    el().w(w)
        .h(h)
        .fill(deck.alpha(a))
        .child(art_layer)
        .child(fade)
        .child(deck_box)
        .child(chrome_pill(m, true).absolute(w - pill_w - 8.0, 8.0))
}

/// View function for the secondary `"mini"` window.
pub fn view(m: &MiniState, _cx: &mut rune::Cx<MiniState>) -> El {
    let (w, h) = m.style.size();
    let corner = if crate::fx::cpu_mode() { 0.0 } else { 16.0 };
    let body = match m.style {
        MiniStyle::Compact => view_compact(m, w, h, corner),
        MiniStyle::Standard => view_standard(m, w, h, corner),
        MiniStyle::Cover => view_cover(m, w, h, corner),
    };
    let border_col = if m.style == MiniStyle::Cover { th::rgba(0xffffff, 0.16) } else { veil_c(m, 0.14) };
    let mut root = el()
        .key("mini-root")
        .w(w)
        .h(h)
        .radius(corner)
        .fill(surface_bg(m))
        .border(1.0, border_col)
        .window_drag()
        .hov_group()
        .transition(Transition::none())
        .on_wheel(mini_wheel)
        .child(body);
    if corner > 0.0 {
        root = root.clip_rounded(corner);
    }
    root
}
