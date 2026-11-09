// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! `pdf2epub BOOK.pdf [-o BOOK.epub] [--title T] [--author A] [--lang de] [--no-cover] [--font SCRIPT=F] [--script-lang SCRIPT=L] [--no-font-search]`

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "pdf2epub BOOK.pdf [-o BOOK.epub] [--title TITLE] [--author AUTHOR] [--lang LANG] [--no-cover]
         [--font SCRIPT=FONT.ttf]... [--script-lang SCRIPT=LANG]... [--no-font-search]

Turns a typeset PDF book into an EPUB 3 with its table of contents, footnotes, printed page numbers,
bibliography, abbreviations and index. Needs poppler-utils (pdftohtml, pdfinfo, pdftoppm).

Every script is kept as Unicode, right-to-left text in reading order. Each foreign script (Greek,
Hebrew, Syriac, Arabic, Coptic, Ge'ez, Cyrillic, cuneiform ...) is marked with its language and
direction and gets an embedded free font (found with fontconfig, or given). A book written from
right to left as a whole becomes a right-to-left EPUB.

  --lang LANG              language of the book (default: de, or that of its main script)
  --font SCRIPT=FILE       font for a script: ISO 15924 code or name (Grek, Hebr, Syriac ...),
                           or Text for the rare characters of the main text (transliteration)
  --script-lang SCRIPT=L   language a script is marked with (default: Grek=grc, Hebr=he, Syrc=syc ...)
  --font-greek FILE, --font-hebrew FILE   the same as --font Grek=FILE, --font Hebr=FILE";

/// `SCRIPT=VALUE` with the script as ISO 15924 code (or Text).
fn script_pair(v: &str) -> Result<(String, String), String> {
    let (s, rest) = v.split_once('=').ok_or_else(|| format!("{v}: SCRIPT=VALUE expected"))?;
    let code = if s.eq_ignore_ascii_case(pdf2epub::fonts::TEXT) {
        pdf2epub::fonts::TEXT.to_string()
    } else {
        pdf2epub::script::parse(s).ok_or_else(|| format!("{s}: not a script (ISO 15924 code such as Grek, Hebr, Syrc, or its name)"))?.to_string()
    };
    Ok((code, rest.to_string()))
}

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
            "--lang" => value(&a).map(|v| opts.language = Some(v)),
            "--font-greek" => value(&a).map(|v| opts.fonts.push(("Grek".into(), v.into()))),
            "--font-hebrew" => value(&a).map(|v| opts.fonts.push(("Hebr".into(), v.into()))),
            "--font" => value(&a).and_then(|v| script_pair(&v)).map(|(s, f)| opts.fonts.push((s, f.into()))),
            "--script-lang" => value(&a).and_then(|v| script_pair(&v)).map(|p| opts.script_langs.push(p)),
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
    println!("  language {}, main script {}{}", r.language, pdf2epub::script::name(&r.main_script), if r.rtl { ", right to left" } else { "" });
    for u in &r.scripts {
        let lang = if u.lang.is_empty() { String::new() } else { format!(" [{}{}]", u.lang, if u.rtl { ", rtl" } else { "" }) };
        let font = u.font.as_deref().unwrap_or("no font");
        let missing = if u.missing.is_empty() { String::new() } else { format!(", {} without font", u.missing.len()) };
        println!("  {}{lang}: {} characters, {font}{missing}", u.name, u.chars);
    }
    if r.problems.is_empty() {
        println!("  check: text, fonts, links, anchors and files sound");
    } else {
        println!("  check: {} problems", r.problems.len());
        for p in r.problems.iter().take(20) {
            println!("    {p}");
        }
    }
    for w in &r.warnings {
        println!("  note: {w}");
    }
    if r.problems.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
