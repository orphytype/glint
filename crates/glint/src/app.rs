//! The application state and everything that changes it. The view only reads this; handlers
//! mutate it on the UI thread; background jobs hand closures to [`State::job`], which applies
//! their results on the UI thread too.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::future::Future;

use rune::{AppHandle, Live, LiveText, PopupAnchor, TextEdit, Tx};
use ym_api::{Account, Album, Artist, ArtistPage, Lyrics, Playlist, SearchResult, Shelf, Track};
use ym_audio::{DiskCache, Engine, EqSettings};

use crate::art::ArtCache;
use crate::desktop::Desktop;
use crate::i18n::{Lang, S};
use crate::net::{self, Backend};
use crate::palette::Palette;
use crate::store::{DownloadStore, PinnedSetting, Repeat, Settings, Store, ThemeMode};
use crate::theme::{self, Pal};

#[derive(Clone, Debug, PartialEq)]
pub enum Page {
    Home,
    Search,
    Library,
    Settings,
    Album(String),
    Playlist(i64, i64),
    Artist(String),
}

/// A page the user chose to keep in the navigation rail.
///
/// The page is retained alongside its display data so the rail can show its cover even after the
/// detail page has been left. A pinned item is keyed by its page, which keeps pinning idempotent.
#[derive(Clone, Debug, PartialEq)]
pub struct PinnedItem {
    pub page: Page,
    pub title: String,
    pub cover: Option<String>,
    pub artist: bool,
}

impl PinnedItem {
    pub fn key(&self) -> String {
        self.page.key()
    }

    fn from_setting(p: &PinnedSetting) -> Option<Self> {
        let page = match p.kind.as_str() {
            "album" => Page::Album(p.id.clone()),
            "artist" => Page::Artist(p.id.clone()),
            "playlist" => Page::Playlist(p.id.parse().ok()?, p.secondary),
            _ => return None,
        };
        Some(Self { artist: matches!(&page, Page::Artist(_)), page, title: p.title.clone(), cover: p.cover.clone() })
    }

    fn setting(&self) -> PinnedSetting {
        let (kind, id, secondary) = match &self.page {
            Page::Album(id) => ("album", id.clone(), 0),
            Page::Artist(id) => ("artist", id.clone(), 0),
            Page::Playlist(uid, kind) => ("playlist", uid.to_string(), *kind),
            _ => return PinnedSetting::default(),
        };
        PinnedSetting { kind: kind.to_owned(), id, secondary, title: self.title.clone(), cover: self.cover.clone() }
    }
}

impl Page {
    pub fn key(&self) -> String {
        match self {
            Page::Home => "home".into(),
            Page::Search => "search".into(),
            Page::Library => "library".into(),
            Page::Settings => "settings".into(),
            Page::Album(id) => format!("album-{id}"),
            Page::Playlist(u, k) => format!("playlist-{u}-{k}"),
            Page::Artist(id) => format!("artist-{id}"),
        }
    }

    pub fn is_root(&self) -> bool {
        matches!(self, Page::Home | Page::Search | Page::Library | Page::Settings)
    }

