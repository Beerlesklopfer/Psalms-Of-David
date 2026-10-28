// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Rules of the data model: roles, limits, release and age rating, the weekly
//! planner. See docs/spec-discovery-profiles-roles.md.

mod common;

use common::{book, open};
use david_server::{Access, Error, NewDevice, NewProfile, Role, Schedule, Store};
use sea_orm::{ConnectionTrait, Database};

/// Monday to Sunday, 07:00 to 19:00.
fn daytime() -> Schedule {
    let mut s = Schedule::none();
    s.allow_every_day(7 * 60, 19 * 60);
    s
}

async fn one_book(store: &Store, name: &str) -> i64 {
    store.import_books(vec![book("Author", name, &[2])]).await.unwrap();
    store.find_book(&format!("Author/{name}")).await.unwrap().unwrap().id
}

async fn titles(store: &Store, user: i64) -> Vec<String> {
    store.visible_books(user).await.unwrap().into_iter().map(|b| b.title).collect()
}

fn device(user_id: i64, n: usize) -> NewDevice {
    NewDevice {
        user_id,
        name: format!("device {n}"),
        wg_pubkey: format!("key-{user_id}-{n}"),
        tunnel_ip: format!("10.77.{user_id}.{n}"),
        shared: false,
        created_by: None,
    }
}

// Roles

#[tokio::test]
async fn the_last_admin_cannot_be_deleted_or_demoted() {
    let (_dir, store) = open().await;
    let boss = store.create_profile("Boss", true).await.unwrap();

    assert!(matches!(store.delete_profile(boss).await, Err(Error::LastAdmin)));
    assert!(matches!(store.set_profile_role(boss, Role::Member).await, Err(Error::LastAdmin)));

    // With a second admin both are fine.
    let second = store.create_profile("Second", true).await.unwrap();
    store.set_profile_role(boss, Role::Member).await.unwrap();
    assert!(matches!(store.delete_profile(second).await, Err(Error::LastAdmin)));
}

#[tokio::test]
async fn a_child_needs_an_age_limit_and_a_planner() {
    let (_dir, store) = open().await;
    let r = store.create_user(NewProfile::new("Kid", Role::Child)).await;
    assert!(matches!(r, Err(Error::ChildRequiresAgeAndSchedule)), "got {r:?}");

    let kid = store.create_user(NewProfile::child("Kid", 8, daytime())).await.unwrap();

    // Taking the planner away, or turning an adult into a child without one, is refused too.
    assert!(matches!(
        store.set_profile_schedule(kid, None).await,
        Err(Error::ChildRequiresAgeAndSchedule)
    ));
    let adult = store.create_profile("Adult", false).await.unwrap();
    assert!(matches!(
        store.set_profile_role(adult, Role::Child).await,
        Err(Error::ChildRequiresAgeAndSchedule)
    ));
}

#[tokio::test]
async fn roles_are_stored_and_the_old_admin_flag_follows() {
    let (_dir, store) = open().await;
    store.create_user(NewProfile::new("A", Role::Admin)).await.unwrap();
    store.create_user(NewProfile::new("G", Role::Guest)).await.unwrap();
    let all = store.list_profiles().await.unwrap();
    let a = all.iter().find(|p| p.name == "A").unwrap();
    let g = all.iter().find(|p| p.name == "G").unwrap();
    assert_eq!((a.role.as_str(), a.is_admin), ("admin", true));
    assert_eq!((g.role.as_str(), g.is_admin), ("guest", false));
}

// Devices

#[tokio::test]
async fn devices_are_limited_per_role_and_a_revoked_device_frees_its_place() {
    let (_dir, store) = open().await;
    let guest = store.create_user(NewProfile::new("Guest", Role::Guest)).await.unwrap();
    let anna = store.create_profile("Anna", false).await.unwrap();

    // Defaults of the role policy: guest 1, member 5.
    let first = store.add_device(device(guest, 1)).await.unwrap();
    assert!(matches!(store.add_device(device(guest, 2)).await, Err(Error::DeviceLimitReached)));
    store.revoke_device(first).await.unwrap();
    store.add_device(device(guest, 3)).await.unwrap();

    for n in 1..=5 {
        store.add_device(device(anna, n)).await.unwrap();
    }
    assert!(matches!(store.add_device(device(anna, 6)).await, Err(Error::DeviceLimitReached)));
}

