//! What survives a restart: settings (JSON in the user's config folder) and the OAuth token.
//! On Windows the token is encrypted with the Data Protection API, so only this Windows user can
//! read the file. On Linux it is a file only its owner may read or write (mode 0600), as other
//! command-line and desktop clients keep theirs.
//!
//! Where: `%APPDATA%\Glint` and `%LOCALAPPDATA%\Glint` on Windows, `~/.config/glint` and
//! `~/.cache/glint` on Linux; next to the exe, in `data`, for a portable copy (a `glint.portable`
//! file beside it); anywhere with `GLINT_DATA_DIR`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Effects {
    #[default]
    Full,
    Light,
    Off,
}

/// Which of the two looks is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
    /// Whatever Windows is set to.
    System,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

impl Repeat {
    pub fn next(self) -> Repeat {
        match self {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum QualityPref {
    Saver,
    #[default]
    Balanced,
    Best,
}

impl QualityPref {
    pub fn api(self) -> ym_api::Quality {
        match self {
            QualityPref::Saver => ym_api::Quality::Saver,
            QualityPref::Balanced => ym_api::Quality::Balanced,
            QualityPref::Best => ym_api::Quality::Best,
        }
    }
}

/// How the full player uses its vertical space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PlayerLayout {
    #[default]
    Auto,
    Compact,
    Comfortable,
}

/// How the picture behind the open player looks, as chosen in its panel (each 0..=2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StageLook {
    /// The burst of light from under the cover: off, subtle, vibrant.
    #[serde(alias = "rays")]
    pub flash: u8,
    /// How much the music moves them: calm, lively, wild.
    pub react: u8,
    /// Which default this look was saved against: a look saved before the flash default is
    /// put back to the default once. (Its own default is 0, so a file without it is old.)
    #[serde(default)]
    pub v: u8,
}

/// How the lyrics look in the open player (each 0..=2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LyricLook {
    /// The words' size: smaller, normal, larger.
    pub size: u8,
    /// How the sung line comes forward: at once, smoothly, on a spring.
    pub anim: u8,
    /// How far the other lines step back: a little, more, far.
    pub focus: u8,
    /// Where the lines stand: on the left, in the middle, on the right.
    #[serde(default)]
    pub align: u8,
}

impl Default for LyricLook {
    fn default() -> Self {
        LyricLook { size: 1, anim: 1, focus: 1, align: 0 }
    }
}

impl LyricLook {
    pub fn sanitized(self) -> LyricLook {
        LyricLook { size: self.size.min(2), anim: self.anim.min(2), focus: self.focus.min(2), align: self.align.min(2) }
    }
}

/// The look's current default (see [`StageLook::v`]).
const STAGE_V: u8 = 2;

impl Default for StageLook {
    fn default() -> Self {
        StageLook { flash: 1, react: 1, v: STAGE_V }
    }
}

impl StageLook {
    pub fn sanitized(self) -> StageLook {
        if self.v < STAGE_V {
            return StageLook::default();
        }
        StageLook { flash: self.flash.min(2), react: self.react.min(2), v: STAGE_V }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub lang: Option<String>,
    /// Index into `theme::ACCENTS`; 0 follows the cover art.
    pub accent: usize,
    pub effects: Effects,
    pub theme: ThemeMode,
    pub reduce_motion: bool,
    pub quality: QualityPref,
    pub player_layout: PlayerLayout,
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub eq_enabled: bool,
    pub eq_preamp: f32,
    pub eq_gains: [f32; 10],
    pub wave: String,
    /// The last session was the offline demo.
    pub demo: bool,
    /// A tray icon with the transport controls in its menu.
    pub show_tray: bool,
    /// Look for a newer version on GitHub when Glint starts (nothing is installed unasked).
    pub auto_update: bool,
    /// The close button hides the window and leaves the music playing (needs the tray icon).
    pub close_to_tray: bool,
    /// Ask lrc.red for lyrics timed word by word.
    pub word_lyrics: bool,
    /// Replace censored tracks with community-maintained clean versions when available.
    pub fkcensor: bool,
    /// What was searched for lately, newest first.
    pub search_history: Vec<String>,
    /// The picture behind the open player.
    pub stage: StageLook,
    /// How the lyrics look in the open player.
    pub lyric_look: LyricLook,
    /// Cover-art shortcuts kept in the navigation rail.
    pub pinned: Vec<PinnedSetting>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PinnedSetting {
    pub kind: String,
    pub id: String,
    pub secondary: i64,
    pub title: String,
    pub cover: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            lang: None,
            accent: 0,
            effects: Effects::Full,
            theme: ThemeMode::Dark,
            reduce_motion: false,
            quality: QualityPref::Balanced,
            player_layout: PlayerLayout::Auto,
            volume: 0.8,
            shuffle: false,
            repeat: Repeat::Off,
            eq_enabled: false,
            eq_preamp: 0.0,
            eq_gains: [0.0; 10],
            wave: "onyourwave".into(),
            demo: false,
            show_tray: true,
            auto_update: true,
            close_to_tray: false,
            word_lyrics: true,
            fkcensor: true,
            search_history: Vec::new(),
            stage: StageLook::default(),
            lyric_look: LyricLook::default(),
            pinned: Vec::new(),
        }
    }
}

