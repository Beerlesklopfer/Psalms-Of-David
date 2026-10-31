// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The CD drive of the server: state, rights, lookup of the inserted disc, ripping, cancelling, ejecting and the push
//! of the state to the devices. The drive is a fake (tray, disc, errors set by the test); there is no internet.

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

#[tokio::test]
async fn only_administrators_and_members_use_the_drive() {
    let mut e = env().await;
    let kid = e.store.create_user(NewProfile::child("Kid", 10, Schedule::always())).await.unwrap();
    let kid_token = pair(&mut e, kid).await;
    for (method, uri, body) in [
        ("GET", "/api/v1/cd/drive", None),
        ("POST", "/api/v1/cd/rip", Some(rip_body())),
        ("POST", "/api/v1/cd/rip/1/cancel", Some(json!({}))),
        ("POST", "/api/v1/cd/eject", Some(json!({}))),
    ] {
        let r = call(&e, method, uri, Some(&kid_token), body.clone()).await;
        assert_eq!((r.status, r.code().as_str()), (StatusCode::FORBIDDEN, "forbidden"), "{uri}");
        assert_eq!(call(&e, method, uri, None, body).await.status, StatusCode::UNAUTHORIZED, "{uri}");
    }
    insert(&e, 3).await;
    let admin = pair_admin(&mut e).await;
    assert_eq!(call(&e, "GET", "/api/v1/cd/drive", Some(&admin), None).await.status, StatusCode::OK);
}


#[tokio::test]
async fn the_drive_state_is_served_and_pushed_when_the_tray_or_the_disc_changes() {
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    let mut events = e.cd.subscribe();
    e.cd.refresh_drives("", true).await;
    let r = call(&e, "GET", "/api/v1/cd/drive", Some(&token), None).await.json();
    assert_eq!((r["mode"].as_str(), r["busy"].as_bool()), (Some("poll"), Some(false)));
    let d = &r["drives"][0];
    assert_eq!((d["device"].as_str(), d["tray"].as_str(), d["medium"].as_str(), d["tracks"].as_u64()), (Some("/dev/fake0"), Some("closed"), Some("none"), Some(0)));

    // The first look was a change from "nothing known": a message went out. A look that changes nothing sends none.
    let first: Value = serde_json::from_str(&events.recv().await.unwrap().text).unwrap();
    assert_eq!((first["type"].as_str(), first["v"].as_u64(), first["drives"][0]["medium"].as_str()), (Some("drive"), Some(1), Some("none")));
    e.cd.refresh_drives("", false).await;
    assert!(events.try_recv().is_err(), "no change, no message");

    insert(&e, 3).await;
    let m: Value = serde_json::from_str(&events.recv().await.unwrap().text).unwrap();
    assert_eq!((m["drives"][0]["medium"].as_str(), m["drives"][0]["tracks"].as_u64(), m["drives"][0]["tray"].as_str()), (Some("audio"), Some(3), Some("closed")));
    let disc_id = m["drives"][0]["disc_id"].as_str().unwrap().to_owned();
    assert_eq!(disc_id.len(), 28);

    // The tray opens (the eject button): the disc is gone from the state and the drive is not read.
    let reads = e.drive.toc_reads.load(std::sync::atomic::Ordering::Relaxed);
    e.drive.edit("/dev/fake0", |u| u.tray_open = true);
    e.cd.refresh_drives("", false).await;
    let m: Value = serde_json::from_str(&events.recv().await.unwrap().text).unwrap();
    assert_eq!((m["drives"][0]["tray"].as_str(), m["drives"][0]["medium"].as_str()), (Some("open"), Some("none")));
    assert_eq!(e.drive.toc_reads.load(std::sync::atomic::Ordering::Relaxed), reads);

    // A drive the service may not use is reported, not hidden.
    e.drive.edit("/dev/fake0", |u| u.denied = true);
    e.cd.refresh_drives("", false).await;
    assert_eq!(call(&e, "GET", "/api/v1/cd/drive", Some(&token), None).await.json()["drives"][0]["denied"], true);
}

