//! The offline demo: a made-up catalogue with synthesised music, procedurally painted covers and
//! original lyrics, so the whole app (and every visual) can be tried without an account.
//!
//! Everything is built from the same `ym_api` models the live service produces, so the UI has
//! one code path. Demo covers use a fake host (`demo.local`) that only the demo loader reads.

use std::sync::Arc;

use ym_api::{Account, Album, Artist, ArtistPage, ImageRef, Lyrics, LyricsSource, Owner, Page, Playlist, PlaylistTrack, SearchResult, Shelf, ShelfItem, ShelfKind, Track};
use ym_audio::synth::Style;

use crate::palette::oklch_to_srgb;

pub const HOST: &str = "demo.local";

pub struct Catalog {
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub tracks: Vec<Track>,
    pub playlists: Vec<Playlist>,
    pub liked: Vec<String>,
}

struct ArtistSpec {
    name: &'static str,
    style: Style,
    hue: f32,
    russian: bool,
    albums: [(&'static str, [&'static str; 4]); 2],
}

const SPECS: [ArtistSpec; 8] = [
    ArtistSpec {
        name: "Nova Kestrel",
        style: Style::Synthwave,
        hue: 318.0,
        russian: false,
        albums: [
            ("Chrome Horizon", ["Afterglow Drive", "Laser Garden", "Midnight Overpass", "Pulse of the Machine"]),
            ("Static Hearts", ["Neon Confession", "Glass Highway", "Parallel Summer", "Last Arcade"]),
        ],
    },
    ArtistSpec {
        name: "Lumen Drift",
        style: Style::Ambient,
        hue: 205.0,
        russian: false,
        albums: [("Slow Light", ["Tidepool", "Soft Aurora", "Hollow Moon", "Breathing Room"]), ("Paper Clouds", ["Drift Theory", "Quiet Orbit", "Lantern Field", "Weightless"])],
    },
    ArtistSpec {
        name: "Mira & the Static",
        style: Style::House,
        hue: 22.0,
        russian: false,
        albums: [
            ("Warehouse Sunrise", ["Four to the Floor", "Sunrise Sequence", "Concrete Bloom", "Press Play"]),
            ("Heat Map", ["City Fever", "Bassline Weather", "Rooftop Signal", "Afterhours"]),
        ],
    },
    ArtistSpec {
        name: "Kitsune Radio",
        style: Style::Chill,
        hue: 352.0,
        russian: false,
        albums: [
            ("Lantern Beats", ["Fox Hour", "Umbrella Stroll", "Tokyo Rain Tape", "Sleepy Neon"]),
            ("Tea & Vinyl", ["Warm Static", "Cat on the Keys", "Slow Kettle", "Sunday Crates"]),
        ],
    },
    ArtistSpec {
        name: "Хрустальный сад",
        style: Style::Ambient,
        hue: 175.0,
        russian: true,
        albums: [("Утренний иней", ["Первый снег", "Стеклянный лес", "Тишина реки", "Лунный свет"]), ("Тихие воды", ["Озеро", "Дождь по крышам", "Дымка", "Рассвет в тумане"])],
    },
    ArtistSpec {
        name: "Полночный экспресс",
        style: Style::Synthwave,
        hue: 268.0,
        russian: true,
        albums: [
            ("Ночной вагон", ["Огни города", "Последний трамвай", "Неоновый дождь", "Станция Мечта"]),
            ("Скорость света", ["Фары", "Кассета 89", "Летняя ночь", "Магистраль"]),
        ],
    },
    ArtistSpec {
        name: "Tidal Bloom",
        style: Style::House,
        hue: 152.0,
        russian: false,
        albums: [("Salt & Sun", ["Coral Chords", "Seafoam", "High Tide Dance", "Sandstorm Lights"]), ("Undertow", ["Deep Current", "Blue Hour", "Kelp Forest", "Moonpool"])],
    },
    ArtistSpec {
        name: "Paper Satellites",
        style: Style::Chill,
        hue: 58.0,
        russian: false,
        albums: [
            ("Small Orbits", ["Pocket Planets", "Bike Lights", "Window Seat", "Tiny Constellations"]),
            ("Letters Home", ["Postcards", "Slow Train", "Handwritten", "Porchlight"]),
        ],
    },
];

