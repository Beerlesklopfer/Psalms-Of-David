// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! A CD that is in the library already: found by its Disc ID or by the book, checked against the table of contents (missing,
//! damaged), repaired (only the affected tracks, the damaged file in the quarantine, the names of the others). Fake drive, no internet.

#![allow(dead_code)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use common::open;
use david_server::api::{ApiState, router};
use david_server::cdimport::CdService;
use david_server::cdrive::fake::{FakeDrives, disc};
use david_server::musicbrainz::{Lookup, LookupError, Service};
use david_server::{NewProfile, Schedule, Store};
use serde_json::{Value, json};
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
    cd: CdService,
    drive: Arc<FakeDrives>,
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
    let drive = FakeDrives::with_drive("/dev/fake0");
    let mut cd = CdService::with_backend(dir.path(), fake.clone(), drive.clone());
    if let Some(h) = heartbeat {
        cd = cd.with_heartbeat(h);
    }
    // The tests never want to wait for the reserve of free space.
    store.save_settings(&[("min_free_gb".to_owned(), "0".to_owned())]).await.unwrap();
    let state = ApiState { store: store.clone(), library: Some(library.path().to_owned()), cd: cd.clone(), features: Default::default() };
    Env { router: router(state), store, fake, library, dir, cd, drive, admin, n: 0 }
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