impl Settings {
    /// Pulls every value into its valid range, so a hand-edited file cannot break the app.
    pub fn sanitized(mut self) -> Settings {
        self.accent = self.accent.min(crate::theme::ACCENTS.len() - 1);
        self.volume = if self.volume.is_finite() { self.volume.clamp(0.0, 1.0) } else { 0.8 };
        self.eq_preamp = if self.eq_preamp.is_finite() { self.eq_preamp.clamp(-12.0, 12.0) } else { 0.0 };
        for g in &mut self.eq_gains {
            *g = if g.is_finite() { g.clamp(-12.0, 12.0) } else { 0.0 };
        }
        self.search_history.retain(|q| !q.trim().is_empty());
        for q in &mut self.search_history {
            *q = q.trim().chars().take(120).collect();
        }
        self.search_history.truncate(crate::app::SEARCH_HISTORY);
        self.stage = self.stage.sanitized();
        self.lyric_look = self.lyric_look.sanitized();
        self.pinned.retain(|p| !p.kind.is_empty() && !p.id.is_empty() && !p.title.trim().is_empty());
        self.pinned.truncate(24);
        self
    }
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
    config: Option<PathBuf>,
    cache: Option<PathBuf>,
}

/// Persistent files that belong to a downloaded track. This store is deliberately separate from
/// the cover-art cache: nothing here is evicted automatically, only when the user deletes a track
/// or clears the downloads.
#[derive(Clone)]
pub struct DownloadStore {
    dir: PathBuf,
    write: Arc<Mutex<()>>,
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
        Ok(Self { dir, write: Arc::new(Mutex::new(())) })
    }

    fn key(key: &str) -> String {
        let clean = Self::clean_key(key);
        let hash = key.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
        format!("{clean}-{hash:016x}")
    }

    fn clean_key(key: &str) -> String {
        key.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') { c } else { '_' }).take(80).collect()
    }

    fn audio_path_with_ext(&self, key: &str, ext: &str) -> PathBuf {
        self.dir.join(format!("audio-{}.{}", Self::key(key), ext))
    }

    fn metadata_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("track-{}.json", Self::key(id)))
    }

    pub fn get_audio(&self, key: &str) -> Option<Arc<[u8]>> {
        ["mp3", "flac", "m4a", "wav", "bin"].into_iter().find_map(|ext| std::fs::read(self.audio_path_with_ext(key, ext)).ok().map(Into::into))
    }

    pub fn put_audio(&self, key: &str, bytes: &[u8]) -> std::io::Result<()> {
        self.put_audio_with_ext(key, bytes, "bin")
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
        let path = self.audio_path_with_ext(key, ext);
        write_atomic_result(&path, bytes)?;
        for old_ext in ["bin", "mp3", "flac", "m4a", "wav"] {
            if old_ext != ext {
                let _ = std::fs::remove_file(self.audio_path_with_ext(key, old_ext));
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
        let metadata = format!("track-{}.json", Self::key(id));
        let mut paths = vec![self.dir.join(metadata)];
        for quality in ["Saver", "Balanced", "Best"] {
            for variant in ["original", "replacement"] {
                let key = format!("{id}-{quality}-{variant}");
                for ext in ["bin", "mp3", "flac", "m4a", "wav"] {
                    paths.push(self.audio_path_with_ext(&key, ext));
                }
            }
        }
        for path in paths {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Every downloaded track whose metadata is present, in stable title order.
    pub fn tracks(&self) -> Vec<DownloadedTrack> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return Vec::new() };
        let mut out: Vec<_> = entries
            .flatten()
            .filter(|e| e.path().file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("track-") && n.ends_with(".json")))
            .filter_map(|e| std::fs::read(e.path()).ok().and_then(|b| serde_json::from_slice(&b).ok()))
            .collect();
        out.sort_by_key(|item: &DownloadedTrack| item.track.full_title().to_lowercase());
        out
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
        Ok(())
    }
}

