// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The PDF as poppler sees it: `pdftohtml -xml` gives every piece of text with its place, its font and its emphasis; `pdfinfo` the
//! title and author; `pdftoppm` the picture of the cover. poppler-utils is what the server needs for its PDFs anyway.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use crate::Error;
use crate::greek;

/// A font of the PDF: its size in points (as poppler rounds it) and its name.
#[derive(Debug, Clone)]
pub struct Font {
    pub size: f32,
    pub family: String,
}

impl Font {
    pub fn name(&self) -> &str {
        self.family.split_once('+').map_or(self.family.as_str(), |(_, r)| r)
    }

    /// The family without subset prefix and style: `LNAUUH+TimesNewRomanPS-BoldMT` → `TimesNewRoman`.
    pub fn key(&self) -> String {
        let name = self.name();
        let base = name.split(['-', ',']).next().unwrap_or(name);
        let base = base.strip_suffix("MT").unwrap_or(base);
        let base = base.strip_suffix("PS").unwrap_or(base);
        base.to_string()
    }

    pub fn bold(&self) -> bool {
        let n = self.name().to_ascii_lowercase();
        ["bold", "black", "heavy", "demi"].iter().any(|w| n.contains(w))
    }

    pub fn italic(&self) -> bool {
        let n = self.name().to_ascii_lowercase();
        n.contains("italic") || n.contains("oblique") || n.ends_with("it")
    }

    pub fn greek(&self) -> bool {
        greek::is_bibleworks(&self.family)
    }
}

/// A piece of text with one emphasis.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub greek: bool,
}

/// One `<text>` of poppler: a piece of a line in one font.
#[derive(Debug, Clone)]
pub struct Item {
    pub top: f32,
    pub left: f32,
    pub width: f32,
    pub height: f32,
    pub font: u32,
    pub runs: Vec<Run>,
}

#[derive(Debug, Clone, Default)]
pub struct Page {
    pub width: f32,
    pub height: f32,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, Default)]
pub struct Pdf {
    pub fonts: HashMap<u32, Font>,
    pub pages: Vec<Page>,
    /// Per font name: its characters, those of them without a meaning in Unicode (private use area, U+FFFD), and the ASCII
    /// signs found between the letters of right-to-left words (a broken Unicode map: `בָּ%רָ`).
    pub chars: HashMap<String, (usize, usize, usize)>,
}

/// Fonts that put Hebrew or Greek on the codes of Latin letters (legacy encodings): what poppler reads from them is Latin
/// gibberish. The Greek of BibleWorks is turned into Unicode ([`greek`]); these are not (yet).
const LEGACY: &[&str] = &["bwhebb", "bwhebl", "sptiberian", "spionic", "spezra", "spatlantis", "graeca", "teknia", "supergreek", "hebraica", "bwsyrc", "bwcopt"];

impl Pdf {
    pub fn font(&self, id: u32) -> Font {
        self.fonts.get(&id).cloned().unwrap_or(Font { size: 10.0, family: String::new() })
    }

    /// What of the text cannot be read: characters without a meaning in Unicode (the PDF does not say which letters its glyphs
    /// are), and fonts with a legacy encoding.
    pub fn unreadable(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut names: Vec<_> = self.chars.iter().collect();
        names.sort_by(|a, b| a.0.cmp(b.0));
        for (name, (total, unmapped, stray)) in names {
            if *unmapped > 0 {
                out.push(format!(
                    "font {name}: {unmapped} of {total} characters have no meaning in Unicode (private use area or U+FFFD): the PDF does not say which letters they are"
                ));
            }
            if *stray >= 3 {
                out.push(format!(
                    "font {name}: {stray} ASCII signs inside right-to-left words: its Unicode map is broken (the PDF says the wrong characters for vowel points or letters)"
                ));
            }
            let lower = name.to_ascii_lowercase();
            if LEGACY.iter().any(|l| lower.contains(l)) {
                out.push(format!("font {name}: {total} characters in a legacy encoding (Latin codes drawn as other letters), not turned into Unicode"));
            }
        }
        out
    }
}

