// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reads the tags of a media file.
//!
//! Every file format keeps its tags in its own way: ID3v2 in MP3, Vorbis comments
//! in FLAC, Ogg and Opus, `ilst` atoms in MP4/M4A/M4B, a RIFF INFO list in WAV.
//! The common fields (title, artist, album, album artist, disc, track) can be read
//! through the shared view of `lofty`. Custom fields cannot: `lofty` does not list
//! them there, they are reachable only through the types of the format. The age
//! recommendation is such a field, so it is read per format below.
//!
//! | Format | Where the age recommendation is looked up |
//! |---|---|
//! | MP3 (ID3v2) | user text frame `TXXX` with the description `AGE_RECOMMENDATION` |
//! | FLAC, Ogg Vorbis, Opus | Vorbis comment `AGE_RECOMMENDATION` |
//! | MP4, M4A, M4B | freeform atom `----:com.david:AGE_RECOMMENDATION` |
//! | WAV | not supported: RIFF INFO has no custom fields |

use std::borrow::Cow;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use lofty::config::ParseOptions;
use lofty::file::{AudioFile, FileType, TaggedFileExt};
use lofty::mp4::{AtomData, AtomIdent, Mp4File};
use lofty::prelude::*;

use crate::error::Error;

/// Name of the custom field in every format that can hold one.
pub const AGE_RECOMMENDATION_KEY: &str = "AGE_RECOMMENDATION";
/// Namespace of the MP4 freeform atom (`----:<mean>:<name>`).
pub const MP4_FREEFORM_MEAN: &str = "com.david";

/// What the scanner needs from a file's tags. Everything may be missing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaTags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub genre: Option<String>,
    /// The narrator of an audiobook: the composer tag.
    pub narrator: Option<String>,
    pub disc: Option<u32>,
    /// How many discs the set has (`TPOS` as `2/4`, or the field `DiscTotal`).
    pub disc_total: Option<u32>,
    pub track: Option<u32>,
    /// The medium's own age recommendation (see `docs/spec-discovery-profiles-roles.md`).
    pub age_recommendation: Option<u32>,
    /// Which kind of tag the values came from, for logs and tests.
    pub tag_kind: Option<&'static str>,
    pub duration_ms: i64,
}

/// How much a file that the tag reader refuses may be forgiven.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Forgive {
    /// Reading: the relaxed mode of the tag reader, and if that fails too, the audio alone (no tags, only the length).
    Reading,
    /// Writing: only the relaxed mode. A tag that cannot be read at all is never replaced by an empty one.
    Writing,
}

/// Opens a file for its tags. Some taggers write frames that the strict reader refuses (a year of four blanks in a `TYER`
/// frame, for example) and it then drops the whole file, although it plays well. The relaxed mode of the reader takes such a
/// file with what it can read of the tag; for reading, a file that is refused even then is taken for its audio alone.
pub(crate) fn open_tagged(path: &Path, forgive: Forgive) -> Result<lofty::file::TaggedFile, lofty::error::FileParseError> {
    use lofty::config::{ParseOptions, ParsingMode};
    use lofty::probe::Probe;
    let first = match lofty::read_from_path(path) {
        Ok(t) => return Ok(t),
        Err(e) => e,
    };
    let relaxed = Probe::open(path).and_then(|p| p.options(ParseOptions::new().parsing_mode(ParsingMode::Relaxed)).read());
    if let Ok(t) = relaxed {
        tracing::debug!(file = %path.display(), error = %first, "tags read in the relaxed mode");
        return Ok(t);
    }
    if forgive == Forgive::Reading
        && let Ok(t) = Probe::open(path).and_then(|p| p.options(ParseOptions::new().read_tags(false)).read())
    {
        tracing::debug!(file = %path.display(), error = %first, "tags unreadable, the file is taken for its audio alone");
        return Ok(t);
    }
    Err(first)
}

