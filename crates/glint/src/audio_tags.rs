//! Small, dependency-free writers for the tags used by downloaded tracks.
//!
//! Keeping the tags in the audio file makes an exported download useful outside Glint. The
//! service already provides the title, artists, album, year and genre on the track model, while
//! the cover is fetched as PNG bytes before this module is called.

use std::borrow::Cow;

use ym_api::Track;

/// Skip in-memory full-file copy tagging above 256 MiB so multi-hour tracks do not double RAM use.
const MAX_TAG_IN_MEMORY: usize = 256 << 20;

pub fn embed<'a>(audio: &'a [u8], ext: &str, track: &Track, cover: Option<&[u8]>) -> Cow<'a, [u8]> {
    if audio.len() > MAX_TAG_IN_MEMORY {
        return Cow::Borrowed(audio);
    }
    match ext {
        "flac" => embed_flac(audio, track, cover).map_or(Cow::Borrowed(audio), Cow::Owned),
        "mp3" => Cow::Owned(embed_mp3(audio, track, cover)),
        _ => Cow::Borrowed(audio),
    }
}

fn metadata(track: &Track) -> (String, String, Option<String>, Option<String>, String) {
    let album = track.album();
    let album_title = album.map(|a| a.title.clone()).filter(|v| !v.is_empty()).unwrap_or_default();
    let year = album.and_then(|a| a.year).map(|v| v.to_string()).or_else(|| album.and_then(|a| a.release_date.as_deref()).and_then(|v| v.get(..4)).map(str::to_owned));
    let genre = album.and_then(|a| a.genre.clone()).filter(|v| !v.is_empty());
    (track.full_title(), track.artists_line(), year, genre, album_title)
}

fn embed_mp3(audio: &[u8], track: &Track, cover: Option<&[u8]>) -> Vec<u8> {
    let (title, artist, year, genre, album) = metadata(track);
    let mut tag = Vec::new();
    text_frame(&mut tag, *b"TIT2", &title);
    text_frame(&mut tag, *b"TPE1", &artist);
    if !album.is_empty() {
        text_frame(&mut tag, *b"TALB", &album);
    }
    if let Some(year) = year {
        text_frame(&mut tag, *b"TDRC", &year);
        text_frame(&mut tag, *b"TYER", &year);
    }
    if let Some(genre) = genre {
        text_frame(&mut tag, *b"TCON", &genre);
    }
    if let Some(image) = cover.filter(|bytes| !bytes.is_empty()) {
        let mut frame = vec![3];
        frame.extend_from_slice(b"image/png");
        frame.push(0);
        frame.push(3);
        frame.push(0);
        frame.extend_from_slice(image);
        frame_with_payload(&mut tag, *b"APIC", &frame);
    }

    let mut out = Vec::with_capacity(10 + tag.len() + audio.len());
    out.extend_from_slice(b"ID3");
    // ID3v2.4: UTF-8 text (encoding 3) is only valid here, and so are the syncsafe frame sizes.
    out.extend_from_slice(&[4, 0, 0]);
    out.extend_from_slice(&syncsafe(tag.len() as u32));
    out.extend_from_slice(&tag);
    out.extend_from_slice(strip_id3(audio));
    out
}

fn text_frame(tag: &mut Vec<u8>, id: [u8; 4], value: &str) {
    let mut payload = vec![3];
    payload.extend_from_slice(value.as_bytes());
    frame_with_payload(tag, id, &payload);
}

fn frame_with_payload(tag: &mut Vec<u8>, id: [u8; 4], payload: &[u8]) {
    tag.extend_from_slice(&id);
    // ID3v2.4 frame sizes are syncsafe (seven bits per byte), unlike the plain 32-bit sizes of
    // v2.3. Writing one where the other is read makes players see a different frame length.
    tag.extend_from_slice(&syncsafe(payload.len() as u32));
    tag.extend_from_slice(&[0, 0]);
    tag.extend_from_slice(payload);
}

fn syncsafe(value: u32) -> [u8; 4] {
    [((value >> 21) & 0x7f) as u8, ((value >> 14) & 0x7f) as u8, ((value >> 7) & 0x7f) as u8, (value & 0x7f) as u8]
}

