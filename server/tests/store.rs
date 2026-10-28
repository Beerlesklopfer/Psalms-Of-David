// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
mod common;

use common::{book, open};
use david_server::{Error, MAX_PROFILES, ProgressUpdate, Store, now_ms};

/// Imports one book and returns (book id, ids of its tracks).
async fn with_book(store: &Store, per_disc: &[usize]) -> (i64, Vec<i64>) {
    store.import_books(vec![book("Ken Follett", "Kinder der Freiheit", per_disc)]).await.unwrap();
    let b = store.find_book("Ken Follett/Kinder der Freiheit").await.unwrap().unwrap();
    let tracks = store.tracks_of(b.id).await.unwrap();
    (b.id, tracks.into_iter().map(|t| t.id).collect())
}

fn update(profile_id: i64, book_id: i64, track_id: i64, position_ms: i64, updated_at: i64) -> ProgressUpdate {
    ProgressUpdate { profile_id, book_id, track_id, position_ms, finished: false, updated_at }
}

#[tokio::test]
async fn profile_limit_is_150() {
    assert_eq!(MAX_PROFILES, 150, "the constant must match migration 0002");
    let (_dir, store) = open().await;
    for i in 0..MAX_PROFILES {
        store.create_profile(&format!("person {i}"), i == 0).await.unwrap();
    }
    let r = store.create_profile("one too many", false).await;
    assert!(matches!(r, Err(Error::ProfileLimitReached)), "got {r:?}");
    assert_eq!(store.list_profiles().await.unwrap().len() as u64, MAX_PROFILES);
}

/// A database created with migration 0001 (limit 25) must accept up to 150
/// profiles after the server has started once with 0002.
#[tokio::test]
async fn upgrading_from_version_1_raises_the_limit() {
    use sea_orm::{ConnectionTrait, Database};

    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("david.db");
    {
        // Simulate an old installation: only migration 0001 applied.
        let old = Database::connect(format!("sqlite://{}?mode=rwc", path.display())).await.unwrap();
        old.execute_unprepared(include_str!("../migrations/0001_init.sql")).await.unwrap();
        old.execute_unprepared("PRAGMA user_version = 1").await.unwrap();
        for i in 0..25 {
            old.execute_unprepared(&format!(
                "INSERT INTO profile (name, created_at) VALUES ('old {i}', 0)"
            ))
            .await
            .unwrap();
        }
        let blocked = old
            .execute_unprepared("INSERT INTO profile (name, created_at) VALUES ('26th', 0)")
            .await;
        assert!(blocked.is_err(), "version 1 must still stop at 25");
    }

    let store = Store::open(&path).await.unwrap();
    for i in 25..MAX_PROFILES {
        store.create_profile(&format!("new {i}"), false).await.unwrap();
    }
    assert!(matches!(store.create_profile("one too many", false).await, Err(Error::ProfileLimitReached)));
}

#[tokio::test]
async fn profile_names_are_unique_ignoring_case() {
    let (_dir, store) = open().await;
    store.create_profile("Anna", false).await.unwrap();
    assert!(store.create_profile("anna", false).await.is_err());
}

#[tokio::test]
async fn deleting_a_profile_removes_its_progress() {
    let (_dir, store) = open().await;
    let (book_id, tracks) = with_book(&store, &[3]).await;
    let anna = store.create_profile("Anna", false).await.unwrap();
    store.save_progress(update(anna, book_id, tracks[1], 5_000, now_ms())).await.unwrap();
    store.flush().await.unwrap();
    assert!(store.progress_of(anna, book_id).await.unwrap().is_some());

    store.delete_profile(anna).await.unwrap();
    assert!(store.progress_of(anna, book_id).await.unwrap().is_none());
}

#[tokio::test]
async fn an_older_update_never_overwrites_a_newer_one() {
    let (_dir, store) = open().await;
    let (book_id, tracks) = with_book(&store, &[3]).await;
    let anna = store.create_profile("Anna", false).await.unwrap();

    // Out of order inside one batch.
    store.save_progress(update(anna, book_id, tracks[2], 5_000, 2_000)).await.unwrap();
    store.save_progress(update(anna, book_id, tracks[0], 100, 1_000)).await.unwrap();
    store.flush().await.unwrap();
    let p = store.progress_of(anna, book_id).await.unwrap().unwrap();
    assert_eq!((p.track_id, p.position_ms), (tracks[2], 5_000));

    // Out of order across batches: the database refuses the older write.
    store.save_progress(update(anna, book_id, tracks[0], 100, 1_500)).await.unwrap();
    store.flush().await.unwrap();
    let p = store.progress_of(anna, book_id).await.unwrap().unwrap();
    assert_eq!((p.track_id, p.position_ms), (tracks[2], 5_000));
}

