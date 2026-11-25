//! Confirmation and text-input dialogs (batch downloads, download deletion, playlist naming/deletion).

use rune::{Cursor, El, col, el, icon, row};

use crate::app::State;
use crate::i18n::S;
use crate::store::QualityPref;
use crate::theme as th;
use crate::ui::{
    icons, np_icons,
    kit::{self, Hover as _, label},
};

pub fn download_confirm(s: &State, w: f32, _h: f32) -> El {
    let tracks = s.download_confirm.as_deref().unwrap_or(&[]);
    let count = tracks.len();
    let total_secs: u64 = tracks.iter().map(|t| t.duration_ms.unwrap_or(195_000) / 1000).sum();
    let lang = s.lang;
    let accent = s.pal().accent;

    let summary_text = if total_secs > 0 {
        format!("{} · {}", lang.tracks(count), lang.duration(total_secs))
    } else {
        lang.tracks(count)
    };
    let summary_badge = row()
        .key("download-summary-pill")
        .h(26.0)
        .px(10.0)
        .gap(6.0)
        .radius(13.0)
        .items_center()
        .fill(accent.alpha(0.14))
        .border(1.0, accent.alpha(0.28))
        .child(icon(&icons::DOWNLOAD, 13.0).color(accent))
        .child(label(summary_text, th::SMALL, th::text()));

    let quality_section = col()
        .key("download-quality-section")
        .w_full()
        .mt(2.0)
        .gap(8.0)
        .child(label(S::DownloadQuality.get(lang), th::SMALL, th::text2()))
        .child(download_quality_choice(s, total_secs));

    kit::dialog_head(
        "download-confirm",
        w,
        S::DownloadConfirm.get(lang),
        Some(summary_badge),
        vec![quality_section],
        vec![
            kit::btn_soft("download-cancel", S::Cancel.get(lang), None).on_press(|s: &mut State| s.cancel_download()),
            kit::btn_primary("download-accept", S::DownloadAll.get(lang), Some(&icons::DOWNLOAD)).on_press(|s: &mut State| s.confirm_download()),
        ],
        State::cancel_download,
    )
}

/// Quality for this batch only. Playback quality stays as the listener set it.
fn download_quality_choice(s: &State, total_secs: u64) -> El {
    let lang = s.lang;
    let accent = s.pal().accent;
    let check_fg = if th::light() { th::rgb(0xffffff) } else { th::rgb(0x111113) };
    let mut choice = col().key("download-quality").w_full().gap(8.0);
    for (quality, desc, kbps) in [
        (QualityPref::Saver, S::QualitySaverDesc, 64_u64),
        (QualityPref::Balanced, S::QualityBalancedDesc, 192_u64),
        (QualityPref::Best, S::QualityBestDesc, 680_u64),
    ] {
        let on = s.download_quality == quality;
        let title = np_icons::quality_label(quality).get(lang);
        let approx_bytes = total_secs.saturating_mul(kbps * 1000 / 8);
        let size_badge = (total_secs > 0).then(|| {
            row()
                .h(22.0)
                .px(8.0)
                .radius(6.0)
                .items_center()
                .fill(if on { accent.alpha(0.18) } else { th::veil(0.06) })
                .child(label(lang.approx_size(approx_bytes), th::MONO, if on { th::text() } else { th::text2() }))
        });
        let badge = row()
            .w(36.0)
            .h(36.0)
            .radius(10.0)
            .center()
            .fill(if on { accent.alpha(0.20) } else { th::veil(0.06) })
            .child(icon(np_icons::quality_icon(quality), 18.0).color(if on { accent } else { th::text2() }));
        let radio = if on {
            row()
                .w(20.0)
                .h(20.0)
                .radius(10.0)
                .center()
                .fill(accent)
                .child(icon(&icons::CHECK, 12.0).color(check_fg))
        } else {
            el().w(20.0).h(20.0).radius(10.0).border(1.5, th::veil(0.20))
        };
        let title_row = row()
            .w_full()
            .gap(8.0)
            .items_center()
            .child(label(title, th::BODY_M, th::text()).grow(1.0).min_w(0.0).truncate())
            .children(size_badge);
        let text_col = col()
            .grow(1.0)
            .min_w(0.0)
            .gap(2.0)
            .child(title_row)
            .child(label(desc.get(lang), th::SMALL, if on { th::text2() } else { th::text3() }).truncate());

        let mut card = row()
            .key(format!("download-q-{quality:?}"))
            .role("radio")
            .label(title)
            .checked(on)
            .w_full()
            .px(12.0)
            .py(10.0)
            .gap(12.0)
            .radius(12.0)
            .items_center()
            .cursor(Cursor::Pointer)
            .on_press(move |s: &mut State| s.download_quality = quality)
            .child(badge)
            .child(text_col)
            .child(radio);
        card = if on {
            card.fill(accent.alpha(if th::light() { 0.11 } else { 0.13 })).border(1.5, accent.alpha(0.60))
        } else {
            card.fill(th::veil(0.035)).border(1.0, th::veil(0.075)).hov(th::veil(0.065)).press(th::veil(0.10))
        };
        choice = choice.child(card);
    }
    choice
}

