// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Where the audio goes. The engine core (decoder, ring buffer, time stretch, session state) knows
//! nothing about PipeWire: it talks to an [`Output`], which owns the engine thread and its command
//! loop, and to the [`StreamFactory`] the output hands to the core on that thread.
//!
//! - [`pipewire`]: the real thing (a PipeWire main loop; the `process` closure runs on the real-time thread),
//! - [`null`]: a thread that pulls the same `process` closure at real or accelerated time and keeps
//!   the samples in memory (and optionally in a file). It needs no sound card and no PipeWire, so decode, seek,
//!   speed, back pressure and gapless run in plain `cargo test`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::thread::JoinHandle;

use crate::Error;
use crate::engine::{Command, Shared};

pub mod null;
pub mod pipewire;

/// What one call of the `process` closure produced.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Produced {
    /// Frames the output has to play, including padding with silence after an underrun.
    pub frames: usize,
    /// Frames of real audio among them (the rest is padding).
    pub real: usize,
}

/// Fills `out` (interleaved f32, `out.len()` is a multiple of the channel count) with the next audio.
/// Runs on the real-time thread of the output: no allocation, no locking, no file access.
pub type ProcessFn = Box<dyn FnMut(&mut [f32]) -> Produced + Send>;

/// How to open one stream.
pub struct StreamSpec {
    pub rate: u32,
    pub channels: u32,
    pub media_name: String,
    /// Does the stream play at once (otherwise it starts inactive)?
    pub active: bool,
    /// The volume to apply at once, if the caller ever set one.
    pub volume: Option<f32>,
    /// The output stores the volume it has (f32 bits) here when the system changes it from outside.
    pub volume_report: Arc<AtomicU32>,
    /// Set by the output when everything handed over has been played (after `flush`).
    pub drained: Arc<AtomicBool>,
}

/// One open stream; dropping it ends it.
pub trait OutStream {
    fn set_active(&self, active: bool);
    /// Asks the output to play out what it has; it sets `drained` when done.
    fn flush(&self);
    fn set_volume(&self, volume: f32);
    /// Cuts the connection now (dropping does the rest).
    fn disconnect(&self);
}

/// Makes streams. Lives on the engine thread.
pub trait StreamFactory {
    fn open(&self, spec: StreamSpec, process: ProcessFn) -> Result<Box<dyn OutStream>, Error>;
}

/// Sends a command to the engine thread; false if that thread is gone.
pub type CommandSink = Arc<dyn Fn(Command) -> bool + Send + Sync>;

/// A started engine thread.
pub struct Started {
    pub sink: CommandSink,
    pub thread: JoinHandle<()>,
}

/// An output owns the engine thread: it creates the command channel, runs the loop that feeds
/// [`crate::core::Core::handle`] and calls [`crate::core::Core::tick`] regularly.
pub trait Output: Send + 'static {
    /// Starts the thread and returns when it is ready (or why it cannot be).
    fn start(self: Box<Self>, shared: Arc<Shared>) -> Result<Started, Error>;
}
