// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The fonts embedded in the EPUB, so that every reader shows every letter of the book (accents, breathings, vowel points and
//! cantillation included) whatever fonts the device has: one for each foreign script the book quotes, and one for the characters
//! of the main text that the fonts of a reader may lack (transliteration such as ḥ, ʿ, ā̆; the letters of the main script when it
//! is not Latin). The fonts of the PDF cannot serve: they are subsets, often under a licence that forbids passing them on, and
//! legacy fonts put Greek or Hebrew on Latin letters.
//!
//! Free fonts (SIL Open Font License and alike) are looked up with fontconfig: the known good ones for a script first, then any
//! installed font that has its letters. Whether a font really has every character the book uses is read from its character map;
//! the characters no font has are reported.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The group of the characters of the main text that need a font of their own (see the module).
pub const TEXT: &str = "Text";

/// A font to embed for a group: a script (ISO 15924 code) or [`TEXT`].
#[derive(Debug, Clone)]
pub struct Font {
    pub group: String,
    pub file_name: String,
    pub media_type: &'static str,
    pub data: Vec<u8>,
    /// Where it came from (for the report).
    pub source: String,
    /// The characters of the group the font does not have.
    pub missing: Vec<char>,
    /// Its character map could be read (WOFF and WOFF2 are taken as they are).
    pub checked: bool,
}

/// The name of the font of a group in the style sheet.
pub fn css_family(group: &str) -> String {
    format!("Embedded-{group}")
}

/// The free fonts tried first for a group, best first. For other scripts the Noto font of the script is tried, then any font.
pub fn preferred(group: &str) -> Vec<String> {
    let known: &[&str] = match group {
        "Grek" => &["Gentium Plus", "Gentium Book Plus", "Noto Serif", "Cardo", "GFS Didot", "SBL BibLit", "SBL Greek", "DejaVu Serif"],
        "Hebr" => &["Ezra SIL", "Noto Serif Hebrew", "Taamey Frank CLM", "Frank Ruehl CLM", "Cardo", "SBL BibLit", "SBL Hebrew", "DejaVu Sans"],
        "Arab" => &["Amiri", "Scheherazade New", "Noto Naskh Arabic", "Noto Sans Arabic", "DejaVu Sans"],
        "Syrc" => &["Noto Sans Syriac", "Noto Sans Syriac Eastern", "Noto Sans Syriac Western", "Estrangelo Edessa", "East Syriac Adiabene", "Serto Jerusalem"],
        "Copt" => &["Antinoou", "Noto Sans Coptic", "New Athena Unicode"],
        "Cyrl" => &["Gentium Plus", "Noto Serif", "Ponomar Unicode", "DejaVu Serif"],
        "Glag" => &["Noto Sans Glagolitic", "Ponomar Unicode"],
        "Armn" => &["Noto Serif Armenian", "Noto Sans Armenian", "DejaVu Sans"],
        "Geor" => &["Noto Serif Georgian", "Noto Sans Georgian", "DejaVu Sans"],
        "Ethi" => &["Abyssinica SIL", "Noto Serif Ethiopic", "Noto Sans Ethiopic"],
        "Samr" => &["Noto Sans Samaritan"],
        "Phnx" => &["Noto Sans Phoenician"],
        "Armi" => &["Noto Sans Imperial Aramaic"],
        "Xsux" => &["Noto Sans Cuneiform"],
        "Ugar" => &["Noto Sans Ugaritic"],
        "Goth" => &["Noto Sans Gothic"],
        "Latn" | TEXT => &["Gentium Plus", "Charis SIL", "Noto Serif", "DejaVu Serif"],
        _ => &[],
    };
    let mut v: Vec<String> = known.iter().map(|s| s.to_string()).collect();
    if group != TEXT {
        let name = crate::script::name(group).replace('_', " ");
        v.push(format!("Noto Serif {name}"));
        v.push(format!("Noto Sans {name}"));
    }
    v
}

fn media_type(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "ttf" => Some("font/ttf"),
        "otf" => Some("font/otf"),
        "woff" => Some("font/woff"),
        "woff2" => Some("font/woff2"),
        _ => None,
    }
}

/// The characters of `chars` a font does not have; `None` when its character map cannot be read.
pub fn missing(data: &[u8], chars: &BTreeSet<char>) -> Option<Vec<char>> {
    let face = ttf_parser::Face::parse(data, 0).ok()?;
    Some(chars.iter().copied().filter(|c| face.glyph_index(*c).is_none()).collect())
}

/// Reads a font file for a group.
pub fn load(group: &str, path: &Path, chars: &BTreeSet<char>) -> Option<Font> {
    let media_type = media_type(path)?;
    let data = std::fs::read(path).ok()?;
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let file_name = format!("{}.{ext}", css_family(group).to_ascii_lowercase());
    let (missing, checked) = match missing(&data, chars) {
        Some(m) => (m, true),
        None => (Vec::new(), false),
    };
    Some(Font { group: group.to_string(), file_name, media_type, data, source: path.display().to_string(), missing, checked })
}

/// The font files fontconfig lists for a query, the upright regular cut first (not a condensed, light or bold one).
fn fc_list(pattern: &str) -> Vec<PathBuf> {
    let Ok(out) = Command::new("fc-list").arg(pattern).args(["file", "style"]).output() else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut files: Vec<(String, String)> = text
        .lines()
        .filter_map(|l| {
            let (file, style) = l.split_once(':')?;
            Some((file.trim().to_string(), style.to_ascii_lowercase()))
        })
        .filter(|(f, _)| media_type(Path::new(f)).is_some())
        .collect();
    files.sort_by_key(|(f, s)| {
        let f = f.to_ascii_lowercase();
        let regular = s.contains("regular") || s.contains("book") || s.contains("normal") || s.contains("roman");
        let narrow = ["condensed", "light", "thin", "bold", "black", "italic", "oblique", "mono"].iter().any(|w| f.contains(w));
        (!regular, narrow, f)
    });
    let mut seen = BTreeSet::new();
    files.into_iter().map(|(f, _)| f).filter(|f| seen.insert(f.clone())).map(PathBuf::from).collect()
}

/// The best free font installed for a group: the first of the known good fonts that has every character, else any installed
/// font that has them all, else the one that has the most.
pub fn find(group: &str, chars: &BTreeSet<char>) -> Option<Font> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    for family in preferred(group) {
        candidates.extend(fc_list(&format!(":family={family}")).into_iter().next());
    }
    // Fonts that have the characters (fontconfig checks up to some dozens of them; the character map decides).
    let sample: Vec<String> = chars.iter().take(48).map(|c| format!("{:x}", *c as u32)).collect();
    candidates.extend(fc_list(&format!(":charset={}", sample.join(" "))));
    if let Some(c) = chars.iter().next() {
        candidates.extend(fc_list(&format!(":charset={:x}", *c as u32)).into_iter().take(40));
    }
    let mut seen = BTreeSet::new();
    let mut best: Option<Font> = None;
    for path in candidates.into_iter().filter(|p| seen.insert(p.clone())) {
        let Some(font) = load(group, &path, chars) else { continue };
        if !font.checked || font.missing.len() == chars.len() {
            continue;
        }
        if font.missing.is_empty() {
            return Some(font);
        }
        if best.as_ref().is_none_or(|b| font.missing.len() < b.missing.len()) {
            best = Some(font);
        }
    }
    best
}
