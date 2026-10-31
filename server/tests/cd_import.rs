// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The CD import of the apps: lookup (MusicBrainz and cover through a fake, no internet), jobs, the upload of ripped
//! files, the settings, and the event channel (a real socket on localhost, a hand-made WebSocket client).

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use common::open;
use david_server::api::{ApiState, router};
use david_server::cdimport::CdService;
use david_server::musicbrainz::{Lookup, LookupError, Service};
use david_server::{NewProfile, Role, Schedule, Store};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tower::ServiceExt;

const DISC_ID: &str = "AzCi4YfdjUrdmGcBX7URtQixDmE-";
const RELEASE_ID: &str = "0f2d4b1c-8d0a-4f17-8d7e-111111111111";

/// The internet, replaced: answers by a part of the address, and writes down what was asked.
#[derive(Default)]
struct Fake {
    answers: Mutex<Vec<(String, Option<Vec<u8>>)>>,
    asked: Mutex<Vec<(Service, String, String)>>,
}

impl Fake {
    fn on(&self, part: &str, body: Option<&[u8]>) {
        self.answers.lock().unwrap().push((part.to_owned(), body.map(<[u8]>::to_vec)));
    }
    fn asked(&self) -> Vec<(Service, String, String)> {
        self.asked.lock().unwrap().clone()
    }
}

impl Lookup for Fake {
    fn get(&self, service: Service, url: &str, user_agent: &str, _max: usize) -> Result<Option<Vec<u8>>, LookupError> {
        self.asked.lock().unwrap().push((service, url.to_owned(), user_agent.to_owned()));
        for (part, body) in self.answers.lock().unwrap().iter() {
            if url.contains(part.as_str()) {
                return Ok(body.clone());
            }
        }
        Err(LookupError::Failed("no answer prepared".into()))
    }
}

struct Env {
    router: Router,
    store: Store,
    fake: Arc<Fake>,
    library: tempfile::TempDir,
    dir: tempfile::TempDir,
    admin: i64,
    n: u8,
}

fn key(n: u8) -> String {
    let letters = b"AEIMQUYcgkosw048";
    format!("{}{}=", "B".repeat(42), letters[n as usize % letters.len()] as char)
}

async fn env_with(heartbeat: Option<Duration>) -> Env {
    let (dir, store) = open().await;
    let admin = store.create_profile("Boss", true).await.unwrap();
    let library = tempfile::TempDir::new().unwrap();
    let fake = Arc::new(Fake::default());
    let mut cd = CdService::with_lookup(dir.path(), fake.clone());
    if let Some(h) = heartbeat {
        cd = cd.with_heartbeat(h);
    }
    // The tests never want to wait for the reserve of free space.
    store.save_settings(&[("min_free_gb".to_owned(), "0".to_owned())]).await.unwrap();
    let state = ApiState { store: store.clone(), library: Some(library.path().to_owned()), cd, features: Default::default() };
    Env { router: router(state), store, fake, library, dir, admin, n: 0 }
}

async fn env() -> Env {
    env_with(None).await
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
    fn code(&self) -> String {
        self.json()["error"].as_str().unwrap_or("").to_owned()
    }
}

async fn send(e: &Env, req: Request<Body>) -> Reply {
    let res = e.router.clone().oneshot(req).await.unwrap();
    let (parts, body) = res.into_parts();
    Reply { status: parts.status, headers: parts.headers, bytes: to_bytes(body, 64 * 1024 * 1024).await.unwrap().to_vec() }
}

async fn call(e: &Env, method: &str, uri: &str, token: Option<&str>, body: Option<Value>) -> Reply {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let req = match body {
        Some(v) => b.header(header::CONTENT_TYPE, "application/json").body(Body::from(v.to_string())).unwrap(),
        None => b.body(Body::empty()).unwrap(),
    };
    send(e, req).await
}

