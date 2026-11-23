//! Album, playlist, and artist detail pages: header, track list, colophon/bio card, discography,
//! and similar artists.

use rune::{Cursor, El, FileDialogOptions, Transition, Tx, col, el, icon, row, text_input};

use super::shared::{GRID_GAP, album_card, artist_card, column, facts_line, fit, heading_side, hscroll, play_button, row_arrows, track_list};
use crate::app::{DiscKind, DiscSort, Load, Page, Popover, Sheet, State, Station, TrackSort};
use crate::i18n::{Lang, S};
use crate::theme::{self as th, Pal};
use crate::ui::shapes::Ill;
use crate::ui::{
    icons,
    kit::{self, Hover as _, ListSrc, PAGE_MAX, RAIL_W, label},
};

pub(super) const HEAD_SIDE: f32 = 176.0;
pub(super) const HEAD_SIDE_S: f32 = 140.0;
pub(crate) const HEAD_COVER_R: f32 = 18.0;
pub(super) const HEAD_STEP_BY: f32 = 6.0;
pub(super) const HEAD_STEP_BUTTONS: f32 = 14.0;
pub(super) const HEAD_GAP: f32 = 28.0;

fn head_picture(s: &State, side: f32) -> El {
    let lang = s.lang;
    let d = &s.detail;
    let radius = if d.is_artist { side * 0.5 } else { HEAD_COVER_R };
    if d.page.is_none() || (d.state.is_loading() && d.cover.is_none()) {
        return el().key("detail-cover-bone").w(side).h(side).radius(radius).fill(kit::bone_fill());
    }
    if d.cover.is_none() {
        return kit::cover(s, None, side, radius);
    }
    let about = if d.is_artist { S::AboutArtist } else { S::Details }.get(lang);
    el().key("detail-cover")
        .role("button")
        .label(about)
        .help(about)
        .w(side)
        .h(side)
        .radius(radius)
        .cursor(Cursor::Pointer)
        .transition(Transition::none())
        .hov_group()
        .on_press(|s: &mut State| s.open_sheet(Sheet::Page))
        .child(kit::cover(s, d.cover.as_deref(), side, radius))
        .child(el().absolute(0.0, 0.0).w_full().h_full().radius(radius).fill(th::rgba(0xffffff, 0.06)).gop(0.0, 1.0))
}

fn detail_facts(s: &State) -> Option<El> {
    let lang = s.lang;
    let d = &s.detail;
    if d.is_artist {
        return None;
    }
    let about = d.facts.iter().find(|(n, _)| *n == S::Description).map(|(_, v)| v.clone());

    let release_date = d.facts.iter().find(|(n, _)| *n == S::ReleaseDate).map(|(_, v)| v.as_str());
    let version = d.facts.iter().find(|(n, _)| *n == S::Version).map(|(_, v)| v.as_str());
    let has_colophon = release_date.is_some() || version.is_some() || !d.labels.is_empty();
    if !has_colophon && about.is_none() {
        return None;
    }

    let mut c = col().key("d-about").w_full().mt(-16.0).gap(12.0).transition(Transition::none()).child(el().w_full().h(1.0).fill(th::veil(0.06)));
    if let Some(text) = about {
        c = c.child(label(text, th::BODY, th::text2()).wrap().max_lines(8).max_w(720.0));
    }
    if has_colophon {
        let mut lines = col().grow(1.0).min_w(0.0).gap(3.0);
        let top = facts_line([release_date.unwrap_or_default().to_owned(), version.unwrap_or_default().to_owned()]);
        if !top.is_empty() {
            lines = lines.child(label(top, th::SMALL, th::text3()).truncate().min_w(0.0));
        }
        if !d.labels.is_empty() {
            lines = lines.child(label(format!("℗ {}", d.labels.join(", ")), th::SMALL, th::text3()).truncate().min_w(0.0));
        }
        let more = kit::more_link("d-details-link", S::Details.get(lang)).on_press(|s: &mut State| s.open_sheet(Sheet::Page));
        c = c.child(row().key("d-colophon").w_full().gap(16.0).items_center().child(lines).child(more));
    }
    Some(c)
}

