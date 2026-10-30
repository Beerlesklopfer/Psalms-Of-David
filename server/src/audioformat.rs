// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The format of an audio file: container (`format`), `codec` and mime type. They are read from the file itself (what the tag
//! library recognises), the extension is only the fallback. One place knows the names and the mime types, so the scan, the stream,
//! the API, the pages and the MCP tools agree (project owner, 2026-10-30: "the database needs the file format, and the mime type").
//!
//! Why it matters: the players differ. Android (ExoPlayer) and the Linux engine (symphonia) play Ogg Vorbis, FLAC and M4A;
//! iOS (AVPlayer) does not play Ogg Vorbis, Opus in Ogg, WMA, APE and the like ([`ios_playable`]).

use lofty::file::FileType;

/// What is known about the format of one file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioFormat {
    /// The container or file kind: `mp3`, `ogg`, `opus`, `flac`, `m4a`, `wav`, `aac`, `aiff`, `ape`, `mpc`, `wv`, `spx`; "" if unknown.
    pub format: String,
    /// The coding: `mpeg` (MP3), `vorbis`, `opus`, `flac`, `aac`, `pcm`, `ape`, `mpc`, `wavpack`, `speex`; "" if unknown.
    pub codec: String,
    /// The mime type the stream is served with: `audio/mpeg`, `audio/ogg`, `audio/flac`, ...; "" if unknown.
    pub mime: String,
}

impl AudioFormat {
    fn new(format: &str, codec: &str, mime: &str) -> AudioFormat {
        AudioFormat { format: format.into(), codec: codec.into(), mime: mime.into() }
    }

    /// True if the format is known at all.
    pub fn is_known(&self) -> bool {
        !self.format.is_empty()
    }
}

/// The format of a file the tag library recognised; `ext` (the file extension) only fills in what the library does not say.
pub fn describe(file_type: Option<FileType>, ext: &str) -> AudioFormat {
    let by_type = match file_type {
        Some(FileType::Mpeg) => Some(AudioFormat::new("mp3", "mpeg", "audio/mpeg")),
        Some(FileType::Vorbis) => Some(AudioFormat::new("ogg", "vorbis", "audio/ogg")),
        Some(FileType::Opus) => Some(AudioFormat::new("opus", "opus", "audio/ogg")),
        Some(FileType::Speex) => Some(AudioFormat::new("spx", "speex", "audio/ogg")),
        Some(FileType::Flac) => Some(AudioFormat::new("flac", "flac", "audio/flac")),
        Some(FileType::Mp4) => Some(AudioFormat::new("m4a", "aac", "audio/mp4")),
        Some(FileType::Wav) => Some(AudioFormat::new("wav", "pcm", "audio/wav")),
        Some(FileType::Aiff) => Some(AudioFormat::new("aiff", "pcm", "audio/aiff")),
        Some(FileType::Aac) => Some(AudioFormat::new("aac", "aac", "audio/aac")),
        Some(FileType::Ape) => Some(AudioFormat::new("ape", "ape", "audio/x-ape")),
        Some(FileType::Mpc) => Some(AudioFormat::new("mpc", "mpc", "audio/x-musepack")),
        Some(FileType::WavPack) => Some(AudioFormat::new("wv", "wavpack", "audio/x-wavpack")),
        _ => None,
    };
    by_type.unwrap_or_else(|| from_extension(ext))
}