fn num(n: roxmltree::Node, name: &str) -> f32 {
    n.attribute(name).and_then(|v| v.parse().ok()).unwrap_or(0.0)
}

fn collect(n: roxmltree::Node, bold: bool, italic: bool, greek: bool, out: &mut Vec<Run>) {
    for c in n.children() {
        if c.is_text() {
            let t = c.text().unwrap_or_default();
            if t.is_empty() {
                continue;
            }
            let text = if greek { greek::bibleworks(t) } else { t.to_string() };
            match out.last_mut() {
                Some(last) if last.bold == bold && last.italic == italic && last.greek == greek => last.text.push_str(&text),
                _ => out.push(Run { text, bold, italic, greek }),
            }
        } else if c.is_element() {
            match c.tag_name().name() {
                "b" => collect(c, true, italic, greek, out),
                "i" => collect(c, bold, true, greek, out),
                _ => collect(c, bold, italic, greek, out),
            }
        }
    }
}

/// Reads the XML of `pdftohtml -xml`.
pub fn parse(xml: &str) -> Result<Pdf, Error> {
    let clean: String = xml.chars().filter(|c| !matches!(c, '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}')).collect();
    let opt = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    let doc = roxmltree::Document::parse_with_options(&clean, opt).map_err(|e| Error::Xml(e.to_string()))?;
    let mut pdf = Pdf::default();
    for page in doc.root_element().children().filter(|n| n.has_tag_name("page")) {
        let mut p = Page { width: num(page, "width"), height: num(page, "height"), items: Vec::new() };
        for n in page.children().filter(|n| n.is_element()) {
            match n.tag_name().name() {
                "fontspec" => {
                    if let Some(id) = n.attribute("id").and_then(|v| v.parse().ok()) {
                        pdf.fonts.insert(id, Font { size: num(n, "size"), family: n.attribute("family").unwrap_or_default().to_string() });
                    }
                }
                "text" => {
                    let font: u32 = n.attribute("font").and_then(|v| v.parse().ok()).unwrap_or(0);
                    let greek = pdf.fonts.get(&font).is_some_and(Font::greek);
                    let mut runs = Vec::new();
                    collect(n, false, false, greek, &mut runs);
                    // A space drawn on its own says where a word ends (right-to-left glyphs drawn one by one have no other sign).
                    if runs.iter().all(|r| r.text.trim().is_empty()) && (runs.is_empty() || num(n, "width") <= 0.0) {
                        continue;
                    }
                    let name = pdf.fonts.get(&font).map(|f| f.name().to_string()).unwrap_or_default();
                    let e = pdf.chars.entry(name).or_default();
                    let text: Vec<char> = runs.iter().flat_map(|r| r.text.chars()).collect();
                    let rtl = |c: Option<&char>| c.is_some_and(|c| matches!(crate::script::dir(*c), crate::script::Dir::R | crate::script::Dir::Mark));
                    for (i, c) in text.iter().enumerate().filter(|(_, c)| !c.is_whitespace()) {
                        e.0 += 1;
                        if matches!(c, '\u{e000}'..='\u{f8ff}' | '\u{f0000}'..='\u{10ffff}' | '\u{fffd}') {
                            e.1 += 1;
                        }
                        if c.is_ascii_punctuation() && !matches!(c, '.' | ',' | ':' | ';' | '-' | '(' | ')') && rtl(i.checked_sub(1).and_then(|j| text.get(j))) && rtl(text.get(i + 1)) {
                            e.2 += 1;
                        }
                    }
                    p.items.push(Item { top: num(n, "top"), left: num(n, "left"), width: num(n, "width"), height: num(n, "height"), font, runs });
                }
                _ => {}
            }
        }
        pdf.pages.push(p);
    }
    Ok(pdf)
}

