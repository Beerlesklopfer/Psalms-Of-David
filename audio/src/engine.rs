// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The playback engine as the caller sees it: commands in, state out.
//!
//! An `Engine` owns one engine thread, started by an [`Output`] (PipeWire, or the null output for tests). The
//! thread runs the portable [`crate::core::Core`]; this module is the thread-safe handle to it: commands go
//! through a channel, state is read from atomics. See `docs/spec-audio-pipewire.md`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::Error;
use crate::core::{Info, Track};
use crate::output::null::{NullCapture, NullOptions, NullOutput};
use crate::output::pipewire::PipewireOutput;
use crate::output::{CommandSink, Output};
use crate::stream::{self, ByteQueue, PushError};

/// The buffer, in seconds of audio, when nobody has set it (`Engine::set_buffer_seconds`).
pub const DEFAULT_BUFFER_SECONDS: u32 = 2;
/// The range of the buffer, in seconds.
pub const BUFFER_SECONDS: std::ops::RangeInclusive<u32> = 1..=30;
pub(crate) const UNKNOWN: u64 = u64::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Nothing loaded.
    Idle,
    Playing,
    Paused,
    /// The file played to its end.
    Ended,
    /// Loading or decoding failed; see `Engine::error`.
    Failed,
    /// A live stream is collecting audio before it (re)starts; it plays by itself.
    Buffering,
    /// A file is being opened (or a request is on its way); the state becomes `Playing`, `Paused` or `Failed` when it is done.
    Loading,
}

