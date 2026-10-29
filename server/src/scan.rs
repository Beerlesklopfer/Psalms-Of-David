// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Library scan: finds `Author/Title/*.mp3` below a root folder and imports it.
//!
//! Disc and track number come from the file name (`..._CD02_TR05`), which is how
//! the ripped audiobooks are named, and fall back to the audio tags.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;

use lofty::prelude::*;
use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait, Value};

use crate::error::{Error, Result};
use crate::store::{Store, now_ms};
use crate::tags;

/// Tracks written per transaction, so the write connection is never held long.
const TRACK_CHUNK: usize = 100;

const AUDIO_EXTENSIONS: &[&str] = &["mp3", "m4a", "m4b", "flac", "ogg", "opus"];

#[derive(Debug, Clone)]
pub struct FoundTrack {
    pub disc: i64,
    pub number: i64,
    pub title: String,
    /// Relative to the book folder.
    pub file: String,
    pub duration_ms: i64,
    pub size_bytes: i64,
    pub mtime: i64,
}

#[derive(Debug, Clone)]
pub struct FoundBook {
    /// `audiobook` or `music`.
    pub kind: String,
    pub author: String,
    pub title: String,
    /// Relative to the library root, e.g. `Follett/Kinder der Freiheit`.
    pub folder: String,
    /// The medium's own age recommendation, read from the tags of its first track.
    pub recommended_age: Option<i64>,
    /// Read from the tags of the first track (the composer field); `None` if they do not say.
    pub narrator: Option<String>,
    /// The genre as the tags of the first track write it (free text), `None` if they do not say.
    pub genre: Option<String>,
    pub tracks: Vec<FoundTrack>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ScanReport {
    pub books: usize,
    pub tracks_added: usize,
    pub tracks_updated: usize,
    pub tracks_unchanged: usize,
    pub tracks_removed: usize,
    /// Books released by the rating rule "recommendation".
    pub books_released: usize,
    /// Albums that could not be imported (they are logged and skipped).
    pub books_failed: usize,
    /// Places that could not be read, with the reason; the rest of the import went on without them.
    pub problems: Vec<String>,
    /// What the library holds after the run (not what changed): audiobooks, their authors,
    /// albums, their artists and all tracks.
    pub audiobooks: usize,
    pub authors: usize,
    pub albums: usize,
    pub artists: usize,
    pub tracks_total: usize,
}

impl ScanReport {
    pub fn merge(&mut self, other: &ScanReport) {
        self.books += other.books;
        self.tracks_added += other.tracks_added;
        self.tracks_updated += other.tracks_updated;
        self.tracks_unchanged += other.tracks_unchanged;
        self.tracks_removed += other.tracks_removed;
        self.books_released += other.books_released;
        self.books_failed += other.books_failed;
        self.problems.extend(other.problems.iter().cloned());
        // The totals describe the library, not one folder: the last one to speak wins.
        if other.tracks_total > 0 {
            self.audiobooks = other.audiobooks;
            self.authors = other.authors;
            self.albums = other.albums;
            self.artists = other.artists;
            self.tracks_total = other.tracks_total;
        }
    }
}

impl Store {
    /// Scans `root` and imports what it finds. Reading the files happens on a
    /// blocking thread; the database is only touched in short transactions.
    pub async fn scan(&self, root: &Path) -> Result<ScanReport> {
        self.scan_kind(root, "audiobook").await
    }

    /// Like [`Store::scan`], for a folder of a given kind (`audiobook` or `music`).
    pub async fn scan_kind(&self, root: &Path, kind: &'static str) -> Result<ScanReport> {
        self.scan_skipping(root, kind, &[]).await
    }

    /// Like [`Store::scan_kind`], leaving out `skip`: other import folders inside this one.
    pub async fn scan_skipping(&self, root: &Path, kind: &'static str, skip: &[PathBuf]) -> Result<ScanReport> {
        let (root, skip) = (root.to_owned(), skip.to_vec());
        let placeholders = self.placeholder_tags().await?;
        let (found, problems) = tokio::task::spawn_blocking(move || {
            let mut problems = Vec::new();
            collect(&root, kind, &skip, &placeholders, &mut problems).map(|books| (books, problems))
        })
        .await??;
        let mut report = self.import_books(found).await?;
        report.problems.extend(problems);
        Ok(report)
    }

