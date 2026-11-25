//! Track rows, queue rows, like/download row controls, and list playback sources.

use crate::fx::Slot as _;
use rune::{Color, Cursor, El, Icon, PopupAnchor, TextStyle, Transition, Tx, col, el, icon, row};
use ym_api::Track;

use super::controls::{ctl_radius, ibtn, ibtn_gop, quiet_halo, swap_glyph};
use super::marquee::{marquee, marquee_hover, marquee_on, measure_room, room};
use super::media::{artists_row_in_width, cover_placeholder, cover_sm};
use super::{Hover as _, label};
use crate::app::{DownloadState, FromName, Page, PlayingFrom, Source, State, TrackSort};
use crate::fx;
use crate::i18n::S;
use crate::theme::{self as th, veil};
use crate::ui::{icons, np_icons};

pub const ROW_H: f32 = 52.0;
/// Horizontal padding inside a track row's plate. Rows stay flush with the column edge so their
/// borders and active highlights align cleanly with the cover, search field, and dividers.
pub const ROW_PAD: f32 = 10.0;
const ROW_GAP: f32 = 12.0;
/// Every slot on the right of a row has the same square, so the icons are evenly spaced.
const ROW_BTN: f32 = 36.0;
const ROW_GLYPH: f32 = 16.0;
/// The duration, and the More button that takes its place on hover.
const ROW_TAIL: f32 = ROW_BTN;
/// Right of the duration, the text ends this far in: the same as a heading's count above.
pub const ROW_TIME_INSET: f32 = 2.0;
/// The right of a row: a download's state, the heart, and the duration or More. Everything else
/// a track can do is in its menu.
const ROW_ACTIONS: f32 = ROW_BTN * 2.0 + ROW_TAIL;
/// The number column of an album's rows, where other lists show a cover. The number sits in its
/// middle, and the title follows closer than it would a cover.
const ROW_NUM: f32 = 24.0;
const ROW_NUM_GAP: f32 = 8.0;
pub const ROW_COVER: f32 = 40.0;
pub const ROW_COVER_R: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListSrc {
    Liked,
    Detail,
    Search,
    Queue,
    History,
}

/// What a track filter looks through: the title, the artists and the album.
/// Rows in the shape of tracks while a list loads: a cover (or an album's number), the title
/// and artists in two lines of different lengths, and the time at the right.
pub fn track_bones(key: &str, n: usize, covers: bool) -> El {
    const TITLES: [f32; 8] = [0.42, 0.30, 0.36, 0.48, 0.27, 0.39, 0.33, 0.45];
    const BYS: [f32; 8] = [0.20, 0.16, 0.24, 0.18, 0.14, 0.22, 0.17, 0.19];
    let mut list = col().key(key.to_owned()).w_full();
    for i in 0..n {
        let lead = if covers {
            el().w(ROW_COVER).h(ROW_COVER).radius(ROW_COVER_R).fill(super::bone_fill())
        } else {
            row().w(ROW_NUM).h(ROW_COVER).center().child(el().w(10.0).h(10.0).radius(5.0).fill(super::bone_fill()))
        };
        let words = col()
            .grow(1.0)
            .min_w(0.0)
            .gap(8.0)
            .child(row().w_full().child(el().w_frac(TITLES[i % 8]).h(10.0).radius(5.0).fill(super::bone_fill())))
            .child(row().w_full().child(el().w_frac(BYS[i % 8]).h(8.0).radius(4.0).fill(super::bone_fill())));
        list = list.child(
            row()
                .key(format!("{key}-{i}"))
                .w_full()
                .h(ROW_H)
                .px(ROW_PAD)
                .gap(if covers { 12.0 } else { ROW_NUM_GAP })
                .items_center()
                .child(lead)
                .child(words)
                .child(el().w(28.0).h(8.0).radius(4.0).fill(super::bone_fill())),
        );
    }
    list
}

pub fn track_fields(t: &Track) -> Vec<String> {
    let mut f = vec![t.full_title(), t.artists_line()];
    f.extend(t.albums.first().map(|a| a.title.clone()));
    f
}