/// The folders in the staging area (a rip that can be resumed leaves its folder `disc-<Disc ID>`).
fn staging_dirs(e: &Env) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(e.dir.path().join("cd-import")).map(|rd| rd.flatten().map(|d| d.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    v.sort();
    v
}

fn clear_staging(e: &Env) {
    let _ = std::fs::remove_dir_all(e.dir.path().join("cd-import"));
}

fn staging_is_empty(e: &Env) -> bool {
    std::fs::read_dir(e.dir.path().join("cd-import")).map(|mut rd| rd.next().is_none()).unwrap_or(true)
}

async fn set(e: &Env, pairs: &[(&str, &str)]) {
    let values: Vec<(String, String)> = pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
    e.store.save_settings(&values).await.unwrap();
}

/// A disc of `n` tracks goes into the fake drive and the server looks at it.
async fn insert(e: &Env, n: u8) {
    e.drive.edit("/dev/fake0", |u| {
        u.disc = Some(disc(n));
        u.tray_open = false;
        u.changed = true;
    });
    e.cd.refresh_drives("", true).await;
}

/// A valid MP3 in the format of the settings for the fake to "rip".
fn give_mp3(e: &Env) {
    let work = tempfile::TempDir::new().unwrap();
    *e.drive.mp3.lock().unwrap() = mp3(work.path(), "t.mp3", "24000");
}

async fn wait_job(e: &Env, token: &str, id: i64) -> Value {
    for _ in 0..300 {
        let job = call(e, "GET", &format!("/api/v1/cd/import/{id}"), Some(token), None).await.json()["job"].clone();
        if job["active"] == false {
            return job;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the job did not end");
}

fn rip_body() -> Value {
    json!({ "author": "Ken Follett", "title": "Sturz der Titanen", "disc": 2, "discs_total": 12, "tracks": ["Teil 014", "Teil 015", "Teil 016"] })
}


/// A disc of `n` tracks of 2 seconds (what the fake rip writes) is put in.
async fn insert_short(e: &Env, n: u8) {
    e.drive.edit("/dev/fake0", |u| {
        u.disc = Some(david_server::cdrive::fake::disc_sectors(n, 150));
        u.tray_open = false;
        u.changed = true;
    });
    e.cd.refresh_drives("", true).await;
}

fn disc_id_of(e: &Env) -> String {
    e.cd.drives.snapshot().0[0].disc_id.clone().unwrap()
}

/// An MP3 of the format of the settings with the given length in seconds.
fn mp3_secs(dir: &std::path::Path, name: &str, secs: u32) -> Vec<u8> {
    let path = dir.join(name);
    let status = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", &format!("sine=frequency=440:duration={secs}"), "-c:a", "libmp3lame", "-b:a", "64k", "-ar", "24000", "-ac", "2"])
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::read(path).unwrap()
}

/// The answer of MusicBrainz for the fake disc: "Sturz der Titanen" as CD 2 of 12 (one release, or two).
fn release_json(e: &Env, releases: usize) -> String {
    let mut v: Value = serde_json::from_slice(include_bytes!("data/mb-discid.json")).unwrap();
    let id = disc_id_of(e);
    let first = serde_json::to_string(&v["releases"][0]).unwrap().replace("AzCi4YfdjUrdmGcBX7URtQixDmE-", &id);
    let mut list = vec![serde_json::from_str::<Value>(&first).unwrap()];
    if releases == 2 {
        let mut second = list[0].clone();
        second["id"] = json!("0f2d4b1c-8d0a-4f17-8d7e-999999999999");
        list.push(second);
    }
    v["releases"] = json!(list);
    v.to_string()
}

fn book_dir(e: &Env) -> std::path::PathBuf {
    e.library.path().join("Ken Follett/Sturz der Titanen")
}

/// Puts hand-ripped files of CD 2 into the library (names as the old rips have them) and has the library scan them.
async fn hand_ripped(e: &Env, tracks: u32) {
    let work = tempfile::tempdir().unwrap();
    let one = mp3_secs(work.path(), "t.mp3", 2);
    std::fs::create_dir_all(book_dir(e)).unwrap();
    for n in 1..=tracks {
        std::fs::write(book_dir(e).join(format!("Follett_Sturz der Titanen_CD02_TR{n:02}.mp3")), &one).unwrap();
    }
    let root = e.library.path().canonicalize().unwrap().to_string_lossy().into_owned();
    e.store.save_import_criteria(&[root], david_server::RatingRule::Manual).await.unwrap();
    e.store.unlock_import("test").await.unwrap();
    e.store.run_import().await.unwrap();
}

async fn drive_json(e: &Env, token: &str) -> Value {
    call(e, "GET", "/api/v1/cd/drive", Some(token), None).await.json()["drives"][0].clone()
}

#[tokio::test]
async fn an_imported_cd_is_known_by_its_disc_id_and_a_new_import_is_turned_down() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    give_mp3(&e);
    set(&e, &[("cd_eject_after", "0")]).await;
    insert_short(&e, 3).await;
    assert!(drive_json(&e, &token).await["present"].is_null(), "not in the library yet");
    let body = json!({ "author": "Ken Follett", "title": "Sturz der Titanen", "disc": 2, "discs_total": 12 });
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(body.clone())).await;
    assert_eq!(r.status, StatusCode::CREATED);
    assert_eq!(wait_job(&e, &token, r.json()["id"].as_i64().unwrap()).await["phase"], "done");

    // The disc and its tracks are written down.
    let (row, tracks) = e.store.cd_disc(&disc_id_of(&e)).await.unwrap().expect("a row for the disc");
    assert_eq!((row.author.as_str(), row.title.as_str(), row.disc, row.discs_total, row.kind.as_str(), row.folder.as_str()), ("Ken Follett", "Sturz der Titanen", 2, 12, "audiobook", "Ken Follett/Sturz der Titanen"));
    assert_eq!(tracks.iter().map(|t| t.number).collect::<Vec<_>>(), [1, 2, 3]);
    assert!(tracks[0].file.ends_with("CD02_TR01.mp3") && tracks[0].sha1.starts_with("sha-") && tracks[0].seconds == 2, "{:?}", tracks[0]);

    // The drive says it is whole in the library, and a new import is not started (and is no error of overwriting).
    let p = drive_json(&e, &token).await["present"].clone();
    assert_eq!((p["via"].as_str(), p["book"].as_str(), p["author"].as_str(), p["disc"].as_u64(), p["complete"].as_bool()), (Some("disc_id"), Some("Sturz der Titanen"), Some("Ken Follett"), Some(2), Some(true)));
    assert_eq!((p["tracks_in_library"].as_u64(), p["tracks_on_disc"].as_u64(), p["missing"].clone(), p["damaged"].clone()), (Some(3), Some(3), json!([]), json!([])));
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(body)).await;
    assert_eq!((r.status, r.code().as_str()), (StatusCode::CONFLICT, "disc_present"));
    // Nothing to heal, and nothing to heal for a CD that is not there.
    let mut heal = json!({ "mode": "heal" });
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(heal.clone())).await;
    assert_eq!((r.status, r.code().as_str()), (StatusCode::CONFLICT, "nothing_to_heal"));
    insert_short(&e, 5).await;
    heal["author"] = json!("Nobody");
    heal["title"] = json!("Nothing");
    assert_eq!(call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(heal)).await.code(), "not_present");
}

