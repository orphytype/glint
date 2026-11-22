//! The look: black and white, with one accent used in very small amounts. Two themes share one
//! set of tokens; `light()` says which is showing, and every colour below follows it. Colours
//! are authored as sRGB and converted once to RUNE's linear `Color`.
//!
//! Type: two bundled families, the same on every machine. Commissioner, a calm grotesque, sets
//! everything that is read or pressed; Cormorant, a high-contrast display serif at weight 500,
//! sets what is looked at: page and section titles, the name in a card, the lyrics. Cormorant's
//! letters are small for their size, so its sizes run about a quarter above the sans they stand
//! beside, with lines kept short and tracking near zero.

use std::sync::atomic::{AtomicBool, Ordering};

use rune::color::{Color, srgb_decode};
use rune::{Family, TextStyle};

pub fn rgb(hex: u32) -> Color {
    Color { r: srgb_decode(((hex >> 16) & 255) as f32 / 255.0), g: srgb_decode(((hex >> 8) & 255) as f32 / 255.0), b: srgb_decode((hex & 255) as f32 / 255.0), a: 1.0 }
}

pub fn rgba(hex: u32, a: f32) -> Color {
    rgb(hex).alpha(a)
}

/// Floats in 0..1, sRGB.
pub fn srgb(c: [f32; 3]) -> Color {
    Color { r: srgb_decode(c[0].clamp(0.0, 1.0)), g: srgb_decode(c[1].clamp(0.0, 1.0)), b: srgb_decode(c[2].clamp(0.0, 1.0)), a: 1.0 }
}

pub fn hex_to_f3(hex: u32) -> [f32; 3] {
    [((hex >> 16) & 255) as f32 / 255.0, ((hex >> 8) & 255) as f32 / 255.0, (hex & 255) as f32 / 255.0]
}

// ---- the two themes ----------------------------------------------------------------------------

static LIGHT: AtomicBool = AtomicBool::new(false);

pub fn set_light(on: bool) {
    LIGHT.store(on, Ordering::Relaxed);
}

pub fn light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}

/// Whether the system is set to light mode for apps: Windows' setting, or on Linux the desktop
/// portal's `color-scheme` (dark when the desktop states no preference, and on anything else).
pub fn system_light() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
        let sub: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0".encode_utf16().collect();
        let val: Vec<u16> = "AppsUseLightTheme\0".encode_utf16().collect();
        let mut data: u32 = 0;
        let mut size = size_of::<u32>() as u32;
        // SAFETY: both names are NUL-terminated and `data` is a DWORD-sized out buffer.
        let rc = unsafe { RegGetValueW(HKEY_CURRENT_USER, sub.as_ptr(), val.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), (&mut data as *mut u32).cast(), &mut size) };
        rc == 0 && data != 0
    }
    #[cfg(target_os = "linux")]
    {
        portal::light()
    }
    #[cfg(target_os = "macos")]
    {
        mac::light()
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        false
    }
}

/// macOS's appearance: asked once at the first call, then on a thread of ours every couple of
/// seconds (asking spawns a process, which the UI thread should not wait on each time).
/// `AppleInterfaceStyle` is "Dark" in dark mode and absent in light.
#[cfg(target_os = "macos")]
mod mac {
    use std::sync::Once;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    static LIGHT: AtomicBool = AtomicBool::new(false);
    static START: Once = Once::new();

    fn ask() -> bool {
        std::process::Command::new("defaults")
            .args(["read", "-g", "AppleInterfaceStyle"])
            .output()
            .map(|o| !String::from_utf8_lossy(&o.stdout).trim().eq_ignore_ascii_case("dark"))
            .unwrap_or(false)
    }

    pub fn light() -> bool {
        START.call_once(|| {
            LIGHT.store(ask(), Ordering::Relaxed);
            let _ = std::thread::Builder::new().name("glint-appearance".into()).spawn(|| {
                loop {
                    std::thread::sleep(Duration::from_secs(2));
                    LIGHT.store(ask(), Ordering::Relaxed);
                }
            });
        });
        LIGHT.load(Ordering::Relaxed)
    }
}

