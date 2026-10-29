// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Live streams through the real engine and a real PipeWire sink.
//!
//! These tests need PipeWire and a sink that is not a speaker, so they are ignored by
//! default. `scripts/verify-stream.sh` makes a null sink, sets `PSALM_TEST_SINK` to its
//! node name and runs `cargo test --release --test stream -- --ignored`.
//! `PSALM_TEST_STREAM_URL` (and `curl`) additionally enables the test against a real station.

use std::process::{Command, Stdio};
use std::io::Read;
use std::thread;
use std::time::{Duration, Instant};

use psalm_audio::{Engine, EngineOptions, State};

fn engine() -> Engine {
    let target = std::env::var("PSALM_TEST_SINK").expect("set PSALM_TEST_SINK to a null sink (see scripts/verify-stream.sh)");
    Engine::start(EngineOptions { target: Some(target), role: Some("Test".into()) }).expect("PipeWire")
}

fn mp3(seconds: u32) -> Vec<u8> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-f", "lavfi", "-i", &format!("sine=frequency=440:duration={seconds}:sample_rate=44100"), "-ac", "2", "-b:a", "128k", "-f", "mp3", "-"])
        .output()
        .expect("ffmpeg");
    assert!(out.status.success());
    out.stdout
}

/// Offers everything, waiting while the queue is full.
fn push_all(engine: &Engine, mut data: &[u8]) {
    while !data.is_empty() {
        let n = engine.stream_push(data).expect("push");
        data = &data[n..];
        if n == 0 {
            thread::sleep(Duration::from_millis(5));
        }
    }
}

fn wait(engine: &Engine, wanted: impl Fn(State) -> bool, secs: u64) -> bool {
    engine.wait_for(wanted, Duration::from_secs(secs))
}

#[test]
#[ignore = "needs PipeWire and PSALM_TEST_SINK"]
fn a_pushed_stream_plays_to_its_end() {
    let e = engine();
    e.load_stream(true, "test stream").unwrap();
    assert!(matches!(e.state(), State::Buffering | State::Playing));
    push_all(&e, &mp3(4));
    e.stream_end().unwrap();
    assert!(wait(&e, |s| s == State::Playing, 5), "playing, was {:?}: {:?}", e.state(), e.error());
    assert_eq!(e.duration(), None, "a live stream has no length");
    e.seek(Duration::from_secs(1)); // no effect
    assert!(wait(&e, |s| s == State::Ended, 15), "ended, was {:?}: {:?}", e.state(), e.error());
    let seconds = e.frames_played() as f64 / e.rate() as f64;
    assert!((3.5..=4.3).contains(&seconds), "played {seconds} s");
}

#[test]
#[ignore = "needs PipeWire and PSALM_TEST_SINK"]
fn running_dry_buffers_and_playing_resumes_by_itself() {
    let e = engine();
    let data = mp3(10);
    e.load_stream(true, "test stream").unwrap();
    push_all(&e, &data[..data.len() / 4]); // about 2.5 s
    assert!(wait(&e, |s| s == State::Playing, 5), "state {:?}", e.state());
    assert!(wait(&e, |s| s == State::Buffering, 8), "ran dry, state {:?}", e.state());
    push_all(&e, &data[data.len() / 4..data.len() / 2]);
    assert!(wait(&e, |s| s == State::Playing, 5), "resumed, state {:?}", e.state());
    e.stop();
}

