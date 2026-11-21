//! Application state and state transitions. The view reads [`State`]; handlers mutate it on the
//! UI thread; background jobs apply their results on the UI thread through [`State::job`].

mod content;
mod media;
mod model;
mod motion;
mod nav;
mod overlay;
mod runtime;
mod search;
mod session;
mod types;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use rune::{AppHandle, Live, LiveText, TextEdit};
use ym_api::{Account, Shelf, Track};
use ym_audio::{DiskCache, EqSettings};

use crate::art::ArtCache;
use crate::desktop::Desktop;
use crate::i18n::Lang;
use crate::net::Backend;
use crate::palette::Palette;
use crate::store::{DownloadStore, Settings, Store};

pub use model::{DownloadActivity, FromName, Library, Lives, LyricsUi, Player, PlayingFrom, Popover, Sheet, Source, Station, Toast, TrackMenu, WaveChoice};
pub use motion::{LYRIC_LEAD_MS, PAGE_KEY, PAGE_SCROLL, Pop, WheelGlide, page_scroll};
pub use types::{
    Detail, DiscKind, DiscSort, DownloadFail, DownloadState, LibTab, Load, LoginUi, NpTab, Page, PinnedItem, SearchTab, SearchUi, Session, TrackSort, release_kind_name,
};

/// The app's home page, sent to lrc.red as the contact required by its API terms.
pub const APP_URL: &str = "https://github.com/orphytype/glint";

/// Maximum entries kept in search history.
pub const SEARCH_HISTORY: usize = 12;

/// Local clock hour (fixed to evening in headless runs so scene snapshots repeat).
pub fn local_hour() -> u32 {
    if is_headless() {
        return 20;
    }
    #[cfg(windows)]
    {
        // SAFETY: SYSTEMTIME is plain data; GetLocalTime initializes all fields in place.
        let mut t: windows_sys::Win32::Foundation::SYSTEMTIME = unsafe { std::mem::zeroed() };
        // SAFETY: `t` is a valid mutable pointer to a SYSTEMTIME struct.
        unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut t) };
        u32::from(t.wHour)
    }
    #[cfg(unix)]
    {
        // SAFETY: `time` with a null pointer only returns the time; `localtime_r` writes only `tm`.
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
    static HEADLESS: OnceLock<bool> = OnceLock::new();
    *HEADLESS.get_or_init(|| {
        std::env::args().any(|a| matches!(a.as_str(), "--snap" | "--scene" | "--tree" | "--script" | "--frames" | "--video" | "--trace" | "--strip" | "--list-scenes"))
    })
}