/// A paired device of `user`; every call makes another device.
async fn pair(e: &mut Env, user: i64) -> String {
    e.n += 1;
    let enrollment = e.store.create_enrollment(user, false, e.admin).await.unwrap();
    let r = call(e, "POST", "/api/v1/pair", None, Some(json!({ "code": enrollment.code, "device_name": "Laptop", "public_key": key(e.n) }))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", String::from_utf8_lossy(&r.bytes));
    r.json()["token"].as_str().unwrap().to_owned()
}

async fn pair_admin(e: &mut Env) -> String {
    let admin = e.admin;
    pair(e, admin).await
}

enum Part<'a> {
    Text(&'a str, String),
    File(&'a str, &'a str, Vec<u8>),
}

async fn upload(e: &Env, token: &str, parts: Vec<Part<'_>>) -> Reply {
    let boundary = "----cdtest";
    let mut body: Vec<u8> = Vec::new();
    for p in parts {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        match p {
            Part::Text(name, text) => body.extend_from_slice(format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n{text}\r\n").as_bytes()),
            Part::File(name, file, bytes) => {
                body.extend_from_slice(format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"{file}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
                body.extend_from_slice(&bytes);
                body.extend_from_slice(b"\r\n");
            }
        }
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/cd/import")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body))
        .unwrap();
    send(e, req).await
}

fn mp3(dir: &std::path::Path, name: &str, rate: &str) -> Vec<u8> {
    let path = dir.join(name);
    common::audio::make(&path, &["-c:a", "libmp3lame", "-b:a", "64k", "-ar", rate, "-ac", "2"], &[("title", "Teil"), ("artist", "Somebody")]);
    std::fs::read(path).unwrap()
}

fn have_ffmpeg() -> bool {
    let ok = common::audio::ffmpeg_available();
    if !ok {
        eprintln!("ffmpeg is not installed: test skipped");
    }
    ok
}

fn meta(disc: u32, discs_total: u32, tracks: u32) -> Value {
    json!({
        "author": "Ken Follett", "album": "Sturz der Titanen", "narrator": "Johannes Steck", "year": "2010",
        "disc": disc, "discs_total": discs_total,
        "tracks": (1..=tracks).map(|n| json!({ "number": n, "title": format!("Teil {n:03}"), "file": format!("t{n}.mp3") })).collect::<Vec<_>>(),
    })
}

fn staging_is_empty(e: &Env) -> bool {
    std::fs::read_dir(e.dir.path().join("cd-import")).map(|mut rd| rd.next().is_none()).unwrap_or(true)
}

fn toc() -> Value {
    json!({ "first_track": 1, "last_track": 14, "leadout_lba": 334259,
        "starts_lba": [0, 16760, 48475, 78673, 103647, 134004, 149048, 162914, 192909, 213458, 248099, 271069, 291260, 306741] })
}

async fn set(e: &Env, pairs: &[(&str, &str)]) {
    let values: Vec<(String, String)> = pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
    e.store.save_settings(&values).await.unwrap();
}

#[tokio::test]
async fn only_administrators_and_members_may_use_it() {
    let mut e = env().await;
    let kid = e.store.create_user(NewProfile::child("Kid", 10, Schedule::always())).await.unwrap();
    let anna = e.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    e.store.set_cd_rights(anna, david_server::cdrights::CdRightsChange { may_insert: Some(true), ..Default::default() }).await.unwrap();
    let kid_token = pair(&mut e, kid).await;
    let anna_token = pair(&mut e, anna).await;
    for (method, uri, body) in [
        ("GET", "/api/v1/cd/settings", None),
        ("POST", "/api/v1/cd/lookup", Some(json!({ "toc": toc() }))),
        ("GET", "/api/v1/cd/cover?release=0f2d4b1c-8d0a-4f17-8d7e-111111111111", None),
        ("POST", "/api/v1/cd/jobs", Some(json!({}))),
        ("GET", "/api/v1/cd/jobs", None),
        ("GET", "/api/v1/cd/import/1", None),
        ("GET", "/api/v1/cd/events", None),
    ] {
        let r = call(&e, method, uri, Some(&kid_token), body.clone()).await;
        assert_eq!((r.status, r.code().as_str()), (StatusCode::FORBIDDEN, "forbidden"), "{uri}");
        let r = call(&e, method, uri, None, body).await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{uri}");
    }
    let r = upload(&e, &kid_token, vec![Part::Text("meta", meta(1, 1, 1).to_string())]).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&e, "GET", "/api/v1/cd/settings", Some(&anna_token), None).await.status, StatusCode::OK);
}

#[tokio::test]
async fn the_apps_read_the_settings_and_the_defaults_are_the_formats_of_the_existing_rips() {
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    let r = call(&e, "GET", "/api/v1/cd/settings", Some(&token), None).await;
    let s = &r.json();
    assert_eq!(s["format"], "mp3");
    assert_eq!((s["bitrate_kbps"].as_u64(), s["sample_rate_hz"].as_u64(), s["quality"].as_u64(), s["cover_size"].as_u64()), (Some(64), Some(24000), Some(3), Some(500)));
    assert_eq!((s["channel_mode"].as_str(), s["tag_version"].as_str(), s["track_number_scheme"].as_str()), (Some("joint_stereo"), Some("2.4"), Some("disc_times_100")));
    assert_eq!(s["file_scheme"], "{author}_{title}_CD{disc:02}_TR{track:02}");
    assert_eq!((s["folder_scheme"].as_str(), s["title_scheme"].as_str(), s["genre"].as_str()), (Some("{author}/{title}"), Some("file_name"), Some("Audiobook")));
    assert_eq!((s["eject"].as_bool(), s["lookup_ready"].as_bool(), s["kind"].as_str()), (Some(true), Some(false), Some("audiobook")));
    assert_eq!(s["udev"].as_bool(), Some(false), "the udev switch is off until the administrator turns it on");
    assert!(r.json()["device"].as_str().is_some_and(|d| d.len() == 12));
    // The token itself is never shown.
    assert!(!String::from_utf8_lossy(&r.bytes).contains(&token));

    set(&e, &[("cd_bitrate_kbps", "96"), ("musicbrainz_contact", "me@example.org"), ("cd_udev", "1")]).await;
    let s = call(&e, "GET", "/api/v1/cd/settings", Some(&token), None).await.json();
    assert_eq!((s["bitrate_kbps"].as_u64(), s["lookup_ready"].as_bool(), s["udev"].as_bool()), (Some(96), Some(true), Some(true)));
    // A damaged stored value falls back instead of breaking the apps.
    e.store.save_settings(&[("cd_sample_rate_hz".to_owned(), "banana".to_owned())]).await.unwrap();
    let s = call(&e, "GET", "/api/v1/cd/settings", Some(&token), None).await.json();
    assert_eq!(s["sample_rate_hz"].as_u64(), Some(24000));
}

#[tokio::test]
async fn a_disc_is_looked_up_with_a_contact_cached_and_asked_for_only_once() {
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    let body = json!({ "toc": toc() });
    // Without a contact address nothing is sent.
    let r = call(&e, "POST", "/api/v1/cd/lookup", Some(&token), Some(body.clone())).await;
    assert_eq!((r.status, r.code().as_str()), (StatusCode::CONFLICT, "no_contact"));
    assert!(e.fake.asked().is_empty());

    set(&e, &[("musicbrainz_contact", "family@example.org")]).await;
    e.fake.on(&format!("/ws/2/discid/{DISC_ID}"), Some(include_bytes!("data/mb-discid.json")));
    let r = call(&e, "POST", "/api/v1/cd/lookup", Some(&token), Some(body.clone())).await;
    assert_eq!(r.status, StatusCode::OK, "{}", String::from_utf8_lossy(&r.bytes));
    let j = r.json();
    assert_eq!(j["disc_id"], DISC_ID);
    let c = &j["candidates"][0];
    assert_eq!((c["title"].as_str(), c["author"].as_str(), c["narrators"][0].as_str()), (Some("Sturz der Titanen"), Some("Ken Follett"), Some("Johannes Steck")));
    assert_eq!((c["disc"].as_u64(), c["discs_total"].as_u64(), c["language"].as_str()), (Some(2), Some(12), Some("deu")));
    assert_eq!((c["tracks"].as_array().map(Vec::len), c["tracks"][0].as_str(), c["track_details"][0]["length_ms"].as_u64(), c["cover"].as_str()), (Some(14), Some("Teil 014"), Some(223000), c["release_id"].as_str()));

    // The exact question, then the one by the table of contents for releases that are nearly the same.
    let asked = e.fake.asked();
    assert_eq!(asked.len(), 2, "{asked:?}");
    assert!(asked[1].1.contains("/ws/2/discid/-?toc="), "{}", asked[1].1);
    assert_eq!(asked[0].0, Service::MusicBrainz);
    assert!(asked[0].1.starts_with("https://musicbrainz.org/ws/2/discid/"), "{}", asked[0].1);
    assert!(asked[0].2.contains("family@example.org") && asked[0].2.starts_with("david-server/"), "{}", asked[0].2);
    // The second look comes from the cache.
    let again = call(&e, "POST", "/api/v1/cd/lookup", Some(&token), Some(body)).await;
    assert_eq!(again.json(), j);
    // The answer for the ID is not asked for again; a question that found nothing is not cached and is asked once more.
    let asked = e.fake.asked();
    assert_eq!(asked.len(), 3, "{asked:?}");
    assert!(asked[2].1.contains("/ws/2/discid/-?toc="), "{}", asked[2].1);
}

#[tokio::test]
async fn a_bad_table_of_contents_and_services_that_fail_are_answered_with_codes() {
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    set(&e, &[("musicbrainz_contact", "family@example.org"), ("cd_musicbrainz_url", "https://mirror.example.org/")]).await;
    let mut bad = toc();
    bad["last_track"] = json!(13);
    let r = call(&e, "POST", "/api/v1/cd/lookup", Some(&token), Some(json!({ "toc": bad }))).await;
    assert_eq!((r.status, r.code().as_str()), (StatusCode::UNPROCESSABLE_ENTITY, "invalid_toc"));

    // The service is down: nothing was prepared, so the fake fails.
    let r = call(&e, "POST", "/api/v1/cd/lookup", Some(&token), Some(json!({ "toc": toc() }))).await;
    assert_eq!((r.status, r.code().as_str()), (StatusCode::BAD_GATEWAY, "lookup_failed"));
    assert!(e.fake.asked()[0].1.starts_with("https://mirror.example.org/ws/2/discid/"), "the address of the setting is used");

    // Unknown by ID: asked again by the table of contents; unknown there too: no candidates, remembered.
    e.fake.on("/ws/2/discid/", None);
    let r = call(&e, "POST", "/api/v1/cd/lookup", Some(&token), Some(json!({ "toc": toc() }))).await;
    assert_eq!((r.status, r.json()["candidates"].as_array().map(Vec::len)), (StatusCode::OK, Some(0)));
    let n = e.fake.asked().len();
    assert!(e.fake.asked().iter().any(|a| a.1.contains("discid/-?toc=1+14+334409+150+")));
    call(&e, "POST", "/api/v1/cd/lookup", Some(&token), Some(json!({ "toc": toc() }))).await;
    assert_eq!(e.fake.asked().len(), n, "a miss is remembered for a while");

    // A nonsense answer is a failure of the service, not of the app.
    let mut e2 = env().await;
    let t2 = pair_admin(&mut e2).await;
    set(&e2, &[("musicbrainz_contact", "x@example.org")]).await;
    e2.fake.on("/ws/2/discid/", Some(b"<html>oops</html>"));
    let r = call(&e2, "POST", "/api/v1/cd/lookup", Some(&t2), Some(json!({ "toc": toc() }))).await;
    assert_eq!((r.status, r.code().as_str()), (StatusCode::BAD_GATEWAY, "lookup_failed"));
}

#[tokio::test]
async fn the_cover_comes_from_the_cover_art_archive_once_and_a_missing_one_is_a_404() {
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    set(&e, &[("musicbrainz_contact", "family@example.org")]).await;
    let uri = format!("/api/v1/cd/cover?release={RELEASE_ID}");
    e.fake.on(&format!("/release/{RELEASE_ID}/front-500"), Some(include_bytes!("data/caa-front-500.jpg")));
    let r = call(&e, "GET", &uri, Some(&token), None).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.headers[header::CONTENT_TYPE], "image/jpeg");
    assert_eq!(r.bytes, include_bytes!("data/caa-front-500.jpg"));
    assert_eq!(e.fake.asked()[0].0, Service::CoverArt);
    call(&e, "GET", &uri, Some(&token), None).await;
    assert_eq!(e.fake.asked().len(), 1);

    let other = "0f2d4b1c-8d0a-4f17-8d7e-222222222222";
    e.fake.on(&format!("/release/{other}/"), None);
    assert_eq!(call(&e, "GET", &format!("/api/v1/cd/cover?release={other}"), Some(&token), None).await.status, StatusCode::NOT_FOUND);
    // Something that is no picture is no cover.
    let third = "0f2d4b1c-8d0a-4f17-8d7e-333333333333";
    e.fake.on(&format!("/release/{third}/"), Some(b"<html>no</html>"));
    assert_eq!(call(&e, "GET", &format!("/api/v1/cd/cover?release={third}"), Some(&token), None).await.status, StatusCode::NOT_FOUND);
    // Only an identifier goes into an address.
    let r = call(&e, "GET", "/api/v1/cd/cover?release=..%2F..%2Fetc", Some(&token), None).await;
    assert_eq!((r.status, r.code().as_str()), (StatusCode::UNPROCESSABLE_ENTITY, "invalid_release"));
}

#[tokio::test]
async fn ripped_files_are_checked_named_tagged_and_placed_with_the_cover() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    let work = tempfile::TempDir::new().unwrap();
    let cover = include_bytes!("data/caa-front-500.jpg").to_vec();
    let parts = vec![
        Part::Text("meta", meta(2, 12, 2).to_string()),
        Part::File("file", "t1.mp3", mp3(work.path(), "a.mp3", "24000")),
        Part::File("file", "t2.mp3", mp3(work.path(), "b.mp3", "24000")),
        Part::File("cover", "cover.jpg", cover.clone()),
    ];
    let r = upload(&e, &token, parts).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", String::from_utf8_lossy(&r.bytes));
    let j = r.json();
    assert_eq!((j["import"].as_str(), j["eject"].as_bool()), (Some("locked"), Some(true)));
    assert_eq!(j["placed"]["folder"], "Ken Follett/Sturz der Titanen");

    // Disc 2 of a work with several discs goes into a disc folder, with the name of the existing rips.
    let book = e.library.path().join("Ken Follett/Sturz der Titanen");
    let first = book.join("CD 2/Ken Follett_Sturz der Titanen_CD02_TR01.mp3");
    assert!(first.is_file() && book.join("CD 2/Ken Follett_Sturz der Titanen_CD02_TR02.mp3").is_file(), "{:?}", std::fs::read_dir(&book).map(|d| d.flatten().map(|x| x.path()).collect::<Vec<_>>()));
    assert_eq!(std::fs::read(book.join("cover.jpg")).unwrap(), cover);
    assert!(staging_is_empty(&e));

    let t = david_server::tags::read(&first).unwrap();
    assert_eq!(t.title.as_deref(), Some("Ken Follett_Sturz der Titanen_CD02_TR01"));
    assert_eq!((t.artist.as_deref(), t.album_artist.as_deref(), t.album.as_deref(), t.narrator.as_deref()), (Some("Ken Follett"), Some("Ken Follett"), Some("Sturz der Titanen"), Some("Johannes Steck")));
    assert_eq!((t.track, t.disc, t.genre.as_deref(), t.tag_kind), (Some(201), Some(2), Some("Audiobook"), Some("ID3v2")));
    assert!(david_server::tags::embedded_cover(&first).is_some_and(|(m, b)| m == "image/jpeg" && b == cover));
    assert!(!david_server::tags::lacks_id3v2(&first));

    // The job is closed; the import is locked, so it did not run.
    let id = j["job"]["id"].as_i64().unwrap();
    let job = call(&e, "GET", &format!("/api/v1/cd/import/{id}"), Some(&token), None).await.json()["job"].clone();
    assert_eq!((job["phase"].as_str(), job["import_ran"].as_bool(), job["active"].as_bool()), (Some("done"), Some(false), Some(false)));
    assert_eq!((job["album"].as_str(), job["author"].as_str(), job["disc"].as_i64(), job["tracks_total"].as_i64()), (Some("Sturz der Titanen"), Some("Ken Follett"), Some(2), Some(2)));

    // The same disc again is refused and leaves nothing behind.
    let again = upload(&e, &token, vec![Part::Text("meta", meta(2, 12, 1).to_string()), Part::File("file", "t1.mp3", mp3(work.path(), "c.mp3", "24000"))]).await;
    assert_eq!((again.status, again.code().as_str()), (StatusCode::CONFLICT, "disc_exists"));
    assert!(staging_is_empty(&e));
    let failed = call(&e, "GET", "/api/v1/cd/jobs", Some(&token), None).await.json();
    assert_eq!(failed["jobs"][0]["phase"], "failed");
    assert_eq!(failed["jobs"][0]["error"], "disc_exists");

    // Disc 1 of the same work joins the same book.
    let r = upload(&e, &token, vec![Part::Text("meta", meta(1, 12, 1).to_string()), Part::File("file", "t1.mp3", mp3(work.path(), "d.mp3", "24000"))]).await;
    assert_eq!(r.status, StatusCode::CREATED);
    assert!(book.join("CD 1/Ken Follett_Sturz der Titanen_CD01_TR01.mp3").is_file());
    assert_eq!(std::fs::read_dir(e.library.path().join("Ken Follett")).unwrap().count(), 1);
}