#[tokio::test]
async fn a_users_own_limit_beats_the_default_of_the_role() {
    let (_dir, store) = open().await;
    let anna = store.create_profile("Anna", false).await.unwrap();
    store.set_profile_max_devices(anna, Some(1)).await.unwrap();

    store.add_device(device(anna, 1)).await.unwrap();
    assert!(matches!(store.add_device(device(anna, 2)).await, Err(Error::DeviceLimitReached)));

    store.set_profile_max_devices(anna, Some(2)).await.unwrap();
    store.add_device(device(anna, 2)).await.unwrap();
}

#[tokio::test]
async fn devices_belong_to_their_user() {
    let (_dir, store) = open().await;
    let anna = store.create_profile("Anna", false).await.unwrap();
    let ben = store.create_profile("Ben", false).await.unwrap();
    store.add_device(device(anna, 1)).await.unwrap();
    store.add_device(device(ben, 1)).await.unwrap();
    assert_eq!(store.devices_of(anna).await.unwrap().len(), 1);

    store.delete_profile(anna).await.unwrap();
    assert!(store.devices_of(anna).await.unwrap().is_empty(), "devices go with their user");
    assert_eq!(store.devices_of(ben).await.unwrap().len(), 1);
}

// Release and age rating

#[tokio::test]
async fn only_released_books_are_shown_and_admins_see_everything() {
    let (_dir, store) = open().await;
    let admin = store.create_profile("Admin", true).await.unwrap();
    let anna = store.create_profile("Anna", false).await.unwrap();
    let id = one_book(&store, "New").await;

    assert_eq!(titles(&store, admin).await, ["New"], "admins see unreleased books");
    assert!(titles(&store, anna).await.is_empty(), "default is hidden");

    store.release_book(id, 0).await.unwrap();
    assert_eq!(titles(&store, anna).await, ["New"]);

    store.unrelease_book(id).await.unwrap();
    assert!(titles(&store, anna).await.is_empty(), "withdrawing hides it at once");
}

#[tokio::test]
async fn the_age_rating_decides_who_sees_a_released_book() {
    let (_dir, store) = open().await;
    let adult = store.create_profile("Adult", false).await.unwrap(); // no age limit
    let small = store.create_user(NewProfile::child("Small", 6, daytime())).await.unwrap();
    let big = store.create_user(NewProfile::child("Big", 12, daytime())).await.unwrap();
    let id = one_book(&store, "Twelve").await;
    store.release_book(id, 12).await.unwrap();

    assert_eq!(titles(&store, adult).await, ["Twelve"]);
    assert_eq!(titles(&store, big).await, ["Twelve"]);
    assert!(titles(&store, small).await.is_empty());

    // Changing the limit changes what is shown at once.
    store.set_profile_max_age(small, Some(12)).await.unwrap();
    assert_eq!(titles(&store, small).await, ["Twelve"]);
}

#[tokio::test]
async fn an_exception_shows_a_book_to_one_user_or_a_role() {
    let (_dir, store) = open().await;
    let admin = store.create_profile("Admin", true).await.unwrap();
    let small = store.create_user(NewProfile::child("Small", 6, daytime())).await.unwrap();
    let other = store.create_user(NewProfile::child("Other", 6, daytime())).await.unwrap();
    // A guest with an age limit, so that only the exceptions can show the book.
    let guest = store
        .create_user(NewProfile { name: "Guest".into(), role: Role::Guest, max_age: Some(6), schedule: None })
        .await
        .unwrap();
    let id = one_book(&store, "Sixteen").await;
    store.release_book(id, 16).await.unwrap();
    assert!(titles(&store, small).await.is_empty());

    store.grant_release_to_user(id, small, Some(admin)).await.unwrap();
    assert_eq!(titles(&store, small).await, ["Sixteen"]);
    assert!(titles(&store, other).await.is_empty(), "only that user");

    // The same exception cannot be given twice.
    assert!(store.grant_release_to_user(id, small, None).await.is_err());

    store.grant_release_to_role(id, Role::Child, Some(admin)).await.unwrap();
    assert_eq!(titles(&store, other).await, ["Sixteen"], "now every child");
    assert!(titles(&store, guest).await.is_empty(), "guests are not affected by an exception for children");
}

