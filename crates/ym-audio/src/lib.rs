//! The audio side of Glint: decoding and playing tracks (gapless, with a 10-band equaliser),
//! a live spectrum for visuals. Its small bounded disk-cache utility is used by the app for
//! metadata and artwork, while downloaded audio is kept in the dedicated download store.
//!
//! The crate knows nothing about the service or the UI. Hand [`Engine::load`] the bytes of a
//! file, listen for [`Event`]s, and poll [`Engine::state`] / [`Engine::spectrum`] once per frame.
//!
//! ```no_run
//! use std::{sync::Arc, time::Duration};
//! use ym_audio::{Engine, LoadHint};
//!
//! let engine = Engine::new(|event| println!("{event:?}"))?;
//! let bytes: Arc<[u8]> = std::fs::read("song.mp3")?.into();
//! engine.load(1, bytes, LoadHint::default().ext("mp3"), true, Duration::ZERO);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod analyzer;
mod cache;
mod engine;
pub mod eq;
mod source;
mod tone;
pub mod synth;
pub mod wav;

use std::time::Duration;

pub use analyzer::Analyzer;
pub use tone::{Tone, midi};
pub use cache::DiskCache;
pub use engine::Engine;
pub use eq::{BAND_HZ, BANDS, EqSettings, MAX_GAIN_DB, PRESETS};

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("no audio output: {0}")]
    Device(String),
    #[error("cannot decode this audio: {0}")]
    Decode(String),
    #[error("unsupported audio: {0}")]
    Unsupported(String),
}

/// Something that happened on the engine thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Playback moved on to the queued track without a gap; `token` is the new current track.
    Advanced { token: u64 },
    /// The track ended and nothing was queued.
    Ended { token: u64 },
    /// A track could not be decoded.
    Error { token: u64, message: String },
    /// The output device reported a problem (unplugged, changed format...).
    DeviceError(String),
}

/// What the decoder cannot always find out for itself.
#[derive(Debug, Clone, Default)]
pub struct LoadHint {
    /// File extension (`"mp3"`, `"m4a"`, `"aac"`), helps pick the demuxer.
    pub ext: Option<String>,
    /// Length from the track's metadata, for files whose header does not say (VBR MP3).
    pub duration: Option<Duration>,
}

impl LoadHint {
    pub fn ext(mut self, ext: &str) -> Self {
        self.ext = Some(ext.to_owned());
        self
    }

    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }
}

/// A snapshot of playback, cheap to take every frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackState {
    /// The `token` of the current track, if any.
    pub token: Option<u64>,
    pub paused: bool,
    pub position: Duration,
    pub duration: Option<Duration>,
}
