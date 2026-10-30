// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The file format, codec and mime type of every track (project owner, 2026-10-30): read from the file by the import, filled in later for rows
//! from before, and the base of the format summary. The files are made with ffmpeg; the tests skip themselves where it is not installed.

mod common;

use common::audio::{ffmpeg_available, make};
use common::open;
use david_server::Store;
use sea_orm::{ConnectionTrait, Database};

/// A library with one album of five files in five formats; returns its folder (the root of the scan) and the book id.
async fn library_with_formats(store: &Store) -> (tempfile::TempDir, i64) {
    let lib = tempfile::TempDir::new().unwrap();
    let album = lib.path().join("Duo Camillo").join("Caipiranha");
    std::fs::create_dir_all(&album).unwrap();
    let cases: [(&str, &[&str]); 5] = [
        ("01 - a.mp3", &["-c:a", "libmp3lame", "-b:a", "64k"]),
        ("02 - b.ogg", &["-c:a", "libvorbis", "-q:a", "2"]),
        ("03 - c.flac", &["-c:a", "flac"]),
        ("04 - d.opus", &["-c:a", "libopus", "-b:a", "48k"]),
        ("05 - e.m4a", &["-c:a", "aac", "-b:a", "64k"]),
    ];
    for (name, codec) in cases {
        make(&album.join(name), codec, &[("artist", "Duo Camillo"), ("album", "Caipiranha")]);
    }
    let report = store.scan_kind(lib.path(), "music").await.unwrap();
    assert_eq!((report.tracks_added, report.books_failed), (5, 0), "{report:?}");
    let id = store.find_book("Duo Camillo/Caipiranha").await.unwrap().expect("the album").id;
    (lib, id)
}

#[tokio::test]
async fn the_import_reads_format_codec_and_mime_type_from_the_files() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let (_dir, store) = open().await;
    let (_lib, id) = library_with_formats(&store).await;
    let tracks = store.tracks_of(id).await.unwrap();
    let by_file = |ext: &str| tracks.iter().find(|t| t.file.ends_with(ext)).unwrap_or_else(|| panic!("a {ext} track"));
    for (ext, format, codec, mime) in [
        (".mp3", "mp3", "mpeg", "audio/mpeg"),
        (".ogg", "ogg", "vorbis", "audio/ogg"),
        (".flac", "flac", "flac", "audio/flac"),
        (".opus", "opus", "opus", "audio/ogg"),
        (".m4a", "m4a", "aac", "audio/mp4"),
    ] {
        let t = by_file(ext);
        assert_eq!((t.format.as_str(), t.codec.as_str(), t.mime_type.as_str()), (format, codec, mime), "{ext}");
        assert!(t.sample_rate >= 8000 && t.channels >= 1, "{ext}: rate {} channels {}", t.sample_rate, t.channels);
    }

    // The summary: five formats, one track each, one album; the iPhone cannot play ogg and opus.
    let summary = store.format_summary().await.unwrap();
    assert_eq!(summary.len(), 5);
    assert!(summary.iter().all(|r| r.tracks == 1 && r.albums == 1));
    let formats = store.formats_by_book().await.unwrap();
    assert!(formats[&id].contains("ogg") && formats[&id].contains("mp3"));
    let ogg = store.format_albums("OGG").await.unwrap();
    assert_eq!(ogg.len(), 1);
    assert_eq!((ogg[0].1.as_str(), ogg[0].2.as_str(), ogg[0].3), ("Caipiranha", "Duo Camillo", 1));
}

#[tokio::test]
async fn rows_from_before_get_their_format_at_the_next_import_without_the_files_changing() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let (dir, store) = open().await;
    let (lib, id) = library_with_formats(&store).await;
    // As they were before the migration: no format, no mime type.
    Database::connect(format!("sqlite://{}?mode=rw", dir.path().join("david.db").display()))
        .await
        .unwrap()
        .execute_unprepared("UPDATE track SET format = '', codec = '', mime_type = '', bitrate_kbps = 0, sample_rate = 0, channels = 0")
        .await
        .unwrap();
    assert!(store.tracks_of(id).await.unwrap().iter().all(|t| t.format.is_empty()));

    let report = store.scan_kind(lib.path(), "music").await.unwrap();
    assert_eq!((report.formats_filled, report.tracks_unchanged, report.tracks_updated, report.tracks_added), (5, 5, 0, 0), "{report:?}");
    let tracks = store.tracks_of(id).await.unwrap();
    assert!(tracks.iter().all(|t| !t.format.is_empty() && !t.mime_type.is_empty() && t.sample_rate > 0));

    // Filled in once: the next import has nothing left to do.
    let again = store.scan_kind(lib.path(), "music").await.unwrap();
    assert_eq!((again.formats_filled, again.tracks_unchanged), (0, 5), "{again:?}");
}