pub struct State {
    // plumbing
    pub app: Option<AppHandle<State>>,
    pub store: Store,
    pub art: ArtCache,
    /// Bounded cache for album art and lightweight metadata. Downloaded audio lives in `download_store`.
    pub cache: Option<Arc<DiskCache>>,
    pub download_store: Option<Arc<DownloadStore>>,
    pub download_confirm: Option<Vec<Track>>,
    pub delete_download_confirm: Option<Vec<Track>>,
    pub name_dialog: Option<crate::playlists::NamePurpose>,
    pub name_edit: TextEdit,
    pub delete_playlist_confirm: Option<(i64, i64)>,
    pub downloads: HashMap<String, DownloadState>,
    /// Tracks whose uncensored version (FckCensor) was loaded.
    pub clean_tracks: HashSet<String>,
    pub download_titles: HashMap<String, String>,
    /// Downloaded track metadata kept in memory so opening the manager avoids disk reads.
    pub downloaded_tracks: HashMap<String, Track>,
    pub download_errors: HashMap<String, DownloadFail>,
    /// Measured loudness envelope (`fx::WAVE_N` values, 0..1) per track id for the seek line.
    pub waveforms: HashMap<String, Arc<[f32]>>,
    pub waveform_jobs: HashSet<String>,
    /// Downloaded track IDs in newest-first order.
    pub download_order: Vec<String>,
    pub download_activity: Option<DownloadActivity>,
    pub backend: Backend,
    pub headless: bool,
    pub now: f64,
    pub prev_now: f64,
    pub vol_w: Cell<f32>,
    pub vol_drawn: Cell<f32>,
    pub win_w: Cell<f32>,
    pub win_size: Cell<[f32; 2]>,
    pub resized_at: Cell<f64>,
    pub lives: Lives,
    pub clock_secs: Cell<u64>,
    pub remaining_secs: Cell<u64>,
    pub clock_text: RefCell<String>,
    pub remaining_text: RefCell<String>,
    /// Cached storage sizes `(art, downloads, total, measured_at)` for Settings.
    pub disk_usage: Cell<(u64, u64, u64, f64)>,
    pub disk_usage_busy: Cell<bool>,
    pub card_cover: RefCell<Option<String>>,
    // settings & session
    pub settings: Settings,
    pub lang: Lang,
    pub session: Session,
    pub account: Option<Account>,
    pub login: LoginUi,
    // navigation
    pub page: Page,
    pub pinned_items: Vec<PinnedItem>,
    pub history: Vec<Page>,
    pub listening_history: Vec<Track>,
    pub listen: Option<crate::player::Listen>,
    pub forward: Vec<Page>,
    pub playing_from: Option<PlayingFrom>,
    pub wave_label: Option<(crate::i18n::S, FromName, Option<Page>)>,
    // content
    pub home: Load<Vec<Shelf>>,
    pub liked: HashSet<String>,
    pub liked_items: HashSet<String>,
    pub likes_loaded: bool,
    pub wave_cover: RefCell<(bool, Option<String>)>,
    pub swarm: crate::bugs::Swarm,
    pub bug_geom: RefCell<crate::bugs::Geom>,
    pub library: Library,
    pub search: SearchUi,
    pub detail: Detail,
    pub wave_choice: String,
    // playback
    pub player: Player,
    pub lyrics: LyricsUi,
    pub ly_e: Vec<f32>,
    pub ly_panel: Cell<bool>,
    pub np_open: bool,
    pub np_tab: NpTab,
    pub menu_open: bool,
    pub menu: Pop,
    pub track_menu: Option<TrackMenu>,
    pub tmenu_open: bool,
    pub tmenu: Pop,
    pub sheet: Option<Sheet>,
    pub sheet_open: bool,
    pub sheet_pop: Pop,
    pub sheet_seq: u64,
    pub id_peek: bool,
    pub backdrop: Option<Arc<crate::backdrop::Image>>,
    pub backdrop_rev: Option<u32>,
    pub sheet_drag: Pop,
    pub sheet_dragging: bool,
    pub sheet_drag_vel: f32,
    pub sheet_drag_mark: Option<(std::time::Instant, f32)>,
    pub sheet_credits: (Option<String>, Load<Vec<ym_api::Credit>>),
    pub settings_open: bool,
    pub settings_tab: crate::ui::settings::SetTab,
    pub rebind: Option<crate::keys::Act>,
    pub title_hover: Option<(&'static str, f64)>,
    pub marquee_hot: Option<String>,
    pub vol_open: bool,
    pub vol_button_hover: bool,
    pub vol_card_hover: bool,
    pub over_bar: bool,
    pub over_titlebar: bool,
    pub vol_close_at: Option<f64>,
    pub vol: Pop,
    pub popover: Option<Popover>,
    pub popover_open: bool,
    pub pop: Pop,
    pub avatar: Option<String>,
    pub home_seed: u64,
    pub garden_hover: Option<&'static str>,
    pub wave_hover: Option<String>,
    pub glide: WheelGlide,
    pub scroll_memory: HashMap<String, f32>,
    pub scroll_restore: Option<(String, f32, f64)>,
    pub np: Pop,
    pub np_veil: Pop,
    pub art_landed: u64,
    pub ui_animating: bool,
    pub spectrum: [f32; 32],
    pub drive: ym_audio::Drive,
    pub tone: ym_audio::Tone,
    pub level: f32,
    pub palette: Palette,
    pub palette_key: Option<String>,
    /// Raw cover template matching `palette_key`, so per-frame checks avoid building a URL.
    pub palette_tpl: Option<String>,
    pub cover_thumb: Option<Arc<crate::art::Thumb>>,
    pub update: crate::update::Ui,
    pub update_method: glint_update::Method,
    pub fkcensor: crate::fkcensor::FkCensor,
    pub fkcensor_checked: f64,
    pub toasts: Vec<Toast>,
    pub toast_seq: u64,
    pub settings_dirty_at: Option<f64>,
    pub playback_dirty: bool,
    pub playback_saved_at: f64,
    pub scroll_lyrics: bool,
    pub scrubbing: bool,
    pub like_burst: f64,
    pub burst_track: String,
    pub burst_on: bool,
    pub focus_search: bool,
    pub lyrics_showing: bool,
    pub queue_placed: Option<usize>,
    pub desktop: Option<Desktop>,
    pub autoplay: bool,
    pub launch_play: bool,
    pub launch: Option<crate::ui::splash::Launch>,
    pub sys_light: bool,
    pub sys_checked: f64,
    pub lrc_usable: bool,
    pub fx_pulse: Live<[f32; 2]>,
    pub fx_phase: bool,
    pub fx_next: f64,
    pub bugs_at: f64,
    pub lyrics_gen: u64,
    pub boot_gen: u64,
    pub boot_playback_done: bool,
    pub downloads_gen: u64,
    pub deleted_downloads: HashSet<String>,
}

impl State {
    pub fn new(app: Option<AppHandle<State>>, store: Store, settings: Settings, headless: bool) -> State {
        let lang = settings.lang.as_deref().and_then(Lang::from_code).unwrap_or_else(Lang::system);
        let eq = EqSettings { enabled: settings.eq_enabled, preamp_db: settings.eq_preamp, gains_db: settings.eq_gains };
        const COVER_DISK_MAX: u64 = 128 << 20;
        let cache = (!headless).then(|| store.cache_dir()).flatten().and_then(|d| DiskCache::open(d, COVER_DISK_MAX).ok()).map(Arc::new);
        let download_store = (!headless).then(|| store.downloads_dir()).flatten().and_then(|d| DownloadStore::open(d).ok()).map(Arc::new);
        State {
            app,
            art: ArtCache::new(),
            cache,
            download_store,
            download_confirm: None,
            delete_download_confirm: None,
            name_dialog: None,
            name_edit: TextEdit::new(""),
            delete_playlist_confirm: None,
            downloads: HashMap::new(),
            clean_tracks: HashSet::new(),
            download_titles: HashMap::new(),
            downloaded_tracks: HashMap::new(),
            download_errors: HashMap::new(),
            waveforms: HashMap::new(),
            waveform_jobs: HashSet::new(),
            download_order: Vec::new(),
            download_activity: None,
            backend: Backend::None,
            headless,
            now: 0.0,
            prev_now: 0.0,
            vol_w: Cell::new(96.0),
            vol_drawn: Cell::new(-1.0),
            win_w: Cell::new(1280.0),
            win_size: Cell::new([0.0, 0.0]),
            resized_at: Cell::new(f64::NEG_INFINITY),
            lives: Lives { clock: LiveText::new("0:00"), remaining: LiveText::new("-0:00"), volume: Live::new([0.0, 0.0, 0.0, 4.0]) },
            clock_secs: Cell::new(u64::MAX),
            remaining_secs: Cell::new(u64::MAX),
            clock_text: RefCell::new(String::new()),
            remaining_text: RefCell::new(String::new()),
            disk_usage: Cell::new((0, 0, 0, f64::NEG_INFINITY)),
            disk_usage_busy: Cell::new(false),
            card_cover: RefCell::new(None),
            lang,
            session: Session::LoggedOut,
            account: None,
            login: LoginUi::default(),
            page: Page::Home,
            pinned_items: settings.pinned.iter().filter_map(PinnedItem::from_setting).collect(),
            history: Vec::new(),
            listening_history: Vec::new(),
            listen: None,
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
                wave_gen: 0,
                sim_started: false,
                eq,
                resume_ms: None,
                seek_hold: None,
            },
            lyrics: LyricsUi::default(),
            ly_e: Vec::new(),
            np_open: false,
            np_tab: NpTab::Closed,
            menu_open: false,
            menu: Pop::default(),
            track_menu: None,
            tmenu_open: false,
            tmenu: Pop::default(),
            sheet: None,
            sheet_open: false,
            sheet_seq: 0,
            id_peek: false,
            backdrop: None,
            backdrop_rev: None,
            sheet_drag: Pop::default(),
            sheet_dragging: false,
            sheet_drag_vel: 0.0,
            sheet_drag_mark: None,
            sheet_credits: (None, Load::Idle),
            sheet_pop: Pop::default(),
            settings_open: false,
            settings_tab: crate::ui::settings::SetTab::default(),
            rebind: None,
            title_hover: None,
            marquee_hot: None,
            vol_open: false,
            vol_button_hover: false,
            vol_card_hover: false,
            over_bar: false,
            over_titlebar: false,
            vol_close_at: None,
            vol: Pop::default(),
            popover: None,
            popover_open: false,
            pop: Pop::default(),
            avatar: None,
            home_seed: if headless { 7 } else { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(7, |d| d.as_nanos() as u64) },
            swarm: crate::bugs::Swarm::new(0),
            bug_geom: RefCell::new(crate::bugs::Geom::default()),
            glide: WheelGlide::default(),
            scroll_memory: HashMap::new(),
            scroll_restore: None,
            garden_hover: None,
            wave_hover: None,
            np: Pop::default(),
            np_veil: Pop::default(),
            art_landed: 0,
            ui_animating: false,
            spectrum: [0.0; 32],
            drive: ym_audio::Drive::default(),
            tone: ym_audio::Tone::default(),
            level: 0.0,
            palette: Palette::idle(),
            palette_key: None,
            palette_tpl: None,
            cover_thumb: None,
            liked_items: HashSet::new(),
            likes_loaded: false,
            wave_cover: RefCell::new((false, None)),
            update: crate::update::Ui::default(),
            update_method: crate::update::method(),
            fkcensor: crate::fkcensor::FkCensor::default(),
            fkcensor_checked: -600.0,
            toasts: Vec::new(),
            toast_seq: 0,
            settings_dirty_at: None,
            playback_dirty: false,
            playback_saved_at: 0.0,
            scroll_lyrics: false,
            scrubbing: false,
            like_burst: -100.0,
            burst_track: String::new(),
            burst_on: false,
            focus_search: false,
            lyrics_showing: false,
            queue_placed: None,
            ly_panel: Cell::new(false),
            desktop: None,
            autoplay: !headless && std::env::var_os("GLINT_AUTOPLAY").is_some(),
            launch_play: false,
            launch: None,
            sys_light: false,
            sys_checked: -100.0,
            lrc_usable: false,
            fx_pulse: Live::new([0.0, 0.0]),
            fx_phase: false,
            fx_next: 0.0,
            bugs_at: 0.0,
            lyrics_gen: 0,
            boot_gen: 0,
            boot_playback_done: headless,
            downloads_gen: 0,
            deleted_downloads: HashSet::new(),
            settings,
            store,
        }
    }
}