#[tokio::test]
async fn the_monitor_follows_the_kernel_events_when_udev_is_on_and_the_interval_when_it_is_off() {
    let e = env().await;
    set(&e, &[("cd_udev", "1")]).await;
    let monitor = tokio::spawn(david_server::cdrive::monitor(e.cd.clone(), e.store.clone(), Some(e.library.path().to_owned())));
    // The first look, then an event from the kernel: the disc appears without any interval running.
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(e.cd.drives.snapshot().1, "udev");
    e.drive.edit("/dev/fake0", |u| {
        u.disc = Some(disc(2));
        u.changed = true;
    });
    let mut seen = false;
    for _ in 0..40 {
        if e.cd.drives.snapshot().0.first().is_some_and(|d| d.tracks == 2) {
            seen = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(seen, "the event made the server look");

    // Switched off at run time: events are not listened to any more, the interval takes over (no restart).
    set(&e, &[("cd_udev", "0"), ("cd_poll_seconds", "2")]).await;
    tokio::time::sleep(Duration::from_millis(1700)).await;
    assert_eq!(e.cd.drives.snapshot().1, "poll");
    e.drive.units.lock().unwrap()[0].1.disc = Some(disc(4));
    e.drive.units.lock().unwrap()[0].1.changed = true;
    let mut seen = false;
    for _ in 0..60 {
        if e.cd.drives.snapshot().0.first().is_some_and(|d| d.tracks == 4) {
            seen = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(seen, "the interval made the server look");
    monitor.abort();
}

#[tokio::test]
async fn the_lookup_reads_the_disc_in_the_drive_when_the_caller_sends_no_toc() {
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    set(&e, &[("musicbrainz_contact", "family@example.org")]).await;
    e.fake.on("/ws/2/discid/", Some(include_bytes!("data/mb-discid.json")));
    let r = call(&e, "POST", "/api/v1/cd/lookup", Some(&token), Some(json!({}))).await;
    assert_eq!((r.status, r.code().as_str()), (StatusCode::CONFLICT, "no_disc"));
    insert(&e, 3).await;
    let r = call(&e, "POST", "/api/v1/cd/lookup", Some(&token), Some(json!({}))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", String::from_utf8_lossy(&r.bytes));
    let state_id = call(&e, "GET", "/api/v1/cd/drive", Some(&token), None).await.json()["drives"][0]["disc_id"].clone();
    assert_eq!(r.json()["disc_id"], state_id, "the id is the one of the disc in the drive");
    // Not while the tray is open.
    e.drive.edit("/dev/fake0", |u| u.tray_open = true);
    e.cd.refresh_drives("", false).await;
    assert_eq!(call(&e, "POST", "/api/v1/cd/lookup", Some(&token), Some(json!({}))).await.code(), "no_disc");
}

#[tokio::test]
async fn a_cd_is_ripped_by_the_server_placed_in_the_library_and_the_tray_opened() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    give_mp3(&e);
    insert(&e, 3).await;
    let mut events = e.cd.subscribe();
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(json!({ "author": "Ken Follett", "title": "Sturz der Titanen", "disc": 1, "discs_total": 1, "tracks": ["Teil 014", "Teil 015", "Teil 016"], "eject": true }))).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", String::from_utf8_lossy(&r.bytes));
    let id = r.json()["id"].as_i64().unwrap();
    let job = wait_job(&e, &token, id).await;
    assert_eq!((job["phase"].as_str(), job["state"].as_str(), job["tracks_total"].as_i64(), job["track"].as_i64()), (Some("done"), Some("done"), Some(3), Some(3)), "{job}");

    let book = e.library.path().join("Ken Follett/Sturz der Titanen");
    for n in 1..=3 {
        let f = book.join(format!("Ken Follett_Sturz der Titanen_CD01_TR{n:02}.mp3"));
        let t = david_server::tags::read(&f).unwrap_or_else(|err| panic!("{}: {err}", f.display()));
        assert_eq!((t.track, t.disc, t.genre.as_deref(), t.tag_kind, t.artist.as_deref()), (Some(100 + n), Some(1), Some("Audiobook"), Some("ID3v2"), Some("Ken Follett")));
    }
    assert!(staging_is_empty(&e), "nothing is left in the staging area");

    // The tray was opened once, after the job; the state says so.
    let mut drive_open = false;
    for _ in 0..100 {
        if call(&e, "GET", "/api/v1/cd/drive", Some(&token), None).await.json()["drives"][0]["tray"] == "open" {
            drive_open = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(drive_open);
    assert_eq!(e.drive.ejects.lock().unwrap().len(), 1);
    // The events had the job and its progress, and the drive.
    let mut types = Vec::new();
    while let Ok(ev) = events.try_recv() {
        let v: Value = serde_json::from_str(&ev.text).unwrap();
        types.push(v["type"].as_str().unwrap().to_owned());
    }
    for t in ["job", "progress", "done", "drive"] {
        assert!(types.iter().any(|x| x == t), "{t} in {types:?}");
    }
}

#[tokio::test]
async fn the_second_cd_of_a_work_with_the_settings_of_the_tab_and_without_eject() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    give_mp3(&e);
    set(&e, &[("cd_eject_after", "0"), ("cd_verify_read", "0")]).await;
    insert_short(&e, 2).await;
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(rip_body())).await;
    let job = wait_job(&e, &token, r.json()["id"].as_i64().unwrap()).await;
    assert_eq!(job["phase"], "done", "{job}");
    // Disc 2 of 12 goes into its disc folder; the setting `cd_eject_after` is off, so the tray stays shut.
    assert!(e.library.path().join("Ken Follett/Sturz der Titanen/CD 2/Ken Follett_Sturz der Titanen_CD02_TR01.mp3").is_file());
    assert!(e.drive.ejects.lock().unwrap().is_empty());
    assert_eq!(call(&e, "GET", "/api/v1/cd/drive", Some(&token), None).await.json()["drives"][0]["tray"], "closed");
    // The CD is in the library now: a new import is turned down (not as an error of overwriting) with what is known of it.
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(rip_body())).await;
    assert_eq!((r.status, r.code().as_str()), (StatusCode::CONFLICT, "disc_present"));
    let j = r.json();
    assert_eq!((j["book"].as_str(), j["author"].as_str(), j["disc"].as_u64(), j["missing"].clone(), j["damaged"].clone()), (Some("Sturz der Titanen"), Some("Ken Follett"), Some(2), json!([]), json!([])));
    assert_eq!((j["present"]["tracks_in_library"].as_u64(), j["present"]["tracks_on_disc"].as_u64(), j["present"]["complete"].as_bool()), (Some(2), Some(2), Some(true)));
    // Overwriting stays forbidden when it is asked for anyway: refused before the disc is read, with the numbers that are there.
    let mut forced = rip_body();
    forced["force"] = json!(true);
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(forced)).await;
    assert_eq!((r.status, r.code().as_str(), r.json()["taken"].clone()), (StatusCode::CONFLICT, "disc_exists", json!([1, 2])));
    let log = call(&e, "GET", "/api/v1/cd/jobs", Some(&token), None).await.json();
    assert_eq!((log["jobs"][0]["error"].as_str(), log["jobs"][0]["message"].as_str()), (Some("disc_exists"), Some("tracks already in the library: 1, 2")));
    assert!(staging_is_empty(&e));
}

#[tokio::test]
async fn only_one_rip_per_drive_it_can_be_cancelled_and_nothing_reaches_the_library() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    give_mp3(&e);
    *e.drive.hold_after.lock().unwrap() = Some(1);
    insert(&e, 3).await;
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(rip_body())).await;
    assert_eq!(r.status, StatusCode::CREATED);
    let id = r.json()["id"].as_i64().unwrap();
    // While it runs: a second rip is refused, an eject too, and the drive says it is busy.
    let second = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(rip_body())).await;
    assert_eq!((second.status, second.code().as_str()), (StatusCode::CONFLICT, "busy"));
    let eject = call(&e, "POST", "/api/v1/cd/eject", Some(&token), Some(json!({}))).await;
    assert_eq!((eject.status, eject.code().as_str()), (StatusCode::CONFLICT, "busy"));
    assert!(e.drive.ejects.lock().unwrap().is_empty());
    let state = call(&e, "GET", "/api/v1/cd/drive", Some(&token), None).await.json();
    assert_eq!((state["busy"].as_bool(), state["drives"][0]["ripping"].as_bool()), (Some(true), Some(true)));

    let c = call(&e, "POST", &format!("/api/v1/cd/rip/{id}/cancel"), Some(&token), Some(json!({}))).await;
    assert_eq!(c.status, StatusCode::ACCEPTED);
    let job = wait_job(&e, &token, id).await;
    assert_eq!((job["phase"].as_str(), job["error"].as_str()), (Some("failed"), Some("cancelled")), "{job}");
    assert_eq!(std::fs::read_dir(e.library.path()).unwrap().count(), 0, "nothing reached the library");
    // What was finished stays for a resume (and nothing but the folder of the disc).
    assert!(staging_dirs(&e).iter().all(|d| d.starts_with("disc-")) && staging_dirs(&e).len() == 1, "{:?}", staging_dirs(&e));
    // A job that is not running cannot be cancelled; the drive is free again.
    assert_eq!(call(&e, "POST", &format!("/api/v1/cd/rip/{id}/cancel"), Some(&token), Some(json!({}))).await.status, StatusCode::NOT_FOUND);
    *e.drive.hold_after.lock().unwrap() = None;
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(rip_body())).await;
    assert_eq!(r.status, StatusCode::CREATED);
    wait_job(&e, &token, r.json()["id"].as_i64().unwrap()).await;
}

