// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The blacklist (project owner, 2026-10-30: "there is a blacklist, MCP can search for it"). The administrators keep a list of words in the table
//! `blacklist_word` (one word, phrase or pattern per row with an optional note; maintained on the page /admin/blacklist, by `edit_blacklist` and
//! nothing comes with the program). A medium whose text contains one of them carries the system mark
//! [`MARK`], which only administrators see; nothing is blocked or hidden by it.
//!
//! How a line matches (so that the false positives can be judged):
//! * Both the line and the text are compared in lower case with the accents of German and French folded (`ä` is `a`, `ß` is `ss`), as the lists sort.
//! * A line without `*` and `?` matches when the text CONTAINS it anywhere (also inside a longer word: `katze` hits `Katzenjammer`).
//! * `*` stands for any run of characters and `?` for one character (the rules of the search, see `search.rs`): `sex * katze`. Between slashes the
//!   line is a regular expression.
//! * Every field is checked on its own (a wildcard does not reach from the title into the blurb). The mark remembers the word and the field that hit.
//!
//! What is checked: for an album or an audiobook its title, author, narrator, blurb, the titles of its tracks and its labels (also those of its tracks);
//! for a podcast its title, description and labels; for a radio station its name and labels. Addresses are not checked.
//!
//! The mark is a row of `blacklist_mark`, not a label of the family's list. `approved`: an administrator took the mark off; it is not put on again
//! as long as the medium hits, and it goes when the medium does not hit any more.

use std::collections::{HashMap, HashSet};

use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
use serde::Serialize;

use crate::error::Result;
use crate::search::{compile, Matcher};
use crate::store::Store;
use crate::tree::fold;

/// The reserved name of the system label. Nobody can use it as an ordinary label (`labels::clean_name` refuses it).
pub const MARK: &str = "Blacklist";

/// The most lines the setting may hold, and the longest line, in characters.
pub const MAX_LINES: usize = 500;
pub const MAX_LINE_CHARS: usize = 100;

/// Is this name the reserved name of the mark (case ignored)?
pub fn is_mark(name: &str) -> bool {
    name.split_whitespace().collect::<Vec<_>>().join(" ").eq_ignore_ascii_case(MARK)
}

/// The names without the mark, and whether the mark was among them.
pub fn split_mark(names: &[String]) -> (Vec<String>, bool) {
    let rest: Vec<String> = names.iter().filter(|n| !is_mark(n)).cloned().collect();
    let had = rest.len() != names.len();
    (rest, had)
}

/// The same for a list written with commas, as the forms of the pages send it: the text without the mark, and whether it was in there.
pub fn strip_mark_text(text: &str) -> (String, bool) {
    let parts: Vec<&str> = text.split([',', ';']).map(str::trim).filter(|p| !p.is_empty()).collect();
    let kept: Vec<&str> = parts.iter().copied().filter(|p| !is_mark(p)).collect();
    (kept.join(", "), kept.len() != parts.len())
}

/// The names as an administrator sees them: the mark comes on top of the labels of a marked medium. Everybody else sees the labels only.
pub fn with_mark(mut names: Vec<String>, marked: bool, admin: bool) -> Vec<String> {
    if admin && marked {
        names.insert(0, MARK.to_owned());
    }
    names
}

/// The three kinds of media the mark can hang on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Book,
    Podcast,
    Station,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Book => "book",
            Kind::Podcast => "podcast",
            Kind::Station => "station",
        }
    }
}

/// The compiled list of words.
#[derive(Debug)]
pub struct Blacklist {
    rules: Vec<(String, Matcher)>,
}

impl Blacklist {
    /// Reads the lines of the setting. `Err` carries the line that does not compile (a broken regular expression) or the limit that was passed.
    pub fn parse(text: &str) -> std::result::Result<Blacklist, String> {
        let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        if lines.len() > MAX_LINES {
            return Err(format!("more than {MAX_LINES} lines"));
        }
        let mut rules = Vec::new();
        for line in lines {
            if line.chars().count() > MAX_LINE_CHARS || line.chars().any(char::is_control) {
                return Err(line.to_owned());
            }
            let matcher = compile(&fold(line)).map_err(|_| line.to_owned())?;
            rules.push((line.to_owned(), matcher));
        }
        Ok(Blacklist { rules })
    }

