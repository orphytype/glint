//! Track rows, queue rows, like/download row controls, and list playback sources.

use crate::fx::Slot as _;
use rune::{Align, Color, Cursor, El, Icon, PopupAnchor, TextStyle, Transition, Tx, col, el, icon, row};
use ym_api::Track;

use super::controls::{ctl_radius, ibtn, quiet_halo, swap_glyph};
use super::marquee::{marquee, marquee_hover, marquee_on, measure_room, room};
use super::media::{artists_row_in_width, cover};
use super::{Hover as _, label};
use crate::app::{DownloadState, FromName, Page, PlayingFrom, Source, State, TrackSort};
use crate::fx;
use crate::i18n::S;
use crate::theme::{self as th, veil};
use crate::ui::{icons, np_icons};

pub const ROW_H: f32 = 56.0;
pub const ROW_PAD: f32 = 8.0;
const ROW_GAP: f32 = 10.0;
const ROW_BTN: f32 = 32.0;
const ROW_GLYPH: f32 = 16.0;
const ROW_TIME: f32 = 44.0;
const ROW_ACTIONS: f32 = ROW_BTN * 2.0 + 8.0 + ROW_TIME;
pub const ROW_COVER: f32 = 44.0;
pub const ROW_COVER_R: f32 = 7.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListSrc {
    Liked,
    Detail,
    Search,
    Queue,
    History,
}

impl State {
    pub fn list(&self, src: ListSrc) -> Vec<Track> {
        match src {
            ListSrc::Liked => {
                let all = self.library.tracks.get().map_or(&[][..], Vec::as_slice);
                if self.library.filter.text.trim().is_empty() && self.library.sort == TrackSort::Original {
                    all.to_vec()
                } else {
                    self.library.shown.borrow().iter().filter_map(|&i| all.get(i).cloned()).collect()
                }
            }
            ListSrc::Detail => self.detail.tracks.clone(),
            ListSrc::Search => self.search_view().map(|r| r.tracks.results.clone()).unwrap_or_default(),
            ListSrc::Queue => self.player.queue.clone(),
            ListSrc::History => self.listening_history.clone(),
        }
    }

    pub fn play_from_list(&mut self, src: ListSrc, id: &str) {
        if src == ListSrc::Queue {
            if let Some(i) = self.player.queue.iter().position(|t| t.id == id) {
                if self.player.index == Some(i) && self.player.pending_index.is_none() {
                    self.toggle_pause();
                } else {
                    self.play_index(i);
                }
            }
            return;
        }
        if self.player.pending_index.is_none() && self.current().is_some_and(|c| c.id == id) {
            self.toggle_pause();
            return;
        }
        if src == ListSrc::Search {
            self.remember_search();
        }
        let list = self.list(src);
        if let Some(i) = list.iter().position(|t| t.id == id) {
            self.play_queue(list, i, Source::Manual);
            self.playing_from = self.list_from(src);
        }
    }

    pub fn list_from(&self, src: ListSrc) -> Option<PlayingFrom> {
        let (kind, name, page) = match src {
            ListSrc::Liked => (S::Liked, FromName::Word(S::LikedTracks), Some(Page::Library)),
            ListSrc::Detail => {
                let page = self.detail.page.clone()?;
                let kind = match page {
                    Page::Album(_) => S::Album,
                    Page::Playlist(..) => S::Playlist,
                    _ => S::Artist,
                };
                (kind, FromName::Text(self.detail.title.clone()), Some(page))
            }
            ListSrc::Search => (S::Search, FromName::Text(self.search.query.clone()), Some(Page::Search)),
            ListSrc::History => (S::Search, FromName::Word(S::ListeningHistory), Some(Page::Search)),
            ListSrc::Queue => return self.playing_from.clone(),
        };
        Some(PlayingFrom { wave: false, kind, name, page })
    }
}

fn open_track_menu(s: &mut State, track: &Track, queue_index: Option<usize>, anchor: impl Into<PopupAnchor>) {
    s.open_track_menu(track.clone(), queue_index, anchor.into());
}

fn heart_glyph(size: f32) -> f32 {
    (size * 0.48).round().clamp(14.0, 18.0)
}

