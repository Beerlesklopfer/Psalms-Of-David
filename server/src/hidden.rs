// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The own library of a profile (project owner, 2026-10-29; see `docs/spec-library-hiding.md`): a profile hides books and the books of
//! an author or artist from its own lists, and decides with the setting `show_hidden` whether the hidden ones are still listed (marked).
//! Hiding is no access rule: it only changes the lists of the profile that did it.

use std::collections::HashSet;

use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
use serde::Serialize;

use crate::error::{Error, Result};
use crate::store::{BookRow, Store, VISIBLE, now_ms};

/// What a profile has hidden.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Hidden {
    pub show_hidden: bool,
    pub books: Vec<HiddenBook>,
    pub authors: Vec<HiddenAuthor>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HiddenBook {
    pub id: i64,
    pub title: String,
    pub author: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HiddenAuthor {
    pub id: i64,
    pub name: String,
    /// `audiobook`, `music`, or empty for both.
    pub kind: String,
}

impl Hidden {
    /// Is the book hidden by this profile (by itself or by its author)?
    pub fn covers(&self, book: &BookRow) -> bool {
        let author = book.author.to_lowercase();
        self.books.iter().any(|b| b.id == book.id) || self.authors.iter().any(|a| a.name.to_lowercase() == author && (a.kind.is_empty() || a.kind == book.kind))
    }

    /// The ids of the books of `rows` that are hidden.
    pub fn ids_in(&self, rows: &[BookRow]) -> HashSet<i64> {
        rows.iter().filter(|b| self.covers(b)).map(|b| b.id).collect()
    }
}

fn stmt(sql: &str, values: impl IntoIterator<Item = Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, sql, values)
}

impl Store {
    /// Everything a profile has hidden, and its setting.
    pub async fn hidden_of(&self, profile_id: i64) -> Result<Hidden> {
        let show = self
            .db
            .reader
            .query_one_raw(stmt("SELECT show_hidden FROM profile WHERE id = ?", [profile_id.into()]))
            .await?
            .map(|r| r.try_get_by_index::<i64>(0))
            .transpose()?
            .unwrap_or(0);
        let books = self
            .db
            .reader
            // Only books the profile may see: a book that is not (or no longer) released for it is not told by name.
            .query_all_raw(stmt(
                &format!("SELECT b.id, b.title, a.name FROM profile_hidden h JOIN profile p ON p.id = h.profile_id JOIN book b ON b.id = h.book_id JOIN author a ON a.id = b.author_id WHERE h.profile_id = ? AND {VISIBLE} ORDER BY a.name, b.title"),
                [profile_id.into()],
            ))
            .await?;
        let authors = self
            .db
            .reader
            // Only authors with a book the profile may see, like the books above.
            .query_all_raw(stmt(
                &format!("SELECT a.id, a.name, h.kind FROM profile_hidden h JOIN profile p ON p.id = h.profile_id JOIN author a ON a.id = h.author_id WHERE h.profile_id = ? AND EXISTS (SELECT 1 FROM book b WHERE b.author_id = a.id AND {VISIBLE}) ORDER BY a.name, h.kind"),
                [profile_id.into()],
            ))
            .await?;
        Ok(Hidden {
            show_hidden: show == 1,
            books: books.iter().map(|r| Ok(HiddenBook { id: r.try_get_by_index(0)?, title: r.try_get_by_index(1)?, author: r.try_get_by_index(2)? })).collect::<Result<_>>()?,
            authors: authors.iter().map(|r| Ok(HiddenAuthor { id: r.try_get_by_index(0)?, name: r.try_get_by_index(1)?, kind: r.try_get_by_index(2)? })).collect::<Result<_>>()?,
        })
    }

    /// The setting "show what I hid".
    pub async fn set_show_hidden(&self, profile_id: i64, show: bool) -> Result<()> {
        self.db.writer.execute_raw(stmt("UPDATE profile SET show_hidden = ? WHERE id = ?", [i64::from(show).into(), profile_id.into()])).await?;
        Ok(())
    }

    /// Hides a book for the profile, or shows it again. `false` if there is no such book for this profile: a book it may not
    /// see answers like one that does not exist, so hiding cannot be used to find out which books there are.
    pub async fn set_book_hidden(&self, profile_id: i64, book_id: i64, hidden: bool) -> Result<bool> {
        // Showing again works for whatever the profile itself hid (that tells it nothing new); hiding needs a book it may see.
        let sql = if hidden {
            format!("SELECT 1 FROM book b JOIN profile p ON p.id = ? WHERE b.id = ? AND {VISIBLE}")
        } else {
            "SELECT 1 FROM profile_hidden WHERE profile_id = ? AND book_id = ?".to_owned()
        };
        let exists = self.db.reader.query_one_raw(stmt(&sql, [profile_id.into(), book_id.into()])).await?.is_some();
        if !exists {
            return Ok(false);
        }
        self.write_book_hidden(profile_id, book_id, hidden).await?;
        Ok(true)
    }

    /// Hides a book for a profile, or shows it again, for an administrator (the MCP tool `hide_albums`): also a book the
    /// profile cannot see yet, so it stays hidden when it is released later. The caller has checked that the book exists.
    pub async fn admin_set_book_hidden(&self, profile_id: i64, book_id: i64, hidden: bool) -> Result<()> {
        self.write_book_hidden(profile_id, book_id, hidden).await
    }

