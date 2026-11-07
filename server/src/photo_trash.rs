// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The trash of the photo backup (docs/spec-photo-sync.md, bisync with `deletes` = `trash`): a picture that a device of the person deleted
//! goes into the hidden folder `.david-trash` of the picture folder for [`TRASH_KEEP_MS`], the other devices are told (`photo_event`) and
//! ask the person before they delete it too, and the person can restore it until then. After that the file is removed for good.

use std::path::{Path, PathBuf};

use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
use serde::Serialize;

use crate::error::{Error, Result};
use crate::photo_store::{Owner, THUMB_DIR, free_name};
use crate::store::{Store, now_ms};

/// The folder of the trash in the picture folder of a person.
pub const TRASH_DIR: &str = ".david-trash";

/// How long a picture stays in the trash: 30 days.
pub const TRASH_KEEP_MS: i64 = 30 * 24 * 3600 * 1000;

/// How many pictures one call may put into the trash.
pub const TRASH_MAX: usize = 1000;

/// A picture in the trash.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TrashedPhoto {
    pub id: i64,
    pub sha256: String,
    pub name: String,
    pub mime: String,
    pub size: i64,
    pub taken_at: i64,
    pub trashed_at: i64,
    /// When it is removed for good.
    pub purge_at: i64,
}

/// A deletion or a restore that another device did.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PhotoEvent {
    pub seq: i64,
    pub photo: i64,
    pub sha256: String,
    pub kind: String,
}

/// Moves `folder/<from>` to `folder/<to dir>/<name>` (a free name there) and gives the new path relative to `folder`.
fn move_within(folder: &Path, from: &str, to_dir: &Path) -> Result<String> {
    let source = folder.join(from);
    let name = Path::new(from).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "picture".into());
    let dir = folder.join(to_dir);
    std::fs::create_dir_all(&dir).map_err(|e| Error::WriteFailed(format!("{}: {e}", dir.display())))?;
    let target = free_name(&dir, &name);
    std::fs::rename(&source, &target).map_err(|e| Error::WriteFailed(format!("{} -> {}: {e}", source.display(), target.display())))?;
    Ok(target.strip_prefix(folder).unwrap_or(&target).to_string_lossy().into_owned())
}