#[tokio::test]
async fn a_single_cd_and_a_book_that_is_kept_flat_stay_flat_and_the_settings_rule_the_names_and_tags() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    let work = tempfile::TempDir::new().unwrap();
    // One CD: no disc folder.
    let r = upload(&e, &token, vec![Part::Text("meta", meta(1, 1, 1).to_string()), Part::File("file", "t1.mp3", mp3(work.path(), "a.mp3", "24000"))]).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", String::from_utf8_lossy(&r.bytes));
    let book = e.library.path().join("Ken Follett/Sturz der Titanen");
    assert!(book.join("Ken Follett_Sturz der Titanen_CD01_TR01.mp3").is_file());
    // Disc 2 arrives later: the book is kept flat, so it stays flat (no mixing of the two layouts).
    let r = upload(&e, &token, vec![Part::Text("meta", meta(2, 2, 1).to_string()), Part::File("file", "t1.mp3", mp3(work.path(), "b.mp3", "24000"))]).await;
    assert_eq!(r.status, StatusCode::CREATED);
    assert!(book.join("Ken Follett_Sturz der Titanen_CD02_TR01.mp3").is_file() && !book.join("CD 2").exists());

    // Other values of the settings: another name, version 2.3, plain track numbers, another genre and folders.
    set(
        &e,
        &[
            ("cd_file_pattern", "{disc}-{track:03} {track_title}"),
            ("cd_folder_pattern", "Books/{author}/{album} ({year})"),
            ("cd_title_pattern", "track_title"),
            ("cd_id3_version", "2.3"),
            ("cd_track_scheme", "plain"),
            ("cd_genre", "Hörbuch"),
            ("cd_disc_folder_pattern", ""),
        ],
    )
    .await;
    let r = upload(&e, &token, vec![Part::Text("meta", meta(3, 5, 1).to_string()), Part::File("file", "t1.mp3", mp3(work.path(), "c.mp3", "24000"))]).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", String::from_utf8_lossy(&r.bytes));
    let file = e.library.path().join("Books/Ken Follett/Sturz der Titanen (2010)/3-001 Teil 001.mp3");
    assert!(file.is_file(), "{}", r.json());
    let t = david_server::tags::read(&file).unwrap();
    assert_eq!((t.title.as_deref(), t.track, t.disc, t.genre.as_deref()), (Some("Teil 001"), Some(1), Some(3), Some("Hörbuch")));
    let head = std::fs::read(&file).unwrap();
    assert_eq!((&head[..3], head[3]), (&b"ID3"[..], 3), "ID3v2.3");
}

