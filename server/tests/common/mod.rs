#![allow(dead_code)]
// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later

pub mod audio;

use david_server::{FoundBook, FoundTrack, Store};
use tempfile::TempDir;

pub async fn open() -> (TempDir, Store) {
    let dir = TempDir::new().unwrap();
    let store = Store::open(&dir.path().join("david.db")).await.unwrap();
    (dir, store)
}

/// A synthetic book with `per_disc[i]` tracks on disc `i + 1`.
pub fn book(author: &str, title: &str, per_disc: &[usize]) -> FoundBook {
    let mut tracks = Vec::new();
    for (d, &n) in per_disc.iter().enumerate() {
        for i in 1..=n {
            tracks.push(FoundTrack {
                disc: d as i64 + 1,
                number: i as i64,
                title: format!("{title}_CD{:02}_TR{i:02}", d + 1),
                file: format!("{title}_CD{:02}_TR{i:02}.mp3", d + 1),
                duration_ms: 240_000,
                size_bytes: 1_000_000,
                mtime: 1_700_000_000_000,
                // The format is what the import reads from the file; the synthetic books name an MP3.
                format: "mp3".into(),
                codec: "mpeg".into(),
                mime_type: "audio/mpeg".into(),
                ..Default::default()
            });
        }
    }
    FoundBook {
        kind: "audiobook".into(),
        author: author.to_owned(),
        title: title.to_owned(),
        folder: format!("{author}/{title}"),
        recommended_age: None,
        narrator: None,
        genre: None,
        discs_total: None,
        tracks,
    }
}
