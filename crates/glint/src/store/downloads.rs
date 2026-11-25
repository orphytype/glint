//! Offline downloaded audio and track metadata store, backed by an in-memory file index so
//! checking non-downloaded tracks avoids probing disk repeatedly.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use serde::{Deserialize, Serialize};

use super::io::write_atomic_result;

/// Every quality a track can be downloaded in (as `ym_api::Quality` prints), best first.
const DOWNLOAD_QUALITIES: [&str; 4] = ["Lossless", "Best", "Balanced", "Saver"];

const AUDIO_EXTS: [&str; 5] = ["mp3", "flac", "m4a", "wav", "bin"];

/// Persistent files that belong to a downloaded track. This store is deliberately separate from
/// the cover-art cache: nothing here is evicted automatically, only when the user deletes a track
/// or clears the downloads.
#[derive(Clone)]
pub struct DownloadStore {
    dir: PathBuf,
    write: Arc<Mutex<()>>,
    /// Existing `audio-*` file names in `dir`, kept in sync on writes/deletes to avoid 20-40
    /// failed `std::fs::read` syscalls per streamed track.
    audio_files: Arc<RwLock<HashSet<String>>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadedTrack {
    pub track: ym_api::Track,
    pub lyrics: ym_api::Lyrics,
    pub ext: String,
}

impl DownloadStore {
    pub fn open(dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        let mut files = HashSet::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.starts_with("audio-") {
                        files.insert(name.to_owned());
                    } else if name.starts_with(".dl-") && name.ends_with(".tmp") {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }
        Ok(Self { dir, write: Arc::new(Mutex::new(())), audio_files: Arc::new(RwLock::new(files)) })
    }

    /// Allocates a unique temporary file path inside the download directory so finished tracks can
    /// be moved into place atomically with zero extra copying or RAM buffering.
    pub fn temp_path(&self, tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(1);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        self.dir.join(format!(".dl-{}-{}-{seq}.tmp", Self::clean_key(tag), std::process::id()))
    }

    fn key(key: &str) -> String {
        let clean = Self::clean_key(key);
        let hash = key.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
        format!("{clean}-{hash:016x}")
    }

    fn clean_key(key: &str) -> String {
        key.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') { c } else { '_' }).take(80).collect()
    }

    fn audio_name(hashed_key: &str, ext: &str) -> String {
        format!("audio-{hashed_key}.{ext}")
    }

