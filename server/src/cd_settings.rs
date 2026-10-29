// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The settings of the CD import (group "CD import" of the settings page): the file format, the encoder values,
//! the naming of folders, files and tags. Nothing of this is built into the program; the fallbacks below are
//! only used when a stored value is missing or damaged, and they are the same as the values of migration 0026.
//!
//! Names and tags are made from templates. A template is text with placeholders in braces, `{author}`, or with a
//! number format, `{disc:02}` (padded with zeros to two places). Only the placeholders of the kind of template
//! are known; anything else, a path separator in a file name, `..` and control characters are refused when a
//! value is saved, so a template can never leave the import folder.

use std::collections::HashMap;

use serde::Serialize;

use crate::error::Result;
use crate::store::Store;

/// Formats the import accepts, with the file ending of each. Opus can be added here later.
pub const FORMATS: &[(&str, &str)] = &[("mp3", "mp3")];
pub const CHANNEL_MODES: &[&str] = &["joint_stereo", "stereo", "mono"];
pub const SAMPLE_RATES: &[u32] = &[8000, 11025, 12000, 16000, 22050, 24000, 32000, 44100, 48000];
pub const ID3_VERSIONS: &[&str] = &["2.4", "2.3"];
pub const TRACK_SCHEMES: &[&str] = &["disc_times_100", "plain"];
pub const KINDS: &[&str] = &["audiobook", "music"];
pub const BITRATE_RANGE: (u32, u32) = (8, 320);
pub const COVER_PX_RANGE: (u32, u32) = (100, 2000);
pub const LOG_KEEP_RANGE: (u32, u32) = (10, 1000);
pub const POLL_RANGE: (u32, u32) = (2, 300);
pub const READ_SPEED_RANGE: (u32, u32) = (0, 52);
/// The buffer between the drive and the encoder, in seconds of audio: 0 (automatic) or 2 to 900.
pub const BUFFER_RANGE: (u32, u32) = (2, 900);
/// The most memory the buffer may take, in MB.
pub const BUFFER_MB_RANGE: (u32, u32) = (16, 4096);
/// The reserve that the automatic size adds to the longest track, in seconds.
pub const BUFFER_AUTO_RESERVE_S: u32 = 10;
/// The nice value of the encoder thread: -20 (highest priority) to 19 (lowest).
pub const NICE_RANGE: (i32, i32) = (-20, 19);
/// The shift of the second read, in sectors: 0 (none) to one less than a command (27).
pub const VERIFY_SHIFT_RANGE: (u32, u32) = (0, 26);
pub const RESUME_HOURS_RANGE: (u32, u32) = (1, 720);
pub const HEAL_TOLERANCE_RANGE: (u32, u32) = (1, 60);

/// The keys of the group, in the order the page shows them.
pub const KEYS: &[&str] = &[
    "cd_format",
    "cd_bitrate_kbps",
    "cd_sample_rate_hz",
    "cd_channel_mode",
    "cd_quality",
    "cd_id3_version",
    "cd_file_pattern",
    "cd_folder_pattern",
    "cd_disc_folder_pattern",
    "cd_title_pattern",
    "cd_track_scheme",
    "cd_genre",
    "cd_cover_px",
    "cd_eject_after",
    "cd_target_kind",
    "cd_udev",
    "cd_poll_seconds",
    "cd_device",
    "cd_verify_read",
    "cd_verify_shift",
    "cd_accept_unverified",
    "cd_read_speed",
    "cd_buffer_seconds",
    "cd_buffer_max_mb",
    "cd_encoder_nice",
    "cd_resume_keep_hours",
    "cd_auto_import",
    "cd_heal_tolerance_s",
    "cd_log_keep",
    "musicbrainz_contact",
    "cd_musicbrainz_url",
    "cd_coverart_url",
];

/// Is this key part of the CD import group?
pub fn is_cd_key(key: &str) -> bool {
    KEYS.contains(&key)
}

