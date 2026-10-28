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
    /// The narrator of an audiobook: the composer tag.
    pub narrator: Option<String>,
    pub disc: Option<u32>,
    pub track: Option<u32>,
    /// The medium's own age recommendation (see `docs/spec-discovery-profiles-roles.md`).
    pub age_recommendation: Option<u32>,
    /// Which kind of tag the values came from, for logs and tests.
    pub tag_kind: Option<&'static str>,
    pub duration_ms: i64,
}

pub fn read(path: &Path) -> Result<MediaTags, Error> {
    let tagged = lofty::read_from_path(path).map_err(|e| tags_error(path, e))?;
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
        out.narrator = non_empty(tag.get_string(ItemKey::Composer).map(str::to_owned));
        out.disc = tag.disk().or_else(|| number(tag.get_string(ItemKey::DiscNumber)));
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
    let tagged = lofty::read_from_path(path).ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    let pictures = tag.pictures();
    let picture = pictures.iter().find(|p| p.pic_type() == lofty::picture::PictureType::CoverFront).or_else(|| pictures.first())?;
    let bytes = picture.data();
    let mime = crate::profile::sniff_image(bytes)?;
    (bytes.len() <= MAX_COVER_BYTES).then(|| (mime, bytes.to_vec()))
}

fn age_recommendation(path: &Path, file_type: FileType) -> Result<Option<u32>, Error> {
    let open = || -> Result<BufReader<File>, Error> { Ok(BufReader::new(File::open(path)?)) };
    let opts = ParseOptions::new();

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
    Ok(text.and_then(|t| t.trim().parse().ok()))
}

fn tags_error(path: &Path, e: impl std::fmt::Display) -> Error {
    Error::Tags { file: path.display().to_string(), message: e.to_string() }
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
    use super::number;

    #[test]
    fn parses_plain_and_fractional_numbers() {
        assert_eq!(number(Some("5")), Some(5));
        assert_eq!(number(Some("5/17")), Some(5));
        assert_eq!(number(Some(" 12 / 12 ")), Some(12));
        assert_eq!(number(Some("x")), None);
        assert_eq!(number(None), None);
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
    let Ok(tagged) = lofty::read_from_path(path) else { return AlbumTags::default() };
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

/// Writes the album tags into one file, in the kind of tag the file uses (ID3v2 for MP3, Vorbis
/// comments for FLAC and Ogg, `ilst` for MP4). A file without a tag gets one. The audio is not touched.
pub fn write_album(path: &Path, t: &AlbumTags) -> Result<(), Error> {
    use lofty::config::WriteOptions;
    use lofty::picture::{MimeType, Picture, PictureType};
    use lofty::tag::{Tag, TagExt};

    let mut tagged = lofty::read_from_path(path).map_err(|e| tags_error(path, e))?;
    if tagged.primary_tag().is_none() {
        let kind = tagged.primary_tag_type();
        tagged.insert_tag(Tag::new(kind));
    }
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
    tag.save_to_path(path, WriteOptions::default()).map_err(|e| tags_error(path, e))
}