    fn metadata_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("track-{}.json", Self::key(id)))
    }

    /// Path and byte length of a downloaded audio file without reading it into RAM.
    pub fn get_audio_path(&self, key: &str) -> Option<(PathBuf, u64)> {
        let hashed = Self::key(key);
        let name = {
            let index = self.audio_files.read().unwrap_or_else(PoisonError::into_inner);
            AUDIO_EXTS.into_iter().map(|ext| Self::audio_name(&hashed, ext)).find(|n| index.contains(n))
        }?;
        let path = self.dir.join(&name);
        match std::fs::metadata(&path) {
            Ok(meta) if meta.len() > 0 => Some((path, meta.len())),
            _ => {
                self.audio_files.write().unwrap_or_else(PoisonError::into_inner).remove(&name);
                None
            }
        }
    }

    /// Path and byte length of the downloaded `variant` ("original" or "replacement") of track
    /// `id`, in `preferred` quality when present, else in any other quality.
    pub fn find_audio_path(&self, id: &str, preferred: &str, variant: &str) -> Option<(PathBuf, u64)> {
        if self.audio_files.read().unwrap_or_else(PoisonError::into_inner).is_empty() {
            return None;
        }
        std::iter::once(preferred).chain(DOWNLOAD_QUALITIES.into_iter().filter(|q| *q != preferred)).find_map(|q| self.get_audio_path(&format!("{id}-{q}-{variant}")))
    }

    pub fn get_audio(&self, key: &str) -> Option<Arc<[u8]>> {
        let hashed = Self::key(key);
        let name = {
            let index = self.audio_files.read().unwrap_or_else(PoisonError::into_inner);
            AUDIO_EXTS.into_iter().map(|ext| Self::audio_name(&hashed, ext)).find(|n| index.contains(n))
        }?;
        match std::fs::read(self.dir.join(&name)) {
            Ok(bytes) => Some(bytes.into()),
            Err(_) => {
                self.audio_files.write().unwrap_or_else(PoisonError::into_inner).remove(&name);
                None
            }
        }
    }

    /// The downloaded `variant` ("original" or "replacement") of track `id`, in the quality
    /// `preferred` names when there is that copy, else in any other.
    pub fn find_audio(&self, id: &str, preferred: &str, variant: &str) -> Option<Arc<[u8]>> {
        if self.audio_files.read().unwrap_or_else(PoisonError::into_inner).is_empty() {
            return None;
        }
        std::iter::once(preferred).chain(DOWNLOAD_QUALITIES.into_iter().filter(|q| *q != preferred)).find_map(|q| self.get_audio(&format!("{id}-{q}-{variant}")))
    }

    /// Moves the downloads an earlier version kept in `legacy` (another folder) into this store,
    /// leaving any file this store already has where it is, then sets the old folder aside so it is
    /// not looked through again. Returns how many files came over.
    pub fn adopt(&self, legacy: &Path) -> usize {
        let Ok(entries) = std::fs::read_dir(legacy) else { return 0 };
        let mut ours = false;
        let _guard = self.write.lock().unwrap_or_else(PoisonError::into_inner);
        let mut index = self.audio_files.write().unwrap_or_else(PoisonError::into_inner);
        let mut moved = 0;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(n) = name.to_str() else { continue };
            let is_audio = n.starts_with("audio-");
            if !(is_audio || (n.starts_with("track-") && n.ends_with(".json"))) {
                continue;
            }
            ours = true;
            let to = self.dir.join(&name);
            if to.exists() {
                continue;
            }
            if std::fs::rename(entry.path(), &to).is_ok() || (std::fs::copy(entry.path(), &to).is_ok() && std::fs::remove_file(entry.path()).is_ok()) {
                if is_audio {
                    index.insert(n.to_owned());
                }
                moved += 1;
            }
        }
        drop(index);
        let aside = legacy.with_file_name("downloads-old");
        if ours && !aside.exists() {
            let _ = std::fs::rename(legacy, aside);
        }
        moved
    }

    /// Stores audio under its container extension so downloaded files can be opened directly by
    /// music players. The legacy `.bin` path remains supported for existing caches.
    pub fn put_audio_with_ext(&self, key: &str, bytes: &[u8], ext: &str) -> std::io::Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        let _guard = self.write.lock().unwrap_or_else(PoisonError::into_inner);
        let ext = match ext {
            "flac" => "flac",
            "mp3" => "mp3",
            "m4a" => "m4a",
            "wav" => "wav",
            _ => "bin",
        };
        let hashed = Self::key(key);
        let name = Self::audio_name(&hashed, ext);
        write_atomic_result(&self.dir.join(&name), bytes)?;
        let mut index = self.audio_files.write().unwrap_or_else(PoisonError::into_inner);
        index.insert(name);
        for old_ext in AUDIO_EXTS {
            if old_ext != ext {
                let old_name = Self::audio_name(&hashed, old_ext);
                if index.remove(&old_name) {
                    let _ = std::fs::remove_file(self.dir.join(old_name));
                }
            }
        }
        Ok(())
    }

    /// Moves a finished temporary audio file on disk into the store without loading it into RAM.
    pub fn put_audio_file(&self, key: &str, src: &Path, ext: &str) -> std::io::Result<()> {
        if std::fs::metadata(src).is_err_and(|_| true) || std::fs::metadata(src).is_ok_and(|m| m.len() == 0) {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "empty audio file"));
        }
        let _guard = self.write.lock().unwrap_or_else(PoisonError::into_inner);
        let ext = match ext {
            "flac" => "flac",
            "mp3" => "mp3",
            "m4a" => "m4a",
            "wav" => "wav",
            _ => "bin",
        };
        let hashed = Self::key(key);
        let name = Self::audio_name(&hashed, ext);
        let dst = self.dir.join(&name);
        if dst.exists() {
            let _ = std::fs::remove_file(&dst);
        }
        if std::fs::rename(src, &dst).is_err() {
            std::fs::copy(src, &dst)?;
            let _ = std::fs::remove_file(src);
        }
        let mut index = self.audio_files.write().unwrap_or_else(PoisonError::into_inner);
        index.insert(name);
        for old_ext in AUDIO_EXTS {
            if old_ext != ext {
                let old_name = Self::audio_name(&hashed, old_ext);
                if index.remove(&old_name) {
                    let _ = std::fs::remove_file(self.dir.join(old_name));
                }
            }
        }
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<DownloadedTrack> {
        let bytes = std::fs::read(self.metadata_path(id)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn put(&self, item: &DownloadedTrack) -> std::io::Result<()> {
        let bytes = serde_json::to_vec(item).map_err(|e| std::io::Error::other(e.to_string()))?;
        let _guard = self.write.lock().unwrap_or_else(PoisonError::into_inner);
        write_atomic_result(&self.metadata_path(&item.track.id), &bytes)
    }

    /// Removes the metadata and every quality/replacement audio file belonging to a track.
    pub fn remove(&self, id: &str) -> std::io::Result<()> {
        let _guard = self.write.lock().unwrap_or_else(PoisonError::into_inner);
        let mut paths = vec![self.metadata_path(id)];
        let mut index = self.audio_files.write().unwrap_or_else(PoisonError::into_inner);
        for quality in DOWNLOAD_QUALITIES {
            for variant in ["original", "replacement"] {
                let hashed = Self::key(&format!("{id}-{quality}-{variant}"));
                for ext in AUDIO_EXTS {
                    let name = Self::audio_name(&hashed, ext);
                    if index.remove(&name) {
                        paths.push(self.dir.join(name));
                    }
                }
            }
        }
        drop(index);
        for path in paths {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Every downloaded track whose metadata is present, newest metadata file first.
    pub fn tracks(&self) -> Vec<DownloadedTrack> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return Vec::new() };
        let mut out: Vec<_> = entries
            .flatten()
            .filter(|e| e.path().file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("track-") && n.ends_with(".json")))
            .filter_map(|e| {
                let modified = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                std::fs::read(e.path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).map(|item| (modified, item))
            })
            .collect();
        out.sort_by_key(|a| std::cmp::Reverse(a.0));
        out.into_iter().map(|(_, item)| item).collect()
    }

    /// Bytes taken by downloaded audio and by the rest of the store (track metadata and any
    /// interrupted writes), the two figures Settings shows apart.
    pub fn usage(&self) -> (u64, u64) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return (0, 0) };
        let (mut audio, mut metadata) = (0, 0);
        for entry in entries.flatten() {
            let is_audio = entry.file_name().to_str().is_some_and(|name| name.starts_with("audio-"));
            if let Ok(meta) = entry.metadata() {
                if is_audio {
                    audio += meta.len();
                } else {
                    metadata += meta.len();
                }
            }
        }
        (audio, metadata)
    }

    pub fn clear(&self) -> std::io::Result<()> {
        let _guard = self.write.lock().unwrap_or_else(PoisonError::into_inner);
        for entry in std::fs::read_dir(&self.dir)?.flatten() {
            std::fs::remove_file(entry.path())?;
        }
        self.audio_files.write().unwrap_or_else(PoisonError::into_inner).clear();
        Ok(())
    }
}
