// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The automatic import of a CD that is put in (setting `cd_auto_import`, needs the kernel events): it starts only for exactly one
//! release of MusicBrainz and a CD that is not in the library, and every other case is written down once with its reason.

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
use david_server::Store;
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


use david_server::cdrive::{self, DriveState};

/// The setting for the automatic import, the contact address for MusicBrainz and the kernel events.
async fn auto_on(e: &Env) {
    set(e, &[("cd_auto_import", "1"), ("cd_udev", "1"), ("musicbrainz_contact", "family@example.org"), ("cd_eject_after", "0")]).await;
}

/// The entries of the log that turned an automatic import down: (reason, detail), oldest first.
async fn skipped(e: &Env) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = e.store.cd_log(100).await.unwrap().into_iter().filter(|j| j.phase == "skipped").map(|j| (j.error, j.message)).collect();
    v.reverse();
    v
}

fn state(e: &Env) -> DriveState {
    e.cd.drives.snapshot().0.into_iter().next().unwrap()
}

async fn auto(e: &Env, library: Option<std::path::PathBuf>) {
    cdrive::auto_import(e.cd.clone(), e.store.clone(), library, state(e)).await;
}

fn lib(e: &Env) -> Option<std::path::PathBuf> {
    Some(e.library.path().to_owned())
}

#[tokio::test]
async fn a_cd_that_is_put_in_is_imported_by_itself_when_musicbrainz_knows_exactly_one_release() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let _ = pair_admin(&mut e).await;
    auto_on(&e).await;
    give_mp3(&e);
    let monitor = tokio::spawn(cdrive::monitor(e.cd.clone(), e.store.clone(), lib(&e)));
    tokio::time::sleep(Duration::from_millis(900)).await;
    insert_short(&e, 3).await;
    // The question by the table of contents also names another release whose disc is only nearly the same: it is no second answer.
    let mut near: Value = serde_json::from_str(&release_json(&e, 2)).unwrap();
    for m in near["releases"][1]["media"].as_array_mut().unwrap() {
        m["discs"] = json!([{ "id": "some-other-disc-id", "offsets": [150], "sectors": 1000, "offset-count": 1 }]);
    }
    e.fake.on("/ws/2/discid/-?toc=", Some(near.to_string().as_bytes()));
    e.fake.on("/ws/2/discid/", Some(release_json(&e, 1).as_bytes()));
    e.drive.edit("/dev/fake0", |u| u.changed = true); // the kernel's event
    let mut job = None;
    for _ in 0..300 {
        job = e.store.cd_log(10).await.unwrap().into_iter().find(|j| j.device == "auto");
        if job.as_ref().is_some_and(|j| !j.active) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let job = job.expect("the import started by itself");
    assert_eq!((job.phase.as_str(), job.author.as_str(), job.album.as_str(), job.disc), ("done", "Ken Follett", "Sturz der Titanen", 2), "{job:?}");
    // The files are in the library as the release says; the type, the details and the cover come from the release.
    for n in 1..=3 {
        assert!(e.library.path().join(format!("Ken Follett/Sturz der Titanen/CD 2/Ken Follett_Sturz der Titanen_CD02_TR{n:02}.mp3")).is_file(), "track {n}");
    }
    assert!(skipped(&e).await.is_empty());
    monitor.abort();
}

