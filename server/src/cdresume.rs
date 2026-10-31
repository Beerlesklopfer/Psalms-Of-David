// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Resuming a rip (project owner, 2026-10-28: a restart of the service must not throw away an hour of reading).
//!
//! A rip works in `<data dir>/cd-import/disc-<Disc ID>/`: the tracks as `NN.mp3` (whole) or `NN.mp3.part` (encoded, not yet
//! confirmed by the second reading) and the file `manifest.json`, which says what the files are: the disc, what the
//! settings were that decide the audio (format, bit rate, sample rate, channel mode, quality), the choice of tracks, and
//! for each track its number, file, size, the SHA-1 of the PCM that was read and the length it should have. A track
//! counts as done only when its file and its entry are written; the manifest is written in one piece (`.part`, then
//! rename), so a cut-off manifest is not read at all.
//!
//! The next rip of the same disc takes over what still checks out and reads only the rest. A change of the settings
//! or of the choice of tracks means no resume (the old folder is removed when the new rip starts). Folders of this kind
//! are kept for the time of the setting `cd_resume_keep_hours`; anything else in the staging area (no manifest, a broken
//! one) is removed after a day as before.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cd_settings::CdSettings;

pub const MANIFEST: &str = "manifest.json";
/// The prefix of the folder of a disc.
pub const DIR_PREFIX: &str = "disc-";
/// A file that is not a manifest of a rip is removed after this time.
pub const STRAY_KEEP: Duration = Duration::from_secs(24 * 3600);
/// A finished file may differ from the length the table of contents gives by this much (encoder delay and padding).
pub const LENGTH_TOLERANCE_S: f64 = 2.0;
/// The version of the manifest.
pub const VERSION: u32 = 1;

/// The settings that decide the audio of a rip. The names and tags are given when the files are placed, so they
/// (and the title and author typed in the form) do not matter here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub format: String,
    pub bitrate_kbps: u32,
    pub sample_rate_hz: u32,
    pub channel_mode: String,
    pub quality: u8,
    pub tag_version: String,
}

impl Fingerprint {
    pub fn of(set: &CdSettings) -> Fingerprint {
        // The ripper always writes ID3v2.4; `cd_id3_version` is applied when the files are placed.
        Fingerprint { format: set.format.clone(), bitrate_kbps: set.bitrate_kbps, sample_rate_hz: set.sample_rate_hz, channel_mode: set.channel_mode.clone(), quality: set.quality, tag_version: "2.4".into() }
    }
}

/// A track of the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// The number on the disc.
    pub number: u32,
    /// The final file name below the folder; while the state is `encoded` the file is `<file>.part`.
    pub file: String,
    pub size: u64,
    pub sha1: String,
    /// The length by the table of contents.
    pub seconds: u32,
    /// `encoded` (written, to be confirmed) or `done` (whole and confirmed).
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub v: u32,
    pub disc_id: String,
    pub started_ms: i64,
    /// When the manifest was written last: for an interrupted rip, the time of the interruption (or a little before).
    pub updated_ms: i64,
    pub fingerprint: Fingerprint,
    /// The numbers (on the disc) that the rip is for.
    pub only: Vec<u32>,
    pub author: String,
    pub title: String,
    pub disc: u32,
    pub discs_total: Option<u32>,
    pub kind: String,
    pub tracks: Vec<Entry>,
}

/// The folder of a disc.
pub fn dir_of(staging: &Path, disc_id: &str) -> PathBuf {
    staging.join(format!("{DIR_PREFIX}{disc_id}"))
}

/// Writes the manifest in one piece: the `.part` file is written and synced, then renamed.
pub fn write(dir: &Path, m: &Manifest) -> std::io::Result<()> {
    use std::io::Write;
    let part = dir.join(format!("{MANIFEST}.part"));
    let mut f = std::fs::File::create(&part)?;
    f.write_all(serde_json::to_string(m).map_err(std::io::Error::other)?.as_bytes())?;
    f.sync_all()?;
    std::fs::rename(&part, dir.join(MANIFEST))
}

