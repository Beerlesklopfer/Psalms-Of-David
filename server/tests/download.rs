// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The hidden feature "download" (`[features] download = true`): off, it does not exist (404, no markup, no
//! capability); on, administrators take the original files, one title or an album as a ZIP archive, and every
//! download is in the change log. The HTTP layer is called directly (no network).

mod common;

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use common::{book, open};
use david_server::admin::pages::AdminUi;
use david_server::api::ApiState;
use david_server::features::Features;
use david_server::web::{AppState, router};
use david_server::zipstream::{self, Crc32};
use david_server::{NewProfile, Role, Schedule, Store};
use tower::ServiceExt;

struct App {
    router: Router,
    store: Store,
    admin: i64,
    /// The folder the library lives in; the test folder around it is for files that must stay outside.
    outer: tempfile::TempDir,
    _data: tempfile::TempDir,
}

impl App {
    fn library(&self) -> std::path::PathBuf {
        self.outer.path().join("library")
    }
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    bytes: Vec<u8>,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }
}

async fn app(download: bool) -> App {
    let (data, store) = open().await;
    let outer = tempfile::TempDir::new().unwrap();
    let library = outer.path().join("library");
    std::fs::create_dir_all(&library).unwrap();
    let admin = store.create_profile("Boss", true).await.unwrap();
    let features = Features { download };
    let cd = david_server::cdimport::CdService::new(data.path());
    let web = AppState {
        store: store.clone(),
        mcp: false,
        listen: "127.0.0.1:8484".parse().unwrap(),
        fingerprint: None,
        ca_host: None,
        ui: Arc::new(AdminUi::new().unwrap().with_features(features)),
        secure_cookie: false,
        library: Some(library.clone()),
        uploads: david_server::upload::Uploads::start(store.clone(), data.path()),
        cd: cd.clone(),
        features,
    };
    let api = ApiState { store: store.clone(), library: Some(library), cd, features };
    App { router: router(web).merge(david_server::api::router(api)), store, admin, outer, _data: data }
}

async fn send(a: &App, req: Request<Body>) -> Reply {
    let res = a.router.clone().oneshot(req).await.unwrap();
    let (parts, body) = res.into_parts();
    Reply { status: parts.status, headers: parts.headers, bytes: to_bytes(body, 64 * 1024 * 1024).await.unwrap().to_vec() }
}

async fn get_as(a: &App, uri: &str, cookie: Option<&str>, extra: &[(&str, &str)]) -> Reply {
    let mut b = Request::builder().method("GET").uri(uri);
    if let Some(c) = cookie {
        b = b.header(header::COOKIE, c);
    }
    for (k, v) in extra {
        b = b.header(*k, *v);
    }
    send(a, b.body(Body::empty()).unwrap()).await
}

/// The cookie of a session of the given user.
async fn cookie_of(a: &App, user: i64, role: Role) -> String {
    format!("david_session={}", a.store.create_session(user, role, None).await.unwrap())
}

async fn admin_cookie(a: &App) -> String {
    cookie_of(a, a.admin, Role::Admin).await
}

/// `len` bytes that tell where they are (so that a wrong range shows).
fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

/// Imports an album with real files below the library: `per_disc[i]` titles on disc `i + 1`, each `len` bytes.
/// Returns the book id and the ids of its titles in order.
async fn add_album(a: &App, title: &str, per_disc: &[usize], len: usize) -> (i64, Vec<i64>) {
    let b = book("Author", title, per_disc);
    let dir = a.library().join(&b.folder);
    std::fs::create_dir_all(&dir).unwrap();
    for t in &b.tracks {
        std::fs::write(dir.join(&t.file), pattern(len)).unwrap();
    }
    let folder = b.folder.clone();
    a.store.import_books(vec![b]).await.unwrap();
    let id = a.store.find_book(&folder).await.unwrap().unwrap().id;
    (id, a.store.tracks_of(id).await.unwrap().iter().map(|t| t.id).collect())
}

