// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! What the database knows about a book, for the tab "Filing" of its window (project owner, 2026-10-30: "the complete place; the metadata of the
//! database here too, the data the triggers fill in"): when it was added, released, made and last changed, and by whom (the audit fields
//! `created_at`, `created_by`, `updated_at`, `updated_by` that the triggers keep), and what its files weigh.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::Serialize;

use crate::error::Result;
use crate::store::Store;

#[derive(Debug, Clone, Serialize, Default)]
pub struct BookMeta {
    /// When the import first saw the book, and when it was released (milliseconds since 1970).
    pub added_at: i64,
    pub released_at: Option<i64>,
    /// The audit fields of the row of the book (filled by the triggers).
    pub created_at: i64,
    pub created_by: Option<String>,
    pub updated_at: i64,
    pub updated_by: Option<String>,
    /// The sum of the sizes of the files, in bytes, and the newest change of a track row.
    pub files_bytes: i64,
    pub tracks_updated_at: Option<i64>,
    pub genre_tag: Option<String>,
}

impl Store {
    pub async fn book_meta(&self, id: i64) -> Result<Option<BookMeta>> {
        let Some(row) = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT added_at, released_at, created_at, created_by, updated_at, updated_by, genre_tag FROM book WHERE id = ?",
                [id.into()],
            ))
            .await?
        else {
            return Ok(None);
        };
        let tracks = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT coalesce(SUM(size_bytes), 0), MAX(updated_at) FROM track WHERE book_id = ?", [id.into()]))
            .await?;
        let (files_bytes, tracks_updated_at) = match tracks {
            Some(t) => (t.try_get_by_index(0)?, t.try_get_by_index(1)?),
            None => (0, None),
        };
        Ok(Some(BookMeta {
            added_at: row.try_get_by_index::<Option<i64>>(0)?.unwrap_or(0),
            released_at: row.try_get_by_index(1)?,
            created_at: row.try_get_by_index::<Option<i64>>(2)?.unwrap_or(0),
            created_by: row.try_get_by_index(3)?,
            updated_at: row.try_get_by_index::<Option<i64>>(4)?.unwrap_or(0),
            updated_by: row.try_get_by_index(5)?,
            files_bytes,
            tracks_updated_at,
            genre_tag: row.try_get_by_index(6)?,
        }))
    }
}
