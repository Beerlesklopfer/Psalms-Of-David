// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The interface of the apps, called directly (no network): pairing, library,
//! streaming with ranges, the weekly planner, progress, and the end of a session.

mod common;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use common::{book, open};
use david_server::api::{ApiState, router};
use david_server::{Anchor, NewProfile, Role, Schedule, Store};
use serde_json::{Value, json};
use tower::ServiceExt;

fn key(n: u8) -> String {
    let letters = b"AEIMQUYcgkosw048";
    format!("{}{}=", "B".repeat(42), letters[n as usize % letters.len()] as char)
}

struct Api {
    router: Router,
    store: Store,
    admin: i64,
    library: tempfile::TempDir,
    _dir: tempfile::TempDir,
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    bytes: Vec<u8>,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.bytes).unwrap_or(Value::Null)
    }
}

async fn api() -> Api {
    let (dir, store) = open().await;
    let admin = store.create_profile("Boss", true).await.unwrap();
    let library = tempfile::TempDir::new().unwrap();
    let state = ApiState { store: store.clone(), library: Some(library.path().to_owned()) };
    Api { router: router(state), store, admin, library, _dir: dir }
}

async fn call(a: &Api, method: &str, uri: &str, token: Option<&str>, body: Option<Value>, range: Option<&str>) -> Reply {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    if let Some(r) = range {
        b = b.header(header::RANGE, r);
    }
    let req = match body {
        Some(v) => b.header(header::CONTENT_TYPE, "application/json").body(Body::from(v.to_string())).unwrap(),
        None => b.body(Body::empty()).unwrap(),
    };
    let res = a.router.clone().oneshot(req).await.unwrap();
    let (parts, body) = res.into_parts();
    Reply { status: parts.status, headers: parts.headers, bytes: to_bytes(body, 8 * 1024 * 1024).await.unwrap().to_vec() }
}

/// Pairs a device for `user` and returns its token.
async fn pair(a: &Api, user: i64, n: u8) -> String {
    let e = a.store.create_enrollment(user, false, a.admin).await.unwrap();
    let r = call(a, "POST", "/api/v1/pair", None, Some(json!({ "code": e.code, "device_name": "Phone", "public_key": key(n) })), None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", String::from_utf8_lossy(&r.bytes));
    r.json()["token"].as_str().unwrap().to_owned()
}

/// Imports a one-track book with a real file of 1000 bytes (byte i = i % 251).
/// Returns (book id, track id).
async fn add_book(a: &Api, title: &str, min_age: Option<i64>) -> (i64, i64) {
    let b = book("Author", title, &[1]);
    let folder = a.library.path().join(&b.folder);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(&b.tracks[0].file), (0..1000u32).map(|i| (i % 251) as u8).collect::<Vec<_>>()).unwrap();
    let folder_name = b.folder.clone();
    a.store.import_books(vec![b]).await.unwrap();
    let id = a.store.find_book(&folder_name).await.unwrap().unwrap().id;
    if let Some(age) = min_age {
        a.store.release_book(id, age).await.unwrap();
    }
    (id, a.store.tracks_of(id).await.unwrap()[0].id)
}

#[tokio::test]
async fn pairing_gives_a_token_once_and_refuses_bad_input() {
    let a = api().await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let e = a.store.create_enrollment(anna, false, a.admin).await.unwrap();

    let bad_key = call(&a, "POST", "/api/v1/pair", None, Some(json!({ "code": e.code, "device_name": "Phone", "public_key": "nope" })), None).await;
    assert_eq!(bad_key.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(bad_key.json()["error"], "invalid_public_key");

    let ok = call(&a, "POST", "/api/v1/pair", None, Some(json!({ "code": e.code, "device_name": "Phone", "public_key": key(1) })), None).await;
    assert_eq!(ok.status, StatusCode::OK, "a refused request does not use the code up");
    let j = ok.json();
    assert_eq!(j["user"]["name"], "Anna");
    assert_eq!(j["user"]["role"], "member");
    let token = j["token"].as_str().unwrap();

    let again = call(&a, "POST", "/api/v1/pair", None, Some(json!({ "code": e.code, "device_name": "Tablet", "public_key": key(2) })), None).await;
    assert_eq!(again.status, StatusCode::FORBIDDEN);
    assert_eq!(again.json()["error"], "enrollment_invalid");

    let me = call(&a, "GET", "/api/v1/me", Some(token), None, None).await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.json()["name"], "Anna");
}

