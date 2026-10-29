// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Playlists of the apps (schema 0035, `docs/spec-playlists.md`): one profile's lists, order, hidden tracks, deleted books.

mod common;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use common::{book, open};
use david_server::api::{ApiState, router};
use david_server::{NewProfile, Role, Schedule, Store};
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
    headers: HeaderMap,
    json: Value,
}

async fn api() -> Api {
    let (dir, store) = open().await;
    let admin = store.create_profile("Boss", true).await.unwrap();
    let state = ApiState { store: store.clone(), library: None, cd: david_server::cdimport::CdService::new(dir.path()) };
    Api { router: router(state), store, admin, _dir: dir }
}

async fn call(a: &Api, method: &str, uri: &str, token: &str, body: Option<Value>) -> Reply {
    call_with(a, method, uri, token, body, None).await
}

async fn call_with(a: &Api, method: &str, uri: &str, token: &str, body: Option<Value>, if_none_match: Option<&str>) -> Reply {
    let mut b = Request::builder().method(method).uri(uri).header(header::AUTHORIZATION, format!("Bearer {token}"));
    if let Some(t) = if_none_match {
        b = b.header(header::IF_NONE_MATCH, t);
    }
    let req = match body {
        Some(v) => b.header(header::CONTENT_TYPE, "application/json").body(Body::from(v.to_string())).unwrap(),
        None => b.body(Body::empty()).unwrap(),
    };
    let (parts, body) = a.router.clone().oneshot(req).await.unwrap().into_parts();
    let bytes = to_bytes(body, 8 * 1024 * 1024).await.unwrap();
    Reply { status: parts.status, headers: parts.headers, json: serde_json::from_slice(&bytes).unwrap_or(Value::Null) }
}

async fn pair(a: &Api, user: i64, n: u8) -> String {
    let e = a.store.create_enrollment(user, false, a.admin).await.unwrap();
    let body = json!({ "code": e.code, "device_name": "Phone", "public_key": key(n) });
    let req = Request::builder().method("POST").uri("/api/v1/pair").header(header::CONTENT_TYPE, "application/json").body(Body::from(body.to_string())).unwrap();
    let res = a.router.clone().oneshot(req).await.unwrap();
    let bytes = to_bytes(res.into_body(), 1024 * 1024).await.unwrap();
    serde_json::from_slice::<Value>(&bytes).unwrap()["token"].as_str().unwrap().to_owned()
}

/// Imports a book of `per_disc` tracks, released for everybody from age 0. Returns (book id, track ids in album order).
async fn add_book(a: &Api, author: &str, title: &str, per_disc: &[usize]) -> (i64, Vec<i64>) {
    let b = book(author, title, per_disc);
    let folder = b.folder.clone();
    a.store.import_books(vec![b]).await.unwrap();
    let id = a.store.find_book(&folder).await.unwrap().unwrap().id;
    a.store.release_book(id, 0).await.unwrap();
    let tracks = a.store.tracks_of(id).await.unwrap().iter().map(|t| t.id).collect();
    (id, tracks)
}

fn ids(v: &Value) -> Vec<i64> {
    v["items"].as_array().unwrap().iter().map(|i| i["item_id"].as_i64().unwrap()).collect()
}

fn titles(v: &Value) -> Vec<String> {
    v["items"].as_array().unwrap().iter().map(|i| i["part"]["title"].as_str().unwrap_or("-").to_owned()).collect()
}

