//! Small, dependency-free writers for the tags used by downloaded tracks.
//!
//! Keeping the tags in the audio file makes an exported download useful outside Glint. The
//! service already provides the title, artists, album, year and genre on the track model, while
//! the cover is fetched as PNG bytes before this module is called.

use ym_api::Track;

pub fn embed(audio: &[u8], ext: &str, track: &Track, cover: Option<&[u8]>) -> Vec<u8> {
    match ext {
        "flac" => embed_flac(audio, track, cover).unwrap_or_else(|| audio.to_vec()),
        "mp3" => embed_mp3(audio, track, cover),
        _ => audio.to_vec(),
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
    text_frame(&mut tag, b"TIT2", &title);
    text_frame(&mut tag, b"TPE1", &artist);
    if !album.is_empty() {
        text_frame(&mut tag, b"TALB", &album);
    }
    if let Some(year) = year {
        text_frame(&mut tag, b"TDRC", &year);
        text_frame(&mut tag, b"TYER", &year);
    }
    if let Some(genre) = genre {
        text_frame(&mut tag, b"TCON", &genre);
    }
    if let Some(image) = cover.filter(|bytes| !bytes.is_empty()) {
        let mut frame = vec![3];
        frame.extend_from_slice(b"image/png");
        frame.push(0);
        frame.push(3);
        frame.push(0);
        frame.extend_from_slice(image);
        frame_with_payload(&mut tag, b"APIC", &frame);
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

fn text_frame(tag: &mut Vec<u8>, id: &[u8; 4], value: &str) {
    let mut payload = vec![3];
    payload.extend_from_slice(value.as_bytes());
    frame_with_payload(tag, id, &payload);
}

fn frame_with_payload(tag: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
    tag.extend_from_slice(id);
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

    let mut out = Vec::with_capacity(at + 1024);
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
    (image.len() >= 24 && &image[..8] == b"\x89PNG\r\n\x1a\n").then(|| (u32::from_be_bytes([image[16], image[17], image[18], image[19]]), u32::from_be_bytes([image[20], image[21], image[22], image[23]])))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn from_syncsafe(bytes: &[u8]) -> u32 {
        bytes.iter().take(4).fold(0, |v, b| (v << 7) | u32::from(b & 0x7f))
    }

    #[test]
    fn mp3_contains_utf8_tags_and_cover() {
        let track = Track { id: "1".into(), title: "Песня".into(), artists: vec![ym_api::Artist { name: "Автор".into(), ..Default::default() }], ..Default::default() };
        let out = embed(b"audio", "mp3", &track, Some(b"png"));
        assert_eq!(&out[..4], b"ID3\x04", "ID3v2.4, where UTF-8 text is valid");
        assert_eq!(&out[10..14], b"TIT2");
        assert!(out[14..18].iter().all(|b| b & 0x80 == 0), "v2.4 frame sizes are syncsafe");
        assert_eq!(from_syncsafe(&out[14..18]) as usize, 1 + "Песня".len());
        assert!(out.windows("APIC".len()).any(|v| v == b"APIC"));
        assert!(out.windows("Песня".len()).any(|v| v == "Песня".as_bytes()));
    }

    #[test]
    fn flac_keeps_streaminfo_and_adds_vorbis_comments() {
        let track = Track { id: "1".into(), title: "Song".into(), ..Default::default() };
        let mut source = b"fLaC".to_vec();
        source.extend_from_slice(&0x8000_0022u32.to_be_bytes());
        source.extend_from_slice(&[0; 34]);
        source.extend_from_slice(b"audio");
        let out = embed(&source, "flac", &track, Some(b"png"));
        assert_eq!(&out[..4], b"fLaC");
        assert!(out.windows(b"TITLE=Song".len()).any(|v| v == b"TITLE=Song"));
        assert!(out.windows(b"image/png".len()).any(|v| v == b"image/png"));
    }

    /// How many metadata blocks of `kind` a FLAC file has.
    fn count_blocks(flac: &[u8], kind: u8) -> usize {
        let (mut at, mut count) = (4, 0);
        loop {
            let header = u32::from_be_bytes(flac[at..at + 4].try_into().unwrap());
            let last = header & 0x8000_0000 != 0;
            count += usize::from((header >> 24) as u8 & 0x7f == kind);
            at += 4 + (header & 0x00ff_ffff) as usize;
            if last {
                return count;
            }
        }
    }

    #[test]
    fn flac_replaces_existing_comments_and_pictures() {
        let track = Track { id: "1".into(), title: "New".into(), ..Default::default() };
        let mut source = b"fLaC".to_vec();
        source.extend_from_slice(&0x0000_0022u32.to_be_bytes());
        source.extend_from_slice(&[0; 34]);
        let old = vorbis_comments(&[("TITLE", "Old".to_string())]);
        source.extend_from_slice(&(((4u32) << 24) | old.len() as u32).to_be_bytes());
        source.extend_from_slice(&old);
        let picture = picture_block(b"png");
        source.extend_from_slice(&((1u32 << 31) | (6u32 << 24) | picture.len() as u32).to_be_bytes());
        source.extend_from_slice(&picture);
        source.extend_from_slice(b"audio");

        let out = embed(&source, "flac", &track, Some(b"png"));
        assert_eq!(count_blocks(&out, 4), 1, "one VORBIS_COMMENT block only");
        assert_eq!(count_blocks(&out, 6), 1, "the old picture is replaced, not doubled");
        assert!(out.windows(b"TITLE=New".len()).any(|v| v == b"TITLE=New"));
        assert!(!out.windows(b"TITLE=Old".len()).any(|v| v == b"TITLE=Old"));
    }
}
