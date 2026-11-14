//! The engine: a thread that owns the output device (the OS audio stream cannot leave the thread
//! that opened it), builds decoders on request and reports what happened.
//!
//! Pause, seek, volume and EQ do not go through the thread at all — they flip atomics the audio
//! callback reads — so they take effect within a few milliseconds and never block the UI.

use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rodio::{DeviceSinkBuilder, MixerDeviceSink};

use crate::analyzer::Analyzer;
use crate::eq::EqSettings;
use crate::source::{NO_SEEK, SWITCH_FADE_SECS, Playback, Shared, SourceCtl, Track};
use crate::{AudioError, Event, LoadHint, PlaybackState};

/// How often the thread checks, while a track is playing, whether the source ended or moved on to
/// the queued track. Nothing is polled when the engine is idle.
const POLL: Duration = Duration::from_millis(15);

enum Cmd {
    Load { token: u64, audio: Arc<[u8]>, hint: LoadHint, play: bool, start: Duration },
    Enqueue { token: u64, audio: Arc<[u8]>, hint: LoadHint },
    Unqueue,
    Stop,
    Shutdown,
}

pub struct Engine {
    tx: Sender<Cmd>,
    shared: Arc<Shared>,
    analyzer: Mutex<Analyzer>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Engine {
    /// Opens the default output device. `on_event` is called on the engine thread, so it should
    /// hand the event over (to a channel, an `AppHandle`, ...) and return quickly.
    pub fn new(on_event: impl Fn(Event) + Send + Sync + 'static) -> Result<Engine, AudioError> {
        let shared = Arc::new(Shared::default());
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let on_event: Arc<dyn Fn(Event) + Send + Sync> = Arc::new(on_event);
        let thread = thread::Builder::new()
            .name("ym-audio".into())
            .spawn({
                let shared = shared.clone();
                move || run(rx, shared, on_event, ready_tx)
            })
            .map_err(|e| AudioError::Device(e.to_string()))?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Engine { tx, shared, analyzer: Mutex::new(Analyzer::new()), thread: Mutex::new(Some(thread)) }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err(AudioError::Device("the audio thread stopped unexpectedly".into()))
            }
        }
    }

    fn send(&self, cmd: Cmd) {
        // A closed channel means the engine is shutting down; nothing useful to report.
        let _ = self.tx.send(cmd);
    }

    /// Starts `audio` (a whole MP3/AAC/WAV file) as the current track, replacing the previous one
    /// with a short fade. Returns at once; a decode failure arrives as [`Event::Error`].
    /// `token` names the track in every later event and in [`PlaybackState`] (use any non-zero id).
    pub fn load(&self, token: u64, audio: Arc<[u8]>, hint: LoadHint, play: bool, start: Duration) {
        self.send(Cmd::Load { token: token.max(1), audio, hint, play, start });
    }

    /// Queues the track that should follow the current one without a gap. At most one is kept;
    /// queueing again replaces it. The engine announces the hand-over with [`Event::Advanced`].
    pub fn enqueue(&self, token: u64, audio: Arc<[u8]>, hint: LoadHint) {
        self.send(Cmd::Enqueue { token: token.max(1), audio, hint });
    }

    /// Forgets the queued track (the user changed the queue).
    pub fn unqueue(&self) {
        self.send(Cmd::Unqueue);
    }

    pub fn stop(&self) {
        self.send(Cmd::Stop);
    }

    pub fn set_paused(&self, paused: bool) {
        self.shared.paused.store(paused, Ordering::Relaxed);
    }

    pub fn play(&self) {
        self.set_paused(false);
    }

    pub fn pause(&self) {
        self.set_paused(true);
    }

    /// Jumps within the current track.
    pub fn seek(&self, to: Duration) {
        let us = u64::try_from(to.as_micros()).unwrap_or(u64::MAX - 1).min(NO_SEEK - 1);
        self.shared.pos_us.store(us, Ordering::Relaxed);
        self.shared.seek_us.store(us, Ordering::Relaxed);
    }

    /// Linear gain; 1.0 is unchanged. Values above 1 are allowed (up to 1.5) for quiet masters.
    pub fn set_volume(&self, gain: f32) {
        let gain = if gain.is_finite() { gain.clamp(0.0, 1.5) } else { 1.0 };
        self.shared.volume.store(gain.to_bits(), Ordering::Relaxed);
    }

    pub fn volume(&self) -> f32 {
        f32::from_bits(self.shared.volume.load(Ordering::Relaxed))
    }

    pub fn set_eq(&self, settings: &EqSettings) {
        self.shared.eq.set(settings);
    }

    pub fn eq(&self) -> EqSettings {
        self.shared.eq.get()
    }

    pub fn state(&self) -> PlaybackState {
        let token = self.shared.token.load(Ordering::Acquire);
        let duration = self.shared.dur_us.load(Ordering::Relaxed);
        PlaybackState {
            token: (token != 0).then_some(token),
            paused: self.shared.paused.load(Ordering::Relaxed),
            position: Duration::from_micros(self.shared.pos_us.load(Ordering::Relaxed)),
            duration: (duration != 0).then(|| Duration::from_micros(duration)),
        }
    }

    pub fn is_playing(&self) -> bool {
        let s = self.state();
        s.token.is_some() && !s.paused
    }

    /// Fills `out` with spectrum bands in 0..1 for the audio playing right now. Call once per UI
    /// frame; the bands are already smoothed.
    pub fn spectrum(&self, out: &mut [f32]) {
        let playing = self.is_playing();
        self.analyzer.lock().unwrap_or_else(PoisonError::into_inner).bands(&self.shared.tap, playing, out);
    }

    /// What the music is doing tonally (as of the last [`Engine::spectrum`] call).
    pub fn tone(&self) -> crate::Tone {
        self.analyzer.lock().unwrap_or_else(PoisonError::into_inner).tone()
    }

    /// Smoothed loudness in 0..1 (as of the last [`Engine::spectrum`] call).
    pub fn level(&self) -> f32 {
        self.analyzer.lock().unwrap_or_else(PoisonError::into_inner).level()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.send(Cmd::Shutdown);
        if let Some(t) = self.thread.lock().unwrap_or_else(PoisonError::into_inner).take() {
            let _ = t.join();
        }
    }
}

