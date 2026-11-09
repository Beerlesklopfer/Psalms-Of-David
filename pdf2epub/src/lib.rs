// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! pdf2epub: a typeset PDF book as EPUB 3, with its scholarly apparatus whole: the table of contents as printed (with links) and
//! as navigation by the headings, the footnotes linked both ways, the printed page numbers as page list (to cite by the page of
//! the print), the bibliography, the list of abbreviations, an index with its page numbers linked. Text is Unicode throughout, in
//! reading order also where it runs from right to left; every foreign script (Greek, also from the BibleWorks fonts, Hebrew,
//! Syriac, Arabic, Coptic …) is marked with its language and direction and shown in an embedded free font. Books written from
//! right to left as a whole become right-to-left EPUBs.
//!
//! The PDF is read with poppler-utils (`pdftohtml`, `pdfinfo`, `pdftoppm`).

pub mod book;
pub mod epub;
pub mod fonts;
pub mod greek;
pub mod layout;
pub mod pdf;
pub mod script;
pub mod verify;

use std::path::{Path, PathBuf};

use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Tool(&'static str, String),
    Xml(String),
    Zip(String),
    Empty,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Tool(t, e) => write!(f, "{t}: {e} (is poppler-utils installed?)"),
            Error::Xml(e) => write!(f, "the XML of pdftohtml: {e}"),
            Error::Zip(e) => write!(f, "writing the EPUB: {e}"),
            Error::Empty => write!(f, "the PDF has no pages"),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone)]
pub struct Options {
    pub title: Option<String>,
    pub author: Option<String>,
    /// The language of the book (BCP 47); without it German, or the language of the main script when that is not Latin.
    pub language: Option<String>,
    pub cover: bool,
    /// Font files given for a script (ISO 15924 code such as `Grek`, `Syrc`) or for [`fonts::TEXT`].
    pub fonts: Vec<(String, PathBuf)>,
    /// The language a script is marked with, instead of [`script::default_lang`] (`Grek` → `el` for modern Greek).
    pub script_langs: Vec<(String, String)>,
    /// Look up free fonts with fontconfig when none is given.
    pub find_fonts: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { title: None, author: None, language: None, cover: true, fonts: Vec::new(), script_langs: Vec::new(), find_fonts: true }
    }
}

/// A script the book uses (or the group of the rare characters of its main text) and how it is shown.
#[derive(Debug, Clone, Default)]
pub struct ScriptUse {
    /// ISO 15924 code, or [`fonts::TEXT`].
    pub code: String,
    pub name: String,
    /// The language it is marked with (empty for the main text).
    pub lang: String,
    pub rtl: bool,
    /// The different characters of it in the book.
    pub chars: usize,
    pub font: Option<String>,
    /// The characters no embedded font has.
    pub missing: Vec<char>,
}

/// What the conversion found, to see at a glance whether the apparatus is whole.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub pages: usize,
    pub files: usize,
    pub headings: usize,
    pub nav_entries: usize,
    pub toc_entries: usize,
    pub toc_to_headings: usize,
    pub page_marks: usize,
    pub notes: usize,
    pub refs: usize,
    pub refs_linked: usize,
    pub notes_without_ref: usize,
    pub bibliography: usize,
    pub glossary: usize,
    pub index_entries: usize,
    /// The language of the book, its main script and its direction.
    pub language: String,
    pub main_script: String,
    pub rtl: bool,
    pub scripts: Vec<ScriptUse>,
    pub fonts: Vec<String>,
    pub problems: Vec<String>,
    /// What is worth knowing but does not make the EPUB faulty.
    pub warnings: Vec<String>,
}

/// Title and author from the file name "Author - Title".
pub fn names_from_file(path: &Path) -> (Option<String>, Option<String>) {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    match stem.split_once(" - ") {
        Some((a, t)) => (Some(t.trim().to_string()), Some(a.trim().to_string())),
        None => (Some(stem.to_string()).filter(|s| !s.is_empty()), None),
    }
}

fn now_utc() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Civil date from days since 1970 (Howard Hinnant's algorithm).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// All the texts of the book.
fn texts(b: &book::Book) -> Vec<&str> {
    fn see<'a>(out: &mut Vec<&'a str>, c: &'a [book::Inline]) {
        for i in c {
            if let book::Inline::Text { text, .. } = i {
                out.push(text.as_str());
            }
        }
    }
    let mut out = Vec::new();
    for p in b.front.iter().chain(&b.blocks) {
        match &p.block {
            book::Block::Heading(x) => see(&mut out, &x.content),
            book::Block::Para { content, .. } => see(&mut out, content),
            book::Block::Entry { term, desc } => {
                see(&mut out, term);
                see(&mut out, desc);
            }
            book::Block::PageMark(_) => {}
        }
    }
    for n in &b.notes {
        see(&mut out, &n.content);
    }
    out.extend(b.toc.iter().map(|e| e.title.as_str()));
    out
}

/// The main script of the book: the one most of its letters belong to.
fn main_script(texts: &[&str]) -> &'static str {
    let mut count: BTreeMap<&'static str, usize> = BTreeMap::new();
    for c in texts.iter().flat_map(|t| t.chars()) {
        if let Some(s) = script::of(c) {
            *count.entry(s).or_default() += 1;
        }
    }
    count.into_iter().max_by_key(|(_, n)| *n).map_or("Latn", |(s, _)| s)
}