    /// Imports books and tracks. Existing rows are updated only if the file
    /// changed (size or modification time); files that vanished are removed.
    ///
    /// Note: removing a track also removes listening progress that points at it
    /// (`ON DELETE CASCADE`).
    pub async fn import_books(&self, books: Vec<FoundBook>) -> Result<ScanReport> {
        let mut report = ScanReport::default();
        for found in books {
            // One album that cannot be imported (say, two files with the same track number) must not
            // stop the others: it is logged, counted and skipped.
            match self.import_book(&found, &mut report).await {
                Ok(()) => {}
                Err(e) => {
                    tracing::warn!(book = %found.folder, error = %e, "this album could not be imported");
                    report.books_failed += 1;
                }
            }
        }
        Ok(report)
    }

    async fn import_book(&self, found: &FoundBook, report: &mut ScanReport) -> Result<()> {
        let book_id = {
            let txn = self.db.writer.begin().await?;
            let author_id = upsert_author(&txn, &found.author).await?;
            let book_id = upsert_book(&txn, author_id, found).await?;
            txn.commit().await?;
            book_id
        };
        report.books += 1;

        let existing = existing_tracks(&self.db.reader, book_id).await?;
        let present: HashSet<&str> = found.tracks.iter().map(|t| t.file.as_str()).collect();
        // Rows of files that are no longer there, by (disc, number): a file that was renamed shows up as
        // a new name for the same track, and its row (with the listening progress on it) is kept.
        let mut gone: HashMap<(i64, i64), String> = existing_numbers(&self.db.reader, book_id)
            .await?
            .into_iter()
            .filter(|(_, file)| !present.contains(file.as_str()))
            .collect();

        for chunk in found.tracks.chunks(TRACK_CHUNK) {
            let txn = self.db.writer.begin().await?;
            for t in chunk {
                match existing.get(&t.file) {
                    Some(&(mtime, size)) if mtime == t.mtime && size == t.size_bytes => {
                        report.tracks_unchanged += 1;
                        continue;
                    }
                    Some(_) => report.tracks_updated += 1,
                    None => match gone.remove(&(t.disc, t.number)) {
                        Some(old_file) => {
                            rename_track(&txn, book_id, &old_file, t).await?;
                            report.tracks_updated += 1;
                            continue;
                        }
                        None => report.tracks_added += 1,
                    },
                }
                upsert_track(&txn, book_id, t).await?;
            }
            txn.commit().await?;
        }

        if !gone.is_empty() {
            let txn = self.db.writer.begin().await?;
            for file in gone.values() {
                let values: [Value; 2] = [book_id.into(), file.clone().into()];
                txn.execute_raw(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "DELETE FROM track WHERE book_id = ? AND file = ?",
                    values,
                ))
                .await?;
                report.tracks_removed += 1;
            }
            txn.commit().await?;
        }
        Ok(())
    }

}

/// (disc, number) and file of the tracks the database holds for a book.
async fn existing_numbers(conn: &impl ConnectionTrait, book_id: i64) -> Result<Vec<((i64, i64), String)>> {
    let values: [Value; 1] = [book_id.into()];
    let rows = conn
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT disc, number, file FROM track WHERE book_id = ?",
            values,
        ))
        .await?;
    rows.iter().map(|r| Ok(((r.try_get_by_index(0)?, r.try_get_by_index(1)?), r.try_get_by_index(2)?))).collect()
}

/// The file of a track was renamed: the row stays, with its id and the progress that points at it.
async fn rename_track(conn: &impl ConnectionTrait, book_id: i64, old_file: &str, t: &FoundTrack) -> Result<()> {
    let values: [Value; 7] = [
        t.file.clone().into(),
        t.title.clone().into(),
        t.duration_ms.into(),
        t.size_bytes.into(),
        t.mtime.into(),
        book_id.into(),
        old_file.to_owned().into(),
    ];
    conn.execute_raw(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE track SET file = ?, title = ?, duration_ms = ?, size_bytes = ?, mtime = ? WHERE book_id = ? AND file = ?",
        values,
    ))
    .await?;
    Ok(())
}

