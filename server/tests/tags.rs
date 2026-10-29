// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tags per file format. The files are made with ffmpeg; the custom field of MP4 is
//! written with lofty, because ffmpeg writes it in a form lofty does not read.

mod common;

use std::borrow::Cow;
use std::fs::File;
use std::io::BufReader;

use common::audio::{ffmpeg_available, make};
use david_server::tags;
use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::AudioFile;
use lofty::mp4::{Atom, AtomData, AtomIdent, Mp4File};

const TAGS: &[(&str, &str)] = &[
    ("title", "Titel äöü"),
    ("artist", "Tag Artist"),
    ("album_artist", "Tag AlbumArtist"),
    ("album", "Tag Album"),
    ("track", "5/17"),
    ("disc", "2/12"),
    ("AGE_RECOMMENDATION", "8"),
];

fn expect_common(t: &tags::MediaTags, kind: &str) {
    assert_eq!(t.title.as_deref(), Some("Titel äöü"), "{kind}");
    assert_eq!(t.artist.as_deref(), Some("Tag Artist"), "{kind}");
    assert_eq!(t.album.as_deref(), Some("Tag Album"), "{kind}");
    assert_eq!(t.album_artist.as_deref(), Some("Tag AlbumArtist"), "{kind}");
    assert_eq!((t.disc, t.track), (Some(2), Some(5)), "{kind}");
    assert!(t.duration_ms > 1500 && t.duration_ms < 2600, "{kind}: duration {}", t.duration_ms);
}

#[test]
fn id3_flac_ogg_and_opus_give_the_same_fields_and_the_custom_age() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let dir = tempfile::TempDir::new().unwrap();
    let cases: [(&str, &[&str], &str); 4] = [
        ("t.mp3", &["-c:a", "libmp3lame", "-b:a", "64k"], "ID3v2"),
        ("t.flac", &["-c:a", "flac"], "Vorbis comments"),
        ("t.ogg", &["-c:a", "libvorbis", "-q:a", "2"], "Vorbis comments"),
        ("t.opus", &["-c:a", "libopus", "-b:a", "48k"], "Vorbis comments"),
    ];
    for (name, codec, kind) in cases {
        let path = dir.path().join(name);
        make(&path, codec, TAGS);
        let t = tags::read(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        expect_common(&t, name);
        assert_eq!(t.tag_kind, Some(kind), "{name}");
        assert_eq!(t.age_recommendation, Some(8), "{name}: the custom field");
    }
}

#[test]
fn mp4_keeps_the_common_fields_and_a_freeform_atom_holds_the_age() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("t.m4a");
    // Without the custom field first: ffmpeg writes the standard atoms.
    let standard: Vec<_> = TAGS.iter().copied().filter(|(k, _)| *k != "AGE_RECOMMENDATION").collect();
    make(&path, &["-c:a", "aac", "-b:a", "64k"], &standard);
    let t = tags::read(&path).unwrap();
    expect_common(&t, "m4a");
    assert_eq!((t.tag_kind, t.age_recommendation), (Some("MP4 ilst"), None));

    // Then the freeform atom, written the way the reader looks for it.
    let mut file = Mp4File::read_from(&mut BufReader::new(File::open(&path).unwrap()), ParseOptions::new()).unwrap();
    let ident = AtomIdent::Freeform {
        mean: Cow::Borrowed(tags::MP4_FREEFORM_MEAN),
        name: Cow::Borrowed(tags::AGE_RECOMMENDATION_KEY),
    };
    file.ilst_mut().expect("an ilst tag").insert(Atom::new(ident, AtomData::UTF8("10".into())));
    file.save_to_path(&path, WriteOptions::default()).unwrap();

    let t = tags::read(&path).unwrap();
    assert_eq!(t.age_recommendation, Some(10));
    expect_common(&t, "m4a after writing the atom");
}

#[test]
fn wav_has_no_custom_field_and_keeps_the_track_as_one_string() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("t.wav");
    make(&path, &["-c:a", "pcm_s16le"], TAGS);
    let t = tags::read(&path).unwrap();
    assert_eq!(t.title.as_deref(), Some("Titel äöü"));
    assert_eq!(t.track, Some(5), "5/17 is read as 5");
    assert_eq!(t.age_recommendation, None, "RIFF INFO cannot hold a custom field");
    assert_eq!(t.disc, None, "and has no disc number");
    assert_eq!(t.tag_kind, Some("RIFF INFO"));
}

#[test]
fn an_age_that_is_not_a_number_is_ignored() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let dir = tempfile::TempDir::new().unwrap();
    for (value, expected) in [("12", Some(12)), (" 7 ", Some(7)), ("twelve", None), ("-3", None), ("", None)] {
        let path = dir.path().join("t.flac");
        make(&path, &["-c:a", "flac"], &[("AGE_RECOMMENDATION", value)]);
        assert_eq!(tags::read(&path).unwrap().age_recommendation, expected, "value {value:?}");
    }
}

