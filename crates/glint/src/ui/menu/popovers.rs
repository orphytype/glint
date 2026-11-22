//! List popovers: liked/detail actions, pinned items, download manager, waves, sort, quality, and EQ presets.

use std::borrow::Cow;
use std::collections::HashSet;
use std::rc::Rc;

use rune::{Cursor, El, Transition, col, el, icon, row};

use super::{Entry, HEAD};
use crate::app::{DiscKind, DiscSort, DownloadState, Page, PinnedItem, Sheet, Source, State, Station, TrackSort, WaveChoice};
use crate::i18n::S;
use crate::store::QualityPref;
use crate::theme as th;
use crate::ui::{
    icons,
    kit::{self, Hover as _, ListSrc, label, menu_row},
    np_icons, settings,
};

const DOWNLOAD_ROW_H: f32 = 44.0;

pub(super) fn liked_entries(s: &State) -> Vec<Entry> {
    let lang = s.lang;
    let tracks = s.library.tracks.get().map_or(&[][..], Vec::as_slice);
    let mut out: Vec<Entry> = Vec::new();
    if !tracks.is_empty() {
        out.push(Entry::Row(menu_row("la-next", &icons::QUEUE_NEXT, S::PlayAllNext.get(lang), false).on_press(|s: &mut State| {
            s.close_popover();
            let list = s.list(ListSrc::Liked);
            s.queue_many(list, true);
        })));
        out.push(Entry::Row(menu_row("la-queue", &icons::QUEUE_ADD, S::AddToQueue.get(lang), false).on_press(|s: &mut State| {
            s.close_popover();
            let list = s.list(ListSrc::Liked);
            s.queue_many(list, false);
        })));
        let settled = s.downloads_settled(tracks);
        let any_kept = tracks.iter().any(|t| matches!(s.downloads.get(&t.id), Some(DownloadState::Done)));
        out.push(Entry::Divider);
        if !settled {
            out.push(Entry::Row(menu_row("la-download", &icons::DOWNLOAD, S::DownloadAllLiked.get(lang), false).on_press(|s: &mut State| {
                s.close_popover();
                let list = s.library.tracks.get().cloned().unwrap_or_default();
                s.ask_download(list);
            })));
        }
        if settled || any_kept {
            out.push(Entry::Row(menu_row("la-undownload", &icons::TRASH, S::DeleteDownload.get(lang), false).on_press(|s: &mut State| {
                s.close_popover();
                let list: Vec<_> =
                    s.library.tracks.get().map_or(&[][..], Vec::as_slice).iter().filter(|t| matches!(s.downloads.get(&t.id), Some(DownloadState::Done))).cloned().collect();
                s.ask_delete_downloads(list);
            })));
        }
    }
    if let Some(link) = s.liked_link() {
        if !out.is_empty() {
            out.push(Entry::Divider);
        }
        out.push(Entry::Row(menu_row("la-share", &icons::COPY, S::CopyLink.get(lang), false).on_press_tx(move |s: &mut State, tx: &mut rune::Tx| {
            s.close_popover();
            tx.copy(link.clone());
            let text = s.tr(S::LinkCopied);
            s.toast(text);
        })));
    }
    out
}

