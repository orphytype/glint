//! Lyrics. Line-synced ones come from LRC files: `[01:23.45] text`, several stamps on one line,
//! ID tags (`[ar:…]`), `[offset:±ms]`, with the "enhanced" per-word `<01:23.45>` marks stripped.
//! Word-synced ones come from lrc.red's TTML (see [`crate::lrcred`]) and carry [`LyricWord`]s.

use serde::{Deserialize, Serialize};

/// One sung unit of a line: a whole word, or one syllable of a word that is split up.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
pub struct LyricWord {
    pub start_ms: u32,
    pub end_ms: u32,
    pub text: String,
    /// Whether a space follows this unit (false inside a word split into syllables).
    pub space_after: bool,
}

impl LyricWord {
    /// How much of the unit has been sung at `pos_ms`, 0.0 to 1.0.
    pub fn progress(&self, pos_ms: u32) -> f32 {
        if pos_ms <= self.start_ms {
            0.0
        } else if pos_ms >= self.end_ms || self.end_ms <= self.start_ms {
            1.0
        } else {
            (pos_ms - self.start_ms) as f32 / (self.end_ms - self.start_ms) as f32
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
pub struct LyricLine {
    /// Start of the line in milliseconds, `None` for unsynced (plain) lyrics.
    pub time_ms: Option<u32>,
    /// The words. Empty for an instrumental gap that carries a stamp.
    pub text: String,
    /// When the line stops being sung, if the source says.
    pub end_ms: Option<u32>,
    /// Per-word timing of the main vocal; empty when the source only times whole lines.
    pub words: Vec<LyricWord>,
    /// Background vocals sung over this line ("(ooh)"), timed per word.
    pub backing: Vec<LyricWord>,
    /// The song section this line opens ("Verse", "Chorus"), when the source labels sections.
    pub part: Option<String>,
}

impl LyricLine {
    pub fn plain(time_ms: Option<u32>, text: impl Into<String>) -> LyricLine {
        LyricLine { time_ms, text: text.into(), ..LyricLine::default() }
    }

    /// The background vocals as one string.
    pub fn backing_text(&self) -> String {
        join_words(&self.backing)
    }
}

/// Whether `text` says nothing: it has no letter or digit in it, only dots, dashes, brackets,
/// music notes and the like ("...", "…", "♪", "- -").
pub fn is_filler(text: &str) -> bool {
    !text.chars().any(char::is_alphanumeric)
}

/// The words of `words` as a string, with a space wherever the source had one.
pub fn join_words(words: &[LyricWord]) -> String {
    let mut out = String::new();
    for (i, w) in words.iter().enumerate() {
        out.push_str(&w.text);
        if w.space_after && i + 1 < words.len() {
            out.push(' ');
        }
    }
    out
}

/// Where lyrics came from, shown as a small credit in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
pub enum LyricsSource {
    #[default]
    Yandex,
    SpicyLyrics,
    Musixmatch,
    Lrclib,
    /// lrc.red: word-synced, and its terms ask for a visible credit.
    LrcRed,
    Demo,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct Lyrics {
    pub lines: Vec<LyricLine>,
    /// True when at least one line has a time stamp.
    pub synced: bool,
    /// True when the lines carry per-word timing.
    pub word_synced: bool,
    /// The service says the track has no vocals.
    pub instrumental: bool,
    pub source: LyricsSource,
    /// The page of these lyrics at the provider, for the credit link.
    pub link: Option<String>,
}

impl Lyrics {
    pub fn from_lrc(src: &str, source: LyricsSource) -> Lyrics {
        let lines = parse(src);
        let synced = lines.iter().any(|l| l.time_ms.is_some());
        Lyrics { lines, synced, source, ..Lyrics::default() }
    }

    pub fn instrumental() -> Lyrics {
        Lyrics { instrumental: true, ..Lyrics::default() }
    }

    /// Whether any line has something to sing. A placeholder is not lyrics: sources send "...",
    /// "…" or a music note for a track they have no words for, and that counts as nothing.
    pub fn has_words(&self) -> bool {
        self.lines.iter().any(|l| !is_filler(&l.text) || l.words.iter().any(|w| !is_filler(&w.text)))
    }

    /// No words and not marked instrumental: there is nothing to show but "no lyrics".
    pub fn is_empty(&self) -> bool {
        !self.has_words() && !self.instrumental
    }

    /// The line to highlight at `pos_ms`, if the lyrics are synced.
    pub fn active(&self, pos_ms: u32) -> Option<usize> {
        if self.synced { line_at(&self.lines, pos_ms) } else { None }
    }
}

/// Index of the last line that has started by `pos_ms` (lines must be sorted by time).
pub fn line_at(lines: &[LyricLine], pos_ms: u32) -> Option<usize> {
    let n = lines.partition_point(|l| l.time_ms.is_some_and(|t| t <= pos_ms));
    n.checked_sub(1)
}

/// Parses `mm:ss`, `mm:ss.f`, `mm:ss.ff`, `mm:ss.fff` (or `mm:ss:ff`) into milliseconds.
fn parse_stamp(s: &str) -> Option<u32> {
    let (min, rest) = s.split_once(':')?;
    let min: u32 = min.trim().parse().ok()?;
    let (sec, frac) = match rest.split_once(['.', ':']) {
        Some((sec, frac)) => (sec, Some(frac)),
        None => (rest, None),
    };
    let sec: u32 = sec.trim().parse().ok()?;
    if sec >= 100 {
        return None;
    }
    let ms = match frac {
        None => 0,
        Some(f) => {
            let f = f.trim();
            if f.is_empty() || !f.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            // One digit is tenths, two are hundredths, three or more are milliseconds.
            let digits: String = f.chars().take(3).collect();
            let v: u32 = digits.parse().ok()?;
            match digits.len() {
                1 => v * 100,
                2 => v * 10,
                _ => v,
            }
        }
    };
    Some(min.saturating_mul(60_000).saturating_add(sec * 1000).saturating_add(ms))
}

/// Strips `<mm:ss.xx>` word marks from the text of an enhanced LRC line.
fn strip_word_marks(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('<') {
        let after = &rest[i + 1..];
        match after.find('>') {
            Some(j) if parse_stamp(&after[..j]).is_some() => {
                out.push_str(&rest[..i]);
                rest = &after[j + 1..];
            }
            _ => {
                out.push_str(&rest[..=i]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn parse(src: &str) -> Vec<LyricLine> {
    let mut stamped: Vec<LyricLine> = Vec::new();
    let mut plain: Vec<LyricLine> = Vec::new();
    let mut offset_ms: i64 = 0;

    for raw in src.lines() {
        let mut line = raw.trim_start_matches('\u{feff}').trim();
        let mut stamps: Vec<u32> = Vec::new();
        let mut had_tag = false;
        while let Some(rest) = line.strip_prefix('[') {
            let Some(end) = rest.find(']') else { break };
            let tag = &rest[..end];
            if let Some(ms) = parse_stamp(tag) {
                stamps.push(ms);
            } else if let Some((k, v)) = tag.split_once(':') {
                if k.trim().eq_ignore_ascii_case("offset") {
                    offset_ms = v.trim().trim_start_matches('+').parse().unwrap_or(0);
                }
                had_tag = true;
            } else {
                break;
            }
            line = rest[end + 1..].trim_start();
        }
        if stamps.is_empty() {
            // `[ar:…]` style metadata lines carry no lyric; anything else is a plain line.
            if !had_tag && !line.is_empty() {
                plain.push(LyricLine::plain(None, strip_word_marks(line).trim()));
            }
            continue;
        }
        let text = strip_word_marks(line).trim().to_owned();
        for ms in stamps {
            stamped.push(LyricLine::plain(Some(ms), text.clone()));
        }
    }

    if stamped.is_empty() {
        return plain;
    }
    // A positive offset makes the lyrics appear sooner.
    for l in &mut stamped {
        if let Some(t) = l.time_ms.as_mut() {
            *t = (i64::from(*t) - offset_ms).clamp(0, i64::from(u32::MAX)) as u32;
        }
    }
    stamped.sort_by_key(|l| l.time_ms);
    stamped
}
