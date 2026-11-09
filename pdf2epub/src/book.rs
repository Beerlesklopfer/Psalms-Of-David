// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! From the lines of the pages to the book: the running heads go (their numbers are the printed pages), the footnotes are taken
//! from the foot of each page and joined to their marks, the headings are found by their fonts and ranked, the printed table of
//! contents is read with its pages, the paragraphs are rejoined over lines and pages with the hyphenation taken back, the
//! bibliography and the abbreviations are recognised.

use std::collections::HashMap;

use regex::Regex;

use crate::layout::{self, Line, Span};
use crate::pdf::Pdf;

#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    Text { text: String, bold: bool, italic: bool, greek: bool },
    /// The mark of a footnote (index into [`Book::refs`]).
    NoteRef(usize),
    /// The beginning of a printed page.
    Page(u32),
}

/// What kind of paragraph, as the print shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Body,
    /// Text in another font set apart (a box, an excursus).
    Box,
    /// Lines indented as a block (the Bible text before its exposition, a long quotation).
    Indent,
    /// An item of a list with a hanging indent (`1.  …`, the following lines further in).
    Item,
    /// Text in the size of the footnotes, not at the foot of the page.
    Small,
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Normal,
    Front,
    Toc,
    Part,
    Bibliography,
    Glossary,
    Index,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Style {
    size: i32,
    bold: bool,
    key: String,
}

impl Style {
    fn of(l: &Line) -> Style {
        Style { size: (l.size * 2.0).round() as i32, bold: l.bold, key: l.key.clone() }
    }
}

#[derive(Debug, Clone)]
pub struct Heading {
    pub id: usize,
    /// 1 = part (or front and back matter), 2 = chapter, …
    pub level: u8,
    /// In the navigation (bigger than the text); run-in headings in the size of the text are not.
    pub nav: bool,
    pub section: Section,
    pub content: Vec<Inline>,
    pub printed: Option<u32>,
    style: Style,
    last_top: f32,
}

#[derive(Debug, Clone)]
pub enum Block {
    Heading(Heading),
    Para { kind: Kind, indent: bool, content: Vec<Inline> },
    /// An entry of a list of abbreviations: the abbreviation and what it stands for.
    Entry { term: Vec<Inline>, desc: Vec<Inline> },
    PageMark(u32),
}

/// A block and the page (index in the PDF) it begins on.
#[derive(Debug, Clone)]
pub struct Placed {
    pub page: usize,
    pub block: Block,
}

