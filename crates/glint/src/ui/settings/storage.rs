//! Storage usage, updates, and about tabs inside Settings.

use rune::{Align, Cursor, El, Icon, Tx, col, el, row};

use super::{GROUP_R, ROW_PAD, app_mark, danger_btn, figure, group, item, toggle};
use crate::app::State;
use crate::i18n::{Lang, S};
use crate::theme::{self as th, veil};
use crate::ui::{
    icons,
    kit::{self, Hover as _, label},
};

const TOTAL: rune::TextStyle = rune::TextStyle::new(30.0, 600.0).family(th::SANS).tracking(-0.6).lh(36.0).features("'tnum' 1");
const STORE_HUES: [u32; 3] = [0x5e5ce6, 0x30b158, 0xff9f0a];
const REPO: &str = "https://github.com/orphytype/glint";

fn size(bytes: u64, lang: Lang) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let ru = lang == Lang::Ru;
    let b = bytes as f64;
    let (n, unit) = if b >= GB {
        (format!("{:.1}", b / GB), if ru { "ГБ" } else { "GB" })
    } else if b >= MB {
        (format!("{:.1}", b / MB), if ru { "МБ" } else { "MB" })
    } else if b >= KB {
        (format!("{:.0}", b / KB), if ru { "КБ" } else { "KB" })
    } else {
        (bytes.to_string(), if ru { "Б" } else { "B" })
    };
    let n = if ru { n.replace('.', ",") } else { n };
    format!("{n}\u{a0}{unit}")
}

fn delete_content(s: &mut State) {
    if let Some(d) = &s.cache {
        let _ = d.clear();
    }
    s.art.clear_raw();
    s.wipe_downloads();
    s.disk_usage.set((0, 0, 0, f64::NEG_INFINITY));
    s.disk_usage_busy.set(false);
}

pub(super) fn cache(s: &State) -> Vec<El> {
    let lang = s.lang;
    let (images, audio, metadata) = {
        let (images, audio, metadata, at) = s.disk_usage.get();
        if s.now - at < 5.0 || s.disk_usage_busy.get() {
            (images, audio, metadata)
        } else if let Some(app) = s.app.clone() {
            s.disk_usage_busy.set(true);
            let cache = s.cache.clone();
            let downloads = s.download_store.clone();
            let spawned = std::thread::Builder::new().name("glint-disk-usage".into()).spawn(move || {
                let images = cache.as_ref().map_or(0, |d| d.total_bytes());
                let (audio, metadata) = downloads.as_ref().map_or((0, 0), |d| d.usage());
                app.update(move |s, _tx| {
                    s.disk_usage.set((images, audio, metadata, s.now));
                    s.disk_usage_busy.set(false);
                });
            });
            if spawned.is_err() {
                s.disk_usage_busy.set(false);
            }
            (images, audio, metadata)
        } else {
            let images = s.cache.as_ref().map_or(0, |d| d.total_bytes());
            let (audio, metadata) = s.download_store.as_ref().map_or((0, 0), |d| d.usage());
            s.disk_usage.set((images, audio, metadata, s.now));
            (images, audio, metadata)
        }
    };
    let total = images + audio + metadata;
    let kinds = [(S::CacheImages, images), (S::CacheAudio, audio), (S::CacheMetadata, metadata)];

    let mut bar = row().key("store-bar").w_full().h(10.0).radius(5.0).gap(2.0).fill(veil(0.07));
    if total > 0 {
        let nonzero: Vec<(usize, u64)> = kinds.iter().enumerate().filter(|(_, (_, b))| *b > 0).map(|(i, (_, b))| (i, *b)).collect();
        let last = nonzero.len().saturating_sub(1);
        for (pos, (i, bytes)) in nonzero.into_iter().enumerate() {
            let l = if pos == 0 { 5.0 } else { 0.0 };
            let r = if pos == last { 5.0 } else { 0.0 };
            bar = bar.child(el().key(format!("store-bar-{i}")).h_full().min_w(4.0).grow(bytes as f32 / total as f32).corners([l, r, r, l]).fill(th::rgb(STORE_HUES[i])));
        }
    }
    let legend = row().w_full().gap(18.0).flex_wrap().children(kinds.iter().enumerate().map(|(i, (name, bytes))| {
        row()
            .gap(7.0)
            .items_center()
            .child(el().w(8.0).h(8.0).radius(4.0).fill(th::rgb(STORE_HUES[i])))
            .child(label(name.get(lang).to_owned(), th::SMALL, th::text2()))
            .child(figure(size(*bytes, lang)))
    }));
    let used = col()
        .key("store-used")
        .w_full()
        .padding(18.0)
        .gap(12.0)
        .radius(GROUP_R)
        .fill(veil(0.035))
        .border(1.0, veil(0.065))
        .child(col().gap(2.0).child(label(S::StorageUsed.get(lang).to_owned(), th::SMALL, th::text3())).child(label(size(total, lang), TOTAL, th::text())))
        .child(bar)
        .child(legend);

    let delete = if total > 0 {
        danger_btn("content-delete", S::DeleteContent.get(lang)).on_press(|s: &mut State| {
            delete_content(s);
            let text = s.tr(S::ContentDeleted);
            s.toast(text);
        })
    } else {
        figure(S::HistoryEmpty.get(lang).to_owned())
    };
    vec![used, group("set-store", None, vec![item(S::StorageContent.get(lang), Some(S::StorageContentNote.get(lang)), delete)], None)]
}

