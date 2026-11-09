// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Writes the book as EPUB 3 (with an NCX for older readers): one file per part and chapter, the footnotes at the end of the file
//! of their mark (`epub:type="noteref"`/`"footnote"`, so readers show them as pop-ups, with a link back), the printed pages as
//! page breaks and page list, the printed table of contents with links, the navigation by the headings, landmarks for contents,
//! text, bibliography, abbreviations and index. All text is UTF-8; every foreign script is marked with its language and, where it
//! runs the other way, its direction, and shown in an embedded font. A book written from right to left is a right-to-left EPUB.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;

use regex::Regex;

use crate::book::{Block, Book, Heading, Inline, Kind, Placed, Section, plain};
use crate::fonts::{self, Font};
use crate::script;
use crate::{Error, Report};

pub struct Meta<'a> {
    pub title: &'a str,
    pub author: &'a str,
    pub language: &'a str,
    pub source: &'a str,
    pub modified: String,
    /// The script most of the book is in (ISO 15924).
    pub main_script: &'a str,
    /// The language each foreign script is marked with.
    pub langs: &'a BTreeMap<String, String>,
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            _ => o.push(c),
        }
    }
    o
}

struct Words {
    contents: &'static str,
    front: &'static str,
    cover: &'static str,
    notes: &'static str,
    pages: &'static str,
    start: &'static str,
    landmarks: &'static str,
}

fn words(lang: &str) -> Words {
    if lang.starts_with("de") {
        Words { contents: "Inhalt", front: "Titelei", cover: "Umschlag", notes: "Anmerkungen", pages: "Seiten", start: "Beginn des Textes", landmarks: "Orientierung" }
    } else {
        Words { contents: "Contents", front: "Front matter", cover: "Cover", notes: "Notes", pages: "Pages", start: "Start of text", landmarks: "Landmarks" }
    }
}

/// One XHTML file of the book.
struct File {
    name: String,
    title: String,
    section: Section,
    blocks: Vec<usize>,
}

struct Links {
    heading: HashMap<usize, usize>,
    page: HashMap<u32, usize>,
    note: HashMap<usize, usize>,
    reference: HashMap<usize, usize>,
}

struct Ctx<'a> {
    book: &'a Book,
    files: &'a [File],
    links: &'a Links,
    page_ref: &'a Regex,
    index_num: &'a Regex,
    main: &'a str,
    langs: &'a BTreeMap<String, String>,
}

impl Ctx<'_> {
    fn href(&self, from: usize, to: usize, id: &str) -> String {
        if from == to { format!("#{id}") } else { format!("{}#{id}", self.files[to].name) }
    }

    /// Text with the foreign scripts marked: each run in a span with its language, its font and (where it runs the other way
    /// than the book) its direction, which also keeps a right-to-left phrase together in left-to-right text.
    fn scripts(&self, text: &str) -> String {
        let mut out = String::new();
        for (s, run) in script::runs(text, self.main) {
            match s {
                Some(code) => {
                    let lang = self.langs.get(code).cloned().unwrap_or_else(|| script::default_lang(code));
                    let dir = match (script::is_rtl(code), self.book.rtl) {
                        (true, false) => " dir=\"rtl\"",
                        (false, true) => " dir=\"ltr\"",
                        _ => "",
                    };
                    out.push_str(&format!("<span class=\"sc-{code}\" lang=\"{lang}\" xml:lang=\"{lang}\"{dir}>{}</span>", esc(run)));
                }
                None => out.push_str(&esc(run)),
            }
        }
        out
    }

    /// Text with the references to printed pages ("S. 123") as links.
    fn text(&self, file: usize, text: &str, section: Section) -> String {
        let re = if section == Section::Index { self.index_num } else { self.page_ref };
        let mut out = String::new();
        let mut last = 0;
        for c in re.captures_iter(text) {
            let m = c.get(0).expect("match");
            let Some(n) = c.name("n").and_then(|n| n.as_str().parse::<u32>().ok()) else { continue };
            if section == Section::Index && text[..m.start()].ends_with(|ch: char| ch.is_ascii_digit() || ch == ',' || ch == '.') {
                continue;
            }
            let Some(&to) = self.links.page.get(&n) else { continue };
            out.push_str(&self.scripts(&text[last..m.start()]));
            out.push_str(&format!("<a href=\"{}\">{}</a>", self.href(file, to, &format!("p{n}")), self.scripts(m.as_str())));
            last = m.end();
        }
        out.push_str(&self.scripts(&text[last..]));
        out
    }

    fn inline(&self, file: usize, content: &[Inline], section: Section) -> String {
        let mut out = String::new();
        for i in content {
            match i {
                Inline::Text { text, bold, italic, .. } => {
                    let mut t = self.text(file, text, section);
                    if *italic {
                        t = format!("<i>{t}</i>");
                    }
                    if *bold {
                        t = format!("<b>{t}</b>");
                    }
                    out.push_str(&t);
                }
                Inline::NoteRef(r) => {
                    let label = esc(&self.book.refs[*r].label);
                    match self.book.refs[*r].note {
                        Some(k) => {
                            let to = self.links.note.get(&k).copied().unwrap_or(file);
                            out.push_str(&format!(
                                "<a epub:type=\"noteref\" role=\"doc-noteref\" id=\"r{k}\" href=\"{}\"><sup>{label}</sup></a>",
                                self.href(file, to, &format!("n{k}"))
                            ));
                        }
                        None => out.push_str(&format!("<sup>{label}</sup>")),
                    }
                }
                Inline::Page(n) => out.push_str(&page_mark(*n)),
            }
        }
        out
    }
}

