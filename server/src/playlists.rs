// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Playlists (project owner, 2026-10-29; see `docs/spec-playlists.md`). A playlist belongs to one profile and lives as
//! rows (`playlist`, `playlist_item`, schema 0035); the server never writes playlist files. Every function takes the
//! profile that owns the list, so a list of somebody else is simply not found. What a profile may hear is decided
//! by the same rule as the lists (`VISIBLE`) for the owner: an entry the owner may not see is kept but reported as not
//! available, and one whose track left the library is gone with it (`ON DELETE CASCADE`).

use sea_orm::{ConnectionTrait, DbBackend, FromQueryResult, Statement, TransactionTrait, Value};
use serde::Serialize;

use crate::store::{Store, VISIBLE};

/// Longest name of a playlist, in characters.
pub const NAME_MAX: usize = 100;
/// Most playlists of one profile.
pub const PER_PROFILE_MAX: i64 = 100;
/// Most entries of one playlist.
pub const ITEMS_MAX: i64 = 5000;

#[derive(Debug, thiserror::Error)]
pub enum PlaylistError {
    #[error("no such playlist")]
    NotFound,
    #[error("the profile has a playlist of this name already")]
    NameTaken,
    #[error("a name has 1 to {NAME_MAX} characters")]
    InvalidName,
    #[error("a profile may have at most {PER_PROFILE_MAX} playlists")]
    TooManyPlaylists,
    #[error("a playlist holds at most {ITEMS_MAX} entries")]
    Full,
    #[error("a track does not exist or the profile may not see it")]
    UnknownPart,
    #[error("the album does not exist or the profile may not see it")]
    UnknownBook,
    #[error("the position must be between 1 and the number of entries plus one")]
    InvalidPosition,
    #[error("the order must name every entry of the playlist exactly once")]
    InvalidOrder,
    #[error("database: {0}")]
    Db(String),
}

impl From<sea_orm::DbErr> for PlaylistError {
    fn from(e: sea_orm::DbErr) -> Self {
        PlaylistError::Db(e.to_string())
    }
}

impl From<crate::Error> for PlaylistError {
    fn from(e: crate::Error) -> Self {
        PlaylistError::Db(e.to_string())
    }
}

type Result<T> = std::result::Result<T, PlaylistError>;

/// One line of a list of playlists.
#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct PlaylistSummary {
    pub id: i64,
    pub name: String,
    pub profile_id: i64,
    pub profile_name: String,
    /// Entries the owner may hear now.
    pub items: i64,
    /// Their duration in seconds.
    pub seconds: i64,
    pub updated_at: i64,
}

/// One entry of a playlist with what the library knows of its track.
#[derive(Debug, Clone, FromQueryResult)]
pub struct Entry {
    pub item_id: i64,
    pub position: i64,
    /// May the owner hear it now? If not, the app skips it and the API says nothing about the track.
    pub available: bool,
    pub track_id: i64,
    pub disc: i64,
    pub number: i64,
    pub title: String,
    pub duration_ms: i64,
    pub plays: i64,
    pub last_played_at: Option<i64>,
    pub book_id: i64,
    pub book_title: String,
    pub author: String,
}

/// A playlist with its entries in order.
#[derive(Debug, Clone)]
pub struct PlaylistView {
    pub id: i64,
    pub name: String,
    pub profile_id: i64,
    pub profile_name: String,
    pub entries: Vec<Entry>,
}

/// Checks a name: trimmed, 1 to `NAME_MAX` characters, no control characters.
pub fn clean_name(name: &str) -> Result<String> {
    let name = name.trim();
    let n = name.chars().count();
    if n == 0 || n > NAME_MAX || name.chars().any(char::is_control) {
        return Err(PlaylistError::InvalidName);
    }
    Ok(name.to_owned())
}

fn sql(text: impl Into<String>, values: impl IntoIterator<Item = Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, text, values)
}

fn int(row: &sea_orm::QueryResult, i: usize) -> Result<i64> {
    Ok(row.try_get_by_index(i)?)
}

