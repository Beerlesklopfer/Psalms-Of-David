// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The portable core of the engine: sessions, the decoder thread, the ring buffer and the
//! `process` closure that an [`crate::output::Output`] calls. Nothing here depends on PipeWire.
//!
//! One *session* is one file being played: a decoder thread fills a lock-free ring buffer, and the
//! output's real-time callback copies from it. Loading a file, and also seeking, builds a new
//! session; pausing deactivates the stream. See `docs/spec-audio-pipewire.md`.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rtrb::{Consumer, Producer, RingBuffer};

use crate::Error;
use crate::engine::{Command, Shared, State, UNKNOWN};
use crate::output::{CommandSink, OutStream, ProcessFn, Produced, StreamFactory, StreamSpec};
use crate::source::Source;
use crate::stream::ByteQueue;
use crate::stretch::Stretcher;

/// Samples of one second of audio at 48 kHz stereo; the ring is sized in these.
const RING_SAMPLES_PER_SECOND: usize = 48_000 * 2;
/// A live stream's ring holds this many times the buffer, so that the pre-buffer (the buffer itself) fits with room to spare and
/// short network stalls are bridged.
const STREAM_RING_FACTOR: usize = 4;

/// What is known about the file a point of the map belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Info {
    /// Position (ms) of the file's frame 0: the start of the load for the first file of a session, 0 for the files after it.
    pub base_ms: u64,
    /// Length of the file (ms), `UNKNOWN` if it does not say.
    pub duration_ms: u64,
    /// Index of the file in this session: 0 for the loaded one, 1 for the first one spliced in behind it, and so on.
    pub index: usize,
}

/// One point of the map from frames handed to the output to frames of the file.
#[derive(Debug, Clone, Copy)]
pub struct Mark {
    /// Frames written to the ring in this session when the point was made.
    pub out: u64,
    /// The frame of the file at that point.
    pub src: u64,
    pub info: Info,
    /// The point where the next file begins (gapless splice): passing it counts as an advance.
    pub splice: bool,
}

/// Pairs of (frames written to the ring, frames of the file they stand for), in
/// order. `from` is the last pair that the listener has already passed.
pub struct Marks {
    from: Mark,
    next: VecDeque<Mark>,
    /// Counts the splices that were passed (shared by all sessions of the engine).
    advanced: Arc<AtomicU64>,
}

impl Marks {
    pub fn new(info: Info, advanced: Arc<AtomicU64>) -> Marks {
        Marks { from: Mark { out: 0, src: 0, info, splice: false }, next: VecDeque::new(), advanced }
    }

    /// Forgets the pairs that are behind `played` (output frames).
    fn advance(&mut self, played: u64) {
        while self.next.front().is_some_and(|m| m.out <= played) {
            self.from = self.next.pop_front().unwrap();
            if self.from.splice {
                self.advanced.fetch_add(1, Ordering::AcqRel);
            }
        }
    }

    /// The frame of the file that output frame `played` belongs to, and what is known about that file.
    pub fn at(&mut self, played: u64) -> (u64, Info) {
        self.advance(played);
        let src = match self.next.front() {
            Some(&Mark { out, src, .. }) if out > self.from.out => {
                let done = (played - self.from.out) as u128;
                let span = (out - self.from.out) as u128;
                self.from.src + ((src.saturating_sub(self.from.src)) as u128 * done / span) as u64
            }
            _ => self.from.src + (played - self.from.out),
        };
        (src, self.from.info)
    }
}

/// What the decoder and the output publish about one session. A new session gets a new `Track`, so
/// that a decoder thread that is still winding down cannot disturb the next session.
pub struct Track {
    /// Frames handed to the output in this session (real audio, not padding).
    pub frames: AtomicU64,
    pub rate: u32,
    /// Frames the decoder has written to the ring in this session.
    pub queued_frames: AtomicU64,
    pub underruns: AtomicU64,
    /// Set by the output when the ring was empty and the decoder is not done.
    pub starved: AtomicBool,
    /// The first failure of the decoder.
    pub error: Mutex<Option<String>>,
    pub marks: Mutex<Marks>,
    /// The files of this session by index (see [`Info::index`]): the loaded one, then the ones spliced in.
    pub paths: Mutex<Vec<PathBuf>>,
}

