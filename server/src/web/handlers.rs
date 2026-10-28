// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pages and procedures of the administration interface.

use std::collections::HashMap;
use axum::extract::{Form, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;

use super::{AdminSession, AppState, COOKIE, Session, cookie, cookies, lang_of};
use crate::auth::{csrf_token, same_secret};
use crate::admin::i18n::{Catalog, Lang};
use crate::admin::pages::{MessageKind, View};
use crate::auth::Login;
use crate::error::Error;
use crate::roles::{Role, Schedule};
use crate::store::{NewProfile, now_ms};

/// Something that went wrong, as the user should see it.
struct Failure {
    status: StatusCode,
    key: &'static str,
}

impl Failure {
    const fn new(status: StatusCode, key: &'static str) -> Failure {
        Failure { status, key }
    }

    fn input() -> Failure {
        Failure::new(StatusCode::UNPROCESSABLE_ENTITY, "err.invalid_input")
    }

    fn not_found() -> Failure {
        Failure::new(StatusCode::NOT_FOUND, "err.not_found")
    }
}

impl From<Error> for Failure {
    /// The typed errors of the data layer become status codes and messages.
    fn from(e: Error) -> Failure {
        match e {
            Error::LastAdmin => Failure::new(StatusCode::CONFLICT, "err.last_admin"),
            Error::DeviceLimitReached => Failure::new(StatusCode::CONFLICT, "err.device_limit"),
            Error::ProfileLimitReached => Failure::new(StatusCode::CONFLICT, "err.profile_limit"),
            Error::ChildRequiresAgeAndSchedule => Failure::new(StatusCode::UNPROCESSABLE_ENTITY, "err.child_requires"),
            Error::ReleaseRequiresAge => Failure::new(StatusCode::UNPROCESSABLE_ENTITY, "err.release_requires_age"),
            Error::InvalidPin => Failure::new(StatusCode::UNPROCESSABLE_ENTITY, "err.invalid_pin"),
            Error::InvalidName => Failure::new(StatusCode::UNPROCESSABLE_ENTITY, "err.invalid_name"),
            Error::DescriptionTooLong => Failure::new(StatusCode::UNPROCESSABLE_ENTITY, "err.description_too_long"),
            Error::InvalidBirthDate => Failure::new(StatusCode::UNPROCESSABLE_ENTITY, "err.invalid_birth_date"),
            Error::ImportLocked => Failure::new(StatusCode::CONFLICT, "err.import_locked"),
            Error::ImportCriteriaMissing => Failure::new(StatusCode::CONFLICT, "err.import_criteria"),
            Error::InvalidImportRoot(_) => Failure::new(StatusCode::UNPROCESSABLE_ENTITY, "err.import_root"),
            // An update of a row that is not there (a user or device removed meanwhile).
            Error::Db(sea_orm::DbErr::RecordNotUpdated) => Failure::not_found(),
            other => {
                if other.to_string().contains("UNIQUE constraint failed: profile.name") {
                    Failure::new(StatusCode::CONFLICT, "err.exists")
                } else {
                    tracing::error!(error = %other, "procedure failed");
                    Failure::new(StatusCode::INTERNAL_SERVER_ERROR, "err.server")
                }
            }
        }
    }
}

type Outcome = Result<String, Failure>;

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A failure that does not need the templates: a short fragment or a minimal page.
pub fn plain_failure(status: StatusCode, key: &str, lang: Lang, htmx: bool) -> Response {
    let text = escape(Catalog::new(lang).t(key));
    let body = format!("<p class=\"message error\">{text}</p>");
    if htmx {
        return (status, [("hx-retarget", "#messages"), ("hx-reswap", "innerHTML")], Html(body)).into_response();
    }
    let page = format!(
        "<!doctype html><html lang=\"{}\"><head><meta charset=\"utf-8\"><title>David</title>\
         <link rel=\"stylesheet\" href=\"/admin/static/admin.css\"></head><body><main>{body}\
         <p><a href=\"/admin/\">David</a></p></main></body></html>",
        lang.code()
    );
    (status, Html(page)).into_response()
}

fn render(result: crate::error::Result<String>) -> Response {
    match result {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "page failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

/// The message to show after a redirect, taken from `?msg=`. Only keys of our own
/// catalog are accepted, so a link cannot put arbitrary text on the page.
#[derive(Deserialize, Default)]
pub struct PageQuery {
    msg: Option<String>,
    /// Where to go after signing in. Only pages of our own that need it are accepted.
    next: Option<String>,
    /// The tab of the settings page.
    tab: Option<String>,
}

fn message_of(q: &PageQuery, lang: Lang) -> Option<(MessageKind, &str)> {
    let key = q.msg.as_deref()?;
    let kind = if key.starts_with("err.") {
        MessageKind::Error
    } else if key.starts_with("ok.") {
        MessageKind::Ok
    } else {
        return None;
    };
    (Catalog::new(lang).t(key) != key).then_some((kind, key))
}

fn view_for<'a>(s: &'a Session, q: &'a PageQuery) -> View<'a> {
    let mut v = View::new(s.lang).csrf(&s.csrf).me(&s.me);
    if let Some((kind, key)) = message_of(q, s.lang) {
        v = v.message(kind, key);
    }
    v
}

// Static files

/// A static file. The browser may keep it but has to ask the server whether it is still
/// the same (`no-cache` with an ETag of the content): after an update of the server the
/// pages show the new styles at once, and an unchanged file costs one small `304`.
fn static_response(request: &HeaderMap, content_type: &'static str, body: &[u8]) -> Response {
    use sha2::{Digest, Sha256};
    let etag = format!("\"{}\"", Sha256::digest(body).iter().take(8).map(|b| format!("{b:02x}")).collect::<String>());
    let headers = [(header::CONTENT_TYPE, content_type.to_owned()), (header::CACHE_CONTROL, "no-cache".to_owned()), (header::ETAG, etag.clone())];
    if request.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
        return (StatusCode::NOT_MODIFIED, headers).into_response();
    }
    (headers, body.to_vec()).into_response()
}

/// A static file of the program, or the one of the same name that a distribution ships
/// in its `static` folder.
fn static_file(state: &AppState, request: &HeaderMap, name: &str, content_type: &'static str, built_in: &'static [u8]) -> Response {
    match state.ui.static_file(name) {
        Some(custom) => static_response(request, content_type, &custom),
        None => static_response(request, content_type, built_in),
    }
}

pub async fn stylesheet(State(state): State<AppState>, headers: HeaderMap) -> Response {
    static_file(&state, &headers, "admin.css", "text/css; charset=utf-8", include_bytes!("../../static/admin.css"))
}

/// The logo and the icon of the tab. A distribution can replace them like the
/// stylesheet, with files of the same name in its `static` folder.
pub async fn logo(State(state): State<AppState>, headers: HeaderMap) -> Response {
    static_file(&state, &headers, "logo.svg", "image/svg+xml", include_bytes!("../../static/logo.svg"))
}

pub async fn favicon(State(state): State<AppState>, headers: HeaderMap) -> Response {
    static_file(&state, &headers, "favicon.svg", "image/svg+xml", include_bytes!("../../static/favicon.svg"))
}

/// The small script of the pages (popup menus, windows, the planner). A distribution
/// can replace it like the stylesheet.
pub async fn ui_js(State(state): State<AppState>, headers: HeaderMap) -> Response {
    static_file(&state, &headers, "ui.js", "text/javascript; charset=utf-8", include_bytes!("../../static/ui.js"))
}

pub async fn htmx(headers: HeaderMap) -> Response {
    static_response(&headers, "text/javascript; charset=utf-8", include_bytes!("../../static/htmx.min.js"))
}

// Sign-in

/// The pages outside /admin that a sign-in may lead back to.
fn safe_next(next: Option<&str>) -> Option<&'static str> {
    match next {
        Some("/profile") => Some("/profile"),
        _ => None,
    }
}

/// The cookie of older versions of David, scoped to `/admin`. It is removed whenever a
/// browser shows it: left in place it shadows nothing, but it is the reason why the
/// profile page, which lives outside /admin, did not see the sign-in.
fn legacy_cookie_removal(secure: bool) -> HeaderValue {
    let secure = if secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!("{COOKIE}=; HttpOnly; SameSite=Strict; Path=/admin; Max-Age=0{secure}")).expect("plain ASCII")
}

