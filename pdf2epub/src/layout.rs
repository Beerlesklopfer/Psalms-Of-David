// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! From the pieces of a page to its lines in reading order: the pieces at the same height form a line; a small number raised in a
//! line of bigger text is the mark of a footnote.
//!
//! Right-to-left text (Hebrew, Arabic, Syriac …) comes out of the PDF in the order it was drawn, not the order it is read in, and
//! every program that makes PDFs draws it its own way: one glyph at a time from right to left with the vowel points as pieces
//! of their own (pango, InDesign), or whole words from left to right with the points before their letter (XeTeX) or after it
//! (LibreOffice, Word). So the line is first laid out as it is seen, from left to right, the points with their letters, and then
//! put back in reading order the way the Unicode bidirectional algorithm would have laid it out: the runs of the other direction
//! turned round, numbers kept as they are, brackets mirrored. In a book written from right to left the page is mirrored as well,
//! so that the start of a line, its indent and its end are where the rest of the program looks for them.

use crate::pdf::{Item, Page, Pdf, Run};
use crate::script::{self, Dir};

/// A piece of a line with one emphasis.
#[derive(Debug, Clone)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub greek: bool,
    /// A raised small number or star: the mark of a footnote.
    pub sup: bool,
    pub size: f32,
    pub key: String,
    pub left: f32,
    pub right: f32,
}

#[derive(Debug, Clone)]
pub struct Line {
    pub top: f32,
    pub bottom: f32,
    /// Where the line starts and ends; in a right-to-left book measured from the right edge of the page.
    pub left: f32,
    pub right: f32,
    /// Size and font family of most of its letters.
    pub size: f32,
    pub key: String,
    /// Most of its letters are bold.
    pub bold: bool,
    pub spans: Vec<Span>,
}

impl Line {
    pub fn text(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }
}

fn is_note_mark(t: &str) -> bool {
    let t = t.trim();
    !t.is_empty() && t.chars().count() <= 4 && (t.chars().all(|c| c.is_ascii_digit()) || t.chars().all(|c| matches!(c, '*' | '†' | '‡')))
}

/// How the right-to-left text of a book was drawn and which way the book runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Order {
    /// The book is written from right to left: more of its letters belong to right-to-left scripts than to the others.
    pub rtl: bool,
    /// Words of right-to-left script come as they were drawn, from left to right (XeTeX, LibreOffice, Word), not in reading
    /// order (as a few programs write them).
    pub words_turned: bool,
}

/// Evidence that right-to-left words are in reading order (first) or turned round (second): the final forms of Hebrew stand at
/// the end of a word, so do taa marbuta and alef maksura in Arabic, and the article al- at its start.
fn turned_evidence(text: &str) -> (usize, usize) {
    let text = script::normalize(text);
    let (mut reading, mut turned) = (0, 0);
    for word in text.split(|c: char| !matches!(script::dir(c), Dir::R | Dir::Mark)) {
        let letters: Vec<char> = word.chars().filter(|c| script::dir(*c) == Dir::R).collect();
        if letters.len() < 2 {
            continue;
        }
        let (first, last) = (letters[0], letters[letters.len() - 1]);
        let end_only = |c: char| matches!(c, 'ך' | 'ם' | 'ן' | 'ף' | 'ץ' | 'ة' | 'ى');
        reading += usize::from(end_only(last)) + usize::from(first == 'ا' && letters[1] == 'ل');
        turned += usize::from(end_only(first)) + usize::from(last == 'ا' && letters[letters.len() - 2] == 'ل' && letters.len() > 3);
    }
    (reading, turned)
}

/// The order of the book, from all its text.
pub fn order(pdf: &Pdf) -> Order {
    let (mut l, mut r) = (0usize, 0usize);
    let (mut reading, mut turned) = (0usize, 0usize);
    for it in pdf.pages.iter().flat_map(|p| &p.items) {
        let text: String = it.runs.iter().map(|r| r.text.as_str()).collect();
        for c in text.chars() {
            match script::dir(c) {
                Dir::L => l += 1,
                Dir::R => r += 1,
                _ => {}
            }
        }
        let (a, b) = turned_evidence(&text);
        reading += a;
        turned += b;
    }
    // Every program measured draws them turned round; only clear evidence says otherwise.
    Order { rtl: r > l, words_turned: turned >= reading }
}

