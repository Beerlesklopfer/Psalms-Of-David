// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! CDs in the family (docs/spec-cd-inbox.md, phases 1 and 2): the rights of a profile, the state machine of a CD job, the held folder, approve
//! and reject, expiry, a restart in every state, a new try after a read error, and a CD that is in the library already. The drive is a fake that
//! counts the tracks it reads; there is no internet.

#![allow(dead_code)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use common::open;
use david_server::api::{ApiState, router};
use david_server::cdflow;
use david_server::cdimport::CdService;
use david_server::cdrights::CdRightsChange;
use david_server::cdrive::fake::{FakeDrives, disc};
use david_server::mcp::{McpState, router as mcp_router};
use david_server::musicbrainz::{Lookup, LookupError, Service};
use david_server::{Error, NewProfile, Role, Schedule, Store};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tower::ServiceExt;

struct NoInternet;

impl Lookup for NoInternet {
    fn get(
        &self,
        _service: Service,
        _url: &str,
        _user_agent: &str,
        _max: usize,
    ) -> Result<Option<Vec<u8>>, LookupError> {
        Err(LookupError::Failed("no internet in the tests".into()))
    }
}

struct Env {
    router: Router,
    mcp: Router,
    store: Store,
    cd: CdService,
    drive: Arc<FakeDrives>,
    library: tempfile::TempDir,
    dir: tempfile::TempDir,
    boss: i64,
    n: u8,
    _seen: Mutex<()>,
}

fn key(n: u8) -> String {
    let letters = b"AEIMQUYcgkosw048";
    format!(
        "{}{}=",
        "B".repeat(42),
        letters[n as usize % letters.len()] as char
    )
}

async fn env() -> Env {
    let (dir, store) = open().await;
    let boss = store.create_profile("Boss", true).await.unwrap();
    let library = tempfile::TempDir::new().unwrap();
    let drive = FakeDrives::with_drive("/dev/fake0");
    let cd = CdService::with_backend(dir.path(), Arc::new(NoInternet), drive.clone());
    store
        .save_settings(&[("min_free_gb".to_owned(), "0".to_owned())])
        .await
        .unwrap();
    // The library is the folder of the import, unlocked, so that an approved CD arrives as an album.
    let root = library
        .path()
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    store
        .save_import_criteria(&[root], david_server::RatingRule::Manual)
        .await
        .unwrap();
    store.unlock_import("test").await.unwrap();
    let state = ApiState {
        store: store.clone(),
        library: Some(library.path().to_owned()),
        cd: cd.clone(),
        features: Default::default(),
    };
    let mcp = mcp_router(McpState {
        store: store.clone(),
        library: Some(library.path().to_owned()),
        cd: cd.clone(),
    });
    Env {
        router: router(state),
        mcp,
        store,
        cd,
        drive,
        library,
        dir,
        boss,
        n: 0,
        _seen: Mutex::new(()),
    }
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

async fn call(e: &Env, method: &str, uri: &str, token: &str, body: Option<Value>) -> Reply {
    let b = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    let req = match body {
        Some(v) => b
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    };
    let res = e.router.clone().oneshot(req).await.unwrap();
    let (parts, body) = res.into_parts();
    Reply {
        status: parts.status,
        headers: parts.headers,
        bytes: to_bytes(body, 64 * 1024 * 1024).await.unwrap().to_vec(),
    }
}

async fn pair(e: &mut Env, user: i64) -> String {
    e.n += 1;
    let enrollment = e
        .store
        .create_enrollment(user, false, e.boss)
        .await
        .unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/pair")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "code": enrollment.code, "device_name": "Laptop", "public_key": key(e.n) })
                .to_string(),
        ))
        .unwrap();
    let res = e.router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let j: Value =
        serde_json::from_slice(&to_bytes(res.into_body(), 1 << 20).await.unwrap()).unwrap();
    j["token"].as_str().unwrap().to_owned()
}

/// A member with the rights given (`insert`, `no_approval`, `approve`) and a device.
async fn member(
    e: &mut Env,
    name: &str,
    insert: bool,
    no_approval: bool,
    approve: bool,
) -> (i64, String) {
    let id = e
        .store
        .create_user(NewProfile::new(name, Role::Member))
        .await
        .unwrap();
    e.store
        .set_cd_rights(
            id,
            CdRightsChange {
                may_insert: Some(insert),
                no_approval: Some(no_approval),
                may_approve: Some(approve),
            },
        )
        .await
        .unwrap();
    let token = pair(e, id).await;
    (id, token)
}

async fn boss_token(e: &mut Env) -> String {
    let boss = e.boss;
    pair(e, boss).await
}

fn give_mp3(e: &Env) {
    let work = tempfile::TempDir::new().unwrap();
    let path = work.path().join("t.mp3");
    common::audio::make(
        &path,
        &[
            "-c:a",
            "libmp3lame",
            "-b:a",
            "64k",
            "-ar",
            "24000",
            "-ac",
            "2",
        ],
        &[("title", "Teil"), ("artist", "Somebody")],
    );
    *e.drive.mp3.lock().unwrap() = std::fs::read(path).unwrap();
}

fn have_ffmpeg() -> bool {
    let ok = common::audio::ffmpeg_available();
    if !ok {
        eprintln!("ffmpeg is not installed: test skipped");
    }
    ok
}

/// A disc of `n` tracks goes into the fake drive and the server looks at it. Another number of tracks is another Disc ID.
async fn insert(e: &Env, n: u8) {
    e.drive.edit("/dev/fake0", |u| {
        u.disc = Some(disc(n));
        u.tray_open = false;
        u.changed = true;
    });
    e.cd.refresh_drives("", true).await;
}

/// A disc whose tracks are as short as the files of the fake drive, so that a ripped CD counts as whole in the library.
async fn insert_short(e: &Env, n: u8) {
    e.drive.edit("/dev/fake0", |u| {
        u.disc = Some(david_server::cdrive::fake::disc_sectors(n, 150));
        u.tray_open = false;
        u.changed = true;
    });
    e.cd.refresh_drives("", true).await;
}

fn rip_body(title: &str) -> Value {
    json!({ "author": "Ken Follett", "title": title, "disc": 1, "discs_total": 1 })
}

