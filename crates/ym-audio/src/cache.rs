//! An on-disk cache of cover art. A limit is optional: with one, the least recently used files go
//! first; with [`DiskCache::UNLIMITED`], nothing is ever evicted.
//!
//! Files are written to a temporary name and renamed into place, so a crash or a second process
//! never sees half a file. Reading a file refreshes its modification time, which is the "recently
//! used" clock.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;

const EXT: &str = "bin";

pub struct DiskCache {
    dir: PathBuf,
    max_bytes: u64,
    /// Serialises writes and evictions within the process.
    write: Mutex<()>,
}

/// FNV-1a: only used to keep different odd keys apart, not for security.
fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

fn touch(path: &Path) {
    if let Ok(f) = fs::OpenOptions::new().write(true).open(path) {
        let _ = f.set_modified(SystemTime::now());
    }
}

impl DiskCache {
    /// Keeps every file: no limit and no eviction.
    pub const UNLIMITED: u64 = u64::MAX;

    /// Opens (creating if needed) the cache directory. `max_bytes == 0` disables caching;
    /// [`DiskCache::UNLIMITED`] keeps everything.
    pub fn open(dir: impl Into<PathBuf>, max_bytes: u64) -> io::Result<DiskCache> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        let cache = DiskCache { dir, max_bytes, write: Mutex::new(()) };
        // Leftovers of an interrupted write.
        for entry in fs::read_dir(&cache.dir)?.flatten() {
            if entry.path().extension().is_some_and(|e| e == "tmp") {
                let _ = fs::remove_file(entry.path());
            }
        }
        Ok(cache)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Keys are file names: anything outside `[A-Za-z0-9_-]` is replaced, and a hash keeps keys
    /// that only differ in such characters apart. A key can never name a path outside `dir`.
    ///
    /// Only an all-lowercase key keeps its name bare. Adding the hash to everything else also
    /// keeps `abc` and `ABC` apart on Windows and macOS, where file names compare without case.
    fn path(&self, key: &str) -> PathBuf {
        let clean: String = key.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') { c } else { '_' }).take(80).collect();
        let bare = !key.is_empty() && clean == key && !key.bytes().any(|b| b.is_ascii_uppercase());
        let name = if bare { clean } else { format!("{clean}-{:016x}", fnv(key)) };
        self.dir.join(format!("{name}.{EXT}"))
    }

    pub fn contains(&self, key: &str) -> bool {
        self.max_bytes > 0 && self.path(key).is_file()
    }

    pub fn get(&self, key: &str) -> Option<Arc<[u8]>> {
        if self.max_bytes == 0 {
            return None;
        }
        let path = self.path(key);
        let bytes = fs::read(&path).ok()?;
        touch(&path);
        Some(bytes.into())
    }

    pub fn put(&self, key: &str, bytes: &[u8]) -> io::Result<()> {
        if self.max_bytes == 0 || bytes.len() as u64 > self.max_bytes || bytes.is_empty() {
            return Ok(());
        }
        let _guard = self.write.lock().unwrap_or_else(PoisonError::into_inner);
        let path = self.path(key);
        let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
        fs::write(&tmp, bytes)?;
        if let Err(e) = fs::rename(&tmp, &path) {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
        self.evict();
        Ok(())
    }

    fn entries(&self) -> Vec<(SystemTime, u64, PathBuf)> {
        let Ok(dir) = fs::read_dir(&self.dir) else { return Vec::new() };
        dir.flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == EXT))
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                Some((meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), meta.len(), e.path()))
            })
            .collect()
    }

    pub fn total_bytes(&self) -> u64 {
        self.entries().iter().map(|e| e.1).sum()
    }

    pub fn len(&self) -> usize {
        self.entries().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Deletes the least recently used files until the cache fits.
    fn evict(&self) {
        if self.max_bytes == Self::UNLIMITED {
            return;
        }
        let mut files = self.entries();
        let mut total: u64 = files.iter().map(|f| f.1).sum();
        if total <= self.max_bytes {
            return;
        }
        files.sort_by_key(|f| f.0);
        for (_, len, path) in files {
            if total <= self.max_bytes {
                break;
            }
            if fs::remove_file(&path).is_ok() {
                total -= len;
            }
        }
    }

    pub fn clear(&self) -> io::Result<()> {
        let _guard = self.write.lock().unwrap_or_else(PoisonError::into_inner);
        for (_, _, path) in self.entries() {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}