#[tokio::test]
async fn nothing_is_started_for_two_releases_none_or_a_cd_that_is_there_and_each_case_is_written_down_with_its_reason() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    auto_on(&e).await;
    give_mp3(&e);
    insert_short(&e, 3).await;
    let nothing_in_library = |e: &Env| std::fs::read_dir(e.library.path()).unwrap().count() == 0;

    // Several releases: the number is told. None: no_match. Without a contact address: no_contact. MusicBrainz down: lookup_failed.
    e.fake.on("/ws/2/discid/", Some(release_json(&e, 2).as_bytes()));
    auto(&e, lib(&e)).await;
    sqlx_clear(&e).await;
    e.fake.answers.lock().unwrap().clear();
    e.fake.on("/ws/2/discid/", None);
    auto(&e, lib(&e)).await;
    sqlx_clear(&e).await;
    e.fake.answers.lock().unwrap().clear();
    auto(&e, lib(&e)).await; // the fake has no answer: the service is not reachable
    set(&e, &[("musicbrainz_contact", "")]).await;
    auto(&e, lib(&e)).await;
    set(&e, &[("musicbrainz_contact", "family@example.org")]).await;
    let got = skipped(&e).await;
    assert_eq!(got[0], ("ambiguous_match".to_owned(), "2".to_owned()));
    assert_eq!(got[1].0, "no_match");
    assert_eq!(got[2].0, "lookup_failed");
    assert_eq!(got[3].0, "no_contact");
    assert!(nothing_in_library(&e) && e.drive.reads.lock().unwrap().is_empty(), "the disc was not read");

    // The target and the room: no import folder the server may write to; not enough free space.
    sqlx_clear(&e).await;
    e.fake.answers.lock().unwrap().clear();
    e.fake.on("/ws/2/discid/", Some(release_json(&e, 1).as_bytes()));
    auto(&e, None).await;
    set(&e, &[("min_free_gb", "100000")]).await;
    auto(&e, lib(&e)).await;
    set(&e, &[("min_free_gb", "0")]).await;
    let got = skipped(&e).await;
    assert_eq!((got[4].0.as_str(), got[5].0.as_str()), ("no_target", "no_space"), "{got:?}");

    // A disc that is not an audio CD, and a drive the service may not use.
    e.drive.edit("/dev/fake0", |u| {
        u.disc = Some(cdrive::fake::Disc { toc: cdrip::discid::TocInfo { tracks: vec![cdrip::discid::TocTrack { number: 1, start_lba: 0, is_audio: false }], leadout_lba: 1000 } });
        u.changed = true;
    });
    e.cd.refresh_drives("", true).await;
    auto(&e, lib(&e)).await;
    e.drive.edit("/dev/fake0", |u| u.denied = true);
    e.cd.refresh_drives("", true).await;
    auto(&e, lib(&e)).await;
    e.drive.edit("/dev/fake0", |u| u.denied = false);
    let got = skipped(&e).await;
    assert_eq!((got[6].0.as_str(), got[7].0.as_str()), ("not_audio", "drive_denied"));

    // A rip is running on the drive: busy.
    insert_short(&e, 3).await;
    *e.drive.hold_in.lock().unwrap() = Some(1);
    let held = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(json!({ "author": "A", "title": "B", "disc": 1, "force": true }))).await;
    auto(&e, lib(&e)).await;
    assert_eq!(skipped(&e).await.last().unwrap().0, "busy");
    *e.drive.hold_in.lock().unwrap() = None;
    wait_job(&e, &token, held.json()["id"].as_i64().unwrap()).await;
    // (that rip imported the disc as "A: B", CD 1: it is in the library now, by its Disc ID)

    // An interrupted rip is not continued by itself ... (another disc)
    let before = skipped(&e).await.len();
    auto(&e, lib(&e)).await;
    let got = skipped(&e).await;
    assert_eq!(got.len(), before + 1);
    assert_eq!((got.last().unwrap().0.as_str(), got.last().unwrap().1.as_str()), ("disc_present", "A: B, CD 1"), "the disc is in the library: nothing is done");
    // ... and a CD with a track missing is for a person to repair.
    let dir = e.library.path().join("A/B");
    let victim = std::fs::read_dir(&dir).unwrap().flatten().map(|d| d.path()).find(|p| p.extension().is_some_and(|x| x == "mp3")).unwrap();
    std::fs::remove_file(victim).unwrap();
    auto(&e, lib(&e)).await;
    let last = skipped(&e).await.pop().unwrap();
    assert_eq!(last.0, "heal_needed");
    assert!(last.1.starts_with("A: B, CD 1 (2 of 3)"), "{last:?}");
}

#[tokio::test]
async fn an_interrupted_rip_is_not_continued_by_itself() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let token = pair_admin(&mut e).await;
    auto_on(&e).await;
    give_mp3(&e);
    insert_short(&e, 4).await;
    e.fake.on("/ws/2/discid/", Some(release_json(&e, 1).as_bytes()));
    *e.drive.hold_after.lock().unwrap() = Some(2);
    let r = call(&e, "POST", "/api/v1/cd/rip", Some(&token), Some(json!({ "author": "A", "title": "B", "disc": 1, "force": true }))).await;
    let id = r.json()["id"].as_i64().unwrap();
    for _ in 0..200 {
        let dir = david_server::cdresume::dir_of(&e.dir.path().join("cd-import"), &disc_id_of(&e));
        if david_server::cdresume::read(&dir).is_some_and(|m| m.tracks.len() >= 2) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    e.cd.drives.stop_gracefully(Duration::from_millis(200), Duration::from_secs(3)).await;
    wait_job(&e, &token, id).await;
    *e.drive.hold_after.lock().unwrap() = None;
    auto(&e, lib(&e)).await;
    let last = skipped(&e).await.pop().unwrap();
    assert_eq!(last, ("resume_pending".to_owned(), "2 of 4".to_owned()));
    assert!(std::fs::read_dir(e.library.path()).unwrap().count() == 0);
}

