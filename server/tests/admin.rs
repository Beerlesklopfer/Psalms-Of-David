// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Renders the pages of the administration interface from a real store.

mod common;

use common::{book, open};
use david_server::admin::i18n::Lang;
use david_server::admin::pages::{AdminUi, View};
use david_server::{NewProfile, Role, Schedule};

fn daytime() -> Schedule {
    let mut s = Schedule::none();
    s.allow_every_day(7 * 60, 19 * 60);
    s
}

#[tokio::test]
async fn the_overview_shows_the_state_in_english_and_german() {
    let (_dir, store) = open().await;
    store.create_profile("Boss", true).await.unwrap();
    store.import_books(vec![book("A", "One", &[1]), book("A", "Two", &[1])]).await.unwrap();
    let ui = AdminUi::new().unwrap();
    let server_id = store.server_id().await.unwrap();

    let en = ui.overview(&store, Lang::En).await.unwrap();
    assert!(en.contains("<html lang=\"en\">"));
    assert!(en.contains("<h1>Dashboard</h1>"));
    assert!(en.contains("Books waiting for a release"));
    assert!(en.contains("<dd>2</dd>"), "two unreleased books: {en}");
    assert!(en.contains("<dd>11</dd>"), "eleven unconfirmed proposals");
    assert!(en.contains("locked"), "the import is locked by default");
    assert!(en.contains(&server_id));

    let de = ui.overview(&store, Lang::De).await.unwrap();
    assert!(de.contains("<html lang=\"de\">"));
    assert!(de.contains("<h1>Dashboard</h1>"));
    assert!(de.contains("gesperrt"));
    assert!(!de.contains("Overview"), "no English left over in the German page");
}

#[tokio::test]
async fn the_user_list_shows_roles_in_the_language_of_the_page() {
    let (_dir, store) = open().await;
    store.create_profile("Boss", true).await.unwrap();
    store.create_user(NewProfile::child("Kim", 8, daytime())).await.unwrap();
    let ui = AdminUi::new().unwrap();

    let en = ui.users(&store, Lang::En).await.unwrap();
    assert!(en.contains("<strong>Kim</strong>") && en.contains(">Child</td>") && en.contains(">8</td>"));
    assert!(en.contains(">Administrator</td>") && en.contains("&ndash;</td>"), "no age shows a dash");

    let de = ui.users(&store, Lang::De).await.unwrap();
    assert!(de.contains(">Kind</td>") && de.contains(">Administrator</td>"));
    assert!(de.contains("&ndash;</td>") && de.contains("<th>Alter</th>"), "no age shows a dash, the column is called Alter");
}

#[tokio::test]
async fn an_empty_user_list_says_so() {
    let (_dir, store) = open().await;
    let ui = AdminUi::new().unwrap();
    assert!(ui.users(&store, Lang::En).await.unwrap().contains("No users yet."));
    assert!(ui.users(&store, Lang::De).await.unwrap().contains("Noch keine Benutzer."));
}

#[tokio::test]
async fn names_are_escaped_and_cannot_inject_markup() {
    let (_dir, store) = open().await;
    store.create_profile("<script>alert(1)</script>", true).await.unwrap();
    store.create_profile("Tom & \"Jerry\" <b>", false).await.unwrap();
    let ui = AdminUi::new().unwrap();

    for lang in [Lang::En, Lang::De] {
        let page = ui.users(&store, lang).await.unwrap();
        assert!(!page.contains("<script>"), "an unescaped script tag got through");
        assert!(!page.contains("<b>"), "an unescaped tag got through");
        assert!(page.contains("&lt;script&gt;"));
        assert!(page.contains("Tom &amp; &#34;Jerry&#34; &lt;b&gt;") || page.contains("Tom &amp; &quot;Jerry&quot; &lt;b&gt;"),
            "the other name must be escaped too: {page}");
    }
}

#[tokio::test]
async fn pages_contain_no_template_leftovers_and_follow_the_header() {
    let (_dir, store) = open().await;
    store.create_profile("Boss", true).await.unwrap();
    let ui = AdminUi::new().unwrap();

    for lang in [Lang::En, Lang::De] {
        for page in [ui.overview(&store, lang).await.unwrap(), ui.users(&store, lang).await.unwrap()] {
            assert!(!page.contains("{{") && !page.contains("{%"), "template syntax left in the page");
            assert!(page.starts_with("<!doctype html>"));
        }
    }

    // An unsupported language gives the English page.
    let lang = Lang::from_accept_language("fr-FR,fr;q=0.9");
    assert!(ui.overview(&store, lang).await.unwrap().contains("<html lang=\"en\">"));
    let lang = Lang::from_accept_language("de-DE,de;q=0.9,en;q=0.8");
    assert!(ui.overview(&store, lang).await.unwrap().contains("<html lang=\"de\">"));
}

#[tokio::test]
async fn the_overview_counts_follow_the_data() {
    let (_dir, store) = open().await;
    store.create_profile("Boss", true).await.unwrap();
    store.import_books(vec![book("A", "One", &[1])]).await.unwrap();
    let id = store.find_book("A/One").await.unwrap().unwrap().id;
    let ui = AdminUi::new().unwrap();

    assert!(ui.overview(&store, Lang::En).await.unwrap().contains("<dd>1</dd>"));
    store.release_book(id, 6).await.unwrap();
    let page = ui.overview(&store, Lang::En).await.unwrap();
    assert!(!page.contains("Books waiting for a release</dt>\n  <dd>1</dd>"), "released books are no longer waiting");
    let _ = Role::Admin;
}

// Lists through sub-templates: a frame around rows, the same row in the page and in an answer.