async fn wait_state(e: &Env, id: i64, states: &[&str]) -> david_server::cdimport::Job {
    for _ in 0..400 {
        if let Some(j) = e.store.cd_job_any(id).await.unwrap()
            && states.contains(&j.cd_state.as_str())
            && !j.active
        {
            return j;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!(
        "job {id} never reached {states:?}: {:?}",
        e.store.cd_job_any(id).await.unwrap()
    );
}

fn held_dir(e: &Env, id: i64) -> std::path::PathBuf {
    e.dir.path().join("cd-import").join(format!("held-{id}"))
}

fn library_is_empty(e: &Env) -> bool {
    std::fs::read_dir(e.library.path())
        .unwrap()
        .next()
        .is_none()
}

async fn tool(e: &Env, token: &str, name: &str, args: Value) -> (bool, Value) {
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": name, "arguments": args } });
    let req = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .unwrap();
    let res = e.mcp.clone().oneshot(req).await.unwrap();
    let j: Value =
        serde_json::from_slice(&to_bytes(res.into_body(), 8 * 1024 * 1024).await.unwrap()).unwrap();
    let text = j["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    (
        j["result"]["isError"].as_bool().unwrap_or(false),
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

// ---------------------------------------------------------------------------------------------------------------
// Phase 1: the rights.

#[tokio::test]
async fn the_rights_start_with_the_careful_defaults_and_children_and_guests_never_get_them() {
    let mut e = env().await;
    let all = david_server::cdrights::CdRights::all();
    assert_eq!(
        e.store.cd_rights(e.boss).await.unwrap(),
        all,
        "an administrator has all three"
    );
    let (anna, _) = member(&mut e, "Anna", false, false, false).await;
    assert_eq!(
        e.store.cd_rights(anna).await.unwrap(),
        david_server::cdrights::CdRights::none(),
        "a member has none"
    );
    // A child or guest is refused every right, and the refusal changes nothing.
    let kid = e
        .store
        .create_user(NewProfile::child("Kid", 10, Schedule::always()))
        .await
        .unwrap();
    let guest = e
        .store
        .create_user(NewProfile::new("Gast", Role::Guest))
        .await
        .unwrap();
    for who in [kid, guest] {
        for change in [
            CdRightsChange {
                may_insert: Some(true),
                ..Default::default()
            },
            CdRightsChange {
                no_approval: Some(true),
                ..Default::default()
            },
            CdRightsChange {
                may_approve: Some(true),
                ..Default::default()
            },
        ] {
            assert!(matches!(
                e.store.set_cd_rights(who, change).await,
                Err(Error::CdRightsRole)
            ));
        }
        assert_eq!(
            e.store.cd_rights(who).await.unwrap(),
            david_server::cdrights::CdRights::none()
        );
    }
    // Becoming an administrator gives the three; becoming a child takes them away.
    e.store.set_profile_role(anna, Role::Admin).await.unwrap();
    assert_eq!(e.store.cd_rights(anna).await.unwrap(), all);
    e.store
        .change_role(anna, Role::Child, Some(9), Some(&Schedule::always()))
        .await
        .unwrap();
    assert_eq!(
        e.store.cd_rights(anna).await.unwrap(),
        david_server::cdrights::CdRights::none()
    );
    // A new administrator starts with all three.
    let second = e.store.create_profile("Boss2", true).await.unwrap();
    assert_eq!(e.store.cd_rights(second).await.unwrap(), all);
}

#[tokio::test]
async fn at_least_one_profile_keeps_the_right_to_approve() {
    let mut e = env().await;
    let second = e.store.create_profile("Boss2", true).await.unwrap();
    let take = CdRightsChange {
        may_approve: Some(false),
        ..Default::default()
    };
    // Of two approvers one may give up the right ...
    e.store.set_cd_rights(second, take).await.unwrap();
    // ... the last one may not: not by the right, and not by deleting the profile.
    assert!(matches!(
        e.store.set_cd_rights(e.boss, take).await,
        Err(Error::LastCdApprover)
    ));
    assert!(e.store.cd_rights(e.boss).await.unwrap().may_approve);
    assert!(
        matches!(
            e.store.delete_profile(e.boss).await,
            Err(Error::LastCdApprover)
        ),
        "deleting the last approver is refused too (another administrator remains)"
    );
    // A member can be the approver; then the administrator may give it up.
    let (anna, _) = member(&mut e, "Anna", false, false, true).await;
    e.store.set_cd_rights(e.boss, take).await.unwrap();
    assert_eq!(
        e.store
            .cd_approvers()
            .await
            .unwrap()
            .iter()
            .map(|a| a.1.as_str())
            .collect::<Vec<_>>(),
        ["Anna"]
    );
    assert!(matches!(
        e.store.set_cd_rights(anna, take).await,
        Err(Error::LastCdApprover)
    ));
}

#[tokio::test]
async fn the_cd_routes_ask_for_cd_may_insert_and_the_decisions_for_cd_may_approve() {
    let mut e = env().await;
    let (_, none) = member(&mut e, "Anna", false, false, false).await;
    let (_, insert_only) = member(&mut e, "Ben", true, false, false).await;
    let (_, approve_only) = member(&mut e, "Bea", false, false, true).await;
    insert(&e, 3).await;
    for (method, uri, body) in [
        ("GET", "/api/v1/cd/settings", None),
        ("GET", "/api/v1/cd/drive", None),
        ("POST", "/api/v1/cd/rip", Some(rip_body("X"))),
        ("POST", "/api/v1/cd/eject", Some(json!({}))),
        ("POST", "/api/v1/cd/lookup", Some(json!({}))),
        ("GET", "/api/v1/cd/jobs", None),
    ] {
        let r = call(&e, method, uri, &none, body.clone()).await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::FORBIDDEN, "forbidden"),
            "a member without the right: {uri}"
        );
    }
    assert_eq!(
        call(&e, "GET", "/api/v1/cd/settings", &insert_only, None)
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        call(&e, "GET", "/api/v1/cd/jobs", &insert_only, None)
            .await
            .status,
        StatusCode::OK
    );
    // Somebody who only approves looks at the jobs but does not put in CDs.
    assert_eq!(
        call(&e, "GET", "/api/v1/cd/jobs", &approve_only, None)
            .await
            .status,
        StatusCode::OK
    );
    let r = call(
        &e,
        "POST",
        "/api/v1/cd/rip",
        &approve_only,
        Some(rip_body("X")),
    )
    .await;
    assert_eq!(
        (r.status, r.code().as_str()),
        (StatusCode::FORBIDDEN, "forbidden")
    );
    // The app learns its rights from the profile.
    let me = call(&e, "GET", "/api/v1/me", &insert_only, None)
        .await
        .json();
    assert_eq!(
        me["cd_rights"],
        json!({ "may_insert": true, "no_approval": false, "may_approve": false })
    );
    let me = call(&e, "GET", "/api/v1/me", &approve_only, None)
        .await
        .json();
    assert_eq!(
        me["cd_rights"],
        json!({ "may_insert": false, "no_approval": false, "may_approve": true })
    );
    // The decisions of somebody without the right.
    for what in ["approve", "reject"] {
        let r = call(
            &e,
            "POST",
            &format!("/api/v1/cd/jobs/1/{what}"),
            &insert_only,
            None,
        )
        .await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "no such job");
    }
}

#[tokio::test]
async fn the_settings_of_the_approval_are_checked_and_have_careful_defaults() {
    let e = env().await;
    let s = david_server::cd_settings::CdSettings::load(&e.store)
        .await
        .unwrap();
    assert_eq!(
        (
            s.auto_approval.as_str(),
            s.held_keep_days,
            s.held_folder.as_str()
        ),
        ("wait", 60, "")
    );
    for (key, ok, bad) in [
        (
            "cd_auto_approval",
            ["wait", "now"].as_slice(),
            ["", "never", "1"].as_slice(),
        ),
        (
            "cd_held_keep_days",
            ["1", "60", "365"].as_slice(),
            ["0", "366", "x", ""].as_slice(),
        ),
    ] {
        for v in ok {
            assert!(
                david_server::cd_settings::validate(key, v).is_some(),
                "{key}={v}"
            );
        }
        for v in bad {
            assert!(
                david_server::cd_settings::validate(key, v).is_none(),
                "{key}={v}"
            );
        }
    }
    assert!(
        david_server::cd_settings::validate("cd_held_folder", "/srv/held").is_some()
            && david_server::cd_settings::validate("cd_held_folder", "relative").is_none()
    );
}

// ---------------------------------------------------------------------------------------------------------------
// Phase 2: the held CD, approve and reject.

#[tokio::test]
async fn a_cd_of_somebody_who_needs_approval_is_read_and_held_outside_the_library_until_it_is_approved()
 {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    give_mp3(&e);
    let (anna, anna_token) = member(&mut e, "Anna", true, false, false).await;
    let (_, bea_token) = member(&mut e, "Bea", false, false, true).await;
    let (_, stranger_token) = member(&mut e, "Carl", true, false, false).await;
    let boss = boss_token(&mut e).await;
    insert(&e, 3).await;
    let r = call(
        &e,
        "POST",
        "/api/v1/cd/rip",
        &anna_token,
        Some(rip_body("Sturz der Titanen")),
    )
    .await;
    assert_eq!(
        r.status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&r.bytes)
    );
    let id = r.json()["id"].as_i64().unwrap();
    let job = wait_state(&e, id, &["awaiting_approval"]).await;
    assert_eq!(
        (job.phase.as_str(), job.starter.as_str(), job.tracks_total),
        ("awaiting_approval", "Anna", 3),
        "{job:?}"
    );
    assert!(job.held_ms.is_some());
    assert_eq!(
        e.drive.reads.lock().unwrap().len(),
        3,
        "the CD was read at once, also though it must wait"
    );

    // The files are in the held folder and nowhere else.
    let held = held_dir(&e, id);
    assert_eq!(std::fs::read_dir(&held).unwrap().count(), 3);
    assert!(library_is_empty(&e), "nothing in the library folder");

    // They are in no list, search, album, stream or answer of the MCP server: even a scan of the library does not find them.
    e.store.run_import().await.unwrap();
    assert!(
        e.store
            .find_book("Ken Follett/Sturz der Titanen")
            .await
            .unwrap()
            .is_none()
    );
    assert!(e.store.admin_book_rows().await.unwrap().is_empty());
    for uri in [
        "/api/v1/items",
        "/api/v1/items?q=Titanen",
        "/api/v1/items?search=Follett",
        "/api/v1/new",
    ] {
        let r = call(&e, "GET", uri, &boss, None).await;
        assert_eq!(r.status, StatusCode::OK, "{uri}");
        assert!(
            !String::from_utf8_lossy(&r.bytes).contains("Titanen"),
            "{uri}: {}",
            String::from_utf8_lossy(&r.bytes)
        );
    }
    let (err, albums) = tool(
        &e,
        &e.store.create_mcp_token("llm", true, e.boss).await.unwrap(),
        "list_albums",
        json!({}),
    )
    .await;
    assert!(!err && !albums.to_string().contains("Titanen"), "{albums}");

    // The starter sees the entry of the job without a way to decide; a stranger sees nothing; an approver sees it with the buttons' right.
    let mine = call(&e, "GET", "/api/v1/cd/jobs", &anna_token, None)
        .await
        .json();
    assert_eq!(mine["jobs"][0]["cd_state"], "awaiting_approval");
    assert_eq!(mine["jobs"][0]["author"], "Ken Follett");
    assert_eq!(mine["jobs"][0]["tracks_total"], 3);
    assert_eq!(
        call(&e, "GET", "/api/v1/cd/jobs", &stranger_token, None)
            .await
            .json()["jobs"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        call(
            &e,
            "GET",
            &format!("/api/v1/cd/jobs/{id}"),
            &stranger_token,
            None
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    let bea_sees = call(&e, "GET", "/api/v1/cd/jobs", &bea_token, None)
        .await
        .json();
    assert_eq!(
        (
            bea_sees["jobs"][0]["id"].as_i64(),
            bea_sees["jobs"][0]["starter"].as_str()
        ),
        (Some(id), Some("Anna"))
    );
    // The MCP server lists the held CD; it cannot decide it.
    let (_, progress) = tool(
        &e,
        &e.store
            .create_mcp_token("llm2", true, e.boss)
            .await
            .unwrap(),
        "cd_progress",
        json!({}),
    )
    .await;
    assert_eq!(progress["state"], "awaiting_approval");
    assert_eq!(
        (
            progress["held"][0]["title"].as_str(),
            progress["held"][0]["put_in_by"].as_str()
        ),
        (Some("Sturz der Titanen"), Some("Anna"))
    );

    // The starter may not decide, a stranger may not either.
    let uri = format!("/api/v1/cd/jobs/{id}/approve");
    for token in [&anna_token, &stranger_token] {
        let r = call(&e, "POST", &uri, token, None).await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::FORBIDDEN, "forbidden")
        );
    }
    assert_eq!(
        e.store.cd_job_any(id).await.unwrap().unwrap().cd_state,
        "awaiting_approval",
        "a refusal changes nothing"
    );

    // An approver decides: the files go into the library, the import runs, the held folder is gone.
    let r = call(&e, "POST", &uri, &bea_token, None).await;
    assert_eq!(
        r.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&r.bytes)
    );
    assert_eq!(r.json()["job"]["decided_by"], "Bea");
    let done = wait_state(&e, id, &["imported"]).await;
    assert_eq!(
        (done.decided_by.as_str(), done.phase.as_str()),
        ("Bea", "done"),
        "{done:?}"
    );
    assert!(
        done.message.contains("files placed") || done.message.contains("tracks added"),
        "{}",
        done.message
    );
    assert!(!held.exists(), "the held folder is deleted");
    assert!(
        e.store
            .find_book("Ken Follett/Sturz der Titanen")
            .await
            .unwrap()
            .is_some(),
        "the album is in the library now"
    );
    assert!(
        e.library
            .path()
            .join("Ken Follett/Sturz der Titanen")
            .is_dir()
    );
    assert_eq!(done.user_id, anna);

    // The faster decider won; the second one is told who and what.
    for what in ["approve", "reject"] {
        let r = call(
            &e,
            "POST",
            &format!("/api/v1/cd/jobs/{id}/{what}"),
            &boss,
            None,
        )
        .await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::CONFLICT, "decided"),
            "{what}"
        );
        assert_eq!(
            (r.json()["by"].as_str(), r.json()["resolution"].as_str()),
            (Some("Bea"), Some("approved"))
        );
    }
}