pub fn read(path: &Path) -> Result<MediaTags, Error> {
    let tagged = open_tagged(path, Forgive::Reading).map_err(|e| tags_error(path, e))?;
    let file_type = tagged.file_type();
    let mut out = MediaTags {
        duration_ms: tagged.properties().duration().as_millis() as i64,
        ..MediaTags::default()
    };

    // Common fields through the shared view.
    if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
        out.title = non_empty(tag.title().map(Cow::into_owned));
        out.artist = non_empty(tag.artist().map(Cow::into_owned));
        out.album = non_empty(tag.album().map(Cow::into_owned));
        out.album_artist = non_empty(tag.get_string(ItemKey::AlbumArtist).map(str::to_owned));
        out.genre = non_empty(tag.genre().map(Cow::into_owned));
        out.narrator = non_empty(tag.get_string(ItemKey::Composer).map(str::to_owned));
        out.disc = tag.disk().or_else(|| number(tag.get_string(ItemKey::DiscNumber)));
        out.disc_total = tag.disk_total().or_else(|| number(tag.get_string(ItemKey::DiscTotal)));
        // RIFF INFO keeps "5/17" as one string, which the shared view cannot split.
        out.track = tag.track().or_else(|| number(tag.get_string(ItemKey::TrackNumber)));
        out.tag_kind = Some(match tag.tag_type() {
            lofty::tag::TagType::Id3v2 => "ID3v2",
            lofty::tag::TagType::Id3v1 => "ID3v1",
            lofty::tag::TagType::VorbisComments => "Vorbis comments",
            lofty::tag::TagType::Mp4Ilst => "MP4 ilst",
            lofty::tag::TagType::RiffInfo => "RIFF INFO",
            lofty::tag::TagType::AiffText => "AIFF text",
            lofty::tag::TagType::Ape => "APE",
            _ => "other",
        });
    }

    // Custom field, per format.
    out.age_recommendation = age_recommendation(path, file_type)?;
    Ok(out)
}

/// Largest picture the server hands out. A cover is a few hundred KiB; anything
/// much larger is not a cover.
pub const MAX_COVER_BYTES: usize = 8 * 1024 * 1024;

/// The embedded front cover of a file: the picture marked as front cover, else the
/// first picture. `None` if the file has none or cannot be read. The type is
/// decided by the bytes (`profile::sniff_image`), never by what the tag claims.
pub fn embedded_cover(path: &Path) -> Option<(&'static str, Vec<u8>)> {
    let tagged = open_tagged(path, Forgive::Reading).ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    let pictures = tag.pictures();
    let picture = pictures.iter().find(|p| p.pic_type() == lofty::picture::PictureType::CoverFront).or_else(|| pictures.first())?;
    let bytes = picture.data();
    let mime = crate::profile::sniff_image(bytes)?;
    (bytes.len() <= MAX_COVER_BYTES).then(|| (mime, bytes.to_vec()))
}

fn age_recommendation(path: &Path, file_type: FileType) -> Result<Option<u32>, Error> {
    // The recommendation is an extra: a file whose tag the strict reader refuses is read in the relaxed mode, and if that
    // fails too the file simply has none (it must never cost the file its place in the library).
    let text = age_text(path, file_type, ParseOptions::new())
        .or_else(|_| age_text(path, file_type, ParseOptions::new().parsing_mode(lofty::config::ParsingMode::Relaxed)))
        .unwrap_or(None);
    Ok(text.and_then(|t| t.trim().parse().ok()))
}

