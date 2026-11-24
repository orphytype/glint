//! Named states for headless snapshots (`--scene NAME`). They fill in the fixtures' made-up data
//! directly, so a picture never depends on a background job finishing.

use std::sync::Arc;

use rune::TextEdit;
use ym_api::Track;

use crate::app::{LibTab, Load, LyricsUi, NpTab, Page, SearchTab, Session, Source, State};
use crate::fixtures::Catalog;
use crate::i18n::{Lang, S};
use crate::net::Backend;
use crate::store::Effects;

type Scene = fn(&mut State);

pub const SCENES: &[(&str, Scene)] = &[
    ("login", login),
    ("login-ru", login_ru),
    ("launch", launch),
    ("home", home),
    ("home-ru", home_ru),
    ("home-cloud", home_cloud),
    ("home-pins", home_pins),
    ("search", search),
    ("search-empty", search_empty),
    ("library", library),
    ("library-albums", library_albums),
    ("album", album),
    ("album-loading", album_loading),
    ("playlist", playlist),
    ("playlist-own", playlist_own),
    ("queue-from", queue_from),
    ("np-from-long", np_from_long),
    ("artist", artist),
    ("nowplaying", nowplaying),
    ("nowplaying-wave", nowplaying_wave),
    ("sheet-open", sheet_open),
    ("sheet-open-np", sheet_open_np),
    ("nowplaying-closed", nowplaying_closed),
    ("nowplaying-ru", nowplaying_ru),
    ("nowplaying-paused", nowplaying_paused),
    ("menu", menu),
    ("volume", volume),
    ("wave-playing", wave_playing),
    ("lrcred", lrcred),
    ("lyrics-blank", lyrics_blank),
    ("queue", queue),
    ("track-menu", track_menu),
    ("track-menu-ru", track_menu_ru),
    ("queue-menu", queue_menu),
    ("settings", settings),
    ("settings-lyrics", settings_lyrics),
    ("settings-ru", settings_ru),
    ("settings-ru-sound", |s| settings_ru_on(s, crate::ui::settings::SetTab::Sound)),
    ("settings-ru-player", |s| settings_ru_on(s, crate::ui::settings::SetTab::Player)),
    ("settings-ru-mini", |s| settings_ru_on(s, crate::ui::settings::SetTab::Mini)),
    ("settings-ru-startup", |s| settings_ru_on(s, crate::ui::settings::SetTab::Startup)),
    ("settings-ru-storage", |s| settings_ru_on(s, crate::ui::settings::SetTab::Storage)),
    ("settings-ru-about", |s| settings_ru_on(s, crate::ui::settings::SetTab::About)),
    ("settings-update", settings_update),
    ("settings-find", settings_find),
    ("library-find", library_find),
    ("settings-playback", settings_playback),
    ("nowplaying-backdrop", nowplaying_backdrop),
    ("nowplaying-backdrop-dither", nowplaying_backdrop_dither),
    ("nowplaying-backdrop-halftone", nowplaying_backdrop_halftone),
    ("nowplaying-cover", nowplaying_cover),
    ("nowplaying-compact", nowplaying_compact),
    ("settings-desktop", settings_desktop),
    ("settings-about", settings_about),
    ("settings-keys", settings_keys),
    ("settings-keys-rebind", settings_keys_rebind),
    ("settings-storage", settings_storage),
    ("light", light),
    ("light-np", light_np),
    ("effects-light", effects_light),
    ("effects-off", effects_off),
    ("amber", amber),
    ("nowplaying-feat", nowplaying_feat),
    ("lyrics-none", lyrics_none),
    ("lyrics-count", lyrics_count),
    ("lyrics-count-run", lyrics_count_run),
    ("lyrics-count-scatter", lyrics_count_scatter),
    ("wave-tune-ru", wave_tune_ru),
    ("update-popover", update_popover),
    ("dock-menu", dock_menu),
    ("download-quality", download_quality),
    ("actions", actions),
    ("np-waves", np_waves),
    ("np-look", np_look),
    ("player-menu", player_menu),
    ("home-next", home_next),
    ("sheet", sheet),
    ("sheet-page", sheet_page),
    ("sheet-artist", sheet_artist),
    ("artist-all", artist_all),
];

