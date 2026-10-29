// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! How long it takes to change from one track to the next in the real engine and a real PipeWire sink.
//!
//! Ignored by default like `stream.rs`: it needs PipeWire and a sink that is not a speaker (`PSALM_TEST_SINK`, a null sink;
//! `scripts/verify-switch.sh` makes one).

use std::process::Command;
use std::time::{Duration, Instant};

use psalm_audio::{Engine, EngineOptions, State};

fn engine() -> Engine {
    let target = std::env::var("PSALM_TEST_SINK").expect("set PSALM_TEST_SINK to a null sink (see scripts/verify-switch.sh)");
    Engine::start(EngineOptions { target: Some(target), role: Some("Test".into()) }).expect("PipeWire")
}

fn file(dir: &std::path::Path, name: &str, hz: u32) -> std::path::PathBuf {
    let path = dir.join(name);
    let status = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", &format!("sine=frequency={hz}:duration=60:sample_rate=44100"), "-ac", "2", "-b:a", "128k"])
        .arg(&path)
        .status()
        .expect("ffmpeg");
    assert!(status.success());
    path
}

#[test]
#[ignore = "needs PipeWire and PSALM_TEST_SINK"]
fn changing_the_track_while_playing_does_not_wait_for_the_buffer_to_run_empty() {
    let dir = std::env::temp_dir().join(format!("psalm-switch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (a, b) = (file(&dir, "a.mp3", 440), file(&dir, "b.mp3", 660));
    let e = engine();
    e.load(&a, Duration::ZERO, true, "a").unwrap();
    assert!(e.wait_for(|s| s == State::Playing, Duration::from_secs(5)));
    std::thread::sleep(Duration::from_millis(1500)); // the buffer is full and playing
    let t = Instant::now();
    e.load(&b, Duration::ZERO, true, "b").unwrap();
    let call = t.elapsed();
    while e.frames_played() == 0 && t.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(2));
    }
    let first_sound = t.elapsed();
    println!("load() took {call:?}, the first frame of the new track was played after {first_sound:?}");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(call < Duration::from_millis(500), "load took {call:?}");
    assert!(first_sound < Duration::from_millis(800), "first sound after {first_sound:?}");
}

#[test]
#[ignore = "needs PipeWire and PSALM_TEST_SINK"]
fn the_size_of_the_buffer_is_set_in_seconds_and_does_not_slow_down_a_change() {
    let dir = std::env::temp_dir().join(format!("psalm-buffer-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (a, b) = (file(&dir, "a.mp3", 440), file(&dir, "b.mp3", 660));
    let e = engine();
    assert_eq!(e.buffer_seconds(), psalm_audio::DEFAULT_BUFFER_SECONDS);
    e.set_buffer_seconds(0);
    assert_eq!(e.buffer_seconds(), *psalm_audio::BUFFER_SECONDS.start(), "too small is the smallest");
    e.set_buffer_seconds(1000);
    assert_eq!(e.buffer_seconds(), *psalm_audio::BUFFER_SECONDS.end(), "too big is the biggest");
    e.set_buffer_seconds(20);
    e.load(&a, Duration::ZERO, true, "a").unwrap();
    assert!(e.wait_for(|s| s == State::Playing, Duration::from_secs(5)));
    std::thread::sleep(Duration::from_millis(1500));
    let t = Instant::now();
    e.load(&b, Duration::ZERO, true, "b").unwrap();
    while e.frames_played() == 0 && t.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(2));
    }
    let first_sound = t.elapsed();
    println!("with a buffer of 20 s the first frame of the new track was played after {first_sound:?}");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(first_sound < Duration::from_millis(800), "first sound after {first_sound:?}");
}
