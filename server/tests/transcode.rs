// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The transcoding to MP3 for devices that cannot play a format (`GET /api/v1/parts/{id}/stream?as=mp3`, docs/spec-transcoding.md). The files
//! are made with ffmpeg; the tests skip themselves where it is not installed.

mod common;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use common::audio::{ffmpeg_available, make};
use common::open;
use david_server::api::{ApiState, router};
use david_server::{NewProfile, Schedule, Store};
use serde_json::{Value, json};
use tower::ServiceExt;

struct Api {
    router: Router,
    store: Store,
    admin: i64,
    library: tempfile::TempDir,
    dir: tempfile::TempDir,
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
    let state = ApiState { store: store.clone(), library: Some(library.path().to_owned()), cd: david_server::cdimport::CdService::new(dir.path()), features: Default::default() };
    Api { router: router(state), store, admin, library, dir }
}

async fn call(a: &Api, uri: &str, token: &str, range: Option<&str>) -> Reply {
    let mut b = Request::builder().method("GET").uri(uri).header(header::AUTHORIZATION, format!("Bearer {token}"));
    if let Some(r) = range {
        b = b.header(header::RANGE, r);
    }
    let res = a.router.clone().oneshot(b.body(Body::empty()).unwrap()).await.unwrap();
    let (parts, body) = res.into_parts();
    Reply { status: parts.status, headers: parts.headers, bytes: to_bytes(body, 64 * 1024 * 1024).await.unwrap().to_vec() }
}

fn key(n: u8) -> String {
    format!("{}{}=", "B".repeat(42), ["A", "E", "I", "M"][n as usize % 4])
}

async fn pair(a: &Api, user: i64, n: u8) -> String {
    let e = a.store.create_enrollment(user, false, a.admin).await.unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/pair")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "code": e.code, "device_name": "Phone", "public_key": key(n) }).to_string()))
        .unwrap();
    let res = a.router.clone().oneshot(req).await.unwrap();
    let bytes = to_bytes(res.into_body(), 1 << 20).await.unwrap();
    serde_json::from_slice::<Value>(&bytes).unwrap()["token"].as_str().unwrap().to_owned()
}

/// An album of an Ogg Vorbis, a FLAC and an MP3 file; returns the book id and the track ids by extension.
async fn album(a: &Api) -> (i64, std::collections::HashMap<&'static str, i64>) {
    let folder = a.library.path().join("Duo Camillo").join("Caipiranha");
    std::fs::create_dir_all(&folder).unwrap();
    let tags = [("artist", "Duo Camillo"), ("album", "Caipiranha")];
    make(&folder.join("01 - a.ogg"), &["-c:a", "libvorbis", "-q:a", "2"], &tags);
    make(&folder.join("02 - b.flac"), &["-c:a", "flac"], &tags);
    make(&folder.join("03 - c.mp3"), &["-c:a", "libmp3lame", "-b:a", "64k"], &tags);
    let report = a.store.scan_kind(a.library.path(), "music").await.unwrap();
    assert_eq!((report.tracks_added, report.books_failed), (3, 0), "{report:?}");
    let id = a.store.find_book("Duo Camillo/Caipiranha").await.unwrap().expect("the album").id;
    let tracks = a.store.tracks_of(id).await.unwrap();
    let by = |ext: &str| tracks.iter().find(|t| t.file.ends_with(ext)).unwrap().id;
    (id, [("ogg", by(".ogg")), ("flac", by(".flac")), ("mp3", by(".mp3"))].into_iter().collect())
}

fn cache_files(a: &Api) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(a.dir.path().join("transcode"))
        .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

macro_rules! need_ffmpeg {
    () => {
        if !ffmpeg_available() {
            eprintln!("skipped: ffmpeg is not installed");
            return;
        }
    };
}

#[tokio::test]
async fn an_ogg_and_a_flac_file_are_served_as_mp3_with_ranges_and_cached() {
    need_ffmpeg!();
    let a = api().await;
    let token = pair(&a, a.admin, 1).await;
    let (_, ids) = album(&a).await;

    for ext in ["ogg", "flac"] {
        let uri = format!("/api/v1/parts/{}/stream?as=mp3", ids[ext]);
        let r = call(&a, &uri, &token, None).await;
        assert_eq!(r.status, StatusCode::OK, "{ext}: {}", String::from_utf8_lossy(&r.bytes));
        assert_eq!(r.headers[header::CONTENT_TYPE], "audio/mpeg", "{ext}");
        assert!(cdrip::encode::first_frame(&r.bytes).is_some(), "{ext}: the body is an MP3");
        // Two seconds at 128 kbit/s: about 32 000 bytes.
        assert!((20_000..60_000).contains(&r.bytes.len()), "{ext}: {} bytes", r.bytes.len());

        // A range of the made file.
        let part = call(&a, &uri, &token, Some("bytes=0-99")).await;
        assert_eq!((part.status, part.bytes.len()), (StatusCode::PARTIAL_CONTENT, 100), "{ext}");
        assert_eq!(part.bytes, r.bytes[..100]);
    }
    // Both files are in the cache once, each made once.
    assert_eq!(cache_files(&a).len(), 2, "{:?}", cache_files(&a));
}

