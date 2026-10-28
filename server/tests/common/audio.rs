// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Makes tiny real audio files with ffmpeg. Tests that need them skip themselves
//! (and say so) where ffmpeg is not installed.
#![allow(dead_code)]

use std::path::Path;
use std::process::{Command, Stdio};

pub fn ffmpeg_available() -> bool {
    Command::new("ffmpeg").arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// Two seconds of a sine tone in the format of the file name's extension, with the
/// given tags (`key=value`).
pub fn make(path: &Path, codec_args: &[&str], tags: &[(&str, &str)]) {
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-v", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=2"]);
    cmd.args(codec_args);
    for (k, v) in tags {
        cmd.args(["-metadata", &format!("{k}={v}")]);
    }
    cmd.arg(path);
    let status = cmd.status().expect("ffmpeg could not be started");
    assert!(status.success(), "ffmpeg failed for {}", path.display());
}

pub fn make_mp3(path: &Path, tags: &[(&str, &str)]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    make(path, &["-c:a", "libmp3lame", "-b:a", "64k"], tags);
}