/// The downloads in the change log, the newest first.
async fn log_of(a: &App, book: i64) -> Vec<(String, String, String, String)> {
    a.store.tag_changes(book, 50).await.unwrap().into_iter().filter(|c| c.field == "Download").map(|c| (c.token_name, c.file, c.old_value, c.new_value)).collect()
}

async fn pair_device(a: &App, user: i64) -> String {
    let e = a.store.create_enrollment(user, false, a.admin).await.unwrap();
    let key = format!("{}A=", "B".repeat(42));
    let body = serde_json::json!({ "code": e.code, "device_name": "Phone", "public_key": key });
    let req = Request::builder().method("POST").uri("/api/v1/pair").header(header::CONTENT_TYPE, "application/json").body(Body::from(body.to_string())).unwrap();
    let r = send(a, req).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text());
    serde_json::from_slice::<serde_json::Value>(&r.bytes).unwrap()["token"].as_str().unwrap().to_owned()
}

// The switch is off

#[tokio::test]
async fn off_by_default_there_is_no_route_no_markup_and_no_capability() {
    let a = app(false).await;
    let (id, tracks) = add_album(&a, "Hidden", &[2], 1000).await;
    let cookie = admin_cookie(&a).await;

    // The routes do not exist: 404 (not 403), even for an administrator.
    for uri in [format!("/admin/books/{id}/tracks/{}/download", tracks[0]), format!("/admin/books/{id}/download")] {
        assert_eq!(get_as(&a, &uri, Some(&cookie), &[]).await.status, StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(get_as(&a, &uri, None, &[]).await.status, StatusCode::NOT_FOUND, "{uri} without a session");
    }
    // No markup: neither the page with the album dialogs nor the list of the titles mentions it, in either language.
    for lang in ["en", "de"] {
        let page = get_as(&a, "/admin/books", Some(&cookie), &[("accept-language", lang)]).await;
        let list = get_as(&a, &format!("/admin/rpc/book.tracks?id={id}"), Some(&cookie), &[("accept-language", lang), ("hx-request", "true")]).await;
        assert_eq!((page.status, list.status), (StatusCode::OK, StatusCode::OK));
        for body in [page.text().to_lowercase(), list.text().to_lowercase()] {
            assert!(!body.contains("download") && !body.contains("herunterladen"), "{lang}");
        }
        assert!(list.text().contains("track-table"), "the list itself is there");
    }
    // The apps are told nothing.
    let admin_token = pair_device(&a, a.admin).await;
    let me = get_as(&a, "/api/v1/me", None, &[("authorization", &format!("Bearer {admin_token}"))]).await;
    let json: serde_json::Value = serde_json::from_slice(&me.bytes).unwrap();
    assert!(json.get("downloads").is_none(), "{json}");
    assert!(log_of(&a, id).await.is_empty());
}

#[tokio::test]
async fn the_list_of_titles_is_the_same_as_before_when_the_switch_is_off() {
    let a = app(false).await;
    let (id, _) = add_album(&a, "Plain", &[1], 10).await;
    let cookie = admin_cookie(&a).await;
    let list = get_as(&a, &format!("/admin/rpc/book.tracks?id={id}"), Some(&cookie), &[("hx-request", "true")]).await.text();
    // Four columns, as they always were; no extra cell.
    assert_eq!(list.matches("<th>").count(), 4, "{list}");
    assert_eq!(list.matches("<td>").count(), 4, "{list}");
    assert!(!list.contains("<a "), "{list}");
}

// The switch is on

#[tokio::test]
async fn an_administrator_gets_the_original_file_with_the_right_headers() {
    let a = app(true).await;
    // A file that needs several chunks.
    let (id, tracks) = add_album(&a, "Hörspiel", &[1], 200_000).await;
    let cookie = admin_cookie(&a).await;
    let r = get_as(&a, &format!("/admin/books/{id}/tracks/{}/download", tracks[0]), Some(&cookie), &[]).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.bytes, pattern(200_000), "the bytes of the file, unchanged");
    assert_eq!(r.header("content-length"), Some("200000"));
    assert_eq!(r.header("accept-ranges"), Some("bytes"));
    assert_eq!(r.header("content-type"), Some("audio/mpeg"));
    let disposition = r.header("content-disposition").unwrap();
    assert!(disposition.starts_with("attachment; "), "{disposition}");
    assert!(disposition.contains("filename=\"H_rspiel_CD01_TR01.mp3\"") && disposition.contains("filename*=UTF-8''H%C3%B6rspiel_CD01_TR01.mp3"), "{disposition}");
    assert_eq!(r.header("x-content-type-options"), Some("nosniff"));

    // The change log: who, which title, how much, complete; no content.
    let log = log_of(&a, id).await;
    assert_eq!(log.len(), 1, "{log:?}");
    assert_eq!(log[0].0, format!("admin:{}", a.admin));
    assert_eq!(log[0].1, "Hörspiel_CD01_TR01.mp3");
    assert_eq!((log[0].2.as_str(), log[0].3.as_str()), ("", "complete, 200000 bytes"));
}