#[tokio::test]
async fn files_that_are_not_what_the_settings_demand_are_refused_and_nothing_stays_behind() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    let work = tempfile::TempDir::new().unwrap();
    let good = mp3(work.path(), "a.mp3", "24000");
    let wrong_rate = mp3(work.path(), "w.mp3", "44100");

    let cases: Vec<(&str, Vec<Part>, StatusCode, &str)> = vec![
        ("sample rate", vec![Part::Text("meta", meta(1, 1, 1).to_string()), Part::File("file", "t1.mp3", wrong_rate)], StatusCode::UNPROCESSABLE_ENTITY, "format_mismatch"),
        ("not audio", vec![Part::Text("meta", meta(1, 1, 1).to_string()), Part::File("file", "t1.mp3", b"this is not an mp3 at all".repeat(100))], StatusCode::UNPROCESSABLE_ENTITY, "unreadable_file"),
        ("a file is missing", vec![Part::Text("meta", meta(1, 1, 2).to_string()), Part::File("file", "t1.mp3", good.clone())], StatusCode::UNPROCESSABLE_ENTITY, "missing_file"),
        ("unknown file", vec![Part::Text("meta", meta(1, 1, 1).to_string()), Part::File("file", "other.mp3", good.clone())], StatusCode::UNPROCESSABLE_ENTITY, "unknown_file"),
        ("ending", vec![Part::Text("meta", json!({ "author": "A", "album": "B", "tracks": [{ "number": 1, "file": "t1.wav" }] }).to_string()), Part::File("file", "t1.wav", good.clone())], StatusCode::UNSUPPORTED_MEDIA_TYPE, "bad_extension"),
        ("file before meta", vec![Part::File("file", "t1.mp3", good.clone()), Part::Text("meta", meta(1, 1, 1).to_string())], StatusCode::UNPROCESSABLE_ENTITY, "invalid_meta"),
        ("no meta", vec![Part::File("cover", "c.jpg", include_bytes!("data/caa-front-500.jpg").to_vec())], StatusCode::UNPROCESSABLE_ENTITY, "invalid_meta"),
        ("bad cover", vec![Part::Text("meta", meta(1, 1, 1).to_string()), Part::File("cover", "c.jpg", b"nope".to_vec()), Part::File("file", "t1.mp3", good.clone())], StatusCode::UNPROCESSABLE_ENTITY, "bad_cover"),
        ("meta is not JSON", vec![Part::Text("meta", "{".into())], StatusCode::UNPROCESSABLE_ENTITY, "invalid_meta"),
        ("no author", vec![Part::Text("meta", json!({ "author": " ", "album": "B", "tracks": [{ "number": 1, "file": "t1.mp3" }] }).to_string())], StatusCode::UNPROCESSABLE_ENTITY, "invalid_meta"),
        ("disc beyond total", vec![Part::Text("meta", meta(3, 2, 1).to_string())], StatusCode::UNPROCESSABLE_ENTITY, "invalid_meta"),
        ("kind", vec![Part::Text("meta", json!({ "author": "A", "album": "B", "kind": "film", "tracks": [{ "number": 1, "file": "t1.mp3" }] }).to_string())], StatusCode::UNPROCESSABLE_ENTITY, "invalid_meta"),
    ];
    for (what, parts, status, code) in cases {
        let r = upload(&e, &token, parts).await;
        assert_eq!((r.status, r.code().as_str()), (status, code), "{what}");
        assert!(staging_is_empty(&e), "{what}: staging was cleaned");
        assert_eq!(std::fs::read_dir(e.library.path()).unwrap().count(), 0, "{what}: the library is untouched");
    }

    // Numbers must be 1..n without a gap or a double.
    for tracks in [json!([{ "number": 1, "file": "a.mp3" }, { "number": 3, "file": "b.mp3" }]), json!([{ "number": 1, "file": "a.mp3" }, { "number": 1, "file": "b.mp3" }]), json!([{ "number": 2, "file": "a.mp3" }]), json!([{ "number": 0, "file": "a.mp3" }])] {
        let r = upload(&e, &token, vec![Part::Text("meta", json!({ "author": "A", "album": "B", "tracks": tracks }).to_string())]).await;
        assert_eq!((r.status, r.code().as_str()), (StatusCode::UNPROCESSABLE_ENTITY, "track_numbers"), "{tracks}");
    }
    // Every failed attempt that got as far as a job closed it with its code.
    let jobs = call(&e, "GET", "/api/v1/cd/jobs", Some(&token), None).await.json();
    assert_eq!(jobs["busy"], false);
    assert!(jobs["jobs"].as_array().unwrap().iter().all(|j| j["phase"] == "failed"));
    // The ones that were refused before there was a job are in the log too, with their code.
    assert!(jobs["jobs"].as_array().unwrap().iter().any(|j| j["error"] == "invalid_meta"));
    assert!(jobs["jobs"].as_array().unwrap().iter().any(|j| j["error"] == "track_numbers"));
}

