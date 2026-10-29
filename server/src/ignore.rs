// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Rules that hide things from the reports and mark duplicates as wanted (project owner,
//! 2026-10-28). A rule names what it ignores (`what`) for a title, an album or a work with all its
//! CDs (`scope`). The reports (`find_duplicates`, `find_problems`, the badge in the lists) leave out
//! what a rule covers; nothing is changed in the library.

use serde::Serialize;

use crate::store::{AdminBookRow, Store};

/// What a rule covers: `duplicate`, a problem code, or everything.
pub const DUPLICATE: &str = "duplicate";
pub const ALL: &str = "*";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Rule {
    pub id: i64,
    pub scope: String,
    pub key: String,
    pub what: String,
    pub note: Option<String>,
    pub created_by: String,
    pub created_at: i64,
}

/// The key of a book for a scope: its folder, its title, or its title with the author.
pub fn key_of(row: &AdminBookRow, scope: &str) -> Option<String> {
    match scope {
        "album" => Some(row.folder.clone()),
        "title" => Some(crate::dupes::title_key(&row.title)),
        "work" => Some(format!("{}\u{1}{}", crate::dupes::title_key(&row.title), crate::dupes::title_key(&row.author))),
        _ => None,
    }
    .filter(|k| !k.is_empty() && k != "\u{1}")
}

impl Rule {
    /// Does this rule hide `what` of `row`?
    pub fn covers(&self, row: &AdminBookRow, what: &str) -> bool {
        (self.what == ALL || self.what == what) && key_of(row, &self.scope).is_some_and(|k| k == self.key)
    }
}

pub fn ignored(rules: &[Rule], row: &AdminBookRow, what: &str) -> bool {
    rules.iter().any(|r| r.covers(row, what))
}

impl Store {
    pub async fn ignore_rules(&self) -> crate::error::Result<Vec<Rule>> {
        use sea_orm::{ConnectionTrait, DbBackend, Statement};
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_string(DbBackend::Sqlite, "SELECT id, scope, key, what, note, created_by, created_at FROM ignore_rule ORDER BY id"))
            .await?;
        rows.into_iter()
            .map(|r| {
                Ok(Rule { id: r.try_get_by_index(0)?, scope: r.try_get_by_index(1)?, key: r.try_get_by_index(2)?, what: r.try_get_by_index(3)?, note: r.try_get_by_index(4)?, created_by: r.try_get_by_index(5)?, created_at: r.try_get_by_index(6)? })
            })
            .collect()
    }

    /// Adds a rule for a book (what is `duplicate`, a problem code or `*`); the same rule twice is one.
    /// Returns the id of the rule.
    pub async fn add_ignore(&self, book_id: i64, scope: &str, what: &str, note: Option<&str>, by: &str) -> crate::error::Result<i64> {
        use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
        let row = self.admin_book_row(book_id).await?.ok_or_else(|| crate::error::Error::WriteFailed("no such book".into()))?;
        let key = key_of(&row, scope).ok_or_else(|| crate::error::Error::WriteFailed(format!("a rule needs a scope of album, title or work and a book with a name; got {scope}")))?;
        let what = what.trim();
        if what.is_empty() || what.chars().count() > 60 || what.chars().any(char::is_control) {
            return Err(crate::error::Error::WriteFailed("what is a problem code, duplicate or *".into()));
        }
        let note = note.map(str::trim).filter(|n| !n.is_empty()).map(|n| n.chars().take(300).collect::<String>());
        let values: [Value; 6] = [scope.to_owned().into(), key.into(), what.to_owned().into(), note.into(), by.to_owned().into(), crate::store::now_ms().into()];
        let res = self
            .db
            .writer
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO ignore_rule (scope, key, what, note, created_by, created_at) VALUES (?, ?, ?, ?, ?, ?) \
                 ON CONFLICT (scope, key, what) DO UPDATE SET note = coalesce(excluded.note, ignore_rule.note) RETURNING id",
                values,
            ))
            .await?
            .ok_or_else(|| crate::error::Error::WriteFailed("rule not saved".into()))?;
        Ok(res.try_get_by_index(0)?)
    }

    pub async fn remove_ignore(&self, id: i64) -> crate::error::Result<bool> {
        use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
        let res = self.db.writer.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "DELETE FROM ignore_rule WHERE id = ?", [Value::from(id)])).await?;
        Ok(res.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: i64, folder: &str, title: &str, author: &str) -> AdminBookRow {
        AdminBookRow {
            id,
            kind: "audiobook".into(),
            folder: folder.into(),
            title: title.into(),
            author: author.into(),
            narrator: None,
            genre: None,
            genre_id: None,
            genre_locked: false,
            genre_tag: None,
            released: false,
            min_age: None,
            recommended_age: None,
            quota_exempt: false,
            description: None,
            tracks: 1,
            discs: 1,
            owner_id: None,
            owner: None,
            duration_ms: 1,
        }
    }

    fn rule(scope: &str, key: String, what: &str) -> Rule {
        Rule { id: 1, scope: scope.into(), key, what: what.into(), note: None, created_by: "t".into(), created_at: 0 }
    }

    #[test]
    fn a_rule_covers_one_album_every_title_or_a_work_with_its_cds() {
        let a = row(1, "A/Kinder der Freiheit CD1", "Kinder der Freiheit CD1", "Follett");
        let b = row(2, "B/Kinder der Freiheit (CD 2)", "Kinder der Freiheit (CD 2)", "Follett");
        let c = row(3, "C/Kinder der Freiheit", "Kinder der Freiheit", "Johannes Steck");
        let album = rule("album", key_of(&a, "album").unwrap(), DUPLICATE);
        assert!(album.covers(&a, DUPLICATE) && !album.covers(&b, DUPLICATE));
        let title = rule("title", key_of(&a, "title").unwrap(), DUPLICATE);
        assert!(title.covers(&a, DUPLICATE) && title.covers(&b, DUPLICATE) && title.covers(&c, DUPLICATE));
        let work = rule("work", key_of(&a, "work").unwrap(), DUPLICATE);
        assert!(work.covers(&a, DUPLICATE) && work.covers(&b, DUPLICATE), "the CDs of the work");
        assert!(!work.covers(&c, DUPLICATE), "another author's copy is not covered");
        // What a rule hides is named: a duplicate rule leaves a problem standing, `*` hides both.
        assert!(!title.covers(&a, "missing_year") && rule("title", key_of(&a, "title").unwrap(), ALL).covers(&a, "missing_year"));
    }
}