#[tokio::test]
async fn umlauts_and_dangerous_characters_in_a_file_name_are_made_safe() {
    let a = app(true).await;
    let mut b = book("Author", "Namen", &[2]);
    b.tracks[0].file = "Äpfel & Birnen – Kapitel 1.mp3".into();
    b.tracks[1].file = "Say \"x\"; no\r\nline.mp3".into();
    let dir = a.library().join(&b.folder);
    std::fs::create_dir_all(&dir).unwrap();
    for t in &b.tracks {
        std::fs::write(dir.join(&t.file), b"data").unwrap();
    }
    let folder = b.folder.clone();
    a.store.import_books(vec![b]).await.unwrap();
    let id = a.store.find_book(&folder).await.unwrap().unwrap().id;
    let tracks = a.store.tracks_of(id).await.unwrap();
    let cookie = admin_cookie(&a).await;

    let first = get_as(&a, &format!("/admin/books/{id}/tracks/{}/download", tracks[0].id), Some(&cookie), &[]).await;
    assert_eq!(first.status, StatusCode::OK);
    assert_eq!(
        first.header("content-disposition"),
        Some("attachment; filename=\"_pfel & Birnen _ Kapitel 1.mp3\"; filename*=UTF-8''%C3%84pfel%20&%20Birnen%20%E2%80%93%20Kapitel%201.mp3")
    );
    let second = get_as(&a, &format!("/admin/books/{id}/tracks/{}/download", tracks[1].id), Some(&cookie), &[]).await;
    assert_eq!(second.status, StatusCode::OK);
    let d = second.header("content-disposition").unwrap();
    assert_eq!(d, "attachment; filename=\"Say x; noline.mp3\"; filename*=UTF-8''Say%20x%3B%20noline.mp3", "no quote, no line break");
}