#[tokio::test]
async fn an_unlocked_import_runs_and_the_book_arrives_in_the_library() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    let work = tempfile::TempDir::new().unwrap();
    let root = e.library.path().canonicalize().unwrap().to_string_lossy().into_owned();
    e.store.save_import_criteria(&[root], david_server::RatingRule::Manual).await.unwrap();
    e.store.unlock_import("test").await.unwrap();
    let r = upload(&e, &token, vec![Part::Text("meta", meta(1, 1, 2).to_string()), Part::File("file", "t1.mp3", mp3(work.path(), "a.mp3", "24000")), Part::File("file", "t2.mp3", mp3(work.path(), "b.mp3", "24000"))]).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", String::from_utf8_lossy(&r.bytes));
    assert_eq!(r.json()["import"], "queued");
    let id = r.json()["job"]["id"].as_i64().unwrap();
    let mut job = Value::Null;
    for _ in 0..200 {
        job = call(&e, "GET", &format!("/api/v1/cd/import/{id}"), Some(&token), None).await.json()["job"].clone();
        if job["active"] == false {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!((job["phase"].as_str(), job["import_ran"].as_bool()), (Some("done"), Some(true)), "{job}");
    assert!(job["message"].as_str().unwrap().contains("2 files placed"), "{job}");
    let found = e.store.find_book("Ken Follett/Sturz der Titanen").await.unwrap();
    assert!(found.is_some(), "the book is in the library");
}

#[tokio::test]
async fn jobs_are_the_accounts_and_show_whether_another_device_is_ripping() {
    let mut e = env().await;
    let anna = e.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    e.store.set_cd_rights(anna, david_server::cdrights::CdRightsChange { may_insert: Some(true), ..Default::default() }).await.unwrap();
    let a = pair_admin(&mut e).await;
    let b = pair_admin(&mut e).await;
    let stranger = pair(&mut e, anna).await;

    let r = call(&e, "POST", "/api/v1/cd/jobs", Some(&a), Some(json!({ "tracks_total": 14, "author": "Ken Follett", "album": "Sturz der Titanen", "disc": 2, "discs_total": 12 }))).await;
    assert_eq!(r.status, StatusCode::CREATED);
    assert_eq!(r.json()["busy_elsewhere"], false);
    let id = r.json()["job"]["id"].as_i64().unwrap();
    assert_eq!(r.json()["job"]["phase"], "ripping");

    let mine = call(&e, "GET", "/api/v1/cd/jobs", Some(&a), None).await.json();
    let theirs = call(&e, "GET", "/api/v1/cd/jobs", Some(&b), None).await.json();
    assert_eq!((mine["busy"].as_bool(), mine["busy_elsewhere"].as_bool()), (Some(true), Some(false)));
    assert_eq!((theirs["busy"].as_bool(), theirs["busy_elsewhere"].as_bool()), (Some(true), Some(true)));
    assert_ne!(mine["device"], theirs["device"]);
    assert_eq!(theirs["jobs"][0]["device"], mine["device"]);
    // B asking to start a job is told that A is ripping.
    let r = call(&e, "POST", "/api/v1/cd/jobs", Some(&b), Some(json!({}))).await;
    assert_eq!(r.json()["busy_elsewhere"], true);
    // Another account sees nothing.
    assert_eq!(call(&e, "GET", &format!("/api/v1/cd/jobs/{id}"), Some(&stranger), None).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call(&e, "GET", "/api/v1/cd/jobs", Some(&stranger), None).await.json()["jobs"].as_array().map(Vec::len), Some(0));

    // Progress.
    let uri = format!("/api/v1/cd/jobs/{id}/progress");
    let r = call(&e, "PUT", &uri, Some(&a), Some(json!({ "phase": "encoding", "track": 3 }))).await;
    assert_eq!((r.json()["job"]["phase"].as_str(), r.json()["job"]["track"].as_i64(), r.json()["job"]["tracks_total"].as_i64()), (Some("encoding"), Some(3), Some(14)));
    for bad in [json!({ "phase": "done" }), json!({ "phase": "importing" }), json!({ "track": 100 }), json!({ "track": -1 })] {
        assert_eq!(call(&e, "PUT", &uri, Some(&a), Some(bad.clone())).await.status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
    }
    assert_eq!(call(&e, "PUT", &uri, Some(&stranger), Some(json!({ "track": 1 }))).await.status, StatusCode::NOT_FOUND);
    // The app gives up: closed with a code, and then it cannot be changed any more.
    let r = call(&e, "PUT", &uri, Some(&a), Some(json!({ "phase": "failed", "error": "read_error", "message": "Track 4 could not be read" }))).await;
    assert_eq!((r.json()["job"]["phase"].as_str(), r.json()["job"]["error"].as_str(), r.json()["job"]["active"].as_bool()), (Some("failed"), Some("read_error"), Some(false)));
    assert_eq!(call(&e, "PUT", &uri, Some(&a), Some(json!({ "track": 5 }))).await.code(), "job_closed");
    assert_eq!(call(&e, "GET", "/api/v1/cd/jobs", Some(&b), None).await.json()["busy_elsewhere"], false);
}

#[tokio::test]
async fn a_job_nobody_has_heard_of_for_long_is_given_up() {
    let mut e = env().await;
    let a = pair_admin(&mut e).await;
    let r = call(&e, "POST", "/api/v1/cd/jobs", Some(&a), Some(json!({}))).await;
    let id = r.json()["job"]["id"].as_i64().unwrap();
    // The server restarted: jobs under way are closed.
    assert_eq!(david_server::cdflow::recover(&e.store, &CdService::with_lookup(e.dir.path(), e.fake.clone())).await, 1);
    let job = call(&e, "GET", &format!("/api/v1/cd/import/{id}"), Some(&a), None).await.json()["job"].clone();
    assert_eq!((job["phase"].as_str(), job["error"].as_str()), (Some("failed"), Some("interrupted")));
}

// The event channel over a real socket.

struct Server {
    addr: std::net::SocketAddr,
}

async fn serve(e: &Env) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = e.router.clone();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Server { addr }
}

async fn ws_open(server: &Server, token: &str) -> TcpStream {
    let mut s = TcpStream::connect(server.addr).await.unwrap();
    let req = format!(
        "GET /api/v1/cd/events HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
    );
    s.write_all(req.as_bytes()).await.unwrap();
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        head.push(s.read_u8().await.unwrap());
    }
    let head = String::from_utf8_lossy(&head).to_lowercase();
    assert!(head.starts_with("http/1.1 101"), "{head}");
    assert!(head.contains("sec-websocket-accept: s3pplmbitxaq9kygzzhzrbk+xoo="), "{head}");
    s
}

