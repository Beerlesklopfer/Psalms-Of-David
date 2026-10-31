// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The own library of a profile (schema 0038, `docs/spec-library-hiding.md`): hiding books and authors, the setting to show what was hidden.

mod common;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use common::{book, open};
use david_server::api::{ApiState, router};
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
    json: Value,
}

async fn api() -> Api {
    let (dir, store) = open().await;
    let admin = store.create_profile("Boss", true).await.unwrap();
    let state = ApiState { store: store.clone(), library: None, cd: david_server::cdimport::CdService::new(dir.path()), features: Default::default() };
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
    Reply { status: parts.status, json: serde_json::from_slice(&bytes).unwrap_or(Value::Null) }
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


#[tokio::test]
async fn a_profile_hides_books_and_authors_for_itself_and_the_setting_shows_them_again() {
    let a = api().await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let ben = a.store.create_user(NewProfile::new("Ben", Role::Member)).await.unwrap();
    let (ta, tb) = (pair(&a, anna, 1).await, pair(&a, ben, 2).await);
    let (hobbit, _) = add_book(&a, "Tolkien", "Der Hobbit", &[2]).await;
    let (ring, _) = add_book(&a, "Tolkien", "Die Gefährten", &[2]).await;
    let (pippi, _) = add_book(&a, "Lindgren", "Pippi", &[2]).await;
    let listed = |r: &Reply| -> Vec<i64> { r.json.as_array().unwrap().iter().map(|i| i["id"].as_i64().unwrap()).collect() };
    let version = |r: &Reply| r.json["hidden"].as_str().unwrap().to_owned();

    assert_eq!(listed(&call(&a, "GET", "/api/v1/items", &ta, None).await).len(), 3);
    let v0 = version(&call(&a, "GET", "/api/v1/library/version", &ta, None).await);
    // One book hidden: gone from Anna's list, and only from hers.
    let r = call(&a, "POST", "/api/v1/me/hidden", &ta, Some(json!({ "scope": "book", "id": hobbit, "hidden": true }))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    assert_eq!((r.json["books"][0]["id"].as_i64(), r.json["show_hidden"].as_bool()), (Some(hobbit), Some(false)));
    let mine = listed(&call(&a, "GET", "/api/v1/items", &ta, None).await);
    assert!(!mine.contains(&hobbit) && mine.contains(&ring) && mine.contains(&pippi), "{mine:?}");
    assert_eq!(listed(&call(&a, "GET", "/api/v1/items", &tb, None).await).len(), 3, "Ben still has everything");
    // The version of the lists changes for Anna only.
    let v1 = version(&call(&a, "GET", "/api/v1/library/version", &ta, None).await);
    assert_ne!(v0, v1);
    assert_eq!(version(&call(&a, "GET", "/api/v1/library/version", &tb, None).await), v0);
    // An author: every book of Tolkien.
    let r = call(&a, "POST", "/api/v1/me/hidden", &ta, Some(json!({ "scope": "author", "name": "tolkien", "hidden": true }))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    assert_eq!(listed(&call(&a, "GET", "/api/v1/items", &ta, None).await), vec![pippi]);
    // The music of an author is another thing than his audiobooks: a kind that the books do not have hides nothing.
    call(&a, "POST", "/api/v1/me/hidden", &tb, Some(json!({ "scope": "author", "name": "Lindgren", "kind": "music", "hidden": true }))).await;
    assert_eq!(listed(&call(&a, "GET", "/api/v1/items", &tb, None).await).len(), 3);
    call(&a, "POST", "/api/v1/me/hidden", &tb, Some(json!({ "scope": "author", "name": "Lindgren", "kind": "audiobook", "hidden": true }))).await;
    assert!(!listed(&call(&a, "GET", "/api/v1/items", &tb, None).await).contains(&pippi));

    // The setting: the hidden ones are listed again, marked.
    let r = call(&a, "PUT", "/api/v1/me/library", &ta, Some(json!({ "show_hidden": true }))).await;
    assert_eq!((r.status, r.json["show_hidden"].as_bool()), (StatusCode::OK, Some(true)));
    let all = call(&a, "GET", "/api/v1/items", &ta, None).await;
    let flags: Vec<(i64, bool)> = all.json.as_array().unwrap().iter().map(|i| (i["id"].as_i64().unwrap(), i["hidden"].as_bool().unwrap())).collect();
    assert_eq!(flags.len(), 3);
    assert!(flags.contains(&(hobbit, true)) && flags.contains(&(ring, true)) && flags.contains(&(pippi, false)), "{flags:?}");
    // What was hidden is told, with names, to undo it.
    let lib = call(&a, "GET", "/api/v1/me/library", &ta, None).await;
    assert_eq!((lib.json["books"][0]["title"].as_str(), lib.json["authors"][0]["name"].as_str()), (Some("Der Hobbit"), Some("Tolkien")), "{}", lib.json);
    // Shown again: the book and the author.
    call(&a, "POST", "/api/v1/me/hidden", &ta, Some(json!({ "scope": "author", "name": "Tolkien", "hidden": false }))).await;
    call(&a, "PUT", "/api/v1/me/library", &ta, Some(json!({ "show_hidden": false }))).await;
    let mine = listed(&call(&a, "GET", "/api/v1/items", &ta, None).await);
    assert!(!mine.contains(&hobbit) && mine.contains(&ring), "the book stays hidden, the author is back: {mine:?}");
    call(&a, "POST", "/api/v1/me/hidden", &ta, Some(json!({ "scope": "book", "id": hobbit, "hidden": false }))).await;
    assert_eq!(listed(&call(&a, "GET", "/api/v1/items", &ta, None).await).len(), 3);
    assert_eq!(version(&call(&a, "GET", "/api/v1/library/version", &ta, None).await), v0, "back to how it was");

    // A hidden book can still be opened by its id (hiding is no access rule), and wrong requests are refused.
    call(&a, "POST", "/api/v1/me/hidden", &ta, Some(json!({ "scope": "book", "id": pippi, "hidden": true }))).await;
    assert_eq!(call(&a, "GET", &format!("/api/v1/items/{pippi}"), &ta, None).await.status, StatusCode::OK);
    for (body, status) in [
        (json!({ "scope": "book", "id": 99999, "hidden": true }), StatusCode::NOT_FOUND),
        (json!({ "scope": "author", "name": "Nobody", "hidden": true }), StatusCode::NOT_FOUND),
        (json!({ "scope": "author", "name": "Tolkien", "kind": "video", "hidden": true }), StatusCode::UNPROCESSABLE_ENTITY),
        (json!({ "scope": "book", "hidden": true }), StatusCode::UNPROCESSABLE_ENTITY),
        (json!({ "scope": "shelf", "hidden": true }), StatusCode::UNPROCESSABLE_ENTITY),
    ] {
        assert_eq!(call(&a, "POST", "/api/v1/me/hidden", &ta, Some(body.clone())).await.status, status, "{body}");
    }
}

#[tokio::test]
async fn the_owner_of_a_book_is_shown_in_the_items_and_is_not_the_release() {
    let a = api().await;
    let anna = a.store.create_user(NewProfile::new("Anni", Role::Member)).await.unwrap();
    let token = pair(&a, anna, 1).await;
    let (mine, _) = add_book(&a, "Band", "Album", &[1]).await;
    let (other, _) = add_book(&a, "Band", "Zwei", &[1]).await;
    let owner = |items: &Reply, id: i64| items.json.as_array().unwrap().iter().find(|i| i["id"] == id).unwrap()["owner"].clone();
    assert_eq!(owner(&call(&a, "GET", "/api/v1/items", &token, None).await, mine), Value::Null);
    assert!(a.store.set_book_owner(mine, Some(anna)).await.unwrap());
    let items = call(&a, "GET", "/api/v1/items", &token, None).await;
    assert_eq!((owner(&items, mine), owner(&items, other)), (json!("Anni"), Value::Null));
    // The owner is no release: a book of Anni's that is not released is still not seen by Anni.
    a.store.unrelease_book(mine).await.unwrap();
    assert!(!items_ids(&call(&a, "GET", "/api/v1/items", &token, None).await).contains(&mine));
    // Unknown books and profiles are refused; removing the owner works.
    assert!(!a.store.set_book_owner(9999, Some(anna)).await.unwrap());
    assert!(!a.store.set_book_owner(mine, Some(9999)).await.unwrap());
    assert!(a.store.set_book_owner(mine, None).await.unwrap());
    assert_eq!(a.store.admin_book_row(mine).await.unwrap().unwrap().owner, None);
}

fn items_ids(r: &Reply) -> Vec<i64> {
    r.json.as_array().unwrap().iter().map(|i| i["id"].as_i64().unwrap()).collect()
}