#[tokio::test]
async fn the_automatic_import_that_cannot_hear_the_disc_says_so_once_and_a_disc_is_written_down_once_per_insertion() {
    let mut e = env().await;
    let _ = pair_admin(&mut e).await;
    // On, but the kernel events are off: one entry, however long it runs.
    set(&e, &[("cd_auto_import", "1"), ("cd_udev", "0"), ("musicbrainz_contact", "family@example.org")]).await;
    insert_short(&e, 3).await;
    e.fake.on("/ws/2/discid/", Some(release_json(&e, 2).as_bytes()));
    let monitor = tokio::spawn(cdrive::monitor(e.cd.clone(), e.store.clone(), lib(&e)));
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(skipped(&e).await, vec![("udev_off".to_owned(), String::new())]);
    // Switched to the kernel events at run time: a disc that is put in now is looked at, once.
    set(&e, &[("cd_udev", "1")]).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    e.drive.edit("/dev/fake0", |u| u.tray_open = true);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    insert_short(&e, 3).await;
    e.drive.edit("/dev/fake0", |u| u.changed = true);
    tokio::time::sleep(Duration::from_millis(2000)).await;
    for _ in 0..4 {
        e.drive.edit("/dev/fake0", |_| {}); // more events, nothing new
        e.cd.refresh_drives("", true).await;
    }
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let got = skipped(&e).await;
    assert_eq!(got.iter().filter(|(r, _)| r == "ambiguous_match").count(), 1, "{got:?}");
    // Put in again: it counts anew.
    e.drive.edit("/dev/fake0", |u| u.tray_open = true);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    insert_short(&e, 3).await;
    e.drive.edit("/dev/fake0", |u| u.changed = true);
    tokio::time::sleep(Duration::from_millis(2000)).await;
    assert_eq!(skipped(&e).await.iter().filter(|(r, _)| r == "ambiguous_match").count(), 2);
    // Off again: a disc that is put in starts nothing and writes nothing.
    set(&e, &[("cd_auto_import", "0")]).await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    e.drive.edit("/dev/fake0", |u| u.tray_open = true);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    insert_short(&e, 3).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(skipped(&e).await.iter().filter(|(r, _)| r == "ambiguous_match").count(), 2);
    monitor.abort();
}

/// The journal gets a line for every turned-down case.
#[tokio::test]
async fn a_turned_down_automatic_import_leaves_a_line_in_the_journal_with_the_reason() {
    use std::io::Write;
    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);
    impl Write for Sink {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    // A global subscriber: a scoped one can miss events whose call site another test of this binary has seen without one.
    static SINK: std::sync::OnceLock<Sink> = std::sync::OnceLock::new();
    let sink = SINK
        .get_or_init(|| {
            let sink = Sink::default();
            let writer = sink.clone();
            let subscriber = tracing_subscriber::fmt().with_ansi(false).with_writer(move || writer.clone()).finish();
            tracing::subscriber::set_global_default(subscriber).expect("no other global subscriber in this test binary");
            sink
        })
        .clone();
    let mut e = env().await;
    let _ = pair_admin(&mut e).await;
    auto_on(&e).await;
    insert_short(&e, 3).await;
    e.fake.on("/ws/2/discid/", Some(release_json(&e, 2).as_bytes()));
    auto(&e, lib(&e)).await;
    let text = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
    let line = text.lines().find(|l| l.contains("nothing started") && l.contains("ambiguous_match")).unwrap_or_else(|| panic!("{text}"));
    assert!(line.contains("ambiguous_match") && line.contains("/dev/fake0") && line.contains("INFO"), "{line}");
}

async fn sqlx_clear(e: &Env) {
    e.store.cd_cache_clear().await.unwrap();
}