pub(super) fn action_entries(s: &State) -> Vec<Entry> {
    let lang = s.lang;
    let d = &s.detail;
    let mut out: Vec<Entry> = Vec::new();
    let group = |out: &mut Vec<Entry>| {
        if !out.is_empty() && !matches!(out.last(), Some(Entry::Divider)) {
            out.push(Entry::Divider);
        }
    };
    if !d.tracks.is_empty() {
        out.push(Entry::Row(menu_row("da-next", &icons::QUEUE_NEXT, S::PlayAllNext.get(lang), false).on_press(|s: &mut State| {
            s.close_popover();
            let list = s.detail.tracks.clone();
            s.queue_many(list, true);
        })));
        out.push(Entry::Row(menu_row("da-queue", &icons::QUEUE_ADD, S::AddToQueue.get(lang), false).on_press(|s: &mut State| {
            s.close_popover();
            let list = s.detail.tracks.clone();
            s.queue_many(list, false);
        })));
    }
    if d.like_key.is_some() || d.page.is_some() || !d.tracks.is_empty() || d.artist.is_some() {
        group(&mut out);
    }
    if let Some(key) = &d.like_key {
        let liked = s.is_item_liked(key);
        let (ic, name) = if liked { (&icons::HEART_FILL, S::RemoveFromCollection) } else { (&icons::HEART, S::AddToCollection) };
        out.push(Entry::Row(menu_row("da-like", ic, name.get(lang), false).on_press(|s: &mut State| {
            s.close_popover();
            s.toggle_detail_like();
        })));
    }
    if d.page.is_some() {
        let pin_on = s.detail_is_pinned();
        let (ic, name) = if pin_on { (&icons::PIN_OFF, S::Pinned) } else { (&icons::PIN, S::Pin) };
        out.push(Entry::Row(menu_row("da-pin", ic, name.get(lang), pin_on).on_press(|s: &mut State| {
            s.close_popover();
            s.toggle_detail_pin();
        })));
    }
    if !d.tracks.is_empty() {
        let download_tracks: &[ym_api::Track] = if d.is_artist {
            match &d.all_tracks {
                crate::app::Load::Ready(tracks) => tracks.as_slice(),
                _ => &[],
            }
        } else {
            &d.tracks
        };
        let downloaded = s.downloads_settled(download_tracks);
        let (ic, name) = if downloaded { (&icons::TRASH, S::DeleteDownload) } else { (&icons::DOWNLOAD, S::DownloadAll) };
        out.push(Entry::Row(menu_row("da-download", ic, name.get(lang), false).on_press(move |s: &mut State| {
            s.close_popover();
            if downloaded {
                s.delete_detail_downloads();
            } else {
                s.download_detail();
            }
        })));
    }
    if let Some(id) = d.artist.clone() {
        out.push(Entry::Row(menu_row("da-artist", &icons::USER, S::GoToArtist.get(lang), false).on_press(move |s: &mut State| {
            s.close_popover();
            s.go(Page::Artist(id.clone()));
        })));
    }
    if let Some(link) = d.link.clone() {
        group(&mut out);
        out.push(Entry::Row(menu_row("da-share", &icons::COPY, S::CopyLink.get(lang), false).on_press_tx(move |s: &mut State, tx: &mut rune::Tx| {
            s.close_popover();
            tx.copy(link.clone());
            let text = s.tr(S::LinkCopied);
            s.toast(text);
        })));
    }
    if let Some((uid, kind)) = s.open_own_playlist() {
        group(&mut out);
        out.push(Entry::Row(menu_row("da-rename", &icons::EDIT, S::Rename.get(lang), false).on_press_tx(move |s: &mut State, tx: &mut rune::Tx| {
            s.ask_rename_playlist(uid, kind);
            tx.focus_when_shown("name-field", false);
        })));
        out.push(Entry::Row(menu_row("da-delete", &icons::TRASH, S::DeletePlaylist.get(lang), false).on_press(move |s: &mut State| s.ask_delete_playlist(uid, kind))));
    }
    let artist_info = d.is_artist && (!d.links.is_empty() || d.donation.is_some());
    if !d.facts.is_empty() || artist_info {
        group(&mut out);
        let name = if d.is_artist { S::AboutArtist } else { S::Details };
        out.push(Entry::Row(menu_row("da-about", &icons::INFO, name.get(lang), false).on_press(|s: &mut State| s.open_sheet(Sheet::Page))));
    }
    if let Some(url) = d.donation.clone().filter(|_| d.is_artist) {
        out.push(Entry::Row(menu_row("da-donate", &icons::HEART, S::SupportArtist.get(lang), false).on_press_tx(move |s: &mut State, tx: &mut rune::Tx| {
            s.close_popover();
            let _ = tx.open_url(&url);
        })));
    }
    out
}