/// One frame from the server: opcode and payload.
async fn ws_read(s: &mut TcpStream, wait: Duration) -> Option<(u8, Vec<u8>)> {
    let read = async {
        let mut head = [0u8; 2];
        s.read_exact(&mut head).await.ok()?;
        let len = match head[1] & 0x7f {
            126 => s.read_u16().await.ok()? as usize,
            127 => s.read_u64().await.ok()? as usize,
            n => n as usize,
        };
        assert_eq!(head[1] & 0x80, 0, "a server never masks");
        let mut payload = vec![0u8; len];
        s.read_exact(&mut payload).await.ok()?;
        Some((head[0] & 0x0f, payload))
    };
    tokio::time::timeout(wait, read).await.ok().flatten()
}

/// The next JSON message that is not about the drive (those have their own tests in cd_drive.rs).
async fn ws_json(s: &mut TcpStream) -> Value {
    loop {
        let (op, payload) = ws_read(s, Duration::from_secs(5)).await.expect("a message");
        if op == 1 {
            let v: Value = serde_json::from_slice(&payload).unwrap();
            if v["type"] != "drive" {
                return v;
            }
        }
    }
}

/// The `drive` message that follows the snapshot.
async fn ws_drive(s: &mut TcpStream) -> Value {
    let (op, payload) = ws_read(s, Duration::from_secs(5)).await.expect("the drive message");
    assert_eq!(op, 1);
    let v: Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(v["type"], "drive");
    v
}

async fn ws_send(s: &mut TcpStream, opcode: u8, payload: &[u8]) {
    let mask = [7u8, 8, 9, 10];
    let mut out = vec![0x80 | opcode, 0x80 | payload.len() as u8];
    out.extend_from_slice(&mask);
    out.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    s.write_all(&out).await.unwrap();
}