#[tokio::test]
async fn an_exception_is_a_release_for_its_audience() {
    // It shows the book to that user even if it was never released in general.
    let (_dir, store) = open().await;
    let anna = store.create_profile("Anna", false).await.unwrap();
    let id = one_book(&store, "Special").await;
    assert!(titles(&store, anna).await.is_empty());
    store.grant_release_to_user(id, anna, None).await.unwrap();
    assert_eq!(titles(&store, anna).await, ["Special"]);
}

#[tokio::test]
async fn releasing_requires_an_age_rating_in_the_database() {
    let (dir, store) = open().await;
    let id = one_book(&store, "Unrated").await;

    // Straight into the database, around the API.
    let raw = Database::connect(format!("sqlite://{}?mode=rw", dir.path().join("david.db").display()))
        .await
        .unwrap();
    let r = raw.execute_unprepared(&format!("UPDATE book SET released = 1 WHERE id = {id}")).await;
    assert!(r.unwrap_err().to_string().contains("release_requires_age"));
}

#[tokio::test]
async fn the_recommendation_is_information_and_never_hides_a_book() {
    let (_dir, store) = open().await;
    let small = store.create_user(NewProfile::child("Small", 6, daytime())).await.unwrap();
    let id = one_book(&store, "Recommended").await;
    store.set_book_recommended_age(id, Some(14)).await.unwrap();
    store.release_book(id, 6).await.unwrap(); // the family decides, recommendation says 14

    let books = store.visible_books(small).await.unwrap();
    assert_eq!(books.len(), 1);
    assert_eq!((books[0].min_age, books[0].recommended_age), (Some(6), Some(14)));
}

// The weekly planner and the decision per book

#[tokio::test]
async fn access_follows_visibility_and_the_planner() {
    let (_dir, store) = open().await;
    let kid = store.create_user(NewProfile::child("Kid", 8, daytime())).await.unwrap();
    let hidden = one_book(&store, "Hidden").await; // never released
    let shown = one_book(&store, "Shown").await;
    store.release_book(shown, 6).await.unwrap();

    let monday = 0;
    let at = |h: u16, m: u16| h * 60 + m;
    assert_eq!(store.access(kid, shown, monday, at(8, 0)).await.unwrap(), Access::Allowed);
    assert_eq!(store.access(kid, shown, monday, at(18, 59)).await.unwrap(), Access::Allowed);
    assert_eq!(
        store.access(kid, shown, monday, at(19, 0)).await.unwrap(),
        Access::OutsideSchedule { next_start: Some((1, at(7, 0))) }
    );
    assert_eq!(
        store.access(kid, shown, 6, at(23, 0)).await.unwrap(),
        Access::OutsideSchedule { next_start: Some((0, at(7, 0))) },
        "Sunday night waits for Monday morning"
    );
    assert_eq!(store.access(kid, hidden, monday, at(8, 0)).await.unwrap(), Access::NotVisible);
}

#[tokio::test]
async fn content_outside_the_quota_ignores_the_planner_but_not_the_release() {
    let (_dir, store) = open().await;
    let kid = store.create_user(NewProfile::child("Kid", 8, daytime())).await.unwrap();
    let learning = one_book(&store, "Learning").await;
    store.set_book_quota_exempt(learning, true).await.unwrap();
    let night = 3 * 60;

    // Not released: exempt from the planner is not a way around the release.
    assert_eq!(store.access(kid, learning, 0, night).await.unwrap(), Access::NotVisible);

    store.release_book(learning, 6).await.unwrap();
    assert_eq!(store.access(kid, learning, 0, night).await.unwrap(), Access::Allowed);

    // Nor around the age rating.
    store.release_book(learning, 12).await.unwrap();
    assert_eq!(store.access(kid, learning, 0, night).await.unwrap(), Access::NotVisible);
}

#[tokio::test]
async fn users_without_a_planner_may_always_listen() {
    let (_dir, store) = open().await;
    let adult = store.create_profile("Adult", false).await.unwrap();
    let id = one_book(&store, "Any").await;
    store.release_book(id, 18).await.unwrap();
    assert_eq!(store.access(adult, id, 2, 3 * 60).await.unwrap(), Access::Allowed);

    // An exception does not lift the planner of a child.
    let kid = store.create_user(NewProfile::child("Kid", 6, daytime())).await.unwrap();
    store.grant_release_to_user(id, kid, None).await.unwrap();
    assert!(matches!(store.access(kid, id, 2, 3 * 60).await.unwrap(), Access::OutsideSchedule { .. }));
    assert_eq!(store.access(kid, id, 2, 10 * 60).await.unwrap(), Access::Allowed);
}

