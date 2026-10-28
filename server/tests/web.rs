// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The HTTP layer, called directly (no network): sign-in, roles, CSRF, fragments
//! and pages, error mapping, security headers.

mod common;

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use common::{book, open};
use david_server::admin::pages::AdminUi;
use david_server::auth::csrf_token;
use david_server::web::{AppState, router};
use david_server::{NewProfile, Role, Schedule, Store};
use tower::ServiceExt;

const PIN: &str = "483920";

struct App {
    router: Router,
    store: Store,
    _dir: tempfile::TempDir,
}

async fn app() -> App {
    let (dir, store) = open().await;
    let boss = store.create_profile("Boss", true).await.unwrap();
    store.set_pin(boss, PIN).await.unwrap();
    let state = AppState { store: store.clone(), listen: "127.0.0.1:8484".parse().unwrap(), fingerprint: None, ui: Arc::new(AdminUi::new().unwrap()), secure_cookie: false, library: None, uploads: david_server::upload::Uploads::start(store.clone(), &std::env::temp_dir().join(format!("david-uploads-{}", std::process::id()))) };
    App { router: router(state), store, _dir: dir }
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }
}

async fn send(app: &App, req: Request<Body>) -> Reply {
    let res = app.router.clone().oneshot(req).await.unwrap();
    let (parts, body) = res.into_parts();
    let bytes = to_bytes(body, 4 * 1024 * 1024).await.unwrap();
    Reply { status: parts.status, headers: parts.headers, body: String::from_utf8_lossy(&bytes).into_owned() }
}

#[derive(Default)]
struct Opts<'a> {
    cookie: Option<&'a str>,
    htmx: bool,
    csrf_header: Option<&'a str>,
    lang: Option<&'a str>,
}

fn get(uri: &str, o: &Opts) -> Request<Body> {
    let mut b = Request::builder().method("GET").uri(uri);
    if let Some(c) = o.cookie {
        b = b.header(header::COOKIE, c);
    }
    if o.htmx {
        b = b.header("hx-request", "true");
    }
    if let Some(l) = o.lang {
        b = b.header(header::ACCEPT_LANGUAGE, l);
    }
    b.body(Body::empty()).unwrap()
}

fn post(uri: &str, body: &str, o: &Opts) -> Request<Body> {
    let mut b = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    if let Some(c) = o.cookie {
        b = b.header(header::COOKIE, c);
    }
    if o.htmx {
        b = b.header("hx-request", "true");
    }
    if let Some(t) = o.csrf_header {
        b = b.header("x-csrf-token", t);
    }
    if let Some(l) = o.lang {
        b = b.header(header::ACCEPT_LANGUAGE, l);
    }
    b.body(Body::from(body.to_owned())).unwrap()
}

/// Opens the sign-in page like a browser and returns the cookie it sets and the token
/// in its form.
async fn login_page_token(a: &App) -> (String, String) {
    let page = send(a, get("/admin/login", &Opts::default())).await;
    let cookie = page.header("set-cookie").expect("the sign-in page sets a cookie").split(';').next().unwrap().to_owned();
    let marker = "name=\"csrf\" value=\"";
    let start = page.body.find(marker).expect("the form carries a token") + marker.len();
    let token = page.body[start..].split('"').next().unwrap().to_owned();
    (cookie, token)
}

/// Signs in the way a browser does: page first, then the form with its token.
async fn login(a: &App, body: &str) -> Reply {
    let (cookie, token) = login_page_token(a).await;
    let o = Opts { cookie: Some(&cookie), ..Default::default() };
    send(a, post("/admin/login", &format!("{body}&csrf={token}"), &o)).await
}

/// Signs in as Boss and returns the cookie header value and the CSRF token.
async fn sign_in(app: &App) -> (String, String) {
    let r = login(app, &format!("name=Boss&pin={PIN}")).await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "sign-in failed: {}", r.body);
    let set = r.header("set-cookie").expect("a session cookie").to_owned();
    let pair = set.split(';').next().unwrap().to_owned();
    let token = pair.strip_prefix("david_session=").unwrap().to_owned();
    (pair, csrf_token(&token))
}

fn form(fields: &[(&str, &str)]) -> String {
    serde_urlencoded::to_string(fields).unwrap()
}

// Access and sign-in

#[tokio::test]
async fn without_a_session_pages_lead_to_sign_in() {
    let a = app().await;
    let r = send(&a, get("/admin/", &Opts::default())).await;
    assert_eq!((r.status, r.header("location")), (StatusCode::SEE_OTHER, Some("/admin/login")));

    // HTMX cannot follow a redirect inside a swap, so it is told to navigate.
    let r = send(&a, get("/admin/users", &Opts { htmx: true, ..Default::default() })).await;
    assert_eq!((r.status, r.header("hx-redirect")), (StatusCode::UNAUTHORIZED, Some("/admin/login")));

    for path in ["/admin/users", "/admin/books", "/admin/settings"] {
        assert_eq!(send(&a, get(path, &Opts::default())).await.status, StatusCode::SEE_OTHER, "{path}");
    }
}

#[tokio::test]
async fn the_sign_in_page_follows_the_language_and_has_no_navigation() {
    let a = app().await;
    let en = send(&a, get("/admin/login", &Opts::default())).await;
    assert_eq!(en.status, StatusCode::OK);
    assert!(en.body.contains("Sign in") && !en.body.contains("/admin/logout"));
    let de = send(&a, get("/admin/login", &Opts { lang: Some("de-DE,de;q=0.9"), ..Default::default() })).await;
    assert!(de.body.contains("Anmelden") && de.body.contains("<html lang=\"de\">"));
}

#[tokio::test]
async fn a_right_pin_opens_a_session_with_a_careful_cookie() {
    let a = app().await;
    let r = login(&a, &format!("name=Boss&pin={PIN}&remember=1")).await;
    assert_eq!((r.status, r.header("location")), (StatusCode::SEE_OTHER, Some("/admin/")));
    let cookie = r.header("set-cookie").unwrap();
    for part in ["david_session=", "HttpOnly", "SameSite=Strict", "Path=/;", "Max-Age="] {
        assert!(cookie.contains(part), "{part} missing in {cookie}");
    }
    assert!(!cookie.contains("Secure"), "no Secure flag while the server speaks plain HTTP");
}

#[tokio::test]
async fn a_wrong_name_and_a_wrong_pin_get_the_same_answer() {
    let a = app().await;
    let wrong_pin = login(&a, "name=Boss&pin=000000").await;
    let wrong_name = login(&a, &format!("name=Nobody&pin={PIN}")).await;
    assert_eq!(wrong_pin.status, StatusCode::UNAUTHORIZED);
    // The pages differ only in the sign-in token, which every visit makes anew.
    let without_token = |body: &str| body.split('"').map(|p| if p.len() == 64 && p.bytes().all(|b| b.is_ascii_hexdigit()) { "TOKEN" } else { p }).collect::<Vec<_>>().join("\"");
    assert_eq!((wrong_name.status, without_token(&wrong_name.body)), (wrong_pin.status, without_token(&wrong_pin.body)));
    assert!(wrong_pin.body.contains("Wrong name or PIN."));
    assert!(wrong_pin.header("set-cookie").is_none());
}

#[tokio::test]
async fn five_wrong_pins_lock_the_account() {
    let a = app().await;
    for _ in 0..4 {
        assert_eq!(login(&a, "name=Boss&pin=000000").await.status, StatusCode::UNAUTHORIZED);
    }
    let fifth = login(&a, "name=Boss&pin=000000").await;
    assert_eq!(fifth.status, StatusCode::TOO_MANY_REQUESTS);
    // Even the right PIN is refused while locked.
    let right = login(&a, &format!("name=Boss&pin={PIN}")).await;
    assert_eq!(right.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(right.header("set-cookie").is_none());
}

#[tokio::test]
async fn only_administrators_may_use_the_administration() {
    let a = app().await;
    let anna = a.store.create_profile("Anna", false).await.unwrap();
    a.store.set_pin(anna, "112233").await.unwrap();

    // A member may sign in, and lands on their own page, not in the administration.
    let r = login(&a, "name=Anna&pin=112233").await;
    assert_eq!((r.status, r.header("location")), (StatusCode::SEE_OTHER, Some("/profile")));
    assert!(r.header("set-cookie").is_some());

    // A session of a member, made by other means, is refused everywhere.
    let token = a.store.create_session(anna, Role::Member, None).await.unwrap();
    let cookie = format!("david_session={token}");
    let o = Opts { cookie: Some(&cookie), ..Default::default() };
    for path in ["/admin/", "/admin/users", "/admin/books", "/admin/settings"] {
        assert_eq!(send(&a, get(path, &o)).await.status, StatusCode::FORBIDDEN, "{path}");
    }
    let csrf = csrf_token(&token);
    let o = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), htmx: true, ..Default::default() };
    let r = send(&a, post("/admin/rpc/user.delete", &form(&[("id", "1")]), &o)).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert!(a.store.list_profiles().await.unwrap().iter().any(|p| p.name == "Boss"), "nothing was deleted");
}

#[tokio::test]
async fn a_demoted_administrator_loses_access_at_once() {
    let a = app().await;
    let second = a.store.create_profile("Second", true).await.unwrap();
    a.store.set_pin(second, "112233").await.unwrap();
    let token = a.store.create_session(second, Role::Admin, None).await.unwrap();
    let cookie = format!("david_session={token}");
    let o = Opts { cookie: Some(&cookie), ..Default::default() };
    assert_eq!(send(&a, get("/admin/users", &o)).await.status, StatusCode::OK);

    a.store.set_profile_role(second, Role::Member).await.unwrap();
    assert_eq!(send(&a, get("/admin/users", &o)).await.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn signing_out_ends_the_session() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), ..Default::default() };
    assert_eq!(send(&a, get("/admin/", &o)).await.status, StatusCode::OK);

    let r = send(&a, post("/admin/logout", &form(&[("csrf", &csrf)]), &o)).await;
    assert_eq!((r.status, r.header("location")), (StatusCode::SEE_OTHER, Some("/admin/login")));
    assert!(r.header("set-cookie").unwrap().contains("Max-Age=0"));
    assert_eq!(send(&a, get("/admin/", &o)).await.status, StatusCode::SEE_OTHER, "the old cookie is worthless now");
}

// Pages, headers and static files

#[tokio::test]
async fn the_pages_carry_the_csrf_token_and_the_security_headers() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), ..Default::default() };
    for path in ["/admin/", "/admin/users", "/admin/books", "/admin/settings"] {
        let r = send(&a, get(path, &o)).await;
        assert_eq!(r.status, StatusCode::OK, "{path}");
        assert!(r.body.contains(&format!("\"X-CSRF-Token\": \"{csrf}\"")), "{path} must hand the token to HTMX");
        assert!(r.body.contains("/admin/logout") && r.body.contains("htmx.min.js"));
        let csp = r.header("content-security-policy").unwrap();
        assert!(csp.contains("script-src 'self'") && csp.contains("default-src 'none'") && csp.contains("frame-ancestors 'none'"));
        assert_eq!(r.header("x-content-type-options"), Some("nosniff"));
        assert_eq!(r.header("x-frame-options"), Some("DENY"));
        assert_eq!(r.header("referrer-policy"), Some("no-referrer"));
        assert_eq!(r.header("cache-control"), Some("no-store"));
        assert!(!r.body.contains("<script>") && !r.body.contains("onclick"), "no inline script: {path}");
    }
}

#[tokio::test]
async fn the_static_files_are_served_by_the_server() {
    let a = app().await;
    let js = send(&a, get("/admin/static/htmx.min.js", &Opts::default())).await;
    assert_eq!(js.status, StatusCode::OK);
    assert!(js.header("content-type").unwrap().starts_with("text/javascript"));
    assert!(js.body.contains("2.0.4"), "the vendored version");
    let css = send(&a, get("/admin/static/admin.css", &Opts::default())).await;
    assert_eq!(css.status, StatusCode::OK);
    assert!(css.header("content-type").unwrap().starts_with("text/css"));
    assert!(css.header("content-security-policy").is_some(), "the headers apply to files too");
}

#[tokio::test]
async fn the_page_follows_the_language_of_the_browser() {
    let a = app().await;
    let (cookie, _) = sign_in(&a).await;
    let de = send(&a, get("/admin/users", &Opts { cookie: Some(&cookie), lang: Some("de"), ..Default::default() })).await;
    assert!(de.body.contains("<html lang=\"de\">") && de.body.contains("Neuer Benutzer") && de.body.contains("Abmelden"));
}

#[tokio::test]
async fn a_message_in_the_address_is_shown_only_if_it_is_one_of_ours() {
    let a = app().await;
    let (cookie, _) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), ..Default::default() };
    let ours = send(&a, get("/admin/users?msg=err.last_admin", &o)).await;
    assert!(ours.body.contains("last administrator") && ours.body.contains("message error"));
    let saved = send(&a, get("/admin/users?msg=ok.saved", &o)).await;
    assert!(saved.body.contains("Saved."));

    for evil in ["%3Cscript%3Ealert(1)%3C/script%3E", "unknown.key", "err.nope", "ok."] {
        let r = send(&a, get(&format!("/admin/users?msg={evil}"), &o)).await;
        assert!(!r.body.contains("alert(1)") && !r.body.contains("class=\"message"), "{evil} must not show");
    }
}

#[tokio::test]
async fn user_names_are_escaped_on_the_pages() {
    let a = app().await;
    a.store.create_profile("<script>alert(1)</script>", false).await.unwrap();
    let (cookie, _) = sign_in(&a).await;
    let r = send(&a, get("/admin/users", &Opts { cookie: Some(&cookie), ..Default::default() })).await;
    assert!(!r.body.contains("<script>alert(1)"), "an unescaped script tag got through");
    assert!(r.body.contains("&lt;script&gt;"));
}

// CSRF

#[tokio::test]
async fn a_change_without_the_csrf_token_is_refused() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let body = form(&[("name", "Eve"), ("role", "member")]);

    let none = Opts { cookie: Some(&cookie), htmx: true, ..Default::default() };
    let r = send(&a, post("/admin/rpc/user.create", &body, &none)).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert!(r.body.contains("expired"), "a readable reason: {}", r.body);
    assert_eq!(r.header("hx-retarget"), Some("#messages"));

    let wrong = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some("0".repeat(64).leak()), ..Default::default() };
    assert_eq!(send(&a, post("/admin/rpc/user.create", &body, &wrong)).await.status, StatusCode::FORBIDDEN);

    // A token that belongs to another session does not work either.
    let other = csrf_token("some other session");
    let foreign = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&other), ..Default::default() };
    assert_eq!(send(&a, post("/admin/rpc/user.create", &body, &foreign)).await.status, StatusCode::FORBIDDEN);

    assert!(!a.store.list_profiles().await.unwrap().iter().any(|p| p.name == "Eve"), "nothing was created");

    // The right token works in the header (HTMX) and as a form field (plain form).
    let header_ok = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    assert_eq!(send(&a, post("/admin/rpc/user.create", &body, &header_ok)).await.status, StatusCode::OK);
    let field_ok = Opts { cookie: Some(&cookie), ..Default::default() };
    let r = send(&a, post("/admin/rpc/user.create", &form(&[("name", "Eva"), ("role", "member"), ("csrf", &csrf)]), &field_ok)).await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
}

// Procedures: fragments or pages, and the errors of the data layer

#[tokio::test]
async fn a_procedure_answers_htmx_with_a_fragment_and_a_plain_form_with_a_redirect() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let htmx = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };

    let r = send(&a, post("/admin/rpc/user.create", &form(&[("name", "Anna"), ("role", "member"), ("pin", "112233")]), &htmx)).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body.contains("id=\"user-list\"") && r.body.contains("<strong>Anna</strong>"));
    assert!(!r.body.contains("<html"), "a fragment, not a page");

    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    let r = send(&a, post("/admin/rpc/user.create", &form(&[("name", "Ben"), ("role", "member"), ("csrf", &csrf)]), &plain)).await;
    assert_eq!((r.status, r.header("location")), (StatusCode::SEE_OTHER, Some("/admin/users?msg=ok.saved")));
    let page = send(&a, get("/admin/users?msg=ok.saved", &plain)).await;
    assert!(page.body.contains("<html") && page.body.contains("<strong>Ben</strong>") && page.body.contains("Saved."));

    // The PIN given at creation works.
    let r = login(&a, "name=Anna&pin=112233").await;
    assert_eq!((r.status, r.header("location")), (StatusCode::SEE_OTHER, Some("/profile")), "a member signs in and lands on their own page");
}

