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
    /// FLAC where the account and the track have it, else the best lossy rendition.
    Lossless,
}

impl QualityPref {
    pub fn api(self) -> ym_api::Quality {
        match self {
            QualityPref::Saver => ym_api::Quality::Saver,
            QualityPref::Balanced => ym_api::Quality::Balanced,
            QualityPref::Best => ym_api::Quality::Best,
            QualityPref::Lossless => ym_api::Quality::Lossless,
        }
    }
}

/// How the open player arranges itself (`np::Layout`). `Comfortable` is the standard one (the
/// name it was saved under before there were more).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PlayerLayout {
    #[default]
    Auto,
    Comfortable,
    Compact,
    Cover,
}

/// How the picture behind the open player looks, as chosen in its panel (each 0..=2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StageLook {
    /// The burst of light from under the cover: off, subtle, vibrant.
    #[serde(alias = "rays")]
    pub flash: u8,
    /// How much the music moves the light rays: off, calm, lively.
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
    /// The words' size: small, medium, large.
    pub size: u8,
    /// Where the lines stand: on the left, in the middle, on the right.
    #[serde(default)]
    pub align: u8,
    /// The face the words are set in (`theme::lyric_face`): IBM Plex Mono, Commissioner,
    /// Cormorant (the default). Stored as `face`: 0.5 kept a `font` whose numbers meant other
    /// faces. Missing, it takes the struct's default, not 0.
    #[serde(rename = "face")]
    pub font: u8,
    /// Which sizes `size` was saved against (see [`LYRIC_V`]). A file without it is old: its own
    /// default is 0.
    #[serde(default)]
    pub v: u8,
}

impl Default for LyricLook {
    fn default() -> Self {
        LyricLook { size: 1, align: 0, font: 2, v: LYRIC_V }
    }
}

/// The user's own picture behind the open player (`backdrop.rs`): whether it is on, which copy
/// of it (bumped when another is picked), how soft it is, how much it is darkened, and the effect
/// laid over it (an index of `backdrop::EFFECTS`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BackdropLook {
    pub on: bool,
    pub rev: u32,
    pub blur: f32,
    pub dim: f32,
    pub effect: u8,
}

impl Default for BackdropLook {
    fn default() -> Self {
        BackdropLook { on: false, rev: 0, blur: 0.3, dim: 0.35, effect: 0 }
    }
}

impl BackdropLook {
    pub fn sanitized(self) -> BackdropLook {
        let unit = |x: f32, or: f32| if x.is_finite() { x.clamp(0.0, 1.0) } else { or };
        BackdropLook { blur: unit(self.blur, 0.3), dim: unit(self.dim, 0.35), effect: self.effect.min(crate::backdrop::EFFECTS.len() as u8 - 1), ..self }
    }
}

/// How many faces the lyrics can be set in.
pub const LYRIC_FONTS: u8 = 3;

/// The sizes the lyrics are offered in now: small is new, medium is what "smaller" was and large is
/// what "normal" was. A look saved before that keeps the words as big as it had them, except that
/// the old default (normal) becomes the new one (medium) and "larger", which is gone, becomes large.
const LYRIC_V: u8 = 1;

impl LyricLook {
    pub fn sanitized(self) -> LyricLook {
        let size = if self.v < LYRIC_V { [1, 1, 2][usize::from(self.size.min(2))] } else { self.size.min(2) };
        LyricLook { size, align: self.align.min(2), font: self.font.min(LYRIC_FONTS - 1), v: LYRIC_V }
    }
}

/// The look's current default (see [`StageLook::v`]). Version 3 put "off" in front of the
/// reaction's choices and dropped "wild".
const STAGE_V: u8 = 3;

impl Default for StageLook {
    fn default() -> Self {
        StageLook { flash: 1, react: 2, v: STAGE_V }
    }
}