#[tokio::test]
async fn a_track_that_cannot_be_read_is_left_out_and_the_rest_is_imported_but_other_failures_fail_the_job_with_a_code_and_leave_nothing() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    give_mp3(&e);
    insert(&e, 3).await;
    for (kind, code) in [("drive_gone", "drive_gone"), ("encode", "encode"), ("disk_full", "no_space"), ("io", "io")] {
        *e.drive.fail_at.lock().unwrap() = Some((2, kind));
        let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(rip_body())).await;
        assert_eq!(r.status, StatusCode::CREATED, "{kind}");
        let job = wait_job(&e, &token, r.json()["id"].as_i64().unwrap()).await;
        assert_eq!((job["phase"].as_str(), job["error"].as_str(), job["track"].as_i64()), (Some("failed"), Some(code), Some(1)), "{kind}: {job}");
        assert_eq!(std::fs::read_dir(e.library.path()).unwrap().count(), 0, "{kind}: the library is untouched");
        // After a vanishing drive the finished tracks stay for a resume; after the other failures nothing stays.
        let resumable = matches!(kind, "drive_gone");
        assert_eq!(staging_dirs(&e).len(), usize::from(resumable), "{kind}: {:?}", staging_dirs(&e));
        clear_staging(&e);
    }
    // A track that cannot be read or confirmed does not end the rip: it is left out, reported as unreadable, and the other tracks are imported
    // ("Check and repair" reads the missing one again). (project owner, 2026-10-29)
    for (n, kind) in ["read", "verify"].into_iter().enumerate() {
        *e.drive.fail_at.lock().unwrap() = Some((2, kind));
        // A CD of its own each time (the first one is in the library now), put in again (the tray was opened after the import).
        insert(&e, 3).await;
        let mut body = rip_body();
        body["disc"] = json!(5 + n);
        body["force"] = json!(true); // the same disc is in the library already from the case before
        let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(body)).await;
        assert_eq!(r.status, StatusCode::CREATED, "{kind}: {}", String::from_utf8_lossy(&r.bytes));
        let job = wait_job(&e, &token, r.json()["id"].as_i64().unwrap()).await;
        assert_eq!((job["phase"].as_str(), job["error"].as_str()), (Some("done"), Some("unreadable_track")), "{kind}: {job}");
        let states: Vec<&str> = job["track_states"].as_array().unwrap().iter().map(|t| t["state"].as_str().unwrap()).collect();
        assert_eq!(states, ["done", "failed", "done"], "{kind}: only the track that failed is marked: {job}");
        assert!(std::fs::read_dir(e.library.path()).unwrap().count() > 0, "{kind}: the other tracks reached the library");
        assert!(job["message"].as_str().unwrap().contains("unreadable tracks: 2"), "{kind}: {job}");
        assert!(staging_is_empty(&e), "{kind}: {:?}", staging_dirs(&e));
        clear_staging(&e);
    }
    std::fs::remove_dir_all(e.library.path().join("Ken Follett")).ok();
}

#[tokio::test]
async fn a_rip_needs_a_disc_a_shut_tray_and_a_drive_the_service_may_use() {
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(rip_body())).await;
    assert_eq!((r.status, r.code().as_str()), (StatusCode::CONFLICT, "no_disc"));
    insert(&e, 3).await;
    e.drive.edit("/dev/fake0", |u| u.tray_open = true);
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(rip_body())).await;
    assert_eq!((r.status, r.code().as_str()), (StatusCode::CONFLICT, "tray_open"));
    e.drive.edit("/dev/fake0", |u| {
        u.tray_open = false;
        u.denied = true;
    });
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(rip_body())).await;
    assert_eq!((r.status, r.code().as_str()), (StatusCode::SERVICE_UNAVAILABLE, "drive_denied"));
    e.drive.edit("/dev/fake0", |u| u.denied = false);
    // The setting names a drive that is not there.
    set(&e, &[("cd_device", "/dev/other")]).await;
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(rip_body())).await;
    assert_eq!((r.status, r.code().as_str()), (StatusCode::CONFLICT, "no_drive"));
    set(&e, &[("cd_device", "")]).await;
    // Details that are not valid are refused before the drive is touched.
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(json!({ "author": "", "title": "x" }))).await;
    assert_eq!((r.status, r.code().as_str()), (StatusCode::UNPROCESSABLE_ENTITY, "invalid_meta"));
    assert!(e.drive.units.lock().unwrap()[0].1.disc.is_some());
}

#[tokio::test]
async fn eject_opens_a_closed_tray_with_a_disc_and_never_closes_an_open_one() {
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    // No disc: nothing is sent.
    e.cd.refresh_drives("", true).await;
    let r = call(&e, "POST", "/api/v1/cd/eject", Some(&token), None).await;
    assert_eq!((r.status, r.json()["result"].as_str()), (StatusCode::OK, Some("nothing_to_eject")));
    insert(&e, 2).await;
    let r = call(&e, "POST", "/api/v1/cd/eject", Some(&token), Some(json!({}))).await;
    assert_eq!(r.json()["result"], "ejected");
    // The tray is open: a second request must not toggle it shut.
    let r = call(&e, "POST", "/api/v1/cd/eject", Some(&token), Some(json!({}))).await;
    assert_eq!(r.json()["result"], "nothing_to_eject");
    assert_eq!(e.drive.ejects.lock().unwrap().len(), 1);
    assert!(e.drive.units.lock().unwrap()[0].1.tray_open);
    // Somebody opened the tray by hand a moment ago and the last look does not know yet: the real state wins.
    insert(&e, 2).await;
    e.drive.units.lock().unwrap()[0].1.tray_open = true;
    let r = call(&e, "POST", "/api/v1/cd/eject", Some(&token), Some(json!({}))).await;
    assert_eq!(r.json()["result"], "nothing_to_eject");
    assert_eq!(e.drive.ejects.lock().unwrap().len(), 1);
    // No drive at all.
    let r = call(&e, "POST", "/api/v1/cd/eject", Some(&token), Some(json!({ "device": "/dev/none" }))).await;
    assert_eq!(r.code(), "no_drive");
}

#[tokio::test]
async fn the_drive_lists_the_tracks_with_numbers_and_lengths() {
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    insert(&e, 3).await;
    let d = call(&e, "GET", "/api/v1/cd/drive", Some(&token), None).await.json()["drives"][0].clone();
    assert_eq!((d["tracks"].as_u64(), d["seconds"].as_u64()), (Some(3), Some(360)));
    assert_eq!(d["track_list"], json!([{ "number": 1, "seconds": 120 }, { "number": 2, "seconds": 120 }, { "number": 3, "seconds": 120 }]));
}