#[tokio::test]
async fn a_repair_keeps_the_number_of_discs_the_import_stored() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    give_mp3(&e);
    set(&e, &[("cd_eject_after", "0")]).await;
    insert_short(&e, 3).await;
    let body = json!({ "author": "Ken Follett", "title": "Sturz der Titanen", "disc": 2, "discs_total": 12 });
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(body)).await;
    assert_eq!(wait_job(&e, &token, r.json()["id"].as_i64().unwrap()).await["phase"], "done");
    std::fs::remove_file(book_dir(&e).join("Follett_Sturz der Titanen_CD02_TR02.mp3")).ok();
    let (_, tracks) = e.store.cd_disc(&disc_id_of(&e)).await.unwrap().unwrap();
    std::fs::remove_file(e.library.path().join(&tracks[1].file)).unwrap();
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(json!({ "mode": "heal" }))).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", String::from_utf8_lossy(&r.bytes));
    let job = wait_job(&e, &token, r.json()["id"].as_i64().unwrap()).await;
    assert_eq!((job["disc"].as_i64(), job["discs_total"].as_i64()), (Some(2), Some(12)), "the repair is of CD 2 of 12, not of 1: {job}");
}

#[tokio::test]
async fn a_repair_of_a_cd_whose_stored_total_is_smaller_than_its_number_still_starts() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    give_mp3(&e);
    set(&e, &[("cd_eject_after", "0")]).await;
    insert_short(&e, 3).await;
    // An earlier import did not know the number of CDs and stored 1 for CD 2.
    let body = json!({ "author": "Ken Follett", "title": "Sturz der Titanen", "disc": 2, "discs_total": 12 });
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(body)).await;
    assert_eq!(wait_job(&e, &token, r.json()["id"].as_i64().unwrap()).await["phase"], "done");
    let (mut row, tracks) = e.store.cd_disc(&disc_id_of(&e)).await.unwrap().unwrap();
    row.discs_total = 1;
    // Another CD of the same book was imported with the real number of CDs.
    let mut other = row.clone();
    other.disc_id = "other-disc-id-of-the-same-book".into();
    other.disc = 3;
    other.discs_total = 12;
    e.store.record_cd_disc(&david_server::cdpresent::DiscRecord { row: other, tracks: Vec::new() }).await.unwrap();
    e.store.record_cd_disc(&david_server::cdpresent::DiscRecord { row, tracks: tracks.clone() }).await.unwrap();
    std::fs::remove_file(e.library.path().join(&tracks[1].file)).unwrap();
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(json!({ "mode": "heal" }))).await;
    assert_eq!(r.status, StatusCode::CREATED, "the repair is not refused with invalid_meta: {}", String::from_utf8_lossy(&r.bytes));
    let job = wait_job(&e, &token, r.json()["id"].as_i64().unwrap()).await;
    assert_eq!((job["disc"].as_i64(), job["discs_total"].as_i64()), (Some(2), Some(12)), "the number of CDs comes from the other CD of the book: {job}");
}

#[tokio::test]
async fn a_hand_ripped_cd_is_found_by_the_book_and_the_disc_number_the_one_release_names() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    set(&e, &[("musicbrainz_contact", "family@example.org")]).await;
    insert_short(&e, 3).await;
    hand_ripped(&e, 3).await;
    e.fake.on("/ws/2/discid/", Some(release_json(&e, 1).as_bytes()));
    let p = drive_json(&e, &token).await["present"].clone();
    assert_eq!((p["via"].as_str(), p["book"].as_str(), p["author"].as_str(), p["disc"].as_u64(), p["complete"].as_bool()), (Some("book"), Some("Sturz der Titanen"), Some("Ken Follett"), Some(2), Some(true)), "{p}");
    // CD 1 of the same book is another disc: it is not found in the files of CD 2.
    e.fake.answers.lock().unwrap().clear();
    let mut v: Value = serde_json::from_str(&release_json(&e, 1)).unwrap();
    let id = disc_id_of(&e);
    v["releases"][0]["media"][0]["discs"] = json!([{ "id": id, "offsets": [150], "sectors": 1000, "offset-count": 1 }]);
    v["releases"][0]["media"][1]["discs"] = json!([]);
    sqlx_clear(&e).await;
    e.fake.on("/ws/2/discid/", Some(v.to_string().as_bytes()));
    assert!(drive_json(&e, &token).await["present"].is_null(), "no tracks of CD 1 in the library");
    // Without MusicBrainz (no contact address) nothing is guessed from the book.
    set(&e, &[("musicbrainz_contact", "")]).await;
    assert!(drive_json(&e, &token).await["present"].is_null());
}