#[tokio::test]
async fn the_errors_of_the_data_layer_become_statuses_and_readable_messages() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let boss = a.store.list_profiles().await.unwrap().iter().find(|p| p.name == "Boss").unwrap().id.to_string();

    // The last administrator.
    let r = send(&a, post("/admin/rpc/user.delete", &form(&[("id", &boss)]), &o)).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert!(r.body.contains("last administrator") && r.header("hx-retarget") == Some("#messages") && r.header("hx-reswap") == Some("innerHTML"));
    let r = send(&a, post("/admin/rpc/user.set_role", &form(&[("id", &boss), ("role", "member")]), &o)).await;
    assert_eq!(r.status, StatusCode::CONFLICT);

    // A child without an age limit.
    let r = send(&a, post("/admin/rpc/user.create", &form(&[("name", "Kim"), ("role", "child")]), &o)).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(r.body.contains("age limit"));

    // A PIN of the wrong shape, and no user is created.
    let r = send(&a, post("/admin/rpc/user.create", &form(&[("name", "Pia"), ("role", "member"), ("pin", "12")]), &o)).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(!a.store.list_profiles().await.unwrap().iter().any(|p| p.name == "Pia"));

    // Nonsense input.
    for (name, role, age) in [("", "member", ""), ("X", "emperor", ""), ("Y", "child", "abc"), ("Z", "child", "99")] {
        let r = send(&a, post("/admin/rpc/user.create", &form(&[("name", name), ("role", role), ("max_age", age)]), &o)).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{name:?} {role:?} {age:?}");
    }

    // A user that does not exist.
    let r = send(&a, post("/admin/rpc/user.set_age_limit", &form(&[("id", "9999"), ("max_age", "6")]), &o)).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "a user that does not exist (any more)");
}

#[tokio::test]
async fn sending_user_create_twice_creates_one_user_and_one_clean_error() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let body = form(&[("name", "Anna"), ("role", "member")]);
    assert_eq!(send(&a, post("/admin/rpc/user.create", &body, &o)).await.status, StatusCode::OK);
    let again = send(&a, post("/admin/rpc/user.create", &body, &o)).await;
    assert_eq!(again.status, StatusCode::CONFLICT);
    assert!(again.body.contains("already exists"));
    assert_eq!(a.store.list_profiles().await.unwrap().iter().filter(|p| p.name == "Anna").count(), 1);
}