fn tool(name: &'static str, cmd: &mut Command) -> Result<(), Error> {
    let out = cmd.output().map_err(|e| Error::Tool(name, e.to_string()))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(Error::Tool(name, format!("{}: {}", out.status, String::from_utf8_lossy(&out.stderr).trim())))
    }
}

/// The XML of `pdftohtml -xml` for the PDF, made in `dir`.
pub fn to_xml(pdf: &Path, dir: &Path) -> Result<String, Error> {
    let out = dir.join("book");
    tool("pdftohtml", Command::new("pdftohtml").args(["-xml", "-i", "-q", "-nodrm", "-enc", "UTF-8"]).arg(pdf).arg(&out))?;
    std::fs::read_to_string(dir.join("book.xml")).map_err(Error::Io)
}

/// Title and author as `pdfinfo` reads them (empty when the PDF does not say).
pub fn info(pdf: &Path) -> (Option<String>, Option<String>) {
    let Ok(out) = Command::new("pdfinfo").args(["-enc", "UTF-8"]).arg(pdf).output() else {
        return (None, None);
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let field = |name: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(name).and_then(|r| r.strip_prefix(':')))
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    (field("Title"), field("Author"))
}

/// The picture of one page (1-based) as JPEG, for the cover.
pub fn render_page(pdf: &Path, page: usize, dir: &Path) -> Option<Vec<u8>> {
    let out = dir.join("cover");
    let p = page.to_string();
    tool(
        "pdftoppm",
        Command::new("pdftoppm").args(["-jpeg", "-jpegopt", "quality=85", "-scale-to", "1600", "-singlefile", "-f", &p, "-l", &p]).arg(pdf).arg(&out),
    )
    .ok()?;
    std::fs::read(dir.join("cover.jpg")).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_xml_of_poppler_gives_the_pieces_with_their_emphasis_and_the_greek_in_unicode() {
        let xml = r##"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE pdf2xml SYSTEM "pdf2xml.dtd">
<pdf2xml producer="poppler" version="25.03.0">
<page number="1" position="absolute" top="0" left="0" height="871" width="574">
	<fontspec id="0" size="15" family="LNAUUH+TimesNewRomanPSMT" color="#231f20"/>
	<fontspec id="1" size="15" family="IUDIEN+Bwgrkl" color="#231f20"/>
<text top="80" left="68" width="438" height="20" font="0">Das <i>Wort</i> und <b><i>beides</i></b></text>
<text top="80" left="300" width="40" height="20" font="1">logoj</text>
<text top="99" left="68" width="4" height="20" font="0"> </text>
</page>
</pdf2xml>"##;
        let pdf = parse(xml).unwrap();
        assert_eq!(pdf.pages.len(), 1);
        let items = &pdf.pages[0].items;
        assert_eq!(items.len(), 3, "a space drawn on its own is kept: it tells where a word ends");
        assert_eq!(items[2].runs[0].text, " ");
        assert_eq!(items[0].runs.len(), 4);
        assert!(items[0].runs[1].italic && !items[0].runs[1].bold);
        assert!(items[0].runs[3].italic && items[0].runs[3].bold);
        assert_eq!(items[1].runs[0].text, "λογος");
        assert!(items[1].runs[0].greek);
        assert!(pdf.unreadable().is_empty());
        let broken = xml.replace("logoj", "בָּ%רָ!אֵ+שִׁ!ית").replace("Bwgrkl", "LiberationMono");
        assert!(parse(&broken).unwrap().unreadable()[0].contains("4 ASCII signs inside right-to-left words"));
        let f = pdf.font(0);
        assert_eq!(f.key(), "TimesNewRoman");
        assert!(Font { size: 20.0, family: "MSDLIF+MyriadPro-SemiboldIt".into() }.bold());
        assert!(Font { size: 20.0, family: "MSDLIF+MyriadPro-SemiboldIt".into() }.italic());
    }
}