/// Numbers the entries of a list 1..n in their present order, and marks the list as changed.
async fn renumber(txn: &impl ConnectionTrait, playlist: i64) -> Result<()> {
    let rows = txn.query_all_raw(sql("SELECT id, position FROM playlist_item WHERE playlist_id = ? ORDER BY position, id", [playlist.into()])).await?;
    for (n, row) in rows.iter().enumerate() {
        let (id, pos) = (int(row, 0)?, int(row, 1)?);
        if pos != n as i64 + 1 {
            txn.execute_raw(sql("UPDATE playlist_item SET position = ? WHERE id = ?", [(n as i64 + 1).into(), id.into()])).await?;
        }
    }
    // An update that changes nothing makes the trigger stamp the time and the actor.
    txn.execute_raw(sql("UPDATE playlist SET updated_at = updated_at WHERE id = ?", [playlist.into()])).await?;
    Ok(())
}

async fn owned(txn: &impl ConnectionTrait, playlist: i64, profile: i64) -> Result<()> {
    let row = txn.query_one_raw(sql("SELECT 1 FROM playlist WHERE id = ? AND profile_id = ?", [playlist.into(), profile.into()])).await?;
    row.map(|_| ()).ok_or(PlaylistError::NotFound)
}

async fn count_items(txn: &impl ConnectionTrait, playlist: i64) -> Result<i64> {
    let row = txn.query_one_raw(sql("SELECT count(*) FROM playlist_item WHERE playlist_id = ?", [playlist.into()])).await?;
    row.map(|r| int(&r, 0)).transpose().map(Option::unwrap_or_default)
}

impl Store {
    /// The playlists of one profile, or of all (`None`), ordered by profile and name.
    pub async fn playlist_summaries(&self, profile: Option<i64>) -> Result<Vec<PlaylistSummary>> {
        let text = format!(
            "SELECT pl.id AS id, pl.name AS name, pl.profile_id AS profile_id, p.name AS profile_name, pl.updated_at AS updated_at, \
                    coalesce(sum(CASE WHEN b.id IS NOT NULL AND {VISIBLE} THEN 1 END), 0) AS items, \
                    coalesce(sum(CASE WHEN b.id IS NOT NULL AND {VISIBLE} THEN t.duration_ms END), 0) / 1000 AS seconds \
             FROM playlist pl JOIN profile p ON p.id = pl.profile_id \
             LEFT JOIN playlist_item i ON i.playlist_id = pl.id LEFT JOIN track t ON t.id = i.track_id LEFT JOIN book b ON b.id = t.book_id \
             WHERE (?1 IS NULL OR pl.profile_id = ?1) GROUP BY pl.id ORDER BY p.name COLLATE NOCASE, pl.name COLLATE NOCASE"
        );
        let value: Value = profile.into();
        Ok(PlaylistSummary::find_by_statement(sql(text, [value])).all(&self.db.reader).await?)
    }

    /// A playlist of the profile with its entries; `None` if there is none (or it belongs to somebody else).
    pub async fn playlist_view(&self, id: i64, profile: i64) -> Result<Option<PlaylistView>> {
        let head = self
            .db
            .reader
            .query_one_raw(sql("SELECT pl.name, p.name FROM playlist pl JOIN profile p ON p.id = pl.profile_id WHERE pl.id = ? AND pl.profile_id = ?", [id.into(), profile.into()]))
            .await?;
        let Some(head) = head else { return Ok(None) };
        let text = format!(
            "SELECT i.id AS item_id, i.position AS position, CASE WHEN {VISIBLE} THEN 1 ELSE 0 END AS available, t.id AS track_id, t.disc AS disc, \
                    t.number AS number, t.title AS title, t.duration_ms AS duration_ms, coalesce(tp.plays, 0) AS plays, tp.last_played_at AS last_played_at, \
                    b.id AS book_id, b.title AS book_title, a.name AS author \
             FROM playlist_item i JOIN track t ON t.id = i.track_id JOIN book b ON b.id = t.book_id JOIN author a ON a.id = b.author_id \
             JOIN profile p ON p.id = ? LEFT JOIN track_play tp ON tp.track_id = t.id AND tp.profile_id = p.id \
             WHERE i.playlist_id = ? ORDER BY i.position, i.id"
        );
        let mut entries = Entry::find_by_statement(sql(text, [profile.into(), id.into()])).all(&self.db.reader).await?;
        // Entries that left with a deleted track leave a gap in the stored numbers; what is told is always 1..n.
        for (n, e) in entries.iter_mut().enumerate() {
            e.position = n as i64 + 1;
        }
        Ok(Some(PlaylistView { id, name: head.try_get_by_index(0)?, profile_id: profile, profile_name: head.try_get_by_index(1)?, entries }))
    }