// Placeholders per kind of template.
// `title` is the title of the book or album (what the apps call it); `album` means the same. `track_title` is the
// title of one track.
pub const FILE_VARS: &[&str] = &["author", "title", "album", "narrator", "year", "disc", "track", "track_title"];
pub const FOLDER_VARS: &[&str] = &["author", "title", "album", "narrator", "year"];
pub const DISC_FOLDER_VARS: &[&str] = &["disc", "title", "album"];
/// The placeholders offered to a person for a template (the alias `album` is left out), by setting.
pub fn vars_of(key: &str) -> &'static [&'static str] {
    match key {
        "cd_file_pattern" => &["author", "title", "narrator", "year", "disc", "track", "track_title"],
        "cd_folder_pattern" => &["author", "title", "narrator", "year"],
        "cd_disc_folder_pattern" => &["disc", "title"],
        _ => &[],
    }
}

/// The placeholders a template must have (`a|b` = one of them), by setting.
fn required_of(key: &str) -> &'static [&'static str] {
    match key {
        "cd_file_pattern" => &["disc", "track"],
        "cd_folder_pattern" => &["title|album"],
        "cd_disc_folder_pattern" => &["disc"],
        _ => &[],
    }
}

fn allowed_of(key: &str) -> &'static [&'static str] {
    match key {
        "cd_file_pattern" => FILE_VARS,
        "cd_folder_pattern" => FOLDER_VARS,
        "cd_disc_folder_pattern" => DISC_FOLDER_VARS,
        _ => &[],
    }
}

/// The first placeholder of a template that the setting does not know.
pub fn unknown_placeholder(key: &str, pattern: &str) -> Option<String> {
    let allowed = allowed_of(key);
    pattern.split('/').filter_map(parse_template).flatten().find_map(|p| match p {
        Part::Var { name, .. } if !allowed.contains(&name.as_str()) => Some(name),
        _ => None,
    })
}

/// The required placeholders a template lacks (the first of each alternative).
pub fn missing_placeholders(key: &str, pattern: &str) -> Vec<String> {
    let have: Vec<String> = pattern
        .split('/')
        .filter_map(parse_template)
        .flatten()
        .filter_map(|p| if let Part::Var { name, .. } = p { Some(name) } else { None })
        .collect();
    required_of(key).iter().filter(|r| !r.split('|').any(|alt| have.iter().any(|h| h == alt))).map(|r| r.split('|').next().unwrap_or("").to_owned()).collect()
}

/// What the title tag of a track is made from: the file name without the ending, or the title of the track.
pub const TITLE_SCHEMES: &[&str] = &["file_name", "track_title"];

