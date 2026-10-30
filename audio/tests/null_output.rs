// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The engine on the null output: decode, seek, speed, live stream back pressure, stop. No sound card and no
//! PipeWire are needed, so these run in a plain `cargo test`. (`scripts/verify*.sh` stay the smoke test of the real output.)

mod common;

use std::process::Command;
use std::time::Duration;

use common::*;
use psalm_audio::{NullMode, State};

#[test]
fn a_wav_plays_to_the_end_sample_for_sample() {
    let dir = Dir::new("wav");
    for channels in [1u16, 2] {
        let path = dir.path(&format!("tone{channels}.wav"));
        sine_wav(&path, 0, 30_000, channels, 440.0);
        let (e, capture) = fast();
        e.load(&path, Duration::ZERO, true, "tone").unwrap();
        assert!(wait_state(&e, State::Ended, 10), "ended, was {:?}: {:?}", e.state(), e.error());
        let played = capture.samples();
        assert_eq!(played.len(), 30_000 * usize::from(channels), "exact sample count");
        assert_eq!(played, sine(0, 30_000, usize::from(channels), 440.0), "exact content");
        assert_eq!(e.rate(), RATE);
        assert_eq!(e.frames_played(), 30_000);
        let duration = e.duration().expect("a WAV knows its length");
        assert_eq!(e.position(), duration, "the position ends at the length");
        assert!((duration.as_secs_f64() - 30_000.0 / f64::from(RATE)).abs() < 0.001);
    }
}

#[test]
fn an_mp3_plays_to_the_end_with_the_right_length() {
    if !have("ffmpeg") {
        return eprintln!("skipped: ffmpeg not found");
    }
    let dir = Dir::new("mp3");
    let Some(path) = sine_mp3(&dir, "a", 0, 3 * RATE as u64) else { return eprintln!("skipped: ffmpeg cannot make mp3") };
    let (e, capture) = fast();
    e.load(&path, Duration::ZERO, true, "mp3").unwrap();
    assert!(wait_state(&e, State::Ended, 10), "{:?}", e.error());
    let frames = capture.len() / 2;
    // With the LAME header the encoder delay and padding are trimmed: exactly the frames that were encoded.
    assert_eq!(frames, 3 * RATE as usize, "exact length of the MP3");
    let samples = capture.samples();
    let middle = &samples[(RATE as usize) * 2..(2 * RATE as usize) * 2];
    let peak = middle.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!((0.45..0.55).contains(&peak), "the sine is there, peak {peak}");
}

#[test]
fn starting_paused_plays_nothing_until_play() {
    let dir = Dir::new("paused");
    let path = dir.path("t.wav");
    sine_wav(&path, 0, 20_000, 2, 440.0);
    let (e, capture) = fast();
    e.load(&path, Duration::ZERO, false, "t").unwrap();
    assert!(wait_state(&e, State::Paused, 5));
    std::thread::sleep(Duration::from_millis(200));
    assert!(capture.is_empty(), "paused: nothing is pulled");
    e.play();
    assert!(wait_state(&e, State::Ended, 10));
    assert_eq!(capture.len(), 20_000 * 2);
}

#[test]
fn pausing_and_resuming_loses_and_repeats_nothing() {
    let dir = Dir::new("pause");
    let path = dir.path("t.wav");
    sine_wav(&path, 0, 2 * RATE as u64, 2, 440.0);
    let (e, capture) = engine(NullMode::Speedup(4));
    e.load(&path, Duration::ZERO, true, "t").unwrap();
    assert!(wait_until(5, || capture.len() > 20_000));
    e.pause();
    assert!(wait_state(&e, State::Paused, 2));
    std::thread::sleep(Duration::from_millis(100));
    let frozen = capture.len();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(capture.len(), frozen, "paused: nothing more is played");
    e.play();
    assert!(wait_state(&e, State::Ended, 10));
    assert_eq!(capture.samples(), sine(0, 2 * RATE as u64, 2, 440.0));
}

#[test]
fn seeking_continues_at_the_new_position_and_keeps_paused_or_playing() {
    let dir = Dir::new("seek");
    let path = dir.path("t.wav");
    let total = 2 * RATE as u64;
    sine_wav(&path, 0, total, 2, 440.0);
    let (e, capture) = fast();
    e.load(&path, Duration::ZERO, false, "t").unwrap();
    e.seek(Duration::from_millis(1000));
    assert!(wait_until(5, || e.position() >= Duration::from_millis(1000)));
    assert_eq!(e.state(), State::Paused, "seeking while paused stays paused");
    assert!(capture.is_empty());
    e.play();
    assert!(wait_state(&e, State::Ended, 10), "{:?}", e.error());
    let from = RATE as u64; // 1000 ms
    assert_eq!(capture.samples(), sine(from, total, 2, 440.0), "plays from the sought frame to the end");
    assert_eq!(e.position(), e.duration().unwrap());
}