#[tokio::test]
async fn range_requests_answer_206_and_416() {
    let a = app(true).await;
    let (id, tracks) = add_album(&a, "Ranges", &[1], 200_000).await;
    let cookie = admin_cookie(&a).await;
    let uri = format!("/admin/books/{id}/tracks/{}/download", tracks[0]);
    let all = pattern(200_000);

    let r = get_as(&a, &uri, Some(&cookie), &[("range", "bytes=100-199")]).await;
    assert_eq!(r.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!((r.header("content-range"), r.header("content-length")), (Some("bytes 100-199/200000"), Some("100")));
    assert_eq!(r.bytes, all[100..200]);

    // Across chunks, open ended.
    let r = get_as(&a, &uri, Some(&cookie), &[("range", "bytes=150000-")]).await;
    assert_eq!((r.status, r.header("content-range")), (StatusCode::PARTIAL_CONTENT, Some("bytes 150000-199999/200000")));
    assert_eq!(r.bytes, all[150_000..]);

    // The last bytes.
    let r = get_as(&a, &uri, Some(&cookie), &[("range", "bytes=-10")]).await;
    assert_eq!((r.status, r.header("content-range")), (StatusCode::PARTIAL_CONTENT, Some("bytes 199990-199999/200000")));
    assert_eq!(r.bytes, all[199_990..]);

    // Behind the end: 416 with the size, and no body of the file.
    let r = get_as(&a, &uri, Some(&cookie), &[("range", "bytes=200000-")]).await;
    assert_eq!((r.status, r.header("content-range")), (StatusCode::RANGE_NOT_SATISFIABLE, Some("bytes */200000")));
    assert!(r.bytes.is_empty());

    // Several ranges (and a conditional range) get the whole file, which the standard allows.
    let r = get_as(&a, &uri, Some(&cookie), &[("range", "bytes=0-1,5-6")]).await;
    assert_eq!((r.status, r.bytes.len()), (StatusCode::OK, 200_000));
    let r = get_as(&a, &uri, Some(&cookie), &[("range", "bytes=0-9"), ("if-range", "\"x\"")]).await;
    assert_eq!((r.status, r.bytes.len()), (StatusCode::OK, 200_000));

    // The log names the part of the first request; the refused one is not a download.
    let log = log_of(&a, id).await;
    assert_eq!(log.len(), 5, "{log:?}");
    let part = log.iter().find(|l| l.2 == "bytes 100-199/200000").expect("the range is in the log");
    assert_eq!(part.3, "complete, 100 bytes");
}

#[tokio::test]
async fn a_head_request_is_no_download() {
    let a = app(true).await;
    let (id, tracks) = add_album(&a, "Head", &[1], 5000).await;
    let cookie = admin_cookie(&a).await;
    let req = Request::builder().method("HEAD").uri(format!("/admin/books/{id}/tracks/{}/download", tracks[0])).header(header::COOKIE, &cookie).body(Body::empty()).unwrap();
    let r = send(&a, req).await;
    assert_eq!((r.status, r.bytes.len(), r.header("content-length")), (StatusCode::OK, 0, Some("5000")));
    assert!(log_of(&a, id).await.is_empty());
}

#[tokio::test]
async fn only_administrators_may_download_not_members_children_or_anonymous_visitors() {
    let a = app(true).await;
    let (id, tracks) = add_album(&a, "Closed", &[1], 1000).await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let mut planner = Schedule::none();
    planner.allow_every_day(0, 1440);
    let kim = a.store.create_user(NewProfile::child("Kim", 8, planner)).await.unwrap();
    let guest = a.store.create_user(NewProfile::new("Gast", Role::Guest)).await.unwrap();
    let track_uri = format!("/admin/books/{id}/tracks/{}/download", tracks[0]);
    let album_uri = format!("/admin/books/{id}/download");

    for (user, role) in [(anna, Role::Member), (kim, Role::Child), (guest, Role::Guest)] {
        let cookie = cookie_of(&a, user, role).await;
        for uri in [&track_uri, &album_uri] {
            let r = get_as(&a, uri, Some(&cookie), &[]).await;
            assert_eq!(r.status, StatusCode::FORBIDDEN, "{role:?} {uri}");
            assert!(r.bytes.len() < 2000 && !r.bytes.starts_with(&pattern(8)), "no file content in the refusal");
        }
    }
    // Not signed in: sent to the sign-in page.
    for uri in [&track_uri, &album_uri] {
        let r = get_as(&a, uri, None, &[]).await;
        assert_eq!((r.status, r.header("location")), (StatusCode::SEE_OTHER, Some("/admin/login")), "{uri}");
    }
    assert!(log_of(&a, id).await.is_empty(), "a refusal is no download");
}

#[tokio::test]
async fn a_title_that_is_not_in_the_album_of_the_address_is_not_found() {
    let a = app(true).await;
    let (first, first_tracks) = add_album(&a, "One", &[1], 100).await;
    let (second, second_tracks) = add_album(&a, "Two", &[1], 100).await;
    let cookie = admin_cookie(&a).await;
    assert_eq!(get_as(&a, &format!("/admin/books/{first}/tracks/{}/download", first_tracks[0]), Some(&cookie), &[]).await.status, StatusCode::OK);
    for uri in [
        format!("/admin/books/{first}/tracks/{}/download", second_tracks[0]),
        format!("/admin/books/{second}/tracks/{}/download", first_tracks[0]),
        format!("/admin/books/{first}/tracks/99999/download"),
        "/admin/books/99999/tracks/1/download".to_owned(),
        "/admin/books/99999/download".to_owned(),
    ] {
        assert_eq!(get_as(&a, &uri, Some(&cookie), &[]).await.status, StatusCode::NOT_FOUND, "{uri}");
    }
}

#[tokio::test]
async fn nothing_outside_the_library_can_be_taken() {
    let a = app(true).await;
    let secret = a.outer.path().join("secret.txt");
    std::fs::write(&secret, "TOP-SECRET").unwrap();
    // A database row that names a file outside the library (as a damaged or hostile database could), one through a link, and one by an absolute path.
    let mut b = book("Author", "Evil", &[3]);
    b.tracks[0].file = "../../secret.txt".into();
    b.tracks[1].file = "link.txt".into();
    b.tracks[2].file = secret.display().to_string();
    let dir = a.library().join(&b.folder);
    std::fs::create_dir_all(&dir).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&secret, dir.join("link.txt")).unwrap();
    let folder = b.folder.clone();
    a.store.import_books(vec![b]).await.unwrap();
    let id = a.store.find_book(&folder).await.unwrap().unwrap().id;
    let tracks = a.store.tracks_of(id).await.unwrap();
    let cookie = admin_cookie(&a).await;

    for t in &tracks {
        let r = get_as(&a, &format!("/admin/books/{id}/tracks/{}/download", t.id), Some(&cookie), &[]).await;
        assert!(r.status.is_client_error(), "{}: {}", t.file, r.status);
        assert!(!r.text().contains("TOP-SECRET"));
    }
    // The album is refused as a whole, and nothing of it starts.
    let r = get_as(&a, &format!("/admin/books/{id}/download"), Some(&cookie), &[]).await;
    assert!(r.status.is_client_error() && !r.text().contains("TOP-SECRET"), "{}", r.status);
    // The address itself never becomes a path: it only names numbers.
    for uri in [
        format!("/admin/books/{id}/tracks/..%2F..%2Fsecret.txt/download"),
        format!("/admin/books/{id}/tracks/{}%2F..%2F..%2Fsecret.txt/download", tracks[0].id),
        "/admin/books/../tracks/1/download".to_owned(),
    ] {
        let r = get_as(&a, &uri, Some(&cookie), &[]).await;
        assert!(r.status.is_client_error() && !r.text().contains("TOP-SECRET"), "{uri}: {}", r.status);
    }
    assert!(log_of(&a, id).await.is_empty());
}

