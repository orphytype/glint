//! Progressive on-disk stream buffer for audio playback: the decoder reads bytes as they arrive
//! from the network while a background task writes incoming chunks to an OS-managed temporary file.
//!
//! Process RAM stays constant (one 64 KiB read cache per open reader) regardless of track length,
//! and seeks into un-downloaded regions request an HTTP Range jump from the producer.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// How long a reader waits for new bytes before treating the download as stalled.
const STALL_TIMEOUT: Duration = Duration::from_secs(60);
/// Size of the per-reader in-memory read cache (avoids mutex/syscall overhead on small reads).
const READER_CACHE_BYTES: usize = 64 * 1024;
/// How far ahead of the sequential write cursor a seek must land before requesting an HTTP Range
/// jump instead of waiting for the sequential stream to catch up.
const RANGE_JUMP_THRESHOLD: u64 = 512 * 1024;

type WakeFn = Box<dyn Fn() + Send + Sync>;

struct StreamState {
    /// Sorted, disjoint, merged `[start, end)` byte intervals already written to disk.
    ranges: Vec<(u64, u64)>,
    /// Known total byte length of the audio file (from `Content-Length` or stream metadata).
    len: Option<u64>,
    /// True once the writer hit EOF at a known byte offset.
    eof_at: Option<u64>,
    /// Whether the server accepts HTTP Range requests for mid-track jumps.
    seekable: bool,
    /// Current sequential write position of the background producer.
    writer_pos: u64,
    /// Pending jump target requested by a reader that sought outside downloaded ranges.
    seek_req: Option<u64>,
    /// Producer finished (complete, cancelled, or fatal error).
    done: bool,
    /// Readers have all dropped or playback switched away.
    cancelled: bool,
    /// Fatal download error after retries were exhausted.
    err: Option<String>,
}

impl StreamState {
    fn insert_range(&mut self, start: u64, end: u64) {
        if end <= start {
            return;
        }
        let pos = self.ranges.partition_point(|r| r.0 < start);
        self.ranges.insert(pos, (start, end));
        let mut merged: Vec<(u64, u64)> = Vec::with_capacity(self.ranges.len());
        for (s, e) in self.ranges.drain(..) {
            if let Some(last) = merged.last_mut()
                && s <= last.1
            {
                last.1 = last.1.max(e);
            } else {
                merged.push((s, e));
            }
        }
        self.ranges = merged;
    }

    /// How many contiguous bytes are available on disk starting at `offset`.
    fn available_from(&self, offset: u64) -> u64 {
        for &(s, e) in &self.ranges {
            if offset < s {
                return 0;
            }
            if offset < e {
                return e - offset;
            }
        }
        0
    }

    /// Highest downloaded byte offset (used as a fallback byte length when `len` is unknown).
    fn max_end(&self) -> u64 {
        self.ranges.last().map_or(0, |r| r.1)
    }

    /// True when the entire file `[0, total)` is present on disk with no gaps and no error.
    fn is_complete(&self) -> bool {
        if self.err.is_some() || self.cancelled {
            return false;
        }
        match self.eof_at.or(self.len) {
            Some(total) if total > 0 => self.ranges.first().is_some_and(|&(s, e)| s == 0 && e >= total),
            _ => self.done && !self.ranges.is_empty(),
        }
    }
}

/// Shared disk-backed progressive stream buffer.
pub struct StreamBuf {
    file: File,
    state: Mutex<StreamState>,
    cv: Condvar,
    aborted: AtomicBool,
    wake: Mutex<Option<WakeFn>>,
}

