//! Updates from the GitHub releases: a look when Glint starts, and a button in Settings that
//! downloads the new version, puts it in place and restarts. The work itself is `glint-update`;
//! this is the state the Settings page shows and the jobs that move it along.

use std::time::{Duration, Instant};

use glint_update::{Method, Release};

use crate::app::State;
use crate::i18n::S;

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Ui {
    #[default]
    Idle,
    Checking,
    /// Looked, and this is the newest.
    Latest,
    Available(Release),
    Downloading { release: Release, done: u64, total: u64 },
    /// The new copy is starting; Glint is about to quit.
    Installing,
    Failed(S),
}

impl Ui {
    /// A check or an install is under way: no second one starts.
    pub fn busy(&self) -> bool {
        matches!(self, Ui::Checking | Ui::Downloading { .. } | Ui::Installing)
    }
}

/// What a failed step says to the user: offline is its own message, the rest is "could not".
fn failure(e: &glint_update::Error) -> S {
    match e {
        glint_update::Error::Network(_) => S::Offline,
        _ => S::UpdateFailed,
    }
}

impl State {
    /// The look at start-up. Not in headless runs, and not when switched off in Settings.
    pub fn auto_check_updates(&mut self) {
        if self.headless || !self.settings.auto_update {
            return;
        }
        self.check_updates(false);
    }

    /// Asks GitHub for the newest release. A look at start-up says nothing when it fails (an
    /// offline start is no news); the button in Settings says why.
    pub fn check_updates(&mut self, by_hand: bool) {
        if self.update.busy() || self.app.is_none() {
            return;
        }
        self.update = Ui::Checking;
        let method = self.update_method;
        self.job(async move { glint_update::check(env!("CARGO_PKG_VERSION"), method).await }, move |s, _, res| {
            s.update = match res {
                Ok(Some(release)) => {
                    if !by_hand {
                        let text = s.tr(S::UpdateToast);
                        s.toast(text);
                    }
                    Ui::Available(release)
                }
                Ok(None) => Ui::Latest,
                Err(e) if by_hand => Ui::Failed(failure(&e)),
                Err(_) => Ui::Idle,
            };
        });
    }

    /// Downloads the new version, checks it, puts it in place and quits for the new copy to take over.
    pub fn install_update(&mut self) {
        let Ui::Available(release) = self.update.clone() else { return };
        let method = self.update_method;
        let (Some(asset), true, Some(app)) = (release.asset.clone(), method.can_install(), self.app.clone()) else { return };
        self.update = Ui::Downloading { release: release.clone(), done: 0, total: asset.size };
        let progress_app = app.clone();
        self.job(
            async move {
                let dest = glint_update::download_path(method, &asset)?;
                let mut last = Instant::now() - Duration::from_secs(1);
                let release_for_progress = release.clone();
                glint_update::download(&asset, &dest, move |done, total| {
                    // The UI needs no more than a few updates a second.
                    if last.elapsed() >= Duration::from_millis(150) {
                        last = Instant::now();
                        let release = release_for_progress.clone();
                        progress_app.update(move |s, _| {
                            if matches!(s.update, Ui::Downloading { .. }) {
                                s.update = Ui::Downloading { release, done, total };
                            }
                        });
                    }
                })
                .await?;
                tokio::task::spawn_blocking(move || glint_update::install(method, &dest)).await.map_err(|e| glint_update::Error::Io(std::io::Error::other(e)))??;
                Ok::<(), glint_update::Error>(())
            },
            move |s, _, res| match res {
                Ok(()) => {
                    s.update = Ui::Installing;
                    app.quit();
                }
                Err(e) => s.update = Ui::Failed(failure(&e)),
            },
        );
    }
}

/// How this copy was installed, for the Settings page and the update job.
pub fn method() -> Method {
    Method::detect()
}
