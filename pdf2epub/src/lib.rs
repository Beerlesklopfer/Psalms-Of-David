// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! pdf2epub: a typeset PDF book as EPUB 3, with its scholarly apparatus whole: the table of contents as printed (with links) and
//! as navigation by the headings, the footnotes linked both ways, the printed page numbers as page list (to cite by the page of
//! the print), the bibliography, the list of abbreviations, an index with its page numbers linked. Text is Unicode throughout;
//! Greek (also from the BibleWorks fonts) and Hebrew are marked with their language and shown in embedded free fonts.
//!
//! The PDF is read with poppler-utils (`pdftohtml`, `pdfinfo`, `pdftoppm`).

pub mod book;
pub mod epub;
pub mod fonts;
pub mod greek;
pub mod layout;
pub mod pdf;
pub mod verify;

use std::path::{Path, PathBuf};

use fonts::Script;

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
    pub language: String,
    pub cover: bool,
    pub font_greek: Option<PathBuf>,
    pub font_hebrew: Option<PathBuf>,
    /// Look up free fonts with fontconfig when none is given.
    pub find_fonts: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { title: None, author: None, language: "de".into(), cover: true, font_greek: None, font_hebrew: None, find_fonts: true }
    }
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
    pub greek: bool,
    pub hebrew: bool,
    pub fonts: Vec<String>,
    pub problems: Vec<String>,
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

/// Which scripts the book uses.
fn scripts(b: &book::Book) -> (bool, bool) {
    let (mut g, mut h) = (false, false);
    let mut see = |c: &[book::Inline]| {
        for i in c {
            if let book::Inline::Text { text, greek, .. } = i {
                g |= *greek || text.chars().any(|c| Script::Greek.has(c));
                h |= text.chars().any(|c| Script::Hebrew.has(c));
            }
        }
    };
    for p in b.front.iter().chain(&b.blocks) {
        match &p.block {
            book::Block::Heading(x) => see(&x.content),
            book::Block::Para { content, .. } => see(content),
            book::Block::Entry { term, desc } => {
                see(term);
                see(desc);
            }
            book::Block::PageMark(_) => {}
        }
    }
    for n in &b.notes {
        see(&n.content);
    }
    (g, h)
}

/// Converts the XML of `pdftohtml -xml` (with the cover picture, if any).
pub fn convert_xml(xml: &str, cover: Option<&[u8]>, title: &str, author: &str, source: &str, opts: &Options) -> Result<(Vec<u8>, Report), Error> {
    let pdf = pdf::parse(xml)?;
    if pdf.pages.is_empty() {
        return Err(Error::Empty);
    }
    let book = book::build(&pdf);
    let (greek, hebrew) = scripts(&book);
    let mut embedded = Vec::new();
    for (used, script, given) in [(greek, Script::Greek, &opts.font_greek), (hebrew, Script::Hebrew, &opts.font_hebrew)] {
        if !used {
            continue;
        }
        let font = match given {
            Some(p) => fonts::load(script, p),
            None if opts.find_fonts => fonts::find(script),
            None => None,
        };
        embedded.extend(font);
    }
    let meta = epub::Meta { title, author, language: &opts.language, source, modified: now_utc() };
    let (bytes, mut report) = epub::write(&book, cover, &embedded, &meta)?;
    report.greek = greek;
    report.hebrew = hebrew;
    report.problems = verify::check(&bytes);
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