pub async fn login_page(State(state): State<AppState>, headers: HeaderMap, Query(q): Query<PageQuery>) -> Response {
    for token in cookies(&headers, COOKIE) {
        let Ok(Some(info)) = state.store.session_for_token(&token).await else { continue };
        // Already signed in. The cookie is issued again for the whole site (an older
        // one reaches only /admin), the old one is removed, and the person goes on to
        // the page they wanted.
        let left_s = ((info.expires_at - now_ms()) / 1000).max(1);
        // A sign-in that was not asked to be remembered lasts at most 12 hours.
        let remembered = left_s > FORGET_AFTER_MS / 1000;
        let mut res = Redirect::to(safe_next(q.next.as_deref()).unwrap_or(home_of(info.role))).into_response();
        res.headers_mut().append(header::SET_COOKIE, session_cookie(&token, remembered.then_some(left_s), state.secure_cookie));
        res.headers_mut().append(header::SET_COOKIE, legacy_cookie_removal(state.secure_cookie));
        return res;
    }
    let lang = lang_of(&headers);
    // One token per browser, kept while it is valid, so a second tab keeps working.
    let (token, fresh) = match login_token(&headers) {
        Some(t) => (t, false),
        None => (crate::auth::new_token(), true),
    };
    let csrf = csrf_token(&token);
    let mut view = View::new(lang).csrf(&csrf);
    if let Some((kind, key)) = message_of(&q, lang) {
        view = view.message(kind, key);
    }
    let mut res = render(state.ui.login(view));
    if fresh {
        res.headers_mut().append(header::SET_COOKIE, login_cookie(&token, 3600, state.secure_cookie));
    }
    res
}

#[derive(Deserialize)]
pub struct LoginForm {
    name: String,
    pin: String,
    /// Derived from the sign-in cookie; see `LOGIN_COOKIE`.
    #[serde(default)]
    csrf: String,
    /// "Remember this device": the sign-in survives closing the browser.
    #[serde(default)]
    remember: String,
}

/// The session cookie. Without `max_age_s` it is a cookie of the browser session: it
/// is gone when the browser is closed ("remember this device" is off).
fn session_cookie(token: &str, max_age_s: Option<i64>, secure: bool) -> HeaderValue {
    let secure = if secure { "; Secure" } else { "" };
    let max_age = max_age_s.map(|s| format!("; Max-Age={s}")).unwrap_or_default();
    HeaderValue::from_str(&format!("{COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/{max_age}{secure}"))
        .expect("a token is plain ASCII")
}

/// The cookie that protects the sign-in itself. A page of another site could
/// otherwise sign a visitor in as an account of its choosing ("login CSRF"). The
/// sign-in page sets it; the form carries a value derived from it; the sign-in is
/// refused unless the two belong together. `SameSite=Strict` keeps other sites from
/// sending it at all.
const LOGIN_COOKIE: &str = "david_login";

fn login_cookie(token: &str, max_age_s: i64, secure: bool) -> HeaderValue {
    let secure = if secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!("{LOGIN_COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/admin; Max-Age={max_age_s}{secure}"))
        .expect("a token is plain ASCII")
}

/// The sign-in token of the request, if the cookie is well formed.
fn login_token(headers: &HeaderMap) -> Option<String> {
    cookie(headers, LOGIN_COOKIE).filter(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// How long a browser remembers a sign-in that was not asked to be remembered: a day
/// at most on the server, and the cookie ends with the browser.
const FORGET_AFTER_MS: i64 = 12 * 60 * 60 * 1000;

/// Where a user lands after signing in: administrators in the administration,
/// everybody else on their own page.
fn home_of(role: Role) -> &'static str {
    if role == Role::Admin { "/admin/" } else { "/profile" }
}

/// The sign-in page again, with a message. It carries a token for the next try:
/// the one of the request if it had a good one, else a new one (with its cookie).
fn login_failure(state: &AppState, headers: &HeaderMap, status: StatusCode, key: &str) -> Response {
    let lang = lang_of(headers);
    let (token, fresh) = match login_token(headers) {
        Some(t) => (t, false),
        None => (crate::auth::new_token(), true),
    };
    let csrf = csrf_token(&token);
    let mut res = match state.ui.login(View::new(lang).csrf(&csrf).message(MessageKind::Error, key)) {
        Ok(html) => (status, Html(html)).into_response(),
        Err(_) => plain_failure(status, key, lang, false),
    };
    if fresh {
        res.headers_mut().append(header::SET_COOKIE, login_cookie(&token, 3600, state.secure_cookie));
    }
    res
}

pub async fn login_submit(State(state): State<AppState>, headers: HeaderMap, Form(form): Form<LoginForm>) -> Response {
    // First the token of the form against the cookie: a request that does not come
    // from our own page is refused before any PIN is tried.
    let token_ok = login_token(&headers).is_some_and(|t| same_secret(&form.csrf, &csrf_token(&t)));
    if !token_ok {
        return login_failure(&state, &headers, StatusCode::FORBIDDEN, "err.csrf");
    }
    let outcome = match state.store.verify_login(form.name.trim(), &form.pin).await {
        Ok(o) => o,
        Err(e) => {
            tracing::error!(error = %e, "sign-in failed");
            return login_failure(&state, &headers, StatusCode::INTERNAL_SERVER_ERROR, "err.server");
        }
    };
    match outcome {
        Login::WrongCredentials => login_failure(&state, &headers, StatusCode::UNAUTHORIZED, "err.login"),
        Login::Locked { .. } => login_failure(&state, &headers, StatusCode::TOO_MANY_REQUESTS, "err.locked"),
        Login::Ok { user_id, role } => {
            let remember = form.remember == "1";
            let token = match state.store.create_session(user_id, role, None).await {
                Ok(t) => t,
                Err(e) => {
                    tracing::error!(error = %e, "cannot open a session");
                    return login_failure(&state, &headers, StatusCode::INTERNAL_SERVER_ERROR, "err.server");
                }
            };
            if !remember {
                let _ = state.store.limit_session(&token, FORGET_AFTER_MS).await;
            }
            let left_s = match state.store.session_for_token(&token).await {
                Ok(Some(info)) => ((info.expires_at - now_ms()) / 1000).max(1),
                _ => 3600,
            };
            let mut res = Redirect::to(home_of(role)).into_response();
            res.headers_mut().append(
                header::SET_COOKIE,
                session_cookie(&token, remember.then_some(left_s), state.secure_cookie),
            );
            // The sign-in token has done its job; a cookie of an older version goes too.
            res.headers_mut().append(header::SET_COOKIE, login_cookie("", 0, state.secure_cookie));
            res.headers_mut().append(header::SET_COOKIE, legacy_cookie_removal(state.secure_cookie));
            res
        }
    }
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    for token in cookies(&headers, COOKIE) {
        let _ = state.store.delete_session(&token).await;
    }
    let mut res = Redirect::to("/admin/login").into_response();
    res.headers_mut().append(header::SET_COOKIE, session_cookie("", Some(0), state.secure_cookie));
    res.headers_mut().append(header::SET_COOKIE, legacy_cookie_removal(state.secure_cookie));
    res
}

// Pages

pub async fn overview(State(state): State<AppState>, s: AdminSession, Query(q): Query<PageQuery>) -> Response {
    render(state.ui.overview(&state.store, view_for(&s, &q)).await)
}

pub async fn users_page(State(state): State<AppState>, s: AdminSession, Query(q): Query<PageQuery>) -> Response {
    render(state.ui.users(&state.store, view_for(&s, &q)).await)
}

pub async fn books_page(State(state): State<AppState>, s: AdminSession, Query(q): Query<PageQuery>) -> Response {
    render(state.ui.books(&state.store, view_for(&s, &q), "audiobook").await)
}

pub async fn music_page(State(state): State<AppState>, s: AdminSession, Query(q): Query<PageQuery>) -> Response {
    render(state.ui.books(&state.store, view_for(&s, &q), "music").await)
}

pub async fn import_page(State(state): State<AppState>, s: AdminSession, Query(q): Query<PageQuery>) -> Response {
    render(state.ui.import(&state.store, view_for(&s, &q)).await)
}

pub async fn pairing_page(State(state): State<AppState>, s: AdminSession, Query(q): Query<PageQuery>) -> Response {
    render(state.ui.pairing(&state.store, view_for(&s, &q), &state.listen.to_string(), None, state.fingerprint.is_some()).await)
}

pub async fn settings_page(State(state): State<AppState>, s: AdminSession, Query(q): Query<PageQuery>) -> Response {
    render(state.ui.settings(&state.store, view_for(&s, &q), q.tab.as_deref()).await)
}

// Procedures

/// Turns the outcome of a procedure into a response. With HTMX the answer is the
/// fragment (or the error message, swapped into `#messages`); without it the
/// browser is sent back to the page, which shows the message.
fn finish(s: &AdminSession, back: &str, outcome: Outcome) -> Response {
    match (outcome, s.htmx) {
        (Ok(fragment), true) => Html(fragment).into_response(),
        (Ok(_), false) => Redirect::to(&format!("{back}{}msg=ok.saved", sep(back))).into_response(),
        (Err(f), true) => plain_failure(f.status, f.key, s.lang, true),
        (Err(f), false) => Redirect::to(&format!("{back}{}msg={}", sep(back), f.key)).into_response(),
    }
}

/// `?` for the first parameter of an address, `&` for the next.
fn sep(back: &str) -> char {
    if back.contains('?') { '&' } else { '?' }
}

fn view(s: &AdminSession) -> View<'_> {
    View::new(s.lang).csrf(&s.csrf)
}