fn style_index(s: Style) -> u8 {
    match s {
        Style::Chill => 0,
        Style::Synthwave => 1,
        Style::Ambient => 2,
        Style::House => 3,
    }
}

fn style_from_name(name: &str) -> Option<Style> {
    Style::ALL.into_iter().find(|s| s.name() == name)
}

/// The demo account's picture: a cover of its own, so the rail shows a picture as it would live.
pub fn avatar_template() -> String {
    cover_template(312.0, Style::Ambient, 4242)
}

fn cover_template(hue: f32, style: Style, seed: u64) -> String {
    format!("{HOST}/{}/{}/{seed}/%%", hue as i32, style_index(style))
}

/// `demo.local/<hue>/<style>/<seed>/<size>x<size>` → its parts.
pub fn parse_cover_url(url: &str) -> Option<(f32, u8, u64, u32)> {
    let rest = url.strip_prefix("https://")?.strip_prefix(HOST)?.strip_prefix('/')?;
    let mut it = rest.split('/');
    let hue = it.next()?.parse().ok()?;
    let style = it.next()?.parse().ok()?;
    let seed = it.next()?.parse().ok()?;
    let size = it.next()?.split('x').next()?.parse().ok()?;
    Some((hue, style, seed, size))
}

/// The demo track id carries everything the synthesiser needs.
fn track_id(style: Style, seed: u64, secs: u32) -> String {
    format!("demo-{}-{seed}-{secs}", style.name())
}

pub fn parse_track_id(id: &str) -> Option<(Style, u64, u32)> {
    let mut it = id.strip_prefix("demo-")?.split('-');
    let style = style_from_name(it.next()?)?;
    Some((style, it.next()?.parse().ok()?, it.next()?.parse().ok()?))
}

/// The WAV for a demo track (a minute and a half or two of synthesised music).
pub fn audio(id: &str) -> Option<Arc<[u8]>> {
    let (style, seed, secs) = parse_track_id(id)?;
    Some(ym_audio::synth::demo_wav(seed, style, secs as f32).into())
}

