// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Labels: the classic tagging (project owner, 2026-10-30: "everything about prayer, or school, or common holidays"). A label is a free word of the
//! list of the family; a book (audiobook or album), a single title (track), a podcast and a radio station carry any number of them, and the lists are filtered by them. Not to be mixed up with the
//! ID3 tags of the files (`tags.rs`). One list for the family, kept by the administrators; a label that nothing carries any more goes.

use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
use serde::Serialize;

use crate::error::{Error, Result};
use crate::store::Store;

/// The longest name of a label, in characters.
pub const MAX_NAME_CHARS: usize = 40;
/// Most labels one book carries.
pub const MAX_PER_BOOK: usize = 30;
/// Most labels of its own one title (track) carries; it has the ones of its album besides.
pub const MAX_PER_TRACK: usize = 10;

/// A name as it is stored: inner white space made one space, 1 to 40 characters, nothing that separates a list (`,` `;` `|`) and no control character,
/// and not the reserved name of the system mark (`blacklist::MARK`).
pub fn clean_name(raw: &str) -> Option<String> {
    let name = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let n = name.chars().count();
    ((1..=MAX_NAME_CHARS).contains(&n) && !name.chars().any(|c| c.is_control() || matches!(c, ',' | ';' | '|')) && !crate::blacklist::is_mark(&name)).then_some(name)
}

/// The names of a list written with commas (`Gebet, Schule`), cleaned and without doubles (case ignored); `None` if one is not usable.
pub fn parse_list(text: &str) -> Option<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for part in text.split([',', ';']).map(str::trim).filter(|p| !p.is_empty()) {
        let name = clean_name(part)?;
        if !out.iter().any(|o| o.to_lowercase() == name.to_lowercase()) {
            out.push(name);
        }
    }
    Some(out)
}

/// Most labels one podcast or one radio station carries.
pub const MAX_PER_FEED: usize = 30;

/// One label of the list with the number of books, of single titles, of podcasts and of radio stations that carry it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LabelRow {
    pub name: String,
    pub books: i64,
    pub tracks: i64,
    pub podcasts: i64,
    pub stations: i64,
}