/// A signed-in session on the fixtures, with everything the pages show already loaded.
fn signed_in(s: &mut State) -> Arc<Catalog> {
    s.lang = Lang::En;
    let c = Arc::new(Catalog::new());
    s.backend = Backend::Fixture(c.clone());
    s.session = Session::Live;
    s.account = Some(c.account());
    s.avatar = Some(ym_api::cover::url(&crate::fixtures::avatar_template(), 200));
    // A measuring tool: `GLINT_SEED=n` draws another garden.
    s.home_seed = std::env::var("GLINT_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(7);
    s.home = Load::Ready(c.home());
    let liked = c.liked_tracks();
    s.liked = liked.iter().map(|t| t.id.clone()).collect();
    s.library.tracks = Load::Ready(liked);
    s.library.playlists = Load::Ready(
        c.playlists
            .iter()
            .cloned()
            .map(|mut p| {
                p.tracks.clear();
                p
            })
            .collect(),
    );
    s.library.albums = Load::Ready(
        c.albums
            .iter()
            .step_by(3)
            .cloned()
            .map(|mut a| {
                a.volumes.clear();
                a
            })
            .collect(),
    );
    s.library.artists = Load::Ready(c.artists.clone());
    c
}

/// Starts `queue[at]` as if it had been playing for `pos_ms`, with its lyrics already found.
fn play(s: &mut State, c: &Catalog, at: usize, pos_ms: u64) {
    let tracks: Vec<Track> = c.tracks.iter().take(14).cloned().collect();
    let track = tracks[at].clone();
    s.play_queue(tracks, at, Source::Manual);
    s.player.loading = false;
    s.player.pos_ms = pos_ms;
    s.lyrics = LyricsUi { for_track: Some(track.id.clone()), lyrics: Load::Ready(c.lyrics(&track)), active: None };
}

fn ru(s: &mut State) {
    s.lang = Lang::Ru;
}

fn login(s: &mut State) {
    s.lang = Lang::En;
}

fn login_ru(s: &mut State) {
    ru(s);
}

/// The launch screen over Home (`--at` picks the moment: the move ends at about 0.55 s).
fn launch(s: &mut State) {
    home(s);
    s.launch = Some(Default::default());
}

fn home(s: &mut State) {
    let c = signed_in(s);
    play(s, &c, 2, 41_000);
}

/// Home with more pins than the rail has room for: they scroll above the foot buttons.
fn home_pins(s: &mut State) {
    let c = signed_in(s);
    play(s, &c, 2, 41_000);
    s.pinned_items = (0..16)
        .map(|i| {
            let t = &c.tracks[i % c.tracks.len()];
            crate::app::PinnedItem { page: Page::Album(format!("pin-{i}")), title: t.full_title(), cover: t.cover_template().map(str::to_owned), artist: i % 3 == 0 }
        })
        .collect();
}

/// Home, on the last track of an album, so "next" brings another cover.
fn home_next(s: &mut State) {
    let c = signed_in(s);
    let album = |t: &Track| t.albums.first().map(|a| a.id.clone());
    let at = (0..13).find(|&i| album(&c.tracks[i]) != album(&c.tracks[i + 1])).unwrap_or(2);
    play(s, &c, at, 41_000);
}

fn home_ru(s: &mut State) {
    home(s);
    s.lang = Lang::Ru;
}

/// Home in Russian with My Wave drawn as a cloud.
fn home_cloud(s: &mut State) {
    home_ru(s);
    s.settings.wave_look.form = 3;
}

fn search(s: &mut State) {
    let c = signed_in(s);
    s.page = Page::Search;
    s.search.edit = TextEdit::new("neon");
    s.search.seen = "neon".into();
    s.search.query = "neon".into();
    s.search.results = Load::Ready(c.search("neon"));
    play(s, &c, 5, 12_000);
}

fn search_empty(s: &mut State) {
    let _c = signed_in(s);
    s.page = Page::Search;
    s.settings.search_history = ["neon", "Kitsune Radio", "slow light", "ambient"].map(String::from).to_vec();
}

fn library(s: &mut State) {
    let c = signed_in(s);
    s.page = Page::Library;
    play(s, &c, 1, 95_000);
}

fn library_albums(s: &mut State) {
    let c = signed_in(s);
    s.page = Page::Library;
    s.library.tab = LibTab::Albums;
    play(s, &c, 1, 95_000);
}

/// An album page whose tracks are still on their way: the list shows its skeleton.
fn album_loading(s: &mut State) {
    let c = signed_in(s);
    let id = c.albums[2].id.clone();
    s.history.push((Page::Home, false));
    s.page = Page::Album(id.clone());
    s.begin_detail(Page::Album(id));
}

fn album(s: &mut State) {
    let c = signed_in(s);
    let id = c.albums[2].id.clone();
    s.history.push((Page::Home, false));
    s.page = Page::Album(id.clone());
    s.begin_detail(Page::Album(id.clone()));
    if let Some(a) = c.album(&id) {
        s.show_album(a);
    }
    play(s, &c, 9, 30_000);
}

fn playlist(s: &mut State) {
    let c = signed_in(s);
    let kind = c.playlists[0].kind;
    let uid = c.playlists[0].owner_uid();
    s.history.push((Page::Library, false));
    s.page = Page::Playlist(uid, kind);
    s.begin_detail(Page::Playlist(uid, kind));
    if let Some(p) = c.playlist(kind) {
        s.show_playlist(p);
    }
}

/// A playlist whose tracks are the queue playing now: its play button pauses.
fn playlist_own(s: &mut State) {
    playlist(s);
    let list = s.detail.tracks.clone();
    s.play_all(list, false);
    s.player.loading = false;
    s.player.pos_ms = 30_000;
}

/// The queue opened over a playlist's tracks: the player names the playlist above the cover.
fn queue_from(s: &mut State) {
    playlist_own(s);
    s.np_open = true;
    s.np.t = 1.0;
    s.np_tab = NpTab::Queue;
}

/// The player in a narrow window, playing from a source with a long name: the pill above the
/// cover shortens its text and stays centered.
fn np_from_long(s: &mut State) {
    nowplaying(s);
    s.lang = Lang::Ru;
    let name = crate::app::FromName::Text("Очень длинное название релиза, которое не помещается над обложкой".into());
    s.playing_from = Some(crate::app::PlayingFrom { wave: true, kind: S::WaveByRelease, name, page: None });
}

fn artist(s: &mut State) {
    let c = signed_in(s);
    let id = c.artists[0].id.clone();
    s.history.push((Page::Home, false));
    s.page = Page::Artist(id.clone());
    s.begin_detail(Page::Artist(id.clone()));
    if let Some(a) = c.artist(&id) {
        s.show_artist(a);
    }
    play(s, &c, 3, 120_000);
}

fn nowplaying(s: &mut State) {
    let c = signed_in(s);
    play(s, &c, 4, 52_000);
    s.np_open = true;
    // The player as it rests, not on its way in.
    s.np.t = 1.0;
    s.np_tab = NpTab::Lyrics;
}

/// The player paused with the track's measured waveform in the seek line: a quiet intro, two
/// loud choruses with a break between them, and a fade at the end.
fn nowplaying_wave(s: &mut State) {
    nowplaying(s);
    s.player.playing = false;
    let n = crate::fx::WAVE_N;
    let wave: Vec<f32> = (0..n)
        .map(|i| {
            let x = i as f32 / n as f32;
            let body = match x {
                x if x < 0.12 => 0.35 + 2.0 * x,
                x if x < 0.38 => 0.95,
                x if x < 0.48 => 0.55,
                x if x < 0.86 => 0.97,
                x => 0.97 * (1.0 - (x - 0.86) / 0.14),
            };
            (body - 0.08 * ((i * 7919) % 13) as f32 / 13.0).clamp(0.0, 1.0)
        })
        .collect();
    if let Some(id) = s.current().map(|t| t.id.clone()) {
        s.waveforms.insert(id, wave.into());
    }
}

/// The player over a picture of the user's own (a made-up dusk sky), with `effect` over it.
fn backdrop_scene(s: &mut State, effect: u8) {
    nowplaying(s);
    let img = image::RgbaImage::from_fn(640, 400, |x, y| {
        let (u, v) = (x as f32 / 640.0, y as f32 / 400.0);
        let sun = (-((u - 0.68).powi(2) + (v - 0.55).powi(2)) / 0.01).exp();
        let hill = if v > 0.72 + 0.06 * (u * 9.0).sin() { 0.25 } else { 1.0 };
        let r = ((0.20 + 0.75 * v + sun) * hill).min(1.0);
        let g = ((0.18 + 0.35 * v + 0.8 * sun) * hill).min(1.0);
        let b = ((0.55 - 0.3 * v + 0.3 * sun) * hill).min(1.0);
        image::Rgba([(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8, 255])
    });
    let mut png = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png, image::ImageFormat::Png).expect("a picture in memory");
    s.settings.backdrop = crate::store::BackdropLook { on: true, rev: 1, blur: 0.25, dim: 0.3, effect };
    s.backdrop_rev = Some(1);
    s.backdrop = crate::backdrop::build(&png.into_inner()).map(Arc::new);
}

fn nowplaying_backdrop(s: &mut State) {
    backdrop_scene(s, 0);
}

fn nowplaying_backdrop_dither(s: &mut State) {
    backdrop_scene(s, 1);
}

fn nowplaying_backdrop_halftone(s: &mut State) {
    backdrop_scene(s, 4);
}

fn nowplaying_cover(s: &mut State) {
    nowplaying(s);
    s.settings.player_layout = crate::store::PlayerLayout::Cover;
}

fn nowplaying_compact(s: &mut State) {
    nowplaying(s);
    s.settings.player_layout = crate::store::PlayerLayout::Compact;
}

/// The player alone, in the middle of the window.
fn nowplaying_closed(s: &mut State) {
    nowplaying(s);
    s.np_tab = NpTab::Closed;
}

/// The player alone, paused: the seek bars hold their pose.
fn nowplaying_paused(s: &mut State) {
    nowplaying_closed(s);
    s.player.playing = false;
}

/// Home with the profile menu open.
fn menu(s: &mut State) {
    home(s);
    s.menu_open = true;
}

/// Home with the volume card open.
fn volume(s: &mut State) {
    wave_playing(s);
    s.vol_open = true;
}

/// Home while a wave plays.
fn wave_playing(s: &mut State) {
    let c = signed_in(s);
    s.wave_choice = "calm".into();
    let tracks: Vec<Track> = c.tracks.iter().take(14).cloned().collect();
    let track = tracks[6].clone();
    s.play_queue(tracks, 6, Source::Wave { preset: "calm".into(), batch: None, round: 1 });
    s.player.loading = false;
    s.player.pos_ms = 33_000;
    s.lyrics = LyricsUi { for_track: Some(track.id.clone()), lyrics: Load::Ready(c.lyrics(&track)), active: None };
}

fn nowplaying_ru(s: &mut State) {
    let c = signed_in(s);
    // The Russian-language catalogue entries come last.
    let at = c.tracks.iter().position(|t| t.title.chars().any(|ch| ('а'..='я').contains(&ch.to_ascii_lowercase()) || ch == 'ё')).unwrap_or(4);
    let tracks = c.tracks.clone();
    let track = tracks[at].clone();
    s.play_queue(tracks, at, Source::Manual);
    s.player.loading = false;
    s.player.pos_ms = 48_000;
    s.lyrics = LyricsUi { for_track: Some(track.id.clone()), lyrics: Load::Ready(c.lyrics(&track)), active: None };
    s.lang = Lang::Ru;
    s.np_open = true;
    // The player as it rests, not on its way in.
    s.np.t = 1.0;
    s.np_tab = NpTab::Lyrics;
}

/// The menu of a track in a list, opened by a right click.
fn track_menu(s: &mut State) {
    let c = signed_in(s);
    s.page = Page::Library;
    play(s, &c, 1, 95_000);
    let t = c.tracks[3].clone();
    s.open_track_menu(t, None, rune::PopupAnchor::Point(520.0, 250.0));
}

fn track_menu_ru(s: &mut State) {
    track_menu(s);
    s.lang = Lang::Ru;
}

/// The menu of a track in the queue, opened from its "..." button (a track still to come, so it
/// can also be removed).
fn queue_menu(s: &mut State) {
    let c = signed_in(s);
    play(s, &c, 1, 95_000);
    s.np_open = true;
    // The player as it rests, not on its way in.
    s.np.t = 1.0;
    s.np_tab = NpTab::Queue;
    let t = c.tracks[4].clone();
    s.open_track_menu(t, Some(4), rune::PopupAnchor::Key("np-more".into()));
}

fn queue(s: &mut State) {
    nowplaying(s);
    s.np_tab = NpTab::Queue;
}

fn settings(s: &mut State) {
    let c = signed_in(s);
    s.settings_open = true;
    s.player.eq.enabled = true;
    s.player.eq.gains_db = ym_audio::PRESETS[4].1;
    play(s, &c, 6, 20_000);
}

/// Word lyrics as lrc.red would deliver them, with the credit it asks for.
fn lrcred(s: &mut State) {
    nowplaying(s);
    // The background job that fetches the fixtures' lyrics would put its own over these when it lands;
    // a different track id makes it drop its answer.
    s.lyrics.for_track = Some("scene".to_owned());
    if let Load::Ready(l) = &mut s.lyrics.lyrics {
        l.source = ym_api::LyricsSource::LrcRed;
        l.link = Some("https://lrc.red/s/GBAYE0601498".to_owned());
    }
}

/// A source that sends "..." for a track it has no words for: that is no lyrics, and says so.
fn lyrics_blank(s: &mut State) {
    nowplaying(s);
    s.lyrics.for_track = Some("scene".to_owned());
    s.lyrics.lyrics = Load::Ready(ym_api::Lyrics::from_lrc("[00:01.00]...\n[00:05.00]\u{2026}\n[00:09.00]\u{266a}", ym_api::LyricsSource::Lrclib));
}

/// The player with a track by four artists: two names with their pictures and "2 more".
fn nowplaying_feat(s: &mut State) {
    nowplaying(s);
    s.np_tab = NpTab::Closed;
    let c = Catalog::new();
    let all: Vec<ym_api::Artist> = c.artists.iter().take(4).cloned().collect();
    if let Some(i) = s.player.index
        && let Some(t) = s.player.queue.get_mut(i)
    {
        t.artists = all;
    }
}

/// A track there are no lyrics for: the lyrics button is off and the player stays in the middle.
fn lyrics_none(s: &mut State) {
    nowplaying(s);
    s.lyrics.for_track = Some("scene".to_owned());
    s.lyrics.lyrics = Load::Ready(ym_api::Lyrics::default());
}

/// Timed lyrics whose first line is still a couple of seconds away: the note counting it in,
/// halfway from 3 to 2.
fn lyrics_count(s: &mut State) {
    nowplaying(s);
    s.lyrics.for_track = Some("scene".to_owned());
    s.lyrics.lyrics = Load::Ready(ym_api::Lyrics::from_lrc(
        "[00:12.00]Neon on the water, the city hums\n[00:16.00]Every window is a little sun\n[00:20.00]We drive until the morning comes\n[00:24.00]And the static sings to everyone",
        ym_api::LyricsSource::Lrclib,
    ));
    s.player.pos_ms = 10_200;
    s.player.playing = false;
}

/// The count-in running into the first line (for frame strips: the beads scatter onto letters).
fn lyrics_count_run(s: &mut State) {
    lyrics_count(s);
    s.player.pos_ms = 9_600;
    s.player.playing = true;
}

/// Grains mid-settle directly on the letter contours of the first lyric line.
fn lyrics_count_scatter(s: &mut State) {
    lyrics_count(s);
    s.lyrics.active = Some(0);
    s.player.pos_ms = 12_620;
    s.player.playing = false;
}

/// Home in Russian with the redesigned My Wave settings popover open.
fn wave_tune_ru(s: &mut State) {
    home_ru(s);
    s.open_popover(crate::app::Popover::WaveTune { anchor: "hm-wave-tune".into() });
    s.pop.t = 1.0;
}

/// Home in Russian with the circular update icon in the title bar and its dropdown open.
fn update_popover(s: &mut State) {
    home_ru(s);
    s.update_method = glint_update::Method::WindowsInstaller;
    s.update = crate::update::Ui::Available(glint_update::Release {
        version: "0.9.5".into(),
        page_url: "https://github.com/orphytype/glint/releases/tag/v0.9.5".into(),
        notes: "Audio quality gets a pill in the mini player and a picker for batch downloads.\n• The quality menu shows each tier with its bitrate.\n• Covers load at the size each view needs, with the large Now Playing cover showing the cached one first.".into(),
        asset: Some(glint_update::Asset { name: "Glint-0.9.5-windows-x64-setup.exe".into(), url: String::new(), size: 12_000_000, sha256: String::new() }),
    });
    s.open_popover(crate::app::Popover::Update { anchor: "update-pill".into() });
    s.pop.t = 1.0;
}

/// Home with the mini-player dock "..." menu open (audio quality removed since it sits beside it).
fn dock_menu(s: &mut State) {
    home_ru(s);
    s.open_player_menu(rune::PopupAnchor::Key("bar-more".into()));
    s.tmenu.t = 1.0;
}

/// The batch download dialog with its quality picker.
fn download_quality(s: &mut State) {
    let c = signed_in(s);
    s.page = Page::Library;
    s.download_confirm = Some(c.liked_tracks());
}

/// What can be done with an album (its "..." menu).
fn actions(s: &mut State) {
    album(s);
    s.open_popover(crate::app::Popover::Actions { anchor: "d-more".into() });
    s.pop.t = 1.0;
}

/// The open player's wave card with its list of waves open (by track, artist, release).
fn np_waves(s: &mut State) {
    nowplaying(s);
    let Some(t) = s.current().cloned() else { return };
    let lang = s.lang;
    let mut choices = vec![crate::app::WaveChoice { station: crate::app::Station::Track(t.id.clone()), kind: S::ByTrack.get(lang).to_owned(), name: t.full_title() }];
    for a in &t.artists {
        choices.push(crate::app::WaveChoice { station: crate::app::Station::Artist(a.id.clone()), kind: S::ByArtist.get(lang).to_owned(), name: a.name.clone() });
    }
    if let Some(al) = t.albums.first() {
        choices.push(crate::app::WaveChoice { station: crate::app::Station::Album(al.id.clone()), kind: S::ByRelease.get(lang).to_owned(), name: al.title.clone() });
    }
    s.open_popover(crate::app::Popover::Waves { choices, anchor: "np-wave-more".into() });
    s.pop.t = 1.0;
}

/// The look of the open player, its popover open from the corner button, a few things changed.
fn np_look(s: &mut State) {
    nowplaying(s);
    s.settings.stage.effects = 2;
    s.open_popover(crate::app::Popover::Stage { anchor: "np-more".into() });
    s.pop.t = 1.0;
}

/// The playing track's menu, opened from the open player.
fn player_menu(s: &mut State) {
    nowplaying(s);
    s.open_player_menu(rune::PopupAnchor::Key("np-more".into()));
}

/// A track's details on the side sheet, over an album page.
fn sheet(s: &mut State) {
    album(s);
    let t = s.detail.tracks[2].clone();
    s.open_sheet(crate::app::Sheet::Track(Box::new(t)));
    s.sheet_pop.t = 1.0;
}

/// A track's details opening from its row (for frame strips of the move).
fn sheet_open(s: &mut State) {
    album(s);
    let t = s.detail.tracks[2].clone();
    s.open_sheet(crate::app::Sheet::Track(Box::new(t)));
}

/// The playing track's details opening from the open player's cover (for frame strips).
fn sheet_open_np(s: &mut State) {
    nowplaying(s);
    if let Some(t) = s.current().cloned() {
        s.open_sheet(crate::app::Sheet::Track(Box::new(t)));
    }
}

/// The open album's details on the side sheet.
fn sheet_page(s: &mut State) {
    album(s);
    s.open_sheet(crate::app::Sheet::Page);
    s.sheet_pop.t = 1.0;
}

/// An artist's details on the drawer: their releases over the years.
fn sheet_artist(s: &mut State) {
    artist(s);
    s.open_sheet(crate::app::Sheet::Page);
    s.sheet_pop.t = 1.0;
}

/// An artist's page listing every one of their tracks.
fn artist_all(s: &mut State) {
    artist(s);
    let c = Catalog::new();
    let id = c.artists[0].id.clone();
    let all = c.artist_tracks(&id);
    s.detail.tracks = all.clone();
    s.detail.all_tracks = Load::Ready(all);
    s.detail.showing_all = true;
}

/// The lyrics group of Settings with word lyrics switched on.
fn settings_lyrics(s: &mut State) {
    settings(s);
    s.settings_tab = crate::ui::settings::SetTab::Player;
    s.settings.word_lyrics = true;
}

/// Settings with a newer version found, for the Updates group (scroll to the end of the page).
fn settings_playback(s: &mut State) {
    settings(s);
    s.settings_tab = crate::ui::settings::SetTab::Sound;
}

fn settings_desktop(s: &mut State) {
    settings(s);
    s.settings_tab = crate::ui::settings::SetTab::Mini;
}

fn settings_about(s: &mut State) {
    settings(s);
    s.settings_tab = crate::ui::settings::SetTab::About;
}

/// The shortcuts, with Like moved to another key.
fn settings_keys(s: &mut State) {
    settings(s);
    s.settings_tab = crate::ui::settings::SetTab::Keys;
    s.settings.keys.insert("like".into(), "j".into());
}

/// ...and Mute waiting for its new key.
fn settings_keys_rebind(s: &mut State) {
    settings_keys(s);
    s.rebind = Some(crate::keys::Act::Mute);
}

/// What is saved on the disk, with the one button that deletes it.
fn settings_storage(s: &mut State) {
    settings(s);
    s.settings_tab = crate::ui::settings::SetTab::Storage;
    s.disk_usage.set((14_300_000, 212_000_000, 48_000, s.now));
}

fn settings_update(s: &mut State) {
    settings(s);
    s.update_method = glint_update::Method::WindowsInstaller;
    s.update = crate::update::Ui::Available(glint_update::Release {
        version: "0.9.5".into(),
        page_url: "https://github.com/orphytype/glint/releases/tag/v0.9.5".into(),
        notes: "Audio quality gets a pill in the mini player and a picker for batch downloads.\n• The quality menu shows each tier with its bitrate.".into(),
        asset: Some(glint_update::Asset { name: "Glint-0.9.5-windows-x64-setup.exe".into(), url: String::new(), size: 12_000_000, sha256: String::new() }),
    });
}

fn settings_ru(s: &mut State) {
    settings(s);
    s.lang = Lang::Ru;
}

/// The Settings search, typed in Latin letters with a typo, and the row a result opened.
fn settings_find(s: &mut State) {
    settings_ru_on(s, crate::ui::settings::SetTab::Startup);
    s.settings_find = TextEdit::new("trei");
    s.settings_found = Some((crate::ui::settings::item_key(S::CloseToTray.get(Lang::Ru)), s.now + 600.0));
}

/// The liked tracks filtered by a query in Cyrillic with a letter wrong.
fn library_find(s: &mut State) {
    library(s);
    s.library.filter = TextEdit::new("пульс машин");
}

/// One page of Settings in Russian.
fn settings_ru_on(s: &mut State, tab: crate::ui::settings::SetTab) {
    settings_ru(s);
    s.settings_tab = tab;
    s.disk_usage.set((14_300_000, 212_000_000, 48_000, s.now));
}

fn light(s: &mut State) {
    home(s);
    s.settings.theme = crate::store::ThemeMode::Light;
}

fn light_np(s: &mut State) {
    nowplaying(s);
    s.settings.theme = crate::store::ThemeMode::Light;
}

fn effects_light(s: &mut State) {
    nowplaying(s);
    s.settings.effects = Effects::Light;
}

fn effects_off(s: &mut State) {
    home(s);
    s.settings.effects = Effects::Off;
}

fn amber(s: &mut State) {
    home(s);
    s.settings.accent = 4;
    let _ = SearchTab::All;
}