#[tokio::test]
async fn a_rejected_cd_is_deleted_and_the_log_says_who_decided() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    give_mp3(&e);
    let (_, anna) = member(&mut e, "Anna", true, false, false).await;
    let boss = boss_token(&mut e).await;
    insert(&e, 3).await;
    let id = call(&e, "POST", "/api/v1/cd/rip", &anna, Some(rip_body("Nein")))
        .await
        .json()["id"]
        .as_i64()
        .unwrap();
    wait_state(&e, id, &["awaiting_approval"]).await;
    let held = held_dir(&e, id);
    assert!(held.is_dir());
    let r = call(
        &e,
        "POST",
        &format!("/api/v1/cd/jobs/{id}/reject"),
        &boss,
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let job = e.store.cd_job_any(id).await.unwrap().unwrap();
    assert_eq!(
        (
            job.cd_state.as_str(),
            job.reason.as_str(),
            job.decided_by.as_str()
        ),
        ("rejected", "declined", "Boss"),
        "{job:?}"
    );
    assert!(job.message.contains("rejected by Boss"), "{}", job.message);
    assert!(
        !held.exists() && library_is_empty(&e),
        "the files are deleted and never reached the library"
    );
    // The loser is told who decided and how; a new decision on an end state is not possible.
    let r = call(
        &e,
        "POST",
        &format!("/api/v1/cd/jobs/{id}/approve"),
        &boss,
        None,
    )
    .await;
    assert_eq!(
        (r.status, r.code().as_str(), r.json()["resolution"].as_str()),
        (StatusCode::CONFLICT, "decided", Some("rejected"))
    );
    // The log of the page shows the decision.
    let a = e.store.cd_log(10).await.unwrap();
    assert_eq!(a[0].decided_by, "Boss");
}

#[tokio::test]
async fn an_administrators_cd_needs_no_approval_and_a_move_that_does_not_fit_the_state_is_invalid()
{
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    give_mp3(&e);
    let boss = boss_token(&mut e).await;
    insert(&e, 3).await;
    let id = call(
        &e,
        "POST",
        "/api/v1/cd/rip",
        &boss,
        Some(rip_body("Gleich")),
    )
    .await
    .json()["id"]
        .as_i64()
        .unwrap();
    wait_state(&e, id, &["imported"]).await;
    assert!(
        e.store
            .find_book("Ken Follett/Gleich")
            .await
            .unwrap()
            .is_some(),
        "imported at once, no held folder"
    );
    assert!(!held_dir(&e, id).exists());
    // approve / reject of a job that never waited, retry / give up of one that was not read badly: invalid_move.
    for what in ["approve", "reject", "retry", "give_up"] {
        let r = call(
            &e,
            "POST",
            &format!("/api/v1/cd/jobs/{id}/{what}"),
            &boss,
            None,
        )
        .await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::CONFLICT, "invalid_move"),
            "{what}"
        );
    }
    assert_eq!(
        call(&e, "POST", "/api/v1/cd/jobs/999/approve", &boss, None)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn a_cd_put_in_at_the_server_waits_or_is_imported_at_once_as_the_setting_says() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    give_mp3(&e);
    let boss = boss_token(&mut e).await;
    // The default is "wait for approval": nobody is known to have put the CD in.
    insert(&e, 3).await;
    let req = || david_server::cdrive::RipRequest {
        author: "A".into(),
        title: "Wartet".into(),
        disc: Some(1),
        auto: true,
        ..Default::default()
    };
    let st = david_server::cdrive::start_rip(
        &e.store,
        &e.cd,
        Some(e.library.path().to_owned()),
        david_server::cdimport::SYSTEM_USER,
        req(),
    )
    .await
    .unwrap();
    let job = wait_state(&e, st.job.id, &["awaiting_approval"]).await;
    assert_eq!(
        (job.device.as_str(), job.starter.as_str()),
        ("auto", "auto")
    );
    assert!(library_is_empty(&e));
    assert_eq!(
        call(
            &e,
            "POST",
            &format!("/api/v1/cd/jobs/{}/approve", job.id),
            &boss,
            None
        )
        .await
        .status,
        StatusCode::OK
    );
    wait_state(&e, job.id, &["imported"]).await;
    // "Import at once".
    e.store
        .save_settings(&[("cd_auto_approval".to_owned(), "now".to_owned())])
        .await
        .unwrap();
    insert(&e, 4).await;
    let mut again = req();
    again.title = "Gleich".into();
    let st = david_server::cdrive::start_rip(
        &e.store,
        &e.cd,
        Some(e.library.path().to_owned()),
        david_server::cdimport::SYSTEM_USER,
        again,
    )
    .await
    .unwrap();
    wait_state(&e, st.job.id, &["imported"]).await;
    assert!(e.store.find_book("A/Gleich").await.unwrap().is_some());
}

#[tokio::test]
async fn an_upload_of_somebody_who_needs_approval_is_held_too() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    let (_, anna) = member(&mut e, "Anna", true, false, false).await;
    let boss = boss_token(&mut e).await;
    let work = tempfile::TempDir::new().unwrap();
    let path = work.path().join("a.mp3");
    common::audio::make(
        &path,
        &[
            "-c:a",
            "libmp3lame",
            "-b:a",
            "64k",
            "-ar",
            "24000",
            "-ac",
            "2",
        ],
        &[("title", "Teil"), ("artist", "Somebody")],
    );
    let bytes = std::fs::read(&path).unwrap();
    let meta = json!({ "author": "Ken Follett", "title": "Hochgeladen", "disc": 1, "discs_total": 1, "tracks": [{ "number": 1, "title": "Eins", "file": "t1.mp3" }] });
    let boundary = "----cdtest";
    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"meta\"\r\n\r\n{meta}\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"t1.mp3\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
    body.extend_from_slice(&bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/cd/import")
        .header(header::AUTHORIZATION, format!("Bearer {anna}"))
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();
    let res = e.router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let j: Value =
        serde_json::from_slice(&to_bytes(res.into_body(), 1 << 20).await.unwrap()).unwrap();
    assert_eq!(
        (j["import"].as_str(), j["job"]["cd_state"].as_str()),
        (Some("held"), Some("awaiting_approval")),
        "{j}"
    );
    assert!(library_is_empty(&e));
    let id = j["id"].as_i64().unwrap();
    assert_eq!(
        call(
            &e,
            "POST",
            &format!("/api/v1/cd/jobs/{id}/approve"),
            &boss,
            None
        )
        .await
        .status,
        StatusCode::OK
    );
    wait_state(&e, id, &["imported"]).await;
    assert!(e.library.path().join("Ken Follett/Hochgeladen").is_dir());
}

#[tokio::test]
async fn a_held_folder_inside_the_library_is_not_used() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    give_mp3(&e);
    let (_, anna) = member(&mut e, "Anna", true, false, false).await;
    let inside = e.library.path().join("held-here");
    e.store
        .save_settings(&[(
            "cd_held_folder".to_owned(),
            inside.to_string_lossy().into_owned(),
        )])
        .await
        .unwrap();
    insert(&e, 3).await;
    let id = call(&e, "POST", "/api/v1/cd/rip", &anna, Some(rip_body("Drin")))
        .await
        .json()["id"]
        .as_i64()
        .unwrap();
    wait_state(&e, id, &["awaiting_approval"]).await;
    assert!(
        !inside.exists() && library_is_empty(&e),
        "the folder inside the library was refused"
    );
    assert!(
        held_dir(&e, id).is_dir(),
        "the files wait below the staging folder instead"
    );
    // A folder outside the library is used.
    let outside = tempfile::TempDir::new().unwrap();
    e.store
        .save_settings(&[(
            "cd_held_folder".to_owned(),
            outside.path().to_string_lossy().into_owned(),
        )])
        .await
        .unwrap();
    insert(&e, 4).await;
    let id2 = call(
        &e,
        "POST",
        "/api/v1/cd/rip",
        &anna,
        Some(rip_body("Draussen")),
    )
    .await
    .json()["id"]
        .as_i64()
        .unwrap();
    wait_state(&e, id2, &["awaiting_approval"]).await;
    assert!(outside.path().join(format!("held-{id2}")).is_dir());
}