    /// Which rail item lights up.
    pub fn rail(&self) -> Page {
        match self {
            Page::Album(_) | Page::Playlist(..) | Page::Artist(_) => Page::Library,
            other => other.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub enum Load<T> {
    #[default]
    Idle,
    Loading,
    Ready(T),
    Failed(String),
}

/// A track's download state, kept in the UI so the row can explain what a click did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DownloadState {
    Downloading,
    Done,
    Failed,
}

impl<T> Load<T> {
    pub fn get(&self) -> Option<&T> {
        match self {
            Load::Ready(v) => Some(v),
            _ => None,
        }
    }

    pub fn is_loading(&self) -> bool {
        matches!(self, Load::Loading)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LibTab {
    #[default]
    Tracks,
    Playlists,
    Albums,
    Artists,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SearchTab {
    #[default]
    All,
    Tracks,
    Albums,
    Artists,
    Playlists,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum NpTab {
    /// Neither side panel: the player alone, in the middle of the window.
    #[default]
    Closed,
    Lyrics,
    Queue,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub enum Session {
    #[default]
    LoggedOut,
    Demo,
    Live,
}

#[derive(Default)]
pub struct LoginUi {
    pub token: TextEdit,
    pub busy: bool,
    pub error: Option<S>,
    /// Show the pasted token instead of dots.
    pub reveal: bool,
}

#[derive(Default)]
pub struct SearchUi {
    pub edit: TextEdit,
    /// The text the current results are for.
    pub query: String,
    /// The text last seen in the box, and when it changed (for the debounce).
    pub seen: String,
    pub changed_at: f64,
    pub results: Load<SearchResult>,
    pub suggestions: Vec<String>,
    pub tab: SearchTab,
    pub generation: u64,
    /// One kind searched on its own (a chip other than "All" picked): many more of that kind than
    /// "All" brings back. `more_for` says which text and which kind it is for.
    pub more: Load<SearchResult>,
    pub more_for: Option<(String, SearchTab)>,
}

#[derive(Default, Clone)]
pub struct Detail {
    pub page: Option<Page>,
    pub title: String,
    pub subtitle: String,
    /// Cover template (as the API gives it).
    pub cover: Option<String>,
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub similar: Vec<Artist>,
    pub state: Load<()>,
    pub generation: u64,
    pub is_artist: bool,
    /// What the "..." card beside the buttons tells about it (genres, year, how many tracks, how
    /// long), each with its name; the head of the page keeps to the title and who made it.
    pub facts: Vec<(S, String)>,
    /// An artist's every track, once asked for ("All" beside the popular ones), and whether the
    /// list shows them instead of the popular ones.
    pub all_tracks: Load<Vec<Track>>,
    pub showing_all: bool,
    pub popular: Vec<Track>,
    /// What it is among the likes (`"album:ID"`, `"artist:ID"`, `"playlist:UID:KIND"`), when it
    /// can be liked at all (not one's own playlist).
    pub like_key: Option<String>,
    /// Its page on music.yandex.ru (to share it, or open it in the browser).
    pub link: Option<String>,
    /// An album's first artist (the menu leads to them).
    pub artist: Option<String>,
    /// An album's artists, each a link at the head of the page.
    pub artists: Vec<ym_api::Artist>,
    /// An album's year, beside its artists.
    pub year: Option<i32>,
    pub monthly_listeners: Option<u64>,
}

/// Where what plays came from, as the player names it.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayingFrom {
    /// My Wave (a mood or a wave by something), or a list.
    pub wave: bool,
    /// What it is, in words ("Album", "My Wave by artist"). The key, not the words: the language
    /// can change while the badge is on screen, and what it says has to follow it.
    pub kind: S,
    /// What it is called.
    pub name: FromName,
    /// Where pressing it leads.
    pub page: Option<Page>,
}

/// What the badge calls what plays: a title as it is, or a word of the app's own.
#[derive(Clone, Debug, PartialEq)]
pub enum FromName {
    /// A title, an artist's name: as it is, in any language.
    Text(String),
    /// A word of the app's own (the moods of My Wave): it follows the language.
    Word(S),
}

impl PlayingFrom {
    /// "My Wave · Calm", "Album · Name"; just "My Wave" when the name only repeats the kind (the
    /// plain wave is called what it is). `lang` is the language to say it in.
    pub fn line(&self, lang: Lang) -> String {
        let kind = self.kind.get(lang);
        let name = match &self.name {
            FromName::Text(t) => t.trim(),
            FromName::Word(w) => w.get(lang).trim(),
        };
        if name.is_empty() || name.to_lowercase() == kind.trim().to_lowercase() {
            kind.to_owned()
        } else {
            format!("{kind} · {name}")
        }
    }
}

#[derive(Default)]
pub struct Library {
    pub tab: LibTab,
    /// What the collection is searched for (every tab), and which liked tracks match it, as the
    /// view last worked out (`ListSrc::Liked` reads through it while there are words).
    pub filter: TextEdit,
    pub shown: std::cell::RefCell<Vec<usize>>,
    pub tracks: Load<Vec<Track>>,
    pub playlists: Load<Vec<Playlist>>,
    pub albums: Load<Vec<Album>>,
    pub artists: Load<Vec<Artist>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Manual,
    Wave { preset: String, batch: Option<String>, round: u64 },
}

pub struct Player {
    pub engine: Option<Engine>,
    pub engine_failed: bool,
    pub queue: Vec<Track>,
    pub original: Vec<Track>,
    pub index: Option<usize>,
    /// Queue index whose audio is loading before it becomes the visible current track.
    pub pending_index: Option<usize>,
    pub pending_token: Option<u64>,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub volume: f32,
    /// The user wants sound (not paused).
    pub playing: bool,
    pub loading: bool,
    pub pos_ms: u64,
    pub dur_ms: u64,
    pub token: u64,
    pub token_seed: u64,
    pub next: Option<(u64, usize)>,
    pub next_loading: Option<u64>,
    pub source: Source,
    pub wave_fetching: bool,
    pub sim_started: bool,
    pub eq: EqSettings,
}

#[derive(Default)]
pub struct LyricsUi {
    pub for_track: Option<String>,
    pub lyrics: Load<Lyrics>,
    pub active: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub id: u64,
    pub text: String,
    pub born: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DownloadActivity {
    pub total: usize,
    pub active: usize,
    pub done: usize,
    pub failed: usize,
    pub changed_at: f64,
}

/// Values that change every frame; bound to elements without re-running the view.
#[derive(Clone)]
pub struct Lives {
    pub seek: Live<[f32; 4]>,
    pub seek_big: Live<[f32; 4]>,
    pub clock: LiveText,
    pub clock_big: LiveText,
    pub remaining: LiveText,
    pub volume: Live<[f32; 4]>,
}

/// A pop-up that opens and closes on a spring: `t` is how open it is drawn (0 shut, 1 open), `v`
/// how fast that is changing.
///
/// The spring keeps its speed when the target flips: pressing again halfway reverses it from
/// wherever it is, with no jump and no waiting for it to finish. Reduce motion lands it at once.
#[derive(Clone, Copy, Debug, Default)]
pub struct Pop {
    pub t: f32,
    pub v: f32,
}

impl Pop {
    /// Steps toward open or shut over `dt` seconds. Returns whether anything moved.
    pub fn step(&mut self, open: bool, dt: f32, reduce_motion: bool) -> bool {
        let target = if open { 1.0 } else { 0.0 };
        if self.t == target && self.v == 0.0 {
            return false;
        }
        if reduce_motion {
            (self.t, self.v) = (target, 0.0);
            return true;
        }
        // Opening is a little slower than closing; both settle without overshoot, so a menu
        // grows out of its button and lands where it rests, and goes straight back in.
        let (omega, zeta) = if open { (std::f32::consts::TAU / 0.40, 0.92) } else { (std::f32::consts::TAU / 0.30, 1.0) };
        self.spring(target, omega, zeta, dt)
    }

    /// Like `step`, on the slower, critically damped spring the player page comes and goes on.
    pub fn glide(&mut self, open: bool, dt: f32, reduce_motion: bool) -> bool {
        let target = if open { 1.0 } else { 0.0 };
        if self.t == target && self.v == 0.0 {
            return false;
        }
        if reduce_motion {
            (self.t, self.v) = (target, 0.0);
            return true;
        }
        let omega = std::f32::consts::TAU / if open { 0.62 } else { 0.5 };
        self.spring(target, omega, 1.0, dt)
    }

    fn spring(&mut self, target: f32, omega: f32, zeta: f32, dt: f32) -> bool {
        let steps = (dt * 240.0).ceil().clamp(1.0, 30.0);
        let h = dt / steps;
        for _ in 0..steps as u32 {
            let accel = omega * omega * (target - self.t) - 2.0 * zeta * omega * self.v;
            self.v += accel * h;
            self.t += self.v * h;
        }
        if (self.t - target).abs() < 0.002 && self.v.abs() < 0.02 {
            (self.t, self.v) = (target, 0.0);
        }
        true
    }

    /// Whether it is still on its way to `open` or shut.
    pub fn moving(&self, open: bool) -> bool {
        self.t != (if open { 1.0 } else { 0.0 }) || self.v != 0.0
    }
}

/// The page's own wheel scrolling: a notch moves a target further down (or up), and the page
/// eases toward it every frame, so it is quick, soft, and coasts to a stop; notches that come
/// fast add up, so a spun wheel carries on further. (A trackpad scrolls as RUNE does, with its
/// own fling.)
#[derive(Clone, Debug, Default)]
pub struct WheelGlide {
    pub key: String,
    pub pos: f32,
    pub target: f32,
    pub active: bool,
}

/// How far one notch of the wheel scrolls a page, in points (RUNE's own step is 40).
pub const WHEEL_STEP: f32 = 104.0;

thread_local! {
    /// Where each page is scrolled to, and how far it can go (`[offset, max]`), as RUNE last
    /// reported it, by the page's scroll key. (One shared pair was read by the next page before it
    /// had reported: a page that had just opened scrolled by the last page's limits, or not at all.)
    pub static PAGE_SCROLL: std::cell::RefCell<HashMap<String, [f32; 2]>> = std::cell::RefCell::new(HashMap::new());
    /// The scroll key of the page on screen: a wheel turned over anything inside it (a row of
    /// cards that scrolls sideways, say) glides the page.
    pub static PAGE_KEY: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// What RUNE last said about the page `key`'s scrolling (`[offset, max]`), if it has said anything.
pub fn page_scroll(key: &str) -> Option<[f32; 2]> {
    PAGE_SCROLL.with_borrow(|m| m.get(key).copied())
}

impl WheelGlide {
    /// A wheel delta (`dy` points, positive moving the content down, i.e. scrolling up).
    pub fn wheel(&mut self, key: &str, dy: f32) {
        // Until the page has reported, it is at its top and its end is not known yet (the frame
        // after the first jump reports it, and `step` clamps to it then).
        let [offset, max] = page_scroll(key).unwrap_or([0.0, f32::MAX]);
        if !self.active || self.key != key {
            self.key = key.to_owned();
            self.pos = offset;
            self.target = offset;
        }
        let notches = dy / 40.0;
        self.target = (self.target - notches * WHEEL_STEP).clamp(0.0, max.max(0.0));
        self.active = true;
    }

    /// A wheel delta over the page on screen, wherever in it the pointer is.
    pub fn wheel_page(&mut self, dy: f32) -> bool {
        let key = PAGE_KEY.with_borrow(Clone::clone);
        if key.is_empty() {
            return false;
        }
        self.wheel(&key, dy);
        true
    }

    /// One frame of `dt` seconds: the offset to show, or `None` once it has arrived.
    pub fn step(&mut self, dt: f32, reduce: bool) -> Option<f32> {
        if !self.active {
            return None;
        }
        if let Some([_, max]) = page_scroll(&self.key) {
            self.target = self.target.min(max.max(0.0));
        }
        let k = if reduce { 1.0 } else { 1.0 - (-dt * 11.0).exp() };
        self.pos += (self.target - self.pos) * k;
        if (self.target - self.pos).abs() < 0.4 {
            self.pos = self.target;
            self.active = false;
        }
        Some(self.pos)
    }
}

/// What a popover lists.
#[derive(Clone, Debug)]
pub enum Popover {
    /// Every artist of a track, each with their picture.
    Artists { artists: Vec<Artist>, anchor: String },
    /// What the open album, playlist or artist is about (`Detail::facts`).
    About { anchor: String },
    /// What can be done with the open album, playlist or artist (the "..." beside its buttons).
    Actions { anchor: String },
    /// The waves a wave card's arrow offers: by the track, by each of its artists, by its release.
    Waves { choices: Vec<WaveChoice>, anchor: String },
    /// The audio quality used for the next track load.
    Quality { anchor: String },
    /// How the picture behind the open player looks (`Settings::stage`).
    Stage { anchor: String },
    /// Actions for one cover in the pinned-items rail.
    Pinned { item: PinnedItem, anchor: String },
    /// The download queue and its completed and failed items.
    Downloads { anchor: String },
}

/// One wave of a wave card's list: what it is seeded by ("By artist"), and the thing's name.
#[derive(Clone, Debug)]
pub struct WaveChoice {
    pub station: Station,
    pub kind: String,
    pub name: String,
}

impl Popover {
    pub fn anchor(&self) -> &str {
        match self {
            Popover::Artists { anchor, .. }
            | Popover::About { anchor }
            | Popover::Actions { anchor }
            | Popover::Waves { anchor, .. }
            | Popover::Quality { anchor }
            | Popover::Stage { anchor }
            | Popover::Pinned { anchor, .. }
            | Popover::Downloads { anchor } => anchor,
        }
    }
}

/// What the track menu is about, and where it opens from.
#[derive(Clone, Debug)]
pub struct TrackMenu {
    pub track: Track,
    pub queue_index: Option<usize>,
    pub anchor: PopupAnchor,
    /// Opened from the player itself (the bar, the open player): the player's own buttons already
    /// play, pause and queue, so the menu leaves those out.
    pub player: bool,
}

/// A wave seeded by one thing, as Yandex Music offers it ("My Wave by track" and so on).
#[derive(Clone, Debug, PartialEq)]
pub enum Station {
    Track(String),
    Artist(String),
    Album(String),
    /// What one has liked.
    Collection,
}

impl Station {
    /// Its id as a wave (`net::Backend::wave`).
    pub fn id(&self) -> String {
        match self {
            Station::Track(id) => format!("track:{id}"),
            Station::Artist(id) => format!("artist:{id}"),
            Station::Album(id) => format!("album:{id}"),
            Station::Collection => "collection".to_owned(),
        }
    }

    /// Whether the wave playing (`Source::Wave`'s preset) is this one.
    pub fn is(&self, preset: &str) -> bool {
        self.id() == preset
    }
}

/// What the details drawer ("Details") tells about.
#[derive(Clone, Debug)]
pub enum Sheet {
    Track(Track),
    /// The open album, playlist or artist (`State::detail`).
    Page,
}

pub struct State {
    // plumbing
    pub app: Option<AppHandle<State>>,
    pub store: Store,
    pub art: ArtCache,
    /// One bounded cache for album art and lightweight metadata. Audio files live in the
    /// dedicated downloaded-track store below.
    pub cache: Option<std::sync::Arc<DiskCache>>,
    pub download_store: Option<std::sync::Arc<DownloadStore>>,
    pub download_confirm: Option<Vec<Track>>,
    pub delete_download_confirm: Option<Track>,
    pub downloads: HashMap<String, DownloadState>,
    pub download_titles: HashMap<String, String>,
    pub download_activity: Option<DownloadActivity>,
    pub backend: Backend,
    pub headless: bool,
    pub now: f64,
    pub prev_now: f64,
    pub bar_w: Cell<f32>,
    pub bar_big_w: Cell<f32>,
    pub vol_w: Cell<f32>,
    pub win_w: Cell<f32>,
    pub lives: Lives,
    /// The whole seconds the clock and the remaining time last showed, and the strings themselves:
    /// the per-frame sync formats a new time only when the second changes, into the same buffer.
    pub clock_secs: Cell<u64>,
    pub remaining_secs: Cell<u64>,
    pub clock_text: RefCell<String>,
    pub remaining_text: RefCell<String>,
    /// Bytes taken by cover art and by downloaded audio and metadata for Settings, and when they
    /// were measured: scanning the folders on every view build is far more work than the numbers
    /// are worth.
    pub disk_usage: Cell<(u64, u64, u64, f64)>,
    /// The track whose cover the Windows media card last asked for.
    pub card_cover: RefCell<Option<String>>,
    // settings & session
    pub settings: Settings,
    pub lang: Lang,
    pub session: Session,
    pub account: Option<Account>,
    pub login: LoginUi,
    // navigation
    pub page: Page,
    /// Pages kept in the rail as cover-art shortcuts.
    pub pinned_items: Vec<PinnedItem>,
    /// The pages behind (back) and ahead (forward), as a browser keeps them.
    pub history: Vec<Page>,
    pub listening_history: Vec<Track>,
    pub forward: Vec<Page>,
    /// Where what plays came from ("from the album …", "My Wave · Calm"), shown by the player.
    pub playing_from: Option<PlayingFrom>,
    /// The name and kind of the wave being started, until its first tracks land. What a wave is
    /// called, and what it was seeded by: the words are kept as keys, so they follow the language.
    pub wave_label: Option<(S, FromName, Option<Page>)>,
    // content
    pub home: Load<Vec<Shelf>>,
    pub liked: HashSet<String>,
    /// The albums, artists and playlists liked (`Detail::like_key`s), once they have been asked for.
    pub liked_items: HashSet<String>,
    pub likes_loaded: bool,
    /// The art cache key of the picture the open artist page draws in its shape (`fx::SHAPE` with
    /// the picture fill reads it from here).
    pub pic_key: std::cell::RefCell<Option<String>>,
    /// The butterflies on Home, and where their layer and the things they land on were last laid out.
    pub swarm: crate::bugs::Swarm,
    pub bug_geom: std::cell::RefCell<crate::bugs::Geom>,
    pub library: Library,
    pub search: SearchUi,
    pub detail: Detail,
    pub wave_choice: String,
    // playback
    pub player: Player,
    pub lyrics: LyricsUi,
    /// How far forward each lyric line is (0 back .. 1 the sung one), and how fast that is
    /// changing: stepped every frame toward the line being sung (`ui::np::step_lines`).
    pub ly_e: Vec<f32>,
    pub ly_v: Vec<f32>,
    /// Whether the lyrics panel was open for the last lyrics that were known (`np_panel`).
    pub ly_panel: Cell<bool>,
    pub np_open: bool,
    pub np_tab: NpTab,
    /// The profile menu (My Wave, Settings, sign out) is open.
    pub menu_open: bool,
    /// How open the profile menu is drawn (0 shut, 1 open) and how fast that is changing: a spring
    /// that `tick_menu` steps toward `menu_open`.
    pub menu_t: f32,
    pub menu_v: f32,
    /// The track menu (right click, or the "..." of a row): which track and where it opens from,
    /// whether it is meant to be open, and the spring that draws it. It is kept while it closes.
    pub track_menu: Option<TrackMenu>,
    pub tmenu_open: bool,
    pub tmenu: Pop,
    /// The side sheet that slides in from the right with the details of a track or the open page.
    pub sheet: Option<Sheet>,
    pub sheet_open: bool,
    pub sheet_pop: Pop,
    /// The playing track's title under the pointer (which one: the bar's or the player's), and
    /// since when: a ray of light runs across it.
    pub title_hover: Option<(&'static str, f64)>,
    /// The volume card that grows out of the dock's speaker button, and the spring that draws it.
    pub vol_open: bool,
    /// A short grace period keeps the volume card open while the pointer travels from the
    /// speaker button to the card across its small visual gap.
    pub vol_close_at: Option<f64>,
    pub vol: Pop,
    /// The small card that grows out of a button with a list in it (the artists of a track, what
    /// a page is about), and the spring that draws it. It is kept while it closes.
    pub popover: Option<Popover>,
    pub popover_open: bool,
    pub pop: Pop,
    /// A cover opened large over the window (its template), where it grew from, and its spring.
    pub lightbox: Option<(String, String)>,
    pub lightbox_open: bool,
    pub light: Pop,
    /// The account's picture (a URL), once Yandex ID has said which it is.
    pub avatar: Option<String>,
    /// What Home's shapes are drawn from: new each time the app starts, and when they are re-rolled.
    pub home_seed: u64,
    /// What the pointer is over in Home's garden (a mood's id, or "wave"): that shape lifts.
    pub garden_hover: Option<&'static str>,
    /// The wave card under the pointer (its key): its ribbons quicken.
    pub wave_hover: Option<String>,
    pub glide: WheelGlide,
    /// How far the player page has come over the window (0: not at all, 1: it covers it).
    pub np: Pop,
    /// The player's soft top and bottom edges, which grow in once it covers the window.
    pub np_veil: Pop,
    /// `ArtCache::landed` as of the last view built.
    pub art_landed: u64,
    /// The equaliser preset list in Settings is open.
    pub eq_open: bool,
    pub spectrum: [f32; 32],
    /// What the music is doing tonally (chords, melody, brightness), for the open player's stage.
    pub tone: ym_audio::Tone,
    pub level: f32,
    pub palette: Palette,
    pub palette_key: Option<String>,
    /// The playing cover, tiny, for the shader behind the player; it changes with `palette_key`.
    pub cover_thumb: Option<std::sync::Arc<crate::art::Thumb>>,

    /// The look for a newer version, and the install of it (see `update.rs`).
    pub update: crate::update::Ui,
    pub update_method: glint_update::Method,
    pub fkcensor: crate::fkcensor::FkCensor,
    pub fkcensor_checked: f64,
    pub toasts: Vec<Toast>,
    pub toast_seq: u64,
    pub settings_dirty_at: Option<f64>,
    pub scroll_lyrics: bool,
    /// The seek bar is being dragged: the engine's position is not fed back meanwhile.
    pub scrubbing: bool,
    /// When the heart was last pressed on (drives the sparkle burst).
    pub like_burst: f64,
    pub burst_track: String,
    pub burst_on: bool,
    /// Focus the search box on the next frame.
    pub focus_search: bool,
    pub eq_preset: usize,
    /// The lyrics are on screen (so a change of line scrolls them).
    pub lyrics_showing: bool,
    /// The tray icon and the media keys; started with the first frame of a real window.
    pub desktop: Option<Desktop>,
    /// `GLINT_AUTOPLAY` is set: play the liked tracks as soon as they have loaded (for
    /// checking the media keys and the tray without clicking through the app).
    pub autoplay: bool,
    /// What Windows says about light and dark, read now and then.
    pub sys_light: bool,
    pub sys_checked: f64,
    /// Whether lrc.red was usable the last time we looked, to reload the lyrics when that flips.
    pub lrc_usable: bool,
}

/// The app's home page, sent to lrc.red as the contact required by its API terms.
pub const APP_URL: &str = "https://github.com/orphytype/glint";

/// How many searches the history keeps.
pub const SEARCH_HISTORY: usize = 12;

/// The hour of the day on this computer's clock (evening in headless runs, so pictures repeat).
pub fn local_hour() -> u32 {
    if is_headless() {
        return 20;
    }
    #[cfg(windows)]
    {
        // SAFETY: GetLocalTime only writes the SYSTEMTIME it is given.
        let mut t: windows_sys::Win32::Foundation::SYSTEMTIME = unsafe { std::mem::zeroed() };
        unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut t) };
        u32::from(t.wHour)
    }
    #[cfg(unix)]
    {
        // SAFETY: `time` with a null pointer only returns the time; `localtime_r` writes only the
        // `tm` it is given.
        unsafe {
            let now = libc::time(std::ptr::null_mut());
            let mut tm: libc::tm = std::mem::zeroed();
            if libc::localtime_r(&now, &mut tm).is_null() { 12 } else { tm.tm_hour.clamp(0, 23) as u32 }
        }
    }
    #[cfg(not(any(windows, unix)))]
    {
        12
    }
}

pub fn is_headless() -> bool {
    std::env::args().any(|a| matches!(a.as_str(), "--snap" | "--scene" | "--tree" | "--script" | "--frames" | "--video" | "--trace" | "--strip" | "--list-scenes"))
}

impl State {
    pub fn new(app: Option<AppHandle<State>>, store: Store, headless: bool) -> State {
        let settings = store.load();
        let lang = settings.lang.as_deref().and_then(Lang::from_code).unwrap_or_else(Lang::system);
        let eq = EqSettings { enabled: settings.eq_enabled, preamp_db: settings.eq_preamp, gains_db: settings.eq_gains };
        let cache = (!headless).then(|| store.cache_dir()).flatten().and_then(|d| DiskCache::open(d, DiskCache::UNLIMITED).ok()).map(std::sync::Arc::new);
        let download_store = (!headless).then(|| store.downloads_dir()).flatten().and_then(|d| DownloadStore::open(d).ok()).map(std::sync::Arc::new);
        let mut downloads = HashMap::new();
        let mut download_titles = HashMap::new();
        if let Some(store) = &download_store {
            for item in store.tracks() {
                downloads.insert(item.track.id.clone(), DownloadState::Done);
                download_titles.insert(item.track.id.clone(), item.track.full_title());
            }
        }
        State {
            app,
            art: ArtCache::new(),
            cache,
            download_store,
            download_confirm: None,
            delete_download_confirm: None,
            downloads,
            download_titles,
            download_activity: None,
            backend: Backend::None,
            headless,
            now: 0.0,
            prev_now: 0.0,
            bar_w: Cell::new(600.0),
            bar_big_w: Cell::new(420.0),
            vol_w: Cell::new(96.0),
            win_w: Cell::new(1280.0),
            lives: Lives {
                seek: Live::new([0.0, 0.0, 0.0, 4.0]),
                seek_big: Live::new([0.0, 0.0, 0.0, 6.0]),
                clock: LiveText::new("0:00"),
                clock_big: LiveText::new("0:00"),
                remaining: LiveText::new("-0:00"),
                volume: Live::new([0.0, 0.0, 0.0, 4.0]),
            },
            clock_secs: Cell::new(u64::MAX),
            remaining_secs: Cell::new(u64::MAX),
            clock_text: RefCell::new(String::new()),
            remaining_text: RefCell::new(String::new()),
            disk_usage: Cell::new((0, 0, 0, f64::NEG_INFINITY)),
            card_cover: RefCell::new(None),
            lang,
            session: Session::LoggedOut,
            account: None,
            login: LoginUi::default(),
            page: Page::Home,
            pinned_items: settings.pinned.iter().filter_map(PinnedItem::from_setting).collect(),
            history: Vec::new(),
            listening_history: Vec::new(),
            forward: Vec::new(),
            playing_from: None,
            wave_label: None,
            home: Load::Idle,
            liked: HashSet::new(),
            library: Library::default(),
            search: SearchUi::default(),
            detail: Detail::default(),
            wave_choice: settings.wave.clone(),
            player: Player {
                engine: None,
                engine_failed: false,
                queue: Vec::new(),
                original: Vec::new(),
                index: None,
                pending_index: None,
                pending_token: None,
                shuffle: settings.shuffle,
                repeat: settings.repeat,
                volume: settings.volume,
                playing: false,
                loading: false,
                pos_ms: 0,
                dur_ms: 0,
                token: 0,
                token_seed: 0,
                next: None,
                next_loading: None,
                source: Source::Manual,
                wave_fetching: false,
                sim_started: false,
                eq,
            },
            lyrics: LyricsUi::default(),
            ly_e: Vec::new(),
            ly_v: Vec::new(),
            np_open: false,
            np_tab: NpTab::Closed,
            menu_open: false,
            menu_t: 0.0,
            menu_v: 0.0,
            track_menu: None,
            tmenu_open: false,
            tmenu: Pop::default(),
            sheet: None,
            sheet_open: false,
            sheet_pop: Pop::default(),
            title_hover: None,
            vol_open: false,
            vol_close_at: None,
            vol: Pop::default(),
            popover: None,
            popover_open: false,
            pop: Pop::default(),
            lightbox: None,
            lightbox_open: false,
            light: Pop::default(),
            avatar: None,
            home_seed: if headless { 7 } else { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(7, |d| d.as_nanos() as u64) },
            swarm: crate::bugs::Swarm::new(0),
            bug_geom: std::cell::RefCell::new(crate::bugs::Geom::default()),
            glide: WheelGlide::default(),
            garden_hover: None,
            wave_hover: None,
            np: Pop::default(),
            np_veil: Pop::default(),
            art_landed: 0,
            eq_open: false,
            spectrum: [0.0; 32],
            tone: ym_audio::Tone::default(),
            level: 0.0,
            palette: Palette::idle(),
            palette_key: None,
            cover_thumb: None,
            liked_items: HashSet::new(),
            likes_loaded: false,
            pic_key: std::cell::RefCell::new(None),

            update: crate::update::Ui::default(),
            update_method: crate::update::method(),
            fkcensor: crate::fkcensor::FkCensor::default(),
            fkcensor_checked: -600.0,
            toasts: Vec::new(),
            toast_seq: 0,
            settings_dirty_at: None,
            scroll_lyrics: false,
            scrubbing: false,
            like_burst: -100.0,
            burst_track: String::new(),
            burst_on: false,
            focus_search: false,
            eq_preset: 0,
            lyrics_showing: false,
            ly_panel: Cell::new(false),
            desktop: None,
            autoplay: !headless && std::env::var_os("GLINT_AUTOPLAY").is_some(),
            sys_light: false,
            sys_checked: -100.0,
            lrc_usable: false,
            settings,
            store,
        }
    }

    // ---- plumbing -----------------------------------------------------------------------------

    /// Runs `fut` on the background runtime and applies its result on the UI thread.
    pub fn job<T: Send + 'static>(&self, fut: impl Future<Output = T> + Send + 'static, apply: impl FnOnce(&mut State, &mut Tx, T) + Send + 'static) {
        let Some(app) = self.app.clone() else { return };
        net::spawn(async move {
            let out = fut.await;
            app.update(move |s, tx| apply(s, tx, out));
        });
    }

    pub fn tr(&self, s: S) -> &'static str {
        s.get(self.lang)
    }

    pub fn pal(&self) -> Pal {
        Pal::new(theme::accent_srgb(self.settings.accent, self.palette.accent))
    }

    /// Whether the light look is showing: the setting, or Windows' own when it says "system".
    pub fn is_light(&self) -> bool {
        match self.settings.theme {
            ThemeMode::Dark => false,
            ThemeMode::Light => true,
            ThemeMode::System => self.sys_light,
        }
    }

    /// Opens or closes the Now Playing page.
    pub fn set_np(&mut self, open: bool) {
        self.menu_open = false;
        self.tmenu_open = false;
        self.vol_open = false;
        self.vol_close_at = None;
        self.np_open = open;
    }

    pub fn toast(&mut self, text: impl Into<String>) {
        self.toast_seq += 1;
        let t = Toast { id: self.toast_seq, text: text.into(), born: self.now };
        self.toasts.push(t);
        if self.toasts.len() > 3 {
            self.toasts.remove(0);
        }
    }

    pub fn open_download_folder(&self) {
        let Some(path) = self.store.downloads_dir() else { return };
        #[cfg(windows)]
        let _ = std::process::Command::new("explorer").arg(path).spawn();
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(path).spawn();
        #[cfg(all(unix, not(target_os = "macos")))]
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }

    pub fn clear_download_history(&mut self) {
        if let Some(store) = &self.download_store {
            let _ = store.clear();
        }
        self.downloads.clear();
        self.download_titles.clear();
        self.download_activity = None;
        self.toast(self.tr(S::ClearDownloadHistory));
    }

    pub fn error_toast(&mut self, e: &ym_api::Error) {
        let text = net::describe(e, self.lang);
        self.toast(text);
    }

    pub fn mark_settings_dirty(&mut self) {
        self.settings_dirty_at = Some(self.now);
    }

    pub fn save_settings_now(&mut self) {
        self.settings.lang = Some(self.lang.code().into());
        self.settings.volume = self.player.volume;
        self.settings.shuffle = self.player.shuffle;
        self.settings.repeat = self.player.repeat;
        self.settings.eq_enabled = self.player.eq.enabled;
        self.settings.eq_preamp = self.player.eq.preamp_db;
        self.settings.eq_gains = self.player.eq.gains_db;
        self.settings.wave = self.wave_choice.clone();
        self.settings.demo = self.session == Session::Demo;
        self.settings.pinned = self.pinned_items.iter().map(PinnedItem::setting).collect();
        self.store.save(&self.settings);
        self.settings_dirty_at = None;
    }

    /// Hands the current lrc.red choice to the client, and reloads the lyrics when it changed
    /// whether lrc.red can be asked at all.
    pub fn apply_lrc(&mut self) {
        let contact = self.settings.word_lyrics.then_some(APP_URL);
        if let Backend::Live(c) = &self.backend {
            c.set_lrcred_contact(contact);
        }
        let usable = contact.is_some();
        if usable != self.lrc_usable {
            self.lrc_usable = usable;
            self.lyrics.for_track = None;
            self.load_lyrics_for_current();
        }
    }

    pub fn set_word_lyrics(&mut self, on: bool) {
        self.settings.word_lyrics = on;
        self.apply_lrc();
        self.mark_settings_dirty();
    }

    pub fn set_fkcensor(&mut self, on: bool) {
        if self.settings.fkcensor == on {
            return;
        }
        self.settings.fkcensor = on;
        if on && let Backend::Live(client) = &self.backend {
            self.refresh_fkcensor(client.clone());
        }
        if let Some(index) = self.player.index {
            self.play_index(index);
        }
        self.mark_settings_dirty();
    }

    fn refresh_fkcensor(&mut self, client: ym_api::Client) {
        self.fkcensor_checked = self.now;
        let fkcensor = self.fkcensor.clone();
        self.job(async move { fkcensor.refresh(&client).await }, |s, _tx, changed| {
            if changed && s.settings.fkcensor && let Some(index) = s.player.index {
                s.play_index(index);
            }
        });
    }

    pub fn set_lang(&mut self, lang: Lang) {
        self.lang = lang;
        if let Backend::Live(c) = &self.backend {
            c.set_language(lang.code());
        }
        self.mark_settings_dirty();
        // Shelf titles and API texts follow the language, so fetch them again.
        if self.session != Session::LoggedOut {
            self.reload_content();
        }
    }

    // ---- sessions -----------------------------------------------------------------------------

    pub fn start_demo(&mut self) {
        self.backend = Backend::demo();
        self.session = Session::Demo;
        self.account = self.backend_account_now();
        self.fetch_avatar();
        self.login.error = None;
        self.reload_content();
        self.mark_settings_dirty();
    }

    fn backend_account_now(&self) -> Option<Account> {
        match &self.backend {
            Backend::Demo(c) => Some(c.account()),
            _ => None,
        }
    }

    /// Signs in with a token the user pasted (or one restored from disk).
    pub fn sign_in(&mut self, token: String, remember: bool) {
        let client = match ym_api::Client::new() {
            Ok(c) => c,
            Err(_) => {
                self.login.error = Some(S::Offline);
                return;
            }
        };
        client.set_token(Some(token.clone()));
        client.set_language(self.lang.code());
        client.set_lrcred_contact(self.settings.word_lyrics.then_some(APP_URL));
        self.lrc_usable = self.settings.word_lyrics;
        self.login.busy = true;
        self.login.error = None;
        let client_for_fkcensor = client.clone();
        let backend = Backend::Live(client);
        let b2 = backend.clone();
        self.job(async move { b2.account().await }, move |s, _tx, res| {
            s.login.busy = false;
            match res {
                Ok(acc) => {
                    if remember {
                        s.store.save_token(Some(&token));
                    }
                    s.backend = backend;
                    s.session = Session::Live;
                    s.account = Some(acc);
                    if s.settings.fkcensor {
                        s.refresh_fkcensor(client_for_fkcensor.clone());
                    }
                    s.fetch_avatar();
                    s.login.token = TextEdit::new("");
                    s.reload_content();
                    s.mark_settings_dirty();
                }
                Err(ym_api::Error::Unauthorized) => {
                    // A rejected token is useless from now on, whether pasted or remembered.
                    s.store.save_token(None);
                    s.login.error = Some(S::TokenRejected);
                }
                Err(e) => {
                    if matches!(e, ym_api::Error::Network(_))
                        && s.download_store.as_ref().is_some_and(|store| !store.tracks().is_empty())
                    {
                        // A remembered token can be valid while the service is unreachable. Keep
                        // the session usable from the downloaded-track store in that case.
                        s.backend = backend;
                        s.session = Session::Live;
                        s.account = Some(Account { uid: 0, login: String::new(), display_name: String::new(), has_plus: false });
                        s.login.token = TextEdit::new("");
                        s.login.error = None;
                        s.reload_content();
                        s.toast(S::Offline.get(s.lang));
                    } else {
                        s.login.error = Some(match e {
                            ym_api::Error::Network(_) => S::Offline,
                            _ => S::TokenRejected,
                        });
                    }
                }
            }
        });
    }

    /// Tries the pasted text as a token; shows an error when it is not one.
    pub fn submit_token(&mut self) {
        match ym_api::auth::extract_token(&self.login.token.text) {
            Some(t) => self.sign_in(t, true),
            None => self.login.error = Some(S::BadToken),
        }
    }

    pub fn sign_out(&mut self) {
        self.menu_open = false;
        self.stop_all();
        self.store.save_token(None);
        self.backend = Backend::None;
        self.session = Session::LoggedOut;
        self.account = None;
        self.avatar = None;
        self.home = Load::Idle;
        self.liked.clear();
        self.liked_items.clear();
        self.likes_loaded = false;
        self.library = Library::default();
        self.detail = Detail::default();
        self.search = SearchUi::default();
        self.history.clear();
        self.forward.clear();
        self.playing_from = None;
        self.page = Page::Home;
        self.np_open = false;
        self.mark_settings_dirty();
        let text = self.tr(S::SignedOut);
        self.toast(text);
    }

    // ---- navigation ---------------------------------------------------------------------------

    /// Restores last time's session: the demo, or the remembered token.
    pub fn boot(&mut self) {
        if self.settings.demo {
            self.start_demo();
        } else if let Some(token) = self.store.load_token() {
            self.sign_in(token, false);
        }
        glint_update::cleanup();
        self.auto_check_updates();
    }

    pub fn go(&mut self, page: Page) {
        // Opening a result is what makes a search worth remembering (not every word typed).
        if self.page == Page::Search && page != Page::Search {
            self.remember_search();
        }
        self.menu_open = false;
        self.tmenu_open = false;
        self.vol_open = false;
        self.vol_close_at = None;
        self.eq_open = false;
        if page == Page::Search {
            self.focus_search = true;
        }
        // Whatever it leads to, the player gets out of the way, also when that page is the one
        // already under it (the title of the playing track, pressed on its own album's page);
        // so do the details drawer and the photo viewer.
        self.np_open = false;
        self.sheet_open = false;
        self.lightbox_open = false;
        if self.page == page {
            return;
        }
        self.history.push(self.page.clone());
        if self.history.len() > 40 {
            self.history.remove(0);
        }
        self.forward.clear();
        self.glide.active = false;
        self.page = page.clone();
        self.enter(page);
    }

    /// Whether there is a page to go back to (or an artist's every track to fold).
    pub fn can_back(&self) -> bool {
        !self.history.is_empty() || (self.detail.showing_all && matches!(self.page, Page::Artist(_)))
    }

    pub fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    pub fn back(&mut self) {
        // From an artist's every track, back is to the artist's page as it was.
        if self.detail.showing_all && matches!(self.page, Page::Artist(_)) {
            self.show_all_artist_tracks(false);
            return;
        }
        if let Some(p) = self.history.pop() {
            self.forward.push(self.page.clone());
            self.leave_to(p);
        }
    }

    pub fn go_forward(&mut self) {
        if let Some(p) = self.forward.pop() {
            self.history.push(self.page.clone());
            self.leave_to(p);
        }
    }

    /// Shows `p` as a step through the history (back or forward): nothing is pushed.
    fn leave_to(&mut self, p: Page) {
        if self.page == Page::Search && p != Page::Search {
            self.remember_search();
        }
        self.menu_open = false;
        self.tmenu_open = false;
        self.vol_open = false;
        self.vol_close_at = None;
        self.np_open = false;
        self.glide.active = false;
        // The same page (an album opened twice in a row) is loaded again only when it is not
        // the one already showing.
        let same = self.page == p;
        self.page = p.clone();
        if !same {
            self.enter(p);
        }
    }

    /// Loads whatever a page needs the first time it is shown.
    fn enter(&mut self, page: Page) {
        let detail = !page.is_root();
        match page {
            Page::Library => self.load_library_tab(self.library.tab),
            Page::Album(id) => self.open_album(&id),
            Page::Playlist(uid, kind) => self.open_playlist(uid, kind),
            Page::Artist(id) => self.open_artist(&id),
            _ => {}
        }
        if detail {
            self.ensure_likes();
        }
    }

    // ---- likes of albums, artists and playlists ---------------------------------------------------

    /// Asks once which albums, artists and playlists are liked (the heart on their pages).
    pub fn ensure_likes(&mut self) {
        if self.likes_loaded || self.session == Session::LoggedOut {
            return;
        }
        self.likes_loaded = true;
        let (b, uid) = (self.backend.clone(), self.account.as_ref().map_or(0, |a| a.uid));
        self.job(async move { b.liked_items(uid).await }, |s, _, res| match res {
            Ok(keys) => s.liked_items.extend(keys),
            // Asked again on the next page that has a heart.
            Err(_) => s.likes_loaded = false,
        });
    }

    pub fn is_item_liked(&self, key: &str) -> bool {
        self.liked_items.contains(key)
    }

    /// The heart on the open album, playlist or artist page.
    pub fn toggle_detail_like(&mut self) {
        let Some(key) = self.detail.like_key.clone() else { return };
        let like = !self.liked_items.contains(&key);
        let (what, id) = match key.split_once(':') {
            Some(("album", id)) => (ym_api::Likeable::Album, id.to_owned()),
            Some(("artist", id)) => (ym_api::Likeable::Artist, id.to_owned()),
            Some(("playlist", id)) => (ym_api::Likeable::Playlist, id.to_owned()),
            _ => return,
        };
        if like {
            self.liked_items.insert(key.clone());
        } else {
            self.liked_items.remove(&key);
        }
        // The collection shows it next time it is opened.
        match what {
            ym_api::Likeable::Album => self.library.albums = Load::Idle,
            ym_api::Likeable::Artist => self.library.artists = Load::Idle,
            ym_api::Likeable::Playlist => self.library.playlists = Load::Idle,
        }
        let text = self.tr(if like { S::AddedToCollection } else { S::RemovedFromCollection });
        self.toast(text);
        let (b, uid) = (self.backend.clone(), self.account.as_ref().map_or(0, |a| a.uid));
        self.job(async move { b.set_item_like(uid, what, &id, like).await }, move |s, _, res| {
            if let Err(e) = res {
                if like {
                    s.liked_items.remove(&key);
                } else {
                    s.liked_items.insert(key);
                }
                s.error_toast(&e);
            }
        });
    }

    /// Whether the open detail page is present in the pinned rail.
    pub fn detail_is_pinned(&self) -> bool {
        self.detail.page.as_ref().is_some_and(|page| self.pinned_items.iter().any(|item| item.page == *page))
    }

    /// Adds or removes the open album, playlist or artist from the pinned rail.
    pub fn toggle_detail_pin(&mut self) {
        let Some(page) = self.detail.page.clone() else { return };
        let key = page.key();
        if let Some(i) = self.pinned_items.iter().position(|item| item.key() == key) {
            self.pinned_items.remove(i);
            let text = self.tr(S::Unpinned);
            self.toast(text);
            self.mark_settings_dirty();
            return;
        }
        if self.detail.title.is_empty() || self.detail.state.is_loading() {
            return;
        }
        self.pinned_items.push(PinnedItem {
            page,
            title: self.detail.title.clone(),
            cover: self.detail.cover.clone(),
            artist: self.detail.is_artist,
        });
        let text = self.tr(S::Pinned);
        self.toast(text);
        self.mark_settings_dirty();
    }

    /// Removes a pinned item by its stable page key.
    pub fn remove_pinned(&mut self, key: &str) {
        let before = self.pinned_items.len();
        self.pinned_items.retain(|item| item.key() != key);
        if self.pinned_items.len() != before {
            self.close_popover();
            let text = self.tr(S::Unpinned);
            self.toast(text);
            self.mark_settings_dirty();
        }
    }

    // ---- content ------------------------------------------------------------------------------

    pub fn reload_content(&mut self) {
        self.home = Load::Loading;
        self.library.tracks = Load::Loading;
        self.library.playlists = Load::Idle;
        self.library.albums = Load::Idle;
        self.library.artists = Load::Idle;
        let (b, uid) = (self.backend.clone(), self.account.as_ref().map_or(0, |a| a.uid));
        let b2 = b.clone();
        let offline_tracks: Vec<Track> = self.download_store.as_ref().map(|store| store.tracks().into_iter().map(|item| item.track).collect()).unwrap_or_default();
        self.job(async move { b2.home().await }, |s, _, res| {
            s.home = match res {
                Ok(v) => Load::Ready(v),
                Err(e) => {
                    s.error_toast(&e);
                    Load::Failed(e.to_string())
                }
            }
        });
        self.job(async move { b.liked_tracks(uid).await }, move |s, _, res| match res {
            Ok(v) => {
                s.liked = v.iter().map(|t| t.id.clone()).collect();
                s.library.tracks = Load::Ready(v);
            }
            Err(_) if !offline_tracks.is_empty() => {
                s.liked = offline_tracks.iter().map(|t| t.id.clone()).collect();
                s.library.tracks = Load::Ready(offline_tracks.clone());
                s.toast(s.tr(S::Offline));
            }
            Err(e) => s.library.tracks = Load::Failed(e.to_string()),
        });
        if self.page == Page::Library {
            self.load_library_tab(self.library.tab);
        }
    }

    pub fn load_library_tab(&mut self, tab: LibTab) {
        self.library.tab = tab;
        let (b, uid) = (self.backend.clone(), self.account.as_ref().map_or(0, |a| a.uid));
        match tab {
            LibTab::Tracks => {}
            LibTab::Playlists if matches!(self.library.playlists, Load::Idle | Load::Failed(_)) => {
                self.library.playlists = Load::Loading;
                self.job(async move { b.playlists(uid).await }, |s, _, res| s.library.playlists = res.map_or_else(|e| Load::Failed(e.to_string()), Load::Ready));
            }
            LibTab::Albums if matches!(self.library.albums, Load::Idle | Load::Failed(_)) => {
                self.library.albums = Load::Loading;
                self.job(async move { b.liked_albums(uid).await }, |s, _, res| s.library.albums = res.map_or_else(|e| Load::Failed(e.to_string()), Load::Ready));
            }
            LibTab::Artists if matches!(self.library.artists, Load::Idle | Load::Failed(_)) => {
                self.library.artists = Load::Loading;
                self.job(async move { b.liked_artists(uid).await }, |s, _, res| s.library.artists = res.map_or_else(|e| Load::Failed(e.to_string()), Load::Ready));
            }
            _ => {}
        }
    }

    pub(crate) fn begin_detail(&mut self, page: Page) -> u64 {
        let is_artist = matches!(page, Page::Artist(_));
        self.detail = Detail { page: Some(page), generation: self.detail.generation + 1, state: Load::Loading, is_artist, ..Detail::default() };
        self.detail.generation
    }

    pub(crate) fn show_album(&mut self, a: Album) {
        self.detail.like_key = Some(format!("album:{}", a.id));
        self.detail.link = Some(format!("https://music.yandex.ru/album/{}", a.id));
        self.detail.artist = a.artists.first().map(|r| r.id.clone()).filter(|id| !id.is_empty());
        self.detail.artists = a.artists.clone();
        self.detail.year = a.year;
        self.detail.title = a.title.clone();
        self.detail.subtitle = a.artists_line();
        self.detail.cover = a.cover_template().map(str::to_owned);
        self.detail.tracks = a.tracks().cloned().collect();
        let mut facts = Vec::new();
        if let Some(y) = a.year {
            facts.push((S::Year, y.to_string()));
        }
        if let Some(g) = a.genre.as_deref().filter(|g| !g.is_empty()) {
            facts.push((S::Genre, crate::genre::name(g, self.lang)));
        }
        if let Some(date) = a.release_date.as_deref().filter(|d| !d.is_empty()) {
            facts.push((S::ReleaseDate, crate::i18n::release_date(date, self.lang)));
        }
        if let Some(version) = a.version.as_deref().filter(|v| !v.is_empty()) {
            facts.push((S::Version, version.to_owned()));
        }
        facts.extend(self.length_facts());
        self.detail.facts = facts;
        self.detail.state = Load::Ready(());
    }

    pub(crate) fn show_playlist(&mut self, p: Playlist) {
        let own = self.account.as_ref().is_some_and(|a| a.uid == p.uid);
        self.detail.like_key = (!own).then(|| format!("playlist:{}:{}", p.uid, p.kind));
        let owner = p.owner.as_ref().map(|o| o.login.clone()).filter(|l| !l.is_empty()).unwrap_or_else(|| p.uid.to_string());
        self.detail.link = Some(format!("https://music.yandex.ru/users/{owner}/playlists/{}", p.kind));
        self.detail.title = p.title.clone();
        self.detail.subtitle = p.owner.as_ref().and_then(|o| o.name.clone().or(Some(o.login.clone()))).unwrap_or_default();
        self.detail.cover = p.cover_template().map(str::to_owned);
        self.detail.tracks = p.resolved_tracks().cloned().collect();
        let mut facts = self.length_facts();
        if let Some(d) = p.description.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
            facts.push((S::Description, d.to_owned()));
        }
        self.detail.facts = facts;
        self.detail.state = Load::Ready(());
    }

    pub(crate) fn show_artist(&mut self, a: ArtistPage) {
        self.detail.like_key = Some(format!("artist:{}", a.artist.id));
        self.detail.link = Some(format!("https://music.yandex.ru/artist/{}", a.artist.id));
        self.detail.title = a.artist.name.clone();
        self.detail.monthly_listeners = a.artist.stats.as_ref().and_then(|s| s.last_month_listeners);
        let mut facts = Vec::new();
        if !a.artist.genres.is_empty() {
            facts.push((S::Genres, crate::genre::names(&a.artist.genres, self.lang)));
        }
        if !a.albums.is_empty() {
            facts.push((S::Albums, a.albums.len().to_string()));
        }
        if !a.similar_artists.is_empty() {
            facts.push((S::SimilarArtists, a.similar_artists.len().to_string()));
        }
        if let Some(listeners) = a.artist.stats.as_ref().and_then(|s| s.last_month_listeners) {
            let value = if listeners >= 1_000_000 { format!("{:.1}M", listeners as f64 / 1_000_000.0) } else if listeners >= 1_000 { format!("{:.1}K", listeners as f64 / 1_000.0) } else { listeners.to_string() };
            facts.push((S::MonthlyListeners, format!("{value} {}", S::MonthlyListeners.get(self.lang).to_lowercase())));
        }
        self.detail.facts = facts;
        self.detail.popular = a.popular_tracks.clone();
        self.detail.cover = a.artist.cover_template().map(str::to_owned).or_else(|| a.popular_tracks.first().and_then(|t| t.cover_template().map(str::to_owned)));
        self.detail.tracks = a.popular_tracks;
        self.detail.albums = a.albums;
        self.detail.similar = a.similar_artists;
        self.detail.state = Load::Ready(());
    }

    /// How many tracks the open page has and how long they play.
    fn length_facts(&self) -> Vec<(S, String)> {
        let t = &self.detail.tracks;
        if t.is_empty() {
            return Vec::new();
        }
        let ms: u64 = t.iter().filter_map(|t| t.duration_ms).sum();
        vec![(S::Tracks, t.len().to_string()), (S::Duration, self.lang.duration(ms / 1000))]
    }

    fn detail_failed(&mut self, e: &ym_api::Error) {
        self.detail.state = Load::Failed(e.to_string());
        self.error_toast(e);
    }

    pub fn open_album(&mut self, id: &str) {
        let generation = self.begin_detail(Page::Album(id.into()));
        let (b, id) = (self.backend.clone(), id.to_owned());
        self.job(async move { b.album(&id).await }, move |s, _, res| {
            if s.detail.generation == generation {
                match res {
                    Ok(a) => s.show_album(a),
                    Err(e) => s.detail_failed(&e),
                }
            }
        });
    }

    pub fn open_playlist(&mut self, uid: i64, kind: i64) {
        let generation = self.begin_detail(Page::Playlist(uid, kind));
        let b = self.backend.clone();
        self.job(async move { b.playlist(uid, kind).await }, move |s, _, res| {
            if s.detail.generation == generation {
                match res {
                    Ok(p) => s.show_playlist(p),
                    Err(e) => s.detail_failed(&e),
                }
            }
        });
    }

    /// Plays an album from its first track without opening its page (a card's play button).
    pub fn play_album(&mut self, id: &str, title: &str) {
        let (b, id, title) = (self.backend.clone(), id.to_owned(), title.to_owned());
        self.job(async move { b.album(&id).await }, move |s, _, res| match res {
            Ok(a) => {
                let tracks: Vec<Track> = a.tracks().cloned().collect();
                s.play_list_from(tracks, S::Album, title, Page::Album(a.id.clone()));
            }
            Err(e) => s.toast(e.to_string()),
        });
    }

    /// Plays a playlist from its first track without opening its page (a card's play button).
    pub fn play_playlist(&mut self, uid: i64, kind: i64, title: &str) {
        let (b, title) = (self.backend.clone(), title.to_owned());
        self.job(async move { b.playlist(uid, kind).await }, move |s, _, res| match res {
            Ok(p) => {
                let tracks: Vec<Track> = p.resolved_tracks().cloned().collect();
                s.play_list_from(tracks, S::Playlist, title, Page::Playlist(uid, kind));
            }
            Err(e) => s.toast(e.to_string()),
        });
    }

    fn play_list_from(&mut self, tracks: Vec<Track>, kind: S, name: String, page: Page) {
        if tracks.is_empty() {
            return;
        }
        self.play_queue(tracks, 0, Source::Manual);
        self.playing_from = Some(PlayingFrom { wave: false, kind, name: FromName::Text(name), page: Some(page) });
    }

    pub fn open_artist(&mut self, id: &str) {
        let generation = self.begin_detail(Page::Artist(id.into()));
        let (b, id) = (self.backend.clone(), id.to_owned());
        self.job(async move { b.artist(&id).await }, move |s, _, res| {
            if s.detail.generation == generation {
                match res {
                    Ok(a) => s.show_artist(a),
                    Err(e) => s.detail_failed(&e),
                }
            }
        });
    }

    // ---- search -------------------------------------------------------------------------------

    pub fn run_search(&mut self, text: String) {
        let text = text.trim().to_owned();
        self.search.generation += 1;
        let generation = self.search.generation;
        if text.is_empty() {
            self.search.results = Load::Idle;
            self.search.query.clear();
            self.search.suggestions.clear();
            return;
        }
        self.search.query = text.clone();
        self.search.results = Load::Loading;
        let b = self.backend.clone();
        self.job(async move { b.search(&text, ym_api::SearchKind::All).await }, move |s, _, res| {
            if s.search.generation != generation {
                return;
            }
            s.search.results = match res {
                Ok(r) => Load::Ready(r),
                Err(e) => Load::Failed(e.to_string()),
            };
        });
        // The kind picked stays picked for the new text.
        self.fetch_search_kind();
    }

    /// Picks a kind of result (a chip under the search field). Any kind but "All" is searched on
    /// its own, so it lists many more than the few "All" brings back.
    pub fn set_search_tab(&mut self, tab: SearchTab) {
        self.search.tab = tab;
        self.fetch_search_kind();
    }

    fn fetch_search_kind(&mut self) {
        let (tab, text) = (self.search.tab, self.search.query.clone());
        let kind = match tab {
            SearchTab::All => return,
            SearchTab::Tracks => ym_api::SearchKind::Track,
            SearchTab::Albums => ym_api::SearchKind::Album,
            SearchTab::Artists => ym_api::SearchKind::Artist,
            SearchTab::Playlists => ym_api::SearchKind::Playlist,
        };
        if text.is_empty() || self.search.more_for.as_ref() == Some(&(text.clone(), tab)) {
            return;
        }
        self.search.more_for = Some((text.clone(), tab));
        self.search.more = Load::Loading;
        let b = self.backend.clone();
        let want = (text.clone(), tab);
        self.job(async move { b.search(&text, kind).await }, move |s, _, res| {
            if s.search.more_for.as_ref() != Some(&want) {
                return;
            }
            // A kind that fails on its own still shows what "All" found of it.
            s.search.more = res.map_or_else(|e| Load::Failed(e.to_string()), Load::Ready);
        });
    }

    /// The results the page shows: the picked kind's own search once it is in, else "All".
    pub fn search_view(&self) -> Option<&SearchResult> {
        let own = self.search.tab != SearchTab::All && self.search.more_for.as_ref().is_some_and(|(q, t)| *q == self.search.query && *t == self.search.tab);
        match (&self.search.more, own) {
            (Load::Ready(r), true) => Some(r),
            _ => self.search.results.get(),
        }
    }

    /// Keeps what was searched for at the top of the history (searching it again moves it up).
    pub fn remember_search(&mut self) {
        let q = self.search.query.trim().to_owned();
        if q.is_empty() {
            return;
        }
        let h = &mut self.settings.search_history;
        h.retain(|x| !x.eq_ignore_ascii_case(&q) && x.to_lowercase() != q.to_lowercase());
        h.insert(0, q);
        h.truncate(SEARCH_HISTORY);
        self.mark_settings_dirty();
    }

    pub fn forget_search(&mut self, q: &str) {
        self.settings.search_history.retain(|x| x != q);
        self.mark_settings_dirty();
    }

    pub fn clear_search_history(&mut self) {
        self.settings.search_history.clear();
        self.mark_settings_dirty();
    }

    /// Searches for `q` again (a line of the history).
    pub fn search_again(&mut self, q: &str) {
        self.search.edit = TextEdit::new(q);
        self.search.seen = q.to_owned();
        self.run_search(q.to_owned());
        self.remember_search();
    }

    /// Called every frame: waits for a pause in typing, then searches.
    fn drive_search(&mut self) {
        let text = self.search.edit.text.clone();
        if text != self.search.seen {
            self.search.seen = text.clone();
            self.search.changed_at = self.now;
            return;
        }
        if text.trim() != self.search.query && self.now - self.search.changed_at > 0.30 && self.session != Session::LoggedOut {
            self.run_search(text);
        }
    }

    // ---- likes --------------------------------------------------------------------------------

    pub fn is_liked(&self, id: &str) -> bool {
        self.liked.contains(id)
    }

    pub fn toggle_like(&mut self, track: &Track) {
        let like = !self.liked.contains(&track.id);
        self.set_like(track, like);
    }

    pub fn set_like(&mut self, track: &Track, like: bool) {
        if like {
            self.liked.insert(track.id.clone());
            if let Load::Ready(v) = &mut self.library.tracks
                && !v.iter().any(|t| t.id == track.id)
            {
                v.insert(0, track.clone());
            }
        } else {
            self.liked.remove(&track.id);
            if let Load::Ready(v) = &mut self.library.tracks {
                v.retain(|t| t.id != track.id);
            }
        }
        let (b, uid, t) = (self.backend.clone(), self.account.as_ref().map_or(0, |a| a.uid), track.clone());
        let id = track.id.clone();
        self.job(async move { b.set_like(uid, &t, like).await }, move |s, _, res| {
            if let Err(e) = res {
                // Put the heart back the way it was.
                if like {
                    s.liked.remove(&id);
                } else {
                    s.liked.insert(id);
                }
                s.error_toast(&e);
            }
        });
    }

    /// "Not this one": tells Yandex (so the wave and the recommendations learn from it), drops the
    /// like if there was one, and moves on to the next track when this is the one playing.
    pub fn dislike(&mut self, track: &Track) {
        if self.liked.contains(&track.id) {
            self.set_like(track, false);
        }
        let (b, uid, t) = (self.backend.clone(), self.account.as_ref().map_or(0, |a| a.uid), track.clone());
        self.job(async move { b.set_dislike(uid, &t).await }, |s, _, res| {
            if let Err(e) = res {
                s.error_toast(&e);
            }
        });
        let text = self.tr(S::Disliked);
        self.toast(text);
        if self.current().is_some_and(|c| c.id == track.id) {
            self.next();
        }
    }

    // ---- lyrics, art, palette -----------------------------------------------------------------

    pub fn current(&self) -> Option<&Track> {
        self.player.index.and_then(|i| self.player.queue.get(i))
    }

    /// The URL a cover of `size` px is loaded from (also the cache key).
    pub fn cover_key(template: Option<&str>, size: u32) -> Option<String> {
        template.map(|t| ym_api::cover::url(t, size))
    }

    pub fn load_lyrics_for_current(&mut self) {
        let Some(track) = self.current().cloned() else { return };
        if self.lyrics.for_track.as_deref() == Some(track.id.as_str()) {
            return;
        }
        self.lyrics = LyricsUi { for_track: Some(track.id.clone()), lyrics: Load::Loading, active: None };
        let b = self.backend.clone();
        let id = track.id.clone();
        let lookup_id = id.clone();
        let downloads = self.download_store.clone();
        self.job(async move {
            if let Some(store) = &downloads
                && let Some(item) = store.get(&lookup_id)
            {
                return item.lyrics;
            }
            b.lyrics(&track).await.unwrap_or_default()
        }, move |s, _, l| {
            if s.lyrics.for_track.as_deref() == Some(id.as_str()) {
                s.lyrics.lyrics = Load::Ready(l);
                s.scroll_lyrics = true;
            }
        });
    }

    /// Starts every load the view has asked for (covers), and applies palettes.
    fn drive_art(&mut self) -> bool {
        for key in self.art.take_wanted() {
            if self.headless {
                // Snapshots must not depend on timing: paint demo covers right here.
                if let Some((hue, style, seed, size)) = crate::demo::parse_cover_url(&key) {
                    let rgba = crate::demo::cover_rgba(hue, style, seed, size);
                    self.art.finish(&key, Some(crate::art::Decoded { width: size, height: size, rgba }));
                }
                continue;
            }
            let (b, art, k, disk) = (self.backend.clone(), self.art.clone(), key.clone(), self.cache.clone());
            let max = ym_api::cover::SIZES.iter().copied().find(|s| key.contains(&format!("{s}x{s}"))).unwrap_or(600);
            net::spawn(async move {
                let cached = disk.as_ref().and_then(|disk| disk.get(&k).and_then(|bytes| crate::art::decode(&bytes, max)));
                let img = match cached {
                    Some(img) => Some(img),
                    None => b.image(&k, max).await,
                };
                if let Some(img) = img {
                    if let Some(disk) = disk
                        && let Some(bytes) = crate::art::encode_png(&img)
                    {
                        let _ = disk.put(&k, &bytes);
                    }
                    art.finish(&k, Some(img));
                } else {
                    art.finish(&k, None);
                }
            });
        }
        // A picture that landed is shown by building the view again (it fades in).
        let landed = self.art.landed();
        let fresh = landed != self.art_landed;
        self.art_landed = landed;
        let Some(track) = self.current() else { return fresh };
        let Some(key) = State::cover_key(track.cover_template(), 400) else { return fresh };
        if self.palette_key.as_deref() != Some(key.as_str()) {
            // Ask for the new track's large cover here: the views only show the copy the colours
            // already come from (the old one) until this one is in, so none of them asks for it,
            // and with the player closed nothing else does either.
            let _ = self.art.pixels(&key);
        }
        if self.palette_key.as_deref() != Some(key.as_str())
            && let Some(p) = self.art.palette(&key)
        {
            self.palette = p;
            self.cover_thumb = self.art.thumb(&key);
            self.palette_key = Some(key);
            return true;
        }
        fresh
    }

    // ---- per-frame ----------------------------------------------------------------------------

    /// Called at the start of every frame. Returns true when something visible in the view changed.
    /// Whether the butterflies fly: on Home, with the player closed, effects on and motion allowed.
    pub fn bugs_alive(&self) -> bool {
        self.page == Page::Home
            && !self.np_open
            && self.session != Session::LoggedOut
            && !self.settings.reduce_motion
            && self.settings.effects != crate::store::Effects::Off
    }

    /// Whether the profile menu is on screen at all (open, or still closing).
    pub fn menu_visible(&self) -> bool {
        self.menu_open || self.menu_t > 0.001 || self.menu_v != 0.0
    }

    /// Steps the menu's openness toward where it is heading, on a critically damped spring that
    /// keeps its speed when the target flips: pressing again halfway reverses it from wherever it
    /// is, with no jump and no waiting for it to finish. Reduce motion lands it at once.
    pub(crate) fn tick_menu(&mut self, dt: f32) -> bool {
        let reduce = self.settings.reduce_motion;
        let mut pop = Pop { t: self.menu_t, v: self.menu_v };
        let moved = pop.step(self.menu_open, dt, reduce);
        (self.menu_t, self.menu_v) = (pop.t, pop.v);
        let np_target = self.np_open && self.current().is_some();
        let veil_target = np_target && self.np.t >= 1.0;
        let moved_track = self.tmenu.step(self.tmenu_open, dt, reduce)
            | self.vol.step(self.vol_open, dt, reduce)
            | self.np.glide(np_target, dt, reduce)
            | self.np_veil.step(veil_target, dt, reduce)
            | self.pop.step(self.popover_open, dt, reduce)
            | self.light.glide(self.lightbox_open, dt, reduce)
            | self.sheet_pop.glide(self.sheet_open, dt, reduce);
        if !self.sheet_open && self.sheet_pop.t == 0.0 && self.sheet_pop.v == 0.0 && self.sheet.is_some() {
            self.sheet = None;
            return true;
        }
        if !self.popover_open && self.pop.t == 0.0 && self.pop.v == 0.0 && self.popover.is_some() {
            self.popover = None;
            return true;
        }
        if !self.lightbox_open && self.light.t == 0.0 && self.light.v == 0.0 && self.lightbox.is_some() {
            self.lightbox = None;
            return true;
        }
        // A closed menu lets go of its track once it has finished closing.
        if !self.tmenu_open && self.tmenu.t == 0.0 && self.tmenu.v == 0.0 && self.track_menu.is_some() {
            self.track_menu = None;
            return true;
        }
        moved || moved_track
    }

    /// Whether either menu is still moving, so the next frame has to be drawn.
    pub fn menu_moving(&self) -> bool {
        Pop { t: self.menu_t, v: self.menu_v }.moving(self.menu_open)
            || self.tmenu.moving(self.tmenu_open)
            || self.vol.moving(self.vol_open)
            || self.np.moving(self.np_open && self.current().is_some())
            || self.np_veil.moving(self.np_open && self.current().is_some() && self.np.t >= 1.0)
            || self.pop.moving(self.popover_open)
            || self.light.moving(self.lightbox_open)
            || self.sheet_pop.moving(self.sheet_open)
    }

    /// Whether the volume card is on screen at all (open, or still closing).
    pub fn vol_visible(&self) -> bool {
        self.vol_open || self.vol.t > 0.001 || self.vol.v != 0.0
    }

    /// Opens the menu of `track`, anchored to a point or to the element with a key.
    /// `queue_index` is the track's place when the row belongs to the queue (it adds "remove").
    pub fn open_track_menu(&mut self, track: Track, queue_index: Option<usize>, anchor: PopupAnchor) {
        self.menu_open = false;
        self.vol_open = false;
        self.vol_close_at = None;
        self.track_menu = Some(TrackMenu { track, queue_index, anchor, player: false });
        self.tmenu_open = true;
    }

    /// Opens the menu of the playing track from the player itself.
    pub fn open_player_menu(&mut self, anchor: PopupAnchor) {
        let Some(t) = self.current().cloned() else { return };
        self.open_track_menu(t, None, anchor);
        if let Some(m) = &mut self.track_menu {
            m.player = true;
        }
    }

    /// Slides the side sheet in with the details of `what`.
    pub fn open_sheet(&mut self, what: Sheet) {
        self.close_track_menu();
        self.close_popover();
        self.menu_open = false;
        self.vol_open = false;
        self.vol_close_at = None;
        self.sheet = Some(what);
        self.sheet_open = true;
    }

    pub fn sheet_visible(&self) -> bool {
        self.sheet.is_some() && (self.sheet_open || self.sheet_pop.t > 0.001 || self.sheet_pop.v != 0.0)
    }

    pub fn close_track_menu(&mut self) {
        self.tmenu_open = false;
    }

    /// Opens a popover out of the element keyed `anchor` (pressing its button again closes it).
    pub fn open_popover(&mut self, p: Popover) {
        let again = self.popover_open && self.popover.as_ref().is_some_and(|q| q.anchor() == p.anchor());
        self.menu_open = false;
        self.tmenu_open = false;
        self.vol_open = false;
        self.vol_close_at = None;
        if again {
            self.popover_open = false;
            return;
        }
        if !self.popover_open {
            self.pop = Pop::default();
        }
        self.popover = Some(p);
        self.popover_open = true;
    }

    pub fn close_popover(&mut self) {
        self.popover_open = false;
    }

    pub fn popover_visible(&self) -> bool {
        self.popover.is_some() && (self.popover_open || self.pop.t > 0.001 || self.pop.v != 0.0)
    }

    /// Opens a cover large over the window, growing out of the element keyed `from`.
    pub fn open_lightbox(&mut self, template: String, from: &str) {
        self.close_popover();
        self.lightbox = Some((template, from.to_owned()));
        self.lightbox_open = true;
    }

    pub fn lightbox_visible(&self) -> bool {
        self.lightbox.is_some() && (self.lightbox_open || self.light.t > 0.001 || self.light.v != 0.0)
    }

    /// Whether this track's lyrics have been asked for and there are none to show (no words, an
    /// instrumental, or none found): the lyrics button is off and the panel stays shut.
    pub fn lyrics_missing(&self) -> bool {
        if self.current().is_none() {
            return false;
        }
        match &self.lyrics.lyrics {
            Load::Failed(_) => true,
            Load::Ready(l) => l.instrumental || !l.has_words(),
            _ => false,
        }
    }

    /// The side panel the player shows: the one asked for, unless that is lyrics there are none of.
    /// While the next track's lyrics are still on their way the panel stays as it was, so it
    /// neither starts to open for lyrics that turn out not to be there nor closes for a moment
    /// between two tracks that both have them.
    pub fn np_panel(&self) -> NpTab {
        if self.np_tab != NpTab::Lyrics {
            return self.np_tab;
        }
        let there = match &self.lyrics.lyrics {
            Load::Ready(l) => {
                let v = !l.instrumental && l.has_words();
                self.ly_panel.set(v);
                v
            }
            Load::Failed(_) => {
                self.ly_panel.set(false);
                false
            }
            _ => self.ly_panel.get(),
        };
        if there { NpTab::Lyrics } else { NpTab::Closed }
    }

    /// Asks Yandex ID for the account's picture.
    pub fn fetch_avatar(&mut self) {
        let b = self.backend.clone();
        self.job(async move { b.avatar().await }, |s, _, url| s.avatar = url);
    }

    /// The artist page's list switches between the popular tracks and every track (loaded the
    /// first time it is asked for).
    pub fn show_all_artist_tracks(&mut self, all: bool) {
        let Some(Page::Artist(id)) = self.detail.page.clone() else { return };
        self.detail.showing_all = all;
        if !all {
            self.detail.tracks = self.detail.popular.clone();
            return;
        }
        if let Load::Ready(v) = &self.detail.all_tracks {
            self.detail.tracks = v.clone();
            return;
        }
        self.detail.all_tracks = Load::Loading;
        let (b, generation) = (self.backend.clone(), self.detail.generation);
        self.job(async move { b.artist_tracks(&id).await }, move |s, _, res| {
            if s.detail.generation != generation {
                return;
            }
            match res {
                Ok(v) => {
                    if s.detail.showing_all {
                        s.detail.tracks = v.clone();
                    }
                    s.detail.all_tracks = Load::Ready(v);
                }
                Err(e) => {
                    s.detail.all_tracks = Load::Failed(e.to_string());
                    s.detail.showing_all = false;
                    s.error_toast(&e);
                }
            }
        });
    }

    pub fn track_menu_visible(&self) -> bool {
        self.track_menu.is_some() && (self.tmenu_open || self.tmenu.t > 0.001 || self.tmenu.v != 0.0)
    }

    pub fn tick(&mut self, t: f64) -> bool {
        self.prev_now = self.now;
        self.now = t;
        if self.settings.fkcensor && t - self.fkcensor_checked >= 600.0
            && let Backend::Live(client) = &self.backend
        {
            self.refresh_fkcensor(client.clone());
        }
        if self.settings.theme == ThemeMode::System && t - self.sys_checked > 2.0 {
            self.sys_checked = t;
            self.sys_light = theme::system_light();
        }
        let dt = (self.now - self.prev_now).clamp(0.0, 0.25) as f32;
        let mut changed = false;
        if self.vol_close_at.is_some_and(|at| self.now >= at) {
            self.vol_close_at = None;
            self.vol_open = false;
            changed = true;
        }
        changed |= self.tick_menu(dt);
        changed |= self.tick_player(dt);
        changed |= self.drive_art();
        if self.autoplay
            && let Load::Ready(tracks) = &self.library.tracks
            && !tracks.is_empty()
        {
            self.autoplay = false;
            let queue = tracks.clone();
            self.play_queue(queue, 0, Source::Manual);
            // `GLINT_AUTOPLAY=np` also opens the player (for measuring that screen).
            if std::env::var("GLINT_AUTOPLAY").is_ok_and(|v| v == "np") {
                self.set_np(true);
            }
        }
        self.sync_desktop();
        if self.page == Page::Search {
            self.drive_search();
        }
        let before = self.toasts.len();
        self.toasts.retain(|x| self.now - x.born < 3.4);
        changed |= before != self.toasts.len();
        if self.download_activity.is_some_and(|a| a.active == 0 && self.now - a.changed_at >= 3.8) {
            self.download_activity = None;
            changed = true;
        }
        let burst = self.now - self.like_burst < 1.3;
        if burst != self.burst_on {
            self.burst_on = burst;
            changed = true;
        }
        if let Some(at) = self.settings_dirty_at
            && self.now - at > 0.8
        {
            self.save_settings_now();
        }
        changed
    }
}
