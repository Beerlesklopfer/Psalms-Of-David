// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The writing systems of a book, for all of Unicode: which script a letter belongs to (ISO 15924: `Latn`, `Grek`, `Hebr`, `Arab`,
//! `Syrc`, `Copt`, `Cyrl`, `Ethi`, `Xsux` …), which way it runs, and the runs of foreign script in a text. A theological work may
//! quote Hebrew, Aramaic, Syriac, Arabic, Coptic, Ge'ez, Old Church Slavonic or cuneiform; every one of them is kept as Unicode,
//! marked with its language and direction, and shown in an embedded font.

use unicode_bidi::{BidiClass, bidi_class};
use unicode_normalization::UnicodeNormalization;
use unicode_script::{Script, UnicodeScript};

/// The script of a character as ISO 15924 code, `None` for the characters all scripts share (spaces, digits, punctuation) and
/// for the combining marks, which belong to the letter before them.
pub fn of(c: char) -> Option<&'static str> {
    match c.script() {
        Script::Common | Script::Inherited | Script::Unknown => None,
        s => Some(s.short_name()),
    }
}

/// The scripts written from right to left.
pub fn is_rtl(code: &str) -> bool {
    matches!(
        code,
        "Hebr" | "Arab" | "Syrc" | "Thaa" | "Nkoo" | "Samr" | "Mand" | "Adlm" | "Rohg" | "Yezi" | "Phnx" | "Armi" | "Prti" | "Phli" | "Phlp"
            | "Avst" | "Sarb" | "Narb" | "Nbat" | "Palm" | "Hatr" | "Mani" | "Lydi" | "Cprt" | "Khar" | "Orkh" | "Sogd" | "Sogo" | "Elym"
            | "Chrs" | "Ougr" | "Mend" | "Hung"
    )
}

/// The English name of a script (`Syriac` for `Syrc`).
pub fn name(code: &str) -> &'static str {
    Script::from_short_name(code).map_or("Unknown", Script::full_name)
}

/// A script from its code or name, in any case: `syrc`, `Syriac`, `SYRIAC`.
pub fn parse(s: &str) -> Option<&'static str> {
    // "old italic" → "Old_Italic", "SYRC" → "Syrc".
    let title: Vec<String> = s
        .split(['_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let w = w.to_ascii_lowercase();
            w.get(..1).map(str::to_ascii_uppercase).unwrap_or_default() + w.get(1..).unwrap_or_default()
        })
        .collect();
    let title = title.join("_");
    Script::from_short_name(&title).or_else(|| Script::from_full_name(&title)).map(Script::short_name)
}

/// The language a script is marked with when the book does not say otherwise: the language a quotation in that script mostly is in
/// a scholarly work (Greek of the New Testament and the fathers, Hebrew of the Old Testament), else the script alone (`und-Cyrl`).
pub fn default_lang(code: &str) -> String {
    let lang = match code {
        "Grek" => "grc",
        "Hebr" => "he",
        "Arab" => "ar",
        "Syrc" => "syc",
        "Copt" => "cop",
        "Armn" => "hy",
        "Geor" => "ka",
        "Ethi" => "gez",
        "Goth" => "got",
        "Glag" => "cu",
        "Ugar" => "uga",
        "Xsux" => "akk",
        "Phnx" => "phn",
        "Armi" => "arc",
        "Samr" => "smp",
        "Thaa" => "dv",
        _ => return format!("und-{code}"),
    };
    lang.to_string()
}

/// The direction of a character, as far as putting the text of a line back in reading order needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    /// A letter written from left to right.
    L,
    /// A letter written from right to left (Hebrew, Arabic, Syriac …).
    R,
    /// A digit: numbers run from left to right also in right-to-left text.
    Digit,
    /// A combining mark (a vowel point, an accent): it belongs to the letter before it.
    Mark,
    /// Spaces and punctuation: they take the direction of their neighbours.
    Neutral,
}

pub fn dir(c: char) -> Dir {
    match bidi_class(c) {
        BidiClass::L => Dir::L,
        BidiClass::R | BidiClass::AL => Dir::R,
        BidiClass::EN | BidiClass::AN => Dir::Digit,
        BidiClass::NSM => Dir::Mark,
        _ => Dir::Neutral,
    }
}

/// The direction of a piece of text: that of its first letter or digit.
pub fn dir_of(s: &str) -> Dir {
    s.chars().map(dir).find(|d| !matches!(d, Dir::Mark | Dir::Neutral)).unwrap_or(Dir::Neutral)
}

/// The bracket a right-to-left run shows for a left-to-right one: `(` for `)`.
pub fn mirror(c: char) -> char {
    unicode_bidi_mirroring::get_mirrored(c).unwrap_or(c)
}

/// The text as it is meant, not as it was drawn: the presentation forms of Arabic and Hebrew (the shapes a letter takes at the
/// start, in the middle or at the end of a word, and the ligatures such as lam-alef or fi) become their letters, and everything
/// is composed (NFC), so that it can be searched and copied.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\u{fb00}'..='\u{fdff}' | '\u{fe70}'..='\u{feff}') && c != '\u{feff}' {
            out.extend(std::iter::once(c).nfkc());
        } else {
            out.push(c);
        }
    }
    if out.is_ascii() { out } else { out.nfc().collect() }
}