#[tokio::test]
async fn a_cd_that_waits_too_long_expires_and_the_clock_can_be_set() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    give_mp3(&e);
    let (_, anna) = member(&mut e, "Anna", true, false, false).await;
    let boss = boss_token(&mut e).await;
    e.store
        .save_settings(&[("cd_held_keep_days".to_owned(), "2".to_owned())])
        .await
        .unwrap();
    insert(&e, 3).await;
    let id = call(&e, "POST", "/api/v1/cd/rip", &anna, Some(rip_body("Alt")))
        .await
        .json()["id"]
        .as_i64()
        .unwrap();
    let job = wait_state(&e, id, &["awaiting_approval"]).await;
    let since = job.held_ms.unwrap();
    let day = 24 * 3600 * 1000;
    assert!(
        cdflow::expire_held(&e.store, &e.cd, since + 2 * day - 1000)
            .await
            .is_empty(),
        "not yet"
    );
    assert!(held_dir(&e, id).is_dir());
    assert_eq!(
        cdflow::expire_held(&e.store, &e.cd, since + 2 * day + 1000).await,
        vec![id]
    );
    let job = e.store.cd_job_any(id).await.unwrap().unwrap();
    assert_eq!(
        (job.cd_state.as_str(), job.reason.as_str()),
        ("rejected", "expired"),
        "{job:?}"
    );
    assert!(!held_dir(&e, id).exists() && library_is_empty(&e));
    // A late decider is told that it expired.
    let r = call(
        &e,
        "POST",
        &format!("/api/v1/cd/jobs/{id}/approve"),
        &boss,
        None,
    )
    .await;
    assert_eq!(
        (r.status, r.code().as_str(), r.json()["resolution"].as_str()),
        (StatusCode::CONFLICT, "decided", Some("expired"))
    );
    // Expiring again changes nothing.
    assert!(
        cdflow::expire_held(&e.store, &e.cd, since + 9 * day)
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn a_restart_keeps_the_held_files_and_brings_every_open_state_back_to_one_that_can_go_on() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    give_mp3(&e);
    let (anna_id, anna) = member(&mut e, "Anna", true, false, false).await;
    let boss = boss_token(&mut e).await;
    insert(&e, 3).await;
    let held_id = call(&e, "POST", "/api/v1/cd/rip", &anna, Some(rip_body("Haelt")))
        .await
        .json()["id"]
        .as_i64()
        .unwrap();
    wait_state(&e, held_id, &["awaiting_approval"]).await;
    // Jobs in the other open states, as a crash would leave them.
    let patch = david_server::cdimport::JobPatch {
        phase: Some("ripping".into()),
        author: Some("A".into()),
        album: Some("B".into()),
        ..Default::default()
    };
    let ripping = e
        .store
        .create_cd_job(anna_id, "server", &patch)
        .await
        .unwrap();
    let ripped = e
        .store
        .create_cd_job(anna_id, "server", &patch)
        .await
        .unwrap();
    e.store
        .cd_move(ripped.id, cdflow::Move::Ripped, &cdflow::Actor::Server)
        .await
        .unwrap();
    let upload = e
        .store
        .create_cd_job(
            anna_id,
            "dev",
            &david_server::cdimport::JobPatch {
                phase: Some("uploading".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let retry = e
        .store
        .create_cd_job(anna_id, "server", &patch)
        .await
        .unwrap();
    e.store
        .cd_move(
            retry.id,
            cdflow::Move::ReadFailed("read".into()),
            &cdflow::Actor::Server,
        )
        .await
        .unwrap();
    let ended = e
        .store
        .create_cd_job(anna_id, "server", &patch)
        .await
        .unwrap();
    e.store
        .cd_move(ended.id, cdflow::Move::Present, &cdflow::Actor::Server)
        .await
        .unwrap();

    // A new process: the same database and folders, a new service.
    let again = CdService::with_backend(e.dir.path(), Arc::new(NoInternet), e.drive.clone());
    again.start(&e.store).await;
    let state = |id: i64| {
        let store = e.store.clone();
        async move { store.cd_job_any(id).await.unwrap().unwrap().cd_state }
    };
    assert_eq!(
        state(held_id).await,
        "awaiting_approval",
        "a held CD stays held"
    );
    assert!(
        held_dir(&e, held_id).is_dir()
            && std::fs::read_dir(held_dir(&e, held_id)).unwrap().count() == 3,
        "with its files"
    );
    assert_eq!(
        state(ripping.id).await,
        "retry_wait",
        "a rip that was under way waits for a new try (the resume folder has what was read)"
    );
    assert_eq!(state(ripped.id).await, "retry_wait");
    assert_eq!(
        state(upload.id).await,
        "failed",
        "an upload cannot be read again here"
    );
    assert_eq!(state(retry.id).await, "retry_wait");
    assert_eq!(state(ended.id).await, "present", "an end state stays");
    // A folder without a record is a leftover and goes; the held one with a record stays.
    std::fs::create_dir_all(held_dir(&e, 4242)).unwrap();
    again.start(&e.store).await;
    assert!(!held_dir(&e, 4242).exists() && held_dir(&e, held_id).is_dir());
    // And the held CD can still be decided after the restart.
    assert_eq!(
        call(
            &e,
            "POST",
            &format!("/api/v1/cd/jobs/{held_id}/approve"),
            &boss,
            None
        )
        .await
        .status,
        StatusCode::OK
    );
    wait_state(&e, held_id, &["imported"]).await;
}

#[tokio::test]
async fn a_cd_that_cannot_be_read_waits_in_the_drive_and_a_new_try_reads_it_again() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    give_mp3(&e);
    let boss = boss_token(&mut e).await;
    let (_, anna) = member(&mut e, "Anna", true, true, false).await;
    let (_, bea) = member(&mut e, "Bea", true, true, true).await;
    let (_, carl) = member(&mut e, "Carl", true, true, false).await;
    insert(&e, 3).await;
    // The only track that is chosen cannot be read.
    *e.drive.fail_at.lock().unwrap() = Some((2, "read"));
    let mut body = rip_body("Kaputt");
    body["only"] = json!([2]);
    let id = call(&e, "POST", "/api/v1/cd/rip", &anna, Some(body))
        .await
        .json()["id"]
        .as_i64()
        .unwrap();
    let job = wait_state(&e, id, &["retry_wait"]).await;
    assert_eq!(
        (job.phase.as_str(), job.error.as_str(), job.state),
        ("retry_wait", "unreadable_track", "retry_wait"),
        "{job:?}"
    );
    assert!(
        e.drive.ejects.lock().unwrap().is_empty(),
        "the CD stays in the drive"
    );
    assert!(e.cd.drives.snapshot().0[0].disc_id.is_some());
    // Somebody who is neither the starter nor an approver may not retry or give up.
    for what in ["retry", "give_up"] {
        let r = call(
            &e,
            "POST",
            &format!("/api/v1/cd/jobs/{id}/{what}"),
            &carl,
            None,
        )
        .await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::FORBIDDEN, "forbidden"),
            "{what}"
        );
    }
    // The starter retries; the disc is read again and the CD goes in.
    *e.drive.fail_at.lock().unwrap() = None;
    let r = call(
        &e,
        "POST",
        &format!("/api/v1/cd/jobs/{id}/retry"),
        &anna,
        None,
    )
    .await;
    assert_eq!(
        r.status,
        StatusCode::ACCEPTED,
        "{}",
        String::from_utf8_lossy(&r.bytes)
    );
    assert_eq!(r.json()["job"]["id"].as_i64(), Some(id), "the same job");
    let done = wait_state(&e, id, &["imported"]).await;
    assert_eq!(done.id, id);
    assert_eq!(*e.drive.reads.lock().unwrap(), vec![2]);
    assert!(
        e.store
            .find_book("Ken Follett/Kaputt")
            .await
            .unwrap()
            .is_some()
    );

    // An approver may give a CD up that was read badly; the job is cancelled and says who.
    insert(&e, 4).await;
    *e.drive.fail_at.lock().unwrap() = Some((3, "read"));
    let mut body = rip_body("Aufgeben");
    body["only"] = json!([3]);
    let id2 = call(&e, "POST", "/api/v1/cd/rip", &anna, Some(body))
        .await
        .json()["id"]
        .as_i64()
        .unwrap();
    wait_state(&e, id2, &["retry_wait"]).await;
    // Somebody who may approve sees it and may retry or give up, though it is not theirs.
    let seen = call(&e, "GET", "/api/v1/cd/jobs", &bea, None).await.json();
    assert!(
        seen["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|j| j["id"].as_i64() == Some(id2))
    );
    let r = call(
        &e,
        "POST",
        &format!("/api/v1/cd/jobs/{id2}/give_up"),
        &bea,
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let job = e.store.cd_job_any(id2).await.unwrap().unwrap();
    assert_eq!(
        (job.cd_state.as_str(), job.decided_by.as_str()),
        ("cancelled", "Bea")
    );
    let r = call(
        &e,
        "POST",
        &format!("/api/v1/cd/jobs/{id2}/retry"),
        &boss,
        None,
    )
    .await;
    assert_eq!(
        (r.status, r.code().as_str()),
        (StatusCode::CONFLICT, "invalid_move")
    );
}

#[tokio::test]
async fn the_eject_of_the_app_gives_up_a_cd_that_could_not_be_read() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    give_mp3(&e);
    let (_, anna) = member(&mut e, "Anna", true, true, false).await;
    insert(&e, 3).await;
    *e.drive.fail_at.lock().unwrap() = Some((1, "read"));
    let mut body = rip_body("Weg");
    body["only"] = json!([1]);
    let id = call(&e, "POST", "/api/v1/cd/rip", &anna, Some(body))
        .await
        .json()["id"]
        .as_i64()
        .unwrap();
    wait_state(&e, id, &["retry_wait"]).await;
    let r = call(&e, "POST", "/api/v1/cd/eject", &anna, Some(json!({}))).await;
    assert_eq!(r.json()["ejected"], true);
    assert_eq!(
        e.store.cd_job_any(id).await.unwrap().unwrap().cd_state,
        "cancelled"
    );
}

#[tokio::test]
async fn an_interrupted_rip_waits_for_a_new_try_that_reads_only_the_rest() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    give_mp3(&e);
    let boss = boss_token(&mut e).await;
    e.store
        .save_settings(&[("cd_eject_after".to_owned(), "0".to_owned())])
        .await
        .unwrap();
    e.drive.edit("/dev/fake0", |u| {
        u.disc = Some(david_server::cdrive::fake::disc_sectors(4, 150));
        u.tray_open = false;
        u.changed = true;
    });
    e.cd.refresh_drives("", true).await;
    *e.drive.hold_after.lock().unwrap() = Some(2);
    let id = call(&e, "POST", "/api/v1/cd/rip", &boss, Some(rip_body("Halb")))
        .await
        .json()["id"]
        .as_i64()
        .unwrap();
    for _ in 0..200 {
        if e.drive.reads.lock().unwrap().len() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    e.cd.drives
        .stop_gracefully(Duration::from_millis(200), Duration::from_secs(3))
        .await;
    let job = wait_state(&e, id, &["retry_wait"]).await;
    assert_eq!(job.error, "interrupted");
    // The new try takes over the finished tracks from the resume folder.
    *e.drive.hold_after.lock().unwrap() = None;
    e.drive.reads.lock().unwrap().clear();
    let r = call(
        &e,
        "POST",
        &format!("/api/v1/cd/jobs/{id}/retry"),
        &boss,
        None,
    )
    .await;
    assert_eq!(
        r.status,
        StatusCode::ACCEPTED,
        "{}",
        String::from_utf8_lossy(&r.bytes)
    );
    assert_eq!(r.json()["resume"], "resumed");
    wait_state(&e, id, &["imported"]).await;
    assert_eq!(
        *e.drive.reads.lock().unwrap(),
        vec![3, 4],
        "only the rest was read"
    );
}

// ---------------------------------------------------------------------------------------------------------------
// A CD that is in the library already.

#[tokio::test]
async fn a_cd_that_is_in_the_library_is_not_read_and_is_ejected_when_the_request_says_so() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    give_mp3(&e);
    let boss = boss_token(&mut e).await;
    e.store
        .save_settings(&[("cd_eject_after".to_owned(), "0".to_owned())])
        .await
        .unwrap();
    insert_short(&e, 3).await;
    let id = call(&e, "POST", "/api/v1/cd/rip", &boss, Some(rip_body("Da")))
        .await
        .json()["id"]
        .as_i64()
        .unwrap();
    wait_state(&e, id, &["imported"]).await;
    e.drive.reads.lock().unwrap().clear();

    // Without "eject" the CD stays in the drive; with it the tray opens at once. Nothing is read either way.
    let mut ask = rip_body("Da");
    let r = call(&e, "POST", "/api/v1/cd/rip", &boss, Some(ask.clone())).await;
    assert_eq!(
        (r.status, r.code().as_str(), r.json()["ejected"].as_bool()),
        (StatusCode::CONFLICT, "disc_present", Some(false)),
        "{}",
        String::from_utf8_lossy(&r.bytes)
    );
    assert!(e.drive.ejects.lock().unwrap().is_empty());
    ask["eject"] = json!(true);
    let r = call(&e, "POST", "/api/v1/cd/rip", &boss, Some(ask)).await;
    assert_eq!(
        (r.status, r.code().as_str(), r.json()["ejected"].as_bool()),
        (StatusCode::CONFLICT, "disc_present", Some(true))
    );
    assert_eq!(
        e.drive.ejects.lock().unwrap().len(),
        1,
        "the tray was opened"
    );
    assert!(e.drive.reads.lock().unwrap().is_empty(), "nothing was read");
    let log = e.store.cd_log(10).await.unwrap();
    let present: Vec<_> = log.iter().filter(|j| j.cd_state == "present").collect();
    assert_eq!(present.len(), 2, "each check is a job in the state present");
    assert!(
        present[0].message.contains("already in the library")
            && present[0].message.contains("ejected"),
        "{}",
        present[0].message
    );
}