async fn sqlx_clear(e: &Env) {
    e.store.cd_cache_clear().await.unwrap();
}

#[tokio::test]
async fn the_check_names_what_is_missing_and_what_is_damaged() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    set(&e, &[("musicbrainz_contact", "family@example.org")]).await;
    insert_short(&e, 5).await;
    hand_ripped(&e, 5).await;
    e.fake.on("/ws/2/discid/", Some(release_json(&e, 1).as_bytes()));
    let work = tempfile::tempdir().unwrap();
    let name = |n: u32| book_dir(&e).join(format!("Follett_Sturz der Titanen_CD02_TR{n:02}.mp3"));
    std::fs::remove_file(name(2)).unwrap(); // missing
    std::fs::write(name(3), b"").unwrap(); // empty
    std::fs::write(name(4), vec![9u8; 4000]).unwrap(); // not audio
    std::fs::write(name(5), mp3_secs(work.path(), "long.mp3", 9)).unwrap(); // 9 seconds instead of 2: too long
    // (track 1 is whole)
    let p = drive_json(&e, &token).await["present"].clone();
    assert_eq!(p["missing"], json!([2]), "{p}");
    let damaged: Vec<(u64, &str)> = p["damaged"].as_array().unwrap().iter().map(|d| (d["number"].as_u64().unwrap(), d["reason"].as_str().unwrap())).collect();
    assert_eq!(damaged, [(3, "empty"), (4, "unreadable"), (5, "length")]);
    assert_eq!((p["damaged"][2]["have_s"].as_u64(), p["damaged"][2]["want_s"].as_u64()), (Some(9), Some(2)));
    assert_eq!((p["tracks_in_library"].as_u64(), p["tracks_on_disc"].as_u64(), p["complete"].as_bool()), (Some(4), Some(5), Some(false)));
    // The tolerance is a setting: a file that is 2 seconds too long passes with the default of 3 and not with 1.
    std::fs::write(name(5), mp3_secs(work.path(), "four.mp3", 4)).unwrap();
    assert!(!drive_json(&e, &token).await["present"]["damaged"].as_array().unwrap().iter().any(|d| d["number"] == 5));
    set(&e, &[("cd_heal_tolerance_s", "1")]).await;
    assert!(drive_json(&e, &token).await["present"]["damaged"].as_array().unwrap().iter().any(|d| d["number"] == 5));
}