impl Track {
    pub fn new(start_ms: u64, rate: u32, duration_ms: u64, advanced: Arc<AtomicU64>) -> Arc<Track> {
        Arc::new(Track {
            frames: AtomicU64::new(0),
            rate,
            queued_frames: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
            starved: AtomicBool::new(false),
            error: Mutex::new(None),
            marks: Mutex::new(Marks::new(Info { base_ms: start_ms, duration_ms, index: 0 }, advanced)),
            paths: Mutex::new(Vec::new()),
        })
    }
}

/// What the output callback owns: the consumer end of the ring, atomics and plain values.
struct Playback {
    consumer: Consumer<f32>,
    channels: usize,
    track: Arc<Track>,
    decoder_done: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
}

/// The `process` closure for an output: copy whole frames from the ring buffer, pad with silence.
fn make_process(mut pb: Playback) -> ProcessFn {
    Box::new(move |out: &mut [f32]| {
        let channels = pb.channels;
        let capacity = out.len() / channels;
        let want = capacity * channels;
        let have = pb.consumer.slots().min(want);
        // Whole frames only, so channels never get out of step.
        let have = have - have % channels;
        if let Ok(chunk) = pb.consumer.read_chunk(have) {
            let (a, b) = chunk.as_slices();
            out[..a.len()].copy_from_slice(a);
            out[a.len()..a.len() + b.len()].copy_from_slice(b);
            chunk.commit_all();
        }
        let real = have / channels;
        pb.track.frames.fetch_add(real as u64, Ordering::Relaxed);
        let mut frames = capacity;
        if real < capacity {
            out[real * channels..capacity * channels].fill(0.0);
            if pb.decoder_done.load(Ordering::Acquire) && pb.consumer.is_empty() {
                // The file ended; the padding is not an error and is not played.
                pb.finished.store(true, Ordering::Release);
                frames = real;
            } else {
                if have == 0 {
                    pb.track.starved.store(true, Ordering::Release);
                }
                pb.track.underruns.fetch_add((capacity - real) as u64, Ordering::Relaxed);
            }
        }
        Produced { frames, real }
    })
}

struct Session {
    path: PathBuf,
    media_name: String,
    stream: Box<dyn OutStream>,
    track: Arc<Track>,
    stop: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    /// The decoder has no more audio to give.
    done: Arc<AtomicBool>,
    drained: Arc<AtomicBool>,
    flushed: bool,
    deadline: Option<Instant>,
    /// Set for a live stream.
    live: bool,
}

/// A source and its first decoded block, or why it could not be opened.
type Opened = Result<(Source, Vec<f32>), Error>;

/// The first block of a stream, decoded on its own thread while we wait for data.
struct Opening {
    stop: Arc<AtomicBool>,
    slot: Arc<Mutex<Option<Opened>>>,
}

/// The live stream in use: kept while paused (no session then, the queue discards).
struct StreamCtl {
    queue: Arc<ByteQueue>,
    media_name: String,
    opening: Option<Opening>,
}

/// A file that a worker thread is opening. The worker reports back with `Command::Opened { token }`.
struct Opener {
    token: u64,
    /// The load request this belongs to (0 for a seek, which has none).
    request: u64,
    path: PathBuf,
    media_name: String,
    start: Duration,
    /// Whether to play when ready; `play` and `pause` while the file is opening change it.
    play: bool,
}

/// Everything the engine thread works with.
pub struct Core {
    factory: Box<dyn StreamFactory>,
    shared: Arc<Shared>,
    sink: CommandSink,
    session: RefCell<Option<Session>>,
    stream: RefCell<Option<StreamCtl>>,
    opener: RefCell<Option<Opener>>,
    tokens: Cell<u64>,
    /// The file queued with `EnqueueNext`, once opened; the decoder of the session takes it when its file ends.
    next: NextSlot,
    /// The path of the queued file while it is wanted (opening, ready, or taken by a decoder that has not been passed yet).
    next_path: RefCell<Option<PathBuf>>,
    /// The worker that opens the queued file: its token.
    next_opening: Cell<Option<u64>>,
    /// The advances seen so far; a rise means that the queued file has been reached.
    seen_advances: Cell<u64>,
}

