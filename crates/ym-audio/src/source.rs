//! What the mixer actually plays: a decoded track fed by a small bounded decode-ahead thread,
//! wrapped with the equaliser, the analysis tap and click-free envelopes for pause, seek, stop,
//! buffering and volume — plus a one-slot queue so the next track follows without a gap.
//!
//! The audio callback thread never blocks on network or disk I/O and never allocates on steady
//! playback. Control comes in through atomics ([`Shared`], [`SourceCtl`]), polled every few frames.

use std::fs::File;
use std::io::{self, BufReader, Cursor, Read, Seek, SeekFrom};
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, Thread};
use std::time::Duration;

use rodio::{ChannelCount, Decoder, SampleRate, Source};

use crate::analyzer::{TAP_BLOCK, TapShared};
use crate::eq::{EqFilter, EqShared};
use crate::stream::{StreamHandle, StreamReader};
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
/// Read buffer per open local file stream (64 KiB keeps RAM tiny while keeping disk reads fast).
const STREAM_BUF_BYTES: usize = 64 * 1024;
/// Approximate samples per decoded ahead chunk (~1024 f32 = 4 KiB).
const DECODE_CHUNK_SAMPLES: usize = 1024;
/// Bounded decode-ahead queue depth (16 * 4 KiB = 64 KiB RAM, ~180 ms @ 44.1 kHz stereo).
const DECODE_QUEUE_CHUNKS: usize = 16;

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

pub(crate) struct FileBacking {
    path: PathBuf,
    len: u64,
    delete_on_drop: bool,
}

impl Drop for FileBacking {
    fn drop(&mut self) {
        if self.delete_on_drop {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[derive(Clone)]
enum AudioBacking {
    Bytes(Arc<[u8]>),
    File(Arc<FileBacking>),
    Stream(Arc<StreamHandle>),
}

/// Audio payload backing a decoded track: an in-memory buffer, a seekable file on disk, or a live
/// progressive stream buffer. Clones share the underlying buffer/file/stream handle; temporary
/// files and streams clean up automatically when the last clone drops.
#[derive(Clone)]
pub struct AudioData(AudioBacking);

impl AudioData {
    /// Opens an existing file on disk without copying it into RAM.
    pub fn file(path: impl Into<PathBuf>, len: u64) -> Self {
        AudioData(AudioBacking::File(Arc::new(FileBacking { path: path.into(), len, delete_on_drop: false })))
    }

    /// Opens a temporary file on disk that is removed automatically when playback and any waveform
    /// analysis finish with it.
    pub fn temp_file(path: impl Into<PathBuf>, len: u64) -> Self {
        AudioData(AudioBacking::File(Arc::new(FileBacking { path: path.into(), len, delete_on_drop: true })))
    }

    /// Opens a progressive disk-backed stream that plays while downloading.
    pub fn stream(handle: Arc<StreamHandle>) -> Self {
        AudioData(AudioBacking::Stream(handle))
    }

    pub fn byte_len(&self) -> u64 {
        match &self.0 {
            AudioBacking::Bytes(b) => b.len() as u64,
            AudioBacking::File(f) => f.len,
            AudioBacking::Stream(s) => s.byte_len(),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        match &self.0 {
            AudioBacking::Bytes(_) | AudioBacking::Stream(_) => None,
            AudioBacking::File(f) => Some(&f.path),
        }
    }

    /// True if all bytes are already available on disk or in memory.
    pub fn is_complete(&self) -> bool {
        match &self.0 {
            AudioBacking::Bytes(_) | AudioBacking::File(_) => true,
            AudioBacking::Stream(s) => s.is_complete(),
        }
    }

    /// Blocks until a progressive stream finishes downloading cleanly (returns `false` if cancelled
    /// or failed). Immediately returns `true` for in-memory or complete local files.
    pub fn wait_complete(&self) -> bool {
        match &self.0 {
            AudioBacking::Bytes(_) | AudioBacking::File(_) => true,
            AudioBacking::Stream(s) => s.wait_complete(),
        }
    }

    /// Cancels an active progressive stream immediately so any blocked reader or downloader stops.
    pub fn cancel(&self) {
        if let AudioBacking::Stream(s) = &self.0 {
            s.cancel();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        match &self.0 {
            AudioBacking::Stream(s) => s.is_cancelled(),
            _ => false,
        }
    }
}

impl From<Arc<[u8]>> for AudioData {
    fn from(bytes: Arc<[u8]>) -> Self {
        AudioData(AudioBacking::Bytes(bytes))
    }
}

pub(crate) enum AudioReader {
    Bytes(Cursor<Arc<[u8]>>),
    File { _keep: Arc<FileBacking>, reader: BufReader<File> },
    Stream(StreamReader),
}

impl AudioReader {
    fn open(data: AudioData) -> Result<(Self, Option<u64>, bool), AudioError> {
        match data.0 {
            AudioBacking::Bytes(bytes) => {
                let len = bytes.len() as u64;
                Ok((AudioReader::Bytes(Cursor::new(bytes)), Some(len), false))
            }
            AudioBacking::File(keep) => {
                let file = File::open(&keep.path).map_err(|e| AudioError::Decode(e.to_string()))?;
                let len = if keep.len > 0 { keep.len } else { file.metadata().map_or(0, |m| m.len()) };
                Ok((AudioReader::File { _keep: keep, reader: BufReader::with_capacity(STREAM_BUF_BYTES, file) }, Some(len), false))
            }
            AudioBacking::Stream(handle) => {
                let len = handle.byte_len();
                let len = (len > 0).then_some(len);
                Ok((AudioReader::Stream(StreamReader::new(handle)), len, true))
            }
        }
    }
}

impl Read for AudioReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            AudioReader::Bytes(c) => c.read(buf),
            AudioReader::File { reader, .. } => reader.read(buf),
            AudioReader::Stream(s) => s.read(buf),
        }
    }
}

impl Seek for AudioReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        match self {
            AudioReader::Bytes(c) => c.seek(pos),
            AudioReader::File { reader, .. } => reader.seek(pos),
            AudioReader::Stream(s) => s.seek(pos),
        }
    }
}

