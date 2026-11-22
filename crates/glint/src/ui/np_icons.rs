//! Glint's own glyphs for the open player: one family drawn for it alone, on a 24 grid with a
//! 1.6 stroke, round caps and soft corners, so the transport reads as one quiet line rather than a
//! row of borrowed symbols. The rest of the app keeps its icon set (`icons.rs`).

use rune::Icon;

macro_rules! icons {
    ($($name:ident => $body:literal;)*) => {
        $( pub static $name: Icon = Icon { name: concat!("glint-np-", stringify!($name)), view: 24.0, body: $body }; )*
    };
}

icons! {
    PLAY => r#"<path d="M7.5 6.1C7.5 4.9 8.8 4.2 9.8 4.8L19 10.7C19.9 11.3 19.9 12.7 19 13.3L9.8 19.2C8.8 19.8 7.5 19.1 7.5 17.9Z" fill="currentColor"/>"#;
    PAUSE => r#"<rect x="6.6" y="5" width="3.6" height="14" rx="1.8" fill="currentColor"/><rect x="13.8" y="5" width="3.6" height="14" rx="1.8" fill="currentColor"/>"#;
    NEXT => r#"<path d="M3.8 7.7C3.8 6.9 4.7 6.4 5.4 6.9L10.8 10.9C11.5 11.4 11.5 12.6 10.8 13.1L5.4 17.1C4.7 17.6 3.8 17.1 3.8 16.3Z" fill="currentColor"/><path d="M12.2 7.7C12.2 6.9 13.1 6.4 13.8 6.9L19.2 10.9C19.9 11.4 19.9 12.6 19.2 13.1L13.8 17.1C13.1 17.6 12.2 17.1 12.2 16.3Z" fill="currentColor"/>"#;
    PREV => r#"<path d="M20.2 7.7C20.2 6.9 19.3 6.4 18.6 6.9L13.2 10.9C12.5 11.4 12.5 12.6 13.2 13.1L18.6 17.1C19.3 17.6 20.2 17.1 20.2 16.3Z" fill="currentColor"/><path d="M11.8 7.7C11.8 6.9 10.9 6.4 10.2 6.9L4.8 10.9C4.1 11.4 4.1 12.6 4.8 13.1L10.2 17.1C10.9 17.6 11.8 17.1 11.8 16.3Z" fill="currentColor"/>"#;
    SHUFFLE => r#"<path d="M3.5 7.5H5.8C8.3 7.5 9.6 8.6 11 10.6L12.9 13.4C14.3 15.4 15.6 16.5 18.1 16.5H20.3" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" fill="none"/><path d="M3.5 16.5H5.8C7.4 16.5 8.5 16 9.4 15" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" fill="none"/><path d="M14.5 9C15.4 8 16.5 7.5 18.1 7.5H20.3" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" fill="none"/><path d="M18.2 5.3L20.4 7.5L18.2 9.7M18.2 14.3L20.4 16.5L18.2 18.7" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#;
    REPEAT => r#"<path d="M4.8 11.2V10.3C4.8 8.2 6.5 6.5 8.6 6.5H18.6M19.2 12.8V13.7C19.2 15.8 17.5 17.5 15.4 17.5H5.4" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" fill="none"/><path d="M16.4 4.3L18.6 6.5L16.4 8.7M7.6 15.3L5.4 17.5L7.6 19.7" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#;
    REPEAT_ONE => r#"<path d="M4.8 11.2V10.3C4.8 8.2 6.5 6.5 8.6 6.5H18.6M19.2 12.8V13.7C19.2 15.8 17.5 17.5 15.4 17.5H5.4" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" fill="none"/><path d="M16.4 4.3L18.6 6.5L16.4 8.7M7.6 15.3L5.4 17.5L7.6 19.7" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" fill="none"/><path d="M11 10.4L12.3 9.6V14.4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#;
    HEART => r#"<path d="M12 19.4C12 19.4 3.6 14.6 3.6 8.9C3.6 6.4 5.5 4.5 7.9 4.5C9.6 4.5 11.1 5.4 12 6.8C12.9 5.4 14.4 4.5 16.1 4.5C18.5 4.5 20.4 6.4 20.4 8.9C20.4 14.6 12 19.4 12 19.4Z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" fill="none"/>"#;
    HEART_FILL => r#"<path d="M12 19.4C12 19.4 3.6 14.6 3.6 8.9C3.6 6.4 5.5 4.5 7.9 4.5C9.6 4.5 11.1 5.4 12 6.8C12.9 5.4 14.4 4.5 16.1 4.5C18.5 4.5 20.4 6.4 20.4 8.9C20.4 14.6 12 19.4 12 19.4Z" fill="currentColor" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>"#;
    DISLIKE => r#"<path d="M12 19.4C12 19.4 3.6 14.6 3.6 8.9C3.6 6.4 5.5 4.5 7.9 4.5C9.6 4.5 11.1 5.4 12 6.8C12.9 5.4 14.4 4.5 16.1 4.5C18.5 4.5 20.4 6.4 20.4 8.9C20.4 14.6 12 19.4 12 19.4Z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" fill="none"/><path d="M12 6.8L10.6 10.1L13.3 12.2L11.5 15.6" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#;
    VOLUME => r#"<path d="M4 9.6C4 9 4.4 8.6 5 8.6H7.4L10.9 5.7C11.6 5.1 12.6 5.6 12.6 6.5V17.5C12.6 18.4 11.6 18.9 10.9 18.3L7.4 15.4H5C4.4 15.4 4 15 4 14.4Z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" fill="none"/><path d="M15.6 9.3C16.3 10.1 16.7 11 16.7 12C16.7 13 16.3 13.9 15.6 14.7M18.3 6.9C19.6 8.3 20.4 10.1 20.4 12C20.4 13.9 19.6 15.7 18.3 17.1" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" fill="none"/>"#;
    VOLUME_LOW => r#"<path d="M4 9.6C4 9 4.4 8.6 5 8.6H7.4L10.9 5.7C11.6 5.1 12.6 5.6 12.6 6.5V17.5C12.6 18.4 11.6 18.9 10.9 18.3L7.4 15.4H5C4.4 15.4 4 15 4 14.4Z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" fill="none"/><path d="M15.6 9.3C16.3 10.1 16.7 11 16.7 12C16.7 13 16.3 13.9 15.6 14.7" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" fill="none"/>"#;
    VOLUME_MUTE => r#"<path d="M4 9.6C4 9 4.4 8.6 5 8.6H7.4L10.9 5.7C11.6 5.1 12.6 5.6 12.6 6.5V17.5C12.6 18.4 11.6 18.9 10.9 18.3L7.4 15.4H5C4.4 15.4 4 15 4 14.4Z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" fill="none"/><path d="M16 9.8L20.4 14.2M20.4 9.8L16 14.2" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" fill="none"/>"#;
    LYRICS => r#"<path d="M4 7C4 5.6 5.1 4.5 6.5 4.5H17.5C18.9 4.5 20 5.6 20 7V14C20 15.4 18.9 16.5 17.5 16.5H10.5L7 19.3V16.5H6.5C5.1 16.5 4 15.4 4 14Z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" fill="none"/><path d="M8 9H16M8 12H13" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" fill="none"/>"#;
    QUEUE => r#"<path d="M4 6H20M4 11H20M4 16H12.5" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" fill="none"/><path d="M16 13.6C16 13.1 16.5 12.8 16.9 13.1L20.4 15.4C20.8 15.7 20.8 16.3 20.4 16.6L16.9 18.9C16.5 19.2 16 18.9 16 18.4Z" fill="currentColor"/>"#;
    LOOK => r#"<circle cx="12" cy="12" r="7.6" stroke="currentColor" stroke-width="1.6" fill="none"/><path d="M12 4.4A7.6 7.6 0 0 1 12 19.6Z" fill="currentColor"/>"#;
    MORE => r#"<circle cx="6" cy="12" r="1.6" fill="currentColor"/><circle cx="12" cy="12" r="1.6" fill="currentColor"/><circle cx="18" cy="12" r="1.6" fill="currentColor"/>"#;
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