#[tokio::test]
async fn changes_of_a_job_are_pushed_to_every_device_of_the_account_and_to_nobody_else() {
    let mut e = env_with(Some(Duration::from_secs(30))).await;
    let anna = e.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    e.store.set_cd_rights(anna, david_server::cdrights::CdRightsChange { may_insert: Some(true), ..Default::default() }).await.unwrap();
    let a = pair_admin(&mut e).await;
    let b = pair_admin(&mut e).await;
    let stranger = pair(&mut e, anna).await;
    let server = serve(&e).await;

    let mut wa = ws_open(&server, &a).await;
    let mut wb = ws_open(&server, &b).await;
    let mut ws_stranger = ws_open(&server, &stranger).await;
    let hello = ws_json(&mut wa).await;
    assert_eq!((hello["type"].as_str(), hello["v"].as_u64()), (Some("hello"), Some(1)));
    let device_a = hello["device"].as_str().unwrap().to_owned();
    let snapshot = ws_json(&mut wa).await;
    assert_eq!((snapshot["type"].as_str(), snapshot["busy"].as_bool(), snapshot["jobs"].as_array().map(Vec::len)), (Some("snapshot"), Some(false), Some(0)));
    ws_json(&mut wb).await;
    ws_json(&mut wb).await;
    ws_json(&mut ws_stranger).await;
    ws_json(&mut ws_stranger).await;
    for w in [&mut wa, &mut wb, &mut ws_stranger] {
        assert!(ws_drive(w).await["drives"].is_array());
    }

    let r = call(&e, "POST", "/api/v1/cd/jobs", Some(&a), Some(json!({ "tracks_total": 14, "author": "Ken Follett", "album": "Sturz der Titanen", "disc": 2, "discs_total": 12 }))).await;
    let id = r.json()["job"]["id"].as_i64().unwrap();
    for w in [&mut wa, &mut wb] {
        let m = ws_json(w).await;
        assert_eq!((m["type"].as_str(), m["v"].as_u64(), m["job"]["id"].as_i64(), m["job"]["device"].as_str()), (Some("job"), Some(1), Some(id), Some(device_a.as_str())));
        assert_eq!(m["job"]["album"], "Sturz der Titanen");
    }
    call(&e, "PUT", &format!("/api/v1/cd/jobs/{id}/progress"), Some(&a), Some(json!({ "phase": "ripping", "track": 4 }))).await;
    let m = ws_json(&mut wb).await;
    assert_eq!((m["type"].as_str(), m["job"]["track"].as_i64(), m["job"]["tracks_total"].as_i64(), m["job"]["active"].as_bool()), (Some("progress"), Some(4), Some(14), Some(true)));
    call(&e, "PUT", &format!("/api/v1/cd/jobs/{id}/progress"), Some(&a), Some(json!({ "phase": "failed", "error": "read_error" }))).await;
    ws_json(&mut wa).await;
    ws_json(&mut wa).await;
    let m = ws_json(&mut wb).await;
    assert_eq!((m["type"].as_str(), m["job"]["error"].as_str()), (Some("failed"), Some("read_error")));
    // The other account heard nothing of it.
    assert!(ws_read(&mut ws_stranger, Duration::from_millis(300)).await.is_none());

    // A device that connects later is told the state at once; B sees that A is not busy any more.
    let mut late = ws_open(&server, &b).await;
    ws_json(&mut late).await;
    let snap = ws_json(&mut late).await;
    ws_drive(&mut late).await;
    assert_eq!((snap["type"].as_str(), snap["busy"].as_bool(), snap["jobs"][0]["phase"].as_str()), (Some("snapshot"), Some(false), Some("failed")));

    // A close from the client is answered with a close.
    ws_send(&mut wa, 8, &1000u16.to_be_bytes()).await;
    let (op, payload) = ws_read(&mut wa, Duration::from_secs(5)).await.expect("a close frame");
    assert_eq!((op, u16::from_be_bytes([payload[0], payload[1]])), (8, 1000));
}

#[tokio::test]
async fn the_heartbeat_pings_and_a_revoked_token_ends_the_connection() {
    let mut e = env_with(Some(Duration::from_millis(200))).await;
    let a = pair_admin(&mut e).await;
    let server = serve(&e).await;
    let mut w = ws_open(&server, &a).await;
    ws_json(&mut w).await;
    ws_json(&mut w).await;
    // A ping arrives; the answer is a pong.
    let (op, _) = loop {
        let f = ws_read(&mut w, Duration::from_secs(5)).await.expect("a ping");
        if f.0 == 9 {
            break f;
        }
    };
    assert_eq!(op, 9);
    ws_send(&mut w, 10, b"david").await;
    // The client's own ping is answered with a pong that carries the same bytes.
    ws_send(&mut w, 9, b"are you there").await;
    let (op, payload) = loop {
        let f = ws_read(&mut w, Duration::from_secs(5)).await.expect("a pong");
        if f.0 == 10 {
            break f;
        }
    };
    assert_eq!((op, payload.as_slice()), (10, &b"are you there"[..]));

    // The device is signed out: the connection is closed with the code for it.
    e.store.delete_session(&a).await.unwrap();
    let code = loop {
        let (op, payload) = ws_read(&mut w, Duration::from_secs(5)).await.expect("a close frame");
        if op == 8 {
            break u16::from_be_bytes([payload[0], payload[1]]);
        }
    };
    assert_eq!(code, 4401);
    assert!(ws_read(&mut w, Duration::from_secs(2)).await.is_none(), "the connection is gone");
}

#[tokio::test]
async fn a_client_that_goes_quiet_is_dropped_and_a_plain_request_to_the_channel_is_refused() {
    let mut e = env_with(Some(Duration::from_millis(100))).await;
    let a = pair_admin(&mut e).await;
    let server = serve(&e).await;
    let mut w = ws_open(&server, &a).await;
    // Nothing is ever sent back: after three heartbeats the server closes with the timeout code.
    let code = loop {
        let (op, payload) = ws_read(&mut w, Duration::from_secs(5)).await.expect("a close frame");
        if op == 8 {
            break u16::from_be_bytes([payload[0], payload[1]]);
        }
    };
    assert_eq!(code, 4408);
    // An unmasked frame is a protocol error.
    let mut w = ws_open(&server, &a).await;
    w.write_all(&[0x81, 1, b'x']).await.unwrap();
    let code = loop {
        let (op, payload) = ws_read(&mut w, Duration::from_secs(5)).await.expect("a close frame");
        if op == 8 {
            break u16::from_be_bytes([payload[0], payload[1]]);
        }
    };
    assert_eq!(code, 1002);
    // Without the upgrade headers it is a normal request that is turned down.
    let r = call(&e, "GET", "/api/v1/cd/events", Some(&a), None).await;
    assert_eq!((r.status, r.code().as_str()), (StatusCode::BAD_REQUEST, "bad_upgrade"));
}