type Pcm = Decoder<AudioReader>;

/// Synchronous decoder handle used for offline probing and waveform computation (no worker thread).
pub(crate) struct SyncTrack {
    dec: Pcm,
    pub channels: usize,
    pub rate: u32,
    pub duration: Option<Duration>,
}

impl SyncTrack {
    pub fn samples(self) -> Pcm {
        self.dec
    }
}

fn open_decoder(audio: AudioData, hint: &LoadHint) -> Result<(Pcm, usize, u32, Option<Duration>), AudioError> {
    let (reader, len, is_stream) = AudioReader::open(audio)?;
    if !is_stream && len == Some(0) {
        return Err(AudioError::Decode("the audio is empty".into()));
    }
    let mut builder = Decoder::builder().with_data(reader).with_gapless(true);
    if let Some(byte_len) = len.filter(|&n| n > 0) {
        builder = builder.with_byte_len(byte_len);
    }
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
    Ok((dec, channels, rate, duration))
}

struct DecodeCtl {
    seek_us: AtomicU64,
    epoch: AtomicU64,
    closed: AtomicBool,
}

enum FrameRead {
    Ready,
    Buffering,
    Eof,
}

/// Bounded decode-ahead worker so the real-time audio callback never blocks on disk or network I/O.
struct DecodeAhead {
    ctl: Arc<DecodeCtl>,
    rx: Receiver<(u64, Vec<f32>)>,
    worker: Thread,
    chunk: Vec<f32>,
    at: usize,
    expected_epoch: u64,
    ended: bool,
}

