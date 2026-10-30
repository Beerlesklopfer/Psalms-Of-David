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

#[tokio::test]
async fn tags_come_first_but_a_placeholder_tag_is_no_tag_and_the_words_are_a_setting() {
    if !ffmpeg_available() {
        return;
    }
    let (_d, store) = open().await;
    let lib = tempfile::TempDir::new().unwrap();
    make_mp3(&lib.path().join("Folder Artist/Folder Title/01.mp3"), &[("artist", "Kein Künstler"), ("album", "Kein Titel")]);
    make_mp3(&lib.path().join("Other Artist/Other Title/01.mp3"), &[("artist", "Tag Artist"), ("album", "Tag Title (CD 2)")]);
    store.scan(lib.path()).await.unwrap();

    // The placeholders are ignored (the folder speaks); a real tag wins over the folder, without its CD number.
    let a = store.find_book("Folder Artist/Folder Title").await.unwrap().unwrap();
    let a = store.admin_book_row(a.id).await.unwrap().unwrap();
    assert_eq!((a.author.as_str(), a.title.as_str()), ("Folder Artist", "Folder Title"));
    let b = store.find_book("Other Artist/Other Title").await.unwrap().unwrap();
    let b = store.admin_book_row(b.id).await.unwrap().unwrap();
    assert_eq!((b.author.as_str(), b.title.as_str()), ("Tag Artist", "Tag Title"));

    // The words are a setting: with another list "Kein Künstler" is a name like any other.
    store.save_settings(&[("placeholder_tags".to_owned(), "nothing".to_owned())]).await.unwrap();
    store.scan(lib.path()).await.unwrap();
    let a = store.find_book("Folder Artist/Folder Title").await.unwrap().unwrap();
    let a = store.admin_book_row(a.id).await.unwrap().unwrap();
    assert_eq!((a.author.as_str(), a.title.as_str()), ("Kein Künstler", "Kein Titel"));
}

#[tokio::test]
async fn the_composer_tag_is_a_narrator_only_for_audiobooks() {
    if !ffmpeg_available() {
        return;
    }
    let (_d, store) = open().await;
    let (books, music) = (tempfile::TempDir::new().unwrap(), tempfile::TempDir::new().unwrap());
    make_mp3(&books.path().join("A/Book/01.mp3"), &[("composer", "Reader")]);
    make_mp3(&music.path().join("B/Album/01.mp3"), &[("composer", "Composer")]);
    store.scan_kind(books.path(), "audiobook").await.unwrap();
    store.scan_kind(music.path(), "music").await.unwrap();
    let narrator = |folder: &'static str| {
        let store = store.clone();
        async move {
            let id = store.find_book(folder).await.unwrap().unwrap().id;
            store.admin_book_row(id).await.unwrap().unwrap().narrator
        }
    };
    assert_eq!(narrator("A/Book").await.as_deref(), Some("Reader"));
    assert_eq!(narrator("B/Album").await, None);
}

/// A file that is only an ID3 tag and no sound (what is left of a copy that went wrong).
fn write_tag_without_sound(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut bytes = b"ID3\x04\x00\x00\x00\x00\x00\x40".to_vec(); // version 2.4, 64 bytes of tag
    bytes.extend(vec![0u8; 64]);
    std::fs::write(path, bytes).unwrap();
}

#[tokio::test]
async fn one_file_without_sound_is_left_out_and_said_and_the_rest_of_the_album_is_imported() {
    if !ffmpeg_available() {
        return;
    }
    let (_d, store) = open().await;
    let lib = tempfile::TempDir::new().unwrap();
    make_mp3(&lib.path().join("Band/Album/01.mp3"), &[("album", "Album"), ("artist", "Band"), ("title", "One")]);
    make_mp3(&lib.path().join("Band/Album/03.mp3"), &[("album", "Album"), ("artist", "Band"), ("title", "Three")]);
    write_tag_without_sound(&lib.path().join("Band/Album/02.mp3"));
    // A folder with nothing readable at all is no album, as before, and says why.
    write_tag_without_sound(&lib.path().join("Band/Empty/01.mp3"));

    let report = store.scan(lib.path()).await.unwrap();
    let book = store.find_book("Band/Album").await.unwrap().expect("the album is there although one file is not");
    assert_eq!(store.tracks_of(book.id).await.unwrap().len(), 2, "the two readable files");
    assert!(store.find_book("Band/Empty").await.unwrap().is_none());
    assert!(report.problems.iter().any(|p| p.contains("02.mp3") && p.contains("left out")), "{:?}", report.problems);
    assert!(report.problems.iter().any(|p| p.contains("Empty")), "{:?}", report.problems);

    // Taking the file away changes nothing for the album; a second scan is quiet about the tracks.
    std::fs::remove_file(lib.path().join("Band/Album/02.mp3")).unwrap();
    let again = store.scan(lib.path()).await.unwrap();
    assert_eq!((again.tracks_added, again.tracks_removed), (0, 0), "{again:?}");
}

