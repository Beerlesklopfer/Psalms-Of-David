// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The genres that the family looks after (project owner, 2026-10-28). The tags of the files say what they like;
//! here is the list that the apps show: its names, its order, and which tag texts belong to which genre. An album
//! takes the genre of the alias of its tag text, unless somebody set it by hand (then it keeps that, whatever the
//! tags say). Albums whose tag text has no alias are "without genre" and the administration lists those texts,
//! so that they can be given a genre.

use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
use serde::Serialize;

use crate::error::{Error, Result};
use crate::store::Store;

/// One genre of the list.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Genre {
    pub id: i64,
    pub name: String,
    pub position: i64,
    pub visible: bool,
    /// How many albums have this genre.
    pub albums: i64,
    /// The tag texts that are given this genre.
    pub tags: Vec<String>,
}

/// A tag text without a genre, with the number of albums that carry it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LooseTag {
    pub text: String,
    pub albums: i64,
}

/// The name of the standard ID3 genre list that `name` is, in its spelling (not case sensitive), or `None`. The list is
/// `lofty::id3::v1::GENRES`: the 80 genres of ID3v1 and the extensions of Winamp, 192 names in all.
pub fn standard_genre(name: &str) -> Option<&'static str> {
    let n = name.trim();
    lofty::id3::v1::GENRES.iter().copied().find(|g| g.eq_ignore_ascii_case(n))
}

/// What writing a genre into the files of the albums with a tag text did.
#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct RetagReport {
    pub albums: usize,
    pub files: usize,
    /// Files that could not be written (the rights of the server on them), with the reason.
    pub failed: Vec<String>,
}

fn clean(name: &str) -> Result<String> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > 80 || n.chars().any(char::is_control) {
        return Err(Error::WriteFailed("a genre has a name of 1 to 80 characters".into()));
    }
    Ok(n.to_owned())
}