/// The format a file extension suggests (case does not matter); unknown for an extension that is none of the audio files.
pub fn from_extension(ext: &str) -> AudioFormat {
    match ext.trim_start_matches('.').to_ascii_lowercase().as_str() {
        "mp3" => AudioFormat::new("mp3", "mpeg", "audio/mpeg"),
        "ogg" | "oga" => AudioFormat::new("ogg", "vorbis", "audio/ogg"),
        "opus" => AudioFormat::new("opus", "opus", "audio/ogg"),
        "spx" => AudioFormat::new("spx", "speex", "audio/ogg"),
        "flac" => AudioFormat::new("flac", "flac", "audio/flac"),
        "m4a" | "m4b" | "mp4" => AudioFormat::new("m4a", "aac", "audio/mp4"),
        "wav" => AudioFormat::new("wav", "pcm", "audio/wav"),
        "aif" | "aiff" => AudioFormat::new("aiff", "pcm", "audio/aiff"),
        "aac" => AudioFormat::new("aac", "aac", "audio/aac"),
        "ape" => AudioFormat::new("ape", "ape", "audio/x-ape"),
        "mpc" => AudioFormat::new("mpc", "mpc", "audio/x-musepack"),
        "wv" => AudioFormat::new("wv", "wavpack", "audio/x-wavpack"),
        "wma" => AudioFormat::new("wma", "wma", "audio/x-ms-wma"),
        _ => AudioFormat::default(),
    }
}

/// The mime type for the stream: what the database holds, else what the extension says, else the generic `application/octet-stream`.
pub fn mime_or_guess(stored: &str, file_name: &str) -> String {
    if !stored.is_empty() {
        return stored.to_owned();
    }
    let ext = file_name.rsplit_once('.').map_or("", |(_, e)| e);
    let guessed = from_extension(ext).mime;
    if guessed.is_empty() { "application/octet-stream".into() } else { guessed }
}

/// Formats that the iPhone (AVPlayer) cannot play. A list in the code, not a setting: it is a fact about the platform. (Ogg Vorbis and Opus in
/// Ogg are not supported by AVPlayer; neither are WMA, Monkey's Audio, Musepack, WavPack and Speex.) MP3, M4A/AAC/ALAC, FLAC, WAV and AIFF are.
const NOT_PLAYABLE_ON_IOS: &[&str] = &["ogg", "opus", "spx", "wma", "ape", "mpc", "wv"];

/// Can the iPhone play a file of this `format` (as `AudioFormat::format` names it)? An unknown (empty) format is assumed playable.
pub fn ios_playable(format: &str) -> bool {
    !NOT_PLAYABLE_ON_IOS.contains(&format)
}

/// A short readable line for a combination, e.g. `ogg/vorbis 44.1 kHz stereo`; parts that are not known are left out.
pub fn describe_line(format: &str, codec: &str, sample_rate: i64, channels: i64) -> String {
    let mut line = match (format.is_empty(), codec.is_empty() || codec == format) {
        (true, _) => "?".to_owned(),
        (false, true) => format.to_owned(),
        (false, false) => format!("{format}/{codec}"),
    };
    if sample_rate > 0 {
        let khz = sample_rate as f64 / 1000.0;
        let text = if (khz - khz.round()).abs() < 0.001 { format!("{khz:.0}") } else { format!("{khz:.1}") };
        line.push_str(&format!(" {text} kHz"));
    }
    match channels {
        1 => line.push_str(" mono"),
        2 => line.push_str(" stereo"),
        n if n > 2 => line.push_str(&format!(" {n} ch")),
        _ => {}
    }
    line
}

/// One line of the summary of the formats of the library.
#[derive(Debug, Clone)]
pub struct FormatRow {
    pub format: String,
    pub codec: String,
    pub mime_type: String,
    pub tracks: i64,
    pub albums: i64,
    /// The distinct sample rates (Hz) of these tracks, lowest first; 0 (unknown) left out.
    pub sample_rates: Vec<i64>,
}