fn head_backdrop(s: &State, w: f32, side: f32) -> Option<El> {
    let tpl = s.detail.cover.as_deref()?;
    let pic = State::cover_key(Some(tpl))?;
    let px = s.art.soft(&pic)?;
    let page_w = (w - RAIL_W).max(0.0);
    let left = ((page_w - page_w.min(PAGE_MAX)) * 0.5).max(0.0);
    let h = kit::TITLE_H + 16.0 + side + 120.0;
    let strength = if th::light() { 0.40 } else { 0.52 };
    Some(
        el().key(format!("d-backdrop-{tpl}"))
            .absolute(-left, 0.0)
            .w(page_w)
            .h(h)
            .clip_fade(0.0, 0.0, 0.0, h * 0.66)
            .transition(Transition::none())
            .child(el().absolute(0.0, 0.0).w_full().h_full().opacity(strength).pixels_fit(&px, rune::SurfaceFit::Fill)),
    )
}

fn head_facts(s: &State, kind: S) -> String {
    let lang = s.lang;
    let d = &s.detail;
    let one = d.facts.iter().any(|(n, v)| *n == S::Tracks && v.trim() == "1");
    let mut parts = vec![kind.get(lang).to_owned()];
    if let Some(y) = d.year {
        parts.push(y.to_string());
    }
    for (name, value) in &d.facts {
        match name {
            S::Tracks if !one => parts.push(value.parse::<usize>().map_or_else(|_| value.clone(), |n| lang.tracks(n))),
            S::Duration if !one => parts.push(value.clone()),
            _ => {}
        }
    }
    if let Some(n) = d.monthly_listeners {
        parts.push(format_listeners(n, lang));
    }
    facts_line(parts)
}

fn head_buttons(s: &State) -> El {
    let lang = s.lang;
    let d = &s.detail;
    let mut bar = row().key("d-buttons").w_full().gap(8.0).items_center().flex_wrap();
    if !d.tracks.is_empty() {
        bar = bar
            .child(play_button(s, "d-play", d.page.clone(), false, |s| {
                let list = s.detail.tracks.clone();
                s.play_all(list, false);
            }))
            .child(kit::round("d-shuffle", kit::Weight::Soft, &icons::SHUFFLE, kit::BTN_H, 18.0, S::Shuffle.get(lang)).on_press(|s: &mut State| {
                let list = s.detail.tracks.clone();
                s.play_all(list, true);
            }));
    }
    let wave = match &d.page {
        Some(Page::Artist(id)) => Some(kit::wave_button(s, "d-wave", S::WaveByArtist.get(lang), &d.title, Station::Artist(id.clone()), kit::BTN_H)),
        Some(Page::Album(id)) => Some(kit::wave_button(s, "d-wave", S::WaveByRelease.get(lang), &d.title, Station::Album(id.clone()), kit::BTN_H)),
        _ => None,
    };
    if let Some(w) = wave {
        bar = bar.child(w);
    }
    let on = s.popover_open && matches!(s.popover, Some(Popover::Actions { .. }));
    bar = bar.child(
        kit::pill_icon("d-more", kit::Weight::Soft, &icons::MORE, kit::BTN_H, 20.0, S::More.get(lang))
            .checked(on)
            .on_press(|s: &mut State| s.open_popover(Popover::Actions { anchor: "d-more".into() })),
    );
    if let Some(Page::Playlist(uid, kind)) = d.page {
        let upload = kit::btn_soft("d-upload", S::UploadTrack.get(lang), Some(&icons::UPLOAD)).on_press_tx(move |_s: &mut State, tx: &mut Tx| {
            let opts = FileDialogOptions { title: S::UploadTrack.get(lang).to_owned(), ..Default::default() }
                .filter(S::AudioFiles.get(lang), &["mp3", "flac", "m4a", "wav", "ogg"]);
            tx.open_file(opts, move |s: &mut State, res, _tx: &mut Tx| {
                if let Ok(Some(path)) = res {
                    s.upload_playlist_files(uid, kind, vec![path]);
                }
            });
        });
        bar = bar.child(el().grow(1.0)).child(upload);
    }
    bar
}