impl Store {
    /// The list, in its order.
    pub async fn genres(&self) -> Result<Vec<Genre>> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT g.id, g.name, g.position, g.visible, (SELECT count(*) FROM book b WHERE b.genre_id = g.id) \
                 FROM genre g ORDER BY g.position, g.name COLLATE NOCASE",
            ))
            .await?;
        let aliases = self
            .db
            .reader
            .query_all_raw(Statement::from_string(DbBackend::Sqlite, "SELECT genre_id, tag_text FROM genre_alias ORDER BY tag_text COLLATE NOCASE"))
            .await?;
        let mut out = Vec::new();
        for r in rows {
            let id: i64 = r.try_get_by_index(0)?;
            let tags = aliases.iter().filter_map(|a| (a.try_get_by_index::<i64>(0).ok()? == id).then(|| a.try_get_by_index::<String>(1).ok()).flatten()).collect();
            out.push(Genre { id, name: r.try_get_by_index(1)?, position: r.try_get_by_index(2)?, visible: r.try_get_by_index::<i64>(3)? == 1, albums: r.try_get_by_index(4)?, tags });
        }
        Ok(out)
    }

    /// Adds a genre at the end of the list. A name that exists already (in any case) is that genre; its id is returned.
    pub async fn add_genre(&self, name: &str) -> Result<i64> {
        let name = clean(name)?;
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO genre (name, position) VALUES (?, (SELECT coalesce(max(position), 0) + 1 FROM genre)) ON CONFLICT (name) DO NOTHING",
                [Value::from(name.clone())],
            ))
            .await?;
        let row = self
            .db
            .writer
            .query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT id FROM genre WHERE name = ?", [Value::from(name)]))
            .await?
            .ok_or_else(|| Error::WriteFailed("genre not saved".into()))?;
        Ok(row.try_get_by_index(0)?)
    }

    pub async fn rename_genre(&self, id: i64, name: &str) -> Result<()> {
        let name = clean(name)?;
        let res = self
            .db
            .writer
            .execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE genre SET name = ? WHERE id = ?", [Value::from(name), Value::from(id)]))
            .await
            .map_err(|_| Error::WriteFailed("another genre has this name".into()))?;
        if res.rows_affected() == 0 { Err(Error::WriteFailed("no such genre".into())) } else { Ok(()) }
    }

    pub async fn set_genre_visible(&self, id: i64, visible: bool) -> Result<()> {
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE genre SET visible = ? WHERE id = ?", [Value::from(i64::from(visible)), Value::from(id)]))
            .await?;
        Ok(())
    }

    /// Moves a genre one place up (`-1`) or down (`1`) in the list.
    pub async fn move_genre(&self, id: i64, by: i64) -> Result<()> {
        let list = self.genres().await?;
        let Some(at) = list.iter().position(|g| g.id == id) else { return Err(Error::WriteFailed("no such genre".into())) };
        let to = (at as i64 + by.signum()).clamp(0, list.len() as i64 - 1) as usize;
        if to == at {
            return Ok(());
        }
        // The list is renumbered, so equal positions never leave the order open.
        let mut ids: Vec<i64> = list.iter().map(|g| g.id).collect();
        ids.swap(at, to);
        for (n, gid) in ids.iter().enumerate() {
            self.db
                .writer
                .execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE genre SET position = ? WHERE id = ?", [Value::from(n as i64 + 1), Value::from(*gid)]))
                .await?;
        }
        Ok(())
    }

    /// Removes a genre: its albums become "without genre" (unless chosen by hand: then too) and its tag texts loose again.
    pub async fn delete_genre(&self, id: i64) -> Result<()> {
        self.db.writer.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "DELETE FROM genre WHERE id = ?", [Value::from(id)])).await?;
        Ok(())
    }

    /// Says that a tag text belongs to a genre, and gives that genre to the albums that carry it (not those set by hand).
    pub async fn assign_genre_tag(&self, tag_text: &str, genre_id: i64) -> Result<()> {
        let text = tag_text.trim();
        if text.is_empty() || text.chars().count() > 200 {
            return Err(Error::WriteFailed("a tag text has 1 to 200 characters".into()));
        }
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO genre_alias (tag_text, genre_id) VALUES (?, ?) ON CONFLICT (tag_text) DO UPDATE SET genre_id = excluded.genre_id",
                [Value::from(text.to_owned()), Value::from(genre_id)],
            ))
            .await
            .map_err(|_| Error::WriteFailed("no such genre".into()))?;
        self.resolve_genres().await
    }

    /// Writes `name` as the genre tag into every file of the albums whose genre tag is `tag_text` (the tag of an MP3 is
    /// ID3v2) and gives those albums that text, so that the library and the files agree. Albums that cannot be written
    /// completely keep their old text. Nothing is written if the text already is the name.
    pub async fn retag_genre(&self, library: Option<&std::path::Path>, tag_text: &str, name: &str) -> Result<RetagReport> {
        let mut report = RetagReport::default();
        if tag_text == name {
            return Ok(report);
        }
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT id, folder FROM book WHERE genre_tag = ? COLLATE NOCASE", [Value::from(tag_text.to_owned())]))
            .await?;
        for row in rows {
            let (id, folder): (i64, String) = (row.try_get_by_index(0)?, row.try_get_by_index(1)?);
            let mut paths = Vec::new();
            for t in self.tracks_of(id).await? {
                match crate::api::resolve(self, library, &folder, &t.file).await {
                    Some(p) => paths.push(p),
                    None => report.failed.push(format!("{folder}/{}: not found", t.file)),
                }
            }
            let value = name.to_owned();
            let results = tokio::task::spawn_blocking(move || {
                paths.into_iter().map(|p| crate::tags::write_fields(&p, &[("Genre".to_owned(), vec![value.clone()])]).map_err(|e| format!("{}: {e}", p.display()))).collect::<Vec<_>>()
            })
            .await
            .map_err(|e| Error::WriteFailed(e.to_string()))?;
            let before = report.failed.len();
            for r in results {
                match r {
                    Ok(()) => report.files += 1,
                    Err(e) => report.failed.push(e),
                }
            }
            if report.failed.len() == before {
                report.albums += 1;
                self.db
                    .writer
                    .execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE book SET genre_tag = ? WHERE id = ?", [Value::from(name.to_owned()), Value::from(id)]))
                    .await?;
            }
        }
        self.resolve_genres().await?;
        Ok(report)
    }

    /// Takes a tag text out of its genre again.
    pub async fn remove_genre_tag(&self, tag_text: &str) -> Result<()> {
        self.db.writer.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "DELETE FROM genre_alias WHERE tag_text = ?", [Value::from(tag_text.trim().to_owned())])).await?;
        self.resolve_genres().await
    }

    /// Gives every album the genre of its tag text (albums set by hand keep theirs).
    pub async fn resolve_genres(&self) -> Result<()> {
        self.db
            .writer
            .execute_raw(Statement::from_string(
                DbBackend::Sqlite,
                "UPDATE book SET genre_id = (SELECT genre_id FROM genre_alias WHERE tag_text = book.genre_tag) WHERE genre_locked = 0",
            ))
            .await?;
        Ok(())
    }

    /// Sets the genre of one album by hand (`None`: without genre), or hands it back to the tags (`automatic`).
    pub async fn set_book_genre(&self, book_id: i64, genre: Option<i64>, automatic: bool) -> Result<()> {
        if automatic {
            self.db.writer.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE book SET genre_locked = 0 WHERE id = ?", [Value::from(book_id)])).await?;
            return self.resolve_genres().await;
        }
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE book SET genre_id = ?, genre_locked = 1 WHERE id = ?",
                [Value::from(genre), Value::from(book_id)],
            ))
            .await
            .map_err(|_| Error::WriteFailed("no such genre".into()))?;
        Ok(())
    }

    /// The tag texts that have no genre yet, with the number of albums, the most used first.
    pub async fn loose_genre_tags(&self) -> Result<Vec<LooseTag>> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT genre_tag, count(*) AS n FROM book WHERE genre_id IS NULL AND genre_locked = 0 AND genre_tag IS NOT NULL AND trim(genre_tag) <> '' \
                 GROUP BY lower(genre_tag) ORDER BY n DESC, genre_tag COLLATE NOCASE",
            ))
            .await?;
        rows.into_iter().map(|r| Ok(LooseTag { text: r.try_get_by_index(0)?, albums: r.try_get_by_index(1)? })).collect()
    }

    /// How many albums have no genre at all (neither a genre nor a tag text).
    pub async fn albums_without_genre(&self) -> Result<i64> {
        let r = self.db.reader.query_one_raw(Statement::from_string(DbBackend::Sqlite, "SELECT count(*) FROM book WHERE genre_id IS NULL")).await?.ok_or_else(|| Error::WriteFailed("count".into()))?;
        Ok(r.try_get_by_index(0)?)
    }
}