fn age_text(path: &Path, file_type: FileType, opts: ParseOptions) -> Result<Option<String>, Error> {
    let open = || -> Result<BufReader<File>, Error> { Ok(BufReader::new(File::open(path)?)) };

    let text: Option<String> = match file_type {
        FileType::Mpeg => {
            let f = lofty::mpeg::MpegFile::read_from(&mut open()?, opts).map_err(|e| tags_error(path, e))?;
            f.id3v2().and_then(|t| t.get_user_text(AGE_RECOMMENDATION_KEY).map(str::to_owned))
        }
        FileType::Flac => {
            let f = lofty::flac::FlacFile::read_from(&mut open()?, opts).map_err(|e| tags_error(path, e))?;
            f.vorbis_comments().and_then(|t| t.get(AGE_RECOMMENDATION_KEY).map(str::to_owned))
        }
        FileType::Vorbis => {
            let f = lofty::ogg::VorbisFile::read_from(&mut open()?, opts).map_err(|e| tags_error(path, e))?;
            f.vorbis_comments().get(AGE_RECOMMENDATION_KEY).map(str::to_owned)
        }
        FileType::Opus => {
            let f = lofty::ogg::OpusFile::read_from(&mut open()?, opts).map_err(|e| tags_error(path, e))?;
            f.vorbis_comments().get(AGE_RECOMMENDATION_KEY).map(str::to_owned)
        }
        FileType::Mp4 => {
            let f = Mp4File::read_from(&mut open()?, opts).map_err(|e| tags_error(path, e))?;
            f.ilst().and_then(|ilst| {
                let ident = AtomIdent::Freeform {
                    mean: Cow::Borrowed(MP4_FREEFORM_MEAN),
                    name: Cow::Borrowed(AGE_RECOMMENDATION_KEY),
                };
                ilst.get(&ident)?.data().find_map(|d| match d {
                    AtomData::UTF8(s) | AtomData::UTF16(s) => Some(s.clone()),
                    _ => None,
                })
            })
        }
        // WAV, AIFF and the rest cannot hold a custom field.
        _ => None,
    };
    Ok(text)
}

/// Gives items with a language that is not three ASCII letters (some taggers write NUL bytes
/// into comment frames) the "unknown" language, because lofty refuses to write such a frame.
fn repair_languages(tag: &mut lofty::tag::Tag) {
    let valid = |l: &[u8; 3]| l.iter().all(u8::is_ascii_alphabetic);
    let mut broken: Vec<_> = tag.items().filter(|i| !valid(i.lang())).cloned().collect();
    if broken.is_empty() {
        return;
    }
    tag.retain(|i| valid(i.lang()));
    for item in &mut broken {
        item.set_lang(*b"XXX");
        tag.push(item.clone());
    }
}

fn tags_error(path: &Path, e: impl std::error::Error) -> Error {
    // lofty's own text is generic ("failed to write Mpeg file"); the cause sits in the source chain.
    let mut message = e.to_string();
    let mut cause = e.source();
    while let Some(c) = cause {
        message.push_str(&format!(": {c}"));
        cause = c.source();
    }
    Error::Tags { file: path.display().to_string(), message }
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

/// "5" or "5/17" becomes 5.
fn number(s: Option<&str>) -> Option<u32> {
    s?.split('/').next()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::{number, write_cover};
    use lofty::file::TaggedFileExt;

    #[test]
    fn parses_plain_and_fractional_numbers() {
        assert_eq!(number(Some("5")), Some(5));
        assert_eq!(number(Some("5/17")), Some(5));
        assert_eq!(number(Some(" 12 / 12 ")), Some(12));
        assert_eq!(number(Some("x")), None);
        assert_eq!(number(None), None);
    }

    /// An MP3 whose comment frame has NUL bytes as its language, as some taggers write it.
    fn mp3_with_broken_comment() -> Vec<u8> {
        let mut frame = vec![3u8]; // ISO-8859-1
        frame.extend_from_slice(&[0, 0, 0]); // the broken language
        frame.extend_from_slice(b"\0a comment");
        let mut tag = b"COMM".to_vec();
        tag.extend_from_slice(&(frame.len() as u32).to_be_bytes());
        tag.extend_from_slice(&[0, 0]);
        tag.extend_from_slice(&frame);
        let mut out = b"ID3\x03\x00\x00".to_vec();
        let size = tag.len() as u32;
        out.extend_from_slice(&[(size >> 21) as u8 & 0x7f, (size >> 14) as u8 & 0x7f, (size >> 7) as u8 & 0x7f, size as u8 & 0x7f]);
        out.extend_from_slice(&tag);
        for _ in 0..8 {
            let mut audio = vec![0u8; 417]; // MPEG-1 layer III, 128 kbit/s, 44.1 kHz
            audio[..4].copy_from_slice(&[0xff, 0xfb, 0x90, 0x00]);
            out.extend_from_slice(&audio);
        }
        out
    }

    #[test]
    fn embeds_a_cover_in_a_file_with_a_broken_comment_language() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.mp3");
        std::fs::write(&path, mp3_with_broken_comment()).unwrap();
        write_cover(&path, b"\xff\xd8\xff\xe0 not really a picture", "image/jpeg").unwrap();
        let tagged = lofty::read_from_path(&path).unwrap();
        assert_eq!(tagged.primary_tag().unwrap().picture_count(), 1);
    }
}

