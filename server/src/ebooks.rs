// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The e-books (docs/spec-ebooks.md; project owner, 2026-11-09): the EPUB and PDF files of the e-book folders (`ebook_roots`), read by
//! the reader of the app. Who sees which e-book is decided as for the audiobooks ([`EBOOK_VISIBLE`]).
//!
//! An EPUB is not shown as it is: the server makes a text of it ([`Text`]), chapter by chapter, block by block (headings, paragraphs,
//! quotations, list items, pictures), with the emphasis of the words. The app draws that text itself, so a passage can be found again by
//! its chapter and block (`epub:<chapter>:<block>`), and the references to the Bible in it can be found by the server. A PDF is shown as
//! its pages by the app; the server gives the file and its first page as the cover (`pdftoppm`, poppler-utils, when it is installed).

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::store::Store;

/// Which e-books a person sees, as `VISIBLE` for the books: an administrator all of them; the others the released ones within their age
/// limit, and the exceptions given to them or to their role. A file that is gone is not listed. Needs the aliases `e` (ebook) and `p`
/// (profile).
pub(crate) const EBOOK_VISIBLE: &str = "(e.missing = 0 AND (p.role = 'admin' \
     OR (e.released = 1 AND (p.max_age IS NULL OR e.min_age <= p.max_age)) \
     OR EXISTS (SELECT 1 FROM ebook_release r WHERE r.ebook_id = e.id AND (r.profile_id = p.id OR r.role = p.role))))";

/// The largest file of the e-book folders that is read (an EPUB is unpacked in memory).
const MAX_FILE: u64 = 512 * 1024 * 1024;
/// The largest single entry of an EPUB that is unpacked (a chapter, a picture).
const MAX_ENTRY: u64 = 64 * 1024 * 1024;

/// An e-book as the lists show it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Ebook {
    pub id: i64,
    pub format: String,
    pub title: String,
    pub author: String,
    pub language: Option<String>,
    pub size_bytes: i64,
    /// When the file changed last (ms); a device that keeps a copy loads it again when this changes.
    pub mtime_ms: i64,
    pub released: bool,
    pub min_age: i64,
    pub added_at: i64,
}

/// Where a person is in an e-book.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Progress {
    pub locator: String,
    pub percent: f64,
    /// When the person was there (ms, as the device says); the newer report wins.
    pub read_at: i64,
}

/// What the scan of the e-book folders did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Scanned {
    pub found: usize,
    pub new: usize,
    pub changed: usize,
    pub gone: usize,
}

// ---- The text of an EPUB ------------------------------------------------------------------------------------------------------------

/// The text of an EPUB for the reader: its chapters in the order of the book (the spine), and the table of contents.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Text {
    pub title: String,
    pub chapters: Vec<Chapter>,
    pub toc: Vec<TocEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Chapter {
    /// The name of the chapter: its entry in the table of contents, else its first heading, else empty.
    pub title: String,
    /// Its file inside the EPUB, for the links of the table of contents.
    pub href: String,
    pub blocks: Vec<Block>,
}

/// A block of a chapter: `h1` … `h3`, `p`, `quote`, `li`, `pre`, `img` (with `src`, the name inside the EPUB, and `alt` as its text), `hr`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Block {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spans: Vec<Span>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
}

/// A run of words with one emphasis.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Span {
    pub t: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub b: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub i: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sup: bool,
}

/// An entry of the table of contents: its name, the chapter (index into `chapters`) and its level (0 the top).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TocEntry {
    pub title: String,
    pub chapter: usize,
    pub level: u8,
}

/// What the package of an EPUB says (`content.opf`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Package {
    pub title: String,
    pub author: String,
    pub language: Option<String>,
    /// The name of the cover picture inside the EPUB.
    pub cover: Option<String>,
    /// The chapters in order: their names inside the EPUB.
    pub spine: Vec<String>,
    /// Every file the package names, by its name inside the EPUB, with its media type.
    pub manifest: HashMap<String, String>,
    /// The navigation document (EPUB 3) or the NCX (EPUB 2).
    pub nav: Option<String>,
    pub ncx: Option<String>,
}

/// Why an EPUB cannot be read.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EpubError {
    #[error("not an EPUB: {0}")]
    Broken(String),
    /// Adobe ADEPT, Readium LCP or another protection (`META-INF/encryption.xml` with encrypted content): never read.
    #[error("the book is copy protected")]
    Protected,
}