pub(super) fn pinned_entries(s: &State, item: &PinnedItem) -> Vec<Entry> {
    let lang = s.lang;
    let page = item.page.clone();
    let key = item.key();
    vec![
        Entry::Row(menu_row("pinned-open", &icons::CHEVRON_RIGHT, S::Open.get(lang), false).on_press(move |s: &mut State| {
            s.close_popover();
            s.go(page.clone());
        })),
        Entry::Row(menu_row("pinned-remove", &icons::TRASH, S::Unpinned.get(lang), false).on_press(move |s: &mut State| s.remove_pinned(&key))),
    ]
}

pub(super) fn download_entries(s: &State) -> Vec<Entry> {
    if s.downloads.is_empty() {
        return vec![download_head(s), Entry::Block(label(S::Nothing.get(s.lang), th::BODY, th::text2()).px(12.0).py(10.0), 42.0)];
    }
    let mut seen: HashSet<&str> = HashSet::with_capacity(s.downloads.len());
    let mut ids: Vec<String> = Vec::with_capacity(s.downloads.len());
    for id in &s.download_order {
        if s.downloads.contains_key(id) && seen.insert(id.as_str()) {
            ids.push(id.clone());
        }
    }
    for id in s.downloads.keys() {
        if seen.insert(id.as_str()) {
            ids.push(id.clone());
        }
    }
    const LIST_H: f32 = 220.0;
    let total_h = ids.len() as f32 * DOWNLOAD_ROW_H;
    let list_h = total_h.min(LIST_H);
    let ids: Rc<[String]> = ids.into();
    let row_ids = Rc::clone(&ids);
    let rows = col().key("download-manager-list").w_full().growth(rune::GrowthSpec::default()).virtual_rows(
        ids.len(),
        DOWNLOAD_ROW_H,
        move |_: &State, i| Cow::Owned(row_ids[i].clone()),
        move |s: &State, i| download_row(s, &ids[i]),
    );
    let scroll = col().key("download-manager-scroll").w_full().h(list_h).scroll(rune::ScrollSpec { stick_bottom: false }).child(rows);
    vec![download_head(s), Entry::Block(scroll, list_h)]
}

fn download_row(s: &State, id: &str) -> El {
    let Some(state) = s.downloads.get(id).copied() else { return el().h(DOWNLOAD_ROW_H) };
    let title = s.download_titles.get(id).cloned().unwrap_or_else(|| id.to_owned());
    let downloaded = matches!(state, DownloadState::Done).then(|| s.downloaded_tracks.get(id)).flatten();
    let status = match state {
        DownloadState::Downloading => S::DownloadInProgress,
        DownloadState::Done => S::DownloadDone,
        DownloadState::Failed => S::DownloadFailed,
    };
    let why = matches!(state, DownloadState::Failed).then(|| s.download_why(id)).flatten();
    let status_text = match why {
        Some(why) => format!("{} · {why}", status.get(s.lang)),
        None => status.get(s.lang).to_owned(),
    };
    let glyph_icon = match state {
        DownloadState::Downloading => &icons::SPINNER,
        DownloadState::Failed if s.download_errors.get(id).is_some_and(|f| f.lasting()) => &icons::ALERT,
        DownloadState::Done | DownloadState::Failed => &icons::DOWNLOAD,
    };
    let color = match state {
        DownloadState::Downloading => th::text2(),
        DownloadState::Done => th::rgb(0x4bd27e),
        DownloadState::Failed => th::rgb(0xf18a8a),
    };
    let glyph =
        if matches!(state, DownloadState::Downloading) { icon(glyph_icon, 12.0).color(color).looping(rune::LoopSpec::rotation(0.9)) } else { icon(glyph_icon, 12.0).color(color) };
    let mut row = row().key(format!("download-manager-{id}")).role("button").label(title.as_str()).w_full().h(DOWNLOAD_ROW_H).px(10.0).gap(10.0).items_center();
    if let Some(track) = downloaded {
        let open_track = track.clone();
        let destination = track.albums.first().map(|album| Page::Album(album.id.clone())).or_else(|| track.artists.first().map(|artist| Page::Artist(artist.id.clone())));
        row = row
            .cursor(Cursor::Pointer)
            .hov(th::veil(0.05))
            .on_press(move |s: &mut State| {
                s.close_popover();
                if let Some(page) = destination.clone() {
                    s.go(page);
                } else {
                    s.open_sheet(Sheet::Track(Box::new(open_track.clone())));
                }
            })
            .child(kit::cover_sm(s, track.cover_template(), 32.0, 8.0));
    }
    row = row.child(
        col()
            .grow(1.0)
            .min_w(0.0)
            .gap(1.0)
            .child(label(title, th::BODY_M, th::text()).truncate())
            .child(rune::row().gap(4.0).items_center().child(glyph).child(label(status_text, th::SMALL, th::text3()).truncate().min_w(0.0)).min_w(0.0)),
    );
    if let Some(track) = downloaded {
        let track = track.clone();
        row = row.child(
            kit::ibtn(format!("download-delete-{id}"), &icons::TRASH, 17.0, 30.0, th::text3(), S::DeleteDownload.get(s.lang))
                .on_press(move |s: &mut State| s.ask_delete_download(track.clone())),
        );
    }
    row
}