#[tokio::test]
async fn a_user_can_be_changed_row_by_row() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let anna = a.store.create_profile("Anna", false).await.unwrap().to_string();

    // Becoming a child needs the age limit; the hours default to 07:00 to 19:00.
    let r = send(&a, post("/admin/rpc/user.set_role", &form(&[("id", &anna), ("role", "child"), ("max_age", "8")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.body.starts_with(&format!("<tr id=\"user-{anna}\">")) && r.body.contains(">Child</td>") && r.body.contains(">8</td>"));
    let stored = a.store.list_profiles().await.unwrap().into_iter().find(|p| p.name == "Anna").unwrap();
    let planner = Schedule::from_bytes(&stored.schedule.unwrap()).unwrap();
    assert!(planner.is_allowed(2, 8 * 60) && !planner.is_allowed(2, 20 * 60));

    let r = send(&a, post("/admin/rpc/user.set_age_limit", &form(&[("id", &anna), ("max_age", "10")]), &o)).await;
    assert!(r.body.contains(">10</td>"));
    // An empty limit turns a child's limit off, which the data layer refuses.
    let r = send(&a, post("/admin/rpc/user.set_age_limit", &form(&[("id", &anna), ("max_age", "")]), &o)).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);

    let r = send(&a, post("/admin/rpc/user.set_device_limit", &form(&[("id", &anna), ("max_devices", "2")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(a.store.list_profiles().await.unwrap().into_iter().find(|p| p.name == "Anna").unwrap().max_devices, Some(2));

    let r = send(&a, post("/admin/rpc/user.set_pin", &form(&[("id", &anna), ("pin", "9876")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(send(&a, post("/admin/rpc/user.set_pin", &form(&[("id", &anna), ("pin", "98")]), &o)).await.status, StatusCode::UNPROCESSABLE_ENTITY);

    // Deleting answers with nothing, which removes the row.
    let r = send(&a, post("/admin/rpc/user.delete", &form(&[("id", &anna)]), &o)).await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::OK, ""));
    assert!(!a.store.list_profiles().await.unwrap().iter().any(|p| p.name == "Anna"));
}

#[tokio::test]
async fn books_are_released_with_an_age_rating_and_the_recommendation_is_guarded() {
    let a = app().await;
    a.store.import_books(vec![book("Ken Follett", "Kinder der Freiheit", &[2])]).await.unwrap();
    let id = a.store.find_book("Ken Follett/Kinder der Freiheit").await.unwrap().unwrap().id;
    a.store.set_book_recommended_age(id, Some(14)).await.unwrap();
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let id = id.to_string();

    let page = send(&a, get("/admin/books", &o)).await;
    assert!(page.body.contains("Kinder der Freiheit") && page.body.contains("Not released"));

    // Below the recommendation needs the tick.
    let r = send(&a, post("/admin/rpc/book.release", &form(&[("id", &id), ("min_age", "6")]), &o)).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert!(r.body.contains("below the recommendation"));
    assert!(!a.store.find_book("Ken Follett/Kinder der Freiheit").await.unwrap().unwrap().released);

    let r = send(&a, post("/admin/rpc/book.release", &form(&[("id", &id), ("min_age", "6"), ("confirm", "1")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.body.starts_with(&format!("<tr id=\"book-{id}\">")) && r.body.contains("Released"));

    // At or above the recommendation no tick is needed; a missing rating is refused.
    assert_eq!(send(&a, post("/admin/rpc/book.release", &form(&[("id", &id), ("min_age", "16")]), &o)).await.status, StatusCode::OK);
    assert_eq!(send(&a, post("/admin/rpc/book.release", &form(&[("id", &id), ("min_age", "")]), &o)).await.status, StatusCode::UNPROCESSABLE_ENTITY);

    let r = send(&a, post("/admin/rpc/book.set_exempt", &form(&[("id", &id), ("exempt", "1")]), &o)).await;
    assert!(r.body.contains("Learning content"));
    let r = send(&a, post("/admin/rpc/book.unrelease", &form(&[("id", &id)]), &o)).await;
    assert!(r.body.contains("Not released"));
}

#[tokio::test]
async fn every_setting_is_a_line_edit_and_one_save_decides_them() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let before = a.store.unconfirmed_defaults().await.unwrap();

    // Three tabs; the values are inputs with their unit, and nothing speaks of proposals any more.
    let page = send(&a, get("/admin/settings?tab=values", &Opts { cookie: Some(&cookie), ..Default::default() })).await.body;
    assert!(page.contains("class=\"tab\"") && page.contains("aria-current=\"page\""), "{page}");
    assert!(page.contains("name=\"import_interval_min\"") && page.contains("name=\"pin_lock_s\"") && page.contains("name=\"time_zone\"") && page.contains("minutes"));
    assert!(!page.contains("Proposal") && !page.contains("Accepted") && !page.contains("import_roots"));
    assert!(page.contains("form=") || page.contains("type=\"submit\" class=\"btn\">Save"));

    // Minutes and days are stored as the engine reads them.
    let ok = form(&[("pin_max_failures", "7"), ("pin_lock_s", "30"), ("enrollment_code_ttl_s", "10"), ("device_inactive_days", "90"), ("guest_expire_days", "3"), ("import_interval_min", "0"), ("time_zone", "Europe/Berlin")]);
    let r = send(&a, post("/admin/rpc/settings.save", &ok, &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.body.starts_with("<form id=\"values-form\"") && r.body.contains("Saved."));
    assert_eq!(a.store.setting("pin_lock_s").await.unwrap().as_deref(), Some("1800"));
    assert_eq!(a.store.setting("enrollment_code_ttl_s").await.unwrap().as_deref(), Some("600"));
    assert_eq!(a.store.setting("pin_max_failures").await.unwrap().as_deref(), Some("7"));
    assert_eq!(a.store.setting("import_interval_min").await.unwrap().as_deref(), Some("0"), "0 turns the automatic look off");
    assert!(a.store.unconfirmed_defaults().await.unwrap() < before, "saving decides the values");

    // Wrong input changes nothing.
    for (key, value) in [("pin_max_failures", "0"), ("pin_lock_s", "abc"), ("guest_expire_days", "999"), ("time_zone", "Mars/Olympus")] {
        let r = send(&a, post("/admin/rpc/settings.save", &form(&[("pin_max_failures", "9"), (key, value)]), &o)).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{key}={value}");
    }
    assert_eq!(a.store.setting("pin_max_failures").await.unwrap().as_deref(), Some("7"));

    // Roles: devices and days; a session that is not whole days stays when its shown number is left alone.
    let guest_before = a.store.role_policies().await.unwrap().into_iter().find(|p| p.role == "guest").unwrap();
    let r = send(&a, post("/admin/rpc/policies.save", &form(&[("devices_child", "2"), ("days_child", "10"), ("devices_guest", "1"), ("days_guest", &(guest_before.session_ttl_s / 86_400).max(1).to_string())]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let child = a.store.role_policies().await.unwrap().into_iter().find(|p| p.role == "child").unwrap();
    assert_eq!((child.max_devices, child.session_ttl_s), (2, 864_000));
    assert!(child.confirmed);
    let guest = a.store.role_policies().await.unwrap().into_iter().find(|p| p.role == "guest").unwrap();
    assert_eq!(guest.session_ttl_s, guest_before.session_ttl_s);
    let r = send(&a, post("/admin/rpc/policies.save", &form(&[("devices_child", "0"), ("days_child", "10")]), &o)).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);

    // The roles tab and the import tab render too, and an unknown tab shows the import.
    for tab in ["roles", "import", "nonsense"] {
        let r = send(&a, get(&format!("/admin/settings?tab={tab}"), &Opts { cookie: Some(&cookie), ..Default::default() })).await;
        assert_eq!(r.status, StatusCode::OK, "{tab}");
    }
}

#[tokio::test]
async fn the_overview_counts_follow_what_happens() {
    let a = app().await;
    a.store.import_books(vec![book("A", "One", &[1]), book("A", "Two", &[1])]).await.unwrap();
    a.store.create_user(NewProfile::child("Kim", 8, { let mut s = Schedule::none(); s.allow_every_day(420, 1140); s })).await.unwrap();
    let (cookie, _) = sign_in(&a).await;
    let r = send(&a, get("/admin/", &Opts { cookie: Some(&cookie), ..Default::default() })).await;
    assert!(r.body.contains("<dd>2</dd>"), "two books wait for a release: {}", r.body);
    assert!(r.body.contains("locked"));
}

// Import

#[tokio::test]
async fn the_import_page_walks_from_locked_to_unlocked_and_back() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };

    let page = send(&a, get("/admin/settings?tab=import", &Opts { cookie: Some(&cookie), ..Default::default() })).await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.body.contains("Locked. Nothing is read from any folder."));
    assert!(page.body.contains("class=\"btn\" disabled>"), "unlocking is not offered before the criteria exist");
    assert!(page.body.contains("Save the import folders and the rating rule first."));

    // Unlocking without criteria is refused by the server, whatever the page offers.
    let r = send(&a, post("/admin/rpc/import.unlock", "", &o)).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert!(r.body.contains("not defined yet"));
    assert!(!a.store.import_state().await.unwrap().enabled);

    // Unusable folders are refused with a reason.
    for roots in ["relative/path", "/definitely/not/there", "/", ""] {
        let r = send(&a, post("/admin/rpc/import.save_criteria", &form(&[("roots", roots), ("rule", "manual")]), &o)).await;
        assert!(r.status == StatusCode::UNPROCESSABLE_ENTITY || r.status == StatusCode::CONFLICT, "{roots:?} gave {}", r.status);
    }
    let r = send(&a, post("/admin/rpc/import.save_criteria", &form(&[("roots", "/tmp"), ("rule", "nonsense")]), &o)).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "an unknown rule");

    // Good criteria, then unlock.
    let lib = tempfile::TempDir::new().unwrap();
    let root = std::fs::canonicalize(lib.path()).unwrap().to_string_lossy().into_owned();
    let r = send(&a, post("/admin/rpc/import.save_criteria", &form(&[("roots", &format!("  {root}  \n")), ("rule", "recommendation")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    // MiniJinja writes the slashes of a path as entities, which a browser shows as slashes.
    let leaf = lib.path().file_name().unwrap().to_string_lossy().into_owned();
    assert!(r.body.starts_with("<section id=\"import-settings\">") && r.body.contains(&leaf), "{}", r.body);
    assert!(!r.body.contains(" disabled>"), "now unlocking is offered");

    let r = send(&a, post("/admin/rpc/import.unlock", "", &o)).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body.contains(">Unlocked<") && r.body.contains("unlocked by Boss"), "{}", r.body);
    assert!(r.body.contains("Lock import"));
    let page = send(&a, get("/admin/import", &Opts { cookie: Some(&cookie), ..Default::default() })).await;
    assert!(page.body.contains("Import now"), "the import page offers the run once unlocked");

    let r = send(&a, post("/admin/rpc/import.lock", "", &o)).await;
    assert!(r.body.contains("Locked. Nothing is read"));
    assert!(r.body.contains("class=\"state shut\""));
    let r = send(&a, post("/admin/rpc/import.run", "", &o)).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert!(r.body.contains("import is locked"));
}

#[tokio::test]
async fn an_import_from_the_page_shows_its_result_and_the_books_wait_for_release() {
    if !common::audio::ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let lib = tempfile::TempDir::new().unwrap();
    common::audio::make_mp3(&lib.path().join("Ken Follett/Kinder der Freiheit/Follett_Kinder_CD01_TR01.mp3"), &[("AGE_RECOMMENDATION", "14")]);
    let root = std::fs::canonicalize(lib.path()).unwrap().to_string_lossy().into_owned();

    send(&a, post("/admin/rpc/import.save_criteria", &form(&[("roots", &root), ("rule", "manual")]), &o)).await;
    send(&a, post("/admin/rpc/import.unlock", "", &o)).await;
    let r = send(&a, post("/admin/rpc/import.run", "", &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.body.contains("Result of the last import") && r.body.contains("<dt>Audiobooks</dt><dd>1</dd>") && r.body.contains("<dt>Authors</dt><dd>1</dd>") && r.body.contains("<dt>Albums</dt><dd>0</dd>") && r.body.contains("<dt>Tracks added</dt><dd>1</dd>"));

    // The book is there, unreleased, with the recommendation of the medium as the starting value.
    let books = send(&a, get("/admin/books", &o)).await;
    assert!(books.body.contains("Kinder der Freiheit") && books.body.contains("Not released"));
    assert!(books.body.contains("value=\"14\""), "the release form starts with the recommendation");
}

#[tokio::test]
async fn a_member_cannot_touch_the_import() {
    let a = app().await;
    let anna = a.store.create_profile("Anna", false).await.unwrap();
    let token = a.store.create_session(anna, Role::Member, None).await.unwrap();
    let cookie = format!("david_session={token}");
    let csrf = csrf_token(&token);
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    for proc in ["import.unlock", "import.lock", "import.run", "import.save_criteria"] {
        let r = send(&a, post(&format!("/admin/rpc/{proc}"), &form(&[("roots", "/tmp"), ("rule", "manual")]), &o)).await;
        assert_eq!(r.status, StatusCode::FORBIDDEN, "{proc}");
    }
    assert_eq!(send(&a, get("/admin/import", &o)).await.status, StatusCode::FORBIDDEN);
    assert!(!a.store.import_state().await.unwrap().enabled);
}

// Weekly planner

fn daytime_schedule() -> Schedule {
    let mut s = Schedule::none();
    s.allow_every_day(7 * 60, 19 * 60);
    s
}

/// The `<day>-<slot>` values of all ticked cells on a planner page.
fn ticked(page: &str) -> Vec<String> {
    page.split("<input type=\"checkbox\" name=\"slot\" value=\"")
        .skip(1)
        .filter_map(|rest| {
            let value = rest.split('"').next()?;
            let tag_end = rest.find('>')?;
            rest[..tag_end].contains(" checked").then(|| value.to_owned())
        })
        .collect()
}

async fn stored_schedule(a: &App, name: &str) -> Option<Schedule> {
    let p = a.store.list_profiles().await.unwrap().into_iter().find(|p| p.name == name).unwrap();
    p.schedule.as_deref().and_then(Schedule::from_bytes)
}

#[tokio::test]
async fn the_planner_page_shows_the_stored_quarters_of_an_hour() {
    let a = app().await;
    let kim = a.store.create_user(NewProfile::child("Kim", 8, daytime_schedule())).await.unwrap();
    let (cookie, _) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), ..Default::default() };

    let page = send(&a, get(&format!("/admin/schedule?user={kim}"), &o)).await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    assert!(page.body.contains("Kim") && page.body.contains("Child") && page.body.contains("Allowed hours per week: <strong class=\"planner-total\">84.00</strong>"));
    let ticks = ticked(&page.body);
    assert_eq!(ticks.len(), 7 * 48, "12 hours a day, 4 quarters, 7 days");
    assert!(ticks.contains(&"0-28".to_owned()), "Monday 07:00 is slot 28");
    assert!(!ticks.contains(&"0-27".to_owned()) && !ticks.contains(&"0-76".to_owned()), "06:45 and 19:00 are not allowed");
    assert!(page.body.contains(">Mon<") && page.body.contains(">Sun<"));

    let de = send(&a, get(&format!("/admin/schedule?user={kim}"), &Opts { cookie: Some(&cookie), lang: Some("de"), ..Default::default() })).await;
    assert!(de.body.contains(">Mo<") && de.body.contains("Wochenplan") && de.body.contains("84.00"));
}

#[tokio::test]
async fn saving_the_grid_gives_the_same_bytes_as_the_schedule_in_code() {
    let a = app().await;
    let kim = a.store.create_user(NewProfile::child("Kim", 8, daytime_schedule())).await.unwrap();
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let before = stored_schedule(&a, "Kim").await.unwrap();

    // Read the grid from the page and post it back untouched: nothing may change.
    let page = send(&a, get(&format!("/admin/schedule?user={kim}"), &o)).await;
    let mut fields: Vec<(&str, String)> = vec![("id", kim.to_string())];
    let ticks = ticked(&page.body);
    fields.extend(ticks.iter().map(|t| ("slot", t.clone())));
    let body = serde_urlencoded::to_string(&fields).unwrap();
    let r = send(&a, post("/admin/rpc/schedule.save", &body, &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.body.starts_with("<section id=\"schedule-panel\">") && !r.body.contains("<html"));
    assert_eq!(stored_schedule(&a, "Kim").await.unwrap().as_bytes(), before.as_bytes(), "a round trip changes nothing");

    // A grid made by hand equals the schedule built in code.
    let mut expected = Schedule::none();
    expected.set_slot(0, 28, true);
    expected.set_slot(6, 95, true);
    expected.allow_range(2, 10 * 60, 10 * 60 + 30);
    let body = form(&[("id", &kim.to_string()), ("slot", "0-28"), ("slot", "6-95"), ("slot", "2-40"), ("slot", "2-41")]);
    assert_eq!(send(&a, post("/admin/rpc/schedule.save", &body, &o)).await.status, StatusCode::OK);
    assert_eq!(stored_schedule(&a, "Kim").await.unwrap().as_bytes(), expected.as_bytes());

    // Nothing ticked is a valid planner: no listening time at all.
    let body = form(&[("id", &kim.to_string())]);
    assert_eq!(send(&a, post("/admin/rpc/schedule.save", &body, &o)).await.status, StatusCode::OK);
    assert_eq!(stored_schedule(&a, "Kim").await.unwrap().allowed_slots(), 0);
}

#[tokio::test]
async fn slots_that_do_not_exist_are_refused_and_change_nothing() {
    let a = app().await;
    let kim = a.store.create_user(NewProfile::child("Kim", 8, daytime_schedule())).await.unwrap();
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let before = stored_schedule(&a, "Kim").await.unwrap();

    for bad in ["7-0", "0-96", "x", "0", "-1-3", "0-", "1-2-3"] {
        let body = form(&[("id", &kim.to_string()), ("slot", "0-28"), ("slot", bad)]);
        let r = send(&a, post("/admin/rpc/schedule.save", &body, &o)).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{bad:?}");
    }
    assert_eq!(stored_schedule(&a, "Kim").await.unwrap().as_bytes(), before.as_bytes());
    assert_eq!(send(&a, post("/admin/rpc/schedule.save", &form(&[("slot", "0-1")]), &o)).await.status, StatusCode::UNPROCESSABLE_ENTITY, "no user given");
}

#[tokio::test]
async fn the_preset_sets_every_day_and_rejects_nonsense_hours() {
    let a = app().await;
    let kim = a.store.create_user(NewProfile::child("Kim", 8, daytime_schedule())).await.unwrap();
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };

    let r = send(&a, post("/admin/rpc/schedule.preset", &form(&[("id", &kim.to_string()), ("hours_from", "8"), ("hours_until", "12")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK);
    let mut expected = Schedule::none();
    expected.allow_every_day(8 * 60, 12 * 60);
    assert_eq!(stored_schedule(&a, "Kim").await.unwrap().as_bytes(), expected.as_bytes());
    assert!(r.body.contains("28.00"), "4 hours a day for 7 days: {}", r.body);

    for (from, until) in [("12", "8"), ("8", "8"), ("24", "24"), ("a", "9")] {
        let r = send(&a, post("/admin/rpc/schedule.preset", &form(&[("id", &kim.to_string()), ("hours_from", from), ("hours_until", until)]), &o)).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{from}-{until}");
    }
    assert_eq!(stored_schedule(&a, "Kim").await.unwrap().as_bytes(), expected.as_bytes(), "unchanged by the refused ones");
}

#[tokio::test]
async fn an_adult_may_drop_the_planner_but_a_child_may_not() {
    let a = app().await;
    let kim = a.store.create_user(NewProfile::child("Kim", 8, daytime_schedule())).await.unwrap();
    let anna = a.store.create_profile("Anna", false).await.unwrap();
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };

    // An adult starts without a planner, can get one, and can drop it again.
    assert!(stored_schedule(&a, "Anna").await.is_none());
    let page = send(&a, get(&format!("/admin/schedule?user={anna}"), &o)).await;
    assert!(page.body.contains("No planner: listening is allowed at any time."));
    send(&a, post("/admin/rpc/schedule.preset", &form(&[("id", &anna.to_string()), ("hours_from", "9"), ("hours_until", "17")]), &o)).await;
    assert!(stored_schedule(&a, "Anna").await.is_some());
    let r = send(&a, post("/admin/rpc/schedule.remove", &form(&[("id", &anna.to_string())]), &o)).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(stored_schedule(&a, "Anna").await.is_none());

    // A child always needs one.
    let r = send(&a, post("/admin/rpc/schedule.remove", &form(&[("id", &kim.to_string())]), &o)).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(r.body.contains("age limit and listening hours"));
    assert!(stored_schedule(&a, "Kim").await.is_some());
}

#[tokio::test]
async fn the_planner_works_without_javascript_and_for_missing_users() {
    let a = app().await;
    let kim = a.store.create_user(NewProfile::child("Kim", 8, daytime_schedule())).await.unwrap();
    let (cookie, csrf) = sign_in(&a).await;

    // A plain form post goes back to the planner page of that user and shows the message.
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    let r = send(&a, post("/admin/rpc/schedule.preset", &form(&[("id", &kim.to_string()), ("hours_from", "9"), ("hours_until", "11"), ("csrf", &csrf)]), &plain)).await;
    let back = format!("/admin/schedule?user={kim}&msg=ok.saved");
    assert_eq!((r.status, r.header("location")), (StatusCode::SEE_OTHER, Some(back.as_str())));
    let page = send(&a, get(&back, &plain)).await;
    assert!(page.body.contains("Saved.") && page.body.contains("<html"));

    assert_eq!(send(&a, get("/admin/schedule?user=9999", &plain)).await.status, StatusCode::NOT_FOUND);
    assert_eq!(send(&a, get("/admin/schedule", &plain)).await.status, StatusCode::BAD_REQUEST, "no user given");

    // The users page links to the planner of each user.
    let users = send(&a, get("/admin/users", &plain)).await;
    assert!(users.body.contains(&format!("href=\"/admin/schedule?user={kim}\"")));
}

// Pairing

/// Reads a QR code back: the data is drawn as a picture (with a quiet zone) and
/// decoded by an independent reader.
fn read_qr(data: &str) -> String {
    let code = qrcode::QrCode::new(data.as_bytes()).unwrap();
    let (w, scale, quiet) = (code.width(), 8usize, 4usize);
    let colors = code.to_colors();
    let size = (w + 2 * quiet) * scale;
    let pixel = move |x: usize, y: usize| -> u8 {
        let (cx, cy) = (x / scale, y / scale);
        if cx < quiet || cy < quiet || cx >= w + quiet || cy >= w + quiet {
            return 255;
        }
        if colors[(cy - quiet) * w + (cx - quiet)] == qrcode::Color::Dark { 0 } else { 255 }
    };
    let mut prepared = rqrr::PreparedImage::prepare_from_greyscale(size, size, pixel);
    let grids = prepared.detect_grids();
    assert_eq!(grids.len(), 1, "exactly one QR code in the picture");
    grids[0].decode().expect("the reader could not decode the code").1
}

/// A browser turns the entities of the page back into characters; so does the test.
fn unescape(html: &str) -> String {
    html.replace("&#x2f;", "/").replace("&amp;", "&").replace("&#38;", "&").replace("&#x27;", "'").replace("&quot;", "\"")
}

fn uri_of(body: &str) -> String {
    let text = unescape(body);
    let start = text.find("david://pair?").expect("a pairing address");
    let rest = &text[start..];
    rest[..rest.find("</code>").unwrap()].to_owned()
}

#[tokio::test]
async fn a_pairing_code_comes_with_a_qr_code_that_reads_back_as_the_address() {
    let a = app().await;
    let anna = a.store.create_profile("Anna", false).await.unwrap();
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };

    let page = send(&a, get("/admin/pairing", &o)).await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.body.contains("Anna") && page.body.contains("plain HTTP") && page.body.contains("value=\"127.0.0.1:8484\""));

    let body = form(&[("id", &anna.to_string()), ("address", "192.168.24.250:8484")]);
    let r = send(&a, post("/admin/rpc/pairing.create", &body, &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(!r.body.contains("<html") && r.body.contains("Scan this with the new device") && r.body.contains("<strong>Anna</strong>"));

    let uri = uri_of(&r.body);
    let server_id = a.store.server_id().await.unwrap();
    assert!(uri.starts_with(&format!("david://pair?v=1&id={server_id}&addr=192.168.24.250%3A8484&code=")), "{uri}");
    let code = uri.rsplit("code=").next().unwrap().to_owned();
    assert_eq!(code.len(), 32);

    // The picture is the QR code of exactly this address, and a reader gets it back.
    let svg = david_server::pairing::qr_svg(&uri).unwrap();
    assert!(r.body.contains(&svg), "the page holds the QR code of the address shown");
    assert!(!svg.contains("style="), "no inline style, the policy would block it");
    assert_eq!(read_qr(&uri), uri);

    // And the code works: a device redeems it and shows up on the page.
    let key = format!("{}A=", "B".repeat(42));
    let paired = a.store.redeem_enrollment(&code, &key, "Annas phone").await.unwrap();
    assert_eq!(paired.user_id, anna);
    let page = send(&a, get("/admin/pairing", &o)).await;
    assert!(page.body.contains("Annas phone") && page.body.contains("10.77.0.2") && page.body.contains("Active"));
}

#[tokio::test]
async fn without_javascript_the_answer_page_carries_the_code() {
    let a = app().await;
    let anna = a.store.create_profile("Anna", false).await.unwrap();
    let (cookie, csrf) = sign_in(&a).await;
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };

    let r = send(&a, post("/admin/rpc/pairing.create", &form(&[("id", &anna.to_string()), ("address", "david.local:8484"), ("csrf", &csrf)]), &plain)).await;
    assert_eq!(r.status, StatusCode::OK, "a page and not a redirect, because the code is shown only once");
    assert!(r.body.contains("<html") && unescape(&r.body).contains("david://pair?") && r.body.contains("<svg"));
}

#[tokio::test]
async fn unusable_addresses_users_and_limits_are_refused() {
    let a = app().await;
    let anna = a.store.create_profile("Anna", false).await.unwrap();
    let guest = a.store.create_user(NewProfile::new("Guest", Role::Guest)).await.unwrap();
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };

    for bad in ["", "http://x", "a b", "x&y=1", "<script>"] {
        let r = send(&a, post("/admin/rpc/pairing.create", &form(&[("id", &anna.to_string()), ("address", bad)]), &o)).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{bad:?}");
        assert!(r.body.contains("not a usable server address"));
    }
    let r = send(&a, post("/admin/rpc/pairing.create", &form(&[("id", "9999"), ("address", "h:1")]), &o)).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);

    // A guest may own one device by the proposed policy; with it taken, no code is made.
    let first = a.store.create_enrollment(guest, false, anna).await.unwrap();
    let key = format!("{}E=", "B".repeat(42));
    a.store.redeem_enrollment(&first.code, &key, "Phone").await.unwrap();
    let r = send(&a, post("/admin/rpc/pairing.create", &form(&[("id", &guest.to_string()), ("address", "h:1")]), &o)).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert!(r.body.contains("as many devices as allowed"));
}

#[tokio::test]
async fn revoking_a_device_frees_its_place_and_shows_in_the_row() {
    let a = app().await;
    let guest = a.store.create_user(NewProfile::new("Guest", Role::Guest)).await.unwrap();
    let e = a.store.create_enrollment(guest, false, guest).await.unwrap();
    let key = format!("{}I=", "B".repeat(42));
    let device = a.store.redeem_enrollment(&e.code, &key, "Phone").await.unwrap().device_id;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let make = form(&[("id", &guest.to_string()), ("address", "h:1")]);
    assert_eq!(send(&a, post("/admin/rpc/pairing.create", &make, &o)).await.status, StatusCode::CONFLICT);

    let r = send(&a, post("/admin/rpc/device.revoke", &form(&[("id", &device.to_string())]), &o)).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body.starts_with(&format!("<tr id=\"device-{device}\">")) && r.body.contains("Revoked") && !r.body.contains("<button"));
    assert_eq!(send(&a, post("/admin/rpc/pairing.create", &make, &o)).await.status, StatusCode::OK, "the place is free again");
    assert_eq!(send(&a, post("/admin/rpc/device.revoke", &form(&[("id", "9999")]), &o)).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_member_cannot_make_codes_or_revoke_devices() {
    let a = app().await;
    let anna = a.store.create_profile("Anna", false).await.unwrap();
    let token = a.store.create_session(anna, Role::Member, None).await.unwrap();
    let cookie = format!("david_session={token}");
    let csrf = csrf_token(&token);
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let r = send(&a, post("/admin/rpc/pairing.create", &form(&[("id", &anna.to_string()), ("address", "h:1")]), &o)).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(send(&a, post("/admin/rpc/device.revoke", &form(&[("id", "1")]), &o)).await.status, StatusCode::FORBIDDEN);
    assert_eq!(send(&a, get("/admin/pairing", &o)).await.status, StatusCode::FORBIDDEN);
    assert!(a.store.device_rows().await.unwrap().is_empty());
}

#[tokio::test]
async fn with_tls_the_pairing_code_carries_the_certificate_fingerprint() {
    let (dir, store) = open().await;
    let boss = store.create_profile("Boss", true).await.unwrap();
    store.set_pin(boss, PIN).await.unwrap();
    let anna = store.create_profile("Anna", false).await.unwrap();
    let fingerprint = "ab".repeat(32);
    let state = AppState {
        store: store.clone(),
        listen: "192.168.24.250:8484".parse().unwrap(),
        fingerprint: Some(fingerprint.clone()),
        ui: Arc::new(AdminUi::new().unwrap()),
        secure_cookie: true,
        library: None,
        uploads: david_server::upload::Uploads::start(store.clone(), &std::env::temp_dir().join(format!("david-uploads-tls-{}", std::process::id()))),
    };
    let a = App { router: router(state), store, _dir: dir };

    // The session cookie is marked Secure, and the page says the code is pinned.
    let login = login(&a, &format!("name=Boss&pin={PIN}")).await;
    assert!(login.header("set-cookie").unwrap().contains("; Secure"));
    let cookie = login.header("set-cookie").unwrap().split(';').next().unwrap().to_owned();
    let csrf = csrf_token(cookie.strip_prefix("david_session=").unwrap());
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let page = send(&a, get("/admin/pairing", &o)).await;
    assert!(page.body.contains("fingerprint of this server") && !page.body.contains("plain HTTP"));

    let r = send(&a, post("/admin/rpc/pairing.create", &form(&[("id", &anna.to_string()), ("address", "192.168.24.250:8484")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let uri = uri_of(&r.body);
    assert!(uri.contains(&format!("&fp={fingerprint}&code=")), "{uri}");
    assert_eq!(read_qr(&uri), uri, "the longer code still reads back");
}


// The user's own page: picture and PIN

/// A 1x1 PNG.
const PNG: &[u8] = &[
    0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0x0D, b'I', b'H', b'D', b'R', 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0,
    0x1F, 0x15, 0xC4, 0x89, 0, 0, 0, 0x0A, b'I', b'D', b'A', b'T', 0x78, 0x9C, 0x63, 0, 1, 0, 0, 5, 0, 1, 0x0D, 0x0A, 0x2D, 0xB4,
    0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82,
];

fn multipart(csrf: &str, cookie: &str, file_name: &str, bytes: &[u8]) -> Request<Body> {
    let boundary = "XBOUNDARYX";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"avatar\"; filename=\"{file_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    Request::builder()
        .method("POST")
        .uri(format!("/profile/avatar?csrf={csrf}"))
        .header(header::COOKIE, cookie)
        .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body))
        .unwrap()
}

async fn member_session(a: &App) -> (i64, String, String) {
    let anna = a.store.create_profile("Anna", false).await.unwrap();
    a.store.set_pin(anna, "112233").await.unwrap();
    let token = a.store.create_session(anna, Role::Member, None).await.unwrap();
    let csrf = csrf_token(&token);
    (anna, format!("david_session={token}"), csrf)
}

#[tokio::test]
async fn every_user_has_a_profile_page_and_the_menu_shows_only_what_they_may_use() {
    let a = app().await;
    let (_, cookie, _) = member_session(&a).await;
    let o = Opts { cookie: Some(&cookie), ..Default::default() };
    let page = send(&a, get("/profile", &o)).await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.body.contains("Your profile") && page.body.contains("Anna") && page.body.contains("action=\"/profile?csrf="));
    assert!(!page.body.contains("/admin/users"), "a member's menu has no administration");
    assert!(page.body.contains("class=\"lyre\""), "without a picture the user gets the lyre");
    let de = send(&a, get("/profile", &Opts { cookie: Some(&cookie), lang: Some("de"), ..Default::default() })).await;
    assert!(de.body.contains("Dein Profil"));

    let boss = login(&a, &format!("name=Boss&pin={PIN}")).await;
    let boss_cookie = boss.header("set-cookie").unwrap().split(';').next().unwrap().to_owned();
    let page = send(&a, get("/admin/", &Opts { cookie: Some(&boss_cookie), ..Default::default() })).await;
    assert!(page.body.contains("/admin/users") && page.body.contains("aria-current=\"page\""), "an administrator's menu marks the page");
    assert_eq!(page.body.matches("href=\"/admin/\"").count(), 1, "the logo is the only link to the dashboard");
    assert!(page.body.contains("class=\"brand\" href=\"/admin/\" title=\"Dashboard\" aria-current=\"page\""), "and it is marked on the dashboard");

    assert_eq!(send(&a, get("/profile", &Opts::default())).await.status, StatusCode::SEE_OTHER, "nobody anonymous");
}

#[tokio::test]
async fn a_picture_is_stored_checked_and_served_only_to_signed_in_users() {
    let a = app().await;
    let (anna, cookie, csrf) = member_session(&a).await;

    let r = send(&a, multipart(&csrf, &cookie, "me.png", PNG)).await;
    assert_eq!((r.status, r.header("location")), (StatusCode::SEE_OTHER, Some("/profile?msg=ok.avatar_saved")));
    let o = Opts { cookie: Some(&cookie), ..Default::default() };
    let image = send(&a, get(&format!("/avatar/{anna}"), &o)).await;
    assert_eq!(image.status, StatusCode::OK);
    assert_eq!(image.header("content-type"), Some("image/png"));
    assert_eq!(image.header("x-content-type-options"), Some("nosniff"));
    let page = send(&a, get("/profile", &o)).await;
    assert!(page.body.contains(&format!("/avatar/{anna}")), "the page shows the picture");

    // The type comes from the bytes, not from the file name: a script named .png is refused.
    let r = send(&a, multipart(&csrf, &cookie, "x.png", b"<svg onload=alert(1)>")).await;
    assert_eq!(r.header("location"), Some("/profile?msg=err.avatar_type"));
    // Too large.
    let big = vec![0x89; 300 * 1024];
    let r = send(&a, multipart(&csrf, &cookie, "big.png", &big)).await;
    assert!(r.status == StatusCode::PAYLOAD_TOO_LARGE || r.header("location") == Some("/profile?msg=err.avatar_size"), "{:?}", r.status);

    // Not for anonymous visitors, and not without the token.
    assert_eq!(send(&a, get(&format!("/avatar/{anna}"), &Opts::default())).await.status, StatusCode::SEE_OTHER);
    assert_eq!(send(&a, multipart("wrong", &cookie, "me.png", PNG)).await.status, StatusCode::FORBIDDEN);

    // A conditional request costs nothing.
    let etag = image.header("etag").unwrap().to_owned();
    let mut req = get(&format!("/avatar/{anna}"), &o);
    req.headers_mut().insert(header::IF_NONE_MATCH, etag.parse().unwrap());
    assert_eq!(send(&a, req).await.status, StatusCode::NOT_MODIFIED);

    let r = send(&a, post("/profile/avatar.remove", &form(&[("csrf", &csrf)]), &o)).await;
    assert_eq!(r.header("location"), Some("/profile?msg=ok.avatar_removed"));
    assert_eq!(send(&a, get(&format!("/avatar/{anna}"), &o)).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_pin_changes_only_with_the_current_one() {
    let a = app().await;
    let (anna, cookie, csrf) = member_session(&a).await;
    let other = a.store.create_session(anna, Role::Member, None).await.unwrap();
    let device = a.store.add_device(david_server::NewDevice { user_id: anna, name: "Phone".into(), wg_pubkey: "K".repeat(43) + "=", tunnel_ip: "10.77.0.2".into(), shared: false, created_by: None }).await.unwrap();
    let device_session = a.store.create_session(anna, Role::Member, Some(device)).await.unwrap();
    let o = Opts { cookie: Some(&cookie), ..Default::default() };
    let change = |current: &str, new: &str, repeat: &str| form(&[("csrf", &csrf), ("current", current), ("new", new), ("repeat", repeat)]);

    let r = send(&a, post("/profile/pin", &change("000000", "445566", "445566"), &o)).await;
    assert_eq!(r.header("location"), Some("/profile?msg=err.pin_wrong"));
    let r = send(&a, post("/profile/pin", &change("112233", "445566", "445567"), &o)).await;
    assert_eq!(r.header("location"), Some("/profile?msg=err.pin_mismatch"));
    let r = send(&a, post("/profile/pin", &change("112233", "12ab", "12ab"), &o)).await;
    assert_eq!(r.header("location"), Some("/profile?msg=err.invalid_pin"));

    let r = send(&a, post("/profile/pin", &change("112233", "445566", "445566"), &o)).await;
    assert_eq!(r.header("location"), Some("/profile?msg=ok.pin_changed"));
    assert_eq!(login(&a, "name=Anna&pin=112233").await.status, StatusCode::UNAUTHORIZED, "the old PIN no longer works");
    assert_eq!(login(&a, "name=Anna&pin=445566").await.status, StatusCode::SEE_OTHER);

    // Other browsers are signed out; this one and the paired device stay.
    assert!(a.store.session_for_token(&other).await.unwrap().is_none());
    assert!(a.store.session_for_token(&device_session).await.unwrap().is_some());
    assert_eq!(send(&a, get("/profile", &o)).await.status, StatusCode::OK);
}

#[tokio::test]
async fn wrong_current_pins_count_towards_the_lock() {
    let a = app().await;
    let (_, cookie, csrf) = member_session(&a).await;
    let o = Opts { cookie: Some(&cookie), ..Default::default() };
    let body = form(&[("csrf", &csrf), ("current", "000000"), ("new", "445566"), ("repeat", "445566")]);
    for _ in 0..5 {
        send(&a, post("/profile/pin", &body, &o)).await;
    }
    let r = send(&a, post("/profile/pin", &form(&[("csrf", &csrf), ("current", "112233"), ("new", "445566"), ("repeat", "445566")]), &o)).await;
    assert_eq!(r.header("location"), Some("/profile?msg=err.locked"), "guessing the PIN here is no way around the lock");
}


#[tokio::test]
async fn the_logo_is_an_image_with_the_name_as_its_text() {
    let a = app().await;
    let login = send(&a, get("/admin/login", &Opts::default())).await;
    assert!(login.body.contains("<img class=\"logo\" src=\"/admin/static/logo.svg\" alt=\"David\">"), "{}", login.body);
    assert!(login.body.contains("rel=\"icon\""));
    for path in ["/admin/static/logo.svg", "/admin/static/favicon.svg"] {
        let r = send(&a, get(path, &Opts::default())).await;
        assert_eq!(r.status, StatusCode::OK, "{path}");
        assert_eq!(r.header("content-type"), Some("image/svg+xml"));
        assert!(r.body.starts_with("<!--") && r.body.contains("<svg"));
    }
    let de = send(&a, get("/admin/login", &Opts { lang: Some("de"), ..Default::default() })).await;
    assert!(de.body.contains("alt=\"David\""), "the name is the same in every language");
}


// The sign-in is protected by a cookie of its own

#[tokio::test]
async fn the_sign_in_page_sets_a_cookie_and_its_form_carries_the_matching_token() {
    let a = app().await;
    let page = send(&a, get("/admin/login", &Opts::default())).await;
    let set = page.header("set-cookie").unwrap();
    for part in ["david_login=", "HttpOnly", "SameSite=Strict", "Path=/admin"] {
        assert!(set.contains(part), "{part} missing in {set}");
    }
    let (cookie, token) = login_page_token(&a).await;
    let value = cookie.strip_prefix("david_login=").unwrap();
    assert_eq!(token, csrf_token(value), "the form's token is derived from the cookie, not stored");
    assert!(page.body.contains("name=\"remember\"") && page.body.contains("Remember this device"));

    // A visitor who has the cookie gets the same token again (second tab), and no new cookie.
    let o = Opts { cookie: Some(&cookie), ..Default::default() };
    let again = send(&a, get("/admin/login", &o)).await;
    assert!(again.header("set-cookie").is_none() && again.body.contains(&token));
}

#[tokio::test]
async fn a_sign_in_that_does_not_come_from_our_own_page_is_refused_before_any_pin_is_tried() {
    let a = app().await;
    let good = format!("name=Boss&pin={PIN}");
    let (cookie, token) = login_page_token(&a).await;
    let with_cookie = Opts { cookie: Some(&cookie), ..Default::default() };

    // No cookie, no token, a wrong token, a token of another cookie: all refused.
    for (opts, body) in [
        (Opts::default(), good.clone()),
        (Opts::default(), format!("{good}&csrf={token}")),
        (Opts { cookie: Some(&cookie), ..Default::default() }, good.clone()),
        (Opts { cookie: Some(&cookie), ..Default::default() }, format!("{good}&csrf=wrong")),
    ] {
        let r = send(&a, post("/admin/login", &body, &opts)).await;
        assert_eq!(r.status, StatusCode::FORBIDDEN, "{body}");
        assert!(r.body.contains("expired") && !r.body.contains("Boss"), "the page says why and reflects nothing");
    }
    let other_cookie = format!("david_login={}", "a".repeat(64));
    let o = Opts { cookie: Some(&other_cookie), ..Default::default() };
    assert_eq!(send(&a, post("/admin/login", &format!("{good}&csrf={token}"), &o)).await.status, StatusCode::FORBIDDEN);

    // A refused request is no wrong PIN: it does not count towards the lock.
    for _ in 0..8 {
        send(&a, post("/admin/login", "name=Boss&pin=000000", &Opts::default())).await;
    }
    let r = send(&a, post("/admin/login", &format!("{good}&csrf={token}"), &with_cookie)).await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "the account was not locked by forged requests");
    // The refused answer hands out a fresh token, so the user can try again at once.
    let retry = send(&a, post("/admin/login", &good, &Opts::default())).await;
    assert!(retry.header("set-cookie").unwrap().starts_with("david_login="));
}

#[tokio::test]
async fn the_sign_in_cookie_is_used_up_by_the_sign_in() {
    let a = app().await;
    let r = login(&a, &format!("name=Boss&pin={PIN}")).await;
    let cookies: Vec<&str> = r.headers.get_all("set-cookie").iter().filter_map(|v| v.to_str().ok()).collect();
    assert!(cookies.iter().any(|c| c.starts_with("david_session=")));
    assert!(cookies.iter().any(|c| c.starts_with("david_login=;") && c.contains("Max-Age=0")), "{cookies:?}");
}

#[tokio::test]
async fn remembering_the_device_decides_how_long_the_sign_in_lasts() {
    let a = app().await;
    let kept = login(&a, &format!("name=Boss&pin={PIN}&remember=1")).await;
    let kept_cookie = kept.header("set-cookie").unwrap();
    assert!(kept_cookie.contains("Max-Age="), "remembered: the cookie outlives the browser: {kept_cookie}");

    let forgotten = login(&a, &format!("name=Boss&pin={PIN}")).await;
    let cookie = forgotten.header("set-cookie").unwrap();
    assert!(!cookie.contains("Max-Age=") && !cookie.contains("Expires="), "not remembered: a cookie of the browser session: {cookie}");
    // On the server it also ends within a day, not after the 30 days of the role.
    let token = cookie.split(';').next().unwrap().strip_prefix("david_session=").unwrap();
    let info = a.store.session_for_token(token).await.unwrap().unwrap();
    assert!(info.expires_at - david_server::now_ms() <= 12 * 60 * 60 * 1000 + 1000);
    let kept_token = kept_cookie.split(';').next().unwrap().strip_prefix("david_session=").unwrap();
    let kept_info = a.store.session_for_token(kept_token).await.unwrap().unwrap();
    assert!(kept_info.expires_at - david_server::now_ms() > 24 * 60 * 60 * 1000, "the policy of the role applies");
}


// Popup menus and the planner grid

#[tokio::test]
async fn the_menus_are_popups_with_a_media_menu_and_a_user_menu() {
    let a = app().await;
    let (cookie, _) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), ..Default::default() };
    let page = send(&a, get("/admin/users", &o)).await.body;

    // The media menu holds the library kinds; music is a page now, videos are only announced.
    assert!(page.contains("<details class=\"popup up\" id=\"popup-media\">"), "{page}");
    for item in ["/admin/books", "/admin/music", "Music", "Videos"] {
        assert!(page.contains(item), "{item}");
    }
    assert_eq!(page.matches("aria-disabled=\"true\"").count(), 1, "only videos are not a link yet");
    assert!(page.matches("soon").count() >= 1);

    // The user menu: picture, name, profile, sign out.
    assert!(page.contains("id=\"popup-user\""));
    let menu = &page[page.find("id=\"popup-user\"").unwrap()..];
    assert!(menu.contains("href=\"/profile\"") && menu.contains("action=\"/admin/logout\"") && menu.contains("Sign out"));
    assert!(page.contains("/admin/static/ui.js"));

    let de = send(&a, get("/admin/users", &Opts { cookie: Some(&cookie), lang: Some("de"), ..Default::default() })).await.body;
    assert!(de.contains("Medien") && de.contains("Musik") && de.contains("bald") && de.contains("Abmelden"));

    let js = send(&a, get("/admin/static/ui.js", &Opts::default())).await;
    assert_eq!((js.status, js.header("content-type")), (StatusCode::OK, Some("text/javascript; charset=utf-8")));
    assert!(js.body.contains("details.popup") && js.body.contains("table.planner"));
}

#[tokio::test]
async fn the_planner_is_a_grid_with_cells_that_know_their_day_and_quarter_hour() {
    let a = app().await;
    let (cookie, _) = sign_in(&a).await;
    let kid = a.store.create_user(NewProfile::child("Kid", 8, daytime_schedule())).await.unwrap();
    let page = send(&a, get(&format!("/admin/schedule?user={kid}"), &Opts { cookie: Some(&cookie), ..Default::default() })).await.body;
    assert_eq!(page.matches("class=\"slot").count(), 7 * 96, "every quarter hour of every day is a cell");
    assert!(page.contains("data-day=\"6\" data-slot=\"95\""));
    assert_eq!(page.matches("rowspan=\"4\"").count(), 24, "one label per hour");
    assert!(page.contains("data-hour=\"23\"") && page.contains("data-day=\"0\" title="));
    // 07:00 to 19:00 is allowed on every day: 12 hours = 48 quarters, 7 days.
    assert_eq!(page.matches("class=\"slot on\"").count(), 7 * 48);
    assert!(page.contains("<strong class=\"planner-total\">84.00</strong>"));
    assert!(page.contains("Drag across the grid"));
}


#[tokio::test]
async fn the_profile_page_is_the_users_own_dashboard() {
    let a = app().await;
    let kim = a.store.create_user(NewProfile::child("Kim", 12, daytime_schedule())).await.unwrap();
    let token = a.store.create_session(kim, Role::Child, None).await.unwrap();
    let cookie = format!("david_session={token}");
    a.store.add_device(david_server::NewDevice { user_id: kim, name: "Kims Phone".into(), wg_pubkey: "K".repeat(43) + "=", tunnel_ip: "10.77.0.2".into(), shared: false, created_by: None }).await.unwrap();
    a.store.import_books(vec![book("Author", "Story", &[1])]).await.unwrap();
    let id = a.store.find_book("Author/Story").await.unwrap().unwrap().id;
    a.store.release_book(id, 6).await.unwrap();
    let track = a.store.tracks_of(id).await.unwrap()[0].id;
    a.store.save_progress(david_server::ProgressUpdate { profile_id: kim, book_id: id, track_id: track, position_ms: 120_000, finished: false, updated_at: david_server::now_ms() }).await.unwrap();
    a.store.flush().await.unwrap();

    let page = send(&a, get("/profile", &Opts { cookie: Some(&cookie), ..Default::default() })).await;
    assert_eq!(page.status, StatusCode::OK);
    let b = &page.body;
    assert!(!b.contains("Dashboard") && b.contains("Child") && b.contains("Age 12"), "role and age of a child, as badges in the strip");
    assert!(b.contains("Devices 1 / 3"), "one of the three devices a child may have");
    assert_eq!(b.matches("1 / 3").count(), 1, "the count is shown once, as a badge; the list below has only the devices");
    assert!(b.contains("Kims Phone") && b.contains("never"), "the device, not yet seen");
    assert!(b.contains("Story") && b.contains("50%") && b.contains("bar-fill w-50"), "the book in progress, half way");
    assert!(b.contains("84 h per week"), "a child sees their hours");
    assert!(b.contains("allowed now") || b.contains("not now"));
    // Other users' things do not appear.
    let (_, other_cookie, _) = member_session(&a).await;
    let other = send(&a, get("/profile", &Opts { cookie: Some(&other_cookie), ..Default::default() })).await.body;
    assert!(!other.contains("Kims Phone") && !other.contains("Story") && !other.contains("Continue listening") && !other.contains("Your devices"), "a person with nothing to list gets no empty cards");
    assert!(!other.contains("Listening hours per week"), "a member has no planner");
}


#[tokio::test]
async fn the_language_of_the_profile_beats_the_browser_and_the_apps_get_it() {
    let a = app().await;
    let (anna, cookie, csrf) = member_session(&a).await;
    let o_de_browser = Opts { cookie: Some(&cookie), lang: Some("de-DE,de;q=0.9"), ..Default::default() };

    // Nothing chosen: the browser decides.
    let page = send(&a, get("/profile", &o_de_browser)).await.body;
    assert!(page.contains("Dein Profil") && page.contains("<option value=\"\" selected>"), "automatic is the default");

    // English chosen: it wins over a German browser; the message comes in English.
    let r = send(&a, post("/profile/language", &form(&[("csrf", &csrf), ("language", "en")]), &o_de_browser)).await;
    assert_eq!(r.header("location"), Some("/profile?msg=ok.language_saved"));
    let page = send(&a, get("/profile?msg=ok.language_saved", &o_de_browser)).await.body;
    assert!(page.contains("Your profile") && page.contains("The language was saved.") && page.contains("<option value=\"en\" selected>"));
    let users_page = send(&a, get("/admin/users", &o_de_browser)).await;
    assert_eq!(users_page.status, StatusCode::FORBIDDEN, "a member still has no administration");

    // German chosen, English browser.
    let o_en = Opts { cookie: Some(&cookie), lang: Some("en"), ..Default::default() };
    send(&a, post("/profile/language", &form(&[("csrf", &csrf), ("language", "de")]), &o_en)).await;
    assert!(send(&a, get("/profile", &o_en)).await.body.contains("Dein Profil"));
    assert_eq!(a.store.profile_by_id(anna).await.unwrap().unwrap().language.as_deref(), Some("de"));

    // Back to automatic; nonsense is refused.
    send(&a, post("/profile/language", &form(&[("csrf", &csrf), ("language", "")]), &o_en)).await;
    assert!(send(&a, get("/profile", &o_en)).await.body.contains("Your profile"));
    let r = send(&a, post("/profile/language", &form(&[("csrf", &csrf), ("language", "klingon")]), &o_en)).await;
    assert_eq!(r.header("location"), Some("/profile?msg=err.invalid_input"));
    assert_eq!(a.store.profile_by_id(anna).await.unwrap().unwrap().language, None);
}


// The edit window: one form, Cancel and Save

#[tokio::test]
async fn the_edit_window_changes_everything_in_one_go() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), htmx: true, ..Default::default() };
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    a.store.set_pin(anna, "112233").await.unwrap();

    // The row carries a window with one form and the buttons Cancel and Save.
    let page = send(&a, get("/admin/users", &Opts { cookie: Some(&cookie), ..Default::default() })).await.body;
    assert!(page.contains(&format!("id=\"user-form-{anna}\"")) && page.contains("data-close>Cancel</button>"));
    assert!(page.contains(&format!("form=\"user-form-{anna}\">Save</button>")), "Save belongs to the form");
    assert!(page.matches("rpc/user.update").count() >= 2);

    let body = form(&[
        ("id", &anna.to_string()), ("name", "Annchen"), ("first_name", "Anna"), ("last_name", "Berg"), ("role", "child"),
        ("max_age", "9"), ("max_devices", "2"), ("hours_from", "8"), ("hours_until", "18"), ("pin", "445566"),
    ]);
    let r = send(&a, post("/admin/rpc/user.update", &body, &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.body.starts_with(&format!("<tr id=\"user-{anna}\">")) && r.body.contains("<strong>Annchen</strong>") && r.body.contains("Anna Berg"));
    let p = a.store.profile_by_id(anna).await.unwrap().unwrap();
    assert_eq!((p.name.as_str(), p.first_name.as_deref(), p.last_name.as_deref(), p.role.as_str(), p.max_age, p.max_devices), ("Annchen", Some("Anna"), Some("Berg"), "child", Some(9), Some(2)));
    let planner = david_server::Schedule::from_bytes(p.schedule.as_deref().unwrap()).unwrap();
    assert!(planner.is_allowed(0, 8 * 60) && !planner.is_allowed(0, 7 * 60) && !planner.is_allowed(0, 18 * 60), "8 to 18 became the planner");
    assert_eq!(login(&a, "name=Annchen&pin=445566").await.status, StatusCode::SEE_OTHER, "the new PIN works under the new name");

    // Saved again with an empty PIN and an empty age limit for a member: the PIN stays, the limit goes.
    let body = form(&[("id", &anna.to_string()), ("name", "Annchen"), ("role", "member"), ("max_age", ""), ("max_devices", ""), ("pin", "")]);
    assert_eq!(send(&a, post("/admin/rpc/user.update", &body, &o)).await.status, StatusCode::OK);
    let p = a.store.profile_by_id(anna).await.unwrap().unwrap();
    assert_eq!((p.role.as_str(), p.max_age, p.max_devices), ("member", None, None));
    assert_eq!(login(&a, "name=Annchen&pin=445566").await.status, StatusCode::SEE_OTHER, "an empty PIN keeps the PIN");
}

#[tokio::test]
async fn the_edit_window_checks_the_input_before_it_changes_anything() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), htmx: true, ..Default::default() };
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let ben = a.store.create_user(NewProfile::new("Ben", Role::Member)).await.unwrap();
    let id = anna.to_string();

    // A bad PIN, a bad number, an unknown role: nothing changes, not even the name.
    for body in [
        form(&[("id", &id), ("name", "Changed"), ("role", "member"), ("pin", "12ab")]),
        form(&[("id", &id), ("name", "Changed"), ("role", "member"), ("max_age", "99")]),
        form(&[("id", &id), ("name", "Changed"), ("role", "king")]),
    ] {
        let r = send(&a, post("/admin/rpc/user.update", &body, &o)).await;
        assert!(r.status.is_client_error(), "{body}: {}", r.status);
        assert_eq!(a.store.profile_by_id(anna).await.unwrap().unwrap().name, "Anna", "{body}");
    }
    // A child needs an age limit; a name that is taken is a conflict.
    let r = send(&a, post("/admin/rpc/user.update", &form(&[("id", &id), ("name", "Anna"), ("role", "child")]), &o)).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = send(&a, post("/admin/rpc/user.update", &form(&[("id", &id), ("name", "Ben"), ("role", "member")]), &o)).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(a.store.profile_by_id(ben).await.unwrap().unwrap().name, "Ben");
    // The last administrator cannot be demoted this way either.
    let boss = a.store.list_profiles().await.unwrap().into_iter().find(|p| p.name == "Boss").unwrap().id.to_string();
    let r = send(&a, post("/admin/rpc/user.update", &form(&[("id", &boss), ("name", "Boss"), ("role", "member")]), &o)).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn the_planner_window_has_cancel_and_save_and_the_save_belongs_to_the_grid() {
    let a = app().await;
    let (cookie, _) = sign_in(&a).await;
    let kid = a.store.create_user(NewProfile::child("Kid", 8, daytime_schedule())).await.unwrap();
    let frag = send(&a, get(&format!("/admin/fragments/schedule?user={kid}"), &Opts { cookie: Some(&cookie), ..Default::default() })).await;
    assert_eq!(frag.status, StatusCode::OK);
    assert!(!frag.body.contains("<html"), "a fragment, not a page");
    assert!(frag.body.contains(&format!("form=\"planner-form-{kid}\">Save planner</button>")) || frag.body.contains(&format!("form=\"planner-form-{kid}\">Save</button>")));
    assert!(frag.body.contains("data-close>Cancel</button>"));
    assert_eq!(frag.body.matches("data-close-window").count(), 2, "saving and removing close the window");
    assert!(!frag.body.contains("schedule.preset\" hx-post=\"/admin/rpc/schedule.preset\" hx-target=\"#schedule-panel\" hx-swap=\"outerHTML\" hx-disabled-elt=\"find button\" data-close-window"), "applying a preset keeps the window open");
    let missing = send(&a, get("/admin/fragments/schedule?user=9999", &Opts { cookie: Some(&cookie), ..Default::default() })).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    // Members may not load it.
    let (_, member_cookie, _) = member_session(&a).await;
    assert_eq!(send(&a, get(&format!("/admin/fragments/schedule?user={kid}"), &Opts { cookie: Some(&member_cookie), ..Default::default() })).await.status, StatusCode::FORBIDDEN);
}


#[tokio::test]
async fn new_user_is_a_window_with_cancel_and_create() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let page = send(&a, get("/admin/users", &Opts { cookie: Some(&cookie), ..Default::default() })).await.body;
    assert!(page.contains("data-dialog=\"dlg-new-user\"") && page.contains("<dialog id=\"dlg-new-user\""), "a button opens the window");
    assert!(page.contains("id=\"new-user-form\"") && page.contains("data-close-window data-reset"));
    assert!(page.contains("data-close>Cancel</button>") && page.contains("form=\"new-user-form\">Create</button>"));
    for field in ["name=\"name\"", "name=\"first_name\"", "name=\"last_name\"", "name=\"role\"", "name=\"birth_date\"", "name=\"pin\""] {
        assert!(page.contains(field), "{field}");
    }
    // The form of the window is the same procedure as before.
    let o = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), htmx: true, ..Default::default() };
    let r = send(&a, post("/admin/rpc/user.create", &form(&[("name", "Oma"), ("first_name", "Ingrid"), ("last_name", "Berg"), ("role", "member")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body.contains("<strong>Oma</strong>") && r.body.contains("Ingrid Berg"));
}


// The birthday decides the age limit

/// A birthday that makes someone `years` years and 10 days old today.
fn birthday_for_age(years: i16) -> String {
    let today = jiff::Zoned::now().date();
    today.checked_sub(jiff::Span::new().years(years).days(10)).unwrap().to_string()
}

#[tokio::test]
async fn a_birthday_gives_the_age_limit_and_the_limit_rises_by_itself() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), htmx: true, ..Default::default() };

    // Creating a child with a birthday: no typed limit needed.
    let born = birthday_for_age(8);
    let r = send(&a, post("/admin/rpc/user.create", &form(&[("name", "Lena"), ("role", "child"), ("birth_date", &born)]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let lena = a.store.list_profiles().await.unwrap().into_iter().find(|p| p.name == "Lena").unwrap();
    assert_eq!((lena.birth_date.as_deref(), lena.max_age), (Some(born.as_str()), Some(8)));
    assert!(r.body.contains(&born), "the list shows the birthday");

    // Opening the edit window and saving it without a typed limit does not touch the age.
    let id = lena.id.to_string();
    let r = send(&a, post("/admin/rpc/user.update", &form(&[("id", &id), ("name", "Lena"), ("role", "child"), ("birth_date", &born), ("max_devices", "")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    // The same save with the birthday field empty (a form of an older page) keeps the limit as well.
    let r = send(&a, post("/admin/rpc/user.update", &form(&[("id", &id), ("name", "Lena"), ("role", "child"), ("max_devices", "")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(a.store.profile_by_id(lena.id).await.unwrap().unwrap().max_age, Some(8));

    // A new birthday moves the limit; the server renews it when a birthday passes.
    let older = birthday_for_age(11);
    send(&a, post("/admin/rpc/user.update", &form(&[("id", &id), ("name", "Lena"), ("role", "child"), ("birth_date", &older)]), &o)).await;
    assert_eq!(a.store.profile_by_id(lena.id).await.unwrap().unwrap().max_age, Some(11));
    let next_year = jiff::Zoned::now().date().checked_add(jiff::Span::new().years(1)).unwrap().to_string();
    assert_eq!(a.store.refresh_ages_on(&next_year).await.unwrap(), 1);
    assert_eq!(a.store.profile_by_id(lena.id).await.unwrap().unwrap().max_age, Some(12), "a year later the limit is 12");
    assert_eq!(a.store.refresh_ages_on(&next_year).await.unwrap(), 0, "nothing more to renew");

    // What a book needs follows: a book rated 12 is hidden today, shown after the renewal.
    a.store.import_books(vec![book("A", "Teen", &[1])]).await.unwrap();
    let bid = a.store.find_book("A/Teen").await.unwrap().unwrap().id;
    a.store.release_book(bid, 12).await.unwrap();
    a.store.set_birth_date(lena.id, Some(&birthday_for_age(11))).await.unwrap();
    assert!(a.store.visible_books(lena.id).await.unwrap().is_empty());
    a.store.refresh_ages_on(&next_year).await.unwrap();
    assert_eq!(a.store.visible_books(lena.id).await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_birthday_that_cannot_be_one_is_refused() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), htmx: true, ..Default::default() };
    let tomorrow = jiff::Zoned::now().date().checked_add(jiff::Span::new().days(2)).unwrap().to_string();
    for bad in [tomorrow.as_str(), "1850-01-01", "2020-02-30", "yesterday"] {
        let r = send(&a, post("/admin/rpc/user.create", &form(&[("name", "Nobody"), ("role", "member"), ("birth_date", bad)]), &o)).await;
        assert!(r.status.is_client_error(), "{bad}: {}", r.status);
        assert!(a.store.list_profiles().await.unwrap().iter().all(|p| p.name != "Nobody"), "{bad}: nothing was created");
    }
    let kid = a.store.create_user(NewProfile::child("Kid", 8, daytime_schedule())).await.unwrap();
    let r = send(&a, post("/admin/rpc/user.update", &form(&[("id", &kid.to_string()), ("name", "Kid"), ("role", "child"), ("birth_date", "3000-01-01")]), &o)).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(r.body.contains("not a usable birthday"));
    assert_eq!(a.store.profile_by_id(kid).await.unwrap().unwrap().max_age, Some(8));
}

#[tokio::test]
async fn the_windows_offer_a_date_picker_for_the_birthday_and_no_typed_age() {
    let a = app().await;
    let (cookie, _) = sign_in(&a).await;
    a.store.create_user(NewProfile::child("Kid", 8, daytime_schedule())).await.unwrap();
    let page = send(&a, get("/admin/users", &Opts { cookie: Some(&cookie), ..Default::default() })).await.body;
    assert_eq!(page.matches("type=\"date\" name=\"birth_date\"").count(), 3, "one in the new-user window, one in the edit window of each of the two users");
    assert!(!page.contains("name=\"max_age\""), "the age limit is not typed any more");
    assert!(page.contains("Birthday"));
    let de = send(&a, get("/admin/users", &Opts { cookie: Some(&cookie), lang: Some("de"), ..Default::default() })).await.body;
    assert!(de.contains("Geburtstag"));
}

#[tokio::test]
async fn the_profile_page_is_not_the_dashboard_and_shows_every_role_only_their_own() {
    let a = app().await;
    let evil = a.store.create_user(NewProfile::new("<b>Eve</b>", Role::Guest)).await.unwrap();
    let token = a.store.create_session(evil, Role::Guest, None).await.unwrap();
    let cookie = format!("david_session={token}");
    let body = send(&a, get("/profile", &Opts { cookie: Some(&cookie), ..Default::default() })).await.body;
    assert!(body.contains("&lt;b&gt;Eve") && !body.contains("<b>Eve"), "names are escaped");
    assert!(body.contains("<title>Your profile") && !body.contains("Dashboard") && !body.contains("/admin/users"), "own title, no admin menu");
    assert!(body.contains("href=\"/profile\" class") || body.contains("aria-current=\"page\""), "the profile is marked as current");
    assert!(!body.contains("Age</dt>"), "no age card without an age limit");
    assert!(body.contains("Guest") && body.contains("name=\"new\"") && body.contains("name=\"language\""));
    // A taken nickname is refused, with the message; the own name is fine.
    let (_, cookie2, csrf2) = member_session(&a).await;
    let o = Opts { cookie: Some(&cookie2), ..Default::default() };
    let r = send(&a, post("/profile/names", &form(&[("csrf", &csrf2), ("nickname", "<b>Eve</b>"), ("first_name", ""), ("last_name", "")]), &o)).await;
    assert_eq!(r.header("location"), Some("/profile?msg=err.exists"));
    let r = send(&a, post("/profile/names", &form(&[("csrf", &csrf2), ("nickname", "  "), ("first_name", ""), ("last_name", "")]), &o)).await;
    assert_eq!(r.header("location"), Some("/profile?msg=err.invalid_name"));
    let page = send(&a, get("/profile?msg=err.exists", &o)).await.body;
    assert!(page.contains("aria-current=\"page\""), "{page}");
}


// The audiobook list and its window

#[tokio::test]
async fn the_audiobook_page_shows_cover_status_content_and_one_window_per_book() {
    let a = app().await;
    let (cookie, _) = sign_in(&a).await;
    a.store.import_books(vec![book("Follett", "Kinder der Freiheit", &[2, 1])]).await.unwrap();
    let id = a.store.find_book("Follett/Kinder der Freiheit").await.unwrap().unwrap().id;
    let o = Opts { cookie: Some(&cookie), ..Default::default() };

    let page = send(&a, get("/admin/books", &o)).await.body;
    assert!(page.contains("<h1>Audiobooks</h1>") && page.contains("Audiobooks</span>"), "the page and the menu item are called Audiobooks");
    assert!(page.contains("<strong>Kinder der Freiheit</strong>") && page.contains("<small>Follett</small>"));
    assert!(page.contains(&format!("src=\"/admin/cover/{id}\"")), "a thumbnail from the cover route");
    assert!(page.contains("Not released") && !page.contains("Released from"));
    assert!(page.contains("2 discs") && page.contains("3 tracks") && page.contains("0 h 12 min"), "three tracks of four minutes");
    assert!(!page.contains("<th>Actions</th>") && page.contains("<span class=\"sr-only\">Actions</span>"), "the last column has no visible label, only one for screen readers");
    assert!(page.contains(&format!("data-dialog=\"dlg-book-{id}\"")) && page.contains("form=\"book-form-") && page.contains("data-close>Cancel</button>"));
    assert!(page.contains("class=\"form-error"), "errors are shown inside the window");
    let de = send(&a, get("/admin/books", &Opts { cookie: Some(&cookie), lang: Some("de"), ..Default::default() })).await.body;
    assert!(de.contains("<h1>Hörbücher</h1>") && de.contains("Nicht freigegeben") && de.contains("2 CDs"));
}

#[tokio::test]
async fn the_book_window_releases_withdraws_and_marks_in_one_save() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), htmx: true, ..Default::default() };
    a.store.import_books(vec![book("A", "Story", &[1])]).await.unwrap();
    let id = a.store.find_book("A/Story").await.unwrap().unwrap().id;
    let sid = id.to_string();

    let r = send(&a, post("/admin/rpc/book.update", &form(&[("id", &sid), ("released", "1"), ("min_age", "12"), ("exempt", "1")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.body.starts_with(&format!("<tr id=\"book-{id}\">")) && r.body.contains("Released from 12") && r.body.contains("Learning content"));
    let row = a.store.admin_book_row(id).await.unwrap().unwrap();
    assert_eq!((row.released, row.min_age, row.quota_exempt), (true, Some(12), true));

    // Unticking "Released" withdraws it; the learning mark follows the box.
    let r = send(&a, post("/admin/rpc/book.update", &form(&[("id", &sid), ("min_age", "12")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK);
    let row = a.store.admin_book_row(id).await.unwrap().unwrap();
    assert_eq!((row.released, row.quota_exempt), (false, false));

    // A release needs an age; nonsense is refused and changes nothing.
    for body in [form(&[("id", &sid), ("released", "1")]), form(&[("id", &sid), ("released", "1"), ("min_age", "99")]), form(&[("id", "9999"), ("released", "1"), ("min_age", "6")])] {
        assert!(send(&a, post("/admin/rpc/book.update", &body, &o)).await.status.is_client_error(), "{body}");
    }
    assert!(!a.store.admin_book_row(id).await.unwrap().unwrap().released);

    // Below the recommendation needs the second tick.
    a.store.set_book_recommended_age(id, Some(14)).await.unwrap();
    let r = send(&a, post("/admin/rpc/book.update", &form(&[("id", &sid), ("released", "1"), ("min_age", "8")]), &o)).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert!(!a.store.admin_book_row(id).await.unwrap().unwrap().released);
    let r = send(&a, post("/admin/rpc/book.update", &form(&[("id", &sid), ("released", "1"), ("min_age", "8"), ("confirm", "1")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(a.store.admin_book_row(id).await.unwrap().unwrap().min_age, Some(8));
}

#[tokio::test]
async fn the_administration_serves_covers_to_administrators_only() {
    let a = app().await;
    let (cookie, _) = sign_in(&a).await;
    let (_, member_cookie, _) = member_session(&a).await;
    // No library, no picture: not found, never an error page.
    assert_eq!(send(&a, get("/admin/cover/1", &Opts { cookie: Some(&cookie), ..Default::default() })).await.status, StatusCode::NOT_FOUND);
    assert_eq!(send(&a, get("/admin/cover/1", &Opts { cookie: Some(&member_cookie), ..Default::default() })).await.status, StatusCode::FORBIDDEN);
    assert_eq!(send(&a, get("/admin/cover/1", &Opts::default())).await.status, StatusCode::SEE_OTHER);
}


#[tokio::test]
async fn the_book_window_keeps_a_blurb() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), htmx: true, ..Default::default() };
    a.store.import_books(vec![book("A", "Story", &[1])]).await.unwrap();
    let id = a.store.find_book("A/Story").await.unwrap().unwrap().id;
    let sid = id.to_string();

    // The window has a field for it.
    let page = send(&a, get("/admin/books", &Opts { cookie: Some(&cookie), ..Default::default() })).await.body;
    assert!(page.contains("<textarea name=\"description\"") && page.contains("Blurb"));

    // Saved with the rest (the blurb is the text of the back cover), escaped when shown again, line breaks kept in the field.
    let text = "Berlin 1961. <script>alert(1)</script> & more\nSecond line.";
    let r = send(&a, post("/admin/rpc/book.update", &form(&[("id", &sid), ("released", "1"), ("min_age", "12"), ("description", text)]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.body.contains("&lt;script&gt;alert(1)&lt;") && r.body.contains("&amp; more") && !r.body.contains("<script>alert(1)"), "the text is escaped");
    assert_eq!(a.store.admin_book_row(id).await.unwrap().unwrap().description.as_deref(), Some(text));

    // A form without the field (an older page) leaves it alone; an empty field removes it.
    send(&a, post("/admin/rpc/book.update", &form(&[("id", &sid), ("released", "1"), ("min_age", "12")]), &o)).await;
    assert_eq!(a.store.admin_book_row(id).await.unwrap().unwrap().description.as_deref(), Some(text));
    send(&a, post("/admin/rpc/book.update", &form(&[("id", &sid), ("released", "1"), ("min_age", "12"), ("description", "  ")]), &o)).await;
    assert_eq!(a.store.admin_book_row(id).await.unwrap().unwrap().description, None);

    // Too long: refused with a reason, and nothing else of the save happens.
    let long = "x".repeat(4001);
    let r = send(&a, post("/admin/rpc/book.update", &form(&[("id", &sid), ("released", "0"), ("min_age", "3"), ("exempt", "1"), ("description", &long)]), &o)).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(r.body.contains("at most 4000 characters"));
    let row = a.store.admin_book_row(id).await.unwrap().unwrap();
    assert_eq!((row.released, row.quota_exempt), (true, false), "the refused save changed nothing");
}


// A browser that still has the session cookie of an older version (scoped to /admin)

#[tokio::test]
async fn the_profile_page_works_for_a_browser_that_only_has_the_cookie_of_an_older_version() {
    let a = app().await;
    let (cookie, _) = sign_in(&a).await;

    // Such a browser sends the cookie to /admin/* but not to /profile.
    let r = send(&a, get("/profile", &Opts::default())).await;
    assert_eq!((r.status, r.header("location")), (StatusCode::SEE_OTHER, Some("/admin/login?next=%2Fprofile")), "the way back is remembered");

    // At the sign-in page the cookie is seen: it is issued again for the whole site, the old one removed,
    // and the person goes on to the profile, not to the dashboard.
    let o = Opts { cookie: Some(&cookie), ..Default::default() };
    let r = send(&a, get("/admin/login?next=%2Fprofile", &o)).await;
    assert_eq!((r.status, r.header("location")), (StatusCode::SEE_OTHER, Some("/profile")));
    let cookies: Vec<&str> = r.headers.get_all("set-cookie").iter().filter_map(|v| v.to_str().ok()).collect();
    assert!(cookies.iter().any(|c| c.starts_with(&format!("{cookie};")) && c.contains("Path=/") && !c.contains("Path=/admin")), "{cookies:?}");
    assert!(cookies.iter().any(|c| c.starts_with("david_session=;") && c.contains("Path=/admin") && c.contains("Max-Age=0")), "{cookies:?}");

    // Without a way back, a signed-in administrator goes to the dashboard as before.
    assert_eq!(send(&a, get("/admin/login", &o)).await.header("location"), Some("/admin/"));
    // Only our own pages are accepted as the way back: no open redirect.
    for bad in ["//evil.example", "https%3A%2F%2Fevil.example", "%2Fadmin%2Fusers", "javascript%3Aalert(1)"] {
        let r = send(&a, get(&format!("/admin/login?next={bad}"), &o)).await;
        assert_eq!(r.header("location"), Some("/admin/"), "{bad}");
    }
}

#[tokio::test]
async fn two_session_cookies_in_one_request_do_not_confuse_the_server() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    // The older cookie is first (the browser sends the more specific path first) and no longer a live session.
    let both = format!("david_session=an-old-and-dead-token; {cookie}");
    let o = Opts { cookie: Some(&both), ..Default::default() };
    assert_eq!(send(&a, get("/profile", &o)).await.status, StatusCode::OK, "the live one counts");
    assert_eq!(send(&a, get("/admin/users", &o)).await.status, StatusCode::OK);
    // The token of the live session is accepted for a change, whichever cookie comes first.
    let r = send(&a, post("/profile/language", &form(&[("csrf", &csrf), ("language", "de")]), &o)).await;
    assert_eq!(r.header("location"), Some("/profile?msg=ok.language_saved"));
    // A token that belongs to no cookie of the request is still refused.
    let r = send(&a, post("/profile/language", &form(&[("csrf", "wrong"), ("language", "en")]), &o)).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    // Signing out ends the live session although another cookie came first, and clears both scopes.
    let r = send(&a, post("/admin/logout", &form(&[("csrf", &csrf)]), &o)).await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    let cleared: Vec<&str> = r.headers.get_all("set-cookie").iter().filter_map(|v| v.to_str().ok()).collect();
    assert!(cleared.iter().any(|c| c.contains("Path=/;") || c.ends_with("Path=/") || c.contains("Path=/; ") || c.contains("Path=/;")) || cleared.len() == 2, "{cleared:?}");
    assert_eq!(send(&a, get("/profile", &Opts { cookie: Some(&cookie), ..Default::default() })).await.status, StatusCode::SEE_OTHER, "the session is gone");
}


// One form, one Save

fn profile_form(csrf: &str, cookie: &str, fields: &[(&str, &str)], file: Option<(&str, &[u8])>) -> Request<Body> {
    let boundary = "XBOUNDARYX";
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
    }
    // The file field is always there, empty when nothing was chosen, like a browser sends it.
    let (file_name, bytes) = file.unwrap_or(("", b""));
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"avatar\"; filename=\"{file_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    Request::builder()
        .method("POST")
        .uri(format!("/profile?csrf={csrf}"))
        .header(header::COOKIE, cookie)
        .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body))
        .unwrap()
}

#[tokio::test]
async fn the_profile_has_one_form_and_one_save_for_everything() {
    let a = app().await;
    let (anna, cookie, csrf) = member_session(&a).await;
    let page = send(&a, get("/profile", &Opts { cookie: Some(&cookie), ..Default::default() })).await.body;
    assert_eq!(page.matches("<form class=\"card profile-form\"").count(), 1);
    assert_eq!(page.matches(">Save</button>").count(), 1, "one Save button on the page, not one per card");
    assert!(page.contains("href=\"/profile\">Cancel</a>"));
    for field in ["name=\"nickname\"", "name=\"first_name\"", "name=\"last_name\"", "name=\"language\"", "name=\"avatar\"", "name=\"current\"", "name=\"new\"", "name=\"repeat\""] {
        assert!(page.contains(field), "{field}");
    }

    // Everything changes in one save: names, language, picture and the secret.
    let r = send(&a, profile_form(&csrf, &cookie, &[("nickname", "Annchen"), ("first_name", "Anna"), ("last_name", "Berg"), ("language", "de"), ("current", "112233"), ("new", "Sommer2026"), ("repeat", "Sommer2026")], Some(("me.png", PNG)))).await;
    assert_eq!(r.header("location"), Some("/profile?msg=ok.profile_saved"), "{}", r.body);
    let p = a.store.profile_by_id(anna).await.unwrap().unwrap();
    assert_eq!((p.name.as_str(), p.first_name.as_deref(), p.last_name.as_deref(), p.language.as_deref()), ("Annchen", Some("Anna"), Some("Berg"), Some("de")));
    assert!(a.store.has_avatar(anna).await.unwrap());
    assert_eq!(login(&a, "name=Annchen&pin=Sommer2026").await.status, StatusCode::SEE_OTHER, "the new password works");
    assert_eq!(login(&a, "name=Annchen&pin=112233").await.status, StatusCode::UNAUTHORIZED);

    // Saving with the secret fields empty keeps it; the picture box removes the picture.
    let r = send(&a, profile_form(&csrf, &cookie, &[("nickname", "Annchen"), ("language", ""), ("remove_avatar", "1")], None)).await;
    assert_eq!(r.header("location"), Some("/profile?msg=ok.profile_saved"));
    assert!(!a.store.has_avatar(anna).await.unwrap());
    assert_eq!(a.store.profile_by_id(anna).await.unwrap().unwrap().language, None);
    assert_eq!(login(&a, "name=Annchen&pin=Sommer2026").await.status, StatusCode::SEE_OTHER, "the secret was kept");
}

#[tokio::test]
async fn a_refused_save_changes_nothing() {
    let a = app().await;
    let (anna, cookie, csrf) = member_session(&a).await;
    a.store.create_user(NewProfile::new("Ben", Role::Member)).await.unwrap();
    let wants = |fields: &[(&str, &str)], file: Option<(&str, &[u8])>| profile_form(&csrf, &cookie, fields, file);
    for (what, req, key) in [
        ("a taken nickname", wants(&[("nickname", "ben"), ("first_name", "Changed")], None), "err.exists"),
        ("an empty nickname", wants(&[("nickname", " ")], None), "err.invalid_name"),
        ("two different new secrets", wants(&[("nickname", "Anna"), ("current", "112233"), ("new", "445566"), ("repeat", "445567")], None), "err.pin_mismatch"),
        ("letters only", wants(&[("nickname", "Anna"), ("current", "112233"), ("new", "abcdefgh"), ("repeat", "abcdefgh")], None), "err.invalid_pin"),
        ("too short a password", wants(&[("nickname", "Anna"), ("current", "112233"), ("new", "ab12"), ("repeat", "ab12")], None), "err.invalid_pin"),
        ("a wrong current secret", wants(&[("nickname", "Anna"), ("first_name", "Changed"), ("current", "000000"), ("new", "445566"), ("repeat", "445566")], None), "err.pin_wrong"),
        ("a script as picture", wants(&[("nickname", "Anna"), ("first_name", "Changed")], Some(("x.png", b"<svg onload=alert(1)>"))), "err.avatar_type"),
        ("a language that does not exist", wants(&[("nickname", "Anna"), ("language", "klingon")], None), "err.invalid_input"),
    ] {
        let r = send(&a, req).await;
        assert_eq!(r.header("location"), Some(format!("/profile?msg={key}").as_str()), "{what}");
        let p = a.store.profile_by_id(anna).await.unwrap().unwrap();
        assert_eq!((p.name.as_str(), p.first_name, p.language), ("Anna", None, None), "{what}: nothing changed");
    }
    assert_eq!(login(&a, "name=Anna&pin=112233").await.status, StatusCode::SEE_OTHER, "the secret is still the old one");
    // No token, no save.
    assert_eq!(send(&a, profile_form("wrong", &cookie, &[("nickname", "X")], None)).await.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_pin_and_an_alphanumeric_password_both_sign_in() {
    let a = app().await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), htmx: true, ..Default::default() };
    // An administrator can give a user either.
    for secret in ["445566", "Sommer2026"] {
        let r = send(&a, post("/admin/rpc/user.update", &form(&[("id", &anna.to_string()), ("name", "Anna"), ("role", "member"), ("pin", secret)]), &o)).await;
        assert_eq!(r.status, StatusCode::OK, "{secret}: {}", r.body);
        assert_eq!(login(&a, &format!("name=Anna&pin={secret}")).await.status, StatusCode::SEE_OTHER, "{secret}");
    }
    // What is neither is refused with the reason.
    for bad in ["abcdefgh", "ab12", "12 34", "x".repeat(65).as_str()] {
        let r = send(&a, post("/admin/rpc/user.update", &form(&[("id", &anna.to_string()), ("name", "Anna"), ("role", "member"), ("pin", bad)]), &o)).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
        assert!(r.body.contains("password has 8 to 64 characters"));
    }
    // The sign-in page asks for a PIN or a password, and the field takes text.
    let page = send(&a, get("/admin/login", &Opts::default())).await.body;
    assert!(page.contains("PIN or password") && !page.contains("inputmode=\"numeric\""));
}


#[tokio::test]
async fn static_files_are_revalidated_so_an_update_shows_at_once() {
    let a = app().await;
    for path in ["/admin/static/admin.css", "/admin/static/ui.js", "/admin/static/logo.svg", "/admin/static/htmx.min.js"] {
        let first = send(&a, get(path, &Opts::default())).await;
        assert_eq!(first.status, StatusCode::OK, "{path}");
        assert_eq!(first.header("cache-control"), Some("no-cache"), "{path}: asks the server every time");
        let etag = first.header("etag").unwrap().to_owned();
        let mut again = get(path, &Opts::default());
        again.headers_mut().insert(header::IF_NONE_MATCH, etag.parse().unwrap());
        let second = send(&a, again).await;
        assert_eq!((second.status, second.body.len()), (StatusCode::NOT_MODIFIED, 0), "{path}: unchanged costs a 304");
        let mut stale = get(path, &Opts::default());
        stale.headers_mut().insert(header::IF_NONE_MATCH, "\"0000\"".parse().unwrap());
        assert_eq!(send(&a, stale).await.status, StatusCode::OK, "{path}: another version is sent in full");
    }
}


#[tokio::test]
async fn the_pairing_page_has_a_window_for_new_codes_and_a_tidy_device_list() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let anna = a.store.create_user(NewProfile::new("Anna", Role::Member)).await.unwrap();
    let d = a.store.add_device(david_server::NewDevice { user_id: anna, name: "Annas Phone".into(), wg_pubkey: "K".repeat(43) + "=", tunnel_ip: "10.77.0.2".into(), shared: true, created_by: None }).await.unwrap();
    let o = Opts { cookie: Some(&cookie), ..Default::default() };

    let page = send(&a, get("/admin/pairing", &o)).await.body;
    assert!(page.contains("cannot reach 127.0.0.1"), "a loopback address gets the warning");
    assert!(page.contains("<h1>Pair a device</h1>") && page.contains("data-dialog=\"dlg-pair\"") && page.contains("<dialog id=\"dlg-pair\""));
    assert!(page.contains("data-close>Cancel</button>") && page.contains("form=\"pair-form\">Create code</button>") && page.contains("id=\"pair-body\"") && page.contains("id=\"pair-foot\"") && !page.contains("data-close-window"), "the code replaces the form inside the same window");
    assert!(page.contains("<strong>Annas Phone</strong>") && page.contains("badge\">shared") && page.contains("badge ok\">Active"));
    assert!(page.contains("<th>Last seen</th>") && !page.contains("<th>Actions</th>"), "no visible Actions label");
    assert!(page.contains("hx-confirm=") && page.contains("Revoke"), "revoking asks first");

    // Revoking changes the badge; the row stays a row.
    let ho = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), htmx: true, ..Default::default() };
    let r = send(&a, post("/admin/rpc/device.revoke", &form(&[("id", &d.to_string())]), &ho)).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body.starts_with(&format!("<tr id=\"device-{d}\">")) && r.body.contains("badge muted\">Revoked") && !r.body.contains("device.revoke"));

    // The code, when made, comes as a card with the QR code and the address.
    let r = send(&a, post("/admin/rpc/pairing.create", &form(&[("id", &anna.to_string()), ("address", "192.168.1.5:8484")]), &ho)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.body.contains("class=\"card pairing-result\"") && r.body.contains("<svg") && r.body.contains("david:&#x2f;&#x2f;pair"));
    assert!(r.body.contains("id=\"pair-foot\"") && r.body.contains("hx-swap-oob") && r.body.contains(">Close</a>"), "the window gets a Close button");
    // Without JavaScript the page answers with the code in place and no window parts.
    let plain = send(&a, post("/admin/rpc/pairing.create", &form(&[("id", &anna.to_string()), ("address", "192.168.1.5:8484")]), &Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), ..Default::default() })).await;
    assert!(plain.body.contains("pairing-result") && !plain.body.contains("hx-swap-oob"));
    let de = send(&a, get("/admin/pairing", &Opts { cookie: Some(&cookie), lang: Some("de"), ..Default::default() })).await.body;
    assert!(de.contains("<h1>Gerät koppeln</h1>") && de.contains("Zuletzt gesehen") && de.contains("Code erzeugen"));
}

/// A multipart request with text fields and, optionally, a picture in the field `cover`.
fn tags_request(csrf: &str, cookie: &str, fields: &[(&str, &str)], cover: Option<&[u8]>) -> Request<Body> {
    let boundary = "XBOUNDARYX";
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
    }
    if let Some(bytes) = cover {
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"cover\"; filename=\"c.png\"\r\nContent-Type: image/png\r\n\r\n").as_bytes());
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Request::builder()
        .method("POST")
        .uri(format!("/admin/rpc/book.tags?csrf={csrf}"))
        .header(header::COOKIE, cookie)
        .header("hx-request", "true")
        .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body))
        .unwrap()
}

#[tokio::test]
async fn the_tags_window_writes_the_album_tags_and_the_cover_into_every_file() {
    if !common::audio::ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let lib = tempfile::TempDir::new().unwrap();
    let dir = lib.path().join("Ken Follett/Kinder der Freiheit");
    common::audio::make_mp3(&dir.join("a_CD01_TR01.mp3"), &[("album", "Old"), ("artist", "Old Artist")]);
    common::audio::make_mp3(&dir.join("a_CD01_TR02.mp3"), &[]);
    let root = std::fs::canonicalize(lib.path()).unwrap().to_string_lossy().into_owned();
    send(&a, post("/admin/rpc/import.save_criteria", &form(&[("roots", &root), ("rule", "manual")]), &o)).await;
    send(&a, post("/admin/rpc/import.unlock", "", &o)).await;
    send(&a, post("/admin/rpc/import.run", "", &o)).await;
    let id = a.store.admin_book_rows().await.unwrap()[0].id;

    // The window shows what the first file says.
    let w = send(&a, get(&format!("/admin/rpc/book.tags_form?id={id}"), &o)).await;
    assert_eq!(w.status, StatusCode::OK, "{}", w.body);
    assert!(w.body.contains("value=\"Old\"") && w.body.contains("value=\"Old Artist\"") && w.body.contains("data-paste-image"));

    // Refusals change nothing: a bad year, a picture that is none.
    let r = send(&a, tags_request(&csrf, &cookie, &[("id", &id.to_string()), ("album", "X"), ("year", "20x6")], None)).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = send(&a, tags_request(&csrf, &cookie, &[("id", &id.to_string()), ("album", "X")], Some(b"<svg onload=alert(1)>"))).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(david_server::tags::read(&dir.join("a_CD01_TR01.mp3")).unwrap().album.as_deref(), Some("Old"));

    // A good save: tags in both files, the picture as the folder's file and embedded in both.
    let r = send(&a, tags_request(&csrf, &cookie, &[("id", &id.to_string()), ("album", "Kinder der Freiheit"), ("artist", "Ken Follett"), ("year", "2010"), ("genre", "Hörbuch")], Some(PNG))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    for f in ["a_CD01_TR01.mp3", "a_CD01_TR02.mp3"] {
        let t = david_server::tags::read(&dir.join(f)).unwrap();
        assert_eq!((t.album.as_deref(), t.artist.as_deref(), t.album_artist.as_deref()), (Some("Kinder der Freiheit"), Some("Ken Follett"), Some("Ken Follett")), "{f}");
        assert!(david_server::tags::embedded_cover(&dir.join(f)).is_some(), "{f} has the picture");
    }
    assert!(dir.join("cover.png").is_file());
    let again = david_server::tags::read_album(&dir.join("a_CD01_TR01.mp3"));
    assert_eq!((again.year.as_deref(), again.genre.as_deref()), (Some("2010"), Some("Hörbuch")));

    // The cover route revalidates, so the new picture shows at once.
    let c = send(&a, get(&format!("/admin/cover/{id}"), &o)).await;
    assert_eq!(c.status, StatusCode::OK);
}

fn upload_request(csrf: &str, cookie: &str, file_name: &str, bytes: &[u8]) -> Request<Body> {
    upload_request_of(csrf, cookie, file_name, bytes, None)
}

/// Like `upload_request`, with the choice "audiobooks or music" (a field before the file, as the form sends it).
fn upload_request_of(csrf: &str, cookie: &str, file_name: &str, bytes: &[u8], kind: Option<&str>) -> Request<Body> {
    let boundary = "XBOUNDARYX";
    let mut body = Vec::new();
    if let Some(kind) = kind {
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"kind\"\r\n\r\n{kind}\r\n").as_bytes());
    }
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"archive\"; filename=\"{file_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    Request::builder()
        .method("POST")
        .uri(format!("/admin/rpc/upload.create?csrf={csrf}"))
        .header(header::COOKIE, cookie)
        .header("hx-request", "true")
        .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body))
        .unwrap()
}

async fn wait_for_uploads(a: &App) -> Vec<david_server::UploadJob> {
    for _ in 0..200 {
        let jobs = a.store.upload_jobs(20).await.unwrap();
        if !jobs.is_empty() && jobs.iter().all(|j| j.state == "done" || j.state == "failed") {
            return jobs;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("the upload queue did not finish");
}

#[tokio::test]
async fn uploaded_archives_are_queued_unpacked_and_imported() {
    let tools = ["7z", "zip", "tar"].iter().all(|t| std::process::Command::new(t).arg("--help").output().is_ok());
    if !tools || !common::audio::ffmpeg_available() {
        eprintln!("skipped: 7z, zip, tar or ffmpeg is missing");
        return;
    }
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let library = tempfile::TempDir::new().unwrap();
    let root = std::fs::canonicalize(library.path()).unwrap().to_string_lossy().into_owned();
    send(&a, post("/admin/rpc/import.save_criteria", &form(&[("roots", &root), ("rule", "manual")]), &o)).await;
    send(&a, post("/admin/rpc/import.unlock", "", &o)).await;

    // An album as a folder of two files (the tags say who and what), once as ZIP and once as TAR.GZ.
    let work = tempfile::TempDir::new().unwrap();
    let album = work.path().join("some folder");
    common::audio::make_mp3(&album.join("t1.mp3"), &[("artist", "Ken Follett"), ("album", "Winter der Welt")]);
    common::audio::make_mp3(&album.join("t2.mp3"), &[("artist", "Ken Follett"), ("album", "Winter der Welt")]);
    std::fs::write(album.join("notes.exe"), b"not audio").unwrap();
    let zip = work.path().join("book.zip");
    assert!(std::process::Command::new("zip").current_dir(work.path()).args(["-qr", "book.zip", "some folder"]).status().unwrap().success());
    let tgz = work.path().join("book2.tar.gz");
    assert!(std::process::Command::new("tar").current_dir(work.path()).args(["czf", "book2.tar.gz", "some folder"]).status().unwrap().success());

    // A wrong kind is refused at once, and nothing is queued.
    let r = send(&a, upload_request(&csrf, &cookie, "virus.exe", b"MZ")).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(a.store.upload_jobs(5).await.unwrap().is_empty());

    let r = send(&a, upload_request(&csrf, &cookie, "book.zip", &std::fs::read(&zip).unwrap())).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.body.contains("book.zip") && r.body.contains("id=\"upload-queue\""));
    let jobs = wait_for_uploads(&a).await;
    assert_eq!(jobs[0].state, "done", "{}", jobs[0].message);

    // The files went to Author/Title below the import folder; the non-audio file did not.
    let dest = library.path().join("Ken Follett/Winter der Welt");
    assert!(dest.join("t1.mp3").is_file() && dest.join("t2.mp3").is_file() && !dest.join("notes.exe").exists());
    let books = a.store.admin_book_rows().await.unwrap();
    assert!(books.iter().any(|b| b.title == "Winter der Welt" && b.tracks == 2), "imported: {books:?}");

    // The second archive waits behind the first and ends the same way (a compressed tarball is unpacked twice).
    let r = send(&a, upload_request(&csrf, &cookie, "book2.tar.gz", &std::fs::read(&tgz).unwrap())).await;
    assert_eq!(r.status, StatusCode::OK);
    let jobs = loop {
        let jobs = wait_for_uploads(&a).await;
        if jobs.len() == 2 {
            break jobs;
        }
    };
    assert!(jobs.iter().all(|j| j.state == "done"), "{jobs:?}");

    // Something that is no archive fails with a reason and does not stop the queue.
    let r = send(&a, upload_request(&csrf, &cookie, "broken.zip", b"this is not a zip file")).await;
    assert_eq!(r.status, StatusCode::OK);
    let jobs = loop {
        let jobs = wait_for_uploads(&a).await;
        if jobs.len() == 3 {
            break jobs;
        }
    };
    assert_eq!(jobs[0].state, "failed");
    assert!(jobs[0].message.contains("cannot be read") || jobs[0].message.contains("could not be unpacked"), "{}", jobs[0].message);

    // The page shows the queue.
    let page = send(&a, get("/admin/import", &Opts { cookie: Some(&cookie), ..Default::default() })).await.body;
    assert!(page.contains("id=\"upload-queue\"") && page.contains("book.zip") && page.contains("dlg-upload"));
}

#[tokio::test]
async fn a_book_can_be_deleted_with_or_without_its_files() {
    if !common::audio::ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let lib = tempfile::TempDir::new().unwrap();
    let keep = lib.path().join("Ken Follett/Keep Me");
    let drop = lib.path().join("Ken Follett/Drop Me");
    common::audio::make_mp3(&keep.join("a1.mp3"), &[]);
    common::audio::make_mp3(&drop.join("b1.mp3"), &[]);
    common::audio::make_mp3(&drop.join("b2.mp3"), &[]);
    std::fs::write(drop.join("cover.jpg"), b"x").unwrap();
    std::fs::write(drop.join("notes.txt"), b"something of the user").unwrap();
    let root = std::fs::canonicalize(lib.path()).unwrap().to_string_lossy().into_owned();
    send(&a, post("/admin/rpc/import.save_criteria", &form(&[("roots", &root), ("rule", "manual")]), &o)).await;
    send(&a, post("/admin/rpc/import.unlock", "", &o)).await;
    send(&a, post("/admin/rpc/import.run", "", &o)).await;
    let books = a.store.admin_book_rows().await.unwrap();
    let id_of = |t: &str| books.iter().find(|b| b.title == t).unwrap().id;
    let (keep_id, drop_id) = (id_of("Keep Me"), id_of("Drop Me"));

    // The window offers it, with the choice about the files.
    let page = send(&a, get("/admin/books", &Opts { cookie: Some(&cookie), ..Default::default() })).await.body;
    assert!(page.contains("/admin/rpc/book.delete") && page.contains("name=\"files\"") && page.contains("hx-confirm="));

    // Without the files: gone from the library, files stay, and an import brings it back.
    let r = send(&a, post("/admin/rpc/book.delete", &form(&[("id", &keep_id.to_string())]), &o)).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body.is_empty());
    assert!(keep.join("a1.mp3").is_file());
    assert!(a.store.admin_book_row(keep_id).await.unwrap().is_none());
    send(&a, post("/admin/rpc/import.run", "", &o)).await;
    assert!(a.store.admin_book_rows().await.unwrap().iter().any(|b| b.title == "Keep Me"), "an import finds it again");

    // With the files: audio and the picture go; what is not ours stays, and so does its folder.
    let r = send(&a, post("/admin/rpc/book.delete", &form(&[("id", &drop_id.to_string()), ("files", "1")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(!drop.join("b1.mp3").exists() && !drop.join("b2.mp3").exists() && !drop.join("cover.jpg").exists());
    assert!(drop.join("notes.txt").is_file(), "a file of the user is never deleted");
    assert!(a.store.admin_book_row(drop_id).await.unwrap().is_none());

    // An unknown book is a 404, and a member cannot do it.
    let r = send(&a, post("/admin/rpc/book.delete", &form(&[("id", "99999")]), &o)).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn nothing_big_is_written_when_the_disk_would_run_below_the_reserve() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    // The reserve is a setting with a line edit; the largest value is more than any disk has free.
    let r = send(&a, post("/admin/rpc/settings.save", &form(&[("min_free_gb", "100000")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(a.store.setting("min_free_gb").await.unwrap().as_deref(), Some("100000"));

    // An upload is refused before it is queued, with its reason.
    let r = send(&a, upload_request(&csrf, &cookie, "book.zip", &vec![0u8; 100_000])).await;
    assert_eq!(r.status, StatusCode::INSUFFICIENT_STORAGE);
    assert!(r.body.contains("free disk space"), "{}", r.body);
    assert!(a.store.upload_jobs(5).await.unwrap().is_empty());

    // Zero turns the protection off again.
    send(&a, post("/admin/rpc/settings.save", &form(&[("min_free_gb", "0")]), &o)).await;
    assert_eq!(diskspace_reserve(&a).await, 0);
    let r = send(&a, get("/admin/settings?tab=values", &Opts { cookie: Some(&cookie), ..Default::default() })).await;
    assert!(r.body.contains("name=\"min_free_gb\"") && r.body.contains("GB"));
}

async fn diskspace_reserve(a: &App) -> u64 {
    david_server::diskspace::reserve(&a.store).await
}

#[tokio::test]
async fn music_and_narrators_are_imported_and_shown() {
    if !common::audio::ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let books = tempfile::TempDir::new().unwrap();
    let music = tempfile::TempDir::new().unwrap();
    common::audio::make_mp3(&books.path().join("Ken Follett/Winter der Welt/a1.mp3"), &[("composer", "Reinhard Kuhnert")]);
    common::audio::make_mp3(&music.path().join("Volkmar Glöckner/Was ist der Mensch/01 Treu.mp3"), &[("album", "Was ist der Mensch")]);
    common::audio::make_mp3(&music.path().join("Volkmar Glöckner/Was ist der Mensch/02 Worte.mp3"), &[]);
    let (b, m) = (std::fs::canonicalize(books.path()).unwrap().to_string_lossy().into_owned(), std::fs::canonicalize(music.path()).unwrap().to_string_lossy().into_owned());
    let r = send(&a, post("/admin/rpc/import.save_criteria", &form(&[("roots", &b), ("music_roots", &m), ("rule", "manual")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let leaf = music.path().file_name().unwrap().to_string_lossy().into_owned();
    assert!(r.body.contains(&leaf), "the music folder is listed");
    // The same folder cannot be both: it is refused with a reason, and nothing changes.
    let r = send(&a, post("/admin/rpc/import.save_criteria", &form(&[("roots", &b), ("music_roots", &b), ("rule", "manual")]), &o)).await;
    assert!(r.status == StatusCode::UNPROCESSABLE_ENTITY || r.status == StatusCode::CONFLICT, "{}", r.status);
    assert!(a.store.import_state().await.unwrap().music_roots.iter().any(|x| x == &m), "the music folder is still there");
    send(&a, post("/admin/rpc/import.unlock", "", &o)).await;
    let r = send(&a, post("/admin/rpc/import.run", "", &o)).await;
    // The report tells what the library holds, by kind.
    for tile in ["<dt>Audiobooks</dt><dd>1</dd>", "<dt>Authors</dt><dd>1</dd>", "<dt>Albums</dt><dd>1</dd>", "<dt>Artists</dt><dd>1</dd>", "<dt>Tracks in the library</dt><dd>3</dd>"] {
        assert!(r.body.contains(tile), "{tile}: {}", r.body);
    }

    let rows = a.store.admin_book_rows().await.unwrap();
    let book = rows.iter().find(|b| b.kind == "audiobook").unwrap();
    let album = rows.iter().find(|b| b.kind == "music").unwrap();
    assert_eq!(book.narrator.as_deref(), Some("Reinhard Kuhnert"), "the narrator comes from the composer tag");
    assert_eq!((album.title.as_str(), album.author.as_str(), album.tracks), ("Was ist der Mensch", "Volkmar Glöckner", 2));

    // Two pages, one for each kind; the narrator shows in the list of audiobooks.
    let ob = Opts { cookie: Some(&cookie), ..Default::default() };
    let books_page = send(&a, get("/admin/books", &ob)).await.body;
    assert!(books_page.contains("Winter der Welt") && books_page.contains("Reinhard Kuhnert") && !books_page.contains("Was ist der Mensch"));
    let music_page = send(&a, get("/admin/music", &ob)).await.body;
    assert!(music_page.contains("Was ist der Mensch") && !music_page.contains("Winter der Welt"));

    // The tags window asks for an author and a narrator for a book, an artist for an album.
    let w = send(&a, get(&format!("/admin/rpc/book.tags_form?id={}", book.id), &o)).await.body;
    assert!(w.contains("name=\"narrator\"") && w.contains("value=\"Reinhard Kuhnert\"") && w.contains(">Author<"));
    let w = send(&a, get(&format!("/admin/rpc/book.tags_form?id={}", album.id), &o)).await.body;
    assert!(!w.contains("name=\"narrator\"") && w.contains(">Artist<"));
}

#[tokio::test]
async fn an_archive_of_music_goes_into_the_music_folder_and_the_import_page_lists_it() {
    let tools = ["7z", "zip"].iter().all(|t| std::process::Command::new(t).arg("--help").output().is_ok());
    if !tools || !common::audio::ffmpeg_available() {
        eprintln!("skipped: 7z, zip or ffmpeg is missing");
        return;
    }
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let books = tempfile::TempDir::new().unwrap();
    let music = tempfile::TempDir::new().unwrap();
    let (b, m) = (std::fs::canonicalize(books.path()).unwrap().to_string_lossy().into_owned(), std::fs::canonicalize(music.path()).unwrap().to_string_lossy().into_owned());

    // Only an audiobook folder so far: a music archive has nowhere to go and says so.
    send(&a, post("/admin/rpc/import.save_criteria", &form(&[("roots", &b), ("rule", "manual")]), &o)).await;
    send(&a, post("/admin/rpc/import.unlock", "", &o)).await;
    let work = tempfile::TempDir::new().unwrap();
    let album = work.path().join("x");
    common::audio::make_mp3(&album.join("01 Pläne.mp3"), &[("artist", "Dania König"), ("album", "Versprochen")]);
    common::audio::make_mp3(&album.join("02 Ewig.mp3"), &[("artist", "Dania König"), ("album", "Versprochen")]);
    assert!(std::process::Command::new("zip").current_dir(work.path()).args(["-qr", "v.zip", "x"]).status().unwrap().success());
    let bytes = std::fs::read(work.path().join("v.zip")).unwrap();
    let r = send(&a, upload_request_of(&csrf, &cookie, "v.zip", &bytes, Some("music"))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let jobs = wait_for_uploads(&a).await;
    assert_eq!(jobs[0].state, "failed");
    assert!(jobs[0].message.contains("no music folder"), "{}", jobs[0].message);
    assert_eq!(jobs[0].kind, "music");

    // With a music folder it is unpacked there, by the artist and album of the tags, and imported as music.
    send(&a, post("/admin/rpc/import.save_criteria", &form(&[("roots", &b), ("music_roots", &m), ("rule", "manual")]), &o)).await;
    let r = send(&a, upload_request_of(&csrf, &cookie, "v.zip", &bytes, Some("music"))).await;
    assert_eq!(r.status, StatusCode::OK);
    let jobs = loop {
        let jobs = wait_for_uploads(&a).await;
        if jobs.len() == 2 {
            break jobs;
        }
    };
    assert_eq!(jobs[0].state, "done", "{}", jobs[0].message);
    assert!(music.path().join("Dania König/Versprochen/01 Pläne.mp3").is_file());
    assert!(!books.path().join("Dania König").exists(), "nothing went into the audiobook folder");
    let rows = a.store.admin_book_rows().await.unwrap();
    assert!(rows.iter().any(|r| r.title == "Versprochen" && r.kind == "music" && r.tracks == 2), "{rows:?}");

    // The import page lists both kinds of folder; the upload window asks which one.
    let page = send(&a, get("/admin/import", &Opts { cookie: Some(&cookie), ..Default::default() })).await.body;
    let leaf = music.path().file_name().unwrap().to_string_lossy().into_owned();
    assert!(page.contains("Music folders") && page.contains(&leaf), "the music folder is on the import page");
    assert!(page.contains("name=\"kind\"") && page.contains("value=\"music\""));
}

#[tokio::test]
async fn nested_import_folders_and_an_unreadable_folder_do_not_stop_the_import() {
    if !common::audio::ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };

    // The layout of a real server: the audiobooks lie inside the music folder (/srv/Musik/Audiobooks).
    let music = tempfile::TempDir::new().unwrap();
    let books = music.path().join("Audiobooks");
    common::audio::make_mp3(&books.join("Ken Follett/Winter der Welt/a1.mp3"), &[]);
    common::audio::make_mp3(&music.path().join("Dania König/Versprochen/01.mp3"), &[("album", "Versprochen")]);
    // A folder the service may not read.
    let locked = music.path().join("Locked Artist");
    std::fs::create_dir_all(locked.join("Album")).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

    let canon = |p: &std::path::Path| std::fs::canonicalize(p).unwrap().to_string_lossy().into_owned();
    let (b, m) = (canon(&books), canon(music.path()));
    // The audiobooks inside the music folder are named as a left-out folder, as in the settings: `!/path`.
    let r = send(&a, post("/admin/rpc/import.save_criteria", &form(&[("roots", &b), ("music_roots", &format!("{m}\n!{b}")), ("rule", "manual")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    send(&a, post("/admin/rpc/import.unlock", "", &o)).await;
    let r = send(&a, post("/admin/rpc/import.run", "", &o)).await;
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(r.status, StatusCode::OK, "an unreadable place is no reason for an error page: {}", r.body);

    let rows = a.store.admin_book_rows().await.unwrap();
    let audiobook = rows.iter().find(|b| b.title == "Winter der Welt").expect("the audiobook is imported");
    assert_eq!(audiobook.kind, "audiobook", "it is not taken for music because it lies inside the music folder");
    let album = rows.iter().find(|b| b.title == "Versprochen").expect("the album is imported");
    assert_eq!(album.kind, "music");
    assert!(!rows.iter().any(|b| b.author == "Audiobooks"), "the audiobook folder is not read as an artist: {rows:?}");

    // What could not be read is told, with the reason (not when the tests run as root, which can read it).
    if !running_as_root() {
        assert!(r.body.contains("Locked Artist") && r.body.contains("no permission to read it"), "{}", r.body);
    }
}

fn running_as_root() -> bool {
    std::fs::read_dir("/root").is_ok()
}

#[tokio::test]
async fn named_folders_are_left_out_of_the_import() {
    if !common::audio::ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let lib = tempfile::TempDir::new().unwrap();
    common::audio::make_mp3(&lib.path().join("Ken Follett/Winter der Welt/a1.mp3"), &[]);
    common::audio::make_mp3(&lib.path().join("Trash/Old Album/a1.mp3"), &[]);
    common::audio::make_mp3(&lib.path().join("Ken Follett/Not Mine/a1.mp3"), &[]);
    let root = std::fs::canonicalize(lib.path()).unwrap().to_string_lossy().into_owned();
    let (trash, notmine) = (format!("{root}/Trash"), format!("{root}/Ken Follett/Not Mine"));

    // Relative paths are refused; absolute ones, also of folders at the book level, are kept.
    let r = send(&a, post("/admin/rpc/import.save_criteria", &form(&[("roots", &format!("{root}\n!Trash")), ("rule", "manual")]), &o)).await;
    assert!(r.status == StatusCode::UNPROCESSABLE_ENTITY || r.status == StatusCode::CONFLICT, "{}", r.status);
    let r = send(&a, post("/admin/rpc/import.save_criteria", &form(&[("roots", &format!("{root}\n!{trash}\n!{notmine}\n")), ("rule", "manual")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.body.contains("left out") && r.body.contains("Trash") && r.body.contains("Not Mine"));
    assert!(r.body.contains("One folder per line") && r.body.contains("a ! in front"), "the syntax is written into the window");
    assert!(r.body.contains("!{trash}".replace("{trash}", &trash.replace('/', "&#x2f;")).as_str()), "the dialog shows them again as ! lines");

    send(&a, post("/admin/rpc/import.unlock", "", &o)).await;
    send(&a, post("/admin/rpc/import.run", "", &o)).await;
    let titles: Vec<String> = a.store.admin_book_rows().await.unwrap().into_iter().map(|b| b.title).collect();
    assert_eq!(titles, vec!["Winter der Welt".to_owned()], "only what is not left out");

    // Taking the entries away brings them back at the next import.
    send(&a, post("/admin/rpc/import.save_criteria", &form(&[("roots", &root), ("rule", "manual")]), &o)).await;
    send(&a, post("/admin/rpc/import.run", "", &o)).await;
    assert_eq!(a.store.admin_book_rows().await.unwrap().len(), 3);
}

#[tokio::test]
async fn albums_are_found_at_any_depth_with_disc_folders_and_without_hidden_ones() {
    if !common::audio::ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let lib = tempfile::TempDir::new().unwrap();
    let mp3 = |path: &str| common::audio::make_mp3(&lib.path().join(path), &[]);
    mp3("Ken Follett/Winter der Welt/01.mp3"); // two levels, as ever
    mp3("JÖRG/Dania König/Versprochen/01.mp3"); // sorted by owner: three levels
    mp3("JÖRG/Dania König/Versprochen/02.mp3");
    mp3("Musik/Rock/1998/Band/Album/01.mp3"); // sorted by genre and year: five levels
    mp3("Opera/Aida/CD 1/01.mp3"); // a disc folder per CD, no audio files in Aida itself
    mp3("Opera/Aida/CD 2/01.mp3");
    mp3("Loose Folder/01.mp3"); // directly below the import folder: no folder above it
    mp3(".recyclebin/Gone/Old/01.mp3"); // hidden: never read
    mp3("Ken Follett/@eaDir/Thumbs/01.mp3"); // NAS bookkeeping: never read
    std::fs::create_dir_all(lib.path().join("Empty Folder")).unwrap();
    let root = std::fs::canonicalize(lib.path()).unwrap().to_string_lossy().into_owned();
    send(&a, post("/admin/rpc/import.save_criteria", &form(&[("roots", &root), ("rule", "manual")]), &o)).await;
    send(&a, post("/admin/rpc/import.unlock", "", &o)).await;
    let r = send(&a, post("/admin/rpc/import.run", "", &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);

    let rows = a.store.admin_book_rows().await.unwrap();
    let find = |title: &str| rows.iter().find(|b| b.title == title).unwrap_or_else(|| panic!("{title} missing in {rows:?}"));
    // Title: the folder; author: the folder above it.
    assert_eq!((find("Winter der Welt").author.as_str(), find("Winter der Welt").tracks), ("Ken Follett", 1));
    assert_eq!((find("Versprochen").author.as_str(), find("Versprochen").tracks), ("Dania König", 2));
    assert_eq!(find("Album").author, "Band");
    // Disc folders make one book with two discs.
    let aida = find("Aida");
    assert_eq!((aida.author.as_str(), aida.tracks, aida.discs), ("Opera", 2, 2));
    // A folder right below the import folder has no folder above: the artist comes from the tags, else it is unknown.
    assert_eq!(find("Loose Folder").author, "Unknown");
    assert!(!rows.iter().any(|b| b.title == "Old" || b.title == "Thumbs"), "hidden and NAS folders are not read: {rows:?}");

    // The identity of a book is its path, so files are found again by it (the tags window finds the files).
    let id = find("Versprochen").id;
    let w = send(&a, get(&format!("/admin/rpc/book.tags_form?id={id}"), &o)).await;
    assert_eq!(w.status, StatusCode::OK, "the files of a deep album are found: {}", w.body);

    // A folder with nothing to import says so, with the reason.
    assert!(r.body.contains("Empty Folder") && r.body.contains("nothing to import here"), "{}", r.body);
}
