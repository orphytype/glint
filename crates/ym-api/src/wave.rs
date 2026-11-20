//! "My Wave" presets: which radio station to ask and with which seeds (the mood / character /
//! language knobs of the Wave). Colours and titles belong to the UI, keyed by [`WavePreset::id`].

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WaveGroup {
    Character,
    Mood,
    Activity,
    Language,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WavePreset {
    pub id: &'static str,
    pub group: WaveGroup,
    /// `type:tag`, e.g. `user:onyourwave`.
    pub station: &'static str,
    pub seeds: &'static [&'static str],
}

macro_rules! preset {
    ($id:literal, $group:ident, $station:literal, [$($seed:literal),*]) => {
        WavePreset { id: $id, group: WaveGroup::$group, station: $station, seeds: &[$($seed),*] }
    };
}

pub const WAVE_PRESETS: &[WavePreset] = &[
    preset!("onyourwave", Character, "user:onyourwave", ["settingDiversity:default"]),
    preset!("favorite", Character, "user:onyourwave", ["settingDiversity:favorite"]),
    preset!("discover", Character, "user:onyourwave", ["settingDiversity:discover"]),
    preset!("popular", Character, "user:onyourwave", ["settingDiversity:popular"]),
    preset!("energetic", Mood, "user:onyourwave", ["settingMoodEnergy:active"]),
    preset!("calm", Mood, "user:onyourwave", ["settingMoodEnergy:calm"]),
    preset!("fun", Mood, "user:onyourwave", ["settingMoodEnergy:fun"]),
    preset!("sad", Mood, "user:onyourwave", ["settingMoodEnergy:sad"]),
    preset!("workbackground", Activity, "activity:workbackground", []),
    preset!("without-words", Language, "user:onyourwave", ["settingLanguage:without-words"]),
    preset!("russian", Language, "user:onyourwave", ["settingLanguage:russian"]),
    preset!("not-russian", Language, "user:onyourwave", ["settingLanguage:not-russian"]),
];

pub fn preset(id: &str) -> Option<&'static WavePreset> {
    WAVE_PRESETS.iter().find(|p| p.id == id)
}
