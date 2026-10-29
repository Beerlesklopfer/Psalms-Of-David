// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later

mod common;

use common::{book, open};

async fn with_genre(store: &david_server::Store, title: &str, tag: Option<&str>) {
    let mut b = book("Artist", title, &[1]);
    b.kind = "music".into();
    b.genre = tag.map(str::to_owned);
    store.import_books(vec![b]).await.unwrap();
}

#[tokio::test]
async fn a_tag_text_gets_its_genre_through_the_alias_and_the_album_follows() {
    let (_d, store) = open().await;
    with_genre(&store, "One", Some("Prog Rock")).await;
    with_genre(&store, "Two", Some("prog rock")).await;
    with_genre(&store, "Three", None).await;

    let loose = store.loose_genre_tags().await.unwrap();
    assert_eq!(loose.len(), 1);
    assert_eq!(loose[0].albums, 2);

    let rock = store.add_genre("Rock").await.unwrap();
    store.assign_genre_tag("Prog Rock", rock).await.unwrap();
    let list = store.genres().await.unwrap();
    assert_eq!(list[0].name, "Rock");
    assert_eq!(list[0].albums, 2);
    assert!(store.loose_genre_tags().await.unwrap().is_empty());
    assert_eq!(store.albums_without_genre().await.unwrap(), 1);
}

#[tokio::test]
async fn a_genre_chosen_by_hand_survives_the_tags_until_handed_back() {
    let (_d, store) = open().await;
    with_genre(&store, "One", Some("Pop")).await;
    let pop = store.add_genre("Pop").await.unwrap();
    let jazz = store.add_genre("Jazz").await.unwrap();
    store.assign_genre_tag("Pop", pop).await.unwrap();
    let id = david_server_book_id(&store).await;

    store.set_book_genre(id, Some(jazz), false).await.unwrap();
    store.assign_genre_tag("Pop", pop).await.unwrap();
    with_genre(&store, "One", Some("Pop")).await; // a rescan
    let list = store.genres().await.unwrap();
    assert_eq!(list.iter().find(|g| g.name == "Jazz").unwrap().albums, 1);
    assert_eq!(list.iter().find(|g| g.name == "Pop").unwrap().albums, 0);

    store.set_book_genre(id, None, true).await.unwrap();
    let list = store.genres().await.unwrap();
    assert_eq!(list.iter().find(|g| g.name == "Pop").unwrap().albums, 1);
}

async fn david_server_book_id(store: &david_server::Store) -> i64 {
    store.find_book("Artist/One").await.unwrap().unwrap().id
}

#[tokio::test]
async fn the_list_can_be_renamed_ordered_and_a_deleted_genre_frees_its_tags() {
    let (_d, store) = open().await;
    let a = store.add_genre("Alpha").await.unwrap();
    let b = store.add_genre("Beta").await.unwrap();
    assert_eq!(store.add_genre("alpha").await.unwrap(), a);
    store.move_genre(b, -1).await.unwrap();
    let names: Vec<_> = store.genres().await.unwrap().into_iter().map(|g| g.name).collect();
    assert_eq!(names, ["Beta", "Alpha"]);
    store.rename_genre(a, "Gamma").await.unwrap();
    assert!(store.rename_genre(a, "Beta").await.is_err());
    assert!(store.add_genre("  ").await.is_err());

    with_genre(&store, "One", Some("Beat")).await;
    store.assign_genre_tag("Beat", a).await.unwrap();
    assert!(store.loose_genre_tags().await.unwrap().is_empty());
    store.delete_genre(a).await.unwrap();
    assert_eq!(store.loose_genre_tags().await.unwrap().len(), 1);
}
