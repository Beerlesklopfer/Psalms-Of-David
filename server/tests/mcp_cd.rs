// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The MCP tools for the CD drive of the server (`cd_drive`, `cd_rip`, `cd_progress`): they use the same code as the web
//! form and the API. The drive is the fake of the other CD tests; there is no internet.

mod common;

use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use common::open;
use david_server::cdimport::CdService;
use david_server::cdrive::fake::{FakeDrives, disc};
use david_server::mcp::{McpState, router};
use david_server::{NewProfile, Role, Store};
use serde_json::{Value, json};
use tower::ServiceExt;

struct Env {
    router: axum::Router,
    store: Store,
    cd: CdService,
    drive: Arc<FakeDrives>,
    library: tempfile::TempDir,
    admin_token: String,
    reader_token: String,
    member_token: String,
    _dir: tempfile::TempDir,
}

async fn env() -> Env {
    let (dir, store) = open().await;
    let admin = store.create_profile("Boss", true).await.unwrap();
    let member = store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let library = tempfile::TempDir::new().unwrap();
    let drive = FakeDrives::with_drive("/dev/fake0");
    let cd = CdService::with_backend(dir.path(), Arc::new(david_server::musicbrainz::HttpLookup::new()), drive.clone());
    store.save_settings(&[("min_free_gb".to_owned(), "0".to_owned()), ("cd_title_pattern".to_owned(), "track_title".to_owned())]).await.unwrap();
    let admin_token = store.create_mcp_token("llm", true, admin).await.unwrap();
    let reader_token = store.create_mcp_token("reader", false, admin).await.unwrap();
    let member_token = store.create_mcp_token("anna", true, member).await.unwrap();
    let router = router(McpState { store: store.clone(), library: Some(library.path().to_owned()), cd: cd.clone() });
    Env { router, store, cd, drive, library, admin_token, reader_token, member_token, _dir: dir }
}

