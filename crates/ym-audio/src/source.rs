//! What the mixer actually plays: a decoded track wrapped with the equaliser, the analysis tap
//! and click-free envelopes for pause, seek, stop and volume — plus a one-slot queue so the next
//! track follows the current one without a gap.
//!
//! Everything here runs on the audio thread, so it never blocks (the only lock it takes is the
//! queue slot, which the control thread holds for a pointer swap) and never allocates.
//! Control comes in through atomics ([`Shared`], [`SourceCtl`]), polled every few frames.

use std::io::Cursor;
use std::num::NonZero;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rodio::{ChannelCount, Decoder, SampleRate, Source};

use crate::analyzer::{TAP_BLOCK, TapShared};
use crate::eq::{EqFilter, EqShared};
use crate::{AudioError, LoadHint};

pub(crate) const NO_SEEK: u64 = u64::MAX;
const MAX_CHANNELS: usize = 8;
/// How often (in frames) the audio thread looks at the control flags.
const POLL_FRAMES: u32 = 32;
/// Pause, seek and stop fade over about this long, which is what removes the clicks.
const FADE_SECS: f32 = 0.012;
/// How long a track that replaces another takes to fade in, and the one it replaces to fade out:
/// long enough to take the edge off the change, short enough that it happens at once.
pub(crate) const SWITCH_FADE_SECS: f32 = 0.1;
/// Volume changes glide with this time constant.
const VOLUME_SECS: f32 = 0.02;

/// Engine-wide state shared between the control side and whichever source is playing.
pub(crate) struct Shared {
    pub paused: AtomicBool,
    /// `f32` bits: the target gain.
    pub volume: AtomicU32,
    /// Pending seek in microseconds, or [`NO_SEEK`].
    pub seek_us: AtomicU64,
    /// Position in the current track, published by the audio thread.
    pub pos_us: AtomicU64,
    /// Length of the current track; 0 = unknown.
    pub dur_us: AtomicU64,
    /// Token of the track that is playing; 0 = none.
    pub token: AtomicU64,
    pub eq: EqShared,
    pub tap: TapShared,
}

impl Default for Shared {
    fn default() -> Self {
        Shared {
            paused: AtomicBool::new(false),
            volume: AtomicU32::new(1f32.to_bits()),
            seek_us: AtomicU64::new(NO_SEEK),
            pos_us: AtomicU64::new(0),
            dur_us: AtomicU64::new(0),
            token: AtomicU64::new(0),
            eq: EqShared::default(),
            tap: TapShared::default(),
        }
    }
}

/// Per-source control: one per `load`.
pub(crate) struct SourceCtl {
    /// Fade out and end (the track is being replaced or stopped).
    pub stop: AtomicBool,
    pub stop_fade_secs: AtomicU32,
    /// The queued next track, taken by the source when the current one runs out.
    pub next: Mutex<Option<Track>>,
    /// How many times the source moved on to the queued track.
    pub joins: AtomicU32,
    /// The source ran out of audio on its own (and had nothing it could join).
    pub finished: AtomicBool,
}

impl SourceCtl {
    pub fn new() -> SourceCtl {
        SourceCtl {
            stop: AtomicBool::new(false),
            stop_fade_secs: AtomicU32::new(FADE_SECS.to_bits()),
            next: Mutex::new(None),
            joins: AtomicU32::new(0),
            finished: AtomicBool::new(false),
        }
    }

    pub fn lock_next(&self) -> std::sync::MutexGuard<'_, Option<Track>> {
        self.next.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn stop_with_fade(&self, seconds: f32) {
        self.stop_fade_secs.store(seconds.max(FADE_SECS).to_bits(), Ordering::Release);
        self.stop.store(true, Ordering::Release);
    }
}

type Pcm = Decoder<Cursor<Arc<[u8]>>>;

/// One decoded track with its own equaliser state.
pub(crate) struct Track {
    pub token: u64,
    dec: Pcm,
    pub channels: usize,
    pub rate: u32,
    pub duration: Option<Duration>,
    eq: EqFilter,
}