fn download_head(s: &State) -> Entry {
    let folder = kit::ibtn("download-folder", &icons::FOLDER, 17.0, 30.0, th::text2(), S::OpenDownloads.get(s.lang)).on_press(|s: &mut State| s.open_download_folder());
    let clear = kit::ibtn("download-clear", &icons::TRASH, 17.0, 30.0, th::text2(), S::ClearDownloadHistory.get(s.lang)).on_press(|s: &mut State| s.clear_download_history());
    Entry::Head(
        row().h(HEAD).w_full().px(10.0).items_center().child(label(S::DownloadManager.get(s.lang), th::SECTION, th::text())).child(el().grow(1.0)).child(folder).child(clear),
    )
}

pub(super) fn wave_entries(s: &State, choices: &[WaveChoice]) -> Vec<Entry> {
    choices
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let station = c.station.clone();
            let wname = c.name.clone();
            let on = matches!(&s.player.source, Source::Wave { preset, .. } if station.is(preset)) && s.current().is_some();
            let ic = match station {
                Station::Track(_) => &icons::NOTE,
                Station::Artist(_) => &icons::USER,
                Station::Album(_) => &icons::DISC,
                Station::Collection => &icons::HEART,
            };
            Entry::Row(
                row()
                    .key(format!("pw-{i}"))
                    .role("button")
                    .label(c.kind.as_str())
                    .checked(on)
                    .w_full()
                    .h(kit::MENU_ROW)
                    .px(10.0)
                    .gap(10.0)
                    .items_center()
                    .radius(8.0)
                    .cursor(Cursor::Pointer)
                    .transition(Transition::none())
                    .fill(if on { th::veil(0.08) } else { rune::Color::TRANSPARENT })
                    .hov(th::veil(0.06))
                    .press(th::veil(0.10))
                    .on_press(move |s: &mut State| {
                        s.close_popover();
                        s.start_station(&station, &wname);
                    })
                    .child(icon(ic, 18.0).color(if on { th::text() } else { th::text2() }))
                    .child(label(c.kind.clone(), th::BODY, th::text()))
                    .child(el().grow(1.0))
                    .child(label(c.name.clone(), th::SMALL, th::text2()).truncate().max_w(150.0)),
            )
        })
        .collect()
}

pub(super) fn eq_entries(s: &State) -> Vec<Entry> {
    let current = s.player.eq.preset_name();
    ym_audio::PRESETS
        .iter()
        .map(|&(name, gains)| {
            let text = settings::preset_label(name, s.lang);
            choice_row(format!("eq-preset-{name}"), &text, current == Some(name), move |s: &mut State| {
                s.player.eq.gains_db = gains;
                s.apply_eq();
            })
        })
        .collect()
}

