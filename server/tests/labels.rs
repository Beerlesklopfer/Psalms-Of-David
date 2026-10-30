// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Labels, the classic tagging (schema 0044): the owner of a book sets them, an administrator may override, everybody who sees the book sees them,
//! the lists can be filtered by them.

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


#[tokio::test]
async fn the_owner_sets_the_labels_of_a_book_an_administrator_may_override_and_everybody_sees_them() {
    let a = api().await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let ben = a.store.create_user(NewProfile::new("Ben", Role::Member)).await.unwrap();
    let gast = a.store.create_user(NewProfile::new("Gast", Role::Guest)).await.unwrap();
    let (ta, tb, tg) = (pair(&a, anna, 1).await, pair(&a, ben, 2).await, pair(&a, gast, 3).await);
    let (mine, _) = add_book(&a, "Taizé", "Lieder zur Stille", &[2]).await;
    let (other, _) = add_book(&a, "Lindgren", "Pippi", &[2]).await;
    assert!(a.store.set_book_owner(mine, Some(anna)).await.unwrap());
    let url = |id: i64| format!("/api/v1/items/{id}/labels");

    // Nobody has labels; the lists carry the field.
    let items = call(&a, "GET", "/api/v1/items", &tb, None).await;
    assert!(items.json.as_array().unwrap().iter().all(|i| i["labels"] == json!([])), "{}", items.json);
    let v0 = call(&a, "GET", "/api/v1/library/version", &tb, None).await.json["version"].as_str().unwrap().to_owned();

    // The owner may, anybody else may not (not even to see that the book is not theirs), a guest neither.
    let r = call(&a, "PUT", &url(mine), &ta, Some(json!({ "labels": ["Gebet", " gemeinsame   Urlaube ", "gebet"] }))).await;
    assert_eq!((r.status, r.json["labels"].clone()), (StatusCode::OK, json!(["Gebet", "gemeinsame Urlaube"])), "{}", r.json);
    assert_eq!(call(&a, "PUT", &url(mine), &tb, Some(json!({ "labels": ["Schule"] }))).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&a, "PUT", &url(mine), &tg, Some(json!({ "labels": ["Schule"] }))).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&a, "PUT", &url(other), &ta, Some(json!({ "labels": ["Schule"] }))).await.status, StatusCode::FORBIDDEN, "a book nobody owns is not Anna's");

    // The list says whether the asker may set the labels of a book: the owner and an administrator may, nobody else.
    let can = |r: &Reply, id: i64| r.json.as_array().unwrap().iter().find(|i| i["id"] == id).unwrap()["can_label"].clone();
    let (ia, ib) = (call(&a, "GET", "/api/v1/items", &ta, None).await, call(&a, "GET", "/api/v1/items", &tb, None).await);
    assert_eq!((can(&ia, mine), can(&ia, other), can(&ib, mine)), (json!(true), json!(false), json!(false)));
    let ic = call(&a, "GET", "/api/v1/items", &pair(&a, a.admin, 5).await, None).await;
    assert_eq!((can(&ic, mine), can(&ic, other)), (json!(true), json!(true)), "an administrator may always");

    // Everybody who sees the book sees its labels, in the lists and on their own; the version of the library changes.
    let items = call(&a, "GET", "/api/v1/items", &tb, None).await;
    let of = |r: &Reply, id: i64| r.json.as_array().unwrap().iter().find(|i| i["id"] == id).unwrap()["labels"].clone();
    assert_eq!(of(&items, mine), json!(["Gebet", "gemeinsame Urlaube"]));
    assert_eq!(call(&a, "GET", &url(mine), &tb, None).await.json["labels"], json!(["Gebet", "gemeinsame Urlaube"]));
    let v1 = call(&a, "GET", "/api/v1/library/version", &tb, None).await.json["version"].as_str().unwrap().to_owned();
    assert_ne!(v0, v1, "a change of labels makes the apps load the library again");

    // The list of labels counts the books the asker sees; a name that is not usable is refused and nothing changes.
    let all = call(&a, "GET", "/api/v1/labels", &tb, None).await;
    assert_eq!(all.json["labels"], json!([{ "name": "Gebet", "books": 1, "podcasts": 0, "stations": 0, "count": 1 }, { "name": "gemeinsame Urlaube", "books": 1, "podcasts": 0, "stations": 0, "count": 1 }]));
    assert_eq!(all.json["can_manage"], false, "only an administrator manages the list");
    let r = call(&a, "PUT", &url(mine), &ta, Some(json!({ "labels": ["Gebet", "a,b"] }))).await;
    assert_eq!((r.status, r.json["error"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_label")), "{}", r.json);
    assert_eq!(a.store.labels_of(mine).await.unwrap().len(), 2);

    // An administrator overrides; a book that is not visible to the asker is not found.
    let tboss = pair(&a, a.admin, 4).await;
    let r = call(&a, "PUT", &url(mine), &tboss, Some(json!({ "labels": ["Schule"] }))).await;
    assert_eq!((r.status, r.json["labels"].clone()), (StatusCode::OK, json!(["Schule"])), "{}", r.json);
    a.store.unrelease_book(other).await.unwrap();
    assert_eq!(call(&a, "PUT", &url(other), &tb, Some(json!({ "labels": ["Schule"] }))).await.status, StatusCode::NOT_FOUND);
    // A label that no book carries any more is gone from the list.
    call(&a, "PUT", &url(mine), &tboss, Some(json!({ "labels": [] }))).await;
    assert_eq!(call(&a, "GET", "/api/v1/labels", &tb, None).await.json["labels"], json!([]));
    // Renaming and deleting a label for every book is for administrators; two labels become one when the name exists.
    call(&a, "PUT", &url(mine), &tboss, Some(json!({ "labels": ["Alt", "Neu"] }))).await;
    let rename = |tok: String, old: &str, new: &str| { let a = &a; let old = old.to_owned(); let new = new.to_owned(); async move { call(a, "PUT", &format!("/api/v1/labels/{old}"), &tok, Some(json!({ "name": new }))).await } };
    assert_eq!(rename(tb.clone(), "Alt", "X").await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&a, "DELETE", "/api/v1/labels/Alt", &tb, None).await.status, StatusCode::FORBIDDEN);
    assert_eq!(rename(tboss.clone(), "Alt", "a,b").await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(rename(tboss.clone(), "Gibt%20es%20nicht", "X").await.status, StatusCode::NOT_FOUND);
    assert_eq!(rename(tboss.clone(), "alt", "neu").await.status, StatusCode::OK, "merged into the label that exists");
    assert_eq!(a.store.labels_of(mine).await.unwrap(), vec!["Neu".to_owned()]);
    assert_eq!(call(&a, "DELETE", "/api/v1/labels/NEU", &tboss, None).await.status, StatusCode::NO_CONTENT);
    assert!(a.store.labels_of(mine).await.unwrap().is_empty());
    assert_eq!(call(&a, "DELETE", "/api/v1/labels/Neu", &tboss, None).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call(&a, "GET", "/api/v1/labels", &tboss, None).await.json["can_manage"], true);
}

#[tokio::test]
async fn labels_ignore_case_are_renamed_and_merged_and_go_with_their_book() {
    let a = api().await;
    let (x, _) = add_book(&a, "A", "Eins", &[1]).await;
    let (y, _) = add_book(&a, "B", "Zwei", &[1]).await;
    a.store.set_book_labels(x, &["Schule".into(), "Chor".into()]).await.unwrap();
    a.store.set_book_labels(y, &["schule".into()]).await.unwrap();
    let names = |l: Vec<david_server::labels::LabelRow>| l.into_iter().map(|r| (r.name, r.books)).collect::<Vec<_>>();
    assert_eq!(names(a.store.labels().await.unwrap()), vec![("Chor".to_owned(), 1), ("Schule".to_owned(), 2)], "Schule and schule are one label");
    // Add and remove keep the others.
    assert_eq!(a.store.change_book_labels(x, &["Gebet".into()], &["CHOR".into()]).await.unwrap(), Some(vec!["Gebet".to_owned(), "Schule".to_owned()]));
    // Rename: a new name, then a name that exists (the two become one).
    assert!(a.store.rename_label("schule", "Unterricht").await.unwrap());
    assert!(a.store.rename_label("Gebet", "unterricht").await.unwrap());
    assert_eq!(names(a.store.labels().await.unwrap()), vec![("Unterricht".to_owned(), 2)]);
    assert!(!a.store.rename_label("gibt es nicht", "x").await.unwrap());
    // A book that is deleted takes its labels along; the unused label goes.
    assert!(a.store.delete_book(x).await.unwrap());
    assert_eq!(names(a.store.labels().await.unwrap()), vec![("Unterricht".to_owned(), 1)]);
    assert!(a.store.delete_label("UNTERRICHT").await.unwrap());
    assert!(a.store.labels().await.unwrap().is_empty());
    // No such book: nothing is made.
    assert_eq!(a.store.set_book_labels(9999, &["Neu".into()]).await.unwrap(), None);
    assert!(a.store.set_book_labels(y, &["x".repeat(41)]).await.is_err());
}

#[tokio::test]
async fn single_titles_carry_labels_of_their_own_next_to_the_ones_of_their_album() {
    let a = api().await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let ben = a.store.create_user(NewProfile::new("Ben", Role::Member)).await.unwrap();
    let (ta, tb) = (pair(&a, anna, 1).await, pair(&a, ben, 2).await);
    let (mine, tracks) = add_book(&a, "Taizé", "Lieder zur Stille", &[3]).await;
    let (other, other_tracks) = add_book(&a, "Lindgren", "Pippi", &[2]).await;
    assert!(a.store.set_book_owner(mine, Some(anna)).await.unwrap());
    let url = |id: i64, part: i64| format!("/api/v1/items/{id}/parts/{part}/labels");
    let v0 = call(&a, "GET", "/api/v1/library/version", &tb, None).await.json["version"].as_str().unwrap().to_owned();

    // The owner sets the labels of a title, not Ben, and not a title of another book (the id must belong to the item).
    let r = call(&a, "PUT", &url(mine, tracks[0]), &ta, Some(json!({ "labels": ["Gebet", "gebet", " Stille "] }))).await;
    assert_eq!((r.status, r.json["labels"].clone()), (StatusCode::OK, json!(["Gebet", "Stille"])), "{}", r.json);
    assert_eq!(call(&a, "PUT", &url(mine, tracks[1]), &tb, Some(json!({ "labels": ["x"] }))).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&a, "PUT", &url(mine, other_tracks[0]), &ta, Some(json!({ "labels": ["x"] }))).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call(&a, "PUT", &url(mine, tracks[1]), &ta, Some(json!({ "labels": ["a,b"] }))).await.json["error"], "invalid_label");
    let too_many: Vec<String> = (0..11).map(|n| format!("w{n}")).collect();
    assert_eq!(call(&a, "PUT", &url(mine, tracks[1]), &ta, Some(json!({ "labels": too_many }))).await.status, StatusCode::UNPROCESSABLE_ENTITY, "at most 10 of its own");

    // Everybody who sees the item sees the labels of its parts; the list names the labels its titles carry, and the version changes.
    let detail = call(&a, "GET", &format!("/api/v1/items/{mine}"), &tb, None).await;
    let parts = detail.json["parts"].as_array().unwrap();
    assert_eq!((parts[0]["labels"].clone(), parts[1]["labels"].clone()), (json!(["Gebet", "Stille"]), json!([])), "{}", detail.json);
    let items = call(&a, "GET", "/api/v1/items", &tb, None).await;
    let of = |id: i64, key: &str| items.json.as_array().unwrap().iter().find(|i| i["id"] == id).unwrap()[key].clone();
    assert_eq!((of(mine, "labels"), of(mine, "track_labels"), of(other, "track_labels")), (json!([]), json!(["Gebet", "Stille"]), json!([])));
    assert_ne!(v0, call(&a, "GET", "/api/v1/library/version", &tb, None).await.json["version"].as_str().unwrap());

    // The list of labels counts an album once, whether the album, its titles or both carry the label.
    a.store.set_book_labels(mine, &["Gebet".into()]).await.unwrap();
    a.store.set_track_labels(tracks[1], &["Gebet".into()]).await.unwrap();
    let all = call(&a, "GET", "/api/v1/labels", &tb, None).await;
    assert_eq!(all.json["labels"], json!([{ "name": "Gebet", "books": 1, "podcasts": 0, "stations": 0, "count": 1 }, { "name": "Stille", "books": 1, "podcasts": 0, "stations": 0, "count": 1 }]));
    let names = |l: Vec<david_server::labels::LabelRow>| l.into_iter().map(|r| (r.name, r.books, r.tracks)).collect::<Vec<_>>();
    assert_eq!(names(a.store.labels().await.unwrap()), vec![("Gebet".to_owned(), 1, 2), ("Stille".to_owned(), 0, 1)]);

    // Rename merges the titles too; a label that only a title carries stays; the last title that loses it takes it out of the list.
    assert!(a.store.rename_label("Stille", "gebet").await.unwrap());
    assert_eq!(a.store.track_labels().await.unwrap().get(&tracks[0]).cloned(), Some(vec!["Gebet".to_owned()]));
    a.store.set_book_labels(mine, &[]).await.unwrap();
    assert_eq!(names(a.store.labels().await.unwrap()), vec![("Gebet".to_owned(), 0, 2)], "a title keeps the label alive");
    assert_eq!(a.store.change_track_labels(tracks[0], &[], &["GEBET".into()]).await.unwrap(), Some(vec![]));
    assert_eq!(a.store.set_track_labels(tracks[1], &[]).await.unwrap(), Some(vec![]));
    assert!(a.store.labels().await.unwrap().is_empty());
    assert_eq!(a.store.set_track_labels(999_999, &["Neu".into()]).await.unwrap(), None);

    // A deleted book takes the labels of its titles with it and the unused label goes.
    a.store.set_track_labels(tracks[2], &["Nur Titel".into()]).await.unwrap();
    assert!(a.store.delete_book(mine).await.unwrap());
    assert!(a.store.labels().await.unwrap().is_empty());
}

const FEED: &str = r#"<?xml version="1.0"?><rss version="2.0"><channel><title>Kinderfunk</title><link>https://example.org/</link><description>Geschichten</description>
  <item><title>Folge 1</title><guid>a</guid><enclosure url="https://example.org/1.mp3" type="audio/mpeg"/><pubDate>Tue, 13 Oct 2026 10:00:00 GMT</pubDate></item></channel></rss>"#;

#[tokio::test]
async fn podcasts_and_radio_stations_carry_labels_too_a_member_labels_what_they_have() {
    let a = api().await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let ben = a.store.create_user(NewProfile::new("Ben", Role::Member)).await.unwrap();
    let kind = a.store.create_user(NewProfile::child("Kind", 10, Schedule::always())).await.unwrap();
    let (ta, tb, tk, tadmin) = (pair(&a, anna, 1).await, pair(&a, ben, 2).await, pair(&a, kind, 3).await, pair(&a, a.admin, 4).await);
    let feed = david_server::podcast::parse(FEED).unwrap();
    let pod = a.store.save_feed("https://example.org/feed.xml", &feed).await.unwrap();
    let station = a.store.save_station("https://stream.example.org/a", "Radio A", None, None).await.unwrap();
    for who in [anna, kind] {
        a.store.subscribe(who, pod, true).await.unwrap();
        a.store.favorite_station(who, station, true).await.unwrap();
    }
    let (pod_url, station_url) = (format!("/api/v1/podcasts/{pod}/labels"), format!("/api/v1/radio/{station}/labels"));

    // The lists carry the field and say who may set it: a member for what they have, never a child.
    let list = |r: &Reply, key: &str| r.json[key].as_array().unwrap()[0].clone();
    let (pa, pk) = (call(&a, "GET", "/api/v1/podcasts", &ta, None).await, call(&a, "GET", "/api/v1/podcasts", &tk, None).await);
    assert_eq!((list(&pa, "podcasts")["labels"].clone(), list(&pa, "podcasts")["can_label"].clone(), list(&pk, "podcasts")["can_label"].clone()), (json!([]), json!(true), json!(false)));
    let v0 = call(&a, "GET", "/api/v1/library/version", &ta, None).await.json["version"].as_str().unwrap().to_owned();

    // Anna labels both; Ben does not have them (not found), the child may not, an administrator may always.
    let r = call(&a, "PUT", &pod_url, &ta, Some(json!({ "labels": ["Kinder", " gebet ", "kinder"] }))).await;
    assert_eq!((r.status, r.json["labels"].clone()), (StatusCode::OK, json!(["gebet", "Kinder"])), "{}", r.json);
    assert_eq!(call(&a, "PUT", &station_url, &ta, Some(json!({ "labels": ["Gebet"] }))).await.status, StatusCode::OK, "same word, other case: the label of the list keeps its first spelling");
    assert_eq!(call(&a, "PUT", &pod_url, &tb, Some(json!({ "labels": ["x"] }))).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call(&a, "PUT", &pod_url, &tk, Some(json!({ "labels": ["x"] }))).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&a, "PUT", &station_url, &tk, Some(json!({ "labels": ["x"] }))).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&a, "PUT", &pod_url, &ta, Some(json!({ "labels": ["a,b"] }))).await.json["error"], "invalid_label");
    assert_eq!(call(&a, "PUT", "/api/v1/podcasts/9999/labels", &tadmin, Some(json!({ "labels": ["x"] }))).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call(&a, "PUT", &pod_url, &tadmin, Some(json!({ "labels": ["Kinder", "gebet", "Weihnachten"] }))).await.status, StatusCode::OK, "an administrator may always");

    // Everybody who has them sees the labels; the version changes; the list of labels counts every kind once per label.
    let seen = call(&a, "GET", "/api/v1/radio", &tk, None).await;
    assert_eq!(list(&seen, "stations")["labels"], json!(["gebet"]), "{}", seen.json);
    assert_ne!(v0, call(&a, "GET", "/api/v1/library/version", &ta, None).await.json["version"].as_str().unwrap());
    let all = call(&a, "GET", "/api/v1/labels", &ta, None).await;
    assert_eq!(
        all.json["labels"],
        json!([
            { "name": "gebet", "books": 0, "podcasts": 1, "stations": 1, "count": 2 },
            { "name": "Kinder", "books": 0, "podcasts": 1, "stations": 0, "count": 1 },
            { "name": "Weihnachten", "books": 0, "podcasts": 1, "stations": 0, "count": 1 }
        ]),
        "{}",
        all.json
    );
    // What Ben does not have does not count for Ben.
    assert_eq!(call(&a, "GET", "/api/v1/labels", &tb, None).await.json["labels"], json!([]));

    // Renaming reaches every kind; deleting a station keeps the labels that others carry.
    assert!(a.store.rename_label("Gebet", "Andacht").await.unwrap());
    assert_eq!(a.store.station_labels().await.unwrap().get(&station).cloned(), Some(vec!["Andacht".to_owned()]));
    a.store.delete_station(station).await.unwrap();
    let names: Vec<String> = a.store.labels().await.unwrap().into_iter().map(|l| l.name).collect();
    assert_eq!(names, vec!["Andacht".to_owned(), "Kinder".to_owned(), "Weihnachten".to_owned()], "the podcast still carries Andacht");
}

