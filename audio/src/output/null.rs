// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The null output: no sound card, no PipeWire. A pump thread per stream pulls the same `process`
//! closure that PipeWire would call, at real time or as fast as the decoder allows, and keeps the
//! real samples (padding after an underrun is not kept, but counted by the engine) in memory and
//! optionally appends them to a file as raw little-endian f32.

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::{CommandSink, OutStream, Output, ProcessFn, Started, StreamFactory, StreamSpec};
use crate::Error;
use crate::core::Core;
use crate::engine::{Command, Shared};

/// Frames per `process` call.
const PERIOD_FRAMES: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NullMode {
    /// One period of audio per period of time, like a sound card.
    Realtime,
    /// As fast as the decoder delivers; the tests use this.
    #[default]
    Accelerated,
    /// Real time multiplied by this factor (20 plays a minute in three seconds, with the buffers filling like in real time).
    Speedup(u32),
}

#[derive(Debug, Clone, Default)]
pub struct NullOptions {
    pub mode: NullMode,
    /// Append the samples (raw f32le, interleaved) to this file as well.
    pub file: Option<PathBuf>,
}

/// What the null output collected. Shared with the test that created the engine.
#[derive(Default)]
pub struct NullCapture {
    samples: Mutex<Vec<f32>>,
    /// Sample index (in `samples`) at which each stream started, with its rate and channel count.
    streams: Mutex<Vec<(usize, u32, u32)>>,
    /// Streams that were opened.
    opened: AtomicU64,
}

impl NullCapture {
    /// All real samples played so far, interleaved, over all streams in order.
    pub fn samples(&self) -> Vec<f32> {
        self.samples.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn len(&self) -> usize {
        self.samples.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Takes the samples collected so far and starts empty again.
    pub fn take(&self) -> Vec<f32> {
        std::mem::take(&mut *self.samples.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// `(first sample index, rate, channels)` of every stream that was opened.
    pub fn streams(&self) -> Vec<(usize, u32, u32)> {
        self.streams.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// How many streams (sessions) were opened.
    pub fn streams_opened(&self) -> u64 {
        self.opened.load(Ordering::Acquire)
    }
}

pub struct NullOutput {
    pub options: NullOptions,
    pub capture: Arc<NullCapture>,
}

impl Output for NullOutput {
    fn start(self: Box<Self>, shared: Arc<Shared>) -> Result<Started, Error> {
        let (tx, rx) = mpsc::channel::<Command>();
        let tx = Mutex::new(tx);
        let sink: CommandSink = Arc::new(move |cmd| tx.lock().map(|t| t.send(cmd).is_ok()).unwrap_or(false));
        let (ready_tx, ready_rx) = mpsc::channel();
        let thread = {
            let sink = sink.clone();
            thread::Builder::new()
                .name("psalm-null-output".into())
                .spawn(move || run(*self, rx, shared, sink, ready_tx))
                .map_err(|e| Error::PipeWire(format!("cannot start the audio thread: {e}")))?
        };
        match ready_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(()) => Ok(Started { sink, thread }),
            Err(_) => Err(Error::PipeWire("the null output did not start".into())),
        }
    }
}

fn run(output: NullOutput, rx: mpsc::Receiver<Command>, shared: Arc<Shared>, sink: CommandSink, ready: mpsc::Sender<()>) {
    let tick = match output.options.mode {
        NullMode::Realtime => Duration::from_millis(50),
        NullMode::Accelerated => Duration::from_millis(2),
        NullMode::Speedup(n) => Duration::from_millis((50 / u64::from(n.max(1))).max(2)),
    };
    let factory = Box::new(NullFactory { options: output.options, capture: output.capture });
    let core = Core::new(factory, shared, sink);
    let _ = ready.send(());
    let mut last = Instant::now();
    loop {
        match rx.recv_timeout(tick) {
            Ok(Command::Shutdown) => break,
            Ok(cmd) => core.handle(cmd),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if last.elapsed() >= tick {
            core.tick();
            last = Instant::now();
        }
    }
    core.shutdown();
}

struct NullFactory {
    options: NullOptions,
    capture: Arc<NullCapture>,
}

struct Pump {
    active: AtomicBool,
    stop: AtomicBool,
}

struct NullStream {
    pump: Arc<Pump>,
    drained: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl OutStream for NullStream {
    fn set_active(&self, active: bool) {
        self.pump.active.store(active, Ordering::Release);
    }
    fn flush(&self) {
        // Everything handed over was "played" at once.
        self.drained.store(true, Ordering::Release);
    }
    fn set_volume(&self, _volume: f32) {}
    fn disconnect(&self) {
        self.pump.stop.store(true, Ordering::Release);
    }
}

impl Drop for NullStream {
    fn drop(&mut self) {
        self.pump.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.lock().ok().and_then(|mut t| t.take()) {
            let _ = t.join();
        }
    }
}

impl StreamFactory for NullFactory {
    fn open(&self, spec: StreamSpec, mut process: ProcessFn) -> Result<Box<dyn OutStream>, Error> {
        let capture = self.capture.clone();
        capture.opened.fetch_add(1, Ordering::AcqRel);
        capture.streams.lock().unwrap_or_else(|e| e.into_inner()).push((capture.len(), spec.rate, spec.channels));
        let mut file = match &self.options.file {
            Some(path) => Some(std::io::BufWriter::new(
                std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(Error::Io)?,
            )),
            None => None,
        };
        let pump = Arc::new(Pump { active: AtomicBool::new(spec.active), stop: AtomicBool::new(false) });
        let (channels, rate) = (spec.channels as usize, spec.rate.max(1));
        let scale = match self.options.mode {
            NullMode::Realtime => Some(1.0),
            NullMode::Speedup(n) => Some(f64::from(n.max(1))),
            NullMode::Accelerated => None,
        };
        let thread = {
            let pump = pump.clone();
            thread::Builder::new()
                .name("psalm-null-pump".into())
                .spawn(move || {
                    let mut buffer = vec![0.0f32; PERIOD_FRAMES * channels];
                    let period = Duration::from_secs_f64(PERIOD_FRAMES as f64 / rate as f64 / scale.unwrap_or(1.0));
                    let mut next = Instant::now();
                    while !pump.stop.load(Ordering::Acquire) {
                        if !pump.active.load(Ordering::Acquire) {
                            thread::sleep(Duration::from_millis(1));
                            next = Instant::now();
                            continue;
                        }
                        let produced = process(&mut buffer);
                        if produced.real > 0 {
                            let real = &buffer[..produced.real * channels];
                            capture.samples.lock().unwrap_or_else(|e| e.into_inner()).extend_from_slice(real);
                            if let Some(f) = file.as_mut() {
                                for s in real {
                                    let _ = f.write_all(&s.to_le_bytes());
                                }
                            }
                        }
                        if scale.is_some() {
                            next += period;
                            let now = Instant::now();
                            if next > now {
                                thread::sleep(next - now);
                            } else {
                                next = now;
                            }
                        } else if produced.real == 0 {
                            // Nothing to play (underrun or the end): let the decoder work.
                            thread::sleep(Duration::from_millis(1));
                        }
                    }
                    if let Some(f) = file.as_mut() {
                        let _ = f.flush();
                    }
                })
                .map_err(|e| Error::PipeWire(format!("cannot start the null pump: {e}")))?
        };
        Ok(Box::new(NullStream { pump, drained: spec.drained, thread: Mutex::new(Some(thread)) }))
    }
}