impl StageLook {
    pub fn sanitized(self) -> StageLook {
        if self.v < 2 {
            return StageLook::default();
        }
        // Calm, lively and wild became calm, lively and lively.
        let react = if self.v == 2 { [1, 2, 2][usize::from(self.react.min(2))] } else { self.react.min(2) };
        StageLook { flash: self.flash.min(2), react, v: STAGE_V }
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
    /// Each wave's own knobs, where they differ from what the wave is (`WaveTune::of`): "Favorite"
    /// stays favourite with any mood the user gave it, and nothing carries over to another wave.
    pub wave_tunes: std::collections::BTreeMap<String, WaveTune>,
    /// How My Wave's shape on Home looks.
    pub wave_look: WaveLook,
    /// A tray icon with the transport controls in its menu.
    pub show_tray: bool,
    /// Look for a newer version on GitHub when Glint starts (nothing is installed unasked).
    pub auto_update: bool,
    /// The close button hides the window and leaves the music playing (needs the tray icon).
    pub close_to_tray: bool,
    /// Glint starts when the user signs in to the computer (`autostart.rs`).
    pub autostart: bool,
    /// Started that way, it waits in the tray instead of opening its window (needs the tray icon).
    pub autostart_hidden: bool,
    /// The butterflies that fly over Home.
    pub butterflies: bool,
    /// Ask lrc.red for lyrics timed word by word.
    pub word_lyrics: bool,
    /// Replace censored tracks with community-maintained clean versions when available.
    pub fkcensor: bool,
    /// Start playing as soon as Glint opens: last time's track where it stopped, or My Wave.
    pub play_on_launch: bool,
    /// What was searched for lately, newest first.
    pub search_history: Vec<String>,
    /// The picture behind the open player.
    pub stage: StageLook,
    /// The user's own picture behind the open player.
    pub backdrop: BackdropLook,
    /// How the lyrics look in the open player.
    pub lyric_look: LyricLook,
    /// Cover-art shortcuts kept in the navigation rail.
    pub pinned: Vec<PinnedSetting>,
    /// The keys the user changed: an action's id (`keys::Act::id`) and its chord, "none" for no
    /// key. An action that is not here has its default.
    pub keys: std::collections::BTreeMap<String, String>,
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
            wave_tunes: std::collections::BTreeMap::new(),
            wave_look: WaveLook::default(),
            show_tray: true,
            auto_update: true,
            close_to_tray: false,
            autostart: false,
            autostart_hidden: true,
            butterflies: true,
            word_lyrics: true,
            fkcensor: true,
            play_on_launch: false,
            search_history: Vec::new(),
            stage: StageLook::default(),
            backdrop: BackdropLook::default(),
            lyric_look: LyricLook::default(),
            pinned: Vec::new(),
            keys: std::collections::BTreeMap::new(),
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
        self.backdrop = self.backdrop.sanitized();
        self.lyric_look = self.lyric_look.sanitized();
        self.pinned.retain(|p| !p.kind.is_empty() && !p.id.is_empty() && !p.title.trim().is_empty());
        self.pinned.truncate(24);
        crate::keys::sanitize(&mut self.keys);
        self.wave_look = WaveLook { form: if self.wave_look.form < WAVE_FORMS { self.wave_look.form } else { 0 }, print: self.wave_look.print.min(WAVE_PRINTS - 1) };
        self.wave_tunes.retain(|id, t| {
            *t = t.sanitized();
            *t != WaveTune::of(id)
        });
        self
    }
}

/// The values of My Wave's three knobs, as the service names them, in the order they are offered.
pub const WAVE_DIVERSITY: [&str; 4] = ["default", "favorite", "discover", "popular"];
pub const WAVE_MOOD: [&str; 5] = ["all", "active", "fun", "calm", "sad"];
pub const WAVE_LANGUAGE: [&str; 4] = ["any", "russian", "not-russian", "without-words"];

/// One wave's knobs: what it leans to (`WAVE_DIVERSITY`), its mood (`WAVE_MOOD`) and its language
/// (`WAVE_LANGUAGE`), each an index. A wave starts with all three set, so none is left as the
/// account had it from another wave or another device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WaveTune {
    pub diversity: u8,
    pub mood: u8,
    pub language: u8,
}

impl WaveTune {
    /// What the wave `id` is before it is tuned: the knob its name sets ("Calm" is the calm mood,
    /// "Favorite" leans to what one likes), the others at their widest.
    pub fn of(id: &str) -> WaveTune {
        let mut t = WaveTune::default();
        let seeds: &[&str] = if id == "collection" { &["settingDiversity:favorite"] } else { ym_api::wave::preset(id).map(|p| p.seeds).unwrap_or(&[]) };
        for seed in seeds {
            let Some((knob, value)) = seed.split_once(':') else { continue };
            let at = |list: &[&str]| list.iter().position(|v| *v == value).unwrap_or(0) as u8;
            match knob {
                "settingDiversity" => t.diversity = at(&WAVE_DIVERSITY),
                "settingMoodEnergy" => t.mood = at(&WAVE_MOOD),
                "settingLanguage" => t.language = at(&WAVE_LANGUAGE),
                _ => {}
            }
        }
        t
    }

    /// Whether the wave `id` takes these knobs: My Wave's own presets do; a station seeded by a
    /// track, an artist or an activity doesn't.
    pub fn applies(id: &str) -> bool {
        id == "collection" || ym_api::wave::preset(id).is_some_and(|p| p.station == "user:onyourwave")
    }

    fn sanitized(self) -> WaveTune {
        WaveTune {
            diversity: self.diversity.min(WAVE_DIVERSITY.len() as u8 - 1),
            mood: self.mood.min(WAVE_MOOD.len() as u8 - 1),
            language: self.language.min(WAVE_LANGUAGE.len() as u8 - 1),
        }
    }

    /// The knobs as the service takes them.
    pub fn knobs(self) -> ym_api::WaveSettings {
        let t = self.sanitized();
        ym_api::WaveSettings {
            diversity: WAVE_DIVERSITY[usize::from(t.diversity)],
            mood_energy: WAVE_MOOD[usize::from(t.mood)],
            language: WAVE_LANGUAGE[usize::from(t.language)],
        }
    }
}

/// How many forms and prints My Wave's shape can take (`WaveLook`).
pub const WAVE_FORMS: u8 = 4;
pub const WAVE_PRINTS: u8 = 4;