#[tokio::test]
async fn a_profile_sees_only_its_own_playlists_and_a_foreign_one_is_not_found() {
    let a = api().await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let ben = a.store.create_user(NewProfile::new("Ben", Role::Member)).await.unwrap();
    let (ta, tb) = (pair(&a, anna, 1).await, pair(&a, ben, 2).await);
    let made = call(&a, "POST", "/api/v1/playlists", &ta, Some(json!({ "name": "  Car trip " }))).await;
    assert_eq!(made.status, StatusCode::CREATED);
    assert_eq!(made.json["name"], "Car trip");
    let id = made.json["id"].as_i64().unwrap();
    let uri = format!("/api/v1/playlists/{id}");

    assert_eq!(call(&a, "GET", "/api/v1/playlists", &tb, None).await.json, json!([]));
    for (method, path, body) in [
        ("GET", uri.clone(), None),
        ("PATCH", uri.clone(), Some(json!({ "name": "Mine" }))),
        ("DELETE", uri.clone(), None),
        ("POST", format!("{uri}/items"), Some(json!({ "part_ids": [1] }))),
        ("PUT", format!("{uri}/order"), Some(json!({ "item_ids": [] }))),
        ("DELETE", format!("{uri}/items/1"), None),
    ] {
        let r = call(&a, method, &path, &tb, body).await;
        assert_eq!((r.status, r.json["error"].as_str()), (StatusCode::NOT_FOUND, Some("not_found")), "{method} {path}");
    }
    // The same name is free for another profile; the owner's list is untouched.
    assert_eq!(call(&a, "POST", "/api/v1/playlists", &tb, Some(json!({ "name": "car trip" }))).await.status, StatusCode::CREATED);
    assert_eq!(call(&a, "GET", &uri, &ta, None).await.status, StatusCode::OK);
    // Without a token there is nothing.
    let req = Request::builder().uri("/api/v1/playlists").body(Body::empty()).unwrap();
    assert_eq!(a.router.clone().oneshot(req).await.unwrap().status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn playlists_are_made_renamed_listed_and_deleted_and_names_are_checked() {
    let a = api().await;
    let t = pair(&a, a.admin, 1).await;
    let (_, tracks) = add_book(&a, "Follett", "Winter", &[3]).await;
    let id = call(&a, "POST", "/api/v1/playlists", &t, Some(json!({ "name": "One" }))).await.json["id"].as_i64().unwrap();
    assert_eq!(call(&a, "POST", "/api/v1/playlists", &t, Some(json!({ "name": "ONE" }))).await.json["error"], "name_taken");
    assert_eq!(call(&a, "POST", "/api/v1/playlists", &t, Some(json!({ "name": "ONE" }))).await.status, StatusCode::CONFLICT);
    for bad in ["", "   ", &"x".repeat(101)] {
        let r = call(&a, "POST", "/api/v1/playlists", &t, Some(json!({ "name": bad }))).await;
        assert_eq!((r.status, r.json["error"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_name")));
    }
    assert_eq!(call(&a, "POST", "/api/v1/playlists", &t, Some(json!({ "name": "y".repeat(100) }))).await.status, StatusCode::CREATED);
    let two = call(&a, "POST", "/api/v1/playlists", &t, Some(json!({ "name": "Two" }))).await.json["id"].as_i64().unwrap();
    assert_eq!(call(&a, "PATCH", &format!("/api/v1/playlists/{two}"), &t, Some(json!({ "name": "one" }))).await.json["error"], "name_taken");
    let r = call(&a, "PATCH", &format!("/api/v1/playlists/{two}"), &t, Some(json!({ "name": " Zwei " }))).await;
    assert_eq!((r.status, r.json["name"].as_str()), (StatusCode::OK, Some("Zwei")));
    // Renaming a list to its own name in another case is fine.
    assert_eq!(call(&a, "PATCH", &format!("/api/v1/playlists/{id}"), &t, Some(json!({ "name": "ONE" }))).await.status, StatusCode::OK);

    call(&a, "POST", &format!("/api/v1/playlists/{id}/items"), &t, Some(json!({ "part_ids": [tracks[0], tracks[1]] }))).await;
    let list = call(&a, "GET", "/api/v1/playlists", &t, None).await.json;
    let one = list.as_array().unwrap().iter().find(|p| p["id"] == id).unwrap();
    assert_eq!((one["name"].as_str(), one["items"].as_i64(), one["seconds"].as_i64()), (Some("ONE"), Some(2), Some(480)), "{list}");
    assert!(one["updated_at"].as_i64().unwrap() > 1_700_000_000_000);

    assert_eq!(call(&a, "DELETE", &format!("/api/v1/playlists/{id}"), &t, None).await.status, StatusCode::NO_CONTENT);
    assert_eq!(call(&a, "GET", &format!("/api/v1/playlists/{id}"), &t, None).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call(&a, "DELETE", &format!("/api/v1/playlists/{id}"), &t, None).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_whole_book_is_appended_in_book_order_and_parts_keep_the_order_sent_with_duplicates() {
    let a = api().await;
    let t = pair(&a, a.admin, 1).await;
    // Two discs: the album order is disc, then number, whatever the ids say.
    let (book_id, tracks) = add_book(&a, "Follett", "Saga", &[2, 2]).await;
    let (other, other_tracks) = add_book(&a, "Lindgren", "Ronja", &[1]).await;
    let id = call(&a, "POST", "/api/v1/playlists", &t, Some(json!({ "name": "L" }))).await.json["id"].as_i64().unwrap();
    let items = format!("/api/v1/playlists/{id}/items");

    let r = call(&a, "POST", &items, &t, Some(json!({ "part_ids": [other_tracks[0], tracks[3], tracks[3]] }))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    assert_eq!(titles(&r.json), ["Ronja_CD01_TR01", "Saga_CD02_TR02", "Saga_CD02_TR02"]);
    let r = call(&a, "POST", &items, &t, Some(json!({ "book_id": book_id }))).await;
    assert_eq!(titles(&r.json)[3..], ["Saga_CD01_TR01", "Saga_CD01_TR02", "Saga_CD02_TR01", "Saga_CD02_TR02"]);
    let first = &r.json["items"][0];
    assert_eq!((first["position"].as_i64(), first["available"].as_bool()), (Some(1), Some(true)));
    assert_eq!((first["part"]["id"].as_i64(), first["book"]["id"].as_i64(), first["book"]["title"].as_str(), first["book"]["author"].as_str()), (Some(other_tracks[0]), Some(other), Some("Ronja"), Some("Lindgren")));
    assert_eq!((first["part"]["disc"].as_i64(), first["part"]["number"].as_i64(), first["part"]["duration_ms"].as_i64(), first["part"]["plays"].as_i64()), (Some(1), Some(1), Some(240_000), Some(0)));

    // Insert at a position; the rest moves down.
    let r = call(&a, "POST", &items, &t, Some(json!({ "part_ids": [other_tracks[0]], "at": 2 }))).await;
    assert_eq!(titles(&r.json)[..3], ["Ronja_CD01_TR01", "Ronja_CD01_TR01", "Saga_CD02_TR02"]);
    assert_eq!(r.json["items"].as_array().unwrap().iter().map(|i| i["position"].as_i64().unwrap()).collect::<Vec<_>>(), (1..=8).collect::<Vec<_>>());
    for at in [0, 10] {
        let r = call(&a, "POST", &items, &t, Some(json!({ "part_ids": [tracks[0]], "at": at }))).await;
        assert_eq!((r.status, r.json["error"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_position")));
    }
    // Exactly one of the two.
    for body in [json!({}), json!({ "part_ids": [], }), json!({ "part_ids": [1], "book_id": 1 })] {
        assert_eq!(call(&a, "POST", &items, &t, Some(body)).await.json["error"], "invalid_request");
    }
}

#[tokio::test]
async fn unknown_or_invisible_ids_are_refused_and_nothing_is_written() {
    let a = api().await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let t = pair(&a, anna, 1).await;
    let (shown, shown_tracks) = add_book(&a, "Follett", "Winter", &[1]).await;
    let (hidden, hidden_tracks) = add_book(&a, "Follett", "Secret", &[1]).await;
    a.store.unrelease_book(hidden).await.unwrap();
    let id = call(&a, "POST", "/api/v1/playlists", &t, Some(json!({ "name": "L" }))).await.json["id"].as_i64().unwrap();
    let items = format!("/api/v1/playlists/{id}/items");
    for bad in [hidden_tracks[0], 99_999] {
        let r = call(&a, "POST", &items, &t, Some(json!({ "part_ids": [shown_tracks[0], bad] }))).await;
        assert_eq!((r.status, r.json["error"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("unknown_part")));
    }
    for bad in [hidden, 99_999] {
        assert_eq!(call(&a, "POST", &items, &t, Some(json!({ "book_id": bad }))).await.json["error"], "unknown_book");
    }
    assert_eq!(call(&a, "GET", &format!("/api/v1/playlists/{id}"), &t, None).await.json["items"], json!([]));
    assert_eq!(call(&a, "POST", &items, &t, Some(json!({ "book_id": shown }))).await.status, StatusCode::OK);
}

#[tokio::test]
async fn removing_and_ordering_entries_renumbers_and_the_order_must_be_a_permutation() {
    let a = api().await;
    let t = pair(&a, a.admin, 1).await;
    let (_, tracks) = add_book(&a, "Follett", "Winter", &[4]).await;
    let id = call(&a, "POST", "/api/v1/playlists", &t, Some(json!({ "name": "L" }))).await.json["id"].as_i64().unwrap();
    let r = call(&a, "POST", &format!("/api/v1/playlists/{id}/items"), &t, Some(json!({ "part_ids": tracks }))).await;
    let item = ids(&r.json);
    let order = format!("/api/v1/playlists/{id}/order");

    let r = call(&a, "PUT", &order, &t, Some(json!({ "item_ids": [item[3], item[1], item[0], item[2]] }))).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(titles(&r.json), ["Winter_CD01_TR04", "Winter_CD01_TR02", "Winter_CD01_TR01", "Winter_CD01_TR03"]);
    // The order stays after reading again.
    let again = call(&a, "GET", &format!("/api/v1/playlists/{id}"), &t, None).await.json;
    assert_eq!(ids(&again), [item[3], item[1], item[0], item[2]]);

    for bad in [json!([]), json!([item[0], item[1], item[2]]), json!([item[0], item[0], item[1], item[2]]), json!([item[0], item[1], item[2], item[3], item[3]]), json!([item[0], item[1], item[2], 99_999])] {
        let r = call(&a, "PUT", &order, &t, Some(json!({ "item_ids": bad }))).await;
        assert_eq!((r.status, r.json["error"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_order")), "{bad}");
    }
    assert_eq!(ids(&call(&a, "GET", &format!("/api/v1/playlists/{id}"), &t, None).await.json), [item[3], item[1], item[0], item[2]]);

    assert_eq!(call(&a, "DELETE", &format!("/api/v1/playlists/{id}/items/{}", item[1]), &t, None).await.status, StatusCode::NO_CONTENT);
    assert_eq!(call(&a, "DELETE", &format!("/api/v1/playlists/{id}/items/{}", item[1]), &t, None).await.status, StatusCode::NOT_FOUND);
    let after = call(&a, "GET", &format!("/api/v1/playlists/{id}"), &t, None).await.json;
    assert_eq!(after["items"].as_array().unwrap().iter().map(|i| i["position"].as_i64().unwrap()).collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(ids(&after), [item[3], item[0], item[2]]);
}

#[tokio::test]
async fn a_track_the_profile_may_no_longer_hear_is_marked_and_left_out_of_the_count_but_stays() {
    let a = api().await;
    let kid = a.store.create_user(NewProfile::child("Kid", 8, Schedule::always())).await.unwrap();
    let t = pair(&a, kid, 1).await;
    let (shown, shown_tracks) = add_book(&a, "Lindgren", "Ronja", &[1]).await;
    let (hidden, hidden_tracks) = add_book(&a, "Follett", "Winter", &[2]).await;
    let id = call(&a, "POST", "/api/v1/playlists", &t, Some(json!({ "name": "L" }))).await.json["id"].as_i64().unwrap();
    let uri = format!("/api/v1/playlists/{id}");
    call(&a, "POST", &format!("{uri}/items"), &t, Some(json!({ "part_ids": [shown_tracks[0], hidden_tracks[0], hidden_tracks[1]] }))).await;
    let before = call(&a, "GET", "/api/v1/library/version", &t, None).await.json;
    assert_eq!(call(&a, "GET", "/api/v1/playlists", &t, None).await.json[0]["items"], 3);

    // The parents take the book back; the entries stay but are not available and tell nothing about the book.
    a.store.unrelease_book(hidden).await.unwrap();
    let r = call(&a, "GET", &uri, &t, None).await.json;
    let flags: Vec<bool> = r["items"].as_array().unwrap().iter().map(|i| i["available"].as_bool().unwrap()).collect();
    assert_eq!(flags, [true, false, false]);
    assert_eq!((&r["items"][1]["part"], &r["items"][1]["book"]), (&Value::Null, &Value::Null));
    assert!(r["items"][1]["item_id"].as_i64().is_some());
    let list = call(&a, "GET", "/api/v1/playlists", &t, None).await.json;
    assert_eq!((list[0]["items"].as_i64(), list[0]["seconds"].as_i64()), (Some(1), Some(240)));
    assert_ne!(call(&a, "GET", "/api/v1/library/version", &t, None).await.json["version"], before["version"]);

    // Released again: they play again, in their old places.
    a.store.release_book(hidden, 0).await.unwrap();
    let r = call(&a, "GET", &uri, &t, None).await.json;
    assert_eq!(titles(&r), ["Ronja_CD01_TR01", "Winter_CD01_TR01", "Winter_CD01_TR02"]);
    assert!(r["items"].as_array().unwrap().iter().all(|i| i["available"] == true));
    let _ = shown;
}

#[tokio::test]
async fn deleting_a_book_takes_its_entries_out_of_the_lists_and_deleting_a_profile_its_lists() {
    let a = api().await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let t = pair(&a, anna, 1).await;
    let (gone, gone_tracks) = add_book(&a, "Follett", "Winter", &[2]).await;
    let (_, kept) = add_book(&a, "Lindgren", "Ronja", &[1]).await;
    let id = call(&a, "POST", "/api/v1/playlists", &t, Some(json!({ "name": "L" }))).await.json["id"].as_i64().unwrap();
    let uri = format!("/api/v1/playlists/{id}");
    call(&a, "POST", &format!("{uri}/items"), &t, Some(json!({ "part_ids": [gone_tracks[0], kept[0], gone_tracks[1]] }))).await;
    assert!(a.store.delete_book(gone).await.unwrap());
    let r = call(&a, "GET", &uri, &t, None).await.json;
    assert_eq!(titles(&r), ["Ronja_CD01_TR01"]);
    assert_eq!(r["items"][0]["position"], 1, "positions told are always 1..n");
    // The list works on: a new entry follows the remaining one.
    let r = call(&a, "POST", &format!("{uri}/items"), &t, Some(json!({ "part_ids": [kept[0]] }))).await.json;
    assert_eq!(r["items"].as_array().unwrap().iter().map(|i| i["position"].as_i64().unwrap()).collect::<Vec<_>>(), [1, 2]);

    assert!(a.store.delete_profile(anna).await.is_ok());
    assert!(a.store.playlist_summaries(None).await.unwrap().is_empty());
}

#[tokio::test]
async fn the_playlist_version_changes_only_for_the_profile_that_changed_its_lists() {
    let a = api().await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let ben = a.store.create_user(NewProfile::new("Ben", Role::Member)).await.unwrap();
    let (ta, tb) = (pair(&a, anna, 1).await, pair(&a, ben, 2).await);
    let (_, tracks) = add_book(&a, "Follett", "Winter", &[2]).await;
    let version = |t: String| {
        let a = &a;
        async move { call(a, "GET", "/api/v1/library/version", &t, None).await.json }
    };
    let (lib0, anna0, ben0) = (version(ta.clone()).await, version(ta.clone()).await["playlists"].clone(), version(tb.clone()).await["playlists"].clone());
    let id = call(&a, "POST", "/api/v1/playlists", &ta, Some(json!({ "name": "L" }))).await.json["id"].as_i64().unwrap();
    let (lib1, anna1, ben1) = (version(ta.clone()).await, version(ta.clone()).await["playlists"].clone(), version(tb.clone()).await["playlists"].clone());
    assert_eq!(lib0["version"], lib1["version"], "the library fingerprint is not touched by a playlist");
    assert_ne!(anna0, anna1);
    assert_eq!(ben0, ben1);
    call(&a, "POST", &format!("/api/v1/playlists/{id}/items"), &ta, Some(json!({ "part_ids": [tracks[0]] }))).await;
    let anna2 = version(ta.clone()).await["playlists"].clone();
    assert_ne!(anna1, anna2);
    assert_eq!(version(tb.clone()).await["playlists"], ben0);

    // The list carries the same fingerprint as ETag and answers 304 when it is unchanged.
    let r = call(&a, "GET", "/api/v1/playlists", &ta, None).await;
    let tag = r.headers.get(header::ETAG).unwrap().to_str().unwrap().to_owned();
    assert_eq!(tag, format!("\"{}\"", anna2.as_str().unwrap()));
    assert_eq!(call_with(&a, "GET", "/api/v1/playlists", &ta, None, Some(&tag)).await.status, StatusCode::NOT_MODIFIED);
    call(&a, "PATCH", &format!("/api/v1/playlists/{id}"), &ta, Some(json!({ "name": "Renamed" }))).await;
    assert_eq!(call_with(&a, "GET", "/api/v1/playlists", &ta, None, Some(&tag)).await.status, StatusCode::OK);
}

#[tokio::test]
async fn the_rows_say_who_made_and_changed_them() {
    let a = api().await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let t = pair(&a, anna, 1).await;
    let (_, tracks) = add_book(&a, "Follett", "Winter", &[1]).await;
    let id = call(&a, "POST", "/api/v1/playlists", &t, Some(json!({ "name": "L" }))).await.json["id"].as_i64().unwrap();
    call(&a, "POST", &format!("/api/v1/playlists/{id}/items"), &t, Some(json!({ "part_ids": [tracks[0]] }))).await;
    let audit = a.store.audit_of("playlist", id).await.unwrap().unwrap();
    assert_eq!((audit.created_by.as_deref(), audit.updated_by.as_deref()), (Some(format!("profile:{anna}").as_str()), Some(format!("profile:{anna}").as_str())));
    let item = a.store.playlist_view(id, anna).await.unwrap().unwrap().entries[0].item_id;
    assert_eq!(a.store.audit_of("playlist_item", item).await.unwrap().unwrap().created_by, Some(format!("profile:{anna}")));
}