/// What the import uses, read from the settings.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CdSettings {
    pub format: String,
    /// The file ending that belongs to the format.
    pub extension: String,
    pub bitrate_kbps: u32,
    pub sample_rate_hz: u32,
    pub channel_mode: String,
    pub quality: u8,
    #[serde(rename = "tag_version")]
    pub id3_version: String,
    #[serde(rename = "file_scheme")]
    pub file_pattern: String,
    #[serde(rename = "folder_scheme")]
    pub folder_pattern: String,
    #[serde(rename = "disc_folder_scheme")]
    pub disc_folder_pattern: String,
    #[serde(rename = "title_scheme")]
    pub title_pattern: String,
    #[serde(rename = "track_number_scheme")]
    pub track_scheme: String,
    pub genre: String,
    #[serde(rename = "cover_size")]
    pub cover_px: u32,
    /// Does the desktop app detect drives and discs by udev events (else by asking)?
    pub udev: bool,
    /// The drive the server uses; empty = every drive it finds.
    #[serde(skip)]
    pub device: String,
    /// How often the drives are looked at when udev events are off, in seconds.
    #[serde(skip)]
    pub poll_seconds: u32,
    /// Read every track twice and compare (a second read catches a drive that returns wrong data).
    #[serde(skip)]
    pub verify_read: bool,
    /// The speed the drive reads at during a rip, as a multiple of 1x (176 kB/s); 0 = the drive's own.
    #[serde(skip)]
    pub read_speed: u32,
    /// Sectors by which the commands of the second read are shifted against the first, 0 to 26.
    #[serde(skip)]
    pub verify_shift: u32,
    /// A track that could not be confirmed (no two readings agree) is filed with its first reading, and marked.
    #[serde(skip)]
    pub accept_unverified: bool,
    /// Seconds of audio the ring between the drive and the encoder holds; 0 = a whole track and a reserve.
    #[serde(skip)]
    pub buffer_seconds: u32,
    /// The most memory the ring may take, in MB.
    #[serde(skip)]
    pub buffer_max_mb: u32,
    /// The nice value the encoder thread asks for, -20 to 19.
    #[serde(skip)]
    pub encoder_nice: i32,
    /// How long the finished tracks of an interrupted rip are kept, in hours.
    #[serde(skip)]
    pub resume_keep_hours: u32,
    /// A CD that is put in is imported by itself (needs `udev`).
    #[serde(skip)]
    pub auto_import: bool,
    /// A file in the library may differ from the length by the table of contents by this many seconds.
    #[serde(skip)]
    pub heal_tolerance_s: u32,
    /// How many entries of the import log are kept.
    #[serde(skip)]
    pub log_keep: u32,
    #[serde(rename = "eject")]
    pub eject_after: bool,
    #[serde(rename = "kind")]
    pub target_kind: String,
    /// Is there a contact address, so that MusicBrainz may be asked?
    pub lookup_ready: bool,
    #[serde(skip)]
    pub contact: String,
    #[serde(skip)]
    pub musicbrainz_url: String,
    #[serde(skip)]
    pub coverart_url: String,
}

/// Why a value was refused; each code has a text `cd.verr.<code>` in the catalogs.
pub const VALIDATION_CODES: &[&str] = &[
    "out_of_range",
    "not_allowed",
    "invalid_text",
    "invalid_address",
    "template_empty",
    "template_syntax",
    "template_unsafe",
    "template_unknown_placeholder",
    "template_missing_placeholder",
];

/// The check of one value: the value as it is stored (trimmed, normalised), or `None` if it is refused.
pub fn validate(key: &str, raw: &str) -> Option<String> {
    check(key, raw).ok()
}

