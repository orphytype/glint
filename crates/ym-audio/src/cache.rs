//! An on-disk cache of cover art. A limit is optional: with one, the least recently used files go
//! first; with [`DiskCache::UNLIMITED`], nothing is ever evicted.
//!
//! Files are written to a temporary name and renamed into place, so a crash or a second process
//! never sees half a file. An in-memory index tracks file sizes and recency so reads never write
//! metadata back to disk and writes do not scan the whole folder.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

const EXT: &str = "bin";

struct Index {
    loaded: bool,
    total: u64,
    tick: u64,
    entries: HashMap<PathBuf, (u64, u64)>,
}

pub struct DiskCache {
    dir: PathBuf,
    max_bytes: u64,
    index: Mutex<Index>,
    seq: AtomicU64,
}

/// FNV-1a: only used to keep different odd keys apart, not for security.
fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

fn mtime_secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl DiskCache {
    /// Keeps every file: no limit and no eviction.
    pub const UNLIMITED: u64 = u64::MAX;

    /// Opens (creating if needed) the cache directory. `max_bytes == 0` disables caching;
    /// [`DiskCache::UNLIMITED`] keeps everything.
    pub fn open(dir: impl Into<PathBuf>, max_bytes: u64) -> io::Result<DiskCache> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        Ok(DiskCache { dir, max_bytes, index: Mutex::new(Index { loaded: false, total: 0, tick: 0, entries: HashMap::new() }), seq: AtomicU64::new(0) })
    }

    /// Removes temporary files left by an interrupted write and preloads the directory index.
    pub fn sweep_tmp(&self) {
        let Ok(entries) = fs::read_dir(&self.dir) else { return };
        let mut map = HashMap::new();
        let mut max_tick = 0u64;
        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str());
            if ext == Some("tmp") {
                let _ = fs::remove_file(&path);
            } else if ext == Some(EXT)
                && let Ok(meta) = entry.metadata()
            {
                let len = meta.len();
                let stamp = mtime_secs(meta.modified().unwrap_or(UNIX_EPOCH));
                max_tick = max_tick.max(stamp);
                map.insert(path, (len, stamp));
            }
        }
        let mut idx = self.index.lock().unwrap_or_else(PoisonError::into_inner);
        if !idx.loaded {
            for (path, (len, stamp)) in map {
                if let std::collections::hash_map::Entry::Vacant(slot) = idx.entries.entry(path) {
                    slot.insert((len, stamp));
                    idx.total = idx.total.saturating_add(len);
                }
            }
            idx.tick = idx.tick.max(max_tick);
            idx.loaded = true;
        }
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

    fn ensure_loaded(&self, idx: &mut Index) {
        if idx.loaded {
            return;
        }
        let Ok(dir) = fs::read_dir(&self.dir) else {
            idx.loaded = true;
            return;
        };
        let mut max_tick = idx.tick;
        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|x| x == EXT)
                && let Ok(meta) = entry.metadata()
            {
                let len = meta.len();
                let stamp = mtime_secs(meta.modified().unwrap_or(UNIX_EPOCH));
                max_tick = max_tick.max(stamp);
                if let Some((old, _)) = idx.entries.insert(path, (len, stamp)) {
                    idx.total = idx.total.saturating_sub(old);
                }
                idx.total = idx.total.saturating_add(len);
            }
        }
        idx.tick = max_tick;
        idx.loaded = true;
    }

    pub fn contains(&self, key: &str) -> bool {
        if self.max_bytes == 0 {
            return false;
        }
        let path = self.path(key);
        let idx = self.index.lock().unwrap_or_else(PoisonError::into_inner);
        if idx.loaded {
            return idx.entries.contains_key(&path);
        }
        drop(idx);
        path.is_file()
    }

    pub fn get(&self, key: &str) -> Option<Arc<[u8]>> {
        if self.max_bytes == 0 {
            return None;
        }
        let path = self.path(key);
        let bytes = fs::read(&path).ok()?;
        let len = bytes.len() as u64;
        let mut idx = self.index.lock().unwrap_or_else(PoisonError::into_inner);
        idx.tick = idx.tick.wrapping_add(1);
        let tick = idx.tick;
        if let Some(entry) = idx.entries.get_mut(&path) {
            entry.1 = tick;
        } else if idx.loaded {
            idx.total = idx.total.saturating_add(len);
            idx.entries.insert(path, (len, tick));
        }
        Some(bytes.into())
    }

    pub fn put(&self, key: &str, bytes: &[u8]) -> io::Result<()> {
        if self.max_bytes == 0 || bytes.len() as u64 > self.max_bytes || bytes.is_empty() {
            return Ok(());
        }
        let path = self.path(key);
        let n = self.seq.fetch_add(1, Ordering::Relaxed);
        let tmp = path.with_extension(format!("{}-{n}.tmp", std::process::id()));
        fs::write(&tmp, bytes)?;
        if let Err(e) = fs::rename(&tmp, &path) {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
        let len = bytes.len() as u64;
        let to_remove = {
            let mut idx = self.index.lock().unwrap_or_else(PoisonError::into_inner);
            if self.max_bytes != Self::UNLIMITED {
                self.ensure_loaded(&mut idx);
            }
            idx.tick = idx.tick.wrapping_add(1);
            let tick = idx.tick;
            if let Some((old, _)) = idx.entries.insert(path, (len, tick)) {
                idx.total = idx.total.saturating_sub(old);
            }
            idx.total = idx.total.saturating_add(len);
            if self.max_bytes != Self::UNLIMITED && idx.total > self.max_bytes {
                // Evict down to 7/8 of the limit so full caches do not evict on every write.
                let target = self.max_bytes - self.max_bytes / 8;
                let mut by_age: Vec<(u64, u64, PathBuf)> = idx.entries.iter().map(|(p, &(l, t))| (t, l, p.clone())).collect();
                by_age.sort_unstable_by_key(|e| e.0);
                let mut drop_paths = Vec::new();
                for (_, item_len, p) in by_age {
                    if idx.total <= target {
                        break;
                    }
                    idx.entries.remove(&p);
                    idx.total = idx.total.saturating_sub(item_len);
                    drop_paths.push(p);
                }
                drop_paths
            } else {
                Vec::new()
            }
        };
        for p in to_remove {
            let _ = fs::remove_file(p);
        }
        Ok(())
    }

    pub fn total_bytes(&self) -> u64 {
        let mut idx = self.index.lock().unwrap_or_else(PoisonError::into_inner);
        self.ensure_loaded(&mut idx);
        idx.total
    }

    pub fn len(&self) -> usize {
        let mut idx = self.index.lock().unwrap_or_else(PoisonError::into_inner);
        self.ensure_loaded(&mut idx);
        idx.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&self) -> io::Result<()> {
        let paths: Vec<PathBuf> = {
            let mut idx = self.index.lock().unwrap_or_else(PoisonError::into_inner);
            self.ensure_loaded(&mut idx);
            let list = idx.entries.keys().cloned().collect();
            idx.entries.clear();
            idx.total = 0;
            list
        };
        for path in paths {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}
