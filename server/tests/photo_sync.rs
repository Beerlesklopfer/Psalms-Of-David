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