pub fn delete_download_confirm(s: &State, w: f32, _h: f32) -> El {
    let lang = s.lang;
    let batch = s.delete_download_confirm.as_ref().is_some_and(|tracks| tracks.len() > 1);
    let title = if batch { S::DeleteDownloadsConfirm } else { S::DeleteDownloadConfirm };
    let note = if batch { S::DeleteDownloadsNote } else { S::DeleteDownloadNote };
    kit::dialog(
        "delete-download-confirm",
        w,
        title.get(lang),
        vec![label(note.get(lang), th::BODY, th::text2()).wrap()],
        vec![
            kit::btn_soft("delete-download-cancel", S::Cancel.get(lang), None).on_press(|s: &mut State| s.cancel_delete_download()),
            kit::btn_primary("delete-download-accept", S::DeleteDownload.get(lang), Some(&icons::TRASH)).on_press(|s: &mut State| s.confirm_delete_download()),
        ],
        State::cancel_delete_download,
    )
}

pub fn name_dialog(s: &State, w: f32) -> El {
    let lang = s.lang;
    let creating = matches!(s.name_dialog, Some(crate::playlists::NamePurpose::Create { .. }));
    let (title, action) = if creating { (S::NewPlaylist, S::Create) } else { (S::RenamePlaylist, S::Save) };
    let p = s.pal();
    let field = rune::text_input(&s.name_edit, th::BODY)
        .key("name-field")
        .single_line()
        .grow(1.0)
        .min_w(0.0)
        .label(S::PlaylistName.get(lang))
        .color(th::text())
        .placeholder(S::PlaylistName.get(lang), th::text3())
        .caret(s.caret_color(s.name_edit.focused, p.accent), p.accent.alpha(0.30))
        .on_edit(|s: &mut State| &mut s.name_edit)
        .on_submit(|s: &mut State, _tx: &mut rune::Tx| s.submit_name());
    let empty = s.name_edit.text.trim().is_empty();
    kit::dialog(
        "name-dialog",
        w,
        title.get(lang),
        vec![
            kit::field_surface(row().key("name-field-box").w_full().h(kit::BTN_H).px(14.0).items_center().clip_rounded(kit::FIELD_R))
                .child(row().grow(1.0).min_w(0.0).h_full().items_center().clip_rounded(0.0).child(field)),
        ],
        vec![
            kit::btn_soft("name-cancel", S::Cancel.get(lang), None).on_press(|s: &mut State| s.close_name_dialog()),
            kit::btn_primary("name-accept", action.get(lang), None).opacity(if empty { 0.45 } else { 1.0 }).on_press(|s: &mut State| s.submit_name()),
        ],
        State::close_name_dialog,
    )
}

pub fn delete_playlist_confirm(s: &State, w: f32) -> El {
    let lang = s.lang;
    kit::dialog(
        "delete-playlist",
        w,
        S::DeletePlaylist.get(lang),
        vec![label(S::DeletePlaylistNote.get(lang), th::BODY, th::text2()).wrap()],
        vec![
            kit::btn_soft("delete-playlist-cancel", S::Cancel.get(lang), None).on_press(|s: &mut State| s.delete_playlist_confirm = None),
            kit::btn_primary("delete-playlist-accept", S::DeletePlaylist.get(lang), Some(&icons::TRASH)).on_press(|s: &mut State| s.delete_playlist()),
        ],
        |s| s.delete_playlist_confirm = None,
    )
}