    /// The same, but a line that does not compile is left out (what is stored was checked when it was saved).
    pub fn lenient(text: &str) -> Blacklist {
        let rules = text.lines().map(str::trim).filter(|l| !l.is_empty()).take(MAX_LINES).filter_map(|l| compile(&fold(l)).ok().map(|m| (l.to_owned(), m))).collect();
        Blacklist { rules }
    }

    /// The list of these words; a word that does not compile is left out.
    pub fn from_words(words: &[String]) -> Blacklist {
        let rules = words.iter().map(|w| w.trim()).filter(|w| !w.is_empty()).filter_map(|w| compile(&fold(w)).ok().map(|m| (w.to_owned(), m))).collect();
        Blacklist { rules }
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// The first word that one of the fields matches, and the field it matched (the fields are looked at in the order given).
    pub fn hit_fields(&self, fields: &[(&'static str, String)]) -> Option<(&str, &'static str)> {
        if self.rules.is_empty() {
            return None;
        }
        for (name, text) in fields {
            if text.trim().is_empty() {
                continue;
            }
            let folded = fold(text);
            if let Some((line, _)) = self.rules.iter().find(|(_, m)| m.is_match(&folded)) {
                return Some((line, *name));
            }
        }
        None
    }

    /// For every word, how many of the media it hits (any field).
    pub fn counts(&self, media: &HashMap<(Kind, i64), Vec<(&'static str, String)>>) -> Vec<usize> {
        let mut counts = vec![0usize; self.rules.len()];
        for fields in media.values() {
            let folded: Vec<String> = fields.iter().filter(|(_, t)| !t.trim().is_empty()).map(|(_, t)| fold(t)).collect();
            for (i, (_, m)) in self.rules.iter().enumerate() {
                if folded.iter().any(|t| m.is_match(t)) {
                    counts[i] += 1;
                }
            }
        }
        counts
    }

    /// The words in the order of the list.
    pub fn words(&self) -> Vec<&str> {
        self.rules.iter().map(|(w, _)| w.as_str()).collect()
    }

    /// The first line that one of the texts matches, as the administrators wrote it.
    pub fn hit<'a, I: IntoIterator<Item = &'a str>>(&self, texts: I) -> Option<&str> {
        if self.rules.is_empty() {
            return None;
        }
        for text in texts {
            if text.trim().is_empty() {
                continue;
            }
            let folded = fold(text);
            if let Some((line, _)) = self.rules.iter().find(|(_, m)| m.is_match(&folded)) {
                return Some(line);
            }
        }
        None
    }
}

/// What a run of the check did.
#[derive(Debug, Default, Clone, Serialize, PartialEq, Eq)]
pub struct BlacklistReport {
    /// Media that carry the mark after the run (without the ones an administrator approved).
    pub marked: usize,
    /// Marks put on in this run.
    pub added: usize,
    /// Marks taken off in this run because the medium does not hit any more.
    pub removed: usize,
}

/// One marked medium, for the administrators.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MarkRow {
    pub kind: String,
    pub id: i64,
    /// The line of the list that hit.
    pub word: String,
    /// The field that hit: `title`, `author`, `narrator`, `description`, `track`, `label` (album), `name` (station); empty for a mark from before the field was kept.
    pub field: String,
    pub approved: bool,
}

/// One word of the list as the administration shows it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WordRow {
    pub id: i64,
    pub pattern: String,
    pub note: String,
    /// `word`, `wildcard` or `regex`, from how it is written.
    pub kind: &'static str,
    pub created_at: i64,
    pub created_by: Option<String>,
    pub updated_at: i64,
    pub updated_by: Option<String>,
}

/// What saving a word came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WordOutcome {
    /// Saved; the media were checked again.
    Saved(BlacklistReport),
    /// Empty, too long, with a control character, a broken regular expression, or more words than allowed.
    Invalid,
    /// The same word is in the list already (case and accents do not count).
    Duplicate,
    /// There is no such word.
    Missing,
}

/// A medium that a word would hit, for the preview and the page of the hits.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PreviewHit {
    pub kind: String,
    pub id: i64,
    pub field: String,
}

/// How a word is written: a regular expression between slashes, a pattern with `*` or `?`, or a plain word.
pub fn word_kind(pattern: &str) -> &'static str {
    let p = pattern.trim();
    if p.len() > 1 && p.starts_with('/') && p.ends_with('/') {
        "regex"
    } else if p.contains('*') || p.contains('?') {
        "wildcard"
    } else {
        "word"
    }
}