// Server identity, proposals and the import lock

#[tokio::test]
async fn the_server_id_is_created_once_and_survives_a_restart() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("david.db");
    let first = Store::open(&path).await.unwrap().server_id().await.unwrap();
    let second = Store::open(&path).await.unwrap().server_id().await.unwrap();
    assert_eq!(first, second);
    assert_eq!(first.len(), 36, "a UUID");

    let other = tempfile::TempDir::new().unwrap();
    let different = Store::open(&other.path().join("david.db")).await.unwrap().server_id().await.unwrap();
    assert_ne!(first, different);
}

#[tokio::test]
async fn proposed_values_are_marked_and_the_import_is_locked_by_default() {
    let (_dir, store) = open().await;

    assert_eq!(store.setting("import_enabled").await.unwrap().as_deref(), Some("0"));
    assert_eq!(store.setting("import_roots").await.unwrap().as_deref(), Some("[]"));

    // The role policy holds the proposals of the spec and none is confirmed yet.
    let policies = store.role_policies().await.unwrap();
    let find = |r: &str| policies.iter().find(|p| p.role == r).unwrap();
    assert_eq!((find("admin").max_devices, find("admin").session_ttl_s), (5, 30 * 24 * 3600));
    assert_eq!((find("child").max_devices, find("child").session_ttl_s), (3, 90 * 24 * 3600));
    assert_eq!((find("guest").max_devices, find("guest").session_ttl_s), (1, 24 * 3600));
    assert!(policies.iter().all(|p| !p.confirmed));

    // 4 role policies + 7 proposed settings. The decided settings are confirmed.
    assert_eq!(store.unconfirmed_defaults().await.unwrap(), 11);
}

#[tokio::test]
async fn upgrading_from_version_2_keeps_users_and_books_and_applies_the_new_rules() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("david.db");
    {
        let old = Database::connect(format!("sqlite://{}?mode=rwc", path.display())).await.unwrap();
        old.execute_unprepared(include_str!("../migrations/0001_init.sql")).await.unwrap();
        old.execute_unprepared(include_str!("../migrations/0002_profile_limit_150.sql")).await.unwrap();
        old.execute_unprepared("PRAGMA user_version = 2").await.unwrap();
        old.execute_unprepared(
            "INSERT INTO profile (name, is_admin, created_at) VALUES ('Boss', 1, 0), ('Anna', 0, 0);
             INSERT INTO author (name) VALUES ('Old Author');
             INSERT INTO book (title, author_id, folder, min_age, added_at) VALUES ('Old', 1, 'Old Author/Old', 6, 0);",
        )
        .await
        .unwrap();
    }

    let store = Store::open(&path).await.unwrap();
    let all = store.list_profiles().await.unwrap();
    let role = |n: &str| all.iter().find(|p| p.name == n).unwrap().role.clone();
    assert_eq!((role("Boss").as_str(), role("Anna").as_str()), ("admin", "member"));

    // The old book is kept, but nothing was released: default is hidden.
    let anna = all.iter().find(|p| p.name == "Anna").unwrap().id;
    let boss = all.iter().find(|p| p.name == "Boss").unwrap().id;
    assert!(titles(&store, anna).await.is_empty());
    assert_eq!(titles(&store, boss).await, ["Old"]);
    // The last-admin rule applies to the migrated data as well.
    assert!(matches!(store.delete_profile(boss).await, Err(Error::LastAdmin)));
}

/// Documents a rule that may surprise: a user without an age limit sees every
/// released book, whatever the role. For adults this is intended. For a guest it
/// probably is not, which is an open decision in the spec.
#[tokio::test]
async fn a_user_without_an_age_limit_sees_every_released_book() {
    let (_dir, store) = open().await;
    let guest = store.create_user(NewProfile::new("Guest", Role::Guest)).await.unwrap();
    let id = one_book(&store, "Adults only").await;
    store.release_book(id, 18).await.unwrap();
    assert_eq!(titles(&store, guest).await, ["Adults only"]);
}
