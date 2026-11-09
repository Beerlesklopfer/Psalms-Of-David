// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! A small invented book, as `pdftohtml -xml` would give it, turned into an EPUB: every part of the apparatus is checked in the
//! written files. (No text of a real book is used here.)

use std::io::Read;

use pdf2epub::{Options, convert_xml};

trait Nfc {
    fn nfc_string(&self) -> String;
}

impl Nfc for str {
    fn nfc_string(&self) -> String {
        pdf2epub::script::normalize(self)
    }
}

const W: f32 = 574.0;

struct Doc {
    pages: Vec<String>,
}

impl Doc {
    fn page(&mut self, texts: &[(f32, f32, f32, u32, &str)]) {
        let mut p = String::new();
        if self.pages.is_empty() {
            for (id, size, family) in [
                (0, 15, "AAAAAA+TimesNewRomanPSMT"),
                (1, 10, "AAAAAA+TimesNewRomanPSMT"),
                (2, 24, "BBBBBB+MyriadPro-Semibold"),
                (3, 21, "BBBBBB+MyriadPro-Semibold"),
                (4, 20, "BBBBBB+MyriadPro-SemiboldIt"),
                (5, 14, "CCCCCC+MyriadPro-Regular"),
                (6, 15, "DDDDDD+Bwgrkl"),
                (7, 9, "AAAAAA+TimesNewRomanPSMT"),
                (8, 15, "EEEEEE+TimesNewRomanPS-BoldMT"),
            ] {
                p.push_str(&format!("<fontspec id=\"{id}\" size=\"{size}\" family=\"{family}\" color=\"#000000\"/>\n"));
            }
        }
        for (top, left, right, font, text) in texts {
            let height = if *font == 1 || *font == 7 { 12 } else { 20 };
            p.push_str(&format!(
                "<text top=\"{top}\" left=\"{left}\" width=\"{}\" height=\"{height}\" font=\"{font}\">{text}</text>\n",
                right - left
            ));
        }
        self.pages.push(p);
    }

    fn xml(&self) -> String {
        let mut x = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE pdf2xml SYSTEM \"pdf2xml.dtd\">\n<pdf2xml producer=\"poppler\" version=\"25.03.0\">\n");
        for (i, p) in self.pages.iter().enumerate() {
            x.push_str(&format!("<page number=\"{}\" position=\"absolute\" top=\"0\" left=\"0\" height=\"871\" width=\"{W}\">\n{p}</page>\n", i + 1));
        }
        x.push_str("</pdf2xml>\n");
        x
    }
}