/// An EPUB opened in memory.
pub struct Epub {
    zip: zip::ZipArchive<std::io::Cursor<Vec<u8>>>,
    pub package: Package,
}

fn broken(e: impl std::fmt::Display) -> EpubError {
    EpubError::Broken(e.to_string())
}

/// The XML options: no DTD from outside, a limit against documents that blow up.
fn xml(text: &str) -> std::result::Result<roxmltree::Document<'_>, EpubError> {
    let opt = roxmltree::ParsingOptions { allow_dtd: true, nodes_limit: 2_000_000, ..roxmltree::ParsingOptions::default() };
    roxmltree::Document::parse_with_options(text, opt).map_err(broken)
}

/// Joins a name relative to the file it stands in (`OEBPS/text/c1.xhtml` + `../img/a.jpg` = `OEBPS/img/a.jpg`); `None` when it leads out.
pub fn join(base: &str, rel: &str) -> Option<String> {
    let rel = rel.split(['#', '?']).next().unwrap_or_default();
    let rel = percent_decode(rel);
    if rel.is_empty() {
        return None;
    }
    let mut parts: Vec<&str> = if rel.starts_with('/') { Vec::new() } else { base.split('/').collect() };
    if !rel.starts_with('/') {
        parts.pop();
    }
    for p in rel.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p),
        }
    }
    Some(parts.join("/"))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(n) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(n);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl Epub {
    pub fn open(bytes: Vec<u8>) -> std::result::Result<Epub, EpubError> {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(broken)?;
        if let Ok(enc) = read_entry(&mut zip, "META-INF/encryption.xml") {
            // Fonts are often "obfuscated" (a mangling, not a protection, and listed here too); anything else encrypted is protected.
            let text = String::from_utf8_lossy(&enc).into_owned();
            let doc = xml(&text)?;
            let protected = doc.descendants().filter(|n| n.has_tag_name("EncryptionMethod")).any(|n| {
                let alg = n.attribute("Algorithm").unwrap_or_default();
                !alg.contains("obfuscation") && !alg.contains("font")
            });
            if protected {
                return Err(EpubError::Protected);
            }
        }
        let container = String::from_utf8_lossy(&read_entry(&mut zip, "META-INF/container.xml")?).into_owned();
        let opf = {
            let doc = xml(&container)?;
            doc.descendants()
                .find(|n| n.has_tag_name("rootfile"))
                .and_then(|n| n.attribute("full-path"))
                .ok_or_else(|| broken("no rootfile"))?
                .to_owned()
        };
        let text = String::from_utf8_lossy(&read_entry(&mut zip, &opf)?).into_owned();
        let package = package(&text, &opf)?;
        Ok(Epub { zip, package })
    }

    /// A file inside the EPUB, when the package names it.
    pub fn entry(&mut self, name: &str) -> Option<(Vec<u8>, String)> {
        let mime = self.package.manifest.get(name)?.clone();
        read_entry(&mut self.zip, name).ok().map(|b| (b, mime))
    }

    /// The text for the reader.
    pub fn text(&mut self) -> Text {
        let toc = self.toc_raw();
        let spine = self.package.spine.clone();
        let mut chapters = Vec::with_capacity(spine.len());
        for href in &spine {
            let Ok(bytes) = read_entry(&mut self.zip, href) else { continue };
            let blocks = chapter_blocks(&String::from_utf8_lossy(&bytes), href);
            if blocks.is_empty() {
                continue;
            }
            let title = toc.iter().find(|(_, h, _)| h == href).map(|(t, _, _)| t.clone()).unwrap_or_else(|| {
                blocks.iter().find(|b| b.kind.starts_with('h')).map(|b| b.spans.iter().map(|s| s.t.as_str()).collect::<String>()).unwrap_or_default()
            });
            chapters.push(Chapter { title, href: href.clone(), blocks });
        }
        let toc = toc
            .into_iter()
            .filter_map(|(title, href, level)| chapters.iter().position(|c| c.href == href).map(|chapter| TocEntry { title, chapter, level }))
            .collect();
        Text { title: self.package.title.clone(), chapters, toc }
    }

    /// The table of contents as (name, file, level), from the navigation document or the NCX.
    fn toc_raw(&mut self) -> Vec<(String, String, u8)> {
        let mut out = Vec::new();
        if let Some(nav) = self.package.nav.clone()
            && let Ok(bytes) = read_entry(&mut self.zip, &nav)
        {
            let text = clean_html(&String::from_utf8_lossy(&bytes));
            if let Ok(doc) = xml(&text) {
                let toc = doc
                    .descendants()
                    .find(|n| n.has_tag_name("nav") && n.attributes().any(|a| a.name() == "type" && a.value().contains("toc")))
                    .or_else(|| doc.descendants().find(|n| n.has_tag_name("nav")));
                if let Some(toc) = toc {
                    for a in toc.descendants().filter(|n| n.has_tag_name("a")) {
                        let level = a.ancestors().filter(|n| n.has_tag_name("ol") || n.has_tag_name("ul")).count().saturating_sub(1) as u8;
                        if let Some(href) = a.attribute("href").and_then(|h| join(&nav, h)) {
                            out.push((squash(&a.descendants().filter(|n| n.is_text()).map(|n| n.text().unwrap_or_default()).collect::<String>()), href, level));
                        }
                    }
                }
            }
        }
        if out.is_empty()
            && let Some(ncx) = self.package.ncx.clone()
            && let Ok(bytes) = read_entry(&mut self.zip, &ncx)
        {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            if let Ok(doc) = xml(&text) {
                for p in doc.descendants().filter(|n| n.has_tag_name("navPoint")) {
                    let level = p.ancestors().filter(|n| n.has_tag_name("navPoint")).count().saturating_sub(1) as u8;
                    let label = p.children().find(|n| n.has_tag_name("navLabel")).map(|l| squash(&l.descendants().filter(|n| n.is_text()).map(|n| n.text().unwrap_or_default()).collect::<String>()));
                    let src = p.children().find(|n| n.has_tag_name("content")).and_then(|c| c.attribute("src")).and_then(|s| join(&ncx, s));
                    if let (Some(label), Some(src)) = (label, src) {
                        out.push((label, src, level));
                    }
                }
            }
        }
        out
    }
}