impl Core {
    pub fn new(factory: Box<dyn StreamFactory>, shared: Arc<Shared>, sink: CommandSink) -> Core {
        Core {
            factory,
            shared,
            sink,
            session: RefCell::new(None),
            stream: RefCell::new(None),
            opener: RefCell::new(None),
            tokens: Cell::new(0),
            next: Arc::new(Mutex::new(None)),
            next_path: RefCell::new(None),
            next_opening: Cell::new(None),
            seen_advances: Cell::new(0),
        }
    }

    pub fn shutdown(&self) {
        self.end_session();
    }

    pub fn handle(&self, cmd: Command) {
        match cmd {
            Command::EnqueueNext { path } => self.enqueue_next(path),
            Command::ClearNext => self.clear_next(),
            Command::NextOpened { token, result } => self.next_opened(token, result),
            Command::Load { id, path, start, play, media_name } => {
                self.clear_next();
                self.end_session();
                *self.shared.error.lock().unwrap() = None;
                self.open_file(Opener { token: 0, request: id, path, media_name, start, play });
            }
            Command::Opened { token, result } => self.opened(token, result),
            Command::LoadStream { queue, play, media_name } => {
                self.clear_next();
                self.end_session();
                *self.shared.error.lock().unwrap() = None;
                *self.stream.borrow_mut() = Some(StreamCtl { queue, media_name, opening: None });
                if play {
                    // The queue is fresh and the sender may have pushed into it already: keep that.
                    self.start_live(true);
                } else {
                    self.suspend_live();
                }
            }
            Command::Play if self.stream.borrow().is_some() => {
                if self.state() == State::Paused {
                    self.start_live(false);
                }
            }
            Command::Pause if self.stream.borrow().is_some() => {
                if matches!(self.state(), State::Playing | State::Buffering) {
                    self.suspend_live();
                }
            }
            Command::Seek(_) if self.stream.borrow().is_some() => {}
            Command::Play | Command::Pause if self.opener.borrow().is_some() => {
                // The file is still being opened: remember what to do when it is ready.
                let play = matches!(cmd, Command::Play);
                if let Some(op) = self.opener.borrow_mut().as_mut() {
                    op.play = play;
                }
                self.shared.set_state(if play { State::Playing } else { State::Paused });
            }
            Command::Play => {
                if let Some(s) = self.session.borrow().as_ref()
                    && self.state() == State::Paused
                {
                    s.stream.set_active(true);
                    self.shared.set_state(State::Playing);
                }
            }
            Command::Pause => {
                if let Some(s) = self.session.borrow().as_ref()
                    && self.state() == State::Playing
                {
                    s.stream.set_active(false);
                    self.shared.set_state(State::Paused);
                }
            }
            Command::Seek(to) => {
                let to_ms = to.as_millis() as u64;
                // While the file is still opening, the opening starts again at the new place.
                let opening = self.opener.borrow_mut().take();
                if let Some(mut op) = opening {
                    op.start = to;
                    self.shared.set_track(self.shared.new_track(to_ms, 0, crate::engine::UNKNOWN));
                    self.open_file(op);
                    return;
                }
                let current = self.session.borrow().as_ref().map(|s| {
                    self.sync_advances_of(s);
                    // After a gapless switch the playing file is not the loaded one.
                    let (_, info) = s.track.marks.lock().map(|mut m| m.at(s.track.frames.load(Ordering::Acquire))).unwrap_or((0, Info { base_ms: 0, duration_ms: UNKNOWN, index: 0 }));
                    let path = s.track.paths.lock().ok().and_then(|p| p.get(info.index).cloned()).unwrap_or_else(|| s.path.clone());
                    (path, s.media_name.clone(), info.duration_ms, s.track.rate)
                });
                if let Some((path, media_name, duration_ms, rate)) = current {
                    let play = self.state() != State::Paused;
                    self.stop_audio();
                    // A queued file that a decoder has already taken is lost with that session: open it again.
                    self.reopen_next_if_lost();
                    // The position is the new one at once; the session replaces it when the file is open at that place.
                    self.shared.set_track(self.shared.new_track(to_ms, rate, duration_ms));
                    self.open_file(Opener { token: 0, request: 0, path, media_name, start: to, play });
                }
            }
            Command::Volume(v) => {
                if let Some(s) = self.session.borrow().as_ref() {
                    s.stream.set_volume(v);
                }
            }
            Command::Stop => {
                self.clear_next();
                self.end_session();
                self.shared.set_state(State::Idle);
            }
            Command::Shutdown => self.shutdown(),
        }
    }