fn book() -> String {
    let mut d = Doc { pages: Vec::new() };
    // 1: the cover, a picture only.
    d.page(&[]);
    // 2: the title page.
    d.page(&[(200.0, 200.0, 380.0, 2, "Ein Testbuch"), (260.0, 230.0, 350.0, 0, "Anna Autorin")]);
    // 3: the copyright page.
    d.page(&[(600.0, 200.0, 380.0, 0, "© 2020 by Testverlag"), (640.0, 190.0, 390.0, 0, "ISBN 978-3-12345-678-9")]);
    // 4: the table of contents.
    d.page(&[
        (80.0, 250.0, 330.0, 2, "Inhalt"),
        (130.0, 77.0, 510.0, 0, "Kapitel 1                                5"),
        (150.0, 77.0, 510.0, 0, "1. Erster Abschnitt mit einem Titel,"),
        (170.0, 88.0, 510.0, 0, "der über zwei Zeilen geht                5"),
        (200.0, 68.0, 510.0, 0, "Teil 1: Der Hauptteil                    6"),
        (220.0, 77.0, 510.0, 0, "Kapitel 2                                6"),
        (240.0, 77.0, 510.0, 0, "Bibliografie                             7"),
        (260.0, 77.0, 510.0, 0, "Abkürzungen                              8"),
        (280.0, 77.0, 510.0, 0, "Register                                 8"),
    ]);
    // 5: chapter 1.
    d.page(&[
        (40.0, 68.0, 80.0, 5, "5 "),
        (40.0, 440.0, 506.0, 5, "Ein Testbuch"),
        (80.0, 240.0, 340.0, 2, "Kapitel 1"),
        (130.0, 150.0, 430.0, 4, "1. Erster Abschnitt mit einem Titel,"),
        (155.0, 180.0, 400.0, 4, "der über zwei Zeilen geht"),
        (200.0, 91.0, 510.0, 0, "Der Apostel schreibt als ein bekannter Pha\u{ad}"),
        (220.0, 68.0, 300.0, 0, "risäer, was »Abgesonderter«"),
        (221.0, 300.0, 305.0, 7, "1"),
        (220.0, 305.0, 400.0, 0, " bedeutet, "),
        (224.0, 400.0, 480.0, 6, "afwrismenoj"),
        (220.0, 480.0, 510.0, 0, ", und"),
        (240.0, 68.0, 360.0, 0, "das Ende des Absatzes."),
        (260.0, 91.0, 510.0, 0, "Siehe S. 6 und das Wort שָׁלוֹם, das Friede heißt, wie es"),
        (280.0, 68.0, 300.0, 0, "oft gesagt wird."),
        (300.0, 91.0, 510.0, 0, "1.  Der erste Punkt der Liste, der etwas länger ist und"),
        (320.0, 115.0, 300.0, 0, "auf der Zeile darunter weitergeht."),
        (340.0, 91.0, 250.0, 0, "2.  Der zweite Punkt."),
        (370.0, 91.0, 300.0, 8, "<b>1 Paulus, Knecht Jesu Christi,</b>"),
        (390.0, 91.0, 300.0, 8, "<b>2 das er zuvor verheißen hat,</b>"),
        (430.0, 91.0, 510.0, 0, "Ein langer Absatz beginnt hier und füllt die Zeile ganz"),
        (450.0, 68.0, 510.0, 0, "aus, er geht weiter auf der folgenden Zeile und dann über"),
        (470.0, 68.0, 510.0, 0, "das Ende der Seite hinaus, wie es in Büchern oft vorkommt,"),
        (760.0, 68.0, 510.0, 1, "1  Vom hebräischen Verb, das abtrennen heißt; diese Anmerkung"),
        (771.0, 87.0, 400.0, 1, "geht auf der nächsten"),
    ]);
    // 6: the paragraph goes on; part 1 and chapter 2.
    d.page(&[
        (40.0, 68.0, 160.0, 5, "Ein Testbuch 6"),
        (80.0, 68.0, 300.0, 0, "bis zu ihrem Ende."),
        (130.0, 150.0, 430.0, 3, "Teil 1: Der Hauptteil"),
        (200.0, 240.0, 340.0, 2, "Kapitel 2"),
        (250.0, 68.0, 300.0, 0, "Text im zweiten Kapitel"),
        (251.0, 300.0, 305.0, 7, "2"),
        (250.0, 305.0, 320.0, 0, "."),
        (760.0, 87.0, 200.0, 1, "Seite weiter."),
        (771.0, 68.0, 300.0, 1, "2  Die zweite Anmerkung."),
    ]);
    // 7: the bibliography.
    d.page(&[
        (40.0, 68.0, 80.0, 5, "7 "),
        (40.0, 440.0, 506.0, 5, "Bibliografie"),
        (80.0, 220.0, 360.0, 2, "Bibliografie"),
        (130.0, 68.0, 200.0, 0, "Calvin, Johannes, "),
        (130.0, 200.0, 300.0, 0, "<i>Institutio</i>"),
        (130.0, 300.0, 400.0, 0, ", Genf 1559."),
        (160.0, 68.0, 400.0, 0, "Luther, Martin, Vorrede, Wittenberg 1522."),
    ]);
    // 8: the abbreviations and the index.
    d.page(&[
        (40.0, 68.0, 160.0, 5, "Abkürzungen 8"),
        (80.0, 220.0, 360.0, 2, "Abkürzungen"),
        (130.0, 68.0, 100.0, 0, "LXX "),
        (130.0, 174.0, 450.0, 0, "Septuaginta (griechische"),
        (150.0, 174.0, 300.0, 0, "Übersetzung)"),
        (190.0, 68.0, 110.0, 0, "UELB "),
        (190.0, 174.0, 400.0, 0, "Elberfelder Bibel"),
        (300.0, 220.0, 360.0, 2, "Register"),
        (350.0, 68.0, 200.0, 0, "Abraham 5, 6"),
        (370.0, 68.0, 200.0, 0, "Paulus 5"),
    ]);
    d.xml()
}