#[test]
fn a_file_without_any_tag_gives_empty_fields_and_no_error() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("bare.flac");
    make(&path, &["-c:a", "flac"], &[]);
    let t = tags::read(&path).unwrap();
    assert_eq!((t.title, t.artist, t.disc, t.track, t.age_recommendation), (None, None, None, None, None));
    assert!(t.duration_ms > 1500);
}

/// An ID3v2.3 tag as a tagger wrote it: a year of four blanks in a `TYER` frame (UTF-16 with a byte order mark), which the
/// strict tag reader refuses together with the whole file. The file is fine and plays.
fn id3v23_tag_with_blank_year(title: &str) -> Vec<u8> {
    let frame = |id: &[u8; 4], data: &[u8]| -> Vec<u8> {
        let mut f = id.to_vec();
        f.extend((data.len() as u32).to_be_bytes());
        f.extend([0, 0]);
        f.extend(data);
        f
    };
    let mut frames = frame(b"TYER", &[1, 0xff, 0xfe, b' ', 0, b' ', 0, b' ', 0, b' ', 0]);
    frames.extend(frame(b"TIT2", &[&[0u8][..], title.as_bytes()].concat()));
    frames.extend(frame(b"TPE1", &[&[0u8][..], b"Phil Collins"].concat()));
    let size = frames.len() + 64;
    let mut tag = b"ID3\x03\x00\x00".to_vec();
    tag.extend([(size >> 21) as u8 & 0x7f, (size >> 14) as u8 & 0x7f, (size >> 7) as u8 & 0x7f, size as u8 & 0x7f]);
    tag.extend(frames);
    tag.resize(10 + size, 0);
    tag
}

#[test]
fn a_file_that_the_strict_tag_reader_refuses_is_read_in_the_relaxed_mode_and_its_tags_survive() {
    if !common::audio::ffmpeg_available() {
        return;
    }
    let dir = tempfile::TempDir::new().unwrap();
    let plain = dir.path().join("plain.mp3");
    common::audio::make_mp3(&plain, &[]);
    let mut audio = std::fs::read(&plain).unwrap();
    if &audio[..3] == b"ID3" {
        let size = ((audio[6] as usize & 0x7f) << 21) | ((audio[7] as usize & 0x7f) << 14) | ((audio[8] as usize & 0x7f) << 7) | (audio[9] as usize & 0x7f);
        audio.drain(..10 + size);
    }
    let path = dir.path().join("strict.mp3");
    let mut bytes = id3v23_tag_with_blank_year("I Wish It Would Rain Down");
    bytes.extend(&audio);
    std::fs::write(&path, bytes).unwrap();

    // What the file does to the reader: the plain call refuses it (the reason this test exists; if a newer reader forgives
    // it, the rest of the test still holds).
    let strict = lofty::read_from_path(&path).is_ok();
    eprintln!("the strict reader {} the file", if strict { "accepts" } else { "refuses" });
    // David takes it: the length and the other frames come through.
    let tags = david_server::tags::read(&path).expect("the file is read");
    assert!(tags.duration_ms > 1000, "the length is read: {}", tags.duration_ms);
    assert_eq!((tags.title.as_deref(), tags.artist.as_deref()), (Some("I Wish It Would Rain Down"), Some("Phil Collins")));
    let fields = david_server::tags::read_fields(&path).unwrap();
    assert!(fields.iter().any(|(n, v)| n == "TrackTitle" && v == &["I Wish It Would Rain Down"]), "{fields:?}");

    // A file whose tag cannot be read at all is taken for its audio (nothing but the length), not dropped.
    let broken = dir.path().join("broken.mp3");
    let mut bytes = b"ID3\x03\x00\x00\x00\x00\x00\x40".to_vec();
    bytes.extend(vec![0xffu8; 64]);
    bytes.extend(&audio);
    std::fs::write(&broken, bytes).unwrap();
    let t = david_server::tags::read(&broken).expect("the audio is read although the tag is not");
    assert!(t.duration_ms > 1000);

    // Writing keeps the tags that could be read (relaxed) and the fixed file reads strictly afterwards.
    let r = david_server::tags::write_fields(&path, &[("AlbumTitle".to_owned(), vec!["Face Value".to_owned()])]);
    assert!(r.is_ok(), "{r:?}");
    assert!(lofty::read_from_path(&path).is_ok(), "after a write the tag is the reader's own and strict reading works");
    assert_eq!(david_server::tags::read(&path).unwrap().album.as_deref(), Some("Face Value"));
}