#[tokio::test]
async fn a_choice_of_tracks_keeps_the_numbers_of_the_disc_and_may_be_completed_later() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    give_mp3(&e);
    set(&e, &[("cd_eject_after", "0")]).await;
    insert_short(&e, 4).await;
    let book = e.library.path().join("Ken Follett/Sturz der Titanen");
    let body = |only: Value| {
        let mut b = rip_body();
        b["only"] = only;
        b["disc"] = json!(1);
        b["discs_total"] = json!(1);
        b["tracks"] = json!(["Teil 1", "Teil 2", "Teil 3", "Teil 4"]);
        b["force"] = json!(true);
        b
    };
    // Tracks 1 and 3 of 4: the files keep their numbers, there is a gap, the progress counts the two.
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(body(json!([3, 1])))).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", String::from_utf8_lossy(&r.bytes));
    assert_eq!(r.json()["job"]["tracks_total"], 2);
    let job = wait_job(&e, &token, r.json()["id"].as_i64().unwrap()).await;
    assert_eq!((job["phase"].as_str(), job["track"].as_i64(), job["tracks_total"].as_i64()), (Some("done"), Some(2), Some(2)), "{job}");
    for (n, there) in [(1, true), (2, false), (3, true), (4, false)] {
        let f = book.join(format!("Ken Follett_Sturz der Titanen_CD01_TR{n:02}.mp3"));
        assert_eq!(f.is_file(), there, "track {n}");
        if there {
            assert_eq!(david_server::tags::read(&f).unwrap().track, Some(100 + n));
        }
    }
    // A choice that names a track that is there already is refused with the numbers; nothing is read or changed.
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(body(json!([2, 3])))).await;
    assert_eq!((r.status, r.code().as_str(), r.json()["taken"].clone()), (StatusCode::CONFLICT, "disc_exists", json!([3])));
    // The free ones are taken over later.
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(body(json!([2, 4])))).await;
    assert_eq!(r.status, StatusCode::CREATED);
    let job = wait_job(&e, &token, r.json()["id"].as_i64().unwrap()).await;
    assert_eq!(job["phase"], "done", "{job}");
    for n in 1..=4 {
        assert!(book.join(format!("Ken Follett_Sturz der Titanen_CD01_TR{n:02}.mp3")).is_file(), "track {n}");
    }
    assert!(staging_is_empty(&e));
}

#[tokio::test]
async fn a_choice_that_is_empty_double_or_not_on_the_disc_is_refused() {
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    insert(&e, 3).await;
    for (only, code) in [(json!([]), "empty_selection"), (json!([1, 1]), "invalid_selection"), (json!([4]), "invalid_selection"), (json!([0]), "invalid_selection"), (json!([1, 7]), "invalid_selection")] {
        let mut b = rip_body();
        b["only"] = only.clone();
        let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(b)).await;
        assert_eq!((r.status, r.code().as_str()), (StatusCode::UNPROCESSABLE_ENTITY, code), "{only}");
    }
    // Nothing started: the drive is free and untouched.
    assert_eq!(call(&e, "GET", "/api/v1/cd/drive", Some(&token), None).await.json()["busy"], false);
    // For files that are handed over (an upload) a gap is still an error.
    let work = tempfile::TempDir::new().unwrap();
    if common::audio::ffmpeg_available() {
        let meta = json!({ "author": "A", "title": "B", "tracks": [{ "number": 1, "file": "a.mp3" }, { "number": 3, "file": "b.mp3" }] });
        let r = upload(&e, &token, vec![Part::Text("meta", meta.to_string()), Part::File("file", "a.mp3", mp3(work.path(), "a.mp3", "24000")), Part::File("file", "b.mp3", mp3(work.path(), "b.mp3", "24000"))]).await;
        assert_eq!((r.status, r.code().as_str()), (StatusCode::UNPROCESSABLE_ENTITY, "track_numbers"));
    }
}

#[tokio::test]
async fn the_panel_takes_audiobook_from_the_release_and_the_setting_otherwise_and_prefills_a_single_release() {
    use david_server::admin::{i18n::Lang, pages::{AdminUi, PanelQuery, View}};
    let mut e = env().await;
    let _ = pair_admin(&mut e).await;
    set(&e, &[("musicbrainz_contact", "family@example.org"), ("cd_target_kind", "music")]).await;
    insert(&e, 3).await;
    let id = e.cd.drives.snapshot().0[0].disc_id.clone().unwrap();
    let json_for = |speaker: bool, group: Option<&str>| {
        let mut v: Value = serde_json::from_slice(include_bytes!("data/mb-discid.json")).unwrap();
        let mut text = v.to_string().replace("AzCi4YfdjUrdmGcBX7URtQixDmE-", &id);
        v = serde_json::from_str(&text).unwrap();
        if !speaker {
            v["releases"][0]["artist-credit"] = json!([{ "name": "Some Band", "joinphrase": "" }]);
        }
        if let Some(g) = group {
            v["releases"][0]["release-group"] = json!({ "primary-type": "Other", "secondary-types": [g] });
        }
        text = v.to_string();
        text
    };
    let panel = |e: &Env| {
        let (store, cd) = (e.store.clone(), e.cd.clone());
        let ui = AdminUi::new().unwrap();
        async move { ui.cd_drive_fragment(&store, &cd, View::new(Lang::En), &PanelQuery::default(), None, None).await.unwrap().unwrap() }
    };
    // A release with a speaker is an audiobook although the setting says music; its titles fill the list.
    e.fake.answers.lock().unwrap().clear();
    e.fake.on("/ws/2/discid/", Some(json_for(true, None).as_bytes()));
    let html = panel(&e).await;
    assert!(html.contains("<option value=\"audiobook\" selected>"), "{html}");
    assert!(html.contains("Teil 014") && html.contains("Teil 016") && html.contains("name=\"author\" value=\"Ken Follett\""), "{html}");
    assert!(html.matches("checked").count() >= 3, "all tracks are ticked at first");
    // The type of the release group alone does it too.
    e.fake.answers.lock().unwrap().clear();
    e.store.cd_cache_put(&format!("mb:discid:{id}"), None).await.unwrap();
    e.fake.on("/ws/2/discid/", Some(json_for(false, Some("Audiobook")).as_bytes()));
    // (the cache holds the earlier answer; a fresh key is made by another contact-free path: clear it)
    sqlx_clear(&e).await;
    let html = panel(&e).await;
    assert!(html.contains("<option value=\"audiobook\" selected>"), "{html}");
    // No speaker, no type: the setting decides.
    e.fake.answers.lock().unwrap().clear();
    sqlx_clear(&e).await;
    e.fake.on("/ws/2/discid/", Some(json_for(false, None).as_bytes()));
    let html = panel(&e).await;
    assert!(html.contains("<option value=\"music\" selected>"), "{html}");
    // Without a release the setting decides as well, and the default setting is audiobook.
    set(&e, &[("cd_target_kind", "audiobook")]).await;
    e.fake.answers.lock().unwrap().clear();
    sqlx_clear(&e).await;
    e.fake.on("/ws/2/discid/", None);
    let html = panel(&e).await;
    assert!(html.contains("<option value=\"audiobook\" selected>") && html.contains("Track 01"), "{html}");
}

/// Forgets the answers of the lookups (the cache lives in the database).
async fn sqlx_clear(e: &Env) {
    e.store.cd_cache_clear().await.unwrap();
}

