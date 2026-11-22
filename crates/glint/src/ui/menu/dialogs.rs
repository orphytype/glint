//! Confirmation and text-input dialogs (batch downloads, download deletion, playlist naming/deletion).

use rune::{Cursor, El, icon, row};

use crate::app::State;
use crate::i18n::S;
use crate::store::QualityPref;
use crate::theme as th;
use crate::ui::{
    icons, np_icons,
    kit::{self, Hover as _, label},
};

pub fn download_confirm(s: &State, w: f32, _h: f32) -> El {
    let count = s.download_confirm.as_ref().map_or(0, Vec::len);
    let lang = s.lang;
    kit::dialog(
        "download-confirm",
        w,
        S::DownloadConfirm.get(lang),
        vec![label(lang.download_confirm_note(count), th::BODY, th::text2()).wrap(), label(S::DownloadQuality.get(lang), th::BODY_M, th::text2()), download_quality_choice(s)],
        vec![
            kit::btn_soft("download-cancel", S::Cancel.get(lang), None).on_press(|s: &mut State| s.cancel_download()),
            kit::btn_primary("download-accept", S::DownloadAll.get(lang), Some(&icons::DOWNLOAD)).on_press(|s: &mut State| s.confirm_download()),
        ],
        State::cancel_download,
    )
}

/// Quality for this batch only. Playback quality stays as the listener set it.
fn download_quality_choice(s: &State) -> El {
    let lang = s.lang;
    let mut choice = row().key("download-quality").padding(3.0).gap(2.0).radius(9.0).fill(th::veil(0.05)).border(1.0, th::veil(0.08)).items_center();
    for quality in [QualityPref::Saver, QualityPref::Balanced, QualityPref::Best] {
        let on = s.download_quality == quality;
        let text = np_icons::quality_label(quality).get(lang);
        let ink = if on { th::text() } else { th::text2() };
        let mut cell = row()
            .key(format!("download-q-{quality:?}"))
            .role("radio")
            .label(text)
            .checked(on)
            .h(30.0)
            .px(10.0)
            .gap(6.0)
            .radius(7.0)
            .items_center()
            .cursor(Cursor::Pointer)
            .hov(th::veil(if on { 0.0 } else { 0.05 }))
            .press(th::veil(0.10))
            .on_press(move |s: &mut State| s.download_quality = quality)
            .child(icon(np_icons::quality_icon(quality), 16.0).color(ink))
            .child(label(text, th::BODY_M, ink));
        if on {
            cell = cell.fill(if th::light() { th::pop() } else { th::veil(0.14) });
        }
        choice = choice.child(cell);
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
        .caret(p.accent, p.accent.alpha(0.30))
        .on_edit(|s: &mut State| &mut s.name_edit)
        .on_submit(|s: &mut State, _tx: &mut rune::Tx| s.submit_name());
    let empty = s.name_edit.text.trim().is_empty();
    kit::dialog(
        "name-dialog",
        w,
        title.get(lang),
        vec![kit::field_surface(row().key("name-field-box").w_full().h(kit::BTN_H).px(14.0).items_center()).child(field)],
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
