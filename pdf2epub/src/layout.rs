// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! From the pieces of a page to its lines: the pieces at the same height form a line, from left to right; a small number raised
//! in a line of bigger text is the mark of a footnote.

use crate::pdf::{Page, Pdf};

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

fn is_mark(t: &str) -> bool {
    let t = t.trim();
    !t.is_empty() && t.chars().count() <= 4 && (t.chars().all(|c| c.is_ascii_digit()) || t.chars().all(|c| matches!(c, '*' | '†' | '‡')))
}

/// The lines of a page, top to bottom.
pub fn lines(pdf: &Pdf, page: &Page) -> Vec<Line> {
    let mut items: Vec<_> = page.items.iter().collect();
    items.sort_by(|a, b| a.top.total_cmp(&b.top).then(a.left.total_cmp(&b.left)));
    // Pieces belong to one line when they overlap in height by at least half of the lower one (a raised mark does, a title
    // above a bigger title does not).
    let mut groups: Vec<(f32, f32, f32, Vec<&crate::pdf::Item>)> = Vec::new();
    for it in items {
        let size = pdf.font(it.font).size;
        let (a, b) = (it.top, it.top + it.height.max(1.0));
        match groups.last_mut() {
            Some((top, bottom, gsize, g)) if (b.min(*bottom) - a.max(*top)) >= 0.5 * (b - a).min(*bottom - *top) => {
                *gsize = gsize.max(size);
                *top = top.min(a);
                *bottom = bottom.max(b);
                g.push(it);
            }
            _ => groups.push((a, b, size, vec![it])),
        }
    }
    let mut out = Vec::new();
    for (_, _, max, mut g) in groups {
        g.sort_by(|a, b| a.left.total_cmp(&b.left));
        let all_small = g.iter().all(|it| pdf.font(it.font).size <= 0.8 * max);
        let mut spans: Vec<Span> = Vec::new();
        let mut prev_right: Option<f32> = None;
        for it in &g {
            let f = pdf.font(it.font);
            let sup = !all_small && f.size <= 0.8 * max && it.runs.len() == 1 && is_mark(&it.runs[0].text);
            if let Some(r) = prev_right
                && it.left - r > 0.15 * f.size
                && !sup
                && let Some(last) = spans.last_mut()
                && !last.text.ends_with(char::is_whitespace)
                && !it.runs[0].text.starts_with(char::is_whitespace)
            {
                last.text.push(' ');
            }
            for r in &it.runs {
                spans.push(Span {
                    text: if sup { r.text.trim().to_string() } else { r.text.clone() },
                    bold: r.bold || f.bold(),
                    italic: r.italic || f.italic(),
                    greek: r.greek,
                    sup,
                    size: f.size,
                    key: f.key(),
                    left: it.left,
                    right: it.left + it.width,
                });
            }
            prev_right = Some(it.left + it.width);
        }
        let main: Vec<&crate::pdf::Item> = g.iter().copied().filter(|it| !(pdf.font(it.font).size <= 0.8 * max && !all_small)).collect();
        let main = if main.is_empty() { g.clone() } else { main };
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
        out.push(Line {
            top: main.iter().map(|it| it.top).fold(f32::MAX, f32::min),
            bottom: g.iter().map(|it| it.top + it.height).fold(0.0, f32::max),
            left: main.iter().map(|it| it.left).fold(f32::MAX, f32::min),
            right: g.iter().map(|it| it.left + it.width).fold(0.0, f32::max),
            size,
            key,
            bold: total > 0 && bold * 2 > total,
            spans,
        });
    }
    out
}
