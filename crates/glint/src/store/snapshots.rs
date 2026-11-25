//! Startup snapshots (home shelves, library, and last playback position).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Store, io};

/// What the window showed last time, kept so the next launch opens straight onto it while the
/// fresh copy loads.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HomeSnapshot {
    pub account: Option<ym_api::Account>,
    pub avatar: Option<String>,
    pub home: Vec<ym_api::Shelf>,
    pub wheel: Vec<ym_api::WheelItem>,
}

/// What was playing when Glint last closed: the queue, the track in it and where in that track,
/// so the next start picks up there (paused, ready for play).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaybackSnapshot {
    pub queue: Vec<ym_api::Track>,
    pub index: usize,
    pub pos_ms: u64,
    /// The My Wave preset it came from, so the wave goes on where it left off.
    pub wave: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LibrarySnapshot {
    pub liked: Vec<ym_api::Track>,
    pub playlists: Vec<ym_api::Playlist>,
    pub liked_playlists: Vec<ym_api::Playlist>,
    pub albums: Vec<ym_api::Album>,
    pub artists: Vec<ym_api::Artist>,
    pub top_artists: Vec<ym_api::TopArtist>,
    pub liked_cover: Option<String>,
}

impl Store {
    fn snapshot_file(&self, name: &str) -> Option<PathBuf> {
        self.cache.as_ref().map(|d| d.join(name))
    }

    pub fn load_home_snapshot(&self) -> Option<HomeSnapshot> {
        let bytes = std::fs::read(self.snapshot_file("home.json")?).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Where the library snapshot lives, for reading it on a worker.
    pub fn library_snapshot_path(&self) -> Option<PathBuf> {
        self.snapshot_file("library.json")
    }

    pub fn read_library_snapshot(path: &Path) -> Option<LibrarySnapshot> {
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    }

    /// Writes a snapshot on the serialized background writer thread so bursts never race on disk.
    pub fn save_snapshot<T: Serialize + Send + 'static>(&self, name: &'static str, value: T) {
        let Some(path) = self.snapshot_file(name) else { return };
        io::enqueue_json(path, value);
    }

    pub fn load_playback(&self) -> Option<PlaybackSnapshot> {
        let bytes = std::fs::read(self.snapshot_file("playback.json")?).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Writes what is playing now, on this thread: it is small, and on quit a worker might not
    /// get to finish.
    pub fn save_playback(&self, snap: &PlaybackSnapshot) {
        let Some(path) = self.snapshot_file("playback.json") else { return };
        if let Some(dir) = path.parent()
            && !dir.is_dir()
        {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec(snap) {
            io::write_atomic(&path, &json);
        }
    }

    /// Writes playback state on the background writer thread for periodic saves while playing.
    pub fn save_playback_async(&self, snap: PlaybackSnapshot) {
        self.save_snapshot("playback.json", snap);
    }

    /// Forgets the snapshots (signing out).
    pub fn clear_snapshots(&self) {
        for name in ["home.json", "library.json", "playback.json"] {
            if let Some(p) = self.snapshot_file(name) {
                let _ = std::fs::remove_file(p);
            }
        }
    }
}