async fn existing_tracks(
    conn: &impl ConnectionTrait,
    book_id: i64,
) -> Result<HashMap<String, (i64, i64)>> {
    let values: [Value; 1] = [book_id.into()];
    let rows = conn
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT file, mtime, size_bytes FROM track WHERE book_id = ?",
            values,
        ))
        .await?;
    let mut map = HashMap::new();
    for row in rows {
        let file: String = row.try_get_by_index(0)?;
        let mtime: i64 = row.try_get_by_index(1)?;
        let size: i64 = row.try_get_by_index(2)?;
        map.insert(file, (mtime, size));
    }
    Ok(map)
}

async fn upsert_author(conn: &impl ConnectionTrait, name: &str) -> Result<i64> {
    conn.execute_raw(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO author (name) VALUES (?) ON CONFLICT (name) DO NOTHING",
        [Value::from(name.to_owned())],
    ))
    .await?;
    let row = conn
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM author WHERE name = ?",
            [Value::from(name.to_owned())],
        ))
        .await?
        .ok_or_else(|| Error::WriteFailed(format!("author {name} not found after insert")))?;
    Ok(row.try_get_by_index(0)?)
}

impl Store {
    /// A book that was moved to another folder keeps its id (and with it the listening progress):
    /// the folder, the author, the title and the kind follow the move.
    pub async fn relocate_book(&self, id: i64, folder: &str, author: &str, title: &str) -> Result<()> {
        let author_id = upsert_author(&self.db.writer, author).await?;
        let values: [Value; 5] = [folder.to_owned().into(), author_id.into(), title.to_owned().into(), "audiobook".to_owned().into(), id.into()];
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE book SET folder = ?, author_id = ?, title = ?, kind = ? WHERE id = ?",
                values,
            ))
            .await?;
        Ok(())
    }
}

impl Store {
    /// A book whose folder, author and title changed by a rename; unlike `relocate_book` the kind stays.
    pub async fn rename_book(&self, id: i64, folder: &str, author: &str, title: &str) -> Result<()> {
        let author_id = upsert_author(&self.db.writer, author).await?;
        let values: [Value; 4] = [folder.to_owned().into(), author_id.into(), title.to_owned().into(), id.into()];
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE book SET folder = ?, author_id = ?, title = ? WHERE id = ?", values))
            .await?;
        Ok(())
    }
}

impl Store {
    /// Puts the CDs of one work together: the tracks of every album in `discs` (`(book id, disc)`)
    /// move into the book `keep`, as disc folders `CD n`, and the other books go. What the family
    /// listened to stays: progress, favourites and releases of a CD move to the book that remains
    /// (unless the book has an entry of its own). All or nothing.
    pub async fn merge_books(&self, keep: i64, discs: &[(i64, i64)], folder: &str, author: &str, title: &str) -> Result<()> {
        let author_id = upsert_author(&self.db.writer, author).await?;
        let txn = self.db.writer.begin().await?;
        let run = |sql: &'static str, values: Vec<Value>| Statement::from_sql_and_values(DbBackend::Sqlite, sql, values);
        for &(id, disc) in discs {
            // The disc and the new place of the files first: the tracks of two discs may share numbers.
            txn.execute_raw(run("UPDATE track SET disc = ?, file = 'CD ' || ? || '/' || file WHERE book_id = ?", vec![disc.into(), disc.into(), id.into()])).await?;
        }
        for &(id, _) in discs.iter().filter(|(id, _)| *id != keep) {
            txn.execute_raw(run("UPDATE track SET book_id = ? WHERE book_id = ?", vec![keep.into(), id.into()])).await?;
            for table in ["progress", "favorite", "book_release"] {
                let sql: &'static str = match table {
                    "progress" => "UPDATE OR IGNORE progress SET book_id = ? WHERE book_id = ?",
                    "favorite" => "UPDATE OR IGNORE favorite SET book_id = ? WHERE book_id = ?",
                    _ => "UPDATE OR IGNORE book_release SET book_id = ? WHERE book_id = ?",
                };
                txn.execute_raw(run(sql, vec![keep.into(), id.into()])).await?;
            }
            txn.execute_raw(run("DELETE FROM book WHERE id = ?", vec![id.into()])).await?;
        }
        txn.execute_raw(run(
            "UPDATE book SET folder = ?, author_id = ?, title = ?, kind = 'audiobook' WHERE id = ?",
            vec![folder.to_owned().into(), author_id.into(), title.to_owned().into(), keep.into()],
        ))
        .await?;
        txn.commit().await?;
        Ok(())
    }
}

