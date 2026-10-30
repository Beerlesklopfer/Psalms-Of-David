// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Duplicates in the library (project owner, 2026-10-28: "Kinder der Freiheit ist doppelt, wir
//! brauchen eine Dupes-Erkennung"). The same work can sit in the library twice: ripped twice, once
//! under the author and once under the reader, once with its CD numbers in the title. Books whose
//! titles are the same after a clean-up are put in a group, and the group says what else matches.
//! Nothing is removed: the people who know the library decide which copy stays.

use serde::Serialize;

use crate::store::AdminBookRow;

/// The shortest title that may match as a part of another one (`Winter der Welt` in `Ken Follett - Winter der Welt`).
const MIN_PART: usize = 10;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DupeBook {
    pub id: i64,
    pub kind: String,
    pub title: String,
    pub author: String,
    pub narrator: Option<String>,
    pub discs: i64,
    pub tracks: i64,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Group {
    pub books: Vec<DupeBook>,
    /// The authors (or reader and author, if one was filed under the other) are the same.
    pub same_author: bool,
    /// The running times differ by less than a tenth.
    pub same_length: bool,
    /// The titles are the same (without CD numbers and case).
    pub same_title: bool,
    /// The albums have the same number of tracks with the same running times, track by track (a second
    /// each way): the same recording, whatever the titles say. This can be wanted on purpose (a school
    /// set and a family set); a rule from `ignore` says so.
    pub same_tracks: bool,
    /// `copies` (the same work more than once), `parts` (the CDs of one work: every book has its own
    /// CD number, so they belong together and are not duplicates) or `mixed` (CD sets that exist twice).
    pub kind: &'static str,
    /// For copies: `certain` when the author and the length agree too, else `possible`.
    pub confidence: &'static str,
}

/// A title as it is compared: no case, no accents of German, no brackets, no CD number, only letters and digits.
fn key(title: &str) -> String {
    let mut s = crate::relocate::strip_disc(title).0.to_lowercase();
    s = s.replace('ä', "ae").replace('ö', "oe").replace('ü', "ue").replace('ß', "ss");
    let mut out = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            c if depth == 0 && c.is_alphanumeric() => out.push(c),
            _ => {}
        }
    }
    out
}

/// A title or name as it is compared (also for the rules of `ignore`).
pub fn title_key(s: &str) -> String {
    key(s)
}

/// A name as it is compared: letters and digits only.
fn name(s: &str) -> String {
    key(s)
}

fn alike(a: &str, b: &str) -> bool {
    !a.is_empty() && !b.is_empty() && (a == b || (a.len().min(b.len()) >= MIN_PART && (a.contains(b) || b.contains(a))))
}

/// The groups of books that look like the same work, biggest trouble first (certain ones first).
pub fn find(rows: &[AdminBookRow]) -> Vec<Group> {
    find_without(rows, &[])
}

/// Like `find`, without the books that a rule hides from the duplicates (wanted on purpose).
pub fn find_without(all: &[AdminBookRow], rules: &[crate::ignore::Rule]) -> Vec<Group> {
    find_with_tracks(all, rules, &std::collections::HashMap::new())
}

/// The running time of every track in seconds, in playing order, for each book (`id` -> times).
pub type Tracks = std::collections::HashMap<i64, Vec<i64>>;

/// Albums with the same tracks: at least three, the same number, each running time within a second.
fn same_recording(a: Option<&Vec<i64>>, b: Option<&Vec<i64>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.len() >= 3 && a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() <= 1),
        _ => false,
    }
}