fn read_entry(zip: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>, name: &str) -> std::result::Result<Vec<u8>, EpubError> {
    let f = zip.by_name(name).map_err(broken)?;
    if f.size() > MAX_ENTRY {
        return Err(broken("an entry is too large"));
    }
    let mut out = Vec::with_capacity(f.size() as usize);
    f.take(MAX_ENTRY).read_to_end(&mut out).map_err(broken)?;
    Ok(out)
}

/// Reads `content.opf`.
pub fn package(text: &str, opf: &str) -> std::result::Result<Package, EpubError> {
    let doc = xml(text)?;
    let meta = |tag: &str| -> Option<String> {
        doc.descendants().find(|n| n.tag_name().name() == tag && n.tag_name().namespace().is_some_and(|ns| ns.contains("purl.org/dc"))).and_then(|n| n.text()).map(squash).filter(|s| !s.is_empty())
    };
    let mut p = Package { title: meta("title").unwrap_or_default(), author: meta("creator").unwrap_or_default(), language: meta("language"), ..Package::default() };
    let mut ids: HashMap<String, String> = HashMap::new();
    for item in doc.descendants().filter(|n| n.has_tag_name("item")) {
        let (Some(id), Some(href)) = (item.attribute("id"), item.attribute("href")) else { continue };
        let Some(name) = join(opf, href) else { continue };
        let mime = item.attribute("media-type").unwrap_or_default().to_owned();
        let props = item.attribute("properties").unwrap_or_default();
        if props.split_whitespace().any(|x| x == "nav") {
            p.nav = Some(name.clone());
        }
        if props.split_whitespace().any(|x| x == "cover-image") {
            p.cover = Some(name.clone());
        }
        if mime == "application/x-dtbncx+xml" {
            p.ncx = Some(name.clone());
        }
        ids.insert(id.to_owned(), name.clone());
        p.manifest.insert(name, mime);
    }
    if p.cover.is_none() {
        // EPUB 2: <meta name="cover" content="<id>"/>.
        p.cover = doc.descendants().find(|n| n.has_tag_name("meta") && n.attribute("name") == Some("cover")).and_then(|n| n.attribute("content")).and_then(|id| ids.get(id).cloned());
    }
    p.spine = doc.descendants().filter(|n| n.has_tag_name("itemref")).filter_map(|n| n.attribute("idref")).filter_map(|id| ids.get(id).cloned()).collect();
    if p.spine.is_empty() {
        return Err(broken("no chapters"));
    }
    Ok(p)
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// HTML in an EPUB is meant to be XML, but often names entities of HTML (`&nbsp;`) and a DOCTYPE from outside: both are made plain.
fn clean_html(text: &str) -> String {
    let mut s = text.to_owned();
    if let Some(start) = s.find("<!DOCTYPE")
        && let Some(len) = s[start..].find('>')
    {
        s.replace_range(start..start + len + 1, "");
    }
    const NAMED: [(&str, &str); 24] = [
        ("&nbsp;", "\u{a0}"),
        ("&shy;", "\u{ad}"),
        ("&ndash;", "–"),
        ("&mdash;", "—"),
        ("&hellip;", "…"),
        ("&lsquo;", "‘"),
        ("&rsquo;", "’"),
        ("&sbquo;", "‚"),
        ("&ldquo;", "“"),
        ("&rdquo;", "”"),
        ("&bdquo;", "„"),
        ("&laquo;", "«"),
        ("&raquo;", "»"),
        ("&auml;", "ä"),
        ("&ouml;", "ö"),
        ("&uuml;", "ü"),
        ("&Auml;", "Ä"),
        ("&Ouml;", "Ö"),
        ("&Uuml;", "Ü"),
        ("&szlig;", "ß"),
        ("&copy;", "©"),
        ("&middot;", "·"),
        ("&bull;", "•"),
        ("&times;", "×"),
    ];
    for (name, ch) in NAMED {
        if s.contains(name) {
            s = s.replace(name, ch);
        }
    }
    s
}

struct Builder<'a> {
    href: &'a str,
    blocks: Vec<Block>,
    kind: Option<String>,
    spans: Vec<Span>,
}