/// The tags that are set for a whole album. `None` leaves the field of a file as it is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AlbumTags {
    pub album: Option<String>,
    /// Written as artist and as album artist.
    pub artist: Option<String>,
    /// The narrator of an audiobook, written as the composer tag.
    pub narrator: Option<String>,
    pub year: Option<String>,
    pub genre: Option<String>,
    /// The picture bytes and their type (decided by the bytes, see `profile::sniff_image`).
    pub cover: Option<(Vec<u8>, &'static str)>,
}

/// What the first file of an album says, to fill in the form.
pub fn read_album(path: &Path) -> AlbumTags {
    let Ok(tagged) = open_tagged(path, Forgive::Reading) else { return AlbumTags::default() };
    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else { return AlbumTags::default() };
    let text = |v: Option<&str>| non_empty(v.map(str::to_owned));
    AlbumTags {
        album: non_empty(tag.album().map(Cow::into_owned)),
        artist: text(tag.get_string(ItemKey::AlbumArtist)).or_else(|| non_empty(tag.artist().map(Cow::into_owned))),
        narrator: text(tag.get_string(ItemKey::Composer)),
        year: text(tag.get_string(ItemKey::RecordingDate).or_else(|| tag.get_string(ItemKey::Year)))
            .map(|y| y.chars().take(4).collect()),
        genre: non_empty(tag.genre().map(Cow::into_owned)),
        cover: None,
    }
}

/// Makes sure a file has the tag of its kind. For an MP3 that is ID3v2, which is binding (project owner,
/// 2026-10-28): a file that has only the obsolete ID3v1 gets an ID3v2 tag made from it, so that nothing it said is lost.
fn ensure_primary(tagged: &mut lofty::file::TaggedFile) {
    use lofty::tag::{Tag, TagType};
    if tagged.primary_tag().is_some() {
        return;
    }
    let kind = tagged.primary_tag_type();
    let from_v1 = if kind == TagType::Id3v2 { tagged.tag(TagType::Id3v1).cloned() } else { None };
    match from_v1 {
        Some(mut v1) => {
            v1.re_map(TagType::Id3v2);
            tagged.insert_tag(v1);
        }
        None => {
            tagged.insert_tag(Tag::new(kind));
        }
    }
}

/// Is this an MP3 without an ID3v2 tag (only the obsolete ID3v1, or no tag)?
pub fn lacks_id3v2(path: &Path) -> bool {
    use lofty::file::FileType;
    use lofty::tag::TagType;
    open_tagged(path, Forgive::Writing).is_ok_and(|t| t.file_type() == FileType::Mpeg && t.tag(TagType::Id3v2).is_none())
}