fn choice_row(key: String, text: &str, on: bool, pick: impl Fn(&mut State) + 'static) -> Entry {
    Entry::Row(
        row()
            .key(key)
            .role("radio")
            .label(text)
            .checked(on)
            .w_full()
            .h(kit::MENU_ROW)
            .pl(10.0)
            .pr(8.0)
            .gap(10.0)
            .items_center()
            .radius(8.0)
            .cursor(Cursor::Pointer)
            .hov(th::veil(0.06))
            .press(th::veil(0.10))
            .on_press(move |s: &mut State| {
                pick(s);
                s.close_popover();
            })
            .child(label(text.to_owned(), th::BODY, if on { th::text() } else { th::text2() }).grow(1.0).min_w(0.0).truncate())
            .children(on.then(|| icon(&icons::CHECK, 15.0).color(th::text()))),
    )
}

fn choice_caption(key: &str, text: &str) -> Entry {
    Entry::Block(row().key(key.to_owned()).w_full().h(28.0).px(10.0).items_end().pb(4.0).child(kit::caps(text, th::text3())), 28.0)
}

pub(super) fn track_sort_entries(s: &State) -> Vec<Entry> {
    let now = s.detail.sort;
    TrackSort::ALL
        .into_iter()
        .filter(|o| *o != TrackSort::Album || !matches!(s.detail.page, Some(Page::Album(_))))
        .map(|o| choice_row(format!("sort-{o:?}"), o.name().get(s.lang), o == now, move |s: &mut State| s.set_track_sort(o)))
        .collect()
}

pub(super) fn lib_sort_entries(s: &State) -> Vec<Entry> {
    let now = s.library.sort;
    TrackSort::ALL.into_iter().map(|o| choice_row(format!("lsort-{o:?}"), o.name().get(s.lang), o == now, move |s: &mut State| s.set_library_sort(o))).collect()
}

pub(super) fn disc_sort_entries(s: &State) -> Vec<Entry> {
    let lang = s.lang;
    let (now, kind) = (s.detail.disc_sort, s.detail.disc_kind);
    let mut v = vec![choice_caption("disc-sort-cap", S::SortBy.get(lang))];
    v.extend(DiscSort::ALL.into_iter().map(|o| choice_row(format!("disc-{o:?}"), o.name().get(lang), o == now, move |s: &mut State| s.detail.disc_sort = o)));
    v.push(Entry::Divider);
    v.push(choice_caption("disc-kind-cap", S::ShowKinds.get(lang)));
    let albums = &s.detail.albums;
    v.extend(
        DiscKind::ALL
            .into_iter()
            .filter(|k| *k == DiscKind::All || *k == kind || albums.iter().any(|a| k.holds(a.kind.as_deref())))
            .map(|k| choice_row(format!("disc-kind-{k:?}"), k.name().get(lang), k == kind, move |s: &mut State| s.detail.disc_kind = k)),
    );
    v
}

pub(super) fn quality_entries(s: &State) -> Vec<Entry> {
    let lang = s.lang;
    let max = crate::ui::dock::current_track_max_quality(s);
    [QualityPref::Saver, QualityPref::Balanced, QualityPref::Best]
        .into_iter()
        .map(|quality| {
            let text = np_icons::quality_label(quality).get(lang);
            let selected = s.settings.quality == quality;
            let ink = if selected { th::text() } else { th::text2() };
            let detail = crate::ui::dock::quality_rate_text(quality, max, lang, false);
            let check = row().w(16.0).h_full().center().children(selected.then(|| icon(&icons::CHECK, 15.0).color(th::text())));
            Entry::Row(
                row()
                    .key(format!("quality-{quality:?}"))
                    .role("radio")
                    .label(text)
                    .checked(selected)
                    .w_full()
                    .h(kit::MENU_ROW)
                    .pl(10.0)
                    .pr(8.0)
                    .gap(8.0)
                    .items_center()
                    .radius(8.0)
                    .cursor(Cursor::Pointer)
                    .fill(if selected { th::veil(0.08) } else { rune::Color::TRANSPARENT })
                    .hov(th::veil(0.06))
                    .press(th::veil(0.10))
                    .on_press(move |s: &mut State| {
                        s.set_quality(quality);
                        s.close_popover();
                    })
                    .child(icon(np_icons::quality_icon(quality), 18.0).color(ink))
                    .child(label(text, th::BODY, ink).grow(1.0).min_w(0.0).truncate())
                    .child(label(detail, th::MONO, if selected { th::text2() } else { th::text3() }))
                    .child(check),
            )
        })
        .collect()
}

