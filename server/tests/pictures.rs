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

/// The slideshow of the browser: the page for whoever signs in (not a guest), and the same rules for what it loads.
#[tokio::test]
async fn the_browser_shows_the_page_and_loads_the_pictures_with_the_session() {
    let (data, store) = open().await;
    let admin = store.create_profile("Boss", true).await.unwrap();
    let anna = store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let guest = store.create_user(NewProfile::new("Gast", Role::Guest)).await.unwrap();
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("Hochzeit")).unwrap();
    std::fs::write(root.path().join("Hochzeit/kuss.png"), png(1200, 900)).unwrap();
    std::fs::create_dir_all(root.path().join("Urlaub")).unwrap();
    std::fs::write(root.path().join("Urlaub/strand.png"), png(3000, 2000)).unwrap();
    let ffmpeg = david_server::picture_render::has_ffmpeg();
    if ffmpeg {
        let ok = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-f", "lavfi", "-i", "testsrc=size=640x360:rate=10", "-t", "2", "-c:v", "mpeg4", "-y"])
            .arg(root.path().join("Urlaub/welle.avi"))
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok);
    }
    store.save_settings(&[("photo_library".into(), root.path().to_str().unwrap().into()), ("photo_places".into(), "off".into())]).await.unwrap();
    store.scan_photo_library().await.unwrap();
    let router = web_router(&store, data.path());
    let get = |uri: String, cookie: String| {
        let router = router.clone();
        async move {
            let req = Request::builder().uri(uri).header(header::COOKIE, cookie).header(header::ACCEPT_LANGUAGE, "de").body(Body::empty()).unwrap();
            let res = router.oneshot(req).await.unwrap();
            let (parts, body) = res.into_parts();
            (parts.status, parts.headers, to_bytes(body, 32 * 1024 * 1024).await.unwrap().to_vec())
        }
    };
    let cookie = |id: i64, role: Role| {
        let store = store.clone();
        async move { format!("david_session={}", store.create_session(id, role, None).await.unwrap()) }
    };
    let (anna_c, guest_c, boss_c) = (cookie(anna, Role::Member).await, cookie(guest, Role::Guest).await, cookie(admin, Role::Admin).await);

    let (status, headers, body) = get("/pictures".into(), anna_c.clone()).await;
    let html = String::from_utf8_lossy(&body);
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.contains("value=\"f:Hochzeit\"") && html.contains("/admin/static/slideshow.js"), "{html}");
    assert!(headers.get(header::CONTENT_SECURITY_POLICY).unwrap().to_str().unwrap().contains("media-src 'self'"), "videos and music may play");
    assert_eq!(get("/pictures".into(), guest_c.clone()).await.0, StatusCode::NOT_FOUND);
    assert_eq!(get("/pictures/list".into(), guest_c.clone()).await.0, StatusCode::NOT_FOUND);
    let boss = String::from_utf8_lossy(&get("/pictures".into(), boss_c).await.2).into_owned();
    assert!(boss.contains("f:Hochzeit"), "{boss}");
    // An administrator finds the pictures under "Media", marked as the page they are on; a member in the menu of the person.
    let media = &boss[boss.find("popup-media").expect("the menu Media")..boss.find("popup-user").unwrap()];
    assert!(media.contains("href=\"/pictures\" aria-current=\"page\""), "{media}");
    assert!(html.contains("href=\"/pictures\" aria-current=\"page\""), "{html}");

    let (status, _, body) = get("/pictures/list?source=f%3AHochzeit".into(), anna_c.clone()).await;
    assert_eq!(status, StatusCode::OK);
    let list: Value = serde_json::from_slice(&body).unwrap();
    let key = list["pictures"][0]["key"].as_str().unwrap().to_owned();
    let (status, headers, body) = get(format!("/pictures/{key}/screen"), anna_c.clone()).await;
    assert_eq!((status, headers.get(header::CONTENT_TYPE).unwrap().to_str().unwrap()), (StatusCode::OK, "image/jpeg"));
    assert_eq!(image::load_from_memory(&body).unwrap().width(), 1200, "a small picture stays as large as it is");
    assert_eq!(get(format!("/pictures/{key}/screen"), guest_c.clone()).await.0, StatusCode::NOT_FOUND);
    assert_eq!(get(format!("/pictures/{key}/nonsense"), anna_c.clone()).await.0, StatusCode::NOT_FOUND);
    let (status, _, body) = get(format!("/pictures/{key}/info"), anna_c.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(serde_json::from_slice::<Value>(&body).unwrap()["place"], Value::Null);
    let (status, _, _) = get("/admin/static/slideshow.js".into(), anna_c.clone()).await;
    assert_eq!(status, StatusCode::OK);

    // The project: one ZIP that plays without the server, with the player, the pictures (screen size), the videos as MP4 and show.json.
    let (status, headers, body) = get("/pictures/project?source=f%3AUrlaub&videos=on&order=random&seconds=5&transition=zoom&info=both".into(), anna_c.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers.get(header::CONTENT_TYPE).unwrap(), "application/zip");
    assert!(headers.get(header::CONTENT_DISPOSITION).unwrap().to_str().unwrap().contains(".zip"));
    let entries = david_server::zipstream::list(&body).expect("a whole archive");
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    for needed in ["show.json", "show.js", "index.html", "slideshow.js", "slideshow.css", "pictures/0001.jpg"] {
        assert!(names.contains(&needed), "{needed} in {names:?}");
    }
    let entry = |name: &str| -> Vec<u8> {
        let e = entries.iter().find(|e| e.name == name).unwrap();
        let at = e.offset as usize;
        let n = u16::from_le_bytes([body[at + 26], body[at + 27]]) as usize;
        let x = u16::from_le_bytes([body[at + 28], body[at + 29]]) as usize;
        body[at + 30 + n + x..at + 30 + n + x + e.size as usize].to_vec()
    };
    let show: Value = serde_json::from_slice(&entry("show.json")).unwrap();
    assert_eq!((show["format"].as_str(), show["version"].as_u64(), show["lang"].as_str()), (Some("david-slideshow"), Some(1), Some("de")));
    assert_eq!(show["settings"], serde_json::json!({ "order": "random", "seconds": 5, "transition": "zoom", "videos": "on", "info": "both" }));
    let pictures = show["pictures"].as_array().unwrap();
    assert_eq!(pictures.len(), if ffmpeg { 2 } else { 1 }, "{show}");
    for p in pictures {
        assert!(names.contains(&p["image"].as_str().unwrap()), "every picture named is in the archive: {p}");
        assert!(p["taken_at"].as_i64().is_some() && p["place"].is_null(), "no places when the server names none: {p}");
    }
    let still = pictures.iter().find(|p| p["video"].is_null()).unwrap();
    assert_eq!(image::load_from_memory(&entry(still["image"].as_str().unwrap())).unwrap().width(), 2048, "the screen size");
    if ffmpeg {
        let clip = pictures.iter().find(|p| !p["video"].is_null()).unwrap();
        assert_eq!(&entry(clip["video"].as_str().unwrap())[4..8], b"ftyp", "the video as an MP4");
    }
    assert!(show["music"].as_array().unwrap().is_empty());
    let script = String::from_utf8(entry("show.js")).unwrap();
    assert!(script.starts_with("window.DAVID_SHOW = {"));
    let page = String::from_utf8(entry("index.html")).unwrap();
    assert!(page.contains("<html lang=\"de\">") && page.contains("Diese Diashow läuft ohne Server"), "{page}");
    assert_eq!(entry("slideshow.js"), std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/static/slideshow.js")).unwrap());
    // A guest gets none, and neither does a choice of nothing the person may see.
    assert_eq!(get("/pictures/project?source=f%3AUrlaub".into(), guest_c.clone()).await.0, StatusCode::NOT_FOUND);
    assert_eq!(get("/pictures/project?source=p999".into(), anna_c.clone()).await.0, StatusCode::NOT_FOUND);
    // The page offers it.
    let (_, _, body) = get("/pictures".into(), anna_c).await;
    assert!(String::from_utf8_lossy(&body).contains("id=\"show-save\""));
}

/// The pages of the server (the session of the browser), on this store.
fn web_router(store: &Store, data: &std::path::Path) -> Router {
    let cd = david_server::cdimport::CdService::new(data);
    david_server::web::router(david_server::web::AppState {
        store: store.clone(),
        mcp: false,
        listen: "127.0.0.1:8484".parse().unwrap(),
        fingerprint: None,
        ca_host: None,
        ui: std::sync::Arc::new(david_server::admin::pages::AdminUi::new().unwrap()),
        secure_cookie: false,
        library: None,
        uploads: david_server::upload::Uploads::start(store.clone(), data),
        cd,
        features: Default::default(),
    })
}

/// A request of the browser with the session [token]; a POST carries the CSRF token as the page sends it.
async fn browser(router: &Router, method: &str, uri: &str, token: &str, body: &str) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::COOKIE, format!("david_session={token}"))
        .header("x-csrf-token", david_server::auth::csrf_token(token))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::ACCEPT_LANGUAGE, "de")
        .body(Body::from(body.to_owned()))
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    let (parts, body) = res.into_parts();
    (parts.status, parts.headers, to_bytes(body, 256 * 1024 * 1024).await.unwrap().to_vec())
}