/// The same with the reason (one of `VALIDATION_CODES`).
pub fn check(key: &str, raw: &str) -> std::result::Result<String, &'static str> {
    let v = raw.trim();
    let plain = |s: &str, max: usize| s.chars().count() <= max && !s.chars().any(char::is_control);
    let number = |lo: u32, hi: u32| v.parse::<u32>().ok().filter(|n| (lo..=hi).contains(n)).map(|n| n.to_string()).ok_or("out_of_range");
    let one_of = |list: &[&str]| list.contains(&v).then(|| v.to_owned()).ok_or("not_allowed");
    let text = |ok: bool| if ok { Ok(v.to_owned()) } else { Err("invalid_text") };
    let template = |allowed: &[&str], required: &[&str], slashes: bool| template_check(v, allowed, required, slashes).map(|()| v.to_owned());
    match key {
        "cd_format" => FORMATS.iter().any(|(f, _)| *f == v).then(|| v.to_owned()).ok_or("not_allowed"),
        "cd_bitrate_kbps" => number(BITRATE_RANGE.0, BITRATE_RANGE.1),
        "cd_sample_rate_hz" => v.parse::<u32>().ok().filter(|n| SAMPLE_RATES.contains(n)).map(|n| n.to_string()).ok_or("not_allowed"),
        "cd_channel_mode" => one_of(CHANNEL_MODES),
        "cd_quality" => number(0, 9),
        "cd_id3_version" => one_of(ID3_VERSIONS),
        "cd_file_pattern" => template(FILE_VARS, &["disc", "track"], false),
        "cd_folder_pattern" => template(FOLDER_VARS, &["title|album"], true),
        "cd_disc_folder_pattern" => if v.is_empty() { Ok(String::new()) } else { template(DISC_FOLDER_VARS, &["disc"], false) },
        "cd_title_pattern" => one_of(TITLE_SCHEMES),
        "cd_track_scheme" => one_of(TRACK_SCHEMES),
        "cd_genre" => text(!v.is_empty() && plain(v, 100)),
        "cd_cover_px" => number(COVER_PX_RANGE.0, COVER_PX_RANGE.1),
        "cd_eject_after" | "cd_udev" | "cd_verify_read" | "cd_auto_import" | "cd_accept_unverified" => one_of(&["0", "1"]),
        "cd_poll_seconds" => number(POLL_RANGE.0, POLL_RANGE.1),
        "cd_read_speed" => number(READ_SPEED_RANGE.0, READ_SPEED_RANGE.1),
        "cd_verify_shift" => number(VERIFY_SHIFT_RANGE.0, VERIFY_SHIFT_RANGE.1),
        "cd_buffer_seconds" => if v == "0" { Ok("0".to_owned()) } else { number(BUFFER_RANGE.0, BUFFER_RANGE.1) },
        "cd_buffer_max_mb" => number(BUFFER_MB_RANGE.0, BUFFER_MB_RANGE.1),
        "cd_encoder_nice" => v.parse::<i32>().ok().filter(|n| (NICE_RANGE.0..=NICE_RANGE.1).contains(n)).map(|n| n.to_string()).ok_or("out_of_range"),
        "cd_resume_keep_hours" => number(RESUME_HOURS_RANGE.0, RESUME_HOURS_RANGE.1),
        "cd_heal_tolerance_s" => number(HEAL_TOLERANCE_RANGE.0, HEAL_TOLERANCE_RANGE.1),
        "cd_device" => text(v.is_empty() || (v.starts_with('/') && plain(v, 100) && !v.contains("..") && !v.contains(' '))),
        "cd_target_kind" => one_of(KINDS),
        "cd_log_keep" => number(LOG_KEEP_RANGE.0, LOG_KEEP_RANGE.1),
        "musicbrainz_contact" => text(plain(v, 200) && !v.contains(['(', ')'])),
        "cd_musicbrainz_url" | "cd_coverart_url" => {
            if v.is_empty() {
                Ok(String::new())
            } else if (v.starts_with("https://") || v.starts_with("http://")) && plain(v, 300) && !v.contains(' ') {
                Ok(v.trim_end_matches('/').to_owned())
            } else {
                Err("invalid_address")
            }
        }
        _ => Err("not_allowed"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Lit(String),
    Var { name: String, pad: usize },
}

fn parse_template(pattern: &str) -> Option<Vec<Part>> {
    let mut parts = Vec::new();
    let mut lit = String::new();
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                let mut inner = String::new();
                loop {
                    match chars.next()? {
                        '}' => break,
                        '{' => return None,
                        c => inner.push(c),
                    }
                }
                let (name, pad) = match inner.split_once(':') {
                    Some((n, f)) => (n, f.parse::<usize>().ok().filter(|p| (1..=9).contains(p) && f.chars().all(|c| c.is_ascii_digit()))?),
                    None => (inner.as_str(), 0),
                };
                if name.is_empty() {
                    return None;
                }
                if !lit.is_empty() {
                    parts.push(Part::Lit(std::mem::take(&mut lit)));
                }
                parts.push(Part::Var { name: name.to_owned(), pad });
            }
            '}' => return None,
            c => lit.push(c),
        }
    }
    if !lit.is_empty() {
        parts.push(Part::Lit(lit));
    }
    Some(parts)
}

/// Is this a usable template? See `template_check`.
pub fn check_template(pattern: &str, allowed: &[&str], required: &[&str], slashes: bool) -> bool {
    template_check(pattern, allowed, required, slashes).is_ok()
}