#[tokio::test]
async fn the_linux_app_asks_with_the_toc_parameter_and_the_cover_reference() {
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    set(&e, &[("musicbrainz_contact", "family@example.org")]).await;
    e.fake.on(&format!("/ws/2/discid/{DISC_ID}"), Some(include_bytes!("data/mb-discid.json")));
    let param = "1+14+334409+150+16910+48625+78823+103797+134154+149198+163064+193059+213608+248249+271219+291410+306891";
    let r = call(&e, "POST", "/api/v1/cd/lookup", Some(&token), Some(json!({ "disc_id": "whatever the app computed", "toc": param }))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", String::from_utf8_lossy(&r.bytes));
    assert_eq!(r.json()["disc_id"], DISC_ID);
    let c = &r.json()["candidates"][0];
    let cover_ref = c["cover"].as_str().unwrap().to_owned();
    e.fake.on(&format!("/release/{cover_ref}/front-500"), Some(include_bytes!("data/caa-front-500.jpg")));
    // The reference of the candidate is what `GET /cd/cover?ref=` takes.
    let img = call(&e, "GET", &format!("/api/v1/cd/cover?ref={cover_ref}"), Some(&token), None).await;
    assert_eq!((img.status, img.bytes.len()), (StatusCode::OK, include_bytes!("data/caa-front-500.jpg").len()));
    assert_eq!(call(&e, "POST", "/api/v1/cd/lookup", Some(&token), Some(json!({ "toc": "1+2+3" }))).await.code(), "invalid_toc");
}

#[tokio::test]
async fn the_linux_app_hands_over_files_that_are_named_and_tagged_already() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    let work = tempfile::TempDir::new().unwrap();
    // The app has tagged its files (track = disc x 100 + number) and named them by the scheme.
    let file = |name: &str, track: &str| {
        let path = work.path().join(name);
        common::audio::make(&path, &["-c:a", "libmp3lame", "-b:a", "64k", "-ar", "24000", "-ac", "2"], &[("title", name), ("track", track), ("artist", "Ken Follett"), ("album", "Sturz der Titanen")]);
        std::fs::read(path).unwrap()
    };
    let cover = include_bytes!("data/caa-front-500.jpg").to_vec();
    let parts = vec![
        Part::Text("kind", "audiobook".into()),
        Part::Text("author", "Ken Follett".into()),
        Part::Text("title", "Sturz der Titanen".into()),
        Part::Text("disc", "2".into()),
        Part::Text("discs_total", "12".into()),
        Part::Text("path", "Ken Follett_Sturz der Titanen_CD02_TR02.mp3".into()),
        Part::File("file", "Ken%20Follett_Sturz%20der%20Titanen_CD02_TR02.mp3", file("two.mp3", "202")),
        Part::Text("path", "Ken Follett_Sturz der Titanen_CD02_TR01.mp3".into()),
        Part::File("file", "Ken%20Follett_Sturz%20der%20Titanen_CD02_TR01.mp3", file("one.mp3", "201")),
        Part::Text("path", "cover.jpg".into()),
        Part::File("file", "cover.jpg", cover.clone()),
    ];
    let r = upload(&e, &token, parts).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", String::from_utf8_lossy(&r.bytes));
    let j = r.json();
    let id = j["id"].as_i64().expect("the app reads {id}");
    assert_eq!((j["job"]["state"].as_str(), j["job"]["title"].as_str(), j["job"]["fraction"].as_f64()), (Some("done"), Some("Sturz der Titanen"), Some(1.0)));
    let book = e.library.path().join("Ken Follett/Sturz der Titanen");
    for n in ["01", "02"] {
        let f = book.join(format!("CD 2/Ken Follett_Sturz der Titanen_CD02_TR{n}.mp3"));
        let t = david_server::tags::read(&f).unwrap();
        assert_eq!((t.track, t.disc, t.genre.as_deref(), t.tag_kind), (Some(200 + n.parse::<u32>().unwrap()), Some(2), Some("Audiobook"), Some("ID3v2")), "{n}");
    }
    assert_eq!(std::fs::read(book.join("cover.jpg")).unwrap(), cover);
    // The app asks for the job: by id (inside "job") and for the open ones (none: this one is done).
    let one = call(&e, "GET", &format!("/api/v1/cd/import/{id}"), Some(&token), None).await.json();
    assert_eq!((one["job"]["state"].as_str(), one["job"]["id"].as_i64()), (Some("done"), Some(id)));
    assert_eq!(call(&e, "GET", "/api/v1/cd/import", Some(&token), None).await.json()["jobs"].as_array().map(Vec::len), Some(0));

    // Files without a number in the tag or the name cannot be put in order.
    let nameless = work.path().join("n.mp3");
    common::audio::make(&nameless, &["-c:a", "libmp3lame", "-b:a", "64k", "-ar", "24000", "-ac", "2"], &[]);
    let r = upload(&e, &token, vec![Part::Text("author", "A".into()), Part::Text("title", "B".into()), Part::File("file", "x.mp3", std::fs::read(nameless).unwrap())]).await;
    assert_eq!((r.status, r.code().as_str()), (StatusCode::UNPROCESSABLE_ENTITY, "track_numbers"));
    assert!(staging_is_empty(&e));
}

#[tokio::test]
async fn a_search_by_hand_adds_releases_to_those_of_the_disc_and_a_chosen_one_comes_with_the_tracks_of_this_disc() {
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    set(&e, &[("musicbrainz_contact", "family@example.org")]).await;
    e.fake.on(&format!("/ws/2/discid/{DISC_ID}"), Some(include_bytes!("data/mb-discid.json")));
    // The search finds another release than the one of the disc, and the same one again.
    let mut found: Value = serde_json::from_slice(include_bytes!("data/mb-discid.json")).unwrap();
    let mut other = found["releases"][0].clone();
    other["id"] = json!("0f2d4b1c-8d0a-4f17-8d7e-222222222222");
    other["title"] = json!("Ein anderes Buch");
    found["releases"].as_array_mut().unwrap().push(other);
    e.fake.on("/ws/2/release/?query=", Some(found.to_string().as_bytes()));
    e.fake.on("/ws/2/release/0f2d4b1c-8d0a-4f17-8d7e-222222222222", Some(include_bytes!("data/mb-release.json")));

    let r = call(&e, "POST", "/api/v1/cd/lookup", Some(&token), Some(json!({ "toc": toc(), "query": "Ken Follett Titanen" }))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", String::from_utf8_lossy(&r.bytes));
    let list = r.json()["candidates"].as_array().unwrap().clone();
    assert_eq!(list.len(), 2, "the release of the disc once, and the other one: {list:?}");
    assert_eq!((list[0]["disc_id_matched"].as_bool(), list[0]["tracks"].as_array().map(Vec::len)), (Some(true), Some(14)), "the release of the disc comes first, with tracks");
    assert_eq!((list[1]["title"].as_str(), list[1]["tracks"].as_array().map(Vec::len)), (Some("Ein anderes Buch"), Some(0)), "a search result has no tracks yet");
    let asked = e.fake.asked();
    assert!(asked.iter().any(|a| a.1.contains("/ws/2/release/?query=") && a.1.contains("release%3A%28Ken%20Follett%20Titanen%29")), "{asked:?}");

    // Chosen: the release with the tracks of the disc that has as many tracks as this CD (14: disc 2).
    let r = call(&e, "POST", "/api/v1/cd/lookup", Some(&token), Some(json!({ "toc": toc(), "release_id": "0f2d4b1c-8d0a-4f17-8d7e-222222222222" }))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", String::from_utf8_lossy(&r.bytes));
    let c = r.json()["candidates"][0].clone();
    assert_eq!((c["disc"].as_u64(), c["tracks"].as_array().map(Vec::len)), (Some(2), Some(14)), "{c}");

    // Too short a search, and a release that is no MBID, are refused.
    let r = call(&e, "POST", "/api/v1/cd/lookup", Some(&token), Some(json!({ "toc": toc(), "query": "K" }))).await;
    assert_eq!((r.status, r.code().as_str()), (StatusCode::UNPROCESSABLE_ENTITY, "invalid_query"));
    let r = call(&e, "POST", "/api/v1/cd/lookup", Some(&token), Some(json!({ "toc": toc(), "release_id": "nonsense" }))).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
}