struct Live {
    ctl: Arc<SourceCtl>,
    token: u64,
    seen_joins: u32,
}

fn open_sink(on_event: &Arc<dyn Fn(Event) + Send + Sync>) -> Result<MixerDeviceSink, AudioError> {
    let device_err = |e: &dyn std::fmt::Display| AudioError::Device(e.to_string());
    let builder = DeviceSinkBuilder::from_default_device().map_err(|e| device_err(&e))?.with_error_callback({
        let on_event = on_event.clone();
        move |err| on_event(Event::DeviceError(err.to_string()))
    });
    let mut sink = builder.open_sink_or_fallback().map_err(|e| device_err(&e))?;
    sink.log_on_drop(false);
    Ok(sink)
}

/// Starts `track`, fading out whatever played before.
fn start_playback(sink: &MixerDeviceSink, shared: &Arc<Shared>, live: &mut Option<Live>, track: Track, start: Duration, play: bool) {
    let old = live.take();
    shared.paused.store(!play, Ordering::Relaxed);
    shared.seek_us.store(NO_SEEK, Ordering::Relaxed);
    shared.tap.clear();
    shared.token.store(track.token, Ordering::Release);
    shared.dur_us.store(track.duration.map_or(0, |d| d.as_micros() as u64), Ordering::Relaxed);
    let (token, ctl) = (track.token, Arc::new(SourceCtl::new()));
    sink.mixer().add(Playback::new(track, ctl.clone(), shared.clone(), start, SWITCH_FADE_SECS));
    if let Some(old) = old {
        old.ctl.stop_with_fade(SWITCH_FADE_SECS);
    }
    *live = Some(Live { ctl, token, seen_joins: 0 });
}