impl StreamBuf {
    /// Creates a new OS-managed temporary stream file and returns `(handle, writer)`.
    ///
    /// When every clone of `handle` (and every open reader) drops, the stream is cancelled
    /// automatically so the background downloader stops immediately.
    pub fn create(len: Option<u64>, seekable: bool) -> io::Result<(Arc<StreamHandle>, StreamWriter)> {
        let file = open_temp_file()?;
        let buf = Arc::new(StreamBuf {
            file,
            state: Mutex::new(StreamState {
                ranges: Vec::new(),
                len: len.filter(|&n| n > 0),
                eof_at: None,
                seekable: seekable && len.is_some_and(|n| n > 0),
                writer_pos: 0,
                seek_req: None,
                done: false,
                cancelled: false,
                err: None,
            }),
            cv: Condvar::new(),
            aborted: AtomicBool::new(false),
            wake: Mutex::new(None),
        });
        let handle = Arc::new(StreamHandle { buf: buf.clone() });
        let writer = StreamWriter { buf };
        Ok((handle, writer))
    }

    fn lock(&self) -> MutexGuard<'_, StreamState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn notify_writer(&self) {
        if let Some(f) = self.wake.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
            f();
        }
    }

    /// Cancels the stream immediately, unblocking any waiting reader and waking the producer.
    pub fn cancel(&self) {
        self.aborted.store(true, Ordering::Release);
        {
            let mut st = self.lock();
            st.cancelled = true;
            st.done = true;
        }
        self.cv.notify_all();
        self.notify_writer();
    }

    pub fn is_cancelled(&self) -> bool {
        self.aborted.load(Ordering::Acquire)
    }

    pub fn byte_len(&self) -> u64 {
        let st = self.lock();
        st.len.or(st.eof_at).unwrap_or_else(|| st.max_end())
    }

    pub fn is_complete(&self) -> bool {
        self.lock().is_complete()
    }

    /// Blocks until the stream finishes downloading cleanly (or fails/cancels). Returns `true`
    /// only if the full file `[0, len)` is on disk without gaps.
    pub fn wait_complete(&self) -> bool {
        let mut st = self.lock();
        while !st.done && !st.cancelled && !st.is_complete() {
            st = self.cv.wait(st).unwrap_or_else(PoisonError::into_inner);
        }
        st.is_complete()
    }
}

/// Lifetime token held by `AudioData` and open `StreamReader`s. Dropping the last handle cancels
/// the background download so skipped tracks never keep transferring in the background.
pub struct StreamHandle {
    pub(crate) buf: Arc<StreamBuf>,
}

impl StreamHandle {
    pub fn cancel(&self) {
        self.buf.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.buf.is_cancelled()
    }

    pub fn byte_len(&self) -> u64 {
        self.buf.byte_len()
    }

    pub fn is_complete(&self) -> bool {
        self.buf.is_complete()
    }

    pub fn wait_complete(&self) -> bool {
        self.buf.wait_complete()
    }
}

impl Drop for StreamHandle {
    fn drop(&mut self) {
        self.buf.cancel();
    }
}

/// Producer half of a [`StreamBuf`], owned by the background network download task.
pub struct StreamWriter {
    buf: Arc<StreamBuf>,
}

impl StreamWriter {
    /// Registers a callback invoked whenever a reader requests a range jump or cancels the stream
    /// (e.g. `move || notify.notify_one()` for an async tokio task).
    pub fn set_waker(&self, wake: impl Fn() + Send + Sync + 'static) {
        *self.buf.wake.lock().unwrap_or_else(PoisonError::into_inner) = Some(Box::new(wake));
        if self.is_aborted() || self.has_seek_request() {
            self.buf.notify_writer();
        }
    }

    pub fn is_aborted(&self) -> bool {
        self.buf.aborted.load(Ordering::Acquire)
    }

    pub fn has_seek_request(&self) -> bool {
        self.buf.lock().seek_req.is_some()
    }

    pub fn take_seek_request(&self) -> Option<u64> {
        let mut st = self.buf.lock();
        let req = st.seek_req.take()?;
        st.writer_pos = req;
        Some(req)
    }

    pub fn set_len(&self, len: u64) {
        if len == 0 {
            return;
        }
        let mut st = self.buf.lock();
        st.len = Some(len);
        self.buf.cv.notify_all();
    }

