//! What survives a restart: settings (JSON in the user's config folder), offline downloads,
//! startup snapshots and the OAuth token. On Windows the token is encrypted with the Data
//! Protection API, so only this Windows user can read the file. On Linux it is a file only its
//! owner may read or write (mode 0600).
//!
//! Where: `%APPDATA%\Glint` and `%LOCALAPPDATA%\Glint` on Windows, `~/.config/glint` and
//! `~/.cache/glint` on Linux; next to the exe, in `data`, for a portable copy (a `glint.portable`
//! file beside it); anywhere with `GLINT_DATA_DIR`.

mod downloads;
mod io;
mod settings;
mod snapshots;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub use downloads::{DownloadStore, DownloadedTrack};
pub use settings::{BackdropLook, LyricLook, MiniStyle, PinnedSetting, PlayerLayout, QualityPref, Repeat, Settings, StageLook, ThemeMode, WaveLook, WaveTune};
pub use snapshots::{HomeSnapshot, LibrarySnapshot, PlaybackSnapshot};

/// Locally uploaded tracks and custom track ordering for a playlist (`"{uid}:{kind}"`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalPlaylistData {
    pub local_tracks: Vec<ym_api::Track>,
    pub order: Vec<String>,
}

/// Where the window was when it last closed, in physical pixels: its frame's top-left, its
/// content's size, and whether it was maximized (then the frame is the one it restores to).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedWindow {
    pub position: (i32, i32),
    pub size: (u32, u32),
    pub maximized: bool,
}

#[derive(Clone, Debug)]
pub struct Store {
    pub(super) config: Option<PathBuf>,
    pub(super) cache: Option<PathBuf>,
}

impl Store {
    /// The real folders, a private folder when `GLINT_DATA_DIR` is set, or the `data` folder next to
    /// the exe for a portable copy.
    pub fn open() -> Store {
        let dirs = match (std::env::var_os("GLINT_DATA_DIR"), io::portable_root()) {
            (Some(root), _) => Some(rune::dirs::AppDirs::rooted(root, io::APP_ID)),
            (None, Some(root)) => Some(rune::dirs::AppDirs::rooted(root, io::APP_ID)),
            (None, None) => rune::dirs::AppDirs::new(io::APP_ID),
        };
        let (config, cache) = match dirs {
            Some(d) => {
                let cfg = d.config().to_path_buf();
                let cache = d.cache().to_path_buf();
                let _ = std::fs::create_dir_all(&cfg);
                let _ = std::fs::create_dir_all(&cache);
                (Some(cfg), Some(cache))
            }
            None => (None, None),
        };
        Store { config, cache }
    }

    /// A store that keeps nothing (headless snapshots).
    pub fn none() -> Store {
        Store { config: None, cache: None }
    }

    /// Where the picture picked for behind the player is kept.
    pub fn backdrop_file(&self) -> Option<PathBuf> {
        self.write_file("backdrop.img")
    }

    pub fn cache_dir(&self) -> Option<PathBuf> {
        self.cache.as_ref().map(|c| c.join("metadata"))
    }

    pub fn downloads_dir(&self) -> Option<PathBuf> {
        self.cache.as_ref().map(|c| c.join("downloads"))
    }

    /// Folder holding local anonymized diagnostic logs (`glint.log`, `glint.prev.log`, `system.txt`).
    pub fn logs_dir(&self) -> Option<PathBuf> {
        self.config.as_ref().map(|d| d.join("logs"))
    }

    /// Where earlier versions kept the downloads (beside the cache folder rather than in it), when
    /// that folder is still there.
    pub fn legacy_downloads_dir(&self) -> Option<PathBuf> {
        let cache = self.cache.as_ref()?;
        // Only the Windows layout moved (`%LOCALAPPDATA%\Glint` to its `Cache` inside); a rooted
        // folder's `downloads` beside the cache is the user's own Downloads.
        if cache.file_name().and_then(|n| n.to_str()) != Some("Cache") {
            return None;
        }
        let old = cache.parent()?.join("downloads");
        (old.is_dir() && Some(&old) != self.downloads_dir().as_ref()).then_some(old)
    }

    fn read_file(&self, name: &str) -> Option<PathBuf> {
        self.config.as_ref().map(|d| d.join(name))
    }

    fn write_file(&self, name: &str) -> Option<PathBuf> {
        let dir = self.config.as_ref()?;
        if !dir.is_dir() {
            std::fs::create_dir_all(dir).ok()?;
        }
        Some(dir.join(name))
    }

    pub fn load(&self) -> Settings {
        self.read_file("settings.json").and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice::<Settings>(&b).ok()).unwrap_or_default().sanitized()
    }

    pub fn save(&self, settings: &Settings) {
        let Some(path) = self.write_file("settings.json") else { return };
        if let Ok(json) = serde_json::to_vec_pretty(settings) {
            io::write_atomic(&path, &json);
        }
    }

    /// Writes settings on the serialized background writer thread for periodic saves while running.
    pub fn save_settings_async(&self, settings: Settings) {
        let Some(path) = self.config.as_ref().map(|d| d.join("settings.json")) else { return };
        io::enqueue_json_pretty(path, settings);
    }

    pub fn load_window(&self) -> Option<SavedWindow> {
        let bytes = std::fs::read(self.read_file("window.json")?).ok()?;
        serde_json::from_slice::<SavedWindow>(&bytes).ok().filter(|w| (200..=20000).contains(&w.size.0) && (200..=20000).contains(&w.size.1))
    }

    pub fn save_window(&self, window: &SavedWindow) {
        let Some(path) = self.write_file("window.json") else { return };
        if let Ok(json) = serde_json::to_vec_pretty(window) {
            io::write_atomic(&path, &json);
        }
    }

    pub fn load_local_playlists(&self) -> std::collections::BTreeMap<String, LocalPlaylistData> {
        self.read_file("playlists.json")
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save_local_playlists(&self, map: &std::collections::BTreeMap<String, LocalPlaylistData>) {
        let Some(path) = self.write_file("playlists.json") else { return };
        if let Ok(json) = serde_json::to_vec_pretty(map) {
            io::write_atomic(&path, &json);
        }
    }

    /// Returns whether a saved token file is present without decrypting it.
    pub fn has_token(&self) -> bool {
        self.config.as_ref().is_some_and(|d| d.join("token.bin").is_file())
    }

    pub fn load_token(&self) -> Option<String> {
        let path = self.read_file("token.bin")?;
        let sealed = std::fs::read(path).ok()?;
        let plain = io::unprotect(&sealed)?;
        String::from_utf8(plain).ok().filter(|t| !t.trim().is_empty())
    }

    pub fn save_token(&self, token: Option<&str>) {
        let Some(path) = self.write_file("token.bin") else { return };
        match token {
            Some(t) => {
                if let Some(sealed) = io::protect(t.as_bytes()) {
                    io::write_private(&path, &sealed);
                }
            }
            None => {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}
