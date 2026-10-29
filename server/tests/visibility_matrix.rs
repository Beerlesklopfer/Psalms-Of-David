// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The matrix of the media list (schema 0039): a book is opened to a member of the family or closed to them, whatever release, age and
//! exceptions of the role say; administrators see everything.

mod common;

use common::{book, open};
use david_server::{NewProfile, Role};

#[tokio::test]
async fn a_book_is_opened_to_one_member_and_closed_to_another_whatever_the_release_says() {
    let (_dir, store) = open().await;
    let admin = store.create_profile("Boss", true).await.unwrap();
    let anna = store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let ben = store.create_user(NewProfile::new("Ben", Role::Member)).await.unwrap();
    let mut ids = Vec::new();
    for (author, title) in [("Tolkien", "Der Hobbit"), ("Follett", "Winter der Welt")] {
        let b = book(author, title, &[1]);
        let folder = b.folder.clone();
        store.import_books(vec![b]).await.unwrap();
        ids.push(store.find_book(&folder).await.unwrap().unwrap().id);
    }
    let (hobbit, winter) = (ids[0], ids[1]);
    let sees = |p: i64, b: i64| {
        let store = store.clone();
        async move { store.visible_books(p).await.unwrap().iter().any(|r| r.id == b) }
    };
    // The Hobbit is released for everybody; Winter is not released at all.
    store.release_book(hobbit, 0).await.unwrap();
    assert!(sees(anna, hobbit).await && sees(ben, hobbit).await && !sees(anna, winter).await);

    // Closed to Anna: she does not see it, Ben still does, the administrator is not closed out.
    assert!(store.set_book_visible_for(hobbit, anna, false, Some(admin)).await.unwrap());
    assert!(!sees(anna, hobbit).await && sees(ben, hobbit).await && sees(admin, hobbit).await, "an administrator is not closed out by a box of Anna's");
    // Opened to Anna again: the closing goes, no exception is needed because the release shows it.
    assert!(store.set_book_visible_for(hobbit, anna, true, Some(admin)).await.unwrap());
    assert!(sees(anna, hobbit).await);
    // Winter is opened to Ben only (an exception for him), and closed again.
    assert!(store.set_book_visible_for(winter, ben, true, Some(admin)).await.unwrap());
    assert!(sees(ben, winter).await && !sees(anna, winter).await);
    assert!(store.set_book_visible_for(winter, ben, false, Some(admin)).await.unwrap());
    assert!(!sees(ben, winter).await);
    // A closing wins over an exception of the role, too.
    store.grant_release_to_role(winter, Role::Member, Some(admin)).await.unwrap();
    assert!(sees(anna, winter).await && !sees(ben, winter).await, "Ben stays closed out, Anna gets the exception of the role");

    // The matrix: a column for every member that is no administrator, the books each one sees.
    let m: Vec<_> = store.visibility_matrix().await.unwrap().into_iter().filter(|(_, _, _, admin)| !admin).collect();
    assert_eq!(m.iter().map(|(_, n, _, _)| n.as_str()).collect::<Vec<_>>(), ["Anna", "Ben"]);
    let of = |name: &str| m.iter().find(|(_, n, _, _)| n == name).unwrap().2.clone();
    assert!(of("Anna").contains(&hobbit) && of("Anna").contains(&winter));
    assert!(of("Ben").contains(&hobbit) && !of("Ben").contains(&winter));

    // Unknown books and unknown profiles are refused.
    assert!(!store.set_book_visible_for(9999, anna, true, None).await.unwrap());
    assert!(!store.set_book_visible_for(hobbit, 9999, true, None).await.unwrap());

    // An administrator sees everything, whether they want it or not, unless the book is closed to them: a couple that are both administrators
    // can keep their own things apart. Opening again only takes the closing away (no exception is added: the rule shows them everything).
    let partner = store.create_profile("Partner", true).await.unwrap();
    assert!(sees(partner, winter).await, "not released, but an administrator sees it");
    assert!(store.set_book_visible_for(winter, partner, false, Some(admin)).await.unwrap());
    assert!(!sees(partner, winter).await && sees(admin, winter).await, "closed to the partner only");
    assert!(store.set_book_visible_for(winter, partner, true, Some(admin)).await.unwrap());
    assert!(sees(partner, winter).await);
    let m = store.visibility_matrix().await.unwrap();
    assert!(m.iter().any(|(_, n, seen, is_admin)| n == "Partner" && *is_admin && seen.contains(&winter)), "administrators have a column, too");
}
