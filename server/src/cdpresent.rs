// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Is this CD in the library already, and is it whole? (project owner, 2026-10-28)
//!
//! A CD counts as present when
//! * an earlier import stored its Disc ID (tables `cd_disc` and `cd_disc_track`, filled by every import and every healing), or
//! * the library has a book of the same author and title with tracks of this CD number (how the CDs were ripped by hand
//!   before: `Ken Follett/Sturz der Titanen/Follett_Sturz der Titanen_CD02_TR05.mp3`, no Disc ID known).
//!
//! `Present` compares the table of contents of the disc with the files: a number of the disc without a file is MISSING; a
//! file that is empty, cannot be read as audio or differs from the length of the table by more than the tolerance
//! (`cd_heal_tolerance_s`) is DAMAGED. The SHA-1 of the PCM that is stored with a track is kept for information: the server
//! has no decoder, so it cannot be compared with a file.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
use serde::Serialize;

use crate::cd_settings::CdSettings;
use crate::error::Result;
use crate::store::Store;

/// Where a CD was found: how, author, book, CD number, kind, import folder, files by number.
type Found = (&'static str, String, String, u32, String, PathBuf, BTreeMap<u32, PathBuf>);

/// One file that is not as it should be.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Damage {
    pub number: u32,
    /// `empty`, `unreadable` or `length`.
    pub reason: &'static str,
    /// The length of the file and the length by the table of contents, in seconds (0 if the file cannot be read).
    pub have_s: u32,
    pub want_s: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Present {
    /// How it was found: `disc_id` or `book`.
    pub via: &'static str,
    pub author: String,
    pub book: String,
    pub disc: u32,
    pub kind: String,
    /// How many CDs the book has, as an import stored it (not known when the book was found by its name).
    #[serde(skip)]
    pub discs_total: Option<u32>,
    pub tracks_in_library: usize,
    pub tracks_on_disc: usize,
    pub missing: Vec<u32>,
    pub damaged: Vec<Damage>,
    /// The folder with the files of this CD (where repaired files go).
    #[serde(skip)]
    pub dir: PathBuf,
    /// The import folder the book lies in.
    #[serde(skip)]
    pub root: PathBuf,
    /// The files of this CD by number.
    #[serde(skip)]
    pub files: BTreeMap<u32, PathBuf>,
}

impl Present {
    /// Is something missing or damaged?
    pub fn needs_healing(&self) -> bool {
        !self.missing.is_empty() || !self.damaged.is_empty()
    }

    /// The numbers to read again: missing and damaged ones.
    pub fn to_heal(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.missing.iter().copied().chain(self.damaged.iter().map(|d| d.number)).collect();
        v.sort_unstable();
        v.dedup();
        v
    }
}

/// What the caller knows of the disc besides its Disc ID: the book it is believed to be (typed in, or the one release
/// MusicBrainz knows).
#[derive(Debug, Clone)]
pub struct Hint {
    pub author: String,
    pub title: String,
    pub disc: u32,
}

fn eq(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// The state of one file against the table of contents.
pub fn check_file(path: &Path, want_s: u32, tolerance_s: u32) -> Option<Damage> {
    use lofty::file::AudioFile;
    let number = 0;
    let Ok(meta) = std::fs::metadata(path) else { return Some(Damage { number, reason: "unreadable", have_s: 0, want_s }) };
    if meta.len() == 0 {
        return Some(Damage { number, reason: "empty", have_s: 0, want_s });
    }
    let Ok(tagged) = lofty::read_from_path(path) else { return Some(Damage { number, reason: "unreadable", have_s: 0, want_s }) };
    let have = tagged.properties().duration().as_secs_f64();
    ((have - f64::from(want_s)).abs() > f64::from(tolerance_s)).then(|| Damage { number, reason: "length", have_s: have.round() as u32, want_s })
}

/// Looks for the disc in the library. `lengths` maps a number on the disc to its length in seconds (the table of
/// contents). `None` if the disc is not in the library.
pub async fn find(store: &Store, library: Option<&Path>, set: &CdSettings, disc_id: &str, lengths: &HashMap<u32, u32>, hint: Option<&Hint>) -> Option<Present> {
    let mut found: Option<Found> = None;
    let mut discs_total: Option<u32> = None;
    // 1. By the Disc ID that an import stored.
    if let Ok(Some((row, tracks))) = store.cd_disc(disc_id).await
        && let Ok(root) = crate::cdimport::import_root(store, library, &row.kind).await
    {
        let files: BTreeMap<u32, PathBuf> = tracks.iter().map(|t| (t.number, root.join(&t.file))).collect();
        if !files.is_empty() {
            // An import that did not know the number of CDs stored 1: that is no total for CD 4.
            discs_total = Some(row.discs_total).filter(|n| *n > 0 && *n >= row.disc);
            found = Some(("disc_id", row.author, row.title, row.disc, row.kind, root.clone(), files));
        }
    }
    // 2. By the book: the same author and title, with tracks of this CD number.
    if found.is_none()
        && let Some(h) = hint
        && let Ok(rows) = store.admin_book_rows().await
    {
        type Best = (String, String, String, PathBuf, BTreeMap<u32, PathBuf>);
        let mut best: Option<Best> = None;
        for row in rows.iter().filter(|r| eq(&r.author, &h.author) && eq(&r.title, &h.title)) {
            let Ok(tracks) = store.tracks_of(row.id).await else { continue };
            let Some((dir, _, _)) = crate::relocate::book_dir(store, library, row.id).await else { continue };
            let files: BTreeMap<u32, PathBuf> = tracks.iter().filter(|t| t.disc == i64::from(h.disc)).map(|t| (t.number as u32, dir.join(&t.file))).collect();
            if files.len() > best.as_ref().map_or(0, |b| b.4.len()) {
                let root = crate::cdimport::import_root(store, library, &row.kind).await.unwrap_or_default();
                best = Some((row.author.clone(), row.title.clone(), row.kind.clone(), root, files));
            }
        }
        if let Some((author, title, kind, root, files)) = best {
            found = Some(("book", author, title, h.disc, kind, root, files));
        }
    }
    let (via, author, book, disc, kind, root, files) = found?;
    // The number of CDs of the book: as the import of this disc stored it, else what an import of another CD of the same book stored.
    if discs_total.is_none() {
        discs_total = store.cd_discs_total_of(&author, &book).await.ok().flatten().filter(|n| *n >= disc);
    }
    let dir = files.values().next().and_then(|f| f.parent()).map(Path::to_path_buf).unwrap_or_default();
    let mut missing = Vec::new();
    let mut damaged = Vec::new();
    let mut numbers: Vec<u32> = lengths.keys().copied().collect();
    numbers.sort_unstable();
    for n in &numbers {
        match files.get(n) {
            // Not in the list, or listed but gone from the disk.
            None => missing.push(*n),
            Some(path) if !path.exists() => missing.push(*n),
            Some(path) => {
                if let Some(mut d) = check_file(path, lengths[n], set.heal_tolerance_s) {
                    d.number = *n;
                    damaged.push(d);
                }
            }
        }
    }
    let in_library = numbers.iter().filter(|n| files.get(n).is_some_and(|f| f.exists())).count();
    Some(Present { via, author, book, disc, kind, discs_total, tracks_in_library: in_library, tracks_on_disc: numbers.len(), missing, damaged, dir, root, files })
}

/// The file name for a healed track: the pattern of the files of this CD that are there (`Follett_..._CD02_TR05.mp3` gives
/// `Follett_..._CD02_TR09.mp3`), so that the files of a CD stay uniform; `None` if no file has the `_TRnn` mark.
pub fn healed_name(existing: &BTreeMap<u32, PathBuf>, number: u32) -> Option<String> {
    for path in existing.values() {
        let name = path.file_name()?.to_string_lossy().into_owned();
        let upper = name.to_ascii_uppercase();
        let Some(at) = upper.rfind("_TR") else { continue };
        let digits: String = upper[at + 3..].chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            continue;
        }
        let rest = &name[at + 3 + digits.len()..];
        return Some(format!("{}_TR{number:0width$}{rest}", &name[..at], width = digits.len()));
    }
    None
}

// The tables.

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiscRow {
    pub disc_id: String,
    pub author: String,
    pub title: String,
    pub disc: u32,
    pub discs_total: u32,
    pub kind: String,
    pub folder: String,
}

#[derive(Debug, Clone)]
pub struct DiscTrackRow {
    pub number: u32,
    pub file: String,
    pub sha1: String,
    pub seconds: u32,
}

/// What an import (or a healing) writes down about a disc.
#[derive(Debug, Clone)]
pub struct DiscRecord {
    pub row: DiscRow,
    /// Number, file below the import folder, SHA-1 of the PCM, seconds.
    pub tracks: Vec<DiscTrackRow>,
}

impl Store {
    pub async fn cd_disc(&self, disc_id: &str) -> Result<Option<(DiscRow, Vec<DiscTrackRow>)>> {
        let Some(r) = self
            .db
            .writer
            .query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT id, disc_id, author, title, disc, discs_total, kind, folder FROM cd_disc WHERE disc_id = ?", [disc_id.to_owned().into()]))
            .await?
        else {
            return Ok(None);
        };
        let id: i64 = r.try_get_by_index(0)?;
        let row = DiscRow { disc_id: r.try_get_by_index(1)?, author: r.try_get_by_index(2)?, title: r.try_get_by_index(3)?, disc: r.try_get_by_index::<i64>(4)? as u32, discs_total: r.try_get_by_index::<i64>(5)? as u32, kind: r.try_get_by_index(6)?, folder: r.try_get_by_index(7)? };
        let rows = self
            .db
            .writer
            .query_all_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT number, file, sha1, seconds FROM cd_disc_track WHERE cd_disc_id = ? ORDER BY number", [id.into()]))
            .await?;
        let tracks = rows
            .iter()
            .map(|t| Ok(DiscTrackRow { number: t.try_get_by_index::<i64>(0)? as u32, file: t.try_get_by_index(1)?, sha1: t.try_get_by_index(2)?, seconds: t.try_get_by_index::<i64>(3)? as u32 }))
            .collect::<Result<Vec<_>>>()?;
        Ok(Some((row, tracks)))
    }

    /// The largest number of CDs that an import stored for a book (author and title), if any.
    pub async fn cd_discs_total_of(&self, author: &str, title: &str) -> Result<Option<u32>> {
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT max(discs_total) FROM cd_disc WHERE author = ? AND title = ?", [author.to_owned().into(), title.to_owned().into()]))
            .await?;
        let n: Option<i64> = match row {
            Some(r) => r.try_get_by_index(0)?,
            None => None,
        };
        Ok(n.filter(|n| *n > 0).map(|n| n as u32))
    }

    /// Writes down a disc and its tracks; a track that is there already is replaced (healing).
    pub async fn record_cd_disc(&self, rec: &DiscRecord) -> Result<()> {
        let r = &rec.row;
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO cd_disc (disc_id, author, title, disc, discs_total, kind, folder) VALUES (?, ?, ?, ?, ?, ?, ?) \
                 ON CONFLICT (disc_id) DO UPDATE SET author = excluded.author, title = excluded.title, disc = excluded.disc, discs_total = excluded.discs_total, kind = excluded.kind, folder = excluded.folder",
                [r.disc_id.clone().into(), r.author.clone().into(), r.title.clone().into(), i64::from(r.disc).into(), i64::from(r.discs_total).into(), r.kind.clone().into(), r.folder.clone().into()],
            ))
            .await?;
        for t in &rec.tracks {
            let values: Vec<Value> = vec![r.disc_id.clone().into(), i64::from(t.number).into(), t.file.clone().into(), t.sha1.clone().into(), i64::from(t.seconds).into()];
            self.db
                .writer
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "INSERT INTO cd_disc_track (cd_disc_id, number, file, sha1, seconds) VALUES ((SELECT id FROM cd_disc WHERE disc_id = ?), ?, ?, ?, ?) \
                     ON CONFLICT (cd_disc_id, number) DO UPDATE SET file = excluded.file, sha1 = excluded.sha1, seconds = excluded.seconds",
                    values,
                ))
                .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_healed_file_gets_the_name_pattern_of_its_neighbours() {
        let mut files: BTreeMap<u32, PathBuf> = BTreeMap::new();
        files.insert(5, PathBuf::from("/lib/Ken Follett/Sturz der Titanen/Follett_Sturz der Titanen_CD02_TR05.mp3"));
        assert_eq!(healed_name(&files, 9).as_deref(), Some("Follett_Sturz der Titanen_CD02_TR09.mp3"));
        assert_eq!(healed_name(&files, 12).as_deref(), Some("Follett_Sturz der Titanen_CD02_TR12.mp3"));
        files.clear();
        files.insert(1, PathBuf::from("/lib/x/track one.mp3"));
        assert_eq!(healed_name(&files, 2), None, "no mark: the setting decides");
        assert_eq!(healed_name(&BTreeMap::new(), 2), None);
    }

    #[test]
    fn a_file_is_damaged_when_it_is_empty_unreadable_or_the_wrong_length() {
        let d = tempfile::tempdir().unwrap();
        let empty = d.path().join("a.mp3");
        std::fs::write(&empty, b"").unwrap();
        assert_eq!(check_file(&empty, 100, 3).unwrap().reason, "empty");
        let junk = d.path().join("b.mp3");
        std::fs::write(&junk, vec![1u8; 500]).unwrap();
        assert_eq!(check_file(&junk, 100, 3).unwrap().reason, "unreadable");
        assert_eq!(check_file(&d.path().join("none.mp3"), 100, 3).unwrap().reason, "unreadable");
    }
}