impl Track {
    pub fn open(token: u64, audio: Arc<[u8]>, hint: &LoadHint) -> Result<Track, AudioError> {
        let len = audio.len() as u64;
        if len == 0 {
            return Err(AudioError::Decode("the audio is empty".into()));
        }
        let mut builder = Decoder::builder().with_data(Cursor::new(audio)).with_byte_len(len).with_gapless(true);
        if let Some(ext) = hint.ext.as_deref() {
            builder = builder.with_hint(ext);
        }
        let dec = builder.build().map_err(|e| AudioError::Decode(e.to_string()))?;
        let channels = usize::from(dec.channels().get());
        let rate = dec.sample_rate().get();
        if channels > MAX_CHANNELS {
            return Err(AudioError::Unsupported(format!("{channels} channels")));
        }
        let duration = dec.total_duration().or(hint.duration);
        Ok(Track { token, dec, channels, rate, duration, eq: EqFilter::new(channels, rate) })
    }

    /// The decoded samples, interleaved, without the equaliser (for measuring the track).
    pub fn samples(self) -> Pcm {
        self.dec
    }

    /// Positions the decoder; the filters restart so stale state cannot ring.
    pub fn seek(&mut self, to: Duration) -> bool {
        let ok = self.dec.try_seek(to).is_ok();
        if ok {
            self.eq.reset();
        }
        ok
    }

    /// Reads one frame (all channels), EQ applied. `false` only when the track has no audio left;
    /// a truncated final frame is padded with silence.
    fn read_frame(&mut self, out: &mut [f32; MAX_CHANNELS]) -> bool {
        for (c, slot) in out.iter_mut().enumerate().take(self.channels) {
            match self.dec.next() {
                Some(s) => *slot = self.eq.process(c, s),
                None if c == 0 => return false,
                None => *slot = 0.0,
            }
        }
        true
    }
}

/// The source handed to the mixer.
pub(crate) struct Playback {
    cur: Track,
    ctl: Arc<SourceCtl>,
    shared: Arc<Shared>,
    frame: [f32; MAX_CHANNELS],
    /// Next sample of `frame` to hand out; `== channels` means "decode another frame".
    out_idx: usize,
    frame_ctr: u32,

    fade: f32,
    fade_target: f32,
    fade_step: f32,
    gain: f32,
    gain_alpha: f32,
    volume_target: f32,
    stopping: bool,
    pending_seek: Option<u64>,

    /// Where the current track's frame counter starts, in microseconds.
    base_us: u64,
    frames: u64,
    /// The position is published every ~10 ms of audio, whatever the sample rate.
    publish_every: u32,
    until_publish: u32,

    tap_block: [f32; TAP_BLOCK],
    tap_len: usize,
}

impl Playback {
    pub fn new(track: Track, ctl: Arc<SourceCtl>, shared: Arc<Shared>, start: Duration, fade_in_secs: f32) -> Playback {
        let rate = track.rate as f32;
        shared.tap.set_rate(track.rate);
        let volume = f32::from_bits(shared.volume.load(Ordering::Relaxed));
        let mut p = Playback {
            out_idx: track.channels,
            cur: track,
            ctl,
            shared,
            frame: [0.0; MAX_CHANNELS],
            frame_ctr: 0,
            fade: 0.0,
            fade_target: 1.0,
            fade_step: 1.0 / (fade_in_secs.max(FADE_SECS) * rate),
            gain: volume,
            gain_alpha: 1.0 - (-1.0 / (VOLUME_SECS * rate)).exp(),
            volume_target: volume,
            stopping: false,
            pending_seek: None,
            base_us: 0,
            frames: 0,
            publish_every: (rate as u32 / 100).max(1),
            until_publish: (rate as u32 / 100).max(1),
            tap_block: [0.0; TAP_BLOCK],
            tap_len: 0,
        };
        p.cur.eq.refresh(&p.shared.eq);
        if !start.is_zero() {
            p.do_seek(start.as_micros() as u64);
        }
        p.publish_position();
        p
    }

    fn publish_position(&self) {
        // While a seek waits for its fade-out, the frames still played are from before the jump:
        // publishing them would pull the position back from where the seek already put it, and
        // the seek line and the lyrics would jump back before going forward.
        let seeking = self.pending_seek.is_some() || self.shared.seek_us.load(Ordering::Relaxed) != NO_SEEK;
        if !self.stopping && !seeking {
            let us = self.base_us + self.frames * 1_000_000 / u64::from(self.cur.rate.max(1));
            self.shared.pos_us.store(us, Ordering::Relaxed);
        }
    }

