// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The fonts for Greek and Hebrew, embedded in the EPUB so that every reader shows them, accents, breathings and points included,
//! whatever fonts the device has. The fonts of the PDF cannot serve: they are subsets, often under a licence that forbids passing
//! them on, and the BibleWorks fonts put Greek on Latin letters. Free fonts (SIL Open Font License and alike) are looked up with
//! fontconfig in this order; a font file of one's own can be given instead.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    Greek,
    Hebrew,
}

impl Script {
    /// The name of the font in the style sheet of the EPUB.
    pub fn css_family(self) -> &'static str {
        match self {
            Script::Greek => "EmbeddedGreek",
            Script::Hebrew => "EmbeddedHebrew",
        }
    }

    /// The free fonts tried, best first.
    pub fn preferred(self) -> &'static [&'static str] {
        match self {
            Script::Greek => &["Gentium Plus", "Gentium Book Plus", "Noto Serif", "Cardo", "GFS Didot", "SBL BibLit", "SBL Greek", "DejaVu Serif"],
            Script::Hebrew => &["Ezra SIL", "Noto Serif Hebrew", "Taamey Frank CLM", "Frank Ruehl CLM", "Cardo", "SBL BibLit", "SBL Hebrew", "DejaVu Sans"],
        }
    }

    /// Whether a character belongs to the script (Greek with its extended block for the polytonic letters, Hebrew with its
    /// presentation forms).
    pub fn has(self, c: char) -> bool {
        match self {
            Script::Greek => matches!(c, '\u{0370}'..='\u{03ff}' | '\u{1f00}'..='\u{1fff}'),
            Script::Hebrew => matches!(c, '\u{0590}'..='\u{05ff}' | '\u{fb1d}'..='\u{fb4f}'),
        }
    }
}

/// A font to embed.
#[derive(Debug, Clone)]
pub struct Font {
    pub script: Script,
    pub file_name: String,
    pub media_type: &'static str,
    pub data: Vec<u8>,
    /// Where it came from (for the report).
    pub source: String,
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

/// Reads a font file for a script.
pub fn load(script: Script, path: &Path) -> Option<Font> {
    let media_type = media_type(path)?;
    let data = std::fs::read(path).ok()?;
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let file_name = format!("{}.{ext}", script.css_family().to_ascii_lowercase());
    Some(Font { script, file_name, media_type, data, source: path.display().to_string() })
}

/// The file of the regular style of an installed font family, if fontconfig knows it.
fn installed(family: &str) -> Option<PathBuf> {
    let out = Command::new("fc-list").arg(format!(":family={family}")).args(["file", "style"]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut files: Vec<(String, String)> = text
        .lines()
        .filter_map(|l| {
            let (file, style) = l.split_once(':')?;
            Some((file.trim().to_string(), style.to_ascii_lowercase()))
        })
        .filter(|(f, _)| media_type(Path::new(f)).is_some())
        .collect();
    // The upright regular cut of the family, not a condensed, light or bold one.
    files.sort_by_key(|(f, s)| {
        let f = f.to_ascii_lowercase();
        let regular = s.contains("regular") || s.contains("book") || s.contains("normal") || s.contains("roman");
        let narrow = ["condensed", "light", "bold", "italic", "oblique"].iter().any(|w| f.contains(w));
        (!regular, narrow)
    });
    files.into_iter().next().map(|(f, _)| PathBuf::from(f))
}

/// The best free font installed for a script.
pub fn find(script: Script) -> Option<Font> {
    script.preferred().iter().find_map(|f| installed(f)).and_then(|p| load(script, &p))
}