fn strip_id3(audio: &[u8]) -> &[u8] {
    if audio.len() < 10 || &audio[..3] != b"ID3" {
        return audio;
    }
    let size = (u32::from(audio[6] & 0x7f) << 21) | (u32::from(audio[7] & 0x7f) << 14) | (u32::from(audio[8] & 0x7f) << 7) | u32::from(audio[9] & 0x7f);
    let end = 10usize.saturating_add(size as usize).min(audio.len());
    &audio[end..]
}

fn embed_flac(audio: &[u8], track: &Track, cover: Option<&[u8]>) -> Option<Vec<u8>> {
    if audio.len() < 4 || &audio[..4] != b"fLaC" {
        return None;
    }
    let mut blocks = Vec::new();
    let mut at = 4;
    loop {
        if at + 4 > audio.len() {
            return None;
        }
        let header = u32::from_be_bytes([audio[at], audio[at + 1], audio[at + 2], audio[at + 3]]);
        let last = header & 0x8000_0000 != 0;
        let kind = (header >> 24) as u8 & 0x7f;
        let len = (header & 0x00ff_ffff) as usize;
        at += 4;
        if at + len > audio.len() {
            return None;
        }
        blocks.push((kind, audio[at..at + len].to_vec()));
        at += len;
        if last {
            break;
        }
    }

    // A FLAC file has at most one VORBIS_COMMENT block, and a re-tag must replace the old picture
    // instead of leaving a second one behind.
    blocks.retain(|(kind, _)| *kind != 4 && *kind != 6);

    let (title, artist, year, genre, album) = metadata(track);
    let mut comments = Vec::new();
    comments.push(("TITLE", title));
    comments.push(("ARTIST", artist));
    if !album.is_empty() {
        comments.push(("ALBUM", album));
    }
    if let Some(year) = year {
        comments.push(("DATE", year));
    }
    if let Some(genre) = genre {
        comments.push(("GENRE", genre));
    }
    let comment = vorbis_comments(&comments);
    blocks.push((4, comment));
    if let Some(image) = cover.filter(|bytes| !bytes.is_empty()) {
        blocks.push((6, picture_block(image)));
    }

    let mut out = Vec::with_capacity(audio.len() + 1024 + cover.map_or(0, |c| c.len() + 64));
    out.extend_from_slice(b"fLaC");
    for (index, (kind, data)) in blocks.iter().enumerate() {
        let last = index + 1 == blocks.len();
        let len = u32::try_from(data.len()).ok()?;
        if len > 0x00ff_ffff {
            return None;
        }
        let header = (u32::from(last) << 31) | ((*kind as u32) << 24) | len;
        out.extend_from_slice(&header.to_be_bytes());
        out.extend_from_slice(data);
    }
    out.extend_from_slice(&audio[at..]);
    Some(out)
}

fn vorbis_comments(comments: &[(&str, String)]) -> Vec<u8> {
    let vendor = b"Glint";
    let mut out = Vec::new();
    out.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    out.extend_from_slice(vendor);
    out.extend_from_slice(&(comments.len() as u32).to_le_bytes());
    for (key, value) in comments {
        let text = format!("{key}={value}");
        out.extend_from_slice(&(text.len() as u32).to_le_bytes());
        out.extend_from_slice(text.as_bytes());
    }
    out
}

fn picture_block(image: &[u8]) -> Vec<u8> {
    let (width, height) = png_size(image).unwrap_or((0, 0));
    let mime = b"image/png";
    let mut out = Vec::with_capacity(image.len() + 32);
    out.extend_from_slice(&3u32.to_be_bytes());
    out.extend_from_slice(&(mime.len() as u32).to_be_bytes());
    out.extend_from_slice(mime);
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&height.to_be_bytes());
    out.extend_from_slice(&32u32.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&(image.len() as u32).to_be_bytes());
    out.extend_from_slice(image);
    out
}

fn png_size(image: &[u8]) -> Option<(u32, u32)> {
    (image.len() >= 24 && &image[..8] == b"\x89PNG\r\n\x1a\n")
        .then(|| (u32::from_be_bytes([image[16], image[17], image[18], image[19]]), u32::from_be_bytes([image[20], image[21], image[22], image[23]])))
}

