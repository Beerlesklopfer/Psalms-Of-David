// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! A check of a written EPUB: the `mimetype` first and uncompressed, every file of the manifest there, every XHTML well formed
//! and UTF-8, every link (with its anchor) leading somewhere, every id once per file.

use std::collections::{HashMap, HashSet};
use std::io::Read;

fn resolve(base: &str, href: &str) -> String {
    let mut parts: Vec<&str> = base.split('/').collect();
    parts.pop();
    for p in href.split('/') {
        match p {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// The problems found; empty when the EPUB is sound.
pub fn check(epub: &[u8]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut z = match zip::ZipArchive::new(std::io::Cursor::new(epub)) {
        Ok(z) => z,
        Err(e) => return vec![format!("not a ZIP: {e}")],
    };
    match z.by_index(0) {
        Ok(f) if f.name() == "mimetype" && f.compression() == zip::CompressionMethod::Stored => {}
        _ => problems.push("mimetype is not the first, uncompressed entry".into()),
    }
    let mut files: HashMap<String, Vec<u8>> = HashMap::new();
    for i in 0..z.len() {
        let Ok(mut f) = z.by_index(i) else { continue };
        let mut b = Vec::new();
        if f.read_to_end(&mut b).is_ok() {
            files.insert(f.name().to_string(), b);
        }
    }
    let Some(opf) = files.get("OEBPS/content.opf").and_then(|b| String::from_utf8(b.clone()).ok()) else {
        problems.push("OEBPS/content.opf missing".into());
        return problems;
    };
    let Ok(doc) = roxmltree::Document::parse(&opf) else {
        problems.push("content.opf is not well formed".into());
        return problems;
    };
    let mut xhtml: Vec<String> = Vec::new();
    for item in doc.descendants().filter(|n| n.has_tag_name("item")) {
        let href = resolve("OEBPS/content.opf", item.attribute("href").unwrap_or_default());
        if !files.contains_key(&href) {
            problems.push(format!("manifest names {href}, which is missing"));
        }
        if item.attribute("media-type") == Some("application/xhtml+xml") {
            xhtml.push(href);
        }
    }
    let mut ids: HashMap<String, HashSet<String>> = HashMap::new();
    let mut hrefs: Vec<(String, String)> = Vec::new();
    for name in &xhtml {
        let Some(bytes) = files.get(name) else { continue };
        let Ok(text) = std::str::from_utf8(bytes) else {
            problems.push(format!("{name} is not UTF-8"));
            continue;
        };
        let opt = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
        let doc = match roxmltree::Document::parse_with_options(text, opt) {
            Ok(d) => d,
            Err(e) => {
                problems.push(format!("{name} is not well formed: {e}"));
                continue;
            }
        };
        let set = ids.entry(name.clone()).or_default();
        for n in doc.descendants().filter(|n| n.is_element()) {
            if let Some(id) = n.attribute("id")
                && !set.insert(id.to_string())
            {
                problems.push(format!("{name}: id {id} twice"));
            }
            for attr in ["href", "src"] {
                if let Some(h) = n.attribute(attr)
                    && !h.contains("://")
                    && !h.starts_with("mailto:")
                {
                    hrefs.push((name.clone(), h.to_string()));
                }
            }
        }
    }
    for (from, h) in hrefs {
        let (file, frag) = h.split_once('#').map_or((h.as_str(), None), |(f, a)| (f, Some(a)));
        let target = if file.is_empty() { from.clone() } else { resolve(&from, file) };
        if !files.contains_key(&target) {
            problems.push(format!("{from}: link to {h}, file missing"));
            continue;
        }
        if let Some(a) = frag
            && !ids.get(&target).is_some_and(|s| s.contains(a))
        {
            problems.push(format!("{from}: link to {h}, anchor missing"));
        }
    }
    problems
}