#[tokio::test]
async fn the_speed_is_set_before_a_rip_and_given_back_after_every_end() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    give_mp3(&e);
    insert(&e, 3).await;
    let speeds = || e.drive.speeds.lock().unwrap().clone();
    // The default setting is 8x.
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(json!({ "author": "A", "title": "B", "disc": 1, "eject": false, "force": true }))).await;
    wait_job(&e, &token, r.json()["id"].as_i64().unwrap()).await;
    assert_eq!(speeds(), ["set 8 /dev/fake0", "reset /dev/fake0"]);
    // After a failure: back to the drive's own speed.
    e.drive.speeds.lock().unwrap().clear();
    set(&e, &[("cd_read_speed", "4")]).await;
    *e.drive.fail_at.lock().unwrap() = Some((2, "read"));
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(json!({ "author": "A", "title": "C", "disc": 1, "eject": false, "force": true }))).await;
    let job = wait_job(&e, &token, r.json()["id"].as_i64().unwrap()).await;
    assert_eq!(job["error"], "unreadable_track", "the track that cannot be read is left out, the rip goes on");
    assert_eq!(speeds(), ["set 4 /dev/fake0", "reset /dev/fake0"]);
    // After a cancel too.
    e.drive.speeds.lock().unwrap().clear();
    *e.drive.fail_at.lock().unwrap() = None;
    *e.drive.hold_after.lock().unwrap() = Some(1);
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(json!({ "author": "A", "title": "D", "disc": 1, "eject": false, "force": true }))).await;
    let id = r.json()["id"].as_i64().unwrap();
    for _ in 0..100 {
        if !speeds().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(speeds(), ["set 4 /dev/fake0"], "while it runs the speed is set");
    call(&e, "POST", &format!("/api/v1/cd/rip/{id}/cancel"), Some(&token), Some(json!({}))).await;
    assert_eq!(wait_job(&e, &token, id).await["error"], "cancelled");
    assert_eq!(speeds(), ["set 4 /dev/fake0", "reset /dev/fake0"]);
    // 0 leaves the drive alone.
    e.drive.speeds.lock().unwrap().clear();
    *e.drive.hold_after.lock().unwrap() = None;
    set(&e, &[("cd_read_speed", "0")]).await;
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(json!({ "author": "A", "title": "E", "disc": 1, "eject": false, "force": true }))).await;
    wait_job(&e, &token, r.json()["id"].as_i64().unwrap()).await;
    assert!(speeds().is_empty());
}

#[tokio::test]
async fn nothing_asks_the_drive_while_a_rip_runs() {
    use david_server::admin::{i18n::Lang, pages::{AdminUi, PanelQuery, View}};
    use std::sync::atomic::Ordering;
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    give_mp3(&e);
    set(&e, &[("cd_udev", "1"), ("musicbrainz_contact", "family@example.org")]).await;
    *e.drive.hold_after.lock().unwrap() = Some(1);
    insert(&e, 3).await;
    let monitor = tokio::spawn(david_server::cdrive::monitor(e.cd.clone(), e.store.clone(), Some(e.library.path().to_owned())));
    tokio::time::sleep(Duration::from_millis(800)).await;
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(rip_body())).await;
    let id = r.json()["id"].as_i64().unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let probes = e.drive.probes.load(Ordering::Relaxed);
    let reads = e.drive.toc_reads.load(Ordering::Relaxed);
    // Kernel events (they come with every change of a tray), the look of the monitor, the panel, the API.
    e.drive.edit("/dev/fake0", |_| {});
    e.drive.edit("/dev/fake0", |_| {});
    e.cd.refresh_drives("", true).await;
    e.cd.refresh_drives("", false).await;
    let ui = AdminUi::new().unwrap();
    for _ in 0..3 {
        ui.cd_drive_fragment(&e.store, &e.cd, View::new(Lang::En), &PanelQuery::default(), None, None).await.unwrap();
        call(&e, "GET", "/api/v1/cd/drive", Some(&token), None).await;
        call(&e, "POST", "/api/v1/cd/lookup", Some(&token), Some(json!({}))).await;
    }
    // A second rip and an eject are turned away without touching the drive.
    assert_eq!(call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(rip_body())).await.code(), "busy");
    assert_eq!(call(&e, "POST", "/api/v1/cd/eject", Some(&token), Some(json!({}))).await.code(), "busy");
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(e.drive.probes.load(Ordering::Relaxed), probes, "the drive was probed during the rip");
    assert_eq!(e.drive.toc_reads.load(Ordering::Relaxed), reads);
    call(&e, "POST", &format!("/api/v1/cd/rip/{id}/cancel"), Some(&token), Some(json!({}))).await;
    wait_job(&e, &token, id).await;
    monitor.abort();
}

// Resume.

use david_server::cdresume;

/// A disc of 4 tracks of 2 seconds (the fake rip writes a file of that length) is put in.
async fn insert_short(e: &Env, n: u8) {
    e.drive.edit("/dev/fake0", |u| {
        u.disc = Some(david_server::cdrive::fake::disc_sectors(n, 150));
        u.tray_open = false;
        u.changed = true;
    });
    e.cd.refresh_drives("", true).await;
}

fn disc_dir(e: &Env) -> std::path::PathBuf {
    let id = e.cd.drives.snapshot().0[0].disc_id.clone().unwrap();
    cdresume::dir_of(&e.dir.path().join("cd-import"), &id)
}