/// Is this a usable template? Only the `allowed` placeholders, all of `required` present (`a|b` = one of them), and
/// nothing in the fixed text that is a path trick (`..`, a backslash, characters no system allows in a name,
/// control characters). A slash separates folders and is only allowed when `slashes` is set, and then not at the
/// ends or twice in a row. The error is one of `VALIDATION_CODES`.
pub fn template_check(pattern: &str, allowed: &[&str], required: &[&str], slashes: bool) -> std::result::Result<(), &'static str> {
    if pattern.trim().is_empty() {
        return Err("template_empty");
    }
    if pattern.chars().count() > 200 || pattern.contains("..") {
        return Err("template_unsafe");
    }
    let segments: Vec<&str> = if slashes { pattern.split('/').collect() } else { vec![pattern] };
    let mut seen: Vec<String> = Vec::new();
    for segment in segments {
        if segment.trim().is_empty() {
            return Err("template_unsafe");
        }
        let Some(parts) = parse_template(segment) else { return Err("template_syntax") };
        for part in &parts {
            match part {
                Part::Lit(t) => {
                    if t.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')) {
                        return Err("template_unsafe");
                    }
                }
                Part::Var { name, .. } => {
                    if !allowed.contains(&name.as_str()) {
                        return Err("template_unknown_placeholder");
                    }
                    seen.push(name.clone());
                }
            }
        }
    }
    if required.iter().all(|r| r.split('|').any(|alt| seen.iter().any(|s| s == alt))) { Ok(()) } else { Err("template_missing_placeholder") }
}

/// Fills a template. The values are used as they are; the caller decides whether they must be cleaned for use in a
/// name (`clean` does that for one value). A number format pads numbers with zeros.
pub fn render(pattern: &str, values: &HashMap<&str, String>) -> String {
    let mut out = String::new();
    for part in parse_template(pattern).unwrap_or_default() {
        match part {
            Part::Lit(t) => out.push_str(&t),
            Part::Var { name, pad } => {
                let v = values.get(name.as_str()).cloned().unwrap_or_default();
                if pad > 0 && v.parse::<u64>().is_ok() {
                    out.push_str(&format!("{v:0>pad$}"));
                } else {
                    out.push_str(&v);
                }
            }
        }
    }
    out
}

/// A value for use inside a folder or file name.
pub fn clean(value: &str) -> String {
    crate::upload::clean_name(value, "")
}

impl CdSettings {
    /// Reads the settings. A value that is missing or fails its check falls back to the value of migration 0026.
    pub async fn load(store: &Store) -> Result<CdSettings> {
        let mut map: HashMap<&'static str, String> = HashMap::new();
        for key in KEYS {
            let stored = store.setting(key).await?.unwrap_or_default();
            let value = validate(key, &stored).or_else(|| validate(key, fallback(key))).unwrap_or_default();
            map.insert(key, value);
        }
        Ok(CdSettings::from_map(&map))
    }

    /// The settings with the values of migration 0026 and the following (what a new database holds), for tests and fallbacks.
    pub fn defaults() -> CdSettings {
        let mut m: HashMap<&'static str, String> = HashMap::new();
        for k in KEYS {
            m.insert(k, fallback(k).to_owned());
        }
        CdSettings::from_map(&m)
    }

