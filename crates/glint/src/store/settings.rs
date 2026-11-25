//! User settings schema, defaults and migration/sanitization helpers.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
enum LegacyEffects {
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
    /// Highest quality available for the track (FLAC when offered, else the best lossy stream).
    #[serde(alias = "Lossless")]
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

/// Which layout the floating mini-player window uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MiniStyle {
    Compact,
    #[default]
    Standard,
    Cover,
}

impl MiniStyle {
    pub fn size(self) -> (f32, f32) {
        match self {
            MiniStyle::Compact => (392.0, 68.0),
            MiniStyle::Standard => (376.0, 148.0),
            MiniStyle::Cover => (256.0, 310.0),
        }
    }

    pub fn next(self) -> MiniStyle {
        match self {
            MiniStyle::Compact => MiniStyle::Standard,
            MiniStyle::Standard => MiniStyle::Cover,
            MiniStyle::Cover => MiniStyle::Compact,
        }
    }
}

/// The look's current default (see [`StageLook::v`]). Version 5 simplified stage effects to on/off.
const STAGE_V: u8 = 5;

/// How the picture behind the open player looks (`0` off, `1` on).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StageLook {
    /// Visual effects behind the cover: 0 off, 1 on.
    pub effects: u8,
    /// Legacy fields kept for migrating settings saved before `STAGE_V = 4`.
    #[serde(alias = "rays", default, skip_serializing)]
    flash: u8,
    #[serde(default, skip_serializing)]
    react: u8,
    /// Which default this look was saved against: an older look is migrated once.
    #[serde(default)]
    pub v: u8,
}

impl Default for StageLook {
    fn default() -> Self {
        StageLook { effects: 1, flash: 0, react: 0, v: STAGE_V }
    }
}

impl StageLook {
    pub fn sanitized(self) -> StageLook {
        if self.v < 2 {
            return StageLook::default();
        }
        let effects = if self.v < 4 {
            u8::from(self.flash > 0 || self.react > 0)
        } else {
            u8::from(self.effects > 0)
        };
        StageLook { effects, flash: 0, react: 0, v: STAGE_V }
    }
}

/// How many faces the lyrics can be set in.
const LYRIC_FONTS: u8 = 3;

/// The sizes the lyrics are offered in now: small is new, medium is what "smaller" was and large is
/// what "normal" was.
const LYRIC_V: u8 = 1;

/// How the lyrics look in the open player (each 0..=2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LyricLook {
    /// The words' size: small, medium, large.
    pub size: u8,
    /// Where the lines stand: on the left, in the middle, on the right.
    #[serde(default)]
    pub align: u8,
    /// The face the words are set in (`theme::lyric_face`): IBM Plex Mono, Commissioner (the
    /// default), Cormorant.
    #[serde(rename = "face")]
    pub font: u8,
    /// Which sizes `size` was saved against (see [`LYRIC_V`]).
    #[serde(default)]
    pub v: u8,
}

impl Default for LyricLook {
    fn default() -> Self {
        LyricLook { size: 1, align: 0, font: 1, v: LYRIC_V }
    }
}

impl LyricLook {
    pub fn sanitized(self) -> LyricLook {
        let size = if self.v < LYRIC_V { [1, 1, 2][usize::from(self.size.min(2))] } else { self.size.min(2) };
        LyricLook { size, align: self.align.min(2), font: self.font.min(LYRIC_FONTS - 1), v: LYRIC_V }
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub lang: Option<String>,
    /// Index into `theme::ACCENTS`; 0 follows the cover art.
    pub accent: usize,
    /// Custom GPU shaders (pre-rendered when on, off when false).
    pub shaders: bool,
    /// Legacy 3-tier effects field migrated once into `shaders`.
    #[serde(default, skip_serializing)]
    effects: Option<LegacyEffects>,
    /// CPU-only renderer with lower RAM use (applies on next launch).
    pub cpu_mode: bool,
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
    /// Each wave's own knobs, where they differ from what the wave is (`WaveTune::of`).
    pub wave_tunes: std::collections::BTreeMap<String, WaveTune>,
    /// How My Wave's shape on Home looks.
    pub wave_look: WaveLook,
    /// A tray icon with the transport controls in its menu.
    pub show_tray: bool,
    /// Look for a newer version on GitHub when Glint starts (nothing is installed unasked).
    pub auto_update: bool,
    /// Write anonymized local diagnostic logs (capped at 1 MiB) and a hardware summary.
    pub logging: bool,
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
    /// Floating mini-player layout (`Compact`, `Standard`, `Cover`).
    pub mini_style: MiniStyle,
    /// Keep the floating mini-player window above other windows.
    pub mini_always_on_top: bool,
    /// Opacity of the floating mini-player surface (`0.65..=1.0`).
    pub mini_opacity: f32,
    /// Automatically show the mini-player when closing the main window to the tray.
    pub mini_on_close: bool,
    /// Last screen position of the floating mini-player window.
    pub mini_pos: Option<(i32, i32)>,
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
            shaders: true,
            effects: None,
            cpu_mode: false,
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
            logging: true,
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
            mini_style: MiniStyle::Standard,
            mini_always_on_top: true,
            mini_opacity: 0.95,
            mini_on_close: false,
            mini_pos: None,
        }
    }
}

impl Settings {
    /// Pulls every value into its valid range, so a hand-edited file cannot break the app.
    pub fn sanitized(mut self) -> Settings {
        if let Some(old) = self.effects.take() {
            self.shaders = old != LegacyEffects::Off;
        }
        self.accent = self.accent.min(crate::theme::ACCENTS.len() - 1);
        self.volume = if self.volume.is_finite() { self.volume.clamp(0.0, 1.0) } else { 0.8 };
        self.mini_opacity = if self.mini_opacity.is_finite() { self.mini_opacity.clamp(0.65, 1.0) } else { 0.95 };
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
const WAVE_DIVERSITY: [&str; 4] = ["default", "favorite", "discover", "popular"];
const WAVE_MOOD: [&str; 5] = ["all", "active", "fun", "calm", "sad"];
const WAVE_LANGUAGE: [&str; 4] = ["any", "russian", "not-russian", "without-words"];

/// One wave's knobs: what it leans to (`WAVE_DIVERSITY`), its mood (`WAVE_MOOD`) and its language
/// (`WAVE_LANGUAGE`), each an index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WaveTune {
    pub diversity: u8,
    pub mood: u8,
    pub language: u8,
}

impl WaveTune {
    /// What the wave `id` is before it is tuned.
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

    /// Whether the wave `id` takes these knobs.
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
        ym_api::WaveSettings { diversity: WAVE_DIVERSITY[usize::from(t.diversity)], mood_energy: WAVE_MOOD[usize::from(t.mood)], language: WAVE_LANGUAGE[usize::from(t.language)] }
    }
}

/// How many forms and prints My Wave's shape can take (`WaveLook`).
const WAVE_FORMS: u8 = 4;
const WAVE_PRINTS: u8 = 4;

/// How My Wave's shape on Home looks: its form (0 liquid, 1 flower, 2 pinwheel, 3 cloud) and how
/// it is printed (0 smooth, 1 dither, 2 halftone dots, 3 lines).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WaveLook {
    pub form: u8,
    pub print: u8,
}
