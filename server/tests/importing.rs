// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The import: locked by default, unlocked by defined criteria, ratings by rule.

mod common;

use common::audio::{ffmpeg_available, make_mp3};
use common::open;
use david_server::{Error, RatingRule, Store};
use std::path::{Path, PathBuf};

fn root_with_books() -> tempfile::TempDir {
    tempfile::TempDir::new().unwrap()
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

async fn ids(store: &Store, folder: &str) -> i64 {
    store.find_book(folder).await.unwrap().unwrap_or_else(|| panic!("book {folder} missing")).id
}

#[tokio::test]
async fn the_import_is_locked_by_default() {
    let (_dir, store) = open().await;
    let state = store.import_state().await.unwrap();
    assert!(!state.enabled && !state.criteria_defined && state.roots.is_empty());
    assert!(matches!(store.run_import().await, Err(Error::ImportLocked)));
    assert!(store.list_profiles().await.unwrap().is_empty(), "and nothing else happened");
}

#[tokio::test]
async fn the_criteria_are_checked_when_they_are_saved() {
    let (_dir, store) = open().await;
    let lib = root_with_books();
    let file = lib.path().join("a-file");
    std::fs::write(&file, b"x").unwrap();

    for (what, roots) in [
        ("relative", vec!["library/books".to_owned()]),
        ("missing", vec![s(&lib.path().join("nope"))]),
        ("a file", vec![s(&file)]),
        ("the filesystem root", vec!["/".to_owned()]),
    ] {
        let r = store.save_import_criteria(&roots, RatingRule::Manual).await;
        assert!(matches!(r, Err(Error::InvalidImportRoot(_))), "{what}: {r:?}");
    }
    assert!(matches!(store.save_import_criteria(&[], RatingRule::Manual).await, Err(Error::ImportCriteriaMissing)));
    assert!(matches!(store.save_import_criteria(&["  ".into()], RatingRule::Manual).await, Err(Error::ImportCriteriaMissing)));

    // Nothing invalid was stored.
    assert!(!store.import_state().await.unwrap().criteria_defined);
}

#[tokio::test]
async fn roots_are_stored_in_their_canonical_form_without_duplicates() {
    let (_dir, store) = open().await;
    let lib = root_with_books();
    std::fs::create_dir_all(lib.path().join("books")).unwrap();
    let plain = s(&lib.path().join("books"));
    let roundabout = format!("{}/books/../books/", lib.path().display());

    store.save_import_criteria(&[plain.clone(), roundabout], RatingRule::Manual).await.unwrap();
    let state = store.import_state().await.unwrap();
    assert_eq!(state.roots, [s(&std::fs::canonicalize(&plain).unwrap())], "one root, no `..`");
    assert!(state.criteria_defined);
}

#[tokio::test]
async fn unlocking_needs_the_criteria_and_locking_works_again() {
    let (_dir, store) = open().await;
    assert!(matches!(store.unlock_import("Boss").await, Err(Error::ImportCriteriaMissing)));
    assert!(!store.import_state().await.unwrap().enabled);

    let lib = root_with_books();
    store.save_import_criteria(&[s(lib.path())], RatingRule::Manual).await.unwrap();
    assert!(!store.import_state().await.unwrap().enabled, "defining the criteria does not unlock");
    store.unlock_import("Boss").await.unwrap();
    let state = store.import_state().await.unwrap();
    assert!(state.enabled);
    assert_eq!(state.unlocked_by.as_deref(), Some("Boss"));
    assert!(state.unlocked_at.is_some());
    store.run_import().await.expect("unlocked, and an empty root is fine");

    store.lock_import().await.unwrap();
    assert!(matches!(store.run_import().await, Err(Error::ImportLocked)));
    assert!(store.import_state().await.unwrap().criteria_defined, "the criteria stay when locked");
}

#[tokio::test]
async fn an_import_reads_only_the_roots_and_everything_arrives_unreleased() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let (_dir, store) = open().await;
    let inside = root_with_books();
    let outside = root_with_books();
    make_mp3(&inside.path().join("Author/Book/Author_Book_CD01_TR01.mp3"), &[]);
    make_mp3(&outside.path().join("Other/Secret/x_CD01_TR01.mp3"), &[]);

    store.save_import_criteria(&[s(inside.path())], RatingRule::Manual).await.unwrap();
    store.unlock_import("Boss").await.unwrap();
    let report = store.run_import().await.unwrap();
    assert_eq!((report.books, report.tracks_added, report.books_released), (1, 1, 0));

    assert!(store.find_book("Author/Book").await.unwrap().is_some());
    assert!(store.find_book("Other/Secret").await.unwrap().is_none(), "a folder outside the roots is never read");
    let book = store.find_book("Author/Book").await.unwrap().unwrap();
    assert!(!book.released, "nothing is shown until an administrator releases it");
}