    /// Starts a worker that opens the file; the engine thread never waits for it.
    fn open_file(&self, mut op: Opener) {
        let token = self.tokens.get() + 1;
        self.tokens.set(token);
        op.token = token;
        let (path, start, sink) = (op.path.clone(), op.start, self.sink.clone());
        let spawned = thread::Builder::new().name("psalm-open".into()).spawn(move || {
            let result = Source::open(&path, start);
            // The engine may be gone by now; then there is nobody to tell.
            let _ = sink(Command::Opened { token, result });
        });
        match spawned {
            Ok(_) => *self.opener.borrow_mut() = Some(op),
            Err(e) => {
                self.fail(format!("cannot start the file reader: {e}"));
                self.shared.finish_request(op.request);
            }
        }
    }

    /// A worker is done. An answer for an opening that has been replaced or cancelled is dropped.
    fn opened(&self, token: u64, result: Result<(Source, Vec<f32>), Error>) {
        let op = {
            let mut guard = self.opener.borrow_mut();
            if guard.as_ref().map(|o| o.token) != Some(token) {
                return;
            }
            guard.take().unwrap()
        };
        let outcome = result.and_then(|(source, first)| self.begin(source, first, op.start, op.play, &op.media_name, op.path.clone(), None));
        if let Err(e) = outcome {
            self.fail(e.to_string());
        }
        self.shared.finish_request(op.request);
    }

    fn state(&self) -> State {
        self.shared.stored_state()
    }

    /// Ends everything, including the live stream and its queue.
    fn end_session(&self) {
        self.opener.borrow_mut().take();
        self.stop_audio();
        if let Some(ctl) = self.stream.borrow_mut().take() {
            if let Some(op) = ctl.opening {
                op.stop.store(true, Ordering::Release);
            }
            ctl.queue.close();
        }
    }

    /// Stops the decoder and removes the output stream, if any; a live stream's
    /// queue stays open. Also cancels a stream that is still opening. The decoder thread is
    /// told to stop and left to wind down by itself: it only touches the `Track` of its own session.
    fn stop_audio(&self) {
        if let Some(op) = self.stream.borrow_mut().as_mut().and_then(|c| c.opening.take()) {
            op.stop.store(true, Ordering::Release);
        }
        if let Some(s) = self.session.borrow_mut().take() {
            self.halt(&s.stop);
            s.stream.disconnect();
        }
    }

    /// Passing the splice of a queued file is noticed when the marks are read; then the file is no longer queued.
    fn sync_advances_of(&self, s: &Session) {
        let _ = s.track.marks.lock().map(|mut m| m.at(s.track.frames.load(Ordering::Acquire)));
        let advances = self.shared.advanced.load(Ordering::Acquire);
        if advances != self.seen_advances.replace(advances) {
            *self.next_path.borrow_mut() = None;
        }
    }

    /// Tells a decoder to stop. Under the lock of the next slot, so that the decoder either took the queued file before (and it is
    /// gone with that session) or sees the flag and leaves it where it is.
    fn halt(&self, stop: &AtomicBool) {
        let _guard = self.next.lock().unwrap_or_else(|e| e.into_inner());
        stop.store(true, Ordering::Release);
    }