    /// A short fingerprint of the playlists of one profile (names, entries, order). It changes when the profile's lists change
    /// and only then, so the devices of the other profiles are not made to reload.
    pub async fn playlists_version(&self, profile: i64) -> Result<String> {
        use sha2::{Digest, Sha256};
        let queries = [
            "SELECT coalesce(group_concat(id || ':' || name || ':' || updated_at, '|'), '') FROM (SELECT * FROM playlist WHERE profile_id = ? ORDER BY id)",
            "SELECT coalesce(group_concat(i.id || ':' || i.playlist_id || ':' || i.position || ':' || i.track_id, '|'), '') \
             FROM (SELECT i.* FROM playlist_item i JOIN playlist pl ON pl.id = i.playlist_id WHERE pl.profile_id = ? ORDER BY i.playlist_id, i.position, i.id) i",
        ];
        let mut hasher = Sha256::new();
        for q in queries {
            let row = self.db.reader.query_one_raw(sql(q, [profile.into()])).await?;
            let text: String = row.map(|r| r.try_get_by_index(0)).transpose()?.unwrap_or_default();
            hasher.update(text.as_bytes());
            hasher.update([0u8]);
        }
        Ok(hasher.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect())
    }

    /// Makes a playlist for the profile and returns its id.
    pub async fn create_playlist(&self, profile: i64, name: &str) -> Result<i64> {
        let name = clean_name(name)?;
        let txn = self.db.writer.begin().await?;
        let taken = txn.query_one_raw(sql("SELECT 1 FROM playlist WHERE profile_id = ? AND name = ? COLLATE NOCASE", [profile.into(), name.clone().into()])).await?;
        if taken.is_some() {
            return Err(PlaylistError::NameTaken);
        }
        let n = txn.query_one_raw(sql("SELECT count(*) FROM playlist WHERE profile_id = ?", [profile.into()])).await?.map(|r| int(&r, 0)).transpose()?.unwrap_or(0);
        if n >= PER_PROFILE_MAX {
            return Err(PlaylistError::TooManyPlaylists);
        }
        let res = txn.execute_raw(sql("INSERT INTO playlist (profile_id, name) VALUES (?, ?)", [profile.into(), name.into()])).await?;
        txn.commit().await?;
        Ok(res.last_insert_id() as i64)
    }

    pub async fn rename_playlist(&self, id: i64, profile: i64, name: &str) -> Result<()> {
        let name = clean_name(name)?;
        let txn = self.db.writer.begin().await?;
        owned(&txn, id, profile).await?;
        let taken = txn.query_one_raw(sql("SELECT 1 FROM playlist WHERE profile_id = ? AND name = ? COLLATE NOCASE AND id <> ?", [profile.into(), name.clone().into(), id.into()])).await?;
        if taken.is_some() {
            return Err(PlaylistError::NameTaken);
        }
        txn.execute_raw(sql("UPDATE playlist SET name = ? WHERE id = ?", [name.into(), id.into()])).await?;
        txn.commit().await?;
        Ok(())
    }

    pub async fn delete_playlist(&self, id: i64, profile: i64) -> Result<()> {
        let res = self.db.writer.execute_raw(sql("DELETE FROM playlist WHERE id = ? AND profile_id = ?", [id.into(), profile.into()])).await?;
        if res.rows_affected() == 0 {
            return Err(PlaylistError::NotFound);
        }
        Ok(())
    }

    /// Appends tracks (in the order given, duplicates allowed) or inserts them at `at` (1-based). Either all are added or none.
    pub async fn add_playlist_parts(&self, id: i64, profile: i64, parts: &[i64], at: Option<i64>) -> Result<()> {
        if parts.len() as i64 > ITEMS_MAX {
            return Err(PlaylistError::Full);
        }
        let txn = self.db.writer.begin().await?;
        owned(&txn, id, profile).await?;
        for part in parts {
            let ok = txn
                .query_one_raw(sql(format!("SELECT 1 FROM track t JOIN book b ON b.id = t.book_id JOIN profile p ON p.id = ? WHERE t.id = ? AND {VISIBLE}"), [profile.into(), (*part).into()]))
                .await?;
            if ok.is_none() {
                return Err(PlaylistError::UnknownPart);
            }
        }
        insert_parts(&txn, id, parts, at).await?;
        txn.commit().await?;
        Ok(())
    }

