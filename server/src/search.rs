// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The search of the media lists and of the MCP tools (project owner, 2026-10-29: "the search has no wildcards / regex").
//!
//! * plain words: the text is found anywhere in the title, author or narrator, ignoring case (as before);
//! * `*` stands for any run of characters and `?` for one character: `der * welt`, `tkkg ?`, `*passion`;
//! * between slashes the text is a regular expression (Rust syntax, case is ignored): `/^die .* der welt$/`, `/(bach|händel)/`.
//!
//! The text that is searched is title, author and narrator joined by spaces, in lower case (the pages carry the same text in `data-search`, so the
//! search that runs while typing in the browser and the one of the server agree). The same rules are in `static/ui.js`.

use regex::{Regex, RegexBuilder};

/// A compiled search.
#[derive(Debug)]
pub enum Matcher {
    /// Nothing was asked: everything matches.
    All,
    /// A plain text (in lower case).
    Contains(String),
    /// A wildcard pattern or a regular expression.
    Pattern(Regex),
}

/// Longest search text, in characters.
pub const MAX_CHARS: usize = 200;

/// Reads a search text. `Err` carries the reason for a regular expression that does not compile or a text that is too long.
pub fn compile(query: &str) -> Result<Matcher, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Matcher::All);
    }
    if q.chars().count() > MAX_CHARS {
        return Err("the search text is too long".into());
    }
    let build = |source: &str| RegexBuilder::new(source).case_insensitive(true).size_limit(1 << 20).build().map_err(|e| e.to_string().lines().last().unwrap_or("invalid pattern").to_owned());
    if q.len() > 2 && q.starts_with('/') && q.ends_with('/') {
        return build(&q[1..q.len() - 1]).map(Matcher::Pattern);
    }
    if q.contains('*') || q.contains('?') {
        let mut source = String::new();
        for c in q.chars() {
            match c {
                '*' => source.push_str(".*"),
                '?' => source.push('.'),
                c => source.push_str(&regex::escape(&c.to_string())),
            }
        }
        return build(&source).map(Matcher::Pattern);
    }
    Ok(Matcher::Contains(q.to_lowercase()))
}

impl Matcher {
    /// Does the text match? `text` is compared in lower case for a plain search; a pattern ignores case itself.
    pub fn is_match(&self, text: &str) -> bool {
        match self {
            Matcher::All => true,
            Matcher::Contains(needle) => text.to_lowercase().contains(needle),
            Matcher::Pattern(re) => re.is_match(text),
        }
    }

    /// The text of a book that is searched: title, author and narrator, joined by spaces.
    pub fn matches_book(&self, title: &str, author: &str, narrator: Option<&str>) -> bool {
        self.is_match(&format!("{title} {author} {}", narrator.unwrap_or("")).trim_end().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(q: &str, text: &str) -> bool {
        compile(q).unwrap().is_match(text)
    }

    #[test]
    fn plain_words_are_found_anywhere_ignoring_case() {
        assert!(ok("welt", "Winter der Welt Ken Follett"));
        assert!(ok("  DER WELT ", "Winter der Welt"));
        assert!(!ok("sommer", "Winter der Welt"));
        assert!(ok("", "anything") && ok("   ", "anything"));
    }

    #[test]
    fn a_star_is_any_run_and_a_question_mark_one_character() {
        assert!(ok("winter * welt", "Winter der Welt Ken Follett"));
        assert!(ok("*passion", "Matthäus-Passion Bach"));
        assert!(ok("tkkg ?", "TKKG 7") && !ok("tkkg ?", "TKKG"), "a character has to be there (a pattern finds its place anywhere in the text)");
        assert!(ok("pip?i", "Pippi") && !ok("pip?i", "Pipi"));
        // The characters of the syntax of a regular expression are plain in a wildcard.
        assert!(ok("a.b*", "a.b und mehr") && !ok("a.b*", "axb und mehr"));
        assert!(ok("(1) *", "(1) Eins") && ok("c++ *", "C++ Buch"));
    }

    #[test]
    fn between_slashes_the_text_is_a_regular_expression() {
        assert!(ok("/^winter/", "winter der welt"));
        assert!(!ok("/^welt/", "winter der welt"));
        assert!(ok("/(bach|händel)/", "Georg Friedrich Händel"));
        assert!(ok("/Welt$/", "winter der welt"), "case is ignored");
        assert!(ok("/\\d{2}/", "Folge 12") && !ok("/\\d{2}/", "Folge 1"));
        // A single slash or a path is a plain text.
        assert!(ok("/", "a/b") && ok("a/b", "x a/b y"));
    }

    #[test]
    fn a_broken_expression_and_a_too_long_text_are_refused_not_matched_wrongly() {
        assert!(compile("/(unclosed/").is_err());
        assert!(compile("/[z-a]/").is_err());
        assert!(compile(&"x".repeat(MAX_CHARS + 1)).is_err());
        // A pattern that would eat memory is limited.
        assert!(compile("/(a{1000}){1000}/").is_err());
    }

    #[test]
    fn a_book_is_searched_by_title_author_and_narrator() {
        let m = compile("/^winter der welt ken follett philipp/").unwrap();
        assert!(m.matches_book("Winter der Welt", "Ken Follett", Some("Philipp Schepmann")));
        assert!(compile("schepmann").unwrap().matches_book("Winter der Welt", "Ken Follett", Some("Philipp Schepmann")));
        assert!(!compile("schepmann").unwrap().matches_book("Winter der Welt", "Ken Follett", None));
    }
}
