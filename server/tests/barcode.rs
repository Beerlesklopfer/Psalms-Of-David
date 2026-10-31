// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! `GET /api/v1/barcode/{code}`: the app scans an EAN and asks whether the album is in the library (schema 0056, `docs/spec-app-api.md`, "Barcodes").

mod common;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use common::{book, open};
use david_server::api::{ApiState, router};
use david_server::{NewProfile, Role, Store};
use serde_json::{Value, json};
use tower::ServiceExt;

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

async fn get(a: &Api, uri: &str, token: &str) -> (StatusCode, Value) {
    let req = Request::builder().uri(uri).header(header::AUTHORIZATION, format!("Bearer {token}")).body(Body::empty()).unwrap();
    let (parts, body) = a.router.clone().oneshot(req).await.unwrap().into_parts();
    let bytes = to_bytes(body, 1024 * 1024).await.unwrap();
    (parts.status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn pair(a: &Api, user: i64, n: u8) -> String {
    let letters = b"AEIMQUYcgkosw048";
    let key = format!("{}{}=", "B".repeat(42), letters[n as usize % letters.len()] as char);
    let e = a.store.create_enrollment(user, false, a.admin).await.unwrap();
    let body = json!({ "code": e.code, "device_name": "Phone", "public_key": key });
    let req = Request::builder().method("POST").uri("/api/v1/pair").header(header::CONTENT_TYPE, "application/json").body(Body::from(body.to_string())).unwrap();
    let res = a.router.clone().oneshot(req).await.unwrap();
    let bytes = to_bytes(res.into_body(), 1024 * 1024).await.unwrap();
    serde_json::from_slice::<Value>(&bytes).unwrap()["token"].as_str().unwrap().to_owned()
}

/// Imports an album with this barcode; `min_age` releases it from that age.
async fn add(a: &Api, title: &str, barcode: Option<&str>, min_age: i64) -> i64 {
    let mut b = book("Artist", title, &[2]);
    b.barcode = barcode.map(str::to_owned);
    let folder = b.folder.clone();
    a.store.import_books(vec![b]).await.unwrap();
    let id = a.store.find_book(&folder).await.unwrap().unwrap().id;
    a.store.release_book(id, min_age).await.unwrap();
    id
}

#[tokio::test]
async fn a_scanned_barcode_finds_the_album_and_the_two_spellings_find_the_same_one() {
    let a = api().await;
    let token = pair(&a, a.admin, 1).await;
    let id = add(&a, "Abbey", Some("0036000291452"), 0).await;
    add(&a, "Other", Some("4006381333931"), 0).await;
    add(&a, "Without", None, 0).await;

    // The EAN-13 with a leading 0, the UPC-A, and the typed form with spaces and hyphens: one product.
    for code in ["0036000291452", "036000291452", "0-36000 291452", "00036000291452"] {
        let (status, body) = get(&a, &format!("/api/v1/barcode/{}", code.replace(' ', "%20")), &token).await;
        assert_eq!(status, StatusCode::OK, "{code}");
        assert_eq!(body["found"], true, "{code}");
        assert_eq!(body["gtin"], "00036000291452");
        assert_eq!(body["items"][0]["item_id"], id);
        assert_eq!(body["items"][0]["title"], "Abbey");
        assert_eq!(body["items"][0]["creator"], "Artist");
        assert_eq!(body["items"][0]["kind"], "audiobook");
        assert_eq!(body["items"][0]["has_cover"], false);
        assert_eq!(body["items"].as_array().unwrap().len(), 1);
    }
    assert_eq!(get(&a, "/api/v1/barcode/036000291452", &token).await.1["code"], "036000291452");

    // A valid code that nobody has, and an EAN-8.
    let (status, body) = get(&a, "/api/v1/barcode/96385074", &token).await;
    assert_eq!((status, &body["found"], body["items"].as_array().unwrap().len()), (StatusCode::OK, &json!(false), 0));
    assert_eq!(body["gtin"], "00000096385074");
}

#[tokio::test]
async fn a_wrong_barcode_is_refused_and_a_device_is_needed() {
    let a = api().await;
    let token = pair(&a, a.admin, 1).await;
    for code in ["4006381333932", "12345", "abc", "400638133393100", "4006381333931x"] {
        let (status, body) = get(&a, &format!("/api/v1/barcode/{code}"), &token).await;
        assert_eq!((status, body["error"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_barcode")), "{code}");
    }
    let (status, _) = get(&a, "/api/v1/barcode/4006381333931", "nope").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_album_the_user_may_not_see_is_not_told() {
    let a = api().await;
    let mut schedule = david_server::Schedule::none();
    schedule.allow_every_day(7 * 60, 19 * 60);
    let kid = a.store.create_user(NewProfile { name: "Kid".into(), role: Role::Child, max_age: Some(6), schedule: Some(schedule) }).await.unwrap();
    let (admin_token, kid_token) = (pair(&a, a.admin, 1).await, pair(&a, kid, 2).await);
    let id = add(&a, "Grown-up", Some("4006381333931"), 16).await;

    let (_, body) = get(&a, "/api/v1/barcode/4006381333931", &admin_token).await;
    assert_eq!((body["found"].as_bool(), body["items"][0]["item_id"].as_i64()), (Some(true), Some(id)));
    let (status, body) = get(&a, "/api/v1/barcode/4006381333931", &kid_token).await;
    assert_eq!((status, body["found"].as_bool(), body["items"].as_array().unwrap().len()), (StatusCode::OK, Some(false), 0));
}

#[tokio::test]
async fn the_item_detail_carries_the_barcode() {
    let a = api().await;
    let token = pair(&a, a.admin, 1).await;
    let with = add(&a, "With", Some("4006381333931"), 0).await;
    let without = add(&a, "Without", None, 0).await;
    let (_, body) = get(&a, &format!("/api/v1/items/{with}"), &token).await;
    assert_eq!(body["barcode"], "4006381333931");
    let (_, body) = get(&a, &format!("/api/v1/items/{without}"), &token).await;
    assert!(body["barcode"].is_null() && body.get("barcode").is_some());
}

#[tokio::test]
async fn a_rescan_with_no_barcode_in_the_tags_takes_it_away_and_a_new_one_replaces_it() {
    let a = api().await;
    let id = add(&a, "Abbey", Some("4006381333931"), 0).await;
    assert_eq!(a.store.book_barcode(id).await.unwrap().as_deref(), Some("4006381333931"));
    let mut b = book("Artist", "Abbey", &[2]);
    b.barcode = Some("036000291452".into());
    a.store.import_books(vec![b]).await.unwrap();
    assert_eq!(a.store.book_barcode(id).await.unwrap().as_deref(), Some("036000291452"));
    a.store.import_books(vec![book("Artist", "Abbey", &[2])]).await.unwrap();
    assert_eq!(a.store.book_barcode(id).await.unwrap(), None);
}
