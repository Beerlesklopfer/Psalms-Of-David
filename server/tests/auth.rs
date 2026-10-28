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

#[tokio::test]
async fn too_many_wrong_pins_lock_the_user_even_for_the_right_pin() {
    let (dir, store) = open().await;
    let id = with_user(&store, "Anna", Role::Member, "483920").await;

    for _ in 0..4 {
        assert_eq!(store.verify_login("Anna", "111111").await.unwrap(), Login::WrongCredentials);
    }
    // The fifth wrong PIN locks.
    let locked = store.verify_login("Anna", "111111").await.unwrap();
    let Login::Locked { until } = locked else { panic!("expected a lock, got {locked:?}") };
    assert!(until > david_server::now_ms());
    assert!(matches!(store.verify_login("Anna", "483920").await.unwrap(), Login::Locked { .. }), "the right PIN does not help while locked");

    // When the lock has run out, the right PIN works and the counter starts again.
    raw(&dir).await.execute_unprepared(&format!("UPDATE profile SET pin_locked_until = 1 WHERE id = {id}")).await.unwrap();
    assert_eq!(store.verify_login("Anna", "483920").await.unwrap(), Login::Ok { user_id: id, role: Role::Member });
    assert_eq!(store.verify_login("Anna", "111111").await.unwrap(), Login::WrongCredentials, "one failure, not a lock");
}

#[tokio::test]
async fn a_right_pin_resets_the_failure_count() {
    let (_dir, store) = open().await;
    with_user(&store, "Anna", Role::Member, "483920").await;
    for _ in 0..4 {
        store.verify_login("Anna", "111111").await.unwrap();
    }
    assert!(matches!(store.verify_login("Anna", "483920").await.unwrap(), Login::Ok { .. }));
    // Four more wrong ones do not lock, because the count started again.
    for _ in 0..4 {
        assert_eq!(store.verify_login("Anna", "111111").await.unwrap(), Login::WrongCredentials);
    }
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