/// The manifest of a folder; `None` if there is none, or it is cut off, damaged or of another version.
pub fn read(dir: &Path) -> Option<Manifest> {
    let m: Manifest = serde_json::from_slice(&std::fs::read(dir.join(MANIFEST)).ok()?).ok()?;
    (m.v == VERSION).then_some(m)
}

/// What a person is told about an interrupted rip.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ResumeInfo {
    pub disc_id: String,
    pub tracks_done: Vec<u32>,
    /// The number of tracks the rip was for.
    pub tracks_total: usize,
    pub only: Vec<u32>,
    pub interrupted_at: i64,
    pub keep_until: i64,
    /// The settings that decide the audio are the same as then (else the disc is read again).
    pub settings_match: bool,
}

pub fn info(dir: &Path, fingerprint: Option<&Fingerprint>, keep_hours: u64) -> Option<ResumeInfo> {
    let m = read(dir)?;
    let mut done: Vec<u32> = m.tracks.iter().map(|t| t.number).collect();
    done.sort_unstable();
    Some(ResumeInfo {
        disc_id: m.disc_id.clone(),
        tracks_done: done,
        tracks_total: m.only.len(),
        interrupted_at: m.updated_ms,
        keep_until: m.updated_ms + (keep_hours * 3600 * 1000) as i64,
        settings_match: fingerprint.is_none_or(|f| *f == m.fingerprint),
        only: m.only,
    })
}

/// The path of the file of an entry as it lies now.
pub fn file_of(dir: &Path, e: &Entry) -> PathBuf {
    if e.state == "done" { dir.join(&e.file) } else { dir.join(format!("{}.part", e.file)) }
}

/// The entries whose files still check out: present, of the size the manifest says, readable as audio, and as long as
/// the table of contents says (within `LENGTH_TOLERANCE_S`). `lengths` maps a number to the seconds by the table.
pub fn valid_entries(dir: &Path, m: &Manifest, lengths: &HashMap<u32, u32>) -> Vec<Entry> {
    m.tracks.iter().filter(|e| check_entry(dir, e, lengths).is_ok()).cloned().collect()
}

/// Why an entry of the manifest cannot be taken over (`Err`), or `Ok` if its file checks out: `missing` (no file), `size` (not the size
/// the manifest says), `not_on_disc` (the table of contents does not know the track), `unreadable` (not audio), `length` (not as long
/// as the table says, within `LENGTH_TOLERANCE_S`).
pub fn check_entry(dir: &Path, e: &Entry, lengths: &HashMap<u32, u32>) -> Result<(), &'static str> {
    use lofty::file::AudioFile;
    let path = file_of(dir, e);
    let meta = std::fs::metadata(&path).map_err(|_| "missing")?;
    if meta.len() != e.size || e.size == 0 {
        return Err("size");
    }
    let expected = lengths.get(&e.number).ok_or("not_on_disc")?;
    // The file type is MP3 by what the ripper writes. It must not be guessed from the name: an encoded track that waits for the second
    // reading is `NN.mp3.part`, and the guess from the extension `part` finds nothing.
    let tagged = lofty::probe::Probe::open(&path).map_err(|_| "unreadable")?.set_file_type(lofty::file::FileType::Mpeg).read().map_err(|_| "unreadable")?;
    if (tagged.properties().duration().as_secs_f64() - f64::from(*expected)).abs() <= LENGTH_TOLERANCE_S {
        Ok(())
    } else {
        Err("length")
    }
}