#[derive(Clone, Copy, Default)]
struct Style {
    b: bool,
    i: bool,
    sup: bool,
    pre: bool,
}

impl Builder<'_> {
    fn start(&mut self, kind: &str) {
        self.flush();
        self.kind = Some(kind.to_owned());
    }

    fn flush(&mut self) {
        let kind = self.kind.take().unwrap_or_else(|| "p".into());
        let mut spans = std::mem::take(&mut self.spans);
        if kind != "pre" {
            // No space at the start and the end of a block.
            if let Some(f) = spans.first_mut() {
                f.t = f.t.trim_start().to_owned();
            }
            if let Some(l) = spans.last_mut() {
                l.t = l.t.trim_end().to_owned();
            }
            spans.retain(|s| !s.t.is_empty());
        }
        if !spans.is_empty() {
            self.blocks.push(Block { kind, spans, src: None });
        }
    }

    fn text(&mut self, t: &str, st: Style) {
        let t: String = if st.pre {
            t.to_owned()
        } else {
            // Spaces as in HTML: every run of white space is one space; the soft hyphen goes.
            let mut out = String::with_capacity(t.len());
            let mut space = self.spans.last().is_some_and(|s| s.t.ends_with(' ')) || self.spans.is_empty();
            for c in t.chars() {
                if c == '\u{ad}' {
                    continue;
                }
                if c.is_whitespace() && c != '\u{a0}' {
                    if !space {
                        out.push(' ');
                        space = true;
                    }
                } else {
                    out.push(c);
                    space = false;
                }
            }
            out
        };
        if t.is_empty() {
            return;
        }
        match self.spans.last_mut() {
            Some(l) if l.b == st.b && l.i == st.i && l.sup == st.sup => l.t.push_str(&t),
            _ => self.spans.push(Span { t, b: st.b, i: st.i, sup: st.sup }),
        }
    }

    fn walk(&mut self, node: roxmltree::Node, st: Style) {
        for c in node.children() {
            if c.is_text() {
                self.text(c.text().unwrap_or_default(), st);
                continue;
            }
            if !c.is_element() {
                continue;
            }
            let tag = c.tag_name().name().to_ascii_lowercase();
            match tag.as_str() {
                "script" | "style" | "head" | "title" => {}
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    let level = match tag.as_str() {
                        "h1" => "h1",
                        "h2" => "h2",
                        _ => "h3",
                    };
                    self.start(level);
                    self.walk(c, st);
                    self.flush();
                }
                "p" | "blockquote" | "li" | "dt" | "dd" | "figcaption" | "caption" | "td" | "th" => {
                    let kind = match tag.as_str() {
                        "blockquote" => "quote",
                        "li" | "dt" | "dd" => "li",
                        _ => "p",
                    };
                    // A quotation of several paragraphs: each of them a quotation.
                    let inner = if self.kind.as_deref() == Some("quote") && kind == "p" { "quote" } else { kind };
                    self.start(inner);
                    if kind == "quote" {
                        self.walk_quote(c, st);
                    } else {
                        self.walk(c, st);
                    }
                    self.flush();
                }
                "pre" => {
                    self.start("pre");
                    self.walk(c, Style { pre: true, ..st });
                    self.flush();
                }
                "hr" => {
                    self.flush();
                    self.blocks.push(Block { kind: "hr".into(), spans: Vec::new(), src: None });
                }
                "br" => self.text(if st.pre { "\n" } else { " " }, st),
                "img" | "image" => {
                    let src = c.attribute("src").or_else(|| c.attributes().find(|a| a.name() == "href").map(|a| a.value()));
                    if let Some(src) = src.and_then(|s| join(self.href, s)) {
                        let kind = self.kind.clone();
                        self.flush();
                        let alt = c.attribute("alt").map(squash).unwrap_or_default();
                        let spans = if alt.is_empty() { Vec::new() } else { vec![Span { t: alt, ..Span::default() }] };
                        self.blocks.push(Block { kind: "img".into(), spans, src: Some(src) });
                        self.kind = kind;
                    }
                }
                "b" | "strong" => self.walk(c, Style { b: true, ..st }),
                "i" | "em" | "cite" | "var" => self.walk(c, Style { i: true, ..st }),
                "sup" => self.walk(c, Style { sup: true, ..st }),
                "div" | "section" | "article" | "aside" | "body" | "html" | "header" | "footer" | "main" | "nav" | "figure" | "ol" | "ul" | "dl" | "table" | "tbody" | "thead" | "tr" | "svg" => {
                    // A box around blocks: what stands loose in it is a paragraph of its own.
                    let kind = self.kind.clone();
                    self.flush();
                    self.walk(c, st);
                    self.flush();
                    self.kind = kind;
                }
                _ => self.walk(c, st),
            }
        }
    }

    fn walk_quote(&mut self, node: roxmltree::Node, st: Style) {
        let has_blocks = node.children().any(|c| c.is_element() && matches!(c.tag_name().name(), "p" | "div"));
        if !has_blocks {
            return self.walk(node, st);
        }
        for c in node.children() {
            if c.is_element() && matches!(c.tag_name().name(), "p" | "div") {
                self.start("quote");
                self.walk(c, st);
                self.flush();
            } else if c.is_text() {
                self.text(c.text().unwrap_or_default(), st);
            }
        }
    }
}

