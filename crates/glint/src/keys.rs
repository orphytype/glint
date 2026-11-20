//! The keys Glint answers to. Each action has a default; the user can give it another key in
//! Settings, and what they chose is the only thing saved (`Settings::keys`, action id to chord).
//!
//! Chords are RUNE's: "mod+k" is Ctrl+K on Windows and Linux and ⌘K on a Mac. A chord matches by
//! the character the key types, and on a Russian layout by the key's place on a US keyboard, so
//! "l" is the same key on both. Escape always goes back or closes the panel on top, and cannot be
//! changed.

use rune::{Chord, ChordKey, ChordMod, Key, KeyEvent, KeyPlatform, Keybind, Mods, Shortcut, Tx};

use crate::app::{Page, State};
use crate::i18n::S;
use crate::store::Settings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    PlayPause,
    SeekBack,
    SeekForward,
    Prev,
    Next,
    VolumeUp,
    VolumeDown,
    Like,
    Mute,
    NowPlaying,
    Search,
    PageBack,
    PageForward,
}

impl Act {
    /// In the order Settings lists them.
    pub const ALL: [Act; 13] = [
        Act::PlayPause,
        Act::SeekBack,
        Act::SeekForward,
        Act::Prev,
        Act::Next,
        Act::VolumeUp,
        Act::VolumeDown,
        Act::Like,
        Act::Mute,
        Act::NowPlaying,
        Act::Search,
        Act::PageBack,
        Act::PageForward,
    ];

    /// The name the action has in the settings file.
    pub fn id(self) -> &'static str {
        match self {
            Act::PlayPause => "play-pause",
            Act::SeekBack => "seek-back",
            Act::SeekForward => "seek-forward",
            Act::Prev => "previous",
            Act::Next => "next",
            Act::VolumeUp => "volume-up",
            Act::VolumeDown => "volume-down",
            Act::Like => "like",
            Act::Mute => "mute",
            Act::NowPlaying => "now-playing",
            Act::Search => "search",
            Act::PageBack => "page-back",
            Act::PageForward => "page-forward",
        }
    }

    pub fn from_id(id: &str) -> Option<Act> {
        Act::ALL.into_iter().find(|a| a.id() == id)
    }

    pub fn name(self) -> S {
        match self {
            Act::PlayPause => S::ScPlay,
            Act::SeekBack => S::ScSeekBack,
            Act::SeekForward => S::ScSeekForward,
            Act::Prev => S::ScPrev,
            Act::Next => S::ScNext,
            Act::VolumeUp => S::ScVolumeUp,
            Act::VolumeDown => S::ScVolumeDown,
            Act::Like => S::ScLike,
            Act::Mute => S::ScMute,
            Act::NowPlaying => S::ScNowPlaying,
            Act::Search => S::ScSearch,
            Act::PageBack => S::ScPageBack,
            Act::PageForward => S::ScPageForward,
        }
    }

    /// What the action is bound to until the user says otherwise. Search keeps the two
    /// shortcuts other apps use for it as well.
    fn default_spec(self) -> &'static str {
        match self {
            Act::PlayPause => "space",
            Act::SeekBack => "left",
            Act::SeekForward => "right",
            Act::Prev => "mod+left",
            Act::Next => "mod+right",
            Act::VolumeUp => "up",
            Act::VolumeDown => "down",
            Act::Like => "l",
            Act::Mute => "m",
            Act::NowPlaying => "f",
            Act::Search => "mod+k, mod+f, mod+l",
            Act::PageBack => "alt+left",
            Act::PageForward => "alt+right",
        }
    }

    pub fn default_bind(self) -> Keybind {
        self.default_spec().parse().unwrap_or_default()
    }

    /// What the action is bound to now.
    pub fn bind(self, settings: &Settings) -> Keybind {
        settings.keys.get(self.id()).and_then(|spec| spec.parse().ok()).unwrap_or_else(|| self.default_bind())
    }
}

/// Drops what a hand-edited file cannot mean: an action that is not there, a chord that does not
/// parse, or a choice that is the default anyway.
pub fn sanitize(keys: &mut std::collections::BTreeMap<String, String>) {
    keys.retain(|id, spec| Act::from_id(id).is_some_and(|a| spec.parse::<Keybind>().is_ok_and(|b| b != a.default_bind())));
}

/// The action `ev` is bound to, with the chord that matched it.
fn pressed(settings: &Settings, ev: &KeyEvent, tx: &Tx) -> Option<(Act, Chord)> {
    Act::ALL.into_iter().find_map(|a| a.bind(settings).chords().iter().find(|c| c.matches_tx(ev, tx)).map(|c| (a, *c)))
}

/// Whether a text field lets the key through: only a character key held with Ctrl (⌘) can mean a
/// command there; the rest is typing, or moving the caret.
fn works_while_typing(c: &Chord) -> bool {
    (c.has(ChordMod::Primary) || c.has(ChordMod::Ctrl)) && matches!(c.key(), ChordKey::Char(_))
}

/// The keycaps of each chord an action has, one label per key: [["Ctrl", "K"], ["Ctrl", "F"]].
/// Empty when it has none.
pub fn keycaps(settings: &Settings, act: Act) -> Vec<Vec<String>> {
    act.bind(settings).chords().iter().map(|c| c.keycaps_on(KeyPlatform::current())).collect()
}

/// Whether anything is bound other than by default.
pub fn changed(settings: &Settings) -> bool {
    !settings.keys.is_empty()
}