/// Removes what is not worth keeping from the staging area: folders of a rip with a good manifest after `keep_hours`
/// from its last write, everything else after `STRAY_KEEP` from its last change.
pub fn sweep(staging: &Path, keep_hours: u64, now_ms: i64) {
    let Ok(entries) = std::fs::read_dir(staging) else { return };
    for e in entries.flatten() {
        let path = e.path();
        // The folders of CDs that wait for approval are not leftovers: they are removed on a decision or on expiry (`cdflow`).
        if e.file_name().to_string_lossy().starts_with(crate::cdflow::HELD_PREFIX) {
            continue;
        }
        let is_rip = e.file_name().to_string_lossy().starts_with(DIR_PREFIX);
        let remove = match (is_rip, if is_rip { read(&path) } else { None }) {
            (true, Some(m)) => now_ms - m.updated_ms > (keep_hours * 3600 * 1000) as i64,
            _ => e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > STRAY_KEEP),
        };
        if remove {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A few seconds of MP3 from ffmpeg; `None` where there is no ffmpeg (the test is then skipped).
    fn mp3_of(seconds: u32) -> Option<Vec<u8>> {
        let out = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-f", "lavfi", "-i", &format!("sine=frequency=440:duration={seconds}:sample_rate=44100"), "-ac", "2", "-b:a", "128k", "-f", "mp3", "-"])
            .output()
            .ok()?;
        out.status.success().then_some(out.stdout)
    }

    #[test]
    fn an_encoded_track_that_waits_for_the_second_reading_is_taken_over_from_its_part_file() {
        let Some(mp3) = mp3_of(4) else { return };
        let d = tempfile::tempdir().unwrap();
        let lengths: HashMap<u32, u32> = [(1, 4), (2, 4)].into_iter().collect();
        // Written but not confirmed: the file is `01.mp3.part`, the entry says `encoded`.
        std::fs::write(d.path().join("01.mp3.part"), &mp3).unwrap();
        let encoded = Entry { number: 1, file: "01.mp3".into(), size: mp3.len() as u64, sha1: String::new(), seconds: 4, state: "encoded".into() };
        assert_eq!(check_entry(d.path(), &encoded, &lengths), Ok(()), "a part file is read as the MP3 it is");
        // The same for a confirmed one (`done`, the file has its final name).
        std::fs::write(d.path().join("02.mp3"), &mp3).unwrap();
        let done = Entry { number: 2, file: "02.mp3".into(), state: "done".into(), ..encoded.clone() };
        assert_eq!(check_entry(d.path(), &Entry { number: 2, ..done }, &lengths), Ok(()));
        // A part file of the wrong length is still refused.
        let longer: HashMap<u32, u32> = [(1, 60)].into_iter().collect();
        assert_eq!(check_entry(d.path(), &encoded, &longer), Err("length"));
    }

    #[test]
    fn an_entry_that_cannot_be_taken_over_says_why() {
        let d = tempfile::tempdir().unwrap();
        let lengths: HashMap<u32, u32> = [(1, 385)].into_iter().collect();
        let entry = |n: u32, size: u64| Entry { number: n, file: format!("{n:02}.mp3"), size, sha1: String::new(), seconds: 385, state: "encoded".into() };
        assert_eq!(check_entry(d.path(), &entry(1, 10), &lengths), Err("missing"), "an encoded track is the file with .part");
        std::fs::write(d.path().join("01.mp3.part"), b"not audio at all").unwrap();
        assert_eq!(check_entry(d.path(), &entry(1, 999), &lengths), Err("size"));
        assert_eq!(check_entry(d.path(), &entry(2, 16), &lengths), Err("missing"), "no file for track 2");
        std::fs::write(d.path().join("02.mp3.part"), b"not audio at all").unwrap();
        assert_eq!(check_entry(d.path(), &entry(2, 16), &lengths), Err("not_on_disc"));
        assert_eq!(check_entry(d.path(), &entry(1, 16), &lengths), Err("unreadable"));
    }

    fn manifest() -> Manifest {
        Manifest {
            v: VERSION,
            disc_id: "AzCi4YfdjUrdmGcBX7URtQixDmE-".into(),
            started_ms: 1,
            updated_ms: 1000,
            fingerprint: Fingerprint { format: "mp3".into(), bitrate_kbps: 64, sample_rate_hz: 24000, channel_mode: "joint_stereo".into(), quality: 3, tag_version: "2.4".into() },
            only: vec![1, 2, 3],
            author: "A".into(),
            title: "B".into(),
            disc: 2,
            discs_total: Some(12),
            kind: "audiobook".into(),
            tracks: vec![Entry { number: 1, file: "01.mp3".into(), size: 10, sha1: "ab".into(), seconds: 120, state: "done".into() }],
        }
    }

    #[test]
    fn the_manifest_is_written_in_one_piece_and_a_cut_off_one_is_ignored() {
        let d = tempfile::tempdir().unwrap();
        assert!(read(d.path()).is_none());
        write(d.path(), &manifest()).unwrap();
        assert_eq!(read(d.path()), Some(manifest()));
        assert!(!d.path().join("manifest.json.part").exists());
        // Cut off (a crash while writing the file in place would look like this), damaged, another version.
        let text = std::fs::read_to_string(d.path().join(MANIFEST)).unwrap();
        std::fs::write(d.path().join(MANIFEST), &text[..text.len() / 2]).unwrap();
        assert!(read(d.path()).is_none());
        std::fs::write(d.path().join(MANIFEST), "not json").unwrap();
        assert!(read(d.path()).is_none());
        let mut other = manifest();
        other.v = 99;
        write(d.path(), &other).unwrap();
        assert!(read(d.path()).is_none());
        // A `.part` manifest that was never renamed is no manifest.
        std::fs::remove_file(d.path().join(MANIFEST)).unwrap();
        std::fs::write(d.path().join("manifest.json.part"), text).unwrap();
        assert!(read(d.path()).is_none());
    }

    #[test]
    fn the_info_names_the_done_tracks_the_time_and_whether_the_settings_match() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), &manifest()).unwrap();
        let i = info(d.path(), Some(&manifest().fingerprint), 48).unwrap();
        assert_eq!((i.tracks_done.clone(), i.tracks_total, i.interrupted_at, i.settings_match), (vec![1], 3, 1000, true));
        assert_eq!(i.keep_until, 1000 + 48 * 3600 * 1000);
        let mut changed = manifest().fingerprint;
        changed.bitrate_kbps = 96;
        assert!(!info(d.path(), Some(&changed), 48).unwrap().settings_match);
    }

    #[test]
    fn the_sweep_keeps_a_good_rip_for_the_hours_and_other_things_for_a_day() {
        let d = tempfile::tempdir().unwrap();
        let good = d.path().join("disc-AAA");
        let broken = d.path().join("disc-BBB");
        let other = d.path().join("some-upload");
        for p in [&good, &broken, &other] {
            std::fs::create_dir(p).unwrap();
        }
        write(&good, &manifest()).unwrap();
        std::fs::write(broken.join(MANIFEST), "{").unwrap();
        // 47 hours after the last write: kept. 49: removed. Folders without a good manifest are as young as the file system says.
        sweep(d.path(), 48, 1000 + 47 * 3600 * 1000);
        assert!(good.exists() && broken.exists() && other.exists());
        sweep(d.path(), 48, 1000 + 49 * 3600 * 1000);
        assert!(!good.exists() && broken.exists() && other.exists(), "the others are not older than a day");
    }

    #[test]
    fn a_file_is_valid_when_it_is_there_whole_and_as_long_as_the_table_says() {
        let d = tempfile::tempdir().unwrap();
        let mut m = manifest();
        let lengths: HashMap<u32, u32> = [(1, 120)].into_iter().collect();
        // Not there / wrong size / not audio: dropped.
        assert!(valid_entries(d.path(), &m, &lengths).is_empty());
        std::fs::write(d.path().join("01.mp3"), [0u8; 10]).unwrap();
        assert!(valid_entries(d.path(), &m, &lengths).is_empty(), "not audio");
        m.tracks[0].size = 11;
        assert!(valid_entries(d.path(), &m, &lengths).is_empty(), "wrong size");
    }
}
