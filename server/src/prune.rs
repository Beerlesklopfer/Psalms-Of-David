// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tidying up the library (project owner, 2026-10-30: "the import never cleans up, the reindexing seems to be additive only"; "let us solve it with a
//! systemd timer that does it periodically"). The import adds albums and drops tracks that are missing from an album it still finds, but it never
//! removed an album whose folder is gone. This does: an album none of whose files is on the disk any more is taken out of the library, with what
//! hangs on it (progress, labels, releases). The files are not touched, they are not there.
//!
//! The danger is a disk that is not mounted: the import folder is then an empty folder and every album looks gone. So an import folder that is
//! missing or empty stops the tidying of its kind, and so does a run that would remove more than a third of the albums of a kind (more than five),
//! unless `force` says that this is meant. `david prune` is what the timer (`david-prune.timer`) runs; the tool `remove_missing_albums` does it by hand.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::Result;
use crate::store::Store;

/// One album that is gone from the disk.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Gone {
    pub id: i64,
    pub kind: String,
    pub folder: String,
    pub title: String,
    pub author: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PruneReport {
    /// The albums that were taken out (or would be, in a dry run).
    pub removed: Vec<Gone>,
    /// The albums that are gone but were left, because too many were (see above) or an import folder was not available.
    pub held_back: Vec<Gone>,
    /// Why something was left.
    pub problems: Vec<String>,
}

/// The albums of a kind whose files are all gone, from `books` (`kind`, `folder`, id ...) and the files of their tracks, as seen from `roots`.
fn gone_among(books: &[Gone], files: &HashMap<i64, Vec<String>>, roots: &[PathBuf]) -> Vec<Gone> {
    books
        .iter()
        .filter(|b| {
            let Some(tracks) = files.get(&b.id).filter(|t| !t.is_empty()) else { return false };
            !roots.iter().any(|r| tracks.iter().any(|f| exists_or_unknown(&r.join(&b.folder).join(f))))
        })
        .cloned()
        .collect()
}

/// Is the file there? A place that cannot be read (no permission, an I/O error) counts as "there": only a file that is really not found is gone.
fn exists_or_unknown(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(m) => m.is_file(),
        Err(e) => e.kind() != std::io::ErrorKind::NotFound,
    }
}

/// Is the import folder there and does it hold anything (an unmounted disk is an empty folder)?
fn available(root: &Path) -> bool {
    root.is_dir() && std::fs::read_dir(root).map(|mut d| d.next().is_some()).unwrap_or(false)
}

impl Store {
    /// Removes the albums whose files are gone. `dry_run` only says which. `force` lifts the stop for a run that would remove many albums
    /// (not the stop for an import folder that is not available).
    pub async fn prune_missing(&self, dry_run: bool, force: bool) -> Result<PruneReport> {
        use sea_orm::{ConnectionTrait, DbBackend, Statement};
        let state = self.import_state().await?;
        let mut report = PruneReport::default();
        let rows = self.admin_book_rows().await?;
        let track_rows = self.db.reader.query_all_raw(Statement::from_string(DbBackend::Sqlite, "SELECT book_id, file FROM track")).await?;
        let mut files: HashMap<i64, Vec<String>> = HashMap::new();
        for r in track_rows {
            files.entry(r.try_get_by_index(0)?).or_default().push(r.try_get_by_index(1)?);
        }
        for (kind, folders) in [("audiobook", &state.roots), ("music", &state.music_roots)] {
            let books: Vec<Gone> = rows.iter().filter(|r| r.kind == kind).map(|r| Gone { id: r.id, kind: r.kind.clone(), folder: r.folder.clone(), title: r.title.clone(), author: r.author.clone() }).collect();
            if books.is_empty() {
                continue;
            }
            let roots: Vec<PathBuf> = folders.iter().map(PathBuf::from).collect();
            if let Some(missing) = roots.iter().find(|r| !available(r)) {
                report.problems.push(format!("{}: the import folder is missing or empty, nothing of the {kind} albums is removed (is the disk mounted?)", missing.display()));
                continue;
            }
            let (roots, books, files) = (roots.clone(), books.clone(), files.clone());
            let gone = tokio::task::spawn_blocking(move || gone_among(&books, &files, &roots)).await.map_err(|e| crate::error::Error::WriteFailed(e.to_string()))?;
            let total = rows.iter().filter(|r| r.kind == kind).count();
            if !force && gone.len() > 5 && gone.len() * 3 > total {
                report.problems.push(format!("{} of {total} {kind} albums are gone from the disk: that is too many to remove by itself (a disk that is not mounted?); look at them and run it again with force", gone.len()));
                report.held_back.extend(gone);
                continue;
            }
            for g in gone {
                if !dry_run {
                    self.delete_book(g.id).await?;
                    self.log_tag_change("prune", g.id, "", "Removed", &g.folder, "").await?;
                    tracing::info!(album = g.id, folder = %g.folder, title = %g.title, "album removed: its files are gone");
                }
                report.removed.push(g);
            }
        }
        Ok(report)
    }
}