impl Store {
    /// All labels with the number of their books, by name.
    pub async fn labels(&self) -> Result<Vec<LabelRow>> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT l.name, (SELECT COUNT(*) FROM book_label bl WHERE bl.label_id = l.id), (SELECT COUNT(*) FROM track_label tl WHERE tl.label_id = l.id), (SELECT COUNT(*) FROM podcast_label pl WHERE pl.label_id = l.id), (SELECT COUNT(*) FROM station_label sl WHERE sl.label_id = l.id) FROM label l ORDER BY l.name COLLATE NOCASE",
            ))
            .await?;
        rows.iter().map(|r| Ok(LabelRow { name: r.try_get_by_index(0)?, books: r.try_get_by_index(1)?, tracks: r.try_get_by_index(2)?, podcasts: r.try_get_by_index(3)?, stations: r.try_get_by_index(4)? })).collect()
    }

    /// The labels of every book: book id -> names, by name.
    pub async fn book_labels(&self) -> Result<HashMap<i64, Vec<String>>> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_string(DbBackend::Sqlite, "SELECT bl.book_id, l.name FROM book_label bl JOIN label l ON l.id = bl.label_id ORDER BY l.name COLLATE NOCASE"))
            .await?;
        let mut out: HashMap<i64, Vec<String>> = HashMap::new();
        for r in rows {
            let (id, name): (i64, String) = (r.try_get_by_index(0)?, r.try_get_by_index(1)?);
            out.entry(id).or_default().push(name);
        }
        Ok(out)
    }

    /// The labels of one book.
    pub async fn labels_of(&self, book_id: i64) -> Result<Vec<String>> {
        Ok(self.book_labels().await?.remove(&book_id).unwrap_or_default())
    }

    /// Gives a book exactly these labels (new ones are made, the ones that no book or title carries any more go). Returns the labels the book has now.
    /// `Err(InvalidName)` for a name that is not usable, `Ok(None)` if there is no such book.
    pub async fn set_book_labels(&self, book_id: i64, names: &[String]) -> Result<Option<Vec<String>>> {
        if !self.set_labels_of("book", "book_label", "book_id", book_id, names, MAX_PER_BOOK).await? {
            return Ok(None);
        }
        Ok(Some(self.labels_of(book_id).await?))
    }

    /// Gives a title (track) exactly these labels of its own; the labels of its album come on top and are not touched. Returns the labels of its own
    /// the title has now. `Err(InvalidName)` for a name that is not usable, `Ok(None)` if there is no such track.
    pub async fn set_track_labels(&self, track_id: i64, names: &[String]) -> Result<Option<Vec<String>>> {
        if !self.set_labels_of("track", "track_label", "track_id", track_id, names, MAX_PER_TRACK).await? {
            return Ok(None);
        }
        Ok(Some(self.track_labels().await?.remove(&track_id).unwrap_or_default()))
    }

    /// Adds and removes labels of a title (the others stay). Same results as `set_track_labels`.
    pub async fn change_track_labels(&self, track_id: i64, add: &[String], remove: &[String]) -> Result<Option<Vec<String>>> {
        let mut names = self.track_labels().await?.remove(&track_id).unwrap_or_default();
        let gone: Vec<String> = remove.iter().map(|r| r.trim().to_lowercase()).collect();
        names.retain(|n| !gone.contains(&n.to_lowercase()));
        names.extend(add.iter().cloned());
        self.set_track_labels(track_id, &names).await
    }

    /// The labels of every title of its own: track id -> names, by name.
    pub async fn track_labels(&self) -> Result<HashMap<i64, Vec<String>>> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_string(DbBackend::Sqlite, "SELECT tl.track_id, l.name FROM track_label tl JOIN label l ON l.id = tl.label_id ORDER BY l.name COLLATE NOCASE"))
            .await?;
        let mut out: HashMap<i64, Vec<String>> = HashMap::new();
        for r in rows {
            let (id, name): (i64, String) = (r.try_get_by_index(0)?, r.try_get_by_index(1)?);
            out.entry(id).or_default().push(name);
        }
        Ok(out)
    }

    /// The labels that the titles of a book carry (without the ones of the book itself): book id -> names, by name, without doubles.
    pub async fn track_labels_by_book(&self) -> Result<HashMap<i64, Vec<String>>> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT DISTINCT t.book_id, l.name FROM track_label tl JOIN track t ON t.id = tl.track_id JOIN label l ON l.id = tl.label_id ORDER BY l.name COLLATE NOCASE",
            ))
            .await?;
        let mut out: HashMap<i64, Vec<String>> = HashMap::new();
        for r in rows {
            let (id, name): (i64, String) = (r.try_get_by_index(0)?, r.try_get_by_index(1)?);
            out.entry(id).or_default().push(name);
        }
        Ok(out)
    }

    /// The labels of every podcast (feed): feed id -> names, by name.
    pub async fn podcast_labels(&self) -> Result<HashMap<i64, Vec<String>>> {
        self.links_by_id("SELECT pl.feed_id, l.name FROM podcast_label pl JOIN label l ON l.id = pl.label_id ORDER BY l.name COLLATE NOCASE").await
    }

    /// The labels of every radio station: station id -> names, by name.
    pub async fn station_labels(&self) -> Result<HashMap<i64, Vec<String>>> {
        self.links_by_id("SELECT sl.station_id, l.name FROM station_label sl JOIN label l ON l.id = sl.label_id ORDER BY l.name COLLATE NOCASE").await
    }

    async fn links_by_id(&self, sql: &str) -> Result<HashMap<i64, Vec<String>>> {
        let rows = self.db.reader.query_all_raw(Statement::from_string(DbBackend::Sqlite, sql)).await?;
        let mut out: HashMap<i64, Vec<String>> = HashMap::new();
        for r in rows {
            let (id, name): (i64, String) = (r.try_get_by_index(0)?, r.try_get_by_index(1)?);
            out.entry(id).or_default().push(name);
        }
        Ok(out)
    }

    /// Gives a podcast (feed) exactly these labels. Returns the labels it has now; `Ok(None)` if there is no such podcast. `Err(InvalidName)` as for books.
    pub async fn set_podcast_labels(&self, feed_id: i64, names: &[String]) -> Result<Option<Vec<String>>> {
        if !self.set_labels_of("feed", "podcast_label", "feed_id", feed_id, names, MAX_PER_FEED).await? {
            return Ok(None);
        }
        Ok(Some(self.podcast_labels().await?.remove(&feed_id).unwrap_or_default()))
    }

    /// Gives a radio station exactly these labels. Same results as `set_podcast_labels`.
    pub async fn set_station_labels(&self, station_id: i64, names: &[String]) -> Result<Option<Vec<String>>> {
        if !self.set_labels_of("station", "station_label", "station_id", station_id, names, MAX_PER_FEED).await? {
            return Ok(None);
        }
        Ok(Some(self.station_labels().await?.remove(&station_id).unwrap_or_default()))
    }

    /// Adds and removes labels of a podcast (the others stay).
    pub async fn change_podcast_labels(&self, feed_id: i64, add: &[String], remove: &[String]) -> Result<Option<Vec<String>>> {
        let mut names = self.podcast_labels().await?.remove(&feed_id).unwrap_or_default();
        change(&mut names, add, remove);
        self.set_podcast_labels(feed_id, &names).await
    }

    /// Adds and removes labels of a radio station (the others stay).
    pub async fn change_station_labels(&self, station_id: i64, add: &[String], remove: &[String]) -> Result<Option<Vec<String>>> {
        let mut names = self.station_labels().await?.remove(&station_id).unwrap_or_default();
        change(&mut names, add, remove);
        self.set_station_labels(station_id, &names).await
    }

    /// The album a title belongs to.
    pub async fn book_of_track(&self, track_id: i64) -> Result<Option<i64>> {
        let row = self.db.reader.query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT book_id FROM track WHERE id = ?", [track_id.into()])).await?;
        Ok(row.map(|r| r.try_get_by_index(0)).transpose()?)
    }

    /// Replaces the labels of one row of `table` (`book`, `track`, `feed` or `station`) in its link table. `false` if the row does not exist.
    async fn set_labels_of(&self, table: &str, link: &str, column: &str, id: i64, names: &[String], max: usize) -> Result<bool> {
        let mut wanted: Vec<String> = Vec::new();
        for raw in names {
            let name = clean_name(raw).ok_or(Error::InvalidName)?;
            if !wanted.iter().any(|w| w.to_lowercase() == name.to_lowercase()) {
                wanted.push(name);
            }
        }
        if wanted.len() > max {
            return Err(Error::InvalidName);
        }
        if self.db.reader.query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, format!("SELECT 1 FROM {table} WHERE id = ?"), [id.into()])).await?.is_none() {
            return Ok(false);
        }
        let w = &self.db.writer;
        for name in &wanted {
            w.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "INSERT OR IGNORE INTO label (name) VALUES (?)", [name.clone().into()])).await?;
        }
        // The rows of this item that are not wanted any more.
        let keep: Vec<Value> = wanted.iter().map(|n| Value::from(n.to_lowercase())).collect();
        let marks = keep.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let mut values: Vec<Value> = vec![id.into()];
        values.extend(keep.iter().cloned());
        let not_in = if keep.is_empty() { String::new() } else { format!(" AND label_id NOT IN (SELECT id FROM label WHERE lower(name) IN ({marks}))") };
        w.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, format!("DELETE FROM {link} WHERE {column} = ?{not_in}"), values)).await?;
        for name in &wanted {
            w.execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("INSERT OR IGNORE INTO {link} ({column}, label_id) SELECT ?, id FROM label WHERE name = ? COLLATE NOCASE"),
                [id.into(), name.clone().into()],
            ))
            .await?;
        }
        self.drop_unused_labels().await?;
        // The labels are checked against the blacklist as well.
        match table {
            "book" => self.blacklist_check_quietly(crate::blacklist::Scope::One(crate::blacklist::Kind::Book, id)).await,
            "track" => self.blacklist_check_track_quietly(id).await,
            "feed" => self.blacklist_check_quietly(crate::blacklist::Scope::One(crate::blacklist::Kind::Podcast, id)).await,
            "station" => self.blacklist_check_quietly(crate::blacklist::Scope::One(crate::blacklist::Kind::Station, id)).await,
            _ => {}
        }
        Ok(true)
    }

    /// Adds and removes labels of a book (the others stay). Same results as `set_book_labels`.
    pub async fn change_book_labels(&self, book_id: i64, add: &[String], remove: &[String]) -> Result<Option<Vec<String>>> {
        let mut names = self.labels_of(book_id).await?;
        let gone: Vec<String> = remove.iter().map(|r| r.trim().to_lowercase()).collect();
        names.retain(|n| !gone.contains(&n.to_lowercase()));
        names.extend(add.iter().cloned());
        self.set_book_labels(book_id, &names).await
    }

    /// Renames a label; if the new name exists already the two become one. `false` if there is no label of the old name.
    pub async fn rename_label(&self, old: &str, new: &str) -> Result<bool> {
        let new = clean_name(new).ok_or(Error::InvalidName)?;
        let w = &self.db.writer;
        let Some(row) = self.db.reader.query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT id FROM label WHERE name = ? COLLATE NOCASE", [old.trim().to_owned().into()])).await? else {
            return Ok(false);
        };
        let id: i64 = row.try_get_by_index(0)?;
        let other: Option<i64> = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT id FROM label WHERE name = ? COLLATE NOCASE AND id <> ?", [new.clone().into(), id.into()]))
            .await?
            .map(|r| r.try_get_by_index(0))
            .transpose()?;
        match other {
            Some(target) => {
                w.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "INSERT OR IGNORE INTO book_label (book_id, label_id) SELECT book_id, ? FROM book_label WHERE label_id = ?", [target.into(), id.into()])).await?;
                w.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "INSERT OR IGNORE INTO track_label (track_id, label_id) SELECT track_id, ? FROM track_label WHERE label_id = ?", [target.into(), id.into()])).await?;
                for (link, column) in [("podcast_label", "feed_id"), ("station_label", "station_id")] {
                    w.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, format!("INSERT OR IGNORE INTO {link} ({column}, label_id) SELECT {column}, ? FROM {link} WHERE label_id = ?"), [target.into(), id.into()])).await?;
                }
                w.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "DELETE FROM label WHERE id = ?", [id.into()])).await?;
            }
            None => {
                w.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE label SET name = ? WHERE id = ?", [new.into(), id.into()])).await?;
            }
        }
        self.blacklist_check_quietly(crate::blacklist::Scope::All).await;
        Ok(true)
    }

    /// Removes a label from every book and title. `false` if there is no such label.
    pub async fn delete_label(&self, name: &str) -> Result<bool> {
        let res = self.db.writer.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "DELETE FROM label WHERE name = ? COLLATE NOCASE", [name.trim().to_owned().into()])).await?;
        let gone = res.rows_affected() > 0;
        if gone {
            self.blacklist_check_quietly(crate::blacklist::Scope::All).await;
        }
        Ok(gone)
    }

    pub(crate) async fn drop_unused_labels(&self) -> Result<()> {
        self.db.writer.execute_raw(Statement::from_string(DbBackend::Sqlite, "DELETE FROM label WHERE id NOT IN (SELECT label_id FROM book_label) AND id NOT IN (SELECT label_id FROM track_label) AND id NOT IN (SELECT label_id FROM podcast_label) AND id NOT IN (SELECT label_id FROM station_label)")).await?;
        Ok(())
    }
}

/// The names after taking `remove` away and putting `add` on.
fn change(names: &mut Vec<String>, add: &[String], remove: &[String]) {
    let gone: Vec<String> = remove.iter().map(|r| r.trim().to_lowercase()).collect();
    names.retain(|n| !gone.contains(&n.to_lowercase()));
    names.extend(add.iter().cloned());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_cleaned_and_lists_are_split_without_doubles() {
        assert_eq!(clean_name("  Gemeinsame   Urlaube "), Some("Gemeinsame Urlaube".into()));
        assert_eq!(clean_name(""), None);
        assert_eq!(clean_name("a,b"), None);
        assert_eq!(clean_name(&"x".repeat(41)), None);
        assert_eq!(parse_list("Gebet, schule; Schule ,Gebet"), Some(vec!["Gebet".to_owned(), "schule".to_owned()]));
        assert_eq!(parse_list(""), Some(vec![]));
        assert_eq!(parse_list("ok, a|b"), None);
    }
}
