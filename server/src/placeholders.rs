// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tag values that say nothing: what rippers and taggers write when they know nothing ("Unbekannter Künstler",
//! "Kein Titel", "Unbekanntes Album (23.12.2020 20:59:13)"). The ID tags come first (project owner, 2026-10-28),
//! but such a value is no tag; the folder name speaks instead. The words are a setting (`placeholder_tags`,
//! separated by commas, in the pages), and a value that starts with one of them, in any case, counts as missing.

use crate::error::Result;
use crate::store::Store;

/// What the setting holds when it has no row; the migration puts the same list into the database.
const DEFAULT_WORDS: &str = "unbekannt,unknown,kein künstler,kein titel,various artists,verschiedene";

#[derive(Debug, Clone)]
pub struct Placeholders {
    words: Vec<String>,
}

impl Placeholders {
    /// From a comma separated list.
    pub fn parse(list: &str) -> Placeholders {
        Placeholders { words: list.split(',').map(|w| w.trim().to_lowercase()).filter(|w| !w.is_empty()).collect() }
    }

    /// Is this value a placeholder?
    pub fn is(&self, value: &str) -> bool {
        let v = value.trim().to_lowercase();
        self.words.iter().any(|w| v.starts_with(w.as_str()))
    }

    /// The value if it says something.
    pub fn real(&self, value: Option<String>) -> Option<String> {
        value.filter(|v| !v.trim().is_empty() && !self.is(v))
    }
}

impl Default for Placeholders {
    fn default() -> Self {
        Placeholders::parse(DEFAULT_WORDS)
    }
}

impl Store {
    /// The placeholder words of the settings.
    pub async fn placeholder_tags(&self) -> Result<Placeholders> {
        Ok(self.setting("placeholder_tags").await?.map_or_else(Placeholders::default, |list| Placeholders::parse(&list)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_that_starts_with_a_word_is_no_tag_and_other_values_stay() {
        let p = Placeholders::parse(" Kein Künstler, unbekannt ,, various artists");
        assert!(p.is("Kein Künstler") && p.is("UNBEKANNTES ALBUM (23.12.2020 20:59:13)") && p.is("Various Artists"));
        assert!(!p.is("Petra") && !p.is("Der Unbekannte"));
        assert_eq!(p.real(Some("Petra".into())).as_deref(), Some("Petra"));
        assert_eq!(p.real(Some("Kein Künstler".into())), None);
        assert_eq!(p.real(Some("  ".into())), None);
        assert!(!Placeholders::parse("").is("Kein Titel"), "an empty list marks nothing");
    }
}