fn page_mark(n: u32) -> String {
    format!("<span epub:type=\"pagebreak\" role=\"doc-pagebreak\" id=\"p{n}\" aria-label=\"{n}\" title=\"{n}\"></span>")
}

fn section_type(s: Section) -> (&'static str, &'static str) {
    match s {
        Section::Front => ("frontmatter", ""),
        Section::Toc => ("toc", "doc-toc"),
        Section::Part => ("part", "doc-part"),
        Section::Bibliography => ("bibliography", "doc-bibliography"),
        Section::Glossary => ("glossary", "doc-glossary"),
        Section::Index => ("index", "doc-index"),
        Section::Normal => ("chapter", "doc-chapter"),
    }
}

fn xhtml(lang: &str, rtl: bool, title: &str, body: &str) -> String {
    let dir = if rtl { " dir=\"rtl\"" } else { "" };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" lang=\"{lang}\" xml:lang=\"{lang}\"{dir}>\n<head>\n<meta charset=\"UTF-8\"/>\n<title>{}</title>\n<link rel=\"stylesheet\" type=\"text/css\" href=\"../style.css\"/>\n</head>\n<body>\n{body}\n</body>\n</html>\n",
        esc(title)
    )
}

fn css(fonts: &[Font], main: &str, rtl: bool) -> String {
    let mut s = String::new();
    for f in fonts {
        s.push_str(&format!("@font-face {{ font-family: \"{}\"; src: url(\"fonts/{}\"); }}\n", fonts::css_family(&f.group), f.file_name));
    }
    // The main text: the reader's serif font, the embedded one for what it lacks; first when the main script is not Latin.
    let body_font = match fonts.iter().find(|f| f.group == fonts::TEXT) {
        Some(_) if main != "Latn" => format!("\"{}\", serif", fonts::css_family(fonts::TEXT)),
        Some(_) => format!("serif, \"{}\"", fonts::css_family(fonts::TEXT)),
        None => "serif".into(),
    };
    let mut scripts = String::new();
    for f in fonts.iter().filter(|f| f.group != fonts::TEXT) {
        let hint = match f.group.as_str() {
            "Grek" => "\"Gentium Plus\", \"SBL BibLit\", ",
            "Hebr" => "\"Ezra SIL\", \"SBL Hebrew\", ",
            _ => "",
        };
        // Hebrew, Arabic and Syriac look smaller than Latin at the same size.
        let size = if matches!(f.group.as_str(), "Hebr" | "Arab" | "Syrc") { " font-size: 1.1em;" } else { "" };
        scripts.push_str(&format!(".sc-{} {{ font-family: \"{}\", {hint}serif;{size} }}\n", f.group, fonts::css_family(&f.group)));
    }
    s.push_str(&format!(
        r#"body {{ font-family: {body_font}; line-height: 1.45; margin: 0 0.4em; }}
p {{ margin: 0; text-indent: 1.2em; text-align: justify; hyphens: auto; -webkit-hyphens: auto; }}
p.noindent, h1 + p, h2 + p, h3 + p, h4 + p, h5 + p, h6 + p {{ text-indent: 0; }}
h1, h2, h3, h4, h5, h6 {{ font-family: sans-serif; text-align: center; line-height: 1.25; hyphens: none; page-break-after: avoid; }}
h1 {{ font-size: 1.5em; margin: 1.5em 0 1em; }}
h2 {{ font-size: 1.35em; margin: 1.4em 0 0.9em; }}
h3 {{ font-size: 1.15em; font-style: italic; margin: 1.3em 0 0.7em; }}
h4 {{ font-size: 1.05em; font-style: italic; margin: 1.2em 0 0.6em; }}
h5, h6 {{ font-size: 1em; margin: 1em 0 0.4em; text-align: left; }}
.center {{ text-align: center; text-indent: 0; margin: 0.3em 0; }}
.small {{ font-size: 0.85em; }}
div.box {{ border: 1px solid #888; padding: 0.4em 0.8em; margin: 1em 0; font-family: sans-serif; font-size: 0.95em; }}
div.box p {{ text-indent: 0; margin: 0.25em 0; text-align: left; }}
div.list {{ margin: 0.5em 0; margin-left: 1.5em; }}
div.list p.item {{ text-indent: -1.5em; padding-left: 1.5em; text-align: left; margin: 0.15em 0; }}
blockquote.indented {{ margin: 0.8em 0; margin-left: 1.5em; }}
blockquote.indented p {{ text-indent: 0; text-align: left; }}
ul.bibliography {{ list-style: none; padding: 0; margin: 0; }}
ul.bibliography li {{ padding-left: 1.5em; text-indent: -1.5em; margin: 0.4em 0; }}
dl.glossary dt {{ font-weight: bold; margin-top: 0.5em; }}
dl.glossary dd {{ margin: 0; margin-left: 2em; }}
p.index {{ text-indent: -1.5em; padding-left: 1.5em; text-align: left; }}
section.footnotes {{ margin-top: 2em; border-top: 1px solid #999; font-size: 0.85em; }}
section.footnotes h2 {{ font-size: 1em; text-align: left; }}
aside.footnote p {{ text-indent: 0; margin: 0.3em 0; text-align: left; }}
a[epub|type~="noteref"] {{ text-decoration: none; }}
ol.printed-toc {{ list-style: none; padding: 0; }}
ol.printed-toc li {{ margin: 0.2em 0; display: flex; justify-content: space-between; gap: 1em; }}
ol.printed-toc li.d1 {{ padding-left: 1.5em; }}
ol.printed-toc li.d2 {{ padding-left: 3em; }}
ol.printed-toc li.d3 {{ padding-left: 4.5em; }}
ol.printed-toc .pg {{ white-space: nowrap; }}
nav ol {{ list-style: none; }}
img.cover {{ max-width: 100%; max-height: 100%; display: block; margin: 0 auto; }}
{scripts}@namespace epub "http://www.idpf.org/2007/ops";
"#
    ));
    // @namespace has to come before the rules that use it.
    let ns = "@namespace epub \"http://www.idpf.org/2007/ops\";\n";
    let s = s.replace(ns, "");
    // A right-to-left book starts its lines, indents and lists on the right.
    let s = if rtl { s.replace("left", "\u{0}").replace("right", "left").replace('\u{0}', "right") } else { s };
    format!("{ns}{s}")
}

/// Splits the blocks into files: a new file at each heading of the first two levels, unless the file so far holds only headings.
fn split(book: &Book, words: &Words) -> Vec<File> {
    let mut files: Vec<File> = Vec::new();
    let mut cur: Option<File> = None;
    for (i, p) in book.blocks.iter().enumerate() {
        if let Block::Heading(h) = &p.block
            && h.level <= 2
            && h.nav
        {
            let substantial = cur.as_ref().is_some_and(|f| f.blocks.iter().any(|&b| !matches!(book.blocks[b].block, Block::Heading(_) | Block::PageMark(_))));
            if substantial || cur.is_none() {
                let mut carried = Vec::new();
                if let Some(mut f) = cur.take() {
                    while let Some(&last) = f.blocks.last() {
                        if matches!(book.blocks[last].block, Block::PageMark(_)) {
                            carried.insert(0, f.blocks.pop().expect("last"));
                        } else {
                            break;
                        }
                    }
                    files.push(f);
                }
                cur = Some(File { name: String::new(), title: plain(&h.content), section: h.section, blocks: carried });
            }
        }
        cur.get_or_insert_with(|| File { name: String::new(), title: words.start.to_string(), section: Section::Normal, blocks: Vec::new() }).blocks.push(i);
    }
    files.extend(cur);
    for (i, f) in files.iter_mut().enumerate() {
        f.name = format!("s{:03}.xhtml", i + 1);
    }
    files
}

fn render_file(ctx: &Ctx, fi: usize, words: &Words) -> (String, usize, usize, usize) {
    let book = ctx.book;
    let file = &ctx.files[fi];
    let (etype, role) = section_type(file.section);
    let mut out = format!("<section epub:type=\"{etype}\"{}>\n", if role.is_empty() { String::new() } else { format!(" role=\"{role}\"") });
    let mut group: Option<&str> = None;
    let mut special: Option<(Section, u8)> = None;
    let mut marks = String::new();
    let (mut bib, mut gloss, mut index) = (0, 0, 0);
    let close = |out: &mut String, group: &mut Option<&str>| {
        if let Some(g) = group.take() {
            out.push_str(match g {
                "box" => "</div>\n",
                "indent" => "</blockquote>\n",
                "bib" => "</ul>\n",
                "list" => "</div>\n",
                _ => "</dl>\n",
            });
        }
    };
    let open = |out: &mut String, group: &mut Option<&str>, want: &'static str| {
        if *group != Some(want) {
            if let Some(g) = group.take() {
                out.push_str(match g {
                    "box" => "</div>\n",
                    "indent" => "</blockquote>\n",
                    "bib" => "</ul>\n",
                    "list" => "</div>\n",
                    _ => "</dl>\n",
                });
            }
            out.push_str(match want {
                "box" => "<div class=\"box\">\n",
                "indent" => "<blockquote class=\"indented\">\n",
                "bib" => "<ul class=\"bibliography\">\n",
                "list" => "<div class=\"list\">\n",
                _ => "<dl class=\"glossary\">\n",
            });
            *group = Some(want);
        }
    };
    for &bi in &file.blocks {
        let Placed { block, .. } = &book.blocks[bi];
        let sec = special.map_or(Section::Normal, |s| s.0);
        match block {
            Block::PageMark(n) => marks.push_str(&page_mark(*n)),
            Block::Heading(h) => {
                close(&mut out, &mut group);
                let Heading { id, level, content, section, .. } = h;
                if matches!(section, Section::Bibliography | Section::Glossary | Section::Index) {
                    special = Some((*section, *level));
                } else if special.is_some_and(|(_, l)| *level <= l) {
                    special = None;
                }
                let n = (*level).clamp(1, 6);
                out.push_str(&format!("<h{n} id=\"h{id}\">{}{}</h{n}>\n", std::mem::take(&mut marks), ctx.inline(fi, content, Section::Normal)));
            }
            Block::Para { kind, indent, content } => {
                let inner = format!("{}{}", std::mem::take(&mut marks), ctx.inline(fi, content, sec));
                match (sec, kind) {
                    (Section::Bibliography, Kind::Body | Kind::Indent) => {
                        open(&mut out, &mut group, "bib");
                        out.push_str(&format!("<li>{inner}</li>\n"));
                        bib += 1;
                    }
                    (Section::Index, _) => {
                        close(&mut out, &mut group);
                        out.push_str(&format!("<p class=\"index\">{inner}</p>\n"));
                        index += 1;
                    }
                    (_, Kind::Box) => {
                        open(&mut out, &mut group, "box");
                        out.push_str(&format!("<p>{inner}</p>\n"));
                    }
                    (_, Kind::Item) => {
                        open(&mut out, &mut group, "list");
                        out.push_str(&format!("<p class=\"item\">{inner}</p>\n"));
                    }
                    (_, Kind::Indent) => {
                        open(&mut out, &mut group, "indent");
                        out.push_str(&format!("<p>{inner}</p>\n"));
                    }
                    (_, Kind::Small) => {
                        close(&mut out, &mut group);
                        out.push_str(&format!("<p class=\"small noindent\">{inner}</p>\n"));
                    }
                    (_, Kind::Center) => {
                        close(&mut out, &mut group);
                        out.push_str(&format!("<p class=\"center\">{inner}</p>\n"));
                    }
                    (_, Kind::Body) => {
                        close(&mut out, &mut group);
                        out.push_str(&format!("<p{}>{inner}</p>\n", if *indent { "" } else { " class=\"noindent\"" }));
                    }
                }
            }
            Block::Entry { term, desc } => {
                open(&mut out, &mut group, "dl");
                out.push_str(&format!("<dt>{}{}</dt>\n<dd>{}</dd>\n", std::mem::take(&mut marks), ctx.inline(fi, term, sec), ctx.inline(fi, desc, sec)));
                gloss += 1;
            }
        }
    }
    close(&mut out, &mut group);
    if !marks.is_empty() {
        out.push_str(&format!("<p class=\"noindent\">{marks}</p>\n"));
    }
    // The footnotes of this file.
    let notes: Vec<usize> = (0..book.notes.len()).filter(|k| ctx.links.note.get(k) == Some(&fi)).collect();
    if !notes.is_empty() {
        out.push_str(&format!("<section class=\"footnotes\" epub:type=\"footnotes\">\n<h2>{}</h2>\n", words.notes));
        for k in notes {
            let n = &book.notes[k];
            let label = esc(&n.label);
            let back = match n.reference.and_then(|r| ctx.links.reference.get(&r).map(|f| (r, *f))) {
                Some((_, to)) => format!("<a href=\"{}\" role=\"doc-backlink\">{label}</a>", ctx.href(fi, to, &format!("r{k}"))),
                None => label,
            };
            out.push_str(&format!(
                "<aside epub:type=\"footnote\" role=\"doc-footnote\" id=\"n{k}\" class=\"footnote\"><p>{back} {}</p></aside>\n",
                ctx.inline(fi, &n.content, Section::Normal)
            ));
        }
        out.push_str("</section>\n");
    }
    out.push_str("</section>");
    (out, bib, gloss, index)
}

struct NavItem {
    label: String,
    href: String,
    level: u8,
}

fn nav_tree(items: &[NavItem], ncx: bool, play: &mut usize) -> String {
    // Nested lists from the levels: an item is a child of the last item with a smaller level.
    fn build(items: &[NavItem], i: &mut usize, level: u8, ncx: bool, play: &mut usize) -> String {
        let mut out = String::new();
        while *i < items.len() && items[*i].level >= level {
            let it = &items[*i];
            *i += 1;
            let children = if *i < items.len() && items[*i].level > it.level { build(items, i, it.level + 1, ncx, play) } else { String::new() };
            if ncx {
                *play += 1;
                out.push_str(&format!(
                    "<navPoint id=\"np{0}\" playOrder=\"{0}\"><navLabel><text>{1}</text></navLabel><content src=\"text/{2}\"/>{3}</navPoint>\n",
                    *play,
                    esc(&it.label),
                    esc(&it.href),
                    children
                ));
            } else {
                let kids = if children.is_empty() { String::new() } else { format!("\n<ol>\n{children}</ol>\n") };
                out.push_str(&format!("<li><a href=\"{}\">{}</a>{kids}</li>\n", esc(&it.href), esc(&it.label)));
            }
        }
        out
    }
    let mut i = 0;
    let mut out = String::new();
    while i < items.len() {
        let level = items[i].level;
        out.push_str(&build(items, &mut i, level, ncx, play));
    }
    out
}

fn uuid(seed: &str) -> String {
    let h = |salt: u64| {
        let mut x: u64 = 0xcbf29ce484222325 ^ salt;
        for b in seed.bytes() {
            x ^= b as u64;
            x = x.wrapping_mul(0x100000001b3);
        }
        x
    };
    let (a, b) = (h(1), h(2));
    format!("{:08x}-{:04x}-4{:03x}-a{:03x}-{:012x}", a >> 32, (a >> 16) & 0xffff, a & 0xfff, (b >> 48) & 0xfff, b & 0xffff_ffff_ffff)
}

/// Writes the EPUB.
pub fn write(book: &Book, cover: Option<&[u8]>, fonts: &[Font], meta: &Meta) -> Result<(Vec<u8>, Report), Error> {
    let w = words(meta.language);
    let lang = esc(meta.language);
    let files = split(book, &w);
    // Where each heading, page, mark and note is.
    let mut links = Links { heading: HashMap::new(), page: HashMap::new(), note: HashMap::new(), reference: HashMap::new() };
    let mut page_file: HashMap<usize, usize> = HashMap::new();
    for (fi, f) in files.iter().enumerate() {
        for &bi in &f.blocks {
            let p = &book.blocks[bi];
            page_file.entry(p.page).or_insert(fi);
            let mut scan = |c: &[Inline]| {
                for i in c {
                    match i {
                        Inline::NoteRef(r) => {
                            links.reference.insert(*r, fi);
                        }
                        Inline::Page(n) => {
                            links.page.entry(*n).or_insert(fi);
                        }
                        Inline::Text { .. } => {}
                    }
                }
            };
            match &p.block {
                Block::Heading(h) => {
                    links.heading.insert(h.id, fi);
                    scan(&h.content);
                }
                Block::Para { content, .. } => scan(content),
                Block::Entry { term, desc } => {
                    scan(term);
                    scan(desc);
                }
                Block::PageMark(n) => {
                    links.page.entry(*n).or_insert(fi);
                }
            }
        }
    }
    for (k, n) in book.notes.iter().enumerate() {
        let f = n.reference.and_then(|r| links.reference.get(&r).copied()).or_else(|| (0..=n.page).rev().find_map(|p| page_file.get(&p).copied())).unwrap_or(0);
        links.note.insert(k, f);
    }
    let page_ref = Regex::new(r"\b(?:S\.|Seite|Seiten|p\.|pp\.)[\s\u{a0}]?(?P<n>\d{1,4})\b").expect("regex");
    let index_num = Regex::new(r"\b(?P<n>\d{1,4})(?:\s?ff?\.)?").expect("regex");
    let ctx = Ctx {
        book,
        files: &files,
        links: &links,
        page_ref: &page_ref,
        index_num: &index_num,
        main: meta.main_script,
        langs: meta.langs,
    };
    let mut report = Report { pages: book.pages, files: files.len(), notes: book.notes.len(), refs: book.refs.len(), ..Report::default() };
    report.refs_linked = book.refs.iter().filter(|r| r.note.is_some()).count();
    report.notes_without_ref = book.notes.iter().filter(|n| n.reference.is_none()).count();
    report.page_marks = links.page.len();
    report.toc_entries = book.toc.len();
    report.toc_to_headings = book.toc.iter().filter(|e| e.heading.is_some()).count();
    report.fonts = fonts.iter().map(|f| f.source.clone()).collect();

    let buf = std::io::Cursor::new(Vec::new());
    let mut z = zip::ZipWriter::new(buf);
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let put = |z: &mut zip::ZipWriter<std::io::Cursor<Vec<u8>>>, name: &str, data: &[u8], opt| -> Result<(), Error> {
        z.start_file(name, opt).map_err(|e| Error::Zip(e.to_string()))?;
        z.write_all(data).map_err(Error::Io)
    };
    put(&mut z, "mimetype", b"application/epub+zip", stored)?;
    put(
        &mut z,
        "META-INF/container.xml",
        br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
<rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>
"#,
        deflated,
    )?;
    let mut manifest = String::new();
    let mut spine = String::new();
    let mut nav_items: Vec<NavItem> = Vec::new();
    if let Some(img) = cover {
        put(&mut z, "OEBPS/images/cover.jpg", img, stored)?;
        manifest.push_str("<item id=\"cover-image\" href=\"images/cover.jpg\" media-type=\"image/jpeg\" properties=\"cover-image\"/>\n");
        let body = format!("<section epub:type=\"cover\"><img class=\"cover\" src=\"../images/cover.jpg\" alt=\"{}\"/></section>", esc(meta.title));
        put(&mut z, "OEBPS/text/cover.xhtml", xhtml(&lang, book.rtl, w.cover, &body).as_bytes(), deflated)?;
        manifest.push_str("<item id=\"cover\" href=\"text/cover.xhtml\" media-type=\"application/xhtml+xml\"/>\n");
        spine.push_str("<itemref idref=\"cover\"/>\n");
    }
    let mut written = std::collections::HashSet::new();
    for f in fonts.iter().filter(|f| written.insert(f.file_name.clone())) {
        put(&mut z, &format!("OEBPS/fonts/{}", f.file_name), &f.data, deflated)?;
        manifest.push_str(&format!("<item id=\"font-{0}\" href=\"fonts/{0}\" media-type=\"{1}\"/>\n", f.file_name, f.media_type));
    }
    put(&mut z, "OEBPS/style.css", css(fonts, meta.main_script, book.rtl).as_bytes(), deflated)?;
    manifest.push_str("<item id=\"css\" href=\"style.css\" media-type=\"text/css\"/>\n");
    // The front matter, a page of the print per section; the copyright page marked as such.
    if !book.front.is_empty() {
        let mut body = String::from("<section epub:type=\"frontmatter\">\n");
        let mut page = usize::MAX;
        for p in &book.front {
            if p.page != page {
                if page != usize::MAX {
                    body.push_str("</section>\n");
                }
                page = p.page;
                let text: String = book.front.iter().filter(|q| q.page == page).filter_map(|q| if let Block::Para { content, .. } = &q.block { Some(plain(content)) } else { None }).collect();
                let kind = if text.contains("ISBN") || text.contains('©') { " epub:type=\"copyright-page\"" } else { "" };
                body.push_str(&format!("<section{kind}>\n"));
            }
            if let Block::Para { kind, content, .. } = &p.block {
                let class = if *kind == Kind::Center { "center" } else { "noindent" };
                body.push_str(&format!("<p class=\"{class}\">{}</p>\n", ctx.inline(0, content, Section::Normal)));
            }
        }
        body.push_str("</section>\n</section>");
        put(&mut z, "OEBPS/text/front.xhtml", xhtml(&lang, book.rtl, w.front, &body).as_bytes(), deflated)?;
        manifest.push_str("<item id=\"front\" href=\"text/front.xhtml\" media-type=\"application/xhtml+xml\"/>\n");
        spine.push_str("<itemref idref=\"front\"/>\n");
        nav_items.push(NavItem { label: w.front.to_string(), href: "front.xhtml".into(), level: 1 });
    }
    // The table of contents as printed, its entries leading to their headings (or at least to their pages).
    let toc_title = book.toc_title.clone().unwrap_or_else(|| w.contents.to_string());
    {
        let mut body = format!("<section epub:type=\"toc\" role=\"doc-toc\">\n<h1>{}</h1>\n<ol class=\"printed-toc\">\n", esc(&toc_title));
        let entries: Vec<(String, Option<u32>, usize, Option<String>)> = if book.toc.is_empty() {
            book.blocks
                .iter()
                .filter_map(|p| match &p.block {
                    Block::Heading(h) if h.nav => Some((plain(&h.content), h.printed, h.level.saturating_sub(1) as usize, links.heading.get(&h.id).map(|f| format!("{}#h{}", files[*f].name, h.id)))),
                    _ => None,
                })
                .collect()
        } else {
            book.toc
                .iter()
                .map(|e| {
                    let target = e
                        .heading
                        .and_then(|h| links.heading.get(&h).map(|f| format!("{}#h{h}", files[*f].name)))
                        .or_else(|| e.page.and_then(|p| links.page.get(&p).map(|f| format!("{}#p{p}", files[*f].name))));
                    (e.title.clone(), e.page, e.depth, target)
                })
                .collect()
        };
        for (title, page, depth, target) in entries {
            let t = ctx.scripts(&title);
            let t = match target {
                Some(href) => format!("<a href=\"{}\">{t}</a>", esc(&href)),
                None => t,
            };
            let pg = page.map(|p| format!("<span class=\"pg\">{p}</span>")).unwrap_or_default();
            body.push_str(&format!("<li class=\"d{}\">{t}{pg}</li>\n", depth.min(3)));
        }
        body.push_str("</ol>\n</section>");
        put(&mut z, "OEBPS/text/contents.xhtml", xhtml(&lang, book.rtl, &toc_title, &body).as_bytes(), deflated)?;
        manifest.push_str("<item id=\"contents\" href=\"text/contents.xhtml\" media-type=\"application/xhtml+xml\"/>\n");
        spine.push_str("<itemref idref=\"contents\"/>\n");
        nav_items.push(NavItem { label: toc_title.clone(), href: "contents.xhtml".into(), level: 1 });
    }
    let mut landmarks: Vec<(&str, String, String)> = Vec::new();
    if cover.is_some() {
        landmarks.push(("cover", "cover.xhtml".into(), w.cover.into()));
    }
    landmarks.push(("toc", "contents.xhtml".into(), toc_title.clone()));
    for (fi, f) in files.iter().enumerate() {
        let (body, bib, gloss, index) = render_file(&ctx, fi, &w);
        report.bibliography += bib;
        report.glossary += gloss;
        report.index_entries += index;
        put(&mut z, &format!("OEBPS/text/{}", f.name), xhtml(&lang, book.rtl, &f.title, &body).as_bytes(), deflated)?;
        let id = f.name.trim_end_matches(".xhtml");
        manifest.push_str(&format!("<item id=\"{id}\" href=\"text/{}\" media-type=\"application/xhtml+xml\"/>\n", f.name));
        spine.push_str(&format!("<itemref idref=\"{id}\"/>\n"));
        let lm = match f.section {
            Section::Bibliography => Some("bibliography"),
            Section::Glossary => Some("glossary"),
            Section::Index => Some("index"),
            _ => None,
        };
        if let Some(t) = lm {
            landmarks.push((t, f.name.clone(), f.title.clone()));
        }
    }
    if let Some(f) = files.iter().find(|f| f.section != Section::Front).or(files.first()) {
        landmarks.insert(landmarks.len().min(if cover.is_some() { 2 } else { 1 }), ("bodymatter", f.name.clone(), w.start.into()));
    }
    // The navigation by the headings.
    let synthetic = nav_items.len();
    for p in &book.blocks {
        if let Block::Heading(h) = &p.block
            && h.nav
            && let Some(f) = links.heading.get(&h.id)
        {
            nav_items.push(NavItem { label: plain(&h.content), href: format!("{}#h{}", files[*f].name, h.id), level: h.level.max(1) });
            report.headings += 1;
        }
    }
    report.headings += book.blocks.iter().filter(|p| matches!(&p.block, Block::Heading(h) if !h.nav)).count();
    // Only headings hold others: after the front matter and the contents the next heading starts on top again, and no level
    // is skipped.
    let mut stack: Vec<u8> = Vec::new();
    for (k, it) in nav_items.iter_mut().enumerate() {
        if k < synthetic {
            it.level = 1;
            continue;
        }
        while stack.last().is_some_and(|l| *l >= it.level) {
            stack.pop();
        }
        let own = it.level;
        it.level = stack.len() as u8 + 1;
        stack.push(own);
    }
    report.nav_entries = nav_items.len();
    let mut pages: Vec<(u32, usize)> = links.page.iter().map(|(p, f)| (*p, *f)).collect();
    pages.sort();
    let mut play = 0;
    let toc_tree = nav_tree(&nav_items, false, &mut play);
    let page_list: String = pages.iter().map(|(p, f)| format!("<li><a href=\"{}#p{p}\">{p}</a></li>\n", files[*f].name)).collect();
    let lm: String = landmarks.iter().map(|(t, href, label)| format!("<li><a epub:type=\"{t}\" href=\"{}\">{}</a></li>\n", esc(href), esc(label))).collect();
    let nav = format!(
        "<nav epub:type=\"toc\" id=\"toc\" role=\"doc-toc\">\n<h1>{}</h1>\n<ol>\n{toc_tree}</ol>\n</nav>\n<nav epub:type=\"page-list\" id=\"page-list\" hidden=\"hidden\">\n<h2>{}</h2>\n<ol>\n{page_list}</ol>\n</nav>\n<nav epub:type=\"landmarks\" id=\"landmarks\" hidden=\"hidden\">\n<h2>{}</h2>\n<ol>\n{lm}</ol>\n</nav>",
        esc(&toc_title),
        w.pages,
        w.landmarks
    );
    put(&mut z, "OEBPS/text/nav.xhtml", xhtml(&lang, book.rtl, &toc_title, &nav).as_bytes(), deflated)?;
    manifest.push_str("<item id=\"nav\" href=\"text/nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n");
    // NCX for readers of EPUB 2.
    let identifier = match &book.isbn {
        Some(i) => format!("urn:isbn:{i}"),
        None => format!("urn:uuid:{}", uuid(&format!("{}|{}|{}", meta.title, meta.author, book.pages))),
    };
    let mut play = 0;
    let nav_map = nav_tree(&nav_items, true, &mut play);
    let page_targets: String = pages
        .iter()
        .map(|(p, f)| {
            play += 1;
            format!("<pageTarget id=\"pt{p}\" type=\"normal\" value=\"{p}\" playOrder=\"{play}\"><navLabel><text>{p}</text></navLabel><content src=\"text/{}#p{p}\"/></pageTarget>\n", files[*f].name)
        })
        .collect();
    let ncx = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\" xml:lang=\"{lang}\">\n<head><meta name=\"dtb:uid\" content=\"{}\"/><meta name=\"dtb:depth\" content=\"{}\"/><meta name=\"dtb:totalPageCount\" content=\"{}\"/><meta name=\"dtb:maxPageNumber\" content=\"{}\"/></head>\n<docTitle><text>{}</text></docTitle>\n<docAuthor><text>{}</text></docAuthor>\n<navMap>\n{nav_map}</navMap>\n<pageList><navLabel><text>{}</text></navLabel>\n{page_targets}</pageList>\n</ncx>\n",
        esc(&identifier),
        nav_items.iter().map(|i| i.level).max().unwrap_or(1),
        pages.len(),
        pages.last().map_or(0, |p| p.0),
        esc(meta.title),
        esc(meta.author),
        w.pages
    );
    put(&mut z, "OEBPS/toc.ncx", ncx.as_bytes(), deflated)?;
    manifest.push_str("<item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>\n");
    // The package.
    let mut extra = String::new();
    if let Some(p) = &book.publisher {
        extra.push_str(&format!("<dc:publisher>{}</dc:publisher>\n", esc(p)));
    }
    if let Some(y) = &book.year {
        extra.push_str(&format!("<dc:date>{}</dc:date>\n", esc(y)));
    }
    if let Some(i) = &book.isbn {
        // The page breaks follow this print.
        extra.push_str(&format!("<dc:source id=\"print\">urn:isbn:{}</dc:source>\n<meta refines=\"#print\" property=\"source-of\">pagination</meta>\n", esc(i)));
    } else if !meta.source.is_empty() {
        extra.push_str(&format!("<dc:source>{}</dc:source>\n", esc(meta.source)));
    }
    if cover.is_some() {
        extra.push_str("<meta name=\"cover\" content=\"cover-image\"/>\n");
    }
    let progression = if book.rtl { " page-progression-direction=\"rtl\"" } else { "" };
    let opf = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"bookid\" xml:lang=\"{lang}\">\n<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n<dc:identifier id=\"bookid\">{}</dc:identifier>\n<dc:title>{}</dc:title>\n<dc:creator id=\"author\">{}</dc:creator>\n<meta refines=\"#author\" property=\"role\" scheme=\"marc:relators\">aut</meta>\n<dc:language>{lang}</dc:language>\n{extra}<meta property=\"dcterms:modified\">{}</meta>\n<meta property=\"schema:accessMode\">textual</meta>\n<meta property=\"schema:accessibilityFeature\">tableOfContents</meta>\n<meta property=\"schema:accessibilityFeature\">printPageNumbers</meta>\n<meta property=\"schema:accessibilityFeature\">structuralNavigation</meta>\n</metadata>\n<manifest>\n{manifest}</manifest>\n<spine toc=\"ncx\"{progression}>\n{spine}</spine>\n</package>\n",
        esc(&identifier),
        esc(meta.title),
        esc(meta.author),
        meta.modified
    );
    put(&mut z, "OEBPS/content.opf", opf.as_bytes(), deflated)?;
    let buf = z.finish().map_err(|e| Error::Zip(e.to_string()))?;
    Ok((buf.into_inner(), report))
}
