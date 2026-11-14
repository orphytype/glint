//! Async client for the unofficial Yandex Music API.
//!
//! This crate is deliberately UI-free: it knows the wire format (JSON envelopes, the signed
//! stream URLs, LRC lyrics) and nothing about windows or audio devices, so it is unit-testable
//! against fixtures and reusable.
//!
//! Security note: the OAuth token is attached per request and only to the Yandex API host, never
//! to image, audio or lyrics requests (see `client`).

pub mod auth;
pub mod client;
pub mod cover;
pub mod error;
pub mod fkcensor;
pub mod lrc;
pub mod lrcred;
pub mod models;
pub mod sign;
pub mod wave;

pub use client::{Client, Likeable, Quality, SearchKind, StreamInfo, WaveEvent};
pub use error::{Error, Result};
pub use lrc::{LyricLine, LyricWord, Lyrics, LyricsSource, join_words};
pub use models::*;
pub use wave::{WAVE_PRESETS, WaveGroup, WavePreset};