#[test]
fn loading_with_a_start_and_seeking_while_playing() {
    let dir = Dir::new("seek2");
    let path = dir.path("t.wav");
    let total = 4 * RATE as u64;
    sine_wav(&path, 0, total, 2, 440.0);
    let (e, capture) = engine(NullMode::Speedup(2));
    e.load(&path, Duration::from_millis(500), true, "t").unwrap();
    assert!(wait_until(5, || capture.len() > 40_000));
    e.seek(Duration::from_millis(3000));
    assert!(wait_state(&e, State::Ended, 10), "{:?}", e.error());
    let samples = capture.samples();
    let streams = capture.streams();
    assert_eq!(streams.len(), 2, "a seek builds a second session");
    // The second session starts exactly at frame 3 s and runs to the end.
    let second = &samples[streams[1].0..];
    assert_eq!(second, sine(3 * RATE as u64, total, 2, 440.0));
    // The first one started at 0.5 s and was contiguous.
    let first = &samples[streams[0].0..streams[1].0];
    assert_eq!(first, sine(RATE as u64 / 2, RATE as u64 / 2 + (first.len() / 2) as u64, 2, 440.0));
}

/// Zero crossings per second of channel 0.
fn crossings(samples: &[f32], channels: usize) -> f64 {
    let ch0: Vec<f32> = samples.chunks_exact(channels).map(|f| f[0]).collect();
    let n = ch0.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
    n as f64 / (ch0.len() as f64 / f64::from(RATE))
}

#[test]
fn a_faster_speed_shortens_the_playback_and_keeps_the_pitch() {
    let dir = Dir::new("speed");
    let path = dir.path("t.wav");
    sine_wav(&path, 0, 4 * RATE as u64, 2, 440.0);
    for (speed, tolerance) in [(2.0f32, 0.06), (0.5, 0.06)] {
        let (e, capture) = fast();
        e.set_speed(speed);
        e.load(&path, Duration::ZERO, true, "t").unwrap();
        assert!(wait_state(&e, State::Ended, 15), "{:?}", e.error());
        let seconds = capture.len() as f64 / 2.0 / f64::from(RATE);
        let expected = 4.0 / f64::from(speed);
        assert!((seconds - expected).abs() < expected * tolerance + 0.1, "speed {speed}: {seconds} s instead of {expected}");
        let samples = capture.samples();
        let pitch = crossings(&samples[samples.len() / 4..samples.len() * 3 / 4], 2) / 2.0;
        assert!((pitch - 440.0).abs() < 15.0, "speed {speed}: the pitch stays at 440 Hz, was {pitch}");
        assert_eq!(e.position(), e.duration().unwrap(), "the position ends at the length of the file");
    }
}

#[test]
fn a_new_speed_applies_while_playing_and_the_position_stays_right() {
    let dir = Dir::new("speed2");
    let path = dir.path("t.wav");
    sine_wav(&path, 0, 6 * RATE as u64, 2, 440.0);
    let (e, capture) = engine(NullMode::Speedup(8));
    e.load(&path, Duration::ZERO, true, "t").unwrap();
    e.set_speed(2.0);
    assert!(wait_state(&e, State::Ended, 15));
    assert!(capture.len() / 2 < 6 * RATE as usize, "faster than normal after the change");
    assert_eq!(e.position(), e.duration().unwrap());
}

fn mp3_bytes(seconds: u32, kbit: u32) -> Option<Vec<u8>> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-f", "lavfi", "-i", &format!("sine=frequency=440:duration={seconds}:sample_rate=44100"), "-ac", "2", "-b:a", &format!("{kbit}k"), "-f", "mp3", "-"])
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