async fn manifest_has(e: &Env, n: usize) {
    for _ in 0..200 {
        if cdresume::read(&disc_dir(e)).is_some_and(|m| m.tracks.iter().filter(|t| t.state == "done").count() >= n) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the manifest never had {n} tracks");
}

async fn interrupted_rip(e: &mut Env, token: &str) -> i64 {
    give_mp3(e);
    set(e, &[("cd_eject_after", "0")]).await;
    insert_short(e, 4).await;
    *e.drive.hold_after.lock().unwrap() = Some(2);
    let r = call(e, "POST", "/api/v1/cd/rip", Some(token), Some(rip_body_all())).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", String::from_utf8_lossy(&r.bytes));
    assert_eq!(r.json()["resume"], "fresh");
    let id = r.json()["id"].as_i64().unwrap();
    manifest_has(e, 2).await;
    // The service is stopped: the track in hand is finished (nothing is running here), the rest is asked to end.
    e.cd.drives.stop_gracefully(Duration::from_millis(200), Duration::from_secs(3)).await;
    let job = wait_job(e, token, id).await;
    // A stop of the server is no failure: the CD waits for a new try, and what was read is in the resume folder.
    assert_eq!((job["phase"].as_str(), job["cd_state"].as_str(), job["error"].as_str()), (Some("retry_wait"), Some("retry_wait"), Some("interrupted")), "{job}");
    id
}

fn rip_body_all() -> Value {
    json!({ "author": "Ken Follett", "title": "Sturz der Titanen", "disc": 1, "discs_total": 1, "force": true })
}

#[tokio::test]
async fn a_stopped_service_keeps_the_finished_tracks_and_the_next_rip_reads_only_the_rest() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    interrupted_rip(&mut e, &token).await;
    // The folder of the disc stays, with the manifest and the two tracks that were done; nothing is in the library.
    let dir = disc_dir(&e);
    let m = cdresume::read(&dir).expect("a manifest");
    assert_eq!((m.tracks.iter().map(|t| t.number).collect::<Vec<_>>(), m.only.clone(), m.tracks[0].state.as_str()), (vec![1, 2], vec![1, 2, 3, 4], "done"));
    assert!(dir.join("01.mp3").is_file() && dir.join("02.mp3").is_file() && !dir.join("03.mp3").exists());
    assert_eq!(std::fs::read_dir(e.library.path()).unwrap().count(), 0);
    // The drive says what is there to resume.
    let state = call(&e, "GET", "/api/v1/cd/drive", Some(&token), None).await.json()["drives"][0]["resume"].clone();
    assert_eq!((state["tracks_done"].clone(), state["tracks_total"].as_u64(), state["settings_match"].as_bool()), (json!([1, 2]), Some(4), Some(true)));
    assert!(state["keep_until"].as_i64().unwrap() > state["interrupted_at"].as_i64().unwrap());

    // The same disc again: the finished tracks are taken over, only 3 and 4 are read; the progress starts at 2 of 4.
    *e.drive.hold_after.lock().unwrap() = None;
    e.drive.reads.lock().unwrap().clear();
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(rip_body_all())).await;
    assert_eq!(r.json()["resume"], "resumed");
    assert_eq!(r.json()["job"]["track"], 2);
    let job = wait_job(&e, &token, r.json()["id"].as_i64().unwrap()).await;
    assert_eq!((job["phase"].as_str(), job["track"].as_i64(), job["tracks_total"].as_i64()), (Some("done"), Some(4), Some(4)), "{job}");
    assert_eq!(*e.drive.reads.lock().unwrap(), vec![3, 4]);
    let book = e.library.path().join("Ken Follett/Sturz der Titanen");
    for n in 1..=4 {
        assert!(book.join(format!("Ken Follett_Sturz der Titanen_CD01_TR{n:02}.mp3")).is_file(), "track {n}");
    }
    assert!(staging_is_empty(&e), "{:?}", staging_dirs(&e));
}

#[tokio::test]
async fn tracks_that_were_read_once_and_wait_for_the_check_are_taken_over_from_their_part_files() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    interrupted_rip(&mut e, &token).await;
    *e.drive.hold_after.lock().unwrap() = None;
    let dir = disc_dir(&e);
    // The rip of a disc with "read every track twice" has written each track once and has not confirmed any: `NN.mp3.part` and `encoded`.
    let mut m = cdresume::read(&dir).unwrap();
    for entry in m.tracks.iter_mut() {
        std::fs::rename(dir.join(&entry.file), dir.join(format!("{}.part", entry.file))).unwrap();
        entry.state = "encoded".into();
    }
    cdresume::write(&dir, &m).unwrap();
    e.drive.reads.lock().unwrap().clear();
    let req = Request::builder().method("POST").uri("/api/v1/cd/rip").header(header::AUTHORIZATION, format!("Bearer {token}")).header(header::CONTENT_TYPE, "application/json").body(Body::from(rip_body_all().to_string())).unwrap();
    let res = e.router.clone().oneshot(req).await.unwrap();
    let bytes = to_bytes(res.into_body(), 1 << 20).await.unwrap();
    let r: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(r["resume"], "resumed", "the part files check out, they are not 'unreadable': {r}");
    // The tracks that are taken over wait for the check; they do not "wait" to be read.
    let first = call(&e, "GET", &format!("/api/v1/cd/import/{}", r["id"].as_i64().unwrap()), Some(&token), None).await.json()["job"]["track_states"].clone();
    let states: Vec<(u64, &str)> = first.as_array().unwrap().iter().map(|t| (t["number"].as_u64().unwrap(), t["state"].as_str().unwrap())).collect();
    assert!(states.iter().filter(|(n, _)| *n <= 2).all(|(_, st)| *st != "waiting"), "tracks 1 and 2 were read once: {states:?}");
    wait_job(&e, &token, r["id"].as_i64().unwrap()).await;
    let reads = e.drive.reads.lock().unwrap().clone();
    assert!(reads.contains(&3) && reads.contains(&4), "the rest is read: {reads:?}");
}

#[tokio::test]
async fn a_damaged_finished_file_is_read_again_and_a_changed_setting_or_a_wish_does_not_resume_but_a_changed_choice_does() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    interrupted_rip(&mut e, &token).await;
    *e.drive.hold_after.lock().unwrap() = None;
    let dir = disc_dir(&e);
    let saved = std::fs::read(dir.join("01.mp3")).unwrap();
    let start = |e: &Env, body: Value| {
        e.drive.reads.lock().unwrap().clear();
        let (router, token) = (e.router.clone(), token.clone());
        async move {
            let req = Request::builder().method("POST").uri("/api/v1/cd/rip").header(header::AUTHORIZATION, format!("Bearer {token}")).header(header::CONTENT_TYPE, "application/json").body(Body::from(body.to_string())).unwrap();
            let res = router.oneshot(req).await.unwrap();
            let bytes = to_bytes(res.into_body(), 1 << 20).await.unwrap();
            serde_json::from_slice::<Value>(&bytes).unwrap()
        }
    };

    // A file that was cut off: its size does not match the manifest, so it is read again (and the other one is taken over).
    std::fs::write(dir.join("01.mp3"), &saved[..saved.len() / 2]).unwrap();
    let r = start(&e, rip_body_all()).await;
    assert_eq!(r["resume"], "resumed");
    wait_job(&e, &token, r["id"].as_i64().unwrap()).await;
    assert_eq!(*e.drive.reads.lock().unwrap(), vec![1, 3, 4]);
    // A file that is not audio (right size, rubbish): read again as well.
    std::fs::remove_dir_all(e.library.path().join("Ken Follett")).unwrap();
    clear_staging(&e);
    interrupted_rip(&mut e, &token).await;
    *e.drive.hold_after.lock().unwrap() = None;
    std::fs::write(dir.join("02.mp3"), vec![7u8; saved.len()]).unwrap();
    let m = cdresume::read(&dir).unwrap();
    assert_eq!(m.tracks[1].size, saved.len() as u64, "same size as the real file");
    let r = start(&e, rip_body_all()).await;
    wait_job(&e, &token, r["id"].as_i64().unwrap()).await;
    assert_eq!(*e.drive.reads.lock().unwrap(), vec![2, 3, 4]);

    // A setting that decides the audio has changed: no resume, everything is read again, the old folder is gone.
    std::fs::remove_dir_all(e.library.path().join("Ken Follett")).unwrap();
    clear_staging(&e);
    interrupted_rip(&mut e, &token).await;
    *e.drive.hold_after.lock().unwrap() = None;
    set(&e, &[("cd_bitrate_kbps", "96")]).await;
    let info = call(&e, "GET", "/api/v1/cd/drive", Some(&token), None).await.json()["drives"][0]["resume"].clone();
    assert_eq!(info["settings_match"], false, "the drive says it would not be resumed");
    let r = start(&e, rip_body_all()).await;
    assert_eq!(r["resume"], "mismatch");
    // (96 kbit/s files do not pass the format check of the placing, so the job ends; what counts here is what was read.)
    wait_job(&e, &token, r["id"].as_i64().unwrap()).await;
    assert_eq!(*e.drive.reads.lock().unwrap(), vec![1, 2, 3, 4]);
    set(&e, &[("cd_bitrate_kbps", "64")]).await;
    std::fs::remove_dir_all(e.library.path()).ok();
    std::fs::create_dir_all(e.library.path()).unwrap();
    clear_staging(&e);

    // A different choice of tracks still resumes: what was read and is chosen again is taken over, what is not chosen any more is dropped
    // (track 1 was read before, tracks 2, 3 and 4 are chosen now: 2 is taken over, 3 and 4 are read). A longer choice takes over everything.
    interrupted_rip(&mut e, &token).await;
    *e.drive.hold_after.lock().unwrap() = None;
    let mut b = rip_body_all();
    b["only"] = json!([2, 3, 4]);
    let r = start(&e, b).await;
    assert_eq!(r["resume"], "resumed", "one track less does not throw away what was read");
    wait_job(&e, &token, r["id"].as_i64().unwrap()).await;
    assert_eq!(*e.drive.reads.lock().unwrap(), vec![3, 4]);
    std::fs::remove_dir_all(e.library.path().join("Ken Follett")).unwrap();
    clear_staging(&e);
    interrupted_rip(&mut e, &token).await;
    *e.drive.hold_after.lock().unwrap() = None;
    let mut b = rip_body_all();
    b["only"] = json!([1, 2, 3]);
    let r = start(&e, b).await;
    assert_eq!(r["resume"], "resumed");
    wait_job(&e, &token, r["id"].as_i64().unwrap()).await;
    assert_eq!(*e.drive.reads.lock().unwrap(), vec![3], "1 and 2 are taken over, 4 is not chosen");
    std::fs::remove_dir_all(e.library.path().join("Ken Follett")).unwrap();
    clear_staging(&e);
    interrupted_rip(&mut e, &token).await;
    *e.drive.hold_after.lock().unwrap() = None;
    let mut b = rip_body_all();
    b["resume"] = json!(false);
    let r = start(&e, b).await;
    assert_eq!(r["resume"], "fresh");
    wait_job(&e, &token, r["id"].as_i64().unwrap()).await;
    assert_eq!(*e.drive.reads.lock().unwrap(), vec![1, 2, 3, 4]);
}

