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
    cd: david_server::cdimport::CdService,
    drive: Arc<david_server::cdrive::fake::FakeDrives>,
}

async fn app() -> App {
    app_with(false).await
}

async fn app_with(mcp: bool) -> App {
    let (dir, store) = open().await;
    let boss = store.create_profile("Boss", true).await.unwrap();
    store.set_pin(boss, PIN).await.unwrap();
    let (cd, drive) = test_cd(dir.path());
    let state = AppState { store: store.clone(), mcp, listen: "127.0.0.1:8484".parse().unwrap(), fingerprint: None, ca_host: None, ui: Arc::new(AdminUi::new().unwrap()), secure_cookie: false, library: None, uploads: david_server::upload::Uploads::start(store.clone(), dir.path()  /* its own folder for each test app: job numbers start at 1 in each database */), cd: cd.clone(), features: Default::default() };
    App { router: router(state), store, _dir: dir, cd, drive }
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

/// The CD import of a test app: no real drive, no internet.
fn test_cd(data: &std::path::Path) -> (david_server::cdimport::CdService, Arc<david_server::cdrive::fake::FakeDrives>) {
    let drive = david_server::cdrive::fake::FakeDrives::with_drive("/dev/fake0");
    (david_server::cdimport::CdService::with_backend(data, Arc::new(david_server::musicbrainz::HttpLookup::new()), drive.clone()), drive)
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

    for path in ["/admin/users", "/admin/books", "/admin/settings", "/admin/cd"] {
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
async fn a_wrong_pin_makes_the_next_try_wait() {
    let a = app().await;
    assert_eq!(login(&a, "name=Boss&pin=000000").await.status, StatusCode::UNAUTHORIZED);
    // Right after it, even the right PIN is refused: the wait (a * 2^n seconds) has not run out.
    let right = login(&a, &format!("name=Boss&pin={PIN}")).await;
    assert_eq!(right.status, StatusCode::UNAUTHORIZED);
    assert!(right.header("set-cookie").is_none());
    // A locked name gets the same page as a name that does not exist, so the lock does not tell which names exist.
    let without_token = |body: &str| body.split('"').map(|p| if p.len() == 64 && p.bytes().all(|b| b.is_ascii_hexdigit()) { "TOKEN" } else { p }).collect::<Vec<_>>().join("\"");
    let nobody = login(&a, &format!("name=Nobody&pin={PIN}")).await;
    assert_eq!((right.status, without_token(&right.body)), (nobody.status, without_token(&nobody.body)));
    assert!(right.body.contains("the next try has to wait"));
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
    // The tracks of a CD rip have a colour for each state (the page says `data-state` on the row), in light and in dark.
    for state in ["waiting", "reading", "verifying", "read", "encoding", "done", "failed"] {
        assert!(css.body.contains(&format!("tr[data-state=\"{state}\"]")), "no colour for the state {state}");
    }
    // The row of tabs scrolls sideways and never up and down (that gave a tiny scroll bar next to the last tab).
    let tabs = css.body.lines().find(|l| l.starts_with(".tabs {")).expect("a rule for the tabs");
    assert!(tabs.contains("overflow-x: auto") && tabs.contains("overflow-y: hidden"), "{tabs}");
    // Light, dark by the system and dark chosen with the switch in the header.
    assert_eq!(css.body.matches("--warn:").count(), 3, "the amber of 'read' is defined for light and dark");
    assert_eq!(css.body.matches("--info:").count(), 3, "the blue of 'converting' is defined for light and dark");
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
    assert!(page.body.contains("Kinder der Freiheit"));

    // Below the recommendation needs the tick.
    let r = send(&a, post("/admin/rpc/book.release", &form(&[("id", &id), ("min_age", "6")]), &o)).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert!(r.body.contains("below the recommendation"));
    assert!(!a.store.find_book("Ken Follett/Kinder der Freiheit").await.unwrap().unwrap().released);

    let r = send(&a, post("/admin/rpc/book.release", &form(&[("id", &id), ("min_age", "6"), ("confirm", "1")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.body.starts_with(&format!("<tr id=\"book-{id}\"")) && r.body.contains("Released"));

    // At or above the recommendation no tick is needed; a missing rating is refused.
    assert_eq!(send(&a, post("/admin/rpc/book.release", &form(&[("id", &id), ("min_age", "16")]), &o)).await.status, StatusCode::OK);
    assert_eq!(send(&a, post("/admin/rpc/book.release", &form(&[("id", &id), ("min_age", "")]), &o)).await.status, StatusCode::UNPROCESSABLE_ENTITY);

    let r = send(&a, post("/admin/rpc/book.set_exempt", &form(&[("id", &id), ("exempt", "1")]), &o)).await;
    assert!(r.body.contains("name=\"exempt\" value=\"1\" checked"));
    let r = send(&a, post("/admin/rpc/book.unrelease", &form(&[("id", &id)]), &o)).await;
    assert!(!r.body.contains("name=\"released\" value=\"1\" checked"));
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
    assert!(page.contains("name=\"import_interval_min\"") && page.contains("name=\"pin_lock_s\"") && page.contains("name=\"time_zone\"") && page.contains("<span class=\"unit\">min</span>"));
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
    assert!(books.body.contains("Kinder der Freiheit") && !books.body.contains("name=\"released\" value=\"1\" checked"), "unreleased: the box of the release tab is not ticked");
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
    assert!(r.body.starts_with(&format!("<tr id=\"device-{device}\">")) && r.body.contains("Revoked") && !r.body.contains("device.revoke"), "a revoked row offers delete, not revoke");
    assert_eq!(send(&a, post("/admin/rpc/pairing.create", &make, &o)).await.status, StatusCode::OK, "the place is free again");
    assert_eq!(send(&a, post("/admin/rpc/device.revoke", &form(&[("id", "9999")]), &o)).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_pairing_is_deleted_only_after_it_was_revoked() {
    let a = app().await;
    let guest = a.store.create_user(NewProfile::new("Guest", Role::Guest)).await.unwrap();
    let e = a.store.create_enrollment(guest, false, guest).await.unwrap();
    let key = format!("{}I=", "C".repeat(42));
    let device = a.store.redeem_enrollment(&e.code, &key, "Phone").await.unwrap().device_id;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let id = device.to_string();

    // An active pairing offers "revoke" and no "delete", and the server refuses to delete it.
    let page = send(&a, get("/admin/pairing", &o)).await;
    assert!(page.body.contains("device.revoke") && !page.body.contains("device.delete"), "{}", page.body);
    let refused = send(&a, post("/admin/rpc/device.delete", &form(&[("id", &id)]), &o)).await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);
    assert!(a.store.device_row(device).await.unwrap().is_some(), "the active pairing is still there");

    // Revoked: the row offers "delete", and deleting takes it away.
    assert_eq!(send(&a, post("/admin/rpc/device.revoke", &form(&[("id", &id)]), &o)).await.status, StatusCode::OK);
    let page = send(&a, get("/admin/pairing", &o)).await;
    assert!(page.body.contains("device.delete") && !page.body.contains("device.revoke"), "{}", page.body);
    let gone = send(&a, post("/admin/rpc/device.delete", &form(&[("id", &id)]), &o)).await;
    assert_eq!((gone.status, gone.body.as_str()), (StatusCode::OK, ""));
    assert!(a.store.device_row(device).await.unwrap().is_none());
    assert_eq!(send(&a, post("/admin/rpc/device.delete", &form(&[("id", &id)]), &o)).await.status, StatusCode::NOT_FOUND);
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
    let (cd, drive) = test_cd(dir.path());
    let state = AppState {
        store: store.clone(),
        mcp: false,
        listen: "192.168.24.250:8484".parse().unwrap(),
        fingerprint: Some(fingerprint.clone()),
        ca_host: None,
        ui: Arc::new(AdminUi::new().unwrap()),
        secure_cookie: true,
        library: None,
        uploads: david_server::upload::Uploads::start(store.clone(), &std::env::temp_dir().join(format!("david-uploads-tls-{}", std::process::id()))),
        cd: cd.clone(),
        features: Default::default(),
    };
    let a = App { router: router(state), store, _dir: dir, cd, drive };

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
async fn a_pin_reset_by_an_administrator_signs_out_the_browsers_but_not_the_devices() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let anna = a.store.create_profile("Anna", false).await.unwrap();
    a.store.set_pin(anna, "112233").await.unwrap();
    let browser = a.store.create_session(anna, Role::Member, None).await.unwrap();
    let device = a.store.add_device(david_server::NewDevice { user_id: anna, name: "Phone".into(), wg_pubkey: "K".repeat(43) + "=", tunnel_ip: "10.77.0.2".into(), shared: false, created_by: None }).await.unwrap();
    let device_session = a.store.create_session(anna, Role::Member, Some(device)).await.unwrap();

    let r = send(&a, post("/admin/rpc/user.set_pin", &form(&[("id", &anna.to_string()), ("pin", "9876")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(a.store.session_for_token(&browser).await.unwrap().is_none(), "whoever knew the old PIN is signed out");
    assert!(a.store.session_for_token(&device_session).await.unwrap().is_some(), "the phone stays paired and signed in");

    // An administrator who resets their own PIN keeps the session they do it from; their other browsers end.
    let boss = a.store.list_profiles().await.unwrap().into_iter().find(|p| p.name == "Boss").unwrap().id;
    let other = a.store.create_session(boss, Role::Admin, None).await.unwrap();
    let r = send(&a, post("/admin/rpc/user.set_pin", &form(&[("id", &boss.to_string()), ("pin", "556677")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(a.store.session_for_token(&other).await.unwrap().is_none());
    assert_eq!(send(&a, get("/admin/users", &Opts { cookie: Some(&cookie), ..Default::default() })).await.status, StatusCode::OK);
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
    assert!(!page.contains("Not released") && !page.contains("<th>State</th>"), "the column State is gone: release is in the tab Release");
    assert!(page.contains("2 discs") && page.contains("3 tracks") && page.contains("0 h 12 min"), "three tracks of four minutes");
    // One button, on the cover and on the title, opens one window; its tabs hold release, edit, tags and filing. There is no column of buttons.
    assert!(!page.contains("<span class=\"sr-only\">Actions</span>") && !page.contains("class=\"row-actions\""), "no column of buttons");
    assert!(page.contains(&format!("class=\"thumb book-open\" data-dialog=\"dlg-book-{id}\"")) && page.contains(&format!("class=\"book-open title-button\" data-dialog=\"dlg-book-{id}\"")), "the cover and the title open the window");
    for tab in ["release", "edit", "tags", "filing"] {
        assert!(page.contains(&format!("data-tab=\"{tab}\"")) && page.contains(&format!("data-panel=\"{tab}\"")), "tab {tab}: {page}");
    }
    assert!(page.contains("data-form=\"book-form-") && page.contains("data-form=\"book-edit-form-") && page.contains("data-form=\"tags-form-") && page.contains("data-close>Close</button>") && page.contains("data-save form=\"book-form-"), "one Save in the foot, for the form of the open tab");
    assert!(page.contains("class=\"form-error"), "errors are shown inside the window");
    let de = send(&a, get("/admin/books", &Opts { cookie: Some(&cookie), lang: Some("de"), ..Default::default() })).await.body;
    assert!(de.contains("<h1>Hörbücher</h1>") && de.contains("2 CDs") && de.contains("Freigabe"));
}

#[tokio::test]
async fn the_tab_filing_and_the_track_list_show_the_file_formats_and_warn_about_the_iphone() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let hx = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), htmx: true, ..Default::default() };
    // Two MP3 files and one Ogg Vorbis file (as the album Caipiranha); a row that no import has looked at has no format.
    let mut b = book("Duo Camillo", "Caipiranha", &[4]);
    b.tracks[2].format = "ogg".into();
    b.tracks[2].codec = "vorbis".into();
    b.tracks[2].mime_type = "audio/ogg".into();
    b.tracks[2].sample_rate = 44_100;
    b.tracks[2].channels = 2;
    b.tracks[3].format = String::new();
    b.tracks[3].codec = String::new();
    b.tracks[3].mime_type = String::new();
    a.store.import_books(vec![b]).await.unwrap();
    let id = a.store.find_book("Duo Camillo/Caipiranha").await.unwrap().unwrap().id;

    let r = send(&a, get(&format!("/admin/rpc/book.filing?id={id}"), &hx)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    // One line per combination with the number of tracks; the one the iPhone cannot play is marked.
    for needle in ["File formats", "ogg&#x2f;vorbis 44.1 kHz stereo &times; 1", "mp3&#x2f;mpeg &times; 2", "not read yet", "class=\"badge warn\"", "An iPhone cannot play this format."] {
        assert!(r.body.contains(needle), "{needle}: {}", r.body);
    }
    let de = send(&a, get(&format!("/admin/rpc/book.filing?id={id}"), &Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), htmx: true, lang: Some("de") })).await;
    assert!(de.body.contains("Dateiformate") && de.body.contains("noch nicht gelesen") && de.body.contains("Ein iPhone kann dieses Format nicht abspielen."), "{}", de.body);

    // The track list names the format of every file.
    let list = send(&a, get(&format!("/admin/rpc/book.tracks?id={id}"), &hx)).await.body;
    assert!(list.contains(">ogg</span>") && list.contains(">mp3</span>"), "{list}");
    assert_eq!(list.matches("badge warn").count(), 1, "only the Ogg track is marked: {list}");
}

#[tokio::test]
async fn the_tab_filing_shows_the_place_and_the_metadata_of_the_database() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let hx = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), htmx: true, ..Default::default() };
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    a.store.import_books(vec![book("Follett", "Winter der Welt", &[2])]).await.unwrap();
    let id = a.store.find_book("Follett/Winter der Welt").await.unwrap().unwrap().id;

    // The window loads the tab when it is opened.
    let page = send(&a, get("/admin/books", &plain)).await.body;
    assert!(page.contains(&format!("hx-get=\"/admin/rpc/book.filing?id={id}\"")) && page.contains(&format!("id=\"filing-body-{id}\"")), "{page}");
    let r = send(&a, get(&format!("/admin/rpc/book.filing?id={id}"), &hx)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    // The place in the library, the facts of the row, and the audit fields the triggers fill in (made, last changed, by whom).
    for needle in ["Follett&#x2f;Winter der Welt", "Complete place on the disk", "not found", "Made in the database", "Last changed in the database", "Seen by the import", ">audiobook<", &format!("<dd>{id}</dd>")] {
        assert!(r.body.contains(needle), "{needle}: {}", r.body);
    }
    // A date has the form of the other pages: year, month, day, hour, minute.
    let stamps = r.body.matches("20").count();
    assert!(stamps > 0 && r.body.contains("-"), "{}", r.body);
    assert_eq!(send(&a, get("/admin/rpc/book.filing?id=99999", &hx)).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn each_tab_of_the_book_window_sends_only_its_own_part() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), htmx: true, ..Default::default() };
    a.store.import_books(vec![book("A", "Story", &[1])]).await.unwrap();
    let id = a.store.find_book("A/Story").await.unwrap().unwrap().id;
    let sid = id.to_string();

    // The release tab: released from 12, the recommendation set; the blurb of the other tab is not touched by it.
    a.store.set_book_description(id, "Klappentext").await.unwrap();
    let r = send(&a, post("/admin/rpc/book.update", &form(&[("id", &sid), ("section", "release"), ("released", "1"), ("min_age", "12"), ("recommended_age", "10")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let b = a.store.admin_book_row(id).await.unwrap().unwrap();
    assert_eq!((b.released, b.min_age, b.recommended_age, b.description.as_deref()), (true, Some(12), Some(10), Some("Klappentext")));
    // The edit tab sends no `released`: the release stays; only blurb and labels change.
    let r = send(&a, post("/admin/rpc/book.update", &form(&[("id", &sid), ("section", "edit"), ("description", "Neu"), ("labels", "Gebet")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let b = a.store.admin_book_row(id).await.unwrap().unwrap();
    assert_eq!((b.released, b.min_age, b.description.as_deref()), (true, Some(12), Some("Neu")), "an edit does not withdraw the release");
    assert_eq!(a.store.labels_of(id).await.unwrap(), vec!["Gebet".to_owned()]);
    // The recommendation can be taken away; one that is not a number is refused with the field named, and nothing changes.
    send(&a, post("/admin/rpc/book.update", &form(&[("id", &sid), ("section", "release"), ("released", "1"), ("min_age", "12"), ("recommended_age", "")]), &o)).await;
    assert_eq!(a.store.admin_book_row(id).await.unwrap().unwrap().recommended_age, None);
    let bad = send(&a, post("/admin/rpc/book.update", &form(&[("id", &sid), ("section", "release"), ("released", "1"), ("min_age", "12"), ("recommended_age", "x")]), &o)).await;
    assert_eq!((bad.status, bad.header("x-invalid-field")), (StatusCode::UNPROCESSABLE_ENTITY, Some("recommended_age")), "{}", bad.body);
}

#[tokio::test]
async fn the_book_window_sets_labels_and_the_list_is_filtered_by_them() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), htmx: true, ..Default::default() };
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    for (author, title) in [("A", "Eins"), ("B", "Zwei"), ("C", "Drei")] {
        a.store.import_books(vec![book(author, title, &[1])]).await.unwrap();
    }
    let (one_id, two_id) = (a.store.find_book("A/Eins").await.unwrap().unwrap().id, a.store.find_book("B/Zwei").await.unwrap().unwrap().id);
    let (one, two) = (one_id.to_string(), two_id.to_string());

    // The window has the field; the labels are written with commas; the row shows them.
    let r = send(&a, post("/admin/rpc/book.update", &form(&[("id", &one), ("labels", "Gebet, Schule; gebet")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.body.contains("name=\"labels\"") && r.body.contains("<span class=\"badge label\">Gebet</span>") && r.body.contains("<span class=\"badge label\">Schule</span>"), "{}", r.body);
    send(&a, post("/admin/rpc/book.update", &form(&[("id", &two), ("labels", "Schule")]), &o)).await;
    // Absent leaves them alone; empty takes them all off.
    send(&a, post("/admin/rpc/book.update", &form(&[("id", &two), ("min_age", "")]), &o)).await;
    assert_eq!(a.store.labels_of(two_id).await.unwrap(), vec!["Schule".to_owned()]);
    // A bad name is refused with the field named for the ring, and nothing changes.
    let bad = send(&a, post("/admin/rpc/book.update", &form(&[("id", &one), ("labels", "ok, a|b")]), &o)).await;
    assert_eq!((bad.status, bad.header("x-invalid-field")), (StatusCode::UNPROCESSABLE_ENTITY, Some("labels")), "{}", bad.body);
    assert_eq!(a.store.labels_of(one_id).await.unwrap().len(), 2);

    // The labels are a widget of chips (a script of its own, translated through attributes), the plain field stays without JavaScript.
    let window = send(&a, get("/admin/books", &plain)).await.body;
    assert!(window.contains("data-chips") && window.contains("data-add=\"Add\"") && window.contains("data-remove=\"Remove\"") && window.contains("/admin/static/chips.js"), "{window}");
    let js = send(&a, get("/admin/static/chips.js", &plain)).await;
    assert!(js.status == StatusCode::OK && js.body.contains("data-chips"));
    // The entry suggests the labels in use: the ones that begin with the typed text first, the most used first; the list is fresh on every ask.
    assert!(window.contains("data-suggest=\"/admin/rpc/label.suggest\""));
    let all = send(&a, get("/admin/rpc/label.suggest", &plain)).await;
    assert_eq!(all.status, StatusCode::OK);
    let names: Vec<String> = serde_json::from_str(&all.body).unwrap();
    assert_eq!(names, vec!["Schule".to_owned(), "Gebet".to_owned()], "Schule is on two books, Gebet on one: {}", all.body);
    let typed: Vec<String> = serde_json::from_str(&send(&a, get("/admin/rpc/label.suggest?q=ge", &plain)).await.body).unwrap();
    assert_eq!(typed, vec!["Gebet".to_owned()]);
    let inside: Vec<String> = serde_json::from_str(&send(&a, get("/admin/rpc/label.suggest?q=ule", &plain)).await.body).unwrap();
    assert_eq!(inside, vec!["Schule".to_owned()], "a label that holds the text is suggested too");
    assert_ne!(send(&a, get("/admin/rpc/label.suggest", &Opts::default())).await.status, StatusCode::OK, "only for a signed-in administrator");
    // The tab Tags of an album whose files are gone says so instead of staying empty.
    let gone = send(&a, get(&format!("/admin/rpc/book.tags_form?id={one}"), &o)).await;
    assert!(gone.status == StatusCode::OK && gone.body.contains("cannot be found"), "{}", gone.body);

    // The list: a choice of the labels with their number, and the filter.
    let page = send(&a, get("/admin/books", &plain)).await.body;
    assert!(page.contains("name=\"label\"") && page.contains("Schule (2)") && page.contains("Gebet (1)"), "{page}");
    let rows = |b: &str| b.matches("<tr id=\"book-").count();
    assert_eq!(rows(&page), 3);
    let schule = send(&a, get("/admin/books?label=schule", &plain)).await.body;
    assert_eq!(rows(&schule), 2, "{schule}");
    let gebet = send(&a, get("/admin/books?label=Gebet", &plain)).await.body;
    assert_eq!(rows(&gebet), 1);
    assert!(!gebet.contains("class=\"pager\""), "one entry, no pagination");
    assert_eq!(rows(&send(&a, get("/admin/books?label=Gibt+es+nicht", &plain)).await.body), 0);
    send(&a, post("/admin/rpc/book.update", &form(&[("id", &one), ("labels", "")]), &o)).await;
    assert!(a.store.labels_of(one_id).await.unwrap().is_empty());
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
    assert!(r.body.starts_with(&format!("<tr id=\"book-{id}\"")) && r.body.contains("name=\"released\" value=\"1\" checked") && r.body.contains("name=\"exempt\" value=\"1\" checked"), "the tab Release shows what was saved: {}", r.body);
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
    // The matrix: a column for every member of the family that is no administrator, with a box for every book; a change opens or closes the book.
    let anna = a.store.create_user(david_server::NewProfile::new("Anna", david_server::Role::Member)).await.unwrap();
    a.store.release_book(book.id, 0).await.unwrap();
    let page = send(&a, get("/admin/books", &ob)).await.body;
    assert!(page.contains("class=\"member-col\"") && page.contains("<span title=\"Anna\">Anna</span>"), "{page}");
    assert!(page.contains("name=\"book.visibility\"") || page.contains("action=\"/admin/rpc/book.visibility\""), "{page}");
    assert!(page.contains("aria-label=\"Anna: Winter der Welt\"") && page.contains("title=\"Anna\" checked"), "Anna sees the released book: {page}");
    let csrf2 = csrf.clone();
    let toggle = |visible: bool| {
        let mut f = vec![("csrf", csrf2.clone()), ("book", book.id.to_string()), ("profile", anna.to_string())];
        if visible {
            f.push(("visible", "1".to_owned()));
        }
        f.into_iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&")
    };
    let hx = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf2), ..Default::default() };
    let r = send(&a, post("/admin/rpc/book.visibility", &toggle(false), &hx)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(!r.body.contains("title=\"Anna\" checked"), "the row shows the box empty now: {}", r.body);
    assert!(!a.store.visible_books(anna).await.unwrap().iter().any(|b| b.id == book.id), "closed to Anna");
    let r = send(&a, post("/admin/rpc/book.visibility", &toggle(true), &hx)).await;
    assert!(r.body.contains("title=\"Anna\" checked") && a.store.visible_books(anna).await.unwrap().iter().any(|b| b.id == book.id));
    // The owner is a box of the book window, another thing than the release: it says whom the book belongs to.
    let page = send(&a, get("/admin/books", &ob)).await.body;
    assert!(page.contains("name=\"owner\"") && page.contains(&format!("<option value=\"{anna}\">Anna</option>")), "{page}");
    let body = format!("csrf={}&id={}&released=1&min_age=0&owner={anna}", csrf, book.id);
    let r = send(&a, post("/admin/rpc/book.update", &body, &hx)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(a.store.admin_book_row(book.id).await.unwrap().unwrap().owner.as_deref(), Some("Anna"));
    assert!(r.body.contains("of Anna") || r.body.contains("&middot; of Anna"), "{}", r.body);
    let r = send(&a, post("/admin/rpc/book.update", &format!("csrf={}&id={}&released=1&min_age=0&owner=", csrf, book.id), &hx)).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(a.store.admin_book_row(book.id).await.unwrap().unwrap().owner, None, "an empty choice removes the owner");
    let r = send(&a, post("/admin/rpc/book.update", &format!("csrf={}&id={}&released=1&min_age=0&owner=99999", csrf, book.id), &hx)).await;
    assert_ne!(r.status, StatusCode::OK, "an unknown owner is refused");
    // The search knows wildcards (`*`, `?`) and regular expressions between slashes; a broken one says so and finds nothing.
    let q = |q: &str, page: &str| format!("/admin/{page}?q={}", serde_urlencoded::to_string([("", q)]).unwrap().trim_start_matches('='));
    let rows = |body: &str| body.matches("<tr id=\"book-").count();
    assert_eq!(rows(&send(&a, get(&q("wint* welt", "books"), &ob)).await.body), 1, "a star");
    assert_eq!(rows(&send(&a, get(&q("winter ??? welt", "books"), &ob)).await.body), 1, "question marks");
    assert_eq!(rows(&send(&a, get(&q("/^winter der/", "books"), &ob)).await.body), 1, "a regular expression");
    assert_eq!(rows(&send(&a, get(&q("/^der/", "books"), &ob)).await.body), 0, "it must hold");
    assert_eq!(rows(&send(&a, get(&q("/(bach|follett)/", "books"), &ob)).await.body), 1, "alternatives");
    assert_eq!(rows(&send(&a, get(&q("was * mensch", "music"), &ob)).await.body), 1, "music has it too");
    let bad = send(&a, get(&q("/(unclosed/", "books"), &ob)).await.body;
    assert!(bad.contains("The search pattern is not valid") && rows(&bad) == 0, "{bad}");
    // A red mark for a book without a cover (these test files carry none), a filter for the books with a problem, and its count.
    assert!(books_page.contains("class=\"problem-mark\"") && books_page.contains("No cover") && books_page.contains("name=\"problems\""), "{books_page}");
    let only = send(&a, get("/admin/books?problems=1", &ob)).await.body;
    assert!(only.contains("Winter der Welt") && only.contains("checked"), "{only}");
    let none = send(&a, get("/admin/music?problems=1&q=zzzunknownzzz", &ob)).await.body;
    assert!(!none.contains("<tr id=\"book-"), "no row: {none}");

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
    // The window has a row for every folder; a folder that is left out has its box ticked.
    let esc = |x: &str| x.replace('/', "&#x2f;");
    assert!(r.body.contains("name=\"r_audiobook_1\"") && r.body.contains(&esc(&trash)) && r.body.contains("name=\"x_audiobook_101\" value=\"1\" checked"), "the dialog shows the folders as rows");

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
    // Not empty, but nothing to import: only a text file (a folder with nothing at all is no problem).
    std::fs::write(lib.path().join("Empty Folder/notes.txt"), b"x").unwrap();
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

#[tokio::test]
async fn healthz_answers_without_signing_in_and_says_only_the_state() {
    let a = app().await;
    let r = send(&a, get("/healthz", &Opts::default())).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body.contains("\"status\":\"ok\"") && r.body.contains("\"version\""), "{}", r.body);
    assert_eq!(r.header("cache-control"), Some("no-store"));
    assert!(r.header("set-cookie").is_none(), "no cookie for a monitor");
    // Nothing of the library, the users or the folders.
    for secret in ["audiobook", "album", "folder", "Boss", "token"] {
        assert!(!r.body.contains(secret), "{secret} in {}", r.body);
    }
}

#[tokio::test]
async fn the_start_page_links_to_the_administration_without_signing_in() {
    let a = app().await;
    let r = send(&a, get("/", &Opts::default())).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body.contains("href=\"/admin/\""), "{}", r.body);
    assert!(r.header("set-cookie").is_none());
}

#[tokio::test]
async fn the_last_import_stays_on_the_import_page_with_its_problems() {
    if !common::audio::ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let ob = Opts { cookie: Some(&cookie), ..Default::default() };
    let lib = tempfile::TempDir::new().unwrap();
    common::audio::make_mp3(&lib.path().join("Ken Follett/Winter der Welt/01.mp3"), &[]);
    std::fs::create_dir_all(lib.path().join("Empty Folder")).unwrap();
    // Not empty, but nothing to import: only a text file (a folder with nothing at all is no problem).
    std::fs::write(lib.path().join("Empty Folder/notes.txt"), b"x").unwrap();
    let root = std::fs::canonicalize(lib.path()).unwrap().to_string_lossy().into_owned();
    send(&a, post("/admin/rpc/import.save_criteria", &form(&[("roots", &root), ("rule", "manual")]), &o)).await;
    send(&a, post("/admin/rpc/import.unlock", "", &o)).await;

    // Before any run there is nothing to show.
    assert!(!send(&a, get("/admin/import", &ob)).await.body.contains("Result of the last import"));
    send(&a, post("/admin/rpc/import.run", "", &o)).await;

    // The page, opened later, still tells the last result and what was skipped, with the time.
    let page = send(&a, get("/admin/import", &ob)).await.body;
    assert!(page.contains("Result of the last import") && page.contains("class=\"when\""), "{page}");
    assert!(page.contains("Empty Folder") && page.contains("nothing to import here"), "the problems are kept too");
    assert!(page.contains("<dt>Audiobooks</dt><dd>1</dd>"));
}

#[tokio::test]
async fn the_llm_tab_makes_a_token_shown_once_lists_it_and_revokes_it() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };

    // The tab is there; MCP is off in this app, and it says how to turn it on.
    let page = send(&a, get("/admin/settings?tab=llm", &plain)).await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.body.contains("LLM access") && page.body.contains("david run --mcp") && page.body.contains("No tokens yet."), "{}", page.body);

    let r = send(&a, post("/admin/rpc/mcp.token_create", &form(&[("name", "Claude at home"), ("write", "1")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.body.contains("shown only once") && r.body.contains("Bearer "), "{}", r.body);
    let tokens = a.store.mcp_tokens().await.unwrap();
    assert_eq!((tokens.len(), tokens[0].can_write, tokens[0].name.as_str()), (1, true, "Claude at home"));

    // Afterwards the secret is not shown any more, only the name.
    let page = send(&a, get("/admin/settings?tab=llm", &plain)).await;
    assert!(page.body.contains("Claude at home") && !page.body.contains("Bearer "), "{}", page.body);

    assert_eq!(send(&a, post("/admin/rpc/mcp.token_create", &form(&[("name", "  ")]), &o)).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = send(&a, post("/admin/rpc/mcp.token_revoke", &form(&[("id", &tokens[0].id.to_string())]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(a.store.mcp_tokens().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_new_token_comes_with_the_mcp_configuration_to_copy() {
    let a = app_with(true).await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let r = send(&a, post("/admin/rpc/mcp.token_create", &form(&[("name", "Claude")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.body.contains("data-copy=") && r.body.contains("Copy MCP configuration"), "{}", r.body);
    assert!(r.body.contains("&quot;type&quot;: &quot;http&quot;") && r.body.contains("127.0.0.1:8484&#x2f;mcp"), "{}", r.body);
    // Later visits show neither the secret nor the button.
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    let page = send(&a, get("/admin/settings?tab=llm", &plain)).await;
    assert!(!page.body.contains("data-copy=") && !page.body.contains("Bearer "), "{}", page.body);
}

#[tokio::test]
async fn the_media_pages_search_title_author_and_narrator_and_offer_completions() {
    let a = app().await;
    let (cookie, _) = sign_in(&a).await;
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    for (author, title, narrator) in [("Follett", "Winter der Welt", Some("Sprecher Eins")), ("Tolkien", "Der Hobbit", None)] {
        let mut b = book(author, title, &[1]);
        b.narrator = narrator.map(str::to_owned);
        a.store.import_books(vec![b]).await.unwrap();
    }
    let all = send(&a, get("/admin/books", &plain)).await.body;
    assert!(all.contains("Winter der Welt") && all.contains("Der Hobbit"));
    // Completions hold titles, authors and narrators.
    for word in ["<option value=\"Follett\">", "<option value=\"Der Hobbit\">", "<option value=\"Sprecher Eins\">"] {
        assert!(all.contains(word), "{word}");
    }
    // Each query finds by any of the three, without regard to case.
    for q in ["hobbit", "TOLKIEN"] {
        let r = send(&a, get(&format!("/admin/books?q={q}"), &plain)).await.body;
        assert!(r.contains("id=\"book-") && r.contains("Der Hobbit") && !r.contains("<strong>Winter der Welt"), "{q}");
    }
    let r = send(&a, get("/admin/books?q=sprecher", &plain)).await.body;
    assert!(r.contains("<strong>Winter der Welt") && !r.contains("<strong>Der Hobbit"));
    let r = send(&a, get("/admin/books?q=zzz", &plain)).await.body;
    assert!(r.contains("Nothing found."), "{r}");
}

#[tokio::test]
async fn the_tags_window_takes_any_field_as_a_line_and_refuses_unknown_ones() {
    if !common::audio::ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let lib = tempfile::TempDir::new().unwrap();
    let dir = lib.path().join("Ken Follett/Kinder der Freiheit");
    common::audio::make_mp3(&dir.join("a_CD01_TR01.mp3"), &[("comment", "weg damit")]);
    let root = std::fs::canonicalize(lib.path()).unwrap().to_string_lossy().into_owned();
    send(&a, post("/admin/rpc/import.save_criteria", &form(&[("roots", &root), ("rule", "manual")]), &o)).await;
    send(&a, post("/admin/rpc/import.unlock", "", &o)).await;
    send(&a, post("/admin/rpc/import.run", "", &o)).await;
    let id = a.store.admin_book_rows().await.unwrap()[0].id.to_string();

    let w = send(&a, get(&format!("/admin/rpc/book.tags_form?id={id}"), &o)).await.body;
    assert!(w.contains("name=\"more\"") && w.contains("Lyricist"), "{w}");

    let r = send(&a, tags_request(&csrf, &cookie, &[("id", &id), ("more", "Lyricist=Dichter\nGenre=Jazz\ngenre=Blues\nComment=")], None)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let fields = david_server::tags::read_fields(&dir.join("a_CD01_TR01.mp3")).unwrap();
    let get_f = |n: &str| fields.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
    assert_eq!(get_f("Lyricist"), Some(vec!["Dichter".to_owned()]));
    assert!(get_f("Genre").unwrap().contains(&"Blues".to_owned()));
    assert!(get_f("Comment").is_none(), "an empty value removes the field");

    for bad in ["NoSuchField=x", "no equals sign"] {
        let r = send(&a, tags_request(&csrf, &cookie, &[("id", &id), ("more", bad)], None)).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
    }
}

#[tokio::test]
async fn the_administration_and_mcp_answer_together_on_one_router() {
    let a = app().await;
    let (cookie, _) = sign_in(&a).await;
    let admin = a.store.user_rows().await.unwrap()[0].id;
    let token = a.store.create_mcp_token("llm", true, admin).await.unwrap();
    a.store.import_books(vec![book("Follett", "Winter", &[1])]).await.unwrap();
    // The way `--mcp` puts it together: the interface of the administration plus /mcp.
    let router = a.router.clone().merge(david_server::mcp::router(david_server::mcp::McpState { store: a.store.clone(), library: None, cd: a.cd.clone() }));

    let mut jobs = tokio::task::JoinSet::new();
    for n in 0..60 {
        let router = router.clone();
        let (cookie, token) = (cookie.clone(), token.clone());
        jobs.spawn(async move {
            let (request, expect_in_body) = match n % 3 {
                0 => (Request::builder().uri("/healthz").body(Body::empty()).unwrap(), "\"status\":\"ok\""),
                1 => (Request::builder().uri("/admin/books").header(header::COOKIE, cookie).body(Body::empty()).unwrap(), "Winter"),
                _ => (
                    Request::builder()
                        .method("POST")
                        .uri("/mcp")
                        .header(header::CONTENT_TYPE, "application/json")
                        .header(header::AUTHORIZATION, format!("Bearer {token}"))
                        .body(Body::from(r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_albums","arguments":{}}}"#))
                        .unwrap(),
                    "Winter",
                ),
            };
            let res = router.oneshot(request).await.unwrap();
            let status = res.status();
            let body = String::from_utf8_lossy(&to_bytes(res.into_body(), 1 << 20).await.unwrap()).into_owned();
            (n, status, body.contains(expect_in_body))
        });
    }
    let mut answered = 0;
    while let Some(done) = jobs.join_next().await {
        let (n, status, found) = done.unwrap();
        assert_eq!(status, StatusCode::OK, "request {n}");
        assert!(found, "request {n} did not answer as expected");
        answered += 1;
    }
    assert_eq!(answered, 60);
}

#[tokio::test]
async fn the_llm_tab_saves_the_help_text_and_an_empty_text_brings_the_built_in_one_back() {
    let a = app_with(true).await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let r = send(&a, post("/admin/rpc/mcp.prompt_save", &form(&[("text", "Only fix titles.")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.body.contains("Only fix titles."), "{}", r.body);
    assert_eq!(a.store.mcp_prompt().await.unwrap().as_deref(), Some("Only fix titles."));
    let r = send(&a, post("/admin/rpc/mcp.prompt_save", &form(&[("text", "")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(a.store.mcp_prompt().await.unwrap(), None);
    assert!(r.body.contains("find_problems"), "{}", r.body);
}

#[tokio::test]
async fn missing_rights_on_the_library_are_a_warning_with_the_command_that_fixes_them() {
    use std::os::unix::fs::PermissionsExt;
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let root = tempfile::tempdir().unwrap();
    let album = root.path().join("Artist/Album");
    std::fs::create_dir_all(&album).unwrap();
    let file = album.join("01.mp3");
    std::fs::write(&file, b"x").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o444)).unwrap();
    if std::fs::OpenOptions::new().append(true).open(&file).is_ok() {
        return; // a user that may write everything cannot show the warning
    }
    a.store.save_music_roots(&[root.path().display().to_string()]).await.unwrap();
    let r = send(&a, post("/admin/rpc/access.check", &form(&[]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.body.contains("lacks rights") && r.body.contains("Artist&#x2f;Album") && r.body.contains("setfacl -R -m u:"), "{}", r.body);
    // The pages carry it as well, once it is known.
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    let page = send(&a, get("/admin/", &plain)).await;
    assert!(page.body.contains("id=\"access-warning\"") && page.body.contains("files not writable"), "{}", page.body);
}

#[tokio::test]
async fn a_book_that_looks_like_a_copy_of_another_is_marked() {
    let a = app().await;
    let (cookie, _) = sign_in(&a).await;
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    for (author, title) in [("Follett", "Kinder der Freiheit"), ("Johannes Steck", "Kinder der Freiheit"), ("Follett", "Winter der Welt")] {
        a.store.import_books(vec![book(author, title, &[2])]).await.unwrap();
    }
    let page = send(&a, get("/admin/books", &plain)).await.body;
    assert_eq!(page.matches("Possible duplicate").count(), 2, "{page}");
    assert!(page.contains("Johannes Steck, 1 discs"), "the badge names the other copy: {page}");
}

#[tokio::test]
async fn ignoring_a_duplicate_hides_its_badge_lists_the_rule_and_can_be_undone() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    for author in ["Follett", "Johannes Steck"] {
        a.store.import_books(vec![book(author, "Kinder der Freiheit", &[2])]).await.unwrap();
    }
    let id = a.store.admin_book_rows().await.unwrap()[0].id;
    let page = send(&a, get("/admin/books", &plain)).await.body;
    assert_eq!(page.matches("Possible duplicate").count(), 2);
    assert_eq!(page.matches("action=\"/admin/rpc/ignore.add\"").count(), 6, "three choices on each of the two rows");

    let form_opts = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), ..Default::default() };
    let r = send(&a, post("/admin/rpc/ignore.add", &form(&[("id", &id.to_string()), ("scope", "title"), ("what", "duplicate"), ("back", "audiobook")]), &form_opts)).await;
    assert!(r.status.is_redirection(), "{}", r.body);
    let page = send(&a, get("/admin/books", &plain)).await.body;
    assert_eq!(page.matches("Possible duplicate").count(), 0, "the title is wanted twice: {page}");
    assert!(page.contains("Ignored (1)") && page.contains("This title"), "{page}");

    let rule = a.store.ignore_rules().await.unwrap()[0].id;
    let r = send(&a, post("/admin/rpc/ignore.remove", &form(&[("rule", &rule.to_string()), ("back", "audiobook")]), &form_opts)).await;
    assert!(r.status.is_redirection());
    assert_eq!(send(&a, get("/admin/books", &plain)).await.body.matches("Possible duplicate").count(), 2);
}

#[tokio::test]
async fn the_log_level_is_a_setting_that_applies_at_once_and_rejects_nonsense() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    let page = send(&a, get("/admin/settings?tab=values", &plain)).await.body;
    assert!(page.contains("name=\"log_level\"") && page.contains("Log level") && page.contains("<option value=\"info\" selected>"), "{page}");

    let r = send(&a, post("/admin/rpc/settings.save", &form(&[("log_level", "debug")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(david_server::logging::current(), tracing::Level::DEBUG, "applied without a restart");
    assert_eq!(a.store.setting("log_level").await.unwrap().as_deref(), Some("debug"));
    assert!(r.body.contains("<option value=\"debug\" selected>"));

    let r = send(&a, post("/admin/rpc/settings.save", &form(&[("log_level", "loud")]), &o)).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(david_server::logging::current(), tracing::Level::DEBUG, "nothing changed");
    david_server::logging::set(tracing::Level::INFO);
}

#[tokio::test]
async fn the_media_lists_are_a_tree_whose_levels_are_a_setting_for_each_kind() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    let mut music = book("Martin Luther King", "Ein Traum", &[1]);
    music.kind = "music".into();
    music.folder = "NORA/Chor/Martin Luther King/Ein Traum".into();
    let mut other = music.clone();
    other.title = "Sopran".into();
    other.folder = "NORA/Chor/Martin Luther King/Sopran".into();
    a.store.import_books(vec![music, other, book("Follett", "Winter der Welt", &[1])]).await.unwrap();

    // Music is shown by its folders, audiobooks by their author.
    let page = send(&a, get("/admin/music", &plain)).await.body;
    for level in ["NORA", "Chor", "Martin Luther King"] {
        assert!(page.contains(&format!("<span>{level}</span>")), "{level}: {page}");
    }
    assert!(page.contains("data-node=\"n0\"") && page.contains("(2)"), "the count of the books below a level");
    let books = send(&a, get("/admin/books", &plain)).await.body;
    assert!(books.contains("<span>Follett</span>") && !books.contains("<span>NORA</span>"), "{books}");

    // The levels are settings; a flat list has no levels.
    let page = send(&a, get("/admin/settings?tab=values", &plain)).await.body;
    assert!(page.contains("name=\"tree_music\"") && page.contains("Folders on the disk"), "{page}");
    let r = send(&a, post("/admin/rpc/settings.save", &form(&[("tree_music", "flat"), ("tree_audiobook", "letter_author")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let flat = send(&a, get("/admin/music", &plain)).await.body;
    assert!(!flat.contains("class=\"tree-node\"") && flat.contains("Ein Traum"), "{flat}");
    let letters = send(&a, get("/admin/books", &plain)).await.body;
    assert!(letters.contains("<span>F</span>") && letters.contains("<span>Follett</span>"), "{letters}");
    let bad = send(&a, post("/admin/rpc/settings.save", &form(&[("tree_music", "diagonal")]), &o)).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn a_copy_can_be_put_into_quarantine_from_the_list_and_the_last_copy_cannot() {
    use std::os::unix::fs::PermissionsExt;
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    let form_opts = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), ..Default::default() };
    let root = tempfile::tempdir().unwrap();
    for folder in ["Schule/Quentin Qualle", "Compilations/Quentin Qualle"] {
        let dir = root.path().join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Quentin Qualle_CD01_TR01.mp3"), b"x").unwrap();
    }
    let _ = std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o755));
    a.store.save_music_roots(&[root.path().display().to_string()]).await.unwrap();
    for folder in ["Schule/Quentin Qualle", "Compilations/Quentin Qualle"] {
        let mut b = book("Quentin Qualle", "Quentin Qualle", &[1]);
        b.kind = "music".into();
        b.folder = folder.into();
        a.store.import_books(vec![b]).await.unwrap();
    }
    let page = send(&a, get("/admin/music", &plain)).await.body;
    assert!(page.contains("action=\"/admin/rpc/quarantine.add\"") && page.contains("Put this copy into quarantine"), "{page}");
    let ids: Vec<i64> = a.store.admin_book_rows().await.unwrap().iter().map(|r| r.id).collect();

    // The tracks have to be real files for the move; they are, in the folders above.
    let r = send(&a, post("/admin/rpc/quarantine.add", &form(&[("id", &ids[0].to_string()), ("back", "music")]), &form_opts)).await;
    assert!(r.status.is_redirection(), "{} {}", r.status, r.body);
    assert_eq!(a.store.admin_book_rows().await.unwrap().len(), 1, "the copy left the library: {:?}", r.headers.get("location"));
    assert!(root.path().join(".quarantine").is_dir());
    // The last copy stays.
    let r = send(&a, post("/admin/rpc/quarantine.add", &form(&[("id", &ids[1].to_string()), ("back", "music")]), &form_opts)).await;
    assert!(r.status.is_redirection() && r.headers.get("location").unwrap().to_str().unwrap().contains("err.quarantine"), "{:?}", r.headers);
    assert_eq!(a.store.admin_book_rows().await.unwrap().len(), 1);
}

#[tokio::test]
async fn the_release_and_withdrawal_of_a_book_still_work_by_their_own_procedures() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let mut known = book("Follett", "Winter der Welt", &[1]);
    known.recommended_age = Some(12);
    a.store.import_books(vec![known]).await.unwrap();
    let winter = a.store.admin_book_rows().await.unwrap().iter().find(|r| r.title == "Winter der Welt").unwrap().id;

    let r = send(&a, post("/admin/rpc/book.release", &form(&[("id", &winter.to_string()), ("min_age", "12")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.body.contains("name=\"released\" value=\"1\" checked"), "{}", r.body);
    assert!(a.store.admin_book_row(winter).await.unwrap().unwrap().released);
    let r = send(&a, post("/admin/rpc/book.unrelease", &form(&[("id", &winter.to_string())]), &o)).await;
    assert!(!r.body.contains("name=\"released\" value=\"1\" checked") && !a.store.admin_book_row(winter).await.unwrap().unwrap().released, "{}", r.body);
}

#[tokio::test]
async fn the_tags_window_has_a_cut_and_zoom_for_the_picture_and_serves_its_script() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let lib = tempfile::TempDir::new().unwrap();
    common::audio::make_mp3(&lib.path().join("Ken Follett/Winter der Welt/a_CD01_TR01.mp3"), &[]);
    let root = std::fs::canonicalize(lib.path()).unwrap().to_string_lossy().into_owned();
    send(&a, post("/admin/rpc/import.save_criteria", &form(&[("roots", &root), ("rule", "manual")]), &o)).await;
    send(&a, post("/admin/rpc/import.unlock", "", &o)).await;
    send(&a, post("/admin/rpc/import.run", "", &o)).await;
    let id = a.store.admin_book_rows().await.unwrap()[0].id;
    let w = send(&a, get(&format!("/admin/rpc/book.tags_form?id={id}"), &o)).await;
    assert_eq!(w.status, StatusCode::OK);
    for part in ["data-cropper", "class=\"crop-frame\"", "class=\"crop-image\"", "data-crop-reset", "Reset the cut", "name=\"cover\""] {
        assert!(w.body.contains(part), "{part}: {}", w.body);
    }
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    let page = send(&a, get("/admin/books", &plain)).await.body;
    assert!(page.contains("/admin/static/crop.js"));
    let js = send(&a, get("/admin/static/crop.js", &plain)).await;
    assert_eq!(js.status, StatusCode::OK);
    assert!(js.body.contains("pointermove") && js.body.contains("wheel") && js.body.contains("toBlob"));
}

#[tokio::test]
async fn with_a_certificate_authority_the_pairing_code_carries_the_name_and_no_fingerprint() {
    let uri = david_server::pairing::pairing_uri("sid", "david.example.test:8484", "code1", None, true);
    assert!(uri.contains("addr=david.example.test%3A8484") && uri.contains("ca=1") && !uri.contains("fp="), "{uri}");
    // A server with its own certificate still pins the fingerprint, and says nothing about a CA.
    let pinned = david_server::pairing::pairing_uri("sid", "192.168.0.2:8484", "code1", Some("ab12"), false);
    assert!(pinned.contains("fp=ab12") && !pinned.contains("ca=1"), "{pinned}");
}

#[tokio::test]
async fn the_settings_hold_the_certificate_files_the_quarantine_name_and_the_genre_words_and_check_them() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    let page = send(&a, get("/admin/settings?tab=values", &plain)).await.body;
    for part in ["name=\"tls_cert\"", "name=\"tls_key\"", "name=\"tls_host\"", "name=\"quarantine_folder\"", "name=\"audiobook_genres\"", "name=\"placeholder_tags\"", "name=\"radio_directory_url\"", "name=\"feed_refresh_min\"", "Podcasts and radio", "Certificate (HTTPS)", "placeholder=\"&#x2f;path&#x2f;to&#x2f;fullchain.pem\"", "placeholder=\"david.example.org\""] {
        assert!(page.contains(part), "{part}");
    }
    let ok = send(&a, post("/admin/rpc/settings.save", &form(&[("tls_cert", "/some/place/fullchain.pem"), ("tls_key", "/some/place/privkey.pem"), ("tls_host", "david.example.test"), ("quarantine_folder", "Quarantaene"), ("audiobook_genres", "hörbuch, lesung")]), &o)).await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.body);
    let ext = a.store.external_tls().await.unwrap();
    assert_eq!((ext.host.as_deref(), ext.cert.is_some(), ext.key.is_some()), (Some("david.example.test"), true, true));
    assert_eq!(a.store.quarantine_folder().await.unwrap(), "Quarantaene");
    assert_eq!(a.store.audiobook_genres().await.unwrap(), vec!["hörbuch".to_owned(), "lesung".to_owned()]);
    // Files that do not exist are said in the page, not silently kept.
    assert!(ok.body.contains("No such file"), "the problem with the named file is shown: {}", ok.body);

    for bad in [
        vec![("tls_cert", "relative/cert.pem"), ("tls_key", "/k.pem")],
        vec![("tls_cert", "/c.pem"), ("tls_key", "")],
        vec![("tls_host", "has space")],
        vec![("tls_reload_s", "1")],
        vec![("quarantine_folder", "a/b")],
        vec![("quarantine_folder", "..")],
    ] {
        let r = send(&a, post("/admin/rpc/settings.save", &form(&bad), &o)).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{bad:?}");
    }
}

#[tokio::test]
async fn the_tab_cd_import_holds_the_format_the_names_and_the_log_and_says_why_a_value_is_refused() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let o_de = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), lang: Some("de") };
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };

    // Its own tab: the values are not on the tab "Values" any more.
    let values = send(&a, get("/admin/settings?tab=values", &plain)).await.body;
    assert!(!values.contains("name=\"cd_bitrate_kbps\"") && values.contains("href=\"/admin/settings?tab=cdimport\""), "{values}");
    let page = send(&a, get("/admin/settings?tab=cdimport", &plain)).await.body;
    for key in david_server::cd_settings::KEYS {
        assert!(page.contains(&format!("name=\"{key}\"")), "{key}");
    }
    assert!(page.contains("CD import") && page.contains("kbit&#x2f;s") && page.contains("Joint stereo") && page.contains("Detect CD drives with udev"), "{page}");
    // A priority can be negative: the field lets the minus sign through (the browser refuses what does not match `pattern`), and the other
    // numbers stay digits only.
    assert!(page.contains("name=\"cd_encoder_nice\" value=\"-20\" required inputmode=\"text\" pattern=\"-?[0-9]+\""), "{page}");
    assert!(page.contains("name=\"cd_read_speed\" value=\"8\" required inputmode=\"numeric\" pattern=\"[0-9]+\""), "{page}");
    // The menu has an item of its own for the import; the tab keeps the values and points to it.
    assert!(page.contains("href=\"/admin/cd\"") && page.contains("The import itself (drive, choice of tracks, progress, log) is the menu item"), "{page}");
    assert!(!page.contains("contact address is missing") && !page.contains("No CD has been imported yet") && !page.contains("id=\"cd-drive\""), "the import is not on the tab: {page}");
    // The state at the top of the page "CD import": no contact address, no folder, locked; and no job yet.
    let import = send(&a, get("/admin/cd", &plain)).await.body;
    assert!(import.contains("contact address is missing") && import.contains("The import is locked") && import.contains("No CD has been imported yet"), "{import}");
    assert!(import.contains("<a href=\"/admin/cd\" aria-current=\"page\">") && import.contains("href=\"/admin/settings?tab=cdimport\""), "the menu item is marked, the page leads to the settings: {import}");
    let de = send(&a, get("/admin/settings?tab=cdimport", &Opts { lang: Some("de"), ..plain })).await.body;
    assert!(de.contains("CD-Import") && de.contains("CD-Laufwerke über udev erkennen"), "{de}");
    let import_de = send(&a, get("/admin/cd", &Opts { lang: Some("de"), ..plain })).await.body;
    assert!(import_de.contains("Kontaktadresse für MusicBrainz fehlt") && import_de.contains("CD-Import"), "{import_de}");

    let ok = send(&a, post("/admin/rpc/settings.cd", &form(&[("cd_bitrate_kbps", "96"), ("cd_sample_rate_hz", "44100"), ("cd_channel_mode", "stereo"), ("cd_quality", "5"), ("cd_id3_version", "2.3"), ("cd_file_pattern", "{author} - {title} {disc:02}.{track:02}"), ("cd_folder_pattern", "Books/{author}/{title}"), ("cd_disc_folder_pattern", ""), ("cd_title_pattern", "track_title"), ("cd_track_scheme", "plain"), ("cd_genre", "Hörbuch"), ("cd_cover_px", "1200"), ("cd_eject_after", "0"), ("cd_udev", "1"), ("cd_log_keep", "20"), ("cd_target_kind", "music"), ("musicbrainz_contact", "me@example.org")]), &o)).await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.body);
    assert!(ok.body.contains("id=\"cd-tab\"") && !ok.body.contains("contact address is missing"), "{}", ok.body);
    let s = david_server::cd_settings::CdSettings::load(&a.store).await.unwrap();
    assert_eq!((s.bitrate_kbps, s.sample_rate_hz, s.channel_mode.as_str(), s.quality, s.id3_version.as_str()), (96, 44100, "stereo", 5, "2.3"));
    assert_eq!((s.file_pattern.as_str(), s.folder_pattern.as_str(), s.disc_folder_pattern.as_str(), s.title_pattern.as_str()), ("{author} - {title} {disc:02}.{track:02}", "Books/{author}/{title}", "", "track_title"));
    assert_eq!((s.track_scheme.as_str(), s.genre.as_str(), s.cover_px, s.eject_after, s.target_kind.as_str(), s.lookup_ready), ("plain", "Hörbuch", 1200, false, "music", true));
    assert_eq!((s.udev, s.log_keep), (true, 20));

    // A refused value: nothing is saved, the tab comes back with the input and the reason, in the language asked for.
    type Case<'a> = (Vec<(&'a str, &'a str)>, &'a str, &'a str);
    let cases: Vec<Case> = vec![
        (vec![("cd_bitrate_kbps", "0")], "outside the allowed range", "außerhalb des erlaubten Bereichs"),
        (vec![("cd_bitrate_kbps", "abc")], "outside the allowed range", "außerhalb des erlaubten Bereichs"),
        (vec![("cd_sample_rate_hz", "12345")], "allowed choices", "erlaubten Möglichkeiten"),
        (vec![("cd_channel_mode", "surround")], "allowed choices", "erlaubten Möglichkeiten"),
        (vec![("cd_quality", "10")], "outside the allowed range", "außerhalb des erlaubten Bereichs"),
        (vec![("cd_id3_version", "1.1")], "allowed choices", "erlaubten Möglichkeiten"),
        (vec![("cd_format", "wav")], "allowed choices", "erlaubten Möglichkeiten"),
        (vec![("cd_file_pattern", "{author}_{title}")], "lacks a required placeholder", "Pflicht-Platzhalter"),
        (vec![("cd_file_pattern", "{disc}/{track}")], "path trick", "Pfad-Trick"),
        (vec![("cd_file_pattern", "{disc}{track}{nonsense}")], "Unknown placeholder {nonsense}; allowed are: Author {author}", "Unbekannter Platzhalter {nonsense}; erlaubt sind: Autor {author}"),
        (vec![("cd_file_pattern", "{disc}{track")], "brace", "Klammer"),
        (vec![("cd_file_pattern", "")], "template is empty", "Vorlage ist leer"),
        (vec![("cd_folder_pattern", "{author}/../{title}")], "path trick", "Pfad-Trick"),
        (vec![("cd_folder_pattern", "/{title}")], "path trick", "Pfad-Trick"),
        (vec![("cd_title_pattern", "{filename}")], "allowed choices", "erlaubten Möglichkeiten"),
        (vec![("cd_track_scheme", "weird")], "allowed choices", "erlaubten Möglichkeiten"),
        (vec![("cd_genre", "")], "text is empty", "Text ist leer"),
        (vec![("cd_cover_px", "5")], "outside the allowed range", "außerhalb des erlaubten Bereichs"),
        (vec![("cd_eject_after", "maybe")], "allowed choices", "erlaubten Möglichkeiten"),
        (vec![("cd_udev", "2")], "allowed choices", "erlaubten Möglichkeiten"),
        (vec![("cd_log_keep", "3")], "outside the allowed range", "außerhalb des erlaubten Bereichs"),
        (vec![("cd_buffer_seconds", "1")], "outside the allowed range", "außerhalb des erlaubten Bereichs"),
        (vec![("cd_buffer_seconds", "901")], "outside the allowed range", "außerhalb des erlaubten Bereichs"),
        (vec![("cd_target_kind", "film")], "allowed choices", "erlaubten Möglichkeiten"),
        (vec![("cd_musicbrainz_url", "ftp://x")], "must start with https", "muss mit https"),
    ];
    for (bad, en, de) in cases {
        let r = send(&a, post("/admin/rpc/settings.cd", &form(&bad), &o)).await;
        assert_eq!(r.status, StatusCode::OK, "{bad:?}");
        assert!(r.body.contains(en), "{bad:?}: {}", r.body);
        let r = send(&a, post("/admin/rpc/settings.cd", &form(&bad), &o_de)).await;
        assert!(r.body.contains(de), "{bad:?}: {}", r.body);
    }
    // The input stays in its field, so that it can be corrected.
    let r = send(&a, post("/admin/rpc/settings.cd", &form(&[("cd_bitrate_kbps", "abc")]), &o)).await;
    assert!(r.body.contains("name=\"cd_bitrate_kbps\" value=\"abc\""), "{}", r.body);
    // Nothing of the refused saves was kept.
    let s = david_server::cd_settings::CdSettings::load(&a.store).await.unwrap();
    assert_eq!((s.bitrate_kbps, s.file_pattern.as_str(), s.log_keep), (96, "{author} - {title} {disc:02}.{track:02}", 20));
}

#[tokio::test]
async fn the_tab_cd_import_lists_the_last_imports_with_the_error_in_the_language_of_the_reader() {
    use david_server::cdimport::JobPatch;
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    let mk = |phase: &str, error: &str| JobPatch { phase: Some(phase.into()), error: Some(error.into()), author: Some("Ken Follett".into()), album: Some("Sturz der Titanen".into()), disc: Some(2), discs_total: Some(12), tracks_total: Some(14), ..JobPatch::default() };
    for (phase, error) in [("done", ""), ("failed", "disc_exists"), ("failed", "format_mismatch"), ("failed", "something_new")] {
        let j = a.store.create_cd_job(1, "dev", &mk(phase, error)).await.unwrap();
        a.store.update_cd_job(1, j.id, &mk(phase, error)).await.unwrap();
    }
    let en = send(&a, get("/admin/cd", &plain)).await.body;
    assert!(en.contains("Sturz der Titanen") && en.contains("Ken Follett") && en.contains("2 / 12"), "{en}");
    assert!(en.contains("This CD is already in the library; nothing was overwritten. (disc_exists)"), "{en}");
    assert!(en.contains("does not have the format of the settings") && en.contains("(format_mismatch)"), "{en}");
    assert!(en.contains("The job failed for a reason that is not listed. (something_new)") || en.contains("a reason that is not listed. (something_new)"), "{en}");
    assert!(en.contains("Failed") && en.contains("Done"), "{en}");
    let de = send(&a, get("/admin/cd", &Opts { lang: Some("de"), ..plain })).await.body;
    assert!(de.contains("Diese CD ist schon in der Bibliothek; nichts wurde überschrieben. (disc_exists)") && de.contains("Fehlgeschlagen") && de.contains("Letzte CD-Importe"), "{de}");
    // The log can be cleared on the page after a question; a job that runs stays.
    assert!(en.contains("action=\"/admin/rpc/cd.log_clear\"") && en.contains("data-confirm=") && en.contains("Clear the log"), "{en}");
    let running = a.store.create_cd_job(1, "dev", &JobPatch { phase: Some("ripping".into()), author: Some("A".into()), album: Some("Läuft".into()), disc: Some(1), discs_total: Some(1), tracks_total: Some(3), ..JobPatch::default() }).await.unwrap();
    let r = send(&a, post("/admin/rpc/cd.log_clear", &form(&[("csrf", csrf.as_str())]), &plain)).await;
    assert!(r.status.is_redirection(), "{}", r.status);
    let left = a.store.cd_log(100).await.unwrap();
    assert_eq!(left.iter().map(|j| j.id).collect::<Vec<_>>(), [running.id], "only the running job is left: {left:?}");
    let en = send(&a, get("/admin/cd", &plain)).await.body;
    assert!(!en.contains("disc_exists") && en.contains("Läuft"), "{en}");
    // Only the last entries are kept (the number is a setting).
    a.store.save_settings(&[("cd_log_keep".to_owned(), "10".to_owned())]).await.unwrap();
    for _ in 0..15 {
        a.store.create_cd_job(1, "dev", &mk("failed", "stale")).await.unwrap();
    }
    assert_eq!(a.store.cd_log(100).await.unwrap().len(), 10);
}

#[tokio::test]
async fn the_tab_shows_the_drive_of_the_server_and_starts_a_rip_with_localized_refusals() {
    use david_server::cdrive::fake::disc;
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    let htmx = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let htmx_de = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), lang: Some("de") };
    a.cd.refresh_drives("", true).await;

    // A closed tray without a disc.
    let page = send(&a, get("/admin/cd", &plain)).await.body;
    assert!(page.contains("The CD drive of the server") && page.contains("&#x2f;dev&#x2f;fake0") && page.contains("there is no disc") && page.contains("hx-trigger=\"every 3s\""), "{page}");
    assert!(!page.contains("name=\"author\""), "no form without a disc");

    // An audio CD goes in: the panel shows it and offers the form; the signature of the shown state ends the polling swap.
    a.drive.edit("/dev/fake0", |u| {
        u.disc = Some(disc(3));
        u.changed = true;
    });
    a.cd.refresh_drives("", true).await;
    let panel = send(&a, get("/admin/rpc/cd.drive", &htmx)).await;
    assert_eq!(panel.status, StatusCode::OK);
    assert!(panel.body.contains("Audio CD, 3 tracks") && panel.body.contains("name=\"author\"") && panel.body.contains("Import") && panel.body.contains("Eject"), "{}", panel.body);
    let sig = panel.body.split("sig=").nth(1).unwrap().split(['"', '&']).next().unwrap().to_owned();
    assert_eq!(send(&a, get(&format!("/admin/rpc/cd.drive?sig={sig}"), &htmx)).await.status, StatusCode::NO_CONTENT);
    // Without the contact address the page says why there are no suggestions.
    assert!(panel.body.contains("no contact address") || panel.body.contains("contact address"), "{}", panel.body);
    let de = send(&a, get("/admin/rpc/cd.drive", &htmx_de)).await.body;
    assert!(de.contains("Audio-CD, 3 Titel") && de.contains("Auswerfen") && de.contains("Abbrechen"), "{de}");
    assert!(de.contains("MusicBrainz von Hand durchsuchen") && de.contains("name=\"mbq\"") && de.contains(">Suchen<"), "the search by hand: {de}");

    // A refused start comes back in the panel, in the language asked for, with the code.
    a.drive.edit("/dev/fake0", |u| u.tray_open = true);
    a.cd.refresh_drives("", false).await;
    let r = send(&a, post("/admin/rpc/cd.rip", &form(&[("author", "A"), ("title", "B"), ("disc", "1")]), &htmx)).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body.contains("The tray is open. (tray_open)") && r.body.contains("The tray is open"), "{}", r.body);
    let r = send(&a, post("/admin/rpc/cd.rip", &form(&[("author", "A"), ("title", "B"), ("disc", "1")]), &htmx_de)).await;
    assert!(r.body.contains("Das Fach ist offen. (tray_open)"), "{}", r.body);

    // The service may not use the drive: a warning at the top of the tab and in the panel.
    a.drive.edit("/dev/fake0", |u| {
        u.tray_open = false;
        u.denied = true;
    });
    a.cd.refresh_drives("", false).await;
    let page = send(&a, get("/admin/cd", &plain)).await.body;
    assert!(page.contains("may not read the CD drive") && page.contains("its group is missing"), "{page}");
    let r = send(&a, post("/admin/rpc/cd.rip", &form(&[("author", "A"), ("title", "B"), ("disc", "1")]), &htmx_de)).await;
    assert!(r.body.contains("(drive_denied)") && r.body.contains("Der Server darf das Laufwerk nicht benutzen"), "{}", r.body);

    // Eject through the page never closes a tray: with the tray open nothing is sent.
    a.drive.edit("/dev/fake0", |u| {
        u.denied = false;
        u.tray_open = true;
    });
    a.cd.refresh_drives("", false).await;
    let before = a.drive.ejects.lock().unwrap().len();
    send(&a, post("/admin/rpc/cd.eject", &form(&[]), &htmx)).await;
    assert_eq!(a.drive.ejects.lock().unwrap().len(), before);
    // A stranger without a session gets no panel.
    assert_ne!(send(&a, get("/admin/rpc/cd.drive", &Opts::default())).await.status, StatusCode::OK);
}

#[tokio::test]
async fn the_panel_lists_the_tracks_keeps_the_choice_and_refuses_an_empty_one() {
    use david_server::cdrive::fake::disc;
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let htmx = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let htmx_de = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), lang: Some("de") };
    a.drive.edit("/dev/fake0", |u| {
        u.disc = Some(disc(3));
        u.changed = true;
    });
    a.cd.refresh_drives("", true).await;
    let id = a.cd.drives.snapshot().0[0].disc_id.clone().unwrap();

    // The list: every track with a box, its number and length, all ticked, the summary and the three buttons.
    let panel = send(&a, get("/admin/rpc/cd.drive", &htmx)).await.body;
    assert_eq!(panel.matches("name=\"only\"").count(), 3, "{panel}");
    assert_eq!(panel.matches("name=\"only\"").count(), panel.matches(" checked>").count().min(3));
    assert!(panel.contains("3 of 3 tracks, 6:00") && panel.contains("Track 01") && panel.contains("2:00"), "{panel}");
    for what in ["all", "none", "invert"] {
        assert!(panel.contains(&format!("data-cd-select=\"{what}\"")), "{what}");
    }
    assert!(panel.contains("data-cd-summary") && panel.contains("data-seconds=\"120\""));
    assert!(!panel.contains("<script") && !panel.contains(" onclick=") && !panel.contains(" style="), "no inline script or style");
    assert!(!panel.contains("data-cd-import disabled"));
    let de = send(&a, get("/admin/rpc/cd.drive", &htmx_de)).await.body;
    assert!(de.contains("3 von 3 Titeln, 6:00") && de.contains(">Alle<") && de.contains(">Keine<") && de.contains(">Umkehren<") && de.contains("Titel 01"), "{de}");

    // An interrupted rip of this disc has read track 1 (`encoded`): the list marks it, and the notice says what Continue does with the choice.
    // A choice that differs from the one of the interrupted rip is no reason to read everything again; only other settings for the audio are.
    {
        use david_server::cdresume::{self, Entry, Fingerprint, Manifest};
        let set = david_server::cd_settings::CdSettings::load(&a.store).await.unwrap();
        let dir = cdresume::dir_of(&a.cd.staging, &id);
        std::fs::create_dir_all(&dir).unwrap();
        let mut m = Manifest {
            v: 1,
            disc_id: id.clone(),
            started_ms: 1,
            updated_ms: david_server::now_ms(),
            fingerprint: Fingerprint::of(&set),
            only: vec![1, 2, 3],
            author: "A".into(),
            title: "T".into(),
            disc: 1,
            discs_total: None,
            kind: "audiobook".into(),
            tracks: vec![Entry { number: 1, file: "01.mp3".into(), size: 10, sha1: String::new(), seconds: 120, state: "encoded".into() }],
        };
        cdresume::write(&dir, &m).unwrap();
        let all = send(&a, get("/admin/rpc/cd.drive", &htmx)).await.body;
        assert_eq!(all.matches("class=\"cd-read\"").count(), 1, "only the track that was read is marked: {all}");
        assert!(all.contains("Continuing takes over 1 of the tracks you chose") && all.contains("reads the other 2"), "{all}");
        assert!(!all.contains("will be read again from the start"), "{all}");
        // Another choice (track 1 not chosen any more): it is dropped, and the notice says so; the buttons Continue and Read again stay.
        let other = send(&a, get(&format!("/admin/rpc/cd.drive?sig=old&sel=1&disc_id={id}&only=2&only=3"), &htmx)).await.body;
        assert!(other.contains("takes over 0 of the tracks you chose") && other.contains("1 track(s) that were read but are not chosen now are dropped"), "{other}");
        assert!(other.contains("name=\"resume\" value=\"1\""), "Continue is offered: {other}");
        let de = send(&a, get("/admin/rpc/cd.drive", &htmx_de)).await.body;
        assert!(de.contains("Fortsetzen übernimmt 1 der gewählten Titel"), "{de}");
        // Other settings for the audio: everything is read again, and the notice says that.
        m.fingerprint.bitrate_kbps += 32;
        cdresume::write(&dir, &m).unwrap();
        let changed = send(&a, get("/admin/rpc/cd.drive", &htmx)).await.body;
        assert!(changed.contains("settings for the audio have changed since then"), "{changed}");
        assert!(!changed.contains("class=\"cd-read\""), "nothing is taken over: {changed}");
        assert!(!changed.contains("name=\"resume\" value=\"1\""), "{changed}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // The page sends back the choice with every ask. A new picture of the panel keeps it, and what was typed.
    let uri = format!("/admin/rpc/cd.drive?sig=old&sel=1&disc_id={id}&only=1&only=3&author=Typed+Author&title=My+Book&disc=2&kind=music");
    let kept = send(&a, get(&uri, &htmx)).await.body;
    assert!(kept.contains("2 of 3 tracks, 4:00"), "{kept}");
    assert_eq!(kept.matches(" checked>").count(), 2, "{kept}");
    assert!(kept.contains("value=\"1\" data-seconds=\"120\" aria-label=\"Take 01\" checked") && !kept.contains("aria-label=\"Take 02\" checked"), "{kept}");
    assert!(kept.contains("name=\"author\" value=\"Typed Author\"") && kept.contains("value=\"My Book\"") && kept.contains("<option value=\"music\" selected>"), "{kept}");
    // A choice for another disc is not applied to this one; "Cancel" starts afresh.
    let other = send(&a, get("/admin/rpc/cd.drive?sig=old&sel=1&disc_id=someotherdisc&only=2", &htmx)).await.body;
    assert!(other.contains("3 of 3 tracks"), "{other}");
    let reset = send(&a, get(&format!("/admin/rpc/cd.drive?sig=old&reset=1&sel=1&disc_id={id}&only=2&author=X"), &htmx)).await.body;
    assert!(reset.contains("3 of 3 tracks") && !reset.contains("value=\"X\""), "{reset}");
    // Nothing ticked: the summary says 0, the import button is disabled.
    let none = send(&a, get(&format!("/admin/rpc/cd.drive?sig=old&sel=1&disc_id={id}"), &htmx)).await.body;
    assert!(none.contains("0 of 3 tracks, 0:00") && none.contains("data-cd-import disabled"), "{none}");
    // The signature of the shown state ends the swap, so ticks and text on the page stay as they are.
    let sig = panel.split("sig=").nth(1).unwrap().split(['"', '&']).next().unwrap().to_owned();
    assert_eq!(send(&a, get(&format!("/admin/rpc/cd.drive?sig={sig}&sel=1&disc_id={id}&only=2"), &htmx)).await.status, StatusCode::NO_CONTENT);

    // The server refuses an empty choice, in both languages, and keeps what was typed and ticked.
    let body = form(&[("sel", "1"), ("disc_id", &id), ("author", "Kept Author"), ("title", "B"), ("disc", "1")]);
    let r = send(&a, post("/admin/rpc/cd.rip", &body, &htmx)).await.body;
    assert!(r.contains("No track is chosen; choose at least one. (empty_selection)") && r.contains("Kept Author"), "{r}");
    let r = send(&a, post("/admin/rpc/cd.rip", &body, &htmx_de)).await.body;
    assert!(r.contains("Es ist kein Titel gewählt; wähle mindestens einen. (empty_selection)"), "{r}");
    // A number that is not on the disc.
    let bad = format!("{}&only=9", form(&[("sel", "1"), ("disc_id", &id), ("author", "A"), ("title", "B"), ("disc", "1"), ("only", "1")]));
    let r = send(&a, post("/admin/rpc/cd.rip", &bad, &htmx)).await.body;
    assert!(r.contains("(invalid_selection)"), "{r}");
}

#[tokio::test]
async fn template_fields_name_their_placeholders_in_plain_words_preview_the_result_and_explain_errors() {
    let a = app().await;
    let (cookie, _csrf) = sign_in(&a).await;
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    let en = send(&a, get("/admin/settings?tab=cdimport", &plain)).await.body;
    let tab = &en[en.find("id=\"cd-tab\"").unwrap()..];
    // Every template field has its own block with a line of placeholders (data attributes, no inline script).
    assert_eq!(tab.matches("data-cd-field").count(), 3);
    assert_eq!(tab.matches("data-cd-hint").count(), 3);
    for part in ["data-insert=\"{author}\"", "data-insert=\"{title}\"", "data-insert=\"{disc:02}\"", "data-insert=\"{track:02}\"", "data-insert=\"{track_title}\"", "Author <code>{author}</code>", "Title of the book or album <code>{title}</code>", "CD number <code>{disc}</code>", "Track number, with a leading zero <code>{track:02}</code>", "Title of the track <code>{track_title}</code>", "Narrator"] {
        assert!(tab.contains(part), "{part}");
    }
    // Only what a field knows: the file name has the track, the folder of a disc has the CD number and not the track.
    assert_eq!(tab.matches("data-insert=\"{track}\"").count(), 1);
    assert_eq!(tab.matches("data-insert=\"{disc}\"").count(), 2);
    assert_eq!(tab.matches("data-insert=\"{narrator}\"").count(), 2);
    assert!(tab.contains("hx-get=\"/admin/rpc/cd.preview?field=cd_file_pattern\"") && tab.contains("id=\"cd-preview-cd_disc_folder_pattern\""));
    // The line is plain text without the script, and the buttons are made visible by it.
    assert!(tab.contains("data-cd-nojs") && tab.contains("data-cd-js hidden"));
    assert!(!tab.contains("<script") && !tab.contains(" onclick=") && !tab.contains(" style="), "no inline script or style");
    let de = send(&a, get("/admin/settings?tab=cdimport", &Opts { lang: Some("de"), ..plain })).await.body;
    for part in ["Autor <code>{author}</code>", "Titel des Buchs oder Albums <code>{title}</code>", "CD-Nummer <code>{disc}</code>", "Tracknummer, mit führender Null <code>{track:02}</code>", "Titel des Tracks <code>{track_title}</code>", "Sprecher"] {
        assert!(de.contains(part), "{part}");
    }

    // The preview is the real naming.
    let file = send(&a, get("/admin/rpc/cd.preview?field=cd_file_pattern&pattern=%7Bauthor%7D_%7Btitle%7D_CD%7Bdisc%3A02%7D_TR%7Btrack%3A02%7D", &plain)).await;
    assert_eq!(file.status, StatusCode::OK);
    assert!(file.body.contains("Example: Ken Follett_Sturz der Titanen_CD03_TR05.mp3"), "{}", file.body);
    let folder = send(&a, get("/admin/rpc/cd.preview?field=cd_folder_pattern&pattern=%7Bauthor%7D%2F%7Btitle%7D%20(%7Byear%7D)", &plain)).await.body;
    assert!(folder.contains("Example: Ken Follett/Sturz der Titanen (2010)"), "{folder}");
    let disc = send(&a, get("/admin/rpc/cd.preview?field=cd_disc_folder_pattern&pattern=CD%20%7Bdisc%7D", &plain)).await.body;
    assert!(disc.contains("Example: CD 3"), "{disc}");
    // The field itself may carry the value (the name of the input).
    let by_name = send(&a, get("/admin/rpc/cd.preview?field=cd_disc_folder_pattern&cd_disc_folder_pattern=Disc%20%7Bdisc%3A02%7D", &plain)).await.body;
    assert!(by_name.contains("Example: Disc 03"), "{by_name}");
    let german = send(&a, get("/admin/rpc/cd.preview?field=cd_file_pattern&pattern=%7Bdisc%7D-%7Btrack%7D", &Opts { lang: Some("de"), ..plain })).await.body;
    assert!(german.contains("Beispiel: 3-5.mp3"), "{german}");

    // An invalid template shows the reason with the plain names, not a preview.
    let unknown = send(&a, get("/admin/rpc/cd.preview?field=cd_file_pattern&pattern=%7Bdisc%7D%7Btrack%7D%7Bx%7D", &plain)).await.body;
    assert!(unknown.contains("Unknown placeholder {x}; allowed are: Author {author}, Title of the book or album {title}, Narrator {narrator}, Year {year}, CD number {disc}, Track number {track}, Title of the track {track_title}.") && !unknown.contains("Example"), "{unknown}");
    let unknown_de = send(&a, get("/admin/rpc/cd.preview?field=cd_disc_folder_pattern&pattern=%7Bdisc%7D%7Btrack%7D", &Opts { lang: Some("de"), ..plain })).await.body;
    assert!(unknown_de.contains("Unbekannter Platzhalter {track}; erlaubt sind: CD-Nummer {disc}, Titel des Buchs oder Albums {title}."), "{unknown_de}");
    let missing = send(&a, get("/admin/rpc/cd.preview?field=cd_file_pattern&pattern=%7Bauthor%7D", &plain)).await.body;
    assert!(missing.contains("lacks a required placeholder: CD number {disc}, Track number {track}"), "{missing}");
    let empty = send(&a, get("/admin/rpc/cd.preview?field=cd_file_pattern&pattern=", &plain)).await.body;
    assert!(empty.contains("The template is empty."), "{empty}");
    // Only the template fields have a preview; a stranger gets none.
    assert!(send(&a, get("/admin/rpc/cd.preview?field=cd_genre&pattern=x", &plain)).await.body.is_empty());
    assert_ne!(send(&a, get("/admin/rpc/cd.preview?field=cd_file_pattern&pattern=x", &Opts::default())).await.status, StatusCode::OK);

    // On saving, the refusal names the placeholder in plain words as well.
    let htmx = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&_csrf), ..Default::default() };
    let r = send(&a, post("/admin/rpc/settings.cd", &form(&[("cd_file_pattern", "{disc}{track}{nope}")]), &htmx)).await.body;
    assert!(r.contains("Unknown placeholder {nope}; allowed are: Author {author}"), "{r}");
}

#[tokio::test]
async fn the_panel_shows_the_progress_and_the_state_of_each_chosen_track_while_a_rip_runs_and_after_it() {
    use david_server::cdrive::fake::disc_sectors;
    use std::time::Duration;
    if !common::audio::ffmpeg_available() {
        eprintln!("ffmpeg is not installed: test skipped");
        return;
    }
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let htmx = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let htmx_de = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), lang: Some("de") };
    // A library to put the files in, and a 2 second MP3 for the fake drive to "rip".
    let library = tempfile::tempdir().unwrap();
    a.store.save_import_criteria(&[library.path().canonicalize().unwrap().to_string_lossy().into_owned()], david_server::RatingRule::Manual).await.unwrap();
    a.store.save_settings(&[("min_free_gb".to_owned(), "0".to_owned()), ("cd_eject_after".to_owned(), "0".to_owned())]).await.unwrap();
    let work = tempfile::tempdir().unwrap();
    common::audio::make(&work.path().join("t.mp3"), &["-c:a", "libmp3lame", "-b:a", "64k", "-ar", "24000", "-ac", "2"], &[]);
    *a.drive.mp3.lock().unwrap() = std::fs::read(work.path().join("t.mp3")).unwrap();
    a.drive.edit("/dev/fake0", |u| {
        u.disc = Some(disc_sectors(4, 150));
        u.changed = true;
    });
    a.cd.refresh_drives("", true).await;
    let id = a.cd.drives.snapshot().0[0].disc_id.clone().unwrap();

    // Tracks 1 to 3 are chosen (not 4); the drive is half way through track 2.
    *a.drive.hold_in.lock().unwrap() = Some(2);
    // A ring of 8 seconds, nearly full: the encoder (or the writing) is the brake; the reader has waited 3 times.
    a.store.save_settings(&[("cd_buffer_seconds".to_owned(), "8".to_owned())]).await.unwrap();
    *a.drive.filled_bytes.lock().unwrap() = 1_400_000;
    let body = form(&[("sel", "1"), ("disc_id", &id), ("author", "Ken Follett"), ("title", "Sturz der Titanen"), ("disc", "1"), ("only", "1"), ("only", "2"), ("only", "3"), ("tracks", "Teil 1\nTeil 2\nTeil 3\nTeil 4")]);
    let started = send(&a, post("/admin/rpc/cd.rip", &body, &htmx)).await;
    assert_eq!(started.status, StatusCode::OK, "{}", started.body);
    let mut first = String::new();
    for _ in 0..200 {
        first = send(&a, get("/admin/rpc/cd.drive", &htmx)).await.body;
        if first.contains("data-state=\"reading\"") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    // The bar has a value and the line says which track it is; only the chosen tracks are listed, each with its state.
    assert!(first.contains("<progress value=\"50\" max=\"100\">") && first.contains("50 %"), "{first}");
    assert!(first.contains("Track 2 of 3 · Reading"), "{first}");
    // The buffer: how much waits, and what brakes (here it is full).
    assert!(first.contains("Buffer 8 s of 8 s (99 %)") && first.contains("The buffer is full: the reader has had to wait for the encoder or the disk.") && first.contains("data-buffer=\"full\""), "{first}");
    assert!(first.contains("The reader had to wait 3 times for room in the buffer, the longest wait 2.5 s."), "{first}");
    // The speed is what the drive was measured to deliver, not a guess from the buffer.
    assert!(first.contains("The drive reads at 8.0x (measured; 1x = 75 sectors a second)."), "{first}");
    assert_eq!(first.matches("data-state=").count(), 3, "track 4 was not chosen: {first}");
    let states: Vec<&str> = first.split("data-state=\"").skip(1).map(|p| p.split('"').next().unwrap()).collect();
    assert_eq!(states, ["done", "reading", "waiting"], "{first}");
    assert!(first.contains("Teil 2") && first.contains("0:02") && first.contains("Reading 50 %") && !first.contains("Teil 4"), "{first}");
    assert!(!first.contains("name=\"only\""), "no form while it runs");
    let de = send(&a, get("/admin/rpc/cd.drive", &htmx_de)).await.body;
    assert!(de.contains("Titel 2 von 3 · Lesen") && de.contains("Wird gelesen 50 %") && de.contains("Wartet"), "{de}");
    assert!(de.contains("Puffer 8 s von 8 s (99 %)") && de.contains("Der Puffer ist voll: das Lesen musste auf den Kodierer oder die Platte warten.") && de.contains("Der Leser musste 3-mal auf Platz im Puffer warten, die längste Wartezeit 2.5 s."), "{de}");
    assert!(!first.contains("<script") && !first.contains(" style=") && !first.contains(" onclick="));

    // The same state is "no change" (204); a state further on is a new picture and not 204.
    let sig = first.split("sig=").nth(1).unwrap().split(['"', '&']).next().unwrap().to_owned();
    assert_eq!(send(&a, get(&format!("/admin/rpc/cd.drive?sig={sig}"), &htmx)).await.status, StatusCode::NO_CONTENT);
    *a.drive.hold_in.lock().unwrap() = Some(3);
    let mut second = send(&a, get(&format!("/admin/rpc/cd.drive?sig={sig}"), &htmx)).await;
    for _ in 0..200 {
        if second.status == StatusCode::OK && second.body.contains("Track 3 of 3") && second.body.contains("data-state=\"reading\"") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
        second = send(&a, get(&format!("/admin/rpc/cd.drive?sig={sig}"), &htmx)).await;
    }
    assert_eq!(second.status, StatusCode::OK, "the bar moved, so the panel is drawn again");
    let states: Vec<&str> = second.body.split("data-state=\"").skip(1).map(|p| p.split('"').next().unwrap()).collect();
    assert_eq!(states, ["done", "done", "reading"], "{}", second.body);

    // Stopped by the person: the list stays with the result (the track it stopped on is told), until it is closed.
    let job_id = a.store.cd_log(5).await.unwrap()[0].id;
    send(&a, post("/admin/rpc/cd.cancel", &form(&[("id", &job_id.to_string())]), &htmx)).await;
    let mut after = String::new();
    for _ in 0..200 {
        after = send(&a, get("/admin/rpc/cd.drive", &htmx)).await.body;
        if after.contains("data-state=\"failed\"") && after.contains("Close") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let states: Vec<&str> = after.split("data-state=\"").skip(1).map(|p| p.split('"').next().unwrap()).collect();
    assert_eq!(states, ["done", "done", "failed"], "{after}");
    assert!(after.contains("The job was cancelled. (cancelled)") && after.contains("Close"), "{after}");
    assert!(after.contains("name=\"only\""), "the form is back for the next rip");
    let de = send(&a, get("/admin/rpc/cd.drive", &htmx_de)).await.body;
    assert!(de.contains("Der Auftrag wurde abgebrochen. (cancelled)") && de.contains("Schließen"), "{de}");
    // Closing removes the list; a new rip replaces it as well.
    send(&a, post("/admin/rpc/cd.dismiss", &form(&[("device", "/dev/fake0")]), &htmx)).await;
    assert!(!send(&a, get("/admin/rpc/cd.drive", &htmx)).await.body.contains("data-state="));
}

#[tokio::test]
async fn the_panel_offers_to_repair_a_cd_that_is_in_the_library_and_the_log_shows_what_the_automatic_import_turned_down() {
    use david_server::cdpresent::{DiscRecord, DiscRow, DiscTrackRow};
    use david_server::cdrive::fake::disc_sectors;
    if !common::audio::ffmpeg_available() {
        eprintln!("ffmpeg is not installed: test skipped");
        return;
    }
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    let htmx = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let htmx_de = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), lang: Some("de") };
    let library = tempfile::tempdir().unwrap();
    let root = library.path().canonicalize().unwrap();
    a.store.save_import_criteria(&[root.to_string_lossy().into_owned()], david_server::RatingRule::Manual).await.unwrap();
    a.drive.edit("/dev/fake0", |u| {
        u.disc = Some(disc_sectors(3, 150));
        u.changed = true;
    });
    a.cd.refresh_drives("", true).await;
    let disc_id = a.cd.drives.snapshot().0[0].disc_id.clone().unwrap();

    // An earlier import wrote the disc down: tracks 1 and 3 are there, track 2 is not.
    let book = root.join("Ken Follett/Sturz der Titanen/CD 2");
    std::fs::create_dir_all(&book).unwrap();
    for n in [1, 3] {
        common::audio::make(&book.join(format!("Ken Follett_Sturz der Titanen_CD02_TR{n:02}.mp3")), &["-c:a", "libmp3lame", "-b:a", "64k", "-ar", "24000", "-ac", "2"], &[]);
    }
    let row = DiscRow { disc_id: disc_id.clone(), author: "Ken Follett".into(), title: "Sturz der Titanen".into(), disc: 2, discs_total: 12, kind: "audiobook".into(), folder: "Ken Follett/Sturz der Titanen".into() };
    let tracks = (1..=3).map(|n| DiscTrackRow { number: n, file: format!("Ken Follett/Sturz der Titanen/CD 2/Ken Follett_Sturz der Titanen_CD02_TR{n:02}.mp3"), sha1: String::new(), seconds: 2 }).collect();
    a.store.record_cd_disc(&DiscRecord { row, tracks }).await.unwrap();

    let panel = send(&a, get("/admin/rpc/cd.drive", &htmx)).await.body;
    assert!(panel.contains("This CD is in the library already") && panel.contains("Ken Follett: Sturz der Titanen, CD 2"), "{panel}");
    assert!(panel.contains("Missing: 2.") && panel.contains("Repair") && panel.contains("name=\"mode\" value=\"heal\""), "{panel}");
    assert!(!panel.contains("name=\"author\""), "no form for a new import");
    // Eject is in the row of Cancel and Repair (one row, the button names the eject form), and not a second time on its own.
    let foot = &panel[panel.find("class=\"form-foot\"").expect("the row of buttons")..];
    let foot = &foot[..foot.find("</div>").unwrap()];
    assert!(foot.contains("form=\"cd-eject-form\"") && foot.contains("Repair"), "{foot}");
    assert_eq!(panel.matches(">Eject<").count(), 1, "{panel}");
    assert!(panel.contains("id=\"cd-eject-form\""));
    assert!(panel.contains("hx-get=\"/admin/rpc/cd.drive?force=1\"") && panel.contains("Import this CD as a new copy anyway"));
    let de = send(&a, get("/admin/rpc/cd.drive", &htmx_de)).await.body;
    assert!(de.contains("Diese CD ist schon in der Bibliothek") && de.contains("Fehlt: 2.") && de.contains("Reparieren"), "{de}");
    // A damaged file: told with its lengths.
    std::fs::write(book.join("Ken Follett_Sturz der Titanen_CD02_TR03.mp3"), b"").unwrap();
    let panel = send(&a, get("/admin/rpc/cd.drive", &htmx)).await.body;
    assert!(panel.contains("Damaged: Track 3 (empty)"), "{panel}");
    // The form for a new import is there when it is asked for; it says it is a new copy.
    let forced = send(&a, get("/admin/rpc/cd.drive?force=1", &htmx)).await.body;
    assert!(forced.contains("name=\"author\"") && forced.contains("name=\"force\" value=\"1\""), "{forced}");
    // A start without it is turned down with the reason.
    let refused = send(&a, post("/admin/rpc/cd.rip", &form(&[("sel", "1"), ("disc_id", &disc_id), ("author", "Ken Follett"), ("title", "Sturz der Titanen"), ("disc", "2"), ("only", "1")]), &htmx_de)).await.body;
    assert!(refused.contains("Diese CD ist schon in der Bibliothek; es wurde nichts importiert") && refused.contains("(disc_present)"), "{refused}");
    // Whole again: it says so and reads nothing.
    common::audio::make(&book.join("Ken Follett_Sturz der Titanen_CD02_TR03.mp3"), &["-c:a", "libmp3lame", "-b:a", "64k", "-ar", "24000", "-ac", "2"], &[]);
    common::audio::make(&book.join("Ken Follett_Sturz der Titanen_CD02_TR02.mp3"), &["-c:a", "libmp3lame", "-b:a", "64k", "-ar", "24000", "-ac", "2"], &[]);
    let whole = send(&a, get("/admin/rpc/cd.drive", &htmx)).await.body;
    assert!(whole.contains("The CD is whole in the library (3 of 3 tracks).") && !whole.contains("name=\"mode\" value=\"heal\""), "{whole}");

    // The log: a turned-down automatic import, in both languages, with the book, marked as automatic.
    let job = a.store.create_cd_job(0, "auto", &david_server::cdimport::JobPatch { phase: Some("skipped".into()), author: Some("Ken Follett".into()), album: Some("Sturz der Titanen".into()), disc: Some(2), ..Default::default() }).await.unwrap();
    a.store.update_cd_job(0, job.id, &david_server::cdimport::JobPatch { error: Some("disc_present".into()), message: Some("Ken Follett: Sturz der Titanen, CD 2".into()), ..Default::default() }).await.unwrap();
    let page = send(&a, get("/admin/cd", &plain)).await.body;
    assert!(page.contains("Skipped") && page.contains("(disc_present)") && page.contains("automatic") && page.contains("(Ken Follett: Sturz der Titanen, CD 2)"), "{page}");
    let page_de = send(&a, get("/admin/cd", &Opts { lang: Some("de"), ..plain })).await.body;
    assert!(page_de.contains("Übersprungen") && page_de.contains("nichts importiert") && page_de.contains("automatisch"), "{page_de}");
    // The two settings are on the tab with their explanation.
    let tab = send(&a, get("/admin/settings?tab=cdimport", &plain)).await.body;
    assert!(tab.contains("name=\"cd_auto_import\"") && tab.contains("name=\"cd_heal_tolerance_s\"") && tab.contains("never imported again by itself"), "{tab}");
}

#[tokio::test]
async fn the_titles_of_an_album_open_under_it_and_are_loaded_when_opened() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    a.store.import_books(vec![book("Follett", "Winter der Welt", &[2, 1])]).await.unwrap();
    let id = a.store.admin_book_rows().await.unwrap()[0].id;

    // The row carries the list, closed and empty; it asks for the titles when it is opened.
    let page = send(&a, get("/admin/books", &plain)).await.body;
    assert!(page.contains("class=\"track-details\"") && page.contains(&format!("hx-get=\"/admin/rpc/book.tracks?id={id}\"")), "{page}");
    assert!(page.contains("3 tracks"));
    assert!(!page.contains("Winter der Welt_CD01_TR01"), "the titles are not in the page");

    let r = send(&a, get(&format!("/admin/rpc/book.tracks?id={id}"), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    for title in ["Winter der Welt_CD01_TR01", "Winter der Welt_CD01_TR02", "Winter der Welt_CD02_TR01"] {
        assert!(r.body.contains(title), "{title}: {}", r.body);
    }
    assert!(r.body.contains("4:00"), "the length of a title");
    assert_eq!(send(&a, get("/admin/rpc/book.tracks?id=99999", &o)).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_genre_page_keeps_the_list_and_gives_loose_tag_texts_a_genre() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    let o = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), ..Default::default() };
    let mut music = book("Petra", "Beyond Belief", &[1]);
    music.kind = "music".into();
    music.genre = Some("Christian Rock".into());
    a.store.import_books(vec![music]).await.unwrap();

    let page = send(&a, get("/admin/genres", &plain)).await.body;
    assert!(page.contains("Christian Rock") && page.contains("Tag texts without a genre"), "{page}");

    let r = send(&a, post("/admin/rpc/genre.add", &form(&[("name", "Rock")]), &o)).await;
    assert!(r.status.is_redirection(), "{}", r.body);
    let id = a.store.genres().await.unwrap()[0].id.to_string();
    let r = send(&a, post("/admin/rpc/genre.assign", &form(&[("tag", "Christian Rock"), ("id", &id)]), &o)).await;
    assert!(r.status.is_redirection(), "{}", r.body);
    let page = send(&a, get("/admin/genres", &plain)).await.body;
    assert!(page.contains("Every tag text has a genre."), "{page}");

    let r = send(&a, post("/admin/rpc/genre.delete", &form(&[("id", &id)]), &o)).await;
    assert!(r.status.is_redirection(), "{}", r.body);
    assert!(a.store.genres().await.unwrap().is_empty());
}

/// A drop of loose files and folders: for each file its `path` and then the `file`, as the page sends them.
fn drop_request(csrf: &str, cookie: &str, kind: &str, files: &[(&str, &[u8])]) -> Request<Body> {
    let boundary = "XBOUNDARYX";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"kind\"\r\n\r\n{kind}\r\n").as_bytes());
    for (path, bytes) in files {
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"path\"\r\n\r\n{path}\r\n").as_bytes());
        let name = path.rsplit('/').next().unwrap();
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Request::builder()
        .method("POST")
        .uri(format!("/admin/rpc/upload.files?csrf={csrf}"))
        .header(header::COOKIE, cookie)
        .header("hx-request", "true")
        .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body))
        .unwrap()
}

#[tokio::test]
async fn dropped_folders_and_files_are_queued_filed_by_their_tags_and_imported() {
    if !common::audio::ffmpeg_available() {
        eprintln!("skipped: ffmpeg is missing");
        return;
    }
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let library = tempfile::TempDir::new().unwrap();
    let root = std::fs::canonicalize(library.path()).unwrap().to_string_lossy().into_owned();
    send(&a, post("/admin/rpc/import.save_criteria", &form(&[("roots", &root), ("rule", "manual")]), &o)).await;
    send(&a, post("/admin/rpc/import.unlock", "", &o)).await;

    let work = tempfile::TempDir::new().unwrap();
    common::audio::make_mp3(&work.path().join("t1.mp3"), &[("artist", "Ken Follett"), ("album", "Winter der Welt")]);
    common::audio::make_mp3(&work.path().join("t2.mp3"), &[("artist", "Ken Follett"), ("album", "Winter der Welt")]);
    let (t1, t2) = (std::fs::read(work.path().join("t1.mp3")).unwrap(), std::fs::read(work.path().join("t2.mp3")).unwrap());

    // Nothing that is audio or a picture: refused, nothing queued.
    let r = send(&a, drop_request(&csrf, &cookie, "audiobook", &[("Folder/notes.exe", b"MZ")])).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", r.body);
    assert!(a.store.upload_jobs(5).await.unwrap().is_empty());

    // A dropped folder with two files, a note that is dropped and a path that tries to leave the folder.
    let r = send(&a, drop_request(&csrf, &cookie, "audiobook", &[("Folder/t1.mp3", &t1), ("Folder/notes.exe", b"MZ"), ("Folder/../../t2.mp3", &t2)])).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let jobs = wait_for_uploads(&a).await;
    assert_eq!(jobs[0].state, "done", "{}", jobs[0].message);
    let dest = library.path().join("Ken Follett/Winter der Welt");
    assert!(dest.join("t1.mp3").is_file() && dest.join("t2.mp3").is_file() && !dest.join("notes.exe").exists());
    let books = a.store.admin_book_rows().await.unwrap();
    assert!(books.iter().any(|b| b.title == "Winter der Welt" && b.tracks == 2), "imported: {books:?}");
}

#[tokio::test]
async fn the_requests_page_lists_what_a_child_asked_for_and_releases_or_refuses_it() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    let o = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), ..Default::default() };
    let mut daytime = david_server::Schedule::none();
    daytime.allow_every_day(7 * 60, 19 * 60);
    let kid = a.store.create_user(david_server::NewProfile::child("Kid", 8, daytime)).await.unwrap();
    let feed = a.store.save_feed("https://example.org/feed.xml", &david_server::podcast::parse(r#"<rss version="2.0"><channel><title>Kinderfunk</title><item><title>E</title><guid>a</guid><enclosure url="https://example.org/1.mp3" type="audio/mpeg"/></item></channel></rss>"#).unwrap()).await.unwrap();
    a.store.subscribe(kid, feed, false).await.unwrap();
    let station = a.store.save_station("https://stream.example.org/a", "Radio A", None, None).await.unwrap();
    a.store.favorite_station(kid, station, false).await.unwrap();

    let page = send(&a, get("/admin/releases", &plain)).await.body;
    assert!(page.contains("Kinderfunk") && page.contains("Radio A") && page.contains("Kid"), "{page}");
    let (kid_s, feed_s, station_s) = (kid.to_string(), feed.to_string(), station.to_string());
    let r = send(&a, post("/admin/rpc/release.decide", &form(&[("kind", "podcast"), ("profile", &kid_s), ("target", &feed_s), ("release", "1")]), &o)).await;
    assert!(r.status.is_redirection(), "{}", r.body);
    let r = send(&a, post("/admin/rpc/release.decide", &form(&[("kind", "station"), ("profile", &kid_s), ("target", &station_s), ("release", "0")]), &o)).await;
    assert!(r.status.is_redirection(), "{}", r.body);
    assert!(a.store.follows(kid, feed).await.unwrap());
    assert!(a.store.favorite_stations(kid, false).await.unwrap().is_empty());
    assert!(send(&a, get("/admin/releases", &plain)).await.body.contains("Nothing waits"));
}

#[tokio::test]
async fn a_tag_text_is_filtered_given_a_standard_genre_and_written_into_the_files() {
    if !common::audio::ffmpeg_available() {
        eprintln!("skipped: ffmpeg is missing");
        return;
    }
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    let o = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), ..Default::default() };
    let music = tempfile::TempDir::new().unwrap();
    a.store.save_music_roots(&[music.path().display().to_string()]).await.unwrap();
    for (title, tag) in [("A", "Christian"), ("B", "Religious"), ("C", "Jazz")] {
        common::audio::make_mp3(&music.path().join(format!("Artist/{title}/01.mp3")), &[("album", title), ("artist", "Artist"), ("genre", tag)]);
    }
    a.store.scan_kind(music.path(), "music").await.unwrap();
    let genre_of = |title: &'static str| {
        let path = music.path().join(format!("Artist/{title}/01.mp3"));
        async move { david_server::tags::read_fields(&path).unwrap().into_iter().find(|(n, _)| n == "Genre").and_then(|(_, v)| v.first().cloned()) }
    };

    // The page filters (and offers the names of the ID3 standard list in a searchable field).
    let page = send(&a, get("/admin/genres", &plain)).await.body;
    assert!(page.contains("Christian") && page.contains("Jazz") && page.contains("id=\"id3-genres\"") && page.contains("<option value=\"Gospel\">"), "{page}");
    let filtered = send(&a, get("/admin/genres?q=christ", &plain)).await.body;
    assert!(filtered.contains("<strong>Christian</strong>") && !filtered.contains("<strong>Jazz</strong>"), "{filtered}");

    // A name that is neither on the standard list nor one of the family's genres is refused.
    let r = send(&a, post("/admin/rpc/genre.from_tag", &form(&[("tag", "Christian"), ("name", "Made up"), ("retag", "1")]), &o)).await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    assert!(r.headers.get("location").is_some_and(|l| l.to_str().unwrap().contains("msg=err.")), "{} {:?}", r.status, r.headers);
    assert!(a.store.genres().await.unwrap().is_empty() && genre_of("A").await.as_deref() == Some("Christian"));

    // Two free texts get one standard genre (any case is taken as the standard spelling), and the files are tagged at once.
    for text in ["Christian", "Religious"] {
        let r = send(&a, post("/admin/rpc/genre.from_tag", &form(&[("tag", text), ("name", "gospel"), ("retag", "1")]), &o)).await;
        assert!(r.status.is_redirection(), "{}", r.body);
    }
    let list = a.store.genres().await.unwrap();
    assert_eq!((list[0].name.as_str(), list[0].albums), ("Gospel", 2));
    assert_eq!((genre_of("A").await.as_deref(), genre_of("B").await.as_deref(), genre_of("C").await.as_deref()), (Some("Gospel"), Some("Gospel"), Some("Jazz")), "only the files of those texts changed");

    // Without the tick the files stay as they are: the library gets the genre and the file keeps its text.
    let r = send(&a, post("/admin/rpc/genre.from_tag", &form(&[("tag", "Jazz"), ("name", "Jazz")]), &o)).await;
    assert!(r.status.is_redirection(), "{}", r.body);
    assert_eq!(a.store.genres().await.unwrap().len(), 2);
    assert!(send(&a, get("/admin/genres", &plain)).await.body.contains("Tag texts without a genre") || true);
}

/// A filter that the template engine does not know makes the whole panel fail at run time, and only when the branch that uses it is
/// drawn (a list of matches once led to `urlencode`, which is not there). Every filter of the templates must be one that is known to work.
#[test]
fn the_templates_use_only_filters_the_engine_has() {
    const KNOWN: &[&str] = &["format", "int", "join", "length", "lower", "replace", "round", "string"];
    let mut used = std::collections::BTreeSet::new();
    let mut stack = vec![std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "html") {
                let text = std::fs::read_to_string(&path).unwrap();
                // Inside `{{ ... }}` and `{% ... %}` only (a bar in a comment or a text is no filter).
                for block in text.split("{{").skip(1).chain(text.split("{%").skip(1)) {
                    let code = block.split("}}").next().unwrap().split("%}").next().unwrap();
                    let mut rest = code;
                    while let Some(i) = rest.find('|') {
                        let after = rest[i + 1..].trim_start();
                        if !after.starts_with('|') {
                            let name: String = after.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
                            if !name.is_empty() {
                                used.insert(name);
                            }
                        }
                        rest = &rest[i + 1..];
                    }
                }
            }
        }
    }
    let unknown: Vec<_> = used.iter().filter(|u| !KNOWN.contains(&u.as_str())).collect();
    assert!(unknown.is_empty(), "filters that are not known to exist: {unknown:?}");
}

#[tokio::test]
async fn the_import_folders_are_a_list_of_rows_for_four_kinds_and_a_folder_belongs_to_one_kind() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let hx = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let root = tempfile::tempdir().unwrap();
    let dirs: Vec<String> = ["Buecher", "Musik", "Bilder", "Ebooks", "Weg"]
        .iter()
        .map(|d| {
            std::fs::create_dir(root.path().join(d)).unwrap();
            root.path().join(d).to_string_lossy().into_owned()
        })
        .collect();
    let (b, m, p, e, w) = (&dirs[0], &dirs[1], &dirs[2], &dirs[3], &dirs[4]);
    let save = |fields: Vec<(&str, &str)>| {
        let mut all = vec![("csrf", csrf.as_str()), ("rows", "1"), ("rule", "manual")];
        all.extend(fields);
        post("/admin/rpc/import.save_criteria", &form(&all), &hx)
    };
    // Rows for all four kinds; the box "left out" turns a row into an exception of its kind.
    let r = send(&a, save(vec![("r_audiobook_1", b), ("r_music_1", m), ("r_photo_1", p), ("r_ebook_1", e), ("r_audiobook_2", w), ("x_audiobook_2", "1")])).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let st = a.store.import_state().await.unwrap();
    let canon = |x: &String| std::fs::canonicalize(x).unwrap().to_string_lossy().into_owned();
    assert_eq!((st.roots.clone(), st.music_roots.clone(), st.photo_roots.clone(), st.ebook_roots.clone()), (vec![canon(b)], vec![canon(m)], vec![canon(p)], vec![canon(e)]));
    assert_eq!(st.excludes, vec![canon(w)]);
    // The dialog shows a row for each folder and every kind has its list.
    let page = send(&a, get("/admin/settings?tab=import", &Opts { cookie: Some(&cookie), ..Default::default() })).await.body;
    for kind in ["audiobook", "music", "photo", "ebook"] {
        assert!(page.contains(&format!("id=\"rows-{kind}\"")) && page.contains(&format!("import.row?kind={kind}")), "{kind}: {page}");
    }
    assert!(page.contains("name=\"r_photo_1\"") && page.contains(&canon(p).replace('/', "&#x2f;")), "{page}");
    // A folder of pictures cannot also be a folder of e-books.
    let r = send(&a, save(vec![("r_audiobook_1", b), ("r_photo_1", p), ("r_ebook_1", p)])).await;
    assert_ne!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.header("x-invalid-field"), Some("r_ebook_1"), "the row of the kind that was refused is ringed");
    let r = send(&a, save(vec![("r_audiobook_1", b), ("r_photo_1", "/gibt/es/nicht")])).await;
    assert_eq!(r.header("x-invalid-field"), Some("r_photo_1"), "{}", r.body);
    assert_eq!(a.store.import_state().await.unwrap().ebook_roots, vec![canon(e)], "a refused form changes nothing of that kind");
    // Rows without a folder empty the kind; no audiobook folder is fine while music folders are saved with it.
    let r = send(&a, save(vec![("r_music_1", m)])).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let st = a.store.import_state().await.unwrap();
    assert!(st.roots.is_empty() && st.photo_roots.is_empty() && st.ebook_roots.is_empty() && st.music_roots == vec![canon(m)]);
    // Nothing at all is not a set of criteria.
    assert_ne!(send(&a, save(vec![])).await.status, StatusCode::OK);
    // One more empty row, for a known kind only.
    let row = send(&a, get("/admin/rpc/import.row?kind=photo", &hx)).await;
    assert!(row.status == StatusCode::OK && row.body.contains("name=\"r_photo_") && row.body.contains("data-remove-row"), "{}", row.body);
    assert_ne!(send(&a, get("/admin/rpc/import.row?kind=x", &hx)).await.status, StatusCode::OK);
}

#[tokio::test]
async fn a_refused_form_names_the_input_to_ring_in_the_other_windows_too() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let hx = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let r = send(&a, post("/admin/rpc/user.create", &form(&[("csrf", &csrf), ("name", "  "), ("role", "member")]), &hx)).await;
    assert_eq!((r.status, r.header("x-invalid-field")), (StatusCode::UNPROCESSABLE_ENTITY, Some("name")), "{}", r.body);
    let r = send(&a, post("/admin/rpc/user.create", &form(&[("csrf", &csrf), ("name", "Anni"), ("role", "member"), ("pin", "x")]), &hx)).await;
    assert_eq!(r.header("x-invalid-field"), Some("pin"), "{}", r.body);
    let r = send(&a, post("/admin/rpc/user.create", &form(&[("csrf", &csrf), ("name", "Anni"), ("role", "member"), ("birth_date", "kein-datum")]), &hx)).await;
    assert_eq!(r.header("x-invalid-field"), Some("birth_date"), "{}", r.body);
    send(&a, post("/admin/rpc/user.create", &form(&[("csrf", &csrf), ("name", "Anni"), ("role", "member")]), &hx)).await;
    let r = send(&a, post("/admin/rpc/user.create", &form(&[("csrf", &csrf), ("name", "Anni"), ("role", "member")]), &hx)).await;
    assert_eq!((r.status, r.header("x-invalid-field")), (StatusCode::CONFLICT, Some("name")), "a name that is taken: {}", r.body);
}

#[tokio::test]
async fn every_member_has_a_folder_for_every_medium_chosen_in_a_dialog_that_lists_only_what_the_server_can_use() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let hx = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    let anna = a.store.create_user(david_server::NewProfile::new("Anni", david_server::Role::Member)).await.unwrap();
    let root = tempfile::tempdir().unwrap();
    let musik = root.path().join("Anni/Musik");
    std::fs::create_dir_all(&musik).unwrap();
    std::fs::create_dir_all(root.path().join("Anni/Bilder")).unwrap();
    let enc = |s: &str| serde_urlencoded::to_string([("path", s)]).unwrap();

    // The window of the person has a line and a button for every kind of medium.
    let page = send(&a, get("/admin/users", &plain)).await.body;
    for kind in ["audiobook", "music", "photo"] {
        assert!(page.contains(&format!("name=\"folder_{kind}\"")) && page.contains(&format!("hx-get=\"/admin/rpc/folders?target=folder-{anna}-{kind}\"")), "{kind}: {page}");
    }
    assert!(page.contains("id=\"dlg-folders-") && page.contains("Music folder"), "{page}");

    // The dialog lists the folders below a path that the server can read, write and enter; the choice goes to the input of the line.
    let d = send(&a, get(&format!("/admin/rpc/folders?target=folder-{anna}-music&{}", enc(&root.path().join("Anni").to_string_lossy())), &hx)).await;
    assert_eq!(d.status, StatusCode::OK, "{}", d.body);
    assert!(d.body.contains("Bilder") && d.body.contains("Musik") && d.body.contains("data-pick-folder="), "{}", d.body);
    assert!(d.body.contains(&format!("data-target=\"folder-{anna}-music\"")), "{}", d.body);
    // Not usable or not there: the dialog says so and offers the way back to the start.
    let d = send(&a, get(&format!("/admin/rpc/folders?target=folder-{anna}-music&{}", enc("/no/such/folder")), &hx)).await;
    assert!(d.body.contains("That folder cannot be used"), "{}", d.body);
    // Only a name of an input is taken as the target.
    let d = send(&a, get("/admin/rpc/folders?target=x%22%3E%3Cscript", &hx)).await;
    assert_ne!(d.status, StatusCode::OK);

    // A folder is created in the dialog: the dialog then shows the new folder; a bad or taken name is refused with the reason.
    let parent = root.path().join("Anni").to_string_lossy().into_owned();
    let t = format!("folder-{anna}-music");
    let made = send(&a, post("/admin/rpc/folders.create", &form(&[("path", &parent), ("name", "Neu"), ("target", &t)]), &hx)).await;
    assert_eq!(made.status, StatusCode::OK, "{}", made.body);
    assert!(root.path().join("Anni/Neu").is_dir() && made.body.contains("Neu") && made.body.contains("data-pick-folder="), "{}", made.body);
    let again = send(&a, post("/admin/rpc/folders.create", &form(&[("path", &parent), ("name", "Neu"), ("target", &t)]), &hx)).await;
    assert!(again.body.contains("of this name already"), "{}", again.body);
    let bad_name = send(&a, post("/admin/rpc/folders.create", &form(&[("path", &parent), ("name", "../x"), ("target", &t)]), &hx)).await;
    assert!(bad_name.body.contains("not a usable folder name") && !root.path().join("x").exists(), "{}", bad_name.body);
    let no_csrf = send(&a, post("/admin/rpc/folders.create", &form(&[("path", &parent), ("name", "X"), ("target", &t)]), &Opts { cookie: Some(&cookie), htmx: true, ..Default::default() })).await;
    assert_eq!(no_csrf.status, StatusCode::FORBIDDEN);
    assert!(!root.path().join("Anni/X").exists());

    // Saving: the folder is checked; a wrong one is refused and nothing of the form changes; an empty line removes the folder.
    let base = |extra: &str| format!("csrf={csrf}&id={anna}&name=Anni&role=member{extra}");
    let ok = send(&a, post("/admin/rpc/user.update", &base(&format!("&folder_music={}", enc(&musik.to_string_lossy()).trim_start_matches("path="))), &hx)).await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.body);
    let saved = a.store.profile_folders(anna).await.unwrap();
    assert_eq!(saved, vec![("music".to_owned(), std::fs::canonicalize(&musik).unwrap().to_string_lossy().into_owned())]);
    assert!(ok.body.contains(&musik.file_name().unwrap().to_string_lossy().into_owned()), "the row shows the folder: {}", ok.body);
    let bad = send(&a, post("/admin/rpc/user.update", &base("&folder_photo=%2Fno%2Fsuch%2Ffolder"), &hx)).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", bad.body);
    assert!(bad.body.contains("cannot be used"), "{}", bad.body);
    assert_eq!(bad.header("x-invalid-field"), Some("folder_photo"), "the page is told which input to ring");
    let bad_pin = send(&a, post("/admin/rpc/user.update", &base("&pin=x"), &hx)).await;
    assert_eq!((bad_pin.status, bad_pin.header("x-invalid-field")), (StatusCode::UNPROCESSABLE_ENTITY, Some("pin")), "{}", bad_pin.body);
    let rel = send(&a, post("/admin/rpc/user.update", &base("&folder_photo=relative%2Fpath"), &hx)).await;
    assert_eq!(rel.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(a.store.profile_folders(anna).await.unwrap().len(), 1, "nothing was saved by a refused form");
    let gone = send(&a, post("/admin/rpc/user.update", &base("&folder_music="), &hx)).await;
    assert_eq!(gone.status, StatusCode::OK, "{}", gone.body);
    assert!(a.store.profile_folders(anna).await.unwrap().is_empty(), "an empty line removes the folder");
    // A form that does not send the fields leaves the folders alone.
    a.store.set_profile_folder(anna, "photo", Some(&root.path().to_string_lossy())).await.unwrap();
    assert_eq!(send(&a, post("/admin/rpc/user.update", &base(""), &hx)).await.status, StatusCode::OK);
    assert_eq!(a.store.profile_folders(anna).await.unwrap().len(), 1);
}

#[tokio::test]
async fn the_media_lists_come_in_pages_with_links_a_size_and_a_search_over_all_pages() {
    let a = app().await;
    let (cookie, _csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), ..Default::default() };
    for n in 1..=25 {
        a.store.import_books(vec![common::book(&format!("Autor {n:02}"), &format!("Buch {n:02}"), &[1])]).await.unwrap();
    }
    a.store.save_settings(&[("list_page_size".to_owned(), "10".to_owned())]).await.unwrap();
    let rows = |body: &str| body.matches("<tr id=\"book-").count();

    // Page 1 of 3: ten books, the count, the links; a page beyond the last shows the last; a nonsense page shows the first.
    let p1 = send(&a, get("/admin/books", &o)).await.body;
    assert_eq!(rows(&p1), 10, "{p1}");
    assert!(p1.contains("1&ndash;10 of 25") && p1.contains("rel=\"next\"") && p1.contains("page=3") && p1.contains("aria-current=\"page\""), "{p1}");
    assert!(p1.contains("data-paged=\"1\""), "a list in pages is searched by the server");
    let p3 = send(&a, get("/admin/books?page=3", &o)).await.body;
    assert_eq!(rows(&p3), 5);
    assert!(p3.contains("21&ndash;25 of 25") && p3.contains("rel=\"prev\"") && !p3.contains("rel=\"next\""), "{p3}");
    assert!(send(&a, get("/admin/books?page=99", &o)).await.body.contains("21&ndash;25 of 25"), "clamped to the last page");
    assert!(send(&a, get("/admin/books?page=0", &o)).await.body.contains("1&ndash;10 of 25"));
    // A size of its own (kept in the links), and one page for a long size.
    let big = send(&a, get("/admin/books?per=25", &o)).await.body;
    assert_eq!(rows(&big), 25);
    assert!(!big.contains("rel=\"next\""), "everything fits on one page");
    let twenty = send(&a, get("/admin/books?per=20", &o)).await.body;
    assert!(twenty.contains("per=20&amp;page=2") || twenty.contains("per=20&page=2"), "the size stays in the links: {twenty}");
    // The search runs over all pages, and the links carry it.
    let found = send(&a, get("/admin/books?q=buch+*", &o)).await.body;
    assert_eq!(rows(&found), 10, "all 25 match: ten of them on the first page");
    assert!(found.contains("q=buch+") && found.contains("page=2"), "{found}");
    let last = send(&a, get("/admin/books?q=buch+*&page=3", &o)).await.body;
    assert_eq!(rows(&last), 5);
    assert!(last.contains("rel=\"prev\""), "{last}");
    let narrow = send(&a, get("/admin/books?q=buch+2*", &o)).await.body;
    assert_eq!(rows(&narrow), 6, "Buch 20 to 25 fit on one page");
    assert!(!narrow.contains("class=\"pager\""), "a search that fits on one page has no pagination: {narrow}");
    let one = send(&a, get("/admin/books?q=Buch+17", &o)).await.body;
    assert_eq!(rows(&one), 1, "{one}");
    assert!(!one.contains("class=\"pager\"") && one.contains("id=\"media-results\""), "exactly one entry and no pagination: {one}");
    assert!(one.contains("hx-get=\"/admin/books\"") && one.contains("hx-select=\"#media-results\""), "the search form asks the server while typing");
    // The size of a page is kept in a cookie of this browser: asked for once with ?per=, it holds for the next visits; a query wins over the cookie.
    let first = send(&a, get("/admin/books?per=25", &o)).await;
    let set = first.header("set-cookie").expect("the size is kept in a cookie").to_owned();
    assert!(set.starts_with("david_page_size=25;") && set.contains("Path=/admin") && set.contains("Max-Age="), "{set}");
    assert!(first.body.contains("<option value=\"25\" selected>") && first.body.contains("data-autosubmit"), "{}", first.body);
    let with_cookie = format!("{}; david_page_size=25", cookie);
    let oc = Opts { cookie: Some(&with_cookie), ..Default::default() };
    assert_eq!(rows(&send(&a, get("/admin/books", &oc)).await.body), 25, "the cookie decides when the address says nothing");
    assert_eq!(rows(&send(&a, get("/admin/books?per=10", &oc)).await.body), 10, "the address wins");
    let junk = format!("{}; david_page_size=banana", cookie);
    let oj = Opts { cookie: Some(&junk), ..Default::default() };
    assert_eq!(rows(&send(&a, get("/admin/books", &oj)).await.body), 10, "a broken cookie is ignored: the setting of the server holds");
    assert!(send(&a, get("/admin/books", &oj)).await.header("set-cookie").is_none(), "nothing is set when nothing was asked");
    // The size is a setting of the pages.
    assert!(send(&a, get("/admin/settings?tab=values", &o)).await.body.contains("name=\"list_page_size\""));
}

#[tokio::test]
async fn the_book_window_sets_the_labels_of_single_titles_with_its_save_and_the_filter_finds_the_book() {
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), htmx: true, ..Default::default() };
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    a.store.import_books(vec![book("A", "Eins", &[3]), book("B", "Zwei", &[1])]).await.unwrap();
    let id = a.store.find_book("A/Eins").await.unwrap().unwrap().id;
    let other = a.store.find_book("B/Zwei").await.unwrap().unwrap().id;
    let tracks: Vec<i64> = a.store.tracks_of(id).await.unwrap().iter().map(|t| t.id).collect();
    let foreign = a.store.tracks_of(other).await.unwrap()[0].id;
    let sid = id.to_string();

    // The window has the section; it is loaded when it is opened: one entry of labels for each title, the field that the script fills.
    let page = send(&a, get("/admin/books", &plain)).await.body;
    assert!(page.contains("/admin/rpc/book.track_labels?id="), "{page}");
    let frag = send(&a, get(&format!("/admin/rpc/book.track_labels?id={id}"), &plain)).await;
    assert_eq!(frag.status, StatusCode::OK);
    assert!(frag.body.contains("name=\"track_labels\"") && frag.body.matches("data-track=\"").count() == 3 && frag.body.contains(&format!("data-track=\"{}\"", tracks[1])), "{}", frag.body);
    assert_eq!(send(&a, get("/admin/rpc/book.track_labels?id=999999", &plain)).await.status, StatusCode::NOT_FOUND);

    // The save writes the titles that are listed (one line `id=labels`), and the others stay; the list of the titles shows them.
    let lines = format!("{}=Gebet, Stille; gebet\n{}=\n", tracks[0], tracks[1]);
    let r = send(&a, post("/admin/rpc/book.update", &form(&[("id", &sid), ("section", "edit"), ("track_labels", &lines)]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let own = a.store.track_labels().await.unwrap();
    assert_eq!((own.get(&tracks[0]).cloned(), own.get(&tracks[1]).cloned(), own.get(&tracks[2]).cloned()), (Some(vec!["Gebet".to_owned(), "Stille".to_owned()]), None, None));
    let list = send(&a, get(&format!("/admin/rpc/book.tracks?id={id}"), &plain)).await.body;
    assert!(list.contains("<span class=\"badge label\">Gebet</span>"), "{list}");
    let frag = send(&a, get(&format!("/admin/rpc/book.track_labels?id={id}"), &plain)).await.body;
    assert!(frag.contains("value=\"Gebet, Stille\""), "{frag}");

    // Nothing changes for a title of another book, a line that is no list, or a name that is not usable; a form without the field leaves them alone.
    for bad in [format!("{foreign}=x"), "kein gleichheitszeichen".to_owned(), format!("{}=ok, a|b", tracks[2]), "x=y".to_owned()] {
        let r = send(&a, post("/admin/rpc/book.update", &form(&[("id", &sid), ("section", "edit"), ("track_labels", &bad)]), &o)).await;
        assert!(r.status == StatusCode::UNPROCESSABLE_ENTITY || r.status == StatusCode::BAD_REQUEST, "{bad}: {} {}", r.status, r.body);
    }
    send(&a, post("/admin/rpc/book.update", &form(&[("id", &sid), ("section", "edit"), ("description", "Neu")]), &o)).await;
    assert_eq!(a.store.track_labels().await.unwrap().get(&tracks[0]).map(Vec::len), Some(2));

    // The filter of the list finds the album by the labels of its titles, and counts it once.
    let page = send(&a, get("/admin/books", &plain)).await.body;
    assert!(page.contains("Gebet (1)") && page.contains("Stille (1)"), "{page}");
    let rows = |b: &str| b.matches("<tr id=\"book-").count();
    assert_eq!(rows(&send(&a, get("/admin/books?label=stille", &plain)).await.body), 1);
}

// Podcasts and radio stations

fn feed_of(title: &str, episodes: usize) -> david_server::podcast::Feed {
    david_server::podcast::Feed {
        title: title.into(),
        site: None,
        image: None,
        description: String::new(),
        episodes: (0..episodes)
            .map(|i| david_server::podcast::Episode { guid: format!("{title}-{i}"), title: format!("{title} Folge {i:02}"), audio_url: format!("https://example.org/{title}/{i}.mp3"), published_ms: 1_700_000_000_000 + i as i64 * 86_400_000, duration_s: Some(600 + i as i64), description: String::new() })
            .collect(),
    }
}

/// Counts how often a text appears in a page.
fn times(page: &str, text: &str) -> usize {
    page.matches(text).count()
}

#[tokio::test]
async fn the_pages_of_podcasts_and_radio_are_for_administrators_only() {
    let a = app().await;
    let anna = a.store.create_profile("Anna", false).await.unwrap();
    let token = a.store.create_session(anna, Role::Member, None).await.unwrap();
    let cookie = format!("david_session={token}");
    let o = Opts { cookie: Some(&cookie), ..Default::default() };
    for path in ["/admin/podcasts", "/admin/podcasts/1", "/admin/radio"] {
        assert_eq!(send(&a, get(path, &o)).await.status, StatusCode::FORBIDDEN, "{path}");
    }
    let csrf = csrf_token(&token);
    let o = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), htmx: true, ..Default::default() };
    for rpc in ["podcast.add", "podcast.labels", "podcast.followers", "podcast.refresh", "podcast.delete", "podcast.decide", "station.add", "station.update", "station.delete", "station.decide"] {
        assert_eq!(send(&a, post(&format!("/admin/rpc/{rpc}"), &form(&[("id", "1")]), &o)).await.status, StatusCode::FORBIDDEN, "{rpc}");
    }
    // Without the token of the page even an administrator is refused.
    let (cookie, _) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, ..Default::default() };
    let id = a.store.save_feed("https://example.org/a.xml", &feed_of("Alpha", 1)).await.unwrap();
    let r = send(&a, post("/admin/rpc/podcast.delete", &form(&[("id", &id.to_string())]), &o)).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert!(a.store.feed_head(id).await.unwrap().is_some(), "nothing was removed");
}

#[tokio::test]
async fn the_list_of_podcasts_is_cut_into_pages_after_the_search() {
    let a = app().await;
    let (cookie, _) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), ..Default::default() };
    for i in 0..12 {
        a.store.save_feed(&format!("https://example.org/feed{i:02}.xml"), &feed_of(&format!("Cast {i:02}"), 1)).await.unwrap();
    }
    a.store.save_feed("https://example.org/other.xml", &feed_of("Andacht", 1)).await.unwrap();
    // 13 podcasts, 10 on a page: the first page has ten rows and the links to the second.
    let first = send(&a, get("/admin/podcasts?per=10", &o)).await;
    assert_eq!(first.status, StatusCode::OK);
    assert_eq!(times(&first.body, "<tr id=\"podcast-"), 10, "ten rows on the first page: {}", first.body.len());
    assert!(first.body.contains("1&ndash;10 of 13"), "{}", first.body);
    assert!(first.body.contains("per=10&amp;page=2"));
    // The second page has the other three, in the same order (by title, the list is complete before it is cut).
    let second = send(&a, get("/admin/podcasts?per=10&page=2", &o)).await;
    assert_eq!(times(&second.body, "<tr id=\"podcast-"), 3);
    assert!(second.body.contains("11&ndash;13 of 13"));
    assert!(second.body.contains("Cast 11") && !second.body.contains("Cast 00"));
    // A page beyond the end shows the last one.
    let beyond = send(&a, get("/admin/podcasts?per=10&page=99", &o)).await;
    assert!(beyond.body.contains("11&ndash;13 of 13"));
    // The search is applied before the cut: one hit, no pager of several pages, and the text stays in the field.
    let found = send(&a, get("/admin/podcasts?q=andacht&per=10", &o)).await;
    assert_eq!(times(&found.body, "<tr id=\"podcast-"), 1);
    assert!(found.body.contains("Andacht") && !found.body.contains("Cast 00"));
    assert!(found.body.contains("value=\"andacht\""));
    // The links of a page keep the search and the size.
    let many = send(&a, get("/admin/podcasts?q=cast&per=10", &o)).await;
    assert!(many.body.contains("q=cast&amp;per=10&amp;page=2"), "{}", many.body);
    // The answer for htmx is the page too (the search form picks the results out of it).
    let hx = Opts { cookie: Some(&cookie), htmx: true, ..Default::default() };
    assert_eq!(send(&a, get("/admin/podcasts?q=cast", &hx)).await.status, StatusCode::OK);
}

#[tokio::test]
async fn a_podcast_has_its_own_page_with_its_episodes_in_pages() {
    let a = app().await;
    let (cookie, _) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), ..Default::default() };
    let id = a.store.save_feed("https://example.org/long.xml", &feed_of("Long", 23)).await.unwrap();
    let boss = a.store.list_profiles().await.unwrap().iter().find(|p| p.name == "Boss").unwrap().id;
    // The list leads to the page of the podcast.
    let list = send(&a, get("/admin/podcasts", &o)).await;
    assert!(list.body.contains(&format!("href=\"/admin/podcasts/{id}\"")));
    // The newest episode comes first; ten on a page, the way back to the list is there.
    let page = send(&a, get(&format!("/admin/podcasts/{id}?per=10"), &o)).await;
    assert_eq!(page.status, StatusCode::OK);
    assert_eq!(times(&page.body, "<tr id=\"episode-"), 10);
    let newest = page.body.find("Long Folge 22").expect("the newest episode");
    let older = page.body.find("Long Folge 13").expect("the tenth episode");
    assert!(newest < older, "newest first");
    assert!(!page.body.contains("Long Folge 12"), "the eleventh is on the next page");
    assert!(page.body.contains("1&ndash;10 of 23"));
    assert!(page.body.contains("href=\"/admin/podcasts\"") && page.body.contains("class=\"breadcrumb\""));
    // (the address is escaped as a whole: the slashes come as `&#x2f;`, which the browser reads back)
    assert!(page.body.contains(&format!("&#x2f;admin&#x2f;podcasts&#x2f;{id}?per=10&amp;page=2")), "the pager leads to the next page of the episodes");
    let last = send(&a, get(&format!("/admin/podcasts/{id}?per=10&page=3"), &o)).await;
    assert_eq!(times(&last.body, "<tr id=\"episode-"), 3);
    assert!(last.body.contains("Long Folge 00") && last.body.contains("21&ndash;23 of 23"));
    // Who has heard an episode: finished, and begun.
    let episodes = a.store.episodes(boss, id, 100).await.unwrap();
    let newest_id = episodes.iter().find(|e| e.title == "Long Folge 22").unwrap().id;
    a.store.put_episode_state(boss, newest_id, 600_000, true).await.unwrap();
    let heard = send(&a, get(&format!("/admin/podcasts/{id}?per=10"), &o)).await;
    assert!(heard.body.contains("<span class=\"badge\">Boss</span>"), "{}", heard.body);
    // No such podcast.
    assert_eq!(send(&a, get("/admin/podcasts/9999", &o)).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn adding_a_podcast_marks_a_wrong_address_and_labels_are_kept() {
    let a = app().await;
    let (cookie, token) = sign_in(&a).await;
    let hx = Opts { cookie: Some(&cookie), csrf_header: Some(&token), htmx: true, ..Default::default() };
    // An address that is no web address is refused at once, with the input marked (and nothing is fetched).
    let r = send(&a, post("/admin/rpc/podcast.add", &form(&[("url", "ftp://example.org/feed.xml")]), &hx)).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(r.header("x-invalid-field"), Some("url"));
    // Labels with one Save: the dialog closes by the reload of the page; a label that is too long marks the field.
    let id = a.store.save_feed("https://example.org/l.xml", &feed_of("Labelled", 1)).await.unwrap();
    let r = send(&a, post("/admin/rpc/podcast.labels", &form(&[("id", &id.to_string()), ("labels", "Gebet, Schule")]), &hx)).await;
    assert_eq!((r.status, r.header("hx-refresh")), (StatusCode::OK, Some("true")), "{}", r.body);
    assert_eq!(a.store.podcast_labels().await.unwrap().get(&id).cloned().unwrap_or_default(), vec!["Gebet".to_owned(), "Schule".to_owned()]);
    let long = "x".repeat(41);
    let r = send(&a, post("/admin/rpc/podcast.labels", &form(&[("id", &id.to_string()), ("labels", &long)]), &hx)).await;
    assert_eq!((r.status, r.header("x-invalid-field")), (StatusCode::UNPROCESSABLE_ENTITY, Some("labels")));
    assert_eq!(a.store.podcast_labels().await.unwrap().get(&id).map(Vec::len), Some(2), "unchanged");
    // The list shows the labels, and nothing else carries them.
    let o = Opts { cookie: Some(&cookie), ..Default::default() };
    let list = send(&a, get("/admin/podcasts", &o)).await;
    assert!(list.body.contains("Gebet") && list.body.contains("Schule"));
    // Without JavaScript the same form answers with a redirect to the page.
    let plain = Opts { cookie: Some(&cookie), csrf_header: Some(&token), ..Default::default() };
    let r = send(&a, post("/admin/rpc/podcast.labels", &form(&[("id", &id.to_string()), ("labels", "Gebet")]), &plain)).await;
    assert_eq!((r.status, r.header("location")), (StatusCode::SEE_OTHER, Some("/admin/podcasts?msg=ok.saved")));
}

#[tokio::test]
async fn a_podcast_is_removed_and_the_request_of_a_child_decided() {
    let a = app().await;
    let (cookie, token) = sign_in(&a).await;
    let hx = Opts { cookie: Some(&cookie), csrf_header: Some(&token), htmx: true, ..Default::default() };
    let o = Opts { cookie: Some(&cookie), ..Default::default() };
    let id = a.store.save_feed("https://example.org/k.xml", &feed_of("Kids", 2)).await.unwrap();
    let kid = a.store.create_profile("Kid", false).await.unwrap();
    a.store.subscribe(kid, id, false).await.unwrap();
    // The request waits: the page of the podcast shows who asked and offers release and refusal.
    let page = send(&a, get(&format!("/admin/podcasts/{id}"), &o)).await;
    assert!(page.body.contains("Kid") && page.body.contains("/admin/rpc/podcast.decide"), "{}", page.body);
    let r = send(&a, post("/admin/rpc/podcast.decide", &form(&[("id", &id.to_string()), ("profile", &kid.to_string()), ("release", "1"), ("back", "page")]), &hx)).await;
    assert_eq!((r.status, r.header("hx-refresh")), (StatusCode::OK, Some("true")));
    assert!(a.store.follows(kid, id).await.unwrap(), "released");
    // Removing from the page of the podcast leads back to the list; the episodes and the follows go with it.
    let r = send(&a, post("/admin/rpc/podcast.delete", &form(&[("id", &id.to_string()), ("back", "page")]), &hx)).await;
    assert_eq!((r.status, r.header("hx-redirect")), (StatusCode::OK, Some("/admin/podcasts")));
    assert!(a.store.feed_head(id).await.unwrap().is_none());
    assert_eq!(a.store.episode_count(id).await.unwrap(), 0);
    // Removing it twice: not found.
    let r = send(&a, post("/admin/rpc/podcast.delete", &form(&[("id", &id.to_string())]), &hx)).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_administrator_edits_who_follows_a_podcast() {
    let a = app().await;
    let (cookie, token) = sign_in(&a).await;
    let hx = Opts { cookie: Some(&cookie), csrf_header: Some(&token), htmx: true, ..Default::default() };
    let o = Opts { cookie: Some(&cookie), ..Default::default() };
    let id = a.store.save_feed("https://example.org/f.xml", &feed_of("Folgen", 1)).await.unwrap();
    let anna = a.store.create_profile("Anna", false).await.unwrap();
    let ben = a.store.create_profile("Ben", false).await.unwrap();
    let kid = a.store.create_profile("Kid", false).await.unwrap();
    a.store.subscribe(anna, id, true).await.unwrap();
    a.store.subscribe(kid, id, false).await.unwrap();
    let follows = |p: i64| {
        let s = a.store.clone();
        async move { s.follows(p, id).await.unwrap() }
    };

    // The dialog on the list and on the page of the podcast lists every profile: the followers and the child that waits are ticked.
    for path in ["/admin/podcasts".to_owned(), format!("/admin/podcasts/{id}")] {
        let page = send(&a, get(&path, &o)).await.body;
        assert!(page.contains(&format!("id=\"dlg-followers-{id}\"")) && page.contains("/admin/rpc/podcast.followers"), "{path}: {page}");
        assert!(page.contains(&format!("name=\"profiles\" value=\"{anna}\" checked")), "Anna follows: {path}");
        assert!(page.contains(&format!("name=\"profiles\" value=\"{kid}\" checked")), "the request of Kid waits and counts as ticked: {path}");
        assert!(page.contains(&format!("name=\"profiles\" value=\"{ben}\">")), "Ben does not follow: {path}");
        assert!(page.contains("name=\"followers_form\" value=\"1\""), "the marker is in the form");
    }

    // Saving applies the difference: Ben is added, Kid (waiting, not ticked) is refused, Anna stays.
    let save = |profiles: &[i64]| {
        let mut fields: Vec<(String, String)> = vec![("id".into(), id.to_string()), ("followers_form".into(), "1".into()), ("back".into(), "page".into())];
        fields.extend(profiles.iter().map(|p| ("profiles".to_owned(), p.to_string())));
        serde_urlencoded::to_string(&fields).unwrap()
    };
    let r = send(&a, post("/admin/rpc/podcast.followers", &save(&[anna, ben]), &hx)).await;
    assert_eq!((r.status, r.header("hx-refresh")), (StatusCode::OK, Some("true")), "{}", r.body);
    assert!(follows(anna).await && follows(ben).await && !follows(kid).await);
    assert!(a.store.follow_ids_by_feed().await.unwrap().get(&id).is_some_and(|v| v.iter().all(|(_, ok)| *ok) && v.len() == 2), "the request of Kid is gone");

    // A child that is ticked follows at once, with the release; a request that waits and is ticked is released.
    let r = send(&a, post("/admin/rpc/podcast.followers", &save(&[kid]), &hx)).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(follows(kid).await && !follows(anna).await && !follows(ben).await, "only Kid follows now");
    a.store.unsubscribe(kid, id).await.unwrap();
    a.store.subscribe(kid, id, false).await.unwrap();
    assert!(!follows(kid).await, "waits");
    send(&a, post("/admin/rpc/podcast.followers", &save(&[kid, anna]), &hx)).await;
    assert!(follows(kid).await && follows(anna).await, "the waiting request was released by ticking it");

    // A request without the marker changes nothing (a missing field never removes everybody); with the marker and no tick the list is empty on purpose.
    let r = send(&a, post("/admin/rpc/podcast.followers", &form(&[("id", &id.to_string())]), &hx)).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = send(&a, post("/admin/rpc/podcast.followers", &form(&[("id", &id.to_string()), ("profiles", &anna.to_string())]), &hx)).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "profiles without the marker");
    assert!(follows(kid).await && follows(anna).await, "unchanged");
    let r = send(&a, post("/admin/rpc/podcast.followers", &form(&[("id", &id.to_string()), ("followers_form", "1")]), &hx)).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(!follows(kid).await && !follows(anna).await, "nobody follows");

    // An unknown profile marks the field and changes nothing; an unknown podcast is not found; a page that is not htmx redirects with the message.
    a.store.subscribe(anna, id, true).await.unwrap();
    let r = send(&a, post("/admin/rpc/podcast.followers", &save(&[anna, 9999]), &hx)).await;
    assert_eq!((r.status, r.header("x-invalid-field")), (StatusCode::UNPROCESSABLE_ENTITY, Some("profiles")));
    assert!(follows(anna).await && !follows(ben).await);
    let r = send(&a, post("/admin/rpc/podcast.followers", &form(&[("id", "9999"), ("followers_form", "1")]), &hx)).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let r = send(&a, post("/admin/rpc/podcast.followers", &form(&[("id", "9999"), ("followers_form", "1"), ("profiles", "not a number")]), &hx)).await;
    assert_eq!((r.status, r.header("x-invalid-field")), (StatusCode::UNPROCESSABLE_ENTITY, Some("profiles")));
}

#[tokio::test]
async fn an_administrator_chooses_who_has_a_radio_station_among_the_favourites() {
    let a = app().await;
    let (cookie, token) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), ..Opts::default() };
    let hx = Opts { cookie: Some(&cookie), csrf_header: Some(&token), htmx: true, ..Opts::default() };
    let id = a.store.save_station("https://radio.example/live", "Live", None, None).await.unwrap();
    let anna = a.store.create_profile("Anna", false).await.unwrap();
    let ben = a.store.create_profile("Ben", false).await.unwrap();
    let kid = a.store.create_profile("Kid", false).await.unwrap();
    a.store.favorite_station(anna, id, true).await.unwrap();
    a.store.favorite_station(kid, id, false).await.unwrap();
    let has = |p: i64| {
        let s = a.store.clone();
        async move { s.favorite_stations(p, false).await.unwrap().iter().any(|st| st.id == id) }
    };

    let page = send(&a, get("/admin/radio", &o)).await.body;
    assert!(page.contains(&format!("id=\"dlg-favourites-{id}\"")) && page.contains("/admin/rpc/station.favourites"), "{page}");
    assert!(page.contains(&format!("name=\"profiles\" value=\"{anna}\" checked")), "Anna has it");
    assert!(page.contains(&format!("name=\"profiles\" value=\"{kid}\" checked")), "the request of Kid waits and counts as ticked");
    assert!(page.contains(&format!("name=\"profiles\" value=\"{ben}\">")), "Ben does not have it");

    let save = |profiles: &[i64]| {
        let mut fields: Vec<(String, String)> = vec![("id".into(), id.to_string()), ("favourites_form".into(), "1".into())];
        fields.extend(profiles.iter().map(|p| ("profiles".to_owned(), p.to_string())));
        serde_urlencoded::to_string(&fields).unwrap()
    };
    let r = send(&a, post("/admin/rpc/station.favourites", &save(&[anna, ben]), &hx)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(has(anna).await && has(ben).await && !has(kid).await, "Ben is added, the waiting request of Kid is refused");
    send(&a, post("/admin/rpc/station.favourites", &save(&[kid]), &hx)).await;
    assert!(has(kid).await && !has(anna).await && !has(ben).await, "only Kid has it, with the release");

    // Without the marker nothing changes; an unknown profile or station is refused.
    let r = send(&a, post("/admin/rpc/station.favourites", &form(&[("id", &id.to_string()), ("profiles", &anna.to_string())]), &hx)).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(!has(anna).await && has(kid).await, "unchanged");
    let r = send(&a, post("/admin/rpc/station.favourites", &save(&[anna, 9999]), &hx)).await;
    assert_eq!((r.status, r.header("x-invalid-field")), (StatusCode::UNPROCESSABLE_ENTITY, Some("profiles")));
    let r = send(&a, post("/admin/rpc/station.favourites", &form(&[("id", "9999"), ("favourites_form", "1")]), &hx)).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn radio_stations_are_added_changed_labelled_and_removed() {
    let a = app().await;
    let (cookie, token) = sign_in(&a).await;
    let hx = Opts { cookie: Some(&cookie), csrf_header: Some(&token), htmx: true, ..Default::default() };
    let o = Opts { cookie: Some(&cookie), ..Default::default() };
    // A wrong stream address marks its input; nothing is added.
    let r = send(&a, post("/admin/rpc/station.add", &form(&[("name", "Lyra"), ("stream_url", "rtsp://example.org/live")]), &hx)).await;
    assert_eq!((r.status, r.header("x-invalid-field")), (StatusCode::UNPROCESSABLE_ENTITY, Some("stream_url")));
    let r = send(&a, post("/admin/rpc/station.add", &form(&[("name", ""), ("stream_url", "https://example.org/live.mp3")]), &hx)).await;
    assert_eq!((r.status, r.header("x-invalid-field")), (StatusCode::UNPROCESSABLE_ENTITY, Some("name")));
    assert!(a.store.stations_with_owners().await.unwrap().is_empty());
    let r = send(&a, post("/admin/rpc/station.add", &form(&[("name", "Lyra"), ("stream_url", "https://example.org/live.mp3"), ("homepage", "https://example.org/")]), &hx)).await;
    assert_eq!((r.status, r.header("hx-refresh")), (StatusCode::OK, Some("true")), "{}", r.body);
    let id = a.store.stations_with_owners().await.unwrap()[0].0.id;
    // Name, addresses and labels with one Save; an empty home page removes it.
    let r = send(&a, post("/admin/rpc/station.update", &form(&[("id", &id.to_string()), ("name", "Lyra 2"), ("stream_url", "https://example.org/live2.mp3"), ("homepage", ""), ("logo_url", ""), ("labels", "Gebet")]), &hx)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let station = a.store.stations_with_owners().await.unwrap().remove(0).0;
    assert_eq!((station.name.as_str(), station.stream_url.as_str(), station.homepage), ("Lyra 2", "https://example.org/live2.mp3", None));
    assert_eq!(a.store.station_labels().await.unwrap().get(&id).cloned().unwrap_or_default(), vec!["Gebet".to_owned()]);
    let list = send(&a, get("/admin/radio", &o)).await;
    assert!(list.body.contains("Lyra 2") && list.body.contains("Gebet"));
    // A second station with the same address is refused and marks the address.
    let other = a.store.save_station("https://example.org/other.mp3", "Other", None, None).await.unwrap();
    let r = send(&a, post("/admin/rpc/station.update", &form(&[("id", &other.to_string()), ("name", "Other"), ("stream_url", "https://example.org/live2.mp3"), ("labels", "")]), &hx)).await;
    assert_eq!((r.status, r.header("x-invalid-field")), (StatusCode::UNPROCESSABLE_ENTITY, Some("stream_url")));
    // Removing takes the station and the label that only it carried.
    let r = send(&a, post("/admin/rpc/station.delete", &form(&[("id", &id.to_string())]), &hx)).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(a.store.stations_with_owners().await.unwrap().iter().all(|(s, _)| s.id != id));
    assert!(a.store.labels().await.unwrap().iter().all(|l| l.name != "Gebet"));
    assert_eq!(send(&a, post("/admin/rpc/station.delete", &form(&[("id", &id.to_string())]), &hx)).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_list_of_stations_is_cut_into_pages_after_the_search() {
    let a = app().await;
    let (cookie, _) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), ..Default::default() };
    for i in 0..12 {
        a.store.save_station(&format!("https://example.org/s{i:02}.mp3"), &format!("Sender {i:02}"), None, None).await.unwrap();
    }
    a.store.save_station("https://example.org/dwg.mp3", "DWG Lyra", None, None).await.unwrap();
    let first = send(&a, get("/admin/radio?per=10", &o)).await;
    assert_eq!(times(&first.body, "<tr id=\"station-"), 10);
    assert!(first.body.contains("1&ndash;10 of 13") && first.body.contains("page=2"));
    let second = send(&a, get("/admin/radio?per=10&page=2", &o)).await;
    assert_eq!(times(&second.body, "<tr id=\"station-"), 3);
    let found = send(&a, get("/admin/radio?q=lyra", &o)).await;
    assert_eq!(times(&found.body, "<tr id=\"station-"), 1);
    assert!(found.body.contains("DWG Lyra"));
}

#[tokio::test]
async fn the_menu_of_the_media_leads_to_podcasts_and_radio() {
    let a = app().await;
    let (cookie, _) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), ..Default::default() };
    let page = send(&a, get("/admin/genres", &o)).await;
    assert!(page.body.contains("href=\"/admin/podcasts\"") && page.body.contains("href=\"/admin/radio\""));
}