/// Reads `(title, artist, album)` from an ID3v2 or FLAC header, falling back to `stem`
/// (`"Artist - Title"` or `"Title"`).
pub fn read_tags(audio: &[u8], stem: &str) -> (String, String, Option<String>) {
    let (mut title, mut artist, mut album) = (String::new(), String::new(), None);
    if audio.len() >= 10 && &audio[..3] == b"ID3" {
        read_id3(audio, &mut title, &mut artist, &mut album);
    } else if audio.len() >= 4 && &audio[..4] == b"fLaC" {
        read_flac_vorbis(audio, &mut title, &mut artist, &mut album);
    }
    let clean_stem = stem.trim();
    if title.trim().is_empty() {
        if let Some((a, t)) = clean_stem
            .split_once(" - ")
            .or_else(|| clean_stem.split_once(" — "))
            .or_else(|| clean_stem.split_once(" – "))
        {
            if artist.trim().is_empty() {
                artist = a.trim().to_owned();
            }
            title = t.trim().to_owned();
        } else {
            title = clean_stem.to_owned();
        }
    }
    if title.is_empty() {
        title = "Track".to_owned();
    }
    (title, artist, album.filter(|s| !s.trim().is_empty()))
}

fn read_id3(audio: &[u8], title: &mut String, artist: &mut String, album: &mut Option<String>) {
    let ver = audio[3];
    if ver != 3 && ver != 4 {
        return;
    }
    let size = (u32::from(audio[6] & 0x7f) << 21)
        | (u32::from(audio[7] & 0x7f) << 14)
        | (u32::from(audio[8] & 0x7f) << 7)
        | u32::from(audio[9] & 0x7f);
    let end = 10usize.saturating_add(size as usize).min(audio.len());
    let mut at = 10usize;
    while at + 10 <= end {
        let id = &audio[at..at + 4];
        if id == [0, 0, 0, 0] {
            break;
        }
        let len = if ver == 4 {
            ((u32::from(audio[at + 4] & 0x7f) << 21)
                | (u32::from(audio[at + 5] & 0x7f) << 14)
                | (u32::from(audio[at + 6] & 0x7f) << 7)
                | u32::from(audio[at + 7] & 0x7f)) as usize
        } else {
            u32::from_be_bytes([audio[at + 4], audio[at + 5], audio[at + 6], audio[at + 7]]) as usize
        };
        at += 10;
        if len == 0 || at + len > end {
            break;
        }
        let payload = &audio[at..at + len];
        at += len;
        match id {
            b"TIT2" => *title = decode_id3_text(payload),
            b"TPE1" => *artist = decode_id3_text(payload),
            b"TALB" => *album = Some(decode_id3_text(payload)),
            _ => {}
        }
    }
}

fn decode_id3_text(payload: &[u8]) -> String {
    let Some((&enc, rest)) = payload.split_first() else { return String::new() };
    let s = match enc {
        1 | 2 => {
            let (le, bytes) = if rest.starts_with(&[0xff, 0xfe]) {
                (true, &rest[2..])
            } else if rest.starts_with(&[0xfe, 0xff]) {
                (false, &rest[2..])
            } else {
                (enc == 1, rest)
            };
            let units: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) })
                .take_while(|&u| u != 0)
                .collect();
            String::from_utf16_lossy(&units)
        }
        _ => String::from_utf8_lossy(rest).into_owned(),
    };
    s.trim_matches('\0').trim().to_owned()
}

fn read_flac_vorbis(audio: &[u8], title: &mut String, artist: &mut String, album: &mut Option<String>) {
    let mut at = 4usize;
    while at + 4 <= audio.len() {
        let header = u32::from_be_bytes([audio[at], audio[at + 1], audio[at + 2], audio[at + 3]]);
        let last = header & 0x8000_0000 != 0;
        let kind = (header >> 24) as u8 & 0x7f;
        let len = (header & 0x00ff_ffff) as usize;
        at += 4;
        if at + len > audio.len() {
            break;
        }
        if kind == 4 {
            let block = &audio[at..at + len];
            if block.len() >= 8 {
                let vlen = u32::from_le_bytes([block[0], block[1], block[2], block[3]]) as usize;
                let mut p = 4usize.saturating_add(vlen);
                if p + 4 <= block.len() {
                    let count = u32::from_le_bytes([block[p], block[p + 1], block[p + 2], block[p + 3]]) as usize;
                    p += 4;
                    for _ in 0..count {
                        if p + 4 > block.len() {
                            break;
                        }
                        let clen = u32::from_le_bytes([block[p], block[p + 1], block[p + 2], block[p + 3]]) as usize;
                        p += 4;
                        if p + clen > block.len() {
                            break;
                        }
                        if let Ok(entry) = std::str::from_utf8(&block[p..p + clen])
                            && let Some((k, v)) = entry.split_once('=')
                        {
                            let v = v.trim();
                            if !v.is_empty() {
                                if k.eq_ignore_ascii_case("TITLE") {
                                    *title = v.to_owned();
                                } else if k.eq_ignore_ascii_case("ARTIST") {
                                    *artist = v.to_owned();
                                } else if k.eq_ignore_ascii_case("ALBUM") {
                                    *album = Some(v.to_owned());
                                }
                            }
                        }
                        p += clen;
                    }
                }
            }
            break;
        }
        at += len;
        if last {
            break;
        }
    }
}

