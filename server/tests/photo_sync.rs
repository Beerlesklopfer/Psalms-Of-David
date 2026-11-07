// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The photo backup of the apps (`/api/v1/photos`): the settings of the profile within the rules of the server.

mod common;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
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
    Reply { status: parts.status, bytes: to_bytes(body, 8 * 1024 * 1024).await.unwrap().to_vec() }
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

async fn member(a: &Api, name: &str, n: u8) -> (i64, String) {
    let id = a.store.create_user(NewProfile::new(name, Role::Member)).await.unwrap();
    (id, pair(a, id, n).await)
}

#[tokio::test]
async fn the_app_reads_and_sets_the_photo_backup_within_the_rules_of_the_server() {
    let a = api().await;
    let (anna, token) = member(&a, "Anna", 1).await;
    // Off on the server: the app offers nothing.
    let r = send(&a, "GET", "/api/v1/photos/settings", Some(&token), "application/json", vec![], None).await;
    assert_eq!((r.status, r.json()["error"].clone()), (StatusCode::NOT_FOUND, json!("photo_sync_off")));
    a.store.save_settings(&[("photo_sync".into(), "on".into())]).await.unwrap();

    let r = send(&a, "GET", "/api/v1/photos/settings", Some(&token), "application/json", vec![], None).await;
    assert_eq!(r.status, StatusCode::OK);
    let v = r.json();
    assert_eq!(v["settings"]["enabled"], "off");
    assert_eq!(v["active"], false);
    assert!(v["settings"].get("parent_view").is_none(), "only a child has it");
    assert_eq!(v["locked"]["enabled"], json!([{ "option": "on", "why": "no_folder" }]), "{v}");
    assert_eq!(v["locked"]["mode"], json!([{ "option": "bi", "why": "no_bisync" }]));

    let folder = tempfile::tempdir().unwrap();
    a.store.set_profile_folder(anna, "photo", Some(folder.path().to_str().unwrap())).await.unwrap();
    let r = json_call(&a, "PUT", "/api/v1/photos/settings", &token, json!({ "enabled": "on", "mode": "bi" })).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(r.json(), json!({ "error": "refused", "refused": { "mode": "no_bisync" } }));
    assert!(!a.store.profile_photo_settings(anna).await.unwrap().enabled, "nothing is saved");

    a.store.save_settings(&[("photo_modes".into(), "both".into())]).await.unwrap();
    let r = json_call(&a, "PUT", "/api/v1/photos/settings", &token, json!({ "enabled": "on", "mode": "bi", "deletes": "trash", "network": "any" })).await;
    assert_eq!(r.status, StatusCode::OK, "{}", String::from_utf8_lossy(&r.bytes));
    let v = r.json();
    assert_eq!((v["active"].clone(), v["effective"]["mode"].clone(), v["effective"]["deletes"].clone()), (json!(true), json!("bi"), json!("trash")));
    // The server takes bisync back: the choice stays stored, but counts as the backup only.
    a.store.save_settings(&[("photo_modes".into(), "uni".into())]).await.unwrap();
    let v = send(&a, "GET", "/api/v1/photos/settings", Some(&token), "application/json", vec![], None).await.json();
    assert_eq!((v["settings"]["mode"].clone(), v["effective"]["mode"].clone(), v["effective"]["deletes"].clone()), (json!("bi"), json!("uni"), json!("never")));
}