#[tokio::test]
async fn the_recommendation_rule_releases_only_new_books_that_carry_a_recommendation() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let (_dir, store) = open().await;
    let lib = root_with_books();
    make_mp3(&lib.path().join("A/Rated/a_CD01_TR01.mp3"), &[("AGE_RECOMMENDATION", "8")]);
    make_mp3(&lib.path().join("A/Plain/b_CD01_TR01.mp3"), &[]);
    store.save_import_criteria(&[s(lib.path())], RatingRule::Recommendation).await.unwrap();
    store.unlock_import("Boss").await.unwrap();

    let report = store.run_import().await.unwrap();
    assert_eq!((report.books, report.books_released), (2, 1));
    let rated = store.find_book("A/Rated").await.unwrap().unwrap();
    assert_eq!((rated.released, rated.min_age, rated.recommended_age), (true, Some(8), Some(8)));
    let plain = store.find_book("A/Plain").await.unwrap().unwrap();
    assert_eq!((plain.released, plain.recommended_age), (false, None), "no recommendation, no release");

    // An administrator withdraws the book; the next import must not release it again.
    store.unrelease_book(rated.id).await.unwrap();
    let again = store.run_import().await.unwrap();
    assert_eq!(again.books_released, 0);
    assert!(!store.find_book("A/Rated").await.unwrap().unwrap().released);
}

#[tokio::test]
async fn with_the_manual_rule_the_recommendation_is_kept_but_nothing_is_released() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let (_dir, store) = open().await;
    let lib = root_with_books();
    make_mp3(&lib.path().join("A/Rated/a_CD01_TR01.mp3"), &[("AGE_RECOMMENDATION", "12")]);
    store.save_import_criteria(&[s(lib.path())], RatingRule::Manual).await.unwrap();
    store.unlock_import("Boss").await.unwrap();
    store.run_import().await.unwrap();

    let book = store.find_book("A/Rated").await.unwrap().unwrap();
    assert_eq!((book.released, book.recommended_age), (false, Some(12)), "shown to the administrator as the starting value");
    let id = ids(&store, "A/Rated").await;
    store.release_book(id, 12).await.unwrap();
}

#[tokio::test]
async fn a_root_that_has_gone_missing_is_skipped() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let (_dir, store) = open().await;
    let kept = root_with_books();
    let gone = root_with_books();
    make_mp3(&kept.path().join("A/Book/a_CD01_TR01.mp3"), &[]);
    store.save_import_criteria(&[s(kept.path()), s(gone.path())], RatingRule::Manual).await.unwrap();
    store.unlock_import("Boss").await.unwrap();
    let gone_path: PathBuf = gone.path().to_owned();
    drop(gone); // removes the folder
    assert!(!gone_path.exists());

    let report = store.run_import().await.expect("one missing disk must not stop the import");
    assert_eq!(report.books, 1);
}

#[tokio::test]
async fn a_renamed_file_keeps_its_track_and_one_bad_album_does_not_stop_the_rest() {
    let (_dir, store) = open().await;
    let first = common::book("Ken Follett", "Winter der Welt", &[3]);
    store.import_books(vec![first.clone()]).await.unwrap();
    let book = store.find_book("Ken Follett/Winter der Welt").await.unwrap().unwrap();
    let before: Vec<(i64, String)> = store.tracks_of(book.id).await.unwrap().into_iter().map(|t| (t.id, t.file)).collect();
    assert_eq!(before.len(), 3);

    // The files are renamed (the same discs and numbers under other names), as after fixing a typo.
    let mut renamed = first.clone();
    for t in &mut renamed.tracks {
        t.file = t.file.replace("Winter der Welt", "Winter der Weld");
        t.mtime += 1;
    }
    let report = store.import_books(vec![renamed]).await.unwrap();
    assert_eq!((report.tracks_added, report.tracks_removed, report.books_failed), (0, 0, 0), "{report:?}");
    assert_eq!(report.tracks_updated, 3);
    let after: Vec<(i64, String)> = store.tracks_of(book.id).await.unwrap().into_iter().map(|t| (t.id, t.file)).collect();
    assert_eq!(after.iter().map(|(id, _)| *id).collect::<Vec<_>>(), before.iter().map(|(id, _)| *id).collect::<Vec<_>>(), "the rows stay");
    assert!(after.iter().all(|(_, f)| f.contains("Weld")));

    // An album with two files of the same number cannot be imported; the next one still is.
    let mut broken = common::book("Dania König", "Versprochen", &[2]);
    broken.tracks[1].number = broken.tracks[0].number;
    let good = common::book("Ken Follett", "Kinder der Freiheit", &[2]);
    let report = store.import_books(vec![broken, good]).await.unwrap();
    assert_eq!(report.books_failed, 1);
    assert!(store.find_book("Ken Follett/Kinder der Freiheit").await.unwrap().is_some());
    assert_eq!(store.tracks_of(store.find_book("Ken Follett/Kinder der Freiheit").await.unwrap().unwrap().id).await.unwrap().len(), 2);
}