/// A note is one line of at most 200 characters.
pub fn valid_note(note: &str) -> bool {
    note.chars().count() <= 200 && !note.chars().any(char::is_control)
}

/// Is this a usable word: 1 to [`MAX_LINE_CHARS`] characters, no control character, and it compiles?
pub fn valid_word(pattern: &str) -> bool {
    let p = pattern.trim();
    !p.is_empty() && p.chars().count() <= MAX_LINE_CHARS && !p.chars().any(char::is_control) && compile(&fold(p)).is_ok()
}

/// What a run looks at: everything, or one medium (after an edit).
#[derive(Debug, Clone, Copy)]
pub enum Scope {
    All,
    One(Kind, i64),
}

impl Store {
    /// The words of the list, in the order they were written.
    pub async fn blacklist_words(&self) -> Result<Vec<String>> {
        let rows = self.db.reader.query_all_raw(Statement::from_string(DbBackend::Sqlite, "SELECT pattern FROM blacklist_word ORDER BY id")).await?;
        rows.iter().map(|r| Ok(r.try_get_by_index(0)?)).collect()
    }

    /// The compiled list.
    pub async fn blacklist(&self) -> Result<Blacklist> {
        Ok(Blacklist::from_words(&self.blacklist_words().await?))
    }

    /// The words with their notes and audit fields, in the order they were written.
    pub async fn blacklist_entries(&self) -> Result<Vec<WordRow>> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_string(DbBackend::Sqlite, "SELECT id, pattern, note, created_at, created_by, updated_at, updated_by FROM blacklist_word ORDER BY id"))
            .await?;
        rows.iter()
            .map(|r| {
                let pattern: String = r.try_get_by_index(1)?;
                Ok(WordRow {
                    id: r.try_get_by_index(0)?,
                    kind: word_kind(&pattern),
                    pattern,
                    note: r.try_get_by_index(2)?,
                    created_at: r.try_get_by_index(3)?,
                    created_by: r.try_get_by_index(4)?,
                    updated_at: r.try_get_by_index(5)?,
                    updated_by: r.try_get_by_index(6)?,
                })
            })
            .collect()
    }

    /// Is a word in the list that is the same as `pattern` (case and accents do not count)? `except` is the row being edited.
    async fn blacklist_has(&self, pattern: &str, except: Option<i64>) -> Result<bool> {
        let key = fold(pattern.trim());
        Ok(self.blacklist_entries().await?.iter().any(|w| Some(w.id) != except && fold(&w.pattern) == key))
    }

    /// Adds a word with a note and checks the media again.
    pub async fn blacklist_add(&self, pattern: &str, note: &str) -> Result<WordOutcome> {
        let (pattern, note) = (pattern.trim(), note.trim());
        if !valid_word(pattern) || !valid_note(note) || self.blacklist_entries().await?.len() >= MAX_LINES {
            return Ok(WordOutcome::Invalid);
        }
        if self.blacklist_has(pattern, None).await? {
            return Ok(WordOutcome::Duplicate);
        }
        self.db.writer.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "INSERT INTO blacklist_word (pattern, note) VALUES (?, ?)", [pattern.into(), note.into()])).await?;
        Ok(WordOutcome::Saved(self.blacklist_check(Scope::All).await?))
    }

    /// Changes a word and its note and checks the media again.
    pub async fn blacklist_update(&self, id: i64, pattern: &str, note: &str) -> Result<WordOutcome> {
        let (pattern, note) = (pattern.trim(), note.trim());
        if self.db.reader.query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT 1 FROM blacklist_word WHERE id = ?", [id.into()])).await?.is_none() {
            return Ok(WordOutcome::Missing);
        }
        if !valid_word(pattern) || !valid_note(note) {
            return Ok(WordOutcome::Invalid);
        }
        if self.blacklist_has(pattern, Some(id)).await? {
            return Ok(WordOutcome::Duplicate);
        }
        self.db.writer.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE blacklist_word SET pattern = ?, note = ? WHERE id = ?", [pattern.into(), note.into(), id.into()])).await?;
        Ok(WordOutcome::Saved(self.blacklist_check(Scope::All).await?))
    }

    /// Removes a word and checks the media again (the marks only it set go, approved or not).
    pub async fn blacklist_delete(&self, id: i64) -> Result<WordOutcome> {
        let gone = self.db.writer.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "DELETE FROM blacklist_word WHERE id = ?", [id.into()])).await?;
        if gone.rows_affected() == 0 {
            return Ok(WordOutcome::Missing);
        }
        Ok(WordOutcome::Saved(self.blacklist_check(Scope::All).await?))
    }

    /// Makes the list exactly these words (a word that stays keeps its note) and checks the media again. `Err(Error::InvalidName)` if a word is not
    /// usable or the list is too long.
    pub async fn save_blacklist_words(&self, words: &[String]) -> Result<BlacklistReport> {
        let mut wanted: Vec<String> = Vec::new();
        for w in words.iter().map(|w| w.trim()).filter(|w| !w.is_empty()) {
            if !valid_word(w) {
                return Err(crate::error::Error::InvalidName);
            }
            if !wanted.iter().any(|x| fold(x) == fold(w)) {
                wanted.push(w.to_owned());
            }
        }
        if wanted.len() > MAX_LINES {
            return Err(crate::error::Error::InvalidName);
        }
        let current = self.blacklist_entries().await?;
        for old in &current {
            if !wanted.iter().any(|w| fold(w) == fold(&old.pattern)) {
                self.db.writer.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "DELETE FROM blacklist_word WHERE id = ?", [old.id.into()])).await?;
            }
        }
        for w in &wanted {
            if !current.iter().any(|old| fold(&old.pattern) == fold(w)) {
                self.db.writer.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "INSERT INTO blacklist_word (pattern) VALUES (?)", [w.clone().into()])).await?;
            }
        }
        self.blacklist_check(Scope::All).await
    }

    /// Checks the media against the list and brings the marks in line. Idempotent: a second run changes nothing. Prefetches everything it reads
    /// (a handful of queries, whatever the size of the library).
    pub async fn blacklist_check(&self, scope: Scope) -> Result<BlacklistReport> {
        let list = self.blacklist().await?;
        let mut hits: HashMap<(Kind, i64), (String, &'static str)> = HashMap::new();
        if !list.is_empty() {
            for (key, fields) in self.blacklist_texts(scope).await? {
                if let Some((word, field)) = list.hit_fields(&fields) {
                    hits.insert(key, (word.to_owned(), field));
                }
            }
        }
        // The rows as they are: the ones this run is about.
        let (clause, values): (String, Vec<Value>) = match scope {
            Scope::All => (String::new(), Vec::new()),
            Scope::One(kind, id) => (" WHERE kind = ? AND item_id = ?".to_owned(), vec![kind.as_str().into(), id.into()]),
        };
        let rows = self.db.reader.query_all_raw(Statement::from_sql_and_values(DbBackend::Sqlite, format!("SELECT id, kind, item_id, word, field, approved FROM blacklist_mark{clause}"), values)).await?;
        let kind_of = |s: &str| match s {
            "podcast" => Kind::Podcast,
            "station" => Kind::Station,
            _ => Kind::Book,
        };
        let mut report = BlacklistReport::default();
        let w = &self.db.writer;
        let mut seen: HashSet<(Kind, i64)> = HashSet::new();
        for r in &rows {
            let (row_id, kind, item, word, field, approved): (i64, String, i64, String, String, i64) =
                (r.try_get_by_index(0)?, r.try_get_by_index(1)?, r.try_get_by_index(2)?, r.try_get_by_index(3)?, r.try_get_by_index(4)?, r.try_get_by_index(5)?);
            let key = (kind_of(&kind), item);
            seen.insert(key);
            match hits.get(&key) {
                None => {
                    w.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "DELETE FROM blacklist_mark WHERE id = ?", [row_id.into()])).await?;
                    report.removed += 1;
                }
                Some((now, now_field)) => {
                    if *now != word || *now_field != field {
                        w.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE blacklist_mark SET word = ?, field = ? WHERE id = ?", [now.clone().into(), (*now_field).into(), row_id.into()])).await?;
                    }
                    if approved == 0 {
                        report.marked += 1;
                    }
                }
            }
        }
        for ((kind, item), (word, field)) in &hits {
            if !seen.contains(&(*kind, *item)) {
                w.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "INSERT INTO blacklist_mark (kind, item_id, word, field) VALUES (?, ?, ?, ?)", [kind.as_str().into(), (*item).into(), word.clone().into(), (*field).into()])).await?;
                report.added += 1;
                report.marked += 1;
            }
        }
        Ok(report)
    }

    /// The texts of the media of the scope that are checked, by field, in the order they are looked at.
    async fn blacklist_texts(&self, scope: Scope) -> Result<HashMap<(Kind, i64), Vec<(&'static str, String)>>> {
        let rd = &self.db.reader;
        let query = |sql: String, values: Vec<Value>| Statement::from_sql_and_values(DbBackend::Sqlite, sql, values);
        let only = |kind: Kind| match scope {
            Scope::One(k, id) if k == kind => Some(id),
            Scope::One(..) => Some(-1),
            Scope::All => None,
        };
        let mut media: HashMap<(Kind, i64), Vec<(&'static str, String)>> = HashMap::new();
        // Albums and audiobooks: their own text, the titles of the tracks and the labels (of the album and of its tracks).
        if only(Kind::Book) != Some(-1) {
            let (clause, values) = match only(Kind::Book) {
                Some(id) => (" WHERE b.id = ?", vec![Value::from(id)]),
                None => ("", vec![]),
            };
            const OWN: [&str; 4] = ["title", "author", "narrator", "description"];
            for r in rd.query_all_raw(query(format!("SELECT b.id, b.title, a.name, b.narrator, b.description FROM book b JOIN author a ON a.id = b.author_id{clause}"), values.clone())).await? {
                let id: i64 = r.try_get_by_index(0)?;
                let list: Vec<(&'static str, String)> = OWN.iter().enumerate().filter_map(|(i, name)| r.try_get_by_index::<Option<String>>(i + 1).ok().flatten().map(|t| (*name, t))).collect();
                media.insert((Kind::Book, id), list);
            }
            let track_clause = if values.is_empty() { "" } else { " WHERE book_id = ?" };
            for r in rd.query_all_raw(query(format!("SELECT book_id, title FROM track{track_clause}"), values.clone())).await? {
                let (id, title): (i64, String) = (r.try_get_by_index(0)?, r.try_get_by_index(1)?);
                media.entry((Kind::Book, id)).or_default().push(("track", title));
            }
            let mut labels = self.book_labels().await?;
            for (id, names) in self.track_labels_by_book().await? {
                labels.entry(id).or_default().extend(names);
            }
            for ((kind, id), fields) in media.iter_mut() {
                if *kind == Kind::Book {
                    fields.extend(labels.remove(id).unwrap_or_default().into_iter().map(|n| ("label", n)));
                }
            }
        }
        if only(Kind::Podcast) != Some(-1) {
            let (clause, values) = match only(Kind::Podcast) {
                Some(id) => (" WHERE id = ?", vec![Value::from(id)]),
                None => ("", vec![]),
            };
            let labels = self.podcast_labels().await?;
            for r in rd.query_all_raw(query(format!("SELECT id, title, description FROM feed{clause}"), values)).await? {
                let id: i64 = r.try_get_by_index(0)?;
                let mut fields: Vec<(&'static str, String)> = vec![("title", r.try_get_by_index(1)?), ("description", r.try_get_by_index(2)?)];
                fields.extend(labels.get(&id).cloned().unwrap_or_default().into_iter().map(|n| ("label", n)));
                media.insert((Kind::Podcast, id), fields);
            }
        }
        if only(Kind::Station) != Some(-1) {
            let (clause, values) = match only(Kind::Station) {
                Some(id) => (" WHERE id = ?", vec![Value::from(id)]),
                None => ("", vec![]),
            };
            let labels = self.station_labels().await?;
            for r in rd.query_all_raw(query(format!("SELECT id, name FROM station{clause}"), values)).await? {
                let id: i64 = r.try_get_by_index(0)?;
                let mut fields: Vec<(&'static str, String)> = vec![("name", r.try_get_by_index(1)?)];
                fields.extend(labels.get(&id).cloned().unwrap_or_default().into_iter().map(|n| ("label", n)));
                media.insert((Kind::Station, id), fields);
            }
        }
        Ok(media)
    }

    /// What a list of words WOULD mark, without writing anything (to judge false positives before saving). The approved state is not applied.
    pub async fn blacklist_preview(&self, words: &[String]) -> Result<Vec<MarkRow>> {
        let list = Blacklist::from_words(words);
        let mut rows: Vec<MarkRow> = Vec::new();
        if !list.is_empty() {
            for ((kind, id), fields) in self.blacklist_texts(Scope::All).await? {
                if let Some((word, field)) = list.hit_fields(&fields) {
                    rows.push(MarkRow { kind: kind.as_str().to_owned(), id, word: word.to_owned(), field: field.to_owned(), approved: false });
                }
            }
        }
        if words.iter().any(|w| !w.trim().is_empty() && !valid_word(w)) {
            return Err(crate::error::Error::InvalidName);
        }
        rows.sort_by(|a, b| (&a.kind, a.id).cmp(&(&b.kind, b.id)));
        Ok(rows)
    }

    /// The media ONE word would hit (the field that hit first), for the preview in the dialog of the word: sorted by kind and id. `Err(InvalidName)`
    /// if the word is not usable.
    pub async fn blacklist_preview_word(&self, pattern: &str) -> Result<Vec<PreviewHit>> {
        if !valid_word(pattern) {
            return Err(crate::error::Error::InvalidName);
        }
        let list = Blacklist::from_words(&[pattern.to_owned()]);
        let mut hits: Vec<PreviewHit> = Vec::new();
        for ((kind, id), fields) in self.blacklist_texts(Scope::All).await? {
            if let Some((_, field)) = list.hit_fields(&fields) {
                hits.push(PreviewHit { kind: kind.as_str().to_owned(), id, field: field.to_owned() });
            }
        }
        hits.sort_by(|a, b| (&a.kind, a.id).cmp(&(&b.kind, b.id)));
        Ok(hits)
    }

    /// For every word of the list (by id), how many media it hits.
    pub async fn blacklist_word_counts(&self) -> Result<HashMap<String, usize>> {
        let list = self.blacklist().await?;
        if list.is_empty() {
            return Ok(HashMap::new());
        }
        let counts = list.counts(&self.blacklist_texts(Scope::All).await?);
        Ok(list.words().into_iter().map(str::to_owned).zip(counts).collect())
    }

    /// The same for the callers that must not fail because of it: an error is logged.
    pub async fn blacklist_check_quietly(&self, scope: Scope) {
        if let Err(e) = self.blacklist_check(scope).await {
            tracing::warn!(error = %e, "the blacklist could not be checked");
        }
    }

    /// The ids of the media of a kind that carry the mark (not the approved ones).
    pub async fn blacklist_marked(&self, kind: Kind) -> Result<HashSet<i64>> {
        let rows = self.db.reader.query_all_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT item_id FROM blacklist_mark WHERE kind = ? AND approved = 0", [kind.as_str().into()])).await?;
        rows.iter().map(|r| Ok(r.try_get_by_index(0)?)).collect()
    }

    /// The labels of the albums as the administration pages show them: with the system mark on the marked ones.
    pub async fn book_labels_with_mark(&self) -> Result<HashMap<i64, Vec<String>>> {
        let mut labels = self.book_labels().await?;
        for id in self.blacklist_marked(Kind::Book).await? {
            labels.entry(id).or_default().insert(0, MARK.to_owned());
        }
        Ok(labels)
    }

    /// The same for the podcasts.
    pub async fn podcast_labels_with_mark(&self) -> Result<HashMap<i64, Vec<String>>> {
        let mut labels = self.podcast_labels().await?;
        for id in self.blacklist_marked(Kind::Podcast).await? {
            labels.entry(id).or_default().insert(0, MARK.to_owned());
        }
        Ok(labels)
    }

    /// The same for the radio stations.
    pub async fn station_labels_with_mark(&self) -> Result<HashMap<i64, Vec<String>>> {
        let mut labels = self.station_labels().await?;
        for id in self.blacklist_marked(Kind::Station).await? {
            labels.entry(id).or_default().insert(0, MARK.to_owned());
        }
        Ok(labels)
    }

    /// The list of the labels as the administration pages show it: the system mark comes first, with the number of media that carry it.
    pub async fn labels_with_mark(&self) -> Result<Vec<crate::labels::LabelRow>> {
        let mut rows = self.labels().await?;
        let (books, podcasts, stations) = (self.blacklist_marked(Kind::Book).await?.len() as i64, self.blacklist_marked(Kind::Podcast).await?.len() as i64, self.blacklist_marked(Kind::Station).await?.len() as i64);
        if books + podcasts + stations > 0 {
            rows.insert(0, crate::labels::LabelRow { name: MARK.to_owned(), books, tracks: 0, podcasts, stations });
        }
        Ok(rows)
    }

    /// All marks with the word and the field that hit, the approved ones included, for the administrators.
    pub async fn blacklist_rows(&self) -> Result<Vec<MarkRow>> {
        let rows = self.db.reader.query_all_raw(Statement::from_string(DbBackend::Sqlite, "SELECT kind, item_id, word, field, approved FROM blacklist_mark ORDER BY kind, item_id")).await?;
        rows.iter().map(|r| Ok(MarkRow { kind: r.try_get_by_index(0)?, id: r.try_get_by_index(1)?, word: r.try_get_by_index(2)?, field: r.try_get_by_index(3)?, approved: r.try_get_by_index::<i64>(4)? == 1 })).collect()
    }

    /// An administrator puts the mark back on a medium they had approved. `false` if it was not approved.
    pub async fn blacklist_unapprove(&self, kind: Kind, id: i64) -> Result<bool> {
        let res = self.db.writer.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE blacklist_mark SET approved = 0 WHERE kind = ? AND item_id = ? AND approved = 1", [kind.as_str().into(), id.into()])).await?;
        Ok(res.rows_affected() > 0)
    }

    /// An administrator takes the mark off a medium: it stays off while the medium hits. `false` if it carried no mark.
    pub async fn blacklist_approve(&self, kind: Kind, id: i64) -> Result<bool> {
        let res = self.db.writer.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE blacklist_mark SET approved = 1 WHERE kind = ? AND item_id = ? AND approved = 0", [kind.as_str().into(), id.into()])).await?;
        Ok(res.rows_affected() > 0)
    }

    /// The scope of a track: the check of the album it belongs to.
    pub async fn blacklist_check_track_quietly(&self, track_id: i64) {
        if let Ok(Some(book)) = self.book_of_track(track_id).await {
            self.blacklist_check_quietly(Scope::One(Kind::Book, book)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_matches_inside_the_text_without_regard_to_case_and_accents() {
        let l = Blacklist::parse("katze\nÜberfall").unwrap();
        assert_eq!(l.hit(["Katzenjammer"]), Some("katze"));
        assert_eq!(l.hit(["Der UEBERFALL und uberfall"]), Some("Überfall"), "accents are folded, not spelled out");
        assert_eq!(l.hit(["Hund und Maus"]), None);
    }

    #[test]
    fn wildcards_and_regular_expressions_work_as_in_the_search_and_stay_inside_one_field() {
        let l = Blacklist::parse("mit * katze\nfoo?bar\n/^x+$/").unwrap();
        assert!(l.hit(["Sex mit meiner Katze"]).is_some());
        assert!(l.hit(["fooXbar"]).is_some());
        assert!(l.hit(["xxx"]).is_some());
        assert!(l.hit(["foobar"]).is_none());
        assert!(l.hit(["Sex mit", "meiner Katze"]).is_none(), "a pattern does not reach from one field into the next");
    }

    #[test]
    fn an_empty_list_hits_nothing_and_a_broken_line_is_refused() {
        assert!(Blacklist::parse("").unwrap().hit(["anything"]).is_none());
        assert!(Blacklist::parse("/(/").is_err());
        assert!(Blacklist::parse(&"x\n".repeat(MAX_LINES + 1)).is_err());
        assert!(Blacklist::lenient("/(/\nok").hit(["ok"]).is_some());
    }

    #[test]
    fn the_reserved_name_is_found_in_any_spelling_and_split_off() {
        assert!(is_mark("blacklist") && is_mark("  BLACKLIST ") && !is_mark("Black"));
        let (rest, had) = split_mark(&["Gebet".into(), "blacklist".into()]);
        assert_eq!((rest, had), (vec!["Gebet".to_owned()], true));
        assert_eq!(with_mark(vec!["Gebet".into()], true, true), vec!["Blacklist".to_owned(), "Gebet".to_owned()]);
        assert_eq!(with_mark(vec!["Gebet".into()], true, false), vec!["Gebet".to_owned()]);
    }
}