#[derive(Debug, Clone)]
pub struct Note {
    pub label: String,
    pub content: Vec<Inline>,
    pub page: usize,
    pub reference: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct NoteRef {
    pub label: String,
    pub note: Option<usize>,
}

/// An entry of the printed table of contents.
#[derive(Debug, Clone)]
pub struct TocEntry {
    pub title: String,
    pub page: Option<u32>,
    pub depth: usize,
    /// The heading it leads to (its id), when it was found.
    pub heading: Option<usize>,
}

#[derive(Debug, Clone, Default)]
pub struct Book {
    pub front: Vec<Placed>,
    pub toc_title: Option<String>,
    pub toc: Vec<TocEntry>,
    pub blocks: Vec<Placed>,
    pub notes: Vec<Note>,
    pub refs: Vec<NoteRef>,
    pub isbn: Option<String>,
    pub publisher: Option<String>,
    pub year: Option<String>,
    pub pages: usize,
    /// The book is written from right to left.
    pub rtl: bool,
}

/// The plain text of some inline content.
pub fn plain(content: &[Inline]) -> String {
    content
        .iter()
        .filter_map(|i| match i {
            Inline::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

pub(crate) struct Res {
    verse: Regex,
    marker: Regex,
    note: Regex,
    toc: Regex,
    head_start: Regex,
    head_end: Regex,
    split: Regex,
    part: Regex,
    first: Regex,
    isbn: Regex,
    copy: Regex,
}

impl Res {
    pub(crate) fn new() -> Res {
        Res {
            verse: Regex::new(r"^\s*\d{1,3}[a-f]?\s").unwrap(),
            marker: Regex::new(r"^\s*(\d{1,3}\.|\d{1,3}\)|[a-z]\.|[a-z]\)|[•–—-])[\s\u{a0}]").unwrap(),
            note: Regex::new(r"^(\d{1,4})[\s\u{a0}]+").unwrap(),
            toc: Regex::new(r"^(.*\S)[\s.·…_]*?\s(\d{1,4})$").unwrap(),
            head_start: Regex::new(r"^\s*(\d{1,4})\b").unwrap(),
            head_end: Regex::new(r"\b(\d{1,4})\s*$").unwrap(),
            split: Regex::new(r"(\p{L}+) {1,3}(\p{Ll}\p{L}{2,})").unwrap(),
            part: Regex::new(r"(?i)^(teil|part|buch|book)[\s\u{a0}]+([0-9]+|[ivxlc]+)\b").unwrap(),
            first: Regex::new(r"(?i)^(kapitel|chapter)[\s\u{a0}]*1$|^1\.[\s\u{a0}]*kapitel$").unwrap(),
            isbn: Regex::new(r"ISBN[^0-9]{0,12}([0-9][0-9\- ]{8,16}[0-9Xx])").unwrap(),
            copy: Regex::new(r"©\s*(\d{4})(?:\s+by)?\s+([^\n·,]{2,40})").unwrap(),
        }
    }
}

/// What a heading stands for, by its words.
fn classify(text: &str, re: &Res) -> Section {
    let t = text.trim().trim_end_matches(':').to_lowercase();
    let t = t.as_str();
    let is = |words: &[&str]| words.contains(&t);
    if is(&["inhalt", "inhaltsverzeichnis", "contents", "table of contents", "inhaltsübersicht"]) {
        Section::Toc
    } else if is(&[
        "bibliografie",
        "bibliographie",
        "bibliography",
        "literatur",
        "literaturverzeichnis",
        "literaturhinweise",
        "quellen",
        "quellenverzeichnis",
        "quellen und literatur",
        "references",
        "works cited",
        "verwendete literatur",
    ]) {
        Section::Bibliography
    } else if is(&["abkürzungen", "abkürzungsverzeichnis", "abbreviations", "siglen", "siglenverzeichnis", "glossar", "glossary", "begriffserklärungen"]) {
        Section::Glossary
    } else if t.contains("register") || t == "index" || t.ends_with("verzeichnis") && (t.contains("stichwort") || t.contains("sach") || t.contains("personen") || t.contains("stellen") || t.contains("namen")) {
        Section::Index
    } else if is(&["vorwort", "einleitung", "geleitwort", "prolog", "preface", "foreword", "introduction", "einführung", "zur einführung", "danksagung", "zum geleit"]) {
        Section::Front
    } else if is(&["schlusswort", "nachwort", "epilog", "anhang", "appendix", "schluss"]) || re.part.is_match(t) {
        Section::Part
    } else {
        Section::Normal
    }
}

fn q(v: f32) -> i32 {
    (v * 2.0).round() as i32
}

fn mode(counts: &HashMap<i32, usize>) -> Option<(f32, usize)> {
    counts.iter().max_by_key(|(k, c)| (**c, -**k)).map(|(k, c)| (*k as f32 / 2.0, *c))
}

/// The measures of the book: text size and family, footnote size, line pitch, margins.
#[derive(Debug, Clone)]
pub(crate) struct Stats {
    pub body: f32,
    pub body_key: String,
    pub note: f32,
    pub pitch: f32,
    pub note_pitch: f32,
    pub left: [f32; 2],
    pub right: [f32; 2],
}

fn stats(pages: &[Vec<Line>]) -> Stats {
    let mut sizes: HashMap<i32, usize> = HashMap::new();
    for l in pages.iter().flatten() {
        for s in l.spans.iter().filter(|s| !s.sup) {
            *sizes.entry(q(s.size)).or_default() += s.text.chars().filter(|c| !c.is_whitespace()).count();
        }
    }
    let body = mode(&sizes).map_or(10.0, |m| m.0);
    let mut keys: HashMap<String, usize> = HashMap::new();
    for s in pages.iter().flatten().flat_map(|l| &l.spans).filter(|s| q(s.size) == q(body) && !s.greek) {
        *keys.entry(s.key.clone()).or_default() += s.text.len();
    }
    let body_key = keys.into_iter().max_by_key(|(_, c)| *c).map(|(k, _)| k).unwrap_or_default();
    let small: HashMap<i32, usize> = sizes.iter().filter(|(k, _)| (**k as f32 / 2.0) >= 0.5 * body && (**k as f32 / 2.0) <= 0.85 * body).map(|(k, v)| (*k, *v)).collect();
    let note = mode(&small).map_or(body * 0.7, |m| m.0);
    let pitch_of = |size: f32| {
        let mut d: HashMap<i32, usize> = HashMap::new();
        for p in pages {
            for w in p.windows(2) {
                if (w[0].size - size).abs() < 0.6 && (w[1].size - size).abs() < 0.6 {
                    let dy = w[1].top - w[0].top;
                    if dy > 0.8 * size && dy < 2.5 * size {
                        *d.entry(q(dy)).or_default() += 1;
                    }
                }
            }
        }
        mode(&d).map_or(size * 1.3, |m| m.0)
    };
    let mut left = [0.0; 2];
    let mut right = [0.0; 2];
    for parity in 0..2 {
        let mut ls: HashMap<i32, usize> = HashMap::new();
        let mut rs: HashMap<i32, usize> = HashMap::new();
        for (_, p) in pages.iter().enumerate().filter(|(i, _)| i % 2 == parity) {
            for l in p.iter().filter(|l| (l.size - body).abs() < 0.6) {
                *ls.entry(q(l.left)).or_default() += 1;
                *rs.entry(l.right.round() as i32 * 2).or_default() += 1;
            }
        }
        // The margin is the smallest left edge of many lines (indents, lists and quotations only lie further in).
        let total: usize = ls.values().sum();
        left[parity] = ls.iter().filter(|(_, c)| **c * 100 >= total * 15).map(|(k, _)| *k).min().map_or(0.0, |k| k as f32 / 2.0);
        right[parity] = mode(&rs).map_or(f32::MAX, |m| m.0);
    }
    Stats { body, body_key, note, pitch: pitch_of(body), note_pitch: pitch_of(note), left, right }
}

fn headingish(l: &Line, st: &Stats) -> bool {
    let n = l.text().trim().chars().count();
    if n == 0 || n > 160 || l.spans.iter().all(|s| s.sup) {
        return false;
    }
    l.size >= st.body + 1.5 || (l.bold && l.key != st.body_key && l.size > st.note + 0.5 && !crate::greek::is_bibleworks(&l.key))
}

fn box_line(l: &Line, st: &Stats) -> bool {
    l.key != st.body_key && !crate::greek::is_bibleworks(&l.key) && (l.size - st.body).abs() <= 3.0 && (l.size - st.note).abs() >= 0.6
}

type Dict = HashMap<String, u32>;

fn words(t: &str) -> impl Iterator<Item = &str> {
    t.split(|c: char| !c.is_alphabetic()).filter(|w| !w.is_empty())
}

/// How often each word occurs in the book (not counting the pieces at the ends of lines, which may be halves of a word).
fn dictionary(pages: &[Vec<Line>]) -> Dict {
    let mut d = Dict::new();
    for l in pages.iter().flatten() {
        let t: String = l.spans.iter().filter(|s| !s.greek && !s.sup).map(|s| s.text.as_str()).collect();
        let t = t.trim();
        let ws: Vec<&str> = words(t).collect();
        let cut_end = t.ends_with('-') || t.ends_with('\u{ad}');
        for (i, w) in ws.iter().enumerate() {
            if i == 0 || (cut_end && i + 1 == ws.len()) {
                continue;
            }
            *d.entry(w.to_lowercase()).or_default() += 1;
        }
    }
    d
}

fn push_text(content: &mut Vec<Inline>, text: &str, bold: bool, italic: bool, greek: bool) {
    if text.is_empty() {
        return;
    }
    if let Some(Inline::Text { text: t, bold: b, italic: i, greek: g }) = content.last_mut()
        && *b == bold
        && *i == italic
        && *g == greek
    {
        t.push_str(text);
        return;
    }
    content.push(Inline::Text { text: text.to_string(), bold, italic, greek });
}

/// Joins the next line to what is there: a soft hyphen at the end goes, a hard one goes only when the word without it is known.
fn join(content: &mut Vec<Inline>, next_word: &str, next_upper: bool, dict: &Dict) {
    match content.last_mut() {
        Some(Inline::Text { text, .. }) => {
            let n = text.trim_end().len();
            text.truncate(n);
            if text.ends_with('\u{ad}') {
                text.pop();
                while text.ends_with(' ') {
                    text.pop();
                }
                if next_upper {
                    text.push('-');
                }
            } else if text.ends_with('-') {
                let stem = &text[..text.len() - 1];
                let frag: String = stem.chars().rev().take_while(|c| c.is_alphabetic()).collect::<Vec<_>>().into_iter().rev().collect();
                let conjunction = ["und", "oder", "bzw", "sowie", "bis", "and", "or"].contains(&next_word);
                if conjunction {
                    // "Ost- und Westrom": the hyphen stands for the part of the word that follows.
                    text.push(' ');
                } else if !frag.is_empty() && next_word.chars().next().is_some_and(char::is_lowercase) {
                    // A hard hyphen at the end of a line (quotations keep the hyphenation of their source): the word is whole
                    // again when the book knows it whole, or when its first half is no word of its own ("mannig-fache"); a
                    // hyphenated compound of two words ("Nicht-mein") keeps it.
                    let whole = dict.get(&format!("{frag}{next_word}").to_lowercase()).is_some_and(|c| *c > 0);
                    let half_is_word = dict.get(&frag.to_lowercase()).is_some_and(|c| *c > 0);
                    if whole || (!half_is_word && frag.chars().count() > 2) {
                        text.pop();
                    }
                }
            } else {
                text.push(' ');
            }
        }
        Some(_) => content.push(Inline::Text { text: " ".into(), bold: false, italic: false, greek: false }),
        None => {}
    }
}

/// Appends the spans of a line. The marks of footnotes become [`Inline::NoteRef`] (unless `marks_as_text`).
fn append(content: &mut Vec<Inline>, spans: &[Span], dict: &Dict, refs: &mut Vec<NoteRef>, new_refs: &mut Vec<usize>, mark: Option<u32>, marks_as_text: bool) {
    let first = spans.iter().find(|s| !s.sup).map_or("", |s| s.text.trim_start());
    let word: String = first.chars().take_while(|c| c.is_alphabetic()).collect();
    let upper = first.chars().next().is_some_and(char::is_uppercase);
    if !content.is_empty() {
        join(content, &word, upper, dict);
    }
    if let Some(p) = mark {
        content.push(Inline::Page(p));
    }
    for s in spans {
        if s.sup && !marks_as_text {
            refs.push(NoteRef { label: s.text.trim().to_string(), note: None });
            new_refs.push(refs.len() - 1);
            content.push(Inline::NoteRef(refs.len() - 1));
            continue;
        }
        let text = if content.is_empty() { s.text.trim_start() } else { s.text.as_str() };
        push_text(content, text, s.bold && !s.sup, s.italic, s.greek);
    }
}

/// Cleans up the text: soft hyphens inside a line are real ones, spaces doubled by the justification go, words torn by spacing
/// (`Na tionen`) are joined when the book knows the whole word better than its second half.
fn tidy(content: &mut Vec<Inline>, dict: &Dict, re: &Res) {
    let mut prev_space = true;
    for i in content.iter_mut() {
        match i {
            Inline::Text { text, greek, .. } => {
                let mut t = text.replace(" \u{ad}", "\u{ad}").replace('\u{ad}', "-");
                while t.contains("  ") {
                    t = t.replace("  ", " ");
                }
                if !*greek {
                    t = re
                        .split
                        .replace_all(&t, |c: &regex::Captures| {
                            let whole = format!("{}{}", &c[1], &c[2]).to_lowercase();
                            let w = dict.get(&whole).copied().unwrap_or(0);
                            let second = dict.get(&c[2].to_lowercase()).copied().unwrap_or(0);
                            if w >= 2 && w > second { format!("{}{}", &c[1], &c[2]) } else { c[0].to_string() }
                        })
                        .into_owned();
                }
                if prev_space {
                    t = t.trim_start().to_string();
                }
                prev_space = t.ends_with(' ') || (t.is_empty() && prev_space);
                *text = t;
            }
            Inline::NoteRef(_) => prev_space = false,
            Inline::Page(_) => {}
        }
    }
    if let Some(Inline::Text { text, .. }) = content.iter_mut().rev().find(|i| matches!(i, Inline::Text { .. })) {
        let n = text.trim_end().len();
        text.truncate(n);
    }
    content.retain(|i| !matches!(i, Inline::Text { text, .. } if text.is_empty()));
}

fn cut_spans(spans: &[Span], mut skip: usize) -> Vec<Span> {
    let mut out = Vec::new();
    for s in spans {
        if skip >= s.text.len() {
            skip -= s.text.len();
            continue;
        }
        let mut s = s.clone();
        if skip > 0 {
            let mut at = skip;
            while !s.text.is_char_boundary(at) {
                at += 1;
            }
            s.text = s.text[at..].to_string();
            skip = 0;
        }
        out.push(s);
    }
    out
}

/// The lines of lists with a hanging indent: a line beginning with a marker, indented or followed by lines further in.
fn list_runs(lines: &[Line], base: f32, st: &Stats, re: &Res) -> Vec<bool> {
    let n = lines.len();
    let mut out = vec![false; n];
    let marked = |l: &Line| !headingish(l, st) && re.marker.is_match(&l.text());
    let mut i = 0;
    while i < n {
        let l = &lines[i];
        if !marked(l) {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let mut cont: Option<f32> = None;
        while j < n && !marked(&lines[j]) && !headingish(&lines[j], st) {
            let left = lines[j].left;
            let fits = left >= l.left + 8.0 && left <= l.left + 50.0 && cont.is_none_or(|c| (c - left).abs() <= 3.0);
            if !fits {
                break;
            }
            cont = Some(left);
            j += 1;
        }
        let next_marked = j < n && marked(&lines[j]) && (lines[j].left - l.left).abs() <= 3.0;
        if j > i + 1 || l.left >= base + 10.0 || next_marked {
            out[i..j].iter_mut().for_each(|o| *o = true);
        }
        i = j;
    }
    out
}

fn indented_runs(lines: &[Line], base: f32, st: &Stats) -> Vec<bool> {
    let n = lines.len();
    let mut out = vec![false; n];
    let mut i = 0;
    while i < n {
        let l = &lines[i];
        if l.left >= base + 12.0 && !headingish(l, st) {
            let mut j = i + 1;
            while j < n && (lines[j].left - l.left).abs() <= 3.0 && !headingish(&lines[j], st) {
                j += 1;
            }
            if j - i >= 2 {
                out[i..j].iter_mut().for_each(|o| *o = true);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

fn glossary_column(lines: &[Line], base: f32) -> Option<f32> {
    let mut cands: HashMap<i32, usize> = HashMap::new();
    for l in lines {
        let sp: Vec<&Span> = l.spans.iter().filter(|s| !s.sup && !s.text.trim().is_empty()).collect();
        let Some(f) = sp.first() else { continue };
        if f.left >= base + 40.0 {
            *cands.entry(f.left.round() as i32).or_default() += 1;
            continue;
        }
        for w in sp.windows(2) {
            if w[1].left - w[0].right >= 8.0 && w[1].left >= base + 40.0 && w[0].left < base + 10.0 {
                *cands.entry(w[1].left.round() as i32).or_default() += 1;
                break;
            }
        }
    }
    let best = cands.keys().map(|&k| (k, cands.iter().filter(|(c, _)| (**c - k).abs() <= 3).map(|(_, n)| *n).sum::<usize>())).max_by_key(|(k, n)| (*n, -*k))?;
    (best.1 >= 2).then_some(best.0 as f32)
}

struct Cur {
    kind: Kind,
    indent: bool,
    content: Vec<Inline>,
    last_top: f32,
    last_full: bool,
    page: usize,
    start: usize,
}

struct Asm<'a> {
    st: &'a Stats,
    dict: &'a Dict,
    re: &'a Res,
    blocks: Vec<Placed>,
    cur: Option<Cur>,
    refs: Vec<NoteRef>,
    notes: Vec<Note>,
    open_refs: Vec<(usize, usize)>,
    section: Section,
    special_size: f32,
    page: usize,
    pending: Option<u32>,
    entry_open: bool,
    headings: usize,
}

impl Asm<'_> {
    fn close(&mut self) {
        if let Some(mut c) = self.cur.take() {
            tidy(&mut c.content, self.dict, self.re);
            self.blocks.push(Placed { page: c.start, block: Block::Para { kind: c.kind, indent: c.indent, content: c.content } });
        }
    }

    fn flush_mark(&mut self) {
        if let Some(p) = self.pending.take() {
            self.blocks.push(Placed { page: self.page, block: Block::PageMark(p) });
        }
    }

    fn heading(&mut self, l: &Line) {
        let style = Style::of(l);
        self.close();
        let mut new_refs = Vec::new();
        let page = self.page;
        if let Some(Placed { block: Block::Heading(h), page: hp }) = self.blocks.last_mut()
            && *hp == page
            && h.style == style
            && l.top - h.last_top < 1.8 * l.size
        {
            append(&mut h.content, &l.spans, self.dict, &mut self.refs, &mut new_refs, None, false);
            tidy(&mut h.content, self.dict, self.re);
            h.last_top = l.top;
            let s = classify(&plain(&h.content), self.re);
            if matches!(s, Section::Bibliography | Section::Glossary | Section::Index) {
                self.section = s;
                self.special_size = l.size;
            }
        } else {
            self.flush_mark();
            self.entry_open = false;
            let mut content = Vec::new();
            append(&mut content, &l.spans, self.dict, &mut self.refs, &mut new_refs, None, false);
            tidy(&mut content, self.dict, self.re);
            let s = classify(&plain(&content), self.re);
            if matches!(s, Section::Bibliography | Section::Glossary | Section::Index) {
                self.section = s;
                self.special_size = l.size;
            } else if l.size >= self.special_size - 0.5 {
                self.section = Section::Normal;
                self.special_size = 0.0;
            }
            self.blocks.push(Placed {
                page,
                block: Block::Heading(Heading { id: self.headings, level: 0, nav: false, section: s, content, printed: None, style, last_top: l.top }),
            });
            self.headings += 1;
        }
        self.open_refs.extend(new_refs.into_iter().map(|r| (r, page)));
    }

    fn body_line(&mut self, l: &Line, kind: Kind, base: f32, full: bool) {
        let pitch = if kind == Kind::Small { self.st.note_pitch } else { self.st.pitch };
        let indented = l.left > base + 8.0;
        let new = match &self.cur {
            None => true,
            Some(c) => {
                c.kind != kind
                    || (c.page == self.page && l.top - c.last_top > 1.45 * pitch)
                    || match kind {
                        Kind::Indent => self.re.verse.is_match(&l.text()),
                        Kind::Item => self.re.marker.is_match(&l.text()),
                        _ => !c.last_full || indented,
                    }
            }
        };
        if new {
            self.close();
            self.flush_mark();
            self.entry_open = false;
            self.cur = Some(Cur { kind, indent: indented && kind == Kind::Body, content: Vec::new(), last_top: l.top, last_full: full, page: self.page, start: self.page });
        }
        let mark = self.pending.take();
        let mut new_refs = Vec::new();
        let c = self.cur.as_mut().expect("a paragraph is open");
        append(&mut c.content, &l.spans, self.dict, &mut self.refs, &mut new_refs, mark, false);
        c.last_top = l.top;
        c.last_full = full;
        c.page = self.page;
        let page = self.page;
        self.open_refs.extend(new_refs.into_iter().map(|r| (r, page)));
    }

    fn glossary_line(&mut self, l: &Line, x: f32, base: f32, right: f32) {
        let (term, desc): (Vec<Span>, Vec<Span>) = l.spans.iter().cloned().partition(|s| s.left < x - 10.0);
        let mut new_refs = Vec::new();
        if !term.is_empty() && !desc.is_empty() {
            self.close();
            self.flush_mark();
            let (mut t, mut d) = (Vec::new(), Vec::new());
            append(&mut t, &term, self.dict, &mut self.refs, &mut new_refs, None, false);
            append(&mut d, &desc, self.dict, &mut self.refs, &mut new_refs, None, false);
            self.blocks.push(Placed { page: self.page, block: Block::Entry { term: t, desc: d } });
            self.entry_open = true;
        } else if term.is_empty() && self.entry_open {
            let mark = self.pending.take();
            if let Some(Placed { block: Block::Entry { desc: d, .. }, .. }) = self.blocks.last_mut() {
                append(d, &desc, self.dict, &mut self.refs, &mut new_refs, mark, false);
            }
        } else {
            self.body_line(l, Kind::Body, base, l.right >= right - 15.0);
        }
        let page = self.page;
        self.open_refs.extend(new_refs.into_iter().map(|r| (r, page)));
    }

    fn note_line(&mut self, l: &Line, left: f32) {
        let text = l.text();
        let t = text.trim_start();
        let mut new_refs = Vec::new();
        if let Some(c) = self.re.note.captures(t)
            && l.left <= left + 4.0
        {
            let num: u32 = c[1].parse().unwrap_or(0);
            let last: Option<u32> = self.notes.iter().rev().find_map(|n| n.label.parse().ok());
            let plausible = match last {
                None => true,
                Some(n) => num == 1 || (num > n && num <= n + 3),
            };
            if plausible {
                let spans = cut_spans(&l.spans, text.len() - t.len() + c[0].len());
                let mut content = Vec::new();
                append(&mut content, &spans, self.dict, &mut self.refs, &mut new_refs, None, true);
                self.notes.push(Note { label: c[1].to_string(), content, page: self.page, reference: None });
                return;
            }
        }
        match self.notes.last_mut() {
            Some(n) => append(&mut n.content, &l.spans, self.dict, &mut self.refs, &mut new_refs, None, true),
            None => {
                let mut content = Vec::new();
                append(&mut content, &l.spans, self.dict, &mut self.refs, &mut new_refs, None, true);
                self.notes.push(Note { label: String::new(), content, page: self.page, reference: None });
            }
        }
    }

    fn link(&mut self, first_note: usize) {
        for k in first_note..self.notes.len() {
            let label = self.notes[k].label.clone();
            if let Some(pos) = self.open_refs.iter().position(|&(r, _)| self.refs[r].label == label && self.refs[r].note.is_none()) {
                let (r, _) = self.open_refs.remove(pos);
                self.refs[r].note = Some(k);
                self.notes[k].reference = Some(r);
            }
        }
        let page = self.page;
        self.open_refs.retain(|&(_, p)| p + 1 >= page);
    }

    fn page(&mut self, idx: usize, mut lines: Vec<Line>, printed: Option<u32>) {
        let st = self.st;
        self.page = idx;
        self.pending = printed;
        let base = st.left[idx % 2];
        let right = st.right[idx % 2];
        // The footnotes: the lines in their size at the foot of the page.
        let mut k = lines.len();
        while k > 0 && (lines[k - 1].size - st.note).abs() < 0.6 && !headingish(&lines[k - 1], st) {
            k -= 1;
        }
        let notes = if k > 0 { lines.split_off(k) } else { Vec::new() };
        let body = lines;
        let indents = indented_runs(&body, base, st);
        let items = list_runs(&body, base, st, self.re);
        let mut column: Option<Option<f32>> = None;
        for (i, l) in body.iter().enumerate() {
            if headingish(l, st) {
                self.heading(l);
                continue;
            }
            if self.section == Section::Glossary {
                let x = *column.get_or_insert_with(|| glossary_column(&body, base));
                if let Some(x) = x {
                    self.glossary_line(l, x, base, right);
                    continue;
                }
            }
            let kind = if (l.size - st.note).abs() < 0.6 {
                Kind::Small
            } else if box_line(l, st) {
                Kind::Box
            } else if items[i] {
                Kind::Item
            } else if indents[i] {
                Kind::Indent
            } else {
                Kind::Body
            };
            let full = l.right >= right - if kind == Kind::Box { 30.0 } else { 15.0 };
            self.body_line(l, kind, base, full);
        }
        let first_note = self.notes.len();
        let nleft = notes.iter().map(|l| l.left).fold(f32::MAX, f32::min);
        for l in &notes {
            self.note_line(l, nleft);
        }
        self.link(first_note);
        if let Some(p) = self.pending.take() {
            match &mut self.cur {
                Some(c) => c.content.push(Inline::Page(p)),
                None => self.blocks.push(Placed { page: idx, block: Block::PageMark(p) }),
            }
        }
    }
}

fn front_blocks(pages: &[(usize, Vec<Line>, f32)], dict: &Dict, re: &Res) -> Vec<Placed> {
    let mut out = Vec::new();
    for (idx, lines, width) in pages {
        let mut cur: Option<(Kind, Vec<Inline>, f32, f32)> = None;
        let mut refs = Vec::new();
        let mut nr = Vec::new();
        for l in lines {
            let center = ((l.left + l.right) / 2.0 - width / 2.0).abs() < width * 0.06 && (l.right - l.left) < width * 0.7;
            let kind = if center { Kind::Center } else { Kind::Body };
            let new = match &cur {
                None => true,
                Some((k, _, top, size)) => *k != kind || (l.size - size).abs() > 0.6 || l.top - top > 1.6 * size.max(l.size),
            };
            if new {
                if let Some((k, mut c, _, _)) = cur.take() {
                    tidy(&mut c, dict, re);
                    out.push(Placed { page: *idx, block: Block::Para { kind: k, indent: false, content: c } });
                }
                cur = Some((kind, Vec::new(), l.top, l.size));
            }
            let c = cur.as_mut().expect("open");
            append(&mut c.1, &l.spans, dict, &mut refs, &mut nr, None, true);
            c.2 = l.top;
        }
        if let Some((k, mut c, _, _)) = cur.take() {
            tidy(&mut c, dict, re);
            out.push(Placed { page: *idx, block: Block::Para { kind: k, indent: false, content: c } });
        }
    }
    out
}

fn norm(t: &str) -> String {
    t.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

fn parse_toc(lines: &[Line], dict: &Dict, re: &Res, last_page: u32) -> Vec<TocEntry> {
    let mut raw: Vec<(String, Option<u32>, f32)> = Vec::new();
    let mut pending: Option<(String, f32)> = None;
    let mut prev_page = 0u32;
    let clean = |l: &Line| {
        let mut c = Vec::new();
        append(&mut c, &l.spans, dict, &mut Vec::new(), &mut Vec::new(), None, true);
        tidy(&mut c, dict, re);
        plain(&c)
    };
    for l in lines {
        let t = clean(l);
        if t.is_empty() {
            continue;
        }
        let parsed = re.toc.captures(&t).and_then(|c| {
            let p: u32 = c[2].parse().ok()?;
            (p >= prev_page && p <= last_page.max(prev_page)).then(|| (c[1].trim().to_string(), p))
        });
        match parsed {
            Some((title, p)) => {
                prev_page = p;
                let (title, left) = match pending.take() {
                    Some((s, left)) => (format!("{s} {title}"), left),
                    None => (title, l.left),
                };
                raw.push((title, Some(p), left));
            }
            None => {
                pending = Some(match pending.take() {
                    Some((s, left)) => (format!("{s} {t}"), left),
                    None => (t, l.left),
                });
            }
        }
    }
    if let Some((s, left)) = pending {
        raw.push((s, None, left));
    }
    let mut lefts: Vec<f32> = Vec::new();
    for (_, _, l) in &raw {
        if !lefts.iter().any(|x| (x - l).abs() <= 4.0) {
            lefts.push(*l);
        }
    }
    lefts.sort_by(f32::total_cmp);
    raw.into_iter()
        .map(|(title, page, left)| TocEntry { title, page, depth: lefts.iter().position(|x| (x - left).abs() <= 4.0).unwrap_or(0), heading: None })
        .collect()
}

/// Builds the book from the PDF.
pub fn build(pdf: &Pdf) -> Book {
    let re = Res::new();
    let order = layout::order(pdf);
    let mut pages: Vec<Vec<Line>> = pdf
        .pages
        .iter()
        .map(|p| layout::lines(pdf, p, order).into_iter().filter(|l| !l.text().trim().is_empty()).collect())
        .collect();
    let n = pages.len();
    // Running heads and feet: the line at the same height at the top (bottom) of many pages; their number is the printed page.
    let mut found: Vec<Option<u32>> = vec![None; n];
    // A running head is short and carries the page number; a running foot is little more than the number. (The last line of
    // the text, at the same height on every full page, is neither.)
    let running = |l: &Line, top: bool| {
        let t = l.text();
        let t = t.trim();
        if top {
            t.chars().count() <= 90 && t.chars().any(|c| c.is_ascii_digit())
        } else {
            let core = t.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
            let core = core.trim_start_matches("seite").trim_start_matches("page").trim();
            !core.is_empty() && core.len() <= 4 && core.chars().all(|c| c.is_ascii_digit())
        }
    };
    for top in [true, false] {
        let mut ys: HashMap<i32, usize> = HashMap::new();
        let mut with = 0;
        for (i, p) in pages.iter().enumerate() {
            let h = pdf.pages[i].height.max(1.0);
            let l = if top { p.first() } else { p.last() };
            if let Some(l) = l
                && (if top { l.top < 0.12 * h } else { l.bottom > 0.9 * h })
                && running(l, top)
            {
                *ys.entry(q(l.top)).or_default() += 1;
            }
            if !p.is_empty() {
                with += 1;
            }
        }
        let Some((y, count)) = mode(&ys) else { continue };
        if count < 2 || count * 10 < with * 3 {
            continue;
        }
        for (i, p) in pages.iter_mut().enumerate() {
            let keep: Vec<Line> = std::mem::take(p);
            for l in keep {
                let is_head = running(&l, top) && if top { l.top <= y + 3.0 && l.top < 0.12 * pdf.pages[i].height } else { l.top >= y - 3.0 && l.bottom > 0.9 * pdf.pages[i].height };
                if is_head {
                    let t = l.text();
                    if found[i].is_none() {
                        found[i] = re.head_start.captures(&t).or_else(|| re.head_end.captures(&t)).and_then(|c| c[1].parse().ok());
                    }
                } else {
                    p.push(l);
                }
            }
        }
    }
    let mut offsets: HashMap<i32, usize> = HashMap::new();
    for (i, f) in found.iter().enumerate() {
        if let Some(p) = f {
            *offsets.entry((*p as i32 - i as i32) * 2).or_default() += 1;
        }
    }
    let offset = mode(&offsets).map(|m| m.0 as i32);
    let printed = |i: usize| offset.and_then(|o| u32::try_from(i as i32 + o).ok()).filter(|p| *p >= 1);
    let st = stats(&pages);
    let dict = dictionary(&pages);
    // Where the book proper begins: the table of contents, a preface, chapter 1.
    let front_end = (0..n)
        .find(|&i| {
            pages[i].iter().any(|l| {
                headingish(l, &st) && {
                    let t = l.text();
                    matches!(classify(&t, &re), Section::Toc | Section::Front) || re.first.is_match(t.trim())
                }
            })
        })
        .unwrap_or(0);
    // The printed table of contents.
    let mut toc_title = None;
    let mut toc_lines = Vec::new();
    let mut emptied = vec![false; n];
    if let Some((p0, i0)) = (front_end..n).find_map(|p| pages[p].iter().position(|l| headingish(l, &st) && classify(&l.text(), &re) == Section::Toc).map(|i| (p, i))) {
        let head = pages[p0].remove(i0);
        toc_title = Some(head.text().trim().to_string());
        'pages: for (p, lines) in pages.iter_mut().enumerate().skip(p0) {
            while !lines.is_empty() {
                let l = &lines[0];
                if headingish(l, &st) && l.size >= head.size - 0.5 {
                    break 'pages;
                }
                toc_lines.push(lines.remove(0));
            }
            emptied[p] = true;
        }
    }
    let last_page = (0..n).rev().find_map(printed).unwrap_or(9999);
    let mut toc = parse_toc(&toc_lines, &dict, &re, last_page);
    // The front matter: as it stands, centred lines and paragraphs.
    let front_pages: Vec<(usize, Vec<Line>, f32)> = (0..front_end).map(|i| (i, pages[i].clone(), pdf.pages[i].width)).collect();
    let front = front_blocks(&front_pages, &dict, &re);
    let front_text: String = front
        .iter()
        .filter_map(|p| match &p.block {
            Block::Para { content, .. } => Some(plain(content)),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut asm = Asm {
        st: &st,
        dict: &dict,
        re: &re,
        blocks: Vec::new(),
        cur: None,
        refs: Vec::new(),
        notes: Vec::new(),
        open_refs: Vec::new(),
        section: Section::Normal,
        special_size: 0.0,
        page: 0,
        pending: None,
        entry_open: false,
        headings: 0,
    };
    for (i, lines) in pages.into_iter().enumerate().skip(front_end) {
        if emptied[i] && lines.is_empty() {
            continue;
        }
        asm.page(i, lines, printed(i));
    }
    asm.close();
    let Asm { mut blocks, mut notes, refs, .. } = asm;
    for p in blocks.iter_mut() {
        if let Block::Entry { term, desc } = &mut p.block {
            tidy(term, &dict, &re);
            tidy(desc, &dict, &re);
        }
    }
    for n in notes.iter_mut() {
        tidy(&mut n.content, &dict, &re);
    }
    // Levels: parts (and front and back matter) on top, then the heading styles by size.
    let mut styles: HashMap<Style, (usize, usize)> = HashMap::new();
    for p in &blocks {
        if let Block::Heading(h) = &p.block {
            let e = styles.entry(h.style.clone()).or_default();
            e.0 += 1;
            if classify(&plain(&h.content), &re) == Section::Part {
                e.1 += 1;
            }
        }
    }
    let part_styles: Vec<Style> = styles.iter().filter(|(_, (n, p))| *p * 10 >= *n * 6 && *p > 0).map(|(s, _)| s.clone()).collect();
    let mut ranked: Vec<Style> = styles.keys().filter(|s| !part_styles.contains(s)).cloned().collect();
    ranked.sort_by(|a, b| b.size.cmp(&a.size).then(b.bold.cmp(&a.bold)).then(a.key.cmp(&b.key)));
    let has_part = !part_styles.is_empty() || blocks.iter().any(|p| matches!(&p.block, Block::Heading(h) if h.section == Section::Part));
    let base = if has_part { 2 } else { 1 };
    for p in blocks.iter_mut() {
        let pr = printed(p.page);
        if let Block::Heading(h) = &mut p.block {
            h.section = classify(&plain(&h.content), &re);
            h.printed = pr;
            h.nav = (h.style.size as f32 / 2.0) > st.body + 0.5;
            h.level = match h.section {
                Section::Part | Section::Bibliography | Section::Glossary | Section::Index | Section::Toc => 1,
                Section::Front => base,
                Section::Normal if part_styles.contains(&h.style) => 1,
                Section::Normal => (base as usize + ranked.iter().position(|s| *s == h.style).unwrap_or(0)).min(6) as u8,
            };
        }
    }
    // The entries of the printed table of contents lead to their headings, in order.
    let heads: Vec<(usize, String, Option<u32>)> = blocks
        .iter()
        .filter_map(|p| match &p.block {
            Block::Heading(h) => Some((h.id, norm(&plain(&h.content)), h.printed)),
            _ => None,
        })
        .collect();
    let mut from = 0;
    for e in toc.iter_mut() {
        let en = norm(&e.title);
        if en.is_empty() {
            continue;
        }
        let fits = |(_, hn, hp): &(usize, String, Option<u32>)| {
            let k = en.chars().count().min(hn.chars().count()).min(30);
            let a: String = en.chars().take(k).collect();
            let b: String = hn.chars().take(k).collect();
            k >= 3 && a == b && match (e.page, hp) {
                (Some(x), Some(y)) => (x as i64 - *y as i64).abs() <= 2,
                _ => true,
            }
        };
        if let Some(pos) = heads[from..].iter().position(fits).map(|p| p + from).or_else(|| heads.iter().position(fits)) {
            e.heading = Some(heads[pos].0);
            from = pos + 1;
        }
    }
    let isbn = re.isbn.captures(&front_text).map(|c| c[1].chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>());
    let (year, publisher) = re.copy.captures(&front_text).map_or((None, None), |c| (Some(c[1].to_string()), Some(c[2].trim().to_string())));
    Book { front, toc_title, toc, blocks, notes, refs, isbn, publisher, year, pages: n, rtl: order.rtl }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn res() -> Res {
        Res::new()
    }

    #[test]
    fn headings_are_known_by_their_words() {
        let re = res();
        assert_eq!(classify("Inhalt", &re), Section::Toc);
        assert_eq!(classify("Bibliografie", &re), Section::Bibliography);
        assert_eq!(classify("Abkürzungen", &re), Section::Glossary);
        assert_eq!(classify("Bibelstellenregister", &re), Section::Index);
        assert_eq!(classify("Teil\u{a0}1: Gottes Gerechtigkeit", &re), Section::Part);
        assert_eq!(classify("Einleitung", &re), Section::Front);
        assert_eq!(classify("Kapitel 3", &re), Section::Normal);
    }

    #[test]
    fn the_hyphenation_is_taken_back_and_torn_words_are_joined() {
        let re = res();
        let mut dict = Dict::new();
        dict.insert("meiner".into(), 3);
        dict.insert("nationen".into(), 4);
        let mut c = vec![Inline::Text { text: "von mei-".into(), bold: false, italic: false, greek: false }];
        join(&mut c, "ner", false, &dict);
        push_text(&mut c, "ner Mutter", false, false, false);
        let mut d = vec![Inline::Text { text: "Pha\u{ad}".into(), bold: false, italic: false, greek: false }];
        join(&mut d, "risäer", false, &dict);
        push_text(&mut d, "risäer und Daniel\u{ad}", false, false, false);
        join(&mut d, "Verlag", true, &dict);
        push_text(&mut d, "Verlag, Mt 10,1\u{ad}5, die Na tionen", false, false, false);
        tidy(&mut d, &dict, &re);
        assert_eq!(plain(&c), "von meiner Mutter");
        assert_eq!(plain(&d), "Pharisäer und Daniel-Verlag, Mt 10,1-5, die Nationen");
        let mut e = vec![Inline::Text { text: "Ost-".into(), bold: false, italic: false, greek: false }];
        join(&mut e, "und", false, &dict);
        push_text(&mut e, "und Westrom", false, false, false);
        assert_eq!(plain(&e), "Ost- und Westrom", "the hyphen standing for a part of a word stays");
    }
}
