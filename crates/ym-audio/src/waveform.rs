//! The shape of a whole track: how loud it is along its length, for a seek line that shows where
//! the quiet and the loud parts are.

use std::sync::Arc;

use crate::source::Track;
use crate::{AudioError, LoadHint};

/// How far below the loudest part a part may be and still show; anything quieter is the floor.
const RANGE_DB: f32 = 20.0;

/// Decodes `audio` and returns `n` values from 0 (silence) to 1 (the loudest part of the track),
/// one per equal slice of its length. It reads the whole file, so call it off the UI thread.
pub fn waveform(audio: Arc<[u8]>, hint: &LoadHint, n: usize) -> Result<Vec<f32>, AudioError> {
    let n = n.max(1);
    let track = Track::open(1, audio, hint)?;
    let (channels, rate) = (track.channels.max(1), track.rate.max(1));
    // The loudness of every 10 ms, as RMS of the mono mix.
    let block = (rate as usize / 100).max(1) * channels;
    let mut blocks: Vec<f32> = Vec::new();
    let (mut sum, mut frame, mut count) = (0.0f64, 0.0f32, 0usize);
    let mut in_block = 0usize;
    for (i, s) in track.samples().enumerate() {
        frame += s;
        if (i + 1) % channels == 0 {
            let m = frame / channels as f32;
            sum += f64::from(m * m);
            count += 1;
            frame = 0.0;
        }
        in_block += 1;
        if in_block == block {
            blocks.push((sum / count.max(1) as f64).sqrt() as f32);
            (sum, count, in_block) = (0.0, 0, 0);
        }
    }
    if count > 0 {
        blocks.push((sum / count as f64).sqrt() as f32);
    }
    if blocks.is_empty() {
        return Err(AudioError::Decode("the audio has no samples".into()));
    }
    // Each slice: mostly how loud it is overall, partly its loudest moment, so a drum hit in a
    // quiet slice still shows.
    let slices: Vec<f32> = (0..n)
        .map(|k| {
            let a = k * blocks.len() / n;
            let b = ((k + 1) * blocks.len() / n).max(a + 1).min(blocks.len());
            let part = &blocks[a.min(blocks.len() - 1)..b];
            let mean = (part.iter().map(|v| v * v).sum::<f32>() / part.len() as f32).sqrt();
            let peak = part.iter().copied().fold(0.0, f32::max);
            0.75 * mean + 0.25 * peak
        })
        .collect();
    // In decibels below the loudest slice, so the shape reads the same for quiet and loud masters.
    let top = slices.iter().copied().fold(0.0, f32::max).max(1e-6);
    Ok(slices
        .iter()
        .map(|v| {
            let db = 20.0 * (v.max(1e-9) / top).log10();
            // A steep curve: most of a loud master sits within a few dB of its loudest part, and
            // only the very loudest moments should stand tall (2 dB down is already a third lower).
            ((db + RANGE_DB) / RANGE_DB).clamp(0.0, 1.0).powi(3)
        })
        .collect())
}