#[derive(Debug, Clone, PartialEq)]
struct Style {
    bold: bool,
    italic: bool,
    greek: bool,
    sup: bool,
    size: f32,
    key: String,
}

/// A piece of poppler on its way into a line.
struct Piece {
    order: usize,
    top: f32,
    left: f32,
    right: f32,
    size: f32,
    key: String,
    bold: bool,
    italic: bool,
    runs: Vec<Run>,
    /// Its text is in reading order already (one glyph with its points), not in the order of drawing.
    logical: bool,
}

impl Piece {
    fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }
}

/// A letter with its points (or a digit, a space, a punctuation mark) at its place in the line.
#[derive(Debug, Clone)]
struct Cluster {
    text: String,
    dir: Dir,
    style: usize,
    /// The piece of poppler it comes from: a span never reaches over two pieces (the gaps between them tell columns apart).
    piece: usize,
    left: f32,
    right: f32,
}

fn style_index(styles: &mut Vec<Style>, s: Style) -> usize {
    styles.iter().position(|x| *x == s).unwrap_or_else(|| {
        styles.push(s);
        styles.len() - 1
    })
}

/// The letters of a piece as seen from left to right, each with its points.
fn clusters(p: &Piece, piece: usize, sup: bool, order: Order, base_rtl: bool, styles: &mut Vec<Style>, out: &mut Vec<Cluster>) {
    let text = p.text();
    let rtl_letters = text.chars().filter(|c| script::dir(*c) == Dir::R).count();
    let style_of = |r: &Run, styles: &mut Vec<Style>| {
        style_index(styles, Style { bold: r.bold || p.bold, italic: r.italic || p.italic, greek: r.greek, sup, size: p.size, key: p.key.clone() })
    };
    // One glyph of right-to-left script with its points, as pango and InDesign draw them: in reading order already.
    if p.logical && rtl_letters > 0 || sup {
        let style = style_of(&p.runs[0], styles);
        let text = if sup { text.trim().to_string() } else { text };
        out.push(Cluster { dir: script::dir_of(&text), text, style, piece, left: p.left, right: p.right });
        return;
    }
    // Words drawn from left to right: the points of a right-to-left letter come before it (XeTeX: the whole word turned round) or
    // after it (LibreOffice, Word: the letters turned round, each with its points). The first letter of a pointed word nearly
    // always carries a point, so a word that ends in a point (seen from the left) has its points after their letters.
    // Words in reading order already: laid out as they would be seen, so that they come back in reading order like the others.
    if rtl_letters >= 2 && !order.words_turned {
        let mut own: Vec<Cluster> = Vec::new();
        for r in &p.runs {
            let style = style_of(r, styles);
            for c in r.text.chars() {
                match own.last_mut() {
                    Some(last) if script::dir(c) == Dir::Mark => last.text.push(c),
                    _ => own.push(Cluster { text: c.to_string(), dir: script::dir(c), style, piece, left: p.left, right: p.right }),
                }
            }
        }
        reading_order(&mut own, base_rtl);
        out.extend(own);
        return;
    }
    let points_before = rtl_letters >= 2
        && !text.split_whitespace().any(|w| w.chars().any(|c| script::dir(c) == Dir::R) && w.chars().last().is_some_and(|c| script::dir(c) == Dir::Mark));
    let mut pending: Vec<char> = Vec::new();
    for r in &p.runs {
        let style = style_of(r, styles);
        for c in r.text.chars() {
            let d = script::dir(c);
            if d == Dir::Mark {
                if points_before {
                    pending.push(c);
                } else if let Some(last) = out.last_mut() {
                    last.text.push(c);
                } else {
                    out.push(Cluster { text: c.to_string(), dir: Dir::Neutral, style, piece, left: p.left, right: p.right });
                }
                continue;
            }
            let mut text = c.to_string();
            if !pending.is_empty() {
                if d == Dir::R {
                    text.extend(pending.drain(..).rev());
                } else if let Some(last) = out.last_mut() {
                    last.text.extend(pending.drain(..));
                }
            }
            out.push(Cluster { text, dir: d, style, piece, left: p.left, right: p.right });
        }
    }
    if let Some(last) = out.last_mut() {
        last.text.extend(pending.drain(..));
    }
}

