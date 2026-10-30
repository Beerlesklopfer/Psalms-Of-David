// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Statistics of listening (project owner, 2026-10-28): which tracks a profile has heard to the end, how often, and when
//! last. The apps show a green check mark on a track that was played, and for a book the number of *complete* plays and the
//! day it was last played.
//!
//! The server finds out from the position reports (`PUT /api/v1/progress`), so every app is counted the same way. A track
//! counts as played when a report comes within `END_MARGIN_MS` of its end, or when the app says `finished` in the last fifth of it
//! (the app sends it at every end of a track, not only at the end of the list). It counts once per pass: it is counted again only
//! after the position went back before the middle of the track. A book was played completely as often as its least played
//! track (a track that was never played makes that 0).

use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::Serialize;

use crate::error::Result;
use crate::store::{now_ms, Store, VISIBLE};

/// How close to the end a report must be for the track to count as heard to the end.
pub const END_MARGIN_MS: i64 = 3_000;

/// What a profile has heard of one track.
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
pub struct TrackPlays {
    pub plays: i64,
    /// Unix milliseconds of the last time it was heard to the end.
    pub last_played_at: Option<i64>,
}

/// What a profile has heard of a book.
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
pub struct BookPlays {
    /// How often the whole book was heard.
    pub plays: i64,
    /// When something of it was last heard to the end.
    pub last_played_at: Option<i64>,
}

/// The book's statistics from those of its tracks.
pub fn book_plays(tracks: &[TrackPlays]) -> BookPlays {
    BookPlays { plays: tracks.iter().map(|t| t.plays).min().unwrap_or(0), last_played_at: tracks.iter().filter_map(|t| t.last_played_at).max() }
}

impl Store {
    /// Takes note of a position report: counts the track if it reached its end, re-arms it if the listener went back.
    /// True if this report counted a play.
    /// `finished` is the app's word that the track played to its end (the player reached the end of the file): it counts although the report is
    /// more than `END_MARGIN_MS` before the length in the tags (variable bit rate files are often off by seconds), if the position is in the last fifth.
    pub async fn note_part_progress(&self, profile_id: i64, track_id: i64, position_ms: i64, duration_ms: i64, finished: bool) -> Result<bool> {
        let run = |sql: &'static str, values: Vec<sea_orm::Value>| Statement::from_sql_and_values(DbBackend::Sqlite, sql, values);
        if duration_ms > 0 && (position_ms + END_MARGIN_MS >= duration_ms || (finished && position_ms * 5 >= duration_ms * 4)) {
            self.db.writer.execute_raw(run("INSERT OR IGNORE INTO track_play (profile_id, track_id) VALUES (?, ?)", vec![profile_id.into(), track_id.into()])).await?;
            let counted = self
                .db
                .writer
                .execute_raw(run(
                    "UPDATE track_play SET plays = plays + 1, last_played_at = ?, armed = 0 WHERE profile_id = ? AND track_id = ? AND armed = 1",
                    vec![now_ms().into(), profile_id.into(), track_id.into()],
                ))
                .await?;
            return Ok(counted.rows_affected() > 0);
        }
        if duration_ms > 0 && position_ms * 2 < duration_ms {
            self.db
                .writer
                .execute_raw(run("UPDATE track_play SET armed = 1 WHERE profile_id = ? AND track_id = ? AND armed = 0", vec![profile_id.into(), track_id.into()]))
                .await?;
        }
        Ok(false)
    }

    /// What a profile has heard of each track of a book, by track id (a track never heard to the end has zero plays).
    pub async fn part_plays(&self, profile_id: i64, book_id: i64) -> Result<HashMap<i64, TrackPlays>> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT t.id, coalesce(p.plays, 0), p.last_played_at FROM track t LEFT JOIN track_play p ON p.track_id = t.id AND p.profile_id = ? WHERE t.book_id = ?",
                [profile_id.into(), book_id.into()],
            ))
            .await?;
        rows.into_iter().map(|r| Ok((r.try_get_by_index(0)?, TrackPlays { plays: r.try_get_by_index(1)?, last_played_at: r.try_get_by_index(2)? }))).collect()
    }
}

/// A book that is new for a profile.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NewBook {
    pub id: i64,
    /// Since when the profile can see it (Unix ms): the release, or for an administrator the day it was added.
    pub new_at: i64,
}

/// A very large time, so that a missing source does not win the minimum.
const NEVER: i64 = i64::MAX / 2;

impl Store {
    /// The books that became visible to a profile since `since_ms` and that it has not started, newest first. Parents
    /// (administrators and members) can use everything, so for them a book is new from the day it was added. For the others
    /// the moment a book became visible is the earliest of its release (for those who see released books) and a release
    /// given to the profile or its role.
    pub async fn new_books(&self, profile_id: i64, since_ms: i64, limit: i64) -> Result<Vec<NewBook>> {
        let sql = format!(
            "SELECT id, new_at FROM ( \
               SELECT b.id AS id, min( \
                 CASE WHEN p.role IN ('admin', 'member') THEN b.added_at ELSE {NEVER} END, \
                 CASE WHEN b.released = 1 AND (p.max_age IS NULL OR b.min_age <= p.max_age) THEN coalesce(b.released_at, b.added_at) ELSE {NEVER} END, \
                 coalesce((SELECT min(r.created_at) FROM book_release r WHERE r.book_id = b.id AND (r.profile_id = p.id OR r.role = p.role)), {NEVER}) \
               ) AS new_at \
               FROM book b JOIN profile p ON p.id = ? \
               WHERE {VISIBLE} \
                 AND NOT EXISTS (SELECT 1 FROM progress g WHERE g.profile_id = p.id AND g.book_id = b.id) \
                 AND NOT EXISTS (SELECT 1 FROM track_play y JOIN track t ON t.id = y.track_id WHERE y.profile_id = p.id AND t.book_id = b.id) \
             ) WHERE new_at < {NEVER} AND new_at >= ? ORDER BY new_at DESC, id DESC LIMIT ?"
        );
        let rows = self.db.reader.query_all_raw(Statement::from_sql_and_values(DbBackend::Sqlite, sql, [profile_id.into(), since_ms.into(), limit.into()])).await?;
        rows.into_iter().map(|r| Ok(NewBook { id: r.try_get_by_index(0)?, new_at: r.try_get_by_index(1)? })).collect()
    }

    /// Moves the day a book was added (and released) back by `ms`, for tests of the window of "new".
    #[doc(hidden)]
    pub async fn backdate_book(&self, book_id: i64, ms: i64) -> Result<()> {
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE book SET added_at = added_at - ?1, released_at = released_at - ?1 WHERE id = ?2", [ms.into(), book_id.into()]))
            .await?;
        Ok(())
    }

    /// How many days a book counts as new (the setting `new_days`; 30 if it is missing or not a number).
    pub async fn new_days(&self) -> Result<i64> {
        Ok(self.setting("new_days").await?.and_then(|v| v.trim().parse::<i64>().ok()).filter(|n| (1..=3650).contains(n)).unwrap_or(30))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_book_was_played_as_often_as_its_least_played_track() {
        let t = |plays, last| TrackPlays { plays, last_played_at: last };
        assert_eq!(book_plays(&[t(2, Some(10)), t(3, Some(30)), t(2, Some(20))]), BookPlays { plays: 2, last_played_at: Some(30) });
        assert_eq!(book_plays(&[t(2, Some(10)), t(0, None)]), BookPlays { plays: 0, last_played_at: Some(10) });
        assert_eq!(book_plays(&[]), BookPlays::default());
    }
}
