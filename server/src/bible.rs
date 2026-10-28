// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Opening an audio Bible at a passage.
//!
//! A reference as typed or dictated ("Johannes 3,16", "1 Cor 13:4-7", "Psalm 23")
//! is parsed into a book number, chapter and verses. The anchors of an item
//! (`bible_anchor`) say where in its files each verse begins; resolving a reference
//! gives the part and position to start at. See `docs/spec-bible.md`.

use sea_orm::{ConnectionTrait, DbBackend, FromQueryResult, Statement, TransactionTrait, Value};
use serde::Serialize;

use crate::error::{Error, Result};
use crate::store::{Store, VISIBLE};

/// A parsed reference. `book` counts from 1 (Genesis) to 66 (Revelation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Reference {
    pub book: u8,
    pub chapter: u16,
    pub verse: Option<u16>,
    pub verse_to: Option<u16>,
}

/// The names of the 66 books after normalizing (see `normalize`): German (Luther
/// and Einheitsübersetzung abbreviations) and English. Books with a number are
/// written `1kor`: the number first, then the name.
const BOOKS: [(u8, &[&str]); 66] = [
    (1, &["genesis", "gen", "gn", "1mose", "1mo", "1mos", "1moses"]),
    (2, &["exodus", "ex", "exod", "2mose", "2mo", "2mos"]),
    (3, &["leviticus", "lev", "lv", "3mose", "3mo", "3mos"]),
    (4, &["numeri", "num", "numbers", "nm", "4mose", "4mo", "4mos"]),
    (5, &["deuteronomium", "dtn", "deut", "deuteronomy", "5mose", "5mo", "5mos"]),
    (6, &["josua", "jos", "joshua"]),
    (7, &["richter", "ri", "judges", "judg"]),
    (8, &["rut", "ruth"]),
    (9, &["1samuel", "1sam", "1sa"]),
    (10, &["2samuel", "2sam", "2sa"]),
    (11, &["1konige", "1kon", "1kings", "1ki"]),
    (12, &["2konige", "2kon", "2kings", "2ki"]),
    (13, &["1chronik", "1chr", "1chronicles"]),
    (14, &["2chronik", "2chr", "2chronicles"]),
    (15, &["esra", "ezra"]),
    (16, &["nehemia", "neh", "nehemiah"]),
    (17, &["ester", "est", "esther"]),
    (18, &["hiob", "ijob", "job", "hi"]),
    (19, &["psalm", "psalmen", "ps", "psalms", "psa"]),
    (20, &["spruche", "spr", "sprichworter", "proverbs", "prov", "pro"]),
    (21, &["prediger", "pred", "kohelet", "koh", "ecclesiastes", "eccl"]),
    (22, &["hohelied", "hld", "hl", "songsolomon", "songsongs", "song", "sos"]),
    (23, &["jesaja", "jes", "isaiah", "isa"]),
    (24, &["jeremia", "jer", "jeremiah"]),
    (25, &["klagelieder", "klgl", "kla", "lamentations", "lam"]),
    (26, &["hesekiel", "hes", "ezechiel", "ez", "ezekiel", "ezek"]),
    (27, &["daniel", "dan"]),
    (28, &["hosea", "hos"]),
    (29, &["joel"]),
    (30, &["amos"]),
    (31, &["obadja", "obd", "ob", "obadiah"]),
    (32, &["jona", "jon", "jonah"]),
    (33, &["micha", "mi", "mic", "micah"]),
    (34, &["nahum", "nah"]),
    (35, &["habakuk", "hab", "habakkuk"]),
    (36, &["zefanja", "zef", "zephaniah", "zeph"]),
    (37, &["haggai", "hag"]),
    (38, &["sacharja", "sach", "zechariah", "zech"]),
    (39, &["maleachi", "mal", "malachi"]),
    (40, &["matthaus", "mt", "matt", "mat", "matthew"]),
    (41, &["markus", "mk", "mar", "mark"]),
    (42, &["lukas", "lk", "luk", "luke"]),
    (43, &["johannes", "joh", "jn", "john"]),
    (44, &["apostelgeschichte", "apg", "act", "acts"]),
    (45, &["romer", "rom", "romans"]),
    (46, &["1korinther", "1kor", "1corinthians", "1cor"]),
    (47, &["2korinther", "2kor", "2corinthians", "2cor"]),
    (48, &["galater", "gal", "galatians"]),
    (49, &["epheser", "eph", "ephesians"]),
    (50, &["philipper", "phil", "philippians"]),
    (51, &["kolosser", "kol", "colossians", "col"]),
    (52, &["1thessalonicher", "1thess", "1th", "1thessalonians"]),
    (53, &["2thessalonicher", "2thess", "2th", "2thessalonians"]),
    (54, &["1timotheus", "1tim", "1timothy"]),
    (55, &["2timotheus", "2tim", "2timothy"]),
    (56, &["titus", "tit"]),
    (57, &["philemon", "phlm", "phm"]),
    (58, &["hebraer", "hebr", "heb", "hebrews"]),
    (59, &["jakobus", "jak", "jas", "james"]),
    (60, &["1petrus", "1petr", "1pet", "1peter"]),
    (61, &["2petrus", "2petr", "2pet", "2peter"]),
    (62, &["1johannes", "1joh", "1jn", "1john"]),
    (63, &["2johannes", "2joh", "2jn", "2john"]),
    (64, &["3johannes", "3joh", "3jn", "3john"]),
    (65, &["judas", "jud", "jude"]),
    (66, &["offenbarung", "offb", "off", "apokalypse", "offenbarungjohannes", "revelation", "rev"]),
];

