// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Sign-in, PIN lockout and sessions.

mod common;

use common::open;
use david_server::auth::{Login, csrf_token};
use david_server::{Error, Role, Store};
use sea_orm::{ConnectionTrait, Database};

async fn with_user(store: &Store, name: &str, role: Role, pin: &str) -> i64 {
    let id = store.create_user(david_server::NewProfile::new(name, role)).await.unwrap();
    store.set_pin(id, pin).await.unwrap();
    id
}

async fn raw(dir: &tempfile::TempDir) -> sea_orm::DatabaseConnection {
    Database::connect(format!("sqlite://{}?mode=rw", dir.path().join("david.db").display())).await.unwrap()
}

#[tokio::test]
async fn a_pin_must_be_four_to_twelve_digits() {
    let (_dir, store) = open().await;
    let id = store.create_profile("Anna", false).await.unwrap();
    for bad in ["", "123", "12345678901234", "12a4", "12 34"] {
        assert!(matches!(store.set_pin(id, bad).await, Err(Error::InvalidPin)), "{bad:?} must be refused");
    }
    store.set_pin(id, "1234").await.unwrap();
}

#[tokio::test]
async fn the_pin_is_stored_as_a_hash_and_checked() {
    let (dir, store) = open().await;
    let id = with_user(&store, "Anna", Role::Member, "483920").await;

    let row = store.list_profiles().await.unwrap().into_iter().find(|p| p.id == id).unwrap();
    let stored = row.pin_hash.unwrap();
    assert!(stored.starts_with("$argon2id$"), "an Argon2id hash: {stored}");
    assert!(!stored.contains("483920"));
    let _ = dir;

    assert_eq!(store.verify_login("Anna", "483920").await.unwrap(), Login::Ok { user_id: id, role: Role::Member });
    assert_eq!(store.verify_login("anna", "483920").await.unwrap(), Login::Ok { user_id: id, role: Role::Member }, "names ignore case");
    assert_eq!(store.verify_login("Anna", "000000").await.unwrap(), Login::WrongCredentials);
}

#[tokio::test]
async fn wrong_name_wrong_pin_and_no_pin_look_the_same() {
    let (_dir, store) = open().await;
    with_user(&store, "Anna", Role::Member, "483920").await;
    store.create_profile("NoPin", false).await.unwrap();

    let wrong_pin = store.verify_login("Anna", "111111").await.unwrap();
    let unknown = store.verify_login("Nobody", "483920").await.unwrap();
    let no_pin = store.verify_login("NoPin", "483920").await.unwrap();
    assert_eq!(wrong_pin, Login::WrongCredentials);
    assert_eq!(unknown, wrong_pin);
    assert_eq!(no_pin, wrong_pin, "a user without a PIN cannot sign in");
}

/// The time the user has to wait until, in Unix milliseconds (None: no wait).
async fn wait_until(dir: &tempfile::TempDir, id: i64) -> Option<i64> {
    let row = raw(dir).await.query_one_raw(sea_orm::Statement::from_string(sea_orm::DbBackend::Sqlite, format!("SELECT pin_locked_until FROM profile WHERE id = {id}"))).await.unwrap().unwrap();
    row.try_get_by_index(0).unwrap()
}

/// Lets the wait run out at once, as if the time had passed.
async fn wait_over(dir: &tempfile::TempDir, id: i64) {
    raw(dir).await.execute_unprepared(&format!("UPDATE profile SET pin_locked_until = 1 WHERE id = {id}")).await.unwrap();
}

#[tokio::test]
async fn every_wrong_pin_in_a_row_doubles_the_wait_and_a_right_one_starts_again() {
    // a * 2^n seconds after the n-th wrong PIN in a row (project owner, 2026-10-31); a is the setting pin_backoff_s.
    let (dir, store) = open().await;
    let id = with_user(&store, "Anna", Role::Member, "483920").await;
    store.save_settings(&[("pin_backoff_s".to_owned(), "3".to_owned())]).await.unwrap();

    for n in 1..=5u32 {
        let before = david_server::now_ms();
        assert_eq!(store.verify_login("Anna", "111111").await.unwrap(), Login::WrongCredentials, "try {n}");
        let wait = wait_until(&dir, id).await.unwrap() - before;
        let expected = 3_000 * 2i64.pow(n);
        assert!((expected..expected + 2_000).contains(&wait), "after {n} wrong PINs the wait is {wait} ms, not {expected}");
        // During the wait even the right PIN is refused, unchecked.
        assert!(matches!(store.verify_login("Anna", "483920").await.unwrap(), Login::Locked { .. }), "try {n}");
        wait_over(&dir, id).await;
    }
    // The right PIN after the wait works and starts the count again: the next wrong one waits a * 2 again.
    assert_eq!(store.verify_login("Anna", "483920").await.unwrap(), Login::Ok { user_id: id, role: Role::Member });
    assert_eq!(wait_until(&dir, id).await, None);
    let before = david_server::now_ms();
    assert_eq!(store.verify_login("Anna", "111111").await.unwrap(), Login::WrongCredentials);
    assert!(wait_until(&dir, id).await.unwrap() - before < 8_000);
}