async fn upsert_book(conn: &impl ConnectionTrait, author_id: i64, b: &FoundBook) -> Result<i64> {
    let values: [Value; 8] = [
        b.narrator.clone().into(),
        b.title.clone().into(),
        author_id.into(),
        b.folder.clone().into(),
        now_ms().into(),
        b.recommended_age.into(),
        b.kind.clone().into(),
        b.genre.clone().into(),
    ];
    let row = conn
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO book (narrator, title, author_id, folder, added_at, recommended_age, kind, genre_tag) VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT (folder) DO UPDATE SET title = excluded.title, author_id = excluded.author_id, kind = excluded.kind, narrator = excluded.narrator, \
               genre_tag = excluded.genre_tag, \
               recommended_age = coalesce(excluded.recommended_age, book.recommended_age) \
             RETURNING id",
            values,
        ))
        .await?
        .ok_or_else(|| Error::WriteFailed("book upsert returned no row".into()))?;
    let id: i64 = row.try_get_by_index(0)?;
    // The genre of the album follows the alias of its tag text, unless somebody chose it by hand.
    conn.execute_raw(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE book SET genre_id = (SELECT genre_id FROM genre_alias WHERE tag_text = book.genre_tag) WHERE id = ? AND genre_locked = 0",
        [Value::from(id)],
    ))
    .await?;
    Ok(id)
}

async fn upsert_track(conn: &impl ConnectionTrait, book_id: i64, t: &FoundTrack) -> Result<()> {
    let values: [Value; 8] = [
        book_id.into(),
        t.disc.into(),
        t.number.into(),
        t.title.clone().into(),
        t.file.clone().into(),
        t.duration_ms.into(),
        t.size_bytes.into(),
        t.mtime.into(),
    ];
    conn.execute_raw(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO track (book_id, disc, number, title, file, duration_ms, size_bytes, mtime) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT (book_id, file) DO UPDATE SET \
           disc = excluded.disc, number = excluded.number, title = excluded.title, \
           duration_ms = excluded.duration_ms, size_bytes = excluded.size_bytes, mtime = excluded.mtime",
        values,
    ))
    .await?;
    Ok(())
}

/// A folder that holds no file at all, however deep (only empty folders inside).
pub(crate) fn is_empty_tree(dir: &Path) -> bool {
    match fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).all(|e| e.file_type().is_ok_and(|t| t.is_dir()) && is_empty_tree(&e.path())),
        Err(_) => false,
    }
}

/// How deep below an import folder the scan looks for albums.
const MAX_DEPTH: usize = 8;

/// Walks an import folder (blocking). **Every folder that directly holds audio files is a book or an
/// album**, however deep it lies: `Follett/Winter der Welt`, `NORA/Thea`, or `JÖRG/Dania König/Versprochen`
/// (families sort their music by owner, by genre, by year). Its title is the name of the folder and its
/// author or artist the name of the folder above it; a folder directly below the import folder has no
/// folder above and takes the artist from the tags. The identity of a book is its path below the import
/// folder, so a two-level layout keeps the identity it always had.
///
/// Disc folders (`CD 1`, `Disc 2`, ...) of a folder that has no audio files of its own are part of
/// that folder's book, with the disc number from the name. Hidden folders (`.recyclebin`) and the
/// folders of the NAS systems (`@eaDir`, `lost+found`) are not read. What cannot be read is skipped
/// and told in `problems`, so one bad place does not stop the rest. Folders in `skip` are other
/// import folders that lie inside this one: they are scanned as roots of their own.
fn collect(root: &Path, kind: &str, skip: &[PathBuf], ph: &crate::placeholders::Placeholders, problems: &mut Vec<String>) -> Result<Vec<FoundBook>> {
    let mut books = Vec::new();
    for top in sorted_dirs(root)? {
        if ignored(&top, skip) {
            continue;
        }
        let (books_before, problems_before) = (books.len(), problems.len());
        walk(root, &top, 1, &Scope { kind, skip, ph }, problems, &mut books);
        // A folder that gave nothing and was readable: say why, so that it is not a mystery. A folder
        // with nothing in it at all (what is left of an album that was moved away) is no mystery.
        if books.len() == books_before && problems.len() == problems_before && !is_empty_tree(&top) {
            problems.push(match first_audio_below(&top, MAX_DEPTH as u8) {
                Some(example) => format!(
                    "{}: no albums could be made from it, for example from {}",
                    top.display(),
                    example.strip_prefix(root).unwrap_or(&example).display()
                ),
                None => format!("{}: nothing to import here (no audio files found in it)", top.display()),
            });
        }
    }
    Ok(books)
}

