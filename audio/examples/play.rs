// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! `cargo run --example play -- <file> [--target <sink node name>]`
use std::path::PathBuf;

use psalm_audio::{PlayOptions, play_file};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(file) = args.next() else {
        eprintln!("usage: play <file> [--target <sink>]");
        std::process::exit(2);
    };
    let mut options = PlayOptions { media_name: "Psalm test".into(), target: None };
    while let Some(a) = args.next() {
        match a.as_str() {
            "--target" => options.target = args.next(),
            "--name" => options.media_name = args.next().unwrap_or_default(),
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }
    let path = PathBuf::from(file);
    match play_file(&path, &options) {
        Ok(report) => {
            let seconds = report.frames_played as f64 / report.rate as f64;
            println!("played {:.2} s ({} Hz), underrun frames: {}", seconds, report.rate, report.underrun_frames);
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