/// Words that may stand between the name and the numbers, or that a speech
/// recognizer adds ("Johannes Kapitel 3 Vers 16").
const FILLER: &[&str] = &["kapitel", "kap", "chapter", "ch", "vers", "verse", "vs", "v", "bis", "to", "buch", "book", "des", "der", "von", "the", "of"];

/// "first", "second", "third" in the two languages, as the number of a book.
fn spoken_ordinal(word: &str) -> Option<char> {
    match word {
        "erste" | "erster" | "ersten" | "first" | "i" => Some('1'),
        "zweite" | "zweiter" | "zweiten" | "second" | "ii" => Some('2'),
        "dritte" | "dritter" | "dritten" | "third" | "iii" => Some('3'),
        _ => None,
    }
}

/// Lower case, without umlauts and ß, so that "Römer" and "Roemer"-less typing both
/// match (`romer`). Spoken text has no marks to lose.
fn normalize(word: &str) -> String {
    word.chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| match c {
            'ä' => 'a',
            'ö' => 'o',
            'ü' => 'u',
            c => c,
        })
        .collect::<String>()
        .replace('ß', "ss")
}

enum Token {
    Word(String),
    Number(u32),
}

fn tokens(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut digits = String::new();
    let flush = |word: &mut String, digits: &mut String, out: &mut Vec<Token>| {
        if !word.is_empty() {
            out.push(Token::Word(normalize(word)));
            word.clear();
        }
        if !digits.is_empty() {
            if let Ok(n) = digits.parse() {
                out.push(Token::Number(n));
            }
            digits.clear();
        }
    };
    for c in text.chars() {
        if c.is_ascii_digit() {
            if !word.is_empty() {
                flush(&mut word, &mut digits, &mut out);
            }
            digits.push(c);
        } else if c.is_alphabetic() {
            if !digits.is_empty() {
                flush(&mut word, &mut digits, &mut out);
            }
            word.push(c);
        } else {
            flush(&mut word, &mut digits, &mut out);
        }
    }
    flush(&mut word, &mut digits, &mut out);
    out
}

/// Parses a reference. `None` if the book is unknown or a number is missing.
pub fn parse(text: &str) -> Option<Reference> {
    let toks = tokens(text);
    let mut i = 0;
    // A leading number (1, 2, 3) or a spoken one belongs to the name.
    let mut name = String::new();
    match toks.first()? {
        Token::Number(n @ 1..=3) if matches!(toks.get(1), Some(Token::Word(_))) => {
            name.push_str(&n.to_string());
            i = 1;
        }
        Token::Word(w) => {
            if let (Some(d), Some(Token::Word(_))) = (spoken_ordinal(w), toks.get(1)) {
                name.push(d);
                i = 1;
            }
        }
        Token::Number(_) => return None,
    }
    while let Some(Token::Word(w)) = toks.get(i) {
        if !FILLER.contains(&w.as_str()) {
            name.push_str(w);
        }
        i += 1;
    }
    let book = BOOKS.iter().find(|(_, names)| names.contains(&name.as_str()))?.0;
    let mut numbers = toks[i..].iter().filter_map(|t| match t {
        Token::Number(n) => u16::try_from(*n).ok(),
        Token::Word(_) => None,
    });
    let chapter = numbers.next().filter(|c| *c >= 1)?;
    let verse = numbers.next().filter(|v| *v >= 1);
    let verse_to = numbers.next().filter(|v| *v >= 1 && verse.is_some_and(|f| *v >= f));
    Some(Reference { book, chapter, verse, verse_to })
}

/// One verse position inside an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anchor {
    pub track_id: i64,
    pub bible_book: u8,
    pub chapter: u16,
    pub verse: u16,
    pub position_ms: i64,
}

/// Where a reference leads in one item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, FromQueryResult)]
pub struct BibleHit {
    pub book_id: i64,
    pub title: String,
    pub track_id: i64,
    pub position_ms: i64,
    /// The verse the position belongs to: the one asked for, or the nearest one
    /// before it that the item has an anchor for.
    pub matched_verse: i64,
    pub exact: bool,
}