#[tokio::test]
async fn a_video_for_the_television_is_made_in_the_background_downloaded_and_deleted() {
    if !david_server::picture_render::has_ffmpeg() {
        eprintln!("skipped: no ffmpeg");
        return;
    }
    let (data, store) = open().await;
    let anna = store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let bert = store.create_user(NewProfile::new("Bert", Role::Member)).await.unwrap();
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("Urlaub")).unwrap();
    std::fs::write(root.path().join("Urlaub/strand.png"), png(3000, 2000)).unwrap();
    std::fs::write(root.path().join("Urlaub/turm.png"), png(900, 1600)).unwrap();
    let ok = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-f", "lavfi", "-i", "testsrc=size=640x360:rate=10", "-f", "lavfi", "-i", "sine=frequency=440", "-t", "2", "-c:v", "mpeg4", "-y"])
        .arg(root.path().join("Urlaub/welle.avi"))
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok);
    store.save_settings(&[("photo_library".into(), root.path().to_str().unwrap().into()), ("photo_places".into(), "off".into())]).await.unwrap();
    store.scan_photo_library().await.unwrap();
    let router = web_router(&store, data.path());
    let (anna_t, bert_t) = (store.create_session(anna, Role::Member, None).await.unwrap(), store.create_session(bert, Role::Member, None).await.unwrap());

    // Nothing to show: no video.
    let (status, _, _) = browser(&router, "POST", "/pictures/videos", &anna_t, "source=p999").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // Without the CSRF token the page cannot ask for one.
    let req = Request::builder().method("POST").uri("/pictures/videos").header(header::COOKIE, format!("david_session={anna_t}")).body(Body::from("source=f%3AUrlaub")).unwrap();
    assert_eq!(router.clone().oneshot(req).await.unwrap().status(), StatusCode::FORBIDDEN);

    let (status, _, body) = browser(&router, "POST", "/pictures/videos", &anna_t, "source=f%3AUrlaub&videos=on&seconds=3&transition=random&info=date").await;
    assert_eq!(status, StatusCode::CREATED, "{}", String::from_utf8_lossy(&body));
    let id = serde_json::from_slice::<Value>(&body).unwrap()["id"].as_i64().unwrap();
    let list: Value = serde_json::from_slice(&browser(&router, "GET", "/pictures/videos", &anna_t, "").await.2).unwrap();
    assert_eq!((list["videos"][0]["id"].as_i64(), list["videos"][0]["state"].as_str()), (Some(id), Some("queued")));
    assert!(list["videos"][0]["title"].as_str().unwrap().starts_with("Bilder "));
    // Another person sees none of it.
    let other: Value = serde_json::from_slice(&browser(&router, "GET", "/pictures/videos", &bert_t, "").await.2).unwrap();
    assert_eq!(other["videos"], json!([]));

    david_server::show_video::Videos::start(store.clone(), None);
    david_server::show_video::wake();
    let mut done = None;
    for _ in 0..600 {
        let v = store.show_video(anna, id).await.unwrap().unwrap();
        if v.state == "done" || v.state == "failed" {
            done = Some(v);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    let v = done.expect("the video is made within two minutes");
    assert_eq!((v.state.as_str(), v.error.as_deref(), v.pictures, v.progress), ("done", None, 3, 1000), "{v:?}");
    // Two pictures of 3 s plus a fade each and a video of about 2 s (its MP4 a little longer), overlapping by their transitions: from
    // about 8 s (all faded) to about 10 s (all cut).
    assert!((7_000..=10_500).contains(&v.duration_ms), "{v:?}");

    let (status, headers, file) = browser(&router, "GET", &format!("/pictures/videos/{id}/file"), &anna_t, "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers.get(header::CONTENT_TYPE).unwrap(), "video/mp4");
    assert!(headers.get(header::CONTENT_DISPOSITION).unwrap().to_str().unwrap().contains(".mp4"));
    assert_eq!(file.len() as i64, v.size_bytes);
    let out = data.path().join("check.mp4");
    if let Ok(keep) = std::env::var("DAVID_VIDEO_OUT") { std::fs::write(keep, &file).unwrap(); }
    std::fs::write(&out, &file).unwrap();
    let probe = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "stream=codec_name,width,height,duration", "-of", "csv=p=0"])
        .arg(&out)
        .output()
        .unwrap();
    let probe = String::from_utf8_lossy(&probe.stdout);
    assert!(probe.contains("h264,1920,1080") && probe.contains("aac"), "Full HD with the sound of the video: {probe}");
    // The picture lasts as long as the sound: every piece is in it (a transition that drops the next piece would leave it short).
    let picture = probe.lines().find(|l| l.starts_with("h264")).and_then(|l| l.rsplit(',').next()).and_then(|d| d.parse::<f64>().ok()).unwrap();
    assert!((picture * 1000.0 - v.duration_ms as f64).abs() < 200.0, "{picture} s of picture for {} ms: {probe}", v.duration_ms);
    assert_eq!(browser(&router, "GET", &format!("/pictures/videos/{id}/file"), &bert_t, "").await.0, StatusCode::NOT_FOUND);
    assert_eq!(browser(&router, "POST", &format!("/pictures/videos/{id}/delete"), &bert_t, "").await.0, StatusCode::NOT_FOUND);

    assert!(david_server::show_video::file_of(data.path(), id).is_file());
    assert_eq!(browser(&router, "POST", &format!("/pictures/videos/{id}/delete"), &anna_t, "").await.0, StatusCode::NO_CONTENT);
    assert!(!david_server::show_video::file_of(data.path(), id).exists());
    let list: Value = serde_json::from_slice(&browser(&router, "GET", "/pictures/videos", &anna_t, "").await.2).unwrap();
    assert_eq!(list["videos"], json!([]));
    let page = String::from_utf8_lossy(&browser(&router, "GET", "/pictures", &anna_t, "").await.2).into_owned();
    assert!(page.contains("id=\"show-video\"") && page.contains("Videos für den Fernseher"), "{page}");
}