impl DecodeAhead {
    fn spawn(mut dec: Pcm, channels: usize, start: Option<Duration>) -> Result<DecodeAhead, AudioError> {
        let chunk_samples = (DECODE_CHUNK_SAMPLES / channels.max(1)).max(1) * channels.max(1);
        let (tx, rx) = mpsc::sync_channel::<(u64, Vec<f32>)>(DECODE_QUEUE_CHUNKS);
        let init_seek = start.filter(|d| !d.is_zero()).map_or(NO_SEEK, |d| u64::try_from(d.as_micros()).unwrap_or(NO_SEEK - 1).min(NO_SEEK - 1));
        let ctl = Arc::new(DecodeCtl { seek_us: AtomicU64::new(init_seek), epoch: AtomicU64::new(0), closed: AtomicBool::new(false) });
        let worker_ctl = ctl.clone();
        let worker = thread::Builder::new()
            .name("ym-decode".into())
            .spawn(move || {
                loop {
                    if worker_ctl.closed.load(Ordering::Acquire) {
                        return;
                    }
                    let req = worker_ctl.seek_us.swap(NO_SEEK, Ordering::AcqRel);
                    if req != NO_SEEK {
                        let _ = dec.try_seek(Duration::from_micros(req));
                    }
                    let cur_epoch = worker_ctl.epoch.load(Ordering::Acquire);
                    let mut buf = Vec::with_capacity(chunk_samples);
                    for _ in 0..chunk_samples {
                        if worker_ctl.closed.load(Ordering::Relaxed) || worker_ctl.epoch.load(Ordering::Relaxed) != cur_epoch {
                            break;
                        }
                        match dec.next() {
                            Some(s) => buf.push(s),
                            None => break,
                        }
                    }
                    if worker_ctl.closed.load(Ordering::Acquire) {
                        return;
                    }
                    if worker_ctl.epoch.load(Ordering::Acquire) != cur_epoch {
                        continue;
                    }
                    let eof = buf.is_empty();
                    if tx.send((cur_epoch, buf)).is_err() {
                        return;
                    }
                    while eof && worker_ctl.epoch.load(Ordering::Acquire) == cur_epoch {
                        if worker_ctl.closed.load(Ordering::Acquire) {
                            return;
                        }
                        thread::park_timeout(Duration::from_millis(150));
                    }
                }
            })
            .map_err(|e| AudioError::Decode(e.to_string()))?
            .thread()
            .clone();
        Ok(DecodeAhead { ctl, rx, worker, chunk: Vec::new(), at: 0, expected_epoch: 0, ended: false })
    }

    fn request_seek(&mut self, to: Duration) {
        let us = u64::try_from(to.as_micros()).unwrap_or(NO_SEEK - 1).min(NO_SEEK - 1);
        self.expected_epoch = self.ctl.epoch.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        self.ctl.seek_us.store(us, Ordering::Release);
        self.chunk.clear();
        self.at = 0;
        self.ended = false;
        // Drain queued pre-seek chunks so a worker blocked in `tx.send` wakes immediately.
        while self.rx.try_recv().is_ok() {}
        self.worker.unpark();
    }

    fn poll_chunk(&mut self) -> FrameRead {
        if self.at < self.chunk.len() {
            return FrameRead::Ready;
        }
        if self.ended {
            return FrameRead::Eof;
        }
        loop {
            match self.rx.try_recv() {
                Ok((epoch, chunk)) => {
                    if epoch != self.expected_epoch {
                        continue;
                    }
                    if chunk.is_empty() {
                        self.ended = true;
                        return FrameRead::Eof;
                    }
                    self.chunk = chunk;
                    self.at = 0;
                    return FrameRead::Ready;
                }
                Err(TryRecvError::Empty) => return FrameRead::Buffering,
                Err(TryRecvError::Disconnected) => {
                    self.ended = true;
                    return FrameRead::Eof;
                }
            }
        }
    }

    fn next_sample(&mut self) -> Option<f32> {
        let s = *self.chunk.get(self.at)?;
        self.at += 1;
        Some(s)
    }
}

impl Drop for DecodeAhead {
    fn drop(&mut self) {
        self.ctl.closed.store(true, Ordering::Release);
        // Drain the channel so a worker waiting in `tx.send` can notice `closed` at once.
        while self.rx.try_recv().is_ok() {}
        self.worker.unpark();
    }
}

/// One decoded track with its own equaliser state and decode-ahead worker.
pub(crate) struct Track {
    pub token: u64,
    ahead: DecodeAhead,
    pub channels: usize,
    pub rate: u32,
    pub duration: Option<Duration>,
    eq: EqFilter,
    /// Held so dropping `Track` cancels any underlying progressive stream immediately.
    audio: AudioData,
}