impl Catalog {
    pub fn new() -> Catalog {
        let mut artists = Vec::new();
        let mut albums = Vec::new();
        let mut tracks = Vec::new();
        for (ai, spec) in SPECS.iter().enumerate() {
            // An artist's picture is the cover of their first album.
            let artist = Artist { id: format!("da{ai}"), name: spec.name.into(), og_image: Some(cover_template(spec.hue, spec.style, ai as u64 * 2 + 1)), ..Artist::default() };
            for (bi, (album_title, titles)) in spec.albums.iter().enumerate() {
                let album_seed = (ai * 2 + bi) as u64 + 1;
                let mut album = Album {
                    id: format!("db{ai}{bi}"),
                    title: (*album_title).into(),
                    year: Some(2021 + ((ai + bi) % 5) as i32),
                    cover_uri: Some(cover_template(spec.hue + bi as f32 * 24.0, spec.style, album_seed)),
                    genre: Some(spec.style.name().into()),
                    track_count: Some(titles.len() as u32),
                    kind: Some("album".into()),
                    artists: vec![artist.clone()],
                    ..Album::default()
                };
                let mut volume = Vec::new();
                for (ti, title) in titles.iter().enumerate() {
                    let seed = album_seed * 10 + ti as u64 + 3;
                    let secs = 96 + (seed % 5) as u32 * 10;
                    let mut lite = album.clone();
                    lite.volumes.clear();
                    volume.push(Track {
                        id: track_id(spec.style, seed, secs),
                        title: (*title).into(),
                        available: Some(true),
                        duration_ms: Some(secs as u64 * 1000),
                        artists: vec![artist.clone()],
                        albums: vec![lite],
                        lyrics_info: Some(ym_api::models::LyricsInfo { has_available_sync_lyrics: spec.style != Style::Ambient || spec.russian, has_available_text_lyrics: true }),
                        ..Track::default()
                    });
                }
                tracks.extend(volume.iter().cloned());
                album.volumes = vec![volume];
                albums.push(album);
            }
            artists.push(artist);
        }

        let by = |name: &str| tracks.iter().find(|t| t.title == name).cloned();
        let pick = |names: &[&str]| -> Vec<Track> { names.iter().filter_map(|n| by(n)).collect() };
        let make = |kind: i64, title: &str, desc: &str, hue: f32, style: Style, items: Vec<Track>| -> Playlist {
            Playlist {
                uid: 1,
                kind,
                title: title.into(),
                description: Some(desc.into()),
                track_count: Some(items.len() as u32),
                duration_ms: Some(items.iter().map(|t| t.duration_ms.unwrap_or(0)).sum()),
                cover: Some(ImageRef { kind: Some("pic".into()), uri: Some(cover_template(hue, style, 90 + kind as u64)), items_uri: vec![] }),
                owner: Some(Owner { uid: 1, login: "glint".into(), name: Some("Glint".into()) }),
                tracks: items.into_iter().map(|t| PlaylistTrack { id: t.id.clone(), track: Some(t) }).collect(),
                ..Playlist::default()
            }
        };
        let playlists = vec![
            make(
                1,
                "Playlist of the Day",
                "Fresh picks, tuned to the hour",
                300.0,
                Style::Synthwave,
                pick(&["Afterglow Drive", "Tidepool", "Fox Hour", "Огни города", "Four to the Floor", "Slow Train", "Seafoam", "Первый снег"]),
            ),
            make(
                2,
                "Night Drive",
                "Neon, chrome and an empty highway",
                268.0,
                Style::Synthwave,
                pick(&["Midnight Overpass", "Laser Garden", "Фары", "Магистраль", "Glass Highway", "Last Arcade", "Неоновый дождь", "Last Arcade"]),
            ),
            make(
                3,
                "Focus Flow",
                "Soft, wordless, steady",
                200.0,
                Style::Ambient,
                pick(&["Soft Aurora", "Breathing Room", "Quiet Orbit", "Weightless", "Тишина реки", "Озеро", "Drift Theory", "Hollow Moon"]),
            ),
            make(
                4,
                "Rainy Window",
                "Lo-fi beats for grey afternoons",
                352.0,
                Style::Chill,
                pick(&["Tokyo Rain Tape", "Umbrella Stroll", "Warm Static", "Slow Kettle", "Window Seat", "Postcards", "Sleepy Neon", "Porchlight"]),
            ),
            make(
                5,
                "Dancefloor Heat",
                "Four-on-the-floor, all night",
                22.0,
                Style::House,
                pick(&["Sunrise Sequence", "Concrete Bloom", "City Fever", "Bassline Weather", "High Tide Dance", "Coral Chords", "Afterhours", "Rooftop Signal"]),
            ),
            make(
                6,
                "Mirror Lake",
                "Calm water, clear sky",
                175.0,
                Style::Ambient,
                pick(&["Стеклянный лес", "Лунный свет", "Дымка", "Рассвет в тумане", "Tidepool", "Lantern Field", "Deep Current", "Blue Hour"]),
            ),
        ];

        let liked = [0usize, 3, 6, 9, 12, 15, 17, 20, 24, 27, 31, 34, 38, 41, 45, 50, 55, 60].iter().filter_map(|i| tracks.get(*i)).map(|t| t.id.clone()).collect();
        Catalog { artists, albums, tracks, playlists, liked }
    }

    pub fn account(&self) -> Account {
        Account { uid: 1, login: "demo".into(), display_name: "Demo listener".into(), has_plus: true }
    }

    pub fn liked_tracks(&self) -> Vec<Track> {
        self.liked.iter().filter_map(|id| self.tracks.iter().find(|t| &t.id == id)).cloned().collect()
    }