    fn from_map(m: &HashMap<&'static str, String>) -> CdSettings {
        let get = |k: &str| m.get(k).cloned().unwrap_or_default();
        let format = get("cd_format");
        let extension = FORMATS.iter().find(|(f, _)| *f == format).map_or("mp3", |(_, e)| e).to_owned();
        CdSettings {
            format,
            extension,
            bitrate_kbps: get("cd_bitrate_kbps").parse().unwrap_or(64),
            sample_rate_hz: get("cd_sample_rate_hz").parse().unwrap_or(24000),
            channel_mode: get("cd_channel_mode"),
            quality: get("cd_quality").parse().unwrap_or(3),
            id3_version: get("cd_id3_version"),
            file_pattern: get("cd_file_pattern"),
            folder_pattern: get("cd_folder_pattern"),
            disc_folder_pattern: get("cd_disc_folder_pattern"),
            title_pattern: get("cd_title_pattern"),
            track_scheme: get("cd_track_scheme"),
            genre: get("cd_genre"),
            cover_px: get("cd_cover_px").parse().unwrap_or(500),
            udev: get("cd_udev") == "1",
            device: get("cd_device"),
            poll_seconds: get("cd_poll_seconds").parse().unwrap_or(5),
            verify_read: get("cd_verify_read") == "1",
            read_speed: get("cd_read_speed").parse().unwrap_or(8),
            verify_shift: get("cd_verify_shift").parse().unwrap_or(13),
            accept_unverified: get("cd_accept_unverified") == "1",
            buffer_seconds: get("cd_buffer_seconds").parse().unwrap_or(0),
            buffer_max_mb: get("cd_buffer_max_mb").parse().unwrap_or(256),
            encoder_nice: get("cd_encoder_nice").parse().unwrap_or(-20),
            resume_keep_hours: get("cd_resume_keep_hours").parse().unwrap_or(48),
            auto_import: get("cd_auto_import") == "1",
            heal_tolerance_s: get("cd_heal_tolerance_s").parse().unwrap_or(3),
            log_keep: get("cd_log_keep").parse().unwrap_or(50),
            eject_after: get("cd_eject_after") == "1",
            target_kind: get("cd_target_kind"),
            lookup_ready: !get("musicbrainz_contact").is_empty(),
            contact: get("musicbrainz_contact"),
            musicbrainz_url: get("cd_musicbrainz_url"),
            coverart_url: get("cd_coverart_url"),
        }
    }

    /// The number written to the track tag.
    pub fn tag_track(&self, disc: u32, track: u32) -> u32 {
        if self.track_scheme == "disc_times_100" { disc * 100 + track } else { track }
    }
}