// ---------------------------------------------------------------------------------------------------------------
// Phase 3: the inbox.

/// The inbox of a token: the answer of `GET /inbox`.
async fn inbox(e: &Env, token: &str) -> Value {
    let r = call(e, "GET", "/api/v1/inbox", token, None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", String::from_utf8_lossy(&r.bytes));
    r.json()
}

/// The codes in an inbox, newest first.
fn codes(list: &Value) -> Vec<String> {
    list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["code"].as_str().unwrap().to_owned())
        .collect()
}

fn item_with<'a>(list: &'a Value, code: &str) -> &'a Value {
    list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["code"] == code)
        .unwrap_or_else(|| panic!("no {code} in {list}"))
}

/// A child and a guest, each with a device.
async fn child_and_guest(e: &mut Env) -> (String, String) {
    let kid = e
        .store
        .create_user(NewProfile::child("Kid", 10, Schedule::always()))
        .await
        .unwrap();
    let guest = e
        .store
        .create_user(NewProfile::new("Gast", Role::Guest))
        .await
        .unwrap();
    (pair(e, kid).await, pair(e, guest).await)
}

#[tokio::test]
async fn the_inbox_goes_to_the_starter_and_to_the_approvers_by_the_rights_and_to_nobody_else() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    give_mp3(&e);
    let (_, anna) = member(&mut e, "Anna", true, false, false).await;
    let (_, bea) = member(&mut e, "Bea", false, false, true).await;
    let (_, carl) = member(&mut e, "Carl", true, true, false).await;
    let (_, eve) = member(&mut e, "Eve", false, false, false).await;
    let (kid, guest) = child_and_guest(&mut e).await;
    let boss = boss_token(&mut e).await;
    insert(&e, 3).await;
    let id = call(&e, "POST", "/api/v1/cd/rip", &anna, Some(rip_body("Paulus")))
        .await
        .json()["id"]
        .as_i64()
        .unwrap();
    wait_state(&e, id, &["awaiting_approval"]).await;

    // Everybody who may approve gets the task, with the buttons.
    for who in [&bea, &boss] {
        let list = inbox(&e, who).await;
        assert_eq!(codes(&list), ["cd_awaiting_approval"]);
        let t = &list["items"][0];
        assert_eq!(
            (t["kind"].as_str(), t["can_decide"].as_bool(), t["read"].as_bool(), t["resolved"].as_bool(), t["job_id"].as_i64()),
            (Some("task"), Some(true), Some(false), Some(false), Some(id))
        );
        assert_eq!(
            t["args"],
            json!({ "job_id": id, "author": "Ken Follett", "title": "Paulus", "disc": 1, "discs_total": 1, "tracks": 3, "by": "Anna" })
        );
        assert_eq!((list["unread"].as_i64(), list["tasks_open"].as_i64()), (Some(1), Some(1)));
    }
    // The starter sees the entry of the job without the buttons; she cannot decide it either.
    let list = inbox(&e, &anna).await;
    let t = &list["items"][0];
    assert_eq!(
        (t["code"].as_str(), t["can_decide"].as_bool(), t["args"]["title"].as_str(), t["args"]["tracks"].as_i64()),
        (Some("cd_awaiting_approval"), Some(false), Some("Paulus"), Some(3))
    );
    assert_eq!((list["unread"].as_i64(), list["tasks_open"].as_i64()), (Some(1), Some(0)));
    let r = call(&e, "POST", &format!("/api/v1/cd/jobs/{id}/approve"), &anna, None).await;
    assert_eq!((r.status, r.code().as_str()), (StatusCode::FORBIDDEN, "forbidden"));
    // Somebody who needs no approval, one without any CD right, a child and a guest get nothing.
    for who in [&carl, &eve, &kid, &guest] {
        let list = inbox(&e, who).await;
        assert_eq!(list["items"], json!([]));
        assert_eq!((list["unread"].as_i64(), list["tasks_open"].as_i64()), (Some(0), Some(0)));
    }
    // The held files are no album: the starter's entry is a job, not a title in any list.
    let r = call(&e, "GET", "/api/v1/items", &anna, None).await;
    assert!(!String::from_utf8_lossy(&r.bytes).contains("Paulus"));
    // The count is in /me.
    assert_eq!(call(&e, "GET", "/api/v1/me", &bea, None).await.json()["inbox_unread"], 1);
    assert_eq!(call(&e, "GET", "/api/v1/me", &bea, None).await.json()["inbox_tasks_open"], 1);
    assert_eq!(call(&e, "GET", "/api/v1/me", &kid, None).await.json()["inbox_unread"], 0);

    // Carl's own CD needs no approval: he alone is told when it is in; the approvers are not.
    insert(&e, 4).await;
    let id2 = call(&e, "POST", "/api/v1/cd/rip", &carl, Some(rip_body("Eigene")))
        .await
        .json()["id"]
        .as_i64()
        .unwrap();
    wait_state(&e, id2, &["imported"]).await;
    for _ in 0..40 {
        if !inbox(&e, &carl).await["items"].as_array().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let list = inbox(&e, &carl).await;
    assert_eq!(codes(&list), ["cd_imported"]);
    assert_eq!(list["items"][0]["kind"], "notice");
    assert_eq!(list["items"][0]["args"]["tracks"], 4);
    assert_eq!(codes(&inbox(&e, &bea).await), ["cd_awaiting_approval"]);
}