impl Store {
    /// The real folders, a private folder when `GLINT_DATA_DIR` is set (tests, demos), or the
    /// `data` folder next to the exe for a portable copy.
    pub fn open() -> Store {
        let dirs = match (std::env::var_os("GLINT_DATA_DIR"), portable_root()) {
            (Some(root), _) => Some(rune::dirs::AppDirs::rooted(root, APP_ID)),
            (None, Some(root)) => Some(rune::dirs::AppDirs::rooted(root, APP_ID)),
            (None, None) => {
                rune::dirs::AppDirs::new(APP_ID)
            }
        };
        let (config, cache) = match dirs {
            Some(d) => (Some(d.config().to_path_buf()), Some(d.cache().to_path_buf())),
            None => (None, None),
        };
        Store { config, cache }
    }

    /// A store that keeps nothing (headless snapshots).
    pub fn none() -> Store {
        Store { config: None, cache: None }
    }

    pub fn cache_dir(&self) -> Option<PathBuf> {
        self.cache.as_ref().map(|c| c.join("metadata"))
    }

    pub fn downloads_dir(&self) -> Option<PathBuf> {
        self.cache.as_ref().map(|c| c.join("downloads"))
    }

    fn file(&self, name: &str) -> Option<PathBuf> {
        let dir = self.config.as_ref()?;
        std::fs::create_dir_all(dir).ok()?;
        Some(dir.join(name))
    }

    pub fn load(&self) -> Settings {
        self.file("settings.json").and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice::<Settings>(&b).ok()).unwrap_or_default().sanitized()
    }

    pub fn save(&self, settings: &Settings) {
        let Some(path) = self.file("settings.json") else { return };
        if let Ok(json) = serde_json::to_vec_pretty(settings) {
            write_atomic(&path, &json);
        }
    }

    pub fn load_window(&self) -> Option<SavedWindow> {
        let bytes = std::fs::read(self.file("window.json")?).ok()?;
        serde_json::from_slice::<SavedWindow>(&bytes).ok().filter(|w| (200..=20000).contains(&w.size.0) && (200..=20000).contains(&w.size.1))
    }

    pub fn save_window(&self, window: &SavedWindow) {
        let Some(path) = self.file("window.json") else { return };
        if let Ok(json) = serde_json::to_vec_pretty(window) {
            write_atomic(&path, &json);
        }
    }

    pub fn load_token(&self) -> Option<String> {
        let path = self.file("token.bin")?;
        let sealed = std::fs::read(path).ok()?;
        let plain = unprotect(&sealed)?;
        String::from_utf8(plain).ok().filter(|t| !t.trim().is_empty())
    }

    pub fn save_token(&self, token: Option<&str>) {
        let Some(path) = self.file("token.bin") else { return };
        match token {
            Some(t) => {
                if let Some(sealed) = protect(t.as_bytes()) {
                    write_private(&path, &sealed);
                }
            }
            None => {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

/// The folder name: "Glint" on Windows, "glint" elsewhere, as each system's own apps name theirs.
const APP_ID: &str = if cfg!(windows) { "Glint" } else { "glint" };

/// A portable copy keeps everything in a `data` folder beside the exe: the zip ships a
/// `glint.portable` file next to it, the installer does not.
fn portable_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    dir.join("glint.portable").is_file().then(|| dir.join("data"))
}

/// `write_atomic` for the token: on Unix the file is made readable by its owner only, before a
/// byte of it is written.
#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = path.with_extension("tmp");
    let _ = std::fs::remove_file(&tmp);
    let written = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp).and_then(|mut f| f.write_all(bytes));
    if written.is_err() || std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) {
    write_atomic(path, bytes);
}

fn write_atomic(path: &Path, bytes: &[u8]) {
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

fn write_atomic_result(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

#[cfg(windows)]
fn protect(plain: &[u8]) -> Option<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData};
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: plain.len() as u32, pbData: plain.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        let ok = CryptProtectData(&input, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out);
        if ok == 0 || out.pbData.is_null() {
            return None;
        }
        let sealed = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(out.pbData as _);
        Some(sealed)
    }
}

#[cfg(windows)]
fn unprotect(sealed: &[u8]) -> Option<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData};
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: sealed.len() as u32, pbData: sealed.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        let ok = CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out);
        if ok == 0 || out.pbData.is_null() {
            return None;
        }
        let plain = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(out.pbData as _);
        Some(plain)
    }
}