/// A request of the browser that sends JSON, as the editors of a project do.
async fn browser_json(router: &Router, uri: &str, token: &str, body: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::COOKIE, format!("david_session={token}"))
        .header("x-csrf-token", david_server::auth::csrf_token(token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = to_bytes(res.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

/// A project put together by hand (docs/spec-pictures.md, 9): picked in the preview, put in order by numbers, each picture with its own
/// transition and seconds, the properties as the start; it plays, is saved as a file and asked for as a video like a choice, and only
/// the person who made it reaches it.
#[tokio::test]
async fn a_project_is_picked_put_in_order_and_played_only_by_the_person_who_made_it() {
    let (data, store) = open().await;
    let anna = store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let bert = store.create_user(NewProfile::new("Bert", Role::Member)).await.unwrap();
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("Urlaub")).unwrap();
    for name in ["a.png", "b.png", "c.png"] {
        std::fs::write(root.path().join("Urlaub").join(name), png(800, 600)).unwrap();
    }
    store.save_settings(&[("photo_library".into(), root.path().to_str().unwrap().into()), ("photo_places".into(), "off".into())]).await.unwrap();
    store.scan_photo_library().await.unwrap();
    let router = web_router(&store, data.path());
    let (anna_t, bert_t) = (store.create_session(anna, Role::Member, None).await.unwrap(), store.create_session(bert, Role::Member, None).await.unwrap());
    let get_json = |uri: String, token: String| {
        let router = router.clone();
        async move {
            let (status, _, body) = browser(&router, "GET", &uri, &token, "").await;
            (status, serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null))
        }
    };
    let keys_of = |v: &Value| -> Vec<String> { v["pictures"].as_array().unwrap().iter().map(|p| p["key"].as_str().unwrap().to_owned()).collect() };
    let all = keys_of(&get_json("/pictures/list?source=f%3AUrlaub".into(), anna_t.clone()).await.1);
    assert_eq!(all.len(), 3);
    let (k1, k2, k3) = (all[0].clone(), all[1].clone(), all[2].clone());

    // A new project needs a title; then the preview opens.
    let (status, headers, _) = browser(&router, "POST", "/pictures/shows", &anna_t, "title=+").await;
    assert_eq!((status, headers.get(header::LOCATION).unwrap().to_str().unwrap()), (StatusCode::SEE_OTHER, "/pictures?msg=err.show_title"));
    let (status, headers, _) = browser(&router, "POST", "/pictures/shows", &anna_t, "title=Allg%C3%A4u").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let to = headers.get(header::LOCATION).unwrap().to_str().unwrap().to_owned();
    let id: i64 = to.trim_start_matches("/pictures/shows/").trim_end_matches("/pick").parse().unwrap();
    let base = format!("/pictures/shows/{id}");
    let (status, _, page) = browser(&router, "GET", &to, &anna_t, "").await;
    let page = String::from_utf8_lossy(&page).into_owned();
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains(&format!("data-show=\"{id}\"")) && page.contains("value=\"f:Urlaub\"") && page.contains("/admin/static/show-editor.js"), "{page}");
    assert_eq!(browser(&router, "GET", &to, &bert_t, "").await.0, StatusCode::NOT_FOUND, "another person does not reach it");
    assert_eq!(browser(&router, "GET", "/admin/static/show-editor.js", &anna_t, "").await.0, StatusCode::OK);

    // Picked: only what the person may see, not one twice, at the end in the order sent.
    let (status, v) = browser_json(&router, &format!("{base}/add"), &anna_t, json!({ "keys": [k3, k1, "b999", "l99999", k3] })).await;
    assert_eq!((status, v["count"].as_i64()), (StatusCode::OK, Some(2)));
    assert_eq!(browser_json(&router, &format!("{base}/add"), &bert_t, json!({ "keys": [k2] })).await.0, StatusCode::NOT_FOUND);
    assert_eq!(browser_json(&router, &format!("{base}/add"), &anna_t, json!({ "keys": [k2, k1] })).await.1["count"].as_i64(), Some(3));
    let page = String::from_utf8_lossy(&browser(&router, "GET", &to, &anna_t, "").await.2).into_owned();
    assert!(page.contains(k3.as_str()) && page.contains(k2.as_str()), "the preview knows what is in the project: {page}");

    // The order by numbers, a picture with its own transition and seconds, the properties.
    assert_eq!(browser_json(&router, &format!("{base}/order"), &anna_t, json!({ "keys": [k2, k3, "x"] })).await.0, StatusCode::OK);
    assert_eq!(browser_json(&router, &format!("{base}/picture"), &anna_t, json!({ "key": k3, "transition": "cut", "seconds": "3" })).await.0, StatusCode::NO_CONTENT);
    assert_eq!(browser_json(&router, &format!("{base}/picture"), &anna_t, json!({ "key": k3, "transition": "random" })).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(browser_json(&router, &format!("{base}/picture"), &anna_t, json!({ "key": k3, "seconds": "4" })).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(browser_json(&router, &format!("{base}/picture"), &anna_t, json!({ "key": "b999" })).await.0, StatusCode::NOT_FOUND);
    let foreign = store.create_playlist(bert, "Bert hört").await.unwrap();
    let props = format!("title=Allg%C3%A4u+2017&ordering=numbers&seconds=5&transition=zoom&videos=on&info=date&music={foreign}");
    assert_eq!(browser(&router, "POST", &base, &anna_t, &props).await.0, StatusCode::NO_CONTENT);
    assert_eq!(browser(&router, "POST", &base, &bert_t, &props).await.0, StatusCode::NOT_FOUND);
    assert_eq!(browser(&router, "POST", &base, &anna_t, "title=x&ordering=date").await.0, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, v) = get_json(format!("/pictures/list?show={id}"), anna_t.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(keys_of(&v), [k2.clone(), k3.clone(), k1.clone()]);
    assert_eq!((v["pictures"][1]["transition"].as_str(), v["pictures"][1]["seconds"].as_u64()), (Some("cut"), Some(3)));
    assert_eq!((v["pictures"][0]["transition"].clone(), v["pictures"][0]["seconds"].clone()), (Value::Null, Value::Null));
    assert_eq!(v["settings"], json!({ "order": "date", "seconds": 5, "transition": "zoom", "videos": "on", "info": "date" }));
    assert_eq!((v["title"].as_str(), v["music"].clone()), (Some("Allgäu 2017"), Value::Null), "a playlist of another person is no music");
    assert_eq!(get_json(format!("/pictures/list?show={id}"), bert_t.clone()).await.0, StatusCode::NOT_FOUND);
    assert_eq!(get_json("/pictures/list?show=x".into(), anna_t.clone()).await.0, StatusCode::NOT_FOUND);

    // The editor: every picture with its number and its own choices; the list of /pictures links it.
    let (status, _, page) = browser(&router, "GET", &base, &anna_t, "").await;
    let page = String::from_utf8_lossy(&page).into_owned();
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page.matches("class=\"edit-item\"").count(), 3, "{page}");
    assert!(page.contains("value=\"3\" inputmode=\"numeric\"") && page.contains("<option value=\"cut\" selected>"), "{page}");
    assert!(page.contains("id=\"edit-start\"") && page.contains(&format!("/pictures/project?show={id}")), "{page}");
    let list = String::from_utf8_lossy(&browser(&router, "GET", "/pictures", &anna_t, "").await.2).into_owned();
    assert!(list.contains(&format!("href=\"{base}\"")) && list.contains("Allgäu 2017"), "{list}");
    let other = String::from_utf8_lossy(&browser(&router, "GET", "/pictures", &bert_t, "").await.2).into_owned();
    assert!(!other.contains("Allgäu 2017"), "{other}");

    // The project file holds the order, the title and the transition and seconds of each picture.
    let (status, headers, body) = browser(&router, "GET", &format!("/pictures/project?show={id}"), &anna_t, "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers.get(header::CONTENT_DISPOSITION).unwrap().to_str().unwrap().contains("2017.zip"), "{headers:?}");
    let entries = david_server::zipstream::list(&body).expect("a whole archive");
    let e = entries.iter().find(|e| e.name == "show.json").unwrap();
    let at = e.offset as usize;
    let (n, x) = (u16::from_le_bytes([body[at + 26], body[at + 27]]) as usize, u16::from_le_bytes([body[at + 28], body[at + 29]]) as usize);
    let show: Value = serde_json::from_slice(&body[at + 30 + n + x..at + 30 + n + x + e.size as usize]).unwrap();
    assert_eq!(show["title"].as_str(), Some("Allgäu 2017"));
    assert_eq!(show["pictures"].as_array().unwrap().len(), 3);
    assert_eq!((show["pictures"][1]["transition"].as_str(), show["pictures"][1]["seconds"].as_u64()), (Some("cut"), Some(3)));
    assert!(show["pictures"][0].get("transition").is_none(), "{show}");
    assert_eq!(browser(&router, "GET", &format!("/pictures/project?show={id}"), &bert_t, "").await.0, StatusCode::NOT_FOUND);

    // A video of the project is asked for with its title; not by another person.
    let (status, _, body) = browser(&router, "POST", "/pictures/videos", &anna_t, &format!("show={id}")).await;
    assert_eq!(status, StatusCode::CREATED, "{}", String::from_utf8_lossy(&body));
    let videos = get_json("/pictures/videos".into(), anna_t.clone()).await.1;
    assert_eq!(videos["videos"][0]["title"].as_str(), Some("Allgäu 2017"));
    assert_eq!(browser(&router, "POST", "/pictures/videos", &bert_t, &format!("show={id}")).await.0, StatusCode::NOT_FOUND);

    // Taken out, the others close up; deleted, it is gone, the pictures stay.
    assert_eq!(browser_json(&router, &format!("{base}/remove"), &anna_t, json!({ "keys": [k2] })).await.1["count"].as_i64(), Some(2));
    assert_eq!(keys_of(&get_json(format!("/pictures/list?show={id}"), anna_t.clone()).await.1), [k3.clone(), k1.clone()]);
    assert_eq!(store.slideshow_pictures(id).await.unwrap().iter().map(|p| p.position).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(browser(&router, "POST", &format!("{base}/delete"), &bert_t, "").await.0, StatusCode::NOT_FOUND);
    let (status, headers, _) = browser(&router, "POST", &format!("{base}/delete"), &anna_t, "").await;
    assert_eq!((status, headers.get(header::LOCATION).unwrap().to_str().unwrap()), (StatusCode::SEE_OTHER, "/pictures?msg=ok.show_deleted"));
    assert_eq!(get_json(format!("/pictures/list?show={id}"), anna_t.clone()).await.0, StatusCode::NOT_FOUND);
    assert_eq!(keys_of(&get_json("/pictures/list?source=f%3AUrlaub".into(), anna_t).await.1).len(), 3);
}