/// Without the spaces poppler puts between the points of one letter when they stand apart: a space is never followed by a
/// combining mark in a text.
fn no_space_before_marks(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    chars.iter().enumerate().filter(|(i, c)| !(c.is_whitespace() && chars.get(i + 1).is_some_and(|n| script::dir(*n) == Dir::Mark))).map(|(_, c)| *c).collect()
}

fn is_number_part(c: &Cluster) -> bool {
    c.dir == Dir::Digit || (c.dir == Dir::Neutral && matches!(c.text.as_str(), "." | "," | ":" | "/" | "-" | "–"))
}

fn mirror(c: &mut Cluster) {
    if c.dir == Dir::Neutral {
        c.text = c.text.chars().map(script::mirror).collect();
    }
}

/// Turns the numbers inside a run that was turned round back again: they run from left to right in any text.
fn keep_numbers(run: &mut [Cluster]) {
    let mut i = 0;
    while i < run.len() {
        if run[i].dir != Dir::Digit {
            mirror(&mut run[i]);
            i += 1;
            continue;
        }
        let mut j = i;
        let mut end = i;
        while j < run.len() && is_number_part(&run[j]) {
            if run[j].dir == Dir::Digit {
                end = j;
            }
            j += 1;
        }
        run[i..=end].reverse();
        i = end + 1;
    }
}

/// The line as it is seen (left to right) put in reading order.
fn reading_order(cs: &mut [Cluster], rtl: bool) {
    // The runs written against the direction of the book: from its first letter of that direction to its last, with what is
    // between (spaces, punctuation, numbers), as long as no letter of the book's own direction comes in between.
    let against = |d: Dir| if rtl { matches!(d, Dir::L | Dir::Digit) } else { d == Dir::R };
    let own = |d: Dir| if rtl { d == Dir::R } else { d == Dir::L };
    if rtl {
        cs.reverse();
    }
    let mut i = 0;
    let mut runs: Vec<(usize, usize)> = Vec::new();
    while i < cs.len() {
        if !against(cs[i].dir) {
            i += 1;
            continue;
        }
        let mut end = i;
        let mut j = i + 1;
        while j < cs.len() && !own(cs[j].dir) {
            if against(cs[j].dir) {
                end = j;
            }
            j += 1;
        }
        runs.push((i, end));
        i = end + 1;
    }
    if rtl {
        // The whole line was turned round; the left-to-right runs in it are turned back, the rest is right-to-left text whose
        // brackets were drawn mirrored.
        let mut k = 0;
        for &(a, b) in &runs {
            cs[k..a].iter_mut().for_each(mirror);
            cs[a..=b].reverse();
            k = b + 1;
        }
        cs[k..].iter_mut().for_each(mirror);
    } else {
        for &(a, b) in &runs {
            cs[a..=b].reverse();
            keep_numbers(&mut cs[a..=b]);
        }
    }
}

/// Whether a line (its pieces as they are seen, from left to right) is read from right to left. A paragraph takes the direction of its first letter (as the
/// bidirectional algorithm has it): a line whose letters at both ends run from left to right is a left-to-right line, one whose
/// letters at both ends run from right to left a right-to-left one; for the others the direction of the book decides.
fn line_rtl(pieces: &[Piece], book_rtl: bool) -> bool {
    let dirs: Vec<Dir> = pieces.iter().flat_map(|p| p.runs.iter().flat_map(|r| r.text.chars())).map(script::dir).filter(|d| matches!(d, Dir::L | Dir::R)).collect();
    match (dirs.first().copied(), dirs.last().copied()) {
        (Some(Dir::L), Some(Dir::L)) => false,
        (Some(Dir::R), Some(Dir::R)) => true,
        _ => book_rtl,
    }
}