    fn poll(&mut self) {
        self.cur.eq.refresh(&self.shared.eq);
        self.volume_target = f32::from_bits(self.shared.volume.load(Ordering::Relaxed));
        if self.ctl.stop.load(Ordering::Acquire) {
            self.stopping = true;
            let seconds = f32::from_bits(self.ctl.stop_fade_secs.load(Ordering::Acquire)).max(FADE_SECS);
            self.fade_step = 1.0 / (seconds * self.cur.rate.max(1) as f32);
        }
        // A replaced source must not swallow the seek meant for its successor.
        if !self.stopping {
            let request = self.shared.seek_us.swap(NO_SEEK, Ordering::AcqRel);
            if request != NO_SEEK {
                self.pending_seek = Some(request);
            }
        }
        let paused = self.shared.paused.load(Ordering::Relaxed);
        self.fade_target = if self.stopping || paused || self.pending_seek.is_some() { 0.0 } else { 1.0 };
    }

    fn do_seek(&mut self, us: u64) {
        let mut target = Duration::from_micros(us);
        if let Some(d) = self.cur.duration {
            target = target.min(d.saturating_sub(Duration::from_millis(50)));
        }
        if self.cur.seek(target) {
            self.base_us = target.as_micros() as u64;
            self.frames = 0;
            self.shared.tap.clear();
        }
        self.publish_position();
    }

    /// Moves on to the queued track if it is compatible; otherwise leaves it for the engine.
    fn join_next(&mut self) -> bool {
        let mut slot = self.ctl.lock_next();
        match slot.take() {
            Some(mut next) if next.channels == self.cur.channels && next.rate == self.cur.rate => {
                next.eq.refresh(&self.shared.eq);
                self.cur = next;
                self.base_us = 0;
                self.frames = 0;
                self.shared.token.store(self.cur.token, Ordering::Release);
                self.shared.dur_us.store(self.cur.duration.map_or(0, |d| d.as_micros() as u64), Ordering::Relaxed);
                self.publish_position();
                self.ctl.joins.fetch_add(1, Ordering::Release);
                true
            }
            other => {
                *slot = other;
                false
            }
        }
    }

    /// Prepares the next frame. `false` ends the source.
    fn fill_frame(&mut self) -> bool {
        if self.frame_ctr.is_multiple_of(POLL_FRAMES) {
            self.poll();
        }
        self.frame_ctr = self.frame_ctr.wrapping_add(1);

        if self.fade < self.fade_target {
            self.fade = (self.fade + self.fade_step).min(self.fade_target);
        } else if self.fade > self.fade_target {
            self.fade = (self.fade - self.fade_step).max(self.fade_target);
        }

        let ch = self.cur.channels;
        if self.fade <= 0.0 {
            if self.stopping {
                return false;
            }
            if let Some(us) = self.pending_seek.take() {
                self.do_seek(us);
            }
            if self.fade_target <= 0.0 {
                // Paused (or between a seek and its fade-in): silence, decoder untouched.
                self.frame[..ch].fill(0.0);
                return true;
            }
        }

        while !self.cur.read_frame(&mut self.frame) {
            if !self.join_next() {
                self.ctl.finished.store(true, Ordering::Release);
                return false;
            }
        }

        self.frames += 1;
        self.until_publish -= 1;
        if self.until_publish == 0 {
            self.until_publish = self.publish_every;
            self.publish_position();
        }

        // The analysis tap hears the mono mix after the EQ but before volume and fades, so the
        // visuals do not vanish when the volume is low.
        let ch = self.cur.channels;
        self.tap_block[self.tap_len] = self.frame[..ch].iter().sum::<f32>() / ch as f32;
        self.tap_len += 1;
        if self.tap_len == TAP_BLOCK {
            self.shared.tap.push(&self.tap_block);
            self.tap_len = 0;
        }

        self.gain += (self.volume_target - self.gain) * self.gain_alpha;
        let f = self.fade;
        let gain = self.gain * f * f * (3.0 - 2.0 * f);
        for s in &mut self.frame[..ch] {
            *s = (*s * gain).clamp(-1.0, 1.0);
        }
        true
    }
}

impl Iterator for Playback {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<f32> {
        if self.out_idx >= self.cur.channels {
            if !self.fill_frame() {
                return None;
            }
            self.out_idx = 0;
        }
        let s = self.frame[self.out_idx];
        self.out_idx += 1;
        Some(s)
    }
}

impl Source for Playback {
    /// The format never changes within a `Playback` (only identical formats are joined).
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        NonZero::new(self.cur.channels as u16).unwrap_or(NonZero::<u16>::MIN)
    }

    fn sample_rate(&self) -> SampleRate {
        NonZero::new(self.cur.rate).unwrap_or(NonZero::<u32>::MIN)
    }

    fn total_duration(&self) -> Option<Duration> {
        self.cur.duration
    }
}