#[tokio::test]
async fn the_same_track_asked_at_once_is_made_once() {
    need_ffmpeg!();
    let a = api().await;
    let token = pair(&a, a.admin, 1).await;
    let (_, ids) = album(&a).await;
    let uri = format!("/api/v1/parts/{}/stream?as=mp3", ids["ogg"]);
    let (r1, r2, r3, r4) = tokio::join!(call(&a, &uri, &token, None), call(&a, &uri, &token, None), call(&a, &uri, &token, None), call(&a, &uri, &token, None));
    for r in [&r1, &r2, &r3, &r4] {
        assert_eq!(r.status, StatusCode::OK);
        assert_eq!(r.bytes, r1.bytes, "every request gets the same file");
    }
    assert_eq!(cache_files(&a).len(), 1);
}

#[tokio::test]
async fn an_mp3_is_served_as_it_is_and_nothing_is_cached() {
    need_ffmpeg!();
    let a = api().await;
    let token = pair(&a, a.admin, 1).await;
    let (_, ids) = album(&a).await;
    let plain = call(&a, &format!("/api/v1/parts/{}/stream", ids["mp3"]), &token, None).await;
    let asked = call(&a, &format!("/api/v1/parts/{}/stream?as=mp3", ids["mp3"]), &token, None).await;
    assert_eq!((asked.status, asked.headers[header::CONTENT_TYPE].to_str().unwrap()), (StatusCode::OK, "audio/mpeg"));
    assert_eq!(asked.bytes, plain.bytes, "the original file, not a copy of a copy");
    assert!(cache_files(&a).is_empty());
}

#[tokio::test]
async fn another_target_than_mp3_is_refused_and_the_parts_say_what_a_device_may_ask_for() {
    need_ffmpeg!();
    let a = api().await;
    let token = pair(&a, a.admin, 1).await;
    let (book, ids) = album(&a).await;
    let r = call(&a, &format!("/api/v1/parts/{}/stream?as=wav", ids["ogg"]), &token, None).await;
    assert_eq!((r.status, r.json()["error"].as_str()), (StatusCode::BAD_REQUEST, Some("unsupported_as")));

    // The part of an Ogg file says `transcode: mp3` (the iPhone cannot play it), an MP3 and a FLAC file say nothing.
    let item = call(&a, &format!("/api/v1/items/{book}"), &token, None).await.json();
    let parts = item["parts"].as_array().unwrap();
    let transcode_of = |id: i64| parts.iter().find(|p| p["id"] == id).unwrap()["transcode"].clone();
    assert_eq!(transcode_of(ids["ogg"]), json!("mp3"));
    assert!(transcode_of(ids["mp3"]).is_null() && transcode_of(ids["flac"]).is_null());
}

#[tokio::test]
async fn a_file_that_cannot_be_decoded_is_a_clear_error_and_leaves_nothing_behind() {
    need_ffmpeg!();
    let a = api().await;
    let token = pair(&a, a.admin, 1).await;
    let (_, ids) = album(&a).await;
    // The Ogg file is damaged after the import: noise instead of audio.
    let file = a.library.path().join("Duo Camillo/Caipiranha/01 - a.ogg");
    std::fs::write(&file, (0..4000u32).map(|i| (i * 7 % 251) as u8).collect::<Vec<_>>()).unwrap();
    let r = call(&a, &format!("/api/v1/parts/{}/stream?as=mp3", ids["ogg"]), &token, None).await;
    assert_eq!((r.status, r.json()["error"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("transcode_failed")), "{}", String::from_utf8_lossy(&r.bytes));
    assert!(cache_files(&a).is_empty(), "no half-made file: {:?}", cache_files(&a));
}

#[tokio::test]
async fn the_rules_of_the_original_stream_apply_to_the_transcoded_one() {
    need_ffmpeg!();
    let a = api().await;
    let kid = a.store.create_user(NewProfile::child("Kid", 8, Schedule::always())).await.unwrap();
    let token = pair(&a, kid, 2).await;
    let (_, ids) = album(&a).await;
    // The album is not released for the child: it does not exist for them, transcoded or not.
    let r = call(&a, &format!("/api/v1/parts/{}/stream?as=mp3", ids["ogg"]), &token, None).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert!(cache_files(&a).is_empty(), "nothing was made for somebody who may not hear it");
    // And without a token.
    let res = a.router.clone().oneshot(Request::builder().uri(format!("/api/v1/parts/{}/stream?as=mp3", ids["ogg"])).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_changed_file_is_made_again_and_its_old_version_goes() {
    need_ffmpeg!();
    let a = api().await;
    let token = pair(&a, a.admin, 1).await;
    let (_, ids) = album(&a).await;
    let uri = format!("/api/v1/parts/{}/stream?as=mp3", ids["flac"]);
    assert_eq!(call(&a, &uri, &token, None).await.status, StatusCode::OK);
    let before = cache_files(&a);
    assert_eq!(before.len(), 1);
    // The file is replaced by a longer one (another size).
    let file = a.library.path().join("Duo Camillo/Caipiranha/02 - b.flac");
    let out = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=330:duration=4", "-c:a", "flac"])
        .arg(&file)
        .status()
        .unwrap();
    assert!(out.success());
    let r = call(&a, &uri, &token, None).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.bytes.len() > 40_000, "the new, longer file: {} bytes", r.bytes.len());
    let after = cache_files(&a);
    assert_eq!(after.len(), 1, "{after:?}");
    assert_ne!(before, after, "another version");
}
