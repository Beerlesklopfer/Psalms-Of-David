// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The e-books (`/api/v1/ebooks`, docs/spec-ebooks.md): the folders are read with the import, an EPUB becomes a text for the reader, a
//! protected one stays out, and who sees which e-book is decided as for the audiobooks.

mod common;

use std::io::Write;
use std::path::Path;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use common::open;
use david_server::api::{ApiState, router};
use david_server::ebooks::{Epub, EpubError, chapter_blocks, join};
use david_server::{NewProfile, RatingRule, Role, Store};
use serde_json::{Value, json};
use tower::ServiceExt;

fn key(n: u8) -> String {
    let letters = b"AEIMQUYcgkosw048";
    format!("{}{}=", "B".repeat(42), letters[n as usize % letters.len()] as char)
}

/// A small EPUB 3: two chapters, a navigation document, a cover and a picture in the text. [protected] adds an encrypted chapter.
fn epub(title: &str, protected: bool) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflated = zip::write::SimpleFileOptions::default();
        z.start_file("mimetype", stored).unwrap();
        z.write_all(b"application/epub+zip").unwrap();
        z.start_file("META-INF/container.xml", deflated).unwrap();
        z.write_all(br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        if protected {
            z.start_file("META-INF/encryption.xml", deflated).unwrap();
            z.write_all(br#"<encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container" xmlns:enc="http://www.w3.org/2001/04/xmlenc#"><enc:EncryptedData><enc:EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#aes128-cbc"/><enc:CipherData><enc:CipherReference URI="OEBPS/text/c1.xhtml"/></enc:CipherData></enc:EncryptedData></encryption>"#).unwrap();
        }
        z.start_file("OEBPS/content.opf", deflated).unwrap();
        write!(
            z,
            r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{title}</dc:title><dc:creator>Johannes Calvin</dc:creator><dc:language>de</dc:language></metadata>
<manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="c1" href="text/c1.xhtml" media-type="application/xhtml+xml"/><item id="c2" href="text/c2%20b.xhtml" media-type="application/xhtml+xml"/><item id="cov" href="img/cover.jpg" media-type="image/jpeg" properties="cover-image"/><item id="map" href="img/map.png" media-type="image/png"/></manifest>
<spine><itemref idref="c1"/><itemref idref="c2"/></spine></package>"#
        )
        .unwrap();
        z.start_file("OEBPS/nav.xhtml", deflated).unwrap();
        z.write_all(r#"<!DOCTYPE html><html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="text/c1.xhtml">Vorwort</a></li><li><a href="text/c2%20b.xhtml#x">Vom Glauben</a><ol><li><a href="text/c2%20b.xhtml#y">Röm 8</a></li></ol></li></ol></nav></body></html>"#.as_bytes()).unwrap();
        z.start_file("OEBPS/text/c1.xhtml", deflated).unwrap();
        z.write_all(r#"<?xml version="1.0" encoding="utf-8"?><!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd"><html xmlns="http://www.w3.org/1999/xhtml"><head><title>x</title><style>p{}</style></head><body><h1>Vorwort</h1><p>Der&nbsp;<em>Glaube</em> ist <strong>Ge&shy;wiss&shy;heit</strong>,
   so Hebr 11<sup>1</sup>.</p><div>Lose im Kasten</div></body></html>"#.as_bytes()).unwrap();
        z.start_file("OEBPS/text/c2 b.xhtml", deflated).unwrap();
        z.write_all(r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h2 id="x">Vom Glauben</h2><blockquote><p>Erste Zeile</p><p>Zweite Zeile</p></blockquote><ul><li>Eins</li><li>Zwei</li></ul><p><img src="../img/map.png" alt="Karte"/></p><hr/><pre>a  b
c</pre></body></html>"#.as_bytes()).unwrap();
        z.start_file("OEBPS/img/cover.jpg", stored).unwrap();
        z.write_all(&[0xff, 0xd8, 0xff, 1]).unwrap();
        z.start_file("OEBPS/img/map.png", stored).unwrap();
        z.write_all(&[0x89, b'P', b'N', b'G']).unwrap();
        z.finish().unwrap();
    }
    buf.into_inner()
}

#[test]
fn an_epub_becomes_chapters_of_blocks_with_their_emphasis_and_its_table_of_contents() {
    let mut e = Epub::open(epub("Institutio", false)).unwrap();
    assert_eq!((e.package.title.as_str(), e.package.author.as_str(), e.package.language.as_deref()), ("Institutio", "Johannes Calvin", Some("de")));
    assert_eq!(e.package.cover.as_deref(), Some("OEBPS/img/cover.jpg"));
    assert_eq!(e.package.spine, ["OEBPS/text/c1.xhtml", "OEBPS/text/c2 b.xhtml"]);
    let t = e.text();
    assert_eq!(t.chapters.len(), 2);
    let c1 = &t.chapters[0];
    assert_eq!(c1.title, "Vorwort");
    let kinds: Vec<&str> = c1.blocks.iter().map(|b| b.kind.as_str()).collect();
    assert_eq!(kinds, ["h1", "p", "p"]);
    let p = &c1.blocks[1];
    let text: String = p.spans.iter().map(|s| s.t.as_str()).collect();
    assert_eq!(text, "Der\u{a0}Glaube ist Gewissheit, so Hebr 111.", "spaces as in HTML, the soft hyphen gone");
    assert!(p.spans.iter().any(|s| s.i && s.t == "Glaube"));
    assert!(p.spans.iter().any(|s| s.b && s.t == "Gewissheit"));
    assert!(p.spans.iter().any(|s| s.sup && s.t == "1"));
    assert_eq!(c1.blocks[2].spans[0].t, "Lose im Kasten");
    let c2 = &t.chapters[1];
    let kinds: Vec<&str> = c2.blocks.iter().map(|b| b.kind.as_str()).collect();
    assert_eq!(kinds, ["h2", "quote", "quote", "li", "li", "img", "hr", "pre"]);
    assert_eq!(c2.blocks[5].src.as_deref(), Some("OEBPS/img/map.png"));
    assert_eq!(c2.blocks[5].spans[0].t, "Karte");
    assert_eq!(c2.blocks[7].spans[0].t, "a  b\nc", "preformatted text keeps its spaces");
    let toc: Vec<(&str, usize, u8)> = t.toc.iter().map(|e| (e.title.as_str(), e.chapter, e.level)).collect();
    assert_eq!(toc, [("Vorwort", 0, 0), ("Vom Glauben", 1, 0), ("Röm 8", 1, 1)]);
    assert_eq!(e.entry("OEBPS/img/map.png").map(|(b, m)| (b.len(), m)), Some((4, "image/png".to_owned())));
    assert!(e.entry("OEBPS/secret.txt").is_none(), "only what the package names");
}

#[test]
fn a_protected_epub_is_refused_and_names_inside_do_not_lead_out() {
    assert_eq!(Epub::open(epub("x", true)).err(), Some(EpubError::Protected));
    assert!(matches!(Epub::open(b"no zip".to_vec()), Err(EpubError::Broken(_))));
    assert_eq!(join("OEBPS/text/c1.xhtml", "../img/a%20b.jpg#x").as_deref(), Some("OEBPS/img/a b.jpg"));
    assert_eq!(join("a.xhtml", "../../etc/passwd"), None);
    assert!(chapter_blocks("<html><body><p>broken", "a").is_empty(), "a chapter that is no XML gives nothing, not an error");
}

struct Api {
    router: Router,
    store: Store,
    admin: i64,
    _dir: tempfile::TempDir,
}

async fn api() -> Api {
    let (dir, store) = open().await;
    let admin = store.create_profile("Boss", true).await.unwrap();
    let state = ApiState { store: store.clone(), library: None, cd: david_server::cdimport::CdService::new(dir.path()), features: Default::default() };
    Api { router: router(state), store, admin, _dir: dir }
}

async fn call(a: &Api, req: Request<Body>) -> (StatusCode, String, Vec<u8>) {
    let res = a.router.clone().oneshot(req).await.unwrap();
    let (parts, body) = res.into_parts();
    let mime = parts.headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
    (parts.status, mime, to_bytes(body, 32 * 1024 * 1024).await.unwrap().to_vec())
}

async fn get(a: &Api, token: &str, uri: &str) -> (StatusCode, String, Vec<u8>) {
    call(a, Request::builder().uri(uri).header(header::AUTHORIZATION, format!("Bearer {token}")).body(Body::empty()).unwrap()).await
}

async fn put(a: &Api, token: &str, uri: &str, body: Value) -> StatusCode {
    let req = Request::builder().method("PUT").uri(uri).header(header::AUTHORIZATION, format!("Bearer {token}")).header(header::CONTENT_TYPE, "application/json");
    call(a, req.body(Body::from(body.to_string())).unwrap()).await.0
}

fn json_of(b: &[u8]) -> Value {
    serde_json::from_slice(b).unwrap_or(Value::Null)
}

async fn pair(a: &Api, user: i64, n: u8) -> String {
    let e = a.store.create_enrollment(user, false, a.admin).await.unwrap();
    let body = json!({ "code": e.code, "device_name": "Phone", "public_key": key(n) }).to_string();
    let (status, _, bytes) = call(a, Request::builder().method("POST").uri("/api/v1/pair").header(header::CONTENT_TYPE, "application/json").body(Body::from(body)).unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&bytes));
    json_of(&bytes)["token"].as_str().unwrap().to_owned()
}

fn titles(v: &Value) -> Vec<String> {
    v["ebooks"].as_array().unwrap().iter().map(|e| e["title"].as_str().unwrap().to_owned()).collect()
}

fn s(p: &Path) -> String {
    p.to_str().unwrap().to_owned()
}

#[tokio::test]
async fn the_folders_are_read_with_the_import_and_each_person_sees_the_released_ones_within_the_age_limit() {
    let a = api().await;
    let lib = tempfile::tempdir().unwrap();
    let books = lib.path().join("Bücher");
    std::fs::create_dir_all(books.join("Kommentare")).unwrap();
    std::fs::create_dir_all(books.join("Alt")).unwrap();
    std::fs::create_dir_all(lib.path().join("Hörbücher")).unwrap();
    std::fs::write(books.join("Kommentare/institutio.epub"), epub("Institutio", false)).unwrap();
    std::fs::write(books.join("Kommentare/geschuetzt.epub"), epub("Geschützt", true)).unwrap();
    std::fs::write(books.join("Spurgeon - Kleinode.pdf"), b"%PDF-1.4 not a real one").unwrap();
    std::fs::write(books.join("Alt/weg.epub"), epub("Weg", false)).unwrap();
    std::fs::write(books.join("notes.txt"), b"no book").unwrap();

    a.store.save_import_criteria(&[s(&lib.path().join("Hörbücher"))], RatingRule::Manual).await.unwrap();
    a.store.save_media_roots("ebook", &[s(&books)], &[s(&books.join("Alt"))]).await.unwrap();
    a.store.unlock_import("Boss").await.unwrap();
    let report = a.store.run_import().await.unwrap();
    assert_eq!((report.ebooks, report.ebooks_new), (3, 2), "the protected one is found but stays out; the folder left out is not read");

    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let mut always = david_server::Schedule::none();
    always.allow_every_day(0, 24 * 60);
    let kid = a.store.create_user(NewProfile { name: "Kid".into(), role: Role::Child, max_age: Some(10), schedule: Some(always) }).await.unwrap();
    let (boss_t, anna_t, kid_t) = (pair(&a, a.admin, 1).await, pair(&a, anna, 2).await, pair(&a, kid, 3).await);

    // An administrator sees what came in; the others only what is released.
    let (status, _, body) = get(&a, &boss_t, "/api/v1/ebooks").await;
    assert_eq!(status, StatusCode::OK);
    let all = json_of(&body);
    assert_eq!(titles(&all), ["Institutio", "Kleinode"], "by author: Calvin, Spurgeon; the PDF is named after its file");
    assert_eq!(all["ebooks"][1]["author"].as_str(), Some("Spurgeon"));
    assert_eq!(all["ebooks"][0]["format"].as_str(), Some("epub"));
    assert_eq!(all["ebooks"][0]["progress"], Value::Null);
    let (epub_id, pdf_id) = (all["ebooks"][0]["id"].as_i64().unwrap(), all["ebooks"][1]["id"].as_i64().unwrap());
    assert_eq!(titles(&json_of(&get(&a, &anna_t, "/api/v1/ebooks").await.2)), Vec::<String>::new());
    assert_eq!(get(&a, &anna_t, &format!("/api/v1/ebooks/{epub_id}/text")).await.0, StatusCode::NOT_FOUND);

    assert!(a.store.release_ebook(epub_id, true, 0).await.unwrap());
    assert!(a.store.release_ebook(pdf_id, true, 16).await.unwrap());
    assert_eq!(titles(&json_of(&get(&a, &anna_t, "/api/v1/ebooks").await.2)), ["Institutio", "Kleinode"]);
    assert_eq!(titles(&json_of(&get(&a, &kid_t, "/api/v1/ebooks").await.2)), ["Institutio"], "the PDF is for 16 and up");
    assert_eq!(get(&a, &kid_t, &format!("/api/v1/ebooks/{pdf_id}/file")).await.0, StatusCode::NOT_FOUND);

    // The text, a picture of it, the cover, the file.
    let (status, mime, body) = get(&a, &kid_t, &format!("/api/v1/ebooks/{epub_id}/text")).await;
    assert_eq!((status, mime.as_str()), (StatusCode::OK, "application/json"));
    let text = json_of(&body);
    assert_eq!(text["chapters"].as_array().unwrap().len(), 2);
    assert_eq!(text["chapters"][1]["blocks"][5]["src"].as_str(), Some("OEBPS/img/map.png"));
    assert_eq!(get(&a, &kid_t, &format!("/api/v1/ebooks/{epub_id}/text")).await.2, body, "the second time from the cache");
    let (status, mime, _) = get(&a, &kid_t, &format!("/api/v1/ebooks/{epub_id}/res?name=OEBPS/img/map.png")).await;
    assert_eq!((status, mime.as_str()), (StatusCode::OK, "image/png"));
    assert_eq!(get(&a, &kid_t, &format!("/api/v1/ebooks/{epub_id}/res?name=OEBPS/text/c1.xhtml")).await.0, StatusCode::NOT_FOUND, "only pictures");
    let (status, mime, _) = get(&a, &kid_t, &format!("/api/v1/ebooks/{epub_id}/cover")).await;
    assert_eq!((status, mime.as_str()), (StatusCode::OK, "image/jpeg"));
    let (status, mime, body) = get(&a, &anna_t, &format!("/api/v1/ebooks/{pdf_id}/file")).await;
    assert_eq!((status, mime.as_str(), body.starts_with(b"%PDF")), (StatusCode::OK, "application/pdf", true));
    assert_eq!(get(&a, &anna_t, &format!("/api/v1/ebooks/{pdf_id}/text")).await.0, StatusCode::NOT_FOUND, "a PDF has no text");

    // Where a person is: the newer report wins, and each person has their own.
    let at = format!("/api/v1/ebooks/{epub_id}/progress");
    assert_eq!(get(&a, &kid_t, &at).await.0, StatusCode::NOT_FOUND);
    assert_eq!(put(&a, &kid_t, &at, json!({ "locator": "epub:1:3", "percent": 0.6, "read_at": 2000 })).await, StatusCode::NO_CONTENT);
    assert_eq!(put(&a, &kid_t, &at, json!({ "locator": "epub:0:1", "percent": 0.1, "read_at": 1000 })).await, StatusCode::NO_CONTENT);
    assert_eq!(json_of(&get(&a, &kid_t, &at).await.2), json!({ "locator": "epub:1:3", "percent": 0.6, "read_at": 2000 }));
    assert_eq!(get(&a, &anna_t, &at).await.0, StatusCode::NOT_FOUND, "Anna's own place is not Kid's");
    assert_eq!(put(&a, &kid_t, &at, json!({ "locator": "", "percent": 0.1, "read_at": 3000 })).await, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(put(&a, &kid_t, &format!("/api/v1/ebooks/{pdf_id}/progress"), json!({ "locator": "pdf:1", "percent": 0.1, "read_at": 1 })).await, StatusCode::NOT_FOUND);
    assert_eq!(json_of(&get(&a, &kid_t, "/api/v1/ebooks").await.2)["ebooks"][0]["progress"]["locator"].as_str(), Some("epub:1:3"));

    // A file that is gone is no longer listed; it comes back with its place when the file does.
    let file = books.join("Kommentare/institutio.epub");
    let kept = std::fs::read(&file).unwrap();
    std::fs::remove_file(&file).unwrap();
    a.store.run_import().await.unwrap();
    assert_eq!(titles(&json_of(&get(&a, &kid_t, "/api/v1/ebooks").await.2)), Vec::<String>::new());
    std::fs::write(&file, kept).unwrap();
    a.store.run_import().await.unwrap();
    let back = json_of(&get(&a, &kid_t, "/api/v1/ebooks").await.2);
    assert_eq!((titles(&back), back["ebooks"][0]["progress"]["percent"].as_f64()), (vec!["Institutio".to_owned()], Some(0.6)));
}