fn head_button_bones(s: &State) -> El {
    let d = &s.detail;
    let mut bar = row()
        .key("d-buttons-bone")
        .w_full()
        .gap(8.0)
        .items_center()
        .flex_wrap()
        .child(kit::button_bone("d-play-bone", kit::button_width(S::Play.get(s.lang), true)))
        .child(kit::button_bone("d-shuffle-bone", kit::BTN_H));
    if matches!(d.page, Some(Page::Artist(_) | Page::Album(_))) {
        bar = bar.child(kit::button_bone("d-wave-bone", kit::BTN_H));
    }
    bar.child(kit::button_bone("d-more-bone", kit::BTN_H))
}

pub(super) fn detail(s: &State, w: f32) -> El {
    let p: Pal = s.pal();
    let lang = s.lang;
    let d = &s.detail;
    let key = s.page.key();
    let kind = match s.page {
        Page::Album(_) => crate::app::release_kind_name(d.release_kind.as_deref()),
        Page::Playlist(..) => S::Playlist,
        _ => S::Artist,
    };
    let all_mode = d.is_artist && d.showing_all;
    let mut body = col().gap(40.0);

    let failed = matches!(d.state, Load::Failed(_));
    let coming = !failed && !matches!(d.state, Load::Ready(_));
    if !all_mode && !failed {
        let cw = column(w);
        let wide = cw >= 700.0;
        let side = if wide { HEAD_SIDE } else { HEAD_SIDE_S };
        if let Some(b) = head_backdrop(s, w, side) {
            body = body.child(b);
        }
        let title_style = if wide { th::RELEASE } else { th::RELEASE_S };
        let name = if coming && d.title.is_empty() {
            kit::text_bone("d-title-bone", title_style.line_height, 26.0, 320.0)
        } else {
            label(d.title.clone(), title_style, th::text()).max_lines(2).w_full()
        };
        let mut meta = col().key("d-meta").min_w(0.0).child(name);
        let by_w = if wide { (cw - side - HEAD_GAP).max(180.0) } else { cw.max(180.0) };
        let has_by = !d.artists.is_empty() || !d.subtitle.is_empty();
        if !d.artists.is_empty() {
            meta = meta.child(el().key("d-by").w_full().min_w(0.0).mt(HEAD_STEP_BY).child(kit::artists_row_in_width(
                s,
                &d.artists,
                th::BYLINE_M,
                true,
                "d-art",
                th::text().alpha(0.9),
                by_w,
                true,
            )));
        } else if !d.subtitle.is_empty() {
            meta = meta.child(label(d.subtitle.clone(), th::BYLINE_M, th::text2()).truncate().min_w(0.0).mt(HEAD_STEP_BY));
        }
        if !coming {
            let facts = head_facts(s, kind);
            if !facts.is_empty() {
                let (style, color, gap) = if d.is_artist { (th::BODY, th::text2(), 6.0) } else { (th::SMALL, th::text3(), if has_by { 4.0 } else { 6.0 }) };
                meta = meta.child(label(facts, style, color).truncate().min_w(0.0).mt(gap));
            }
        } else {
            meta = meta.child(kit::text_bone("d-by-bone", th::BYLINE_M.line_height, 12.0, 200.0).mt(6.0));
        }
        if coming {
            meta = meta.child(head_button_bones(s).mt(HEAD_STEP_BUTTONS));
        } else if !d.tracks.is_empty() || d.like_key.is_some() || !d.facts.is_empty() {
            meta = meta.child(head_buttons(s).mt(HEAD_STEP_BUTTONS));
        }
        let head = if wide {
            row()
                .key("detail-head")
                .w_full()
                .mt(8.0)
                .gap(HEAD_GAP)
                .items_center()
                .transition(Transition::none())
                .child(head_picture(s, side))
                .child(kit::rise(meta.grow(1.0).min_w(0.0), 1))
        } else {
            col().key("detail-head").w_full().mt(8.0).gap(20.0).transition(Transition::none()).child(head_picture(s, side)).child(kit::rise(meta.w_full().min_w(0.0), 1))
        };
        body = body.child(head);
    }

    match &d.state {
        Load::Ready(()) => {
            let (_, card) = fit(w, 144.0, GRID_GAP, 8);
            let card = card.min(196.0);
            let is_playlist = matches!(d.page, Some(Page::Playlist(..)));
            if !d.tracks.is_empty() {
                let q = d.filter.text.trim().to_lowercase();
                let shown_count = if is_playlist && !q.is_empty() {
                    d.tracks.iter().filter(|t| s.detail_track_matches(t, &q)).count()
                } else {
                    d.tracks.len()
                };
                let list = if shown_count == 0 {
                    kit::empty(&p, Ill::Lens, S::NoResults.get(lang))
                } else {
                    track_list("detail-rows", shown_count, ListSrc::Detail)
                };
                let section = if all_mode {
                    let count = match &d.all_tracks {
                        Load::Ready(v) => format!("{} · {}", d.title, lang.tracks(v.len())),
                        _ => d.title.clone(),
                    };
                    let heading = row()
                        .key("d-tracks-h")
                        .w_full()
                        .h(32.0)
                        .gap(12.0)
                        .items_center()
                        .child(label(S::AllTracks.get(lang), th::SECTION, th::text()))
                        .child(label(count, th::BODY, th::text3()).truncate().min_w(0.0))
                        .child(el().grow(1.0))
                        .child(track_sort_button(s))
                        .child(play_button(s, "d-all-play", d.page.clone(), true, |s| {
                            let list = s.detail.tracks.clone();
                            s.play_all(list, false);
                        }))
                        .child(kit::round("d-all-shuffle", kit::Weight::Soft, &icons::SHUFFLE, 32.0, 16.0, S::Shuffle.get(lang)).on_press(|s: &mut State| {
                            let list = s.detail.tracks.clone();
                            s.play_all(list, true);
                        }));
                    col().key("d-tracks").mt(16.0).gap(10.0).child(heading).child(list)
                } else if is_playlist {
                    let input = text_input(&d.filter, th::BODY)
                        .key("pl-search")
                        .single_line()
                        .grow(1.0)
                        .min_w(0.0)
                        .color(th::text())
                        .placeholder(S::SearchTrack.get(lang), th::text3())
                        .caret(p.accent, p.accent.alpha(0.30))
                        .on_edit(|s: &mut State| &mut s.detail.filter);
                    let mut field = kit::field_surface(row())
                        .key("pl-search-field")
                        .grow(1.0)
                        .min_w(0.0)
                        .h(38.0)
                        .pl(12.0)
                        .pr(6.0)
                        .gap(8.0)
                        .radius(19.0)
                        .items_center()
                        .child(icon(&icons::SEARCH, 16.0).color(th::text3()))
                        .child(input);
                    if !q.is_empty() {
                        field = field.child(
                            kit::ibtn("pl-search-clear", &icons::CLOSE, 14.0, 28.0, th::text2(), S::ClearSearch.get(lang)).on_press(|s: &mut State| s.detail.filter.clear()),
                        );
                    }
                    let mut search_row = row().key("pl-search-row").w_full().gap(10.0).items_center().child(field);
                    if d.tracks.len() > 2 {
                        search_row = search_row.child(track_sort_button(s));
                    }
                    col().key("d-tracks").gap(14.0).child(search_row).child(list)
                } else {
                    let title = if d.is_artist { S::PopularTracks } else { S::Tracks };
                    let all = d.is_artist.then(|| {
                        let mut m = kit::more_link("d-all", S::All.get(lang)).on_press_tx(|s: &mut State, tx: &mut Tx| {
                            s.show_all_artist_tracks(true);
                            tx.scroll_to_y(s.page.key(), 0.0);
                        });
                        if d.all_tracks.is_loading() {
                            m = m.child(icon(&icons::SPINNER, 14.0).color(th::text3()).looping(rune::LoopSpec::rotation(0.9)));
                        }
                        m
                    });
                    let sort = (d.tracks.len() > 2 && !d.is_artist).then(|| track_sort_button(s));
                    let more = heading_side(sort, all);
                    col().key("d-tracks").gap(10.0).child(kit::list_heading("d-tracks-h", title.get(lang), more)).child(list)
                };
                body = body.child(kit::rise(section, 2));
            }
            if !all_mode && let Some(f) = detail_facts(s) {
                body = body.child(kit::rise(f, 3));
            }
            if !all_mode && !d.albums.is_empty() {
                let shown = s.shown_albums();
                let open = s.popover_open && matches!(s.popover, Some(Popover::DiscSort { .. }));
                let changed = d.disc_sort != DiscSort::default() || d.disc_kind != DiscKind::default();
                let now = changed.then(|| if d.disc_kind != DiscKind::default() { d.disc_kind.name() } else { d.disc_sort.name() }.get(lang));
                let sort = kit::sort_button("d-disc-sort", s, now, open).on_press(|s: &mut State| s.open_popover(Popover::DiscSort { anchor: "d-disc-sort".into() }));
                let row_key = format!("detail-albums-{:?}-{:?}", d.disc_sort, d.disc_kind);
                let arrows = row_arrows(s, &row_key, shown.len(), card, w);
                let releases = if shown.is_empty() {
                    row().key("d-albums-none").h(card * 0.5).items_center().pl(4.0).child(label(S::NoneOfKind.get(lang), th::BODY, th::text3()))
                } else {
                    hscroll(row_key, shown.iter().map(|a| album_card(s, a, card)).collect())
                };
                body = body.child(
                    col()
                        .key("d-albums")
                        .gap(14.0)
                        .transition(Transition::asymmetric(Transition::offset(0.0, 10.0), Transition::none()).springs(th::RISE))
                        .child(kit::heading("d-albums-h", S::Discography.get(lang), heading_side(arrows, Some(sort))))
                        .child(releases),
                );
            }
            if !all_mode && !d.similar.is_empty() {
                let ar_size = (card * 0.86).floor();
                body = body.child(
                    col()
                        .key("d-similar")
                        .gap(14.0)
                        .transition(Transition::asymmetric(Transition::offset(0.0, 10.0), Transition::none()).springs(th::RISE))
                        .child(kit::heading("d-similar-h", S::SimilarArtists.get(lang), row_arrows(s, "detail-similar", d.similar.len(), ar_size, w)))
                        .child(hscroll("detail-similar".into(), d.similar.iter().map(|a| artist_card(s, a, ar_size)).collect())),
                );
            }
            if d.tracks.is_empty() && d.albums.is_empty() {
                body = body.child(kit::empty(&p, Ill::Shelf, S::Nothing.get(lang)));
            }
        }
        Load::Failed(_) => {
            if let Some(e) = kit::list_state(&d.state, s) {
                body = body.child(e);
            }
        }
        _ => {}
    }
    let page_el = kit::scroll_page_live(key, body);
    if let Page::Playlist(uid, kind) = s.page {
        page_el.on_drop_files(move |s: &mut State, paths: &[std::path::PathBuf], _tx: &mut Tx| {
            s.upload_playlist_files(uid, kind, paths.to_vec());
        })
    } else {
        page_el
    }
}

fn track_sort_button(s: &State) -> El {
    let d = &s.detail;
    let open = s.popover_open && matches!(s.popover, Some(Popover::TrackSort { .. }));
    let now = (d.sort != TrackSort::Original).then(|| d.sort.name().get(s.lang));
    kit::sort_button("d-sort", s, now, open).on_press(|s: &mut State| s.open_popover(Popover::TrackSort { anchor: "d-sort".into() }))
}

fn format_listeners(n: u64, lang: Lang) -> String {
    format!("{} {}", crate::i18n::group_digits(n), S::ListenersPerMonth.get(lang))
}