fn sha(b: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

/// One piece of an upload.
#[allow(clippy::too_many_arguments)]
async fn piece(a: &Api, token: &str, client: &str, whole: &[u8], start: usize, end: usize, name: &str, mime: &str) -> Reply {
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/v1/photos/upload/{client}"))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, mime)
        .header("x-sha256", sha(whole))
        .header("x-size", whole.len().to_string())
        .header("x-file-name", name)
        // 2026-03-15 12:00 UTC: the picture goes into 2026/03.
        .header("x-taken-at", "1773576000000")
        .header(header::CONTENT_RANGE, format!("bytes {start}-{}/{}", end.saturating_sub(1), whole.len()))
        .body(Body::from(whole[start..end].to_vec()))
        .unwrap();
    let res = a.router.clone().oneshot(req).await.unwrap();
    let (parts, body) = res.into_parts();
    Reply { status: parts.status, bytes: to_bytes(body, 8 * 1024 * 1024).await.unwrap().to_vec() }
}

async fn get_json(a: &Api, token: &str, uri: &str) -> Reply {
    send(a, "GET", uri, Some(token), "application/json", vec![], None).await
}

#[tokio::test]
async fn a_picture_is_sent_in_pieces_checked_kept_once_and_found_by_the_other_devices() {
    let a = api().await;
    let (anna, phone) = member(&a, "Anna", 1).await;
    let tablet = pair(&a, anna, 2).await;
    let folder = tempfile::tempdir().unwrap();
    a.store.set_profile_folder(anna, "photo", Some(folder.path().to_str().unwrap())).await.unwrap();
    a.store.save_settings(&[("photo_sync".into(), "on".into()), ("photo_modes".into(), "both".into()), ("time_zone".into(), "UTC".into())]).await.unwrap();
    let img: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();

    // Not switched on yet: refused.
    let r = piece(&a, &phone, "img-1", &img, 0, img.len(), "IMG_0001.jpg", "image/jpeg").await;
    assert_eq!((r.status, r.json()["error"].clone()), (StatusCode::CONFLICT, json!("backup_off")));
    let r = json_call(&a, "PUT", "/api/v1/photos/settings", &phone, json!({ "enabled": "on", "mode": "bi" })).await;
    assert_eq!(r.status, StatusCode::OK, "{}", String::from_utf8_lossy(&r.bytes));

    // The server lacks it.
    let r = json_call(&a, "POST", "/api/v1/photos/have", &phone, json!({ "files": [{ "sha256": sha(&img), "size": img.len() }] })).await;
    assert_eq!(r.json()["missing"], json!([sha(&img)]));

    // First piece, then a lost connection: the device asks where to go on, and a piece from the wrong place is refused with the place.
    let r = piece(&a, &phone, "img-1", &img, 0, 100_000, "DCIM/Camera/IMG_0001.jpg", "image/jpeg").await;
    assert_eq!((r.status, r.json()), (StatusCode::ACCEPTED, json!({ "received": 100_000 })));
    let r = get_json(&a, &phone, "/api/v1/photos/upload/img-1").await;
    assert_eq!(r.json(), json!({ "sha256": sha(&img), "received": 100_000 }));
    let r = piece(&a, &phone, "img-1", &img, 200_000, 300_000, "IMG_0001.jpg", "image/jpeg").await;
    assert_eq!((r.status, r.json()), (StatusCode::CONFLICT, json!({ "error": "wrong_offset", "received": 100_000 })));
    let r = piece(&a, &phone, "img-1", &img, 100_000, 300_000, "IMG_0001.jpg", "image/jpeg").await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", String::from_utf8_lossy(&r.bytes));
    let id = r.json()["photo"].as_i64().unwrap();
    let kept = folder.path().join("2026/03/IMG_0001.jpg");
    assert_eq!(std::fs::read(&kept).unwrap(), img, "in its month, under its own name, byte for byte");
    assert!(std::fs::read_dir(folder.path().join(".david-upload")).unwrap().next().is_none(), "no bytes of the upload are left");

    // Known now: nothing to send, from either device.
    let r = json_call(&a, "POST", "/api/v1/photos/have", &tablet, json!({ "files": [{ "sha256": sha(&img), "size": img.len() }] })).await;
    assert_eq!(r.json()["missing"], json!([]));
    let r = piece(&a, &tablet, "x", &img, 0, img.len(), "IMG_0001.jpg", "image/jpeg").await;
    assert_eq!((r.status, r.json()), (StatusCode::OK, json!({ "photo": id, "known": true })));

    // Another picture of the same name is kept next to it, never over it.
    let other: Vec<u8> = vec![7; 1000];
    let r = piece(&a, &tablet, "img-2", &other, 0, other.len(), "IMG_0001.jpg", "image/jpeg").await;
    assert_eq!(r.status, StatusCode::CREATED);
    let second = r.json()["photo"].as_i64().unwrap();
    assert_eq!(std::fs::read(folder.path().join("2026/03/IMG_0001 (2).jpg")).unwrap(), other);

    // Bytes that are not the announced ones: refused, and the upload starts over.
    let req = Request::builder()
        .method("PUT")
        .uri("/api/v1/photos/upload/bad")
        .header(header::AUTHORIZATION, format!("Bearer {phone}"))
        .header(header::CONTENT_TYPE, "image/png")
        .header("x-sha256", sha(b"something else"))
        .header("x-size", "4")
        .body(Body::from(b"abcd".to_vec()))
        .unwrap();
    let r = a.router.clone().oneshot(req).await.unwrap();
    assert_eq!(r.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(get_json(&a, &phone, "/api/v1/photos/upload/bad").await.status, StatusCode::NOT_FOUND);

    // Bisync: each device gets what the other sent, not its own.
    let r = get_json(&a, &phone, "/api/v1/photos/changes?since=0").await.json();
    assert_eq!(r["photos"].as_array().unwrap().iter().map(|p| p["id"].as_i64().unwrap()).collect::<Vec<_>>(), vec![second], "{r}");
    assert_eq!(r["next"], json!(second));
    let r = get_json(&a, &tablet, "/api/v1/photos/changes?since=0").await.json();
    assert_eq!(r["photos"][0]["id"], json!(id));
    assert_eq!(r["photos"][0]["name"], json!("IMG_0001.jpg"));

    // The file, for the own person only.
    let r = get_json(&a, &tablet, &format!("/api/v1/photos/{id}/file")).await;
    assert_eq!((r.status, r.bytes.len()), (StatusCode::OK, img.len()));
    let (_, bert) = member(&a, "Bert", 3).await;
    assert_eq!(get_json(&a, &bert, &format!("/api/v1/photos/{id}/file")).await.status, StatusCode::NOT_FOUND);

    let r = get_json(&a, &phone, "/api/v1/photos/state").await.json();
    assert_eq!((r["count"].clone(), r["bytes"].clone(), r["active"].clone()), (json!(2), json!(img.len() + other.len()), json!(true)));

    // The preview: made by the device that sent the picture, a JPEG only, for the own pictures only; `changes` says whether there is one.
    let jpeg: Vec<u8> = [0xFF, 0xD8, 0xFF, 0xE0].into_iter().chain(std::iter::repeat_n(1u8, 2000)).collect();
    assert_eq!(get_json(&a, &tablet, &format!("/api/v1/photos/{id}/thumb")).await.json()["error"], json!("no_thumb"));
    assert_eq!(get_json(&a, &tablet, "/api/v1/photos/changes?since=0").await.json()["photos"][0]["thumb"], json!(false));
    let r = send(&a, "PUT", &format!("/api/v1/photos/{id}/thumb"), Some(&phone), "image/png", b"\x89PNG....".to_vec(), None).await;
    assert_eq!((r.status, r.json()["error"].clone()), (StatusCode::UNSUPPORTED_MEDIA_TYPE, json!("wrong_type")));
    let r = send(&a, "PUT", &format!("/api/v1/photos/{id}/thumb"), Some(&bert), "image/jpeg", jpeg.clone(), None).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "nobody puts a preview on the picture of another person");
    let r = send(&a, "PUT", &format!("/api/v1/photos/{id}/thumb"), Some(&phone), "image/jpeg", jpeg.clone(), None).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{}", String::from_utf8_lossy(&r.bytes));
    assert_eq!(get_json(&a, &tablet, &format!("/api/v1/photos/{id}/thumb")).await.bytes, jpeg);
    assert_eq!(get_json(&a, &bert, &format!("/api/v1/photos/{id}/thumb")).await.status, StatusCode::NOT_FOUND);
    assert_eq!(get_json(&a, &tablet, "/api/v1/photos/changes?since=0").await.json()["photos"][0]["thumb"], json!(true));
    assert!(folder.path().join(".david-thumbs").join(format!("{id}.jpg")).is_file());
}

#[tokio::test]
async fn videos_large_files_and_other_types_are_refused_by_the_rules() {
    let a = api().await;
    let (anna, phone) = member(&a, "Anna", 1).await;
    let folder = tempfile::tempdir().unwrap();
    a.store.set_profile_folder(anna, "photo", Some(folder.path().to_str().unwrap())).await.unwrap();
    a.store.save_settings(&[("photo_sync".into(), "on".into()), ("photo_max_file_mb".into(), "1".into())]).await.unwrap();
    json_call(&a, "PUT", "/api/v1/photos/settings", &phone, json!({ "enabled": "on" })).await;
    let small = vec![1u8; 10];
    let r = piece(&a, &phone, "doc", &small, 0, 10, "a.pdf", "application/pdf").await;
    assert_eq!((r.status, r.json()["error"].clone()), (StatusCode::UNSUPPORTED_MEDIA_TYPE, json!("wrong_type")));
    let big = vec![1u8; 1024 * 1024 + 1];
    let r = piece(&a, &phone, "big", &big, 0, 10, "a.jpg", "image/jpeg").await;
    assert_eq!((r.status, r.json()["error"].clone()), (StatusCode::PAYLOAD_TOO_LARGE, json!("too_large")));
    let r = piece(&a, &phone, "film", &small, 0, 10, "a.mp4", "video/mp4").await;
    assert_eq!(r.status, StatusCode::CREATED, "videos are on by default");
    json_call(&a, "PUT", "/api/v1/photos/settings", &phone, json!({ "videos": "off" })).await;
    let r = piece(&a, &phone, "film2", &[2u8; 10], 0, 10, "b.mp4", "video/mp4").await;
    assert_eq!((r.status, r.json()["error"].clone()), (StatusCode::UNSUPPORTED_MEDIA_TYPE, json!("wrong_type")));
    // A quota that is full.
    a.store.save_settings(&[("photo_quota_gb".into(), "1".into())]).await.unwrap();
    let r = piece(&a, &phone, "fits", &[3u8; 10], 0, 10, "c.jpg", "image/jpeg").await;
    assert_eq!(r.status, StatusCode::CREATED);
}