pub(super) fn updates(s: &State, k: &rune_kit::Kit) -> Vec<El> {
    use crate::update::Ui;
    let lang = s.lang;
    let check = || kit::btn_small("update-check", S::CheckNow.get(lang), None).on_press(|s: &mut State| s.check_updates(true));
    let page = |url: String| {
        kit::btn_small("update-page", S::UpdateReleasePage.get(lang), Some(&icons::EXTERNAL)).on_press_tx(move |_s: &mut State, tx: &mut Tx| {
            let _ = tx.open_url(&url);
        })
    };
    let installable = s.update_method.can_install();
    let (status, note, control): (String, Option<&str>, El) = match &s.update {
        Ui::Idle => (S::UpdateIdleNote.get(lang).to_owned(), None, check()),
        Ui::Checking => (S::UpdateChecking.get(lang).to_owned(), None, kit::btn_small_disabled("update-check", S::CheckNow.get(lang), None)),
        Ui::Latest => (S::UpToDate.get(lang).to_owned(), None, check()),
        Ui::Failed(why) => (why.get(lang).to_owned(), None, check()),
        Ui::Available(r) => {
            let mut buttons = row().gap(8.0).items_center().child(page(r.page_url.clone()));
            if installable && r.asset.is_some() {
                buttons = buttons.child(kit::btn_small("update-install", S::UpdateInstall.get(lang), Some(&icons::DOWNLOAD)).on_press(|s: &mut State| s.install_update()));
            }
            let note = (!installable).then(|| S::UpdateManual.get(lang));
            (format!("{} {}", S::UpdateAvailable.get(lang), r.version), note, buttons)
        }
        Ui::Downloading { release, done, total } => {
            let pct = if *total > 0 { (done * 100 / total).min(100) } else { 0 };
            (format!("{} {} · {pct}%", S::UpdateDownloading.get(lang), release.version), None, kit::btn_small_disabled("update-wait", S::UpdateInstall.get(lang), None))
        }
        Ui::Installing => (S::UpdateInstalling.get(lang).to_owned(), None, el()),
    };
    let mut says = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(3.0)
        .child(label(format!("Glint {}", env!("CARGO_PKG_VERSION")), th::H2, th::text()))
        .child(label(status, th::SMALL, th::text2()).wrap().align(Align::Start));
    if let Some(n) = note {
        says = says.child(label(n.to_owned(), th::SMALL, th::text3()).wrap().align(Align::Start));
    }
    let now = row().key("set-version").w_full().px(ROW_PAD).py(16.0).gap(16.0).items_center().child(app_mark(44.0)).child(says).child(control);
    let auto = toggle(k, s.settings.auto_update, S::AutoUpdate.get(lang), |s| {
        s.settings.auto_update = !s.settings.auto_update;
        s.mark_settings_dirty();
    });
    vec![group("set-update", None, vec![now], None), group("set-update-auto", None, vec![item(S::AutoUpdate.get(lang), None, auto)], None)]
}