pub(super) const UPDATE_W: f32 = 312.0;

pub(super) fn update_entries(s: &State) -> Vec<Entry> {
    use crate::update::Ui;
    let lang = s.lang;
    let (release, status, progress): (Option<&glint_update::Release>, String, Option<f32>) = match &s.update {
        Ui::Available(r) => (Some(r), S::UpdateMenuSub.get(lang).to_owned(), None),
        Ui::Downloading { release, done, total } => {
            let frac = if *total > 0 { (*done as f32 / *total as f32).clamp(0.0, 1.0) } else { 0.0 };
            let pct = (frac * 100.0).round() as u32;
            (Some(release), format!("{} · {pct}%", S::UpdateDownloading.get(lang)), Some(frac))
        }
        Ui::Installing => (None, S::UpdateInstalling.get(lang).to_owned(), Some(1.0)),
        _ => return Vec::new(),
    };
    let ver = release.map_or_else(|| env!("CARGO_PKG_VERSION").to_owned(), |r| r.version.trim_start_matches('v').to_owned());
    let head_h = 56.0;
    let head =
        row().key("upd-head").w_full().h(head_h).px(10.0).gap(12.0).items_center().child(settings::app_mark(38.0)).child(
            col().grow(1.0).min_w(0.0).gap(2.0).child(label(format!("Glint {ver}"), th::BODY_M, th::text()).truncate()).child(label(status, th::SMALL, th::text2()).truncate()),
        );
    let mut out = vec![Entry::Block(head, head_h)];
    if let Some(r) = release
        && !r.notes.is_empty()
    {
        let text_w = UPDATE_W - 2.0 * super::PAD - 20.0;
        let max_h = th::SMALL.line_height * 5.0;
        let measured = rune::text::measure(&r.notes, &[], th::SMALL, Some(text_w)).1.ceil().min(max_h);
        let block_h = measured + 10.0;
        let notes =
            col().key("upd-notes").w_full().h(block_h).px(10.0).pb(10.0).child(label(r.notes.clone(), th::SMALL, th::text2()).wrap().max_lines(5).align(rune::Align::Start));
        out.push(Entry::Block(notes, block_h));
    }
    if let Some(frac) = progress {
        let bar_h = 14.0;
        let track_w = UPDATE_W - 2.0 * super::PAD - 20.0;
        let fill_w = (track_w * frac).max(if frac > 0.0 { 4.0 } else { 0.0 });
        let bar = row()
            .key("upd-progress")
            .w_full()
            .h(bar_h)
            .px(10.0)
            .pb(8.0)
            .items_center()
            .child(el().w_full().h(4.0).radius(2.0).fill(th::veil(0.12)).child(el().w(fill_w).h(4.0).radius(2.0).fill(s.pal().accent)));
        out.push(Entry::Block(bar, bar_h));
    }
    if let Some(r) = release {
        let can_install = matches!(s.update, Ui::Available(_)) && s.update_method.can_install() && r.asset.is_some();
        out.push(Entry::Divider);
        if can_install {
            out.push(Entry::Row(menu_row("upd-install", &icons::DOWNLOAD, S::UpdateInstall.get(lang), false).on_press(|s: &mut State| {
                s.close_popover();
                s.install_update();
            })));
        }
        let url = r.page_url.clone();
        out.push(Entry::Row(menu_row("upd-page", &icons::EXTERNAL, S::UpdateReleasePage.get(lang), false).on_press_tx(move |s: &mut State, tx: &mut rune::Tx| {
            s.close_popover();
            let _ = tx.open_url(&url);
        })));
    }
    out
}