    async fn write_book_hidden(&self, profile_id: i64, book_id: i64, hidden: bool) -> Result<()> {
        if hidden {
            self.db
                .writer
                .execute_raw(stmt("INSERT OR IGNORE INTO profile_hidden (profile_id, book_id, kind, created_at) VALUES (?, ?, '', ?)", [profile_id.into(), book_id.into(), now_ms().into()]))
                .await
                .map_err(Error::from_db)?;
        } else {
            self.db.writer.execute_raw(stmt("DELETE FROM profile_hidden WHERE profile_id = ? AND book_id = ?", [profile_id.into(), book_id.into()])).await?;
        }
        Ok(())
    }

    /// Hides the books of an author or artist (by name) of one kind (`""` = both) for the profile, or shows them again. `false` if no
    /// book of the library that the profile may see has this author (so the name of an author of hidden books is not confirmed).
    pub async fn set_author_hidden(&self, profile_id: i64, author: &str, kind: &str, hidden: bool) -> Result<bool> {
        let Some(row) = self
            .db
            .reader
            .query_one_raw(if hidden {
                stmt(
                    &format!("SELECT a.id FROM author a JOIN book b ON b.author_id = a.id JOIN profile p ON p.id = ? WHERE a.name = ? COLLATE NOCASE AND {VISIBLE} LIMIT 1"),
                    [profile_id.into(), author.trim().to_owned().into()],
                )
            } else {
                // Showing again: an author this profile hid, whatever it may see now.
                stmt(
                    "SELECT a.id FROM profile_hidden h JOIN author a ON a.id = h.author_id WHERE h.profile_id = ? AND a.name = ? COLLATE NOCASE LIMIT 1",
                    [profile_id.into(), author.trim().to_owned().into()],
                )
            })
            .await?
        else {
            return Ok(false);
        };
        let author_id: i64 = row.try_get_by_index(0)?;
        if hidden {
            self.db
                .writer
                .execute_raw(stmt("INSERT OR IGNORE INTO profile_hidden (profile_id, author_id, kind, created_at) VALUES (?, ?, ?, ?)", [profile_id.into(), author_id.into(), kind.to_owned().into(), now_ms().into()]))
                .await
                .map_err(Error::from_db)?;
        } else {
            self.db.writer.execute_raw(stmt("DELETE FROM profile_hidden WHERE profile_id = ? AND author_id = ? AND kind = ?", [profile_id.into(), author_id.into(), kind.to_owned().into()])).await?;
        }
        Ok(true)
    }

    /// A short value that changes when the profile hides or shows something or changes its setting: the apps reload their lists then.
    pub async fn hidden_version(&self, profile_id: i64) -> Result<String> {
        use sha2::{Digest, Sha256};
        let h = self.hidden_of(profile_id).await?;
        let mut hasher = Sha256::new();
        hasher.update([u8::from(h.show_hidden)]);
        for b in &h.books {
            hasher.update(format!("b{}|", b.id).as_bytes());
        }
        for a in &h.authors {
            hasher.update(format!("a{}:{}|", a.id, a.kind).as_bytes());
        }
        Ok(hasher.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect())
    }
}

impl Store {
    /// The books each profile may see, for the matrix of the media list: for every profile (administrators too, who see everything, last
    /// field `true`) the ids of the books it sees (the same rule as every list: release, age, exceptions, and what is closed to it).
    pub async fn visibility_matrix(&self) -> Result<Vec<(i64, String, HashSet<i64>, bool)>> {
        let mut out = Vec::new();
        for user in self.user_rows().await? {
            let seen = self.visible_books(user.id).await?.into_iter().map(|b| b.id).collect();
            out.push((user.id, user.name, seen, user.role == "admin"));
        }
        Ok(out)
    }

    /// Opens a book to a family member or closes it to them. Opening takes away what closes it and, if the rules do not show it to them
    /// anyway, adds an exception that does; closing takes away the exception and closes the book to them. `false` if the book or the
    /// profile does not exist. An administrator sees everything unless the book is closed to them, so for them opening only takes the closing away.
    pub async fn set_book_visible_for(&self, book_id: i64, profile_id: i64, visible: bool, by: Option<i64>) -> Result<bool> {
        let ok = self
            .db
            .reader
            .query_one_raw(stmt("SELECT 1 FROM book b JOIN profile p ON p.id = ? WHERE b.id = ?", [profile_id.into(), book_id.into()]))
            .await?
            .is_some();
        if !ok {
            return Ok(false);
        }
        let w = &self.db.writer;
        if visible {
            w.execute_raw(stmt("DELETE FROM book_block WHERE book_id = ? AND profile_id = ?", [book_id.into(), profile_id.into()])).await?;
            if !self.visible_books(profile_id).await?.iter().any(|b| b.id == book_id) {
                w.execute_raw(stmt("INSERT OR IGNORE INTO book_release (book_id, profile_id, created_by, created_at) VALUES (?, ?, ?, ?)", [book_id.into(), profile_id.into(), by.into(), now_ms().into()])).await?;
            }
        } else {
            w.execute_raw(stmt("DELETE FROM book_release WHERE book_id = ? AND profile_id = ?", [book_id.into(), profile_id.into()])).await?;
            w.execute_raw(stmt("INSERT OR IGNORE INTO book_block (book_id, profile_id) VALUES (?, ?)", [book_id.into(), profile_id.into()])).await?;
        }
        Ok(true)
    }
}