/// "" is no value, anything else must be a number in range.
fn optional_number(text: &str, min: i64, max: i64) -> Result<Option<i64>, Failure> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let n: i64 = text.parse().map_err(|_| Failure::input())?;
    if (min..=max).contains(&n) { Ok(Some(n)) } else { Err(Failure::input()) }
}

/// The listening hours of a new child: every day from `from` to `until` o'clock.
fn daytime(from: &str, until: &str) -> Result<Schedule, Failure> {
    let from = optional_number(from, 0, 23)?.unwrap_or(7);
    let until = optional_number(until, 1, 24)?.unwrap_or(19);
    if from >= until {
        return Err(Failure::input());
    }
    let mut schedule = Schedule::none();
    schedule.allow_every_day((from * 60) as u16, (until * 60) as u16);
    Ok(schedule)
}

/// The age limit that a birthday gives today; a date that is no birthday is refused.
async fn age_of_birth(state: &AppState, born: &str) -> Result<i64, Failure> {
    let today = state.store.today().await?;
    crate::profile::age_on(born, &today)
        .filter(|a| (0..=120).contains(a))
        .ok_or_else(|| Error::InvalidBirthDate.into())
}

fn parse_role(text: &str) -> Result<Role, Failure> {
    text.parse().map_err(|_| Failure::input())
}

#[derive(Deserialize)]
pub struct UserCreate {
    name: String,
    role: String,
    #[serde(default)]
    first_name: String,
    #[serde(default)]
    last_name: String,
    /// `YYYY-MM-DD`. With it the age limit follows from the birthday.
    #[serde(default)]
    birth_date: String,
    #[serde(default)]
    max_age: String,
    #[serde(default)]
    hours_from: String,
    #[serde(default)]
    hours_until: String,
    #[serde(default)]
    pin: String,
}

pub async fn user_create(State(state): State<AppState>, s: AdminSession, Form(f): Form<UserCreate>) -> Response {
    let outcome = async {
        let name = f.name.trim();
        if name.is_empty() || name.chars().count() > 80 {
            return Err(Failure::input());
        }
        let role = parse_role(&f.role)?;
        let birth = f.birth_date.trim();
        let max_age = if birth.is_empty() { optional_number(&f.max_age, 0, 21)? } else { Some(age_of_birth(&state, birth).await?) };
        let pin = f.pin.trim();
        if !pin.is_empty() && !crate::auth::valid_secret(pin) {
            return Err(Error::InvalidPin.into());
        }
        let new = if role == Role::Child {
            NewProfile { name: name.to_owned(), role, max_age, schedule: Some(daytime(&f.hours_from, &f.hours_until)?) }
        } else {
            NewProfile { name: name.to_owned(), role, max_age, schedule: None }
        };
        let id = state.store.create_user(new).await?;
        if !f.first_name.trim().is_empty() || !f.last_name.trim().is_empty() {
            state.store.set_person_names(id, name, &f.first_name, &f.last_name).await?;
        }
        if !birth.is_empty() {
            state.store.set_birth_date(id, Some(birth)).await?;
        }
        if !pin.is_empty() {
            state.store.set_pin(id, pin).await?;
        }
        Ok(state.ui.user_list_fragment(&state.store, view(&s)).await?)
    }
    .await;
    finish(&s, "/admin/users", outcome)
}

#[derive(Deserialize)]
pub struct UserNames {
    id: i64,
    name: String,
    #[serde(default)]
    first_name: String,
    #[serde(default)]
    last_name: String,
}

/// An administrator changes the names of a user.
pub async fn user_names(State(state): State<AppState>, s: AdminSession, Form(f): Form<UserNames>) -> Response {
    let outcome = async {
        state.store.set_person_names(f.id, &f.name, &f.first_name, &f.last_name).await?;
        user_row(&state, &s, f.id).await
    }
    .await;
    finish(&s, "/admin/users", outcome)
}

async fn user_row(state: &AppState, s: &AdminSession, id: i64) -> Outcome {
    state.ui.user_row_fragment(&state.store, view(s), id).await?.ok_or_else(Failure::not_found)
}

#[derive(Deserialize)]
pub struct UserRole {
    id: i64,
    role: String,
    #[serde(default)]
    max_age: String,
    #[serde(default)]
    hours_from: String,
    #[serde(default)]
    hours_until: String,
}

pub async fn user_set_role(State(state): State<AppState>, s: AdminSession, Form(f): Form<UserRole>) -> Response {
    let outcome = async {
        let role = parse_role(&f.role)?;
        let max_age = optional_number(&f.max_age, 0, 21)?;
        // A child needs an age limit and hours. The planner is only given when the user becomes a child.
        let schedule = if role == Role::Child { Some(daytime(&f.hours_from, &f.hours_until)?) } else { None };
        state.store.change_role(f.id, role, max_age, schedule.as_ref()).await?;
        user_row(&state, &s, f.id).await
    }
    .await;
    finish(&s, "/admin/users", outcome)
}

#[derive(Deserialize)]
pub struct UserUpdate {
    id: i64,
    name: String,
    #[serde(default)]
    first_name: String,
    #[serde(default)]
    last_name: String,
    role: String,
    /// The birthday; with it the age limit follows from it. Empty: the limit stays as it is.
    #[serde(default)]
    birth_date: String,
    /// A typed age limit, for forms that still send one. Absent: not touched.
    #[serde(default)]
    max_age: Option<String>,
    #[serde(default)]
    max_devices: String,
    #[serde(default)]
    hours_from: String,
    #[serde(default)]
    hours_until: String,
    /// Empty keeps the PIN.
    #[serde(default)]
    pin: String,
}