    fn enqueue_next(&self, path: PathBuf) {
        self.clear_next();
        if self.stream.borrow().is_some() {
            return; // a live stream has no next file
        }
        // Advances up to now concern files queued earlier; a player that enqueues right after it was told about a splice must
        // not have the new file forgotten by the next tick noticing that splice.
        self.seen_advances.set(self.shared.advanced.load(Ordering::Acquire));
        *self.next_path.borrow_mut() = Some(path.clone());
        self.open_next(path);
    }

    fn open_next(&self, path: PathBuf) {
        let token = self.tokens.get() + 1;
        self.tokens.set(token);
        let sink = self.sink.clone();
        let spawned = thread::Builder::new().name("psalm-open-next".into()).spawn(move || {
            let result = Source::open(&path, Duration::ZERO);
            let _ = sink(Command::NextOpened { token, result });
        });
        if spawned.is_ok() {
            self.next_opening.set(Some(token));
        }
    }

    fn next_opened(&self, token: u64, result: Result<(Source, Vec<f32>), Error>) {
        if self.next_opening.get() != Some(token) {
            return;
        }
        self.next_opening.set(None);
        let Some(path) = self.next_path.borrow().clone() else { return };
        // A file that cannot be opened is not an error of the playback: the loaded file ends normally and the caller deals with it.
        if let Ok((source, first)) = result {
            *self.next.lock().unwrap_or_else(|e| e.into_inner()) = Some(NextSource { source, first, path });
        }
    }

