// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Greek set in the BibleWorks fonts (Bwgrkl, Bwgrkn, Bwgrki): the PDF holds Latin letters that the font draws as Greek ones
//! (`afwrismenoj` is drawn as αφωρισμενος). Turned into Unicode Greek here, so that it can be read, searched and copied.

/// Whether a font family (with or without the subset prefix `ABCDEF+`) is one of the BibleWorks Greek fonts.
pub fn is_bibleworks(family: &str) -> bool {
    let name = family.split_once('+').map_or(family, |(_, r)| r);
    name.to_ascii_lowercase().starts_with("bwgrk")
}

/// The Unicode Greek of a text in a BibleWorks Greek font. A sigma at the end of a word becomes the final sigma; characters
/// without a Greek counterpart stay as they are.
pub fn bibleworks(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() * 2);
    for (i, &c) in chars.iter().enumerate() {
        let g = match c {
            'a' => 'α',
            'b' => 'β',
            'g' => 'γ',
            'd' => 'δ',
            'e' => 'ε',
            'z' => 'ζ',
            'h' => 'η',
            'q' => 'θ',
            'i' => 'ι',
            'k' => 'κ',
            'l' => 'λ',
            'm' => 'μ',
            'n' => 'ν',
            'x' => 'ξ',
            'o' => 'ο',
            'p' => 'π',
            'r' => 'ρ',
            's' => {
                if chars.get(i + 1).is_none_or(|n| !n.is_alphabetic()) {
                    'ς'
                } else {
                    'σ'
                }
            }
            'j' => 'ς',
            't' => 'τ',
            'u' => 'υ',
            'f' => 'φ',
            'c' => 'χ',
            'y' => 'ψ',
            'w' => 'ω',
            'A' => 'Α',
            'B' => 'Β',
            'G' => 'Γ',
            'D' => 'Δ',
            'E' => 'Ε',
            'Z' => 'Ζ',
            'H' => 'Η',
            'Q' => 'Θ',
            'I' => 'Ι',
            'K' => 'Κ',
            'L' => 'Λ',
            'M' => 'Μ',
            'N' => 'Ν',
            'X' => 'Ξ',
            'O' => 'Ο',
            'P' => 'Π',
            'R' => 'Ρ',
            'S' => 'Σ',
            'T' => 'Τ',
            'U' => 'Υ',
            'F' => 'Φ',
            'C' => 'Χ',
            'Y' => 'Ψ',
            'W' => 'Ω',
            // The joining stroke of the font (ἐκ-λογή as the author separates it).
            'Ä' => '-',
            other => other,
        };
        out.push(g);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_latin_letters_of_the_font_become_greek_with_the_final_sigma() {
        assert_eq!(bibleworks("afwrismenoj"), "αφωρισμενος");
        assert_eq!(bibleworks("qeou dikaiosunh"), "θεου δικαιοσυνη");
        assert_eq!(bibleworks("Cristos kai"), "Χριστος και");
        assert_eq!(bibleworks("ekÄlogh"), "εκ-λογη");
        assert!(is_bibleworks("IUDIEN+Bwgrkl"));
        assert!(!is_bibleworks("LNAUUH+TimesNewRomanPSMT"));
    }
}