#[tokio::test]
async fn the_import_takes_albums_out_whose_files_are_gone_and_stops_for_a_disk_that_is_not_there() {
    if !ffmpeg_available() {
        return;
    }
    let (_dir, store) = open().await;
    let lib = tempfile::tempdir().unwrap();
    for n in 1..=8 {
        make_mp3(&lib.path().join(format!("Autor/Buch {n}/Autor_Buch_CD01_TR01.mp3")), &[]);
    }
    store.save_import_criteria(&[s(lib.path())], RatingRule::Manual).await.unwrap();
    store.create_profile("Boss", true).await.unwrap();
    store.unlock_import("Boss").await.unwrap();
    assert_eq!(store.run_import().await.unwrap().books, 8);
    let one = store.find_book("Autor/Buch 1").await.unwrap().unwrap().id;
    let two = store.find_book("Autor/Buch 2").await.unwrap().unwrap().id;
    store.set_book_labels(one, &["Schule".into()]).await.unwrap();

    // One album gone from the disk: the next import takes it out, with what hung on it; the others stay and nothing is removed twice.
    std::fs::remove_dir_all(lib.path().join("Autor/Buch 1")).unwrap();
    let r = store.run_import().await.unwrap();
    assert_eq!((r.books_removed, r.books), (1, 7), "{r:?}");
    assert!(store.find_book("Autor/Buch 1").await.unwrap().is_none() && store.labels().await.unwrap().is_empty(), "labels go with the album");
    assert!(store.find_book("Autor/Buch 2").await.unwrap().is_some());
    assert_eq!(store.run_import().await.unwrap().books_removed, 0);

    // A look first says what would go and changes nothing.
    std::fs::remove_dir_all(lib.path().join("Autor/Buch 2")).unwrap();
    let look = store.prune_missing(true, false).await.unwrap();
    assert_eq!((look.removed.len(), look.removed[0].folder.as_str()), (1, "Autor/Buch 2"));
    assert!(store.find_book("Autor/Buch 2").await.unwrap().is_some(), "a dry run removes nothing");
    let _ = two;

    // Many albums gone at once looks like a disk that is not mounted: nothing is removed by itself, with a force it is.
    for n in 3..=8 {
        std::fs::remove_dir_all(lib.path().join(format!("Autor/Buch {n}"))).unwrap();
    }
    std::fs::write(lib.path().join("Autor/lies-mich.txt"), b"x").unwrap();
    let held = store.prune_missing(false, false).await.unwrap();
    assert_eq!((held.removed.len(), held.held_back.len()), (0, 7), "{held:?}");
    assert!(held.problems.iter().any(|p| p.contains("too many")), "{held:?}");
    assert_eq!(store.prune_missing(false, true).await.unwrap().removed.len(), 7);

    // An import folder that is empty (a disk that is not mounted) stops the tidying of its kind, even for one album.
    make_mp3(&lib.path().join("Autor/Neu/Autor_Neu_CD01_TR01.mp3"), &[]);
    store.run_import().await.unwrap();
    for e in std::fs::read_dir(lib.path()).unwrap() {
        std::fs::remove_dir_all(e.unwrap().path()).ok();
    }
    let r = store.prune_missing(false, true).await.unwrap();
    assert!(r.removed.is_empty() && r.problems.iter().any(|p| p.contains("missing or empty")), "{r:?}");
    assert!(store.find_book("Autor/Neu").await.unwrap().is_some());
}

#[cfg(unix)]
#[tokio::test]
async fn a_folder_that_cannot_be_read_keeps_its_albums_and_a_problem_of_the_scan_does_not_stop_the_tidying() {
    use std::os::unix::fs::PermissionsExt;
    if !ffmpeg_available() {
        return;
    }
    let (_dir, store) = open().await;
    let lib = tempfile::tempdir().unwrap();
    for n in ["Offen", "Zu", "Weg"] {
        make_mp3(&lib.path().join(format!("Autor/{n}/Autor_{n}_CD01_TR01.mp3")), &[]);
    }
    make_mp3(&lib.path().join("Anderer/Eins/Anderer_Eins_CD01_TR01.mp3"), &[]);
    make_mp3(&lib.path().join("Anderer/Zwei/Anderer_Zwei_CD01_TR01.mp3"), &[]);
    store.save_import_criteria(&[s(lib.path())], RatingRule::Manual).await.unwrap();
    store.create_profile("Boss", true).await.unwrap();
    store.unlock_import("Boss").await.unwrap();
    store.run_import().await.unwrap();

    std::fs::remove_dir_all(lib.path().join("Autor/Weg")).unwrap();
    std::fs::set_permissions(lib.path().join("Autor/Zu"), std::fs::Permissions::from_mode(0o000)).unwrap();
    let root_user = std::fs::metadata(lib.path()).map(|m| std::os::unix::fs::MetadataExt::uid(&m) == 0).unwrap_or(false);
    let r = store.run_import().await.unwrap();
    std::fs::set_permissions(lib.path().join("Autor/Zu"), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(store.find_book("Autor/Weg").await.unwrap().is_none(), "what is not found is removed, although the scan had a problem: {r:?}");
    assert!(store.find_book("Autor/Zu").await.unwrap().is_some(), "an album whose folder cannot be read stays");
    assert!(store.find_book("Autor/Offen").await.unwrap().is_some());
    if !root_user {
        assert!(!r.problems.is_empty(), "the unreadable folder is a problem of the scan");
    }
}