fn run(rx: mpsc::Receiver<Cmd>, shared: Arc<Shared>, on_event: Arc<dyn Fn(Event) + Send + Sync>, ready: mpsc::SyncSender<Result<(), AudioError>>) {
    let sink = match open_sink(&on_event) {
        Ok(sink) => sink,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let _ = ready.send(Ok(()));
    let emit = |e: Event| on_event(e);
    let mut live: Option<Live> = None;

    loop {
        // While a track plays, poll often enough to notice it ending or handing over to the
        // queued one. With nothing playing there is nothing to look at, so block on the channel
        // instead of waking up every 15 ms for nothing.
        let cmd = if live.is_some() {
            match rx.recv_timeout(POLL) {
                Ok(cmd) => Some(cmd),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(cmd) => Some(cmd),
                Err(_) => break,
            }
        };
        match cmd {
            Some(Cmd::Shutdown) => break,
            None => {}
            Some(Cmd::Load { token, audio, hint, play, start }) => match Track::open(token, audio, &hint) {
                Ok(track) => start_playback(&sink, &shared, &mut live, track, start, play),
                Err(e) => {
                    // The old track must not keep playing under the name of the one that failed.
                    if let Some(old) = live.take() {
                        old.ctl.stop_with_fade(0.012);
                    }
                    shared.token.store(0, Ordering::Release);
                    shared.pos_us.store(0, Ordering::Relaxed);
                    emit(Event::Error { token, message: e.to_string() });
                }
            },
            Some(Cmd::Enqueue { token, audio, hint }) => match &live {
                None => emit(Event::Error { token, message: "nothing is playing to queue a track after".into() }),
                Some(l) => match Track::open(token, audio, &hint) {
                    Ok(track) => *l.ctl.lock_next() = Some(track),
                    Err(e) => emit(Event::Error { token, message: e.to_string() }),
                },
            },
            Some(Cmd::Unqueue) => {
                if let Some(l) = &live {
                    l.ctl.lock_next().take();
                }
            }
            Some(Cmd::Stop) => {
                if let Some(old) = live.take() {
                    old.ctl.stop_with_fade(0.012);
                }
                shared.token.store(0, Ordering::Release);
                shared.pos_us.store(0, Ordering::Relaxed);
            }
        }

        // Did the source move on to the queued track, or run out?
        let Some(l) = live.as_mut() else { continue };
        let joins = l.ctl.joins.load(Ordering::Acquire);
        if joins != l.seen_joins {
            l.seen_joins = joins;
            l.token = shared.token.load(Ordering::Acquire);
            emit(Event::Advanced { token: l.token });
        }
        if l.ctl.finished.load(Ordering::Acquire) {
            let ended = l.token;
            let leftover = l.ctl.lock_next().take();
            live = None;
            match leftover {
                // A queued track in a different format could not be joined sample-exactly:
                // start it now (a gap of a few milliseconds, but no lost track).
                Some(track) => {
                    let token = track.token;
                    let play = !shared.paused.load(Ordering::Relaxed);
                    start_playback(&sink, &shared, &mut live, track, Duration::ZERO, play);
                    emit(Event::Advanced { token });
                }
                None => {
                    shared.token.store(0, Ordering::Release);
                    emit(Event::Ended { token: ended });
                }
            }
        }
    }

    if let Some(l) = live {
        l.ctl.stop_with_fade(0.012);
        // Let the fade-out play so the stream does not end on a click.
        thread::sleep(Duration::from_millis(40));
    }
    drop(sink);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wav::fixtures::sine;
    use std::time::Instant;

    /// Tests that need a sound card skip themselves (loudly) where there is none, e.g. CI.
    fn engine() -> Option<(Engine, mpsc::Receiver<Event>)> {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        match Engine::new(move |e| {
            let _ = tx.lock().unwrap().send(e);
        }) {
            Ok(engine) => {
                engine.set_volume(0.02); // a whisper: these tests really play
                Some((engine, rx))
            }
            Err(e) => {
                eprintln!("SKIPPED (no audio device): {e}");
                None
            }
        }
    }

    fn wait_for(rx: &mpsc::Receiver<Event>, secs: u64, want: impl Fn(&Event) -> bool) -> Option<Event> {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(e) if want(&e) => return Some(e),
                Ok(_) => {}
                Err(_) => break,
            }
        }
        None
    }

    #[test]
    fn plays_a_track_to_its_end() {
        let Some((engine, rx)) = engine() else { return };
        engine.load(7, sine(440.0, 0.6, 44_100, 2), LoadHint::default(), true, Duration::ZERO);
        let ended = wait_for(&rx, 6, |e| matches!(e, Event::Ended { .. }));
        assert_eq!(ended, Some(Event::Ended { token: 7 }));
        assert!(engine.state().position >= Duration::from_millis(450), "{:?}", engine.state());
        assert_eq!(engine.state().token, None);
    }

    #[test]
    fn queued_track_follows_gaplessly_and_is_announced() {
        let Some((engine, rx)) = engine() else { return };
        engine.load(1, sine(440.0, 0.5, 44_100, 2), LoadHint::default(), true, Duration::ZERO);
        engine.enqueue(2, sine(660.0, 0.5, 44_100, 2), LoadHint::default());
        assert_eq!(wait_for(&rx, 6, |e| matches!(e, Event::Advanced { .. })), Some(Event::Advanced { token: 2 }));
        assert_eq!(engine.state().token, Some(2));
        assert_eq!(wait_for(&rx, 6, |e| matches!(e, Event::Ended { .. })), Some(Event::Ended { token: 2 }));
    }

    #[test]
    fn a_queued_track_in_another_format_still_plays() {
        let Some((engine, rx)) = engine() else { return };
        engine.load(1, sine(440.0, 0.4, 44_100, 2), LoadHint::default(), true, Duration::ZERO);
        engine.enqueue(2, sine(440.0, 0.4, 22_050, 1), LoadHint::default());
        assert_eq!(wait_for(&rx, 6, |e| matches!(e, Event::Advanced { .. })), Some(Event::Advanced { token: 2 }));
        assert_eq!(wait_for(&rx, 6, |e| matches!(e, Event::Ended { .. })), Some(Event::Ended { token: 2 }));
    }

    #[test]
    fn loading_replaces_the_current_track() {
        let Some((engine, rx)) = engine() else { return };
        engine.load(1, sine(440.0, 30.0, 44_100, 2), LoadHint::default(), true, Duration::ZERO);
        std::thread::sleep(Duration::from_millis(150));
        engine.load(2, sine(660.0, 0.3, 44_100, 2), LoadHint::default(), true, Duration::ZERO);
        assert_eq!(wait_for(&rx, 6, |e| matches!(e, Event::Ended { .. })), Some(Event::Ended { token: 2 }), "the replaced track must not report an end");
    }

    #[test]
    fn paused_load_waits_and_seek_moves_the_position() {
        let Some((engine, _rx)) = engine() else { return };
        engine.load(5, sine(440.0, 30.0, 44_100, 2), LoadHint::default(), false, Duration::ZERO);
        std::thread::sleep(Duration::from_millis(200));
        let s = engine.state();
        assert!(s.paused && s.position < Duration::from_millis(50), "{s:?}");
        assert!(s.duration.is_some_and(|d| d > Duration::from_secs(29)), "{s:?}");
        engine.seek(Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(300));
        let p = engine.state().position;
        assert!(p >= Duration::from_millis(9_900) && p < Duration::from_millis(10_500), "{p:?}");
        engine.play();
        std::thread::sleep(Duration::from_millis(400));
        assert!(engine.state().position > Duration::from_millis(10_200));
        engine.stop();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(engine.state().token, None);
    }

    #[test]
    fn start_offset_resumes_mid_track() {
        let Some((engine, _rx)) = engine() else { return };
        engine.load(5, sine(440.0, 30.0, 44_100, 2), LoadHint::default(), false, Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(200));
        let p = engine.state().position;
        assert!(p >= Duration::from_millis(19_900) && p < Duration::from_millis(20_300), "{p:?}");
    }

    #[test]
    fn undecodable_audio_reports_an_error_and_leaves_the_engine_usable() {
        let Some((engine, rx)) = engine() else { return };
        engine.load(9, Arc::from(vec![0u8; 64]), LoadHint::default(), true, Duration::ZERO);
        assert!(matches!(wait_for(&rx, 4, |e| matches!(e, Event::Error { .. })), Some(Event::Error { token: 9, .. })));
        engine.load(10, sine(440.0, 0.3, 44_100, 2), LoadHint::default(), true, Duration::ZERO);
        assert_eq!(wait_for(&rx, 6, |e| matches!(e, Event::Ended { .. })), Some(Event::Ended { token: 10 }));
    }

    #[test]
    fn a_failed_load_stops_the_track_that_was_playing() {
        let Some((engine, rx)) = engine() else { return };
        engine.load(1, sine(440.0, 30.0, 44_100, 2), LoadHint::default(), true, Duration::ZERO);
        std::thread::sleep(Duration::from_millis(200));
        engine.load(2, Arc::from(vec![0u8; 64]), LoadHint::default(), true, Duration::ZERO);
        assert!(matches!(wait_for(&rx, 4, |e| matches!(e, Event::Error { token: 2, .. })), Some(Event::Error { .. })));
        assert_eq!(engine.state().token, None);
        // The old track fades out and stays stopped: its clock must not keep running.
        std::thread::sleep(Duration::from_millis(150));
        let stopped = engine.state().position;
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(engine.state().position, stopped, "the old track must not keep playing");
    }

    #[test]
    fn queueing_with_nothing_playing_is_reported() {
        let Some((engine, rx)) = engine() else { return };
        engine.enqueue(3, sine(440.0, 0.3, 44_100, 2), LoadHint::default());
        assert!(matches!(wait_for(&rx, 4, |e| matches!(e, Event::Error { .. })), Some(Event::Error { token: 3, .. })));
    }

    #[test]
    fn spectrum_reacts_to_playing_audio() {
        let Some((engine, _rx)) = engine() else { return };
        engine.set_volume(0.0); // silent, yet the visuals must still see the music
        engine.load(1, sine(1_000.0, 3.0, 44_100, 2), LoadHint::default(), true, Duration::ZERO);
        std::thread::sleep(Duration::from_millis(500));
        let mut bands = [0.0f32; 24];
        for _ in 0..30 {
            engine.spectrum(&mut bands);
            std::thread::sleep(Duration::from_millis(16));
        }
        assert!(bands.iter().copied().fold(0.0, f32::max) > 0.4, "{bands:?}");
        assert!(engine.level() > 0.2);
        engine.stop();
    }

    #[test]
    fn dropping_the_engine_shuts_it_down_cleanly() {
        let Some((engine, _rx)) = engine() else { return };
        engine.load(1, sine(440.0, 30.0, 44_100, 2), LoadHint::default(), true, Duration::ZERO);
        std::thread::sleep(Duration::from_millis(100));
        let t = Instant::now();
        drop(engine);
        assert!(t.elapsed() < Duration::from_secs(2));
    }
}