/// The value a missing or damaged setting falls back to (the same as migration 0026).
fn fallback(key: &str) -> &'static str {
    match key {
        "cd_format" => "mp3",
        "cd_bitrate_kbps" => "64",
        "cd_sample_rate_hz" => "24000",
        "cd_channel_mode" => "joint_stereo",
        "cd_quality" => "3",
        "cd_id3_version" => "2.4",
        "cd_file_pattern" => "{author}_{title}_CD{disc:02}_TR{track:02}",
        "cd_folder_pattern" => "{author}/{title}",
        "cd_disc_folder_pattern" => "CD {disc}",
        "cd_title_pattern" => "file_name",
        "cd_track_scheme" => "disc_times_100",
        "cd_genre" => "Audiobook",
        "cd_cover_px" => "500",
        "cd_eject_after" => "1",
        "cd_udev" => "0",
        "cd_poll_seconds" => "5",
        "cd_verify_read" => "1",
        "cd_read_speed" => "8",
        "cd_verify_shift" => "13",
        "cd_accept_unverified" => "0",
        "cd_buffer_seconds" => "0",
        "cd_buffer_max_mb" => "256",
        "cd_encoder_nice" => "-20",
        "cd_resume_keep_hours" => "48",
        "cd_auto_import" => "0",
        "cd_heal_tolerance_s" => "3",
        "cd_log_keep" => "50",
        "cd_target_kind" => "audiobook",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_checked() {
        assert_eq!(validate("cd_bitrate_kbps", " 64 ").as_deref(), Some("64"));
        for bad in ["0", "7", "321", "abc", "-1", ""] {
            assert_eq!(validate("cd_bitrate_kbps", bad), None, "{bad}");
        }
        assert!(validate("cd_sample_rate_hz", "24000").is_some() && validate("cd_sample_rate_hz", "24001").is_none());
        assert!(validate("cd_channel_mode", "mono").is_some() && validate("cd_channel_mode", "quad").is_none());
        assert!(validate("cd_quality", "9").is_some() && validate("cd_quality", "10").is_none());
        assert!(validate("cd_id3_version", "2.3").is_some() && validate("cd_id3_version", "1.1").is_none());
        assert!(validate("cd_format", "mp3").is_some() && validate("cd_format", "wav").is_none());
        assert!(validate("cd_cover_px", "500").is_some() && validate("cd_cover_px", "50").is_none());
        assert!(validate("cd_eject_after", "0").is_some() && validate("cd_eject_after", "yes").is_none());
        assert!(validate("cd_genre", "Audiobook").is_some() && validate("cd_genre", "").is_none());
        assert_eq!(validate("cd_musicbrainz_url", "https://mb.example.org/").as_deref(), Some("https://mb.example.org"));
        assert!(validate("cd_musicbrainz_url", "ftp://x").is_none() && validate("cd_musicbrainz_url", "").is_some());
        assert!(validate("musicbrainz_contact", "me@example.org").is_some() && validate("musicbrainz_contact", "a (b)").is_none());
        assert!(validate("unknown_key", "x").is_none());
    }

    #[test]
    fn templates_know_only_their_placeholders_and_no_path_tricks() {
        for (key, good) in [
            ("cd_file_pattern", "{author}_{title}_CD{disc:02}_TR{track:02}"),
            ("cd_file_pattern", "{disc}-{track} {track_title}"),
            ("cd_folder_pattern", "{author}/{title}"),
            ("cd_folder_pattern", "Books/{album} ({year})"),
            ("cd_disc_folder_pattern", "CD {disc}"),
            ("cd_disc_folder_pattern", ""),
            ("cd_title_pattern", "file_name"),
            ("cd_title_pattern", "track_title"),
        ] {
            assert!(validate(key, good).is_some(), "{key} {good}");
        }
        for (key, bad) in [
            ("cd_file_pattern", "{author}_{album}"),          // no disc and track: names would collide
            ("cd_file_pattern", "{disc}_{track}_{nonsense}"), // unknown placeholder
            ("cd_file_pattern", "{disc}/{track}"),            // a separator in a file name
            ("cd_file_pattern", "..{disc}{track}"),
            ("cd_file_pattern", "{disc}\\{track}"),
            ("cd_file_pattern", "{disc}{track"),
            ("cd_file_pattern", "{disc}{track}}"),
            ("cd_file_pattern", "{disc:x}{track}"),
            ("cd_folder_pattern", "{author}//{album}"),
            ("cd_folder_pattern", "/{author}/{album}"),
            ("cd_folder_pattern", "{author}/../{album}"),
            ("cd_folder_pattern", "{author}"),                // no title
            ("cd_folder_pattern", "{author}/{disc}"),         // disc is no folder placeholder
            ("cd_disc_folder_pattern", "CD/{disc}"),
            ("cd_disc_folder_pattern", "CD"),
            ("cd_title_pattern", ""),
            ("cd_title_pattern", "{file_name}"),
        ] {
            assert!(validate(key, bad).is_none(), "{key} {bad}");
        }
    }

    #[test]
    fn a_refusal_names_its_reason() {
        assert_eq!(check("cd_bitrate_kbps", "0"), Err("out_of_range"));
        assert_eq!(check("cd_channel_mode", "x"), Err("not_allowed"));
        assert_eq!(check("cd_file_pattern", ""), Err("template_empty"));
        assert_eq!(check("cd_file_pattern", "{disc}{track"), Err("template_syntax"));
        assert_eq!(check("cd_file_pattern", "../{disc}{track}"), Err("template_unsafe"));
        assert_eq!(check("cd_file_pattern", "{disc}{track}{x}"), Err("template_unknown_placeholder"));
        assert_eq!(check("cd_file_pattern", "{disc}"), Err("template_missing_placeholder"));
        assert_eq!(check("cd_genre", ""), Err("invalid_text"));
        assert_eq!(check("cd_coverart_url", "ftp://x"), Err("invalid_address"));
        assert_eq!(check("cd_udev", "1"), Ok("1".to_owned()));
        assert_eq!(check("cd_log_keep", "5"), Err("out_of_range"));
        assert_eq!(check("cd_poll_seconds", "1"), Err("out_of_range"));
        assert_eq!(check("cd_poll_seconds", "300"), Ok("300".to_owned()));
        assert_eq!(check("cd_device", ""), Ok(String::new()));
        assert_eq!(check("cd_device", "/dev/sr1"), Ok("/dev/sr1".to_owned()));
        assert_eq!(check("cd_device", "sr1"), Err("invalid_text"));
        assert_eq!(check("cd_device", "/dev/../etc"), Err("invalid_text"));
        assert_eq!(check("cd_verify_read", "0"), Ok("0".to_owned()));
        assert_eq!(check("cd_read_speed", "0"), Ok("0".to_owned()));
        assert_eq!(check("cd_read_speed", "52"), Ok("52".to_owned()));
        assert_eq!(check("cd_read_speed", "53"), Err("out_of_range"));
        assert_eq!(check("cd_accept_unverified", "1"), Ok("1".to_owned()));
        assert_eq!(check("cd_accept_unverified", "2"), Err("not_allowed"));
        assert_eq!(check("cd_verify_shift", "0"), Ok("0".to_owned()));
        assert_eq!(check("cd_verify_shift", "26"), Ok("26".to_owned()));
        assert_eq!(check("cd_verify_shift", "27"), Err("out_of_range"));
        assert_eq!(check("cd_verify_shift", "-1"), Err("out_of_range"));
        assert_eq!(check("cd_buffer_seconds", "0"), Ok("0".to_owned()), "automatic");
        assert_eq!(check("cd_buffer_seconds", "2"), Ok("2".to_owned()));
        assert_eq!(check("cd_buffer_seconds", "900"), Ok("900".to_owned()));
        for bad in ["1", "901", "-1", "x", ""] {
            assert_eq!(check("cd_buffer_seconds", bad), Err("out_of_range"), "{bad}");
        }
        assert_eq!(check("cd_buffer_max_mb", "16"), Ok("16".to_owned()));
        assert_eq!(check("cd_buffer_max_mb", "4096"), Ok("4096".to_owned()));
        assert_eq!(check("cd_buffer_max_mb", "15"), Err("out_of_range"));
        assert_eq!(check("cd_buffer_max_mb", "4097"), Err("out_of_range"));
        assert_eq!(check("cd_encoder_nice", "-20"), Ok("-20".to_owned()));
        assert_eq!(check("cd_encoder_nice", "19"), Ok("19".to_owned()));
        assert_eq!(check("cd_encoder_nice", "-21"), Err("out_of_range"));
        assert_eq!(check("cd_encoder_nice", "20"), Err("out_of_range"));
        assert_eq!(check("cd_encoder_nice", "fast"), Err("out_of_range"));
        assert_eq!((CdSettings::defaults().buffer_seconds, CdSettings::defaults().buffer_max_mb), (0, 256));

    }

    #[test]
    fn a_template_is_filled_with_padded_numbers() {
        let mut v: HashMap<&str, String> = HashMap::new();
        v.insert("author", "Ken Follett".into());
        v.insert("title", "Sturz der Titanen".into());
        v.insert("disc", "2".into());
        v.insert("track", "5".into());
        assert_eq!(render("{author}_{title}_CD{disc:02}_TR{track:02}", &v), "Ken Follett_Sturz der Titanen_CD02_TR05");
        assert_eq!(render("{disc}-{track:03}", &v), "2-005");
        assert_eq!(clean("a/b:c"), "a_b_c");
    }

    #[test]
    fn the_track_tag_follows_the_scheme() {
        let mut m: HashMap<&'static str, String> = HashMap::new();
        for k in KEYS {
            m.insert(k, fallback(k).to_owned());
        }
        let mut s = CdSettings::from_map(&m);
        assert_eq!(s.tag_track(2, 5), 205);
        s.track_scheme = "plain".into();
        assert_eq!(s.tag_track(2, 5), 5);
        assert_eq!((s.bitrate_kbps, s.sample_rate_hz, s.quality, s.extension.as_str()), (64, 24000, 3, "mp3"));
    }

    #[test]
    fn the_fallbacks_pass_their_own_check() {
        for key in KEYS {
            let f = fallback(key);
            assert!(f.is_empty() || validate(key, f).is_some(), "{key}");
        }
    }
}