#[tokio::test]
async fn healing_reads_only_the_affected_tracks_names_them_like_the_others_and_puts_the_damaged_file_aside() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    set(&e, &[("musicbrainz_contact", "family@example.org"), ("cd_eject_after", "0")]).await;
    give_mp3(&e);
    insert_short(&e, 5).await;
    hand_ripped(&e, 5).await;
    e.fake.on("/ws/2/discid/", Some(release_json(&e, 1).as_bytes()));
    let name = |n: u32| book_dir(&e).join(format!("Follett_Sturz der Titanen_CD02_TR{n:02}.mp3"));
    let untouched = std::fs::read(name(1)).unwrap();
    std::fs::remove_file(name(2)).unwrap();
    std::fs::write(name(4), vec![9u8; 4000]).unwrap();

    // The import of this CD is not started; healing is, and it reads tracks 2 and 4 only.
    let body = json!({ "author": "Ken Follett", "title": "Sturz der Titanen", "disc": 2 });
    assert_eq!(call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(body)).await.code(), "disc_present");
    e.drive.reads.lock().unwrap().clear();
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(json!({ "mode": "heal" }))).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", String::from_utf8_lossy(&r.bytes));
    assert_eq!(r.json()["job"]["tracks_total"], 2);
    let job = wait_job(&e, &token, r.json()["id"].as_i64().unwrap()).await;
    assert_eq!((job["phase"].as_str(), job["track"].as_i64()), (Some("done"), Some(2)), "{job}");
    assert_eq!(*e.drive.reads.lock().unwrap(), vec![2, 4]);

    // The files have the names of the others (`Follett_...`, not `Ken Follett_...`), in the same folder; the whole CD is there.
    assert!(name(2).is_file() && name(4).is_file());
    assert!(david_server::tags::read(&name(2)).unwrap().duration_ms > 1000);
    assert_eq!(std::fs::read(name(1)).unwrap(), untouched, "a whole file is not touched");
    let names: Vec<String> = std::fs::read_dir(book_dir(&e)).unwrap().flatten().map(|d| d.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".mp3")).collect();
    assert_eq!(names.len(), 5, "{names:?}");
    assert!(names.iter().all(|n| n.starts_with("Follett_Sturz der Titanen_CD02_TR")), "{names:?}");
    // The damaged file is in the quarantine with its content, not deleted.
    let aside: Vec<_> = std::fs::read_dir(e.library.path().join(".quarantine/Ken Follett/Sturz der Titanen")).unwrap().flatten().collect();
    assert_eq!(aside.len(), 1);
    assert_eq!(std::fs::read(aside[0].path()).unwrap(), vec![9u8; 4000]);
    assert!(aside[0].file_name().to_string_lossy().starts_with("Follett_Sturz der Titanen_CD02_TR04.mp3."));
    // The disc is written down whole, and the CD is whole now.
    let (_, tracks) = e.store.cd_disc(&disc_id_of(&e)).await.unwrap().unwrap();
    assert_eq!(tracks.iter().map(|t| t.number).collect::<Vec<_>>(), [1, 2, 3, 4, 5]);
    assert_eq!(drive_json(&e, &token).await["present"]["complete"], true);
    assert!(staging_is_empty(&e));
}

#[tokio::test]
async fn a_track_that_cannot_be_read_is_told_and_the_others_are_healed_all_the_same() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    set(&e, &[("musicbrainz_contact", "family@example.org"), ("cd_eject_after", "0")]).await;
    give_mp3(&e);
    insert_short(&e, 5).await;
    hand_ripped(&e, 5).await;
    e.fake.on("/ws/2/discid/", Some(release_json(&e, 1).as_bytes()));
    let name = |n: u32| book_dir(&e).join(format!("Follett_Sturz der Titanen_CD02_TR{n:02}.mp3"));
    std::fs::remove_file(name(2)).unwrap();
    std::fs::remove_file(name(3)).unwrap();
    *e.drive.fail_at.lock().unwrap() = Some((3, "read")); // track 3 is scratched
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(json!({ "mode": "heal" }))).await;
    let job = wait_job(&e, &token, r.json()["id"].as_i64().unwrap()).await;
    assert_eq!((job["phase"].as_str(), job["error"].as_str()), (Some("done"), Some("unreadable_track")), "{job}");
    assert!(job["message"].as_str().unwrap().contains("unreadable tracks: 3"), "{job}");
    assert!(name(2).is_file() && !name(3).exists(), "track 2 was healed, track 3 stays missing");
}

#[tokio::test]
async fn rights_of_the_check_and_the_repair() {
    let mut e = env().await;
    let kid = e.store.create_user(NewProfile::child("Kid", 10, Schedule::always())).await.unwrap();
    let kid_token = pair(&mut e, kid).await;
    for body in [json!({ "mode": "heal" }), json!({ "author": "A", "title": "B" })] {
        let r = call(&e, "POST", "/api/v1/cd/rip", Some(&kid_token), Some(body)).await;
        assert_eq!((r.status, r.code().as_str()), (StatusCode::FORBIDDEN, "forbidden"));
    }
    assert_eq!(call(&e, "GET", "/api/v1/cd/drive", Some(&kid_token), None).await.status, StatusCode::FORBIDDEN);
}