#[tokio::test]
async fn labels_are_changed_by_deltas_the_owner_and_the_administrator_may_and_the_same_change_twice_is_harmless() {
    let a = api().await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let ben = a.store.create_user(NewProfile::new("Ben", Role::Member)).await.unwrap();
    let (ta, tb, tadmin) = (pair(&a, anna, 1).await, pair(&a, ben, 2).await, pair(&a, a.admin, 3).await);
    let (mine, tracks) = add_book(&a, "Taizé", "Lieder zur Stille", &[2]).await;
    assert!(a.store.set_book_owner(mine, Some(anna)).await.unwrap());
    let (url, part_url) = (format!("/api/v1/items/{mine}/labels"), format!("/api/v1/items/{mine}/parts/{}/labels", tracks[0]));

    // The administrator set "Gebet" in the meantime; the delta of Anna (who was offline) adds and removes only its own words.
    assert_eq!(call(&a, "PUT", &url, &tadmin, Some(json!({ "labels": ["Gebet", "Alt"] }))).await.status, StatusCode::OK);
    let r = call(&a, "PATCH", &url, &ta, Some(json!({ "add": ["Schule", "schule"], "remove": ["alt", "Nie da"] }))).await;
    assert_eq!((r.status, r.json["labels"].clone()), (StatusCode::OK, json!(["Gebet", "Schule"])), "{}", r.json);
    // Sent again: the same state.
    let again = call(&a, "PATCH", &url, &ta, Some(json!({ "add": ["Schule"], "remove": ["alt"] }))).await;
    assert_eq!(again.json["labels"], json!(["Gebet", "Schule"]));
    // A title as well.
    let r = call(&a, "PATCH", &part_url, &ta, Some(json!({ "add": ["Abend"] }))).await;
    assert_eq!((r.status, r.json["labels"].clone()), (StatusCode::OK, json!(["Abend"])), "{}", r.json);
    assert_eq!(call(&a, "PATCH", &part_url, &ta, Some(json!({ "remove": ["abend"] }))).await.json["labels"], json!([]));

    // Anybody but the owner and the administrator is refused; a title of another book and an unknown book are not found; a bad word is invalid.
    assert_eq!(call(&a, "PATCH", &url, &tb, Some(json!({ "add": ["x"] }))).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&a, "PATCH", "/api/v1/items/9999/labels", &tadmin, Some(json!({ "add": ["x"] }))).await.status, StatusCode::NOT_FOUND);
    let (other, other_tracks) = add_book(&a, "Lindgren", "Pippi", &[1]).await;
    assert_eq!(call(&a, "PATCH", &format!("/api/v1/items/{mine}/parts/{}/labels", other_tracks[0]), &tadmin, Some(json!({ "add": ["x"] }))).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call(&a, "PATCH", &url, &ta, Some(json!({ "add": ["a,b"] }))).await.json["error"], "invalid_label");
    let _ = other;
}