#[tokio::test]
async fn everything_else_needs_a_token() {
    let a = api().await;
    for uri in ["/api/v1/me", "/api/v1/library/version", "/api/v1/items", "/api/v1/items/1", "/api/v1/continue", "/api/v1/parts/1/stream"] {
        let r = call(&a, "GET", uri, None, None, None).await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{uri}");
        let wrong = call(&a, "GET", uri, Some("not-a-token"), None, None).await;
        assert_eq!(wrong.status, StatusCode::UNAUTHORIZED, "{uri}");
    }
}

#[tokio::test]
async fn only_released_books_exist_for_a_user() {
    let a = api().await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let token = pair(&a, anna, 1).await;
    let (hidden, hidden_track) = add_book(&a, "Hidden", None).await;
    let (shown, _) = add_book(&a, "Shown", Some(0)).await;

    let list = call(&a, "GET", "/api/v1/items", Some(&token), None, None).await.json();
    let ids: Vec<i64> = list.as_array().unwrap().iter().map(|b| b["id"].as_i64().unwrap()).collect();
    assert_eq!(ids, vec![shown]);

    assert_eq!(call(&a, "GET", &format!("/api/v1/items/{hidden}"), Some(&token), None, None).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call(&a, "GET", &format!("/api/v1/parts/{hidden_track}/stream"), Some(&token), None, None).await.status, StatusCode::NOT_FOUND);

    let detail = call(&a, "GET", &format!("/api/v1/items/{shown}"), Some(&token), None, None).await.json();
    assert_eq!(detail["title"], "Shown");
    assert_eq!(detail["kind"], "audiobook");
    assert_eq!(detail["parts"].as_array().unwrap().len(), 1);
    assert_eq!(detail["access"]["state"], "allowed");
    assert!(detail["progress"].is_null());
}