/// Not read at all: other import folders, hidden folders and the folders of NAS systems.
fn ignored(dir: &Path, skip: &[PathBuf]) -> bool {
    let name = file_name(dir);
    name.starts_with('.') || name == "@eaDir" || name == "lost+found" || dir.canonicalize().is_ok_and(|d| skip.contains(&d))
}

/// What a scan carries down the folders: the kind of the import folder, the other import folders inside it, and
/// the words that mark a tag as a placeholder.
struct Scope<'a> {
    kind: &'a str,
    skip: &'a [PathBuf],
    ph: &'a crate::placeholders::Placeholders,
}

fn walk(root: &Path, dir: &Path, depth: usize, scope: &Scope, problems: &mut Vec<String>, books: &mut Vec<FoundBook>) {
    if depth > MAX_DEPTH {
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            problems.push(format!("{}: {}", dir.display(), reason(&e)));
            return;
        }
    };
    let (mut files, mut subdirs): (Vec<PathBuf>, Vec<PathBuf>) = (Vec::new(), Vec::new());
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else { continue };
        if file_type.is_dir() {
            subdirs.push(path);
        } else if path.is_file() && is_audio(&path) {
            files.push(path);
        }
    }
    files.sort();
    subdirs.sort();

    // Disc folders of a folder without audio files of its own belong to it.
    let mut merged: Vec<PathBuf> = Vec::new();
    let mut parts: Vec<(PathBuf, Option<i64>)> = files.iter().map(|f| (f.clone(), None)).collect();
    if files.is_empty() {
        for sub in &subdirs {
            let Some(disc) = disc_number(&file_name(sub)) else { continue };
            let Ok(inside) = fs::read_dir(sub) else { continue };
            let mut disc_files: Vec<PathBuf> = inside.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.is_file() && is_audio(p)).collect();
            disc_files.sort();
            if !disc_files.is_empty() {
                parts.extend(disc_files.into_iter().map(|f| (f, Some(disc))));
                merged.push(sub.clone());
            }
        }
    }

    if !parts.is_empty() {
        match book_of(root, dir, &parts, scope.kind, scope.ph, problems) {
            Ok(book) => books.push(book),
            Err(message) => problems.push(message),
        }
    }
    for sub in subdirs {
        if !merged.contains(&sub) && !ignored(&sub, scope.skip) {
            walk(root, &sub, depth + 1, scope, problems, books);
        }
    }
}

/// `CD 1`, `Disc 02`, `Disk3`, `CD_4`: the number of a disc folder.
fn disc_number(name: &str) -> Option<i64> {
    let lower = name.to_lowercase();
    let rest = ["cd", "disc", "disk", "teil"].iter().find_map(|p| lower.strip_prefix(p))?;
    split_digits(rest.trim_start_matches([' ', '_', '-', '.']))?.0.into()
}

/// Makes disc and track numbers fit what the database demands: a disc is 1 or more, a track number
/// is 1 or more, and no two tracks of a book share both. Tags often fail that (track 0, disc 0, the
/// same number in several files, no numbers at all). Numbers that are fine are kept; if any is not,
/// the tracks are numbered in file order within their disc, so that the album can be imported and
/// plays in the order of its files.
fn normalize_numbers(tracks: &mut [FoundTrack]) {
    for t in tracks.iter_mut() {
        t.disc = t.disc.max(1);
    }
    let mut seen = std::collections::HashSet::new();
    let broken = tracks.iter().any(|t| t.number < 1 || !seen.insert((t.disc, t.number)));
    if !broken {
        return;
    }
    let mut counters: HashMap<i64, i64> = HashMap::new();
    for t in tracks.iter_mut() {
        let n = counters.entry(t.disc).or_insert(0);
        *n += 1;
        t.number = *n;
    }
}