fn files(epub: &[u8]) -> std::collections::HashMap<String, String> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(epub)).unwrap();
    let mut out = std::collections::HashMap::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let mut s = Vec::new();
        f.read_to_end(&mut s).unwrap();
        out.insert(f.name().to_string(), String::from_utf8_lossy(&s).into_owned());
    }
    out
}

fn all_text(f: &std::collections::HashMap<String, String>) -> String {
    let mut names: Vec<&String> = f.keys().filter(|n| n.starts_with("OEBPS/text/s")).collect();
    names.sort();
    names.iter().map(|n| f[*n].as_str()).collect()
}

#[test]
fn the_book_comes_out_with_its_whole_apparatus() {
    let dir = std::env::temp_dir().join(format!("pdf2epub-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let greek = dir.join("greek.ttf");
    let hebrew = dir.join("hebrew.otf");
    std::fs::write(&greek, b"not really a font").unwrap();
    std::fs::write(&hebrew, b"not really a font either").unwrap();
    let opts = Options { find_fonts: false, fonts: vec![("Grek".into(), greek), ("Hebr".into(), hebrew)], ..Options::default() };
    let (epub, r) = convert_xml(&book(), Some(b"JPEG"), "Ein Testbuch", "Anna Autorin", "test.pdf", &opts).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    let f = files(&epub);
    let text = all_text(&f);

    // The package: the ISBN of the print as identifier and as source of the page breaks, the cover, the fonts.
    let opf = &f["OEBPS/content.opf"];
    assert!(opf.contains("<dc:identifier id=\"bookid\">urn:isbn:9783123456789</dc:identifier>"), "{opf}");
    assert!(opf.contains("source-of\">pagination"));
    assert!(opf.contains("<dc:publisher>Testverlag</dc:publisher>") && opf.contains("<dc:date>2020</dc:date>"));
    assert!(opf.contains("properties=\"cover-image\""));
    assert!(f.contains_key("OEBPS/fonts/embedded-grek.ttf") && f.contains_key("OEBPS/fonts/embedded-hebr.otf"));
    let css = &f["OEBPS/style.css"];
    assert!(css.contains("@font-face { font-family: \"Embedded-Hebr\"; src: url(\"fonts/embedded-hebr.otf\"); }"));
    assert!(css.contains(".sc-Hebr { font-family: \"Embedded-Hebr\", \"Ezra SIL\", \"SBL Hebrew\", serif; font-size: 1.1em; }"), "{css}");
    let codes: Vec<&str> = r.scripts.iter().map(|u| u.code.as_str()).collect();
    assert!(codes.contains(&"Grek") && codes.contains(&"Hebr"), "{codes:?}");
    assert_eq!((r.main_script.as_str(), r.rtl, r.language.as_str()), ("Latn", false, "de"));
    // The fonts given here are no real fonts: their characters cannot be checked, which is said.
    assert!(r.warnings.iter().any(|w| w.contains("could not be checked")), "{:?}", r.warnings);

    // The navigation: part above chapter, the two-line section heading whole, the page list, the landmarks.
    let nav = &f["OEBPS/text/nav.xhtml"];
    let toc = nav.split("page-list").next().unwrap();
    let part = toc.find("Teil 1: Der Hauptteil").unwrap();
    let ch2 = toc.find("Kapitel 2").unwrap();
    assert!(part < ch2 && toc[part..ch2].contains("<ol>"), "chapter 2 lies inside part 1: {toc}");
    assert!(toc.contains("1. Erster Abschnitt mit einem Titel, der über zwei Zeilen geht"));
    for p in 5..=8 {
        assert!(nav.contains(&format!("#p{p}\">{p}</a>")), "page {p} in the page list: {nav}");
    }
    assert!(nav.contains("epub:type=\"bibliography\"") && nav.contains("epub:type=\"glossary\"") && nav.contains("epub:type=\"index\""));
    assert!(f["OEBPS/toc.ncx"].contains("<pageTarget id=\"pt6\""));

    // The printed table of contents: every entry leads to its heading, the entry of two lines is one.
    let contents = &f["OEBPS/text/contents.xhtml"];
    assert_eq!(r.toc_entries, 7);
    assert_eq!(r.toc_to_headings, 7, "{contents}");
    assert!(contents.contains("1. Erster Abschnitt mit einem Titel, der über zwei Zeilen geht</a><span class=\"pg\">5</span>"));
    assert!(contents.contains("<li class=\"d0\"><a href=\"s002.xhtml#h"), "the part sits on the outer level: {contents}");

    // The running heads are gone, the hyphenation is taken back, the Greek is Unicode and marked, so is the Hebrew.
    assert!(!text.contains("Ein Testbuch 6") && !text.contains(">5 <"));
    assert!(text.contains("ein bekannter Pharisäer, was »Abgesonderter«"), "{text}");
    assert!(text.contains("<span class=\"sc-Grek\" lang=\"grc\" xml:lang=\"grc\">αφωρισμενος</span>"), "{text}");
    assert!(text.contains("das Wort <span class=\"sc-Hebr\" lang=\"he\" xml:lang=\"he\" dir=\"rtl\">שָׁלוֹם</span>, das Friede"), "{text}");
    // "S. 6" leads to the printed page 6.
    assert!(text.contains("<a href=\"s002.xhtml#p6\">S. 6</a>") || text.contains("<a href=\"#p6\">S. 6</a>"), "{text}");

    // The footnotes: both marks linked, the note that runs over to the next page is one, with a link back.
    assert_eq!((r.notes, r.refs, r.refs_linked, r.notes_without_ref), (2, 2, 2, 0));
    assert!(text.contains("<a epub:type=\"noteref\" role=\"doc-noteref\" id=\"r0\" href=\"#n0\"><sup>1</sup></a>"), "{text}");
    assert!(text.contains("diese Anmerkung geht auf der nächsten Seite weiter."), "{text}");
    assert!(text.contains("<aside epub:type=\"footnote\" role=\"doc-footnote\" id=\"n0\" class=\"footnote\"><p><a href=\"#r0\" role=\"doc-backlink\">1</a>"));

    // The paragraph over the page break is one, with the page mark inside.
    assert!(text.contains("wie es in Büchern oft vorkommt, <span epub:type=\"pagebreak\" role=\"doc-pagebreak\" id=\"p6\""), "{text}");

    // The list with its hanging indent, the Bible text as an indented block with a line per verse.
    assert!(text.contains("<p class=\"item\">1. Der erste Punkt der Liste, der etwas länger ist und auf der Zeile darunter weitergeht.</p>"), "{text}");
    assert!(text.contains("<blockquote class=\"indented\">\n<p><b>1 Paulus, Knecht Jesu Christi,</b></p>\n<p><b>2 das er zuvor verheißen hat,</b></p>"), "{text}");

    // Bibliography, abbreviations, index with its page numbers linked.
    assert_eq!((r.bibliography, r.glossary, r.index_entries), (2, 2, 2));
    assert!(text.contains("<li>Calvin, Johannes, <i>Institutio</i>, Genf 1559.</li>"), "{text}");
    assert!(text.contains("<dt>LXX</dt>\n<dd>Septuaginta (griechische Übersetzung)</dd>"), "{text}");
    assert!(text.contains("Abraham <a href=\"s001.xhtml#p5\">5</a>, <a href=\"s001.xhtml#p6\">6</a>"), "{text}");

    // The front matter keeps the copyright page.
    assert!(f["OEBPS/text/front.xhtml"].contains("epub:type=\"copyright-page\""));
}

#[test]
fn a_real_pdf_goes_through_poppler_when_it_is_installed() {
    if std::process::Command::new("pdftohtml").arg("-v").output().is_err() {
        eprintln!("pdftohtml missing, skipped");
        return;
    }
    // A PDF of two pages written by hand: a heading and a paragraph, a running head with the page number.
    let pages = [
        "BT /F1 9 Tf 50 800 Td (Ein Testbuch 1) Tj ET BT /F2 20 Tf 200 700 Td (Kapitel 1) Tj ET BT /F1 11 Tf 50 650 Td (Der Text des ersten Kapitels steht hier.) Tj ET",
        "BT /F1 9 Tf 50 800 Td (Ein Testbuch 2) Tj ET BT /F1 11 Tf 50 750 Td (Und er geht auf der zweiten Seite weiter.) Tj ET BT /F2 20 Tf 200 650 Td (Kapitel 2) Tj ET BT /F1 11 Tf 50 600 Td (Das zweite Kapitel.) Tj ET",
    ];
    let mut objs: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [5 0 R 7 0 R] /Count 2 >>".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman /Encoding /WinAnsiEncoding >>".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".into(),
    ];
    for (i, c) in pages.iter().enumerate() {
        objs.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {} 0 R >>", 6 + 2 * i));
        objs.push(format!("<< /Length {} >>\nstream\n{c}\nendstream", c.len() + 1));
    }
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        pdf.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    let dir = std::env::temp_dir().join(format!("pdf2epub-real-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Anna Autorin - Kurzes Buch.pdf");
    std::fs::write(&path, &pdf).unwrap();
    let opts = Options { find_fonts: false, cover: false, ..Options::default() };
    let (epub, r) = pdf2epub::convert_pdf(&path, &opts).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    let f = files(&epub);
    assert!(f["OEBPS/content.opf"].contains("<dc:title>Kurzes Buch</dc:title>") && f["OEBPS/content.opf"].contains("Anna Autorin"));
    let text = all_text(&f);
    assert!(text.contains("Der Text des ersten Kapitels steht hier."), "{text}");
    assert!(text.contains("Das zweite Kapitel."));
    assert!(!text.contains("Ein Testbuch 2"), "the running head is gone: {text}");
    assert!(f["OEBPS/text/nav.xhtml"].contains("Kapitel 2"));
}

/// One page of `pdftohtml -xml` with the given fonts and pieces (top, left, width, font, text).
/// A piece of text: top, left, width, font, text.
type Piece = (f32, f32, f32, u32, String);

fn page_xml(fonts: &[(u32, u32, &str)], pages: &[Vec<Piece>]) -> String {
    let mut x = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<pdf2xml producer=\"poppler\" version=\"25.03.0\">\n");
    for (i, items) in pages.iter().enumerate() {
        x.push_str(&format!("<page number=\"{}\" position=\"absolute\" top=\"0\" left=\"0\" height=\"871\" width=\"{W}\">\n", i + 1));
        if i == 0 {
            for (id, size, family) in fonts {
                x.push_str(&format!("<fontspec id=\"{id}\" size=\"{size}\" family=\"{family}\" color=\"#000000\"/>\n"));
            }
        }
        for (top, left, width, font, text) in items {
            x.push_str(&format!("<text top=\"{top}\" left=\"{left}\" width=\"{width}\" height=\"16\" font=\"{font}\">{text}</text>\n"));
        }
        x.push_str("</page>\n");
    }
    x.push_str("</pdf2xml>\n");
    x
}

fn seen(s: &str) -> String {
    s.chars().rev().collect()
}

#[test]
fn every_foreign_script_is_marked_with_its_language_and_direction() {
    // A German page quoting Syriac, Coptic, Arabic, Ge'ez and Old Church Slavonic, drawn as XeTeX draws right-to-left words.
    let pages = vec![vec![
        (100.0, 60.0, 400.0, 0, "Das Wort lautet".into()),
        (120.0, 60.0, 120.0, 0, "syrisch".into()),
        (120.0, 190.0, 60.0, 1, seen("ܡܠܬܐ")),
        (120.0, 250.0, 200.0, 0, ", koptisch ⲡⲗⲟⲅⲟⲥ und".into()),
        (140.0, 60.0, 80.0, 0, "arabisch".into()),
        (140.0, 150.0, 70.0, 1, seen("الكلمة")),
        (140.0, 220.0, 250.0, 0, ", ge'ez ቃል, slawisch слово.".into()),
    ]];
    let fonts = [(0, 15, "AAAAAA+Serif"), (1, 15, "BBBBBB+NotoSansSyriac")];
    let opts = Options { find_fonts: false, script_langs: vec![("Cyrl".into(), "cu".into())], ..Options::default() };
    let (epub, r) = convert_xml(&page_xml(&fonts, &pages), None, "Wörter", "Anna Autorin", "w.pdf", &opts).unwrap();
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    let text = all_text(&files(&epub));
    assert!(text.contains("<span class=\"sc-Syrc\" lang=\"syc\" xml:lang=\"syc\" dir=\"rtl\">ܡܠܬܐ</span>, koptisch"), "{text}");
    assert!(text.contains("<span class=\"sc-Copt\" lang=\"cop\" xml:lang=\"cop\">ⲡⲗⲟⲅⲟⲥ</span>"), "{text}");
    assert!(text.contains("arabisch <span class=\"sc-Arab\" lang=\"ar\" xml:lang=\"ar\" dir=\"rtl\">الكلمة</span>,"), "{text}");
    assert!(text.contains("<span class=\"sc-Ethi\" lang=\"gez\" xml:lang=\"gez\">ቃል</span>"), "{text}");
    assert!(text.contains("<span class=\"sc-Cyrl\" lang=\"cu\" xml:lang=\"cu\">слово</span>."), "{text}");
    // Without a font search the scripts have no font; that is said, as a note.
    let codes: Vec<&str> = r.scripts.iter().map(|u| u.code.as_str()).collect();
    assert_eq!(codes, ["Arab", "Copt", "Cyrl", "Ethi", "Syrc"]);
    assert!(r.warnings.iter().any(|w| w.starts_with("Syriac: 4 characters without an embedded font")), "{:?}", r.warnings);
}

#[test]
fn a_book_written_from_right_to_left_becomes_a_right_to_left_epub() {
    // A Hebrew book: the lines start on the right, a heading, two paragraphs, a Latin word and a verse reference in it.
    let p1 = vec![
        (60.0, 300.0, 120.0, 2, seen("פרק א")),
        (120.0, 40.0, 500.0, 0, seen("בְּרֵאשִׁית בָּרָא אֱלֹהִים אֵת הַשָּׁמַיִם וְאֵת הָאָרֶץ וְהָאָרֶץ הָיְתָה תֹהוּ")),
        (140.0, 100.0, 440.0, 0, seen("וָבֹהוּ וְחֹשֶׁךְ עַל פְּנֵי תְהוֹם.")),
        (160.0, 40.0, 30.0, 0, "Gen".into()),
        (160.0, 80.0, 30.0, 0, "1,2".into()),
        (160.0, 120.0, 380.0, 0, seen("וְרוּחַ אֱלֹהִים מְרַחֶפֶת עַל פְּנֵי הַמָּיִם כמו שכתוב")),
    ];
    let fonts = [(0, 15, "AAAAAA+SBLHebrew"), (2, 24, "AAAAAA+SBLHebrew-Bold")];
    let opts = Options { find_fonts: false, cover: false, ..Options::default() };
    let (epub, r) = convert_xml(&page_xml(&fonts, &[p1]), None, "ספר", "", "s.pdf", &opts).unwrap();
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert_eq!((r.main_script.as_str(), r.rtl, r.language.as_str()), ("Hebr", true, "he"));
    let f = files(&epub);
    assert!(f["OEBPS/content.opf"].contains("<spine toc=\"ncx\" page-progression-direction=\"rtl\">"));
    assert!(f["OEBPS/content.opf"].contains("<dc:language>he</dc:language>"));
    let text = all_text(&f);
    assert!(text.contains("lang=\"he\" xml:lang=\"he\" dir=\"rtl\">"), "{text}");
    let first = "בְּרֵאשִׁית בָּרָא אֱלֹהִים".nfc_string();
    assert!(text.contains(&first), "{text}");
    // The line that runs on is joined to the one before, the Latin is marked as running from left to right.
    assert!(text.contains(&"הָיְתָה תֹהוּ וָבֹהוּ".nfc_string()), "{text}");
    assert!(text.contains(&format!("{} <span class=\"sc-Latn\" lang=\"und-Latn\" xml:lang=\"und-Latn\" dir=\"ltr\">Gen</span> 1,2", "כמו שכתוב")), "{text}");
    // Indents and lists start on the right.
    let css = &f["OEBPS/style.css"];
    assert!(css.contains("div.list p.item { text-indent: -1.5em; padding-right: 1.5em; text-align: right;"), "{css}");
}

#[test]
fn hebrew_greek_and_arabic_set_by_pango_come_out_in_reading_order() {
    // pango draws right-to-left text glyph by glyph from the right, the vowel points as pieces of their own.
    let tools = ["pdftohtml", "pango-view"];
    if tools.iter().any(|t| std::process::Command::new(t).arg("--version").output().is_err()) {
        eprintln!("pdftohtml or pango-view missing, skipped");
        return;
    }
    let dir = std::env::temp_dir().join(format!("pdf2epub-pango-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let text = dir.join("text.txt");
    std::fs::write(
        &text,
        "Kapitel 1\nIm Anfang (בְּרֵאשִׁית בָּרָא) schuf Gott, griechisch ἐν ἀρχῇ, arabisch في البدء mit 12 Wörtern.\n",
    )
    .unwrap();
    let path = dir.join("Anna Autorin - Drei Schriften.pdf");
    let out = std::process::Command::new("pango-view")
        .args(["-q", "--font", "DejaVu Sans 11", "-o"])
        .arg(&path)
        .arg(&text)
        .output()
        .unwrap();
    if !out.status.success() {
        let _ = std::fs::remove_dir_all(&dir);
        eprintln!("pango-view failed (no display backend?), skipped");
        return;
    }
    let opts = Options { find_fonts: false, cover: false, ..Options::default() };
    let (epub, r) = pdf2epub::convert_pdf(&path, &opts).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    let text = all_text(&files(&epub));
    let n = |s: &str| s.nfc_string();
    assert!(text.contains(&format!("Im Anfang (<span class=\"sc-Hebr\" lang=\"he\" xml:lang=\"he\" dir=\"rtl\">{}</span>) schuf Gott", n("בְּרֵאשִׁית בָּרָא"))), "{text}");
    assert!(text.contains(&format!("<span class=\"sc-Grek\" lang=\"grc\" xml:lang=\"grc\">{}</span>", n("ἐν ἀρχῇ"))), "{text}");
    assert!(text.contains("arabisch <span class=\"sc-Arab\" lang=\"ar\" xml:lang=\"ar\" dir=\"rtl\">في البدء</span> mit 12 Wörtern."), "{text}");
}
