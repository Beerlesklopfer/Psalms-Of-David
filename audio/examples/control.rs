// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Drives the engine by a timetable and logs the position, so that the behaviour
//! can be checked against a recording of the sink.
//!
//! `control <file> [--target SINK] [--paused] --script "3000:pause,6000:play,9000:quit"`
//!
//! Actions: `play`, `pause`, `seek=<ms>`, `volume=<0..1>`, `speed=<0.5..3>`, `quit`. Output lines:
//! `POS <elapsed ms> <position ms> <state> <volume> <underruns>` every 100 ms and
//! `DO <elapsed ms> <action>` when an action runs.
use std::path::PathBuf;
use std::time::{Duration, Instant};

use psalm_audio::{Engine, EngineOptions};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(file) = args.next() else {
        eprintln!("usage: control <file> [--target SINK] [--paused] --script \"ms:action,...\"");
        std::process::exit(2);
    };
    let (mut target, mut script, mut paused) = (None, String::new(), false);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--target" => target = args.next(),
            "--script" => script = args.next().unwrap_or_default(),
            "--paused" => paused = true,
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }
    let mut actions: Vec<(u64, String)> = script
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| {
            let (ms, action) = s.split_once(':').expect("ms:action");
            (ms.trim().parse().expect("milliseconds"), action.trim().to_owned())
        })
        .collect();
    actions.sort_by_key(|(ms, _)| *ms);

    let engine = Engine::start(EngineOptions { target, role: None }).expect("PipeWire is not available");
    engine.load(&PathBuf::from(&file), Duration::ZERO, !paused, "Psalm control test").expect("cannot load the file");
    let began = Instant::now();
    let mut next_action = 0;
    let mut last_log = 0u64;
    loop {
        let elapsed = began.elapsed().as_millis() as u64;
        while next_action < actions.len() && actions[next_action].0 <= elapsed {
            let action = &actions[next_action].1;
            println!("DO {elapsed} {action}");
            match action.as_str() {
                "play" => engine.play(),
                "pause" => engine.pause(),
                "quit" => return,
                a if a.starts_with("seek=") => engine.seek(Duration::from_millis(a[5..].parse().expect("ms"))),
                a if a.starts_with("speed=") => engine.set_speed(a[6..].parse().expect("speed")),
                a if a.starts_with("volume=") => engine.set_volume(a[7..].parse().expect("volume")),
                other => panic!("unknown action {other}"),
            }
            next_action += 1;
        }
        if elapsed >= last_log + 100 {
            last_log = elapsed;
            println!(
                "POS {elapsed} {} {:?} {:.2} {}",
                engine.position().as_millis(),
                engine.state(),
                engine.volume(),
                engine.underrun_frames()
            );
        }
        if actions.len() == next_action && next_action > 0 && elapsed > actions.last().map_or(0, |a| a.0) + 500 {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
