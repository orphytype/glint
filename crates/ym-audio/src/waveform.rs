//! The shape of a whole track: how loud it is along its length, for a seek line that shows where
//! the quiet and the loud parts are.
//!
//! Uses bounded O(n) memory even on multi-hour tracks: when duration is known, samples accumulate
//! straight into `n` fixed buckets; otherwise blocks downsample in place once they hit a small cap.

use crate::source::{AudioData, Track};
use crate::{AudioError, LoadHint};

/// How far below the loudest part a part may be and still show; anything quieter is the floor.
const RANGE_DB: f32 = 20.0;
/// Maximum RMS blocks retained when track duration is unknown (~64 KiB).
const MAX_UNKNOWN_BLOCKS: usize = 16_384;

/// Decodes `audio` and returns `n` values from 0 (silence) to 1 (the loudest part of the track),
/// one per equal slice of its length. It reads the whole file, so call it off the UI thread.
pub fn waveform(audio: impl Into<AudioData>, hint: &LoadHint, n: usize) -> Result<Vec<f32>, AudioError> {
    let n = n.max(1);
    let track = Track::open_sync(1, audio.into(), hint)?;
    let (channels, rate) = (track.channels.max(1), track.rate.max(1));
    // The loudness of every 10 ms, as RMS of the mono mix.
    let block_frames = (rate as usize / 100).max(1);
    let block = block_frames * channels;

    let slices: Vec<f32> = if let Some(total_frames) = track.duration.map(|d| (d.as_secs_f64() * f64::from(rate)).round() as u64).filter(|&f| f > 0) {
        // Fixed O(n) buckets: constant ~4 KiB RAM regardless of track duration.
        let mut buckets: Vec<(f64, u32, f32)> = vec![(0.0, 0, 0.0); n];
        let (mut sum, mut frame, mut count) = (0.0f64, 0.0f32, 0usize);
        let mut in_block = 0usize;
        let mut frames_seen = 0u64;
        let mut any = false;
        let mut push_block = |rms: f32, at_frame: u64| {
            let k = ((at_frame.saturating_mul(n as u64)) / total_frames).min((n - 1) as u64) as usize;
            let b = &mut buckets[k];
            b.0 += f64::from(rms * rms);
            b.1 += 1;
            b.2 = b.2.max(rms);
            any = true;
        };
        for (i, s) in track.samples().enumerate() {
            frame += s;
            if (i + 1) % channels == 0 {
                let m = frame / channels as f32;
                sum += f64::from(m * m);
                count += 1;
                frames_seen += 1;
                frame = 0.0;
            }
            in_block += 1;
            if in_block == block {
                let rms = (sum / count.max(1) as f64).sqrt() as f32;
                push_block(rms, frames_seen.saturating_sub((count / 2) as u64));
                (sum, count, in_block) = (0.0, 0, 0);
            }
        }
        if count > 0 {
            let rms = (sum / count as f64).sqrt() as f32;
            push_block(rms, frames_seen.saturating_sub((count / 2) as u64));
        }
        if !any {
            return Err(AudioError::Decode("the audio has no samples".into()));
        }
        buckets
            .iter()
            .map(|&(sum_sq, cnt, peak)| {
                if cnt == 0 {
                    0.0
                } else {
                    let mean = (sum_sq / f64::from(cnt)).sqrt() as f32;
                    0.75 * mean + 0.25 * peak
                }
            })
            .collect()
    } else {
        // Duration unknown: cap blocks at MAX_UNKNOWN_BLOCKS by folding pairs 2:1 when full.
        let mut blocks: Vec<(f32, f32)> = Vec::new();
        let mut stride = 1usize;
        let (mut sum, mut frame, mut count) = (0.0f64, 0.0f32, 0usize);
        let mut in_block = 0usize;
        let push = |blocks: &mut Vec<(f32, f32)>, stride: &mut usize, rms: f32| {
            blocks.push((rms, rms));
            if blocks.len() >= MAX_UNKNOWN_BLOCKS {
                let pairs = blocks.len() / 2;
                for p in 0..pairs {
                    let (m0, p0) = blocks[p * 2];
                    let (m1, p1) = blocks[p * 2 + 1];
                    blocks[p] = (((m0 * m0 + m1 * m1) * 0.5).sqrt(), p0.max(p1));
                }
                blocks.truncate(pairs);
                *stride = stride.saturating_mul(2);
            }
        };
        for (i, s) in track.samples().enumerate() {
            frame += s;
            if (i + 1) % channels == 0 {
                let m = frame / channels as f32;
                sum += f64::from(m * m);
                count += 1;
                frame = 0.0;
            }
            in_block += 1;
            if in_block == block.saturating_mul(stride) {
                push(&mut blocks, &mut stride, (sum / count.max(1) as f64).sqrt() as f32);
                (sum, count, in_block) = (0.0, 0, 0);
            }
        }
        if count > 0 {
            push(&mut blocks, &mut stride, (sum / count as f64).sqrt() as f32);
        }
        if blocks.is_empty() {
            return Err(AudioError::Decode("the audio has no samples".into()));
        }
        (0..n)
            .map(|k| {
                let a = k * blocks.len() / n;
                let b = ((k + 1) * blocks.len() / n).max(a + 1).min(blocks.len());
                let part = &blocks[a.min(blocks.len() - 1)..b];
                let mean = (part.iter().map(|(m, _)| m * m).sum::<f32>() / part.len() as f32).sqrt();
                let peak = part.iter().map(|&(_, p)| p).fold(0.0, f32::max);
                0.75 * mean + 0.25 * peak
            })
            .collect()
    };

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
