//! lrc.red: lyrics timed per word. Its lyrics come as TTML, a small XML dialect:
//!
//! ```xml
//! <div begin="0:12.0" end="0:20.5" lrc:songPart="Verse">
//!   <p begin="0:12.000" end="0:15.200" ttm:agent="v1">
//!     <span begin="0:12.000" end="0:12.400">Hel</span><span begin="0:12.400" end="0:12.900">lo</span>
//!     <span begin="0:13.100" end="0:13.800">there</span>
//!     <span ttm:role="x-bg"><span begin="0:14.000" end="0:14.600">(ooh)</span></span>
//!   </p>
//! </div>
//! ```
//!
//! Spans with nothing between them are the syllables of one word; whitespace ends the word.
//! Background vocals sit in a span with `ttm:role="x-bg"`. The parser below is small and strict
//! about limits (nesting, line and word counts) because the input comes off the network.

use serde::Deserialize;

use crate::lrc::{LyricLine, LyricWord, Lyrics, LyricsSource, join_words};

const MAX_LINES: usize = 3000;
const MAX_WORDS_PER_LINE: usize = 400;
const MAX_DEPTH: usize = 48;
/// Longest contact string we put in a User-Agent.
const MAX_CONTACT: usize = 160;

/// The site that serves the lyrics, for credit links.
pub const HOME: &str = "https://lrc.red";

/// The page of a track at lrc.red.
pub fn page_url(isrc: &str) -> String {
    format!("{HOME}/s/{isrc}")
}

/// ISRCs end up in a URL path: twelve letters and digits, nothing else.
pub fn valid_isrc(s: &str) -> bool {
    s.len() == 12 && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// The User-Agent lrc.red asks for: the app, and a way to reach whoever runs it. `contact` is
/// whatever the person typed (a link, an address); it is cut down to plain characters so it
/// cannot smuggle a header break into the request. `None` when there is nothing usable.
pub fn user_agent(app_version: &str, contact: &str) -> Option<String> {
    let plain: String = contact.chars().filter(|c| c.is_ascii_alphanumeric() || " @.:/;,_-+~?=&#%".contains(*c)).take(MAX_CONTACT).collect();
    let plain = plain.split_whitespace().collect::<Vec<_>>().join(" ");
    if plain.len() < 5 || !plain.contains(['.', '@']) {
        return None;
    }
    Some(format!("Glint/{app_version} (+{plain})"))
}

/// One entry of lrc.red's `search.json`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Hit {
    pub isrc: String,
    pub title: String,
    pub artist: String,
    /// Seconds.
    pub duration: Option<f64>,
}

// ---------------------------------------------------------------------------------------------
// TTML

/// Parses a TTML document into word-timed [`Lyrics`]. `None` when it holds no lines.
pub fn parse_ttml(src: &str) -> Option<Lyrics> {
    let toks = tokenize(src)?;
    let mut lines = build(&toks)?;
    if lines.is_empty() {
        return None;
    }
    lines.truncate(MAX_LINES);
    lines.sort_by_key(|l| l.time_ms);
    let word_synced = lines.iter().any(|l| l.words.len() >= 2);
    Some(Lyrics { lines, synced: true, word_synced, instrumental: false, source: LyricsSource::LrcRed, link: None })
}

/// `12.5`, `1:02.345`, `1:02:03.5`, `12.5s`, `250ms` to milliseconds.
pub fn parse_clock(s: &str) -> Option<u32> {
    let s = s.trim();
    let (num, unit) = if let Some(v) = s.strip_suffix("ms") {
        (v, 1.0)
    } else if let Some(v) = s.strip_suffix('s') {
        (v, 1000.0)
    } else {
        (s, 0.0)
    };
    if unit > 0.0 {
        let v = plain_number(num)?;
        return to_ms(v * unit / 1000.0);
    }
    let parts: Vec<&str> = s.split(':').collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let (secs, head) = parts.split_last()?;
    let mut total = plain_number(secs)?;
    for (i, p) in head.iter().rev().enumerate() {
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        total += p.parse::<f64>().ok()? * if i == 0 { 60.0 } else { 3600.0 };
    }
    to_ms(total)
}

fn plain_number(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return None;
    }
    s.parse().ok()
}

fn to_ms(secs: f64) -> Option<u32> {
    (secs.is_finite() && (0.0..86_400.0).contains(&secs)).then(|| (secs * 1000.0).round() as u32)
}