/// Gives an MP3 without ID3v2 its ID3v2 tag (from the ID3v1 one if there is one). True if the file changed.
pub fn upgrade_to_id3v2(path: &Path) -> Result<bool, Error> {
    use lofty::config::WriteOptions;
    use lofty::file::FileType;
    use lofty::tag::{TagExt, TagType};
    let mut tagged = open_tagged(path, Forgive::Writing).map_err(|e| tags_error(path, e))?;
    if tagged.file_type() != FileType::Mpeg || tagged.tag(TagType::Id3v2).is_some() {
        return Ok(false);
    }
    ensure_primary(&mut tagged);
    let tag = tagged.primary_tag().ok_or_else(|| Error::Tags { file: path.display().to_string(), message: "no tag".into() })?;
    tag.save_to_path(path, WriteOptions::default()).map_err(|e| tags_error(path, e))?;
    Ok(true)
}

/// Writes the album tags into one file, in the kind of tag the file uses (ID3v2 for MP3, Vorbis
/// comments for FLAC and Ogg, `ilst` for MP4). A file without a tag gets one. The audio is not touched.
pub fn write_album(path: &Path, t: &AlbumTags) -> Result<(), Error> {
    use lofty::config::WriteOptions;
    use lofty::picture::{MimeType, Picture, PictureType};
    use lofty::tag::TagExt;

    let mut tagged = open_tagged(path, Forgive::Writing).map_err(|e| tags_error(path, e))?;
    ensure_primary(&mut tagged);
    let tag = tagged.primary_tag_mut().ok_or_else(|| Error::Tags { file: path.display().to_string(), message: "no tag".into() })?;
    if let Some(album) = &t.album {
        tag.set_album(album.clone());
    }
    if let Some(artist) = &t.artist {
        tag.set_artist(artist.clone());
        tag.insert_text(ItemKey::AlbumArtist, artist.clone());
    }
    if let Some(narrator) = &t.narrator {
        tag.insert_text(ItemKey::Composer, narrator.clone());
    }
    if let Some(year) = &t.year {
        // TDRC in ID3v2.4, the date field of the other formats.
        tag.insert_text(ItemKey::RecordingDate, year.clone());
    }
    if let Some(genre) = &t.genre {
        tag.set_genre(genre.clone());
    }
    if let Some((bytes, mime)) = &t.cover {
        tag.remove_picture_type(PictureType::CoverFront);
        let mime = match *mime {
            "image/png" => MimeType::Png,
            "image/jpeg" => MimeType::Jpeg,
            other => MimeType::Unknown(other.to_owned()),
        };
        tag.push_picture(Picture::unchecked(bytes.clone()).pic_type(PictureType::CoverFront).mime_type(mime).build());
    }
    repair_languages(tag);
    tag.save_to_path(path, WriteOptions::default()).map_err(|e| tags_error(path, e))
}

// The whole width of the tags: every field `lofty` can keep in some kind of tag, by its name in the
// shared view (`TrackTitle`, `AlbumArtist`, `Genre`, ...), whatever the format calls it. A field
// can have several values (several artists). What the shared view does not know is not reachable.

/// All fields that at least one kind of tag can hold.
fn all_keys() -> &'static [(String, ItemKey)] {
    use lofty::tag::TagType;
    static KEYS: std::sync::OnceLock<Vec<(String, ItemKey)>> = std::sync::OnceLock::new();
    KEYS.get_or_init(|| {
        let mut keys: Vec<(String, ItemKey)> = Vec::new();
        for kind in [TagType::Id3v2, TagType::VorbisComments, TagType::Mp4Ilst, TagType::Ape, TagType::RiffInfo, TagType::AiffText, TagType::Id3v1] {
            for key in ItemKey::supported_keys(kind) {
                if !keys.iter().any(|(_, k)| k == key) {
                    keys.push((format!("{key:?}"), *key));
                }
            }
        }
        keys.sort_by(|a, b| a.0.cmp(&b.0));
        keys
    })
}

/// The names of all fields that can be set.
pub fn field_names() -> Vec<&'static str> {
    all_keys().iter().map(|(n, _)| n.as_str()).collect()
}

