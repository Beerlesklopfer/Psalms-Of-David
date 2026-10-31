// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The personal profile of the apps (`/api/v1/me`): names, language, PIN and the picture.

mod common;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use common::open;
use david_server::api::{ApiState, router};
use david_server::{NewProfile, Role, Store};
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
    let state = ApiState { store: store.clone(), library: None, cd: david_server::cdimport::CdService::new(dir.path()), features: Default::default() };
    Api { router: router(state), store, admin, _dir: dir }
}

async fn send(a: &Api, method: &str, uri: &str, token: Option<&str>, content_type: &str, body: Vec<u8>, etag: Option<&str>) -> Reply {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    if let Some(e) = etag {
        b = b.header(header::IF_NONE_MATCH, e);
    }
    let req = b.header(header::CONTENT_TYPE, content_type).body(Body::from(body)).unwrap();
    let res = a.router.clone().oneshot(req).await.unwrap();
    let (parts, body) = res.into_parts();
    Reply { status: parts.status, headers: parts.headers, bytes: to_bytes(body, 8 * 1024 * 1024).await.unwrap().to_vec() }
}

async fn json_call(a: &Api, method: &str, uri: &str, token: &str, body: Value) -> Reply {
    send(a, method, uri, Some(token), "application/json", body.to_string().into_bytes(), None).await
}