/// The desktop portal's colour scheme, read on a thread of ours every couple of seconds so the UI
/// thread never waits on the session bus: it only reads the last answer.
#[cfg(target_os = "linux")]
mod portal {
    use std::sync::Once;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use zbus::{Connection, OwnedVariant, Proxy, Variant};

    static LIGHT: AtomicBool = AtomicBool::new(false);
    static START: Once = Once::new();

    pub fn light() -> bool {
        START.call_once(|| {
            let _ = std::thread::Builder::new().name("glint-appearance".into()).spawn(watch);
        });
        LIGHT.load(Ordering::Relaxed)
    }

    fn watch() {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
        runtime.block_on(async {
            let Ok(Ok(conn)) = tokio::time::timeout(Duration::from_secs(3), Connection::session()).await else { return };
            loop {
                if let Ok(Some(scheme)) = tokio::time::timeout(Duration::from_secs(2), read(&conn)).await {
                    // 0: no preference, 1: prefers dark, 2: prefers light.
                    LIGHT.store(scheme == 2, Ordering::Relaxed);
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
    }

    async fn read(conn: &Connection) -> Option<u32> {
        let proxy = Proxy::new(conn, "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop", "org.freedesktop.portal.Settings").await.ok()?;
        let key = ("org.freedesktop.appearance", "color-scheme");
        // `ReadOne` is the newer call; older portals have only `Read`, which wraps the answer once more.
        let value: OwnedVariant = match proxy.call("ReadOne", &key).await {
            Ok(v) => v,
            Err(_) => proxy.call("Read", &key).await.ok()?,
        };
        match &*value {
            Variant::U32(n) => Some(*n),
            Variant::Variant(inner) => match &**inner {
                Variant::U32(n) => Some(*n),
                _ => None,
            },
            _ => None,
        }
    }
}

const DARK_CANVAS: u32 = 0x0d0d0d;
const DARK_INK: u32 = 0xf2f2f1;
const LIGHT_CANVAS: u32 = 0xf7f7f5;
const LIGHT_INK: u32 = 0x1c1c1b;

/// The page colour, as sRGB floats (the shaders want it that way).
pub fn canvas() -> [f32; 3] {
    hex_to_f3(if light() { LIGHT_CANVAS } else { DARK_CANVAS })
}

/// The text colour, as sRGB floats.
pub fn ink() -> [f32; 3] {
    hex_to_f3(if light() { LIGHT_INK } else { DARK_INK })
}

fn ink_hex() -> u32 {
    if light() { LIGHT_INK } else { DARK_INK }
}

/// The page.
pub fn bg() -> Color {
    rgb(if light() { LIGHT_CANVAS } else { DARK_CANVAS })
}

pub fn text() -> Color {
    rgb(ink_hex())
}
/// Secondary text: what the first line is about, in a quieter voice.
pub fn text2() -> Color {
    rgba(ink_hex(), 0.62)
}
/// Tertiary: labels, timestamps, placeholders.
pub fn text3() -> Color {
    rgba(ink_hex(), 0.42)
}
/// The ink at strength `a`: the one tint behind buttons, rows and cards.
pub fn veil(a: f32) -> Color {
    rgba(ink_hex(), a)
}
pub fn hairline() -> Color {
    veil(0.08)
}
/// The card of a menu: flat, a step off the page, with the hairline for its edge.
pub fn pop() -> Color {
    rgb(if light() { 0xffffff } else { 0x171717 })
}
/// A solid that stands out from the page: the primary button's fill.
pub fn solid() -> Color {
    rgb(ink_hex())
}
/// Text on a `solid()`.
pub fn on_solid() -> Color {
    rgb(if light() { LIGHT_CANVAS } else { DARK_CANVAS })
}

// ---- the accent --------------------------------------------------------------------------------

/// Pickable accents. Index 0 follows the cover art; the last is the ink itself, for no colour at all.
pub const ACCENTS: [(&str, u32); 6] = [("adaptive", 0x3d6bff), ("blue", 0x3d6bff), ("lime", 0xc4f02e), ("red", 0xff4a35), ("amber", 0xffb02e), ("mono", 0xeeeeec)];

/// The accent as sRGB floats: `adaptive` is the cover's colour; the last entry is the text colour.
/// On the light page the colours are taken down a little so a thin line of them still reads.
pub fn accent_srgb(index: usize, adaptive: [f32; 3]) -> [f32; 3] {
    let index = index.min(ACCENTS.len() - 1);
    if index == ACCENTS.len() - 1 {
        return ink();
    }
    let c = if index == 0 { adaptive } else { hex_to_f3(ACCENTS[index].1) };
    if light() { [c[0] * 0.78, c[1] * 0.78, c[2] * 0.78] } else { c }
}

/// What a view needs to colour itself this frame.
#[derive(Clone, Copy, Debug)]
pub struct Pal {
    pub accent: Color,
}

impl Pal {
    pub fn new(accent_srgb: [f32; 3]) -> Pal {
        Pal { accent: srgb(accent_srgb) }
    }
}

// ---- type --------------------------------------------------------------------------------------

pub const SANS: Family = Family::Named("Commissioner");
pub const DISPLAY: Family = Family::Named("Cormorant");

/// Cormorant's weight wherever it is used.
const SERIF_W: f32 = 500.0;

/// The display face at `size` points: its line `lh` points, tracking a little tighter as it grows.
const fn display(size: f32, lh: f32) -> TextStyle {
    TextStyle::new(size, SERIF_W).family(DISPLAY).tracking(-size * 0.008).lh(lh)
}

/// A name set on a shape (My Wave on Home, the moods): Cormorant's italic, to read as written,
/// set a size up (its letters are small for their size).
pub fn name_on(size: f32) -> TextStyle {
    TextStyle::new((size * 1.22).round(), SERIF_W).family(DISPLAY).italic().lh((size * 1.3).round())
}

// Sizes, tracking and line heights are in points.

/// The name at the head of a release, a playlist or an artist: the sans, bold and tight, and a
/// size smaller for a narrow window.
pub const RELEASE: TextStyle = TextStyle::new(38.0, 700.0).family(SANS).tracking(-1.0).lh(44.0);
pub const RELEASE_S: TextStyle = TextStyle::new(30.0, 700.0).family(SANS).tracking(-0.7).lh(36.0);
/// The name in the details card, and at the head of the queue.
pub const SHEET_TITLE: TextStyle = TextStyle::new(24.0, 700.0).family(SANS).tracking(-0.5).lh(30.0);
/// Who made it, under that name.
pub const BYLINE_M: TextStyle = TextStyle::new(17.0, 500.0).family(SANS).lh(24.0);
/// A page's title (Home's greeting, a root page).
pub const TITLE: TextStyle = display(40.0, 44.0);
/// The name at the head of an album, a playlist or an artist.
pub const TITLE_D: TextStyle = display(46.0, 50.0);
/// The name inside a card or a dialog (the best match, a settings page).
pub const H0: TextStyle = display(30.0, 34.0);
/// A pane's or a dialog's title.
pub const H1: TextStyle = display(24.0, 28.0);
/// A section's name over its row of cards or list.
pub const SECTION: TextStyle = display(25.0, 30.0);
/// A track's name where it stands alone (the player).
pub const H2: TextStyle = TextStyle::new(16.0, 600.0).family(SANS).tracking(-0.1).lh(22.0);
/// A track's name in the bar.
pub const H3: TextStyle = TextStyle::new(14.0, 600.0).family(SANS).lh(19.0);
pub const BODY: TextStyle = TextStyle::new(14.0, 400.0).family(SANS).lh(20.0);
pub const BODY_M: TextStyle = TextStyle::new(14.0, 500.0).family(SANS).lh(20.0);
pub const SMALL: TextStyle = TextStyle::new(12.5, 400.0).family(SANS).lh(17.0);
/// The line under a title, and longer reading (an artist's story).
pub const LEAD: TextStyle = TextStyle::new(15.5, 400.0).family(SANS).lh(24.0);
/// Section labels and tags: small, medium, in sentence case.
pub const LABEL: TextStyle = TextStyle::new(12.0, 500.0).family(SANS).tracking(0.1).lh(16.0);
/// Times, counts and other numbers: the interface face with figures of one width, so a column of
/// them lines up.
pub const TIME: TextStyle = TextStyle::new(12.5, 450.0).family(SANS).lh(16.0).features("'tnum' 1");
pub const MONO: TextStyle = TextStyle::new(12.0, 450.0).family(SANS).tracking(0.2).lh(16.0).features("'tnum' 1");
/// The words on a button.
pub const BTN: TextStyle = TextStyle::new(14.0, 500.0).family(SANS).lh(20.0);
/// The sung line in the three sizes Settings offers: large, medium, small. The smallest is also
/// what a narrow panel takes for the small one.
pub const LYRIC: TextStyle = display(48.0, 55.0);
pub const LYRIC_S: TextStyle = display(42.0, 48.0);
pub const LYRIC_XS: TextStyle = display(35.0, 40.0);
/// A line in the details card's lyrics.
pub const LYRIC_CARD: TextStyle = display(26.0, 32.0);
/// `style` (one of the lyric styles above, sized for Cormorant) in the face picked for the lyrics
/// (`LyricLook::font`): 0 IBM Plex Mono, 1 Commissioner, 2 Cormorant. The mono and the sans have
/// bigger letters for their size, so they are set smaller to stand as tall, and upright.
pub fn lyric_face(style: TextStyle, font: u8) -> TextStyle {
    let mut t = style;
    match font {
        2 => {}
        1 => {
            t.family = SANS;
            t.size *= 0.80;
            t.weight = 600.0;
            t.tracking = -t.size * 0.02;
            t.line_height *= 0.92;
            t.italic = false;
        }
        _ => {
            t.family = Family::Named("IBM Plex Mono");
            t.size *= 0.70;
            t.weight = 400.0;
            t.tracking = -t.size * 0.01;
            t.line_height *= 0.94;
            t.italic = false;
        }
    }
    t
}

/// What is sung behind the line, in the display italic.
pub const ASIDE: TextStyle = TextStyle::new(21.0, SERIF_W).family(DISPLAY).lh(26.0);

// ---- motion ------------------------------------------------------------------------------------

/// How much of a frosted surface (a menu, a popover) is its own colour over the blurred page.
pub const FROST: f32 = 0.36;

/// Press feedback: a control gives a little, never more than this.
pub const PRESS: f32 = 0.96;

/// The spring for panels, docks and toasts that arrive and leave: a little slower than the default
/// state change, so they settle instead of snapping into place.
pub const SOFT: rune::SpringPair = rune_motion::tokens::pair::REFLOW;

/// How a block of a page comes in: quick, and without a bounce.
/// The cover flying between the bar and the player: matched to the player curtain's glide.
pub const FLIGHT: rune::Spring = rune::Spring::new(0.42, 0.0);

/// The player's column sliding aside when the lyrics or the queue open, and back.
pub const SLIDE: rune::Spring = rune::Spring::new(0.38, 0.0);

/// How a page (and each block of it) comes in: a calm rise that settles without a bounce, and a
/// quick fade out of the way.
pub const PAGE_IN: rune::SpringPair = rune::SpringPair::new(rune::Spring::new(0.38, 0.0), rune::Spring::new(0.16, 0.0));
/// The glow at the head of an album or artist page: slower than a block of the page, so it wipes in
/// after the page is there.
pub const GLOW: rune::SpringPair = rune::SpringPair::new(rune::Spring::new(0.7, 0.0), rune::Spring::new(0.16, 0.0));
pub const RISE: rune::SpringPair = rune::SpringPair::new(rune::Spring::new(0.26, 0.0), rune::Spring::new(0.18, 0.0));