#[tokio::test]
async fn an_album_comes_as_one_zip_archive_that_lists_the_right_entries() {
    let a = app(true).await;
    // Two discs: folders per disc. Files of several chunks, so that the archive is streamed.
    let (id, tracks) = add_album(&a, "Große Reise", &[2, 1], 150_000).await;
    let cookie = admin_cookie(&a).await;
    let r = get_as(&a, &format!("/admin/books/{id}/download"), Some(&cookie), &[]).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.header("content-type"), Some("application/zip"));
    let d = r.header("content-disposition").unwrap();
    assert_eq!(d, "attachment; filename=\"Gro_e Reise.zip\"; filename*=UTF-8''Gro%C3%9Fe%20Reise.zip");
    assert!(r.header("content-length").is_none(), "the archive is written as it is read");

    let listed = zipstream::list(&r.bytes).unwrap();
    let names: Vec<&str> = listed.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        ["CD 1/Große Reise_CD01_TR01.mp3", "CD 1/Große Reise_CD01_TR02.mp3", "CD 2/Große Reise_CD02_TR01.mp3"],
        "{names:?}"
    );
    let content = pattern(150_000);
    let mut crc = Crc32::new();
    crc.update(&content);
    for e in &listed {
        assert_eq!((e.size, e.crc), (150_000, crc.finish()), "{}", e.name);
        // The data stand behind a local header of 30 bytes and the name, stored as they are.
        let start = e.offset as usize + 30 + e.name.len();
        assert_eq!(&r.bytes[start..start + 150_000], &content[..], "{}", e.name);
    }
    assert_eq!(tracks.len(), listed.len());

    let log = log_of(&a, id).await;
    assert_eq!(log.len(), 1, "{log:?}");
    assert_eq!((log[0].0.as_str(), log[0].1.as_str(), log[0].2.as_str()), (format!("admin:{}", a.admin).as_str(), "", "album, 3 files"));
    assert_eq!(log[0].3, format!("complete, {} bytes", r.bytes.len()));
}