/// The book of a folder and its audio files; `Err` says what is wrong with it.
///
/// A file whose audio cannot be read at all (a tag without any sound, a file that is no audio) is left out and said in
/// `skipped`; the rest of the album is imported, it must not lose its place because of one file. A file that cannot be
/// opened or read (rights, a disk that is gone for the moment) is another matter: the folder is skipped as a whole and
/// nothing changes, so that a short failure cannot remove tracks and their listening progress from the library.
fn book_of(root: &Path, dir: &Path, parts: &[(PathBuf, Option<i64>)], kind: &str, ph: &crate::placeholders::Placeholders, skipped: &mut Vec<String>) -> std::result::Result<FoundBook, String> {
    let relative = dir.strip_prefix(root).unwrap_or(dir);
    let names: Vec<String> = relative.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    let mut tracks = Vec::with_capacity(parts.len());
    let mut readable: Vec<&PathBuf> = Vec::with_capacity(parts.len());
    let mut left_out: Vec<String> = Vec::new();
    for (path, disc) in parts {
        let file = path.strip_prefix(dir).unwrap_or(path).components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
        match read_track(path, &file, readable.len() as i64 + 1, *disc) {
            Ok(track) => {
                tracks.push(track);
                readable.push(path);
            }
            Err(e @ Error::Tags { .. }) => left_out.push(format!("{}: {e} (left out, the rest of the album was imported)", path.display())),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
    }
    if tracks.is_empty() {
        return Err(left_out.into_iter().next().unwrap_or_else(|| format!("{}: no audio file could be read", dir.display())));
    }
    skipped.extend(left_out);
    normalize_numbers(&mut tracks);
    // The recommendation and the narrator sit in the tags; the first track speaks for the book.
    let first_tags = tags::read(readable[0]).ok();
    let recommended_age = first_tags.as_ref().and_then(|t| t.age_recommendation).map(i64::from);
    // The tags come first (project owner, 2026-10-28: the folders are only the filing); the folder
    // names are the fallback for what the tags do not say. A CD number in the album tag is not part of the title.
    let tag_author = first_tags.as_ref().and_then(|t| ph.real(t.album_artist.clone()).or_else(|| ph.real(t.artist.clone())));
    let tag_title = first_tags.as_ref().and_then(|t| t.album.clone()).and_then(|a| ph.real(Some(a))).map(|a| crate::relocate::strip_disc(&a).0).filter(|a| !a.is_empty());
    let author = tag_author.unwrap_or_else(|| match names.len() {
        0 | 1 => "Unknown".to_owned(),
        n => names[n - 2].clone(),
    });
    let title = tag_title.unwrap_or_else(|| names.last().cloned().unwrap_or_default());
    let genre = first_tags.as_ref().and_then(|t| t.genre.clone());
    // The composer tag is the narrator of an audiobook; on music it names the composer, who is nobody to listen to.
    let narrator = first_tags.and_then(|t| t.narrator).filter(|_| kind == "audiobook");
    Ok(FoundBook { kind: kind.to_owned(), folder: names.join("/"), author, title, recommended_age, narrator, genre, tracks })
}

/// The first audio file below `dir`, at most `depth` folders down; `None` if there is none (or it
/// cannot be read).
fn first_audio_below(dir: &Path, depth: u8) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let mut subdirs = Vec::new();
    for entry in fs::read_dir(dir).ok()?.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file() && is_audio(&path) {
            return Some(path);
        }
        if path.is_dir() {
            subdirs.push(path);
        }
    }
    subdirs.sort();
    subdirs.iter().find_map(|d| first_audio_below(d, depth - 1))
}

/// What went wrong with a folder, in a few words for the report.
fn reason(e: &impl std::fmt::Display) -> String {
    let text = e.to_string();
    if text.contains("Permission denied") || text.contains("os error 13") {
        "no permission to read it (the service user needs read access)".to_owned()
    } else if text.contains("No such file") || text.contains("os error 2") {
        "not found".to_owned()
    } else {
        text
    }
}