impl State {
    pub(crate) fn from_u8(v: u8) -> State {
        match v {
            1 => State::Playing,
            2 => State::Paused,
            3 => State::Ended,
            4 => State::Failed,
            5 => State::Buffering,
            6 => State::Loading,
            _ => State::Idle,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct EngineOptions {
    /// Node name or object serial of the sink. `None` lets the session manager choose.
    pub target: Option<String>,
    /// The role of the stream (`media.role`). Default `Music`, or the value of the
    /// environment variable `PSALM_AUDIO_ROLE`.
    ///
    /// The role matters for more than the name: WirePlumber remembers the volume of
    /// a stream *per role*, so every program that plays as `Music` shares one
    /// remembered volume. The checks in `scripts/` use another role so that they can
    /// never change the volume of real programs.
    pub role: Option<String>,
}

/// What the engine thread publishes and the control side reads. Plain atomics.
pub struct Shared {
    state: AtomicU8,
    /// The volume the stream has (as last set or reported by the output), as f32 bits.
    pub(crate) volume: Arc<AtomicU32>,
    /// Has the caller ever set a volume? If not, the engine leaves the volume to the
    /// system, which may restore one it remembers.
    pub(crate) volume_set: AtomicBool,
    /// The playback speed as f32 bits; the decoder thread stretches the audio to it.
    pub(crate) speed: AtomicU32,
    /// Seconds of audio the ring between the decoder and the output holds (applies from the next load).
    pub(crate) buffer_s: AtomicU32,
    pub(crate) error: Mutex<Option<String>>,
    /// The id of the last load request (ids start at 1).
    requests: AtomicU64,
    /// The id of the load that has not finished yet, 0 if none. While it is set the state is `Loading`.
    pub(crate) pending: AtomicU64,
    /// How many times the engine moved on to the queued next file by itself (gapless), over its whole life.
    pub(crate) advanced: Arc<AtomicU64>,
    /// The counters of the current session.
    track: Mutex<Arc<Track>>,
}

impl Shared {
    fn new() -> Shared {
        let advanced = Arc::new(AtomicU64::new(0));
        Shared {
            state: AtomicU8::new(State::Idle as u8),
            volume: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            volume_set: AtomicBool::new(false),
            speed: AtomicU32::new(1.0f32.to_bits()),
            buffer_s: AtomicU32::new(DEFAULT_BUFFER_SECONDS),
            error: Mutex::new(None),
            requests: AtomicU64::new(0),
            pending: AtomicU64::new(0),
            track: Mutex::new(Track::new(0, 0, UNKNOWN, advanced.clone())),
            advanced,
        }
    }

    pub(crate) fn set_state(&self, s: State) {
        self.state.store(s as u8, Ordering::Release);
    }

    /// The state the engine thread last set (without the `Loading` of a request that is still open).
    pub(crate) fn stored_state(&self) -> State {
        State::from_u8(self.state.load(Ordering::Acquire))
    }

    pub(crate) fn state(&self) -> State {
        if self.pending.load(Ordering::Acquire) != 0 { State::Loading } else { self.stored_state() }
    }

    /// The load with `id` is over (a newer one that has been requested since stays pending).
    pub(crate) fn finish_request(&self, id: u64) {
        if id != 0 {
            let _ = self.pending.compare_exchange(id, 0, Ordering::AcqRel, Ordering::Acquire);
        }
    }

    pub(crate) fn volume_arc(&self) -> Arc<AtomicU32> {
        self.volume.clone()
    }

    pub(crate) fn track(&self) -> Arc<Track> {
        self.track.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub(crate) fn new_track(&self, start_ms: u64, rate: u32, duration_ms: u64) -> Arc<Track> {
        Track::new(start_ms, rate, duration_ms, self.advanced.clone())
    }

    pub(crate) fn set_track(&self, track: Arc<Track>) {
        *self.track.lock().unwrap_or_else(|e| e.into_inner()) = track;
    }
}

pub enum Command {
    Load { id: u64, path: PathBuf, start: Duration, play: bool, media_name: String },
    /// The worker that opens a file is done (`token` says which opening).
    Opened { token: u64, result: Result<(crate::source::Source, Vec<f32>), Error> },
    /// The worker that opens the queued next file is done.
    NextOpened { token: u64, result: Result<(crate::source::Source, Vec<f32>), Error> },
    EnqueueNext { path: PathBuf },
    ClearNext,
    Play,
    Pause,
    LoadStream { queue: Arc<ByteQueue>, play: bool, media_name: String },
    Seek(Duration),
    Volume(f32),
    Stop,
    Shutdown,
}

pub struct Engine {
    sink: CommandSink,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    /// The byte queue of the live stream in use, if any.
    stream: Mutex<Option<Arc<ByteQueue>>>,
    /// What the null output collected, if this engine uses it.
    null: Option<Arc<NullCapture>>,
}

impl Engine {
    /// Connects to PipeWire on a new thread. Fails if PipeWire is not running.
    pub fn start(options: EngineOptions) -> Result<Engine, Error> {
        Engine::start_with(Box::new(PipewireOutput { target: options.target, role: options.role }), None)
    }

    /// An engine with the null output: no sound card, no PipeWire. The samples that would be played are collected
    /// ([`Engine::null_capture`]). For tests, and for drivers that only need the decoded audio.
    pub fn start_null(options: NullOptions) -> Result<Engine, Error> {
        let capture = Arc::new(NullCapture::default());
        Engine::start_with(Box::new(NullOutput { options, capture: capture.clone() }), Some(capture))
    }

    fn start_with(output: Box<dyn Output>, null: Option<Arc<NullCapture>>) -> Result<Engine, Error> {
        let shared = Arc::new(Shared::new());
        let started = output.start(shared.clone())?;
        Ok(Engine { sink: started.sink, shared, thread: Some(started.thread), stream: Mutex::new(None), null })
    }

    /// The samples collected by the null output; `None` for an engine with a real output.
    pub fn null_capture(&self) -> Option<Arc<NullCapture>> {
        self.null.clone()
    }

    fn send(&self, cmd: Command) {
        // The only failure is a stopped loop, and then there is nothing to command.
        let _ = (self.sink)(cmd);
    }

    /// Opens a file and starts at `start`, playing or paused. Returns at once with the id of the request: the file is opened by
    /// a worker thread, the state is `Loading` until it is done and then `Playing`, `Paused` or `Failed` (see [`Engine::error`]).
    /// A newer `load` (or `stop`) replaces one that has not finished. The position is `start` and the length unknown meanwhile.
    pub fn load(&self, path: &Path, start: Duration, play: bool, media_name: &str) -> Result<u64, Error> {
        self.drop_stream();
        let id = self.shared.requests.fetch_add(1, Ordering::AcqRel) + 1;
        *self.shared.error.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.shared.set_track(self.shared.new_track(start.as_millis() as u64, 0, UNKNOWN));
        self.shared.pending.store(id, Ordering::Release);
        if (self.sink)(Command::Load { id, path: path.to_owned(), start, play, media_name: media_name.to_owned() }) {
            Ok(id)
        } else {
            self.shared.finish_request(id);
            Err(Error::PipeWire("the audio thread is not running".into()))
        }
    }

    fn drop_stream(&self) {
        if let Some(q) = self.stream.lock().unwrap_or_else(|e| e.into_inner()).take() {
            q.close();
        }
    }

    fn queue(&self) -> Result<Arc<ByteQueue>, Error> {
        self.stream.lock().unwrap_or_else(|e| e.into_inner()).clone().ok_or_else(|| Error::Decode("no stream is open".into()))
    }

    /// Opens a live stream (MP3) that the caller feeds with [`Engine::stream_push`].
    /// Returns at once: the output stream is built when enough data has arrived to
    /// know the format, and the state is `Buffering` until then. Errors in the data
    /// show up later as the state `Failed` and [`Engine::error`]. Returns the id of the request.
    pub fn load_stream(&self, play: bool, media_name: &str) -> Result<u64, Error> {
        self.drop_stream();
        let id = self.shared.requests.fetch_add(1, Ordering::AcqRel) + 1;
        self.shared.pending.store(0, Ordering::Release);
        *self.shared.error.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.shared.set_track(self.shared.new_track(0, 0, UNKNOWN));
        let queue = ByteQueue::new(stream::QUEUE_BYTES);
        *self.stream.lock().unwrap_or_else(|e| e.into_inner()) = Some(queue.clone());
        if (self.sink)(Command::LoadStream { queue, play, media_name: media_name.to_owned() }) {
            Ok(id)
        } else {
            Err(Error::PipeWire("the audio thread is not running".into()))
        }
    }

    /// Offers stream bytes; returns how many were taken. Fewer than offered (or 0)
    /// means the queue is full: wait a moment and offer the rest again. Never blocks.
    pub fn stream_push(&self, data: &[u8]) -> Result<usize, Error> {
        self.queue()?.push(data).map_err(|PushError::Closed| Error::Decode("the stream is closed".into()))
    }

    /// The stream is over; what was pushed is still played, then the state is `Ended`.
    pub fn stream_end(&self) -> Result<(), Error> {
        self.queue()?.end();
        Ok(())
    }

    /// The stream failed on the sender's side: the state becomes `Failed` with `message`.
    pub fn stream_fail(&self, message: &str) -> Result<(), Error> {
        self.queue()?.fail(message);
        Ok(())
    }

    pub fn play(&self) {
        self.send(Command::Play);
    }

    pub fn pause(&self) {
        self.send(Command::Pause);
    }

    /// Jumps to `to`. Keeps playing or staying paused, whichever the engine was.
    /// A live stream cannot seek: the call does nothing.
    pub fn seek(&self, to: Duration) {
        self.send(Command::Seek(to));
    }

    /// 0.0 (silent) to 1.0 (full). It is the volume of this stream, so it shows up
    /// in mixers such as pavucontrol, and the system remembers it for the role of the
    /// stream. `volume()` returns the new value at once; it changes again if PipeWire
    /// reports another one (a mixer was used, or the system restored a remembered one).
    pub fn set_volume(&self, volume: f32) {
        let volume = volume.clamp(0.0, 1.0);
        self.shared.volume.store(volume.to_bits(), Ordering::Release);
        self.shared.volume_set.store(true, Ordering::Release);
        self.send(Command::Volume(volume));
    }

    /// Playback speed from 0.5 to 3.0 (1.0 is normal), with the pitch kept. It takes
    /// effect within about two seconds, the audio that is already queued; `position()`
    /// stays right across the change.
    pub fn set_speed(&self, speed: f32) {
        let speed = if speed.is_finite() { speed.clamp(0.5, 3.0) } else { 1.0 };
        self.shared.speed.store(speed.to_bits(), Ordering::Release);
    }

    /// How many seconds of audio the engine keeps ready between the decoder and the output (1 to 30, 2 if never set). A bigger buffer
    /// bridges a slow disk or network, a smaller one lets a change of track be heard sooner. A live stream holds four times as much
    /// and starts, and starts again after running dry, when one buffer is full. It applies from the next load; what plays is not
    /// changed. The ring is thrown away at a change of track, so the size never delays one.
    pub fn set_buffer_seconds(&self, seconds: u32) {
        let seconds = seconds.clamp(*BUFFER_SECONDS.start(), *BUFFER_SECONDS.end());
        self.shared.buffer_s.store(seconds, Ordering::Release);
    }

    pub fn buffer_seconds(&self) -> u32 {
        self.shared.buffer_s.load(Ordering::Acquire)
    }

    pub fn speed(&self) -> f32 {
        f32::from_bits(self.shared.speed.load(Ordering::Acquire))
    }

    pub fn stop(&self) {
        self.drop_stream();
        self.shared.pending.store(0, Ordering::Release);
        self.send(Command::Stop);
    }

    pub fn state(&self) -> State {
        self.shared.state()
    }

    /// Where the listening is. Counts the audio handed to the output, so it
    /// runs ahead of what is audible by the latency of the device (tens of milliseconds).
    pub fn position(&self) -> Duration {
        self.now().0
    }

    /// Length of the file, if its container says so.
    pub fn duration(&self) -> Option<Duration> {
        self.now().1
    }

    /// Position and length of the file that is being played (after a gapless switch: of the new one).
    fn now(&self) -> (Duration, Option<Duration>) {
        let track = self.shared.track();
        let rate = u64::from(track.rate).max(1);
        let played = track.frames.load(Ordering::Acquire);
        let (source, info) = track.marks.lock().map_or((played, Info { base_ms: 0, duration_ms: UNKNOWN, index: 0 }), |mut m| m.at(played));
        let position = Duration::from_millis(info.base_ms + source * 1000 / rate);
        match info.duration_ms {
            UNKNOWN => (position, None),
            ms => {
                let d = Duration::from_millis(ms);
                (position.min(d), Some(d))
            }
        }
    }

    /// How many times the engine has moved on to the file queued with [`Engine::enqueue_next`] by itself (gapless) since it was
    /// created. It counts when the first sample of the new file has been handed to the output. A load, seek or stop does not count.
    pub fn track_generation(&self) -> u64 {
        // Passing a splice is noticed when the marks are read.
        let _ = self.now();
        self.shared.advanced.load(Ordering::Acquire)
    }

    /// Queues the file that follows the loaded one. When the loaded one ends, the engine continues with it at the exact sample
    /// (no silence, no repeated or lost sample; the encoder delay and padding of MP3 are trimmed) and counts it in
    /// [`Engine::track_generation`]. Replaces an earlier queued file. A load or stop drops it. If the file cannot be opened, or
    /// cannot be spliced in (another sample rate or channel count), the loaded one simply ends. Returns at once.
    pub fn enqueue_next(&self, path: &Path) -> Result<(), Error> {
        if (self.sink)(Command::EnqueueNext { path: path.to_owned() }) { Ok(()) } else { Err(Error::PipeWire("the audio thread is not running".into())) }
    }

    /// Drops the file queued with [`Engine::enqueue_next`].
    pub fn clear_next(&self) {
        self.send(Command::ClearNext);
    }

    pub fn volume(&self) -> f32 {
        f32::from_bits(self.shared.volume.load(Ordering::Acquire))
    }

    /// Sample rate of the loaded file.
    pub fn rate(&self) -> u32 {
        self.shared.track().rate
    }

    /// Frames of audio handed to the output since the file was loaded or sought.
    pub fn frames_played(&self) -> u64 {
        self.shared.track().frames.load(Ordering::Acquire)
    }

    /// Frames of silence written while the file was still playing (the decoder fell behind).
    pub fn underrun_frames(&self) -> u64 {
        self.shared.track().underruns.load(Ordering::Acquire)
    }

    pub fn error(&self) -> Option<String> {
        self.shared.error.lock().ok().and_then(|e| e.clone())
    }

    /// Waits until the state satisfies `wanted`, or the time is up.
    pub fn wait_for(&self, wanted: impl Fn(State) -> bool, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if wanted(self.state()) {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        wanted(self.state())
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.drop_stream();
        let _ = (self.sink)(Command::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