/// Characters the fonts of every reader have: Latin with its usual accents and the general punctuation. Others in the main
/// script (transliteration such as ḥ, ʿ, ā̆, rare symbols) get an embedded font as well.
pub fn is_common(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{024f}' | '\u{2000}'..='\u{206f}' | '\u{20ac}' | '\u{2122}')
}

/// A text cut into runs of one script: `(Some("Hebr"), "שָׁלוֹם")` for a foreign script, `(None, …)` for the main script of the
/// book. Spaces, digits and punctuation inside a foreign phrase stay with it; after its last letter they belong to the main text
/// again. Combining marks stay with their letter.
pub fn runs<'a>(text: &'a str, main: &str) -> Vec<(Option<&'static str>, &'a str)> {
    // Pieces with their script; `pending` is where the shared characters since the last letter begin.
    let mut pieces: Vec<(Option<&'static str>, usize, usize)> = Vec::new();
    let mut cur: Option<&'static str> = None;
    let mut cur_end = 0;
    let mut after_letter = false;
    for (i, c) in text.char_indices() {
        let end = i + c.len_utf8();
        if of(c).is_none() {
            if dir(c) == Dir::Mark && after_letter {
                cur_end = end;
            } else {
                after_letter = false;
            }
            continue;
        }
        let s = of(c).filter(|s| *s != main);
        // The shared characters between two letters go with them when both are of one script, else to the main text.
        let pending = if s == cur { None } else { Some(cur_end) };
        if let Some(p) = pending {
            pieces.push((cur, pieces.last().map_or(0, |l| l.2), p));
            if p < i {
                pieces.push((None, p, i));
            }
            pieces.push((s, i, end));
        } else {
            match pieces.last_mut() {
                Some(last) if last.0 == s => last.2 = end,
                _ => pieces.push((s, pieces.last().map_or(0, |l| l.2), end)),
            }
        }
        cur = s;
        cur_end = end;
        after_letter = true;
    }
    pieces.push((cur, pieces.last().map_or(0, |l| l.2), cur_end));
    if cur_end < text.len() {
        pieces.push((None, cur_end, text.len()));
    }
    let mut out: Vec<(Option<&'static str>, usize, usize)> = Vec::new();
    for (s, a, b) in pieces {
        if a >= b {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.0 == s && last.2 == a => last.2 = b,
            _ => out.push((s, a, b)),
        }
    }
    out.into_iter().map(|(s, a, b)| (s, &text[a..b])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scripts_of_letters_and_their_direction() {
        assert_eq!(of('α'), Some("Grek"));
        assert_eq!(of('ש'), Some("Hebr"));
        assert_eq!(of('ܐ'), Some("Syrc"));
        assert_eq!(of('ⲁ'), Some("Copt"));
        assert_eq!(of('ሀ'), Some("Ethi"));
        assert_eq!(of('𒀀'), Some("Xsux"));
        assert_eq!(of(' '), None);
        assert_eq!(of('\u{0301}'), None, "a combining accent belongs to its letter");
        assert!(is_rtl("Syrc") && is_rtl("Arab") && !is_rtl("Copt"));
        assert_eq!(dir('ب'), Dir::R);
        assert_eq!(dir('\u{05bc}'), Dir::Mark);
        assert_eq!(dir('٣'), Dir::Digit);
        assert_eq!(parse("syriac"), Some("Syrc"));
        assert_eq!(parse("HEBR"), Some("Hebr"));
        assert_eq!(parse("Old_Italic"), Some("Ital"));
        assert_eq!(default_lang("Syrc"), "syc");
        assert_eq!(default_lang("Cyrl"), "und-Cyrl");
    }

    #[test]
    fn presentation_forms_become_letters_and_points_are_composed() {
        assert_eq!(normalize("\u{fefc}"), "\u{0644}\u{0627}", "lam-alef");
        assert_eq!(normalize("\u{fb01}nden"), "finden");
        assert_eq!(normalize("\u{fb2a}"), "\u{05e9}\u{05c1}", "shin with its dot");
        assert_eq!(normalize("e\u{0301}"), "é");
        // Points in canonical order: the hiriq before the shin dot.
        assert_eq!(normalize("ש\u{05c1}\u{05b4}"), "ש\u{05b4}\u{05c1}");
    }

    #[test]
    fn a_text_is_cut_into_runs_of_one_script() {
        assert_eq!(runs("Im Anfang (בְּרֵאשִׁית בָּרָא) schuf", "Latn"), vec![
            (None, "Im Anfang ("),
            (Some("Hebr"), "בְּרֵאשִׁית בָּרָא"),
            (None, ") schuf")
        ]);
        assert_eq!(runs("λόγος, ܡܠܬܐ und דָּבָר.", "Latn"), vec![
            (Some("Grek"), "λόγος"),
            (None, ", "),
            (Some("Syrc"), "ܡܠܬܐ"),
            (None, " und "),
            (Some("Hebr"), "דָּבָר"),
            (None, ".")
        ]);
        assert_eq!(runs("ܒ 12 ܐ", "Latn"), vec![(Some("Syrc"), "ܒ 12 ܐ")]);
        // In a Hebrew book Hebrew is the main text and Latin the foreign one.
        assert_eq!(runs("שָׁלוֹם (Friede) עֲלֵיכֶם", "Hebr"), vec![(None, "שָׁלוֹם ("), (Some("Latn"), "Friede"), (None, ") עֲלֵיכֶם")]);
        assert_eq!(runs("Gott", "Latn"), vec![(None, "Gott")]);
    }
}