fn like_button(s: &State, track: &Track, size: f32, on: bool) -> El {
    like_button_with(s, track, size, heart_glyph(size), (&icons::HEART, &icons::HEART_FILL), false, on)
}

pub fn np_like_button(s: &State, track: &Track, size: f32, glyph: f32) -> El {
    let on = s.is_liked(&track.id);
    like_button_with(s, track, size, glyph, (&np_icons::HEART, &np_icons::HEART_FILL), true, on)
}

fn like_button_with(s: &State, track: &Track, size: f32, glyph_px: f32, hearts: (&'static Icon, &'static Icon), quiet: bool, on: bool) -> El {
    let p = s.pal();
    let t = track.clone();
    let name = if on { S::Unlike.get(s.lang) } else { S::Like.get(s.lang) };
    let rest = if quiet { th::text3() } else { th::text2() };
    let mut glyph = icon(if on { hearts.1 } else { hearts.0 }, glyph_px).color(if on { p.accent } else { rest });
    if quiet && !on {
        glyph = glyph.gcol(th::text());
    }
    let mut b = row().key(format!("like-{}", track.id)).role("button").label(name).help(name).w(size).h(size).radius(ctl_radius(size)).cursor(Cursor::Pointer);
    b = if quiet { b.hov_group().child(quiet_halo(glyph_px)) } else { b.hov(veil(0.07)).press(veil(0.12)) };
    let mut b =
        b.press_scale(th::PRESS).child(swap_glyph(format!("like-{}-{on}", track.id), glyph, s.burst_on && s.burst_track == track.id)).checked(on).on_press(move |s: &mut State| {
            let liking = !s.is_liked(&t.id);
            s.toggle_like(&t);
            if liking {
                s.like_burst = s.now;
                s.burst_track = t.id.clone();
                s.burst_on = true;
            }
        });
    if s.burst_on && s.burst_track == track.id {
        b = b.child(el().key("burst").fx(fx::SPARKLES).fx_data([s.like_burst as f32, 14.0, 6.0, 0.0]).fx_bleed(40.0).absolute(0.0, 0.0).w_full().h_full().radius(ctl_radius(size)));
    }
    b
}

pub fn explicit_badge(s: &State) -> El {
    row()
        .key("explicit")
        .role("note")
        .label(S::Explicit.get(s.lang))
        .help(S::ExplicitHint.get(s.lang))
        .w(15.0)
        .h(14.0)
        .radius(3.5)
        .border(1.0, th::text3().alpha(0.7))
        .center()
        .child(label("E", TextStyle::new(9.0, 650.0).family(th::SANS).lh(9.0), th::text3()))
}

fn clean_badge(s: &State) -> El {
    let accent = s.pal().accent;
    row()
        .key("clean")
        .role("note")
        .label(S::CleanLoaded.get(s.lang))
        .help(S::CleanLoaded.get(s.lang))
        .w(18.0)
        .h(14.0)
        .radius(3.5)
        .fill(accent.alpha(0.16))
        .center()
        .child(icon(&icons::VOLUME, 11.0).color(accent))
}

fn title_line(s: &State, t: &Track, name: String, ink: Color, width: f32, moving: bool) -> El {
    let explicit = t.is_explicit();
    let clean = s.clean_tracks.contains(&t.id);
    let badges = 26.0 * (u8::from(explicit) + u8::from(clean)) as f32;
    let name_w = rune::text::measure(&name, &[], th::BODY_M, None).0.ceil();
    let room = if width.is_finite() { (width - badges).max(40.0) } else { width };
    let line = marquee(format!("tl-{}", t.id), room, name_w, moving, s.settings.reduce_motion, move || label(name.clone(), th::BODY_M, ink));
    let mut r = row().min_w(0.0).gap(6.0).items_center().child(line.shrink());
    if explicit {
        r = r.child(explicit_badge(s));
    }
    if clean {
        r = r.child(clean_badge(s));
    }
    r
}

fn marker(s: &State) -> El {
    el().key("playing-marker").w(22.0).h(22.0).transition(Transition::none()).fx(fx::MORPH).fx_data([if s.player.playing { 1.0 } else { 0.0 }, 1.0, 0.0, 0.0])
}