#[tokio::test]
async fn labels_of_podcasts_and_stations_are_changed_by_deltas_too() {
    let a = api().await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let ben = a.store.create_user(NewProfile::new("Ben", Role::Member)).await.unwrap();
    let kind = a.store.create_user(NewProfile::child("Kind", 10, Schedule::always())).await.unwrap();
    let (ta, tb, tk) = (pair(&a, anna, 1).await, pair(&a, ben, 2).await, pair(&a, kind, 3).await);
    let feed = david_server::podcast::parse(FEED).unwrap();
    let pod = a.store.save_feed("https://example.org/feed.xml", &feed).await.unwrap();
    let station = a.store.save_station("https://stream.example.org/a", "Radio A", None, None).await.unwrap();
    for who in [anna, kind] {
        a.store.subscribe(who, pod, true).await.unwrap();
        a.store.favorite_station(who, station, true).await.unwrap();
    }
    for url in [format!("/api/v1/podcasts/{pod}/labels"), format!("/api/v1/radio/{station}/labels")] {
        let r = call(&a, "PATCH", &url, &ta, Some(json!({ "add": ["Kinder", "kinder"] }))).await;
        assert_eq!((r.status, r.json["labels"].clone()), (StatusCode::OK, json!(["Kinder"])), "{}", r.json);
        assert_eq!(call(&a, "PATCH", &url, &ta, Some(json!({ "add": ["Kinder"], "remove": ["x"] }))).await.json["labels"], json!(["Kinder"]), "twice is harmless");
        assert_eq!(call(&a, "PATCH", &url, &tb, Some(json!({ "add": ["x"] }))).await.status, StatusCode::NOT_FOUND, "not theirs");
        assert_eq!(call(&a, "PATCH", &url, &tk, Some(json!({ "add": ["x"] }))).await.status, StatusCode::FORBIDDEN, "a child may not");
        assert_eq!(call(&a, "PATCH", &url, &ta, Some(json!({ "remove": ["kinder"] }))).await.json["labels"], json!([]));
    }
}