impl State {
    /// Which of a playlist's tracks its search finds, in order; None when the search is empty or
    /// the page isn't a playlist. Worked out once per query and kept.
    pub fn detail_shown(&self) -> Option<std::cell::Ref<'_, Vec<usize>>> {
        let q = self.detail.filter.text.trim();
        if q.is_empty() || !matches!(self.page, Page::Playlist(..)) {
            return None;
        }
        let v = &self.detail.tracks;
        let key = (q.to_owned(), self.detail.sort, v.as_ptr() as usize, v.len());
        if *self.detail.shown_key.borrow() != key {
            let query = crate::find::Query::new(q);
            *self.detail.shown.borrow_mut() = match query {
                None => (0..v.len()).collect(),
                Some(qr) => crate::find::best(v.iter().map(|t| {
                    let f = track_fields(t);
                    qr.tier(&f.iter().map(String::as_str).collect::<Vec<_>>())
                })),
            };
            *self.detail.shown_key.borrow_mut() = key;
        }
        Some(self.detail.shown.borrow())
    }

    pub fn list(&self, src: ListSrc) -> Vec<Track> {
        match src {
            ListSrc::Liked => {
                let all = self.library.tracks.get().map_or(&[][..], Vec::as_slice);
                if self.library.filter.text.trim().is_empty() && self.library.track_filter.is_empty() && self.library.sort == TrackSort::Original {
                    all.to_vec()
                } else {
                    self.library.shown.borrow().iter().filter_map(|&i| all.get(i).cloned()).collect()
                }
            }
            ListSrc::Detail => {
                match self.detail_shown() {
                    None => self.detail.tracks.clone(),
                    Some(shown) => shown.iter().filter_map(|&i| self.detail.tracks.get(i).cloned()).collect(),
                }
            }
            ListSrc::Search => self.search_view().map(|r| r.tracks.results.clone()).unwrap_or_default(),
            ListSrc::Queue => self.player.queue.clone(),
            ListSrc::History => self.listening_history.clone(),
        }
    }

    pub fn play_from_list(&mut self, src: ListSrc, id: &str) {
        if src == ListSrc::Queue {
            if let Some(i) = self.player.queue.iter().position(|t| t.id == id) {
                if self.player.index == Some(i) {
                    self.toggle_pause();
                } else {
                    self.play_index(i);
                }
            }
            return;
        }
        if self.current().is_some_and(|c| c.id == id) {
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
                    Page::Album(_) => {
                        let k = crate::app::release_kind_name(self.detail.release_kind.as_deref());
                        if k == S::Album && self.detail.tracks.len() == 1 { S::Single } else { k }
                    }
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

/// A square button on the right of a row, shown only while the row is hovered.
fn row_btn(key: String, ic: &'static Icon, glyph: f32, name: &str) -> El {
    ibtn_gop(key, ic, glyph, ROW_BTN, th::text2(), name)
}

/// Keeps a track list flush with the column's left and right edges so active/hover row plates
/// align directly with the release cover, search field, and section dividers.
pub fn bleed(list: El) -> El {
    row().w_full().child(list.grow(1.0))
}

fn heart_glyph(size: f32) -> f32 {
    (size * 0.48).round().clamp(14.0, 18.0)
}

fn like_button(s: &State, track: &Track, size: f32, on: bool, hover_only: bool) -> El {
    like_button_with(s, track, size, heart_glyph(size), (&icons::HEART, &icons::HEART_FILL), false, on, hover_only)
}

pub fn np_like_button(s: &State, track: &Track, size: f32, glyph: f32) -> El {
    let on = s.is_liked(&track.id);
    like_button_with(s, track, size, glyph, (&np_icons::HEART, &np_icons::HEART_FILL), true, on, false)
}

#[expect(clippy::too_many_arguments, reason = "shared builder for row and player like buttons")]
fn like_button_with(s: &State, track: &Track, size: f32, glyph_px: f32, hearts: (&'static Icon, &'static Icon), quiet: bool, on: bool, hover_only: bool) -> El {
    let p = s.pal();
    let t = track.clone();
    let name = if on { S::Unlike.get(s.lang) } else { S::Like.get(s.lang) };
    let rest = if quiet { th::text3() } else { th::text2() };
    let mut glyph = icon(if on { hearts.1 } else { hearts.0 }, glyph_px).color(if on { p.accent } else { rest });
    if quiet && !on {
        glyph = glyph.gcol(th::text());
    }
    if hover_only {
        glyph = glyph.gop(0.0, 1.0);
    }
    let r = ctl_radius(size);
    let mut b = row().key(format!("like-{}", track.id)).role("button").label(name).help(name).w(size).h(size).radius(r).cursor(Cursor::Pointer);
    b = if quiet {
        b.hov_group().child(quiet_halo(glyph_px))
    } else {
        // Put `.hov` on a leaf sibling so the button doesn't override the row's hover group for an
        // unliked heart.
        b.press(veil(0.12)).child(el().key("hov").absolute(0.0, 0.0).w_full().h_full().radius(r).hov(veil(0.07)))
    };
    let mut b = b.press_scale(th::PRESS).child(swap_glyph(format!("like-{}-{on}", track.id), glyph, size, glyph_px, s.burst_on && s.burst_track == track.id)).checked(on).on_press(
        move |s: &mut State| {
            let liking = !s.is_liked(&t.id);
            s.toggle_like(&t);
            if liking {
                s.like_burst = s.now;
                s.burst_track = t.id.clone();
                s.burst_on = true;
            }
        },
    );
    if s.burst_on && s.burst_track == track.id {
        b = b.child(el().key("burst").fx(fx::SPARKLES).fx_data([s.like_burst as f32, 14.0, 6.0, 0.0]).fx_bleed(40.0).absolute(0.0, 0.0).w_full().h_full().radius(r));
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
    let name_w = super::measure_w(&name, th::BODY_M).ceil();
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

fn marker(s: &State, on_cover: bool) -> Option<El> {
    if !fx::ready() {
        return None;
    }
    Some(
        el().key("playing-marker")
            .w(22.0)
            .h(22.0)
            .transition(Transition::none())
            .fx(fx::MORPH)
            .fx_data([if s.player.playing { 1.0 } else { 0.0 }, if on_cover { 1.0 } else { 0.0 }, 0.0, 0.0]),
    )
}

fn row_cover(s: &State, t: &Track, playing: bool, hovered: bool, dim_past: bool, load_art: bool) -> El {
    let dim_a = if playing && fx::ready() {
        0.52
    } else if dim_past {
        0.38
    } else {
        0.0
    };
    let dim = th::rgba(0x000000, dim_a);
    let art = if load_art { cover_sm(s, t.cover_template(), ROW_COVER, ROW_COVER_R) } else { cover_placeholder(ROW_COVER, ROW_COVER_R) };
    let mut c = el()
        .key("rc")
        .w(ROW_COVER)
        .h(ROW_COVER)
        .transition(Transition::none())
        .child(art)
        .child(el().key("rc-dim").absolute(0.0, 0.0).w_full().h_full().radius(ROW_COVER_R).transition(Transition::none()).fill(dim));
    if playing {
        if hovered {
            c = c.child(
                row()
                    .key("rc-toggle")
                    .absolute(0.0, 0.0)
                    .w_full()
                    .h_full()
                    .center()
                    .fill(th::rgba(0x000000, 0.36))
                    .radius(ROW_COVER_R)
                    .transition(Transition::none())
                    .child(icon(if s.player.playing { &icons::PAUSE } else { &icons::PLAY }, 16.0).color(th::rgb(0xffffff))),
            );
        } else if let Some(m) = marker(s, true) {
            c = c.child(row().key("rc-mark").absolute(0.0, 0.0).w_full().h_full().center().transition(Transition::none()).child(m));
        }
    } else {
        c = c
            .child(el().key("rc-play-bg").absolute(0.0, 0.0).w_full().h_full().fill(th::rgba(0x000000, 0.36)).radius(ROW_COVER_R).gop(0.0, 1.0))
            .child(row().key("rc-play").absolute(0.0, 0.0).w_full().h_full().center().child(icon(&icons::PLAY, 16.0).color(th::rgb(0xffffff)).gop(0.0, 1.0)));
    }
    c
}

/// A download in progress, done or failed, shown whether or not the row is hovered. A track that
/// isn't downloaded shows nothing here: Download is in its menu.
fn download_status(s: &State, t: &Track) -> Option<El> {
    let state = s.downloads.get(&t.id).copied();
    let (mut ic, color, help) = match state {
        Some(DownloadState::Downloading) => (&icons::SPINNER, th::text2(), S::DownloadInProgress),
        Some(DownloadState::Done) => (&icons::DOWNLOAD, th::rgb(0x4bd27e).alpha(0.85), S::DownloadDone),
        Some(DownloadState::Failed) => (&icons::DOWNLOAD, th::rgb(0xf18a8a), S::DownloadFailed),
        None => (&icons::DOWNLOAD, th::text2(), S::Download),
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
    Some(match state {
        Some(DownloadState::Downloading) => row()
            .key(format!("download-{}", t.id))
            .role("status")
            .label(help_text)
            .help(help_text)
            .w(ROW_BTN)
            .h(ROW_BTN)
            .center()
            .child(super::spin(icon(ic, ROW_GLYPH).color(color))),
        Some(DownloadState::Done) => row()
            .key(format!("download-done-{}", t.id))
            .role("status")
            .label(help_text)
            .help(help_text)
            .w(ROW_BTN)
            .h(ROW_BTN)
            .center()
            .child(icon(ic, ROW_GLYPH).color(color)),
        Some(DownloadState::Failed) => {
            let track = t.clone();
            ibtn(format!("download-{}", t.id), ic, ROW_GLYPH, ROW_BTN, color, help_text).on_press(move |s: &mut State| s.ask_download(vec![track.clone()]))
        }
        None => return None,
    })
}

pub fn track_row(s: &State, t: &Track, n: usize, src: ListSrc) -> El {
    track_row_lazy(s, t, n, src, true)
}

pub fn track_row_lazy(s: &State, t: &Track, n: usize, src: ListSrc, load_art: bool) -> El {
    let qi = (src == ListSrc::Queue).then(|| s.player.queue.iter().position(|queued| queued.id == t.id).unwrap_or_else(|| n.saturating_sub(1)));
    let num = (src == ListSrc::Detail && matches!(s.detail.page, Some(Page::Album(_)))).then_some(n);
    track_row_at(s, t, src, qi, num, false, n.saturating_sub(1), load_art)
}

pub fn queue_row(s: &State, t: &Track, qi: usize, dim_past: bool) -> El {
    track_row_at(s, t, ListSrc::Queue, Some(qi), None, dim_past, qi, true).on_press(move |s: &mut State| {
        if s.player.index == Some(qi) {
            s.toggle_pause();
        } else {
            s.play_index(qi);
        }
    })
}

#[expect(clippy::too_many_arguments, reason = "shared builder for list and queue track rows")]
fn track_row_at(s: &State, t: &Track, src: ListSrc, qi: Option<usize>, num: Option<usize>, dim_past: bool, row_idx: usize, load_art: bool) -> El {
    let queue = src == ListSrc::Queue;
    let reorderable = queue
        || (src == ListSrc::Detail
            && matches!(s.page, Page::Playlist(..))
            && s.detail.sort == TrackSort::Original
            && s.detail.filter.text.trim().is_empty()
            && s.detail.tracks.len() > 1);
    let active_drag = s.row_drag.as_ref().filter(|d| d.queue == queue && d.cur == row_idx);
    let is_dragging = active_drag.is_some();
    let playing = match (queue, qi) {
        (true, Some(i)) => s.player.index == Some(i),
        _ => s.current().is_some_and(|c| c.id == t.id),
    };
    let ink = if playing && !queue {
        s.pal().accent
    } else if !s.track_playable(t) {
        th::text3()
    } else if dim_past {
        th::text2()
    } else {
        th::text()
    };
    let artist_ink = if dim_past { th::text3() } else { th::text2() };
    let title = t.full_title();
    let liked = s.is_liked(&t.id);
    let hot = format!("row-{}", t.id);
    let moving = marquee_on(s, &hot);
    let room_w = room(&hot);
    let meta = artists_row_in_width(s, &t.artists, th::SMALL, false, &format!("{hot}-ar"), artist_ink, room_w, moving);
    let (id, t2, t3) = (t.id.clone(), t.clone(), t.clone());
    let more_key = format!("more-{}", t.id);
    let more_anchor = more_key.clone();
    // In the queue the empty heart stays in sight, not only on hover.
    let heart = like_button(s, t, ROW_BTN, liked, false);

    let time = row()
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .items_center()
        .justify_end()
        .pr(ROW_TIME_INSET)
        .child(label(crate::player::fmt_time(t.duration_ms.unwrap_or(0)), th::TIME, th::text3()).gop(1.0, 0.0));
    let more = row().absolute(0.0, 0.0).w_full().h_full().items_center().justify_end().child(
        row_btn(more_key, &icons::MORE, ROW_GLYPH + 2.0, S::More.get(s.lang)).on_press(move |s: &mut State| open_track_menu(s, &t3, qi, more_anchor.clone())),
    );
    let tail = el().key(format!("row-tail-{}", t.id)).w(ROW_TAIL).h(ROW_BTN).child(time).child(more);
    let actions = row()
        .key(format!("row-actions-{}", t.id))
        .w(ROW_ACTIONS)
        .h(ROW_BTN)
        .justify_end()
        .items_center()
        .children(download_status(s, t))
        .child(heart)
        .child(tail);

    let lead_pad = if queue { (ROW_H - ROW_COVER) * 0.5 } else { ROW_PAD };
    let mut r = row()
        .key(hot.clone())
        .role("button")
        .label(title.as_str())
        .w_full()
        .h(ROW_H)
        .pl(lead_pad)
        .pr(ROW_PAD)
        .gap(ROW_GAP)
        .items_center()
        .radius(10.0)
        .cursor(Cursor::Pointer)
        .fill(if is_dragging {
            veil(0.12)
        } else if playing {
            veil(0.07)
        } else {
            Color::TRANSPARENT
        })
        .hov(veil(if playing { 0.095 } else { 0.045 }))
        .press(veil(0.08))
        .on_press(move |s: &mut State| s.play_from_list(src, &id))
        .on_secondary_press(move |s: &mut State, press, _tx: &mut Tx| open_track_menu(s, &t2, qi, press))
        .hov_group()
        .on_hover_change(marquee_hover(hot.clone()));
    if reorderable {
        r = r.on_drag(move |s: &mut State, d: &mut rune::Drag, tx: &mut Tx| s.drag_row(queue, row_idx, d, tx));
    }
    if is_dragging {
        r = r.cursor(Cursor::Grabbing);
    }
    if let Some(drag) = active_drag {
        let residual = drag.dy - (drag.cur as f32 - drag.start as f32) * ROW_H;
        r = r.offset(0.0, residual).raise().instant_changes();
    }
    let lead = match num {
        Some(n) => row_number(s, n, playing, moving),
        None => row_cover(s, t, playing, moving, dim_past, load_art),
    };
    r = r
        .child(lead)
        .child(measure_room(col().grow(1.0).min_w(0.0).h_full().justify_center().gap(2.0).child(title_line(s, t, title, ink, room_w, moving)).child(meta), hot))
        .child(actions);
    r
}

/// An album's row starts with its track number where other lists show a cover: the covers would
/// all be the album's own. The playing track shows the marker; a hovered one, play or pause.
fn row_number(s: &State, n: usize, playing: bool, hovered: bool) -> El {
    let mut c = el().key("rn").w(ROW_NUM).h(ROW_COVER).mr(ROW_NUM_GAP - ROW_GAP).transition(Transition::none());
    if playing {
        let inner = if hovered {
            icon(if s.player.playing { &icons::PAUSE } else { &icons::PLAY }, 15.0).color(th::text())
        } else if let Some(m) = marker(s, false) {
            m
        } else {
            label(n.to_string(), th::TIME, s.pal().accent)
        };
        c = c.child(row().absolute(0.0, 0.0).w_full().h_full().center().transition(Transition::none()).child(inner));
    } else {
        c = c
            .child(row().absolute(0.0, 0.0).w_full().h_full().center().child(label(n.to_string(), th::TIME, th::text3()).gop(1.0, 0.0)))
            .child(row().absolute(0.0, 0.0).w_full().h_full().center().child(icon(&icons::PLAY, 15.0).color(th::text()).gop(0.0, 1.0)));
    }
    c
}