// Elsewhere the file itself is the protection: `save_token` makes it readable by its owner only.
#[cfg(not(windows))]
fn protect(plain: &[u8]) -> Option<Vec<u8>> {
    Some(plain.to_vec())
}

#[cfg(not(windows))]
fn unprotect(sealed: &[u8]) -> Option<Vec<u8>> {
    Some(sealed.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> Store {
        let dir = std::env::temp_dir().join(format!("glint-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Store { config: Some(dir.join("config")), cache: Some(dir.join("cache")) }
    }

    #[test]
    fn settings_round_trip_and_sanitize() {
        let store = scratch("settings");
        assert_eq!(store.load(), Settings::default());
        assert!(Settings::default().word_lyrics, "word-by-word lyrics are enabled by default");
        assert!(Settings::default().fkcensor, "FckCensor is enabled by default");
        let mut s = Settings { accent: 3, volume: 0.4, ..Settings::default() };
        s.eq_gains[2] = 5.5;
        store.save(&s);
        assert_eq!(store.load(), s);

        let wild = Settings { accent: 99, volume: f32::NAN, eq_gains: [100.0; 10], ..Settings::default() }.sanitized();
        assert_eq!(wild.accent, crate::theme::ACCENTS.len() - 1);
        assert_eq!(wild.volume, 0.8);
        assert!(wild.eq_gains.iter().all(|g| *g == 12.0));
    }

    #[test]
    fn the_search_history_is_kept_tidy() {
        let s = Settings { search_history: vec!["  neon ".into(), "".into(), "   ".into(), "x".repeat(300)].into_iter().chain((0..20).map(|i| format!("q{i}"))).collect(), ..Settings::default() }.sanitized();
        assert_eq!(s.search_history[0], "neon");
        assert_eq!(s.search_history[1].chars().count(), 120);
        assert_eq!(s.search_history.len(), crate::app::SEARCH_HISTORY);
        // An older settings file has no history at all.
        let old: Settings = serde_json::from_str(r#"{"volume":0.5}"#).unwrap();
        assert!(old.search_history.is_empty());
    }

    #[test]
    fn a_corrupt_file_means_defaults() {
        let store = scratch("corrupt");
        let path = store.file("settings.json").unwrap();
        std::fs::write(path, b"{ not json").unwrap();
        assert_eq!(store.load(), Settings::default());
    }

    #[test]
    fn downloaded_tracks_keep_audio_metadata_and_lyrics_together() {
        let store = scratch("downloads");
        let downloads = DownloadStore::open(store.downloads_dir().unwrap()).unwrap();
        let track = ym_api::Track { id: "track/1".into(), title: "Offline song".into(), ..Default::default() };
        let lyrics = ym_api::Lyrics::from_lrc("[00:01.00]hello", ym_api::LyricsSource::Yandex);
        downloads.put_audio("track/1-Best-original", b"audio").unwrap();
        downloads.put(&DownloadedTrack { track: track.clone(), lyrics: lyrics.clone(), ext: "mp3".into() }).unwrap();
        assert_eq!(&*downloads.get_audio("track/1-Best-original").unwrap(), b"audio");
        let item = downloads.get("track/1").unwrap();
        assert_eq!(item.track, track);
        assert_eq!(item.lyrics, lyrics);
        assert_eq!(downloads.tracks().len(), 1);
        let (audio_bytes, metadata_bytes) = downloads.usage();
        assert_eq!(audio_bytes, 5, "audio-* files count as audio");
        assert_eq!(metadata_bytes, std::fs::metadata(downloads.metadata_path("track/1")).unwrap().len(), "track-*.json counts as metadata");
        assert!(metadata_bytes > 0);
        downloads.remove("track/1").unwrap();
        assert!(downloads.get("track/1").is_none());
        assert!(downloads.get_audio("track/1-Best-original").is_none());
        downloads.clear().unwrap();
        assert!(downloads.tracks().is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn the_token_is_encrypted_at_rest() {
        let store = scratch("token");
        assert_eq!(store.load_token(), None);
        store.save_token(Some("y0_AgAAAAexample-token"));
        let on_disk = std::fs::read(store.file("token.bin").unwrap()).unwrap();
        assert!(!String::from_utf8_lossy(&on_disk).contains("example-token"), "token must not be stored in clear text");
        assert_eq!(store.load_token().as_deref(), Some("y0_AgAAAAexample-token"));
        store.save_token(None);
        assert_eq!(store.load_token(), None);
    }
}