/// The blocks of one chapter (an XHTML file of the EPUB named [href]).
pub fn chapter_blocks(html: &str, href: &str) -> Vec<Block> {
    let text = clean_html(html);
    let Ok(doc) = xml(&text) else { return Vec::new() };
    let body = doc.descendants().find(|n| n.has_tag_name("body")).unwrap_or(doc.root_element());
    let mut b = Builder { href, blocks: Vec::new(), kind: None, spans: Vec::new() };
    b.walk(body, Style::default());
    b.flush();
    b.blocks
}

// ---- The files ----------------------------------------------------------------------------------------------------------------------

/// What the server reads of a file for the list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Meta {
    pub title: String,
    pub author: String,
    pub language: Option<String>,
}

fn has(program: &'static str, cell: &'static OnceLock<bool>) -> bool {
    *cell.get_or_init(|| Command::new(program).arg("-v").output().is_ok())
}

/// Whether `pdfinfo` and `pdftoppm` (poppler-utils) are installed.
pub fn has_poppler() -> bool {
    static INFO: OnceLock<bool> = OnceLock::new();
    has("pdfinfo", &INFO)
}

/// The title and the author of a PDF (`pdfinfo`); empty without poppler-utils.
pub fn pdf_meta(path: &Path) -> Meta {
    let mut m = Meta::default();
    if !has_poppler() {
        return m;
    }
    let Ok(out) = Command::new("pdfinfo").arg("-enc").arg("UTF-8").arg(path).output() else { return m };
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some((k, v)) = line.split_once(':') {
            let v = v.trim().to_owned();
            match k.trim() {
                "Title" => m.title = v,
                "Author" => m.author = v,
                _ => {}
            }
        }
    }
    m
}

/// The title from the name of a file: `Calvin - Institutio.epub` → (`Institutio`, `Calvin`).
fn from_name(path: &Path) -> (String, String) {
    let stem = path.file_stem().map(|s| s.to_string_lossy().replace('_', " ")).unwrap_or_default();
    match stem.split_once(" - ") {
        Some((a, t)) => (t.trim().to_owned(), a.trim().to_owned()),
        None => (stem.trim().to_owned(), String::new()),
    }
}