    fn clear_next(&self) {
        self.next_opening.set(None);
        *self.next_path.borrow_mut() = None;
        *self.next.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// After a seek: the queued file may have been taken by the decoder of the session that is gone (it was buffered ahead of
    /// the playback). It is still wanted, so it is opened again.
    fn reopen_next_if_lost(&self) {
        let lost = self.next.lock().unwrap_or_else(|e| e.into_inner()).is_none() && self.next_opening.get().is_none();
        let path = self.next_path.borrow().clone();
        if let (true, Some(path)) = (lost, path) {
            self.open_next(path);
        }
    }

    /// Builds the output stream and the decoder thread for an opened source.
    /// A live stream starts inactive and waits in `Buffering` for its pre-buffer.
    #[allow(clippy::too_many_arguments)]
    fn begin(&self, source: Source, first: Vec<f32>, start: Duration, play: bool, media_name: &str, path: PathBuf, live_stop: Option<Arc<AtomicBool>>) -> Result<(), Error> {
        let live = live_stop.is_some();
        let (rate, channels) = (source.rate, source.channels);
        let seconds = self.shared.buffer_s.load(Ordering::Acquire) as usize;
        let ring = RING_SAMPLES_PER_SECOND * seconds * if live { STREAM_RING_FACTOR } else { 1 };
        let (producer, consumer) = RingBuffer::<f32>::new(ring);
        // A live stream's reader already watches the flag of its opening; reuse it so that
        // stopping the session also ends a read that waits for network data.
        let stop = live_stop.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
        let decoder_done = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let drained = Arc::new(AtomicBool::new(false));

        let track = self.shared.new_track(start.as_millis() as u64, rate, if live { UNKNOWN } else { source.duration.map_or(UNKNOWN, |d| d.as_millis() as u64) });
        self.shared.set_track(track.clone());

        {
            let (stop, done, shared, track, next) = (stop.clone(), decoder_done.clone(), self.shared.clone(), track.clone(), self.next.clone());
            if let Ok(mut paths) = track.paths.lock() {
                paths.push(path.clone());
            }
            thread::Builder::new()
                .name("psalm-decoder".into())
                .spawn(move || feed(source, first, producer, stop, done, shared, track, next, live))
                .map_err(|e| Error::Decode(format!("cannot start the decoder: {e}")))?;
        }

        let process = make_process(Playback { consumer, channels: channels as usize, track: track.clone(), decoder_done: decoder_done.clone(), finished: finished.clone() });
        let volume = self.shared.volume_set.load(Ordering::Acquire).then(|| f32::from_bits(self.shared.volume.load(Ordering::Acquire)));
        let spec = StreamSpec {
            rate,
            channels,
            media_name: media_name.to_owned(),
            active: play && !live,
            volume,
            volume_report: self.shared.volume_arc(),
            drained: drained.clone(),
        };
        let stream = match self.factory.open(spec, process) {
            Ok(s) => s,
            Err(e) => {
                self.halt(&stop);
                return Err(e);
            }
        };

        self.shared.set_state(if live { State::Buffering } else if play { State::Playing } else { State::Paused });
        *self.session.borrow_mut() = Some(Session {
            path,
            media_name: media_name.to_owned(),
            stream,
            track,
            stop,
            finished,
            done: decoder_done,
            drained,
            flushed: false,
            deadline: None,
            live,
        });
        Ok(())
    }

    /// (Re)starts a live stream: opens again, at the live edge (queued bytes are forgotten) unless the stream is `fresh`.
    fn start_live(&self, fresh: bool) {
        self.stop_audio();
        let mut guard = self.stream.borrow_mut();
        let Some(ctl) = guard.as_mut() else { return };
        ctl.queue.set_discarding(false);
        if !fresh {
            ctl.queue.clear();
        }
        let stop = Arc::new(AtomicBool::new(false));
        let slot = Arc::new(Mutex::new(None));
        let (queue, out, abort) = (ctl.queue.clone(), slot.clone(), stop.clone());
        match thread::Builder::new().name("psalm-stream-open".into()).spawn(move || {
            let result = Source::open_stream(queue, abort);
            *out.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
        }) {
            Ok(_) => {
                ctl.opening = Some(Opening { stop, slot });
                self.shared.set_state(State::Buffering);
            }
            Err(e) => {
                drop(guard);
                self.fail(format!("cannot start the stream reader: {e}"));
            }
        }
    }

    /// Pauses a live stream: the connection and queue stay, old data is thrown away.
    fn suspend_live(&self) {
        self.stop_audio();
        if let Some(ctl) = self.stream.borrow().as_ref() {
            ctl.queue.set_discarding(true);
        }
        self.shared.set_state(State::Paused);
    }

    fn fail(&self, message: String) {
        *self.shared.error.lock().unwrap() = Some(message);
        self.shared.set_state(State::Failed);
    }

    /// Takes the result of a finished `Opening` and starts the session.
    fn poll_opening(&self) {
        let (result, name, stop) = {
            let mut guard = self.stream.borrow_mut();
            let Some(ctl) = guard.as_mut() else { return };
            let Some(op) = ctl.opening.as_ref() else { return };
            let Some(result) = op.slot.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
            let mut stop = None;
            if let Some(op) = ctl.opening.take() {
                stop = Some(op.stop);
            }
            (result, ctl.media_name.clone(), stop)
        };
        match result {
            Ok((source, first)) => {
                if let Err(e) = self.begin(source, first, Duration::ZERO, true, &name, PathBuf::new(), stop) {
                    self.fail(e.to_string());
                }
            }
            Err(e) => {
                let text = match e {
                    Error::Decode(t) => t,
                    other => other.to_string(),
                };
                self.fail(text);
            }
        }
    }

    /// Every 50 ms, on the engine thread: notices the end of the file and the first
    /// failure of the decoder. Quitting at once at the end would cut off what is
    /// already queued in the output, so the stream is drained first, with a deadline.
    pub fn tick(&self) {
        self.poll_opening();
        let mut guard = self.session.borrow_mut();
        let Some(s) = guard.as_mut() else { return };
        self.sync_advances_of(s);
        if let Some(e) = s.track.error.lock().ok().and_then(|e| e.clone()) {
            let mut shared_error = self.shared.error.lock().unwrap();
            if shared_error.is_none() {
                *shared_error = Some(e);
            }
        }
        if self.shared.error.lock().map(|e| e.is_some()).unwrap_or(false) {
            self.shared.set_state(State::Failed);
            return;
        }
        if s.live {
            let rate = u64::from(s.track.rate);
            match self.state() {
                State::Playing if s.track.starved.swap(false, Ordering::AcqRel) && !s.finished.load(Ordering::Acquire) => {
                    // The network could not keep up: wait for the pre-buffer, then go on.
                    s.stream.set_active(false);
                    self.shared.set_state(State::Buffering);
                }
                State::Buffering => {
                    let buffered = s.track.queued_frames.load(Ordering::Acquire).saturating_sub(s.track.frames.load(Ordering::Acquire));
                    if buffered >= rate * u64::from(self.shared.buffer_s.load(Ordering::Acquire)) || s.done.load(Ordering::Acquire) {
                        s.track.starved.store(false, Ordering::Release);
                        s.stream.set_active(true);
                        self.shared.set_state(State::Playing);
                    }
                }
                _ => {}
            }
        }
        if !matches!(self.state(), State::Playing | State::Paused) || !s.finished.load(Ordering::Acquire) {
            return;
        }
        if !s.flushed {
            s.flushed = true;
            s.deadline = Some(Instant::now() + Duration::from_secs(2));
            s.stream.flush();
        }
        if s.drained.load(Ordering::Acquire) || s.deadline.is_some_and(|d| Instant::now() > d) {
            // A queued file that the decoder could not splice in (it was not open yet, or has another format) starts now.
            let queued = self.next.lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some(n) = queued {
                let name = s.media_name.clone();
                drop(guard);
                self.stop_audio();
                self.shared.advanced.fetch_add(1, Ordering::AcqRel);
                *self.next_path.borrow_mut() = None;
                if let Err(e) = self.begin(n.source, n.first, Duration::ZERO, true, &name, n.path, None) {
                    self.fail(e.to_string());
                }
                return;
            }
            self.shared.set_state(State::Ended);
        }
    }
}

/// A file opened ahead of time, to be spliced in when the current one ends.
pub struct NextSource {
    source: Source,
    first: Vec<f32>,
    path: PathBuf,
}

/// Where the opened next file waits. The decoder takes it when its file ends; `halt` and the decoder's take exclude each
/// other through the mutex, so that a decoder that has been told to stop can never take a file the next session needs.
pub type NextSlot = Arc<Mutex<Option<NextSource>>>;

/// Writes samples into the ring buffer, waiting while it is full, and records which
/// frame of the file they stand for.
struct Writer {
    producer: Producer<f32>,
    channels: usize,
    stop: Arc<AtomicBool>,
    track: Arc<Track>,
    written: u64,
    /// What the marks written now say about the file.
    info: Info,
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
        self.track.queued_frames.store(self.written, Ordering::Release);
        if let Ok(mut marks) = self.track.marks.lock() {
            marks.advance(self.track.frames.load(Ordering::Acquire));
            marks.next.push_back(Mark { out: self.written, src: source_end, info: self.info, splice: false });
        }
        true
    }