fn link_row(key: &str, ic: &'static Icon, name: &str, url: &'static str) -> El {
    row()
        .key(key.to_owned())
        .role("link")
        .label(name)
        .w_full()
        .h(48.0)
        .px(ROW_PAD)
        .gap(12.0)
        .items_center()
        .cursor(Cursor::Pointer)
        .hov(veil(0.04))
        .press(veil(0.07))
        .hov_group()
        .on_press_tx(move |_s: &mut State, tx: &mut Tx| {
            let _ = tx.open_url(url);
        })
        .child(rune::icon(ic, 16.0).color(th::text2()))
        .child(label(name.to_owned(), th::BODY_M, th::text()).grow(1.0).min_w(0.0))
        .child(rune::icon(&icons::EXTERNAL, 14.0).color(th::text3()).gcol(th::text2()))
}

pub(super) fn about(s: &State, k: &rune_kit::Kit) -> Vec<El> {
    let lang = s.lang;
    let version = row().h(22.0).px(9.0).radius(11.0).center().fill(veil(0.07)).child(label(format!("v{}", env!("CARGO_PKG_VERSION")), th::MONO, th::text2()));
    let hero = col()
        .key("set-about")
        .w_full()
        .pt(12.0)
        .pb(4.0)
        .gap(10.0)
        .items_center()
        .child(app_mark(72.0))
        .child(row().gap(10.0).items_center().mt(6.0).child(label("Glint".to_owned(), th::H0, th::text())).child(version))
        .child(label(S::AboutText.get(lang).to_owned(), th::SMALL, th::text2()).wrap().align(Align::Center).max_w(420.0));
    let log_toggle = toggle(k, s.settings.logging, S::DiagLogs.get(lang), |s| {
        s.settings.logging = !s.settings.logging;
        crate::diag_log::set_enabled(s.settings.logging);
        if s.settings.logging {
            crate::diag_log::write_system(&s.settings, s.update_method, s.win_size.get());
        }
        s.mark_settings_dirty();
    });
    let log_bytes = crate::diag_log::total_bytes();
    let log_actions = row()
        .gap(8.0)
        .items_center()
        .child(figure(size(log_bytes, lang)))
        .child(kit::btn_small("logs-open", S::OpenLogs.get(lang), Some(&icons::FOLDER)).on_press(|s: &mut State| s.open_logs_folder()))
        .children((log_bytes > 0).then(|| {
            kit::btn_small("logs-clear", S::ClearShort.get(lang), Some(&icons::TRASH)).on_press(|s: &mut State| {
                crate::diag_log::clear();
                let text = s.tr(S::LogsCleared);
                s.toast(text);
            })
        }));
    let diagnostics = group(
        "set-diag",
        Some(S::Diagnostics.get(lang)),
        vec![item(S::DiagLogs.get(lang), Some(S::DiagLogsNote.get(lang)), log_toggle), item(S::DiagLogFiles.get(lang), None, log_actions)],
        None,
    );
    let links = group(
        "set-links",
        Some(S::Links.get(lang)),
        vec![
            link_row("set-source", &icons::CODE, S::SourceCode.get(lang), REPO),
            link_row("set-issues", &icons::ALERT, S::ReportIssue.get(lang), "https://github.com/orphytype/glint/issues"),
            link_row("set-license", &icons::INFO, S::License.get(lang), "https://github.com/orphytype/glint/blob/main/LICENSE"),
        ],
        Some(S::Credits.get(lang)),
    );
    vec![hero, diagnostics, links]
}