/// The groups, found by title and also by the combination of the tracks and their lengths.
pub fn find_with_tracks(all: &[AdminBookRow], rules: &[crate::ignore::Rule], tracks: &Tracks) -> Vec<Group> {
    let rows: Vec<AdminBookRow> = all.iter().filter(|r| !crate::ignore::ignored(rules, r, crate::ignore::DUPLICATE)).cloned().collect();
    let rows = rows.as_slice();
    let keys: Vec<String> = rows.iter().map(|r| key(&r.title)).collect();
    // Union-find over the books that match a title.
    let mut parent: Vec<usize> = (0..rows.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for i in 0..rows.len() {
        for j in i + 1..rows.len() {
            if alike(&keys[i], &keys[j]) || same_recording(tracks.get(&rows[i].id), tracks.get(&rows[j].id)) {
                let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                parent[a] = b;
            }
        }
    }
    let mut by_root: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
    for i in 0..rows.len() {
        let r = root(&mut parent, i);
        by_root.entry(r).or_default().push(i);
    }
    let mut groups: Vec<Group> = by_root
        .into_values()
        .filter(|m| m.len() > 1)
        .map(|members| {
            let books: Vec<DupeBook> = members
                .iter()
                .map(|&i| {
                    let r = &rows[i];
                    DupeBook { id: r.id, kind: r.kind.clone(), title: r.title.clone(), author: r.author.clone(), narrator: r.narrator.clone(), discs: r.discs, tracks: r.tracks, duration_ms: r.duration_ms }
                })
                .collect();
            // Somebody is the author of every copy, or the reader of one and the author of another.
            let people = |b: &DupeBook| -> Vec<String> { [Some(name(&b.author)), b.narrator.as_deref().map(name)].into_iter().flatten().filter(|n| !n.is_empty()).collect() };
            let first = people(&books[0]);
            let same_author = books[1..].iter().all(|b| people(b).iter().any(|n| first.iter().any(|f| alike(f, n) || f == n || n.contains(f.as_str()) || f.contains(n.as_str()))));
            let (min, max) = books.iter().fold((i64::MAX, 0), |(lo, hi), b| (lo.min(b.duration_ms), hi.max(b.duration_ms)));
            let same_length = max > 0 && (max - min) * 10 <= max;
            let same_title = members.iter().all(|&i| alike(&keys[members[0]], &keys[i]));
            let same_tracks = members.iter().all(|&i| same_recording(tracks.get(&rows[members[0]].id), tracks.get(&rows[i].id)));
            let numbers: Vec<Option<i64>> = books.iter().map(|b| crate::relocate::strip_disc(&b.title).1).collect();
            let distinct: std::collections::HashSet<i64> = numbers.iter().flatten().copied().collect();
            let kind = if numbers.iter().all(Option::is_some) && distinct.len() == numbers.len() {
                "parts"
            } else if distinct.len() > 1 {
                "mixed"
            } else {
                "copies"
            };
            // The same recording under the same title, or the same author and length: a copy for sure.
            let certain = (same_tracks && same_title) || (same_author && same_length && same_title);
            let confidence = if kind == "parts" { "parts" } else if certain { "certain" } else { "possible" };
            Group { books, same_author, same_length, same_title, same_tracks, kind, confidence }
        })
        .collect();
    groups.sort_by_key(|g| (g.kind == "parts", g.confidence != "certain", g.books[0].title.to_lowercase()));
    groups
}

impl crate::store::Store {
    /// The running time of each track in seconds, in playing order, for every book.
    pub async fn track_times(&self) -> crate::error::Result<Tracks> {
        use sea_orm::{ConnectionTrait, DbBackend, Statement};
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_string(DbBackend::Sqlite, "SELECT book_id, (duration_ms + 500) / 1000 FROM track ORDER BY book_id, disc, number"))
            .await?;
        let mut out: Tracks = Tracks::new();
        for r in rows {
            out.entry(r.try_get_by_index(0)?).or_default().push(r.try_get_by_index(1)?);
        }
        Ok(out)
    }

    /// The duplicate groups of the library as they are now: by title and by tracks, without what a rule hides.
    pub async fn duplicate_groups(&self) -> crate::error::Result<Vec<Group>> {
        let rows = self.admin_book_rows().await?;
        let rules = self.ignore_rules().await?;
        let tracks = self.track_times().await?;
        Ok(find_with_tracks(&rows, &rules, &tracks))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: i64, title: &str, author: &str, narrator: Option<&str>, minutes: i64) -> AdminBookRow {
        AdminBookRow {
            id,
            discs_total: None,
            barcode: None,
            kind: "audiobook".into(),
            folder: format!("{id}"),
            title: title.into(),
            author: author.into(),
            narrator: narrator.map(str::to_owned),
            genre: None,
            genre_id: None,
            genre_locked: false,
            genre_tag: None,
            released: false,
            min_age: None,
            recommended_age: None,
            quota_exempt: false,
            description: None,
            tracks: 10,
            discs: 1,
            owner_id: None,
            owner: None,
            duration_ms: minutes * 60_000,
        }
    }

    #[test]
    fn the_same_title_under_the_author_and_under_the_reader_is_one_group() {
        // The case of the library: "Kinder der Freiheit" by Follett and by its reader, 14 h 49 and 7 h 19.
        let rows = vec![
            row(1, "Winter der Welt", "Follett", Some("Philipp Schepmann"), 869),
            row(2, "Kinder der Freiheit", "Follett", Some("Johannes Steck"), 889),
            row(3, "Kinder der Freiheit", "Johannes Steck", Some("Johannes Steck"), 439),
        ];
        let groups = find(&rows);
        assert_eq!(groups.len(), 1);
        let g = &groups[0];
        assert_eq!(g.books.iter().map(|b| b.id).collect::<Vec<_>>(), vec![2, 3]);
        assert!(g.same_author && !g.same_length, "{g:?}");
        assert_eq!(g.confidence, "possible");
    }

    #[test]
    fn copies_of_the_same_length_are_certain_and_cd_numbers_do_not_matter() {
        let rows = vec![
            row(1, "Die Vermessung der Welt (CD 1)", "Daniel Kehlmann", None, 70),
            row(2, "Die Vermessung der Welt CD01", "Daniel Kehlmann", None, 72),
            row(3, "Die Vermessung der Welt (CD 2)", "Daniel Kehlmann", None, 70),
            row(4, "Das Geheimnis des Siegelrings", "Die Rotsteinkids", None, 90),
            row(5, "Das Geheimnis der Totenmaske", "Die Rotsteinkids", None, 90),
        ];
        let groups = find(&rows);
        assert_eq!(groups.len(), 1, "{groups:?}");
        assert_eq!(groups[0].books.len(), 3);
        // CD 1 twice (once with the number in the title, once as CD01): copies of a set, 3 books.
        assert_eq!(groups[0].kind, "mixed");
    }

    #[test]
    fn the_cds_of_one_work_are_parts_and_two_copies_of_an_album_are_copies() {
        let parts = vec![row(1, "Klassenhits [Disc 1]", "Schule", None, 63), row(2, "Klassenhits [Disc 2]", "Schule", None, 54)];
        assert_eq!((find(&parts)[0].kind, find(&parts)[0].confidence), ("parts", "parts"));
        let copies = vec![row(1, "Ein Musikbilderbuch", "Schule", None, 48), row(2, "Ein Musikbilderbuch-Vivaldi", "Schule", None, 48)];
        let g = &find(&copies)[0];
        assert_eq!((g.kind, g.confidence), ("copies", "certain"));
    }

    #[test]
    fn the_same_tracks_under_different_titles_are_found_and_a_rule_can_keep_them() {
        let rows = vec![
            row(1, "06_Das Geheimnis des verschwundenen Manuskripts", "Die Rotsteinkids", None, 79),
            row(2, "Das Geheimnis des Siegelrings", "Die Rotsteinkids", None, 79),
            row(3, "Etwas anderes", "Die Rotsteinkids", None, 79),
        ];
        let tracks: Tracks = [(1, vec![200, 310, 240, 255]), (2, vec![200, 311, 240, 255]), (3, vec![200, 400, 240, 255])].into_iter().collect();
        let groups = find_with_tracks(&rows, &[], &tracks);
        assert_eq!(groups.len(), 1, "{groups:?}");
        let g = &groups[0];
        assert_eq!(g.books.iter().map(|b| b.id).collect::<Vec<_>>(), vec![1, 2], "within a second per track: the third differs");
        assert!(g.same_tracks && !g.same_title, "{g:?}");
        assert_eq!(g.confidence, "possible", "other titles: it may be wanted, the owner decides");
        // Too few tracks prove nothing.
        let two: Tracks = [(1, vec![200, 310]), (2, vec![200, 310])].into_iter().collect();
        assert!(find_with_tracks(&rows[..2], &[], &two).is_empty());
    }

    #[test]
    fn short_or_different_titles_do_not_match() {
        let rows = vec![row(1, "Ginger", "A", None, 10), row(2, "Ginger 2-1", "A", None, 10), row(3, "Abba", "A", None, 10), row(4, "Abba Gold", "A", None, 10)];
        assert!(find(&rows).is_empty());
        let rows = vec![row(1, "Winter der Welt", "Follett", None, 800), row(2, "Ken Follett - Winter der Welt", "Follett", None, 800)];
        assert_eq!(find(&rows).len(), 1, "a title that contains the other one matches");
    }
}