// ---- MP4 -> FLAC remuxing (for Yandex lossless `flac-mp4`) -------------------------------------------

fn mp4_be32(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at + 4)?;
    Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

fn mp4_be64(b: &[u8], at: usize) -> Option<u64> {
    Some((u64::from(mp4_be32(b, at)?) << 32) | u64::from(mp4_be32(b, at + 4)?))
}

fn mp4_children(mut b: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut out = Vec::new();
    while b.len() >= 8 {
        let size = u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize;
        let kind = [b[4], b[5], b[6], b[7]];
        let (head, size) = match size {
            0 => (8, b.len()),
            1 => match mp4_be64(b, 8).and_then(|s| usize::try_from(s).ok()) {
                Some(s) => (16, s),
                None => break,
            },
            s => (8, s),
        };
        if size < head || size > b.len() {
            break;
        }
        out.push((kind, &b[head..size]));
        b = &b[size..];
    }
    out
}

fn mp4_child(b: &[u8], kind: [u8; 4]) -> Option<&[u8]> {
    mp4_children(b).into_iter().find(|(k, _)| *k == kind).map(|(_, c)| c)
}

fn mp4_stbl_has_flac(stbl: &[u8]) -> bool {
    mp4_child(stbl, *b"stsd").and_then(|s| s.get(8..)).is_some_and(|e| mp4_child(e, *b"fLaC").is_some())
}

fn mp4_moov_has_flac(moov: &[u8]) -> bool {
    mp4_children(moov)
        .into_iter()
        .filter(|(k, _)| k == b"trak")
        .any(|(_, t)| mp4_child(t, *b"mdia").and_then(|m| mp4_child(m, *b"minf")).and_then(|m| mp4_child(m, *b"stbl")).is_some_and(mp4_stbl_has_flac))
}

/// Checks whether a local ISO BMFF (`.m4a`/`.mp4`) file holds a FLAC track (`fLaC` sample entry).
pub fn mp4_has_flac_path(path: &std::path::Path) -> bool {
    use std::io::{Read as _, Seek as _, SeekFrom};

    let Ok(mut f) = std::fs::File::open(path) else { return false };
    let Ok(len) = f.metadata().map(|m| m.len()) else { return false };
    let mut at = 0u64;
    while at + 8 <= len {
        if f.seek(SeekFrom::Start(at)).is_err() {
            return false;
        }
        let mut h = [0u8; 16];
        if f.read_exact(&mut h[..8]).is_err() {
            return false;
        }
        let (head, size) = match mp4_be32(&h, 0) {
            Some(0) => (8u64, len - at),
            Some(1) => {
                if f.read_exact(&mut h[8..]).is_err() {
                    return false;
                }
                let Some(s) = mp4_be64(&h, 8) else { return false };
                (16u64, s)
            }
            Some(s) => (8u64, u64::from(s)),
            None => return false,
        };
        if size < head || at + size > len {
            return false;
        }
        if &h[4..8] == b"moov" {
            let body_len = (size - head) as usize;
            if body_len > 16 << 20 {
                return false;
            }
            let mut moov = vec![0u8; body_len];
            if f.read_exact(&mut moov).is_err() {
                return false;
            }
            return mp4_moov_has_flac(&moov);
        }
        at += size;
    }
    false
}