#[test]
fn a_live_stream_pushes_back_when_the_queue_is_full_and_plays_it_all() {
    let Some(data) = mp3_bytes(40, 320) else { return eprintln!("skipped: ffmpeg not found") };
    assert!(data.len() > 1_500_000, "the stream is larger than the queue");
    let (e, capture) = engine(NullMode::Speedup(20));
    e.load_stream(true, "radio").unwrap();
    let (mut rest, mut refused, mut taken) = (&data[..], 0u32, 0usize);
    let start = std::time::Instant::now();
    while !rest.is_empty() {
        let n = e.stream_push(rest).expect("push");
        assert!(n <= rest.len());
        if n < rest.len() {
            refused += 1;
        }
        taken += n;
        rest = &rest[n..];
        if n == 0 {
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(start.elapsed() < Duration::from_secs(30), "pushing does not hang");
    }
    assert_eq!(taken, data.len());
    assert!(refused > 0, "the queue pushed back at least once");
    e.stream_end().unwrap();
    assert!(wait_state(&e, State::Ended, 30), "{:?} {:?}", e.state(), e.error());
    let seconds = capture.len() as f64 / 2.0 / f64::from(RATE);
    assert!((39.0..41.0).contains(&seconds), "played {seconds} s of 40");
    assert_eq!(e.duration(), None, "a live stream has no length");
}

#[test]
fn a_live_stream_that_is_not_mp3_fails_with_the_reason() {
    let (e, _capture) = fast();
    e.load_stream(true, "radio").unwrap();
    e.stream_push(b"<html><body>this is a web page, not a stream</body></html>").unwrap();
    e.stream_end().unwrap();
    assert!(wait_state(&e, State::Failed, 5));
    assert_eq!(e.error().as_deref(), Some(psalm_audio::STREAM_UNSUPPORTED));
}

#[test]
fn stop_goes_idle_and_plays_nothing_more() {
    let dir = Dir::new("stop");
    let path = dir.path("t.wav");
    sine_wav(&path, 0, 10 * RATE as u64, 2, 440.0);
    let (e, capture) = engine(NullMode::Realtime);
    e.load(&path, Duration::ZERO, true, "t").unwrap();
    assert!(wait_until(5, || capture.len() > 10_000));
    e.stop();
    assert!(wait_state(&e, State::Idle, 2));
    std::thread::sleep(Duration::from_millis(100));
    let frozen = capture.len();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(capture.len(), frozen);
    assert!(frozen < 10 * RATE as usize * 2);
    // And the engine can load again after a stop.
    e.load(&path, Duration::from_millis(9_900), true, "t").unwrap();
    assert!(wait_state(&e, State::Ended, 5));
}

#[test]
fn a_missing_or_broken_file_fails_with_a_message_and_the_engine_recovers() {
    let dir = Dir::new("bad");
    let (e, _capture) = fast();
    let started = std::time::Instant::now();
    let id = e.load(&dir.path("nope.wav"), Duration::ZERO, true, "x").expect("the request is accepted");
    assert!(id >= 1);
    assert!(started.elapsed() < Duration::from_millis(100), "load does not wait for the file");
    assert!(wait_state(&e, State::Failed, 5), "was {:?}", e.state());
    assert!(e.error().unwrap().contains("nope.wav"), "{:?}", e.error());
    let junk = dir.path("junk.mp3");
    std::fs::write(&junk, vec![7u8; 4000]).unwrap();
    e.load(&junk, Duration::ZERO, true, "x").unwrap();
    assert!(wait_state(&e, State::Failed, 5));
    assert!(e.error().is_some());
    // A good file after a failure plays, and the old error is gone.
    let ok = dir.path("ok.wav");
    sine_wav(&ok, 0, 5000, 2, 440.0);
    e.load(&ok, Duration::ZERO, true, "x").unwrap();
    assert!(wait_state(&e, State::Ended, 5));
    assert_eq!(e.error(), None);
}

#[test]
fn load_returns_at_once_and_the_state_goes_through_loading() {
    let dir = Dir::new("loading");
    let path = dir.path("t.wav");
    sine_wav(&path, 0, 2 * RATE as u64, 2, 440.0);
    let (e, _capture) = engine(NullMode::Realtime);
    let id1 = e.load(&path, Duration::from_millis(250), false, "t").unwrap();
    assert_eq!(e.state(), State::Loading, "loading until the worker is done");
    assert_eq!(e.position(), Duration::from_millis(250), "the position is the start while loading");
    assert!(wait_state(&e, State::Paused, 5), "{:?}", e.state());
    let id2 = e.load(&path, Duration::ZERO, false, "t").unwrap();
    assert!(id2 > id1, "request ids grow");
}

/// A named pipe: opening it for reading blocks until somebody writes, like a file on a hung network drive.
fn fifo(dir: &Dir, name: &str) -> Option<std::path::PathBuf> {
    let path = dir.path(name);
    Command::new("mkfifo").arg(&path).status().ok()?.success().then_some(path)
}

/// Lets the blocked worker of `fifo` finish (it finds an empty file).
fn release(fifo: &std::path::Path) {
    let _ = std::fs::OpenOptions::new().write(true).open(fifo);
}

#[test]
fn a_file_that_never_opens_does_not_freeze_and_a_newer_load_replaces_it() {
    let dir = Dir::new("hung");
    let Some(hung) = fifo(&dir, "hung.mp3") else { return eprintln!("skipped: mkfifo not found") };
    let good = dir.path("good.wav");
    sine_wav(&good, 0, 20_000, 2, 440.0);
    let (e, capture) = fast();
    let t = std::time::Instant::now();
    e.load(&hung, Duration::ZERO, true, "hung").unwrap();
    assert!(t.elapsed() < Duration::from_millis(100), "the call returns while the file hangs");
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(e.state(), State::Loading, "still waiting for the file");
    // Everything else still answers.
    e.set_volume(0.5);
    assert_eq!(e.volume(), 0.5);
    // The second load replaces the first.
    e.load(&good, Duration::ZERO, true, "good").unwrap();
    assert!(wait_state(&e, State::Ended, 5), "was {:?}", e.state());
    assert_eq!(capture.samples(), sine(0, 20_000, 2, 440.0), "only the second file was played");
    assert_eq!(capture.streams_opened(), 1);
    // Releasing the hung worker later changes nothing: its answer is dropped.
    release(&hung);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(e.state(), State::Ended);
    assert_eq!(e.error(), None);
    // Dropping the engine does not wait for workers either.
    let hung2 = fifo(&dir, "hung2.mp3").unwrap();
    e.load(&hung2, Duration::ZERO, true, "hung").unwrap();
    let t = std::time::Instant::now();
    drop(e);
    assert!(t.elapsed() < Duration::from_secs(2), "drop took {:?}", t.elapsed());
    release(&hung2);
}

#[test]
fn stop_cancels_a_load_that_is_still_opening() {
    let dir = Dir::new("stoploading");
    let Some(hung) = fifo(&dir, "hung.mp3") else { return eprintln!("skipped: mkfifo not found") };
    let (e, capture) = fast();
    e.load(&hung, Duration::ZERO, true, "hung").unwrap();
    assert_eq!(e.state(), State::Loading);
    e.stop();
    assert_eq!(e.state(), State::Idle, "stop ends the loading at once");
    assert!(wait_state(&e, State::Idle, 2));
    // The worker's late answer (an empty file) must not turn into a failure or a session.
    release(&hung);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(e.state(), State::Idle);
    assert_eq!(e.error(), None);
    assert!(capture.is_empty());
}

#[test]
fn a_seek_play_or_pause_that_arrives_while_the_file_opens_is_not_lost() {
    let dir = Dir::new("intent");
    let path = dir.path("t.wav");
    let total = 2 * RATE as u64;
    sine_wav(&path, 0, total, 2, 440.0);
    for _ in 0..5 {
        let (e, capture) = fast();
        e.load(&path, Duration::ZERO, false, "t").unwrap();
        e.seek(Duration::from_millis(1000)); // usually arrives before the file is open
        e.play();
        assert!(wait_state(&e, State::Ended, 5), "{:?}", e.error());
        let samples = capture.samples();
        let streams = capture.streams();
        let last = &samples[streams.last().unwrap().0..];
        assert_eq!(last, sine(RATE as u64, total, 2, 440.0), "the last session starts at the sought frame and runs to the end");
    }
    let (e, _capture) = fast();
    e.load(&path, Duration::ZERO, true, "t").unwrap();
    e.pause();
    assert!(wait_until(5, || e.state() == State::Paused));
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(e.state(), State::Paused, "a pause during loading stays paused");
}

#[test]
fn changing_the_track_while_playing_replaces_it() {
    let dir = Dir::new("replace");
    let (a, b) = (dir.path("a.wav"), dir.path("b.wav"));
    sine_wav(&a, 0, 30 * RATE as u64, 2, 440.0);
    sine_wav(&b, 0, 10_000, 2, 660.0);
    let (e, capture) = engine(NullMode::Realtime);
    e.load(&a, Duration::ZERO, true, "a").unwrap();
    assert!(wait_until(5, || capture.len() > 20_000));
    e.load(&b, Duration::ZERO, true, "b").unwrap();
    assert_eq!(e.position(), Duration::ZERO, "the old position is gone at once");
    assert!(wait_state(&e, State::Ended, 5));
    let streams = capture.streams();
    assert_eq!(streams.len(), 2);
    let samples = capture.samples();
    assert_eq!(&samples[streams[1].0..], sine(0, 10_000, 2, 660.0));
    assert_eq!(e.duration().unwrap().as_millis(), 226);
}

#[test]
fn the_samples_can_also_go_to_a_file() {
    let dir = Dir::new("file");
    let path = dir.path("t.wav");
    sine_wav(&path, 0, 8000, 2, 440.0);
    let out = dir.path("out.f32");
    let e = psalm_audio::Engine::start_null(psalm_audio::NullOptions { mode: NullMode::Accelerated, file: Some(out.clone()) }).unwrap();
    e.load(&path, Duration::ZERO, true, "t").unwrap();
    assert!(wait_state(&e, State::Ended, 5));
    drop(e); // flushes the file
    assert_eq!(std::fs::metadata(&out).unwrap().len(), 8000 * 2 * 4);
}