// -- tokens -----------------------------------------------------------------------------------

#[derive(Debug)]
enum Tok {
    Open { name: String, attrs: Vec<(String, String)>, empty: bool },
    Close,
    Text(String),
}

/// `ttm:role` to `role`.
fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let decoded = after.find(';').filter(|j| *j <= 10).and_then(|j| {
            let ent = &after[..j];
            let ch = match ent {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => ent
                    .strip_prefix("#x")
                    .or_else(|| ent.strip_prefix("#X"))
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                    .and_then(char::from_u32),
            };
            ch.map(|c| (c, j))
        });
        match decoded {
            Some((c, j)) => {
                out.push(c);
                rest = &after[j + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn tokenize(src: &str) -> Option<Vec<Tok>> {
    let mut out = Vec::new();
    let mut rest = src.trim_start_matches('\u{feff}');
    while !rest.is_empty() {
        let Some(i) = rest.find('<') else {
            out.push(Tok::Text(unescape(rest)));
            break;
        };
        if i > 0 {
            out.push(Tok::Text(unescape(&rest[..i])));
        }
        rest = &rest[i..];
        if let Some(r) = rest.strip_prefix("<!--") {
            rest = &r[r.find("-->")? + 3..];
        } else if let Some(r) = rest.strip_prefix("<![CDATA[") {
            let j = r.find("]]>")?;
            out.push(Tok::Text(r[..j].to_owned()));
            rest = &r[j + 3..];
        } else if let Some(r) = rest.strip_prefix("<?") {
            rest = &r[r.find("?>")? + 2..];
        } else if let Some(r) = rest.strip_prefix("<!") {
            let j = r.find('>')?;
            if r[..j].contains('[') {
                return None; // a DOCTYPE with an internal subset: entity tricks, not lyrics
            }
            rest = &r[j + 1..];
        } else if let Some(r) = rest.strip_prefix("</") {
            let j = r.find('>')?;
            out.push(Tok::Close);
            rest = &r[j + 1..];
        } else {
            let (tok, used) = open_tag(&rest[1..])?;
            out.push(tok);
            rest = &rest[1 + used..];
        }
        if out.len() > 400_000 {
            return None;
        }
    }
    Some(out)
}

/// Parses the inside of a start tag (after `<`); returns the token and how many bytes it used.
fn open_tag(s: &str) -> Option<(Tok, usize)> {
    let b = s.as_bytes();
    let is_ws = |c: u8| c.is_ascii_whitespace();
    let mut i = 0;
    while i < b.len() && !is_ws(b[i]) && b[i] != b'/' && b[i] != b'>' {
        i += 1;
    }
    let name = local(&s[..i]).to_ascii_lowercase();
    if name.is_empty() {
        return None;
    }
    let mut attrs = Vec::new();
    loop {
        while i < b.len() && is_ws(b[i]) {
            i += 1;
        }
        match b.get(i)? {
            b'>' => return Some((Tok::Open { name, attrs, empty: false }, i + 1)),
            b'/' => {
                return (b.get(i + 1) == Some(&b'>')).then_some((Tok::Open { name, attrs, empty: true }, i + 2));
            }
            _ => {}
        }
        let start = i;
        while i < b.len() && !is_ws(b[i]) && !matches!(b[i], b'=' | b'>' | b'/') {
            i += 1;
        }
        if i == start {
            return None;
        }
        let key = local(&s[start..i]).to_owned();
        while i < b.len() && is_ws(b[i]) {
            i += 1;
        }
        let mut value = String::new();
        if b.get(i) == Some(&b'=') {
            i += 1;
            while i < b.len() && is_ws(b[i]) {
                i += 1;
            }
            match b.get(i)? {
                q @ (b'"' | b'\'') => {
                    let from = i + 1;
                    let len = s[from..].find(*q as char)?;
                    value = unescape(&s[from..from + len]);
                    i = from + len + 1;
                }
                _ => {
                    let from = i;
                    while i < b.len() && !is_ws(b[i]) && b[i] != b'>' {
                        i += 1;
                    }
                    value = unescape(&s[from..i]);
                }
            }
        }
        attrs.push((key, value));
    }
}

fn attr<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

// -- lines ------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Ctx {
    Div,
    Line,
    Backing,
    Word,
    Other,
}

#[derive(Default)]
struct Builder {
    begin: Option<u32>,
    end: Option<u32>,
    words: Vec<LyricWord>,
    backing: Vec<LyricWord>,
    /// Text that sits straight in the line with no timed span around it (lines timed as a whole).
    loose: String,
    part: Option<String>,
}

struct Open {
    start: u32,
    end: Option<u32>,
    text: String,
}

fn build(toks: &[Tok]) -> Option<Vec<LyricLine>> {
    let mut stack: Vec<Ctx> = Vec::new();
    let mut lines: Vec<LyricLine> = Vec::new();
    let mut part: Option<String> = None;
    let mut cur: Option<Builder> = None;
    let mut word: Option<Open> = None;
    let mut backing_depth = 0usize;

    for tok in toks {
        match tok {
            Tok::Open { name, attrs, empty } => {
                if *empty {
                    if name == "br" {
                        space(&mut cur, backing_depth > 0);
                    }
                    continue;
                }
                if stack.len() >= MAX_DEPTH {
                    return None;
                }
                let ctx = match name.as_str() {
                    "div" => {
                        part = attr(attrs, "songPart").map(str::trim).filter(|p| !p.is_empty()).map(str::to_owned);
                        Ctx::Div
                    }
                    "p" => {
                        cur = Some(Builder {
                            begin: attr(attrs, "begin").and_then(parse_clock),
                            end: attr(attrs, "end").and_then(parse_clock),
                            part: part.take(),
                            ..Builder::default()
                        });
                        Ctx::Line
                    }
                    "span" if cur.is_some() => {
                        if attr(attrs, "role") == Some("x-bg") {
                            backing_depth += 1;
                            Ctx::Backing
                        } else if word.is_none()
                            && let Some(start) = attr(attrs, "begin").and_then(parse_clock)
                        {
                            word = Some(Open { start, end: attr(attrs, "end").and_then(parse_clock), text: String::new() });
                            Ctx::Word
                        } else {
                            Ctx::Other
                        }
                    }
                    "br" => {
                        space(&mut cur, backing_depth > 0);
                        Ctx::Other
                    }
                    _ => Ctx::Other,
                };
                stack.push(ctx);
            }
            Tok::Text(t) => {
                if let Some(w) = word.as_mut() {
                    w.text.push_str(t);
                } else if let Some(b) = cur.as_mut() {
                    if t.trim().is_empty() {
                        if !t.is_empty() {
                            space(&mut cur, backing_depth > 0);
                        }
                    } else if backing_depth == 0 {
                        b.loose.push_str(t);
                    }
                }
            }
            Tok::Close => match stack.pop() {
                Some(Ctx::Word) => {
                    if let (Some(w), Some(b)) = (word.take(), cur.as_mut()) {
                        push_word(b, w, backing_depth > 0);
                    }
                }
                Some(Ctx::Backing) => backing_depth = backing_depth.saturating_sub(1),
                Some(Ctx::Line) => {
                    if let Some(line) = cur.take().and_then(finish) {
                        lines.push(line);
                    }
                    backing_depth = 0;
                    word = None;
                }
                Some(Ctx::Div) => part = None,
                Some(Ctx::Other) | None => {}
            },
        }
    }
    Some(lines)
}

/// Whitespace between spans: the word before it is complete.
fn space(cur: &mut Option<Builder>, backing: bool) {
    if let Some(b) = cur.as_mut() {
        let list = if backing { &mut b.backing } else { &mut b.words };
        if let Some(last) = list.last_mut() {
            last.space_after = true;
        }
    }
}

fn push_word(b: &mut Builder, w: Open, backing: bool) {
    let list = if backing { &mut b.backing } else { &mut b.words };
    if list.len() >= MAX_WORDS_PER_LINE {
        return;
    }
    let text = w.text.trim();
    if text.is_empty() {
        return;
    }
    // Whitespace inside the span, at either end, separates it from its neighbours.
    if w.text.starts_with(char::is_whitespace)
        && let Some(prev) = list.last_mut()
    {
        prev.space_after = true;
    }
    list.push(LyricWord { start_ms: w.start, end_ms: w.end.unwrap_or(w.start).max(w.start), text: text.to_owned(), space_after: w.text.ends_with(char::is_whitespace) });
}

/// Words with no end of their own last until the next one starts (or the line ends).
fn close_gaps(words: &mut [LyricWord], line_end: Option<u32>) {
    for i in 0..words.len() {
        if words[i].end_ms <= words[i].start_ms {
            let next = words.get(i + 1).map(|n| n.start_ms).filter(|n| *n > words[i].start_ms);
            if let Some(e) = next.or(line_end.filter(|e| *e > words[i].start_ms)) {
                words[i].end_ms = e;
            }
        }
    }
}

fn finish(mut b: Builder) -> Option<LyricLine> {
    if b.words.is_empty() && !b.backing.is_empty() {
        // A line that is only background vocals is just a line.
        b.words = std::mem::take(&mut b.backing);
    }
    let loose = b.loose.split_whitespace().collect::<Vec<_>>().join(" ");
    if b.words.is_empty() && loose.is_empty() {
        return None;
    }
    close_gaps(&mut b.words, b.end);
    close_gaps(&mut b.backing, b.end);
    let start = b.begin.or_else(|| b.words.first().map(|w| w.start_ms)).or_else(|| b.backing.first().map(|w| w.start_ms))?;
    let end = b.end.or_else(|| b.words.last().map(|w| w.end_ms));
    if let Some(last) = b.words.last_mut() {
        last.space_after = false;
    }
    let text = if b.words.is_empty() { loose } else { join_words(&b.words) };
    Some(LyricLine { time_ms: Some(start), text, end_ms: end, words: b.words, backing: b.backing, part: b.part })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<tt xmlns="http://www.w3.org/ns/ttml" xmlns:ttm="http://www.w3.org/ns/ttml#metadata" xmlns:lrc="https://lrc.red/ns" lrc:timing="Word" xml:lang="en">
  <head><metadata><ttm:agent type="person" xml:id="v1"/><audio lyricOffset="0.147"/></metadata></head>
  <body dur="3:20.000">
    <div begin="0:05.000" end="0:20.000" lrc:songPart="Verse">
      <p begin="0:05.000" end="0:08.000" lrc:key="L1" ttm:agent="v1">
        <span begin="0:05.000" end="0:05.400">Hel</span><span begin="0:05.400" end="0:05.900">lo</span>
        <span begin="0:06.100" end="0:06.800">there</span>
        <span ttm:role="x-bg"><span begin="0:07.000" end="0:07.600">(ooh,</span> <span begin="0:07.600" end="0:08.000">ooh)</span></span>
      </p>
      <p begin="0:09.000" end="0:12.000">
        <span begin="0:09.000" end="0:09.500">Rock&amp;</span> <span begin="0:09.500" end="0:10.000">Roll</span>
      </p>
    </div>
    <div begin="1:00.000" end="1:10.000" lrc:songPart="Chorus">
      <p begin="1:00.000" end="1:04.000"><span begin="1:00.000" end="1:02.000">Sing </span><span begin="1:02.000" end="1:04.000">along</span></p>
    </div>
  </body>
</tt>"#;

    #[test]
    fn parses_words_syllables_sections_and_backing_vocals() {
        let l = parse_ttml(SAMPLE).expect("lyrics");
        assert!(l.synced && l.word_synced);
        assert_eq!(l.source, LyricsSource::LrcRed);
        assert_eq!(l.lines.len(), 3);

        let first = &l.lines[0];
        assert_eq!(first.text, "Hello there");
        assert_eq!(first.time_ms, Some(5_000));
        assert_eq!(first.end_ms, Some(8_000));
        assert_eq!(first.part.as_deref(), Some("Verse"));
        let units: Vec<_> = first.words.iter().map(|w| (w.text.as_str(), w.space_after)).collect();
        assert_eq!(units, vec![("Hel", false), ("lo", true), ("there", false)], "syllables join, the last unit has no trailing space");
        assert_eq!(first.words[1].start_ms, 5_400);
        assert_eq!(first.backing_text(), "(ooh, ooh)");

        let second = &l.lines[1];
        assert_eq!(second.text, "Rock& Roll", "entities decode");
        assert_eq!(second.part, None, "only the first line of a section carries its label");

        let chorus = &l.lines[2];
        assert_eq!(chorus.text, "Sing along", "a space inside a span separates it from the next");
        assert_eq!(chorus.part.as_deref(), Some("Chorus"));
    }

    #[test]
    fn lines_timed_as_a_whole_still_parse() {
        let src = r#"<tt><body><div><p begin="0:01.0" end="0:03.0">Just a line</p><p begin="0:04.0">Another</p></div></body></tt>"#;
        let l = parse_ttml(src).unwrap();
        assert!(l.synced && !l.word_synced);
        assert_eq!(l.lines[0].text, "Just a line");
        assert!(l.lines[0].words.is_empty());
        assert_eq!(l.lines[1].time_ms, Some(4_000));
    }

    #[test]
    fn a_line_of_only_backing_vocals_becomes_a_line() {
        let src = r#"<tt><body><div><p begin="1.0" end="2.0"><span role="x-bg"><span begin="1.0" end="2.0">ooh</span></span></p></div></body></tt>"#;
        let l = parse_ttml(src).unwrap();
        assert_eq!(l.lines[0].text, "ooh");
        assert!(l.lines[0].backing.is_empty());
    }

    #[test]
    fn missing_word_ends_run_to_the_next_word() {
        let src = r#"<tt><body><div><p begin="1.0" end="4.0"><span begin="1.0">a</span> <span begin="2.0">b</span></p></div></body></tt>"#;
        let l = parse_ttml(src).unwrap();
        assert_eq!(l.lines[0].words[0].end_ms, 2_000);
        assert_eq!(l.lines[0].words[1].end_ms, 4_000);
    }

    #[test]
    fn lines_come_out_sorted() {
        let src = r#"<tt><body><div><p begin="9.0"><span begin="9.0" end="9.5">b</span></p><p begin="2.0"><span begin="2.0" end="2.5">a</span></p></div></body></tt>"#;
        let l = parse_ttml(src).unwrap();
        assert_eq!(l.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
    }

    #[test]
    fn garbage_and_hostile_input_is_refused() {
        assert!(parse_ttml("").is_none());
        assert!(parse_ttml("not xml at all").is_none());
        assert!(parse_ttml("<tt><body></body></tt>").is_none());
        assert!(parse_ttml(r#"<!DOCTYPE tt [<!ENTITY a "aaaa">]><tt><body><div><p begin="1"><span begin="1" end="2">&a;</span></p></div></body></tt>"#).is_none());
        let deep = "<div>".repeat(MAX_DEPTH + 5);
        assert!(parse_ttml(&deep).is_none());
        assert!(parse_ttml("<tt><p begin=\"1\" ").is_none(), "an unterminated tag must not panic");
    }

    #[test]
    fn clocks() {
        assert_eq!(parse_clock("5.123"), Some(5_123));
        assert_eq!(parse_clock("1:02.345"), Some(62_345));
        assert_eq!(parse_clock("1:02:03.5"), Some(3_723_500));
        assert_eq!(parse_clock("12.5s"), Some(12_500));
        assert_eq!(parse_clock("250ms"), Some(250));
        assert_eq!(parse_clock("0:00.000"), Some(0));
        assert_eq!(parse_clock(""), None);
        assert_eq!(parse_clock("-1"), None);
        assert_eq!(parse_clock("inf"), None);
        assert_eq!(parse_clock("1:2:3:4"), None);
        assert_eq!(parse_clock("a:12"), None);
        assert_eq!(parse_clock("99999999"), None);
    }

    #[test]
    fn user_agent_names_the_app_and_keeps_the_contact_plain() {
        let ua = user_agent("0.1.0", "https://example.org/rune ; me@example.org").unwrap();
        assert_eq!(ua, "Glint/0.1.0 (+https://example.org/rune ; me@example.org)");
        let evil = user_agent("0.1.0", "me@example.org\r\nX-Evil: 1<script>").unwrap();
        assert!(!evil.contains(['\r', '\n', '<']));
        assert!(user_agent("0.1.0", "").is_none());
        assert!(user_agent("0.1.0", "   ").is_none());
        assert!(user_agent("0.1.0", "nothing useful").is_none(), "needs something that looks like an address or a link");
        assert!(user_agent("0.1.0", &"a.".repeat(500)).unwrap().len() < 200);
    }

    #[test]
    fn isrcs_are_plain() {
        assert!(valid_isrc("GBAYE0601498"));
        assert!(!valid_isrc("GBAYE060149"));
        assert!(!valid_isrc("../etc/passw"));
        assert!(!valid_isrc("GBAYE0601498/x"));
    }
}