    /// Appends (or inserts at `at`) all tracks of an album in album order (disc, number).
    pub async fn add_playlist_book(&self, id: i64, profile: i64, book: i64, at: Option<i64>) -> Result<()> {
        let txn = self.db.writer.begin().await?;
        owned(&txn, id, profile).await?;
        let seen = txn.query_one_raw(sql(format!("SELECT 1 FROM book b JOIN profile p ON p.id = ? WHERE b.id = ? AND {VISIBLE}"), [profile.into(), book.into()])).await?;
        if seen.is_none() {
            return Err(PlaylistError::UnknownBook);
        }
        let rows = txn.query_all_raw(sql("SELECT id FROM track WHERE book_id = ? ORDER BY disc, number", [book.into()])).await?;
        let parts = rows.iter().map(|r| int(r, 0)).collect::<Result<Vec<_>>>()?;
        insert_parts(&txn, id, &parts, at).await?;
        txn.commit().await?;
        Ok(())
    }

    /// Removes one entry; the others move up.
    pub async fn remove_playlist_item(&self, id: i64, profile: i64, item: i64) -> Result<()> {
        let txn = self.db.writer.begin().await?;
        owned(&txn, id, profile).await?;
        let res = txn.execute_raw(sql("DELETE FROM playlist_item WHERE id = ? AND playlist_id = ?", [item.into(), id.into()])).await?;
        if res.rows_affected() == 0 {
            return Err(PlaylistError::NotFound);
        }
        renumber(&txn, id).await?;
        txn.commit().await?;
        Ok(())
    }

    /// Removes all entries.
    pub async fn clear_playlist(&self, id: i64, profile: i64) -> Result<()> {
        let txn = self.db.writer.begin().await?;
        owned(&txn, id, profile).await?;
        txn.execute_raw(sql("DELETE FROM playlist_item WHERE playlist_id = ?", [id.into()])).await?;
        renumber(&txn, id).await?;
        txn.commit().await?;
        Ok(())
    }

    /// Puts the entries in the given order, which must name each entry of the list exactly once.
    pub async fn reorder_playlist(&self, id: i64, profile: i64, order: &[i64]) -> Result<()> {
        let txn = self.db.writer.begin().await?;
        owned(&txn, id, profile).await?;
        let rows = txn.query_all_raw(sql("SELECT id FROM playlist_item WHERE playlist_id = ?", [id.into()])).await?;
        let mut have = rows.iter().map(|r| int(r, 0)).collect::<Result<Vec<_>>>()?;
        let mut want = order.to_vec();
        have.sort_unstable();
        want.sort_unstable();
        if have != want {
            return Err(PlaylistError::InvalidOrder);
        }
        for (n, item) in order.iter().enumerate() {
            txn.execute_raw(sql("UPDATE playlist_item SET position = ? WHERE id = ?", [(n as i64 + 1).into(), (*item).into()])).await?;
        }
        renumber(&txn, id).await?;
        txn.commit().await?;
        Ok(())
    }
}

/// Inserts the tracks as new entries at `at` (default: the end) and shifts the entries behind them.
async fn insert_parts(txn: &impl ConnectionTrait, id: i64, parts: &[i64], at: Option<i64>) -> Result<()> {
    // Close the gaps that a deleted track may have left, so that `at` counts what the caller saw.
    renumber(txn, id).await?;
    let have = count_items(txn, id).await?;
    if have + parts.len() as i64 > ITEMS_MAX {
        return Err(PlaylistError::Full);
    }
    let at = match at {
        None => have + 1,
        Some(p) if (1..=have + 1).contains(&p) => p,
        Some(_) => return Err(PlaylistError::InvalidPosition),
    };
    if parts.is_empty() {
        return Ok(());
    }
    txn.execute_raw(sql("UPDATE playlist_item SET position = position + ? WHERE playlist_id = ? AND position >= ?", [(parts.len() as i64).into(), id.into(), at.into()])).await?;
    for (n, part) in parts.iter().enumerate() {
        txn.execute_raw(sql("INSERT INTO playlist_item (playlist_id, position, track_id) VALUES (?, ?, ?)", [id.into(), (at + n as i64).into(), (*part).into()])).await?;
    }
    renumber(txn, id).await
}