fn sorted_dirs(dir: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut dirs: Vec<_> = fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    Ok(dirs)
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn is_audio(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

fn read_track(path: &Path, file: &str, ordinal: i64, disc_hint: Option<i64>) -> Result<FoundTrack> {
    let file = file.to_owned();
    // A file that the strict tag reader refuses but that plays is taken with what can be read of it (see `tags::open_tagged`).
    let tagged = tags::open_tagged(path, tags::Forgive::Reading)
        .map_err(|e| Error::Tags { file: file.clone(), message: e.to_string() })?;
    let meta = fs::metadata(path)?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();

    let (disc, number) = parse_disc_track(&stem).unwrap_or_else(|| {
        let disc = disc_hint.or_else(|| tag.and_then(|t| t.disk()).map(i64::from)).unwrap_or(1);
        let number = tag.and_then(|t| t.track()).map(i64::from).unwrap_or(ordinal);
        // The rips store disc and track as one number: 101 = disc 1, track 1.
        (disc, if number >= 100 { number % 100 } else { number })
    });
    let title = tag
        .and_then(|t| t.title().map(|c| c.to_string()))
        .filter(|t| !t.is_empty())
        .unwrap_or(stem);

    Ok(FoundTrack {
        disc,
        number,
        title,
        file,
        duration_ms: tagged.properties().duration().as_millis() as i64,
        size_bytes: meta.len() as i64,
        mtime,
    })
}

/// Extracts `(disc, track)` from names like `Follett_Kinder der Freiheit_CD02_TR05`.
pub(crate) fn parse_disc_track(stem: &str) -> Option<(i64, i64)> {
    let rest = &stem[stem.rfind("_CD")? + 3..];
    let (disc, rest) = split_digits(rest)?;
    let (number, _) = split_digits(rest.strip_prefix("_TR")?)?;
    Some((disc, number))
}

fn split_digits(s: &str) -> Option<(i64, &str)> {
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    if end == 0 {
        return None;
    }
    Some((s[..end].parse().ok()?, &s[end..]))
}

#[cfg(test)]
mod tests {
    use super::{FoundTrack, normalize_numbers, parse_disc_track};

    fn track(disc: i64, number: i64) -> FoundTrack {
        FoundTrack { disc, number, title: String::new(), file: format!("{disc}-{number}.mp3"), duration_ms: 0, size_bytes: 0, mtime: 0 }
    }

    #[test]
    fn good_numbers_stay_and_bad_ones_are_replaced_by_the_file_order_within_each_disc() {
        let mut fine = vec![track(1, 1), track(1, 2), track(2, 1)];
        normalize_numbers(&mut fine);
        assert_eq!(fine.iter().map(|t| (t.disc, t.number)).collect::<Vec<_>>(), [(1, 1), (1, 2), (2, 1)]);

        // Track 0, the same number twice and disc 0 are what the database refuses.
        for bad in [vec![track(1, 0), track(1, 1)], vec![track(1, 3), track(1, 3)], vec![track(0, 1), track(0, 1)]] {
            let mut t = bad;
            normalize_numbers(&mut t);
            assert!(t.iter().all(|x| x.disc >= 1 && x.number >= 1));
            let keys: std::collections::HashSet<_> = t.iter().map(|x| (x.disc, x.number)).collect();
            assert_eq!(keys.len(), t.len());
        }
        let mut mixed = vec![track(1, 5), track(2, 5), track(2, 5), track(1, 9)];
        normalize_numbers(&mut mixed);
        assert_eq!(mixed.iter().map(|t| (t.disc, t.number)).collect::<Vec<_>>(), [(1, 1), (2, 1), (2, 2), (1, 2)]);
    }

    #[test]
    fn parses_ripped_file_names() {
        assert_eq!(parse_disc_track("Follett_Kinder der Freiheit_CD02_TR05"), Some((2, 5)));
        assert_eq!(parse_disc_track("Follett_Kinder der Freiheit_CD12_TR17"), Some((12, 17)));
    }

    #[test]
    fn rejects_other_names() {
        assert_eq!(parse_disc_track("01 - Intro"), None);
        assert_eq!(parse_disc_track("x_CD_TR1"), None);
    }
}
