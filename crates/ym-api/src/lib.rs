//! Async client for the unofficial Yandex Music API.
//!
//! This crate is deliberately UI-free: it knows the wire format (JSON envelopes, the signed
//! stream URLs, LRC lyrics) and nothing about windows or audio devices, so it is unit-testable
//! against fixtures and reusable.
//!
//! Coverage follows the reference Python client (MarshalX/yandex-music-api): account, likes,
//! playlists (with editing), albums, artists (with discography and similar), search and
//! suggestions, landing shelves and charts, Rotor sessions and stations, signed lossless streams,
//! lyrics, play reporting and server queues. Ynison (the websocket protocol that syncs playback
//! across devices) is not implemented: it is undocumented and needs a long-lived socket, so the
//! server queue endpoints are the supported way to publish what is playing.
//!
//! Security note: the OAuth token is attached per request and only to the Yandex API host, never
//! to image, audio or lyrics requests (see `client`).

pub(crate) mod aes_ctr;
pub mod auth;
pub mod client;
pub mod cover;
pub mod discover;
pub mod error;
pub mod files;
pub mod fkcensor;
pub mod library;
pub mod lrc;
pub mod lrcred;
pub mod models;
pub mod rotor;
pub mod sign;
pub mod wave;

pub use client::{AudioResponse, Client, Likeable, Quality, SearchKind, StreamInfo, WaveEvent};
pub use discover::{Chart, ChartEntry};
pub use error::{Error, Result};
pub use library::{PlayReport, QueueInfo, Visibility, device_header};
pub use lrc::{LyricLine, LyricWord, Lyrics, LyricsSource, join_words};
pub use models::*;
pub use rotor::{RotorSession, Station, WaveSettings, WheelItem};
pub use wave::{WAVE_PRESETS, WaveGroup, WavePreset};