    pub fn home(&self) -> Vec<Shelf> {
        let pl = |ids: &[usize]| ids.iter().filter_map(|i| self.playlists.get(*i)).cloned().map(ShelfItem::Playlist).collect::<Vec<_>>();
        vec![
            Shelf { kind: ShelfKind::Personal, items: pl(&[0, 1, 2, 3]) },
            Shelf {
                kind: ShelfKind::NewReleases,
                items: self
                    .albums
                    .iter()
                    .step_by(2)
                    .cloned()
                    .map(|mut a| {
                        a.volumes.clear();
                        ShelfItem::Album(a)
                    })
                    .collect(),
            },
            Shelf { kind: ShelfKind::Editorial, items: pl(&[4, 5, 2, 0]) },
            Shelf { kind: ShelfKind::Chart, items: self.tracks.iter().step_by(6).take(10).cloned().map(ShelfItem::Track).collect() },
        ]
    }

    pub fn album(&self, id: &str) -> Option<Album> {
        self.albums.iter().find(|a| a.id == id).cloned()
    }

    pub fn playlist(&self, kind: i64) -> Option<Playlist> {
        self.playlists.iter().find(|p| p.kind == kind).cloned()
    }

    /// Every track whose first artist is `id`, in catalogue order.
    pub fn artist_tracks(&self, id: &str) -> Vec<Track> {
        self.tracks.iter().filter(|t| t.artists.iter().any(|x| x.id == id)).cloned().collect()
    }

    pub fn artist(&self, id: &str) -> Option<ArtistPage> {
        let artist = self.artists.iter().find(|a| a.id == id)?.clone();
        let albums: Vec<Album> = self
            .albums
            .iter()
            .filter(|a| a.artists.first().is_some_and(|x| x.id == id))
            .cloned()
            .map(|mut a| {
                a.volumes.clear();
                a
            })
            .collect();
        let popular = self.tracks.iter().filter(|t| t.artists.first().is_some_and(|x| x.id == id)).take(8).cloned().collect();
        let similar = self.artists.iter().filter(|a| a.id != id).take(5).cloned().collect();
        Some(ArtistPage { artist, popular_tracks: popular, albums, similar_artists: similar })
    }

    pub fn search(&self, text: &str) -> SearchResult {
        let q = text.trim().to_lowercase();
        let hit = |s: &str| !q.is_empty() && s.to_lowercase().contains(&q);
        fn page<T>(v: Vec<T>) -> Page<T> {
            Page { total: v.len() as u32, per_page: 20, results: v }
        }
        SearchResult {
            text: text.into(),
            tracks: page(self.tracks.iter().filter(|t| hit(&t.title) || hit(&t.artists_line()) || t.album().is_some_and(|a| hit(&a.title))).take(30).cloned().collect()),
            albums: page(
                self.albums
                    .iter()
                    .filter(|a| hit(&a.title) || hit(&a.artists_line()))
                    .cloned()
                    .map(|mut a| {
                        a.volumes.clear();
                        a
                    })
                    .collect(),
            ),
            artists: page(self.artists.iter().filter(|a| hit(&a.name)).cloned().collect()),
            playlists: page(self.playlists.iter().filter(|p| hit(&p.title)).cloned().collect()),
        }
    }

    /// A batch for the "wave" stations: the preset leans toward certain styles.
    pub fn wave(&self, preset: &str, round: u64) -> Vec<Track> {
        let wanted: &[Style] = match preset {
            "energetic" | "fun" | "popular" => &[Style::House, Style::Synthwave],
            "calm" | "sad" | "workbackground" | "without-words" => &[Style::Ambient, Style::Chill],
            "russian" => &[],
            _ => &[Style::Chill, Style::Synthwave, Style::House, Style::Ambient],
        };
        let russian = |t: &Track| t.artists.first().is_some_and(|a| SPECS.iter().any(|s| s.russian && s.name == a.name));
        let mut pool: Vec<&Track> = self
            .tracks
            .iter()
            .filter(|t| match preset {
                "russian" => russian(t),
                "not-russian" => !russian(t),
                _ => parse_track_id(&t.id).is_some_and(|(s, _, _)| wanted.contains(&s)),
            })
            .collect();
        let mut rng = round.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ 0xdead_beef;
        let mut next = move || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng
        };
        for i in (1..pool.len()).rev() {
            pool.swap(i, (next() % (i as u64 + 1)) as usize);
        }
        pool.into_iter().take(8).cloned().collect()
    }

    pub fn lyrics(&self, track: &Track) -> Lyrics {
        let Some((style, seed, secs)) = parse_track_id(&track.id) else { return Lyrics::default() };
        let russian = track.artists.first().is_some_and(|a| SPECS.iter().any(|s| s.russian && s.name == a.name));
        if style == Style::Ambient && !russian {
            return Lyrics::instrumental();
        }
        word_timed(Lyrics::from_lrc(&lrc(seed, secs, russian), LyricsSource::Demo), russian)
    }
}