#[tokio::test]
async fn read_marks_are_per_person_and_the_list_pages() {
    let mut e = env().await;
    let (a, ta) = member(&mut e, "Ann", true, true, true).await;
    let (b, tb) = member(&mut e, "Ben", true, true, true).await;
    let (_, tc) = member(&mut e, "Cy", true, true, false).await;
    let mut ids = Vec::new();
    for n in 0..5 {
        let id = e
            .store
            .inbox_add(
                &david_server::inbox::NewItem {
                    kind: "notice",
                    job_id: None,
                    code: "cd_no_match",
                    args: json!({ "disc_id": format!("d{n}") }),
                    speak: None,
                    device: String::new(),
                },
                &[(a, false), (b, false)],
            )
            .await
            .unwrap();
        ids.push(id);
    }
    assert_eq!(inbox(&e, &ta).await["unread"], 5);
    // Ann reads one; Ben still has it unread; Cy was never a recipient.
    let r = call(&e, "POST", &format!("/api/v1/inbox/{}/read", ids[2]), &ta, None).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(call(&e, "POST", &format!("/api/v1/inbox/{}/read", ids[2]), &ta, None).await.status, StatusCode::NO_CONTENT, "twice is fine");
    let (la, lb) = (inbox(&e, &ta).await, inbox(&e, &tb).await);
    assert_eq!((la["unread"].as_i64(), lb["unread"].as_i64()), (Some(4), Some(5)));
    let read_flags = |l: &Value| l["items"].as_array().unwrap().iter().map(|i| i["read"].as_bool().unwrap()).collect::<Vec<_>>();
    assert_eq!(read_flags(&la), [false, false, true, false, false]);
    assert_eq!(read_flags(&lb), [false; 5]);
    let r = call(&e, "POST", &format!("/api/v1/inbox/{}/read", ids[0]), &tc, None).await;
    assert_eq!((r.status, r.code().as_str()), (StatusCode::NOT_FOUND, "not_found"));
    assert_eq!(call(&e, "POST", "/api/v1/inbox/99999/read", &ta, None).await.status, StatusCode::NOT_FOUND);
    // Paging: newest first, `next` is the id to pass as `after`.
    let page = call(&e, "GET", "/api/v1/inbox?limit=2", &tb, None).await.json();
    let got: Vec<i64> = page["items"].as_array().unwrap().iter().map(|i| i["id"].as_i64().unwrap()).collect();
    assert_eq!((got, page["next"].as_i64()), (vec![ids[4], ids[3]], Some(ids[3])));
    let page = call(&e, "GET", &format!("/api/v1/inbox?limit=2&after={}", ids[3]), &tb, None).await.json();
    let got: Vec<i64> = page["items"].as_array().unwrap().iter().map(|i| i["id"].as_i64().unwrap()).collect();
    assert_eq!((got, page["next"].as_i64()), (vec![ids[2], ids[1]], Some(ids[1])));
    let page = call(&e, "GET", &format!("/api/v1/inbox?limit=2&after={}", ids[1]), &tb, None).await.json();
    assert_eq!((page["items"].as_array().unwrap().len(), page["next"].is_null()), (1, true));
    assert_eq!(page["unread"], 5, "the count is of the whole inbox, not of the page");
    assert_eq!(call(&e, "GET", "/api/v1/inbox?limit=0", &tb, None).await.code(), "invalid_paging");
    // All read: only for the one who asks.
    assert_eq!(call(&e, "POST", "/api/v1/inbox/read_all", &ta, None).await.status, StatusCode::NO_CONTENT);
    assert_eq!((inbox(&e, &ta).await["unread"].as_i64(), inbox(&e, &tb).await["unread"].as_i64()), (Some(0), Some(5)));
    assert_eq!(call(&e, "GET", "/api/v1/inbox", "nonsense", None).await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_task_is_decided_once_for_everybody_and_the_others_get_a_notice_in_code_and_args() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    give_mp3(&e);
    let (_, anna) = member(&mut e, "Anna", true, false, false).await;
    let (_, bea) = member(&mut e, "Bea", false, false, true).await;
    let boss = boss_token(&mut e).await;
    insert(&e, 3).await;
    let id = call(&e, "POST", "/api/v1/cd/rip", &anna, Some(rip_body("Paulus")))
        .await
        .json()["id"]
        .as_i64()
        .unwrap();
    wait_state(&e, id, &["awaiting_approval"]).await;
    let r = call(&e, "POST", &format!("/api/v1/cd/jobs/{id}/approve"), &bea, None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", String::from_utf8_lossy(&r.bytes));
    // The slower decider is told who was faster.
    let r = call(&e, "POST", &format!("/api/v1/cd/jobs/{id}/reject"), &boss, None).await;
    assert_eq!(
        (r.status, r.code().as_str(), r.json()["by"].as_str(), r.json()["resolution"].as_str()),
        (StatusCode::CONFLICT, "decided", Some("Bea"), Some("approved"))
    );
    // The task is done for the other approver: resolved, read, nothing left to do; and a notice says who decided.
    let list = inbox(&e, &boss).await;
    assert_eq!(codes(&list), ["cd_decided", "cd_awaiting_approval"]);
    let task = item_with(&list, "cd_awaiting_approval");
    assert_eq!(
        (task["resolved"].as_bool(), task["resolution"].as_str(), task["by"].as_str(), task["can_decide"].as_bool(), task["read"].as_bool(), task["job_state"].as_str()),
        (Some(true), Some("approved"), Some("Bea"), Some(false), Some(true), Some("imported"))
    );
    assert!(task["resolved_ms"].as_i64().unwrap() > 0);
    let note = item_with(&list, "cd_decided");
    assert_eq!(
        note["args"],
        json!({ "job_id": id, "author": "Ken Follett", "title": "Paulus", "disc": 1, "approved": true, "by": "Bea" })
    );
    assert_eq!((note["kind"].as_str(), note["read"].as_bool()), (Some("notice"), Some(false)));
    assert_eq!((list["unread"].as_i64(), list["tasks_open"].as_i64()), (Some(1), Some(0)));
    // The decider gets no notice about their own decision; her task is resolved too.
    let list = inbox(&e, &bea).await;
    assert_eq!(codes(&list), ["cd_awaiting_approval"]);
    assert_eq!((list["items"][0]["resolved"].as_bool(), list["unread"].as_i64()), (Some(true), Some(0)));
    // The starter: the entry of the job is gone, the decision notice is there.
    let list = inbox(&e, &anna).await;
    assert_eq!(codes(&list), ["cd_decided"]);
    assert_eq!(list["items"][0]["args"]["approved"], true);
    assert_eq!(list["tasks_open"], 0);

    // A rejection: the same, with the words turned round, and the files are gone.
    insert(&e, 4).await;
    let id2 = call(&e, "POST", "/api/v1/cd/rip", &anna, Some(rip_body("Nein")))
        .await
        .json()["id"]
        .as_i64()
        .unwrap();
    wait_state(&e, id2, &["awaiting_approval"]).await;
    assert_eq!(call(&e, "POST", &format!("/api/v1/cd/jobs/{id2}/reject"), &boss, None).await.status, StatusCode::OK);
    let list = inbox(&e, &bea).await;
    let note = &list["items"][0];
    assert_eq!(
        (note["code"].as_str(), note["args"]["approved"].as_bool(), note["args"]["by"].as_str(), note["args"]["title"].as_str()),
        (Some("cd_decided"), Some(false), Some("Boss"), Some("Nein"))
    );
    assert_eq!(item_with(&list, "cd_awaiting_approval")["resolution"], "rejected", "the newest task");
    assert_eq!(list["tasks_open"], 0);
}

#[tokio::test]
async fn a_held_cd_that_expires_tells_the_approvers_and_the_starter_and_the_cleanup_keeps_open_tasks() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    give_mp3(&e);
    let (_, anna) = member(&mut e, "Anna", true, false, false).await;
    let (bea_id, bea) = member(&mut e, "Bea", false, false, true).await;
    e.store.save_settings(&[("cd_held_keep_days".to_owned(), "2".to_owned())]).await.unwrap();
    insert(&e, 3).await;
    let id = call(&e, "POST", "/api/v1/cd/rip", &anna, Some(rip_body("Alt")))
        .await
        .json()["id"]
        .as_i64()
        .unwrap();
    let since = wait_state(&e, id, &["awaiting_approval"]).await.held_ms.unwrap();
    let day = 24 * 3600 * 1000;
    // A cleanup does not take a task nobody decided, however old.
    assert_eq!(e.store.inbox_cleanup(since + 400 * day, 90).await.unwrap(), 0);
    assert_eq!(codes(&inbox(&e, &bea).await), ["cd_awaiting_approval"]);
    assert_eq!(cdflow::expire_held(&e.store, &e.cd, since + 3 * day).await, vec![id]);
    for who in [&bea, &anna] {
        let list = inbox(&e, who).await;
        assert_eq!(item_with(&list, "cd_expired")["args"], json!({ "job_id": id, "author": "Ken Follett", "title": "Alt", "disc": 1 }));
        assert_eq!(list["tasks_open"], 0);
    }
    let list = inbox(&e, &bea).await;
    assert_eq!(item_with(&list, "cd_awaiting_approval")["resolution"], "expired");
    assert_eq!(list["items"].as_array().unwrap().len(), 2);
    // The starter's entry of the job is gone; she keeps the notice.
    assert_eq!(codes(&inbox(&e, &anna).await), ["cd_expired"]);

    // The cleanup after `inbox_keep_days`: the setting counts, the default is 90.
    let now = now_ms();
    assert_eq!(david_server::inbox::cleanup(&e.store, now).await, 0);
    assert_eq!(david_server::inbox::cleanup(&e.store, now + 89 * day).await, 0, "default 90 days");
    e.store.save_settings(&[("inbox_keep_days".to_owned(), "5".to_owned())]).await.unwrap();
    assert_eq!(david_server::inbox::cleanup(&e.store, now + 4 * day).await, 0);
    assert_eq!(david_server::inbox::cleanup(&e.store, now + 6 * day).await, 2, "the task and the notice go");
    assert_eq!(inbox(&e, &bea).await["items"], json!([]));
    assert_eq!(inbox(&e, &anna).await["items"], json!([]));
    let left: i64 = e.store.inbox_counts(bea_id).await.unwrap().0;
    assert_eq!(left, 0);
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[tokio::test]
async fn every_code_comes_from_its_event_and_nothing_is_stored_as_prose() {
    use david_server::cdflow::CdEvent;
    let mut e = env().await;
    let (_, bea) = member(&mut e, "Bea", false, false, true).await;
    let boss = boss_token(&mut e).await;
    // A CD put in at the server belongs to nobody: its notices go to the approvers.
    let patch = david_server::cdimport::JobPatch {
        author: Some("A".into()),
        album: Some("T".into()),
        disc: Some(2),
        tracks_total: Some(7),
        ..Default::default()
    };
    let job = e.store.create_cd_job(david_server::cdimport::SYSTEM_USER, "auto", &patch).await.unwrap();
    let events = [
        CdEvent::Present { job: job.clone() },
        CdEvent::Imported { job: job.clone() },
        CdEvent::ReadError { job: job.clone(), tracks: vec![2, 5] },
        CdEvent::AwaitingApproval { job: job.clone() },
        CdEvent::Decided { job: job.clone(), approved: true, by_id: e.boss, by: "Boss".into() },
        CdEvent::Expired { job: job.clone() },
    ];
    let mut not_started: Vec<CdEvent> = ["no_match", "ambiguous_match", "heal_needed", "no_space", "no_target", "busy", "drive_denied"]
        .iter()
        .map(|r| CdEvent::NotStarted { disc_id: "abc".into(), reason: (*r).to_owned(), author: "A".into(), title: "T".into(), disc: 2, detail: "3".into() })
        .collect();
    for ev in events.iter().chain(not_started.iter()) {
        cdflow::notify(&e.store, &e.cd, ev).await;
    }
    not_started.clear();
    // The boss decided: he got every notice but the one about his own decision.
    let list = inbox(&e, &bea).await;
    let mut got = codes(&list);
    got.sort();
    let mut want: Vec<String> = david_server::inbox::CODES.iter().map(|c| (*c).to_owned()).collect();
    want.sort();
    assert_eq!(got, want, "every code is made by its event, `busy` and `drive_denied` make none");
    let args = |code: &str| item_with(&list, code)["args"].clone();
    assert_eq!(args("cd_present"), json!({ "job_id": job.id, "author": "A", "title": "T", "disc": 2, "album_id": null }));
    assert_eq!(args("cd_imported")["tracks"], 7);
    assert_eq!(args("cd_read_error")["tracks"], json!([2, 5]));
    assert_eq!(args("cd_awaiting_approval")["by"], "auto");
    assert_eq!(args("cd_ambiguous"), json!({ "disc_id": "abc", "candidates": 3, "author": "A", "title": "T", "disc": 2 }));
    assert_eq!(args("cd_no_match")["disc_id"], "abc");
    assert!(inbox(&e, &boss).await["items"].as_array().unwrap().iter().all(|i| i["code"] != "cd_decided"));
    // Nothing is stored as prose: the code is a key, the arguments are data, and no text of the panel is in the tables.
    let rows = e.store.inbox_stored().await.unwrap();
    assert!(!rows.is_empty());
    for (code, text) in rows {
        assert!(david_server::inbox::CODES.contains(&code.as_str()), "{code}");
        assert!(serde_json::from_str::<Value>(&text).unwrap().is_object());
        for (_, m) in david_server::admin::messages::EN.iter().chain(david_server::admin::messages::DE) {
            for chunk in m.split(['{', '}']).filter(|c| c.len() >= 14) {
                assert!(!text.contains(chunk), "prose {chunk:?} in {text}");
            }
        }
    }
}

#[tokio::test]
async fn a_cd_that_cannot_be_read_and_one_that_is_there_already_are_told_to_the_right_people() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    give_mp3(&e);
    let (_, anna) = member(&mut e, "Anna", true, false, false).await;
    let (_, bea) = member(&mut e, "Bea", false, false, true).await;
    let (_, carl) = member(&mut e, "Carl", true, true, false).await;
    insert(&e, 3).await;
    *e.drive.fail_at.lock().unwrap() = Some((2, "read"));
    let mut body = rip_body("Kaputt");
    body["only"] = json!([2]);
    let id = call(&e, "POST", "/api/v1/cd/rip", &anna, Some(body)).await.json()["id"].as_i64().unwrap();
    wait_state(&e, id, &["retry_wait"]).await;
    // Anna needs approval, so the approvers are told too; the notice carries the job and the tracks, and the job state for "try again".
    for who in [&anna, &bea] {
        let list = inbox(&e, who).await;
        let n = item_with(&list, "cd_read_error");
        assert_eq!(n["args"]["tracks"], json!([2]));
        assert_eq!((n["args"]["job_id"].as_i64(), n["job_state"].as_str(), n["kind"].as_str()), (Some(id), Some("retry_wait"), Some("notice")));
    }
    assert!(inbox(&e, &carl).await["items"].as_array().unwrap().is_empty());
    // The app of the starter started it: the message names the device, which is the `device` of the `hello` message.
    let n = item_with(&inbox(&e, &bea).await, "cd_read_error").clone();
    assert_eq!(n["speak"], "starter");
    assert_eq!(n["device"].as_str().unwrap(), david_server::cdimport::device_label(&anna));
    // After a new try the job is in again, and the notice shows that "try again" is over.
    *e.drive.fail_at.lock().unwrap() = None;
    assert_eq!(call(&e, "POST", &format!("/api/v1/cd/jobs/{id}/retry"), &anna, None).await.status, StatusCode::ACCEPTED);
    wait_state(&e, id, &["awaiting_approval"]).await;
    assert_eq!(item_with(&inbox(&e, &bea).await, "cd_read_error")["job_state"], "awaiting_approval");
}

#[tokio::test]
async fn a_cd_that_is_in_the_library_is_a_notice_for_the_starter_and_a_rip_from_the_web_speaks_at_home() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    give_mp3(&e);
    let boss = boss_token(&mut e).await;
    let (_, bea) = member(&mut e, "Bea", true, true, true).await;
    e.store.save_settings(&[("cd_eject_after".to_owned(), "0".to_owned())]).await.unwrap();
    insert_short(&e, 3).await;
    let id = call(&e, "POST", "/api/v1/cd/rip", &boss, Some(rip_body("Da"))).await.json()["id"].as_i64().unwrap();
    wait_state(&e, id, &["imported"]).await;
    let mut ask = rip_body("Da");
    ask["eject"] = json!(true);
    // The drive is free a moment after the job is done.
    let mut code = String::new();
    for _ in 0..40 {
        code = call(&e, "POST", "/api/v1/cd/rip", &boss, Some(ask.clone())).await.code();
        if code != "busy" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(code, "disc_present");
    let list = inbox(&e, &boss).await;
    let n = item_with(&list, "cd_present");
    assert_eq!((n["args"]["title"].as_str(), n["args"]["disc"].as_i64(), n["speak"].as_str()), (Some("Da"), Some(1), Some("starter")));
    assert!(n["args"]["album_id"].as_i64().is_some(), "the album that is there already: {n}");
    // Bea is no starter, and the CD was put in by an administrator who needs no approval: she gets nothing about it.
    assert!(!codes(&inbox(&e, &bea).await).contains(&"cd_present".to_owned()));

    // A rip from the web (no device is known) is marked for the home; so is one at the drive.
    let patch = david_server::cdimport::JobPatch { author: Some("W".into()), album: Some("Web".into()), ..Default::default() };
    let job = e.store.create_cd_job(e.boss, "server", &patch).await.unwrap();
    cdflow::notify(&e.store, &e.cd, &cdflow::CdEvent::Imported { job }).await;
    let list = inbox(&e, &boss).await;
    let n = list["items"].as_array().unwrap().iter().find(|i| i["args"]["title"] == "Web").unwrap();
    assert_eq!((n["speak"].as_str(), n["device"].as_str()), (Some("home"), Some("")));
    // The notices of a decision are not spoken.
    let job = e.store.create_cd_job(e.boss, "server", &patch).await.unwrap();
    cdflow::notify(&e.store, &e.cd, &cdflow::CdEvent::Expired { job }).await;
    let list = inbox(&e, &boss).await;
    assert!(item_with(&list, "cd_expired").get("speak").is_none());
}

// The event channel over a real socket.

async fn serve(e: &Env) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = e.router.clone();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

async fn ws_open(addr: std::net::SocketAddr, token: &str) -> TcpStream {
    let mut s = TcpStream::connect(addr).await.unwrap();
    let req = format!(
        "GET /api/v1/cd/events HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
    );
    s.write_all(req.as_bytes()).await.unwrap();
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        head.push(s.read_u8().await.unwrap());
    }
    assert!(String::from_utf8_lossy(&head).starts_with("HTTP/1.1 101"));
    s
}