/// How My Wave's shape on Home looks: its form (0 liquid, 1 flower, 2 pinwheel, 3 cloud) and how
/// it is printed (0 smooth, 1 dither, 2 halftone dots, 3 lines). A form that is no longer offered
/// (4 drops, 5 crystal) goes back to liquid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WaveLook {
    pub form: u8,
    pub print: u8,
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

/// Every quality a track can be downloaded in (as `ym_api::Quality` prints), best first.
pub const DOWNLOAD_QUALITIES: [&str; 4] = ["Lossless", "Best", "Balanced", "Saver"];

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

    /// The downloaded `variant` ("original" or "replacement") of track `id`, in the quality
    /// `preferred` names when there is that copy, else in any other. A track downloaded before the
    /// quality setting changed is still the user's track: offline it must play as it is.
    pub fn find_audio(&self, id: &str, preferred: &str, variant: &str) -> Option<Arc<[u8]>> {
        std::iter::once(preferred).chain(DOWNLOAD_QUALITIES.into_iter().filter(|q| *q != preferred)).find_map(|q| self.get_audio(&format!("{id}-{q}-{variant}")))
    }

    /// Moves the downloads an earlier version kept in `legacy` (another folder) into this store,
    /// leaving any file this store already has where it is, then sets the old folder aside so it is
    /// not looked through again. Returns how many files came over.
    pub fn adopt(&self, legacy: &Path) -> usize {
        let Ok(entries) = std::fs::read_dir(legacy) else { return 0 };
        let mut ours = false;
        let _guard = self.write.lock().unwrap_or_else(PoisonError::into_inner);
        let mut moved = 0;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(n) = name.to_str() else { continue };
            if !(n.starts_with("audio-") || (n.starts_with("track-") && n.ends_with(".json"))) {
                continue;
            }
            ours = true;
            let to = self.dir.join(&name);
            if to.exists() {
                continue;
            }
            if std::fs::rename(entry.path(), &to).is_ok() || (std::fs::copy(entry.path(), &to).is_ok() && std::fs::remove_file(entry.path()).is_ok()) {
                moved += 1;
            }
        }
        let aside = legacy.with_file_name("downloads-old");
        if ours && !aside.exists() {
            let _ = std::fs::rename(legacy, aside);
        }
        moved
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
        for quality in DOWNLOAD_QUALITIES {
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
        Ok(())
    }
}

impl Store {
    /// The real folders, a private folder when `GLINT_DATA_DIR` is set, or the `data` folder next to
    /// the exe for a portable copy.
    pub fn open() -> Store {
        let dirs = match (std::env::var_os("GLINT_DATA_DIR"), portable_root()) {
            (Some(root), _) => Some(rune::dirs::AppDirs::rooted(root, APP_ID)),
            (None, Some(root)) => Some(rune::dirs::AppDirs::rooted(root, APP_ID)),
            (None, None) => rune::dirs::AppDirs::new(APP_ID),
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

    /// Where the picture picked for behind the player is kept.
    pub fn backdrop_file(&self) -> Option<PathBuf> {
        self.file("backdrop.img")
    }

    pub fn cache_dir(&self) -> Option<PathBuf> {
        self.cache.as_ref().map(|c| c.join("metadata"))
    }

    pub fn downloads_dir(&self) -> Option<PathBuf> {
        self.cache.as_ref().map(|c| c.join("downloads"))
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

/// What the window showed last time, kept so the next launch opens straight onto it while the
/// fresh copy loads: the account, its picture and the home shelves (small, read before the first
/// frame), and the library (liked tracks and playlists, read on a worker).
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HomeSnapshot {
    pub account: Option<ym_api::Account>,
    pub avatar: Option<String>,
    pub home: Vec<ym_api::Shelf>,
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
    pub albums: Vec<ym_api::Album>,
    pub artists: Vec<ym_api::Artist>,
    pub liked_cover: Option<String>,
}

impl Store {
    fn snapshot_file(&self, name: &str) -> Option<PathBuf> {
        let dir = self.cache.as_ref()?;
        Some(dir.join(name))
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

    /// Writes a snapshot on a thread of its own (the library one can be megabytes).
    pub fn save_snapshot<T: Serialize + Send + 'static>(&self, name: &'static str, value: T) {
        let Some(path) = self.snapshot_file(name) else { return };
        let _ = std::thread::Builder::new().name("glint-snapshot".into()).spawn(move || {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(json) = serde_json::to_vec(&value) {
                write_atomic(&path, &json);
            }
        });
    }

    pub fn load_playback(&self) -> Option<PlaybackSnapshot> {
        let bytes = std::fs::read(self.snapshot_file("playback.json")?).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Writes what is playing now, on this thread: it is small, and on quit a thread of its own
    /// might not get to finish.
    pub fn save_playback(&self, snap: &PlaybackSnapshot) {
        let Some(path) = self.snapshot_file("playback.json") else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec(snap) {
            write_atomic(&path, &json);
        }
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

/// The folder name: "Glint" on Windows and macOS, "glint" on Linux, as each system's own apps name theirs.
const APP_ID: &str = if cfg!(any(windows, target_os = "macos")) { "Glint" } else { "glint" };

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
