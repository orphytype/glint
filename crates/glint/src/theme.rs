//! The look: black and white, with one accent used in very small amounts. Two themes share one
//! set of tokens; `light()` says which is showing, and every colour below follows it. Colours
//! are authored as sRGB and converted once to RUNE's linear `Color`.
//!
//! Type: Bahnschrift (a DIN-style variable grotesk that ships with Windows 10 and 11, and falls
//! back to the bundled Inter elsewhere) in two weights, 300 for text and large titles and 500
//! for emphasis; IBM Plex Mono (bundled with RUNE) for labels and numbers; Sitka for the rare
//! italic aside.

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
        let mut size = std::mem::size_of::<u32>() as u32;
        // SAFETY: both names are NUL-terminated and `data` is a DWORD-sized out buffer.
        let rc = unsafe { RegGetValueW(HKEY_CURRENT_USER, sub.as_ptr(), val.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), (&mut data as *mut u32).cast(), &mut size) };
        rc == 0 && data != 0
    }
    #[cfg(target_os = "linux")]
    {
        portal::light()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        false
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

const DARK_CANVAS: u32 = 0x0a0a0a;
const DARK_INK: u32 = 0xeeeeec;
const LIGHT_CANVAS: u32 = 0xf2f1ee;
const LIGHT_INK: u32 = 0x111111;

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
    rgba(ink_hex(), 0.64)
}
/// Tertiary: labels, timestamps, placeholders.
pub fn text3() -> Color {
    rgba(ink_hex(), 0.40)
}
/// The ink at strength `a`: the one tint behind buttons, rows and cards.
pub fn veil(a: f32) -> Color {
    rgba(ink_hex(), a)
}
pub fn hairline() -> Color {
    veil(0.10)
}
/// The card of a menu: flat, a step off the page, with the hairline for its edge.
pub fn pop() -> Color {
    rgb(if light() { 0xfcfbf9 } else { 0x1b1b1a })
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
    /// Text or an icon on a solid accent.
    #[allow(dead_code)]
    pub on_accent: Color,
}

impl Pal {
    pub fn new(accent_srgb: [f32; 3]) -> Pal {
        let accent = srgb(accent_srgb);
        let luma = 0.2126 * accent_srgb[0] + 0.7152 * accent_srgb[1] + 0.0722 * accent_srgb[2];
        Pal { accent, on_accent: if luma > 0.55 { rgb(0x0a0a0a) } else { rgb(0xffffff) } }
    }
}

// ---- type --------------------------------------------------------------------------------------

pub const SANS: Family = Family::Named("Bahnschrift, Segoe UI");
/// Big titles: the condensed cut of the same family, tall and tight like a poster.
pub const DISPLAY: Family = Family::Named("Bahnschrift Condensed, Bahnschrift, Segoe UI");
pub const SERIF: Family = Family::Named("Sitka Text, Georgia");
pub const MONO_FAMILY: Family = Family::Mono;

// Sizes, tracking and line heights are in points (a line height of 0 would mean 1.25 x the size).
// Bahnschrift's weight axis lands on only two steps here: 300 reads as regular text and 500 as a
// strong one.
/// Handwriting, for the name of My Wave on Home (each of these has Cyrillic).
pub const HAND: Family = Family::Named("Segoe Script, Segoe Print, Ink Free");

/// The handwritten title at `size` points; the script's loops reach well above and below the line.
pub fn hand(size: f32) -> TextStyle {
    TextStyle::new(size, 400.0).family(HAND).lh(size * 1.4)
}
pub const TITLE: TextStyle = TextStyle::new(48.0, 500.0).family(DISPLAY).tracking(-0.6).lh(50.0);
/// A large name inside a card (the best match, the liked tracks).
/// The name at the head of an album, a playlist or an artist.
pub const TITLE_D: TextStyle = TextStyle::new(40.0, 500.0).family(DISPLAY).tracking(-0.5).lh(44.0);
/// Who made it, under that name.
pub const BYLINE: TextStyle = TextStyle::new(18.0, 500.0).family(SANS).tracking(-0.1).lh(24.0);
pub const H0: TextStyle = TextStyle::new(30.0, 500.0).family(SANS).tracking(-0.6).lh(36.0);
pub const H1: TextStyle = TextStyle::new(24.0, 300.0).family(SANS).tracking(-0.4).lh(30.0);
pub const H2: TextStyle = TextStyle::new(18.0, 500.0).family(SANS).tracking(-0.1).lh(24.0);
pub const H3: TextStyle = TextStyle::new(15.0, 500.0).family(SANS).lh(20.0);
pub const BODY: TextStyle = TextStyle::new(14.0, 300.0).family(SANS).lh(20.0);
pub const BODY_M: TextStyle = TextStyle::new(14.0, 500.0).family(SANS).lh(20.0);
pub const SMALL: TextStyle = TextStyle::new(12.5, 300.0).family(SANS).lh(17.0);
/// The line under a title.
pub const LEAD: TextStyle = TextStyle::new(16.0, 300.0).family(SANS).tracking(-0.1).lh(24.0);
/// Section labels and tags: small mono capitals.
pub const LABEL: TextStyle = TextStyle::new(10.5, 500.0).family(MONO_FAMILY).tracking(1.3).lh(14.0);
/// Times, counts and other numbers.
pub const MONO: TextStyle = TextStyle::new(12.0, 400.0).family(MONO_FAMILY).tracking(0.2).lh(16.0).features("'tnum' 1");
/// The words on a button.
pub const BTN: TextStyle = TextStyle::new(14.5, 500.0).family(SANS).tracking(-0.05).lh(20.0);
/// A section's name over its row of cards or list.
pub const SECTION: TextStyle = TextStyle::new(21.0, 500.0).family(SANS).tracking(-0.3).lh(28.0);
/// The sung line.
pub const LYRIC: TextStyle = TextStyle::new(40.0, 500.0).family(SANS).tracking(-1.0).lh(48.0);
/// The sung line in a narrow panel, where the full size would wrap most lines.
pub const LYRIC_L: TextStyle = TextStyle::new(48.0, 500.0).family(SANS).tracking(-1.2).lh(57.0);
pub const LYRIC_S: TextStyle = TextStyle::new(30.0, 500.0).family(SANS).tracking(-0.8).lh(38.0);
/// A quiet aside in italics.
pub const ASIDE: TextStyle = TextStyle::new(17.0, 400.0).family(SERIF).italic().lh(24.0);

// ---- motion ------------------------------------------------------------------------------------

/// Press feedback: a control gives a little, never more than this.
pub const PRESS: f32 = 0.96;

/// The spring for panels, docks and toasts that arrive and leave: a little slower than the default
/// state change, so they settle instead of snapping into place.
pub const SOFT: rune::SpringPair = rune_motion::tokens::pair::REFLOW;

/// How a block of a page comes in: quick, and without a bounce.
/// The cover flying between the bar and the player: unhurried, so it glides rather than shoots.
pub const FLIGHT: rune::Spring = rune::Spring::new(0.55, 0.0);

/// The player's column sliding aside when the lyrics or the queue open, and back.
pub const SLIDE: rune::Spring = rune::Spring::new(0.5, 0.0);

pub const RISE: rune::SpringPair = rune::SpringPair::new(rune::Spring::new(0.26, 0.0), rune::Spring::new(0.18, 0.0));

/// A page that is replaced: it holds still when it enters (the compositor skips what sits in a
/// fading group, and a page carries such slots) and fades out quickly when it goes.
pub const LEAVE: rune::SpringPair = rune::SpringPair::new(rune::Spring::NONE, rune::Spring::new(0.16, 0.0));