impl Store {
    /// Replaces the anchors of an item. Every anchor must point to a track of the
    /// item. Used by the import of an audio Bible.
    pub async fn set_bible_anchors(&self, book_id: i64, anchors: &[Anchor]) -> Result<()> {
        let txn = self.db.writer.begin().await?;
        txn.execute_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM bible_anchor WHERE book_id = ?",
            [Value::from(book_id)],
        ))
        .await?;
        for a in anchors {
            let belongs = txn
                .query_one_raw(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT 1 FROM track WHERE id = ? AND book_id = ?",
                    [Value::from(a.track_id), Value::from(book_id)],
                ))
                .await?
                .is_some();
            if !belongs {
                return Err(Error::WriteFailed(format!("anchor points to track {} which is not part of item {book_id}", a.track_id)));
            }
            let values: [Value; 6] = [
                book_id.into(),
                a.track_id.into(),
                i64::from(a.bible_book).into(),
                i64::from(a.chapter).into(),
                i64::from(a.verse).into(),
                a.position_ms.into(),
            ];
            txn.execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO bible_anchor (book_id, track_id, bible_book, chapter, verse, position_ms) VALUES (?, ?, ?, ?, ?, ?)",
                values,
            ))
            .await
            .map_err(Error::from_db)?;
        }
        txn.commit().await?;
        Ok(())
    }

    /// The items this user may see that can open `reference`, each with the part and
    /// position to start at. A verse the item has no anchor for leads to the nearest
    /// one before it in the chapter. Only items with anchors for the chapter appear.
    pub async fn resolve_bible(&self, user_id: i64, reference: Reference) -> Result<Vec<BibleHit>> {
        let verse = i64::from(reference.verse.unwrap_or(1));
        let sql = format!(
            "SELECT b.id AS book_id, b.title, a.track_id, a.position_ms, a.verse AS matched_verse, \
                    (a.verse = ?) AS exact \
             FROM bible_anchor a \
             JOIN book b ON b.id = a.book_id \
             JOIN profile p ON p.id = ? \
             WHERE a.bible_book = ? AND a.chapter = ? \
               AND a.verse = (SELECT max(x.verse) FROM bible_anchor x WHERE x.book_id = a.book_id \
                              AND x.bible_book = a.bible_book AND x.chapter = a.chapter AND x.verse <= ?) \
               AND {VISIBLE} \
             ORDER BY b.title"
        );
        let values: [Value; 5] = [
            verse.into(),
            user_id.into(),
            i64::from(reference.book).into(),
            i64::from(reference.chapter).into(),
            verse.into(),
        ];
        let stmt = Statement::from_sql_and_values(DbBackend::Sqlite, sql, values);
        Ok(BibleHit::find_by_statement(stmt).all(&self.db.reader).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(book: u8, chapter: u16, verse: Option<u16>, verse_to: Option<u16>) -> Option<Reference> {
        Some(Reference { book, chapter, verse, verse_to })
    }

    #[test]
    fn german_and_english_forms_give_the_same_reference() {
        for text in ["Johannes 3,16", "Joh 3:16", "john 3:16", "Johannes Kapitel 3 Vers 16", "joh3,16", "Johannes 3 16"] {
            assert_eq!(parse(text), r(43, 3, Some(16), None), "{text}");
        }
    }

    #[test]
    fn numbered_books_and_ranges() {
        assert_eq!(parse("1. Korinther 13,4-7"), r(46, 13, Some(4), Some(7)));
        assert_eq!(parse("1 Cor 13:4-7"), r(46, 13, Some(4), Some(7)));
        assert_eq!(parse("Erster Korinther 13"), r(46, 13, None, None));
        assert_eq!(parse("2 Mose 20,1-17"), r(2, 20, Some(1), Some(17)));
        assert_eq!(parse("3. Johannes 4"), r(64, 4, None, None));
        assert_eq!(parse("Offenbarung des Johannes 21,1"), r(66, 21, Some(1), None));
    }

    #[test]
    fn a_chapter_alone_and_umlauts() {
        assert_eq!(parse("Psalm 23"), r(19, 23, None, None));
        assert_eq!(parse("Ps 23,1"), r(19, 23, Some(1), None));
        assert_eq!(parse("Römer 8,28"), r(45, 8, Some(28), None));
        assert_eq!(parse("Sprüche 3,5"), r(20, 3, Some(5), None));
        assert_eq!(parse("1. Könige 19,12"), r(11, 19, Some(12), None));
    }

    #[test]
    fn nonsense_is_refused() {
        for text in ["", "Johannes", "3,16", "Hobbit 3,16", "Johannes 0", "Johannes 0,5", "99 Johannes 3"] {
            assert_eq!(parse(text), None, "{text}");
        }
        // A range that runs backwards keeps the start verse only.
        assert_eq!(parse("Joh 3,16-2"), r(43, 3, Some(16), None));
    }

    #[test]
    fn every_book_name_is_unique_and_all_66_exist() {
        let mut seen = std::collections::HashSet::new();
        for (n, names) in BOOKS {
            assert!(usize::from(n) <= BOOKS.len());
            for name in names {
                assert!(seen.insert(*name), "{name} names two books");
            }
        }
        assert_eq!(BOOKS.iter().map(|(n, _)| *n).collect::<Vec<_>>(), (1..=66).collect::<Vec<_>>());
    }
}