#[tokio::test]
async fn the_entries_of_an_album_of_one_disc_are_flat() {
    let a = app(true).await;
    let (id, _) = add_album(&a, "Kurz", &[2], 500).await;
    let cookie = admin_cookie(&a).await;
    let r = get_as(&a, &format!("/admin/books/{id}/download"), Some(&cookie), &[]).await;
    let names: Vec<String> = zipstream::list(&r.bytes).unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["Kurz_CD01_TR01.mp3", "Kurz_CD01_TR02.mp3"]);
}

#[tokio::test]
async fn an_album_with_a_missing_file_is_refused_before_anything_is_sent() {
    let a = app(true).await;
    let (id, _) = add_album(&a, "Lücke", &[2], 500).await;
    std::fs::remove_file(a.library().join("Author/Lücke/Lücke_CD01_TR02.mp3")).unwrap();
    let cookie = admin_cookie(&a).await;
    let r = get_as(&a, &format!("/admin/books/{id}/download"), Some(&cookie), &[]).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert!(log_of(&a, id).await.is_empty());
}

#[tokio::test]
async fn the_pages_offer_the_buttons_only_while_the_switch_is_on() {
    let a = app(true).await;
    let (id, tracks) = add_album(&a, "Knöpfe", &[1], 100).await;
    let cookie = admin_cookie(&a).await;
    let page = get_as(&a, "/admin/books", Some(&cookie), &[]).await.text();
    assert!(page.contains(&format!("href=\"/admin/books/{id}/download\"")), "the album header has the button");
    assert!(page.contains("Download</span>"), "{}", page.len());
    let list = get_as(&a, &format!("/admin/rpc/book.tracks?id={id}"), Some(&cookie), &[("hx-request", "true")]).await.text();
    assert!(list.contains(&format!("href=\"/admin/books/{id}/tracks/{}/download\"", tracks[0])), "{list}");
    assert_eq!(list.matches("<th>").count(), 5);
    // Plain links: no script is needed.
    assert!(!list.contains("hx-") && !list.contains("onclick"), "{list}");
    // German.
    let de = get_as(&a, "/admin/books", Some(&cookie), &[("accept-language", "de")]).await.text();
    assert!(de.contains("Herunterladen</span>") && de.contains("Das ganze Album als ZIP-Archiv"), "German labels");
    let de_list = get_as(&a, &format!("/admin/rpc/book.tracks?id={id}"), Some(&cookie), &[("accept-language", "de"), ("hx-request", "true")]).await.text();
    assert!(de_list.contains("Diesen Titel herunterladen"));
    // Music albums have the same dialog.
    assert_eq!(get_as(&a, "/admin/music", Some(&cookie), &[]).await.status, StatusCode::OK);
}

#[tokio::test]
async fn the_apps_learn_the_capability_only_while_it_is_on() {
    let a = app(true).await;
    let token = pair_device(&a, a.admin).await;
    let me = get_as(&a, "/api/v1/me", None, &[("authorization", &format!("Bearer {token}"))]).await;
    let json: serde_json::Value = serde_json::from_slice(&me.bytes).unwrap();
    assert_eq!(json["downloads"], serde_json::json!(true), "{json}");
    assert_eq!(json["role"], "admin");
}