/// The edit window of a user: everything in one go. All input is checked before the
/// first change is made; the rules of the database (last administrator, a child needs
/// an age limit) can still refuse a step, and then the steps before it stay done.
pub async fn user_update(State(state): State<AppState>, s: AdminSession, Form(f): Form<UserUpdate>) -> Response {
    let outcome = async {
        let role = parse_role(&f.role)?;
        let birth = f.birth_date.trim();
        let typed_age = f.max_age.as_deref().map(|a| optional_number(a, 0, 21)).transpose()?;
        // The age that holds after this change: from the birthday, else the typed one, else the current one.
        let current = state.store.profile_by_id(f.id).await?.ok_or_else(Failure::not_found)?;
        let max_age = if !birth.is_empty() { Some(age_of_birth(&state, birth).await?) } else { typed_age.unwrap_or(current.max_age) };
        let max_devices = optional_number(&f.max_devices, 0, 50)?;
        let pin = f.pin.trim();
        if !pin.is_empty() && !crate::auth::valid_secret(pin) {
            return Err(Error::InvalidPin.into());
        }
        // The planner is only given when the user becomes a child; then the hours count.
        let schedule = if role == Role::Child && current.role != "child" { Some(daytime(&f.hours_from, &f.hours_until)?) } else { None };

        state.store.set_person_names(f.id, &f.name, &f.first_name, &f.last_name).await?;
        state.store.change_role(f.id, role, max_age, schedule.as_ref()).await?;
        if !birth.is_empty() {
            state.store.set_birth_date(f.id, Some(birth)).await?;
        } else if typed_age.is_some() {
            // An empty age limit means "none", which change_role leaves alone.
            state.store.set_profile_max_age(f.id, max_age).await?;
        }
        state.store.set_profile_max_devices(f.id, max_devices).await?;
        if !pin.is_empty() {
            state.store.set_pin(f.id, pin).await?;
        }
        user_row(&state, &s, f.id).await
    }
    .await;
    finish(&s, "/admin/users", outcome)
}

#[derive(Deserialize)]
pub struct UserAge {
    id: i64,
    #[serde(default)]
    max_age: String,
}

pub async fn user_set_age_limit(State(state): State<AppState>, s: AdminSession, Form(f): Form<UserAge>) -> Response {
    let outcome = async {
        let max_age = optional_number(&f.max_age, 0, 21)?;
        state.store.set_profile_max_age(f.id, max_age).await?;
        user_row(&state, &s, f.id).await
    }
    .await;
    finish(&s, "/admin/users", outcome)
}

#[derive(Deserialize)]
pub struct UserDevices {
    id: i64,
    #[serde(default)]
    max_devices: String,
}

pub async fn user_set_device_limit(State(state): State<AppState>, s: AdminSession, Form(f): Form<UserDevices>) -> Response {
    let outcome = async {
        let max_devices = optional_number(&f.max_devices, 0, 50)?;
        state.store.set_profile_max_devices(f.id, max_devices).await?;
        user_row(&state, &s, f.id).await
    }
    .await;
    finish(&s, "/admin/users", outcome)
}

#[derive(Deserialize)]
pub struct UserPin {
    id: i64,
    pin: String,
}

pub async fn user_set_pin(State(state): State<AppState>, s: AdminSession, Form(f): Form<UserPin>) -> Response {
    let outcome = async {
        state.store.set_pin(f.id, f.pin.trim()).await?;
        user_row(&state, &s, f.id).await
    }
    .await;
    finish(&s, "/admin/users", outcome)
}

#[derive(Deserialize)]
pub struct IdForm {
    id: i64,
}

pub async fn user_delete(State(state): State<AppState>, s: AdminSession, Form(f): Form<IdForm>) -> Response {
    let outcome = async {
        state.store.delete_profile(f.id).await?;
        // The row is gone: an empty answer replaces it with nothing.
        Ok(String::new())
    }
    .await;
    finish(&s, "/admin/users", outcome)
}

async fn book_row(state: &AppState, s: &AdminSession, id: i64) -> Outcome {
    state.ui.book_row_fragment(&state.store, view(s), id).await?.ok_or_else(Failure::not_found)
}

#[derive(Deserialize)]
pub struct BookRelease {
    id: i64,
    min_age: String,
    #[serde(default)]
    confirm: String,
}

pub async fn book_release(State(state): State<AppState>, s: AdminSession, Form(f): Form<BookRelease>) -> Response {
    let outcome = async {
        let min_age = optional_number(&f.min_age, 0, 21)?.ok_or_else(|| Error::ReleaseRequiresAge)?;
        let book = state.store.admin_book_row(f.id).await?.ok_or_else(Failure::not_found)?;
        // The family decides, but releasing below the recommendation needs a second look.
        if book.recommended_age.is_some_and(|r| min_age < r) && f.confirm != "1" {
            return Err(Failure::new(StatusCode::CONFLICT, "err.below_recommendation"));
        }
        state.store.release_book(f.id, min_age).await?;
        book_row(&state, &s, f.id).await
    }
    .await;
    finish(&s, "/admin/books", outcome)
}

#[derive(Deserialize)]
pub struct BookUpdate {
    id: i64,
    /// "1" when the box "Released" is ticked.
    #[serde(default)]
    released: String,
    #[serde(default)]
    min_age: String,
    /// "1" for learning content (outside the time quota).
    #[serde(default)]
    exempt: String,
    /// "1" to release below the recommendation anyway.
    #[serde(default)]
    confirm: String,
    /// The blurb; absent (an older form) leaves it alone, empty removes it.
    #[serde(default)]
    description: Option<String>,
}

/// The window of a book: released or not, from which age, learning content or not. All
/// input is checked before anything changes.
pub async fn book_update(State(state): State<AppState>, s: AdminSession, Form(f): Form<BookUpdate>) -> Response {
    let outcome = async {
        let book = state.store.admin_book_row(f.id).await?.ok_or_else(Failure::not_found)?;
        if f.description.as_deref().is_some_and(|t| t.trim().chars().count() > crate::store::MAX_DESCRIPTION_CHARS) {
            return Err(Error::DescriptionTooLong.into());
        }
        let release = f.released == "1";
        let min_age = if release { Some(optional_number(&f.min_age, 0, 21)?.ok_or(Error::ReleaseRequiresAge)?) } else { None };
        // The family decides, but releasing below the recommendation needs a second look.
        if let Some(age) = min_age
            && book.recommended_age.is_some_and(|r| age < r)
            && f.confirm != "1"
        {
            return Err(Failure::new(StatusCode::CONFLICT, "err.below_recommendation"));
        }
        match min_age {
            Some(age) => state.store.release_book(f.id, age).await?,
            None if book.released => state.store.unrelease_book(f.id).await?,
            None => {}
        }
        state.store.set_book_quota_exempt(f.id, f.exempt == "1").await?;
        if let Some(text) = &f.description {
            state.store.set_book_description(f.id, text).await?;
        }
        book_row(&state, &s, f.id).await
    }
    .await;
    finish(&s, "/admin/books", outcome)
}

#[derive(Deserialize)]
pub struct BookQuery {
    id: i64,
}

/// The window of the tags of an album, loaded when it opens: it shows what the first file says.
pub async fn book_tags_form(State(state): State<AppState>, s: AdminSession, Query(q): Query<BookQuery>) -> Response {
    let outcome: Result<String, Failure> = async {
        let (book, files) = book_files(&state, q.id).await?;
        let first = files.into_iter().next();
        let tags = match first {
            Some(path) => tokio::task::spawn_blocking(move || crate::tags::read_album(&path)).await.unwrap_or_default(),
            None => crate::tags::AlbumTags::default(),
        };
        Ok(state.ui.book_tags_fragment(view(&s), book.id, &book.kind, &tags)?)
    }
    .await;
    match outcome {
        Ok(html) => Html(html).into_response(),
        Err(f) => plain_failure(f.status, f.key, s.lang, true),
    }
}

/// The book and the real paths of its files, all of them inside the library or an import folder.
/// A file that cannot be found stops the whole action before anything is written.
async fn book_files(state: &AppState, id: i64) -> Result<(crate::store::AdminBookRow, Vec<std::path::PathBuf>), Failure> {
    let book = state.store.admin_book_row(id).await?.ok_or_else(Failure::not_found)?;
    let folder = state.store.book_folder(id).await?.ok_or_else(Failure::not_found)?;
    let mut files = Vec::new();
    for t in state.store.tracks_of(id).await? {
        let path = crate::api::resolve(&state.store, state.library.as_deref(), &folder, &t.file)
            .await
            .ok_or_else(|| Failure::new(StatusCode::CONFLICT, "err.files_missing"))?;
        files.push(path);
    }
    if files.is_empty() {
        return Err(Failure::not_found());
    }
    Ok((book, files))
}

