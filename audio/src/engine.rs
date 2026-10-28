// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The playback engine: PipeWire on its own thread, commands in, state out.
//!
//! One *session* is one file being played: a decoder thread fills a lock-free ring
//! buffer, and the real-time callback of a PipeWire stream copies from it. Loading a
//! file, and also seeking, builds a new session; pausing deactivates the stream.
//! See `docs/spec-audio-pipewire.md`.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use pipewire as pw;
use pw::core::CoreRc;
use pw::main_loop::MainLoopRc;
use pw::properties::properties;
use pw::spa;
use pw::stream::{StreamFlags, StreamListener, StreamRc};
use rtrb::{Consumer, Producer, RingBuffer};
use spa::param::audio::{AudioFormat, AudioInfoRaw, MAX_CHANNELS};
use spa::pod::Pod;

use crate::Error;
use crate::source::Source;
use crate::stretch::Stretcher;

/// About 2 seconds of audio at 48 kHz stereo; the decoder stays this far ahead.
const RING_SAMPLES: usize = 48_000 * 2 * 2;
const UNKNOWN: u64 = u64::MAX;

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
}

impl State {
    fn from_u8(v: u8) -> State {
        match v {
            1 => State::Playing,
            2 => State::Paused,
            3 => State::Ended,
            4 => State::Failed,
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

/// What the loop thread publishes and the control side reads. Plain atomics.
struct Shared {
    state: AtomicU8,
    /// Frames handed to PipeWire in the current session (real audio, not padding).
    frames: AtomicU64,
    start_ms: AtomicU64,
    rate: AtomicU32,
    duration_ms: AtomicU64,
    /// The volume the stream has (as last set or reported by PipeWire), as f32 bits.
    volume: AtomicU32,
    /// Has the caller ever set a volume? If not, the engine leaves the volume to the
    /// system, which may restore one it remembers.
    volume_set: AtomicBool,
    /// The playback speed as f32 bits; the decoder thread stretches the audio to it.
    speed: AtomicU32,
    /// Maps frames handed to PipeWire to frames of the file, which differ at another speed.
    marks: Mutex<Marks>,
    underruns: AtomicU64,
    error: Mutex<Option<String>>,
}

/// Pairs of (frames written to the ring, frames of the file they stand for), in
/// order. `from` is the last pair that the listener has already passed.
#[derive(Default)]
struct Marks {
    from: (u64, u64),
    next: VecDeque<(u64, u64)>,
}

impl Marks {
    /// Forgets the pairs that are behind `played` (output frames).
    fn advance(&mut self, played: u64) {
        while self.next.front().is_some_and(|m| m.0 <= played) {
            self.from = self.next.pop_front().unwrap();
        }
    }

    /// The frame of the file that output frame `played` belongs to.
    fn source_frame(&mut self, played: u64) -> u64 {
        self.advance(played);
        match self.next.front() {
            Some(&(out, src)) if out > self.from.0 => {
                let done = (played - self.from.0) as u128;
                let span = (out - self.from.0) as u128;
                self.from.1 + ((src.saturating_sub(self.from.1)) as u128 * done / span) as u64
            }
            _ => self.from.1 + (played - self.from.0),
        }
    }
}

impl Shared {
    fn set_state(&self, s: State) {
        self.state.store(s as u8, Ordering::Release);
    }
}

enum Command {
    Load { path: PathBuf, start: Duration, play: bool, media_name: String, reply: mpsc::Sender<Result<(), String>> },
    Play,
    Pause,
    Seek(Duration),
    Volume(f32),
    Stop,
    Shutdown,
}

pub struct Engine {
    tx: pw::channel::Sender<Command>,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Engine {
    /// Connects to PipeWire on a new thread. Fails if PipeWire is not running.
    pub fn start(options: EngineOptions) -> Result<Engine, Error> {
        let shared = Arc::new(Shared {
            state: AtomicU8::new(State::Idle as u8),
            frames: AtomicU64::new(0),
            start_ms: AtomicU64::new(0),
            rate: AtomicU32::new(0),
            duration_ms: AtomicU64::new(UNKNOWN),
            volume: AtomicU32::new(1.0f32.to_bits()),
            volume_set: AtomicBool::new(false),
            speed: AtomicU32::new(1.0f32.to_bits()),
            marks: Mutex::new(Marks::default()),
            underruns: AtomicU64::new(0),
            error: Mutex::new(None),
        });
        let (tx, rx) = pw::channel::channel::<Command>();
        let (ready_tx, ready_rx) = mpsc::channel();
        let thread = {
            let shared = shared.clone();
            thread::Builder::new()
                .name("psalm-pipewire".into())
                .spawn(move || run(options, rx, shared, ready_tx))
                .map_err(|e| Error::PipeWire(format!("cannot start the audio thread: {e}")))?
        };
        match ready_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(())) => Ok(Engine { tx, shared, thread: Some(thread) }),
            Ok(Err(e)) => Err(Error::PipeWire(e)),
            Err(_) => Err(Error::PipeWire("PipeWire did not answer".into())),
        }
    }

    fn send(&self, cmd: Command) {
        // The only failure is a stopped loop, and then there is nothing to command.
        let _ = self.tx.send(cmd);
    }

    /// Opens a file and starts at `start`, playing or paused. Returns when the
    /// stream is set up (or the file could not be opened).
    pub fn load(&self, path: &Path, start: Duration, play: bool, media_name: &str) -> Result<(), Error> {
        let (reply, answer) = mpsc::channel();
        self.send(Command::Load { path: path.to_owned(), start, play, media_name: media_name.to_owned(), reply });
        match answer.recv_timeout(Duration::from_secs(20)) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(Error::Decode(e)),
            Err(_) => Err(Error::PipeWire("the audio thread did not answer".into())),
        }
    }

    pub fn play(&self) {
        self.send(Command::Play);
    }

    pub fn pause(&self) {
        self.send(Command::Pause);
    }

    /// Jumps to `to`. Keeps playing or staying paused, whichever the engine was.
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

    pub fn speed(&self) -> f32 {
        f32::from_bits(self.shared.speed.load(Ordering::Acquire))
    }

    pub fn stop(&self) {
        self.send(Command::Stop);
    }

    pub fn state(&self) -> State {
        State::from_u8(self.shared.state.load(Ordering::Acquire))
    }

    /// Where the listening is. Counts the audio handed to PipeWire, so it runs ahead
    /// of what is audible by the latency of the device (tens of milliseconds).
    pub fn position(&self) -> Duration {
        let rate = self.shared.rate.load(Ordering::Acquire).max(1) as u64;
        let played = self.shared.frames.load(Ordering::Acquire);
        let source = self.shared.marks.lock().map_or(played, |mut m| m.source_frame(played));
        let ms = self.shared.start_ms.load(Ordering::Acquire) + source * 1000 / rate;
        let position = Duration::from_millis(ms);
        self.duration().map_or(position, |d| position.min(d))
    }

    /// Length of the file, if its container says so.
    pub fn duration(&self) -> Option<Duration> {
        match self.shared.duration_ms.load(Ordering::Acquire) {
            UNKNOWN => None,
            ms => Some(Duration::from_millis(ms)),
        }
    }

    pub fn volume(&self) -> f32 {
        f32::from_bits(self.shared.volume.load(Ordering::Acquire))
    }

    /// Sample rate of the loaded file.
    pub fn rate(&self) -> u32 {
        self.shared.rate.load(Ordering::Acquire)
    }

    /// Frames of audio handed to PipeWire since the file was loaded or sought.
    pub fn frames_played(&self) -> u64 {
        self.shared.frames.load(Ordering::Acquire)
    }

    /// Frames of silence written while the file was still playing (the decoder fell behind).
    pub fn underrun_frames(&self) -> u64 {
        self.shared.underruns.load(Ordering::Acquire)
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
        let _ = self.tx.send(Command::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// What the real-time callback owns. Only things that are safe to use there: the
/// consumer end of the ring, atomics and plain values. No `Rc`, no locks.
struct Rt {
    consumer: Consumer<f32>,
    channels: usize,
    shared: Arc<Shared>,
    decoder_done: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    drained: Arc<AtomicBool>,
}

struct Session {
    path: PathBuf,
    media_name: String,
    stream: StreamRc,
    _listener: StreamListener<Rt>,
    /// Reports volume changes from outside. A listener of its own with its own data,
    /// because `process` runs on the real-time thread and must not share a `&mut` with it.
    _controls: StreamListener<Arc<Shared>>,
    stop: Arc<AtomicBool>,
    decoder: Option<JoinHandle<()>>,
    finished: Arc<AtomicBool>,
    drained: Arc<AtomicBool>,
    flushed: bool,
    deadline: Option<Instant>,
}

/// Everything the loop thread works with.
struct Ctx {
    core: CoreRc,
    mainloop: MainLoopRc,
    shared: Arc<Shared>,
    target: Option<String>,
    role: String,
    session: RefCell<Option<Session>>,
}

fn run(options: EngineOptions, rx: pw::channel::Receiver<Command>, shared: Arc<Shared>, ready: mpsc::Sender<Result<(), String>>) {
    pw::init();
    let setup = || -> Result<(MainLoopRc, pw::context::ContextRc, CoreRc), pw::Error> {
        let mainloop = MainLoopRc::new(None)?;
        let context = pw::context::ContextRc::new(&mainloop, None)?;
        let core = context.connect_rc(None)?;
        Ok((mainloop, context, core))
    };
    let (mainloop, _context, core) = match setup() {
        Ok(parts) => parts,
        Err(e) => {
            let _ = ready.send(Err(e.to_string()));
            return;
        }
    };
    let role = options.role.or_else(|| std::env::var("PSALM_AUDIO_ROLE").ok()).unwrap_or_else(|| "Music".to_owned());
    let ctx = Rc::new(Ctx { core, mainloop: mainloop.clone(), shared, target: options.target, role, session: RefCell::new(None) });

    let handler = ctx.clone();
    let _attached = rx.attach(mainloop.loop_(), move |cmd| handler.handle(cmd));
    let ticker = ctx.clone();
    let timer = mainloop.loop_().add_timer(move |_| ticker.tick());
    let _ = timer.update_timer(Some(Duration::from_millis(50)), Some(Duration::from_millis(50)));

    let _ = ready.send(Ok(()));
    mainloop.run();
    ctx.end_session();
}

impl Ctx {
    fn handle(&self, cmd: Command) {
        match cmd {
            Command::Load { path, start, play, media_name, reply } => {
                let result = self.load(&path, start, play, &media_name);
                if let Err(e) = &result {
                    *self.shared.error.lock().unwrap() = Some(e.to_string());
                    self.shared.set_state(State::Failed);
                }
                let _ = reply.send(result.map_err(|e| e.to_string()));
            }
            Command::Play => {
                if let Some(s) = self.session.borrow().as_ref()
                    && self.state() == State::Paused {
                        let _ = s.stream.set_active(true);
                        self.shared.set_state(State::Playing);
                    }
            }
            Command::Pause => {
                if let Some(s) = self.session.borrow().as_ref()
                    && self.state() == State::Playing {
                        let _ = s.stream.set_active(false);
                        self.shared.set_state(State::Paused);
                    }
            }
            Command::Seek(to) => {
                let current = self.session.borrow().as_ref().map(|s| (s.path.clone(), s.media_name.clone()));
                if let Some((path, name)) = current {
                    let paused = self.state() == State::Paused;
                    if let Err(e) = self.load(&path, to, !paused, &name) {
                        *self.shared.error.lock().unwrap() = Some(e.to_string());
                        self.shared.set_state(State::Failed);
                    }
                }
            }
            Command::Volume(v) => {
                if let Some(s) = self.session.borrow().as_ref() {
                    let _ = s.stream.set_control(spa::sys::SPA_PROP_volume, &[v]);
                }
            }
            Command::Stop => {
                self.end_session();
                self.shared.set_state(State::Idle);
            }
            Command::Shutdown => {
                self.end_session();
                self.mainloop.quit();
            }
        }
    }

    fn state(&self) -> State {
        State::from_u8(self.shared.state.load(Ordering::Acquire))
    }

    /// Stops the decoder and removes the stream of the current session, if any.
    fn end_session(&self) {
        if let Some(mut s) = self.session.borrow_mut().take() {
            s.stop.store(true, Ordering::Release);
            let _ = s.stream.disconnect();
            if let Some(d) = s.decoder.take() {
                let _ = d.join();
            }
        }
    }

    fn load(&self, path: &Path, start: Duration, play: bool, media_name: &str) -> Result<(), Error> {
        self.end_session();
        *self.shared.error.lock().unwrap() = None;

        let (source, first) = Source::open(path, start)?;
        let (rate, channels) = (source.rate, source.channels);
        let (producer, consumer) = RingBuffer::<f32>::new(RING_SAMPLES);
        let stop = Arc::new(AtomicBool::new(false));
        let decoder_done = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let drained = Arc::new(AtomicBool::new(false));

        self.shared.frames.store(0, Ordering::Release);
        self.shared.start_ms.store(start.as_millis() as u64, Ordering::Release);
        self.shared.rate.store(rate, Ordering::Release);
        self.shared.duration_ms.store(source.duration.map_or(UNKNOWN, |d| d.as_millis() as u64), Ordering::Release);
        self.shared.underruns.store(0, Ordering::Release);
        *self.shared.marks.lock().unwrap() = Marks::default();

        let decoder = {
            let (stop, done, shared) = (stop.clone(), decoder_done.clone(), self.shared.clone());
            thread::Builder::new()
                .name("psalm-decoder".into())
                .spawn(move || feed(source, first, producer, stop, done, shared))
                .map_err(|e| Error::Decode(format!("cannot start the decoder: {e}")))?
        };

        let mut props = properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Playback",
            *pw::keys::MEDIA_ROLE => self.role.as_str(),
            *pw::keys::APP_NAME => "Psalm",
            *pw::keys::NODE_NAME => "psalm",
            *pw::keys::MEDIA_NAME => media_name,
        };
        if let Some(target) = &self.target {
            props.insert(*pw::keys::TARGET_OBJECT, target.as_str());
        }
        let stream = StreamRc::new(self.core.clone(), "psalm", props)?;

        let rt = Rt {
            consumer,
            channels: channels as usize,
            shared: self.shared.clone(),
            decoder_done,
            finished: finished.clone(),
            drained: drained.clone(),
        };
        let listener = stream
            .add_local_listener_with_user_data(rt)
            .process(process)
            .drained(|_, rt| rt.drained.store(true, Ordering::Release))
            .register()?;

        let controls = stream
            .add_local_listener_with_user_data(self.shared.clone())
            .control_info(|_stream, shared, id, control| {
                if id != spa::sys::SPA_PROP_volume || control.is_null() {
                    return;
                }
                // SAFETY: PipeWire passes a valid control for the duration of the call.
                let control = unsafe { &*control };
                if control.n_values > 0 && !control.values.is_null() {
                    // SAFETY: `values` holds `n_values` floats.
                    let volume = unsafe { *control.values };
                    shared.volume.store(volume.to_bits(), Ordering::Release);
                }
            })
            .register()?;

        let values = format_pod(rate, channels)?;
        let mut params = [Pod::from_bytes(&values).ok_or_else(|| Error::PipeWire("bad format pod".into()))?];
        let mut flags = StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS;
        if !play {
            flags |= StreamFlags::INACTIVE;
        }
        stream.connect(spa::utils::Direction::Output, None, flags, &mut params)?;
        // Only a volume that the caller has set is applied. Otherwise the system decides.
        if self.shared.volume_set.load(Ordering::Acquire) {
            let _ = stream.set_control(spa::sys::SPA_PROP_volume, &[f32::from_bits(self.shared.volume.load(Ordering::Acquire))]);
        }

        self.shared.set_state(if play { State::Playing } else { State::Paused });
        *self.session.borrow_mut() = Some(Session {
            path: path.to_owned(),
            media_name: media_name.to_owned(),
            stream,
            _listener: listener,
            _controls: controls,
            stop,
            decoder: Some(decoder),
            finished,
            drained,
            flushed: false,
            deadline: None,
        });
        Ok(())
    }

    /// Every 50 ms, on the loop thread: notices the end of the file and the first
    /// failure of the decoder. Quitting at once at the end would cut off what is
    /// already queued in PipeWire, so the stream is drained first, with a deadline.
    fn tick(&self) {
        let mut guard = self.session.borrow_mut();
        let Some(s) = guard.as_mut() else { return };
        if self.shared.error.lock().map(|e| e.is_some()).unwrap_or(false) {
            self.shared.set_state(State::Failed);
            return;
        }
        if !matches!(self.state(), State::Playing | State::Paused) || !s.finished.load(Ordering::Acquire) {
            return;
        }
        if !s.flushed {
            s.flushed = true;
            s.deadline = Some(Instant::now() + Duration::from_secs(2));
            let _ = s.stream.flush(true);
        }
        if s.drained.load(Ordering::Acquire) || s.deadline.is_some_and(|d| Instant::now() > d) {
            self.shared.set_state(State::Ended);
        }
    }
}

/// The real-time callback: copy whole frames from the ring buffer, pad with silence.
fn process(stream: &pw::stream::Stream, rt: &mut Rt) {
    let Some(mut buffer) = stream.dequeue_buffer() else { return };
    let datas = buffer.datas_mut();
    let data = &mut datas[0];
    let stride = rt.channels * std::mem::size_of::<f32>();
    let mut frames = 0;
    if let Some(slice) = data.data() {
        let capacity = slice.len() / stride;
        let want = capacity * rt.channels;
        let have = rt.consumer.slots().min(want);
        // Whole frames only, so channels never get out of step.
        let have = have - have % rt.channels;
        if let Ok(chunk) = rt.consumer.read_chunk(have) {
            let (a, b) = chunk.as_slices();
            let (samples, _) = slice.as_chunks_mut::<4>();
            for (out, s) in samples.iter_mut().zip(a.iter().chain(b.iter())) {
                *out = s.to_le_bytes();
            }
            chunk.commit_all();
        }
        let real = have / rt.channels;
        rt.shared.frames.fetch_add(real as u64, Ordering::Relaxed);
        frames = capacity;
        if real < capacity {
            slice[real * stride..capacity * stride].fill(0);
            if rt.decoder_done.load(Ordering::Acquire) && rt.consumer.is_empty() {
                // The file ended; the padding is not an error and is not played.
                rt.finished.store(true, Ordering::Release);
                frames = real;
            } else {
                rt.shared.underruns.fetch_add((capacity - real) as u64, Ordering::Relaxed);
            }
        }
    }
    let chunk = data.chunk_mut();
    *chunk.offset_mut() = 0;
    *chunk.stride_mut() = stride as _;
    *chunk.size_mut() = (frames * stride) as _;
}

/// Writes samples into the ring buffer, waiting while it is full, and records which
/// frame of the file they stand for.
struct Writer {
    producer: Producer<f32>,
    channels: usize,
    stop: Arc<AtomicBool>,
    shared: Arc<Shared>,
    written: u64,
}

impl Writer {
    /// `source_end` is the frame of the file at the end of `samples`. False if told to stop.
    fn write(&mut self, samples: &[f32], source_end: u64) -> bool {
        let mut offset = 0;
        while offset < samples.len() {
            if self.stop.load(Ordering::Acquire) {
                return false;
            }
            let free = self.producer.slots();
            if free == 0 {
                thread::sleep(Duration::from_millis(5));
                continue;
            }
            let n = free.min(samples.len() - offset);
            if let Ok(mut chunk) = self.producer.write_chunk_uninit(n) {
                let (a, b) = chunk.as_mut_slices();
                let (head, tail) = samples[offset..offset + n].split_at(a.len());
                for (dst, src) in a.iter_mut().zip(head) {
                    dst.write(*src);
                }
                for (dst, src) in b.iter_mut().zip(tail) {
                    dst.write(*src);
                }
                // SAFETY: exactly `n` slots were initialized above.
                unsafe { chunk.commit_all() };
                offset += n;
            }
        }
        self.written += (samples.len() / self.channels) as u64;
        if let Ok(mut marks) = self.shared.marks.lock() {
            marks.advance(self.shared.frames.load(Ordering::Acquire));
            marks.next.push_back((self.written, source_end));
        }
        true
    }
}

/// The decoder thread: stays ahead of the callback until the file ends or it is told to stop.
/// At a speed other than 1.0 the audio goes through the time stretcher on its way.
fn feed(mut source: Source, first: Vec<f32>, producer: Producer<f32>, stop: Arc<AtomicBool>, done: Arc<AtomicBool>, shared: Arc<Shared>) {
    let channels = source.channels as usize;
    let rate = source.rate;
    let mut writer = Writer { producer, channels, stop, shared: shared.clone(), written: 0 };
    let mut stretcher: Option<Stretcher> = None;
    // Frames of the file before the block in hand, and before the stretcher's first input.
    let mut consumed = 0u64;
    let mut stretch_origin = 0u64;
    let mut pending = first;
    loop {
        let frames = (pending.len() / channels) as u64;
        let speed = f32::from_bits(shared.speed.load(Ordering::Acquire));
        if (speed - 1.0).abs() < 0.01 {
            if let Some(mut st) = stretcher.take() {
                let rest = st.take_rest();
                if !writer.write(&rest, consumed) {
                    return;
                }
            }
            if !writer.write(&pending, consumed + frames) {
                return;
            }
        } else {
            let st = stretcher.get_or_insert_with(|| {
                stretch_origin = consumed;
                Stretcher::new(rate, channels)
            });
            st.set_speed(speed as f64);
            st.push(&pending);
            while let Some(block) = st.pull() {
                if !writer.write(&block, (stretch_origin + st.position()).min(consumed + frames)) {
                    return;
                }
            }
        }
        consumed += frames;
        match source.next_samples() {
            Ok(Some(samples)) => pending = samples,
            Ok(None) => break,
            Err(e) => {
                *shared.error.lock().unwrap() = Some(e.to_string());
                break;
            }
        }
    }
    if let Some(mut st) = stretcher {
        st.finish();
        while let Some(block) = st.pull() {
            if !writer.write(&block, consumed) {
                return;
            }
        }
    }
    done.store(true, Ordering::Release);
}

/// The format the stream offers: the file's own rate and channel count, `F32LE`.
/// PipeWire converts to what the device wants.
fn format_pod(rate: u32, channels: u32) -> Result<Vec<u8>, Error> {
    let mut info = AudioInfoRaw::new();
    info.set_format(AudioFormat::F32LE);
    info.set_rate(rate);
    info.set_channels(channels);
    let mut position = [0; MAX_CHANNELS];
    match channels {
        1 => position[0] = spa::sys::SPA_AUDIO_CHANNEL_MONO,
        _ => {
            position[0] = spa::sys::SPA_AUDIO_CHANNEL_FL;
            position[1] = spa::sys::SPA_AUDIO_CHANNEL_FR;
        }
    }
    info.set_position(position);
    pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(pw::spa::pod::Object {
            type_: spa::sys::SPA_TYPE_OBJECT_Format,
            id: spa::sys::SPA_PARAM_EnumFormat,
            properties: info.into(),
        }),
    )
    .map(|(cursor, _)| cursor.into_inner())
    .map_err(|e| Error::PipeWire(format!("cannot build the format: {e:?}")))
}