#[tokio::test]
async fn the_stream_serves_the_file_and_ranges() {
    let a = api().await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let token = pair(&a, anna, 1).await;
    let (_, track) = add_book(&a, "Shown", Some(0)).await;
    let uri = format!("/api/v1/parts/{track}/stream");

    let whole = call(&a, "GET", &uri, Some(&token), None, None).await;
    assert_eq!(whole.status, StatusCode::OK);
    assert_eq!(whole.bytes.len(), 1000);
    assert_eq!(whole.headers.get(header::ACCEPT_RANGES).unwrap(), "bytes");

    let part = call(&a, "GET", &uri, Some(&token), None, Some("bytes=250-499")).await;
    assert_eq!(part.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(part.bytes, whole.bytes[250..500]);
    assert_eq!(part.headers.get(header::CONTENT_RANGE).unwrap(), "bytes 250-499/1000");
}

#[tokio::test]
async fn a_file_outside_the_library_is_never_served() {
    let a = api().await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let token = pair(&a, anna, 1).await;
    std::fs::write(a.library.path().join("..").join("david-secret.txt"), "secret").unwrap();
    let mut b = book("Author", "Evil", &[1]);
    b.tracks[0].file = "../../david-secret.txt".into();
    let folder = b.folder.clone();
    a.store.import_books(vec![b]).await.unwrap();
    let id = a.store.find_book(&folder).await.unwrap().unwrap().id;
    a.store.release_book(id, 0).await.unwrap();
    let track = a.store.tracks_of(id).await.unwrap()[0].id;
    std::fs::create_dir_all(a.library.path().join(&folder)).unwrap();

    let r = call(&a, "GET", &format!("/api/v1/parts/{track}/stream"), Some(&token), None, None).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let _ = std::fs::remove_file(a.library.path().join("..").join("david-secret.txt"));
}

#[tokio::test]
async fn the_planner_blocks_the_stream_unless_the_book_is_exempt() {
    let a = api().await;
    let kid = a.store.create_user(NewProfile::child("Kid", 12, Schedule::none())).await.unwrap();
    let token = pair(&a, kid, 1).await;
    let (book_id, track) = add_book(&a, "Story", Some(6)).await;
    let (exempt, exempt_track) = add_book(&a, "Lesson", Some(6)).await;
    a.store.set_book_quota_exempt(exempt, true).await.unwrap();

    let blocked = call(&a, "GET", &format!("/api/v1/parts/{track}/stream"), Some(&token), None, None).await;
    assert_eq!(blocked.status, StatusCode::FORBIDDEN);
    assert_eq!(blocked.json()["error"], "outside_schedule");
    let detail = call(&a, "GET", &format!("/api/v1/items/{book_id}"), Some(&token), None, None).await.json();
    assert_eq!(detail["access"]["state"], "outside_schedule");

    let free = call(&a, "GET", &format!("/api/v1/parts/{exempt_track}/stream"), Some(&token), None, None).await;
    assert_eq!(free.status, StatusCode::OK);

    // A planner that allows every minute lets the story through.
    a.store.set_profile_schedule(kid, Some(&Schedule::always())).await.unwrap();
    let open = call(&a, "GET", &format!("/api/v1/parts/{track}/stream"), Some(&token), None, None).await;
    assert_eq!(open.status, StatusCode::OK);
}

#[tokio::test]
async fn a_child_does_not_see_books_above_the_age_limit() {
    let a = api().await;
    let kid = a.store.create_user(NewProfile::child("Kid", 8, Schedule::always())).await.unwrap();
    let token = pair(&a, kid, 1).await;
    let (old, old_track) = add_book(&a, "Grown up", Some(16)).await;
    let list = call(&a, "GET", "/api/v1/items", Some(&token), None, None).await.json();
    assert!(list.as_array().unwrap().is_empty());
    assert_eq!(call(&a, "GET", &format!("/api/v1/items/{old}"), Some(&token), None, None).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call(&a, "GET", &format!("/api/v1/parts/{old_track}/stream"), Some(&token), None, None).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn progress_is_stored_and_listed_for_continuing() {
    let a = api().await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let token = pair(&a, anna, 1).await;
    let (book_id, track) = add_book(&a, "Shown", Some(0)).await;
    let (other_book, other_track) = add_book(&a, "Other", Some(0)).await;

    let put = |b: i64, t: i64, p: i64| json!({ "item_id": b, "part_id": t, "position_ms": p, "finished": false });
    let ok = call(&a, "PUT", "/api/v1/progress", Some(&token), Some(put(book_id, track, 61_000)), None).await;
    assert_eq!(ok.status, StatusCode::NO_CONTENT);
    // A track of another book, and a negative position, are refused.
    assert_eq!(call(&a, "PUT", "/api/v1/progress", Some(&token), Some(put(book_id, other_track, 1)), None).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&a, "PUT", "/api/v1/progress", Some(&token), Some(put(book_id, track, -5)), None).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    a.store.flush().await.unwrap();

    let cont = call(&a, "GET", "/api/v1/continue", Some(&token), None, None).await.json();
    assert_eq!(cont.as_array().unwrap().len(), 1);
    assert_eq!(cont[0]["item_id"], book_id);
    assert_eq!(cont[0]["position_ms"], 61_000);
    let detail = call(&a, "GET", &format!("/api/v1/items/{book_id}"), Some(&token), None, None).await.json();
    assert_eq!(detail["progress"]["position_ms"], 61_000);
    let _ = other_book;
}

#[tokio::test]
async fn progress_of_an_unseen_book_is_refused() {
    let a = api().await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let token = pair(&a, anna, 1).await;
    let (hidden, track) = add_book(&a, "Hidden", None).await;
    let r = call(&a, "PUT", "/api/v1/progress", Some(&token), Some(json!({ "item_id": hidden, "part_id": track, "position_ms": 1 })), None).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn logout_and_revoking_the_device_end_the_session() {
    let a = api().await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let t1 = pair(&a, anna, 1).await;
    let t2 = pair(&a, anna, 2).await;

    assert_eq!(call(&a, "POST", "/api/v1/logout", Some(&t1), None, None).await.status, StatusCode::NO_CONTENT);
    assert_eq!(call(&a, "GET", "/api/v1/me", Some(&t1), None, None).await.status, StatusCode::UNAUTHORIZED);

    assert_eq!(call(&a, "GET", "/api/v1/me", Some(&t2), None, None).await.status, StatusCode::OK);
    let device = a.store.devices_of(anna).await.unwrap().into_iter().find(|d| d.wg_pubkey == key(2)).unwrap();
    a.store.revoke_device(device.id).await.unwrap();
    assert_eq!(call(&a, "GET", "/api/v1/me", Some(&t2), None, None).await.status, StatusCode::UNAUTHORIZED);
}

fn anchor(track_id: i64, chapter: u16, verse: u16, position_ms: i64) -> Anchor {
    Anchor { track_id, bible_book: 43, chapter, verse, position_ms }
}

fn uri(q: &str) -> String {
    format!("/api/v1/bible/resolve?q={}", q.replace(' ', "%20").replace(',', "%2C").replace(':', "%3A"))
}

#[tokio::test]
async fn a_passage_opens_at_its_anchor_or_the_nearest_verse_before() {
    let a = api().await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let token = pair(&a, anna, 1).await;
    let (bible, track) = add_book(&a, "Bible", Some(0)).await;
    a.store.set_bible_anchors(bible, &[anchor(track, 3, 1, 10_000), anchor(track, 3, 16, 95_000), anchor(track, 4, 1, 200_000)]).await.unwrap();

    let exact = call(&a, "GET", &uri("Johannes 3,16"), Some(&token), None, None).await.json();
    assert_eq!(exact["reference"], json!({ "book": 43, "chapter": 3, "verse": 16, "verse_to": null }));
    let m = &exact["matches"][0];
    assert_eq!((m["item_id"].as_i64(), m["part_id"].as_i64(), m["position_ms"].as_i64()), (Some(bible), Some(track), Some(95_000)));
    assert_eq!(m["exact"], true);
    assert_eq!(m["access"]["state"], "allowed");

    // Verse 20 has no anchor: the nearest one before it, and the answer says so.
    let near = call(&a, "GET", &uri("Joh 3:20"), Some(&token), None, None).await.json();
    assert_eq!(near["matches"][0]["position_ms"], 95_000);
    assert_eq!(near["matches"][0]["matched_verse"], 16);
    assert_eq!(near["matches"][0]["exact"], false);

    // A chapter alone opens at its start.
    let chapter = call(&a, "GET", &uri("John 4"), Some(&token), None, None).await.json();
    assert_eq!(chapter["matches"][0]["position_ms"], 200_000);

    // A chapter with no anchors has no match; an unknown book is an error.
    let none = call(&a, "GET", &uri("Johannes 9,1"), Some(&token), None, None).await.json();
    assert!(none["matches"].as_array().unwrap().is_empty());
    let bad = call(&a, "GET", &uri("Hobbit 1,1"), Some(&token), None, None).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(bad.json()["error"], "invalid_reference");
}

#[tokio::test]
async fn an_unreleased_bible_does_not_open_and_the_planner_is_reported() {
    let a = api().await;
    let kid = a.store.create_user(NewProfile::child("Kid", 12, Schedule::none())).await.unwrap();
    let token = pair(&a, kid, 1).await;
    let (hidden, hidden_track) = add_book(&a, "Hidden Bible", None).await;
    a.store.set_bible_anchors(hidden, &[anchor(hidden_track, 3, 16, 1_000)]).await.unwrap();
    assert!(call(&a, "GET", &uri("Joh 3,16"), Some(&token), None, None).await.json()["matches"].as_array().unwrap().is_empty());

    let (shown, shown_track) = add_book(&a, "Shown Bible", Some(0)).await;
    a.store.set_bible_anchors(shown, &[anchor(shown_track, 3, 16, 2_000)]).await.unwrap();
    let r = call(&a, "GET", &uri("Joh 3,16"), Some(&token), None, None).await.json();
    assert_eq!(r["matches"].as_array().unwrap().len(), 1);
    assert_eq!(r["matches"][0]["access"]["state"], "outside_schedule", "the app can tell why it does not play");
}

#[tokio::test]
async fn anchors_must_point_into_their_own_item_and_are_replaced_as_a_set() {
    let a = api().await;
    let (one, track_one) = add_book(&a, "One", Some(0)).await;
    let (_, track_two) = add_book(&a, "Two", Some(0)).await;
    assert!(a.store.set_bible_anchors(one, &[anchor(track_two, 1, 1, 0)]).await.is_err());
    a.store.set_bible_anchors(one, &[anchor(track_one, 1, 1, 0), anchor(track_one, 1, 2, 5_000)]).await.unwrap();
    a.store.set_bible_anchors(one, &[anchor(track_one, 2, 1, 0)]).await.unwrap();
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let hits = a.store.resolve_bible(anna, david_server::bible::parse("Joh 1,2").unwrap()).await.unwrap();
    assert!(hits.is_empty(), "the old anchors are gone");
}


// Product picture

/// A 1x1 PNG.
const PNG: &[u8] = &[
    0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0x0D, b'I', b'H', b'D', b'R', 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0,
    0x1F, 0x15, 0xC4, 0x89, 0, 0, 0, 0x0A, b'I', b'D', b'A', b'T', 0x78, 0x9C, 0x63, 0, 1, 0, 0, 5, 0, 1, 0x0D, 0x0A, 0x2D, 0xB4,
    0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82,
];

#[tokio::test]
async fn a_cover_file_in_the_folder_is_served_as_the_product_picture() {
    let a = api().await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let token = pair(&a, anna, 1).await;
    let (id, _) = add_book(&a, "Shown", Some(0)).await;
    let uri = format!("/api/v1/items/{id}/cover");

    // No picture yet: not found, not an error.
    assert_eq!(call(&a, "GET", &uri, Some(&token), None, None).await.status, StatusCode::NOT_FOUND);

    std::fs::write(a.library.path().join("Author/Shown/cover.png"), PNG).unwrap();
    let r = call(&a, "GET", &uri, Some(&token), None, None).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.headers.get(header::CONTENT_TYPE).unwrap(), "image/png");
    assert_eq!(r.bytes, PNG);

    // The type comes from the bytes: a script named cover.jpg is no picture.
    std::fs::remove_file(a.library.path().join("Author/Shown/cover.png")).unwrap();
    std::fs::write(a.library.path().join("Author/Shown/cover.jpg"), b"<svg onload=alert(1)>").unwrap();
    assert_eq!(call(&a, "GET", &uri, Some(&token), None, None).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_picture_embedded_in_the_first_file_is_the_fallback() {
    if !common::audio::ffmpeg_available() {
        eprintln!("ffmpeg is not installed; skipping");
        return;
    }
    let a = api().await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let token = pair(&a, anna, 1).await;
    // A real MP3 with a cover inside.
    let b = book("Author", "Embedded", &[1]);
    let folder = a.library.path().join(&b.folder);
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join(&b.tracks[0].file);
    common::audio::make_mp3(&file, &[("title", "T")]);
    {
        use lofty::prelude::*;
        use lofty::picture::{MimeType, Picture, PictureType};
        let mut tagged = lofty::read_from_path(&file).unwrap();
        let tag = tagged.primary_tag_mut().expect("make_mp3 writes a tag");
        tag.push_picture(Picture::unchecked(PNG.to_vec()).pic_type(PictureType::CoverFront).mime_type(MimeType::Png).build());
        tag.save_to_path(&file, lofty::config::WriteOptions::default()).unwrap();
    }
    let folder_name = b.folder.clone();
    a.store.import_books(vec![b]).await.unwrap();
    let id = a.store.find_book(&folder_name).await.unwrap().unwrap().id;
    a.store.release_book(id, 0).await.unwrap();

    let r = call(&a, "GET", &format!("/api/v1/items/{id}/cover"), Some(&token), None, None).await;
    assert_eq!((r.status, r.bytes), (StatusCode::OK, PNG.to_vec()));
}

#[tokio::test]
async fn the_cover_of_an_unreleased_item_does_not_exist_for_the_user() {
    let a = api().await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let token = pair(&a, anna, 1).await;
    let (id, _) = add_book(&a, "Hidden", None).await;
    std::fs::write(a.library.path().join("Author/Hidden/cover.png"), PNG).unwrap();
    assert_eq!(call(&a, "GET", &format!("/api/v1/items/{id}/cover"), Some(&token), None, None).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call(&a, "GET", &format!("/api/v1/items/{id}/cover"), None, None, None).await.status, StatusCode::UNAUTHORIZED);
}


#[tokio::test]
async fn the_blurb_comes_with_the_item() {
    let a = api().await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let token = pair(&a, anna, 1).await;
    let (id, _) = add_book(&a, "Shown", Some(0)).await;
    let uri = format!("/api/v1/items/{id}");
    assert!(call(&a, "GET", &uri, Some(&token), None, None).await.json()["description"].is_null());
    a.store.set_book_description(id, "  Two families, two sides of a wall.  ").await.unwrap();
    assert_eq!(call(&a, "GET", &uri, Some(&token), None, None).await.json()["description"], "Two families, two sides of a wall.");
}

async fn library_version(a: &Api, token: &str) -> String {
    call(a, "GET", "/api/v1/library/version", Some(token), None, None).await.json()["version"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn the_library_version_changes_when_the_library_does() {
    let a = api().await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let token = pair(&a, anna, 1).await;
    let empty = library_version(&a, &token).await;
    assert_eq!(empty, library_version(&a, &token).await, "the same library has the same version");

    let (book, _) = add_book(&a, "New CD", None).await;
    let added = library_version(&a, &token).await;
    assert_ne!(empty, added, "a new book changes it");

    a.store.release_book(book, 0).await.unwrap();
    let released = library_version(&a, &token).await;
    assert_ne!(added, released, "so does a release");

    a.store.set_book_description(book, "A blurb").await.unwrap();
    assert_ne!(released, library_version(&a, &token).await, "and a blurb");
}