/// Gives the demo lyrics what lrc.red's carry: every word timed (longer words last longer), a
/// section label every few lines, and a background voice now and then, so the word-by-word view
/// can be seen without the network.
fn word_timed(mut l: Lyrics, russian: bool) -> Lyrics {
    let starts: Vec<u32> = l.lines.iter().filter_map(|line| line.time_ms).collect();
    for (i, line) in l.lines.iter_mut().enumerate() {
        let Some(start) = line.time_ms else { continue };
        let next = starts.get(i + 1).copied().unwrap_or(start + 5_000);
        let end = (start + 4_800).min(next.saturating_sub(500)).max(start + 900);
        let units: Vec<&str> = line.text.split_whitespace().collect();
        let weight: u32 = units.iter().map(|w| w.chars().count() as u32 + 2).sum::<u32>().max(1);
        let mut at = start;
        line.words = units
            .iter()
            .enumerate()
            .map(|(k, w)| {
                let len = (end - start) * (w.chars().count() as u32 + 2) / weight;
                let word = ym_api::LyricWord { start_ms: at, end_ms: at + len, text: (*w).to_owned(), space_after: k + 1 < units.len() };
                at += len;
                word
            })
            .collect();
        line.end_ms = Some(end);
        if i % 4 == 0 {
            line.part = Some(match (i / 4 % 2 == 0, russian) {
                (true, false) => "Verse".to_owned(),
                (false, false) => "Chorus".to_owned(),
                (true, true) => "Куплет".to_owned(),
                (false, true) => "Припев".to_owned(),
            });
        }
        if i % 5 == 2 {
            let (a, b) = if russian { ("(о-о,", "о-о)") } else { ("(oh,", "oh)") };
            let mid = start + (end - start) / 2;
            line.backing = vec![
                ym_api::LyricWord { start_ms: mid, end_ms: mid + (end - mid) / 2, text: a.to_owned(), space_after: true },
                ym_api::LyricWord { start_ms: mid + (end - mid) / 2, end_ms: end, text: b.to_owned(), space_after: false },
            ];
        }
    }
    l.word_synced = l.lines.iter().any(|line| line.words.len() > 1);
    l
}

impl Default for Catalog {
    fn default() -> Self {
        Catalog::new()
    }
}

const EN_LINES: [&str; 20] = [
    "City lights are bending in the rain",
    "I keep your name inside my pocket",
    "Hold the line while the night goes on",
    "We were electric before the dawn",
    "Paper moons and satellites",
    "Run with me through the quiet streets",
    "Every window hums a different song",
    "Turn it up, let the whole sky shake",
    "Static dreams on the radio",
    "Take my hand, the night is ours",
    "Slow motion, golden hour glow",
    "We never learned how to say goodbye",
    "Echoes in the afterglow",
    "Carry me home on a wave of light",
    "Somewhere past the neon sea",
    "Let the silence sing for us",
    "Heartbeat drums and velvet skies",
    "We're only passing through the light",
    "Dancing on the edge of morning",
    "Stay awake, the world is new",
];

const RU_LINES: [&str; 18] = [
    "Город дышит огнями в ночи",
    "Тишина разливается светом",
    "Я иду по следам твоих слов",
    "Звёзды падают прямо в ладони",
    "Ветер тихо играет на струнах дождя",
    "Мы с тобой — как два эха в пути",
    "Не оставляй меня на рассвете",
    "Отражения плывут по воде",
    "Время тает, как иней на стёклах",
    "Дай мне руку, пойдём за огнями",
    "Где-то там, за границей тумана",
    "Ночь поёт нам свою колыбельную",
    "Свет фонарей на мокром асфальте",
    "Всё, что было, осталось со мной",
    "Мы летим сквозь неоновый дым",
    "Тихо-тихо, не спугни этот миг",
    "Сердце бьётся в такт проводам",
    "Небо дышит у нас над головой",
];