impl Track {
    pub fn open(token: u64, audio: AudioData, hint: &LoadHint, start: Duration) -> Result<Track, AudioError> {
        let (dec, channels, rate, duration) = open_decoder(audio.clone(), hint)?;
        let start = if start.is_zero() {
            None
        } else {
            let clamped = duration.map_or(start, |d| start.min(d.saturating_sub(Duration::from_millis(50))));
            Some(clamped)
        };
        let ahead = DecodeAhead::spawn(dec, channels, start)?;
        Ok(Track { token, ahead, channels, rate, duration, eq: EqFilter::new(channels, rate), audio })
    }

    pub fn open_sync(_token: u64, audio: AudioData, hint: &LoadHint) -> Result<SyncTrack, AudioError> {
        let (dec, channels, rate, duration) = open_decoder(audio, hint)?;
        Ok(SyncTrack { dec, channels, rate, duration })
    }

    /// Requests a non-blocking seek on the decode-ahead worker; resets EQ state immediately.
    pub fn seek(&mut self, to: Duration) {
        self.ahead.request_seek(to);
        self.eq.reset();
    }

    fn poll_ready(&mut self) -> FrameRead {
        self.ahead.poll_chunk()
    }

    /// Reads one frame (all channels), EQ applied, without blocking the audio callback thread.
    fn read_frame(&mut self, out: &mut [f32; MAX_CHANNELS]) -> FrameRead {
        match self.ahead.poll_chunk() {
            FrameRead::Ready => {}
            other => return other,
        }
        for (c, slot) in out.iter_mut().enumerate().take(self.channels) {
            match self.ahead.next_sample() {
                Some(s) => *slot = self.eq.process(c, s),
                None if c == 0 => return FrameRead::Eof,
                None => *slot = 0.0,
            }
        }
        FrameRead::Ready
    }
}

impl Drop for Track {
    fn drop(&mut self) {
        self.audio.cancel();
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
    buffering: bool,
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
        let start = if start.is_zero() {
            Duration::ZERO
        } else {
            track.duration.map_or(start, |d| start.min(d.saturating_sub(Duration::from_millis(50))))
        };
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
            buffering: false,
            pending_seek: None,
            base_us: start.as_micros() as u64,
            frames: 0,
            publish_every: (rate as u32 / 100).max(1),
            until_publish: (rate as u32 / 100).max(1),
            tap_block: [0.0; TAP_BLOCK],
            tap_len: 0,
        };
        p.cur.eq.refresh(&p.shared.eq);
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
        self.fade_target = if self.stopping || paused || self.pending_seek.is_some() || self.buffering { 0.0 } else { 1.0 };
    }

    fn do_seek(&mut self, us: u64) {
        let mut target = Duration::from_micros(us);
        if let Some(d) = self.cur.duration {
            target = target.min(d.saturating_sub(Duration::from_millis(50)));
        }
        self.cur.seek(target);
        self.base_us = target.as_micros() as u64;
        self.frames = 0;
        self.buffering = true;
        self.shared.tap.clear();
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
                self.buffering = false;
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
            if self.buffering {
                match self.cur.poll_ready() {
                    FrameRead::Ready => {
                        self.buffering = false;
                        self.fade_step = 1.0 / (FADE_SECS * self.cur.rate.max(1) as f32);
                        let paused = self.shared.paused.load(Ordering::Relaxed);
                        self.fade_target = if self.stopping || paused || self.pending_seek.is_some() { 0.0 } else { 1.0 };
                    }
                    FrameRead::Buffering => {
                        self.frame[..ch].fill(0.0);
                        return true;
                    }
                    FrameRead::Eof => {
                        if !self.join_next() {
                            self.ctl.finished.store(true, Ordering::Release);
                            return false;
                        }
                        self.buffering = false;
                    }
                }
            }
            if self.fade_target <= 0.0 {
                // Paused (or between a seek and its fade-in): silence, decoder untouched.
                self.frame[..ch].fill(0.0);
                return true;
            }
        }

        loop {
            match self.cur.read_frame(&mut self.frame) {
                FrameRead::Ready => break,
                FrameRead::Buffering => {
                    self.buffering = true;
                    self.fade = 0.0;
                    self.fade_target = 0.0;
                    self.frame[..ch].fill(0.0);
                    return true;
                }
                FrameRead::Eof => {
                    if !self.join_next() {
                        self.ctl.finished.store(true, Ordering::Release);
                        return false;
                    }
                }
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