impl crate::store::Store {
    /// The formats of the library: per format and codec the tracks and albums.
    pub async fn format_summary(&self) -> crate::error::Result<Vec<FormatRow>> {
        use sea_orm::{ConnectionTrait, DbBackend, Statement};
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT format, codec, max(mime_type), count(*), count(DISTINCT book_id), \
                        group_concat(DISTINCT CASE WHEN sample_rate > 0 THEN sample_rate END) \
                 FROM track GROUP BY format, codec ORDER BY count(*) DESC",
            ))
            .await?;
        rows.iter()
            .map(|r| {
                let rates: Option<String> = r.try_get_by_index(5)?;
                let mut sample_rates: Vec<i64> = rates.unwrap_or_default().split(',').filter_map(|v| v.trim().parse().ok()).collect();
                sample_rates.sort_unstable();
                Ok(FormatRow {
                    format: r.try_get_by_index(0)?,
                    codec: r.try_get_by_index(1)?,
                    mime_type: r.try_get_by_index(2)?,
                    tracks: r.try_get_by_index(3)?,
                    albums: r.try_get_by_index(4)?,
                    sample_rates,
                })
            })
            .collect()
    }

    /// The formats of the tracks of every book: book id -> formats (lower case). A track with an unknown format counts as "".
    pub async fn formats_by_book(&self) -> crate::error::Result<std::collections::HashMap<i64, std::collections::HashSet<String>>> {
        use sea_orm::{ConnectionTrait, DbBackend, Statement};
        let rows = self.db.reader.query_all_raw(Statement::from_string(DbBackend::Sqlite, "SELECT DISTINCT book_id, format FROM track")).await?;
        let mut out: std::collections::HashMap<i64, std::collections::HashSet<String>> = std::collections::HashMap::new();
        for r in rows {
            let (id, format): (i64, String) = (r.try_get_by_index(0)?, r.try_get_by_index(1)?);
            out.entry(id).or_default().insert(format.to_lowercase());
        }
        Ok(out)
    }

    /// The albums that have tracks of `format`, with the number of those tracks, the most first.
    pub async fn format_albums(&self, format: &str) -> crate::error::Result<Vec<(i64, String, String, i64)>> {
        use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT b.id, b.title, a.name, count(*) FROM track t JOIN book b ON b.id = t.book_id JOIN author a ON a.id = b.author_id \
                 WHERE lower(t.format) = ? GROUP BY b.id ORDER BY count(*) DESC, b.title",
                [Value::from(format.to_lowercase())],
            ))
            .await?;
        rows.iter().map(|r| Ok((r.try_get_by_index(0)?, r.try_get_by_index(1)?, r.try_get_by_index(2)?, r.try_get_by_index(3)?))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_type_decides_and_the_extension_only_fills_in() {
        assert_eq!(describe(Some(FileType::Vorbis), "mp3"), AudioFormat::new("ogg", "vorbis", "audio/ogg"));
        assert_eq!(describe(Some(FileType::Mpeg), "OGG").mime, "audio/mpeg");
        assert_eq!(describe(None, "flac"), AudioFormat::new("flac", "flac", "audio/flac"));
        assert_eq!(describe(None, ".M4B").format, "m4a");
        assert!(!describe(None, "txt").is_known());
    }

    #[test]
    fn the_mime_type_falls_back_to_the_extension_and_then_to_octet_stream() {
        assert_eq!(mime_or_guess("audio/ogg", "x.mp3"), "audio/ogg");
        assert_eq!(mime_or_guess("", "01 - Lied.OGG"), "audio/ogg");
        assert_eq!(mime_or_guess("", "noextension"), "application/octet-stream");
    }

    #[test]
    fn the_iphone_cannot_play_ogg_and_a_few_others() {
        for f in ["ogg", "opus", "wma", "ape"] {
            assert!(!ios_playable(f), "{f}");
        }
        for f in ["mp3", "m4a", "flac", "wav", "aiff", "aac", ""] {
            assert!(ios_playable(f), "{f}");
        }
    }

    #[test]
    fn a_combination_reads_as_one_short_line() {
        assert_eq!(describe_line("ogg", "vorbis", 44100, 2), "ogg/vorbis 44.1 kHz stereo");
        assert_eq!(describe_line("mp3", "mpeg", 24000, 1), "mp3/mpeg 24 kHz mono");
        assert_eq!(describe_line("flac", "flac", 0, 0), "flac");
        assert_eq!(describe_line("", "", 0, 0), "?");
    }
}