/// Sets the ID3 (or Vorbis, MP4) tags of every file of an album and, if a picture comes with
/// it, the cover: as the picture file of the folder and embedded in every file. A field left
/// empty stays as it is in the files.
pub async fn book_tags(State(state): State<AppState>, s: AdminSession, mut form: axum::extract::Multipart) -> Response {
    let outcome = async {
        let (mut id, mut album, mut artist, mut narrator, mut year, mut genre) = (0i64, String::new(), String::new(), String::new(), String::new(), String::new());
        let mut picture = None;
        loop {
            let field = form.next_field().await.map_err(|_| Failure::new(StatusCode::PAYLOAD_TOO_LARGE, "err.cover_size"))?;
            let Some(field) = field else { break };
            let name = field.name().unwrap_or_default().to_owned();
            if name == "cover" {
                let bytes = field.bytes().await.map_err(|_| Failure::new(StatusCode::PAYLOAD_TOO_LARGE, "err.cover_size"))?;
                if !bytes.is_empty() {
                    picture = Some(bytes);
                }
                continue;
            }
            let text = field.text().await.map_err(|_| Failure::input())?;
            match name.as_str() {
                "id" => id = text.trim().parse().map_err(|_| Failure::input())?,
                "album" => album = text,
                "artist" => artist = text,
                "narrator" => narrator = text,
                "year" => year = text,
                "genre" => genre = text,
                _ => {}
            }
        }
        let clean = |t: &str| -> Result<Option<String>, Failure> {
            let t = t.trim();
            if t.chars().count() > 200 || t.chars().any(char::is_control) {
                return Err(Failure::input());
            }
            Ok((!t.is_empty()).then(|| t.to_owned()))
        };
        let year = clean(&year)?;
        if year.as_deref().is_some_and(|y| y.len() != 4 || !y.chars().all(|c| c.is_ascii_digit())) {
            return Err(Failure::input());
        }
        let cover = match picture {
            Some(bytes) => {
                if bytes.len() > crate::tags::MAX_COVER_BYTES {
                    return Err(Failure::new(StatusCode::PAYLOAD_TOO_LARGE, "err.cover_size"));
                }
                let mime = crate::profile::sniff_image(&bytes).ok_or_else(|| Failure::new(StatusCode::UNPROCESSABLE_ENTITY, "err.cover_type"))?;
                Some((bytes.to_vec(), mime))
            }
            None => None,
        };
        let tags = crate::tags::AlbumTags { album: clean(&album)?, artist: clean(&artist)?, narrator: clean(&narrator)?, year, genre: clean(&genre)?, cover };
        let (_, files) = book_files(&state, id).await?;
        // A picture goes into every file: that many bytes more on the disk of the album.
        if let Some((bytes, _)) = &tags.cover {
            let need = (bytes.len() as u64).saturating_mul(files.len() as u64 + 1);
            if !crate::diskspace::fits(&files[0], need, crate::diskspace::reserve(&state.store).await) {
                return Err(Failure::new(StatusCode::INSUFFICIENT_STORAGE, "err.disk_low"));
            }
        }
        let folder = files[0].parent().map(std::path::Path::to_path_buf);
        let written = tokio::task::spawn_blocking(move || -> Result<(), String> {
            if let (Some((bytes, mime)), Some(dir)) = (&tags.cover, &folder) {
                // The picture file of the folder wins over the embedded ones, so there must be only one.
                for name in crate::api::COVER_FILES {
                    let _ = std::fs::remove_file(dir.join(name));
                }
                let name = if *mime == "image/png" { "cover.png" } else { "cover.jpg" };
                std::fs::write(dir.join(name), bytes).map_err(|e| format!("{name}: {e}"))?;
            }
            for path in &files {
                crate::tags::write_album(path, &tags).map_err(|e| e.to_string())?;
            }
            Ok(())
        })
        .await
        .map_err(|_| Failure::new(StatusCode::INTERNAL_SERVER_ERROR, "err.tags_write"))?;
        if let Err(e) = written {
            tracing::warn!(book = id, error = %e, "writing the tags failed");
            return Err(Failure::new(StatusCode::INTERNAL_SERVER_ERROR, "err.tags_write"));
        }
        book_row(&state, &s, id).await
    }
    .await;
    finish(&s, "/admin/books", outcome)
}

#[derive(Deserialize)]
pub struct BookDelete {
    id: i64,
    /// "1": the files are deleted from the disk too. Otherwise they stay, and the next import
    /// brings the book back.
    #[serde(default)]
    files: String,
}

/// Deletes a book from the library and, if asked, its audio files, pictures and the emptied
/// folders from the disk. Files are looked up the way the player finds them, so only files
/// inside the library or an import folder are ever touched; folders are removed only if they are
/// empty then. A file that is already gone does not matter.
pub async fn book_delete(State(state): State<AppState>, s: AdminSession, Form(f): Form<BookDelete>) -> Response {
    let outcome: Result<String, Failure> = async {
        state.store.admin_book_row(f.id).await?.ok_or_else(Failure::not_found)?;
        if f.files == "1" {
            let folder = state.store.book_folder(f.id).await?.ok_or_else(Failure::not_found)?;
            let mut found = Vec::new();
            for t in state.store.tracks_of(f.id).await? {
                if let Some(path) = crate::api::resolve(&state.store, state.library.as_deref(), &folder, &t.file).await {
                    found.push(path);
                }
            }
            let removed = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
                let dirs: std::collections::BTreeSet<std::path::PathBuf> = found.iter().filter_map(|p| p.parent().map(std::path::Path::to_path_buf)).collect();
                for path in &found {
                    std::fs::remove_file(path)?;
                }
                for dir in dirs {
                    for name in crate::api::COVER_FILES {
                        let _ = std::fs::remove_file(dir.join(name));
                    }
                    // Only if nothing else is in them; the author's folder too, when it is empty.
                    if std::fs::remove_dir(&dir).is_ok()
                        && let Some(author) = dir.parent()
                    {
                        let _ = std::fs::remove_dir(author);
                    }
                }
                Ok(())
            })
            .await;
            if !matches!(removed, Ok(Ok(()))) {
                tracing::warn!(book = f.id, "deleting the files of a book failed");
                return Err(Failure::new(StatusCode::INTERNAL_SERVER_ERROR, "err.delete_files"));
            }
        }
        state.store.delete_book(f.id).await?;
        // The row is gone: an empty answer replaces it with nothing.
        Ok(String::new())
    }
    .await;
    finish(&s, "/admin/books", outcome)
}