/// The next text message, or `None` after `wait`.
async fn ws_next(s: &mut TcpStream, wait: Duration) -> Option<Value> {
    let read = async {
        loop {
            let mut head = [0u8; 2];
            s.read_exact(&mut head).await.ok()?;
            let len = match head[1] & 0x7f {
                126 => s.read_u16().await.ok()? as usize,
                127 => s.read_u64().await.ok()? as usize,
                n => n as usize,
            };
            let mut payload = vec![0u8; len];
            s.read_exact(&mut payload).await.ok()?;
            if head[0] & 0x0f == 1 {
                return serde_json::from_slice::<Value>(&payload).ok();
            }
        }
    };
    tokio::time::timeout(wait, read).await.ok().flatten()
}

/// The next message of a type, skipping the others (jobs, progress, drive).
async fn ws_type(s: &mut TcpStream, kind: &str) -> Value {
    for _ in 0..60 {
        let m = ws_next(s, Duration::from_secs(10)).await.unwrap_or_else(|| panic!("no {kind}"));
        if m["type"] == kind {
            return m;
        }
    }
    panic!("no {kind}");
}

/// Every message that comes within a short time.
async fn ws_rest(s: &mut TcpStream) -> Vec<Value> {
    let mut out = Vec::new();
    while let Some(m) = ws_next(s, Duration::from_millis(300)).await {
        out.push(m);
    }
    out
}