/// What the list says of a file: from the file, else from its name. An EPUB that cannot be read or is protected is `Err`.
pub fn meta_of(path: &Path, format: &str) -> std::result::Result<Meta, EpubError> {
    let mut m = if format == "epub" {
        let bytes = std::fs::read(path).map_err(broken)?;
        let e = Epub::open(bytes)?;
        Meta { title: e.package.title, author: e.package.author, language: e.package.language }
    } else {
        pdf_meta(path)
    };
    let (title, author) = from_name(path);
    if m.title.trim().is_empty() {
        m.title = title;
    }
    if m.author.trim().is_empty() {
        m.author = author;
    }
    Ok(m)
}

fn format_of(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "epub" => Some("epub"),
        "pdf" => Some("pdf"),
        _ => None,
    }
}

/// The e-book files below [root], leaving out [skip] (folders that are left out, or roots of their own); hidden files and folders too.
pub fn files(root: &Path, skip: &[PathBuf]) -> Vec<(PathBuf, &'static str, u64, i64)> {
    let mut out = Vec::new();
    let mut todo = vec![root.to_path_buf()];
    while let Some(dir) = todo.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            if e.file_name().to_string_lossy().starts_with('.') || skip.iter().any(|s| path.starts_with(s)) {
                continue;
            }
            let Ok(md) = e.metadata() else { continue };
            if md.is_dir() {
                todo.push(path);
            } else if let Some(format) = format_of(&path)
                && md.len() <= MAX_FILE
            {
                let mtime = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64).unwrap_or(0);
                out.push((path, format, md.len(), mtime));
            }
        }
    }
    out.sort();
    out
}

fn sql<const N: usize>(text: &str, values: [Value; N]) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, text, values)
}

const COLUMNS: &str = "e.id, e.format, e.title, e.author, e.language, e.size_bytes, e.mtime_ms, e.released, e.min_age, e.added_at";

fn row(r: &sea_orm::QueryResult) -> std::result::Result<Ebook, sea_orm::DbErr> {
    Ok(Ebook {
        id: r.try_get("", "id")?,
        format: r.try_get("", "format")?,
        title: r.try_get("", "title")?,
        author: r.try_get("", "author")?,
        language: r.try_get("", "language")?,
        size_bytes: r.try_get("", "size_bytes")?,
        mtime_ms: r.try_get("", "mtime_ms")?,
        released: r.try_get::<i64>("", "released")? == 1,
        min_age: r.try_get("", "min_age")?,
        added_at: r.try_get("", "added_at")?,
    })
}

