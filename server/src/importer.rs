// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The import and the release criteria that unlock it.
//!
//! Imports are locked by default (spec `spec-discovery-profiles-roles.md`, 3c).
//! An administrator defines the criteria, the import roots (the only folders that
//! are ever read) and the rating rule, and then unlocks the import. The criteria
//! can be changed and the import locked again at any time.

use std::path::{Path, PathBuf};

use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
use serde::Serialize;

use crate::error::{Error, Result};
use crate::scan::ScanReport;
use crate::store::{Store, now_ms};

/// How a medium gets its release age when it is imported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RatingRule {
    /// An administrator rates each book. Everything lands unreleased.
    Manual,
    /// A medium that carries an age recommendation is released at that age; one
    /// without stays unreleased.
    Recommendation,
}

impl RatingRule {
    pub fn as_str(self) -> &'static str {
        match self {
            RatingRule::Manual => "manual",
            RatingRule::Recommendation => "recommendation",
        }
    }

    pub fn parse(s: &str) -> Option<RatingRule> {
        match s {
            "manual" => Some(RatingRule::Manual),
            "recommendation" => Some(RatingRule::Recommendation),
            _ => None,
        }
    }
}

/// What the import page shows.
#[derive(Debug, Clone, Serialize)]
pub struct ImportState {
    pub enabled: bool,
    pub roots: Vec<String>,
    /// Folders with music (`Artist/Album/*.mp3`), imported as albums of the kind `music`.
    pub music_roots: Vec<String>,
    /// Folders inside the audiobook folders that are left out (absolute paths): another library
    /// that lies below, a folder of something else, a trash folder. Written as `!/path` lines in
    /// the list of folders.
    pub excludes: Vec<String>,
    /// The same for the music folders.
    pub music_excludes: Vec<String>,
    /// Folders with pictures and with e-books, and what they leave out. They are only kept for now: the import of both follows.
    pub photo_roots: Vec<String>,
    pub photo_excludes: Vec<String>,
    pub ebook_roots: Vec<String>,
    pub ebook_excludes: Vec<String>,
    pub rule: RatingRule,
    /// Both the roots and the rating rule have been saved by an administrator.
    pub criteria_defined: bool,
    pub unlocked_by: Option<String>,
    pub unlocked_at: Option<i64>,
}

/// The quarantine folder when the setting is empty or unusable.
pub const DEFAULT_QUARANTINE_FOLDER: &str = ".quarantine";

