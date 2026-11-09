// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! `pdf2epub BOOK.pdf [-o BOOK.epub] [--title T] [--author A] [--lang de] [--no-cover] [--font-greek F] [--font-hebrew F] [--no-font-search]`

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "pdf2epub BOOK.pdf [-o BOOK.epub] [--title TITLE] [--author AUTHOR] [--lang de] [--no-cover]
         [--font-greek FONT.ttf] [--font-hebrew FONT.ttf] [--no-font-search]

Turns a typeset PDF book into an EPUB 3 with its table of contents, footnotes, printed page numbers,
bibliography, abbreviations and index. Needs poppler-utils (pdftohtml, pdfinfo, pdftoppm).
Greek and Hebrew get embedded free fonts (found with fontconfig, or given).";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut opts = pdf2epub::Options::default();
    while let Some(a) = args.next() {
        let mut value = |name: &str| match args.next() {
            Some(v) => Ok(v),
            None => Err(format!("{name} needs a value")),
        };
        let r = match a.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-o" | "--output" => value(&a).map(|v| output = Some(v.into())),
            "--title" => value(&a).map(|v| opts.title = Some(v)),
            "--author" => value(&a).map(|v| opts.author = Some(v)),
            "--lang" => value(&a).map(|v| opts.language = v),
            "--font-greek" => value(&a).map(|v| opts.font_greek = Some(v.into())),
            "--font-hebrew" => value(&a).map(|v| opts.font_hebrew = Some(v.into())),
            "--no-cover" => {
                opts.cover = false;
                Ok(())
            }
            "--no-font-search" => {
                opts.find_fonts = false;
                Ok(())
            }
            s if s.starts_with('-') => Err(format!("unknown option {s}")),
            s => {
                input = Some(s.into());
                Ok(())
            }
        };
        if let Err(e) = r {
            eprintln!("{e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    }
    let Some(input) = input else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let output = output.unwrap_or_else(|| input.with_extension("epub"));
    let (bytes, r) = match pdf2epub::convert_pdf(&input, &opts) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("{}: {e}", input.display());
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = std::fs::write(&output, &bytes) {
        eprintln!("{}: {e}", output.display());
        return ExitCode::FAILURE;
    }
    println!("{} ({} KB)", output.display(), bytes.len() / 1024);
    println!("  pages {}, files {}, headings {} ({} in the navigation)", r.pages, r.files, r.headings, r.nav_entries);
    println!("  contents: {} entries, {} lead to their heading, the others to their page", r.toc_entries, r.toc_to_headings);
    println!("  printed pages marked: {}", r.page_marks);
    println!("  footnotes {}, marks {}, linked {}, notes without mark {}", r.notes, r.refs, r.refs_linked, r.notes_without_ref);
    println!("  bibliography {}, abbreviations {}, index lines {}", r.bibliography, r.glossary, r.index_entries);
    println!("  Greek {}, Hebrew {}, fonts embedded: {}", r.greek, r.hebrew, if r.fonts.is_empty() { "none".into() } else { r.fonts.join(", ") });
    if r.problems.is_empty() {
        println!("  check: links, anchors and files sound");
    } else {
        println!("  check: {} problems", r.problems.len());
        for p in r.problems.iter().take(20) {
            println!("    {p}");
        }
    }
    if (r.greek || r.hebrew) && r.fonts.is_empty() {
        println!("  warning: Greek or Hebrew without an embedded font (install fonts-sil-gentiumplus / fonts-sil-ezra or give --font-greek/--font-hebrew)");
    }
    if r.problems.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