/// The characters of each foreign script, and those of the main text that need a font of their own.
fn script_chars(texts: &[&str], main: &str) -> BTreeMap<String, BTreeSet<char>> {
    let mut out: BTreeMap<String, BTreeSet<char>> = BTreeMap::new();
    for t in texts {
        for (s, run) in script::runs(t, main) {
            for c in run.chars().filter(|c| !c.is_whitespace() && !c.is_control()) {
                match s {
                    Some(code) if script::of(c) == Some(code) || script::dir(c) == script::Dir::Mark => {
                        out.entry(code.to_string()).or_default().insert(c);
                    }
                    _ if main != "Latn" && script::of(c) == Some(main) => {
                        out.entry(fonts::TEXT.to_string()).or_default().insert(c);
                    }
                    _ if !script::is_common(c) => {
                        out.entry(fonts::TEXT.to_string()).or_default().insert(c);
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

fn hex(cs: &[char]) -> String {
    let mut s: Vec<String> = cs.iter().take(12).map(|c| format!("U+{:04X} {c}", *c as u32)).collect();
    if cs.len() > 12 {
        s.push(format!("and {} more", cs.len() - 12));
    }
    s.join(", ")
}

/// Converts the XML of `pdftohtml -xml` (with the cover picture, if any).
pub fn convert_xml(xml: &str, cover: Option<&[u8]>, title: &str, author: &str, source: &str, opts: &Options) -> Result<(Vec<u8>, Report), Error> {
    let pdf = pdf::parse(xml)?;
    if pdf.pages.is_empty() {
        return Err(Error::Empty);
    }
    let book = book::build(&pdf);
    let texts = texts(&book);
    let main = main_script(&texts);
    let language = opts.language.clone().unwrap_or_else(|| if main == "Latn" { "de".into() } else { script::default_lang(main) });
    let mut langs: BTreeMap<String, String> = BTreeMap::new();
    let chars = script_chars(&texts, main);
    for code in chars.keys().filter(|c| c.as_str() != fonts::TEXT) {
        langs.insert(code.clone(), script::default_lang(code));
    }
    for (code, lang) in &opts.script_langs {
        langs.insert(code.clone(), lang.clone());
    }
    let mut embedded = Vec::new();
    let mut uses = Vec::new();
    let mut problems = Vec::new();
    let mut warnings = Vec::new();
    for (group, set) in &chars {
        let given = opts.fonts.iter().find(|(g, _)| g == group).map(|(_, p)| p);
        let font = match given {
            Some(p) => {
                let f = fonts::load(group, p, set);
                if f.is_none() {
                    problems.push(format!("{}: the font {} cannot be read (TTF, OTF, WOFF or WOFF2)", p.display(), group));
                }
                f
            }
            None if opts.find_fonts => fonts::find(group, set),
            None => None,
        };
        let name = if group == fonts::TEXT { format!("rare characters of the main text ({})", script::name(main).replace('_', " ")) } else { script::name(group).replace('_', " ") };
        let missing: Vec<char> = match &font {
            Some(f) => f.missing.clone(),
            None => set.iter().copied().collect(),
        };
        if let Some(f) = font.as_ref().filter(|f| !f.checked) {
            warnings.push(format!("{name}: the characters of {} could not be checked", f.source));
        }
        if !missing.is_empty() {
            let msg = format!("{name}: {} characters without an embedded font: {}", missing.len(), hex(&missing));
            // Without a font search (and none given) it was asked for; still worth saying.
            if opts.find_fonts || given.is_some() { problems.push(msg) } else { warnings.push(msg) }
        }
        uses.push(ScriptUse {
            code: group.clone(),
            name,
            lang: langs.get(group).cloned().unwrap_or_default(),
            rtl: group != fonts::TEXT && script::is_rtl(group),
            chars: set.len(),
            font: font.as_ref().map(|f| f.source.clone()),
            missing,
        });
        embedded.extend(font);
    }
    // One file for a font that serves several groups.
    for i in 1..embedded.len() {
        if let Some(j) = (0..i).find(|&j| embedded[j].source == embedded[i].source) {
            embedded[i].file_name = embedded[j].file_name.clone();
        }
    }
    problems.extend(pdf.unreadable());
    let meta = epub::Meta { title, author, language: &language, source, modified: now_utc(), main_script: main, langs: &langs };
    let (bytes, mut report) = epub::write(&book, cover, &embedded, &meta)?;
    report.language = language.clone();
    report.main_script = main.to_string();
    report.rtl = book.rtl;
    report.scripts = uses;
    report.problems = problems;
    report.problems.extend(verify::check(&bytes));
    report.warnings = warnings;
    Ok((bytes, report))
}

/// Converts a PDF file.
pub fn convert_pdf(path: &Path, opts: &Options) -> Result<(Vec<u8>, Report), Error> {
    let dir = std::env::temp_dir().join(format!("pdf2epub-{}-{}", std::process::id(), now_utc().replace(':', "")));
    std::fs::create_dir_all(&dir).map_err(Error::Io)?;
    let result = (|| {
        let xml = pdf::to_xml(path, &dir)?;
        let (info_title, info_author) = pdf::info(path);
        let (file_title, file_author) = names_from_file(path);
        let title = opts.title.clone().or(info_title).or(file_title).unwrap_or_else(|| "Untitled".into());
        let author = opts.author.clone().or(info_author).or(file_author).unwrap_or_default();
        let cover = if opts.cover { pdf::render_page(path, 1, &dir) } else { None };
        let source = path.file_name().and_then(|s| s.to_str()).unwrap_or_default().to_string();
        convert_xml(&xml, cover.as_deref(), &title, &author, &source, opts)
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}