impl Store {
    async fn set_setting(&self, key: &str, value: &str, confirmed: bool) -> Result<()> {
        let values: [Value; 3] = [key.to_owned().into(), value.to_owned().into(), i64::from(confirmed).into()];
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO setting (key, value, confirmed) VALUES (?, ?, ?) \
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value, confirmed = excluded.confirmed",
                values,
            ))
            .await?;
        Ok(())
    }

    /// The name of the quarantine folder inside each import folder (a setting; `DEFAULT_QUARANTINE_FOLDER`
    /// if there is none or it is not a plain folder name).
    pub async fn quarantine_folder(&self) -> Result<String> {
        let v = self.setting("quarantine_folder").await?.map(|s| s.trim().to_owned()).unwrap_or_default();
        Ok(if v.is_empty() || v.contains('/') || v.contains('\\') || v == "." || v == ".." { DEFAULT_QUARANTINE_FOLDER.to_owned() } else { v })
    }

    /// The words in the genre tag that make an album an audiobook (a setting, separated by commas), in lower case.
    pub async fn audiobook_genres(&self) -> Result<Vec<String>> {
        Ok(self.setting("audiobook_genres").await?.unwrap_or_default().split(',').map(|w| w.trim().to_lowercase()).filter(|w| !w.is_empty()).collect())
    }

    /// The certificate files and the name of the settings in the pages (empty values are not set).
    pub async fn external_tls(&self) -> Result<crate::tls::External> {
        let get = |key: &'static str| async move { self.setting(key).await.map(|v| v.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())) };
        Ok(crate::tls::External {
            cert: get("tls_cert").await?.map(PathBuf::from),
            key: get("tls_key").await?.map(PathBuf::from),
            host: get("tls_host").await?,
            reload_s: get("tls_reload_s").await?.and_then(|v| v.parse().ok()),
        })
    }

    pub async fn import_state(&self) -> Result<ImportState> {
        let roots: Vec<String> = self
            .setting("import_roots")
            .await?
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default();
        let music_roots: Vec<String> = self
            .setting("music_roots")
            .await?
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default();
        let excludes: Vec<String> = self.setting("import_excludes").await?.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default();
        let music_excludes: Vec<String> = self.setting("music_excludes").await?.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default();
        let list = |key: &'static str| async move { Ok::<Vec<String>, Error>(self.setting(key).await?.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default()) };
        let (photo_roots, photo_excludes) = (list("photo_roots").await?, list("photo_excludes").await?);
        let (ebook_roots, ebook_excludes) = (list("ebook_roots").await?, list("ebook_excludes").await?);
        let rule = self.setting("rating_rule").await?.and_then(|v| RatingRule::parse(&v)).unwrap_or(RatingRule::Manual);
        let rule_confirmed = self.setting_rows().await?.iter().any(|s| s.key == "rating_rule" && s.confirmed);
        let unlocked_by = self.setting("import_unlocked_by").await?;
        let unlocked_at = self.setting("import_unlocked_at").await?.and_then(|v| v.parse().ok());
        Ok(ImportState {
            enabled: self.setting("import_enabled").await?.as_deref() == Some("1"),
            criteria_defined: !(roots.is_empty() && music_roots.is_empty()) && rule_confirmed,
            roots,
            music_roots,
            excludes,
            music_excludes,
            photo_roots,
            photo_excludes,
            ebook_roots,
            ebook_excludes,
            rule,
            unlocked_by,
            unlocked_at,
        })
    }

    /// Checks folder names and turns them into canonical paths: absolute, existing, a folder,
    /// not a filesystem root. Doubles are dropped.
    fn canonical_roots(roots: &[String]) -> Result<Vec<String>> {
        let mut canonical: Vec<String> = Vec::new();
        for root in roots.iter().map(|r| r.trim()).filter(|r| !r.is_empty()) {
            let path = Path::new(root);
            if !path.is_absolute() {
                return Err(Error::InvalidImportRoot(format!("{root} is not an absolute path")));
            }
            let real = std::fs::canonicalize(path)
                .map_err(|_| Error::InvalidImportRoot(format!("{root} does not exist")))?;
            if !real.is_dir() {
                return Err(Error::InvalidImportRoot(format!("{root} is not a folder")));
            }
            if real.parent().is_none() {
                return Err(Error::InvalidImportRoot("the filesystem root cannot be imported".into()));
            }
            let text = real.to_string_lossy().into_owned();
            if !canonical.contains(&text) {
                canonical.push(text);
            }
        }
        Ok(canonical)
    }

    /// The result of the last import (by hand or by itself) and when it ran, if there was one.
    pub async fn last_import(&self) -> Option<(i64, ScanReport)> {
        let json: serde_json::Value = serde_json::from_str(&self.setting("last_import").await.ok()??).ok()?;
        Some((json.get("at")?.as_i64()?, serde_json::from_value(json.get("report")?.clone()).ok()?))
    }

    /// Does the database answer, for reading and for writing? (The health check of the server.)
    pub async fn healthy(&self) -> bool {
        let one = |c: &sea_orm::DatabaseConnection| {
            let c = c.clone();
            async move { c.query_one_raw(Statement::from_string(DbBackend::Sqlite, "SELECT 1")).await.is_ok() }
        };
        one(&self.db.reader).await && one(&self.db.writer).await
    }

    /// Saves the folders that are left out of the import. They need not exist (a mount point that is
    /// not there yet is fine), but they must be absolute paths; the ones that exist are stored in
    /// their canonical form so that a link or a `..` cannot get around them.
    pub async fn save_import_excludes(&self, music: bool, excludes: &[String]) -> Result<()> {
        let mut saved: Vec<String> = Vec::new();
        for e in excludes.iter().map(|e| e.trim()).filter(|e| !e.is_empty()) {
            let path = Path::new(e);
            if !path.is_absolute() {
                return Err(Error::InvalidImportRoot(format!("{e} is not an absolute path")));
            }
            let text = std::fs::canonicalize(path).map_or_else(|_| e.trim_end_matches('/').to_owned(), |p| p.to_string_lossy().into_owned());
            if !saved.contains(&text) {
                saved.push(text);
            }
        }
        let key = if music { "music_excludes" } else { "import_excludes" };
        self.set_setting(key, &serde_json::to_string(&saved).unwrap_or_else(|_| "[]".into()), true).await
    }

    /// Saves the folders with music. May be empty (no music).
    pub async fn save_music_roots(&self, roots: &[String]) -> Result<()> {
        let canonical = Self::canonical_roots(roots)?;
        // A folder is either audiobooks or music: read as both, its albums would change kind on every run.
        let books = self.import_state().await?.roots;
        if let Some(both) = canonical.iter().find(|r| books.contains(r)) {
            return Err(Error::InvalidImportRoot(format!("{both} is already an audiobook folder")));
        }
        self.set_setting("music_roots", &serde_json::to_string(&canonical).unwrap_or_else(|_| "[]".into()), true).await
    }

    /// Saves the folders of one of the kinds that are only kept for now (`photo`, `ebook`) and what they leave out. A folder belongs to one
    /// kind only: audiobooks, music, pictures and e-books must not share a folder.
    pub async fn save_media_roots(&self, kind: &str, roots: &[String], excludes: &[String]) -> Result<()> {
        if !matches!(kind, "photo" | "ebook") {
            return Err(Error::InvalidImportRoot(format!("{kind} is not a kind of medium with folders of its own")));
        }
        let canonical = Self::canonical_roots(roots)?;
        let state = self.import_state().await?;
        let others: Vec<(&str, &Vec<String>)> = vec![("audiobook", &state.roots), ("music", &state.music_roots), ("photo", &state.photo_roots), ("ebook", &state.ebook_roots)];
        for (other, list) in others.into_iter().filter(|(o, _)| *o != kind) {
            if let Some(both) = canonical.iter().find(|r| list.contains(r)) {
                return Err(Error::InvalidImportRoot(format!("{both} is already a folder of {other}")));
            }
        }
        let mut left_out: Vec<String> = Vec::new();
        for e in excludes.iter().map(|e| e.trim()).filter(|e| !e.is_empty()) {
            if !Path::new(e).is_absolute() {
                return Err(Error::InvalidImportRoot(format!("{e} is not an absolute path")));
            }
            let text = std::fs::canonicalize(e).map_or_else(|_| e.trim_end_matches('/').to_owned(), |p| p.to_string_lossy().into_owned());
            if !left_out.contains(&text) {
                left_out.push(text);
            }
        }
        self.set_setting(&format!("{kind}_roots"), &serde_json::to_string(&canonical).unwrap_or_else(|_| "[]".into()), true).await?;
        self.set_setting(&format!("{kind}_excludes"), &serde_json::to_string(&left_out).unwrap_or_else(|_| "[]".into()), true).await
    }

    /// Saves the release criteria. Every root must be an existing folder that is
    /// not a filesystem root; it is stored in its canonical form. Saving the
    /// criteria is the administrator's confirmation of the rating rule.
    pub async fn save_import_criteria(&self, roots: &[String], rule: RatingRule) -> Result<()> {
        self.save_import_criteria_with(roots, rule, false).await
    }

    /// The same; `music_follows` says that music folders are saved with it, so that no audiobook folder at all is fine.
    pub async fn save_import_criteria_with(&self, roots: &[String], rule: RatingRule, music_follows: bool) -> Result<()> {
        let canonical = Self::canonical_roots(roots)?;
        if canonical.is_empty() && !music_follows {
            return Err(Error::ImportCriteriaMissing);
        }
        let music = self.import_state().await?.music_roots;
        if let Some(both) = canonical.iter().find(|r| music.contains(r)) {
            return Err(Error::InvalidImportRoot(format!("{both} is already a music folder")));
        }
        self.set_setting("import_roots", &serde_json::to_string(&canonical).unwrap_or_else(|_| "[]".into()), true).await?;
        self.set_setting("rating_rule", rule.as_str(), true).await?;
        Ok(())
    }

    /// Unlocks the import. Fails with `ImportCriteriaMissing` until the criteria exist.
    pub async fn unlock_import(&self, by: &str) -> Result<()> {
        if !self.import_state().await?.criteria_defined {
            return Err(Error::ImportCriteriaMissing);
        }
        self.set_setting("import_enabled", "1", true).await?;
        self.set_setting("import_unlocked_by", by, true).await?;
        self.set_setting("import_unlocked_at", &now_ms().to_string(), true).await?;
        Ok(())
    }

    pub async fn lock_import(&self) -> Result<()> {
        self.set_setting("import_enabled", "0", true).await
    }

    /// Imports every root. Fails with `ImportLocked` while the import is locked.
    /// A root that has gone missing since it was saved is skipped and logged, so
    /// one unmounted disk does not stop the others.
    /// Counts what the library holds: audiobooks and their authors, albums and their artists, tracks.
    async fn fill_library_counts(&self, report: &mut ScanReport) -> Result<()> {
        let row = self
            .db
            .writer
            .query_one_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT \
                   (SELECT count(*) FROM book WHERE kind = 'audiobook'), \
                   (SELECT count(DISTINCT author_id) FROM book WHERE kind = 'audiobook'), \
                   (SELECT count(*) FROM book WHERE kind = 'music'), \
                   (SELECT count(DISTINCT author_id) FROM book WHERE kind = 'music'), \
                   (SELECT count(*) FROM track)",
            ))
            .await?
            .ok_or_else(|| Error::WriteFailed("count returned no row".into()))?;
        let n = |i: usize| -> Result<usize> { Ok(row.try_get_by_index::<i64>(i)? as usize) };
        report.audiobooks = n(0)?;
        report.authors = n(1)?;
        report.albums = n(2)?;
        report.artists = n(3)?;
        report.tracks_total = n(4)?;
        Ok(())
    }

    /// Brings the library in line with the files a little later, after something changed them (tags,
    /// covers, moved folders): one import in the background, however many changes come in a row.
    /// Without it the lists keep the old narrator, title and author until the next import.
    pub fn rescan_soon(&self) {
        use std::sync::atomic::{AtomicBool, Ordering};
        static PENDING: AtomicBool = AtomicBool::new(false);
        if PENDING.swap(true, Ordering::SeqCst) {
            return;
        }
        let store = self.clone();
        tokio::spawn(async move {
            // The changes of one session arrive in bursts; wait until it is quiet.
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            PENDING.store(false, Ordering::SeqCst);
            match store.run_import().await {
                Ok(report) => tracing::info!(books = report.books, tracks_updated = report.tracks_updated, "library brought in line after a change"),
                Err(Error::ImportLocked) => {}
                Err(e) => tracing::warn!(error = %e, "the rescan after a change failed"),
            }
        });
    }

    pub async fn run_import(&self) -> Result<ScanReport> {
        let state = self.import_state().await?;
        if !state.enabled {
            return Err(Error::ImportLocked);
        }
        // Books created in this run are the ones with a higher id than any before.
        // A timestamp would not do: two runs can fall into the same millisecond.
        let newest_before: i64 = self
            .db
            .writer
            .query_one_raw(Statement::from_string(DbBackend::Sqlite, "SELECT coalesce(max(id), 0) FROM book"))
            .await?
            .map(|r| r.try_get_by_index(0))
            .transpose()?
            .unwrap_or(0);
        let mut total = ScanReport::default();
        // The quarantine of each import folder is not part of the library, whatever it is called.
        let quarantine = self.quarantine_folder().await?;
        // Every folder, with the kind of what is in it. A folder that lies inside another one is left
        // out of the scan of that one: it is scanned as a root of its own.
        let roots: Vec<(PathBuf, &'static str)> = state
            .roots
            .iter()
            .map(|r| (PathBuf::from(r), "audiobook"))
            .chain(state.music_roots.iter().map(|r| (PathBuf::from(r), "music")))
            .collect();
        for (path, kind) in &roots {
            if !path.is_dir() {
                tracing::warn!(root = %path.display(), "import root is not available, skipped");
                total.problems.push(format!("{}: not available", path.display()));
                continue;
            }
            // Left out: the folders the administrator named, and other import folders inside this one.
            let inside: Vec<PathBuf> = roots
                .iter()
                .filter(|(other, _)| other != path && other.starts_with(path))
                .map(|(other, _)| other.clone())
                .chain(if *kind == "music" { &state.music_excludes } else { &state.excludes }.iter().map(PathBuf::from))
                .chain(std::iter::once(path.join(&quarantine)))
                .map(|p| p.canonicalize().unwrap_or(p))
                .collect();
            // A folder that cannot be read must not stop the others.
            match self.scan_skipping(path, kind, &inside).await {
                Ok(report) => total.merge(&report),
                Err(e) => {
                    tracing::warn!(root = %path.display(), error = %e, "this import folder could not be read");
                    total.problems.push(format!("{}: {e}", path.display()));
                }
            }
        }
        if state.rule == RatingRule::Recommendation {
            // Only books that arrived in this run: a book an administrator withdrew
            // must not come back by itself.
            let values: [Value; 1] = [newest_before.into()];
            let res = self
                .db
                .writer
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE book SET released = 1, min_age = recommended_age \
                     WHERE released = 0 AND recommended_age IS NOT NULL AND id > ?",
                    values,
                ))
                .await?;
            total.books_released = res.rows_affected() as usize;
        }
        // Tidy up: the albums whose files are gone leave the library (the scan above only adds). A place that cannot be read counts as "there"
        // (see `prune.rs`), and an import folder that is missing or empty stops the tidying of its kind, so a small problem of the scan need not stop it.
        let pruned = self.prune_missing(false, false).await?;
        total.books_removed = pruned.removed.len();
        total.problems.extend(pruned.problems);
        // Everything that came in or changed is checked against the blacklist (a failure is logged only).
        self.blacklist_check_quietly(crate::blacklist::Scope::All).await;
        self.fill_library_counts(&mut total).await?;
        // The last result is kept, so the import page can show it later: the problems of a run (a folder
        // without permission) are then readable without the server log.
        if let Ok(json) = serde_json::to_string(&serde_json::json!({ "at": now_ms(), "report": total })) {
            let _ = self.set_setting("last_import", &json, true).await;
        }
        Ok(total)
    }
}