#[tokio::test]
async fn a_burst_for_one_book_is_coalesced_into_one_write() {
    let (_dir, store) = open().await;
    let (book_id, tracks) = with_book(&store, &[3]).await;
    let anna = store.create_profile("Anna", false).await.unwrap();

    for i in 0..1_000 {
        store.save_progress(update(anna, book_id, tracks[i % 3], i as i64, 10_000 + i as i64)).await.unwrap();
    }
    store.flush().await.unwrap();

    let stats = store.writer_stats();
    let (received, written) = (
        stats.received.load(std::sync::atomic::Ordering::Relaxed),
        stats.written.load(std::sync::atomic::Ordering::Relaxed),
    );
    assert_eq!(received, 1_000);
    assert!(written < 10, "expected few writes, got {written}");
    let p = store.progress_of(anna, book_id).await.unwrap().unwrap();
    assert_eq!(p.position_ms, 999);
}

#[tokio::test]
async fn continue_listening_is_recent_first_and_skips_finished_books() {
    let (_dir, store) = open().await;
    store
        .import_books(vec![book("A", "One", &[2]), book("A", "Two", &[2]), book("A", "Three", &[2])])
        .await
        .unwrap();
    let anna = store.create_profile("Anna", false).await.unwrap();
    for (i, folder) in ["A/One", "A/Two", "A/Three"].iter().enumerate() {
        let b = store.find_book(folder).await.unwrap().unwrap();
        store.release_book(b.id, 0).await.unwrap(); // only released books are shown
        let t = store.tracks_of(b.id).await.unwrap()[0].id;
        let mut u = update(anna, b.id, t, 1_000, 1_000 * (i as i64 + 1));
        u.finished = i == 2; // "Three" is finished
        store.save_progress(u).await.unwrap();
    }
    store.flush().await.unwrap();

    let list = store.continue_listening(anna, 10).await.unwrap();
    assert_eq!(list.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), ["Two", "One"]);
}

#[tokio::test]
async fn importing_twice_changes_nothing_and_vanished_files_are_removed() {
    let (_dir, store) = open().await;
    let first = store.import_books(vec![book("A", "Book", &[3])]).await.unwrap();
    assert_eq!((first.books, first.tracks_added), (1, 3));

    let second = store.import_books(vec![book("A", "Book", &[3])]).await.unwrap();
    assert_eq!((second.tracks_added, second.tracks_updated, second.tracks_unchanged), (0, 0, 3));

    let third = store.import_books(vec![book("A", "Book", &[2])]).await.unwrap();
    assert_eq!((third.tracks_unchanged, third.tracks_removed), (2, 1));
    let b = store.find_book("A/Book").await.unwrap().unwrap();
    assert_eq!(store.tracks_of(b.id).await.unwrap().len(), 2);
}

#[tokio::test]
async fn data_survives_reopening_and_migrations_run_only_once() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("david.db");
    {
        let store = Store::open(&path).await.unwrap();
        store.create_profile("Anna", true).await.unwrap();
    }
    let store = Store::open(&path).await.unwrap();
    let names: Vec<_> = store.list_profiles().await.unwrap().into_iter().map(|p| p.name).collect();
    assert_eq!(names, ["Anna"]);
}

/// Scans a real library. Run with `DAVID_LIBRARY=~/Musik/Hörbücher cargo test -- --ignored --nocapture`.
#[tokio::test]
#[ignore = "needs DAVID_LIBRARY"]
async fn scans_a_real_library() {
    let Ok(root) = std::env::var("DAVID_LIBRARY") else { panic!("DAVID_LIBRARY is not set") };
    let (_dir, store) = open().await;
    let report = store.scan(std::path::Path::new(&root)).await.unwrap();
    println!("{report:?}");

    let adult = store.create_profile("Adult", true).await.unwrap();
    for b in store.visible_books(adult).await.unwrap() {
        println!("{} / {}: {} discs, {} tracks, {} min", b.author, b.title, b.discs, b.tracks, b.duration_ms / 60_000);
    }
    assert!(report.books >= 1);
    let again = store.scan(std::path::Path::new(&root)).await.unwrap();
    assert_eq!((again.tracks_added, again.tracks_updated), (0, 0), "second scan must be a no-op");
}