fn now_ms() -> i64 {
    (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}

impl Store {
    /// Reads the e-book folders: new files come in (not released: an administrator releases them, as audiobooks without an age), a
    /// changed file is read again, a file that is gone is marked so (its positions stay, in case it comes back).
    pub async fn scan_ebooks(&self, roots: &[PathBuf], skip: &[PathBuf]) -> Result<Scanned> {
        let mut found = Vec::new();
        for root in roots {
            let (root, skip) = (root.clone(), skip.to_vec());
            found.extend(tokio::task::spawn_blocking(move || files(&root, &skip)).await.unwrap_or_default());
        }
        let known: HashMap<String, (i64, i64, i64)> = self
            .db
            .reader
            .query_all_raw(sql("SELECT path, size_bytes, mtime_ms, missing FROM ebook", []))
            .await?
            .iter()
            .filter_map(|r| Some((r.try_get::<String>("", "path").ok()?, (r.try_get("", "size_bytes").ok()?, r.try_get("", "mtime_ms").ok()?, r.try_get("", "missing").ok()?))))
            .collect();
        let mut report = Scanned { found: found.len(), ..Scanned::default() };
        let mut seen = std::collections::HashSet::new();
        for (path, format, size, mtime) in found {
            let key = path.to_string_lossy().into_owned();
            seen.insert(key.clone());
            let before = known.get(&key).copied();
            if let Some((s, m, missing)) = before
                && s == size as i64
                && m == mtime
            {
                if missing == 1 {
                    self.db.writer.execute_raw(sql("UPDATE ebook SET missing = 0 WHERE path = ?", [key.into()])).await?;
                }
                continue;
            }
            let p = path.clone();
            let meta = match tokio::task::spawn_blocking(move || meta_of(&p, format)).await {
                Ok(Ok(m)) => m,
                Ok(Err(e)) => {
                    tracing::warn!(path = %path.display(), error = %e, "an e-book is left out");
                    continue;
                }
                Err(_) => continue,
            };
            self.db
                .writer
                .execute_raw(sql(
                    "INSERT INTO ebook (path, format, title, author, language, size_bytes, mtime_ms, added_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
                     ON CONFLICT (path) DO UPDATE SET format = excluded.format, title = excluded.title, author = excluded.author, \
                     language = excluded.language, size_bytes = excluded.size_bytes, mtime_ms = excluded.mtime_ms, missing = 0",
                    [key.into(), format.into(), meta.title.into(), meta.author.into(), meta.language.into(), (size as i64).into(), mtime.into(), now_ms().into()],
                ))
                .await?;
            if before.is_some() {
                report.changed += 1;
            } else {
                report.new += 1;
            }
        }
        for (path, (_, _, missing)) in known {
            if missing == 0 && !seen.contains(&path) {
                self.db.writer.execute_raw(sql("UPDATE ebook SET missing = 1 WHERE path = ?", [path.into()])).await?;
                report.gone += 1;
            }
        }
        Ok(report)
    }

    /// The e-books [profile] sees, by author and title.
    pub async fn ebooks(&self, profile: i64) -> Result<Vec<Ebook>> {
        let rows = self
            .db
            .reader
            .query_all_raw(sql(
                &format!("SELECT {COLUMNS} FROM ebook e JOIN profile p ON p.id = ? WHERE {EBOOK_VISIBLE} ORDER BY e.author COLLATE NOCASE, e.title COLLATE NOCASE, e.id"),
                [profile.into()],
            ))
            .await?;
        Ok(rows.iter().map(row).collect::<std::result::Result<_, _>>()?)
    }

    /// One e-book [profile] sees, with its path; `None` when it is not there for the person (the same as when it does not exist).
    pub async fn ebook(&self, profile: i64, id: i64) -> Result<Option<(Ebook, PathBuf)>> {
        let r = self
            .db
            .reader
            .query_one_raw(sql(&format!("SELECT {COLUMNS}, e.path FROM ebook e JOIN profile p ON p.id = ? WHERE e.id = ? AND {EBOOK_VISIBLE}"), [profile.into(), id.into()]))
            .await?;
        let Some(r) = r else { return Ok(None) };
        let path: String = r.try_get("", "path")?;
        Ok(Some((row(&r)?, PathBuf::from(path))))
    }

    /// Every e-book for the administrator, the ones not yet released first.
    pub async fn all_ebooks(&self) -> Result<Vec<Ebook>> {
        let rows = self.db.reader.query_all_raw(sql(&format!("SELECT {COLUMNS} FROM ebook e WHERE e.missing = 0 ORDER BY e.released, e.author COLLATE NOCASE, e.title COLLATE NOCASE"), [])).await?;
        Ok(rows.iter().map(row).collect::<std::result::Result<_, _>>()?)
    }

    /// Releases an e-book (or takes the release back) and sets the age it is for; false when there is no such e-book.
    pub async fn release_ebook(&self, id: i64, released: bool, min_age: i64) -> Result<bool> {
        let r = self.db.writer.execute_raw(sql("UPDATE ebook SET released = ?, min_age = ? WHERE id = ?", [i64::from(released).into(), min_age.clamp(0, 99).into(), id.into()])).await?;
        Ok(r.rows_affected() == 1)
    }

    pub async fn ebook_progress(&self, profile: i64, id: i64) -> Result<Option<Progress>> {
        let r = self.db.reader.query_one_raw(sql("SELECT locator, percent, read_at FROM ebook_progress WHERE profile_id = ? AND ebook_id = ?", [profile.into(), id.into()])).await?;
        let Some(r) = r else { return Ok(None) };
        Ok(Some(Progress { locator: r.try_get("", "locator")?, percent: r.try_get("", "percent")?, read_at: r.try_get("", "read_at")? }))
    }

    /// Notes where a person is; an older report than the one kept changes nothing.
    pub async fn save_ebook_progress(&self, profile: i64, id: i64, p: &Progress) -> Result<()> {
        self.db
            .writer
            .execute_raw(sql(
                "INSERT INTO ebook_progress (profile_id, ebook_id, locator, percent, read_at) VALUES (?, ?, ?, ?, ?) \
                 ON CONFLICT (profile_id, ebook_id) DO UPDATE SET locator = excluded.locator, percent = excluded.percent, read_at = excluded.read_at \
                 WHERE excluded.read_at >= ebook_progress.read_at",
                [profile.into(), id.into(), p.locator.clone().into(), p.percent.clamp(0.0, 1.0).into(), p.read_at.into()],
            ))
            .await?;
        Ok(())
    }

    /// The positions of a person in all e-books, for the list (`{id: progress}`).
    pub async fn ebook_progresses(&self, profile: i64) -> Result<HashMap<i64, Progress>> {
        let rows = self.db.reader.query_all_raw(sql("SELECT ebook_id, locator, percent, read_at FROM ebook_progress WHERE profile_id = ?", [profile.into()])).await?;
        let mut out = HashMap::new();
        for r in rows {
            out.insert(r.try_get("", "ebook_id")?, Progress { locator: r.try_get("", "locator")?, percent: r.try_get("", "percent")?, read_at: r.try_get("", "read_at")? });
        }
        Ok(out)
    }

    /// The text of an EPUB for the reader, made once per version of the file and kept in the data folder.
    pub async fn ebook_text(&self, e: &Ebook, path: &Path) -> Result<Option<Vec<u8>>> {
        if e.format != "epub" {
            return Ok(None);
        }
        let cache = self.data_dir.join("ebooks").join(format!("{}-{}.json", e.id, e.mtime_ms));
        if let Ok(b) = tokio::fs::read(&cache).await {
            return Ok(Some(b));
        }
        let path = path.to_path_buf();
        let made = tokio::task::spawn_blocking(move || -> Option<Vec<u8>> {
            let mut epub = Epub::open(std::fs::read(&path).ok()?).ok()?;
            serde_json::to_vec(&epub.text()).ok()
        })
        .await
        .ok()
        .flatten();
        let Some(bytes) = made else { return Ok(None) };
        if let Some(dir) = cache.parent() {
            let _ = tokio::fs::create_dir_all(dir).await;
            // Older versions of the same book go.
            if let Ok(mut rd) = tokio::fs::read_dir(dir).await {
                while let Ok(Some(f)) = rd.next_entry().await {
                    if f.file_name().to_string_lossy().starts_with(&format!("{}-", e.id)) {
                        let _ = tokio::fs::remove_file(f.path()).await;
                    }
                }
            }
            let temp = dir.join(format!(".{}.json", uuid::Uuid::new_v4().simple()));
            if tokio::fs::write(&temp, &bytes).await.is_ok() {
                let _ = tokio::fs::rename(&temp, &cache).await;
            }
        }
        Ok(Some(bytes))
    }

    /// The cover of an e-book as (bytes, mime): the cover picture of an EPUB, the first page of a PDF (made once, kept).
    pub async fn ebook_cover(&self, e: &Ebook, path: &Path) -> Option<(Vec<u8>, String)> {
        let path = path.to_path_buf();
        if e.format == "epub" {
            return tokio::task::spawn_blocking(move || {
                let mut epub = Epub::open(std::fs::read(&path).ok()?).ok()?;
                let cover = epub.package.cover.clone()?;
                epub.entry(&cover)
            })
            .await
            .ok()
            .flatten();
        }
        let cache = self.data_dir.join("ebooks").join(format!("{}-{}-cover.jpg", e.id, e.mtime_ms));
        if let Ok(b) = tokio::fs::read(&cache).await {
            return Some((b, "image/jpeg".into()));
        }
        static PPM: OnceLock<bool> = OnceLock::new();
        if !has("pdftoppm", &PPM) {
            return None;
        }
        let target = cache.clone();
        tokio::task::spawn_blocking(move || -> Option<()> {
            std::fs::create_dir_all(target.parent()?).ok()?;
            let stem = target.with_extension("");
            let ok = Command::new("pdftoppm").args(["-jpeg", "-f", "1", "-l", "1", "-scale-to", "600", "-singlefile"]).arg(&path).arg(&stem).status().ok()?.success();
            ok.then_some(())
        })
        .await
        .ok()
        .flatten()?;
        tokio::fs::read(&cache).await.ok().map(|b| (b, "image/jpeg".into()))
    }

    /// A file inside an EPUB (a picture of the text), when the package names it.
    pub async fn ebook_resource(&self, path: &Path, name: &str) -> Option<(Vec<u8>, String)> {
        let (path, name) = (path.to_path_buf(), name.to_owned());
        tokio::task::spawn_blocking(move || Epub::open(std::fs::read(&path).ok()?).ok()?.entry(&name)).await.ok().flatten()
    }
}
