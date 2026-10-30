// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The MCP endpoint for a remote LLM: tokens, reading, correcting tags, finding problems, the log.

mod common;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use common::audio::{ffmpeg_available, make, make_mp3};
use common::{book, open};
use david_server::mcp::{McpState, router};
use david_server::{NewProfile, Role, Store, tags};
use serde_json::{Value, json};
use tower::ServiceExt;

struct Mcp {
    router: axum::Router,
    store: Store,
    library: tempfile::TempDir,
    admin: i64,
    _dir: tempfile::TempDir,
}

async fn mcp() -> Mcp {
    let (dir, store) = open().await;
    let admin = store.create_profile("Boss", true).await.unwrap();
    let library = tempfile::TempDir::new().unwrap();
    let router = router(McpState { store: store.clone(), library: Some(library.path().to_owned()) });
    Mcp { router, store, library, admin, _dir: dir }
}

async fn post(m: &Mcp, token: Option<&str>, body: Value) -> (StatusCode, Value) {
    let mut b = Request::builder().method("POST").uri("/mcp").header(header::CONTENT_TYPE, "application/json");
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let res = m.router.clone().oneshot(b.body(Body::from(body.to_string())).unwrap()).await.unwrap();
    let status = res.status();
    let bytes = to_bytes(res.into_body(), 8 * 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

/// Calls a tool; the answer is the parsed text of the result, and whether it is an error.
async fn tool(m: &Mcp, token: &str, name: &str, args: Value) -> (bool, Value) {
    let (status, j) = post(m, Some(token), json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": name, "arguments": args } })).await;
    assert_eq!(status, StatusCode::OK);
    let text = j["result"]["content"][0]["text"].as_str().unwrap_or_default().to_owned();
    (j["result"]["isError"].as_bool().unwrap_or(false), serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

/// An album of two real MP3 files with tags, imported; returns its id.
async fn album(m: &Mcp, tags: &[(&str, &str)]) -> i64 {
    let b = book("Follett", "Winter", &[2]);
    let folder = m.library.path().join(&b.folder);
    for t in &b.tracks {
        make_mp3(&folder.join(&t.file), tags);
    }
    let name = b.folder.clone();
    m.store.import_books(vec![b]).await.unwrap();
    m.store.find_book(&name).await.unwrap().unwrap().id
}

#[tokio::test]
async fn nothing_works_without_a_valid_token_and_a_read_only_token_cannot_write() {
    let m = mcp().await;
    let ask = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
    assert_eq!(post(&m, None, ask.clone()).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(post(&m, Some("nope"), ask.clone()).await.0, StatusCode::UNAUTHORIZED);

    let read = m.store.create_mcp_token("reader", false, m.admin).await.unwrap();
    let (status, j) = post(&m, Some(&read), ask).await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<&str> = j["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    for n in ["list_albums", "get_tags", "set_tags", "find_problems", "change_log", "set_cover_from_folder"] {
        assert!(names.contains(&n), "{n}");
    }
    let (is_error, text) = tool(&m, &read, "set_tags", json!({ "id": 1, "fields": { "Genre": "x" } })).await;
    assert!(is_error && text.as_str().unwrap().contains("only read"), "{text}");

    // A revoked token is refused at once.
    let id = m.store.mcp_tokens().await.unwrap()[0].id;
    m.store.revoke_mcp_token(id).await.unwrap();
    assert_eq!(post(&m, Some(&read), json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" })).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_handshake_and_notifications_follow_the_protocol() {
    let m = mcp().await;
    let t = m.store.create_mcp_token("llm", false, m.admin).await.unwrap();
    let (_, init) = post(&m, Some(&t), json!({ "jsonrpc": "2.0", "id": 7, "method": "initialize", "params": {} })).await;
    assert_eq!(init["id"], 7);
    assert_eq!(init["result"]["serverInfo"]["name"], "david");
    assert!(init["result"]["capabilities"]["tools"].is_object());
    // A notification has no id and gets no answer.
    let (status, body) = post(&m, Some(&t), json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(body.is_null());
    let (_, unknown) = post(&m, Some(&t), json!({ "jsonrpc": "2.0", "id": 2, "method": "nope" })).await;
    assert_eq!(unknown["error"]["code"], -32601);
}

#[tokio::test]
async fn set_tags_changes_every_file_logs_each_change_and_a_dry_run_changes_nothing() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let m = mcp().await;
    let id = album(&m, &[("artist", "Foleet"), ("album", "Winter")]).await;
    let write = m.store.create_mcp_token("llm", true, m.admin).await.unwrap();

    let (err, listed) = tool(&m, &write, "list_albums", json!({ "query": "follett" })).await;
    assert!(!err && listed["total"] == 1 && listed["albums"][0]["id"] == id, "{listed}");

    let (_, dry) = tool(&m, &write, "set_tags", json!({ "id": id, "fields": { "TrackArtist": "Ken Follett" }, "dry_run": true })).await;
    assert_eq!(dry["dry_run"], true);
    assert_eq!(dry["changes"], 2, "{dry}");
    assert!(m.store.tag_changes(0, 10).await.unwrap().is_empty(), "a dry run is not logged");
    let (_, before) = tool(&m, &write, "get_tags", json!({ "id": id })).await;
    assert_eq!(before["tracks"][0]["fields"]["TrackArtist"], "Foleet");

    let fields = json!({ "TrackArtist": "Ken Follett", "AlbumArtist": ["Ken Follett"], "Genre": ["Hörbuch", "Roman"], "Comment": null });
    let (err, done) = tool(&m, &write, "set_tags", json!({ "id": id, "fields": fields })).await;
    assert!(!err, "{done}");
    let (_, after) = tool(&m, &write, "get_tags", json!({ "id": id })).await;
    for track in after["tracks"].as_array().unwrap() {
        assert_eq!(track["fields"]["TrackArtist"], "Ken Follett");
        assert_eq!(track["fields"]["Genre"], json!(["Hörbuch", "Roman"]), "several values stay several");
        assert!(track["fields"].get("Comment").is_none());
    }
    // The old and the new value of every file are in the log.
    let log = m.store.tag_changes(id, 50).await.unwrap();
    let artist: Vec<_> = log.iter().filter(|c| c.field == "TrackArtist").collect();
    assert_eq!(artist.len(), 2);
    assert!(artist.iter().all(|c| c.old_value == "Foleet" && c.new_value == "Ken Follett" && c.token_name == "llm"));
    let (_, shown) = tool(&m, &write, "change_log", json!({ "id": id })).await;
    assert!(shown.as_array().unwrap().len() >= 4);

    // Bad input is refused and writes nothing.
    let (err, text) = tool(&m, &write, "set_tags", json!({ "id": id, "fields": { "NoSuchField": "x" } })).await;
    assert!(err && text.as_str().unwrap().contains("unknown field"), "{text}");
    let (err, _) = tool(&m, &write, "set_tags", json!({ "id": id + 99, "fields": { "Genre": "x" } })).await;
    assert!(err);
}

#[tokio::test]
async fn find_problems_names_what_is_wrong_and_skips_what_is_fine() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let m = mcp().await;
    let bad = album(&m, &[("title", "x")]).await;
    // A second album with everything set, and a picture file in its folder.
    let b = book("Beethoven", "Sinfonien", &[2]);
    let folder = m.library.path().join(&b.folder);
    for t in &b.tracks {
        make_mp3(&folder.join(&t.file), &[("artist", "Beethoven"), ("album", "Sinfonien"), ("date", "1999"), ("composer", "Sprecher")]);
    }
    std::fs::write(folder.join("Folder.JPG"), [0xFF, 0xD8, 0xFF, 0xE0, 0, 0, 0, 0, 0, 0]).unwrap();
    let name = b.folder.clone();
    m.store.import_books(vec![b]).await.unwrap();
    let good = m.store.find_book(&name).await.unwrap().unwrap().id;

    let t = m.store.create_mcp_token("llm", false, m.admin).await.unwrap();
    let (err, found) = tool(&m, &t, "find_problems", json!({})).await;
    assert!(!err, "{found}");
    let albums = found["albums_with_problems"].as_array().unwrap();
    let of_bad = albums.iter().find(|a| a["id"] == bad).expect("the faulty album is listed");
    let problems: Vec<&str> = of_bad["problems"].as_array().unwrap().iter().map(|p| p.as_str().unwrap()).collect();
    for p in ["missing_artist", "missing_album", "missing_year", "missing_narrator"] {
        assert!(problems.contains(&p), "{p} in {problems:?}");
    }
    let of_good = albums.iter().find(|a| a["id"] == good);
    assert!(of_good.is_none_or(|a| !a["problems"].to_string().contains("missing_artist")), "{of_good:?}");
    assert!(found["next_offset"].is_null());
}

#[tokio::test]
async fn the_cover_of_the_folder_is_embedded_on_request() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let m = mcp().await;
    let id = album(&m, &[("artist", "A")]).await;
    let folder = m.library.path().join("Follett/Winter");
    // A real, tiny PNG.
    let png = {
        let p = folder.join("probe.png");
        let ok = std::process::Command::new("ffmpeg").args(["-v", "error", "-y", "-f", "lavfi", "-i", "color=red:s=8x8", "-frames:v", "1"]).arg(&p).status().unwrap().success();
        assert!(ok);
        let bytes = std::fs::read(&p).unwrap();
        std::fs::remove_file(&p).unwrap();
        bytes
    };
    std::fs::write(folder.join("cdcover.png"), &png).unwrap();
    let t = m.store.create_mcp_token("llm", true, m.admin).await.unwrap();
    let (err, done) = tool(&m, &t, "set_cover_from_folder", json!({ "id": id })).await;
    assert!(!err, "{done}");
    assert_eq!(done["embedded_in"], 2);
    assert!(tags::embedded_cover(&folder.join("Winter_CD01_TR01.mp3")).is_some());
    let _ = Role::Member;
}

#[test]
fn every_format_keeps_the_whole_width_of_the_fields_and_several_values() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let dir = tempfile::TempDir::new().unwrap();
    let cases: [(&str, &[&str]); 5] = [
        ("t.mp3", &["-c:a", "libmp3lame", "-b:a", "64k"]),
        ("t.flac", &["-c:a", "flac"]),
        ("t.ogg", &["-c:a", "libvorbis", "-q:a", "2"]),
        ("t.opus", &["-c:a", "libopus", "-b:a", "48k"]),
        ("t.m4a", &["-c:a", "aac", "-b:a", "48k"]),
    ];
    let changes = vec![
        ("TrackTitle".to_owned(), vec!["Ein Titel äöü".to_owned()]),
        ("Genre".to_owned(), vec!["Jazz".to_owned(), "Blues".to_owned()]),
        ("Composer".to_owned(), vec!["Sprecherin".to_owned()]),
        ("Lyricist".to_owned(), vec!["Dichter".to_owned()]),
        ("Isrc".to_owned(), vec!["DEX000000001".to_owned()]),
        ("Comment".to_owned(), vec!["Kommentar".to_owned()]),
    ];
    for (name, codec) in cases {
        let path = dir.path().join(name);
        make(&path, codec, &[("title", "alt")]);
        tags::write_fields(&path, &changes).unwrap_or_else(|e| panic!("{name}: {e}"));
        let read = tags::read_fields(&path).unwrap();
        let get = |f: &str| read.iter().find(|(n, _)| n == f).map(|(_, v)| v.clone());
        assert_eq!(get("TrackTitle"), Some(vec!["Ein Titel äöü".to_owned()]), "{name}");
        assert_eq!(get("Composer"), Some(vec!["Sprecherin".to_owned()]), "{name}");
        assert_eq!(get("Comment"), Some(vec!["Kommentar".to_owned()]), "{name}");
        let genre = get("Genre").unwrap_or_default();
        assert!(genre.contains(&"Jazz".to_owned()), "{name}: {genre:?}");
        // Removing a field works too.
        tags::write_fields(&path, &[("Comment".to_owned(), Vec::new())]).unwrap();
        assert!(tags::read_fields(&path).unwrap().iter().all(|(n, _)| n != "Comment"), "{name}");
    }
    assert!(tags::field_names().len() > 40);
    assert!(tags::write_fields(&dir.path().join("t.mp3"), &[("Nonsense".to_owned(), vec!["x".to_owned()])]).is_err());
}

#[tokio::test]
async fn set_cover_from_url_refuses_what_could_reach_inside_the_network() {
    let m = mcp().await;
    let read = m.store.create_mcp_token("reader", false, m.admin).await.unwrap();
    let write = m.store.create_mcp_token("writer", true, m.admin).await.unwrap();
    let id = album_without_audio(&m).await;
    let (err, text) = tool(&m, &read, "set_cover_from_url", json!({ "id": id, "url": "https://example.com/c.jpg" })).await;
    assert!(err && text.as_str().unwrap().contains("only read"), "{text}");
    for url in ["http://example.com/c.jpg", "file:///etc/passwd", "https://127.0.0.1/c.jpg", "https://192.168.24.250/c.jpg", "https://localhost/c.jpg"] {
        let (err, text) = tool(&m, &write, "set_cover_from_url", json!({ "id": id, "url": url })).await;
        assert!(err, "{url} must be refused: {text}");
    }
    assert!(m.store.tag_changes(0, 10).await.unwrap().is_empty(), "a refused fetch changes and logs nothing");
}

/// An album whose files exist (real MP3s are not needed: the refusals come before any file is touched).
async fn album_without_audio(m: &Mcp) -> i64 {
    let b = book("Follett", "Leer", &[1]);
    let folder = m.library.path().join(&b.folder);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(&b.tracks[0].file), b"not audio").unwrap();
    let name = b.folder.clone();
    m.store.import_books(vec![b]).await.unwrap();
    m.store.find_book(&name).await.unwrap().unwrap().id
}

#[tokio::test]
async fn list_and_help_for_describe_the_tools_and_the_help_text_can_be_set() {
    let m = mcp().await;
    let token = m.store.create_mcp_token("reader", false, m.admin).await.unwrap();

    let (err, listed) = tool(&m, &token, "list", json!({})).await;
    assert!(!err, "{listed}");
    let tools = listed["tools"].as_array().unwrap();
    let writes = |name: &str| tools.iter().find(|t| t["name"] == name).map(|t| t["changes_files"].as_bool().unwrap());
    assert_eq!((writes("set_tags"), writes("find_problems"), writes("help_for")), (Some(true), Some(false), Some(false)));
    assert_eq!(listed["your_token_may_write"], false);

    let (err, help) = tool(&m, &token, "help_for", json!({ "tool": "set_tags" })).await;
    assert!(!err, "{help}");
    assert!(help["description"].as_str().unwrap().contains("dry_run") && help["parameters"]["required"].is_array(), "{help}");
    assert_eq!(help["example"]["dry_run"], true);
    let (err, unknown) = tool(&m, &token, "help_for", json!({ "tool": "nope" })).await;
    assert!(err && unknown.as_str().unwrap().contains("set_tags"), "{unknown}");

    // The built-in help text, until the owner sets one; it is also what a client is told on connecting.
    let (_, general) = tool(&m, &token, "help_for", json!({})).await;
    assert!(general["help"].as_str().unwrap().contains("find_problems"), "{general}");
    m.store.set_mcp_prompt("Fix titles first. Never touch years.").await.unwrap();
    let (_, general) = tool(&m, &token, "help_for", json!({})).await;
    assert_eq!(general["help"], "Fix titles first. Never touch years.");
    let (_, init) = post(&m, Some(&token), json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} })).await;
    assert_eq!(init["result"]["instructions"], "Fix titles first. Never touch years.");
    m.store.set_mcp_prompt("  ").await.unwrap();
    let (_, general) = tool(&m, &token, "help_for", json!({})).await;
    assert!(general["help"].as_str().unwrap().contains("find_problems"));
}

#[tokio::test]
async fn find_problems_flags_titles_that_look_like_file_names() {
    if !ffmpeg_available() {
        return;
    }
    let m = mcp().await;
    let token = m.store.create_mcp_token("reader", false, m.admin).await.unwrap();
    let b = book("Die Rotsteinkids", "01_Das Geheimnis von Schloss Morillion", &[2]);
    let folder = m.library.path().join(&b.folder);
    for t in &b.tracks {
        make_mp3(&folder.join(&t.file), &[("album", "Das Geheimnis"), ("artist", "Die Rotsteinkids")]);
    }
    m.store.import_books(vec![b]).await.unwrap();
    let (_, found) = tool(&m, &token, "find_problems", json!({})).await;
    let problems = found["albums_with_problems"][0]["problems"].to_string();
    assert!(problems.contains("title_has_underscores_or_number_prefix"), "{problems}");
}

#[tokio::test]
async fn an_audiobook_in_the_music_folder_moves_to_the_audiobooks_after_a_dry_run_and_keeps_its_id() {
    if !ffmpeg_available() {
        return;
    }
    let m = mcp().await;
    let books = tempfile::TempDir::new().unwrap();
    let music = tempfile::TempDir::new().unwrap();
    m.store.save_import_criteria(&[books.path().display().to_string()], david_server::importer::RatingRule::Manual).await.unwrap();
    m.store.save_music_roots(&[music.path().display().to_string()]).await.unwrap();
    make_mp3(&music.path().join("NORA/01_Alt/01.mp3"), &[("album", "Die Vermessung der Welt"), ("album_artist", "Daniel Kehlmann"), ("genre", "Lesung"), ("track", "1")]);
    make_mp3(&music.path().join("NORA/Song/01.mp3"), &[("album", "Song"), ("album_artist", "Band"), ("genre", "Rock"), ("track", "1")]);
    m.store.scan_kind(music.path(), "music").await.unwrap();
    let write = m.store.create_mcp_token("w", true, m.admin).await.unwrap();
    let read = m.store.create_mcp_token("r", false, m.admin).await.unwrap();
    let id = m.store.find_book("NORA/01_Alt").await.unwrap().unwrap().id;

    // The default is a dry run, and a reading token may do it: only the audiobook is found.
    let (err, plan) = tool(&m, &read, "move_to_audiobooks", json!({})).await;
    assert!(!err, "{plan}");
    assert_eq!((plan["dry_run"].as_bool(), plan["movable"].as_u64(), plan["albums"].as_array().unwrap().len()), (Some(true), Some(1), 1), "{plan}");
    assert!(music.path().join("NORA/01_Alt/01.mp3").is_file());
    assert!(plan["albums"][0]["to"].as_str().unwrap().ends_with("Daniel Kehlmann/Die Vermessung der Welt"), "{plan}");

    // Moving needs a token that may write.
    let (err, _) = tool(&m, &read, "move_to_audiobooks", json!({ "dry_run": false })).await;
    assert!(err);
    let (err, done) = tool(&m, &write, "move_to_audiobooks", json!({ "dry_run": false })).await;
    assert!(!err && done["moved"] == 1, "{done}");
    assert!(!music.path().join("NORA/01_Alt").exists());
    assert!(books.path().join("Daniel Kehlmann/Die Vermessung der Welt/01.mp3").is_file());
    assert!(music.path().join("NORA/Song/01.mp3").is_file());

    // The same book, with its id, now an audiobook in the new folder.
    let row = m.store.admin_book_row(id).await.unwrap().unwrap();
    assert_eq!((row.kind.as_str(), row.author.as_str(), row.title.as_str()), ("audiobook", "Daniel Kehlmann", "Die Vermessung der Welt"));
    assert_eq!(m.store.book_folder(id).await.unwrap().as_deref(), Some("Daniel Kehlmann/Die Vermessung der Welt"));
    // Nothing is left to move; a second run is quiet.
    let (_, again) = tool(&m, &write, "move_to_audiobooks", json!({ "dry_run": false })).await;
    assert_eq!(again["albums"].as_array().unwrap().len(), 0, "{again}");
}

#[tokio::test]
async fn the_cds_of_one_audiobook_become_one_book_with_disc_folders_and_keep_the_progress() {
    if !ffmpeg_available() {
        return;
    }
    let m = mcp().await;
    let books = tempfile::TempDir::new().unwrap();
    let music = tempfile::TempDir::new().unwrap();
    m.store.save_import_criteria(&[books.path().display().to_string()], david_server::importer::RatingRule::Manual).await.unwrap();
    m.store.save_music_roots(&[music.path().display().to_string()]).await.unwrap();
    for n in 1..=3 {
        let folder = music.path().join(format!("Hörbücher/Kehlmann - Vermessung/Die Vermessung der Welt CD0{n}"));
        for t in 1..=2 {
            make_mp3(&folder.join(format!("{t:02}.mp3")), &[("album", &format!("Die Vermessung der Welt (CD {n})")), ("album_artist", "Daniel Kehlmann"), ("genre", "Lesung"), ("track", &t.to_string())]);
        }
    }
    // A book without an author in the tags is left alone.
    make_mp3(&music.path().join("Hörbücher/Schule/Ginger/01.mp3"), &[("album", "Ginger"), ("genre", "Hörbuch"), ("track", "1")]);
    m.store.scan_kind(music.path(), "music").await.unwrap();
    let write = m.store.create_mcp_token("w", true, m.admin).await.unwrap();
    async fn id(m: &Mcp, name: &str) -> i64 {
        m.store.find_book(name).await.unwrap().unwrap().id
    }
    let dir = "Hörbücher/Kehlmann - Vermessung/Die Vermessung der Welt";
    let (cd1, cd2, cd3) = (id(&m, &format!("{dir} CD01")).await, id(&m, &format!("{dir} CD02")).await, id(&m, &format!("{dir} CD03")).await);
    let ginger = id(&m, "Hörbücher/Schule/Ginger").await;
    // Somebody had listened to the second CD.
    let second_track = m.store.tracks_of(cd2).await.unwrap()[1].id;
    m.store.save_progress(david_server::ProgressUpdate { profile_id: m.admin, book_id: cd2, track_id: second_track, position_ms: 4242, finished: false, updated_at: 1 }).await.unwrap();
    m.store.flush().await.unwrap();

    let (_, plan) = tool(&m, &write, "move_to_audiobooks", json!({})).await;
    assert_eq!(plan["movable"], 3, "{plan}");
    let blocked: Vec<&Value> = plan["albums"].as_array().unwrap().iter().filter(|a| !a["problem"].is_null()).collect();
    assert_eq!((blocked.len(), blocked[0]["id"].as_i64()), (1, Some(ginger)), "{plan}");
    assert!(blocked[0]["problem"].as_str().unwrap().contains("author"), "{plan}");
    assert!(music.path().join("Hörbücher/Kehlmann - Vermessung/Die Vermessung der Welt CD02").is_dir(), "a dry run moves nothing");

    let (err, done) = tool(&m, &write, "move_to_audiobooks", json!({ "dry_run": false })).await;
    assert!(!err && done["moved"] == 3, "{done}");
    for n in 1..=3 {
        assert!(books.path().join(format!("Daniel Kehlmann/Die Vermessung der Welt/CD {n}/01.mp3")).is_file(), "CD {n}");
    }
    assert!(music.path().join("Hörbücher/Schule/Ginger/01.mp3").is_file());

    // One book remains, the first CD's, with all six tracks as discs 1 to 3, and the progress came along.
    let row = m.store.admin_book_row(cd1).await.unwrap().unwrap();
    assert_eq!((row.kind.as_str(), row.title.as_str(), row.author.as_str()), ("audiobook", "Die Vermessung der Welt", "Daniel Kehlmann"));
    assert!(m.store.admin_book_row(cd2).await.unwrap().is_none() && m.store.admin_book_row(cd3).await.unwrap().is_none());
    let tracks = m.store.tracks_of(cd1).await.unwrap();
    assert_eq!(tracks.iter().map(|t| (t.disc, t.number, t.file.as_str())).collect::<Vec<_>>().len(), 6);
    assert_eq!(tracks.iter().map(|t| t.disc).collect::<Vec<_>>(), vec![1, 1, 2, 2, 3, 3]);
    assert_eq!(tracks[2].file, "CD 2/01.mp3");
    let p = m.store.progress_of(m.admin, cd1).await.unwrap().expect("the progress moved to the remaining book");
    assert_eq!((p.position_ms, p.track_id), (4242, second_track));
    // A scan of the audiobook folder finds the same book and its discs, not a new one.
    m.store.scan_kind(books.path(), "audiobook").await.unwrap();
    assert_eq!(m.store.find_book("Daniel Kehlmann/Die Vermessung der Welt").await.unwrap().unwrap().id, cd1);
    assert_eq!(m.store.tracks_of(cd1).await.unwrap().len(), 6);
}

#[tokio::test]
async fn a_copy_goes_to_the_quarantine_of_its_import_folder_and_one_copy_always_stays() {
    if !ffmpeg_available() {
        return;
    }
    let m = mcp().await;
    let books = tempfile::TempDir::new().unwrap();
    let music = tempfile::TempDir::new().unwrap();
    m.store.save_import_criteria(&[books.path().display().to_string()], david_server::importer::RatingRule::Manual).await.unwrap();
    m.store.save_music_roots(&[music.path().display().to_string()]).await.unwrap();
    for folder in ["Compilations/Ohrenöffner", "Schule/Ohrenöffner"] {
        make_mp3(&music.path().join(folder).join("01.mp3"), &[("album", "Ohrenöffner"), ("album_artist", "Various Kids"), ("genre", "Kinder"), ("track", "1")]);
    }
    make_mp3(&music.path().join("Schule/Solo/01.mp3"), &[("album", "Solo"), ("album_artist", "Einer"), ("track", "1")]);
    m.store.scan_kind(music.path(), "music").await.unwrap();
    let write = m.store.create_mcp_token("w", true, m.admin).await.unwrap();
    let read = m.store.create_mcp_token("r", false, m.admin).await.unwrap();
    let a = m.store.find_book("Compilations/Ohrenöffner").await.unwrap().unwrap().id;
    let b = m.store.find_book("Schule/Ohrenöffner").await.unwrap().unwrap().id;
    let solo = m.store.find_book("Schule/Solo").await.unwrap().unwrap().id;

    let (_, dupes) = tool(&m, &read, "find_duplicates", json!({})).await;
    assert_eq!(dupes["groups"], 1, "{dupes}");

    // Not both, and not a book that has no copy.
    let (_, both) = tool(&m, &read, "quarantine_duplicates", json!({ "ids": [a, b] })).await;
    assert!(both["albums"].as_array().unwrap().iter().all(|x| x["problem"].as_str().unwrap().contains("one has to stay")), "{both}");
    let (_, single) = tool(&m, &read, "quarantine_duplicates", json!({ "ids": [solo] })).await;
    assert!(single["albums"][0]["problem"].as_str().unwrap().contains("not part of a group"), "{single}");

    // A dry run and a reading token change nothing; moving needs a writing one.
    let (_, dry) = tool(&m, &read, "quarantine_duplicates", json!({ "ids": [b] })).await;
    assert_eq!((dry["dry_run"].as_bool(), dry["movable"].as_u64()), (Some(true), Some(1)), "{dry}");
    assert!(music.path().join("Schule/Ohrenöffner/01.mp3").is_file());
    let (err, _) = tool(&m, &read, "quarantine_duplicates", json!({ "ids": [b], "dry_run": false })).await;
    assert!(err);

    let (err, done) = tool(&m, &write, "quarantine_duplicates", json!({ "ids": [b], "dry_run": false })).await;
    assert!(!err && done["moved"] == 1, "{done}");
    assert!(!music.path().join("Schule/Ohrenöffner").exists());
    assert!(music.path().join(".quarantine/Schule/Ohrenöffner/01.mp3").is_file(), "the same place below the quarantine of the import folder");
    assert!(m.store.admin_book_row(b).await.unwrap().is_none() && m.store.admin_book_row(a).await.unwrap().is_some());
    // The scan does not see the quarantine: the book does not come back.
    m.store.scan_kind(music.path(), "music").await.unwrap();
    assert!(m.store.find_book("Schule/Ohrenöffner").await.unwrap().is_none());
    // Nothing is left to put aside.
    let (_, rest) = tool(&m, &write, "find_duplicates", json!({})).await;
    assert_eq!(rest["groups"], 0, "{rest}");
}

#[tokio::test]
async fn the_blurb_is_set_read_back_logged_and_needs_a_token_that_may_write() {
    let m = mcp().await;
    let id = if ffmpeg_available() { album(&m, &[("album", "Winter")]).await } else { return };
    let write = m.store.create_mcp_token("w", true, m.admin).await.unwrap();
    let read = m.store.create_mcp_token("r", false, m.admin).await.unwrap();
    let (err, _) = tool(&m, &read, "set_description", json!({ "id": id, "text": "x" })).await;
    assert!(err, "a reading token may not");
    let (err, r) = tool(&m, &write, "set_description", json!({ "id": id, "text": "  Ein Winter, der alles verändert.  " })).await;
    assert!(!err && r["chars"] == 32, "{r}");
    let (_, got) = tool(&m, &write, "get_album", json!({ "id": id })).await;
    assert_eq!(got["description"], "Ein Winter, der alles verändert.");
    let (_, log) = tool(&m, &write, "change_log", json!({ "id": id })).await;
    assert!(log.to_string().contains("Description") && log.to_string().contains("Ein Winter"), "{log}");
    let (err, _) = tool(&m, &write, "set_description", json!({ "id": id, "text": "x".repeat(4001) })).await;
    assert!(err, "too long");
    let (_, _) = tool(&m, &write, "set_description", json!({ "id": id, "text": "" })).await;
    let (_, got) = tool(&m, &write, "get_album", json!({ "id": id })).await;
    assert!(got["description"].is_null(), "{got}");
}

#[tokio::test]
async fn ignore_rules_hide_duplicates_and_problems_and_can_be_listed_and_removed() {
    let m = mcp().await;
    for (author, title) in [("Follett", "Kinder der Freiheit"), ("Johannes Steck", "Kinder der Freiheit"), ("Follett", "Winter der Welt")] {
        m.store.import_books(vec![book(author, title, &[2])]).await.unwrap();
    }
    let write = m.store.create_mcp_token("w", true, m.admin).await.unwrap();
    let read = m.store.create_mcp_token("r", false, m.admin).await.unwrap();
    let rows = m.store.admin_book_rows().await.unwrap();
    let steck = rows.iter().find(|r| r.author == "Johannes Steck").unwrap().id;
    let (_, d) = tool(&m, &read, "find_duplicates", json!({})).await;
    assert_eq!(d["groups"], 1, "{d}");

    let (err, _) = tool(&m, &read, "ignore", json!({ "id": steck, "scope": "album" })).await;
    assert!(err, "a reading token may not");
    let (err, bad) = tool(&m, &write, "ignore", json!({ "id": steck, "scope": "everything" })).await;
    assert!(err, "{bad}");
    // This one album is wanted: no duplicate is left, the other one is not paired with anything.
    let (err, made) = tool(&m, &write, "ignore", json!({ "id": steck, "scope": "album", "note": "both copies are wanted" })).await;
    assert!(!err, "{made}");
    let (_, d) = tool(&m, &read, "find_duplicates", json!({})).await;
    assert_eq!((d["groups"].as_u64(), d["ignored_by_rules"].as_u64()), (Some(0), Some(1)), "{d}");
    // The same rule twice is one rule.
    tool(&m, &write, "ignore", json!({ "id": steck, "scope": "album" })).await;
    let (_, list) = tool(&m, &read, "list_ignores", json!({})).await;
    assert_eq!(list["rules"].as_array().unwrap().len(), 1, "{list}");
    assert_eq!(list["rules"][0]["note"], "both copies are wanted");

    // A problem code can be hidden too, for a work with all its CDs, and `*` hides everything.
    let (_, p) = tool(&m, &read, "find_problems", json!({})).await;
    let winter = rows.iter().find(|r| r.title == "Winter der Welt").unwrap().id;
    let has = |p: &Value, id: i64, what: &str| p["albums_with_problems"].as_array().unwrap().iter().any(|a| a["id"] == id && a["problems"].to_string().contains(what));
    assert!(has(&p, winter, "files_missing"), "{p}");
    tool(&m, &write, "ignore", json!({ "id": winter, "scope": "work", "what": "files_missing" })).await;
    let (_, p) = tool(&m, &read, "find_problems", json!({})).await;
    assert!(!has(&p, winter, "files_missing"), "{p}");

    let rule = list["rules"][0]["id"].as_i64().unwrap();
    let (_, gone) = tool(&m, &write, "unignore", json!({ "rule": rule })).await;
    assert_eq!(gone["removed"], true);
    let (_, d) = tool(&m, &read, "find_duplicates", json!({})).await;
    assert_eq!(d["groups"], 1, "back in the report: {d}");
}

#[tokio::test]
async fn the_quarantine_lists_what_the_server_put_there_and_what_was_done_by_hand() {
    if !ffmpeg_available() {
        return;
    }
    let m = mcp().await;
    let books = tempfile::TempDir::new().unwrap();
    let music = tempfile::TempDir::new().unwrap();
    m.store.save_import_criteria(&[books.path().display().to_string()], david_server::importer::RatingRule::Manual).await.unwrap();
    m.store.save_music_roots(&[music.path().display().to_string()]).await.unwrap();
    for folder in ["A/Doppelt", "B/Doppelt"] {
        make_mp3(&music.path().join(folder).join("01.mp3"), &[("album", "Doppelt"), ("album_artist", "Band"), ("track", "1")]);
    }
    m.store.scan_kind(music.path(), "music").await.unwrap();
    let write = m.store.create_mcp_token("w", true, m.admin).await.unwrap();
    let id = m.store.find_book("B/Doppelt").await.unwrap().unwrap().id;
    let (err, _) = tool(&m, &write, "quarantine_duplicates", json!({ "ids": [id], "dry_run": false })).await;
    assert!(!err);
    // Somebody also moves a folder into the quarantine by hand.
    make_mp3(&music.path().join(".quarantine/Von Hand/Album/01.mp3"), &[("album", "Von Hand"), ("track", "1")]);

    let (_, q) = tool(&m, &write, "list_quarantine", json!({})).await;
    assert_eq!((q["albums"].as_u64(), q["by_hand"].as_u64()), (Some(2), Some(1)), "{q}");
    let by = |was: &str| q["entries"].as_array().unwrap().iter().find(|e| e["was"] == was).unwrap_or_else(|| panic!("{was} missing in {q}")).clone();
    assert_eq!(by("B/Doppelt")["put_there_by"], "server (w)");
    assert_eq!(by("B/Doppelt")["book_id"], id);
    assert_eq!(by("Von Hand/Album")["put_there_by"], "by hand");
}

#[tokio::test]
async fn the_age_recommendation_is_set_checked_logged_and_does_not_release() {
    let m = mcp().await;
    m.store.import_books(vec![book("Follett", "Winter der Welt", &[1])]).await.unwrap();
    let id = m.store.admin_book_rows().await.unwrap()[0].id;
    let write = m.store.create_mcp_token("w", true, m.admin).await.unwrap();
    let read = m.store.create_mcp_token("r", false, m.admin).await.unwrap();
    assert!(tool(&m, &read, "set_recommended_age", json!({ "id": id, "age": 8 })).await.0, "a reading token may not");
    for bad in [json!(-1), json!(22), json!("acht")] {
        assert!(tool(&m, &write, "set_recommended_age", json!({ "id": id, "age": bad })).await.0, "{bad}");
    }
    let (err, ok) = tool(&m, &write, "set_recommended_age", json!({ "id": id, "age": 12 })).await;
    assert!(!err && ok["recommended_age"] == 12, "{ok}");
    let row = m.store.admin_book_row(id).await.unwrap().unwrap();
    assert_eq!((row.recommended_age, row.released), (Some(12), false), "information only: nothing is released");
    let (_, log) = tool(&m, &write, "change_log", json!({ "id": id })).await;
    assert!(log.to_string().contains("RecommendedAge"), "{log}");
    let (err, _) = tool(&m, &write, "set_recommended_age", json!({ "id": id, "age": null })).await;
    assert!(!err && m.store.admin_book_row(id).await.unwrap().unwrap().recommended_age.is_none());
}

#[tokio::test]
async fn the_album_list_can_carry_the_blurbs_and_filter_by_having_one() {
    let m = mcp().await;
    for (author, title) in [("Follett", "Winter der Welt"), ("Tolkien", "Der Hobbit")] {
        m.store.import_books(vec![book(author, title, &[1])]).await.unwrap();
    }
    let write = m.store.create_mcp_token("w", true, m.admin).await.unwrap();
    let winter = m.store.admin_book_rows().await.unwrap().iter().find(|r| r.title == "Winter der Welt").unwrap().id;
    tool(&m, &write, "set_description", json!({ "id": winter, "text": "Zweiter Band der Trilogie." })).await;
    tool(&m, &write, "set_recommended_age", json!({ "id": winter, "age": 14 })).await;

    let (_, all) = tool(&m, &write, "list_albums", json!({})).await;
    let first = &all["albums"].as_array().unwrap().iter().find(|a| a["id"] == winter).unwrap().clone();
    assert_eq!((first["has_description"].as_bool(), first["recommended_age"].as_i64()), (Some(true), Some(14)), "{all}");
    assert!(first.get("description").is_none(), "the text only comes on request");

    let (_, with) = tool(&m, &write, "list_albums", json!({ "with_description": true })).await;
    let texts: Vec<_> = with["albums"].as_array().unwrap().iter().map(|a| a["description"].clone()).collect();
    assert!(texts.contains(&json!("Zweiter Band der Trilogie.")) && texts.contains(&Value::Null), "{texts:?}");

    let (_, without) = tool(&m, &write, "list_albums", json!({ "has_description": false })).await;
    assert_eq!((without["total"].as_u64(), without["albums"][0]["title"].as_str()), (Some(1), Some("Der Hobbit")), "{without}");
    let (_, only) = tool(&m, &write, "list_albums", json!({ "has_description": true })).await;
    assert_eq!(only["total"], 1);
}

#[tokio::test]
async fn a_narrator_removed_from_the_tags_is_removed_from_the_library_at_the_next_scan() {
    if !ffmpeg_available() {
        return;
    }
    let m = mcp().await;
    let music = tempfile::TempDir::new().unwrap();
    m.store.save_import_criteria(&[music.path().display().to_string()], david_server::importer::RatingRule::Manual).await.unwrap();
    make_mp3(&music.path().join("Band/Album/01.mp3"), &[("album", "Album"), ("album_artist", "Band"), ("composer", "Ein Verein e.V."), ("track", "1")]);
    m.store.scan_kind(music.path(), "audiobook").await.unwrap();
    let id = m.store.find_book("Band/Album").await.unwrap().unwrap().id;
    assert_eq!(m.store.admin_book_row(id).await.unwrap().unwrap().narrator.as_deref(), Some("Ein Verein e.V."));

    let write = m.store.create_mcp_token("w", true, m.admin).await.unwrap();
    let (err, _) = tool(&m, &write, "set_tags", json!({ "id": id, "fields": { "Composer": null } })).await;
    assert!(!err);
    m.store.scan_kind(music.path(), "audiobook").await.unwrap();
    assert_eq!(m.store.admin_book_row(id).await.unwrap().unwrap().narrator, None, "the tag is empty, so the list says nobody reads it");
}

#[tokio::test]
async fn folders_left_empty_by_a_move_are_removed_and_never_reported_by_the_import() {
    if !ffmpeg_available() {
        return;
    }
    let m = mcp().await;
    let books = tempfile::TempDir::new().unwrap();
    let music = tempfile::TempDir::new().unwrap();
    m.store.save_import_criteria(&[books.path().display().to_string()], david_server::importer::RatingRule::Manual).await.unwrap();
    m.store.save_music_roots(&[music.path().display().to_string()]).await.unwrap();
    for folder in ["Johannes Steck/Doppelt", "Follett/Doppelt"] {
        make_mp3(&music.path().join(folder).join("01.mp3"), &[("album", "Doppelt"), ("album_artist", "Band"), ("track", "1")]);
    }
    m.store.scan_kind(music.path(), "music").await.unwrap();
    let write = m.store.create_mcp_token("w", true, m.admin).await.unwrap();
    let id = m.store.find_book("Johannes Steck/Doppelt").await.unwrap().unwrap().id;
    let (err, _) = tool(&m, &write, "quarantine_duplicates", json!({ "ids": [id], "dry_run": false })).await;
    assert!(!err);
    assert!(music.path().join(".quarantine/Johannes Steck/Doppelt/01.mp3").is_file());
    assert!(!music.path().join("Johannes Steck").exists(), "the author's folder is empty now and goes with it");
    assert!(music.path().join("Follett/Doppelt/01.mp3").is_file() && music.path().exists(), "the other copy and the import folder stay");

    // An empty folder from before is not a problem in the report, and the tool finds and removes it.
    std::fs::create_dir_all(music.path().join("Alt/Leer/Noch Leerer")).unwrap();
    std::fs::write(music.path().join("Behalten").join("x.txt"), b"x").ok();
    let report = m.store.scan_kind(music.path(), "music").await.unwrap();
    assert!(!report.problems.iter().any(|p| p.contains("nothing to import")), "{:?}", report.problems);
    let (_, dry) = tool(&m, &write, "remove_empty_folders", json!({})).await;
    assert_eq!(dry["empty_folders"].as_array().unwrap().len(), 1, "{dry}");
    assert!(music.path().join("Alt").is_dir(), "a dry run removes nothing");
    let (_, done) = tool(&m, &write, "remove_empty_folders", json!({ "dry_run": false })).await;
    assert_eq!(done["removed"].as_array().unwrap().len(), 1, "{done}");
    assert!(!music.path().join("Alt").exists() && music.path().join(".quarantine").is_dir(), "the quarantine is never touched");
}

#[tokio::test]
async fn labels_are_listed_and_set_by_a_token_that_may_write() {
    let m = mcp().await;
    m.store.import_books(vec![common::book("A", "Eins", &[1]), common::book("B", "Zwei", &[1])]).await.unwrap();
    let one = m.store.find_book("A/Eins").await.unwrap().unwrap().id;
    let two = m.store.find_book("B/Zwei").await.unwrap().unwrap().id;
    let write = m.store.create_mcp_token("w", true, m.admin).await.unwrap();
    let read = m.store.create_mcp_token("r", false, m.admin).await.unwrap();

    // A look changes nothing; a reading token may not write; a bad name or an empty request is refused.
    let (err, look) = tool(&m, &read, "set_labels", json!({ "ids": [one, two], "add": ["Gebet", "Schule"] })).await;
    assert!(!err && look["dry_run"] == true && look["changed"] == 2 && m.store.labels().await.unwrap().is_empty(), "{look}");
    assert!(tool(&m, &read, "set_labels", json!({ "ids": [one], "add": ["Gebet"], "dry_run": false })).await.0);
    assert!(tool(&m, &write, "set_labels", json!({ "ids": [one], "add": ["a,b"], "dry_run": false })).await.0);
    assert!(tool(&m, &write, "set_labels", json!({ "ids": [one] })).await.0);
    assert!(tool(&m, &write, "set_labels", json!({ "ids": [one], "set": ["x"], "add": ["y"] })).await.0);

    let (err, done) = tool(&m, &write, "set_labels", json!({ "ids": [one, two], "add": ["Gebet", "Schule"], "dry_run": false })).await;
    assert!(!err && done["changed"] == 2, "{done}");
    let (_, again) = tool(&m, &write, "set_labels", json!({ "ids": [one, two], "add": ["gebet"], "dry_run": false })).await;
    assert_eq!(again["changed"], 0, "case is ignored: {again}");
    let (_, removed) = tool(&m, &write, "set_labels", json!({ "ids": [two], "remove": ["GEBET"], "dry_run": false })).await;
    assert_eq!(removed["albums"][0]["after"], json!(["Schule"]), "{removed}");
    let (_, list) = tool(&m, &read, "list_labels", json!({})).await;
    assert_eq!(list["labels"], json!([{ "name": "Gebet", "books": 1, "tracks": 0, "podcasts": 0, "stations": 0 }, { "name": "Schule", "books": 2, "tracks": 0, "podcasts": 0, "stations": 0 }]), "{list}");
    let (_, of) = tool(&m, &read, "list_labels", json!({ "ids": [one, 99999] })).await;
    assert_eq!(of["albums"][0]["labels"], json!(["Gebet", "Schule"]), "{of}");
    // set replaces them all, an empty list takes them off; an unknown album is told.
    let (_, set) = tool(&m, &write, "set_labels", json!({ "ids": [one, 99999], "set": [], "dry_run": false })).await;
    assert_eq!((set["albums"][0]["after"].clone(), set["albums"][1]["problem"].as_str()), (json!([]), Some("no such album")), "{set}");
    assert_eq!(m.store.labels().await.unwrap().len(), 1);
}

#[tokio::test]
async fn cds_are_put_together_by_the_numbers_of_their_tags_or_by_numbers_given_and_a_missing_cd_is_shown() {
    if !ffmpeg_available() {
        return;
    }
    let m = mcp().await;
    let music = tempfile::TempDir::new().unwrap();
    m.store.save_music_roots(&[music.path().display().to_string()]).await.unwrap();
    for n in [1, 2, 3] {
        make_mp3(&music.path().join(format!("NORA/Heck/Heimatlieder {n}/01.mp3")), &[("album", &format!("Heimatlieder {n}")), ("artist", "Heck"), ("disc", &format!("{n}/4")), ("track", "1")]);
    }
    m.store.scan_kind(music.path(), "music").await.unwrap();
    let write = m.store.create_mcp_token("w", true, m.admin).await.unwrap();
    let read = m.store.create_mcp_token("r", false, m.admin).await.unwrap();
    let id = |n: usize| { let store = m.store.clone(); async move { store.find_book(&format!("NORA/Heck/Heimatlieder {n}")).await.unwrap().unwrap().id } };
    let (a, b, c) = (id(1).await, id(2).await, id(3).await);

    // The look: the numbers come from the tags (given in the wrong order on purpose), the total too; nothing moves.
    let (err, look) = tool(&m, &read, "merge_albums", json!({ "ids": [c, a, b] })).await;
    assert!(!err && look["dry_run"] == true && look["merged"] == false && look["discs_total"] == 4, "{look}");
    assert_eq!(look["albums"].as_array().unwrap().iter().map(|x| x["disc"].as_i64().unwrap()).collect::<Vec<_>>(), vec![3, 1, 2], "{look}");
    assert_eq!((look["title"].as_str(), look["folder"].as_str()), (Some("Heimatlieder"), Some("NORA/Heck/Heimatlieder")), "{look}");
    assert!(music.path().join("NORA/Heck/Heimatlieder 1/01.mp3").is_file());
    assert!(tool(&m, &read, "merge_albums", json!({ "ids": [a, b], "dry_run": false })).await.0, "a reading token may not");
    assert!(tool(&m, &write, "merge_albums", json!({ "ids": [a] })).await.0, "one album is no work of several CDs");
    assert!(tool(&m, &write, "merge_albums", json!({ "ids": [a, b], "discs": [1, 1] })).await.0, "a number twice");

    // The numbers are given: they win over the tags and go into the files.
    let (err, done) = tool(&m, &write, "merge_albums", json!({ "ids": [a, b, c], "discs": [1, 2, 3], "dry_run": false })).await;
    assert!(!err && done["merged"] == true, "{done}");
    for n in [1, 2, 3] {
        assert!(music.path().join(format!("NORA/Heck/Heimatlieder/CD {n}/01.mp3")).is_file(), "CD {n}");
    }
    assert!(!music.path().join("NORA/Heck/Heimatlieder 1").exists() && !music.path().join("NORA/Heck/Heimatlieder 2").exists());
    let row = m.store.admin_book_row(a).await.unwrap().unwrap();
    assert_eq!((row.kind.as_str(), row.title.as_str(), row.discs, row.discs_total), ("music", "Heimatlieder", 3, Some(4)), "one book of three CDs; music stays music");
    assert!(m.store.admin_book_row(b).await.unwrap().is_none() && m.store.admin_book_row(c).await.unwrap().is_none(), "the others went into the first");
    // The CD that is missing is shown like a missing track.
    let gaps = m.store.track_gaps().await.unwrap();
    assert!(gaps.get(&a).is_some_and(|g| g.contains("CD 4")), "{gaps:?}");
    // The tags carry the numbers and the title: the next import changes nothing.
    let fields = david_server::tags::read_fields(&music.path().join("NORA/Heck/Heimatlieder/CD 2/01.mp3")).unwrap();
    let value = |n: &str| fields.iter().find(|(f, _)| f == n).map(|(_, v)| v.join("/")).unwrap_or_default();
    assert_eq!((value("AlbumTitle").as_str(), value("DiscNumber").as_str(), value("DiscTotal").as_str()), ("Heimatlieder", "2", "4"), "{fields:?}");
    m.store.scan_kind(music.path(), "music").await.unwrap();
    let again = m.store.admin_book_row(a).await.unwrap().unwrap();
    assert_eq!((again.title.as_str(), again.discs, again.discs_total), ("Heimatlieder", 3, Some(4)), "the import reads the same from the files");
    // How many CDs a work has can be told afterwards, and taken away.
    let (err, set) = tool(&m, &write, "set_discs_total", json!({ "id": a, "total": 3 })).await;
    assert!(!err && set["discs_total"] == 3, "{set}");
    assert!(!m.store.track_gaps().await.unwrap().contains_key(&a), "three of three: nothing is missing");
    assert!(tool(&m, &write, "set_discs_total", json!({ "id": a, "total": 100 })).await.0);
}

#[tokio::test]
async fn a_branch_of_the_filing_moves_up_with_ids_kept_and_a_place_that_exists_stays() {
    if !ffmpeg_available() {
        return;
    }
    let m = mcp().await;
    let music = tempfile::TempDir::new().unwrap();
    m.store.save_music_roots(&[music.path().display().to_string()]).await.unwrap();
    make_mp3(&music.path().join("JÖRG/MP3-I/Artist A/Album 1/01.mp3"), &[("album", "Album 1"), ("artist", "Artist A"), ("track", "1")]);
    make_mp3(&music.path().join("JÖRG/MP3-I/Artist B/Album 2/01.mp3"), &[("album", "Album 2"), ("artist", "Artist B"), ("track", "1")]);
    make_mp3(&music.path().join("JÖRG/MP3-I/Single/01.mp3"), &[("album", "Single"), ("artist", "Solo"), ("track", "1")]);
    make_mp3(&music.path().join("JÖRG/Artist B/Album 2/01.mp3"), &[("album", "Album 2 again"), ("artist", "Artist B"), ("track", "1")]);
    m.store.scan_kind(music.path(), "music").await.unwrap();
    let write = m.store.create_mcp_token("w", true, m.admin).await.unwrap();
    let read = m.store.create_mcp_token("r", false, m.admin).await.unwrap();
    let a1 = m.store.find_book("JÖRG/MP3-I/Artist A/Album 1").await.unwrap().unwrap().id;
    let a2 = m.store.find_book("JÖRG/MP3-I/Artist B/Album 2").await.unwrap().unwrap().id;
    let single = m.store.find_book("JÖRG/MP3-I/Single").await.unwrap().unwrap().id;

    // The default is a look: three albums move, the one whose place exists already is told as a problem.
    let (err, plan) = tool(&m, &read, "move_albums", json!({ "from": "JÖRG/MP3-I", "to": "JÖRG" })).await;
    assert!(!err && plan["dry_run"] == true && plan["movable"] == 2 && plan["blocked"] == 1 && plan["moved"] == 0, "{plan}");
    assert!(music.path().join("JÖRG/MP3-I/Artist A/Album 1/01.mp3").is_file(), "a look changes nothing");
    let (err, _) = tool(&m, &read, "move_albums", json!({ "from": "JÖRG/MP3-I", "to": "JÖRG", "dry_run": false })).await;
    assert!(err, "a reading token may not move");
    for bad in [json!({ "from": "" }), json!({ "from": "JÖRG/../x", "to": "JÖRG" }), json!({ "from": "JÖRG", "to": "JÖRG/MP3-I" }), json!({ "from": "JÖRG", "to": "JÖRG" })] {
        let (err, _) = tool(&m, &write, "move_albums", bad.clone()).await;
        assert!(err, "{bad}");
    }

    let (err, done) = tool(&m, &write, "move_albums", json!({ "from": "JÖRG/MP3-I", "to": "JÖRG", "dry_run": false })).await;
    assert!(!err && done["moved"] == 2 && done["failed"] == 0, "{done}");
    assert!(music.path().join("JÖRG/Artist A/Album 1/01.mp3").is_file() && music.path().join("JÖRG/Single/01.mp3").is_file());
    assert!(!music.path().join("JÖRG/MP3-I/Artist A").exists() && !music.path().join("JÖRG/MP3-I/Single").exists());
    assert!(music.path().join("JÖRG/MP3-I/Artist B/Album 2/01.mp3").is_file(), "the album whose place exists stays where it is");
    // The ids stay, the places are the new ones.
    assert_eq!(m.store.book_folder(a1).await.unwrap().as_deref(), Some("JÖRG/Artist A/Album 1"));
    assert_eq!(m.store.book_folder(single).await.unwrap().as_deref(), Some("JÖRG/Single"));
    assert_eq!(m.store.book_folder(a2).await.unwrap().as_deref(), Some("JÖRG/MP3-I/Artist B/Album 2"));
    // A second run has only the album that could not move.
    let (_, again) = tool(&m, &write, "move_albums", json!({ "from": "JÖRG/MP3-I", "to": "JÖRG" })).await;
    assert_eq!((again["movable"].as_i64(), again["blocked"].as_i64()), (Some(0), Some(1)), "{again}");
}

#[tokio::test]
async fn an_album_is_renamed_and_moved_by_author_and_title_and_keeps_its_id() {
    if !ffmpeg_available() {
        return;
    }
    let m = mcp().await;
    let music = tempfile::TempDir::new().unwrap();
    m.store.save_music_roots(&[music.path().display().to_string()]).await.unwrap();
    make_mp3(&music.path().join("JÖRG/MP3-I/Edit Piaf/CD1/01.mp3"), &[("album", "Chansons"), ("genre", "Chanson"), ("track", "1")]);
    make_mp3(&music.path().join("JÖRG/MP3-I/Other/01.mp3"), &[("album", "Other"), ("genre", "Pop"), ("track", "1")]);
    m.store.scan_kind(music.path(), "music").await.unwrap();
    let write = m.store.create_mcp_token("w", true, m.admin).await.unwrap();
    let read = m.store.create_mcp_token("r", false, m.admin).await.unwrap();
    let id = m.store.find_book("JÖRG/MP3-I/Edit Piaf").await.unwrap().unwrap().id;
    let other = m.store.find_book("JÖRG/MP3-I/Other").await.unwrap().unwrap().id;

    // A dry run (the default) shows the new place and changes nothing.
    let (err, plan) = tool(&m, &read, "rename_album", json!({ "id": id, "author": "Edith Piaf", "title": "Edith Piaf" })).await;
    assert!(!err && plan["renamed"] == false && plan["plan"]["problem"].is_null(), "{plan}");
    assert_eq!(plan["plan"]["new_folder"], "JÖRG/Edith Piaf/Edith Piaf", "{plan}");
    assert!(music.path().join("JÖRG/MP3-I/Edit Piaf/CD1/01.mp3").is_file());

    let (err, _) = tool(&m, &read, "rename_album", json!({ "id": id, "author": "Edith Piaf", "dry_run": false })).await;
    assert!(err, "a reading token may not rename");
    let (err, done) = tool(&m, &write, "rename_album", json!({ "id": id, "author": "Edith Piaf", "title": "Edith Piaf", "dry_run": false })).await;
    assert!(!err && done["renamed"] == true, "{done}");
    assert!(music.path().join("JÖRG/Edith Piaf/Edith Piaf/CD1/01.mp3").is_file());
    assert!(!music.path().join("JÖRG/MP3-I/Edit Piaf").exists());
    assert!(music.path().join("JÖRG/MP3-I/Other/01.mp3").is_file(), "the other album stays");

    // Only the place changed: author and title are what the tags say (the album tag; no artist tag, so the folder above).
    let row = m.store.admin_book_row(id).await.unwrap().unwrap();
    assert_eq!((row.kind.as_str(), row.author.as_str(), row.title.as_str()), ("music", "MP3-I", "Chansons"));
    assert_eq!(m.store.book_folder(id).await.unwrap().as_deref(), Some("JÖRG/Edith Piaf/Edith Piaf"));
    // The tags win over the folder at the next scan, and where the tags are silent the folder speaks.
    m.store.scan_kind(music.path(), "music").await.unwrap();
    let row = m.store.admin_book_row(id).await.unwrap().unwrap();
    assert_eq!((row.author.as_str(), row.title.as_str()), ("Edith Piaf", "Chansons"));
    assert!(m.store.admin_book_row(other).await.unwrap().is_some());

    // Nothing changes a second time, and a place that exists is refused.
    let (_, again) = tool(&m, &write, "rename_album", json!({ "id": id, "author": "Edith Piaf", "title": "Edith Piaf", "dry_run": false })).await;
    assert_eq!(again["plan"]["problem"], "nothing to change", "{again}");
    let (_, clash) = tool(&m, &write, "rename_album", json!({ "id": other, "author": "Edith Piaf", "title": "Edith Piaf" })).await;
    assert!(clash["plan"]["problem"].as_str().unwrap().contains("exists already"), "{clash}");
}

#[tokio::test]
async fn id3v2_is_binding_an_mp3_with_only_id3v1_is_flagged_and_upgraded_without_losing_its_tags() {
    if !ffmpeg_available() {
        return;
    }
    let m = mcp().await;
    let music = tempfile::TempDir::new().unwrap();
    m.store.save_music_roots(&[music.path().display().to_string()]).await.unwrap();
    let old = music.path().join("Band/Old/01.mp3");
    std::fs::create_dir_all(old.parent().unwrap()).unwrap();
    // Only the obsolete ID3v1 tag, no ID3v2.
    // ffmpeg always writes ID3v2 when it has metadata, so the file is made without tags and the ID3v1 block (128 bytes) is added by hand.
    common::audio::make(&old, &["-c:a", "libmp3lame", "-b:a", "64k", "-fflags", "+bitexact", "-flags:a", "+bitexact"], &[]);
    let mut v1 = b"TAG".to_vec();
    for (text, width) in [("Song", 30), ("Band", 30), ("Old", 30), ("2001", 4), ("", 30)] {
        let mut field = text.as_bytes().to_vec();
        field.resize(width, 0);
        v1.extend(field);
    }
    v1.push(17); // Rock, number 17 of the ID3 list
    let mut bytes = std::fs::read(&old).unwrap();
    if &bytes[..3] == b"ID3" {
        // ffmpeg adds a small ID3v2 tag of its own; cut it off (its size is four 7-bit bytes after the 6-byte header start).
        let size = ((bytes[6] as usize & 0x7f) << 21) | ((bytes[7] as usize & 0x7f) << 14) | ((bytes[8] as usize & 0x7f) << 7) | (bytes[9] as usize & 0x7f);
        bytes.drain(..10 + size);
    }
    bytes.extend(v1);
    std::fs::write(&old, bytes).unwrap();
    make_mp3(&music.path().join("Band/New/01.mp3"), &[("album", "New"), ("artist", "Band"), ("genre", "Rock")]);
    assert!(david_server::tags::lacks_id3v2(&old), "the test file has no ID3v2");
    m.store.scan_kind(music.path(), "music").await.unwrap();
    let write = m.store.create_mcp_token("w", true, m.admin).await.unwrap();
    let read = m.store.create_mcp_token("r", false, m.admin).await.unwrap();
    let id = m.store.find_book("Band/Old").await.unwrap().unwrap().id;

    // The report flags only the old file; the dry run lists it and changes nothing.
    let (_, found) = tool(&m, &read, "find_problems", json!({})).await;
    let flagged: Vec<_> = found["albums_with_problems"].as_array().unwrap().iter().filter(|a| a["problems"].to_string().contains("no_id3v2")).map(|a| a["title"].as_str().unwrap().to_owned()).collect();
    assert_eq!(flagged, ["Old"], "{found}");
    let (err, plan) = tool(&m, &read, "upgrade_to_id3v2", json!({})).await;
    assert!(!err && plan["albums"][0]["files"] == 1 && plan["files_changed"] == 0, "{plan}");
    assert!(david_server::tags::lacks_id3v2(&old));
    let (err, _) = tool(&m, &read, "upgrade_to_id3v2", json!({ "dry_run": false })).await;
    assert!(err, "a reading token may not write");

    // The upgrade makes the ID3v2 tag out of the ID3v1 one: the values stay.
    let (err, done) = tool(&m, &write, "upgrade_to_id3v2", json!({ "dry_run": false, "ids": [id] })).await;
    assert!(!err && done["files_changed"] == 1, "{done}");
    assert!(!david_server::tags::lacks_id3v2(&old));
    let fields = david_server::tags::read_fields(&old).unwrap();
    let value = |name: &str| fields.iter().find(|(n, _)| n == name).and_then(|(_, v)| v.first().cloned());
    assert_eq!((value("AlbumTitle").as_deref(), value("TrackArtist").as_deref(), value("Genre").as_deref()), (Some("Old"), Some("Band"), Some("Rock")), "{fields:?}");
    let (_, again) = tool(&m, &write, "upgrade_to_id3v2", json!({})).await;
    assert!(again["albums"].as_array().unwrap().is_empty(), "{again}");
}

#[tokio::test]
async fn a_change_through_the_mcp_names_the_token_in_the_audit_fields_of_the_row() {
    if !ffmpeg_available() {
        return;
    }
    let m = mcp().await;
    let b = book("Author", "Audit", &[1]);
    m.store.import_books(vec![b]).await.unwrap();
    let id = m.store.find_book("Author/Audit").await.unwrap().unwrap().id;
    // The import is the system's work.
    let made = m.store.audit_of("book", id).await.unwrap().unwrap();
    assert_eq!((made.created_by.as_deref(), made.updated_by.as_deref()), (Some("system"), Some("system")));
    let write = m.store.create_mcp_token("scribe", true, m.admin).await.unwrap();
    let (err, _) = tool(&m, &write, "set_description", json!({ "id": id, "text": "Ein Klappentext." })).await;
    assert!(!err);
    let changed = m.store.audit_of("book", id).await.unwrap().unwrap();
    assert_eq!((changed.created_by.as_deref(), changed.updated_by.as_deref()), (Some("system"), Some("mcp:scribe")));
    assert!(changed.updated_at >= made.updated_at);
    assert!(m.store.audit_of("no_such_table", 1).await.unwrap().is_none() && m.store.audit_of("book; DROP TABLE book", 1).await.unwrap().is_none());
}

#[tokio::test]
async fn radio_stations_are_listed_added_changed_given_to_profiles_by_name_and_deleted() {
    let m = mcp().await;
    let write = m.store.create_mcp_token("w", true, m.admin).await.unwrap();
    let read = m.store.create_mcp_token("r", false, m.admin).await.unwrap();
    let anna = m.store.create_user(david_server::NewProfile::new("Anna", david_server::Role::Member)).await.unwrap();
    let _ = anna;

    // Adding needs a token that may write; "*" is every adult (the administrator and Anna).
    let add = json!({ "action": "add", "name": "DWG Lyra", "stream_url": "https://server34599.streamplus.de/stream.mp3", "for": ["*"] });
    let (err, _) = tool(&m, &read, "edit_stations", add.clone()).await;
    assert!(err, "a reading token may not edit");
    let (err, done) = tool(&m, &write, "edit_stations", add).await;
    assert!(!err, "{done}");
    let (_, list) = tool(&m, &read, "list_stations", json!({})).await;
    assert_eq!(list["stations"][0]["name"], "DWG Lyra", "{list}");
    assert_eq!(list["stations"][0]["favourite_of"].as_array().unwrap().len(), 2, "{list}");

    // A station is found by its name (any case) or its address; a refused address changes nothing.
    let (err, done) = tool(&m, &write, "edit_stations", json!({ "action": "update", "station": "dwg lyra", "name": "DWG Lyra 128", "homepage": "https://radio.dwgradio.net/de/" })).await;
    assert!(!err, "{done}");
    let (err, _) = tool(&m, &write, "edit_stations", json!({ "action": "update", "station": "DWG Lyra 128", "stream_url": "ftp://nope" })).await;
    assert!(err);
    let (_, list) = tool(&m, &read, "list_stations", json!({})).await;
    assert_eq!((list["stations"][0]["name"].as_str(), list["stations"][0]["homepage"].as_str()), (Some("DWG Lyra 128"), Some("https://radio.dwgradio.net/de/")));

    // Profiles are named; an unknown name is refused; unassign takes it from one profile only.
    let (err, _) = tool(&m, &write, "edit_stations", json!({ "action": "unassign", "station": "DWG Lyra 128", "for": ["Nobody"] })).await;
    assert!(err);
    let (err, _) = tool(&m, &write, "edit_stations", json!({ "action": "unassign", "station": "DWG Lyra 128", "for": ["Anna"] })).await;
    assert!(!err);
    let (_, list) = tool(&m, &read, "list_stations", json!({})).await;
    assert_eq!(list["stations"][0]["favourite_of"].as_array().unwrap().len(), 1, "{list}");
    let (err, _) = tool(&m, &write, "edit_stations", json!({ "action": "assign", "station": "https://server34599.streamplus.de/stream.mp3", "for": ["Anna"] })).await;
    assert!(!err);

    // Deleting removes the station from every list.
    let (err, _) = tool(&m, &write, "edit_stations", json!({ "action": "delete", "station": "DWG Lyra 128" })).await;
    assert!(!err);
    let (_, list) = tool(&m, &read, "list_stations", json!({})).await;
    assert!(list["stations"].as_array().unwrap().is_empty(), "{list}");
}

/// Imports a synthetic album (no files needed for playlists) and returns its id.
async fn plain_album(m: &Mcp, author: &str, title: &str, per_disc: &[usize]) -> i64 {
    let b = book(author, title, per_disc);
    let folder = b.folder.clone();
    m.store.import_books(vec![b]).await.unwrap();
    m.store.find_book(&folder).await.unwrap().unwrap().id
}

#[tokio::test]
async fn playlists_are_made_filled_moved_and_deleted_by_names_and_the_token_is_the_actor() {
    let m = mcp().await;
    let anna = m.store.create_user(david_server::NewProfile::new("Anna", Role::Member)).await.unwrap();
    let read = m.store.create_mcp_token("reader", false, m.admin).await.unwrap();
    let write = m.store.create_mcp_token("llm", true, m.admin).await.unwrap();
    plain_album(&m, "Follett", "Winter", &[3]).await;
    let ronja = plain_album(&m, "Lindgren", "Ronja", &[2]).await;
    let all = m.store.tracks_of(ronja).await.unwrap();
    assert_eq!(all.len(), 2);
    for t in ["list_playlists", "get_playlist", "edit_playlists"] {
        let (err, help) = tool(&m, &read, "help_for", json!({ "tool": t })).await;
        assert!(!err && help["example"].is_object(), "{t}: {help}");
    }
    let (_, listing) = tool(&m, &read, "list", json!({})).await;
    let writes = |name: &str| listing["tools"].as_array().unwrap().iter().find(|t| t["name"] == name).unwrap()["changes_files"].as_bool().unwrap();
    assert!(writes("edit_playlists") && !writes("list_playlists") && !writes("get_playlist"));

    let create = json!({ "action": "create", "profile": "anna", "name": "Car trip" });
    let (err, text) = tool(&m, &read, "edit_playlists", create.clone()).await;
    assert!(err && text.as_str().unwrap().contains("only read"), "{text}");
    let (err, made) = tool(&m, &write, "edit_playlists", create.clone()).await;
    assert!(!err, "{made}");
    assert_eq!((made["profile"].as_str(), made["name"].as_str(), made["tracks"].as_i64()), (Some("Anna"), Some("Car trip"), Some(0)));
    let (err, text) = tool(&m, &write, "edit_playlists", create).await;
    assert!(err && text.as_str().unwrap().contains("already"), "{text}");
    let (err, text) = tool(&m, &write, "edit_playlists", json!({ "action": "create", "profile": "Nobody", "name": "x" })).await;
    assert!(err && text.as_str().unwrap().contains("no profile called Nobody"), "{text}");

    // The actor of the changes is the token, in the rows made by it.
    let id = m.store.playlist_summaries(Some(anna)).await.unwrap()[0].id;
    let audit = m.store.audit_of("playlist", id).await.unwrap().unwrap();
    assert_eq!((audit.created_by.as_deref(), audit.updated_by.as_deref()), (Some("mcp:llm"), Some("mcp:llm")));

    // A book the profile may not see is refused; once released it can be added whole, in album order.
    let add_album = json!({ "action": "add", "profile": "Anna", "name": "car trip", "album": "winter" });
    let (err, text) = tool(&m, &write, "edit_playlists", add_album.clone()).await;
    assert!(err && text.as_str().unwrap().contains("may not see"), "{text}");
    let winter = m.store.find_book("Follett/Winter").await.unwrap().unwrap().id;
    m.store.release_book(winter, 0).await.unwrap();
    m.store.release_book(ronja, 0).await.unwrap();
    let (err, got) = tool(&m, &write, "edit_playlists", add_album).await;
    assert!(!err, "{got}");
    assert_eq!(got["tracks"], 3);
    assert_eq!(got["entries"][0]["album"], "Winter");
    assert_eq!(got["entries"][0]["author"], "Follett");
    assert_eq!(got["entries"][2]["track"], "Winter_CD01_TR03");
    assert!(!got.to_string().contains("\"id\""), "names, not numbers: {got}");

    // One track by title, one by number, at a position.
    let (_, got) = tool(&m, &write, "edit_playlists", json!({ "action": "add", "profile": "Anna", "name": "Car trip", "album": "Ronja", "track": "ronja_cd01_tr02", "at": 1 })).await;
    assert_eq!(got["entries"][0]["track"], "Ronja_CD01_TR02", "{got}");
    let (_, got) = tool(&m, &write, "edit_playlists", json!({ "action": "add", "profile": "Anna", "name": "Car trip", "album": "Ronja", "track_number": 1 })).await;
    assert_eq!((got["entries"][4]["track"].as_str(), got["tracks"].as_i64()), (Some("Ronja_CD01_TR01"), Some(5)), "{got}");
    let (err, text) = tool(&m, &write, "edit_playlists", json!({ "action": "add", "profile": "Anna", "name": "Car trip", "album": "Ronja", "track": "nope" })).await;
    assert!(err && text.as_str().unwrap().contains("no such track"), "{text}");

    // Move by position and by title, then remove.
    let (_, got) = tool(&m, &write, "edit_playlists", json!({ "action": "move", "profile": "Anna", "name": "Car trip", "position": 5, "to": 1 })).await;
    assert_eq!(got["entries"][0]["track"], "Ronja_CD01_TR01", "{got}");
    assert_eq!(got["entries"][1]["track"], "Ronja_CD01_TR02");
    let (_, got) = tool(&m, &write, "edit_playlists", json!({ "action": "move", "profile": "Anna", "name": "Car trip", "track": "winter_cd01_tr01", "to": 5 })).await;
    assert_eq!(got["entries"][4]["track"], "Winter_CD01_TR01", "{got}");
    let (err, _) = tool(&m, &write, "edit_playlists", json!({ "action": "move", "profile": "Anna", "name": "Car trip", "position": 1, "to": 6 })).await;
    assert!(err);
    let (_, got) = tool(&m, &write, "edit_playlists", json!({ "action": "remove", "profile": "Anna", "name": "Car trip", "track": "Ronja_CD01_TR02" })).await;
    assert_eq!((got["tracks"].as_i64(), got["entries"][0]["track"].as_str()), (Some(4), Some("Ronja_CD01_TR01")), "{got}");
    let (_, got) = tool(&m, &write, "edit_playlists", json!({ "action": "remove", "profile": "Anna", "name": "Car trip", "position": 1 })).await;
    assert_eq!(got["tracks"], 3);
    assert_eq!(m.store.audit_of("playlist", id).await.unwrap().unwrap().updated_by.as_deref(), Some("mcp:llm"));

    // Reading: by profile, by name.
    let (_, one) = tool(&m, &read, "get_playlist", json!({ "profile": "Anna", "name": "CAR TRIP" })).await;
    assert_eq!((one["tracks"].as_i64(), one["duration"].as_str()), (Some(3), Some("0:12:00")), "{one}");
    let (_, lists) = tool(&m, &read, "list_playlists", json!({})).await;
    assert_eq!(lists["playlists"][0], json!({ "profile": "Anna", "name": "Car trip", "tracks": 3, "duration": "0:12:00", "seconds": 720 }));
    let (_, none) = tool(&m, &read, "list_playlists", json!({ "profile": "Boss" })).await;
    assert_eq!(none["playlists"], json!([]));
    let (err, _) = tool(&m, &read, "get_playlist", json!({ "profile": "Boss", "name": "Car trip" })).await;
    assert!(err, "a list of Anna is not found under Boss");

    let (_, renamed) = tool(&m, &write, "edit_playlists", json!({ "action": "rename", "profile": "Anna", "name": "Car trip", "new_name": "Holiday" })).await;
    assert_eq!(renamed["name"], "Holiday");
    let (_, cleared) = tool(&m, &write, "edit_playlists", json!({ "action": "clear", "profile": "Anna", "name": "Holiday" })).await;
    assert_eq!(cleared["entries"], json!([]));
    let (err, gone) = tool(&m, &write, "edit_playlists", json!({ "action": "delete", "profile": "Anna", "name": "Holiday" })).await;
    assert!(!err && gone["deleted"] == "Holiday", "{gone}");
    assert!(m.store.playlist_summaries(None).await.unwrap().is_empty());
    // The changes are in the change log.
    let (_, log) = tool(&m, &write, "change_log", json!({})).await;
    assert!(log.to_string().contains("Playlists"), "{log}");
}

#[tokio::test]
async fn an_ambiguous_album_or_track_is_refused_with_the_candidates() {
    let m = mcp().await;
    let write = m.store.create_mcp_token("llm", true, m.admin).await.unwrap();
    plain_album(&m, "Follett", "Winter", &[2]).await;
    plain_album(&m, "Grimm", "Winter", &[1]).await;
    let (err, _) = tool(&m, &write, "edit_playlists", json!({ "action": "create", "profile": "Boss", "name": "L" })).await;
    assert!(!err);
    let add = json!({ "action": "add", "profile": "Boss", "name": "L", "album": "Winter" });
    let (err, text) = tool(&m, &write, "edit_playlists", add.clone()).await;
    let text = text.as_str().unwrap().to_owned();
    assert!(err && text.contains("several albums") && text.contains("Winter (Follett)") && text.contains("Winter (Grimm)"), "{text}");
    assert_eq!(m.store.playlist_summaries(None).await.unwrap()[0].items, 0);
    // The author tells them apart.
    let mut by_author = add.clone();
    by_author["author"] = json!("grimm");
    let (err, got) = tool(&m, &write, "edit_playlists", by_author).await;
    assert!(!err && got["entries"][0]["author"] == "Grimm", "{got}");
    // Two tracks of the same number on two discs: say which.
    plain_album(&m, "Lindgren", "Ronja", &[1, 1]).await;
    let (err, text) = tool(&m, &write, "edit_playlists", json!({ "action": "add", "profile": "Boss", "name": "L", "album": "Ronja", "track_number": 1 })).await;
    assert!(err && text.as_str().unwrap().contains("several tracks") && text.as_str().unwrap().contains("disc 2"), "{text}");
    // A near miss is not guessed but offered.
    let (err, text) = tool(&m, &write, "edit_playlists", json!({ "action": "add", "profile": "Boss", "name": "L", "album": "Wint" })).await;
    assert!(err && text.as_str().unwrap().contains("similar"), "{text}");
    // Two entries of the same title: the position is needed.
    let (_, _) = tool(&m, &write, "edit_playlists", json!({ "action": "add", "profile": "Boss", "name": "L", "album": "Ronja", "track_number": 1, "disc": 1 })).await;
    let (_, _) = tool(&m, &write, "edit_playlists", json!({ "action": "add", "profile": "Boss", "name": "L", "album": "Ronja", "track_number": 1, "disc": 1 })).await;
    let (err, text) = tool(&m, &write, "edit_playlists", json!({ "action": "remove", "profile": "Boss", "name": "L", "track": "Ronja_CD01_TR01" })).await;
    assert!(err && text.as_str().unwrap().contains("use the position"), "{text}");
}

#[tokio::test]
async fn podcasts_are_listed_by_profile_unfollowed_by_name_and_a_follow_needs_a_real_feed() {
    let m = mcp().await;
    let write = m.store.create_mcp_token("w", true, m.admin).await.unwrap();
    let read = m.store.create_mcp_token("r", false, m.admin).await.unwrap();
    let anna = m.store.create_user(david_server::NewProfile::new("Anna", david_server::Role::Member)).await.unwrap();
    let boss = m.admin;
    // Two feeds that are followed (what following over the network leaves behind), one of them by both.
    let feed = |title: &str| david_server::podcast::Feed { title: title.into(), site: None, image: None, description: String::new(), episodes: Vec::new() };
    let teacher = m.store.save_feed("https://example.org/teacher.xml", &feed("Teacher Unscripted")).await.unwrap();
    let mom = m.store.save_feed("https://example.org/mom.xml", &feed("Mom and Dad")).await.unwrap();
    m.store.subscribe(anna, teacher, true).await.unwrap();
    m.store.subscribe(boss, teacher, true).await.unwrap();
    m.store.subscribe(anna, mom, false).await.unwrap(); // waits for a release

    let (err, list) = tool(&m, &read, "list_podcasts", json!({})).await;
    assert!(!err, "{list}");
    assert_eq!(list["podcasts"].as_array().unwrap().len(), 2, "{list}");
    let teacher_row = list["podcasts"].as_array().unwrap().iter().find(|p| p["title"] == "Teacher Unscripted").unwrap();
    assert_eq!(teacher_row["followed_by"].as_array().unwrap().len(), 2, "{list}");
    assert!(list["podcasts"].to_string().contains("Anna (waiting)"), "a request that waits is marked: {list}");
    let (_, only_boss) = tool(&m, &read, "list_podcasts", json!({ "profile": m.store.profile_by_id(boss).await.unwrap().unwrap().name })).await;
    assert_eq!(only_boss["podcasts"].as_array().unwrap().len(), 1, "{only_boss}");
    let (err, _) = tool(&m, &read, "list_podcasts", json!({ "profile": "Nobody" })).await;
    assert!(err, "an unknown profile is refused");

    // Changing needs a token that may write; the podcast is found by its title (any case) or its feed address; profiles are named.
    let stop = json!({ "action": "unfollow", "podcast": "teacher unscripted", "for": ["Anna"] });
    let (err, _) = tool(&m, &read, "edit_podcasts", stop.clone()).await;
    assert!(err, "a reading token may not edit");
    let (err, done) = tool(&m, &write, "edit_podcasts", stop).await;
    assert!(!err, "{done}");
    let (_, list) = tool(&m, &read, "list_podcasts", json!({})).await;
    let teacher_row = list["podcasts"].as_array().unwrap().iter().find(|p| p["title"] == "Teacher Unscripted").unwrap();
    assert_eq!(teacher_row["followed_by"].as_array().unwrap().len(), 1, "only Anna stopped: {list}");
    let (err, _) = tool(&m, &write, "edit_podcasts", json!({ "action": "unfollow", "podcast": "No such show", "for": ["Anna"] })).await;
    assert!(err);
    let (err, _) = tool(&m, &write, "edit_podcasts", json!({ "action": "unfollow", "podcast": "Mom and Dad" })).await;
    assert!(err, "for: is required");
    // "*": everybody who follows it, also a request that waits.
    let (err, done) = tool(&m, &write, "edit_podcasts", json!({ "action": "unfollow", "podcast": "https://example.org/mom.xml", "for": ["*"] })).await;
    assert!(!err, "{done}");
    assert_eq!(done["for"], json!(["Anna"]), "{done}");

    // Following needs a feed that can be read: an address that is not one, or that the server may not fetch, changes nothing.
    for url in ["not an address", "http://127.0.0.1:1/feed.xml", "https://localhost/feed.xml"] {
        let (err, done) = tool(&m, &write, "edit_podcasts", json!({ "action": "follow", "url": url, "for": ["Anna"] })).await;
        assert!(err, "{url}: {done}");
    }
    let (err, _) = tool(&m, &write, "edit_podcasts", json!({ "action": "follow", "url": "https://example.org/x.xml" })).await;
    assert!(err, "for: is required");
    let (_, list) = tool(&m, &read, "list_podcasts", json!({})).await;
    assert_eq!(list["podcasts"].as_array().unwrap().len(), 2, "nothing was added: {list}");
    // The change is in the log of the changes, under the name of the token.
    assert!(m.store.tag_changes(0, 20).await.unwrap().iter().any(|c| c.token_name == "w" && c.field == "Podcasts"), "logged");
}

#[tokio::test]
async fn albums_are_released_by_their_recommended_age_and_those_without_one_are_listed() {
    let m = mcp().await;
    for (author, title) in [("Follett", "Winter der Welt"), ("Tolkien", "Der Hobbit"), ("Lindgren", "Pippi")] {
        m.store.import_books(vec![book(author, title, &[1])]).await.unwrap();
    }
    let write = m.store.create_mcp_token("w", true, m.admin).await.unwrap();
    let read = m.store.create_mcp_token("r", false, m.admin).await.unwrap();
    let all = m.store.admin_book_rows().await.unwrap();
    let id = |t: &str| all.iter().find(|r| r.title == t).unwrap().id;
    let (winter, hobbit, pippi) = (id("Winter der Welt"), id("Der Hobbit"), id("Pippi"));
    tool(&m, &write, "set_recommended_age", json!({ "id": winter, "age": 14 })).await;
    tool(&m, &write, "set_recommended_age", json!({ "id": pippi, "age": 6 })).await;
    // A reading token may not.
    assert!(tool(&m, &read, "release_albums", json!({ "dry_run": false })).await.0);
    // By default it only reports.
    let (err, out) = tool(&m, &write, "release_albums", json!({})).await;
    assert!(!err, "{out}");
    assert_eq!(out["dry_run"], true);
    assert_eq!(out["released"].as_array().unwrap().len(), 2, "{out}");
    assert_eq!((out["no_age"].as_array().unwrap().len(), out["no_age"][0]["id"].as_i64()), (1, Some(hobbit)), "{out}");
    assert!(m.store.admin_book_rows().await.unwrap().iter().all(|r| !r.released), "nothing changed");
    // Really: each from its own age, the one without an age untouched, and the change log knows.
    let (_, out) = tool(&m, &write, "release_albums", json!({ "dry_run": false })).await;
    assert_eq!(out["dry_run"], false);
    let rows = m.store.admin_book_rows().await.unwrap();
    let by = |i: i64| rows.iter().find(|r| r.id == i).unwrap().clone();
    assert_eq!((by(winter).released, by(winter).min_age), (true, Some(14)));
    assert_eq!((by(pippi).released, by(pippi).min_age), (true, Some(6)));
    assert!(!by(hobbit).released, "no age: not released");
    // A second run finds nothing to do but the one without an age.
    let (_, again) = tool(&m, &write, "release_albums", json!({ "dry_run": false })).await;
    assert_eq!((again["released"].as_array().unwrap().len(), again["already_released"].as_u64()), (0, Some(2)), "{again}");
}

#[tokio::test]
async fn the_owner_is_set_by_nickname_and_taken_from_the_folders() {
    let m = mcp().await;
    let anna = m.store.create_user(NewProfile::new("Anni", Role::Member)).await.unwrap();
    m.store.set_person_names(anna, "Anni", "Anna", "Beispiel").await.unwrap();
    for (author, title, folder) in [("Band", "Album", "Anna Beispiel/Band/Album"), ("Band", "Zwei", "Beispiel, Anna/Band/Zwei"), ("Band", "Drei", "Sammlung/Band/Drei")] {
        let mut b = book(author, title, &[1]);
        b.folder = folder.to_owned();
        b.kind = "music".to_owned();
        m.store.import_books(vec![b]).await.unwrap();
    }
    let write = m.store.create_mcp_token("w", true, m.admin).await.unwrap();
    let read = m.store.create_mcp_token("r", false, m.admin).await.unwrap();
    let all = m.store.admin_book_rows().await.unwrap();
    let id = |t: &str| all.iter().find(|r| r.title == t).unwrap().id;
    // A reading token may not; the call without dry_run=false only reports.
    assert!(tool(&m, &read, "assign_owners", json!({ "dry_run": false })).await.0);
    let (err, out) = tool(&m, &write, "assign_owners", json!({ "kind": "music" })).await;
    assert!(!err, "{out}");
    assert_eq!((out["dry_run"].as_bool(), out["assigned"].as_array().unwrap().len(), out["without_match"].as_u64()), (Some(true), 2, Some(1)), "{out}");
    assert!(m.store.admin_book_rows().await.unwrap().iter().all(|r| r.owner.is_none()), "nothing changed");
    // Really.
    let (_, out) = tool(&m, &write, "assign_owners", json!({ "kind": "music", "dry_run": false })).await;
    assert_eq!(out["assigned"][0]["owner"], "Anni");
    let rows = m.store.admin_book_rows().await.unwrap();
    let owner = |t: &str| rows.iter().find(|r| r.title == t).unwrap().owner.clone();
    assert_eq!((owner("Album").as_deref(), owner("Zwei").as_deref(), owner("Drei")), (Some("Anni"), Some("Anni"), None));
    // By hand: another owner, none again, wrong nicknames refused, and the list and the album tell it.
    let (err, out) = tool(&m, &write, "set_owner", json!({ "id": id("Drei"), "owner": "anni" })).await;
    assert!(!err && out["owner"] == "Anni", "{out}");
    let (_, listed) = tool(&m, &write, "list_albums", json!({ "query": "Drei" })).await;
    assert_eq!(listed["albums"][0]["owner"], "Anni");
    assert!(tool(&m, &write, "set_owner", json!({ "id": id("Drei"), "owner": "Nobody" })).await.0);
    assert!(tool(&m, &read, "set_owner", json!({ "id": id("Drei"), "owner": null })).await.0);
    let (err, out) = tool(&m, &write, "set_owner", json!({ "id": id("Drei"), "owner": null })).await;
    assert!(!err && out["owner"].is_null() && out["was"] == "Anni", "{out}");
    let (_, listed) = tool(&m, &write, "list_albums", json!({ "query": "Album" })).await;
    assert_eq!(listed["albums"][0]["owner"], "Anni");
    let (_, log) = tool(&m, &write, "change_log", json!({ "id": id("Drei") })).await;
    assert!(log.to_string().contains("Owner"), "{log}");
}

#[tokio::test]
async fn the_album_query_knows_wildcards_and_regular_expressions() {
    let m = mcp().await;
    for (author, title) in [("Follett", "Winter der Welt"), ("Tolkien", "Der Hobbit"), ("Bach", "Matthäus-Passion")] {
        m.store.import_books(vec![book(author, title, &[1])]).await.unwrap();
    }
    let write = m.store.create_mcp_token("w", true, m.admin).await.unwrap();
    let titles = |v: &Value| -> Vec<String> { v["albums"].as_array().unwrap().iter().map(|a| a["title"].as_str().unwrap().to_owned()).collect() };
    for (query, want) in [("winter*welt", vec!["Winter der Welt"]), ("*passion", vec!["Matthäus-Passion"]), ("/^der /", vec!["Der Hobbit"]), ("/(tolkien|bach)/", vec!["Matthäus-Passion", "Der Hobbit"]), ("hobbi?", vec!["Der Hobbit"])] {
        let (err, out) = tool(&m, &write, "list_albums", json!({ "query": query })).await;
        assert!(!err, "{query}: {out}");
        let mut got = titles(&out);
        got.sort();
        let mut want: Vec<String> = want.into_iter().map(str::to_owned).collect();
        want.sort();
        assert_eq!(got, want, "{query}");
    }
    let (err, out) = tool(&m, &write, "list_albums", json!({ "query": "/(open/" })).await;
    assert!(err && out.to_string().contains("query"), "{out}");
}

#[tokio::test]
async fn labels_of_single_titles_are_set_by_a_token_that_may_write() {
    let m = mcp().await;
    m.store.import_books(vec![common::book("A", "Eins", &[3])]).await.unwrap();
    let one = m.store.find_book("A/Eins").await.unwrap().unwrap().id;
    let tracks: Vec<i64> = m.store.tracks_of(one).await.unwrap().iter().map(|t| t.id).collect();
    let write = m.store.create_mcp_token("w", true, m.admin).await.unwrap();
    let read = m.store.create_mcp_token("r", false, m.admin).await.unwrap();

    // A look changes nothing; a reading token may not write; a bad name, an empty request and a track of another album are refused or told.
    let (err, look) = tool(&m, &read, "set_track_labels", json!({ "id": one, "tracks": [tracks[0]], "add": ["Gebet"] })).await;
    assert!(!err && look["dry_run"] == true && look["changed"] == 1 && m.store.labels().await.unwrap().is_empty(), "{look}");
    assert!(tool(&m, &read, "set_track_labels", json!({ "id": one, "add": ["Gebet"], "dry_run": false })).await.0);
    assert!(tool(&m, &write, "set_track_labels", json!({ "id": one, "add": ["a,b"], "dry_run": false })).await.0);
    assert!(tool(&m, &write, "set_track_labels", json!({ "id": one })).await.0);
    let (_, odd) = tool(&m, &write, "set_track_labels", json!({ "id": one, "tracks": [99999], "add": ["x"] })).await;
    assert_eq!(odd["tracks"][0]["problem"], "no such track in this album", "{odd}");

    // Without `tracks` all titles; the labels show in get_album and in the list; case is ignored; remove and set work.
    let (err, done) = tool(&m, &write, "set_track_labels", json!({ "id": one, "add": ["Gebet"], "dry_run": false })).await;
    assert!(!err && done["changed"] == 3, "{done}");
    let (_, again) = tool(&m, &write, "set_track_labels", json!({ "id": one, "add": ["gebet"], "dry_run": false })).await;
    assert_eq!(again["changed"], 0, "{again}");
    let (_, removed) = tool(&m, &write, "set_track_labels", json!({ "id": one, "tracks": [tracks[2]], "remove": ["GEBET"], "dry_run": false })).await;
    assert_eq!(removed["tracks"][0]["after"], json!([]), "{removed}");
    let own = m.store.track_labels().await.unwrap();
    assert_eq!((own.get(&tracks[0]).cloned(), own.get(&tracks[2]).cloned()), (Some(vec!["Gebet".to_owned()]), None));
    let (_, list) = tool(&m, &read, "list_labels", json!({})).await;
    assert_eq!(list["labels"], json!([{ "name": "Gebet", "books": 0, "tracks": 2, "podcasts": 0, "stations": 0 }]), "{list}");
    let (_, set) = tool(&m, &write, "set_track_labels", json!({ "id": one, "set": [], "dry_run": false })).await;
    assert_eq!(set["changed"], 2, "{set}");
    assert!(m.store.labels().await.unwrap().is_empty());
}

#[tokio::test]
async fn podcasts_and_stations_are_labelled_by_name_or_address_with_a_look_first() {
    let m = mcp().await;
    let write = m.store.create_mcp_token("w", true, m.admin).await.unwrap();
    let read = m.store.create_mcp_token("r", false, m.admin).await.unwrap();
    let (err, _) = tool(&m, &write, "edit_stations", json!({ "action": "add", "name": "DWG Lyra", "stream_url": "https://server34599.streamplus.de/stream.mp3", "for": ["*"] })).await;
    assert!(!err);
    let feed = david_server::podcast::parse(r#"<?xml version="1.0"?><rss version="2.0"><channel><title>Kinderfunk</title><link>https://example.org/</link><description>x</description>
      <item><title>Folge 1</title><guid>a</guid><enclosure url="https://example.org/1.mp3" type="audio/mpeg"/><pubDate>Tue, 13 Oct 2026 10:00:00 GMT</pubDate></item></channel></rss>"#).unwrap();
    let feed_id = m.store.save_feed("https://example.org/feed.xml", &feed).await.unwrap();
    m.store.subscribe(m.admin, feed_id, true).await.unwrap();

    // A look changes nothing; a reading token may not write; a bad kind, a bad name and an empty request are refused.
    let (err, look) = tool(&m, &read, "set_feed_labels", json!({ "kind": "station", "names": ["dwg lyra"], "add": ["Gebet"] })).await;
    assert!(!err && look["dry_run"] == true && look["changed"] == 1 && m.store.labels().await.unwrap().is_empty(), "{look}");
    assert!(tool(&m, &read, "set_feed_labels", json!({ "kind": "station", "names": ["DWG Lyra"], "add": ["Gebet"], "dry_run": false })).await.0);
    assert!(tool(&m, &write, "set_feed_labels", json!({ "kind": "radio", "names": ["x"], "add": ["Gebet"] })).await.0);
    assert!(tool(&m, &write, "set_feed_labels", json!({ "kind": "station", "names": ["DWG Lyra"], "add": ["a,b"], "dry_run": false })).await.0);
    assert!(tool(&m, &write, "set_feed_labels", json!({ "kind": "station", "names": ["DWG Lyra"] })).await.0);
    let (_, odd) = tool(&m, &write, "set_feed_labels", json!({ "kind": "podcast", "names": ["Nope"], "add": ["x"] })).await;
    assert_eq!(odd["items"][0]["problem"], "no podcast called Nope", "{odd}");

    // By name (any case) or by address; the labels show in the lists and in the counts of list_labels.
    let (err, done) = tool(&m, &write, "set_feed_labels", json!({ "kind": "station", "names": ["dwg lyra"], "add": ["Gebet"], "dry_run": false })).await;
    assert!(!err && done["changed"] == 1, "{done}");
    let (err, done) = tool(&m, &write, "set_feed_labels", json!({ "kind": "podcast", "names": ["https://example.org/feed.xml"], "add": ["gebet", "Kinder"], "dry_run": false })).await;
    assert!(!err && done["changed"] == 1, "{done}");
    let (_, stations) = tool(&m, &read, "list_stations", json!({})).await;
    let (_, podcasts) = tool(&m, &read, "list_podcasts", json!({})).await;
    assert_eq!((stations["stations"][0]["labels"].clone(), podcasts["podcasts"][0]["labels"].clone()), (json!(["Gebet"]), json!(["Gebet", "Kinder"])));
    let (_, list) = tool(&m, &read, "list_labels", json!({})).await;
    assert_eq!(list["labels"], json!([{ "name": "Gebet", "books": 0, "tracks": 0, "podcasts": 1, "stations": 1 }, { "name": "Kinder", "books": 0, "tracks": 0, "podcasts": 1, "stations": 0 }]), "{list}");
    let (_, again) = tool(&m, &write, "set_feed_labels", json!({ "kind": "podcast", "names": ["Kinderfunk"], "add": ["GEBET"], "dry_run": false })).await;
    assert_eq!(again["changed"], 0, "{again}");

    // Remove and set; a label nothing carries any more goes.
    let (_, set) = tool(&m, &write, "set_feed_labels", json!({ "kind": "podcast", "names": ["Kinderfunk"], "set": [], "dry_run": false })).await;
    assert_eq!(set["changed"], 1, "{set}");
    let (_, set) = tool(&m, &write, "set_feed_labels", json!({ "kind": "station", "names": ["DWG Lyra"], "remove": ["gebet"], "dry_run": false })).await;
    assert_eq!(set["items"][0]["after"], json!([]), "{set}");
    assert!(m.store.labels().await.unwrap().is_empty());
}

#[tokio::test]
async fn the_blacklist_is_kept_with_a_look_first_marks_albums_and_the_mark_is_taken_off_with_set_labels() {
    let m = mcp().await;
    let write = m.store.create_mcp_token("w", true, m.admin).await.unwrap();
    let read = m.store.create_mcp_token("r", false, m.admin).await.unwrap();
    let one = album(&m, &[]).await;

    // Nothing comes with the program.
    let (err, list) = tool(&m, &read, "edit_blacklist", json!({})).await;
    assert!(!err && list["words"] == json!([]) && list["marked"] == json!([]), "{list}");

    // A look (the default) shows what would be marked and changes nothing; a reading token may not write; broken lines are refused.
    let (err, look) = tool(&m, &write, "edit_blacklist", json!({ "action": "add", "words": ["WINTER"] })).await;
    assert!(!err && look["dry_run"] == true && look["would_mark"] == 1 && look["media"][0]["id"] == one, "{look}");
    assert!(m.store.blacklist_words().await.unwrap().is_empty());
    assert!(tool(&m, &read, "edit_blacklist", json!({ "action": "add", "words": ["x"], "dry_run": false })).await.0);
    assert!(tool(&m, &write, "edit_blacklist", json!({ "action": "add", "words": ["/(/"], "dry_run": false })).await.0);
    assert!(tool(&m, &write, "edit_blacklist", json!({ "action": "add" })).await.0);
    assert!(tool(&m, &write, "edit_blacklist", json!({ "action": "remove", "words": ["nope"], "dry_run": false })).await.0);

    // Saved: the albums are marked; the list names the album and the word that hit; list_labels counts the system mark.
    let (err, done) = tool(&m, &write, "edit_blacklist", json!({ "action": "add", "words": ["WINTER", "winter"], "dry_run": false })).await;
    assert!(!err && done["words"] == json!(["WINTER"]) && done["marked"] == 1, "{done}");
    let (_, list) = tool(&m, &read, "edit_blacklist", json!({ "action": "list" })).await;
    assert_eq!((list["marked"][0]["kind"].clone(), list["marked"][0]["id"].clone(), list["marked"][0]["word"].clone(), list["marked"][0]["approved"].clone()), (json!("book"), json!(one), json!("WINTER"), json!(false)), "{list}");
    let (_, labels) = tool(&m, &read, "list_labels", json!({})).await;
    assert_eq!(labels["labels"][0]["name"], "Blacklist", "{labels}");
    let (_, of) = tool(&m, &read, "list_labels", json!({ "ids": [one] })).await;
    assert_eq!(of["albums"][0]["labels"], json!(["Blacklist"]), "{of}");

    // The mark is not set by hand; taking it off approves the album, and the next check does not put it on again.
    assert!(tool(&m, &write, "set_labels", json!({ "ids": [one], "add": ["blacklist"], "dry_run": false })).await.0);
    assert!(tool(&m, &write, "set_labels", json!({ "ids": [one], "set": ["Blacklist"], "dry_run": false })).await.0);
    let (err, off) = tool(&m, &write, "set_labels", json!({ "ids": [one], "remove": ["Blacklist"], "dry_run": false })).await;
    assert!(!err && off["albums"][0]["mark_approved"] == true, "{off}");
    assert!(m.store.blacklist_marked(david_server::blacklist::Kind::Book).await.unwrap().is_empty());
    m.store.blacklist_check(david_server::blacklist::Scope::All).await.unwrap();
    let (_, list) = tool(&m, &read, "edit_blacklist", json!({})).await;
    assert_eq!(list["marked"][0]["approved"], true, "{list}");

    // Remove the word: nothing is marked and nothing is approved any more.
    let (err, gone) = tool(&m, &write, "edit_blacklist", json!({ "action": "remove", "words": ["winter"], "dry_run": false })).await;
    assert!(!err && gone["words"] == json!([]) && gone["marked"] == 0, "{gone}");
    assert!(m.store.blacklist_rows().await.unwrap().is_empty());
}
