// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pairing: one-time codes for a user, redeemed by a device with its key.

mod common;

use common::open;
use david_server::{Error, NewProfile, Role, Store};
use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};

/// A well-formed key; `n` (below 16) makes it unique. The last character of a
/// canonical key can only be one of 16, because 32 bytes leave it two spare bits.
fn key(n: u8) -> String {
    let letters = b"AEIMQUYcgkosw048";
    let c = letters[n as usize % letters.len()] as char;
    format!("{}{c}=", "B".repeat(42))
}

async fn raw(dir: &tempfile::TempDir) -> sea_orm::DatabaseConnection {
    Database::connect(format!("sqlite://{}?mode=rw", dir.path().join("david.db").display())).await.unwrap()
}

async fn admin_and_user(store: &Store, role: Role) -> (i64, i64) {
    let admin = store.create_profile("Boss", true).await.unwrap();
    let user = store.create_user(NewProfile::new("Anna", role)).await.unwrap();
    (admin, user)
}

#[tokio::test]
async fn a_code_is_long_stored_only_as_a_hash_and_expires() {
    let (dir, store) = open().await;
    let (admin, anna) = admin_and_user(&store, Role::Member).await;
    let before = david_server::now_ms();
    let e = store.create_enrollment(anna, false, admin).await.unwrap();
    assert_eq!(e.code.len(), 32);
    assert_eq!(e.user_name, "Anna");
    let left = e.expires_at - before;
    assert!((14 * 60 * 1000..=15 * 60 * 1000 + 1000).contains(&left), "15 minutes by the proposed setting, got {left} ms");

    let rows = raw(&dir)
        .await
        .query_all_raw(Statement::from_string(DbBackend::Sqlite, "SELECT code_hash FROM enrollment"))
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    let stored: String = rows[0].try_get_by_index(0).unwrap();
    assert_ne!(stored, e.code);
    assert_eq!(stored.len(), 64);
}

#[tokio::test]
async fn redeeming_makes_the_device_belong_to_the_user_and_works_once() {
    let (_dir, store) = open().await;
    let (admin, anna) = admin_and_user(&store, Role::Member).await;
    let e = store.create_enrollment(anna, true, admin).await.unwrap();

    let paired = store.redeem_enrollment(&e.code, &key(1), "Annas phone").await.unwrap();
    assert_eq!((paired.user_id, paired.shared, paired.tunnel_ip.as_str()), (anna, true, "10.77.0.2"));
    let devices = store.devices_of(anna).await.unwrap();
    assert_eq!((devices.len(), devices[0].name.as_str(), devices[0].shared), (1, "Annas phone", true));
    assert_eq!(devices[0].created_by, Some(admin), "the administrator who made the code");

    // The same code a second time: refused, and no second device.
    let again = store.redeem_enrollment(&e.code, &key(2), "Another").await;
    assert!(matches!(again, Err(Error::EnrollmentInvalid)), "{again:?}");
    assert_eq!(store.devices_of(anna).await.unwrap().len(), 1);

    // The next device gets the next tunnel address.
    let e2 = store.create_enrollment(anna, false, admin).await.unwrap();
    assert_eq!(store.redeem_enrollment(&e2.code, &key(3), "Tablet").await.unwrap().tunnel_ip, "10.77.0.3");
}