/// The lines of a page, top to bottom, each in reading order.
pub fn lines(pdf: &Pdf, page: &Page, order: Order) -> Vec<Line> {
    let rtl = order.rtl;
    let mut items: Vec<(usize, &Item)> = page.items.iter().enumerate().collect();
    items.sort_by(|a, b| a.1.top.total_cmp(&b.1.top).then(a.1.left.total_cmp(&b.1.left)));
    // Pieces belong to one line when they overlap in height by at least half of the lower one (a raised mark does, a title
    // above a bigger title does not).
    type Group<'a> = (f32, f32, f32, Vec<(usize, &'a Item)>);
    let mut groups: Vec<Group> = Vec::new();
    for (order, it) in items {
        let size = pdf.font(it.font).size;
        let (a, b) = (it.top, it.top + it.height.max(1.0));
        match groups.last_mut() {
            Some((top, bottom, gsize, g)) if (b.min(*bottom) - a.max(*top)) >= 0.5 * (b - a).min(*bottom - *top) => {
                *gsize = gsize.max(size);
                *top = top.min(a);
                *bottom = bottom.max(b);
                g.push((order, it));
            }
            _ => groups.push((a, b, size, vec![(order, it)])),
        }
    }
    let mut out = Vec::new();
    for (_, _, max, g) in groups {
        let mut pieces: Vec<Piece> = g
            .iter()
            .map(|(order, it)| {
                let f = pdf.font(it.font);
                Piece {
                    order: *order,
                    top: it.top,
                    left: it.left,
                    right: it.left + it.width,
                    size: f.size,
                    key: f.key(),
                    bold: f.bold(),
                    italic: f.italic(),
                    runs: it.runs.iter().map(|r| Run { text: no_space_before_marks(&r.text), ..r.clone() }).collect(),
                    logical: false,
                }
            })
            .collect();
        let bottom = g.iter().map(|(_, it)| it.top + it.height).fold(0.0, f32::max);
        // A space drawn on top of a word that has its spaces already.
        let blank = |p: &Piece| p.text().trim().is_empty();
        let covered: Vec<bool> = pieces
            .iter()
            .map(|p| blank(p) && pieces.iter().any(|q| !blank(q) && (p.left + p.right) / 2.0 > q.left + 0.5 && (p.left + p.right) / 2.0 < q.right - 0.5))
            .collect();
        let mut keep = covered.iter().map(|c| !c);
        pieces.retain(|_| keep.next().unwrap_or(true));
        // Spaces at the start or the end of a line say nothing.
        let first_word = pieces.iter().filter(|p| !blank(p)).map(|p| p.left).fold(f32::MAX, f32::min);
        let last_word = pieces.iter().filter(|p| !blank(p)).map(|p| p.right).fold(f32::MIN, f32::max);
        pieces.retain(|p| !blank(p) || (p.left >= first_word && p.right <= last_word));
        if pieces.is_empty() {
            continue;
        }
        // Points drawn as pieces of their own belong to the glyph drawn just before them, or else to the one they stand on.
        pieces.sort_by_key(|p| p.order);
        let mut i = 0;
        while i < pieces.len() {
            let starts_with_mark = pieces[i].text().chars().next().is_some_and(|c| script::dir(c) == Dir::Mark);
            if !starts_with_mark || pieces.len() == 1 {
                i += 1;
                continue;
            }
            let center = (pieces[i].left + pieces[i].right) / 2.0;
            let tol = 0.2 * pieces[i].size;
            let on = |q: &Piece| center >= q.left - tol && center <= q.right + tol;
            // Right-to-left glyphs drawn one by one: the points (with the letter after them, as poppler joins them) follow their
            // letter in the order of drawing, whatever the places poppler gives them say.
            let prev_rtl = i > 0 && pieces[i - 1].runs.iter().flat_map(|r| r.text.chars()).any(|c| script::dir(c) == Dir::R);
            let target = if i > 0 && pieces[i - 1].order + 1 == pieces[i].order && (prev_rtl || on(&pieces[i - 1])) {
                Some(i - 1)
            } else {
                (0..pieces.len())
                    .filter(|&j| j != i && on(&pieces[j]))
                    .min_by(|&a, &b| ((pieces[a].left + pieces[a].right) / 2.0 - center).abs().total_cmp(&((pieces[b].left + pieces[b].right) / 2.0 - center).abs()))
            };
            let Some(t) = target else {
                i += 1;
                continue;
            };
            let m = pieces.remove(i);
            let t = if t > i { t - 1 } else { t };
            let text = m.text();
            let tp = &mut pieces[t];
            match tp.runs.last_mut() {
                Some(r) => r.text.push_str(&text),
                None => tp.runs.push(Run { text, bold: false, italic: false, greek: false }),
            }
            tp.logical = true;
            tp.left = tp.left.min(m.left);
            tp.right = tp.right.max(m.right);
            // The next piece may be a point of the same glyph: it now follows the glyph in the order of drawing.
            tp.order = m.order;
            // The piece after the point has moved up to `i`.
        }
        pieces.sort_by(|a, b| a.left.total_cmp(&b.left).then(a.order.cmp(&b.order)));
        let all_small = pieces.iter().all(|p| p.size <= 0.8 * max);
        let base_rtl = line_rtl(&pieces, rtl);
        let spaces_drawn = pieces.iter().any(blank);
        // The line as it is seen.
        let mut styles: Vec<Style> = Vec::new();
        let mut cs: Vec<Cluster> = Vec::new();
        let mut sups: Vec<bool> = Vec::new();
        let mut prev_right: Option<f32> = None;
        let mut prev_logical = false;
        for (pi, p) in pieces.iter().enumerate() {
            let text = p.text();
            let sup = !all_small && p.size <= 0.8 * max && p.runs.len() == 1 && is_note_mark(&text);
            if let Some(r) = prev_right
                && !sup
                && let Some(last) = cs.last()
                && !last.text.ends_with(char::is_whitespace)
                && !text.starts_with(char::is_whitespace)
            {
                // Between two right-to-left glyphs drawn one by one the gaps say little; where the spaces are drawn, they say it.
                // A glyph joined to the points drawn as pieces of their own has the place and width of the points, not its own.
                let (a, b) = (last.dir == Dir::R, script::dir_of(&text) == Dir::R);
                let unsure = (a || b) && (p.logical || prev_logical);
                let gap = p.left - r;
                let space = if a && b || unsure { !spaces_drawn && gap > 0.5 * p.size } else { gap > 0.15 * p.size };
                if space {
                    let style = if styles[last.style].sup {
                        let st = Style { sup: false, ..styles[last.style].clone() };
                        style_index(&mut styles, st)
                    } else {
                        last.style
                    };
                    cs.push(Cluster { text: " ".into(), dir: Dir::Neutral, style, piece: last.piece, left: r, right: p.left });
                }
            }
            let n = cs.len();
            clusters(p, pi, sup, order, base_rtl, &mut styles, &mut cs);
            sups.extend(std::iter::repeat_n(sup, cs.len() - n));
            prev_right = Some(prev_right.map_or(p.right, |r| r.max(p.right)));
            prev_logical = p.logical;
        }
        reading_order(&mut cs, base_rtl);
        // Clusters of one style are one span.
        let width = page.width;
        let flip = |l: f32, r: f32| if rtl { (width - r, width - l) } else { (l, r) };
        let mut spans: Vec<Span> = Vec::new();
        let mut last = (usize::MAX, usize::MAX);
        for c in cs {
            let (l, r) = flip(c.left, c.right);
            if (c.style, c.piece) == last
                && let Some(s) = spans.last_mut()
            {
                s.text.push_str(&c.text);
                s.left = s.left.min(l);
                s.right = s.right.max(r);
                continue;
            }
            let st = &styles[c.style];
            last = (c.style, c.piece);
            spans.push(Span { text: c.text, bold: st.bold, italic: st.italic, greek: st.greek, sup: st.sup, size: st.size, key: st.key.clone(), left: l, right: r });
        }
        for s in &mut spans {
            s.text = script::normalize(&s.text);
        }
        let words: Vec<&Piece> = pieces.iter().filter(|p| !blank(p)).collect();
        let main: Vec<&Piece> = words.iter().copied().filter(|p| !(p.size <= 0.8 * max && !all_small)).collect();
        let main: Vec<&Piece> = if main.is_empty() { words.clone() } else { main };
        // Size, family and boldness of most letters.
        let mut weights: Vec<(f32, String, usize, usize)> = Vec::new();
        for s in spans.iter().filter(|s| !s.sup) {
            let n = s.text.chars().filter(|c| !c.is_whitespace()).count();
            let b = if s.bold { n } else { 0 };
            match weights.iter_mut().find(|w| w.0 == s.size && w.1 == s.key) {
                Some(w) => {
                    w.2 += n;
                    w.3 += b;
                }
                None => weights.push((s.size, s.key.clone(), n, b)),
            }
        }
        let total: usize = weights.iter().map(|w| w.2).sum();
        let bold: usize = weights.iter().map(|w| w.3).sum();
        let (size, key) = weights.iter().max_by_key(|w| w.2).map(|w| (w.0, w.1.clone())).unwrap_or((max, String::new()));
        let (left, right) = flip(main.iter().map(|p| p.left).fold(f32::MAX, f32::min), words.iter().map(|p| p.right).fold(0.0, f32::max));
        let top = main.iter().map(|p| p.top).fold(f32::MAX, f32::min);
        out.push(Line { top, bottom, left, right, size, key, bold: total > 0 && bold * 2 > total, spans });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::Font;

    fn item(left: f32, width: f32, font: u32, text: &str) -> Item {
        Item { top: 100.0, left, width, height: 12.0, font, runs: vec![Run { text: text.into(), bold: false, italic: false, greek: false }] }
    }

    const LTR: Order = Order { rtl: false, words_turned: true };

    fn line(items: Vec<Item>, order: Order) -> String {
        let mut pdf = Pdf::default();
        pdf.fonts.insert(0, Font { size: 12.0, family: "Serif".into() });
        pdf.fonts.insert(1, Font { size: 12.0, family: "Hebrew".into() });
        pdf.fonts.insert(2, Font { size: 7.0, family: "Serif".into() });
        let page = Page { width: 500.0, height: 800.0, items };
        lines(&pdf, &page, order).iter().map(Line::text).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn hebrew_drawn_glyph_by_glyph_from_the_right_with_its_points_apart() {
        // As pango draws it: every glyph a piece, from right to left, the points as pieces of their own after their letter.
        let items = vec![
            item(15.0, 108.0, 0, "Im Anfang ("),
            item(214.0, 10.0, 1, "ב"),
            item(213.0, 1.0, 1, "\u{05b0}\u{05bc}"),
            item(204.0, 10.0, 1, "ר"),
            item(203.0, 0.0, 1, "\u{05b5}"),
            item(192.0, 12.0, 1, "א"),
            item(178.0, 13.0, 1, "ש"),
            item(179.0, 0.0, 1, "\u{05b4}\u{05c1}י"),
            item(162.0, 12.0, 1, "ת"),
            item(156.0, 6.0, 0, " "),
            item(146.0, 10.0, 1, "ב"),
            item(144.0, 1.0, 1, "\u{05b8}\u{05bc}"),
            item(135.0, 10.0, 1, "ר"),
            item(133.0, 0.0, 1, "\u{05b8}"),
            item(123.0, 12.0, 1, "א"),
            item(225.0, 114.0, 0, ") schuf Gott."),
        ];
        assert_eq!(line(items, LTR), script::normalize("Im Anfang (בְּרֵאשִׁית בָּרָא) schuf Gott."));
    }

    #[test]
    fn hebrew_drawn_as_words_turned_round_with_the_points_before_their_letters() {
        // XeTeX: the word turned round character by character.
        let word: String = "בְּרֵאשִׁית בָּרָא".chars().rev().collect();
        let items = vec![item(223.0, 89.0, 0, "Im Anfang ("), item(313.0, 83.0, 1, &word), item(395.0, 162.0, 0, ") schuf Gott, Gen 1,1.")];
        assert_eq!(line(items, LTR), script::normalize("Im Anfang (בְּרֵאשִׁית בָּרָא) schuf Gott, Gen 1,1."));
    }

    #[test]
    fn hebrew_drawn_as_words_turned_round_with_the_points_after_their_letters() {
        // LibreOffice: the letters turned round, each with its points; the Hebrew phrase opens the line, the German follows.
        let logical = "בְּרֵאשִׁית בָּרָא";
        let mut clusters: Vec<String> = Vec::new();
        for c in logical.chars() {
            if script::dir(c) == Dir::Mark {
                clusters.last_mut().unwrap().push(c);
            } else {
                clusters.push(c.to_string());
            }
        }
        let seen: String = clusters.into_iter().rev().collect();
        let items = vec![item(85.0, 432.0, 0, &format!("{seen}, Gen 1,1 und 23 Verse."))];
        assert_eq!(line(items, LTR), script::normalize("בְּרֵאשִׁית בָּרָא, Gen 1,1 und 23 Verse."));
    }

    #[test]
    fn arabic_with_a_number_and_a_ligature() {
        // The number stays as it is, the lam-alef becomes its two letters, the spaces drawn on top of the words are dropped.
        let items = vec![
            item(85.0, 36.0, 1, "باتك"),
            item(121.0, 9.0, 0, " "),
            item(130.0, 18.0, 0, "12"),
            item(148.0, 9.0, 0, " "),
            item(157.0, 198.0, 1, "مﻼسلا"),
            item(193.0, 9.0, 0, " "),
        ];
        assert_eq!(line(items, LTR), "السلام 12 كتاب");
    }

    #[test]
    fn a_right_to_left_book_with_latin_and_numbers_in_it() {
        // In a Hebrew book the page is read from the right: the Latin word and the number stay as they are written, the
        // brackets drawn mirrored are put right.
        let seen = |s: &str| s.chars().rev().collect::<String>();
        let items = vec![
            item(100.0, 15.0, 0, "12"),
            item(120.0, 30.0, 1, &seen("פרק")),
            item(160.0, 30.0, 0, "pax"),
            item(195.0, 5.0, 0, "("),
            item(200.0, 55.0, 1, &seen("שָׁלוֹם")),
            item(255.0, 5.0, 0, ")"),
        ];
        let mut pdf = Pdf::default();
        pdf.fonts.insert(0, Font { size: 12.0, family: "Serif".into() });
        pdf.fonts.insert(1, Font { size: 12.0, family: "Hebrew".into() });
        let page = Page { width: 500.0, height: 800.0, items };
        let ls = lines(&pdf, &page, Order { rtl: true, words_turned: true });
        assert_eq!(ls[0].text(), script::normalize("(שָׁלוֹם) pax פרק 12"));
        // Measured from the right edge: the line starts 240 from it and ends 400 from it.
        assert_eq!((ls[0].left, ls[0].right), (240.0, 400.0));
    }

    #[test]
    fn words_in_reading_order_are_told_from_words_turned_round() {
        let pdf = |t: &str| Pdf { pages: vec![Page { width: 500.0, height: 800.0, items: vec![item(10.0, 100.0, 1, t)] }], ..Pdf::default() };
        // Final mem and final nun at the end: in reading order.
        assert!(!order(&pdf("שָׁלוֹם בְּמָקוֹם אֶחָד וְזְמַן")).words_turned);
        let turned: String = "שָׁלוֹם בְּמָקוֹם אֶחָד וְזְמַן".chars().rev().collect();
        assert!(order(&pdf(&turned)).words_turned);
        // The Arabic article at the start, taa marbuta at the end.
        assert!(!order(&pdf("الكتاب رحمة")).words_turned);
        assert!(order(&pdf("باتكلا ةمحر")).words_turned);
        // In reading order the words go into the line as they are.
        let items = vec![item(10.0, 40.0, 0, "Wort"), item(60.0, 60.0, 1, "שָׁלוֹם"), item(130.0, 40.0, 0, "Ende")];
        assert_eq!(line(items, Order { rtl: false, words_turned: false }), script::normalize("Wort שָׁלוֹם Ende"));
    }

    #[test]
    fn spaces_between_the_points_of_a_letter_are_dropped() {
        assert_eq!(no_space_before_marks("\u{05bc} \u{05b0}"), "\u{05bc}\u{05b0}");
        assert_eq!(no_space_before_marks("a b"), "a b");
    }

    #[test]
    fn a_footnote_mark_stays_raised() {
        let items = vec![item(10.0, 60.0, 0, "Wort"), Item { top: 98.0, ..item(70.0, 4.0, 2, "3") }, item(76.0, 40.0, 0, " weiter")];
        let mut pdf = Pdf::default();
        pdf.fonts.insert(0, Font { size: 12.0, family: "Serif".into() });
        pdf.fonts.insert(2, Font { size: 7.0, family: "Serif".into() });
        let page = Page { width: 500.0, height: 800.0, items };
        let ls = lines(&pdf, &page, LTR);
        assert_eq!(ls[0].text(), "Wort3 weiter");
        assert!(ls[0].spans.iter().any(|s| s.sup && s.text == "3"));
    }
}
