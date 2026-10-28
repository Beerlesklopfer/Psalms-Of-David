// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Audio output for Psalm on Linux, through PipeWire.
//!
//! The design is described in `docs/spec-audio-pipewire.md`:
//!
//! - a decoder thread reads the file with symphonia and fills a lock-free ring buffer,
//! - the PipeWire loop thread owns the stream; its `process` callback only copies
//!   from the ring buffer (no allocation, no locking, no file access),
//! - the stream declares the file's own rate and channel count, PipeWire resamples.
//!
//! `Engine` is the controllable player (load, play, pause, seek, volume, position).
//! `play_file` plays one file to its end and is what the command line example uses.

mod engine;
pub mod ffi;
mod source;
mod stretch;

pub use engine::{Engine, EngineOptions, State};

use std::path::Path;
use std::time::Duration;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Decode(String),
    PipeWire(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io: {e}"),
            Error::Decode(e) => write!(f, "decode: {e}"),
            Error::PipeWire(e) => write!(f, "pipewire: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<pipewire::Error> for Error {
    fn from(e: pipewire::Error) -> Self {
        Error::PipeWire(e.to_string())
    }
}

/// What to play and where to send it.
#[derive(Debug, Clone, Default)]
pub struct PlayOptions {
    /// Name shown in mixers such as pavucontrol, e.g. "Kinder der Freiheit - Ken Follett".
    pub media_name: String,
    /// Node name or object serial of the sink. `None` lets the session manager choose.
    pub target: Option<String>,
}

/// Result of a finished playback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub rate: u32,
    pub frames_played: u64,
    pub underrun_frames: u64,
}

/// Plays `path` to the end and returns when everything has been played.
pub fn play_file(path: &Path, options: &PlayOptions) -> Result<Report, Error> {
    let engine = Engine::start(EngineOptions { target: options.target.clone(), role: None })?;
    engine.load(path, Duration::ZERO, true, &options.media_name)?;
    engine.wait_for(|s| !matches!(s, State::Playing | State::Paused), Duration::from_secs(60 * 60 * 24));
    if engine.state() == State::Failed {
        return Err(Error::Decode(engine.error().unwrap_or_else(|| "playback failed".into())));
    }
    let rate = engine.rate();
    Ok(Report { rate, frames_played: engine.frames_played(), underrun_frames: engine.underrun_frames() })
}