/// The cover of a book for the list of the administration.
pub async fn admin_cover(State(state): State<AppState>, _s: AdminSession, headers: header::HeaderMap, axum::extract::Path(id): axum::extract::Path<i64>) -> Response {
    match crate::api::find_cover(&state.store, state.library.as_deref(), id).await {
        Some((mime, bytes)) => {
            // A changed picture must show at once, so the browser asks every time; the tag keeps that cheap.
            use sha2::{Digest, Sha256};
            let tag = format!("\"{}\"", Sha256::digest(&bytes).iter().take(8).map(|b| format!("{b:02x}")).collect::<String>());
            if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(tag.as_str()) {
                return (StatusCode::NOT_MODIFIED, [(header::ETAG, tag)]).into_response();
            }
            ([(header::CONTENT_TYPE, mime.to_owned()), (header::CACHE_CONTROL, "private, no-cache".to_owned()), (header::ETAG, tag)], bytes).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

pub async fn book_unrelease(State(state): State<AppState>, s: AdminSession, Form(f): Form<IdForm>) -> Response {
    let outcome = async {
        state.store.unrelease_book(f.id).await?;
        book_row(&state, &s, f.id).await
    }
    .await;
    finish(&s, "/admin/books", outcome)
}

#[derive(Deserialize)]
pub struct BookExempt {
    id: i64,
    exempt: String,
}

pub async fn book_set_exempt(State(state): State<AppState>, s: AdminSession, Form(f): Form<BookExempt>) -> Response {
    let outcome = async {
        state.store.set_book_quota_exempt(f.id, f.exempt == "1").await?;
        book_row(&state, &s, f.id).await
    }
    .await;
    finish(&s, "/admin/books", outcome)
}

/// The values of the settings page. Each is a number in the unit the page shows, except the time zone.
const SETTING_RULES: [(&str, i64, i64, i64); 7] = [
    ("import_interval_min", 0, 1440, 1),
    ("min_free_gb", 0, 100_000, 1),
    // key, smallest, largest, factor to the stored unit
    ("pin_max_failures", 1, 20, 1),
    ("pin_lock_s", 1, 1440, 60),
    ("enrollment_code_ttl_s", 1, 1440, 60),
    ("device_inactive_days", 1, 3650, 1),
    ("guest_expire_days", 1, 365, 1),
];

pub async fn settings_save(State(state): State<AppState>, s: AdminSession, Form(f): Form<HashMap<String, String>>) -> Response {
    let outcome = async {
        let mut values = Vec::new();
        for (key, min, max, factor) in SETTING_RULES {
            let Some(text) = f.get(key) else { continue };
            let n: i64 = text.trim().parse().map_err(|_| Failure::input())?;
            if !(min..=max).contains(&n) {
                return Err(Failure::input());
            }
            values.push((key.to_owned(), (n * factor).to_string()));
        }
        if let Some(zone) = f.get("time_zone") {
            let zone = zone.trim();
            if jiff::tz::TimeZone::get(zone).is_err() {
                return Err(Failure::input());
            }
            values.push(("time_zone".to_owned(), zone.to_owned()));
        }
        state.store.save_settings(&values).await?;
        Ok(state.ui.settings_values_fragment(&state.store, view(&s), true).await?)
    }
    .await;
    finish(&s, "/admin/settings?tab=values", outcome)
}

pub async fn policies_save(State(state): State<AppState>, s: AdminSession, Form(f): Form<HashMap<String, String>>) -> Response {
    let outcome = async {
        let mut policies = Vec::new();
        for p in state.store.role_policies().await? {
            let (Some(devices), Some(days)) = (f.get(&format!("devices_{}", p.role)), f.get(&format!("days_{}", p.role))) else { continue };
            let devices: i64 = devices.trim().parse().map_err(|_| Failure::input())?;
            let days: i64 = days.trim().parse().map_err(|_| Failure::input())?;
            if !(1..=50).contains(&devices) || !(1..=3650).contains(&days) {
                return Err(Failure::input());
            }
            // A session length that is not whole days (the guest's 24 hours is one) stays as it was
            // while the shown number is left alone.
            let session = if days == p.session_ttl_s / 86_400 && p.session_ttl_s % 86_400 != 0 { p.session_ttl_s } else { days * 86_400 };
            policies.push((p.role, devices, session));
        }
        state.store.save_role_policies(&policies).await?;
        Ok(state.ui.settings_roles_fragment(&state.store, view(&s), true).await?)
    }
    .await;
    finish(&s, "/admin/settings?tab=roles", outcome)
}

// Import

async fn import_panel(state: &AppState, s: &AdminSession, report: Option<crate::scan::ScanReport>) -> Outcome {
    Ok(state.ui.import_panel_fragment(&state.store, view(s), report).await?)
}

/// What the import tab of the settings shows after a change.
async fn import_settings(state: &AppState, s: &AdminSession) -> Outcome {
    Ok(state.ui.import_settings_fragment(&state.store, view(s)).await?)
}

#[derive(Deserialize)]
pub struct ImportCriteria {
    #[serde(default)]
    roots: String,
    /// Folders with music, one per line. Absent (an older form) leaves them alone.
    #[serde(default)]
    music_roots: Option<String>,
    rule: String,
}

pub async fn import_save(State(state): State<AppState>, s: AdminSession, Form(f): Form<ImportCriteria>) -> Response {
    let outcome = async {
        let rule = crate::importer::RatingRule::parse(&f.rule).ok_or_else(Failure::input)?;
        let roots: Vec<String> = f.roots.lines().map(|l| l.trim().to_owned()).filter(|l| !l.is_empty()).collect();
        state.store.save_import_criteria(&roots, rule).await?;
        if let Some(text) = &f.music_roots {
            let music: Vec<String> = text.lines().map(|l| l.trim().to_owned()).filter(|l| !l.is_empty()).collect();
            state.store.save_music_roots(&music).await?;
        }
        import_settings(&state, &s).await
    }
    .await;
    finish(&s, "/admin/settings?tab=import", outcome)
}

pub async fn import_unlock(State(state): State<AppState>, s: AdminSession) -> Response {
    let outcome = async {
        let name = state
            .store
            .list_profiles()
            .await?
            .into_iter()
            .find(|p| p.id == s.user_id)
            .map_or_else(|| "?".to_owned(), |p| p.name);
        state.store.unlock_import(&name).await?;
        import_settings(&state, &s).await
    }
    .await;
    finish(&s, "/admin/settings?tab=import", outcome)
}

pub async fn import_lock(State(state): State<AppState>, s: AdminSession) -> Response {
    let outcome = async {
        state.store.lock_import().await?;
        import_settings(&state, &s).await
    }
    .await;
    finish(&s, "/admin/settings?tab=import", outcome)
}

pub async fn import_run(State(state): State<AppState>, s: AdminSession) -> Response {
    let outcome = async {
        let report = state.store.run_import().await?;
        import_panel(&state, &s, Some(report)).await
    }
    .await;
    finish(&s, "/admin/import", outcome)
}

// Upload of archives

/// The queue as a fragment: the newest jobs and, while one is still open, a timer that asks again.
async fn upload_queue(state: &AppState, s: &AdminSession) -> Outcome {
    Ok(state.ui.upload_queue_fragment(&state.store, view(s)).await?)
}

pub async fn upload_list(State(state): State<AppState>, s: AdminSession) -> Response {
    match upload_queue(&state, &s).await {
        Ok(html) => Html(html).into_response(),
        Err(f) => plain_failure(f.status, f.key, s.lang, true),
    }
}

/// Takes an archive (ZIP, TAR, TAR.GZ, RAR, 7z) and puts it into the queue. The file is
/// written to disk as it arrives, never held in memory.
pub async fn upload_create(State(state): State<AppState>, s: AdminSession, headers: header::HeaderMap, mut form: axum::extract::Multipart) -> Response {
    use tokio::io::AsyncWriteExt;
    let outcome: Result<String, Failure> = async {
        let too_big = || Failure::new(StatusCode::PAYLOAD_TOO_LARGE, "err.upload_size");
        let disk_full = || Failure::new(StatusCode::INSUFFICIENT_STORAGE, "err.upload_disk");
        // Refuse a big upload at once if the disk cannot take it with the reserve: better before
        // several GB have travelled than after.
        let reserve = crate::diskspace::reserve(&state.store).await;
        let announced = headers.get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
        let uploads_dir = state.uploads.archive_path(0, "x");
        let uploads_dir = uploads_dir.parent().and_then(std::path::Path::parent).map(std::path::Path::to_path_buf).unwrap_or_default();
        // The archive is unpacked after it arrived, so twice its size is needed.
        if !crate::diskspace::fits(&uploads_dir, announced.saturating_mul(2), reserve) {
            return Err(disk_full());
        }
        while let Some(mut field) = form.next_field().await.map_err(|_| too_big())? {
            if field.name() != Some("archive") {
                continue;
            }
            let name = field.file_name().unwrap_or_default().to_owned();
            let ending = crate::upload::archive_ending(&name).ok_or_else(|| Failure::new(StatusCode::UNPROCESSABLE_ENTITY, "err.upload_type"))?;
            let id = state.store.create_upload_job(&crate::upload::clean_name(&name, "archive"), s.user_id).await?;
            let path = state.uploads.archive_path(id, &ending);
            let written = async {
                tokio::fs::create_dir_all(path.parent().ok_or(std::io::ErrorKind::InvalidInput)?).await?;
                let mut file = tokio::fs::File::create(&path).await?;
                let mut size = 0u64;
                while let Some(chunk) = field.chunk().await.map_err(|_| std::io::ErrorKind::InvalidData)? {
                    size += chunk.len() as u64;
                    if size > crate::upload::MAX_UPLOAD_BYTES {
                        return Err(std::io::ErrorKind::FileTooLarge.into());
                    }
                    // The announced size can be wrong or missing: look at the disk while it fills.
                    if size / (64 * 1024 * 1024) != (size - chunk.len() as u64) / (64 * 1024 * 1024) && !crate::diskspace::fits(&path, 0, reserve) {
                        return Err(std::io::ErrorKind::StorageFull.into());
                    }
                    file.write_all(&chunk).await?;
                }
                file.flush().await?;
                Ok::<u64, std::io::Error>(size)
            }
            .await;
            match written {
                Ok(0) => {
                    state.store.set_upload_state(id, "failed", "the file is empty").await?;
                    let _ = tokio::fs::remove_dir_all(path.parent().unwrap_or(&path)).await;
                    return Err(Failure::input());
                }
                Ok(size) => {
                    state.store.set_upload_size(id, size as i64).await?;
                    state.uploads.enqueue(id);
                    return upload_queue(&state, &s).await;
                }
                Err(e) => {
                    state.store.set_upload_state(id, "failed", "the upload was interrupted, too large or the disk is full").await?;
                    let _ = tokio::fs::remove_dir_all(path.parent().unwrap_or(&path)).await;
                    let reason = match e.kind() {
                        std::io::ErrorKind::FileTooLarge => "err.upload_size",
                        std::io::ErrorKind::StorageFull => "err.upload_disk",
                        _ => "err.upload_broken",
                    };
                    return Err(Failure::new(if reason == "err.upload_size" { StatusCode::PAYLOAD_TOO_LARGE } else { StatusCode::INSUFFICIENT_STORAGE }, reason));
                }
            }
        }
        Err(Failure::input())
    }
    .await;
    finish(&s, "/admin/import", outcome)
}

// Weekly planner

#[derive(Deserialize)]
pub struct UserQuery {
    user: i64,
    msg: Option<String>,
}

pub async fn schedule_page(State(state): State<AppState>, s: AdminSession, Query(q): Query<UserQuery>) -> Response {
    let page = PageQuery { msg: q.msg, next: None, tab: None };
    match state.ui.schedule(&state.store, view_for(&s, &page), q.user).await {
        Ok(Some(html)) => Html(html).into_response(),
        Ok(None) => plain_failure(StatusCode::NOT_FOUND, "err.not_found", s.lang, s.htmx),
        Err(e) => render(Err(e)),
    }
}

/// The planner of one user as a fragment, for the window that opens from the user list.
pub async fn schedule_fragment(State(state): State<AppState>, s: AdminSession, Query(q): Query<UserQuery>) -> Response {
    match schedule_panel(&state, &s, q.user).await {
        Ok(html) => Html(html).into_response(),
        Err(f) => plain_failure(f.status, f.key, s.lang, true),
    }
}

async fn schedule_panel(state: &AppState, s: &AdminSession, id: i64) -> Outcome {
    state.ui.schedule_panel_fragment(&state.store, view(s), id).await?.ok_or_else(Failure::not_found)
}

fn back_to_schedule(id: i64) -> String {
    format!("/admin/schedule?user={id}")
}

/// The grid sends one `slot=<day>-<slot>` per ticked cell. Anything else is refused,
/// so a hand-made request cannot smuggle in a slot that does not exist.
pub async fn schedule_save(State(state): State<AppState>, s: AdminSession, Form(fields): Form<Vec<(String, String)>>) -> Response {
    let id = fields.iter().find(|(k, _)| k == "id").and_then(|(_, v)| v.parse::<i64>().ok());
    let back = back_to_schedule(id.unwrap_or(0));
    let outcome = async {
        let id = id.ok_or_else(Failure::input)?;
        let mut planner = Schedule::none();
        for (key, value) in fields.iter().filter(|(k, _)| k == "slot") {
            let _ = key;
            let (day, slot) = value.split_once('-').ok_or_else(Failure::input)?;
            let (day, slot): (u8, usize) = (day.parse().map_err(|_| Failure::input())?, slot.parse().map_err(|_| Failure::input())?);
            if day > 6 || slot > 95 {
                return Err(Failure::input());
            }
            planner.set_slot(day, slot, true);
        }
        state.store.set_profile_schedule(id, Some(&planner)).await?;
        schedule_panel(&state, &s, id).await
    }
    .await;
    finish(&s, &back, outcome)
}

#[derive(Deserialize)]
pub struct SchedulePreset {
    id: i64,
    #[serde(default)]
    hours_from: String,
    #[serde(default)]
    hours_until: String,
}

pub async fn schedule_preset(State(state): State<AppState>, s: AdminSession, Form(f): Form<SchedulePreset>) -> Response {
    let outcome = async {
        let planner = daytime(&f.hours_from, &f.hours_until)?;
        state.store.set_profile_schedule(f.id, Some(&planner)).await?;
        schedule_panel(&state, &s, f.id).await
    }
    .await;
    finish(&s, &back_to_schedule(f.id), outcome)
}

pub async fn schedule_remove(State(state): State<AppState>, s: AdminSession, Form(f): Form<IdForm>) -> Response {
    let outcome = async {
        state.store.set_profile_schedule(f.id, None).await?;
        schedule_panel(&state, &s, f.id).await
    }
    .await;
    finish(&s, &back_to_schedule(f.id), outcome)
}

// Pairing and devices

#[derive(Deserialize)]
pub struct PairingCreate {
    id: i64,
    address: String,
    #[serde(default)]
    shared: String,
}

pub async fn pairing_create(State(state): State<AppState>, s: AdminSession, Form(f): Form<PairingCreate>) -> Response {
    let outcome = async {
        let address = f.address.trim();
        if !crate::pairing::valid_address(address) {
            return Err(Failure::new(StatusCode::UNPROCESSABLE_ENTITY, "err.invalid_address"));
        }
        let enrollment = state.store.create_enrollment(f.id, f.shared == "1", s.user_id).await.map_err(|e| match e {
            // The user vanished between the page and the click.
            Error::WriteFailed(m) if m == "no such user" => Failure::not_found(),
            other => Failure::from(other),
        })?;
        let uri = crate::pairing::pairing_uri(&state.store.server_id().await?, address, &enrollment.code, state.fingerprint.as_deref());
        let minutes = ((enrollment.expires_at - now_ms()) / 60_000).max(1);
        Ok(state.ui.pairing_result_fragment(view(&s), &enrollment.user_name, &uri, minutes, s.htmx)?)
    }
    .await;
    match outcome {
        // Without JavaScript the code must be on the page that answers: it is shown only once.
        Ok(fragment) if !s.htmx => {
            render(state.ui.pairing(&state.store, view(&s), &state.listen.to_string(), Some(fragment), state.fingerprint.is_some()).await)
        }
        other => finish(&s, "/admin/pairing", other),
    }
}

pub async fn device_revoke(State(state): State<AppState>, s: AdminSession, Form(f): Form<IdForm>) -> Response {
    let outcome = async {
        state.store.revoke_device(f.id).await?;
        state.ui.device_row_fragment(&state.store, view(&s), f.id).await?.ok_or_else(Failure::not_found)
    }
    .await;
    finish(&s, "/admin/pairing", outcome)
}

// The user's own page: picture and PIN

pub async fn profile_page(State(state): State<AppState>, s: Session, Query(q): Query<PageQuery>) -> Response {
    render(state.ui.profile(&state.store, view_for(&s, &q), s.user_id).await)
}

fn back_to_profile(key: &str) -> Response {
    Redirect::to(&format!("/profile?msg={key}")).into_response()
}

#[derive(Deserialize)]
pub struct PinForm {
    current: String,
    new: String,
    repeat: String,
}

pub async fn profile_pin(State(state): State<AppState>, s: Session, Form(f): Form<PinForm>) -> Response {
    if f.new != f.repeat {
        return back_to_profile("err.pin_mismatch");
    }
    match state.store.change_pin(s.user_id, &f.current, &f.new, &s.token).await {
        Ok(()) => back_to_profile("ok.pin_changed"),
        Err(crate::error::Error::WrongPin) => back_to_profile("err.pin_wrong"),
        Err(crate::error::Error::PinLocked) => back_to_profile("err.locked"),
        Err(crate::error::Error::InvalidPin) => back_to_profile("err.invalid_pin"),
        Err(e) => {
            tracing::error!(error = %e, "cannot change the PIN");
            back_to_profile("err.server")
        }
    }
}

pub async fn profile_avatar(State(state): State<AppState>, s: Session, mut form: axum::extract::Multipart) -> Response {
    use crate::error::Error;
    let mut picture = None;
    loop {
        match form.next_field().await {
            Ok(Some(field)) if field.name() == Some("avatar") => match field.bytes().await {
                Ok(bytes) => picture = Some(bytes),
                // Larger than the body limit allows, or cut off.
                Err(_) => return back_to_profile("err.avatar_size"),
            },
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(_) => return back_to_profile("err.avatar_size"),
        }
    }
    let Some(bytes) = picture.filter(|b| !b.is_empty()) else {
        return back_to_profile("err.avatar_none");
    };
    match state.store.set_avatar(s.user_id, &bytes).await {
        Ok(()) => back_to_profile("ok.avatar_saved"),
        Err(Error::AvatarType) => back_to_profile("err.avatar_type"),
        Err(Error::AvatarSize) => back_to_profile("err.avatar_size"),
        Err(e) => {
            tracing::error!(error = %e, "cannot store the picture");
            back_to_profile("err.server")
        }
    }
}

pub async fn profile_avatar_remove(State(state): State<AppState>, s: Session) -> Response {
    match state.store.remove_avatar(s.user_id).await {
        Ok(()) => back_to_profile("ok.avatar_removed"),
        Err(e) => {
            tracing::error!(error = %e, "cannot remove the picture");
            back_to_profile("err.server")
        }
    }
}

/// The picture of a user, for every signed-in user (the profile list of the apps
/// shows them). Only PNG, JPEG and WebP are ever served, with the type that was
/// found by looking at the bytes.
pub async fn avatar_image(State(state): State<AppState>, _s: Session, axum::extract::Path(id): axum::extract::Path<i64>, headers: HeaderMap) -> Response {
    let avatar = match state.store.avatar(id).await {
        Ok(Some(a)) => a,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let etag = format!("\"{}\"", avatar.updated_at);
    let cache = [(header::ETAG, etag.clone()), (header::CACHE_CONTROL, "private, no-cache".to_owned())];
    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
        return (StatusCode::NOT_MODIFIED, cache).into_response();
    }
    (cache, [(header::CONTENT_TYPE, avatar.mime)], avatar.data).into_response()
}

#[derive(Deserialize)]
pub struct NamesForm {
    nickname: String,
    #[serde(default)]
    first_name: String,
    #[serde(default)]
    last_name: String,
}

/// A user changes their own nickname, first name and last name.
pub async fn profile_names(State(state): State<AppState>, s: Session, Form(f): Form<NamesForm>) -> Response {
    use crate::error::Error;
    match state.store.set_person_names(s.user_id, &f.nickname, &f.first_name, &f.last_name).await {
        Ok(()) => back_to_profile("ok.names_saved"),
        Err(Error::InvalidName) => back_to_profile("err.invalid_name"),
        Err(e) if e.to_string().contains("UNIQUE constraint failed: profile.name") => back_to_profile("err.exists"),
        Err(e) => {
            tracing::error!(error = %e, "cannot change the names");
            back_to_profile("err.server")
        }
    }
}

#[derive(Deserialize)]
pub struct LanguageForm {
    /// `en`, `de`, or empty for "follow the browser".
    #[serde(default)]
    language: String,
}

pub async fn profile_language(State(state): State<AppState>, s: Session, Form(f): Form<LanguageForm>) -> Response {
    let chosen = (!f.language.is_empty()).then_some(f.language.as_str());
    match state.store.set_language(s.user_id, chosen).await {
        Ok(()) => back_to_profile("ok.language_saved"),
        Err(crate::error::Error::InvalidName) => back_to_profile("err.invalid_input"),
        Err(e) => {
            tracing::error!(error = %e, "cannot save the language");
            back_to_profile("err.server")
        }
    }
}

/// The whole profile in one go: picture, names, language and PIN or password, from the
/// one form of the page. Everything is checked before the first change; the PIN or
/// password is changed first, because it can be refused for a reason the user has to
/// fix (a wrong current one), and the nickname is checked for being free beforehand.
pub async fn profile_save(State(state): State<AppState>, s: Session, mut form: axum::extract::Multipart) -> Response {
    use crate::error::Error;
    let (mut nickname, mut first, mut last, mut language) = (String::new(), String::new(), String::new(), String::new());
    let (mut current, mut new, mut repeat, mut remove_avatar) = (String::new(), String::new(), String::new(), false);
    let mut picture = None;
    loop {
        let field = match form.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(_) => return back_to_profile("err.avatar_size"),
        };
        let name = field.name().unwrap_or_default().to_owned();
        if name == "avatar" {
            match field.bytes().await {
                Ok(bytes) if !bytes.is_empty() => picture = Some(bytes),
                Ok(_) => {}
                Err(_) => return back_to_profile("err.avatar_size"),
            }
            continue;
        }
        let Ok(text) = field.text().await else { return back_to_profile("err.invalid_input") };
        match name.as_str() {
            "nickname" => nickname = text,
            "first_name" => first = text,
            "last_name" => last = text,
            "language" => language = text,
            "current" => current = text,
            "new" => new = text,
            "repeat" => repeat = text,
            "remove_avatar" => remove_avatar = text == "1",
            _ => {}
        }
    }

    // Check everything first.
    let nickname = nickname.trim();
    if nickname.is_empty() || [nickname, first.trim(), last.trim()].iter().any(|n| n.chars().count() > 80) {
        return back_to_profile("err.invalid_name");
    }
    if !matches!(language.as_str(), "" | "en" | "de") {
        return back_to_profile("err.invalid_input");
    }
    let change_secret = !new.is_empty() || !repeat.is_empty();
    if change_secret {
        if new != repeat {
            return back_to_profile("err.pin_mismatch");
        }
        if !crate::auth::valid_secret(&new) {
            return back_to_profile("err.invalid_pin");
        }
    }
    if let Some(bytes) = &picture {
        if bytes.len() > crate::profile::MAX_AVATAR_BYTES {
            return back_to_profile("err.avatar_size");
        }
        if crate::profile::sniff_image(bytes).is_none() {
            return back_to_profile("err.avatar_type");
        }
    }
    match state.store.nickname_free(nickname, s.user_id).await {
        Ok(true) => {}
        Ok(false) => return back_to_profile("err.exists"),
        Err(e) => {
            tracing::error!(error = %e, "cannot check the nickname");
            return back_to_profile("err.server");
        }
    }

    // Then change.
    let outcome = async {
        if change_secret {
            state.store.change_pin(s.user_id, &current, &new, &s.token).await?;
        }
        state.store.set_person_names(s.user_id, nickname, &first, &last).await?;
        state.store.set_language(s.user_id, (!language.is_empty()).then_some(language.as_str())).await?;
        match &picture {
            Some(bytes) => state.store.set_avatar(s.user_id, bytes).await?,
            None if remove_avatar => state.store.remove_avatar(s.user_id).await?,
            None => {}
        }
        Ok::<(), Error>(())
    }
    .await;
    match outcome {
        Ok(()) => back_to_profile("ok.profile_saved"),
        Err(Error::WrongPin) => back_to_profile("err.pin_wrong"),
        Err(Error::PinLocked) => back_to_profile("err.locked"),
        Err(Error::InvalidPin) => back_to_profile("err.invalid_pin"),
        Err(Error::InvalidName) => back_to_profile("err.invalid_name"),
        Err(Error::AvatarType) => back_to_profile("err.avatar_type"),
        Err(Error::AvatarSize) => back_to_profile("err.avatar_size"),
        Err(e) if e.to_string().contains("UNIQUE constraint failed: profile.name") => back_to_profile("err.exists"),
        Err(e) => {
            tracing::error!(error = %e, "cannot save the profile");
            back_to_profile("err.server")
        }
    }
}