fn row_cover(s: &State, t: &Track, playing: bool, queue: bool) -> El {
    let dim = th::rgba(0x000000, if playing { 0.52 } else { 0.0 });
    let mut c = el()
        .key("rc")
        .w(ROW_COVER)
        .h(ROW_COVER)
        .transition(Transition::none())
        .child(cover(s, t.cover_template(), ROW_COVER, ROW_COVER_R))
        .child(el().key("rc-dim").absolute(0.0, 0.0).w_full().h_full().radius(ROW_COVER_R).transition(Transition::none()).fill(dim));
    if playing && queue {
        c = c.child(
            row()
                .key("rc-play-queue")
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .center()
                .transition(Transition::none())
                .child(icon(if s.player.playing { &icons::PAUSE } else { &icons::PLAY }, 16.0).color(th::rgb(0xffffff))),
        );
    } else if playing {
        c = c.child(row().key("rc-mark").absolute(0.0, 0.0).w_full().h_full().center().transition(Transition::none()).gop(1.0, 0.0).child(marker(s))).child(
            row()
                .key("rc-toggle")
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .center()
                .fill(th::rgba(0x000000, 0.36))
                .radius(ROW_COVER_R)
                .gop(0.0, 1.0)
                .child(icon(if s.player.playing { &icons::PAUSE } else { &icons::PLAY }, 16.0).color(th::rgb(0xffffff))),
        );
    } else {
        c = c.child(
            row()
                .key("rc-play")
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .center()
                .fill(th::rgba(0x000000, 0.36))
                .radius(ROW_COVER_R)
                .gop(0.0, 1.0)
                .child(icon(&icons::PLAY, 16.0).color(th::rgb(0xffffff))),
        );
    }
    c
}

fn download_action(s: &State, t: &Track) -> El {
    let state = s.downloads.get(&t.id).copied();
    let (mut ic, color, help) = match state {
        Some(DownloadState::Downloading) => (&icons::SPINNER, th::text2(), S::DownloadInProgress),
        Some(DownloadState::Done) => (&icons::DOWNLOAD, th::rgb(0x4bd27e).alpha(0.75), S::DeleteDownload),
        Some(DownloadState::Failed) => (&icons::DOWNLOAD, th::rgb(0xf18a8a), S::DownloadFailed),
        None => (&icons::DOWNLOAD, th::text3(), S::Download),
    };
    let mut help_buf = None;
    if matches!(state, Some(DownloadState::Failed))
        && let Some(fail) = s.download_errors.get(&t.id)
    {
        if fail.lasting() {
            ic = &icons::ALERT;
        }
        help_buf = Some(format!("{}: {}", help.get(s.lang), fail.reason().get(s.lang)));
    }
    let help_text = help_buf.as_deref().unwrap_or_else(|| help.get(s.lang));
    if matches!(state, Some(DownloadState::Downloading)) {
        return row()
            .key(format!("download-{}", t.id))
            .role("status")
            .label(help_text)
            .help(help_text)
            .w(ROW_BTN)
            .h(ROW_BTN)
            .center()
            .child(icon(ic, ROW_GLYPH).color(color).looping(rune::LoopSpec::rotation(0.9)));
    }
    let glyph_sz = if matches!(state, Some(DownloadState::Done)) { 14.0 } else { ROW_GLYPH };
    let mut b = ibtn(format!("download-{}", t.id), ic, glyph_sz, ROW_BTN, color, help_text);
    if state.is_none() {
        b = b.gop(0.0, 1.0);
    }
    let track = t.clone();
    match state {
        Some(DownloadState::Done) => b.on_press(move |s: &mut State| s.ask_delete_download(track.clone())),
        Some(DownloadState::Downloading) => b,
        _ => b.on_press(move |s: &mut State| s.ask_download(vec![track.clone()])),
    }
}

pub fn track_row(s: &State, t: &Track, n: usize, src: ListSrc) -> El {
    let qi = (src == ListSrc::Queue).then(|| s.player.queue.iter().position(|queued| queued.id == t.id).unwrap_or_else(|| n.saturating_sub(1)));
    let num = (src == ListSrc::Detail && !s.detail.is_artist).then_some(n);
    track_row_at(s, t, src, qi, num)
}