#[tokio::test]
async fn the_messages_of_the_inbox_reach_only_the_recipients_and_a_reconnect_reads_the_list() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    give_mp3(&e);
    let (_, anna) = member(&mut e, "Anna", true, false, false).await;
    let (_, bea) = member(&mut e, "Bea", false, false, true).await;
    let (_, eve) = member(&mut e, "Eve", false, false, false).await;
    let (_, carl) = member(&mut e, "Carl", true, true, false).await;
    let addr = serve(&e).await;
    let mut wa = ws_open(addr, &anna).await;
    let mut wb = ws_open(addr, &bea).await;
    let mut weve = ws_open(addr, &eve).await;
    let mut wc = ws_open(addr, &carl).await;
    let hello = ws_type(&mut wa, "hello").await;
    let device_anna = hello["device"].as_str().unwrap().to_owned();
    for w in [&mut wb, &mut weve, &mut wc] {
        ws_type(w, "hello").await;
    }
    let snap = ws_type(&mut wb, "snapshot").await;
    assert_eq!((snap["unread"].as_i64(), snap["tasks_open"].as_i64()), (Some(0), Some(0)));
    for w in [&mut wa, &mut weve, &mut wc] {
        ws_type(w, "snapshot").await;
    }

    insert(&e, 3).await;
    let id = call(&e, "POST", "/api/v1/cd/rip", &anna, Some(rip_body("Paulus"))).await.json()["id"].as_i64().unwrap();
    // Bea: the task, with the buttons, the counts and the marks for speaking.
    let m = ws_type(&mut wb, "inbox").await;
    assert_eq!((m["v"].as_u64(), m["unread"].as_i64(), m["tasks_open"].as_i64()), (Some(1), Some(1), Some(1)));
    let item = &m["item"];
    assert_eq!(
        (item["code"].as_str(), item["kind"].as_str(), item["can_decide"].as_bool(), item["job_id"].as_i64(), item["speak"].as_str(), item["device"].as_str()),
        (Some("cd_awaiting_approval"), Some("task"), Some(true), Some(id), Some("starter"), Some(device_anna.as_str()))
    );
    assert_eq!(item["args"]["title"], "Paulus");
    // Anna: the entry of her job, without the buttons.
    let m = ws_type(&mut wa, "inbox").await;
    assert_eq!((m["item"]["code"].as_str(), m["item"]["can_decide"].as_bool()), (Some("cd_awaiting_approval"), Some(false)));
    let task_id = m["item"]["id"].as_i64().unwrap();
    // Eve (no CD right) and Carl (needs no approval, did not start it) hear nothing of the inbox.
    for w in [&mut weve, &mut wc] {
        assert!(ws_rest(w).await.iter().all(|m| m["type"] != "inbox" && m["type"] != "inbox_resolved"));
    }

    // A reconnect: the state is in the list and in the snapshot, not in missed events.
    drop(wb);
    let mut wb = ws_open(addr, &bea).await;
    ws_type(&mut wb, "hello").await;
    let snap = ws_type(&mut wb, "snapshot").await;
    assert_eq!((snap["unread"].as_i64(), snap["tasks_open"].as_i64()), (Some(1), Some(1)));
    assert_eq!(codes(&inbox(&e, &bea).await), ["cd_awaiting_approval"]);

    // The decision: the task is resolved for Anna, who hears `inbox_resolved`, and the notice `cd_decided` follows.
    let r = call(&e, "POST", &format!("/api/v1/cd/jobs/{id}/approve"), &bea, None).await;
    assert_eq!(r.status, StatusCode::OK);
    let m = ws_type(&mut wa, "inbox_resolved").await;
    assert_eq!(
        (m["id"].as_i64(), m["resolution"].as_str(), m["by"].as_str(), m["job_id"].as_i64(), m["tasks_open"].as_i64()),
        (Some(task_id), Some("approved"), Some("Bea"), Some(id), Some(0))
    );
    assert!(m["resolved_ms"].as_i64().unwrap() > 0);
    let m = ws_type(&mut wa, "inbox").await;
    assert_eq!((m["item"]["code"].as_str(), m["item"]["args"]["by"].as_str(), m["item"]["args"]["approved"].as_bool()), (Some("cd_decided"), Some("Bea"), Some(true)));
    // Bea decided herself: she hears her own task resolved, but gets no notice.
    let rest = ws_rest(&mut wb).await;
    assert!(rest.iter().any(|m| m["type"] == "inbox_resolved"));
    assert!(rest.iter().all(|m| m["type"] != "inbox"));
    for w in [&mut weve, &mut wc] {
        assert!(ws_rest(w).await.iter().all(|m| m["type"] != "inbox" && m["type"] != "inbox_resolved"));
    }
}

#[tokio::test]
async fn the_mcp_server_has_no_tool_to_read_or_decide_the_inbox() {
    let mut e = env().await;
    let token = e.store.create_mcp_token("llm", true, e.boss).await.unwrap();
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
    let req = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .unwrap();
    let res = e.mcp.clone().oneshot(req).await.unwrap();
    let j: Value = serde_json::from_slice(&to_bytes(res.into_body(), 8 * 1024 * 1024).await.unwrap()).unwrap();
    let names: Vec<String> = j["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_owned()).collect();
    assert!(names.contains(&"cd_progress".to_owned()));
    for n in &names {
        assert!(!n.contains("inbox") && !n.contains("approve") && !n.contains("reject") && !n.contains("decide"), "{n}");
    }
    let _ = &mut e;
}

#[tokio::test]
async fn the_automatic_import_tells_the_approvers_when_the_cd_is_there_already_or_cannot_be_started() {
    if !have_ffmpeg() {
        return;
    }
    let mut e = env().await;
    give_mp3(&e);
    let boss = boss_token(&mut e).await;
    let (_, bea) = member(&mut e, "Bea", false, false, true).await;
    let (_, carl) = member(&mut e, "Carl", true, true, false).await;
    e.store.save_settings(&[("cd_eject_after".to_owned(), "0".to_owned())]).await.unwrap();
    insert_short(&e, 3).await;
    let id = call(&e, "POST", "/api/v1/cd/rip", &boss, Some(rip_body("Da"))).await.json()["id"].as_i64().unwrap();
    wait_state(&e, id, &["imported"]).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    // The disc is put in again at the server: nothing is read, it is ejected, and the approvers are told (the drive belongs to nobody).
    let state = e.cd.drives.snapshot().0[0].clone();
    david_server::cdrive::auto_import(e.cd.clone(), e.store.clone(), Some(e.library.path().to_owned()), state).await;
    let list = inbox(&e, &bea).await;
    assert_eq!(codes(&list), ["cd_present"]);
    assert_eq!(list["items"][0]["args"]["title"], "Da");
    assert_eq!(list["items"][0]["speak"], "home");
    assert!(inbox(&e, &carl).await["items"].as_array().unwrap().is_empty());
    // Another disc that nobody knows (no internet): the log of the tab has it; the reason is not one a person is told.
    insert(&e, 5).await;
    let state = e.cd.drives.snapshot().0[0].clone();
    david_server::cdrive::auto_import(e.cd.clone(), e.store.clone(), Some(e.library.path().to_owned()), state).await;
    assert_eq!(codes(&inbox(&e, &bea).await), ["cd_present"]);
}