/// Time-stamped lyrics spread over the track, in a deterministic order for each seed.
fn lrc(seed: u64, secs: u32, russian: bool) -> String {
    let pool: &[&str] = if russian { &RU_LINES } else { &EN_LINES };
    let mut order: Vec<usize> = (0..pool.len()).collect();
    let mut rng = seed.wrapping_mul(0x2545_f491_4f6c_dd1d) | 1;
    for i in (1..order.len()).rev() {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        order.swap(i, (rng % (i as u64 + 1)) as usize);
    }
    let intro = 9.0;
    let n = (((secs as f32 - intro - 8.0) / 6.2) as usize).clamp(6, pool.len());
    let step = (secs as f32 - intro - 8.0) / n as f32;
    let mut out = String::new();
    for (k, idx) in order.into_iter().take(n).enumerate() {
        let t = intro + k as f32 * step + ((rng >> (k % 16)) % 7) as f32 * 0.07;
        out.push_str(&format!("[{:02}:{:05.2}]{}\n", (t / 60.0) as u32, t % 60.0, pool[idx]));
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Covers

/// A square cover, RGBA8. Each style has its own motif; the hue sets the colour world.
pub fn cover_rgba(hue: f32, style: u8, seed: u64, size: u32) -> Vec<u8> {
    let n = size.max(8) as usize;
    let mut px = vec![255u8; n * n * 4];
    let mut rng = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    let mut rand = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        (rng >> 11) as f32 / (1u64 << 53) as f32
    };
    let top = oklch_to_srgb(0.20, 0.085, hue);
    let mid = oklch_to_srgb(0.42, 0.15, hue + 28.0);
    let bottom = oklch_to_srgb(0.66, 0.17, hue + 62.0);
    let glow = oklch_to_srgb(0.84, 0.14, hue + 150.0);
    let hot = oklch_to_srgb(0.78, 0.18, hue - 40.0);
    // A few soft blobs everywhere.
    let blobs: Vec<(f32, f32, f32, [f32; 3])> = (0..4).map(|i| (rand(), rand(), 0.16 + rand() * 0.26, if i % 2 == 0 { glow } else { hot })).collect();
    let (sun_r, bars): (f32, Vec<f32>) = (0.2 + rand() * 0.06, (0..14).map(|_| 0.25 + rand() * 0.6).collect());
    let phase = rand() * std::f32::consts::TAU;
    for y in 0..n {
        let v = y as f32 / (n - 1) as f32;
        for x in 0..n {
            let u = x as f32 / (n - 1) as f32;
            // Vertical gradient top → mid → bottom.
            let g = v * 0.8 + u * 0.2;
            let k = ((g - 0.0) * 2.0).clamp(0.0, 2.0);
            let mut c = if k < 1.0 { mix(top, mid, k) } else { mix(mid, bottom, k - 1.0) };
            for &(bx, by, br, bc) in &blobs {
                let d2 = ((u - bx).powi(2) + (v - by).powi(2)) / (br * br);
                let a = (-d2 * 1.8).exp() * 0.42;
                c = mix(c, bc, a);
            }
            match style {
                // Synthwave: striped sun over a perspective grid.
                1 => {
                    let (cx, cy) = (0.5, 0.42);
                    let d = ((u - cx).powi(2) + (v - cy).powi(2)).sqrt();
                    if d < sun_r + 0.12 && v < 0.62 {
                        let slit = ((v - cy) * 38.0).sin() > (0.9 - (v - cy + 0.1).max(0.0) * 4.0).min(0.85) && v > cy;
                        if d < sun_r + 0.12 && !slit {
                            let t = ((v - (cy - sun_r - 0.12)) / (2.0 * (sun_r + 0.12))).clamp(0.0, 1.0);
                            let s = mix(mix([1.0, 0.86, 0.35], hot, t), mix(hot, [0.62, 0.15, 0.62], t), t);
                            let a = ((sun_r + 0.12 - d) * n as f32 * 0.5).clamp(0.0, 1.0);
                            c = mix(c, s, a * 0.95);
                        }
                    }
                    if v > 0.62 {
                        let depth = (v - 0.62) / 0.38;
                        let persp = (u - 0.5) / (0.05 + depth);
                        let lines = ((persp * 9.0).fract() - 0.5).abs() < 0.04 + 0.03 * (1.0 - depth);
                        let horiz = ((1.0 / (depth + 0.07) * 0.9).fract()) < 0.07;
                        let base = mix(top, [0.05, 0.02, 0.1], 0.7);
                        c = base;
                        if lines || horiz {
                            c = mix(base, glow, 0.7 * (0.4 + depth));
                        }
                    }
                }
                // Ambient: soft rings.
                2 => {
                    let d = ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt();
                    let ring = ((d * 22.0 + phase).sin() * 0.5 + 0.5).powf(6.0) * (1.0 - d * 1.4).max(0.0);
                    c = mix(c, glow, ring * 0.55);
                    let core = (-d * d * 40.0).exp();
                    c = mix(c, [1.0, 1.0, 1.0], core * 0.5);
                }
                // House: equaliser bars rising from the bottom.
                3 => {
                    let i = (u * bars.len() as f32).floor() as usize;
                    let within = (u * bars.len() as f32).fract();
                    if i < bars.len() && within > 0.14 && within < 0.86 && v > 1.0 - bars[i] {
                        let t = (v - (1.0 - bars[i])) / bars[i];
                        let a = 0.35 + 0.65 * t;
                        c = mix(c, mix(glow, hot, t), a);
                    }
                }
                // Chill: layered waves and a moon.
                _ => {
                    for layer in 0..3 {
                        let fl = layer as f32;
                        let wave = 0.55 + fl * 0.13 + 0.05 * ((u * (5.0 + fl * 2.0) + phase + fl).sin());
                        if v > wave {
                            c = mix(c, mix(mid, top, 0.3 + fl * 0.3), 0.55);
                        }
                    }
                    let d = ((u - 0.68).powi(2) + (v - 0.3).powi(2)).sqrt();
                    let moon = ((0.13 - d) * n as f32 * 0.5).clamp(0.0, 1.0);
                    c = mix(c, [1.0, 0.95, 0.88], moon * 0.92);
                }
            }
            // Vignette and film grain.
            let vig = 1.0 - ((u - 0.5).powi(2) + (v - 0.5).powi(2)) * 0.9;
            let grain = (rand() - 0.5) * 0.045;
            let o = (y * n + x) * 4;
            for ch in 0..3 {
                px[o + ch] = (((c[ch] * vig) + grain).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            }
        }
    }
    px
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn the_catalogue_is_consistent() {
        let c = Catalog::new();
        assert!(!c.artists.is_empty() && !c.albums.is_empty() && !c.tracks.is_empty());
        let ids: HashSet<&str> = c.tracks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids.len(), c.tracks.len(), "track ids must be unique");
        for t in &c.tracks {
            let (_, _, secs) = parse_track_id(&t.id).expect("demo ids parse");
            assert_eq!(t.duration_ms, Some(secs as u64 * 1000));
            assert!(t.cover_url(300).is_some_and(|u| parse_cover_url(&u).is_some()), "cover url {:?}", t.cover_url(300));
        }
        assert!(c.playlists.iter().all(|p| p.resolved_tracks().count() >= 6));
        assert!(!c.liked_tracks().is_empty());
        for s in c.home() {
            assert!(!s.items.is_empty());
        }
    }

    #[test]
    fn cover_urls_round_trip() {
        let t = Catalog::new().tracks[5].clone();
        let url = t.cover_url(400).unwrap();
        let (hue, style, seed, size) = parse_cover_url(&url).unwrap();
        assert!(hue >= 0.0 && style < 4 && seed > 0 && size == 400);
        assert!(parse_cover_url("https://avatars.yandex.net/x/400x400").is_none());
    }

    #[test]
    fn search_finds_titles_artists_and_albums() {
        let c = Catalog::new();
        assert!(c.search("afterglow").tracks.results.iter().any(|t| t.title == "Afterglow Drive"));
        assert_eq!(c.search("nova kes").artists.results.len(), 1);
        assert!(c.search("хрусталь").artists.results.len() == 1, "cyrillic matches");
        assert!(c.search("slow light").albums.results.iter().any(|a| a.title == "Slow Light"));
        assert!(c.search("   ").tracks.results.is_empty());
    }

    #[test]
    fn lyrics_are_synced_and_ordered() {
        let c = Catalog::new();
        let t = c.tracks.iter().find(|t| t.title == "Afterglow Drive").unwrap();
        let l = c.lyrics(t);
        assert!(l.synced && l.lines.len() >= 6, "{} lines", l.lines.len());
        let times: Vec<u32> = l.lines.iter().filter_map(|x| x.time_ms).collect();
        assert!(times.windows(2).all(|w| w[0] < w[1]), "{times:?}");
        assert!(*times.last().unwrap() < t.duration_ms.unwrap() as u32);
        assert!(c.lyrics(c.tracks.iter().find(|t| t.title == "Tidepool").unwrap()).instrumental);
        let ru = c.lyrics(c.tracks.iter().find(|t| t.title == "Огни города").unwrap());
        assert!(ru.lines[0].text.chars().any(|ch| ('а'..='я').contains(&ch)));
    }

    #[test]
    fn demo_lyrics_are_timed_per_word_inside_their_line() {
        let c = Catalog::new();
        for title in ["Afterglow Drive", "Огни города"] {
            let l = c.lyrics(c.tracks.iter().find(|t| t.title == title).unwrap());
            assert!(l.word_synced, "{title}");
            for (i, line) in l.lines.iter().enumerate() {
                let start = line.time_ms.unwrap();
                assert!(!line.words.is_empty() && line.end_ms.unwrap() > start);
                assert!(line.words.windows(2).all(|w| w[0].end_ms <= w[1].start_ms), "{title} line {i}: words overlap");
                assert!(line.words.first().unwrap().start_ms == start && line.words.last().unwrap().end_ms <= line.end_ms.unwrap());
                assert_eq!(ym_api::join_words(&line.words), line.text, "{title} line {i}");
                if let Some(next) = l.lines.get(i + 1) {
                    assert!(line.end_ms.unwrap() < next.time_ms.unwrap(), "{title} line {i} runs into the next");
                }
            }
            assert!(l.lines.iter().any(|x| x.part.is_some()) && l.lines.iter().any(|x| !x.backing.is_empty()));
        }
    }

    #[test]
    fn wave_batches_follow_the_preset() {
        let c = Catalog::new();
        let calm = c.wave("calm", 1);
        assert_eq!(calm.len(), 8);
        assert!(calm.iter().all(|t| matches!(parse_track_id(&t.id), Some((Style::Ambient | Style::Chill, _, _)))));
        assert_ne!(c.wave("onyourwave", 1).iter().map(|t| &t.id).collect::<Vec<_>>(), c.wave("onyourwave", 2).iter().map(|t| &t.id).collect::<Vec<_>>());
        assert!(c.wave("russian", 3).iter().all(|t| t.primary_artist() == "Хрустальный сад" || t.primary_artist() == "Полночный экспресс"));
    }

    #[test]
    fn covers_are_deterministic_and_not_flat() {
        for style in 0..4 {
            let a = cover_rgba(210.0, style, 7, 96);
            assert_eq!(a, cover_rgba(210.0, style, 7, 96));
            assert_ne!(a, cover_rgba(210.0, style, 8, 96), "seed changes the art");
            let mean = a.chunks(4).map(|p| p[0] as u32 + p[1] as u32 + p[2] as u32).sum::<u32>() as f32 / (a.len() / 4) as f32 / 3.0;
            let var = a.chunks(4).map(|p| ((p[0] as f32 + p[1] as f32 + p[2] as f32) / 3.0 - mean).powi(2)).sum::<f32>() / (a.len() / 4) as f32;
            assert!(var > 200.0, "style {style} looks flat (variance {var})");
        }
    }
}
