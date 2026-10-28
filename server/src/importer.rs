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
    pub rule: RatingRule,
    /// Both the roots and the rating rule have been saved by an administrator.
    pub criteria_defined: bool,
    pub unlocked_by: Option<String>,
    pub unlocked_at: Option<i64>,
}

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
        let rule = self.setting("rating_rule").await?.and_then(|v| RatingRule::parse(&v)).unwrap_or(RatingRule::Manual);
        let rule_confirmed = self.setting_rows().await?.iter().any(|s| s.key == "rating_rule" && s.confirmed);
        let unlocked_by = self.setting("import_unlocked_by").await?;
        let unlocked_at = self.setting("import_unlocked_at").await?.and_then(|v| v.parse().ok());
        Ok(ImportState {
            enabled: self.setting("import_enabled").await?.as_deref() == Some("1"),
            criteria_defined: !(roots.is_empty() && music_roots.is_empty()) && rule_confirmed,
            roots,
            music_roots,
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

    /// Saves the release criteria. Every root must be an existing folder that is
    /// not a filesystem root; it is stored in its canonical form. Saving the
    /// criteria is the administrator's confirmation of the rating rule.
    pub async fn save_import_criteria(&self, roots: &[String], rule: RatingRule) -> Result<()> {
        let canonical = Self::canonical_roots(roots)?;
        if canonical.is_empty() {
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
        for root in &state.roots {
            let path = PathBuf::from(root);
            if !path.is_dir() {
                tracing::warn!(root = %root, "import root is not available, skipped");
                continue;
            }
            total.merge(&self.scan(&path).await?);
        }
        for root in &state.music_roots {
            let path = PathBuf::from(root);
            if !path.is_dir() {
                tracing::warn!(root = %root, "music folder is not available, skipped");
                continue;
            }
            total.merge(&self.scan_kind(&path, "music").await?);
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
        self.fill_library_counts(&mut total).await?;
        Ok(total)
    }
}