#[tokio::test]
async fn unknown_expired_and_used_codes_look_the_same() {
    let (dir, store) = open().await;
    let (admin, anna) = admin_and_user(&store, Role::Member).await;
    let e = store.create_enrollment(anna, false, admin).await.unwrap();

    let unknown = store.redeem_enrollment(&"0".repeat(32), &key(1), "x").await;
    assert!(matches!(unknown, Err(Error::EnrollmentInvalid)));

    raw(&dir).await.execute_unprepared("UPDATE enrollment SET expires_at = 1").await.unwrap();
    let expired = store.redeem_enrollment(&e.code, &key(1), "x").await;
    assert!(matches!(expired, Err(Error::EnrollmentInvalid)), "{expired:?}");
    assert!(store.devices_of(anna).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_bad_key_or_name_is_refused_and_leaves_the_code_usable() {
    let (_dir, store) = open().await;
    let (admin, anna) = admin_and_user(&store, Role::Member).await;
    let e = store.create_enrollment(anna, false, admin).await.unwrap();

    for bad in ["", "short", &"A".repeat(44), &format!("{}!=", "A".repeat(42))] {
        assert!(matches!(store.redeem_enrollment(&e.code, bad, "Phone").await, Err(Error::InvalidPublicKey)), "{bad:?}");
    }
    for bad in ["", "   ", &"x".repeat(81)] {
        assert!(matches!(store.redeem_enrollment(&e.code, &key(1), bad).await, Err(Error::InvalidDeviceName)), "{bad:?}");
    }
    store.redeem_enrollment(&e.code, &key(1), "Phone").await.expect("the refused attempts did not use up the code");
}

#[tokio::test]
async fn a_key_can_belong_to_one_device_only() {
    let (_dir, store) = open().await;
    let (admin, anna) = admin_and_user(&store, Role::Member).await;
    let first = store.create_enrollment(anna, false, admin).await.unwrap();
    let second = store.create_enrollment(anna, false, admin).await.unwrap();
    store.redeem_enrollment(&first.code, &key(1), "Phone").await.unwrap();

    let same_key = store.redeem_enrollment(&second.code, &key(1), "Copy").await;
    assert!(matches!(same_key, Err(Error::InvalidPublicKey)), "{same_key:?}");
    store.redeem_enrollment(&second.code, &key(2), "Tablet").await.expect("and the code is still usable");
}

#[tokio::test]
async fn the_device_limit_is_checked_when_the_code_is_made_and_when_it_is_used() {
    let (_dir, store) = open().await;
    let admin = store.create_profile("Boss", true).await.unwrap();
    let guest = store.create_user(NewProfile::new("Guest", Role::Guest)).await.unwrap(); // 1 device by the proposed policy

    // Two codes before any device exists: allowed.
    let a = store.create_enrollment(guest, false, admin).await.unwrap();
    let b = store.create_enrollment(guest, false, admin).await.unwrap();
    let first = store.redeem_enrollment(&a.code, &key(1), "Phone").await.unwrap();

    // The limit is reached now: no new code, and the second code does not work.
    assert!(matches!(store.create_enrollment(guest, false, admin).await, Err(Error::DeviceLimitReached)));
    let blocked = store.redeem_enrollment(&b.code, &key(2), "Tablet").await;
    assert!(matches!(blocked, Err(Error::DeviceLimitReached)), "{blocked:?}");

    // Freeing the place makes the code usable again, because the refusal rolled back.
    store.revoke_device(first.device_id).await.unwrap();
    store.redeem_enrollment(&b.code, &key(2), "Tablet").await.expect("the code was not used up by the refusal");
}

#[tokio::test]
async fn two_devices_redeeming_one_code_at_the_same_moment_get_one_winner() {
    let (_dir, store) = open().await;
    let (admin, anna) = admin_and_user(&store, Role::Admin).await;
    let e = store.create_enrollment(anna, false, admin).await.unwrap();

    let (s1, s2) = (store.clone(), store.clone());
    let (c1, c2) = (e.code.clone(), e.code.clone());
    let (r1, r2) = tokio::join!(
        tokio::spawn(async move { s1.redeem_enrollment(&c1, &key(1), "One").await }),
        tokio::spawn(async move { s2.redeem_enrollment(&c2, &key(2), "Two").await }),
    );
    let results = [r1.unwrap(), r2.unwrap()];
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1, "{results:?}");
    assert!(results.iter().any(|r| matches!(r, Err(Error::EnrollmentInvalid))));
    assert_eq!(store.devices_of(anna).await.unwrap().len(), 1);
}

#[tokio::test]
async fn codes_and_devices_go_with_their_user() {
    let (dir, store) = open().await;
    let (admin, anna) = admin_and_user(&store, Role::Member).await;
    let used = store.create_enrollment(anna, false, admin).await.unwrap();
    store.redeem_enrollment(&used.code, &key(1), "Phone").await.unwrap();
    store.create_enrollment(anna, false, admin).await.unwrap();

    store.delete_profile(anna).await.unwrap();
    let count = |table: &'static str| {
        let dir = &dir;
        async move {
            let r = raw(dir).await.query_one_raw(Statement::from_string(DbBackend::Sqlite, format!("SELECT count(*) FROM {table}"))).await.unwrap().unwrap();
            r.try_get_by_index::<i64>(0).unwrap()
        }
    };
    assert_eq!((count("device").await, count("enrollment").await), (0, 0));
}

#[tokio::test]
async fn the_device_list_names_the_owner() {
    let (_dir, store) = open().await;
    let (admin, anna) = admin_and_user(&store, Role::Member).await;
    let e = store.create_enrollment(anna, false, admin).await.unwrap();
    let paired = store.redeem_enrollment(&e.code, &key(1), "Phone").await.unwrap();

    let rows = store.device_rows().await.unwrap();
    assert_eq!((rows.len(), rows[0].user_name.as_str(), rows[0].name.as_str()), (1, "Anna", "Phone"));
    assert_eq!(rows[0].revoked_at, None);
    store.revoke_device(paired.device_id).await.unwrap();
    assert!(store.device_row(paired.device_id).await.unwrap().unwrap().revoked_at.is_some());
    assert!(store.device_row(9999).await.unwrap().is_none());
}
