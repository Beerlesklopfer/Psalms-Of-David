// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Gapless playback on the null output: `enqueue_next` splices the next file in at the exact sample. Two files that are
//! consecutive pieces of one sine wave must come out as one continuous sine with exactly the sum of their lengths.

mod common;

use std::time::Duration;

use common::*;
use psalm_audio::{NullMode, State};

const R: u64 = RATE as u64;

/// Loads `a` paused, queues `b` and gives the worker time to open it, so that the splice is the decoder's (not the late fallback).
fn queued(e: &psalm_audio::Engine, a: &std::path::Path, b: &std::path::Path) {
    e.load(a, Duration::ZERO, false, "a").unwrap();
    e.enqueue_next(b).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(wait_state(e, State::Paused, 5), "{:?}", e.error());
}

#[test]
fn two_wavs_make_one_continuous_sine_with_the_exact_length() {
    let dir = Dir::new("g-wav");
    for channels in [1u16, 2] {
        let (a, b) = (dir.path(&format!("a{channels}.wav")), dir.path(&format!("b{channels}.wav")));
        // Odd lengths, and A longer than the 2 s buffer so the splice happens while A is still being played out.
        let (n1, n2) = (3 * R + 1234, 2 * R + 777);
        sine_wav(&a, 0, n1, channels, 440.0);
        sine_wav(&b, n1, n1 + n2, channels, 440.0);
        let (e, capture) = fast();
        queued(&e, &a, &b);
        assert_eq!(e.track_generation(), 0);
        e.play();
        assert!(wait_state(&e, State::Ended, 10), "{:?}", e.error());
        let played = capture.samples();
        assert_eq!(played.len() as u64, (n1 + n2) * u64::from(channels), "exact total length");
        assert_eq!(played, sine(0, n1 + n2, usize::from(channels), 440.0), "no lost, repeated or changed sample");
        assert_eq!(capture.streams_opened(), 1, "one session: the output was never interrupted");
        assert_eq!(e.track_generation(), 1);
        assert_eq!(e.position(), e.duration().unwrap());
        assert_eq!(e.duration().unwrap().as_millis(), (n2 * 1000 / R) as u128, "the length is the new file's");
    }
}

#[test]
fn the_counter_and_the_position_switch_when_the_new_file_is_reached() {
    let dir = Dir::new("g-gen");
    let (a, b) = (dir.path("a.wav"), dir.path("b.wav"));
    sine_wav(&a, 0, R, 2, 440.0);
    sine_wav(&b, R, 3 * R, 2, 440.0);
    let (e, capture) = engine(NullMode::Speedup(2));
    queued(&e, &a, &b);
    e.play();
    assert!(wait_until(5, || capture.len() > 2 * 10_000), "playing");
    assert_eq!(e.track_generation(), 0, "still in the first file");
    assert_eq!(e.duration().unwrap(), Duration::from_secs(1));
    assert!(e.position() < Duration::from_secs(1));
    assert!(wait_until(5, || e.track_generation() == 1), "switched");
    assert_eq!(e.duration().unwrap(), Duration::from_secs(2), "the length of the new file");
    assert!(e.position() < Duration::from_millis(1500), "the position is in the new file: {:?}", e.position());
    assert!(wait_state(&e, State::Ended, 5));
    assert_eq!(e.track_generation(), 1);
    assert_eq!(e.state(), State::Ended);
}

#[test]
fn a_chain_of_three_files_and_a_queue_that_is_refilled_after_each_switch() {
    let dir = Dir::new("g-chain");
    let files: Vec<_> = (0..3).map(|i| dir.path(&format!("{i}.wav"))).collect();
    let n = R + 500;
    for (i, f) in files.iter().enumerate() {
        sine_wav(f, i as u64 * n, (i as u64 + 1) * n, 2, 440.0);
    }
    let (e, capture) = engine(NullMode::Speedup(2));
    queued(&e, &files[0], &files[1]);
    e.play();
    assert!(wait_until(5, || e.track_generation() == 1));
    e.enqueue_next(&files[2]).unwrap(); // what a player does when it is told about the switch
    assert!(wait_state(&e, State::Ended, 10), "{:?}", e.error());
    assert_eq!(e.track_generation(), 2);
    assert_eq!(capture.samples(), sine(0, 3 * n, 2, 440.0));
}