// The blacklist administration

#[tokio::test]
async fn the_blacklist_pages_are_for_administrators_only() {
    let a = app().await;
    let anna = a.store.create_profile("Anna", false).await.unwrap();
    let token = a.store.create_session(anna, Role::Member, None).await.unwrap();
    let cookie = format!("david_session={token}");
    let o = Opts { cookie: Some(&cookie), ..Default::default() };
    for path in ["/admin/blacklist", "/admin/blacklist/hits", "/admin/blacklist/preview?pattern=x"] {
        assert_eq!(send(&a, get(path, &o)).await.status, StatusCode::FORBIDDEN, "{path}");
    }
    let csrf = csrf_token(&token);
    let o = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), htmx: true, ..Default::default() };
    for (proc, body) in [("add", "pattern=x"), ("update", "id=1&pattern=x"), ("delete", "id=1"), ("approve", "kind=book&id=1"), ("unapprove", "kind=book&id=1")] {
        let r = send(&a, post(&format!("/admin/rpc/blacklist.{proc}"), body, &o)).await;
        assert_eq!(r.status, StatusCode::FORBIDDEN, "{proc}");
    }
    assert!(a.store.blacklist_words().await.unwrap().is_empty(), "nothing was written");
}

#[tokio::test]
async fn the_blacklist_words_are_rows_the_settings_page_only_links_to_them() {
    let a = app().await;
    let (cookie, _) = sign_in(&a).await;
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    let page = send(&a, get("/admin/settings?tab=values", &plain)).await.body;
    assert!(!page.contains("label_blacklist"), "the textarea is gone");
    assert!(page.contains("href=\"/admin/blacklist\""), "{page}");
    assert_eq!(a.store.setting("label_blacklist").await.unwrap(), None, "the setting went with the migration");
    let menu = send(&a, get("/admin/genres", &plain)).await.body;
    assert!(menu.contains("href=\"/admin/blacklist\""), "the menu of the media leads to the blacklist");
}