#[tokio::test]
async fn a_new_pin_lifts_the_wait_and_starts_the_count_again() {
    let (dir, store) = open().await;
    let id = with_user(&store, "Anna", Role::Member, "483920").await;
    for _ in 0..6 {
        store.verify_login("Anna", "111111").await.unwrap();
        wait_over(&dir, id).await;
    }
    store.verify_login("Anna", "111111").await.unwrap();
    assert!(matches!(store.verify_login("Anna", "483920").await.unwrap(), Login::Locked { .. }));
    store.set_pin(id, "715204").await.unwrap();
    assert!(matches!(store.verify_login("Anna", "715204").await.unwrap(), Login::Ok { .. }), "an administrator's reset lets the user in at once");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_burst_of_parallel_guesses_gets_one_try() {
    // The lock used to be read before the PIN was checked and written after: parallel attempts all passed the read,
    // and each got a guess (on the old code a right PIN sent as the 40th of a burst got through). Now an attempt is
    // counted before it is checked and sets the wait, so a burst gets one try; all the others are refused unchecked.
    let (_dir, store) = open().await;
    with_user(&store, "Anna", Role::Member, "483920").await;
    let mut burst = tokio::task::JoinSet::new();
    for i in 0..40 {
        let store = store.clone();
        let pin = if i == 39 { "483920" } else { "111111" };
        burst.spawn(async move { store.verify_login("Anna", pin).await.unwrap() });
    }
    let results = burst.join_all().await;
    let checked = results.iter().filter(|r| !matches!(r, Login::Locked { .. })).count();
    assert_eq!(checked, 1, "{results:?}");
}

#[tokio::test]
async fn an_expired_user_cannot_sign_in() {
    let (dir, store) = open().await;
    let id = with_user(&store, "Guest", Role::Guest, "483920").await;
    raw(&dir).await.execute_unprepared(&format!("UPDATE profile SET expires_at = 1 WHERE id = {id}")).await.unwrap();
    assert_eq!(store.verify_login("Guest", "483920").await.unwrap(), Login::WrongCredentials);
}

#[tokio::test]
async fn a_session_is_found_by_its_token_and_the_token_is_not_stored() {
    let (dir, store) = open().await;
    let id = with_user(&store, "Boss", Role::Admin, "483920").await;
    let token = store.create_session(id, Role::Admin, None).await.unwrap();
    assert_eq!(token.len(), 64);

    let info = store.session_for_token(&token).await.unwrap().unwrap();
    assert_eq!((info.user_id, info.role), (id, Role::Admin));
    assert!(store.session_for_token("not a token").await.unwrap().is_none());

    // The database holds a hash, never the token.
    let stored: Vec<String> = raw(&dir)
        .await
        .query_all_raw(sea_orm::Statement::from_string(sea_orm::DbBackend::Sqlite, "SELECT token_hash FROM session"))
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.try_get_by_index(0).unwrap())
        .collect();
    assert_eq!(stored.len(), 1);
    assert_ne!(stored[0], token);
    assert_eq!(stored[0].len(), 64);

    store.delete_session(&token).await.unwrap();
    assert!(store.session_for_token(&token).await.unwrap().is_none());
}

#[tokio::test]
async fn a_session_ends_when_it_expires_and_follows_the_role_of_the_user() {
    let (dir, store) = open().await;
    let boss = with_user(&store, "Boss", Role::Admin, "483920").await;
    let second = with_user(&store, "Second", Role::Admin, "483920").await;
    let token = store.create_session(second, Role::Admin, None).await.unwrap();

    // A demotion takes effect at once, without signing in again.
    store.set_profile_role(second, Role::Member).await.unwrap();
    assert_eq!(store.session_for_token(&token).await.unwrap().unwrap().role, Role::Member);

    // Expiry.
    raw(&dir).await.execute_unprepared("UPDATE session SET expires_at = 1").await.unwrap();
    assert!(store.session_for_token(&token).await.unwrap().is_none());
    assert_eq!(store.purge_expired_sessions().await.unwrap(), 1);
    let _ = boss;
}

#[tokio::test]
async fn sessions_go_with_their_user_and_use_the_lifetime_of_the_role() {
    let (dir, store) = open().await;
    let _boss = with_user(&store, "Boss", Role::Admin, "483920").await;
    let guest = with_user(&store, "Guest", Role::Guest, "483920").await;
    let token = store.create_session(guest, Role::Guest, None).await.unwrap();

    // A guest session lasts 24 hours by the proposed policy.
    let info = store.session_for_token(&token).await.unwrap().unwrap();
    let left = info.expires_at - david_server::now_ms();
    assert!((23 * 3600 * 1000..=24 * 3600 * 1000).contains(&left), "about 24 hours, got {left} ms");

    store.delete_profile(guest).await.unwrap();
    assert!(store.session_for_token(&token).await.unwrap().is_none());
    let count: i64 = raw(&dir)
        .await
        .query_one_raw(sea_orm::Statement::from_string(sea_orm::DbBackend::Sqlite, "SELECT count(*) FROM session"))
        .await
        .unwrap()
        .unwrap()
        .try_get_by_index(0)
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn the_csrf_token_follows_the_session_token() {
    assert_ne!(csrf_token("a"), csrf_token("b"));
}