async fn pair(a: &Api, user: i64, n: u8) -> String {
    let e = a.store.create_enrollment(user, false, a.admin).await.unwrap();
    let r = send(a, "POST", "/api/v1/pair", None, "application/json", json!({ "code": e.code, "device_name": "Phone", "public_key": key(n) }).to_string().into_bytes(), None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", String::from_utf8_lossy(&r.bytes));
    r.json()["token"].as_str().unwrap().to_owned()
}

/// A tiny but real PNG (1x1), JPEG-free: the type is decided by the first bytes.
fn png() -> Vec<u8> {
    let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    b.extend_from_slice(&[0; 40]);
    b
}

fn jpeg() -> Vec<u8> {
    let mut b = vec![0xFF, 0xD8, 0xFF, 0xE0];
    b.extend_from_slice(&[1; 100]);
    b
}

async fn member(a: &Api, name: &str, n: u8) -> (i64, String) {
    let id = a.store.create_user(NewProfile::new(name, Role::Member)).await.unwrap();
    (id, pair(a, id, n).await)
}

#[tokio::test]
async fn the_profile_names_the_picture_and_the_devices() {
    let a = api().await;
    let (_, anna) = member(&a, "Anna", 1).await;
    let r = send(&a, "GET", "/api/v1/me", Some(&anna), "application/json", vec![], None).await;
    assert_eq!(r.status, StatusCode::OK);
    let j = r.json();
    assert_eq!((j["name"].as_str(), j["role"].as_str(), j["has_avatar"].as_bool(), j["avatar_version"].is_null(), j["device_count"].as_i64()), (Some("Anna"), Some("member"), Some(false), true, Some(1)));
    let r = send(&a, "GET", "/api/v1/me/devices", Some(&anna), "application/json", vec![], None).await;
    let d = r.json();
    assert_eq!((d.as_array().map(Vec::len), d[0]["name"].as_str(), d[0]["this"].as_bool()), (Some(1), Some("Phone"), Some(true)));
}

#[tokio::test]
async fn names_and_language_change_partly_and_a_taken_nickname_is_refused() {
    let a = api().await;
    let (_, anna) = member(&a, "Anna", 1).await;
    member(&a, "Ben", 2).await;
    let r = json_call(&a, "PUT", "/api/v1/me", &anna, json!({ "first_name": "Anna", "last_name": "Muster", "language": "de" })).await;
    let j = r.json();
    assert_eq!((r.status, j["name"].as_str(), j["first_name"].as_str(), j["last_name"].as_str(), j["language"].as_str()), (StatusCode::OK, Some("Anna"), Some("Anna"), Some("Muster"), Some("de")));
    // The nickname alone changes it and keeps the other names; an empty language follows the device again.
    let r = json_call(&a, "PUT", "/api/v1/me", &anna, json!({ "nickname": "Annchen", "language": "" })).await;
    let j = r.json();
    assert_eq!((j["name"].as_str(), j["last_name"].as_str(), j["language"].is_null()), (Some("Annchen"), Some("Muster"), true));
    let r = json_call(&a, "PUT", "/api/v1/me", &anna, json!({ "nickname": "ben" })).await;
    assert_eq!((r.status, r.json()["error"].as_str()), (StatusCode::CONFLICT, Some("exists")));
    let r = json_call(&a, "PUT", "/api/v1/me", &anna, json!({ "nickname": "  " })).await;
    assert_eq!((r.status, r.json()["error"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_name")));
    let r = json_call(&a, "PUT", "/api/v1/me", &anna, json!({ "language": "xx" })).await;
    assert_eq!((r.status, r.json()["error"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_language")));
    // Role and limits are not fields of this call: they are ignored, not taken.
    let r = json_call(&a, "PUT", "/api/v1/me", &anna, json!({ "role": "admin", "max_age": 99 })).await;
    assert_eq!((r.json()["role"].as_str(), r.json()["max_age"].is_null()), (Some("member"), true));
}

#[tokio::test]
async fn the_picture_is_stored_read_with_its_version_and_removed() {
    let a = api().await;
    let (anna_id, anna) = member(&a, "Anna", 1).await;
    let none = send(&a, "GET", "/api/v1/me/avatar", Some(&anna), "application/json", vec![], None).await;
    assert_eq!(none.status, StatusCode::NOT_FOUND);

    let put = send(&a, "PUT", "/api/v1/me/avatar", Some(&anna), "image/jpeg", jpeg(), None).await;
    assert_eq!(put.status, StatusCode::OK, "{}", String::from_utf8_lossy(&put.bytes));
    let version = put.json()["avatar_version"].as_i64().unwrap();
    assert_eq!(put.json()["has_avatar"].as_bool(), Some(true));
    let got = send(&a, "GET", "/api/v1/me/avatar", Some(&anna), "application/json", vec![], None).await;
    assert_eq!((got.status, got.headers[header::CONTENT_TYPE].to_str().unwrap(), got.bytes), (StatusCode::OK, "image/jpeg", jpeg()));
    let etag = got.headers[header::ETAG].to_str().unwrap().to_owned();
    assert_eq!(etag, format!("\"{version}\""));
    let again = send(&a, "GET", "/api/v1/me/avatar", Some(&anna), "application/json", vec![], Some(&etag)).await;
    assert_eq!((again.status, again.bytes.len()), (StatusCode::NOT_MODIFIED, 0));

    // A PNG replaces it; the type comes from the bytes, not from what the sender says.
    let put = send(&a, "PUT", "/api/v1/me/avatar", Some(&anna), "text/plain", png(), None).await;
    assert_eq!(put.status, StatusCode::OK);
    let got = send(&a, "GET", "/api/v1/me/avatar", Some(&anna), "application/json", vec![], None).await;
    assert_eq!(got.headers[header::CONTENT_TYPE].to_str().unwrap(), "image/png");
    // The web page of the same user serves the same picture.
    assert!(a.store.has_avatar(anna_id).await.unwrap());

    let gone = send(&a, "DELETE", "/api/v1/me/avatar", Some(&anna), "application/json", vec![], None).await;
    assert_eq!((gone.status, gone.json()["has_avatar"].as_bool()), (StatusCode::OK, Some(false)));
    assert_eq!(send(&a, "GET", "/api/v1/me/avatar", Some(&anna), "application/json", vec![], None).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_picture_that_is_empty_too_large_or_no_picture_is_refused_and_each_user_has_only_their_own() {
    let a = api().await;
    let (_, anna) = member(&a, "Anna", 1).await;
    let (_, ben) = member(&a, "Ben", 2).await;
    let empty = send(&a, "PUT", "/api/v1/me/avatar", Some(&anna), "image/png", vec![], None).await;
    assert_eq!((empty.status, empty.json()["error"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("avatar_none")));
    let svg = send(&a, "PUT", "/api/v1/me/avatar", Some(&anna), "image/svg+xml", b"<svg onload=alert(1)></svg>".to_vec(), None).await;
    assert_eq!((svg.status, svg.json()["error"].as_str()), (StatusCode::UNSUPPORTED_MEDIA_TYPE, Some("avatar_type")));
    let mut big = jpeg();
    big.extend(std::iter::repeat_n(0u8, 300 * 1024));
    let r = send(&a, "PUT", "/api/v1/me/avatar", Some(&anna), "image/jpeg", big, None).await;
    assert_eq!((r.status, r.json()["error"].as_str()), (StatusCode::PAYLOAD_TOO_LARGE, Some("avatar_size")));
    let mut huge = jpeg();
    huge.extend(std::iter::repeat_n(0u8, 2 * 1024 * 1024));
    let r = send(&a, "PUT", "/api/v1/me/avatar", Some(&anna), "image/jpeg", huge, None).await;
    assert_eq!(r.status, StatusCode::PAYLOAD_TOO_LARGE);
    // Nothing was stored; Ben's picture is not Anna's.
    assert_eq!(send(&a, "GET", "/api/v1/me/avatar", Some(&anna), "application/json", vec![], None).await.status, StatusCode::NOT_FOUND);
    send(&a, "PUT", "/api/v1/me/avatar", Some(&ben), "image/png", png(), None).await;
    assert_eq!(send(&a, "GET", "/api/v1/me/avatar", Some(&anna), "application/json", vec![], None).await.status, StatusCode::NOT_FOUND);
    // Without a token nothing works.
    assert_eq!(send(&a, "PUT", "/api/v1/me/avatar", None, "image/png", png(), None).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(send(&a, "GET", "/api/v1/me/avatar", None, "application/json", vec![], None).await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_pin_changes_only_with_the_right_current_one() {
    let a = api().await;
    let (id, anna) = member(&a, "Anna", 1).await;
    a.store.set_pin(id, "Start1234").await.unwrap();
    let wrong = json_call(&a, "PUT", "/api/v1/me/pin", &anna, json!({ "current": "Nope12345", "new": "Neu123456" })).await;
    assert_eq!((wrong.status, wrong.json()["error"].as_str()), (StatusCode::FORBIDDEN, Some("wrong_pin")));
    let weak = json_call(&a, "PUT", "/api/v1/me/pin", &anna, json!({ "current": "Start1234", "new": "abc" })).await;
    assert_eq!((weak.status, weak.json()["error"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_pin")));
    let ok = json_call(&a, "PUT", "/api/v1/me/pin", &anna, json!({ "current": "Start1234", "new": "Neu123456" })).await;
    assert_eq!(ok.status, StatusCode::NO_CONTENT);
    // The device keeps its session.
    assert_eq!(send(&a, "GET", "/api/v1/me", Some(&anna), "application/json", vec![], None).await.status, StatusCode::OK);
}

#[tokio::test]
async fn the_own_listening_can_be_hidden_unless_an_administrator_forbids_it() {
    let a = api().await;
    let (id, token) = member(&a, "Anna", 5).await;
    let me = json_call(&a, "GET", "/api/v1/me", &token, Value::Null).await.json();
    assert_eq!((me["share_listening"].as_bool(), me["may_hide_listening"].as_bool()), (Some(true), Some(true)));

    let hidden = json_call(&a, "PUT", "/api/v1/me", &token, json!({ "share_listening": false })).await;
    assert_eq!(hidden.status, StatusCode::OK);
    assert_eq!(hidden.json()["share_listening"], false);

    a.store.set_may_hide_listening(id, false).await.unwrap();
    let forced = json_call(&a, "GET", "/api/v1/me", &token, Value::Null).await.json();
    assert_eq!((forced["share_listening"].as_bool(), forced["may_hide_listening"].as_bool()), (Some(true), Some(false)));
    let refused = json_call(&a, "PUT", "/api/v1/me", &token, json!({ "share_listening": false })).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    let family = json_call(&a, "GET", "/api/v1/family/media-time?days=7", &token, Value::Null).await;
    assert_eq!(family.status, StatusCode::OK);
    assert_eq!(family.json()["days"], 7);
    assert_eq!(family.json()["members"][0]["me"], true);
}

#[tokio::test]
async fn a_paired_device_is_seen_when_it_pairs() {
    let a = api().await;
    let (_, token) = member(&a, "Anna", 6).await;
    let devices = json_call(&a, "GET", "/api/v1/me/devices", &token, Value::Null).await.json();
    let this = devices.as_array().unwrap().iter().find(|d| d["this"] == true).expect("this device");
    assert!(this["last_seen"].as_i64().is_some_and(|ms| ms > 0), "\"last seen\" is set at the pairing, not never: {this}");
}