#[test]
#[ignore = "needs PipeWire and PSALM_TEST_SINK"]
fn pausing_keeps_the_connection_and_resuming_goes_live() {
    let e = engine();
    let data = mp3(10);
    e.load_stream(true, "test stream").unwrap();
    push_all(&e, &data[..data.len() / 2]);
    assert!(wait(&e, |s| s == State::Playing, 5));
    e.pause();
    assert!(wait(&e, |s| s == State::Paused, 2), "state {:?} {:?}", e.state(), e.error());
    // Far more than the queue holds: every push is still accepted, nothing blocks.
    let started = Instant::now();
    for _ in 0..40 {
        let n = e.stream_push(&data).expect("the connection stays open while paused");
        assert_eq!(n, data.len(), "a paused live stream discards instead of pushing back");
    }
    assert!(started.elapsed() < Duration::from_secs(2));
    e.play();
    thread::scope(|scope| {
        // Keep the "broadcast" going after the resume.
        let feeder = scope.spawn(|| {
            for _ in 0..4 {
                push_all(&e, &data);
            }
        });
        assert!(wait(&e, |s| s == State::Playing, 8), "state {:?}: {:?}", e.state(), e.error());
        feeder.join().unwrap();
    });
    e.stop();
}

#[test]
#[ignore = "needs PipeWire and PSALM_TEST_SINK"]
fn data_that_is_not_mp3_fails_with_a_clear_message() {
    let e = engine();
    e.load_stream(true, "test stream").unwrap();
    push_all(&e, b"<html><body>This is a web page, not a station.</body></html>");
    e.stream_end().unwrap();
    assert!(wait(&e, |s| s == State::Failed, 5), "state {:?}", e.state());
    assert_eq!(e.error().as_deref(), Some("stream type not supported"));
}

#[test]
#[ignore = "needs PipeWire and PSALM_TEST_SINK"]
fn a_failure_of_the_sender_becomes_the_error() {
    let e = engine();
    e.load_stream(true, "test stream").unwrap();
    e.stream_fail("HTTP 404").unwrap();
    assert!(wait(&e, |s| s == State::Failed, 5));
    assert!(e.error().unwrap().contains("HTTP 404"));
}

#[test]
#[ignore = "needs PipeWire and PSALM_TEST_SINK"]
fn closing_in_every_state_does_not_hang() {
    let started = Instant::now();
    for step in 0..3 {
        let e = engine();
        e.load_stream(true, "test stream").unwrap();
        if step >= 1 {
            push_all(&e, &mp3(2));
            wait(&e, |s| s == State::Playing, 5);
        }
        if step == 2 {
            e.pause();
        }
        drop(e);
    }
    assert!(started.elapsed() < Duration::from_secs(30));
}

/// ~8 s of a real station, fetched with curl (no ICY metadata is requested, so the
/// body is plain MP3). Nothing is stored.
#[test]
#[ignore = "needs network, PipeWire, PSALM_TEST_SINK and PSALM_TEST_STREAM_URL"]
fn a_real_station_plays() {
    let Ok(url) = std::env::var("PSALM_TEST_STREAM_URL") else { return println!("skipped: PSALM_TEST_STREAM_URL not set") };
    let e = engine();
    e.load_stream(true, "real station").unwrap();
    let mut curl = Command::new("curl")
        .args(["-s", "-N", "-m", "9", &url])
        .stdout(Stdio::piped())
        .spawn()
        .expect("curl");
    let mut body = curl.stdout.take().unwrap();
    let started = Instant::now();
    let mut buf = [0u8; 4096];
    let mut total = 0usize;
    let mut was_playing = false;
    while started.elapsed() < Duration::from_secs(8) {
        let n = body.read(&mut buf).expect("read");
        if n == 0 {
            break;
        }
        push_all(&e, &buf[..n]);
        total += n;
        was_playing |= e.state() == State::Playing;
    }
    let _ = curl.kill();
    let _ = curl.wait();
    println!("received {total} bytes, state {:?}, frames {}, rate {}, underruns {}", e.state(), e.frames_played(), e.rate(), e.underrun_frames());
    assert!(total > 100_000, "bytes arrived");
    assert!(was_playing || e.state() == State::Playing, "state {:?}: {:?}", e.state(), e.error());
    assert!(e.frames_played() > e.rate() as u64 * 3, "decoded frames arrived");
    assert_eq!(e.duration(), None);
}
