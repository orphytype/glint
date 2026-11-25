//! Glint's own glyphs for the player: the open player, the bar at the bottom and the mini player
//! all draw from this one family. Every glyph sits on a 24 grid inside the same 4 to 20 box, with
//! a 1.7 stroke, round caps and soft corners; the transport buttons are filled shapes with rounded
//! tips. So one glyph size per row of buttons is enough, and nothing looks borrowed. The rest of
//! the app keeps its icon set (`icons.rs`).

use rune::Icon;

macro_rules! icons {
    ($($name:ident => $body:literal;)*) => {
        $( pub static $name: Icon = Icon { name: concat!("glint-np-", stringify!($name)), view: 24.0, body: $body }; )*
    };
}

icons! {
    PLAY => r#"<path d="M8.5 6.3C8.5 5.1 9.8 4.4 10.8 5.0L18.5 10.3C19.5 11.0 19.5 13.0 18.5 13.7L10.8 19.0C9.8 19.6 8.5 18.9 8.5 17.7Z" fill="currentColor"/>"#;
    PAUSE => r#"<rect x="6.8" y="5.2" width="3.8" height="13.6" rx="1.9" fill="currentColor"/><rect x="13.4" y="5.2" width="3.8" height="13.6" rx="1.9" fill="currentColor"/>"#;
    NEXT => r#"<path d="M5 7.3C5 6.2 6.2 5.6 7.1 6.2L14.2 11.1C14.9 11.6 14.9 12.4 14.2 12.9L7.1 17.8C6.2 18.4 5 17.8 5 16.7Z" fill="currentColor"/><rect x="16.4" y="5.8" width="2.6" height="12.4" rx="1.3" fill="currentColor"/>"#;
    PREV => r#"<path d="M19 7.3C19 6.2 17.8 5.6 16.9 6.2L9.8 11.1C9.1 11.6 9.1 12.4 9.8 12.9L16.9 17.8C17.8 18.4 19 17.8 19 16.7Z" fill="currentColor"/><rect x="5" y="5.8" width="2.6" height="12.4" rx="1.3" fill="currentColor"/>"#;
    SHUFFLE => r#"<path d="M4 7.5H6.2C8.4 7.5 9.7 8.5 11 10.4L13 13.6C14.3 15.5 15.6 16.5 17.8 16.5H20M4 16.5H6.2C7.6 16.5 8.6 16 9.5 15.1M14.5 8.9C15.4 8 16.4 7.5 17.8 7.5H20" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" fill="none"/><path d="M17.8 5.3L20 7.5L17.8 9.7M17.8 14.3L20 16.5L17.8 18.7" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#;
    REPEAT => r#"<path d="M5 12V10.6C5 8.3 6.8 6.5 9.1 6.5H18.5M19 12V13.4C19 15.7 17.2 17.5 14.9 17.5H5.5" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" fill="none"/><path d="M16.3 4.3L18.5 6.5L16.3 8.7M7.7 15.3L5.5 17.5L7.7 19.7" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#;
    REPEAT_ONE => r#"<path d="M5 12V10.6C5 8.3 6.8 6.5 9.1 6.5H18.5M19 12V13.4C19 15.7 17.2 17.5 14.9 17.5H5.5" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" fill="none"/><path d="M16.3 4.3L18.5 6.5L16.3 8.7M7.7 15.3L5.5 17.5L7.7 19.7" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" fill="none"/><path d="M11 10.5L12.3 9.7V14.3" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#;
    HEART => r#"<path d="M12 19.6C12 19.6 3.8 15.2 3.8 9.4C3.8 6.9 5.7 4.9 8.1 4.9C9.7 4.9 11.1 5.8 12 7.2C12.9 5.8 14.3 4.9 15.9 4.9C18.3 4.9 20.2 6.9 20.2 9.4C20.2 15.2 12 19.6 12 19.6Z" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round" fill="none"/>"#;
    HEART_FILL => r#"<path d="M12 19.6C12 19.6 3.8 15.2 3.8 9.4C3.8 6.9 5.7 4.9 8.1 4.9C9.7 4.9 11.1 5.8 12 7.2C12.9 5.8 14.3 4.9 15.9 4.9C18.3 4.9 20.2 6.9 20.2 9.4C20.2 15.2 12 19.6 12 19.6Z" fill="currentColor" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"/>"#;
    DISLIKE => r#"<path d="M12 19.6C12 19.6 3.8 15.2 3.8 9.4C3.8 6.9 5.7 4.9 8.1 4.9C9.7 4.9 11.1 5.8 12 7.2C12.9 5.8 14.3 4.9 15.9 4.9C18.3 4.9 20.2 6.9 20.2 9.4C20.2 15.2 12 19.6 12 19.6Z" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round" fill="none"/><path d="M12 7.2L10.7 10.4L13.3 12.4L11.6 15.7" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#;
    VOLUME => r#"<path d="M3.9 9.7C3.9 9.1 4.4 8.6 5.0 8.6H7.3L10.7 5.8C11.4 5.2 12.5 5.7 12.5 6.6V17.4C12.5 18.3 11.4 18.8 10.7 18.2L7.3 15.4H5.0C4.4 15.4 3.9 14.9 3.9 14.3Z" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round" fill="none"/><path d="M15.5 9.4C16.2 10.1 16.6 11 16.6 12C16.6 13 16.2 13.9 15.5 14.6M18.1 7C19.4 8.4 20.1 10.1 20.1 12C20.1 13.9 19.4 15.6 18.1 17" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" fill="none"/>"#;
    VOLUME_LOW => r#"<path d="M5.6 9.7C5.6 9.1 6.1 8.6 6.7 8.6H9.0L12.4 5.8C13.1 5.2 14.2 5.7 14.2 6.6V17.4C14.2 18.3 13.1 18.8 12.4 18.2L9.0 15.4H6.7C6.1 15.4 5.6 14.9 5.6 14.3Z" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round" fill="none"/><path d="M17.2 9.4C17.9 10.1 18.3 11 18.3 12C18.3 13 17.9 13.9 17.2 14.6" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" fill="none"/>"#;
    VOLUME_MUTE => r#"<path d="M4 9.7C4 9.1 4.5 8.6 5.1 8.6H7.4L10.8 5.8C11.5 5.2 12.6 5.7 12.6 6.6V17.4C12.6 18.3 11.5 18.8 10.8 18.2L7.4 15.4H5.1C4.5 15.4 4 14.9 4 14.3Z" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round" fill="none"/><path d="M16 9.9L20 13.9M20 9.9L16 13.9" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" fill="none"/>"#;
    LYRICS => r#"<path d="M4 7.7C4 6.0 5.3 4.7 7 4.7H17C18.7 4.7 20 6.0 20 7.7V13.7C20 15.4 18.7 16.7 17 16.7H11L7.8 19.3C7.3 19.7 6.6 19.3 6.6 18.7V16.7C5.1 16.5 4 15.2 4 13.7Z" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round" fill="none"/><path d="M8.2 9.2H15.8M8.2 12.2H12.8" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" fill="none"/>"#;
    QUEUE => r#"<path d="M4.5 5.8H19.5M4.5 10.8H19.5M4.5 15.8H11.5" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" fill="none"/><path d="M15 13.6C15 13.0 15.7 12.6 16.2 12.9L19.6 15.1C20.1 15.4 20.1 16.2 19.6 16.5L16.2 18.7C15.7 19.0 15 18.6 15 18.0Z" fill="currentColor"/>"#;
    LOOK => r#"<circle cx="12" cy="12" r="7.6" stroke="currentColor" stroke-width="1.7" fill="none"/><path d="M12 4.4A7.6 7.6 0 0 1 12 19.6Z" fill="currentColor"/>"#;
    MORE => r#"<circle cx="6" cy="12" r="1.7" fill="currentColor"/><circle cx="12" cy="12" r="1.7" fill="currentColor"/><circle cx="18" cy="12" r="1.7" fill="currentColor"/>"#;
    // The mini player: a window with a small player in its corner.
    MINI => r#"<rect x="3.8" y="5" width="16.4" height="14" rx="3.4" stroke="currentColor" stroke-width="1.7" fill="none"/><rect x="11.6" y="11.6" width="5.8" height="4.6" rx="1.4" fill="currentColor"/>"#;
    // Back to the main window: an arrow leaving a window's corner.
    EXPAND => r#"<path d="M13.8 4.5H19.5V10.2M19 5L12.6 11.4" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" fill="none"/><path d="M10.4 4.5H8C6.1 4.5 4.5 6.1 4.5 8V16C4.5 17.9 6.1 19.5 8 19.5H16C17.9 19.5 19.5 17.9 19.5 16V13.6" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" fill="none"/>"#;
    // The mini player's look: two stacked cards.
    LAYOUT => r#"<rect x="4.5" y="4.5" width="15" height="6.4" rx="2.2" stroke="currentColor" stroke-width="1.7" fill="none"/><rect x="4.5" y="13.1" width="15" height="6.4" rx="2.2" stroke="currentColor" stroke-width="1.7" fill="none"/>"#;
    PIN => r#"<path d="M8.6 4.5H15.4M9.9 4.5V8.9L7.6 12.4C7.3 12.9 7.6 13.5 8.2 13.5H15.8C16.4 13.5 16.7 12.9 16.4 12.4L14.1 8.9V4.5" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" fill="none"/><path d="M12 13.5V19.5" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" fill="none"/>"#;
    PIN_OFF => r#"<path d="M8.6 4.5H15.4M9.9 4.5V8.9L7.6 12.4C7.3 12.9 7.6 13.5 8.2 13.5H15.8C16.4 13.5 16.7 12.9 16.4 12.4L14.1 8.9V4.5" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" fill="none"/><path d="M12 13.5V19.5M4.5 4.5L19.5 19.5" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" fill="none"/>"#;
    CLOSE => r#"<path d="M6.5 6.5L17.5 17.5M17.5 6.5L6.5 17.5" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" fill="none"/>"#;
    // In place of a missing cover.
    NOTE => r#"<path d="M9.4 17.2V7C9.4 6.4 9.8 5.9 10.4 5.8L17.4 4.6C18 4.5 18.6 5 18.6 5.6V15.2M9.4 9.6L18.6 8" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" fill="none"/><circle cx="7.2" cy="17.2" r="2.2" stroke="currentColor" stroke-width="1.7" fill="none"/><circle cx="16.4" cy="15.2" r="2.2" stroke="currentColor" stroke-width="1.7" fill="none"/>"#;
    QUALITY_SAVER => r#"<circle cx="6.5" cy="12" r="1.3" fill="currentColor"/><path d="M12 8.2V15.8" stroke="currentColor" stroke-width="1.85" stroke-linecap="round"/><circle cx="17.5" cy="12" r="1.3" fill="currentColor"/>"#;
    QUALITY_BALANCED => r#"<path d="M7 9.2V14.8M12 5.8V18.2M17 9.2V14.8" stroke="currentColor" stroke-width="1.85" stroke-linecap="round"/>"#;
    QUALITY_BEST => r#"<path d="M4.6 10V14M8.3 7V17M12 4.6V19.4M15.7 7V17M19.4 10V14" stroke="currentColor" stroke-width="1.75" stroke-linecap="round"/>"#;
}

/// Custom audio quality icon for each playback bitrate tier.
pub fn quality_icon(q: crate::store::QualityPref) -> &'static Icon {
    use crate::store::QualityPref;
    match q {
        QualityPref::Saver => &QUALITY_SAVER,
        QualityPref::Balanced => &QUALITY_BALANCED,
        QualityPref::Best => &QUALITY_BEST,
    }
}

/// Localized label key for each playback bitrate tier.
pub fn quality_label(q: crate::store::QualityPref) -> crate::i18n::S {
    use crate::i18n::S;
    use crate::store::QualityPref;
    match q {
        QualityPref::Saver => S::QualitySaver,
        QualityPref::Balanced => S::QualityBalanced,
        QualityPref::Best => S::QualityBest,
    }
}