/// The fields a person may add to an album, with the kind of input each has: `string`, `integer` or `date`. The common ones come first, in this
/// order; the rest follows by name. The fields that the form has an input of its own for (album, artist, genre) are not in it.
pub fn field_catalog() -> Vec<(String, &'static str)> {
    const COMMON: [&str; 20] = [
        "DiscNumber", "DiscTotal", "TrackTotal", "RecordingDate", "ReleaseDate", "Composer", "Conductor", "Lyricist", "Publisher", "Label", "CatalogNumber",
        "Comment", "Description", "Language", "Copyright", "ISRC", "Barcode", "Work", "MovementName", "Bpm",
    ];
    const OWN: [&str; 5] = ["AlbumTitle", "AlbumArtist", "TrackArtist", "Genre", "TrackNumber"];
    let kind = |name: &str| -> &'static str {
        match name {
            "DiscNumber" | "DiscTotal" | "TrackNumber" | "TrackTotal" | "Bpm" | "MovementNumber" | "MovementTotal" | "Year" => "integer",
            n if n.ends_with("Date") => "date",
            _ => "string",
        }
    };
    let all = field_names();
    let mut out: Vec<(String, &'static str)> = COMMON.iter().filter(|n| all.contains(n)).map(|n| ((*n).to_owned(), kind(n))).collect();
    for n in all {
        if !COMMON.contains(&n) && !OWN.contains(&n) {
            out.push((n.to_owned(), kind(n)));
        }
    }
    out
}

fn key_of(name: &str) -> Option<ItemKey> {
    all_keys().iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, k)| *k)
}

fn name_of(key: ItemKey) -> String {
    all_keys().iter().find(|(_, k)| *k == key).map(|(n, _)| n.clone()).unwrap_or_else(|| format!("{key:?}"))
}

/// All text fields of the primary tag, in file order; a field with several values appears once
/// with all of them. Pictures are not fields (see `embedded_cover`).
pub fn read_fields(path: &Path) -> Result<Vec<(String, Vec<String>)>, Error> {
    let tagged = open_tagged(path, Forgive::Reading).map_err(|e| tags_error(path, e))?;
    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else { return Ok(Vec::new()) };
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for item in tag.items() {
        let Some(value) = item.value().text() else { continue };
        let name = name_of(item.key());
        match out.iter_mut().find(|(n, _)| *n == name) {
            Some((_, values)) => values.push(value.to_owned()),
            None => out.push((name, vec![value.to_owned()])),
        }
    }
    Ok(out)
}

/// Sets fields of one file: a list of values replaces the field, an empty list removes it. The
/// names are those of `field_names`. Nothing is written if a name is unknown or a value cannot be
/// kept in the kind of tag the file has.
pub fn write_fields(path: &Path, changes: &[(String, Vec<String>)]) -> Result<(), Error> {
    use lofty::config::WriteOptions;
    use lofty::tag::{ItemValue, TagExt, TagItem};

    let fail = |message: String| Error::Tags { file: path.display().to_string(), message };
    let mut tagged = open_tagged(path, Forgive::Writing).map_err(|e| tags_error(path, e))?;
    ensure_primary(&mut tagged);
    let tag = tagged.primary_tag_mut().ok_or_else(|| fail("no tag".into()))?;
    for (name, values) in changes {
        let key = key_of(name).ok_or_else(|| fail(format!("unknown field {name}")))?;
        tag.remove_key(key);
        for value in values {
            if !tag.push(TagItem::new(key, ItemValue::Text(value.clone()))) {
                return Err(fail(format!("{name} cannot be kept in this kind of tag")));
            }
        }
    }
    repair_languages(tag);
    tag.save_to_path(path, WriteOptions::default()).map_err(|e| tags_error(path, e))
}

/// Puts a picture into the file as the front cover, replacing the one that is there.
pub fn write_cover(path: &Path, bytes: &[u8], mime: &'static str) -> Result<(), Error> {
    write_album(path, &AlbumTags { cover: Some((bytes.to_vec(), mime)), ..AlbumTags::default() })
}
