// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! `david reset-pin`: a forgotten PIN is replaced from the command line.

mod common;

use david_server::app::{init_admin, reset_pin};
use david_server::auth::Login;
use david_server::{Error, Role, Store};

#[tokio::test]
async fn a_new_pin_replaces_the_old_one_ends_the_sessions_and_lifts_the_lock() {
    let dir = tempfile::TempDir::new().unwrap();
    init_admin(dir.path(), "Papa", "483920").await.unwrap();
    let store = Store::open(&dir.path().join("david.db")).await.unwrap();
    let id = store.list_profiles().await.unwrap().into_iter().find(|p| p.name == "Papa").unwrap().id;
    let token = store.create_session(id, Role::Admin, None).await.unwrap();

    // a = 10 s, so the wait cannot run out between the attempts, however slow the machine.
    store.save_settings(&[("pin_backoff_s".to_owned(), "10".to_owned())]).await.unwrap();
    // Locked out by wrong attempts.
    for _ in 0..5 {
        let _ = store.verify_login("Papa", "000000").await;
    }
    assert!(matches!(store.verify_login("Papa", "483920").await.unwrap(), Login::Locked { .. }), "locked, even for the right PIN");
    drop(store);

    reset_pin(dir.path(), "Papa", "715204").await.unwrap();

    let store = Store::open(&dir.path().join("david.db")).await.unwrap();
    assert!(matches!(store.verify_login("Papa", "715204").await.unwrap(), Login::Ok { .. }), "the new PIN works at once");
    assert!(matches!(store.verify_login("Papa", "483920").await.unwrap(), Login::WrongCredentials), "the old one does not");
    assert!(store.session_for_token(&token).await.unwrap().is_none(), "the old sign-in is over");
}

#[tokio::test]
async fn it_refuses_what_it_cannot_do_and_changes_nothing() {
    let dir = tempfile::TempDir::new().unwrap();
    // No database yet.
    assert!(reset_pin(dir.path(), "Papa", "715204").await.is_err());
    init_admin(dir.path(), "Papa", "483920").await.unwrap();
    // No such user, and a secret that is neither a PIN nor a password.
    assert!(matches!(reset_pin(dir.path(), "Nobody", "715204").await, Err(Error::WriteFailed(_))));
    assert!(matches!(reset_pin(dir.path(), "Papa", "12").await, Err(Error::InvalidPin)));
    let store = Store::open(&dir.path().join("david.db")).await.unwrap();
    assert!(matches!(store.verify_login("Papa", "483920").await.unwrap(), Login::Ok { .. }), "the old PIN still works");
}
