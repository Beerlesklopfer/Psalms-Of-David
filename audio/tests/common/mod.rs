// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Helpers of the tests that run on the null output (no sound card, no PipeWire).
#![allow(dead_code)]

use std::f32::consts::PI;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use psalm_audio::{Engine, NullCapture, NullMode, NullOptions, State};
use std::sync::Arc;

pub const RATE: u32 = 44_100;

/// A temporary directory that is removed at the end of the test.
pub struct Dir(pub PathBuf);

impl Dir {
    pub fn new(name: &str) -> Dir {
        let dir = std::env::temp_dir().join(format!("psalm-null-{name}-{}-{}", std::process::id(), std::thread::current().name().unwrap_or("t").replace("::", "_")));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The sample `n` of a 440 Hz sine of amplitude 0.5 (the same signal wherever a file starts in it).
pub fn sine_at(n: u64, hz: f32) -> f32 {
    0.5 * (2.0 * PI * hz * ((n % (RATE as u64 * 1000)) as f32) / RATE as f32).sin()
}

/// Interleaved samples of frames `from..to` of the sine, the same value on every channel.
pub fn sine(from: u64, to: u64, channels: usize, hz: f32) -> Vec<f32> {
    (from..to).flat_map(|n| std::iter::repeat_n(sine_at(n, hz), channels)).collect()
}

/// Writes a 32-bit float WAV file (lossless for the f32 samples).
pub fn write_wav(path: &Path, rate: u32, channels: u16, samples: &[f32]) {
    let data_len = (samples.len() * 4) as u32;
    let mut out = Vec::with_capacity(58 + samples.len() * 4);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(50 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&18u32.to_le_bytes());
    out.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * u32::from(channels) * 4).to_le_bytes());
    out.extend_from_slice(&(channels * 4).to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // cbSize
    out.extend_from_slice(b"fact");
    out.extend_from_slice(&4u32.to_le_bytes());
    out.extend_from_slice(&((samples.len() / usize::from(channels)) as u32).to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, out).unwrap();
}

/// A WAV file with frames `from..to` of the sine.
pub fn sine_wav(path: &Path, from: u64, to: u64, channels: u16, hz: f32) {
    write_wav(path, RATE, channels, &sine(from, to, usize::from(channels), hz));
}

pub fn engine(mode: NullMode) -> (Engine, Arc<NullCapture>) {
    let e = Engine::start_null(NullOptions { mode, file: None }).expect("null engine");
    let capture = e.null_capture().unwrap();
    (e, capture)
}

pub fn fast() -> (Engine, Arc<NullCapture>) {
    engine(NullMode::Accelerated)
}

pub fn wait_state(e: &Engine, wanted: State, secs: u64) -> bool {
    e.wait_for(|s| s == wanted, Duration::from_secs(secs))
}

/// Waits until `cond` holds; false after `secs`.
pub fn wait_until(secs: u64, cond: impl Fn() -> bool) -> bool {
    let end = std::time::Instant::now() + Duration::from_secs(secs);
    while std::time::Instant::now() < end {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    cond()
}

pub fn have(program: &str) -> bool {
    Command::new(program).arg("-version").output().is_ok() || Command::new(program).arg("--version").output().is_ok()
}

/// An MP3 of frames `from..to` of the sine, made from a WAV by ffmpeg (LAME encoder, with the LAME/Xing header).
pub fn sine_mp3(dir: &Dir, name: &str, from: u64, to: u64) -> Option<PathBuf> {
    let wav = dir.path(&format!("{name}.wav"));
    sine_wav(&wav, from, to, 2, 440.0);
    let mp3 = dir.path(&format!("{name}.mp3"));
    let ok = Command::new("ffmpeg").args(["-v", "error", "-y", "-i"]).arg(&wav).args(["-c:a", "libmp3lame", "-b:a", "192k"]).arg(&mp3).status().ok()?.success();
    ok.then_some(mp3)
}

/// The largest jump between two neighbouring frames (channel 0) of interleaved samples.
pub fn max_step(samples: &[f32], channels: usize) -> f32 {
    samples.chunks_exact(channels).zip(samples.chunks_exact(channels).skip(1)).map(|(a, b)| (a[0] - b[0]).abs()).fold(0.0, f32::max)
}