#[tokio::test]
async fn a_restart_closes_the_open_jobs_as_interrupted_keeps_the_folder_and_the_hours_decide_how_long() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    interrupted_rip(&mut e, &token).await;
    let dir = disc_dir(&e);
    // A job that was under way when the process died.
    let job = e.store.create_cd_job(e.admin, "server", &david_server::cdimport::JobPatch { phase: Some("ripping".into()), ..Default::default() }).await.unwrap();
    // The service starts again.
    e.cd.start(&e.store).await;
    let closed = e.store.cd_job(e.admin, job.id).await.unwrap().unwrap();
    // A rip of the server waits for a new try (its CD is still in the drive); an upload of an app cannot be read again here and fails.
    assert_eq!((closed.cd_state.as_str(), closed.phase.as_str(), closed.error.as_str()), ("retry_wait", "retry_wait", "interrupted"));
    let upload = e.store.create_cd_job(e.admin, "dev", &david_server::cdimport::JobPatch { phase: Some("uploading".into()), ..Default::default() }).await.unwrap();
    e.cd.start(&e.store).await;
    let upload = e.store.cd_job(e.admin, upload.id).await.unwrap().unwrap();
    assert_eq!((upload.cd_state.as_str(), upload.phase.as_str(), upload.error.as_str()), ("failed", "failed", "interrupted"));
    assert!(dir.join("manifest.json").is_file() && dir.join("01.mp3").is_file(), "the finished tracks are kept");
    // Within the hours: kept. After them: removed. Other things in the staging area go after a day only.
    let mut m = cdresume::read(&dir).unwrap();
    let keep_ms = 48 * 3600 * 1000;
    m.updated_ms = david_server::now_ms() - keep_ms + 60_000;
    cdresume::write(&dir, &m).unwrap();
    e.cd.sweep_staging();
    assert!(dir.exists());
    m.updated_ms = david_server::now_ms() - keep_ms - 60_000;
    cdresume::write(&dir, &m).unwrap();
    e.cd.sweep_staging();
    assert!(!dir.exists());
    // The time is a setting.
    interrupted_rip(&mut e, &token).await;
    set(&e, &[("cd_resume_keep_hours", "1")]).await;
    e.cd.start(&e.store).await;
    let mut m = cdresume::read(&dir).unwrap();
    m.updated_ms = david_server::now_ms() - 2 * 3600 * 1000;
    cdresume::write(&dir, &m).unwrap();
    e.cd.sweep_staging();
    assert!(!dir.exists(), "one hour is up");
}

#[tokio::test]
async fn a_cut_off_manifest_is_no_manifest_and_the_disc_is_read_from_the_start() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    interrupted_rip(&mut e, &token).await;
    *e.drive.hold_after.lock().unwrap() = None;
    let dir = disc_dir(&e);
    let text = std::fs::read_to_string(dir.join("manifest.json")).unwrap();
    std::fs::write(dir.join("manifest.json"), &text[..text.len() / 2]).unwrap();
    e.drive.reads.lock().unwrap().clear();
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(rip_body_all())).await;
    assert_eq!(r.json()["resume"], "fresh");
    wait_job(&e, &token, r.json()["id"].as_i64().unwrap()).await;
    assert_eq!(*e.drive.reads.lock().unwrap(), vec![1, 2, 3, 4]);
}

