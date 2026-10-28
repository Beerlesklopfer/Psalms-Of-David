// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Language selection and message catalogs for the administration interface.
//! English is the first language and the fallback; German is the second.

use super::messages::{DE, EN};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
}

impl Lang {
    /// The value for the `lang` attribute of the page.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::De => "de",
        }
    }

    /// A language code as stored in a profile: `en` or `de`.
    pub fn from_code(code: &str) -> Option<Lang> {
        match code {
            "en" => Some(Lang::En),
            "de" => Some(Lang::De),
            _ => None,
        }
    }

    /// Picks the best supported language from an `Accept-Language` header such as
    /// `de-DE,de;q=0.9,en;q=0.8`. Anything unsupported, or an empty header, gives
    /// English. On equal weight the language named first wins.
    pub fn from_accept_language(header: &str) -> Lang {
        let mut best: Option<(f32, Lang)> = None;
        for part in header.split(',') {
            let mut pieces = part.trim().split(';');
            let tag = pieces.next().unwrap_or("").trim().to_ascii_lowercase();
            let primary = tag.split('-').next().unwrap_or("");
            let lang = match primary {
                "en" => Lang::En,
                "de" => Lang::De,
                _ => continue,
            };
            let weight = pieces
                .find_map(|p| p.trim().strip_prefix("q=").and_then(|q| q.trim().parse::<f32>().ok()))
                .unwrap_or(1.0);
            if weight > 0.0 && best.is_none_or(|(w, _)| weight > w) {
                best = Some((weight, lang));
            }
        }
        best.map_or(Lang::En, |(_, l)| l)
    }
}

/// Looks up texts for one language, falling back to English and, for a key that
/// exists nowhere, to the key itself so that a gap is visible instead of empty.
#[derive(Debug, Clone, Copy)]
pub struct Catalog {
    lang: Lang,
}

impl Catalog {
    pub fn new(lang: Lang) -> Catalog {
        Catalog { lang }
    }

    pub fn lang(&self) -> Lang {
        self.lang
    }

    pub fn t<'a>(&self, key: &'a str) -> &'a str {
        let find = |table: &'static [(&'static str, &'static str)]| {
            table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
        };
        match self.lang {
            Lang::En => find(EN),
            Lang::De => find(DE).or_else(|| find(EN)),
        }
        .unwrap_or(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_best_weighted_language_wins() {
        assert_eq!(Lang::from_accept_language("de-DE,de;q=0.9,en;q=0.8"), Lang::De);
        assert_eq!(Lang::from_accept_language("en-US,en;q=0.9,de;q=0.8"), Lang::En);
        assert_eq!(Lang::from_accept_language("fr;q=0.9,de;q=0.5"), Lang::De);
        assert_eq!(Lang::from_accept_language("en;q=0.3, de;q=0.7"), Lang::De);
    }

    #[test]
    fn anything_unsupported_gives_english() {
        assert_eq!(Lang::from_accept_language(""), Lang::En);
        assert_eq!(Lang::from_accept_language("fr-FR,fr;q=0.9"), Lang::En);
        assert_eq!(Lang::from_accept_language("*"), Lang::En);
        assert_eq!(Lang::from_accept_language("de;q=0"), Lang::En, "q=0 means not acceptable");
    }

    #[test]
    fn a_missing_key_shows_the_key() {
        assert_eq!(Catalog::new(Lang::De).t("no.such.key"), "no.such.key");
    }
}