async fn tool(e: &Env, token: &str, name: &str, args: Value) -> (bool, Value) {
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": name, "arguments": args } });
    let req = Request::builder().method("POST").uri("/mcp").header(header::CONTENT_TYPE, "application/json").header(header::AUTHORIZATION, format!("Bearer {token}")).body(Body::from(body.to_string())).unwrap();
    let res = e.router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let j: Value = serde_json::from_slice(&to_bytes(res.into_body(), 8 * 1024 * 1024).await.unwrap()).unwrap();
    let text = j["result"]["content"][0]["text"].as_str().unwrap_or_default().to_owned();
    (j["result"]["isError"].as_bool().unwrap_or(false), serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

async fn insert(e: &Env, n: u8) {
    e.drive.edit("/dev/fake0", |u| {
        u.disc = Some(disc(n));
        u.tray_open = false;
        u.changed = true;
    });
    e.cd.refresh_drives("", true).await;
}

fn give_mp3(e: &Env) {
    if !common::audio::ffmpeg_available() {
        return;
    }
    let work = tempfile::TempDir::new().unwrap();
    let path = work.path().join("t.mp3");
    common::audio::make(&path, &["-c:a", "libmp3lame", "-b:a", "64k", "-ar", "24000", "-ac", "2"], &[("title", "Teil"), ("artist", "Somebody")]);
    *e.drive.mp3.lock().unwrap() = std::fs::read(path).unwrap();
}

fn rip_args(dry: bool) -> Value {
    json!({ "author": "Ken Follett", "title": "Sturz der Titanen", "disc": 1, "discs_total": 2, "tracks": ["Teil A", "Teil B", "Teil C"], "narrator": "Johannes Steck", "kind": "audiobook", "dry_run": dry })
}

async fn wait_done(e: &Env, id: i64) -> Value {
    for _ in 0..300 {
        let (_, p) = tool(e, &e.admin_token, "cd_progress", json!({ "job": id })).await;
        if p["state"] != "running" {
            return p;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the rip did not end");
}

#[tokio::test]
async fn the_tools_are_listed_and_have_help() {
    let e = env().await;
    let (_, list) = tool(&e, &e.admin_token, "list", json!({})).await;
    let names: Vec<&str> = list["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    for n in ["cd_drive", "cd_rip", "cd_progress"] {
        assert!(names.contains(&n), "{n} in the list");
    }
    let writes = |n: &str| list["tools"].as_array().unwrap().iter().find(|t| t["name"] == n).unwrap()["changes_files"].as_bool().unwrap();
    assert!(writes("cd_rip") && !writes("cd_drive") && !writes("cd_progress"));
    let (err, help) = tool(&e, &e.admin_token, "help_for", json!({ "tool": "cd_rip" })).await;
    assert!(!err);
    assert_eq!(help["example"]["dry_run"], true);
    assert!(help["description"].as_str().unwrap().contains("dry_run"));
}

#[tokio::test]
async fn only_tokens_of_profiles_that_may_put_in_cds_use_the_cd_tools() {
    let e = env().await;
    insert(&e, 3).await;
    for name in ["cd_drive", "cd_progress"] {
        let (err, msg) = tool(&e, &e.member_token, name, json!({})).await;
        assert!(err && msg.as_str().unwrap().contains("may put in CDs"), "{name}: {msg}");
    }
    let (err, msg) = tool(&e, &e.member_token, "cd_rip", rip_args(true)).await;
    assert!(err && msg.as_str().unwrap().contains("may put in CDs"), "{msg}");
    // An administrator's token that may only read can look and plan, not start.
    assert!(!tool(&e, &e.reader_token, "cd_drive", json!({})).await.0);
    assert!(!tool(&e, &e.reader_token, "cd_rip", rip_args(true)).await.0);
    let (err, msg) = tool(&e, &e.reader_token, "cd_rip", rip_args(false)).await;
    assert!(err && msg.as_str().unwrap().contains("may only read"), "{msg}");
    // The right counts, not the role: a member who was given it may use the tools, an administrator whose right was taken away may not.
    let anna = e.store.list_profiles().await.unwrap().into_iter().find(|p| p.name == "Anna").unwrap().id;
    e.store.set_cd_rights(anna, david_server::cdrights::CdRightsChange { may_insert: Some(true), ..Default::default() }).await.unwrap();
    assert!(!tool(&e, &e.member_token, "cd_drive", json!({})).await.0);
    let boss = e.store.list_profiles().await.unwrap().into_iter().find(|p| p.name == "Boss").unwrap().id;
    e.store.set_cd_rights(boss, david_server::cdrights::CdRightsChange { may_insert: Some(false), ..Default::default() }).await.unwrap();
    let (err, msg) = tool(&e, &e.admin_token, "cd_drive", json!({})).await;
    assert!(err && msg.as_str().unwrap().contains("may put in CDs"), "{msg}");
    // The profiles are listed with their rights; no tool approves or rejects a CD.
    let (err, profiles) = tool(&e, &e.member_token, "list_profiles", json!({})).await;
    assert!(!err);
    let boss_row = profiles["profiles"].as_array().unwrap().iter().find(|p| p["name"] == "Boss").unwrap();
    assert_eq!((boss_row["may_put_in_cds"].as_bool(), boss_row["needs_no_approval"].as_bool(), boss_row["may_approve_cds"].as_bool()), (Some(false), Some(true), Some(true)));
    let (_, list) = tool(&e, &e.member_token, "list", json!({})).await;
    let names: Vec<String> = list["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_owned()).collect();
    assert!(names.iter().all(|n| !n.contains("approve") && !n.contains("reject")), "{names:?}");
}

#[tokio::test]
async fn cd_drive_reports_the_disc_the_tracks_and_the_lookup_status() {
    let e = env().await;
    let (err, empty) = tool(&e, &e.admin_token, "cd_drive", json!({})).await;
    assert!(!err);
    assert_eq!((empty["rip_running"].as_bool(), empty["lookup"]["status"].as_str()), (Some(false), Some("no_disc")), "{empty}");
    insert(&e, 3).await;
    let (_, st) = tool(&e, &e.admin_token, "cd_drive", json!({})).await;
    assert_eq!(st["drives"][0]["device"], "/dev/fake0");
    assert_eq!(st["drives"][0]["tracks"], 3);
    assert_eq!(st["disc"]["tracks"].as_array().unwrap().len(), 3);
    assert!(st["disc"]["tracks"][0]["seconds"].as_u64().unwrap() > 0 && st["disc"]["total_minutes"].as_f64().unwrap() > 0.0);
    // No contact address for MusicBrainz: the lookup says so and nothing is asked.
    assert_eq!((st["lookup"]["status"].as_str(), st["lookup"]["contact_set"].as_bool()), (Some("no_contact"), Some(false)), "{st}");
}

#[tokio::test]
async fn a_dry_run_starts_nothing_and_says_what_would_happen() {
    let e = env().await;
    insert(&e, 3).await;
    // dry_run is the default.
    let mut args = rip_args(true);
    args.as_object_mut().unwrap().remove("dry_run");
    let (err, plan) = tool(&e, &e.admin_token, "cd_rip", args).await;
    assert!(!err, "{plan}");
    assert_eq!((plan["dry_run"].as_bool(), plan["started"].as_bool(), plan["would_start"].as_bool()), (Some(true), Some(false), Some(true)), "{plan}");
    assert_eq!(plan["files"].as_array().unwrap().len(), 3);
    assert_eq!(plan["files"][0]["file"], "Ken Follett_Sturz der Titanen_CD01_TR01.mp3");
    assert_eq!(plan["files"][0]["tag_title"], "Teil A");
    assert_eq!(plan["files"][1]["tag_track"], 102);
    assert_eq!((plan["tags"]["album"].as_str(), plan["tags"]["narrator"].as_str(), plan["tags"]["discs_total"].as_i64()), (Some("Sturz der Titanen"), Some("Johannes Steck"), Some(2)));
    assert!(plan["folder"].as_str().unwrap().contains("Ken Follett/Sturz der Titanen"), "{plan}");
    assert!(plan["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("no mbid")));
    // Nothing happened: no job, nothing running, no folder, no entry in the change log.
    assert!(e.store.cd_log(10).await.unwrap().is_empty());
    assert!(!e.cd.drives.is_ripping());
    assert_eq!(std::fs::read_dir(e.library.path()).unwrap().count(), 0);
    assert!(e.store.tag_changes(0, 10).await.unwrap().is_empty());
    let (_, p) = tool(&e, &e.admin_token, "cd_progress", json!({})).await;
    assert_eq!(p["state"], "none");
}

#[tokio::test]
async fn the_year_goes_into_the_tags_and_a_wrong_one_is_refused() {
    let e = env().await;
    insert(&e, 3).await;
    let mut args = rip_args(true);
    args["year"] = json!(1991);
    let (err, plan) = tool(&e, &e.admin_token, "cd_rip", args.clone()).await;
    assert!(!err, "{plan}");
    assert_eq!(plan["tags"]["year"], "1991", "{plan}");
    // A text of four digits is the same; three digits or letters are refused and nothing starts.
    args["year"] = json!("2000");
    assert_eq!(tool(&e, &e.admin_token, "cd_rip", args.clone()).await.1["tags"]["year"], "2000");
    for bad in [json!(91), json!("nineteen"), json!(true)] {
        args["year"] = bad;
        assert!(tool(&e, &e.admin_token, "cd_rip", args.clone()).await.0, "{args}");
    }
    assert!(e.store.cd_log(10).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_wrong_number_of_titles_is_refused_with_both_counts() {
    let e = env().await;
    insert(&e, 3).await;
    let mut args = rip_args(true);
    args["tracks"] = json!(["Teil A", "Teil B"]);
    let (err, msg) = tool(&e, &e.admin_token, "cd_rip", args.clone()).await;
    let msg = msg.as_str().unwrap().to_owned();
    assert!(err && msg.contains("2 track titles") && msg.contains("3 tracks are selected"), "{msg}");
    // Also for the real start, and with a choice of tracks: two titles are right for two tracks.
    args["dry_run"] = json!(false);
    assert!(tool(&e, &e.admin_token, "cd_rip", args.clone()).await.0);
    assert!(e.store.cd_log(10).await.unwrap().is_empty(), "a refused start leaves no job");
    args["only_tracks"] = json!([1, 3]);
    args["dry_run"] = json!(true);
    let (err, plan) = tool(&e, &e.admin_token, "cd_rip", args.clone()).await;
    assert!(!err, "{plan}");
    assert_eq!(plan["files"][1]["number"], 3);
    assert_eq!(plan["files"][1]["title"], "Teil B");
    args["only_tracks"] = json!([1, 9]);
    let (err, msg) = tool(&e, &e.admin_token, "cd_rip", args).await;
    assert!(err && msg.as_str().unwrap().contains("track 9"), "{msg}");
}

#[tokio::test]
async fn without_a_disc_nothing_is_planned_or_started() {
    let e = env().await;
    let (err, msg) = tool(&e, &e.admin_token, "cd_rip", rip_args(true)).await;
    assert!(err && msg.as_str().unwrap().contains("no_disc"), "{msg}");
}

#[tokio::test]
async fn the_start_makes_a_job_the_album_with_the_titles_and_a_log_entry_and_a_second_start_is_refused_while_it_runs() {
    if !common::audio::ffmpeg_available() {
        eprintln!("ffmpeg is not installed: test skipped");
        return;
    }
    let e = env().await;
    give_mp3(&e);
    *e.drive.hold_after.lock().unwrap() = Some(1);
    insert(&e, 3).await;
    let root = e.library.path().canonicalize().unwrap().to_string_lossy().into_owned();
    e.store.save_import_criteria(&[root], david_server::RatingRule::Manual).await.unwrap();
    e.store.unlock_import("test").await.unwrap();

    let (err, started) = tool(&e, &e.admin_token, "cd_rip", rip_args(false)).await;
    assert!(!err, "{started}");
    assert_eq!(started["started"], true);
    let id = started["job_id"].as_i64().unwrap();
    // While it runs: progress says so, a second start and a plan are refused with the same message.
    let (_, running) = tool(&e, &e.admin_token, "cd_progress", json!({})).await;
    assert_eq!((running["state"].as_str(), running["rip_running"].as_bool(), running["tracks"].as_array().unwrap().len()), (Some("running"), Some(true), 3), "{running}");
    for dry in [false, true] {
        let (err, msg) = tool(&e, &e.admin_token, "cd_rip", rip_args(dry)).await;
        assert!(err && msg.as_str().unwrap().contains("already running"), "{msg}");
    }
    assert_eq!(e.store.cd_log(10).await.unwrap().len(), 1, "no second job");
    *e.drive.hold_after.lock().unwrap() = None;

    let done = wait_done(&e, id).await;
    assert_eq!(done["state"], "done", "{done}");
    assert_eq!(done["percent"], 100);
    let book = e.library.path().join("Ken Follett/Sturz der Titanen/CD 1");
    for (n, title) in [(1, "Teil A"), (2, "Teil B"), (3, "Teil C")] {
        let f = book.join(format!("Ken Follett_Sturz der Titanen_CD01_TR{n:02}.mp3"));
        let t = david_server::tags::read(&f).unwrap_or_else(|err| panic!("{}: {err}", f.display()));
        assert_eq!((t.title.as_deref(), t.album.as_deref(), t.artist.as_deref()), (Some(title), Some("Sturz der Titanen"), Some("Ken Follett")));
    }
    // The album is in the library once the import has run, and the change log has the start.
    let mut found = None;
    for _ in 0..100 {
        found = e.store.find_book("Ken Follett/Sturz der Titanen").await.unwrap();
        if found.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(found.is_some(), "the album is in the library");
    let log = e.store.tag_changes(0, 10).await.unwrap();
    assert!(log.iter().any(|c| c.field == "CdRip" && c.token_name == "llm" && c.new_value.contains(&format!("job {id}"))), "{log:?}");
}