#[tokio::test]
async fn the_drive_and_the_job_tell_the_state_of_each_track_while_a_rip_runs_and_after_it() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    give_mp3(&e);
    set(&e, &[("cd_eject_after", "0")]).await;
    insert_short(&e, 4).await;
    *e.drive.hold_in.lock().unwrap() = Some(2);
    let mut body = rip_body_all();
    body["only"] = json!([1, 2, 3]);
    body["tracks"] = json!(["Teil 1", "Teil 2", "Teil 3", "Teil 4"]);
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(body)).await;
    let id = r.json()["id"].as_i64().unwrap();
    // Wait until the drive is half way through track 2.
    let mut states = json!(null);
    for _ in 0..200 {
        states = drive_json_for(&e, &token).await["rip"]["track_states"].clone();
        if states.as_array().is_some_and(|a| a.iter().any(|t| t["state"] == "reading")) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let list: Vec<(u64, &str)> = states.as_array().unwrap().iter().map(|t| (t["number"].as_u64().unwrap(), t["state"].as_str().unwrap())).collect();
    assert_eq!(list, [(1, "done"), (2, "reading"), (3, "waiting")], "only the chosen tracks: {states}");
    assert_eq!((states[1]["title"].as_str(), states[1]["seconds"].as_u64(), states[1]["fraction"].as_f64()), (Some("Teil 2"), Some(2), Some(0.5)));
    // The same list is in the job (GET and events), with the fraction of the whole job.
    let job = call(&e, "GET", &format!("/api/v1/cd/import/{id}"), Some(&token), None).await.json()["job"].clone();
    assert_eq!(job["track_states"], states);
    assert_eq!(job["fraction"].as_f64(), Some(0.5));
    let open = call(&e, "GET", "/api/v1/cd/import", Some(&token), None).await.json()["jobs"][0].clone();
    assert_eq!(open["track_states"].as_array().map(Vec::len), Some(3));

    // Cancelled: the list stays with the result, the track it stopped on is told, until it is closed.
    call(&e, "POST", &format!("/api/v1/cd/rip/{id}/cancel"), Some(&token), Some(json!({}))).await;
    wait_job(&e, &token, id).await;
    let rip = drive_json_for(&e, &token).await["rip"].clone();
    assert_eq!((rip["finished"].as_bool(), rip["error"].as_str()), (Some(true), Some("cancelled")));
    let list: Vec<(&str, &str)> = rip["track_states"].as_array().unwrap().iter().map(|t| (t["state"].as_str().unwrap(), t["error"].as_str().unwrap_or(""))).collect();
    assert_eq!(list, [("done", ""), ("failed", "cancelled"), ("waiting", "")]);
    assert_eq!(call(&e, "POST", "/api/v1/cd/rip/dismiss", Some(&token), Some(json!({}))).await.status, StatusCode::NO_CONTENT);
    assert!(drive_json_for(&e, &token).await["rip"].is_null());
    assert_eq!(call(&e, "POST", "/api/v1/cd/rip/dismiss", Some(&token), Some(json!({}))).await.status, StatusCode::NOT_FOUND);
}

async fn drive_json_for(e: &Env, token: &str) -> Value {
    call(e, "GET", "/api/v1/cd/drive", Some(token), None).await.json()["drives"][0].clone()
}

#[tokio::test]
async fn the_buffer_is_a_ring_whose_size_is_automatic_or_a_setting_and_the_drive_and_the_job_tell_how_full_it_is() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    give_mp3(&e);
    set(&e, &[("cd_eject_after", "0"), ("cd_verify_read", "0")]).await;
    insert_short(&e, 3).await;
    let second = 176_400u64;
    let asked = |e: &Env| {
        let drive = e.drive.clone();
        async move {
            for _ in 0..200 {
                let v = *drive.asked_buffer.lock().unwrap();
                if v != 0 {
                    return v;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            0
        }
    };
    // The default is automatic: the longest chosen track by the table of contents (2 s) and 10 s more.
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(rip_body_all())).await;
    wait_job(&e, &token, r.json()["id"].as_i64().unwrap()).await;
    assert_eq!(asked(&e).await, 12 * second);
    // A fixed size in seconds; no restart, the next rip uses it.
    std::fs::remove_dir_all(e.library.path().join("Ken Follett")).unwrap();
    *e.drive.asked_buffer.lock().unwrap() = 0;
    set(&e, &[("cd_buffer_seconds", "20")]).await;
    *e.drive.filled_bytes.lock().unwrap() = 1_270_080; // 7.2 seconds wait in it
    *e.drive.hold_in.lock().unwrap() = Some(2);
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(rip_body_all())).await;
    let id = r.json()["id"].as_i64().unwrap();
    assert_eq!(asked(&e).await, 20 * second);
    let mut rip = Value::Null;
    for _ in 0..200 {
        rip = drive_json_for(&e, &token).await["rip"].clone();
        if rip["buffer_seconds"].as_f64().is_some_and(|b| b > 0.0) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!((rip["buffer_seconds"].as_f64(), rip["buffer_max"].as_u64(), rip["buffer_percent"].as_u64()), (Some(7.2), Some(20), Some(36)), "{rip}");
    assert_eq!((rip["reader_waits"].as_u64(), rip["reader_wait_max_s"].as_f64()), (Some(3), Some(2.5)));
    let job = call(&e, "GET", &format!("/api/v1/cd/import/{id}"), Some(&token), None).await.json()["job"].clone();
    assert_eq!((job["buffer_seconds"].as_f64(), job["buffer_max"].as_u64(), job["buffer_percent"].as_u64(), job["reader_waits"].as_u64()), (Some(7.2), Some(20), Some(36), Some(3)));
    call(&e, "POST", &format!("/api/v1/cd/rip/{id}/cancel"), Some(&token), Some(json!({}))).await;
    wait_job(&e, &token, id).await;
    let job = call(&e, "GET", &format!("/api/v1/cd/import/{id}"), Some(&token), None).await.json()["job"].clone();
    assert!(job["buffer_seconds"].is_null(), "no buffer is told after the end");
    // The upper limit (MB) caps the size, a fixed one as well as the automatic one.
    *e.drive.asked_buffer.lock().unwrap() = 0;
    *e.drive.hold_in.lock().unwrap() = None;
    clear_staging(&e);
    std::fs::remove_dir_all(e.library.path().join("Ken Follett")).ok();
    set(&e, &[("cd_buffer_seconds", "900"), ("cd_buffer_max_mb", "16")]).await;
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(rip_body_all())).await;
    wait_job(&e, &token, r.json()["id"].as_i64().unwrap()).await;
    assert_eq!(asked(&e).await, 16 * 1024 * 1024);
}

#[test]
fn the_size_of_the_ring_is_whole_track_plus_reserve_capped_by_the_memory_limit_and_never_below_two_seconds() {
    use david_server::cd_settings::CdSettings;
    let second = 176_400u64;
    let mut set = CdSettings::defaults();
    // Automatic (0): a 20 minute track and 10 s more: 212.7 MB, under the limit of 256 MB.
    assert_eq!(david_server::cdrive::ring_bytes(&set, 20 * 60), (20 * 60 + 10) * second);
    assert!((david_server::cdrive::ring_bytes(&set, 20 * 60) as f64 / 1e6 - 213.5).abs() < 1.0);
    // A 30 minute track does not fit under 256 MB: the limit wins, the track is read through as before.
    assert_eq!(david_server::cdrive::ring_bytes(&set, 30 * 60), 256 * 1024 * 1024);
    // A fixed size, also capped; the automatic size knows no less than 2 seconds.
    set.buffer_seconds = 30;
    assert_eq!(david_server::cdrive::ring_bytes(&set, 3600), 30 * second);
    set.buffer_max_mb = 16;
    set.buffer_seconds = 900;
    assert_eq!(david_server::cdrive::ring_bytes(&set, 0), 16 * 1024 * 1024);
    set.buffer_seconds = 2;
    assert_eq!(david_server::cdrive::ring_bytes(&set, 0), 2 * second);
}