    /// The next file begins with the next sample written.
    fn splice(&mut self, info: Info) {
        self.info = info;
        if let Ok(mut marks) = self.track.marks.lock() {
            marks.next.push_back(Mark { out: self.written, src: 0, info, splice: true });
        }
    }
}

/// A file that follows the current one in the decoder's stream, by position in the input (frames since the decoder began).
struct Upcoming {
    start: u64,
    info: Info,
}

/// The decoder thread: stays ahead of the callback until the file ends or it is told to stop.
/// At a speed other than 1.0 the audio goes through the time stretcher on its way. When the file ends and a next one is
/// waiting in `next` (same rate and channels), the audio simply goes on with it.
#[allow(clippy::too_many_arguments)]
fn feed(mut source: Source, first: Vec<f32>, producer: Producer<f32>, stop: Arc<AtomicBool>, done: Arc<AtomicBool>, shared: Arc<Shared>, track: Arc<Track>, next: NextSlot, live: bool) {
    let channels = source.channels as usize;
    let rate = source.rate;
    let info = track.marks.lock().map(|m| m.from.info).unwrap_or(Info { base_ms: 0, duration_ms: crate::engine::UNKNOWN, index: 0 });
    let mut writer = Writer { producer, channels, stop, track: track.clone(), written: 0, info };
    let mut stretcher: Option<Stretcher> = None;
    // All positions are in frames of the input since the decoder began, over all files: `consumed` is where the block in hand
    // starts, `file_start` where the current file started, `stretch_origin` where the stretcher's first input was.
    let mut consumed = 0u64;
    let mut file_start = 0u64;
    let mut stretch_origin = 0u64;
    // Where the analysis of the last written output stood; used to see when the output passes into the next file.
    let mut last_end = 0u64;
    let mut upcoming: VecDeque<Upcoming> = VecDeque::new();
    let mut pending = first;
    loop {
        let frames = (pending.len() / channels) as u64;
        // A live stream cannot be stretched: it would run dry.
        let speed = if live { 1.0 } else { f32::from_bits(shared.speed.load(Ordering::Acquire)) };
        if (speed - 1.0).abs() < 0.01 {
            if let Some(mut st) = stretcher.take() {
                let rest = st.take_rest();
                if !write_block(&mut writer, &mut upcoming, &mut file_start, &rest, last_end, consumed) {
                    return;
                }
            }
            if !write_block(&mut writer, &mut upcoming, &mut file_start, &pending, consumed, consumed + frames) {
                return;
            }
            last_end = consumed + frames;
        } else {
            let st = stretcher.get_or_insert_with(|| {
                stretch_origin = consumed;
                Stretcher::new(rate, channels)
            });
            st.set_speed(speed as f64);
            st.push(&pending);
            while let Some(block) = st.pull() {
                let end = (stretch_origin + st.position()).min(consumed + frames);
                if !write_block(&mut writer, &mut upcoming, &mut file_start, &block, last_end, end) {
                    return;
                }
                last_end = end;
            }
        }
        consumed += frames;
        match source.next_samples() {
            Ok(Some(samples)) => pending = samples,
            Ok(None) => {
                // The file is over. Is there a next one to go on with?
                let Some(n) = take_next(&next, &writer.stop, rate, channels) else { break };
                let index = upcoming.back().map_or(writer.info.index, |u| u.info.index) + 1;
                if let Ok(mut paths) = track.paths.lock() {
                    paths.push(n.path);
                }
                let duration_ms = n.source.duration.map_or(crate::engine::UNKNOWN, |d| d.as_millis() as u64);
                upcoming.push_back(Upcoming { start: consumed, info: Info { base_ms: 0, duration_ms, index } });
                source = n.source;
                pending = n.first;
            }
            Err(e) => {
                // A read that was cut short because the session ended is not a failure.
                if writer.stop.load(Ordering::Acquire) {
                    return;
                }
                *track.error.lock().unwrap() = Some(e.to_string());
                break;
            }
        }
    }
    if let Some(mut st) = stretcher {
        st.finish();
        while let Some(block) = st.pull() {
            if !write_block(&mut writer, &mut upcoming, &mut file_start, &block, last_end, consumed) {
                return;
            }
        }
    }
    done.store(true, Ordering::Release);
}

/// Writes one block; first marks the splice if the block's analysis starts in a file that follows the current one.
fn write_block(writer: &mut Writer, upcoming: &mut VecDeque<Upcoming>, file_start: &mut u64, block: &[f32], from: u64, to: u64) -> bool {
    while upcoming.front().is_some_and(|u| from >= u.start) {
        let u = upcoming.pop_front().unwrap();
        *file_start = u.start;
        writer.splice(u.info);
    }
    writer.write(block, to.saturating_sub(*file_start))
}

/// The next file, if there is one and it can follow without a change of format; `None` also when the session was stopped.
fn take_next(next: &NextSlot, stop: &AtomicBool, rate: u32, channels: usize) -> Option<NextSource> {
    let mut slot = next.lock().unwrap_or_else(|e| e.into_inner());
    if stop.load(Ordering::Acquire) {
        return None;
    }
    if slot.as_ref().is_some_and(|n| n.source.rate == rate && n.source.channels as usize == channels) {
        return slot.take();
    }
    None
}