/// Remuxes an unencrypted `flac-mp4` buffer into a native `.flac` stream without re-encoding.
/// Returns `None` if the buffer is too large to copy in RAM or does not contain a valid FLAC track.
pub fn remux_flac_from_mp4(mp4: &[u8]) -> Option<Vec<u8>> {
    if mp4.len() > MAX_TAG_IN_MEMORY || mp4.len() < 16 || &mp4[4..8] != b"ftyp" {
        return None;
    }
    let moov = mp4_child(mp4, *b"moov")?;
    let stbl = mp4_children(moov)
        .into_iter()
        .filter(|(k, _)| k == b"trak")
        .find_map(|(_, t)| mp4_child(t, *b"mdia").and_then(|m| mp4_child(m, *b"minf")).and_then(|m| mp4_child(m, *b"stbl")).filter(|s| mp4_stbl_has_flac(s)))?;

    let stsd = mp4_child(stbl, *b"stsd")?;
    let entry = mp4_child(stsd.get(8..)?, *b"fLaC")?;
    let dfla = mp4_child(entry.get(28..)?, *b"dfLa")?;
    let mut blocks: Vec<(u8, &[u8])> = Vec::new();
    let mut b = dfla.get(4..)?;
    while b.len() >= 4 {
        let len = ((b[1] as usize) << 16) | ((b[2] as usize) << 8) | (b[3] as usize);
        let body = b.get(4..4 + len)?;
        blocks.push((b[0] & 0x7f, body));
        if b[0] & 0x80 != 0 {
            break;
        }
        b = &b[4 + len..];
    }
    if blocks.first().map(|(kind, _)| *kind) != Some(0) {
        return None;
    }

    let stsz = mp4_child(stbl, *b"stsz")?;
    let fixed = mp4_be32(stsz, 4)?;
    let count = mp4_be32(stsz, 8)? as usize;
    let sample_size = |i: usize| -> Option<usize> {
        if fixed != 0 { Some(fixed as usize) } else { mp4_be32(stsz, 12 + 4 * i).map(|s| s as usize) }
    };

    let offsets: Vec<usize> = if let Some(co) = mp4_child(stbl, *b"stco") {
        let n = mp4_be32(co, 4)? as usize;
        (0..n).map(|i| mp4_be32(co, 8 + 4 * i).map(|o| o as usize)).collect::<Option<_>>()?
    } else {
        let co = mp4_child(stbl, *b"co64")?;
        let n = mp4_be32(co, 4)? as usize;
        (0..n).map(|i| mp4_be64(co, 8 + 8 * i).and_then(|o| usize::try_from(o).ok())).collect::<Option<_>>()?
    };

    let stsc = mp4_child(stbl, *b"stsc")?;
    let run_count = mp4_be32(stsc, 4)? as usize;
    let runs: Vec<(usize, usize)> = (0..run_count).map(|i| Some((mp4_be32(stsc, 8 + 12 * i)? as usize, mp4_be32(stsc, 12 + 12 * i)? as usize))).collect::<Option<_>>()?;

    let mut parts: Vec<(usize, usize)> = Vec::new();
    let mut sample = 0usize;
    let mut total_audio = 0usize;
    for (chunk_idx, &off) in offsets.iter().enumerate() {
        let per = runs.iter().take_while(|(first, _)| *first <= chunk_idx + 1).last().map_or(0, |r| r.1);
        let mut chunk_len = 0usize;
        for _ in 0..per.min(count.saturating_sub(sample)) {
            chunk_len = chunk_len.checked_add(sample_size(sample)?)?;
            sample += 1;
        }
        total_audio = total_audio.checked_add(chunk_len)?;
        match parts.last_mut() {
            Some((o, l)) if *o + *l == off => *l += chunk_len,
            _ => parts.push((off, chunk_len)),
        }
    }
    if sample != count {
        return None;
    }

    let mut out = Vec::with_capacity(4 + blocks.iter().map(|(_, d)| 4 + d.len()).sum::<usize>() + total_audio);
    out.extend_from_slice(b"fLaC");
    for (idx, (kind, body)) in blocks.iter().enumerate() {
        let last = if idx + 1 == blocks.len() { 0x80 } else { 0 };
        let len = u32::try_from(body.len()).ok()?;
        if len >= (1 << 24) {
            return None;
        }
        out.push(last | *kind);
        out.extend_from_slice(&len.to_be_bytes()[1..]);
        out.extend_from_slice(body);
    }
    for (off, len) in parts {
        let slice = mp4.get(off..off.checked_add(len)?)?;
        out.extend_from_slice(slice);
    }
    Some(out)
}