#[test]
fn without_a_queued_file_the_end_is_the_end() {
    let dir = Dir::new("g-none");
    let (a, b) = (dir.path("a.wav"), dir.path("b.wav"));
    sine_wav(&a, 0, R, 2, 440.0);
    sine_wav(&b, R, 2 * R, 2, 440.0);
    let (e, capture) = fast();
    // Cleared.
    e.load(&a, Duration::ZERO, false, "a").unwrap();
    e.enqueue_next(&b).unwrap();
    e.clear_next();
    std::thread::sleep(Duration::from_millis(200));
    e.play();
    assert!(wait_state(&e, State::Ended, 5));
    assert_eq!(capture.len() as u64, R * 2);
    assert_eq!(e.track_generation(), 0);
    // A load drops the queue.
    e.enqueue_next(&b).unwrap();
    e.load(&a, Duration::ZERO, true, "a").unwrap();
    assert!(wait_until(5, || capture.len() as u64 >= R * 4));
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(capture.len() as u64, R * 4, "only a was played again");
    assert_eq!(e.track_generation(), 0);
}

#[test]
fn a_newer_queue_replaces_the_older_one() {
    let dir = Dir::new("g-replace");
    let (a, b, c) = (dir.path("a.wav"), dir.path("b.wav"), dir.path("c.wav"));
    sine_wav(&a, 0, R, 2, 440.0);
    sine_wav(&b, R, 2 * R, 2, 440.0);
    sine_wav(&c, R, 2 * R + 100, 2, 440.0);
    let (e, capture) = fast();
    e.load(&a, Duration::ZERO, false, "a").unwrap();
    e.enqueue_next(&b).unwrap();
    e.enqueue_next(&c).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    e.play();
    assert!(wait_state(&e, State::Ended, 5));
    assert_eq!(capture.samples(), sine(0, 2 * R + 100, 2, 440.0), "c followed, b did not");
}

#[test]
fn a_next_file_that_cannot_be_opened_lets_the_first_end_normally() {
    let dir = Dir::new("g-bad");
    let a = dir.path("a.wav");
    sine_wav(&a, 0, R, 2, 440.0);
    let junk = dir.path("junk.mp3");
    std::fs::write(&junk, vec![3u8; 3000]).unwrap();
    for next in [dir.path("missing.wav"), junk] {
        let (e, capture) = fast();
        e.load(&a, Duration::ZERO, false, "a").unwrap();
        e.enqueue_next(&next).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        e.play();
        assert!(wait_state(&e, State::Ended, 5), "{:?}", e.state());
        assert_eq!(e.error(), None, "a bad next file is not an error of the playback");
        assert_eq!(capture.len() as u64, 2 * R);
        assert_eq!(e.track_generation(), 0);
    }
}

#[test]
fn a_file_queued_too_late_still_follows_without_losing_a_sample() {
    let dir = Dir::new("g-late");
    let (a, b) = (dir.path("a.wav"), dir.path("b.wav"));
    sine_wav(&a, 0, R + 100, 2, 440.0);
    sine_wav(&b, R + 100, 2 * R, 2, 440.0);
    let (e, capture) = engine(NullMode::Realtime);
    e.load(&a, Duration::ZERO, true, "a").unwrap();
    // The decoder has long finished a (1 s fits the buffer) when b is queued; the engine thread starts b when a has played out.
    std::thread::sleep(Duration::from_millis(400));
    e.enqueue_next(&b).unwrap();
    assert!(wait_state(&e, State::Ended, 5), "{:?}", e.state());
    assert_eq!(capture.samples(), sine(0, 2 * R, 2, 440.0));
    assert_eq!(e.track_generation(), 1);
}

#[test]
fn a_file_with_another_format_follows_in_a_new_session() {
    let dir = Dir::new("g-format");
    let (a, b) = (dir.path("a.wav"), dir.path("b.wav"));
    sine_wav(&a, 0, R, 2, 440.0);
    write_wav(&b, 22_050, 1, &sine(0, 22_050, 1, 220.0));
    let (e, capture) = fast();
    queued(&e, &a, &b);
    e.play();
    assert!(wait_state(&e, State::Ended, 5), "{:?}", e.error());
    assert_eq!(e.track_generation(), 1);
    let streams = capture.streams();
    assert_eq!(streams.len(), 2, "the output is reopened for the other format");
    assert_eq!((streams[1].1, streams[1].2), (22_050, 1));
    assert_eq!(capture.len(), (R * 2 + 22_050) as usize);
}