pub fn queue_row(s: &State, t: &Track, qi: usize) -> El {
    track_row_at(s, t, ListSrc::Queue, Some(qi), None).on_press(move |s: &mut State| {
        if s.player.index == Some(qi) && s.player.pending_index.is_none() {
            s.toggle_pause();
        } else {
            s.play_index(qi);
        }
    })
}

fn track_row_at(s: &State, t: &Track, src: ListSrc, qi: Option<usize>, num: Option<usize>) -> El {
    let queue = src == ListSrc::Queue;
    let playing = match (queue, qi) {
        (true, Some(i)) => s.player.index == Some(i),
        _ => s.current().is_some_and(|c| c.id == t.id),
    };
    let ink = if playing {
        s.pal().accent
    } else if t.is_available() {
        th::text()
    } else {
        th::text3()
    };
    let title = t.full_title();
    let liked = s.is_liked(&t.id);
    let hot = format!("row-{}", t.id);
    let moving = marquee_on(s, &hot);
    let room_w = room(&hot);
    let meta = artists_row_in_width(s, &t.artists, th::SMALL, false, &format!("{hot}-ar"), th::text2(), room_w, moving);
    let (id, t2, t3) = (t.id.clone(), t.clone(), t.clone());
    let more_key = format!("more-{}", t.id);
    let more_anchor = more_key.clone();
    let heart = like_button(s, t, ROW_BTN, liked);
    let heart = if queue || liked { heart } else { heart.gop(0.0, 1.0) };

    let time = row().absolute(0.0, 0.0).w_full().h_full().items_center().justify_end().gop(1.0, 0.0).child(label(
        crate::player::fmt_time(t.duration_ms.unwrap_or(0)),
        th::TIME,
        th::text3(),
    ));
    let more = row().absolute(0.0, 0.0).w_full().h_full().items_center().justify_end().gop(0.0, 1.0).child(
        ibtn(more_key, &icons::MORE, ROW_GLYPH + 2.0, ROW_BTN, th::text2(), S::More.get(s.lang)).on_press(move |s: &mut State| open_track_menu(s, &t3, qi, more_anchor.clone())),
    );
    let tail = el().key(format!("row-tail-{}", t.id)).w(ROW_TIME).h(ROW_BTN).child(time).child(more);
    let lead_action = if let Some(i) = qi.filter(|&i| queue && Some(i) > s.player.index) {
        ibtn(format!("q-rem-{i}-{}", t.id), &icons::CLOSE, 14.0, ROW_BTN, th::text3(), S::RemoveFromQueue.get(s.lang))
            .gop(0.0, 1.0)
            .on_press(move |s: &mut State| s.remove_from_queue(i))
    } else {
        download_action(s, t)
    };

    let mut r = row()
        .key(hot.clone())
        .role("button")
        .label(title.as_str())
        .w_full()
        .h(ROW_H)
        .pl(if num.is_some() { 4.0 } else { ROW_PAD })
        .pr(ROW_PAD)
        .gap(ROW_GAP)
        .items_center()
        .radius(10.0)
        .cursor(Cursor::Pointer)
        .fill(if playing { veil(0.07) } else { Color::TRANSPARENT })
        .hov(veil(0.045))
        .press(veil(0.08))
        .on_press(move |s: &mut State| s.play_from_list(src, &id))
        .on_secondary_press(move |s: &mut State, press, _tx: &mut Tx| open_track_menu(s, &t2, qi, press))
        .hov_group()
        .on_hover_change(marquee_hover(hot.clone()));
    if let Some(n) = num {
        r = r.child(label(n.to_string(), th::TIME, if playing { s.pal().accent } else { th::text3() }).w(20.0).align(Align::Center));
    }
    r.child(row_cover(s, t, playing, queue))
        .child(measure_room(col().grow(1.0).min_w(0.0).h_full().justify_center().gap(2.0).child(title_line(s, t, title, ink, room_w, moving)).child(meta), hot))
        .child(row().key(format!("row-actions-{}", t.id)).w(ROW_ACTIONS).h(ROW_BTN).gap(4.0).justify_end().items_center().child(lead_action).child(heart).child(tail))
}