impl Store {
    async fn photo_event(&self, owner: i64, photo: i64, sha: &str, kind: &str, device: i64) -> Result<()> {
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO photo_event (owner_id, photo_id, sha256, kind, device_id, at) VALUES (?, ?, ?, ?, ?, ?)",
                [Value::from(owner), Value::from(photo), Value::from(sha.to_owned()), Value::from(kind), Value::from((device != 0).then_some(device)), Value::from(now_ms())],
            ))
            .await?;
        Ok(())
    }

    /// Puts the pictures of the owner with these SHA-256 into the trash (those not there yet) and tells the other devices; the ids moved.
    pub async fn trash_photos(&self, o: &Owner, shas: &[String]) -> Result<Vec<i64>> {
        let Some(folder) = o.folder.clone() else { return Ok(Vec::new()) };
        let mut moved = Vec::new();
        for sha in shas {
            let row = self
                .db
                .reader
                .query_one_raw(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT id, file FROM photo WHERE owner_id = ? AND sha256 = ? AND trashed_at IS NULL",
                    [Value::from(o.id), Value::from(sha.clone())],
                ))
                .await?;
            let Some(r) = row else { continue };
            let (id, file): (i64, String) = (r.try_get_by_index(0)?, r.try_get_by_index(1)?);
            let sub = Path::new(TRASH_DIR).join(Path::new(&file).parent().unwrap_or(Path::new("")));
            let now = move_within(&folder, &file, &sub)?;
            self.db
                .writer
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE photo SET file = ?, trashed_at = ? WHERE id = ?",
                    [Value::from(now.clone()), Value::from(now_ms()), Value::from(id)],
                ))
                .await?;
            self.photo_event(o.id, id, sha, "trash", o.device).await?;
            tracing::info!(owner = o.id, photo = id, file = %now, "picture into the trash");
            moved.push(id);
        }
        Ok(moved)
    }

    /// Takes a picture of the owner out of the trash, back to its month (a free name there); false when it is not in the trash.
    pub async fn restore_photo(&self, o: &Owner, id: i64) -> Result<bool> {
        let Some(folder) = o.folder.clone() else { return Ok(false) };
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT file, sha256 FROM photo WHERE owner_id = ? AND id = ? AND trashed_at IS NOT NULL",
                [Value::from(o.id), Value::from(id)],
            ))
            .await?;
        let Some(r) = row else { return Ok(false) };
        let (file, sha): (String, String) = (r.try_get_by_index(0)?, r.try_get_by_index(1)?);
        let inside = Path::new(&file).strip_prefix(TRASH_DIR).map(Path::to_path_buf).unwrap_or_else(|_| PathBuf::from(&file));
        let back = move_within(&folder, &file, inside.parent().unwrap_or(Path::new("")))?;
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE photo SET file = ?, trashed_at = NULL WHERE id = ?", [Value::from(back), Value::from(id)]))
            .await?;
        self.photo_event(o.id, id, &sha, "restore", o.device).await?;
        Ok(true)
    }

    /// The pictures of the owner in the trash, the latest deletion first.
    pub async fn photo_trash(&self, owner: i64) -> Result<Vec<TrashedPhoto>> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, sha256, original_name, mime, size_bytes, taken_at, trashed_at FROM photo WHERE owner_id = ? AND trashed_at IS NOT NULL \
                 ORDER BY trashed_at DESC, id DESC",
                [Value::from(owner)],
            ))
            .await?;
        rows.iter()
            .map(|r| {
                let trashed_at: i64 = r.try_get_by_index(6)?;
                Ok(TrashedPhoto {
                    id: r.try_get_by_index(0)?,
                    sha256: r.try_get_by_index(1)?,
                    name: r.try_get_by_index(2)?,
                    mime: r.try_get_by_index(3)?,
                    size: r.try_get_by_index(4)?,
                    taken_at: r.try_get_by_index(5)?,
                    trashed_at,
                    purge_at: trashed_at + TRASH_KEEP_MS,
                })
            })
            .collect()
    }

    /// Count and bytes of the trash of the owner.
    pub async fn photo_trash_totals(&self, owner: i64) -> Result<(i64, i64)> {
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT count(*), coalesce(sum(size_bytes), 0) FROM photo WHERE owner_id = ? AND trashed_at IS NOT NULL",
                [Value::from(owner)],
            ))
            .await?;
        let Some(r) = row else { return Ok((0, 0)) };
        Ok((r.try_get_by_index(0)?, r.try_get_by_index(1)?))
    }

    /// What the other devices of the owner deleted or restored after `since` (the `seq` of the last one seen).
    pub async fn photo_removals(&self, owner: i64, device: i64, since: i64, limit: u64) -> Result<Vec<PhotoEvent>> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT seq, photo_id, sha256, kind FROM photo_event WHERE owner_id = ? AND seq > ? AND (device_id IS NULL OR device_id != ?) ORDER BY seq LIMIT ?",
                [Value::from(owner), Value::from(since), Value::from(device), Value::from(i64::try_from(limit).unwrap_or(500))],
            ))
            .await?;
        rows.iter()
            .map(|r| Ok(PhotoEvent { seq: r.try_get_by_index(0)?, photo: r.try_get_by_index(1)?, sha256: r.try_get_by_index(2)?, kind: r.try_get_by_index(3)? }))
            .collect()
    }

    /// The `seq` of the newest event of the owner: where a device that has seen nothing yet starts (it has nothing to remove of before).
    pub async fn photo_events_head(&self, owner: i64) -> Result<i64> {
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT coalesce(max(seq), 0) FROM photo_event WHERE owner_id = ?", [Value::from(owner)]))
            .await?;
        Ok(row.map(|r| r.try_get_by_index(0)).transpose()?.unwrap_or(0))
    }

    /// Removes for good what has been in the trash for [`TRASH_KEEP_MS`]: the file, its preview and the row.
    pub async fn purge_photo_trash(&self) -> Result<usize> {
        self.purge_photo_trash_before(now_ms() - TRASH_KEEP_MS).await
    }

    /// Removes for good what went into the trash before `before` (ms).
    pub async fn purge_photo_trash_before(&self, before: i64) -> Result<usize> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, owner_id, file FROM photo WHERE trashed_at IS NOT NULL AND trashed_at < ?",
                [Value::from(before)],
            ))
            .await?;
        let mut removed = 0;
        for r in &rows {
            let (id, owner, file): (i64, i64, String) = (r.try_get_by_index(0)?, r.try_get_by_index(1)?, r.try_get_by_index(2)?);
            if let Some(folder) = self.profile_folders(owner).await?.into_iter().find(|(k, _)| k == "photo").map(|(_, p)| PathBuf::from(p)) {
                let path = folder.join(&file);
                if let Err(e) = std::fs::remove_file(&path)
                    && e.kind() != std::io::ErrorKind::NotFound
                {
                    // Kept in the table: the next round tries again.
                    tracing::warn!(photo = id, path = %path.display(), "the trash could not be emptied: {e}");
                    continue;
                }
                let _ = std::fs::remove_file(folder.join(THUMB_DIR).join(format!("{id}.jpg")));
            }
            self.db.writer.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "DELETE FROM photo WHERE id = ?", [Value::from(id)])).await?;
            removed += 1;
        }
        if removed > 0 {
            tracing::info!(removed, "pictures removed from the trash for good");
        }
        Ok(removed)
    }
}