impl State {
    /// A key press while Settings waits for the new key of `act`. Everything is taken here, so
    /// nothing else answers to the key meant for the choice.
    pub fn record_key(&mut self, act: Act, ev: &KeyEvent, tx: &Tx) -> bool {
        match ev.key {
            Key::Escape => self.rebind = None,
            Key::Backspace | Key::Delete => {
                self.set_binding(act, None);
                self.rebind = None;
            }
            _ => {
                let Some(chord) = chord_of(ev, tx) else { return true };
                let taken = Act::ALL.into_iter().filter(|a| *a != act).find(|a| a.bind(&self.settings).chords().iter().any(|c| c.same_keys(&chord, KeyPlatform::current())));
                match taken {
                    Some(other) => {
                        let text = format!("{} {}", S::KeyTaken.get(self.lang), other.name().get(self.lang));
                        self.toast(text);
                    }
                    None => {
                        self.set_binding(act, Some(chord));
                        self.rebind = None;
                    }
                }
            }
        }
        true
    }

    /// Gives `act` one chord, or none. Going back to the default takes the choice out of the file.
    pub fn set_binding(&mut self, act: Act, chord: Option<Chord>) {
        let spec = chord.map_or_else(|| "none".to_owned(), |c| c.to_string());
        let default = act.default_bind();
        if chord.is_some_and(|c| default.chords() == [c]) {
            self.settings.keys.remove(act.id());
        } else {
            self.settings.keys.insert(act.id().to_owned(), spec);
        }
        self.mark_settings_dirty();
    }

    pub fn reset_keys(&mut self) {
        self.settings.keys.clear();
        self.rebind = None;
        self.mark_settings_dirty();
    }

    /// Runs the action `ev` is bound to. False when the key means nothing here, so it goes on to
    /// whatever else would take it.
    pub fn answer_key(&mut self, ev: &KeyEvent, tx: &Tx, typing: bool) -> bool {
        let Some((act, chord)) = pressed(&self.settings, ev, tx) else { return false };
        if typing && !works_while_typing(&chord) {
            return false;
        }
        let has_track = self.current().is_some();
        match act {
            Act::Search => {
                self.go(Page::Search);
                self.focus_search = true;
            }
            Act::PageBack => self.back(),
            Act::PageForward => self.go_forward(),
            Act::VolumeUp => self.set_volume((self.player.volume + 0.05).clamp(0.0, 1.0)),
            Act::VolumeDown => self.set_volume((self.player.volume - 0.05).clamp(0.0, 1.0)),
            Act::Mute => self.toggle_mute(),
            _ if !has_track => return false,
            Act::PlayPause => self.toggle_pause(),
            Act::Next => self.next(),
            Act::Prev => self.prev(),
            Act::SeekBack | Act::SeekForward => {
                let delta: i64 = if act == Act::SeekForward { 5_000 } else { -5_000 };
                let end = self.player.dur_ms.saturating_sub(1) as i64;
                self.seek_ms((self.player.pos_ms as i64 + delta).clamp(0, end.max(0)) as u64);
            }
            Act::NowPlaying => self.set_np(!self.np_open),
            Act::Like => {
                if let Some(track) = self.current().cloned() {
                    let liking = !self.is_liked(&track.id);
                    self.toggle_like(&track);
                    if liking {
                        self.like_burst = self.now;
                        self.burst_track = track.id;
                        self.burst_on = true;
                    }
                }
            }
        }
        true
    }
}

/// The chord a key press makes, for the user to bind: the key by the character it types (or by
/// its place on a US keyboard when the layout types something else), with the modifiers held.
/// `None` for a key that cannot be bound (Enter, Tab and the like).
fn chord_of(ev: &KeyEvent, tx: &Tx) -> Option<Chord> {
    let key = match &ev.key {
        Key::Text(t) => ChordKey::Char(typed(t, tx)?),
        Key::Command(c) if c.is_ascii() => ChordKey::Char(c.to_ascii_lowercase()),
        Key::Command(_) => ChordKey::Char(placed(tx)?),
        Key::Left => ChordKey::Left,
        Key::Right => ChordKey::Right,
        Key::Up => ChordKey::Up,
        Key::Down => ChordKey::Down,
        Key::Home => ChordKey::Home,
        Key::End => ChordKey::End,
        Key::PageUp => ChordKey::PageUp,
        Key::PageDown => ChordKey::PageDown,
        Key::Insert => ChordKey::Insert,
        Key::Function(n) => ChordKey::Function(*n),
        _ => return None,
    };
    let m: Mods = ev.mods;
    let mut chord = Chord::new(key);
    // `cmd` is the platform's primary key (⌘, or Ctrl elsewhere, where it comes with `ctrl`); only
    // on a Mac is Control a key of its own.
    if m.cmd {
        chord = chord.with(ChordMod::Primary);
    } else if m.ctrl {
        chord = chord.with(ChordMod::Ctrl);
    }
    if m.alt {
        chord = chord.with(ChordMod::Alt);
    }
    if m.shift {
        chord = chord.with(ChordMod::Shift);
    }
    if m.logo && !m.cmd {
        chord = chord.with(ChordMod::Super);
    }
    Some(chord)
}

/// The one character a text key types, as the chord names it: the key's own character without
/// Shift when the layout has a Latin one, else the Latin key in its place.
fn typed(text: &str, tx: &Tx) -> Option<char> {
    let mut it = text.chars();
    let c = it.next()?;
    if it.next().is_some() {
        return None;
    }
    tx.key_base().filter(char::is_ascii).or_else(|| c.is_ascii().then(|| c.to_ascii_lowercase())).or_else(|| placed(tx))
}

/// The character the key under the finger types on a US keyboard, for a layout (a Russian one)
/// with no Latin letters of its own.
fn placed(tx: &Tx) -> Option<char> {
    let shortcut = Shortcut::from_physical_key(tx.physical_key()?, Mods::default()).ok()?;
    match Chord::parse(&shortcut.to_string()).ok()?.key() {
        ChordKey::Char(c) => Some(c),
        _ => None,
    }
}