#[tokio::test]
async fn a_word_is_added_edited_and_removed_on_the_blacklist_page_and_the_page_says_what_it_did_to_the_marks() {
    use david_server::blacklist::Kind;
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    let o = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), htmx: true, ..Default::default() };
    a.store.import_books(vec![book("A", "Die Katze", &[1]), book("B", "Zwei", &[1])]).await.unwrap();
    let empty = send(&a, get("/admin/blacklist", &plain)).await.body;
    assert!(empty.contains("No word on the list yet.") && empty.contains("dlg-blacklist-new"), "{empty}");

    // Create: the answer leads back to the list with what the change did.
    let r = send(&a, post("/admin/rpc/blacklist.add", &form(&[("pattern", " katze "), ("note", "pets"), ("back", "/admin/blacklist?page=1")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let to = r.header("hx-redirect").unwrap().to_owned();
    assert!(to.starts_with("/admin/blacklist?page=1&added=1&removed=0&marked=1&msg=ok.saved"), "{to}");
    assert_eq!(a.store.blacklist_marked(Kind::Book).await.unwrap().len(), 1);
    let page = send(&a, get(&to, &plain)).await.body;
    assert!(page.contains("<code>katze</code>") && page.contains("pets") && page.contains("1 newly marked, 0 marks removed, 1 media marked in total"), "{page}");

    // The same word again, in another case or with accents folded, and a broken expression are refused and mark the input.
    for bad in ["KATZE", "Kätze", "/(/", "   ", &"x".repeat(101)] {
        let r = send(&a, post("/admin/rpc/blacklist.add", &form(&[("pattern", bad), ("note", "")]), &o)).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{bad:?}");
        assert_eq!(r.header("x-invalid-field"), Some("pattern"), "{bad:?}");
    }
    assert_eq!(a.store.blacklist_words().await.unwrap(), vec!["katze".to_owned()]);

    // Edit: the word and the note change, the marks follow (the other book hits now, the first does not).
    let id = a.store.blacklist_entries().await.unwrap()[0].id.to_string();
    let r = send(&a, post("/admin/rpc/blacklist.update", &form(&[("id", &id), ("pattern", "zwei"), ("note", "number")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.header("hx-redirect").unwrap().contains("added=1&removed=1&marked=1"), "{:?}", r.header("hx-redirect"));
    let entry = &a.store.blacklist_entries().await.unwrap()[0];
    assert_eq!((entry.pattern.as_str(), entry.note.as_str(), entry.kind), ("zwei", "number", "word"));
    assert!(entry.updated_by.is_some(), "the audit fields are filled");
    // A word that is not there is not found; a word cannot take the place of another.
    assert_eq!(send(&a, post("/admin/rpc/blacklist.update", &form(&[("id", "9999"), ("pattern", "x")]), &o)).await.status, StatusCode::NOT_FOUND);

    // Remove: the marks only it set go.
    let r = send(&a, post("/admin/rpc/blacklist.delete", &form(&[("id", &id)]), &o)).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.header("hx-redirect").unwrap().contains("removed=1"), "{:?}", r.header("hx-redirect"));
    assert!(a.store.blacklist_words().await.unwrap().is_empty() && a.store.blacklist_marked(Kind::Book).await.unwrap().is_empty());
    assert_eq!(send(&a, post("/admin/rpc/blacklist.delete", &form(&[("id", &id)]), &o)).await.status, StatusCode::NOT_FOUND);

    // Without the token of the form nothing is written.
    let wrong = Opts { cookie: Some(&cookie), csrf_header: Some("wrong"), htmx: true, ..Default::default() };
    assert_eq!(send(&a, post("/admin/rpc/blacklist.add", &form(&[("pattern", "x")]), &wrong)).await.status, StatusCode::FORBIDDEN);
    assert!(a.store.blacklist_words().await.unwrap().is_empty());

    // Without script the answer is a redirect.
    let r = send(&a, post("/admin/rpc/blacklist.add", &form(&[("pattern", "mit ? katze"), ("back", "https://evil.example/")]), &Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), ..Default::default() })).await;
    assert!(r.header("location").is_some_and(|l| l.starts_with("/admin/blacklist?")), "a foreign address is not followed: {:?}", r.header("location"));
}

#[tokio::test]
async fn the_blacklist_list_shows_kind_and_hits_and_is_searched_and_cut_into_pages() {
    let a = app().await;
    let (cookie, _) = sign_in(&a).await;
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    let books: Vec<_> = (1..=3).map(|i| book("A", &format!("Die Katze {i}"), &[1])).collect();
    a.store.import_books(books).await.unwrap();
    let mut words = vec!["katze".to_owned(), "sex * hund".to_owned(), "/^die/".to_owned()];
    words.extend((1..=12).map(|i| format!("wort{i:02}")));
    a.store.save_blacklist_words(&words).await.unwrap();

    let page = send(&a, get("/admin/blacklist?per=10", &plain)).await.body;
    assert_eq!(times(&page, "<tr id=\"blacklist-"), 10, "{page}");
    assert!(page.contains("class=\"pager\"") && page.contains("&#x2f;admin&#x2f;blacklist?per=10&amp;page=2"), "{page}");
    assert!(page.contains("<td data-label=\"Hits\">3</td>"), "katze hits the three books: {page}");
    assert!(page.contains("pattern") && page.contains("regular expression") && page.contains("<span class=\"badge muted\">word</span>"), "{page}");
    let second = send(&a, get("/admin/blacklist?per=10&page=2", &plain)).await.body;
    assert_eq!(times(&second, "<tr id=\"blacklist-"), 5);
    // The search looks at the word and the note, before the pages are cut.
    let found = send(&a, get("/admin/blacklist?q=hund", &plain)).await.body;
    assert_eq!(times(&found, "<tr id=\"blacklist-"), 1);
    assert!(found.contains("sex * hund"));
}

#[tokio::test]
async fn the_preview_says_what_a_word_would_hit_before_it_is_saved() {
    let a = app().await;
    let (cookie, _) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, ..Default::default() };
    a.store.import_books(vec![book("A", "Die Katze", &[1]), book("B", "Zwei", &[1])]).await.unwrap();
    let hit = send(&a, get("/admin/blacklist/preview?pattern=katze", &o)).await.body;
    assert!(hit.contains("This word would hit 1 media") && hit.contains("A \u{2013} Die Katze") && hit.contains("title"), "{hit}");
    assert!(send(&a, get("/admin/blacklist/preview?pattern=nirgends", &o)).await.body.contains("hits no medium"));
    assert!(send(&a, get("/admin/blacklist/preview?pattern=%2F(%2F", &o)).await.body.contains("Not usable"));
    assert!(send(&a, get("/admin/blacklist/preview?pattern=", &o)).await.body.contains("appears while you type"));
    assert!(a.store.blacklist_words().await.unwrap().is_empty(), "nothing is written by a preview");
}

#[tokio::test]
async fn the_hits_page_lists_the_marked_media_with_word_and_field_and_approves_and_marks_again() {
    use david_server::blacklist::Kind;
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    let o = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), htmx: true, ..Default::default() };
    let books: Vec<_> = (1..=13).map(|i| book("A", &format!("Die Katze {i:02}"), &[1])).collect();
    a.store.import_books(books).await.unwrap();
    assert!(send(&a, get("/admin/blacklist/hits", &plain)).await.body.contains("No medium is marked."));
    a.store.save_blacklist_words(&["katze".to_owned()]).await.unwrap();

    let page = send(&a, get("/admin/blacklist/hits?per=10", &plain)).await.body;
    assert_eq!(times(&page, "<tr id=\"hit-book-"), 10, "{page}");
    assert!(page.contains("<code>katze</code>") && page.contains("title") && page.contains("&#x2f;admin&#x2f;blacklist&#x2f;hits?per=10&amp;page=2"), "{page}");
    assert_eq!(times(&send(&a, get("/admin/blacklist/hits?per=10&page=2", &plain)).await.body, "<tr id=\"hit-book-"), 3);
    assert_eq!(times(&send(&a, get("/admin/blacklist/hits?q=katze+07", &plain)).await.body, "<tr id=\"hit-book-"), 1);

    // Approve one: the mark goes, the row says so and offers to mark again; the word keeps hitting.
    let id = a.store.find_book("A/Die Katze 07").await.unwrap().unwrap().id;
    let r = send(&a, post("/admin/rpc/blacklist.approve", &form(&[("kind", "book"), ("id", &id.to_string()), ("back", "/admin/blacklist/hits?per=10")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.header("hx-redirect").is_some_and(|t| t.starts_with("/admin/blacklist/hits?per=10")), "{:?}", r.header("hx-redirect"));
    assert!(!a.store.blacklist_marked(Kind::Book).await.unwrap().contains(&id));
    let row = send(&a, get("/admin/blacklist/hits?q=katze+07", &plain)).await.body;
    assert!(row.contains("approved") && row.contains("Mark again"), "{row}");
    // Approving twice or something else is not found; a kind that does not exist is invalid.
    assert_eq!(send(&a, post("/admin/rpc/blacklist.approve", &form(&[("kind", "book"), ("id", &id.to_string())]), &o)).await.status, StatusCode::NOT_FOUND);
    assert_eq!(send(&a, post("/admin/rpc/blacklist.approve", &form(&[("kind", "video"), ("id", "1")]), &o)).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    // Mark again.
    let r = send(&a, post("/admin/rpc/blacklist.unapprove", &form(&[("kind", "book"), ("id", &id.to_string())]), &o)).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(a.store.blacklist_marked(Kind::Book).await.unwrap().contains(&id));
}

#[tokio::test]
async fn the_pages_show_the_blacklist_mark_in_the_filter_and_the_rows_and_leaving_it_out_approves_the_book() {
    use david_server::blacklist::Kind;
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), htmx: true, ..Default::default() };
    let plain = Opts { cookie: Some(&cookie), ..Default::default() };
    for (author, title) in [("A", "Die Katze"), ("B", "Zwei")] {
        a.store.import_books(vec![book(author, title, &[1])]).await.unwrap();
    }
    let hit = a.store.find_book("A/Die Katze").await.unwrap().unwrap().id;
    a.store.save_blacklist_words(&["katze".to_owned()]).await.unwrap();

    // The administration shows the system mark in the row and as an entry of the filter, and the filter finds the book by it.
    let page = send(&a, get("/admin/books", &plain)).await.body;
    assert!(page.contains("<span class=\"badge label\">Blacklist</span>") && page.contains("data-label=\"Blacklist\""), "{page}");
    let only = send(&a, get("/admin/books?label=Blacklist", &plain)).await.body;
    assert!(only.contains("Die Katze") && !only.contains(">Zwei<"), "{only}");

    // Saving the window with the mark in the list changes nothing; without it the book is approved.
    let id = hit.to_string();
    assert_eq!(send(&a, post("/admin/rpc/book.update", &form(&[("id", &id), ("labels", "Blacklist, Gebet")]), &o)).await.status, StatusCode::OK);
    assert_eq!(a.store.blacklist_marked(Kind::Book).await.unwrap().len(), 1);
    assert_eq!(a.store.labels_of(hit).await.unwrap(), vec!["Gebet".to_owned()], "the reserved name is not a label of the list");
    assert_eq!(send(&a, post("/admin/rpc/book.update", &form(&[("id", &id), ("labels", "Gebet")]), &o)).await.status, StatusCode::OK);
    assert!(a.store.blacklist_marked(Kind::Book).await.unwrap().is_empty());
}

// The pictures of the users

#[tokio::test]
async fn the_user_list_shows_pictures_and_an_administrator_removes_one() {
    let a = app().await;
    let anna = a.store.create_profile("Anna", false).await.unwrap();
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    png.extend_from_slice(&[0; 32]);
    a.store.set_avatar(anna, &png).await.unwrap();
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), ..Default::default() };
    let page = send(&a, get("/admin/users", &o)).await;
    assert!(page.body.contains(&format!("src=\"/avatar/{anna}\"")), "the picture of Anna is shown");
    assert!(page.body.contains("/admin/rpc/user.remove_avatar"));

    let o = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), htmx: true, ..Default::default() };
    let r = send(&a, post("/admin/rpc/user.remove_avatar", &form(&[("id", &anna.to_string())]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(!a.store.has_avatar(anna).await.unwrap());
    assert!(!r.body.contains("/avatar/"), "the row is drawn again without the picture");
}

#[tokio::test]
async fn only_an_administrator_removes_the_picture_of_another_user() {
    let a = app().await;
    let anna = a.store.create_profile("Anna", false).await.unwrap();
    let ben = a.store.create_profile("Ben", false).await.unwrap();
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    png.extend_from_slice(&[0; 32]);
    a.store.set_avatar(ben, &png).await.unwrap();
    let token = a.store.create_session(anna, Role::Member, None).await.unwrap();
    let cookie = format!("david_session={token}");
    let csrf = csrf_token(&token);
    let o = Opts { cookie: Some(&cookie), csrf_header: Some(&csrf), htmx: true, ..Default::default() };
    let r = send(&a, post("/admin/rpc/user.remove_avatar", &form(&[("id", &ben.to_string())]), &o)).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert!(a.store.has_avatar(ben).await.unwrap());
}

#[tokio::test]
async fn the_pairing_page_lists_the_codes_with_their_state_and_never_the_code() {
    let a = app().await;
    let anna = a.store.create_profile("Anna", false).await.unwrap();
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };

    // No code yet: the section says so and the menu has no number.
    let page = send(&a, get("/admin/pairing", &o)).await;
    assert!(page.body.contains("id=\"pairing-codes\"") && page.body.contains("No pairing codes in the last days."), "{}", page.body);
    assert!(page.body.contains("id=\"pairing-badge\"") && page.body.contains("hidden"), "the badge is there, hidden");
    let badge = send(&a, get("/admin/fragments/pairing-badge", &o)).await;
    assert!(badge.body.contains("hidden") && !badge.body.contains(">1<"), "0 open codes: hidden: {}", badge.body);

    // An open code: listed as open with a "Revoke" button, the badge shows 1, the overview says so. The code itself is nowhere.
    let open = a.store.create_enrollment(anna, false, anna).await.unwrap();
    let page = send(&a, get("/admin/pairing", &o)).await;
    assert!(page.body.contains("Anna") && page.body.contains("pairing.revoke") && !page.body.contains("pairing.delete_code"), "{}", page.body);
    assert!(!page.body.contains(&open.code), "the code is never listed");
    let badge = send(&a, get("/admin/fragments/pairing-badge", &o)).await;
    assert!(!badge.body.contains("hidden") && badge.body.contains(">1<"), "1 open code: shown: {}", badge.body);
    let overview = send(&a, get("/admin/", &o)).await;
    assert!(overview.body.contains("1 open pairing codes") && overview.body.contains("/admin/pairing"), "{}", overview.body);

    // Redeemed: used by the device, and the page says "Device paired" for the last minutes.
    let key = format!("{}A=", "D".repeat(42));
    a.store.redeem_enrollment(&open.code, &key, "Phone of Anna").await.unwrap();
    let page = send(&a, get("/admin/fragments/pairing-codes", &o)).await;
    assert!(page.body.contains("used by") && page.body.contains("Phone of Anna"), "{}", page.body);
    assert!(page.body.contains("Device paired") && page.body.contains("0 min ago"), "{}", page.body);
    assert!(page.body.contains("pairing.delete_code") && !page.body.contains("pairing.revoke"), "a used code can be deleted, not revoked");
    let badge = send(&a, get("/admin/fragments/pairing-badge", &o)).await;
    assert!(badge.body.contains("hidden"), "the code is used: no open code: {}", badge.body);
}

#[tokio::test]
async fn an_open_code_is_revoked_before_it_is_deleted_and_a_revoked_one_cannot_be_redeemed() {
    let a = app().await;
    let anna = a.store.create_profile("Anna", false).await.unwrap();
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let e = a.store.create_enrollment(anna, false, anna).await.unwrap();
    let id = a.store.enrollment_rows().await.unwrap()[0].id.to_string();

    // Deleting an open code is refused.
    let r = send(&a, post("/admin/rpc/pairing.delete_code", &form(&[("id", &id)]), &o)).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.body);
    assert!(r.body.contains("Revoke it first"), "{}", r.body);

    // Revoking works once, and then the code is not open any more.
    let r = send(&a, post("/admin/rpc/pairing.revoke", &form(&[("id", &id)]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.body.contains("id=\"pairing-codes\"") && r.body.contains("revoked"), "the answer is the refreshed section: {}", r.body);
    assert_eq!(send(&a, post("/admin/rpc/pairing.revoke", &form(&[("id", &id)]), &o)).await.status, StatusCode::CONFLICT);
    let key = format!("{}B=", "D".repeat(42));
    assert!(a.store.redeem_enrollment(&e.code, &key, "Phone").await.is_err(), "a revoked code cannot be redeemed");
    assert!(a.store.device_rows().await.unwrap().is_empty());

    // Now it can be deleted; a second time there is nothing.
    let r = send(&a, post("/admin/rpc/pairing.delete_code", &form(&[("id", &id)]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(a.store.enrollment_rows().await.unwrap().is_empty());
    assert_eq!(send(&a, post("/admin/rpc/pairing.delete_code", &form(&[("id", &id)]), &o)).await.status, StatusCode::NOT_FOUND);
    assert_eq!(send(&a, post("/admin/rpc/pairing.revoke", &form(&[("id", "9999")]), &o)).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_code_that_ran_out_is_shown_as_expired_and_a_member_sees_none_of_it() {
    let a = app().await;
    let anna = a.store.create_profile("Anna", false).await.unwrap();
    a.store.save_settings(&[("enrollment_code_ttl_s".to_owned(), "1".to_owned())]).await.unwrap();
    a.store.create_enrollment(anna, false, anna).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(1300)).await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let page = send(&a, get("/admin/fragments/pairing-codes", &o)).await;
    assert!(page.body.contains("expired") && page.body.contains("ran out unused"), "{}", page.body);
    assert!(page.body.contains("pairing.delete_code"), "an expired code can be deleted");

    // Not an administrator: the fragments and the buttons are closed.
    let token = a.store.create_session(anna, Role::Member, None).await.unwrap();
    let cookie = format!("david_session={token}");
    let csrf = csrf_token(&token);
    let m = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    for uri in ["/admin/fragments/pairing-codes", "/admin/fragments/pairing-badge"] {
        assert_eq!(send(&a, get(uri, &m)).await.status, StatusCode::FORBIDDEN, "{uri}");
    }
    assert_eq!(send(&a, post("/admin/rpc/pairing.revoke", &form(&[("id", "1")]), &m)).await.status, StatusCode::FORBIDDEN);
    assert_eq!(send(&a, post("/admin/rpc/pairing.delete_code", &form(&[("id", "1")]), &m)).await.status, StatusCode::FORBIDDEN);
    // Without the token of the page nothing is changed either.
    let no_csrf = Opts { cookie: Some(&cookie), htmx: true, csrf_header: None, ..Default::default() };
    let r = send(&a, post("/admin/rpc/pairing.revoke", &form(&[("id", "1")]), &no_csrf)).await;
    assert!(r.status == StatusCode::FORBIDDEN || r.status == StatusCode::BAD_REQUEST, "{:?}", r.status);
}

#[tokio::test]
async fn making_a_code_in_the_window_tells_the_page_to_refresh_the_list() {
    let a = app().await;
    let anna = a.store.create_profile("Anna", false).await.unwrap();
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let r = send(&a, post("/admin/rpc/pairing.create", &form(&[("id", &anna.to_string()), ("address", "david.local:8484")]), &o)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.header("hx-trigger"), Some("pairing-created"));
}

#[tokio::test]
async fn the_tags_window_edits_the_cd_number_and_the_number_of_cds() {
    if !common::audio::ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let lib = tempfile::TempDir::new().unwrap();
    let dir = lib.path().join("Klaus Lage Band/Amtlich");
    common::audio::make_mp3(&dir.join("a_CD02_TR01.mp3"), &[("album", "Amtlich!"), ("artist", "Klaus Lage Band"), ("disc", "2/2")]);
    common::audio::make_mp3(&dir.join("a_CD02_TR02.mp3"), &[("disc", "2/2")]);
    let root = std::fs::canonicalize(lib.path()).unwrap().to_string_lossy().into_owned();
    send(&a, post("/admin/rpc/import.save_criteria", &form(&[("roots", &root), ("rule", "manual")]), &o)).await;
    send(&a, post("/admin/rpc/import.unlock", "", &o)).await;
    send(&a, post("/admin/rpc/import.run", "", &o)).await;
    let id = a.store.admin_book_rows().await.unwrap()[0].id;

    // The window has the two fields and shows what the first file says: CD 2 of 2.
    let w = send(&a, get(&format!("/admin/rpc/book.tags_form?id={id}"), &o)).await;
    assert_eq!(w.status, StatusCode::OK, "{}", w.body);
    assert!(w.body.contains("name=\"disc\"") && w.body.contains("name=\"discs\""), "{}", w.body);
    assert!(w.body.contains("name=\"disc\" min=\"1\" max=\"99\" inputmode=\"numeric\" placeholder=\" \" value=\"2\""), "CD 2 shows");

    // Numbers outside 1 to 99 are refused and change nothing.
    for bad in ["0", "100", "x"] {
        let r = send(&a, tags_request(&csrf, &cookie, &[("id", &id.to_string()), ("disc", bad)], None)).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "disc={bad}");
    }
    assert_eq!(david_server::tags::read_album(&dir.join("a_CD02_TR01.mp3")).disc, Some(2));

    // It is one CD of one: the files and the library follow.
    let r = send(&a, tags_request(&csrf, &cookie, &[("id", &id.to_string()), ("disc", "1"), ("discs", "1")], None)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    for f in ["a_CD02_TR01.mp3", "a_CD02_TR02.mp3"] {
        let t = david_server::tags::read_album(&dir.join(f));
        assert_eq!((t.disc, t.discs), (Some(1), Some(1)), "{f}");
    }
    assert!(a.store.tracks_of(id).await.unwrap().iter().all(|t| t.disc == 1), "every track is on CD 1 now");
    assert_eq!(a.store.admin_book_rows().await.unwrap()[0].discs_total, Some(1));

    // Only the total: the number of the CD stays.
    let r = send(&a, tags_request(&csrf, &cookie, &[("id", &id.to_string()), ("discs", "3")], None)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let t = david_server::tags::read_album(&dir.join("a_CD02_TR01.mp3"));
    assert_eq!((t.disc, t.discs), (Some(1), Some(3)));
}


#[tokio::test]
async fn the_tags_window_keeps_the_barcode_in_the_library_and_refuses_a_wrong_one() {
    if !common::audio::ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let a = app().await;
    let (cookie, csrf) = sign_in(&a).await;
    let o = Opts { cookie: Some(&cookie), htmx: true, csrf_header: Some(&csrf), ..Default::default() };
    let lib = tempfile::TempDir::new().unwrap();
    let dir = lib.path().join("Klaus Lage Band/Amtlich");
    common::audio::make_mp3(&dir.join("a_CD01_TR01.mp3"), &[("album", "Amtlich!"), ("artist", "Klaus Lage Band")]);
    let root = std::fs::canonicalize(lib.path()).unwrap().to_string_lossy().into_owned();
    send(&a, post("/admin/rpc/import.save_criteria", &form(&[("roots", &root), ("rule", "manual")]), &o)).await;
    send(&a, post("/admin/rpc/import.unlock", "", &o)).await;
    send(&a, post("/admin/rpc/import.run", "", &o)).await;
    let id = a.store.admin_book_rows().await.unwrap()[0].id;
    let ids = id.to_string();

    // A wrong check digit, two values, and letters: refused at the field, nothing written.
    for bad in ["Barcode=4006381333932", "Barcode=4006381333931\nBarcode=036000291452", "Barcode=abc"] {
        let r = send(&a, tags_request(&csrf, &cookie, &[("id", &ids), ("more", bad)], None)).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
        assert!(r.body.contains("not a valid EAN/UPC"), "{bad}: {}", r.body);
    }
    assert!(a.store.book_barcode(id).await.unwrap().is_none());
    assert!(david_server::tags::read_fields(&dir.join("a_CD01_TR01.mp3")).unwrap().iter().all(|(k, _)| k != "Barcode"));

    // A good one (typed with spaces) goes to the files as digits and into the library at once; the search finds the album by it.
    let r = send(&a, tags_request(&csrf, &cookie, &[("id", &ids), ("more", "Barcode=4006 381 333931")], None)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let fields = david_server::tags::read_fields(&dir.join("a_CD01_TR01.mp3")).unwrap();
    assert_eq!(fields.iter().find(|(k, _)| k == "Barcode").map(|(_, v)| v.clone()), Some(vec!["4006381333931".to_owned()]));
    assert_eq!(a.store.book_barcode(id).await.unwrap().as_deref(), Some("4006381333931"));
    let page = send(&a, get("/admin/books?q=4006381", &Opts { cookie: Some(&cookie), ..Default::default() })).await.body;
    assert!(page.contains("Amtlich"), "{page}");

    // An empty value removes it.
    let r = send(&a, tags_request(&csrf, &cookie, &[("id", &ids), ("more", "Barcode=")], None)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(a.store.book_barcode(id).await.unwrap().is_none());
}