    pub fn set_seekable(&self, seekable: bool) {
        let mut st = self.buf.lock();
        st.seekable = seekable;
    }

    /// Writes `data` at `offset` in the backing temporary file and wakes waiting readers.
    pub fn write(&self, offset: u64, data: &[u8]) -> io::Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        write_at(&self.buf.file, data, offset)?;
        let end = offset.saturating_add(data.len() as u64);
        {
            let mut st = self.buf.lock();
            st.insert_range(offset, end);
            st.writer_pos = end;
            if st.seek_req.is_some_and(|req| st.available_from(req) > 0) {
                st.seek_req = None;
            }
        }
        self.buf.cv.notify_all();
        Ok(())
    }

    /// Records that the server reached EOF at `offset`. Returns `true` if the entire file is now
    /// complete on disk (so the writer can exit); returns `false` if earlier gaps remain and the
    /// writer should stay parked waiting for a seek request or cancellation.
    pub fn reached_eof(&self, offset: u64) -> bool {
        let mut st = self.buf.lock();
        st.eof_at = Some(offset);
        if st.len.is_none() {
            st.len = Some(offset);
        }
        let complete = st.is_complete();
        if complete {
            st.done = true;
        }
        drop(st);
        self.buf.cv.notify_all();
        complete
    }

    /// Marks the stream finished (e.g. on clean shutdown or after all bytes arrived).
    pub fn finish(&self) {
        {
            let mut st = self.buf.lock();
            st.done = true;
        }
        self.buf.cv.notify_all();
    }

    /// Marks the stream failed with `message` and wakes any waiting readers.
    pub fn fail(&self, message: String) {
        {
            let mut st = self.buf.lock();
            st.err = Some(message);
            st.done = true;
        }
        self.buf.cv.notify_all();
    }
}

impl Drop for StreamWriter {
    fn drop(&mut self) {
        let mut st = self.buf.lock();
        if !st.done {
            st.done = true;
        }
        drop(st);
        self.buf.cv.notify_all();
    }
}

/// Seekable reader over a [`StreamBuf`] with a small 64 KiB local read cache.
pub(crate) struct StreamReader {
    _keep: Arc<StreamHandle>,
    buf: Arc<StreamBuf>,
    pos: u64,
    cache: Vec<u8>,
    cache_start: u64,
    cache_len: usize,
}

impl StreamReader {
    pub fn new(handle: Arc<StreamHandle>) -> Self {
        let buf = handle.buf.clone();
        StreamReader {
            _keep: handle,
            buf,
            pos: 0,
            cache: vec![0u8; READER_CACHE_BYTES],
            cache_start: 0,
            cache_len: 0,
        }
    }

    fn wait_available(&self, offset: u64) -> io::Result<u64> {
        let mut st = self.buf.lock();
        loop {
            let avail = st.available_from(offset);
            if avail > 0 {
                return Ok(avail);
            }
            if st.cancelled {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "stream cancelled"));
            }
            if st.eof_at.is_some_and(|eof| offset >= eof) || (st.done && st.len.is_some_and(|len| offset >= len)) {
                return Ok(0);
            }
            if st.done {
                return match &st.err {
                    Some(e) => Err(io::Error::other(e.clone())),
                    None => Ok(0),
                };
            }
            if st.seekable && st.len.is_none_or(|len| offset < len) {
                let far_ahead = offset > st.writer_pos.saturating_add(RANGE_JUMP_THRESHOLD);
                let behind = offset < st.writer_pos;
                if (far_ahead || behind) && st.seek_req != Some(offset) {
                    st.seek_req = Some(offset);
                    drop(st);
                    self.buf.notify_writer();
                    st = self.buf.lock();
                    continue;
                }
            }
            let (next, timed_out) = self.buf.cv.wait_timeout(st, STALL_TIMEOUT).unwrap_or_else(PoisonError::into_inner);
            st = next;
            if timed_out.timed_out() && st.available_from(offset) == 0 && !st.done && !st.cancelled {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "stream stalled"));
            }
        }
    }
}

