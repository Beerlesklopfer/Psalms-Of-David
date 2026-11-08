// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The pictures to look at (`/api/v1/pictures`): who sees which source, the previews the server makes, the old folders.

mod common;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use common::open;
use david_server::api::{ApiState, router};
use david_server::photo_settings::ProfilePhotoSettings;
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
    mime: String,
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

async fn call(a: &Api, req: Request<Body>) -> Reply {
    let res = a.router.clone().oneshot(req).await.unwrap();
    let (parts, body) = res.into_parts();
    let mime = parts.headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
    Reply { status: parts.status, mime, bytes: to_bytes(body, 32 * 1024 * 1024).await.unwrap().to_vec() }
}

async fn get(a: &Api, token: &str, uri: &str) -> Reply {
    call(a, Request::builder().uri(uri).header(header::AUTHORIZATION, format!("Bearer {token}")).body(Body::empty()).unwrap()).await
}

async fn pair(a: &Api, user: i64, n: u8) -> String {
    let e = a.store.create_enrollment(user, false, a.admin).await.unwrap();
    let body = json!({ "code": e.code, "device_name": "Phone", "public_key": key(n) }).to_string();
    let r = call(a, Request::builder().method("POST").uri("/api/v1/pair").header(header::CONTENT_TYPE, "application/json").body(Body::from(body)).unwrap()).await;
    assert_eq!(r.status, StatusCode::OK, "{}", String::from_utf8_lossy(&r.bytes));
    r.json()["token"].as_str().unwrap().to_owned()
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(w, h, image::Rgb([10, 120, 200])).write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn sha(b: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

/// Backs up a picture in one piece; its id.
async fn upload(a: &Api, token: &str, client: &str, bytes: &[u8]) -> i64 {
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/v1/photos/upload/{client}"))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "image/png")
        .header("x-sha256", sha(bytes))
        .header("x-size", bytes.len().to_string())
        .header("x-file-name", format!("{client}.png"))
        .header("x-taken-at", "1773576000000")
        .body(Body::from(bytes.to_vec()))
        .unwrap();
    let r = call(a, req).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", String::from_utf8_lossy(&r.bytes));
    r.json()["photo"].as_i64().unwrap()
}

fn ids(v: &Value) -> Vec<String> {
    v["sources"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap().to_owned()).collect()
}

#[tokio::test]
async fn everybody_sees_their_own_the_parents_a_child_that_allows_it_and_the_adults_the_old_folders() {
    let a = api().await;
    let root = tempfile::tempdir().unwrap();
    // The family's pictures: an old folder, and the picture folders of two profiles inside (they are no old folders).
    std::fs::create_dir_all(root.path().join("Allgäu2017/Tag 1")).unwrap();
    std::fs::write(root.path().join("Allgäu2017/Tag 1/berg.png"), png(3000, 2000)).unwrap();
    std::fs::write(root.path().join("Allgäu2017/notes.txt"), b"no picture").unwrap();
    for f in ["Anna", "Kid"] {
        std::fs::create_dir_all(root.path().join(f)).unwrap();
    }
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let mut always = david_server::Schedule::none();
    always.allow_every_day(0, 24 * 60);
    let kid = a.store.create_user(NewProfile { name: "Kid".into(), role: Role::Child, max_age: Some(10), schedule: Some(always) }).await.unwrap();
    for (id, f) in [(anna, "Anna"), (kid, "Kid")] {
        a.store.set_profile_folder(id, "photo", Some(root.path().join(f).to_str().unwrap())).await.unwrap();
        let on = ProfilePhotoSettings { enabled: true, ..ProfilePhotoSettings::default() };
        a.store.save_profile_photo_settings(id, &on).await.unwrap();
    }
    a.store
        .save_settings(&[
            ("photo_sync".into(), "on".into()),
            ("time_zone".into(), "UTC".into()),
            ("photo_parents_may_view".into(), "on".into()),
            ("photo_places".into(), "off".into()),
            ("photo_library".into(), root.path().to_str().unwrap().into()),
        ])
        .await
        .unwrap();
    let (boss, anna_t, kid_t) = (pair(&a, a.admin, 1).await, pair(&a, anna, 2).await, pair(&a, kid, 3).await);
    let anna_pic = upload(&a, &anna_t, "anna-1", &png(800, 600)).await;
    let kid_pic = upload(&a, &kid_t, "kid-1", &png(640, 480)).await;
    assert_eq!(a.store.scan_photo_library().await.unwrap(), (1, 0), "one picture in the old folder, the backups are not old folders");

    let v = get(&a, &boss, "/api/v1/pictures/sources").await.json();
    assert_eq!(ids(&v), vec!["f:Allgäu2017"], "the child does not allow it yet: {v}");
    assert_eq!(v["places"], false);
    assert_eq!(ids(&get(&a, &anna_t, "/api/v1/pictures/sources").await.json()), vec!["own", "f:Allgäu2017"]);
    assert_eq!(ids(&get(&a, &kid_t, "/api/v1/pictures/sources").await.json()), vec!["own"], "a child sees no old folder");

    let list = get(&a, &anna_t, "/api/v1/pictures").await.json();
    let keys: Vec<&str> = list["pictures"].as_array().unwrap().iter().map(|p| p["key"].as_str().unwrap()).collect();
    assert_eq!(keys.len(), 2, "{list}");
    let old = keys.iter().find(|k| k.starts_with('l')).unwrap().to_string();
    assert!(keys.contains(&format!("b{anna_pic}").as_str()));
    let only = get(&a, &anna_t, "/api/v1/pictures?source=f%3AAllg%C3%A4u2017").await.json();
    assert_eq!(only["pictures"].as_array().unwrap().len(), 1);
    assert_eq!(only["pictures"][0]["name"], "berg.png");
    // The backup was taken on 2026-03-15: a time range before it leaves only the old picture (its file time is now) out too.
    let none = get(&a, &anna_t, "/api/v1/pictures?source=own&to=1000").await.json();
    assert_eq!(none["pictures"], json!([]));

    // The server makes the previews: smaller, a JPEG.
    let r = get(&a, &anna_t, &format!("/api/v1/pictures/{old}/screen")).await;
    assert_eq!((r.status, r.mime.as_str()), (StatusCode::OK, "image/jpeg"));
    let img = image::load_from_memory(&r.bytes).unwrap();
    assert_eq!((img.width(), img.height()), (2048, 1365));
    let r = get(&a, &anna_t, &format!("/api/v1/pictures/{old}/thumb")).await;
    let img = image::load_from_memory(&r.bytes).unwrap();
    assert_eq!((img.width(), img.height()), (480, 320));
    let r = get(&a, &anna_t, &format!("/api/v1/pictures/b{anna_pic}/file")).await;
    assert_eq!((r.status, r.mime.as_str()), (StatusCode::OK, "image/png"));
    let r = get(&a, &anna_t, &format!("/api/v1/pictures/b{anna_pic}/info")).await;
    assert_eq!(r.json(), json!({ "taken_at": 1_773_576_000_000_i64, "place": null, "at": r.json()["at"] }));

    // Somebody else's picture is not found.
    assert_eq!(get(&a, &boss, &format!("/api/v1/pictures/b{anna_pic}/thumb")).await.status, StatusCode::NOT_FOUND);
    assert_eq!(get(&a, &kid_t, &format!("/api/v1/pictures/{old}/thumb")).await.status, StatusCode::NOT_FOUND);
    assert_eq!(get(&a, &boss, &format!("/api/v1/pictures/b{kid_pic}/screen")).await.status, StatusCode::NOT_FOUND);

    // The child allows it: the parents see its pictures.
    let allow = ProfilePhotoSettings { enabled: true, parent_view: true, ..ProfilePhotoSettings::default() };
    a.store.save_profile_photo_settings(kid, &allow).await.unwrap();
    assert_eq!(ids(&get(&a, &boss, "/api/v1/pictures/sources").await.json()), vec![format!("p{kid}"), "f:Allgäu2017".to_owned()]);
    assert_eq!(get(&a, &boss, &format!("/api/v1/pictures/b{kid_pic}/screen")).await.status, StatusCode::OK);
    assert_eq!(get(&a, &anna_t, &format!("/api/v1/pictures/b{kid_pic}/screen")).await.status, StatusCode::NOT_FOUND, "a member is no parent");
    // ... unless the server takes it back.
    a.store.save_settings(&[("photo_parents_may_view".into(), "off".into())]).await.unwrap();
    assert_eq!(get(&a, &boss, &format!("/api/v1/pictures/b{kid_pic}/screen")).await.status, StatusCode::NOT_FOUND);

    // A file that is gone is dropped at the next reading.
    std::fs::remove_file(root.path().join("Allgäu2017/Tag 1/berg.png")).unwrap();
    assert_eq!(a.store.scan_photo_library().await.unwrap(), (0, 1));
    assert_eq!(get(&a, &anna_t, &format!("/api/v1/pictures/{old}/thumb")).await.status, StatusCode::NOT_FOUND);
    // Without the setting there are no old folders.
    a.store.save_settings(&[("photo_library".into(), String::new())]).await.unwrap();
    assert_eq!(ids(&get(&a, &anna_t, "/api/v1/pictures/sources").await.json()), vec!["own"]);
}