#[test]
fn seeking_after_the_switch_stays_in_the_new_file() {
    let dir = Dir::new("g-seek");
    let (a, b) = (dir.path("a.wav"), dir.path("b.wav"));
    sine_wav(&a, 0, R, 2, 440.0);
    sine_wav(&b, R, 5 * R, 2, 440.0);
    let (e, capture) = engine(NullMode::Speedup(2));
    queued(&e, &a, &b);
    e.play();
    assert!(wait_until(5, || e.track_generation() == 1));
    e.seek(Duration::from_millis(2500));
    assert!(wait_state(&e, State::Ended, 10), "{:?}", e.error());
    let streams = capture.streams();
    let last = &capture.samples()[streams.last().unwrap().0..];
    let want = sine(R + 2 * R + R / 2, 5 * R, 2, 440.0);
    assert_eq!(last.len(), want.len(), "length");
    assert!(last == want, "the seek is relative to b");
    assert_eq!(e.track_generation(), 1, "a seek is not an advance");
}

#[test]
fn a_seek_does_not_lose_the_queued_file() {
    let dir = Dir::new("g-seek2");
    let (a, b) = (dir.path("a.wav"), dir.path("b.wav"));
    sine_wav(&a, 0, R, 2, 440.0);
    sine_wav(&b, R, 2 * R, 2, 440.0);
    let (e, capture) = fast();
    queued(&e, &a, &b); // a is short: the decoder has already taken b into its pipeline
    e.seek(Duration::from_millis(200));
    std::thread::sleep(Duration::from_millis(300));
    e.play();
    assert!(wait_state(&e, State::Ended, 5), "{:?}", e.error());
    assert_eq!(e.track_generation(), 1);
    let streams = capture.streams();
    let last = &capture.samples()[streams.last().unwrap().0..];
    assert_eq!(last, sine(R / 5, 2 * R, 2, 440.0), "a from 200 ms, then b");
}

#[test]
fn gapless_works_with_a_faster_speed() {
    let dir = Dir::new("g-speed");
    let (a, b) = (dir.path("a.wav"), dir.path("b.wav"));
    sine_wav(&a, 0, 2 * R, 2, 440.0);
    sine_wav(&b, 2 * R, 4 * R, 2, 440.0);
    let (e, capture) = fast();
    e.set_speed(2.0);
    queued(&e, &a, &b);
    e.play();
    assert!(wait_state(&e, State::Ended, 10), "{:?}", e.error());
    assert_eq!(e.track_generation(), 1);
    let samples = capture.samples();
    let seconds = samples.len() as f64 / 2.0 / f64::from(RATE);
    assert!((1.9..2.2).contains(&seconds), "4 s at speed 2 take about 2 s, took {seconds}");
    // Continuous and with the pitch kept; the time stretcher itself makes no jumps, the seam must not either.
    let step = max_step(&samples, 2);
    assert!(step < 0.12, "no jump at the seam, the largest step is {step}");
}

#[test]
fn two_mp3s_cut_from_one_sine_join_without_a_gap_or_a_duplicate() {
    if !have("ffmpeg") {
        return eprintln!("skipped: ffmpeg not found");
    }
    let dir = Dir::new("g-mp3");
    let (n1, n2) = (3 * R + 321, 3 * R + 654);
    let (Some(a), Some(b)) = (sine_mp3(&dir, "a", 0, n1), sine_mp3(&dir, "b", n1, n1 + n2)) else { return eprintln!("skipped: no mp3 encoder") };
    let (e, capture) = fast();
    queued(&e, &a, &b);
    e.play();
    assert!(wait_state(&e, State::Ended, 10), "{:?}", e.error());
    let played = capture.samples();
    assert_eq!(played.len() as u64, (n1 + n2) * 2, "the encoder delay and padding are trimmed: the length is exact");
    assert_eq!(capture.streams_opened(), 1);
    assert_eq!(e.track_generation(), 1);
    // The seam: the largest step around it stays near the natural one of the sine (0.5 * 2 pi 440 / 44100 = 0.031).
    let seam = n1 as usize;
    let around = &played[(seam - 200) * 2..(seam + 200) * 2];
    let step = max_step(around, 2);
    // The lossy encoder starts the second file from nothing, so its first frames are not exact (about 0.12 measured, against 0.03
    // for the pure sine); a hole of the encoder delay would be near silence and shows in the rms below.
    assert!(step < 0.25, "no gap or jump at the seam, the largest step is {step}");
    // And no hole: the energy right after the seam is that of the sine (a missing delay would leave ~1100 near-silent frames).
    let after = &played[seam * 2..(seam + 1000) * 2];
    let rms = (after.iter().map(|s| s * s).sum::<f32>() / after.len() as f32).sqrt();
    assert!((0.30..0.40).contains(&rms), "rms after the seam {rms} (0.354 for the sine)");
}