impl Read for StreamReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let cache_end = self.cache_start.saturating_add(self.cache_len as u64);
        if self.pos >= self.cache_start && self.pos < cache_end {
            let off = (self.pos - self.cache_start) as usize;
            let n = out.len().min(self.cache_len - off);
            out[..n].copy_from_slice(&self.cache[off..off + n]);
            self.pos += n as u64;
            return Ok(n);
        }
        let avail = self.wait_available(self.pos)?;
        if avail == 0 {
            return Ok(0);
        }
        let want = (avail as usize).min(self.cache.len());
        let read_n = read_at(&self.buf.file, &mut self.cache[..want], self.pos)?;
        if read_n == 0 {
            return Ok(0);
        }
        self.cache_start = self.pos;
        self.cache_len = read_n;
        let copy_n = out.len().min(read_n);
        out[..copy_n].copy_from_slice(&self.cache[..copy_n]);
        self.pos += copy_n as u64;
        Ok(copy_n)
    }
}

impl Seek for StreamReader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let next = match to {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::Current(d) => self.pos.checked_add_signed(d),
            SeekFrom::End(d) => {
                let len = {
                    let mut st = self.buf.lock();
                    loop {
                        if let Some(len) = st.len.or(st.eof_at) {
                            break Ok(len);
                        }
                        if st.cancelled {
                            break Err(io::Error::new(io::ErrorKind::Interrupted, "stream cancelled"));
                        }
                        if st.done {
                            break Ok(st.max_end());
                        }
                        let (guard, timed_out) = self.buf.cv.wait_timeout(st, STALL_TIMEOUT).unwrap_or_else(PoisonError::into_inner);
                        st = guard;
                        if timed_out.timed_out() && st.len.is_none() && !st.done {
                            break Err(io::Error::new(io::ErrorKind::TimedOut, "stream stalled"));
                        }
                    }
                }?;
                len.checked_add_signed(d)
            }
        };
        let pos = next.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before start of stream"))?;
        self.pos = pos;
        Ok(pos)
    }
}

fn temp_stream_dir() -> PathBuf {
    std::env::temp_dir().join("glint-stream")
}

fn open_temp_file() -> io::Result<File> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = temp_stream_dir();
    std::fs::create_dir_all(&dir)?;
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let path = dir.join(format!("live-{}-{n}.bin", std::process::id()));
    let mut opts = OpenOptions::new();
    opts.read(true).write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        const FILE_FLAG_DELETE_ON_CLOSE: u32 = 0x0400_0000;
        const FILE_SHARE_ALL: u32 = 0x7;
        opts.custom_flags(FILE_FLAG_DELETE_ON_CLOSE).share_mode(FILE_SHARE_ALL);
    }
    let file = opts.open(&path)?;
    #[cfg(unix)]
    let _ = std::fs::remove_file(&path);
    Ok(file)
}

#[cfg(unix)]
fn write_at(f: &File, data: &[u8], at: u64) -> io::Result<()> {
    std::os::unix::fs::FileExt::write_all_at(f, data, at)
}

#[cfg(windows)]
fn write_at(f: &File, mut data: &[u8], mut at: u64) -> io::Result<()> {
    while !data.is_empty() {
        let n = std::os::windows::fs::FileExt::seek_write(f, data, at)?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "failed to write stream chunk"));
        }
        data = &data[n..];
        at += n as u64;
    }
    Ok(())
}

#[cfg(unix)]
fn read_at(f: &File, out: &mut [u8], at: u64) -> io::Result<usize> {
    std::os::unix::fs::FileExt::read_at(f, out, at)
}

#[cfg(windows)]
fn read_at(f: &File, out: &mut [u8], at: u64) -> io::Result<usize> {
    std::os::windows::fs::FileExt::seek_read(f, out, at)
}
