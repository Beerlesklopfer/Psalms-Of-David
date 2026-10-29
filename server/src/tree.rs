// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The tree of the media lists (project owner, 2026-10-28: "wir brauchen eine Treeview, und deren
//! Hierarchie muss in den Settings eingestellt werden"). A setting names the levels above the
//! albums, for the audiobooks and for the music separately:
//!
//! * `flat`          no levels, one list
//! * `author`        the author or artist, then the album
//! * `folder`        the folders on the disk below the import folder, as they are (`NORA/Schule/Marko Simsa`)
//! * `letter_author` the first letter, the author or artist, then the album

use serde::Serialize;

use crate::store::AdminBookRow;

pub const MODES: [&str; 4] = ["flat", "author", "folder", "letter_author"];

pub fn valid(mode: &str) -> bool {
    MODES.contains(&mode)
}

/// One line of the list: a node of the tree (a level with a name) or a book.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Item {
    Node { id: usize, depth: usize, name: String, books: usize, parents: String },
    Book { book: Box<AdminBookRow>, depth: usize, parents: String },
}

/// A name as it is sorted: lower case, accents of German and French folded.
fn fold(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .flat_map(|c| match c {
            'ä' | 'á' | 'à' | 'â' => vec!['a'],
            'ö' | 'ó' | 'ò' | 'ô' => vec!['o'],
            'ü' | 'ú' | 'ù' | 'û' => vec!['u'],
            'é' | 'è' | 'ê' | 'ë' => vec!['e'],
            'í' | 'î' | 'ï' => vec!['i'],
            'ç' => vec!['c'],
            'ß' => vec!['s', 's'],
            c => vec![c],
        })
        .collect()
}

/// The names of the levels above a book.
fn path(row: &AdminBookRow, mode: &str) -> Vec<String> {
    let author = row.author.trim().to_owned();
    match mode {
        "author" => vec![author],
        "letter_author" => {
            let first = fold(&author).chars().next().map(|c| c.to_uppercase().collect::<String>()).filter(|l| l.chars().all(char::is_alphabetic)).unwrap_or_else(|| "#".to_owned());
            vec![first, author]
        }
        "folder" => {
            let mut parts: Vec<String> = row.folder.split('/').filter(|p| !p.is_empty()).map(str::to_owned).collect();
            parts.pop(); // the album itself is the leaf
            parts
        }
        _ => Vec::new(),
    }
}

/// Arranges the books as a tree: levels as nodes in sorted order, each book below its levels.
pub fn build(rows: Vec<AdminBookRow>, mode: &str) -> Vec<Item> {
    let mut keyed: Vec<(Vec<String>, AdminBookRow)> = rows.into_iter().map(|r| (path(&r, mode), r)).collect();
    keyed.sort_by(|a, b| {
        let folded = |p: &[String]| p.iter().map(|s| fold(s)).collect::<Vec<_>>();
        // The same name in another case is one node, named as its first book spells it.
        folded(&a.0).cmp(&folded(&b.0)).then_with(|| fold(&a.1.title).cmp(&fold(&b.1.title)))
    });
    let mut items: Vec<Item> = Vec::new();
    // The open nodes: their names and ids, from the top.
    let mut open: Vec<(String, usize)> = Vec::new();
    let mut next_id = 0;
    for (levels, book) in keyed {
        // How much of the open path this book shares.
        let shared = open.iter().zip(&levels).take_while(|((name, _), level)| fold(name) == fold(level)).count();
        open.truncate(shared);
        for (depth, name) in levels.iter().enumerate().skip(shared) {
            let parents = open.iter().map(|(_, id)| format!("n{id}")).collect::<Vec<_>>().join(" ");
            items.push(Item::Node { id: next_id, depth, name: name.clone(), books: 0, parents });
            open.push((name.clone(), next_id));
            next_id += 1;
        }
        let parents = open.iter().map(|(_, id)| format!("n{id}")).collect::<Vec<_>>().join(" ");
        items.push(Item::Book { book: Box::new(book), depth: levels.len(), parents });
    }
    // The number of books below every node.
    let mut counts = vec![0usize; next_id];
    for item in &items {
        if let Item::Book { parents, .. } = item {
            for id in parents.split(' ').filter_map(|p| p.strip_prefix('n')).filter_map(|n| n.parse::<usize>().ok()) {
                counts[id] += 1;
            }
        }
    }
    for item in &mut items {
        if let Item::Node { id, books, .. } = item {
            *books = counts[*id];
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: i64, folder: &str, title: &str, author: &str) -> AdminBookRow {
        AdminBookRow {
            id,
            kind: "music".into(),
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

    fn shape(items: &[Item]) -> Vec<String> {
        items
            .iter()
            .map(|i| match i {
                Item::Node { depth, name, books, .. } => format!("{}{name} ({books})", "  ".repeat(*depth)),
                Item::Book { book, depth, .. } => format!("{}- {}", "  ".repeat(*depth), book.title),
            })
            .collect()
    }

    #[test]
    fn the_folders_on_the_disk_become_the_levels() {
        let rows = vec![
            row(1, "NORA/Chor/Martin Luther King/Ein Traum", "Ein Traum", "Martin Luther King"),
            row(2, "NORA/Schule/Marko Simsa/Paukenschlag", "Paukenschlag", "Marko Simsa"),
            row(3, "NORA/Chor/Martin Luther King/Sopran", "Sopran", "Martin Luther King"),
            row(4, "Einzeln", "Einzeln", "X"),
        ];
        assert_eq!(
            shape(&build(rows, "folder")),
            ["- Einzeln", "NORA (3)", "  Chor (2)", "    Martin Luther King (2)", "      - Ein Traum", "      - Sopran", "  Schule (1)", "    Marko Simsa (1)", "      - Paukenschlag"]
        );
    }

    #[test]
    fn the_author_is_the_level_and_a_letter_can_come_above_it() {
        let rows = vec![row(1, "a/b/Winter", "Winter", "Follett"), row(2, "a/b/Kinder", "Kinder", "follett"), row(3, "a/Hobbit", "Hobbit", "Tolkien"), row(4, "a/Z", "Z", "Étienne")];
        assert_eq!(shape(&build(rows.clone(), "author")), ["Étienne (1)", "  - Z", "follett (2)", "  - Kinder", "  - Winter", "Tolkien (1)", "  - Hobbit"]);
        let by_letter = shape(&build(rows.clone(), "letter_author"));
        assert_eq!(by_letter[0], "E (1)", "accents are folded for the letter");
        assert!(by_letter.contains(&"F (2)".to_owned()) && by_letter.contains(&"  follett (2)".to_owned()), "{by_letter:?}");
        assert_eq!(shape(&build(rows, "flat")).len(), 4, "flat has no nodes");
    }

    #[test]
    fn a_book_knows_its_nodes_for_collapsing() {
        let items = build(vec![row(1, "A/B/C/D", "D", "x")], "folder");
        let Item::Book { parents, depth, .. } = items.last().unwrap() else { panic!() };
        assert_eq!((parents.as_str(), *depth), ("n0 n1 n2", 3));
    }
}