#[tokio::test]
async fn the_page_contains_exactly_the_list_fragment() {
    let (_dir, store) = open().await;
    store.create_profile("Boss", true).await.unwrap();
    store.create_user(NewProfile::child("Kim", 8, daytime())).await.unwrap();
    let ui = AdminUi::new().unwrap();

    for lang in [Lang::En, Lang::De] {
        let page = ui.users(&store, lang).await.unwrap();
        let list = ui.user_list_fragment(&store, lang).await.unwrap();
        assert!(!list.contains("<html"), "a fragment is not a page");
        assert!(page.contains(&list), "the page must contain the list fragment unchanged");
    }
}

#[tokio::test]
async fn a_row_fragment_is_the_row_of_the_list() {
    let (_dir, store) = open().await;
    store.create_profile("Boss", true).await.unwrap();
    let kim = store.create_user(NewProfile::child("Kim", 8, daytime())).await.unwrap();
    let ui = AdminUi::new().unwrap();

    let row = ui.user_row_fragment(&store, Lang::En, kim).await.unwrap().unwrap();
    assert!(row.starts_with(&format!("<tr id=\"user-{kim}\">")), "an id lets a procedure replace just this row: {row}");
    assert!(row.contains("<strong>Kim</strong>") && row.contains(">Child</td>"));
    assert!(!row.contains("<table") && !row.contains("<html"), "a row only");

    let list = ui.user_list_fragment(&store, Lang::En).await.unwrap();
    assert!(list.contains(&row), "the list is made of the same row");

    // The row follows the language and changes with the data.
    store.set_profile_max_age(kim, Some(12)).await.unwrap();
    let changed = ui.user_row_fragment(&store, Lang::De, kim).await.unwrap().unwrap();
    assert!(changed.contains(">Kind</td>") && changed.contains(">12</td>"));
}

#[tokio::test]
async fn a_row_of_a_missing_user_is_none_and_names_are_escaped_in_rows() {
    let (_dir, store) = open().await;
    let evil = store.create_profile("<img src=x onerror=alert(1)>", true).await.unwrap();
    let ui = AdminUi::new().unwrap();

    assert!(ui.user_row_fragment(&store, Lang::En, 9999).await.unwrap().is_none());
    let row = ui.user_row_fragment(&store, Lang::En, evil).await.unwrap().unwrap();
    assert!(!row.contains("<img"), "an unescaped tag in a row: {row}");
    assert!(row.contains("&lt;img"));
}

#[tokio::test]
async fn an_empty_list_fragment_keeps_the_id_that_a_procedure_targets() {
    let (_dir, store) = open().await;
    let ui = AdminUi::new().unwrap();
    let list = ui.user_list_fragment(&store, Lang::En).await.unwrap();
    assert!(list.contains("id=\"user-list\"") && list.contains("No users yet."));
}


// Templates of a distribution

#[test]
fn a_template_in_an_override_folder_replaces_the_built_in_one() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("login.html"), "{% extends \"base.html\" %}{% block content %}Welcome to {{ t(\"app.name\") }} on Debian{% endblock %}").unwrap();
    let ui = AdminUi::with_override_dirs(vec![dir.path().to_owned(), "/no/such/folder".into()]).unwrap();
    let page = ui.login(View::new(Lang::En)).unwrap();
    assert!(page.contains("Welcome to David on Debian"), "the distribution's template is used");
    assert!(page.contains("<header"), "it still extends the built-in base template");
    assert_eq!(ui.overrides, vec![dir.path().join("login.html")]);
    let stock = AdminUi::new().unwrap().login(View::new(Lang::En)).unwrap();
    assert!(stock.contains("Sign in") && !stock.contains("Debian"), "the built-in one is untouched");
}

#[test]
fn the_first_folder_wins_and_a_broken_template_stops_the_start_with_its_name() {
    let (first, second) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    std::fs::write(first.path().join("login.html"), "first").unwrap();
    std::fs::write(second.path().join("login.html"), "second").unwrap();
    let ui = AdminUi::with_override_dirs(vec![first.path().to_owned(), second.path().to_owned()]).unwrap();
    assert_eq!(ui.login(View::new(Lang::En)).unwrap(), "first");

    std::fs::write(first.path().join("login.html"), "{% if %}").unwrap();
    let err = AdminUi::with_override_dirs(vec![first.path().to_owned()]).err().expect("a broken template is refused");
    assert!(err.to_string().contains("login.html"), "{err}");
}

#[test]
fn exported_templates_are_a_complete_starting_point() {
    let dir = tempfile::tempdir().unwrap();
    let n = david_server::admin::pages::export_templates(dir.path()).unwrap();
    assert!(n >= 20, "{n} files");
    assert!(dir.path().join("base.html").is_file() && dir.path().join("fragments/user_row.html").is_file());
    assert!(dir.path().join("static/admin.css").is_file());
    // Loading the unchanged export gives the same pages as the built-in ones.
    let exported = AdminUi::with_override_dirs(vec![dir.path().to_owned()]).unwrap();
    assert_eq!(exported.overrides.len(), n - 7, "every template, but not the seven static files, is an override");
    assert!(dir.path().join("static/logo.svg").is_file() && dir.path().join("static/favicon.svg").is_file() && dir.path().join("static/crop.js").is_file() && dir.path().join("static/chips.js").is_file() && dir.path().join("static/fields.js").is_file());
    assert_eq!(exported.login(View::new(Lang::De)).unwrap(), AdminUi::new().unwrap().login(View::new(Lang::De)).unwrap());
    assert!(exported.static_file("admin.css").is_some());
    assert!(AdminUi::new().unwrap().static_file("admin.css").is_none());
}
