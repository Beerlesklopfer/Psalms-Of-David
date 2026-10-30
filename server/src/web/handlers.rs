// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pages and procedures of the administration interface.

use std::collections::HashMap;
use axum::extract::{Form, Query, RawQuery, State};
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
    /// The input of the form that the failure is about (its `name`), so that the page can mark it.
    field: Option<String>,
}

impl Failure {
    const fn new(status: StatusCode, key: &'static str) -> Failure {
        Failure { status, key, field: None }
    }

    /// Says which input of the form is wrong. The first one named stays.
    fn at(mut self, field: impl Into<String>) -> Failure {
        self.field.get_or_insert_with(|| field.into());
        self
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
    /// What the search of the media pages looks for.
    q: Option<String>,
    /// Only the books with a problem (`1`).
    problems: Option<String>,
    /// Only the books with this label.
    label: Option<String>,
    /// The page of the list (from 1) and how many books it holds (10 to 500; the setting `list_page_size` when absent).
    page: Option<usize>,
    per: Option<usize>,
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

/// The cut and the zoom of a cover in the tags window.
pub async fn crop_js(State(state): State<AppState>, headers: HeaderMap) -> Response {
    static_file(&state, &headers, "crop.js", "text/javascript; charset=utf-8", include_bytes!("../../static/crop.js"))
}

/// The chips of the labels of a book (a list of words as a row of chips).
pub async fn chips_js(State(state): State<AppState>, headers: HeaderMap) -> Response {
    static_file(&state, &headers, "chips.js", "text/javascript; charset=utf-8", include_bytes!("../../static/chips.js"))
}

/// The rows for the fields of an album that are added by a "+" (in the tab Tags).
pub async fn fields_js(State(state): State<AppState>, headers: HeaderMap) -> Response {
    static_file(&state, &headers, "fields.js", "text/javascript; charset=utf-8", include_bytes!("../../static/fields.js"))
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
            crate::actor::set(crate::actor::profile(user_id));
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

/// The cookie that keeps the size of a page of the media lists (a setting of this browser, not of the server).
const PAGE_SIZE_COOKIE: &str = "david_page_size";

/// The size of a page: the one asked for (`?per=`, which also sets the cookie), else the one the cookie keeps, else none (the setting of the server
/// then decides). Only 10 to 500 count.
fn page_size(q: &PageQuery, headers: &HeaderMap) -> (Option<usize>, bool) {
    let valid = |n: usize| (10..=500).contains(&n);
    if let Some(n) = q.per.filter(|n| valid(*n)) {
        return (Some(n), true);
    }
    (super::cookie(headers, PAGE_SIZE_COOKIE).and_then(|v| v.parse::<usize>().ok()).filter(|n| valid(*n)), false)
}

fn page_size_cookie(n: usize, secure: bool) -> HeaderValue {
    let secure = if secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!("{PAGE_SIZE_COOKIE}={n}; HttpOnly; SameSite=Lax; Path=/admin; Max-Age=31536000{secure}")).expect("digits are plain ASCII")
}

async fn media_page(state: &AppState, s: &AdminSession, q: &PageQuery, headers: &HeaderMap, kind: &str) -> Response {
    let (per, remember) = page_size(q, headers);
    let mut res = render(state.ui.books(&state.store, view_for(s, q), state.library.as_deref(), kind, q.q.as_deref(), q.problems.as_deref() == Some("1"), q.label.as_deref().map(str::trim).filter(|l| !l.is_empty()), q.page, per).await);
    if let (true, Some(n)) = (remember, per) {
        res.headers_mut().append(header::SET_COOKIE, page_size_cookie(n, state.secure_cookie));
    }
    res
}

pub async fn books_page(State(state): State<AppState>, s: AdminSession, headers: HeaderMap, Query(q): Query<PageQuery>) -> Response {
    media_page(&state, &s, &q, &headers, "audiobook").await
}

pub async fn music_page(State(state): State<AppState>, s: AdminSession, headers: HeaderMap, Query(q): Query<PageQuery>) -> Response {
    media_page(&state, &s, &q, &headers, "music").await
}

pub async fn genres_page(State(state): State<AppState>, s: AdminSession, Query(q): Query<PageQuery>) -> Response {
    render(state.ui.genres(&state.store, view_for(&s, &q), q.q.as_deref()).await)
}

/// Gives a tag text a genre of the standard ID3 list (or one that the list of the family has already), makes the genre
/// if it is not there, and, if asked (`retag=1`), writes the name into the files of the albums with that text.
pub async fn genre_from_tag(State(state): State<AppState>, s: AdminSession, Form(f): Form<GenreForm>) -> Response {
    let outcome = async {
        let existing = state.store.genres().await?;
        let wanted = f.name.trim();
        let name = match crate::genres::standard_genre(wanted) {
            Some(standard) => standard.to_owned(),
            None => existing.iter().find(|g| g.name.eq_ignore_ascii_case(wanted)).map(|g| g.name.clone()).ok_or_else(Failure::input)?,
        };
        let id = state.store.add_genre(&name).await?;
        // The files first, so that a refusal of the disk does not leave the library saying more than the files do.
        let report = if f.retag == "1" { Some(state.store.retag_genre(state.library.as_deref(), f.tag.trim(), &name).await?) } else { None };
        state.store.assign_genre_tag(&f.tag, id).await?;
        state.store.assign_genre_tag(&name, id).await?;
        if let Some(r) = &report {
            tracing::info!(tag = %f.tag, genre = %name, albums = r.albums, files = r.files, failed = r.failed.len(), "genre written into the files");
            if let Some(first) = r.failed.first() {
                tracing::warn!(error = %first, "files of a genre could not be written");
                return Err(Failure::new(StatusCode::INTERNAL_SERVER_ERROR, "err.genre_files"));
            }
        }
        Ok(())
    }
    .await;
    let outcome: Result<(), Failure> = outcome;
    finish(&s, "/admin/genres", outcome.map(|()| String::new()))
}

#[derive(Deserialize)]
pub struct GenreForm {
    #[serde(default)]
    id: i64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    tag: String,
    #[serde(default)]
    by: i64,
    #[serde(default)]
    visible: String,
    /// `1`: write the genre into the files too.
    #[serde(default)]
    retag: String,
}

/// The genre pages post plain forms; the answer is the page again with a message.
fn genre_done(s: &AdminSession, outcome: std::result::Result<(), crate::Error>) -> Response {
    finish(s, "/admin/genres", outcome.map(|()| String::new()).map_err(Into::into))
}

pub async fn genre_add(State(state): State<AppState>, s: AdminSession, Form(f): Form<GenreForm>) -> Response {
    genre_done(&s, state.store.add_genre(&f.name).await.map(|_| ()))
}

pub async fn genre_rename(State(state): State<AppState>, s: AdminSession, Form(f): Form<GenreForm>) -> Response {
    genre_done(&s, state.store.rename_genre(f.id, &f.name).await)
}

pub async fn genre_visible(State(state): State<AppState>, s: AdminSession, Form(f): Form<GenreForm>) -> Response {
    genre_done(&s, state.store.set_genre_visible(f.id, f.visible == "1").await)
}

pub async fn genre_move(State(state): State<AppState>, s: AdminSession, Form(f): Form<GenreForm>) -> Response {
    genre_done(&s, state.store.move_genre(f.id, f.by).await)
}

pub async fn genre_delete(State(state): State<AppState>, s: AdminSession, Form(f): Form<GenreForm>) -> Response {
    genre_done(&s, state.store.delete_genre(f.id).await)
}

pub async fn genre_assign(State(state): State<AppState>, s: AdminSession, Form(f): Form<GenreForm>) -> Response {
    genre_done(&s, state.store.assign_genre_tag(&f.tag, f.id).await)
}

pub async fn genre_unassign(State(state): State<AppState>, s: AdminSession, Form(f): Form<GenreForm>) -> Response {
    genre_done(&s, state.store.remove_genre_tag(&f.tag).await)
}

pub async fn releases_page(State(state): State<AppState>, s: AdminSession, Query(q): Query<PageQuery>) -> Response {
    render(state.ui.releases(&state.store, view_for(&s, &q)).await)
}

#[derive(Deserialize)]
pub struct ReleaseForm {
    kind: String,
    profile: i64,
    target: i64,
    /// `1` releases, anything else refuses.
    #[serde(default)]
    release: String,
}

/// An administrator releases or refuses what a child asked for (a podcast or a station).
pub async fn release_decide(State(state): State<AppState>, s: AdminSession, Form(f): Form<ReleaseForm>) -> Response {
    let outcome = state.store.decide_release(&f.kind, f.profile, f.target, f.release == "1").await;
    finish(&s, "/admin/releases", outcome.map(|()| String::new()).map_err(Into::into))
}

pub async fn import_page(State(state): State<AppState>, s: AdminSession, Query(q): Query<PageQuery>) -> Response {
    render(state.ui.import(&state.store, view_for(&s, &q)).await)
}

pub async fn cd_page(State(state): State<AppState>, s: AdminSession, Query(q): Query<PageQuery>) -> Response {
    render(state.ui.cd_page(&state.store, &state.cd, view_for(&s, &q)).await)
}

pub async fn pairing_page(State(state): State<AppState>, s: AdminSession, Query(q): Query<PageQuery>) -> Response {
    render(state.ui.pairing(&state.store, view_for(&s, &q), &public_address(&state), None, state.https()).await)
}

pub async fn settings_page(State(state): State<AppState>, s: AdminSession, Query(q): Query<PageQuery>) -> Response {
    let endpoint = mcp_endpoint(&state);
    render(state.ui.settings(&state.store, &state.cd, view_for(&s, &q), q.tab.as_deref(), endpoint.as_deref()).await)
}

/// Where a remote LLM connects, if the server runs with `--mcp`.
fn mcp_endpoint(state: &AppState) -> Option<String> {
    let scheme = if state.https() { "https" } else { "http" };
    state.mcp.then(|| format!("{scheme}://{}/mcp", public_address(state)))
}

#[derive(Deserialize)]
pub struct McpTokenCreate {
    name: String,
    #[serde(default)]
    write: String,
}

/// Makes a token for a remote LLM. It is shown on the answer, once.
pub async fn mcp_token_create(State(state): State<AppState>, s: AdminSession, Form(f): Form<McpTokenCreate>) -> Response {
    let name = f.name.trim();
    if name.is_empty() || name.chars().count() > 60 || name.chars().any(char::is_control) {
        return finish(&s, "/admin/settings?tab=llm", Err(Failure::input()));
    }
    let outcome = async {
        let token = state.store.create_mcp_token(name, f.write == "1", s.user_id).await?;
        tracing::info!(name, write = f.write == "1", "MCP token made");
        Ok::<_, Failure>(token)
    }
    .await;
    match outcome {
        Ok(token) => {
            let endpoint = mcp_endpoint(&state);
            if s.htmx {
                render(state.ui.settings_llm_fragment(&state.store, view(&s), endpoint.as_deref(), Some(&token)).await)
            } else {
                // Without JavaScript the token must be on the page that answers.
                let mut v = view(&s);
                v.message = None;
                render(state.ui.settings_with_token(&state.store, v, endpoint.as_deref(), &token).await)
            }
        }
        Err(f) => finish(&s, "/admin/settings?tab=llm", Err(f)),
    }
}

#[derive(Deserialize)]
pub struct McpPromptForm {
    #[serde(default)]
    text: String,
}

/// Saves the help text for remote LLMs; an empty text brings the built-in one back.
pub async fn mcp_prompt_save(State(state): State<AppState>, s: AdminSession, Form(f): Form<McpPromptForm>) -> Response {
    if f.text.chars().count() > 8000 {
        return finish(&s, "/admin/settings?tab=llm", Err(Failure::input()));
    }
    let outcome = async {
        // The built-in text, saved unchanged, stays the built-in text.
        let text = if f.text.trim() == crate::mcp::DEFAULT_PROMPT { "" } else { f.text.as_str() };
        state.store.set_mcp_prompt(text).await?;
        tracing::info!("MCP help text saved");
        let endpoint = mcp_endpoint(&state);
        Ok(state.ui.settings_llm_fragment(&state.store, view(&s), endpoint.as_deref(), None).await?)
    }
    .await;
    finish(&s, "/admin/settings?tab=llm", outcome)
}

#[derive(Deserialize)]
pub struct IgnoreAdd {
    id: i64,
    scope: String,
    #[serde(default)]
    what: String,
    #[serde(default)]
    back: String,
}

/// "Ignore this": hides a book from the reports or says that its duplicates are wanted. Goes back to the list.
pub async fn ignore_add(State(state): State<AppState>, s: AdminSession, Form(f): Form<IgnoreAdd>) -> Response {
    let what = if f.what.trim().is_empty() { crate::ignore::DUPLICATE } else { f.what.trim() };
    let back = if f.back == "music" { "/admin/music" } else { "/admin/books" };
    let outcome = async {
        state.store.add_ignore(f.id, &f.scope, what, None, &format!("admin:{}", s.user_id)).await?;
        tracing::info!(album = f.id, scope = %f.scope, what, "ignore rule made");
        Ok::<_, Failure>(String::new())
    }
    .await;
    match outcome {
        Ok(_) => Redirect::to(&format!("{back}?msg=ok.saved")).into_response(),
        Err(failure) => finish(&s, back, Err(failure)),
    }
}

#[derive(Deserialize)]
pub struct QuarantineAdd {
    id: i64,
    #[serde(default)]
    back: String,
}

/// "Quarantine": puts this copy of a duplicate into the quarantine folder of its import folder. The
/// same checks as for the MCP tool: it must be a copy, and one copy of the work stays.
pub async fn quarantine_add(State(state): State<AppState>, s: AdminSession, Form(f): Form<QuarantineAdd>) -> Response {
    let back = if f.back == "music" { "/admin/music" } else { "/admin/books" };
    let outcome = async {
        let plans = crate::quarantine::plan(&state.store, state.library.as_deref(), &[f.id]).await.map_err(|e| {
            tracing::warn!(album = f.id, error = %e, "quarantine refused");
            Failure::new(StatusCode::CONFLICT, "err.quarantine")
        })?;
        let plan = plans.first().ok_or_else(Failure::not_found)?;
        if let Some(problem) = &plan.problem {
            tracing::warn!(album = f.id, %problem, "quarantine refused");
            return Err(Failure::new(StatusCode::CONFLICT, "err.quarantine"));
        }
        crate::quarantine::execute(&state.store, plan).await.map_err(|e| {
            tracing::warn!(album = f.id, error = %e, "quarantine failed");
            Failure::new(StatusCode::INTERNAL_SERVER_ERROR, "err.quarantine")
        })?;
        tracing::info!(album = f.id, from = %plan.from.display(), to = %plan.to.display(), "album put into quarantine from the pages");
        state.store.log_tag_change(&format!("admin:{}", s.user_id), f.id, "", "Quarantine", &plan.from.display().to_string(), &plan.to.display().to_string()).await?;
        Ok::<_, Failure>(String::new())
    }
    .await;
    match outcome {
        Ok(_) => Redirect::to(&format!("{back}?msg=ok.saved")).into_response(),
        Err(failure) => finish(&s, back, Err(failure)),
    }
}

#[derive(Deserialize)]
pub struct IgnoreRemove {
    rule: i64,
    #[serde(default)]
    back: String,
}

pub async fn ignore_remove(State(state): State<AppState>, s: AdminSession, Form(f): Form<IgnoreRemove>) -> Response {
    let back = if f.back == "music" { "/admin/music" } else { "/admin/books" };
    match state.store.remove_ignore(f.rule).await {
        Ok(_) => {
            tracing::info!(rule = f.rule, "ignore rule removed");
            Redirect::to(&format!("{back}?msg=ok.saved")).into_response()
        }
        Err(e) => finish(&s, back, Err(e.into())),
    }
}

/// Looks at the rights on the library again, after the administrator changed the ACLs.
pub async fn access_check(State(state): State<AppState>, s: AdminSession) -> Response {
    let outcome = async { Ok(state.ui.access_fragment(&state.store, view(&s)).await?) }.await;
    finish(&s, "/admin/", outcome)
}

pub async fn mcp_token_revoke(State(state): State<AppState>, s: AdminSession, Form(f): Form<IdForm>) -> Response {
    let outcome = async {
        state.store.revoke_mcp_token(f.id).await?;
        tracing::info!(token = f.id, "MCP token revoked");
        let endpoint = mcp_endpoint(&state);
        Ok(state.ui.settings_llm_fragment(&state.store, view(&s), endpoint.as_deref(), None).await?)
    }
    .await;
    finish(&s, "/admin/settings?tab=llm", outcome)
}

// Procedures

/// Turns the outcome of a procedure into a response. With HTMX the answer is the
/// fragment (or the error message, swapped into `#messages`); without it the
/// browser is sent back to the page, which shows the message.
fn finish(s: &AdminSession, back: &str, outcome: Outcome) -> Response {
    match (outcome, s.htmx) {
        (Ok(fragment), true) => Html(fragment).into_response(),
        (Ok(_), false) => Redirect::to(&format!("{back}{}msg=ok.saved", sep(back))).into_response(),
        (Err(f), true) => {
            let mut res = plain_failure(f.status, f.key, s.lang, true);
            // The input that is wrong: the page puts a ring around it (`X-Invalid-Field`, the name of the input).
            if let Some(field) = f.field.as_deref().and_then(|n| HeaderValue::from_str(n).ok()) {
                res.headers_mut().insert("x-invalid-field", field);
            }
            res
        }
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
            return Err(Failure::input().at("name"));
        }
        let role = parse_role(&f.role).map_err(|e| e.at("role"))?;
        let birth = f.birth_date.trim();
        let max_age = if birth.is_empty() { optional_number(&f.max_age, 0, 21).map_err(|e| e.at("max_age"))? } else { Some(age_of_birth(&state, birth).await.map_err(|e| e.at("birth_date"))?) };
        let pin = f.pin.trim();
        if !pin.is_empty() && !crate::auth::valid_secret(pin) {
            return Err(Failure::from(Error::InvalidPin).at("pin"));
        }
        let new = if role == Role::Child {
            NewProfile { name: name.to_owned(), role, max_age, schedule: Some(daytime(&f.hours_from, &f.hours_until).map_err(|e| e.at("hours_from"))?) }
        } else {
            NewProfile { name: name.to_owned(), role, max_age, schedule: None }
        };
        let id = state.store.create_user(new).await.map_err(|e| Failure::from(e).at("name"))?;
        if !f.first_name.trim().is_empty() || !f.last_name.trim().is_empty() {
            state.store.set_person_names(id, name, &f.first_name, &f.last_name).await.map_err(|e| Failure::from(e).at("first_name"))?;
        }
        if !birth.is_empty() {
            state.store.set_birth_date(id, Some(birth)).await.map_err(|e| Failure::from(e).at("birth_date"))?;
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
    /// The folders of this person for every kind of medium; absent leaves them alone, empty removes the folder.
    #[serde(default)]
    folder_audiobook: Option<String>,
    #[serde(default)]
    folder_music: Option<String>,
    #[serde(default)]
    folder_photo: Option<String>,
}

/// The edit window of a user: everything in one go. All input is checked before the
/// first change is made; the rules of the database (last administrator, a child needs
/// an age limit) can still refuse a step, and then the steps before it stay done.
pub async fn user_update(State(state): State<AppState>, s: AdminSession, Form(f): Form<UserUpdate>) -> Response {
    let outcome = async {
        let role = parse_role(&f.role).map_err(|e| e.at("role"))?;
        let birth = f.birth_date.trim();
        let typed_age = f.max_age.as_deref().map(|a| optional_number(a, 0, 21)).transpose().map_err(|e| e.at("max_age"))?;
        // The age that holds after this change: from the birthday, else the typed one, else the current one.
        let current = state.store.profile_by_id(f.id).await?.ok_or_else(Failure::not_found)?;
        let max_age = if !birth.is_empty() { Some(age_of_birth(&state, birth).await.map_err(|e| e.at("birth_date"))?) } else { typed_age.unwrap_or(current.max_age) };
        let max_devices = optional_number(&f.max_devices, 0, 50).map_err(|e| e.at("max_devices"))?;
        let pin = f.pin.trim();
        if !pin.is_empty() && !crate::auth::valid_secret(pin) {
            return Err(Failure::from(Error::InvalidPin).at("pin"));
        }
        // The planner is only given when the user becomes a child; then the hours count.
        let schedule = if role == Role::Child && current.role != "child" { Some(daytime(&f.hours_from, &f.hours_until).map_err(|e| e.at("hours_from"))?) } else { None };

        // The folders: a path the server can read, write and enter; checked before anything changes.
        let mut folders: Vec<(&str, Option<String>)> = Vec::new();
        for (kind, value) in [("audiobook", &f.folder_audiobook), ("music", &f.folder_music), ("photo", &f.folder_photo)] {
            if let Some(v) = value {
                let v = v.trim();
                if v.is_empty() {
                    folders.push((kind, None));
                } else {
                    let real = crate::folders::check(v).map_err(|_| Failure::new(StatusCode::UNPROCESSABLE_ENTITY, "err.folder").at(format!("folder_{kind}")))?;
                    folders.push((kind, Some(real.to_string_lossy().into_owned())));
                }
            }
        }

        state.store.set_person_names(f.id, &f.name, &f.first_name, &f.last_name).await.map_err(|e| Failure::from(e).at("name"))?;
        for (kind, path) in &folders {
            state.store.set_profile_folder(f.id, kind, path.as_deref()).await?;
        }
        state.store.change_role(f.id, role, max_age, schedule.as_ref()).await?;
        if !birth.is_empty() {
            state.store.set_birth_date(f.id, Some(birth)).await.map_err(|e| Failure::from(e).at("birth_date"))?;
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
    state.ui.book_row_fragment(&state.store, state.library.as_deref(), view(s), id).await?.ok_or_else(Failure::not_found)
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
    /// The owner (the id of a profile); absent leaves it alone, empty removes it.
    #[serde(default)]
    owner: Option<String>,
    /// The labels, separated by commas; absent leaves them alone, empty removes them all.
    #[serde(default)]
    labels: Option<String>,
    /// The labels of single titles, one line `track id=label, label` each; the titles that are not listed stay as they are.
    #[serde(default)]
    track_labels: Option<String>,
    /// Which part of the window sent the form: `release` (release, ages, learning content) or `edit` (owner, labels, blurb). Absent: everything at once.
    #[serde(default)]
    section: Option<String>,
    /// The age the medium recommends ("ab N Jahren"); absent leaves it alone, empty removes it.
    #[serde(default)]
    recommended_age: Option<String>,
}

/// The window of a book: released or not, from which age, learning content or not. All
/// input is checked before anything changes.
pub async fn book_update(State(state): State<AppState>, s: AdminSession, Form(f): Form<BookUpdate>) -> Response {
    let outcome = async {
        let book = state.store.admin_book_row(f.id).await?.ok_or_else(Failure::not_found)?;
        if f.description.as_deref().is_some_and(|t| t.trim().chars().count() > crate::store::MAX_DESCRIPTION_CHARS) {
            return Err(Failure::from(Error::DescriptionTooLong).at("description"));
        }
        // The labels are read first: nothing changes when one is not usable.
        let labels = match f.labels.as_deref() {
            Some(text) => Some(crate::labels::parse_list(text).filter(|l| l.len() <= crate::labels::MAX_PER_BOOK).ok_or_else(|| Failure::from(Error::InvalidName).at("labels"))?),
            None => None,
        };
        // The labels of titles: only titles of this book, each list as good as the one of the book.
        let track_labels: Vec<(i64, Vec<String>)> = match f.track_labels.as_deref() {
            Some(text) => {
                let ours: Vec<i64> = state.store.tracks_of(f.id).await?.iter().map(|t| t.id).collect();
                let mut out = Vec::new();
                for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
                    let (id, list) = line.split_once('=').ok_or_else(|| Failure::from(Error::InvalidName).at("track_labels"))?;
                    let id: i64 = id.trim().parse().map_err(|_| Failure::input().at("track_labels"))?;
                    if !ours.contains(&id) {
                        return Err(Failure::input().at("track_labels"));
                    }
                    out.push((id, crate::labels::parse_list(list).filter(|l| l.len() <= crate::labels::MAX_PER_TRACK).ok_or_else(|| Failure::from(Error::InvalidName).at("track_labels"))?));
                }
                out
            }
            None => Vec::new(),
        };
        // The window has a form for the release and one for the rest; each sends only its own part.
        let (do_release, do_edit) = match f.section.as_deref() {
            Some("release") => (true, false),
            Some("edit") => (false, true),
            _ => (true, true),
        };
        let recommended = match (do_release, f.recommended_age.as_deref()) {
            (true, Some(text)) => Some(optional_number(text, 0, 21).map_err(|e| e.at("recommended_age"))?),
            _ => None,
        };
        if do_release {
            let release = f.released == "1";
            let min_age = if release { Some(optional_number(&f.min_age, 0, 21).map_err(|e| e.at("min_age"))?.ok_or_else(|| Failure::from(Error::ReleaseRequiresAge).at("min_age"))?) } else { None };
            // The family decides, but releasing below the recommendation needs a second look (the recommendation that is typed counts).
            let recommendation = match &recommended {
                Some(r) => *r,
                None => book.recommended_age,
            };
            if let Some(age) = min_age
                && recommendation.is_some_and(|r| age < r)
                && f.confirm != "1"
            {
                return Err(Failure::new(StatusCode::CONFLICT, "err.below_recommendation").at("confirm"));
            }
            if let Some(r) = recommended {
                state.store.set_book_recommended_age(f.id, r).await?;
            }
            match min_age {
                Some(age) => state.store.release_book(f.id, age).await?,
                None if book.released => state.store.unrelease_book(f.id).await?,
                None => {}
            }
            state.store.set_book_quota_exempt(f.id, f.exempt == "1").await?;
        }
        if do_edit {
            if let Some(text) = &f.description {
                state.store.set_book_description(f.id, text).await?;
            }
            if let Some(names) = &labels {
                state.store.set_book_labels(f.id, names).await.map_err(|e| Failure::from(e).at("labels"))?.ok_or_else(Failure::not_found)?;
            }
            for (track, names) in &track_labels {
                state.store.set_track_labels(*track, names).await.map_err(|e| Failure::from(e).at("track_labels"))?;
            }
            // The owner is another thing than the release: whom the book belongs to.
            if let Some(owner) = f.owner.as_deref() {
                let owner = owner.trim();
                let id = if owner.is_empty() { None } else { Some(owner.parse::<i64>().map_err(|_| Failure::input().at("owner"))?) };
                if !state.store.set_book_owner(f.id, id).await? {
                    return Err(Failure::input().at("owner"));
                }
            }
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

/// The titles of an album with an entry of labels each, loaded when the section in the tab "Edit" is opened.
pub async fn book_track_labels(State(state): State<AppState>, s: AdminSession, Query(q): Query<BookQuery>) -> Response {
    let outcome: Result<String, Failure> = async {
        state.store.admin_book_row(q.id).await?.ok_or_else(Failure::not_found)?;
        Ok(state.ui.book_track_labels_fragment(&state.store, view(&s), q.id).await?)
    }
    .await;
    match outcome {
        Ok(html) => Html(html).into_response(),
        Err(f) => plain_failure(f.status, f.key, s.lang, true),
    }
}

/// The titles of an album, loaded when its list is opened (an album can have hundreds, the page does not carry them).
pub async fn book_tracks(State(state): State<AppState>, s: AdminSession, Query(q): Query<BookQuery>) -> Response {
    let outcome: Result<String, Failure> = async {
        state.store.admin_book_row(q.id).await?.ok_or_else(Failure::not_found)?;
        Ok(state.ui.book_tracks_fragment(&state.store, view(&s), q.id).await?)
    }
    .await;
    match outcome {
        Ok(html) => Html(html).into_response(),
        Err(f) => plain_failure(f.status, f.key, s.lang, true),
    }
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
        // The files are gone (the import takes the album out soon): the tab says so instead of staying empty.
        Err(f) if f.key == "err.files_missing" => Html(format!("<p class=\"message error\" role=\"alert\">{}</p>", escape(Catalog::new(s.lang).t("err.files_missing")))).into_response(),
        Err(f) => plain_failure(f.status, f.key, s.lang, true),
    }
}

#[derive(Deserialize)]
pub struct SuggestQuery {
    #[serde(default)]
    q: String,
}

/// The labels that fit what is typed, for the suggestions of the entry of labels: the ones that begin with it first, then the ones that hold it,
/// the most used first; at most eight. Without a text the most used ones.
pub async fn label_suggest(State(state): State<AppState>, _s: AdminSession, Query(q): Query<SuggestQuery>) -> Response {
    match state.store.labels().await {
        Ok(mut all) => {
            let needle = q.q.trim().to_lowercase();
            all.retain(|l| needle.is_empty() || l.name.to_lowercase().contains(&needle));
            all.sort_by(|a, b| {
                let (pa, pb) = (a.name.to_lowercase().starts_with(&needle), b.name.to_lowercase().starts_with(&needle));
                pb.cmp(&pa).then(b.books.cmp(&a.books)).then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });
            axum::Json(all.into_iter().take(8).map(|l| l.name).collect::<Vec<_>>()).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "the suggestions of labels failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// The tab "Filing" of a book: the complete place and the metadata of the database, loaded when the tab is opened.
pub async fn book_filing(State(state): State<AppState>, s: AdminSession, Query(q): Query<BookQuery>) -> Response {
    match state.ui.book_filing_fragment(&state.store, state.library.as_deref(), view(&s), q.id).await {
        Ok(Some(html)) => Html(html).into_response(),
        Ok(None) => plain_failure(StatusCode::NOT_FOUND, "err.not_found", s.lang, true),
        Err(e) => {
            tracing::error!(error = %e, "the filing tab failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
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
        let mut more = String::new();
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
                "more" => more = text,
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
        let more_fields = parse_more_fields(&more)?;
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
                if !more_fields.is_empty() {
                    crate::tags::write_fields(path, &more_fields).map_err(|e| e.to_string())?;
                }
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

/// The free lines of the tags window: `Field=Value`, one per line. A field written twice gets both
/// values; `Field=` with nothing after it removes the field. Names are those of `tags::field_names`.
fn parse_more_fields(text: &str) -> Result<Vec<(String, Vec<String>)>, Failure> {
    let names = crate::tags::field_names();
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let (name, value) = line.split_once('=').ok_or_else(Failure::input)?;
        let name = names.iter().find(|n| n.eq_ignore_ascii_case(name.trim())).ok_or_else(Failure::input)?;
        let value = value.trim();
        if value.chars().count() > 2000 || value.chars().any(char::is_control) {
            return Err(Failure::input());
        }
        match out.iter_mut().find(|(n, _)| n == name) {
            Some((_, values)) => values.extend((!value.is_empty()).then(|| value.to_owned())),
            None => out.push(((*name).to_owned(), if value.is_empty() { Vec::new() } else { vec![value.to_owned()] })),
        }
    }
    Ok(out)
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

#[derive(Deserialize)]
pub struct BookVisibility {
    book: i64,
    profile: i64,
    /// Present (`1`) when the box is ticked: the member sees the book.
    visible: Option<String>,
}

/// A box of the matrix of the media list: the book is opened to the member or closed to them. The row is drawn again with the truth.
pub async fn book_visibility(State(state): State<AppState>, s: AdminSession, Form(f): Form<BookVisibility>) -> Response {
    let outcome = async {
        let visible = f.visible.as_deref() == Some("1");
        if !state.store.set_book_visible_for(f.book, f.profile, visible, Some(s.user_id)).await? {
            return Err(Failure::not_found());
        }
        state.store.log_tag_change(&format!("admin:{}", s.user_id), f.book, "", "Visibility", &f.profile.to_string(), if visible { "open" } else { "closed" }).await?;
        tracing::info!(book = f.book, profile = f.profile, visible, by = s.user_id, "visibility of a book changed in the matrix");
        book_row(&state, &s, f.book).await
    }
    .await;
    finish(&s, "/admin/books", outcome)
}

#[derive(Deserialize)]
pub struct BookOwner {
    book: i64,
    /// The id of a profile; empty removes the owner.
    #[serde(default)]
    owner: String,
}

/// The owner box of a row of the media list: whom the book belongs to. This is not the release and not who may see it.
pub async fn book_owner(State(state): State<AppState>, s: AdminSession, Form(f): Form<BookOwner>) -> Response {
    let outcome = async {
        let owner = f.owner.trim();
        let id = if owner.is_empty() { None } else { Some(owner.parse::<i64>().map_err(|_| Failure::input())?) };
        let before = state.store.admin_book_row(f.book).await?.ok_or_else(Failure::not_found)?;
        if !state.store.set_book_owner(f.book, id).await? {
            return Err(Failure::input());
        }
        let after = state.store.admin_book_row(f.book).await?.ok_or_else(Failure::not_found)?;
        state.store.log_tag_change(&format!("admin:{}", s.user_id), f.book, "", "Owner", before.owner.as_deref().unwrap_or(""), after.owner.as_deref().unwrap_or("")).await?;
        book_row(&state, &s, f.book).await
    }
    .await;
    finish(&s, "/admin/books", outcome)
}

#[derive(Deserialize)]
pub struct FolderQuery {
    #[serde(default)]
    path: String,
    /// The id of the input that gets the choice.
    #[serde(default)]
    target: String,
}

/// The dialog that chooses a folder: the folders below `path` that the server can read, write and enter.
pub async fn folder_picker(State(state): State<AppState>, s: AdminSession, Query(q): Query<FolderQuery>) -> Response {
    // The id of an input: letters, digits and dashes only, so that nothing but a name reaches the page.
    if q.target.is_empty() || !q.target.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return plain_failure(StatusCode::UNPROCESSABLE_ENTITY, "err.input", s.lang, true);
    }
    match state.ui.folder_picker(&state.store, view(&s), &q.path, &q.target, state.library.as_deref(), None).await {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "the folder dialog failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct FolderCreate {
    /// The folder the new one goes into.
    path: String,
    name: String,
    target: String,
}

/// Creates a folder in the folder dialog, then shows the dialog inside the new folder (or in the parent with the reason it failed).
pub async fn folder_create(State(state): State<AppState>, s: AdminSession, Form(f): Form<FolderCreate>) -> Response {
    if f.target.is_empty() || !f.target.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return plain_failure(StatusCode::UNPROCESSABLE_ENTITY, "err.input", s.lang, true);
    }
    let (path, reason) = {
        let (parent, name) = (f.path.clone(), f.name.clone());
        match tokio::task::spawn_blocking(move || crate::folders::create(&parent, &name)).await {
            Ok(Ok(made)) => (made.to_string_lossy().into_owned(), None),
            Ok(Err(e)) => {
                tracing::info!(error = %e, parent = %f.path, "a folder was not created");
                let key = match e {
                    crate::folders::CreateError::BadName => "folders.err_name",
                    crate::folders::CreateError::Exists => "folders.err_exists",
                    crate::folders::CreateError::Parent(_) => "folders.err_parent",
                    crate::folders::CreateError::Failed => "folders.err_failed",
                };
                (f.path.clone(), Some(key))
            }
            Err(_) => (f.path.clone(), Some("folders.err_failed")),
        }
    };
    match state.ui.folder_picker(&state.store, view(&s), &path, &f.target, state.library.as_deref(), reason).await {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "the folder dialog failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
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
const SETTING_RULES: [(&str, i64, i64, i64); 11] = [
    ("new_days", 1, 365, 1),
    ("list_page_size", 10, 500, 1),
    ("feed_refresh_min", 0, 10_080, 1),
    ("feed_max_bytes", 1, 100, 1_048_576),
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
        // Names and files that the machine decides on: nothing is built in, so anything sensible is taken.
        let text = |key: &str| f.get(key).map(|v| v.trim().to_owned());
        let plain = |v: &str| v.chars().count() <= 500 && !v.chars().any(char::is_control);
        for key in ["tls_cert", "tls_key"] {
            if let Some(v) = text(key) {
                if !plain(&v) || (!v.is_empty() && !std::path::Path::new(&v).is_absolute()) {
                    return Err(Failure::input());
                }
                values.push((key.to_owned(), v));
            }
        }
        if let (Some(c), Some(k)) = (text("tls_cert"), text("tls_key"))
            && c.is_empty() != k.is_empty()
        {
            return Err(Failure::input());
        }
        if let Some(v) = text("tls_host") {
            if !plain(&v) || v.contains(['/', ' ', ':']) {
                return Err(Failure::input());
            }
            values.push(("tls_host".to_owned(), v));
        }
        if let Some(v) = text("tls_reload_s") {
            if !v.is_empty() && v.parse::<u64>().map_or(true, |n| n < 5) {
                return Err(Failure::input());
            }
            values.push(("tls_reload_s".to_owned(), v));
        }
        if let Some(v) = text("quarantine_folder") {
            if v.is_empty() || !plain(&v) || v.contains(['/', '\\']) || v == "." || v == ".." {
                return Err(Failure::input());
            }
            values.push(("quarantine_folder".to_owned(), v));
        }
        if let Some(v) = text("radio_directory_url") {
            // Where stations are searched: the address of a directory in the format of radio-browser.info. Empty = no search.
            if !plain(&v) || (!v.is_empty() && !(v.starts_with("https://") || v.starts_with("http://"))) || v.contains(' ') {
                return Err(Failure::input());
            }
            values.push(("radio_directory_url".to_owned(), v));
        }
        if let Some(v) = text("placeholder_tags") {
            if !plain(&v) {
                return Err(Failure::input());
            }
            values.push(("placeholder_tags".to_owned(), v));
        }
        if let Some(v) = text("audiobook_genres") {
            if !plain(&v) {
                return Err(Failure::input());
            }
            values.push(("audiobook_genres".to_owned(), v));
        }
        for key in ["tree_audiobook", "tree_music"] {
            if let Some(mode) = f.get(key) {
                if !crate::tree::valid(mode.trim()) {
                    return Err(Failure::input());
                }
                values.push((key.to_owned(), mode.trim().to_owned()));
            }
        }
        let level = match f.get("log_level") {
            Some(text) => Some(crate::logging::parse(text).ok_or_else(Failure::input)?),
            None => None,
        };
        if let Some(level) = level {
            values.push(("log_level".to_owned(), level.to_string().to_lowercase()));
        }
        state.store.save_settings(&values).await?;
        if let Some(level) = level {
            // At once, without a restart.
            crate::logging::set(level);
            tracing::info!(%level, "log level changed in the pages");
        }
        Ok(state.ui.settings_values_fragment(&state.store, view(&s), true).await?)
    }
    .await;
    finish(&s, "/admin/settings?tab=values", outcome)
}

/// The tab "CD import": every value is checked by its own rule (ranges, known placeholders, no path tricks). A
/// refused value is not an error page: the tab comes back with the input kept and the reason, in the language of
/// the administrator, next to the field and at the top.
pub async fn cd_settings_save(State(state): State<AppState>, s: AdminSession, Form(f): Form<HashMap<String, String>>) -> Response {
    let outcome = async {
        let mut values = Vec::new();
        let mut refused: Vec<(String, String, &'static str)> = Vec::new();
        for key in crate::cd_settings::KEYS {
            if let Some(raw) = f.get(*key) {
                match crate::cd_settings::check(key, raw) {
                    Ok(v) => values.push(((*key).to_owned(), v)),
                    Err(code) => refused.push(((*key).to_owned(), raw.clone(), code)),
                }
            }
        }
        if refused.is_empty() {
            state.store.save_settings(&values).await?;
        }
        Ok(state.ui.settings_cd_fragment(&state.store, &state.cd, view(&s), refused.is_empty(), &refused).await?)
    }
    .await;
    finish(&s, "/admin/settings?tab=cdimport", outcome)
}

// The drive of the server on the tab "CD import"

/// The drive panel. The page asks for it every few seconds and sends back what it shows: the signature, the choice
/// of tracks and the typed text. When nothing changed the answer is empty (204) and the page keeps what it has; when
/// the state changed the panel is drawn again with that choice and text, so nothing the person did is lost.
pub async fn cd_drive_panel(State(state): State<AppState>, s: AdminSession, RawQuery(query): RawQuery) -> Response {
    let pairs: Vec<(String, String)> = serde_urlencoded::from_str(query.as_deref().unwrap_or("")).unwrap_or_default();
    let sig = pairs.iter().find(|(k, _)| k == "sig").map(|(_, v)| v.clone());
    let q = crate::admin::pages::PanelQuery::from_pairs(pairs);
    match state.ui.cd_drive_fragment(&state.store, &state.cd, view(&s), &q, sig.as_deref(), None).await {
        Ok(Some(html)) => Html(html).into_response(),
        Ok(None) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::error!(error = %e, "drive panel failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn cd_panel_after(state: &AppState, s: &AdminSession, q: &crate::admin::pages::PanelQuery, error: Option<&str>, taken: &[u32]) -> Response {
    // After an action the panel is always shown, with the reason when it was refused.
    match state.ui.cd_drive_fragment(&state.store, &state.cd, view(s), q, None, error.map(|e| (e, taken))).await {
        Ok(html) => Html(html.unwrap_or_default()).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "drive panel failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Starts the rip of the disc in the drive of the server.
pub async fn cd_rip(State(state): State<AppState>, s: AdminSession, Form(pairs): Form<Vec<(String, String)>>) -> Response {
    let q = crate::admin::pages::PanelQuery::from_pairs(pairs);
    let text = |k: &str| q.fields.get(k).map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
    let number = |k: &str| text(k).and_then(|v| v.parse::<u32>().ok());
    let req = crate::cdrive::RipRequest {
        kind: text("kind"),
        author: text("author").unwrap_or_default(),
        title: text("title").unwrap_or_default(),
        disc: number("disc"),
        discs_total: number("discs_total"),
        narrator: text("narrator"),
        year: text("year"),
        mbid: text("mbid"),
        tracks: q.fields.get("tracks").map(|t| t.lines().map(|l| l.trim().to_owned()).collect()).unwrap_or_default(),
        eject: Some(q.fields.get("eject").is_some_and(|v| v == "1" || v == "on")),
        device: text("device"),
        // The boxes of the list: ticked numbers. A form without the list takes the whole disc.
        only: q.sel.then(|| q.only.clone()),
        resume: q.fields.get("resume").map(|v| v == "1"),
        mode: text("mode"),
        force: q.fields.get("force").is_some_and(|v| v == "1"),
        auto: false,
    };
    match crate::cdrive::start_rip(&state.store, &state.cd, state.library.clone(), s.user_id, req).await {
        // The settings or the choice changed since the interrupted rip: it is read again, and the page says so.
        Ok(st) => cd_panel_after(&state, &s, &crate::admin::pages::PanelQuery::default(), (st.resume == "mismatch").then_some("resume_mismatch"), &[]).await,
        // A refusal keeps what was typed and ticked.
        Err(r) => cd_panel_after(&state, &s, &q, Some(r.refusal.1), &r.taken).await,
    }
}

/// The example for a template field, while it is typed: the real naming, or why the template is refused.
pub async fn cd_preview(State(state): State<AppState>, s: AdminSession, RawQuery(query): RawQuery) -> Response {
    let pairs: Vec<(String, String)> = serde_urlencoded::from_str(query.as_deref().unwrap_or("")).unwrap_or_default();
    let get = |k: &str| pairs.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
    let field = get("field").unwrap_or_default();
    let pattern = get("pattern").or_else(|| get(&field)).unwrap_or_default();
    let text = crate::admin::pages::cd_preview(&state.store, s.lang, &field, &pattern).await;
    Html(escape(&text)).into_response()
}

pub async fn cd_cancel(State(state): State<AppState>, s: AdminSession, Form(f): Form<HashMap<String, String>>) -> Response {
    let id = f.get("id").and_then(|v| v.parse::<i64>().ok());
    let found = id.is_some_and(|id| state.cd.drives.cancel(id));
    cd_panel_after(&state, &s, &crate::admin::pages::PanelQuery::default(), (!found).then_some("not_found"), &[]).await
}

/// Closes the list of tracks of a rip that has ended.
pub async fn cd_dismiss(State(state): State<AppState>, s: AdminSession, Form(f): Form<HashMap<String, String>>) -> Response {
    let devices: Vec<String> = match f.get("device").filter(|d| !d.is_empty()) {
        Some(d) => vec![d.clone()],
        None => state.cd.drives.snapshot().0.into_iter().map(|d| d.device).collect(),
    };
    for d in devices {
        state.cd.drives.dismiss(&d);
    }
    cd_panel_after(&state, &s, &crate::admin::pages::PanelQuery::default(), None, &[]).await
}

/// Empties the log of the imports on the tab "CD import" (the page asks first); what runs stays.
pub async fn cd_log_clear(State(state): State<AppState>, s: AdminSession) -> Response {
    match state.store.clear_cd_log().await {
        Ok(n) => {
            tracing::info!(removed = n, by = s.user_id, "the log of the CD imports was cleared from the pages");
            Redirect::to("/admin/cd?msg=ok.cd_log_cleared").into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "the log of the CD imports could not be cleared");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn cd_eject(State(state): State<AppState>, s: AdminSession) -> Response {
    let error = crate::cdrive::eject(&state.store, &state.cd, None).await.err().map(|r| r.1);
    cd_panel_after(&state, &s, &crate::admin::pages::PanelQuery::default(), error, &[]).await
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

/// Splits the lines of a list of folders: `!/path` lines name folders to leave out, the others are
/// import folders.
fn folder_lines(text: &str) -> (Vec<String>, Vec<String>) {
    let (mut folders, mut left_out) = (Vec::new(), Vec::new());
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        match line.strip_prefix('!') {
            Some(path) => left_out.push(path.trim().to_owned()),
            None => folders.push(line.to_owned()),
        }
    }
    (folders, left_out)
}

/// A refusal of a folder of the import settings, with the row it was typed in: the message names the folder, and the rows of the kind that
/// was being saved are looked at first (a folder that is taken by another kind is typed in both).
fn ring_row(pairs: &[(String, String)], kind: &str, e: Error) -> Failure {
    let named = if let Error::InvalidImportRoot(msg) = &e {
        let prefix = format!("r_{kind}_");
        let fits = |v: &str| {
            let typed = v.trim();
            !typed.is_empty() && (msg.starts_with(typed) || std::fs::canonicalize(typed).is_ok_and(|c| msg.starts_with(&*c.to_string_lossy())))
        };
        let rows = || pairs.iter().filter(|(k, _)| k.starts_with("r_"));
        rows().find(|(k, v)| k.starts_with(&prefix) && fits(v)).or_else(|| rows().find(|(_, v)| fits(v))).map(|(k, _)| k.clone())
    } else {
        None
    };
    let failure = Failure::from(e);
    match named {
        Some(key) => failure.at(key),
        None => failure,
    }
}

/// The kinds of medium that have folders of their own in the import settings.
const ROOT_KINDS: [&str; 4] = ["audiobook", "music", "photo", "ebook"];

/// Saves the folders and the rating rule. The form of the dialog sends a row for every folder (`r_<kind>_<n>` is the path, `x_<kind>_<n>=1`
/// leaves it out) and `rows=1`; an older form sends `roots` and `music_roots` as lines (`!/path` leaves out), which is still understood.
pub async fn import_save(State(state): State<AppState>, s: AdminSession, Form(pairs): Form<Vec<(String, String)>>) -> Response {
    let outcome = async {
        let get = |key: &str| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
        let rule = crate::importer::RatingRule::parse(get("rule").unwrap_or("")).ok_or_else(|| Failure::input().at("rule"))?;
        let by_rows = get("rows").is_some();
        // The folders of a kind and the ones it leaves out, from the rows of the form.
        let rows_of = |kind: &str| -> (Vec<String>, Vec<String>) {
            let prefix = format!("r_{kind}_");
            let (mut folders, mut left_out) = (Vec::new(), Vec::new());
            for (k, v) in &pairs {
                let Some(n) = k.strip_prefix(&prefix) else { continue };
                let path = v.trim();
                if path.is_empty() {
                    continue;
                }
                let out = pairs.iter().any(|(k2, v2)| k2.strip_prefix(&format!("x_{kind}_")) == Some(n) && v2 == "1");
                if out { left_out.push(path.to_owned()) } else { folders.push(path.to_owned()) }
            }
            (folders, left_out)
        };
        let (books, books_out) = if by_rows { rows_of("audiobook") } else { folder_lines(get("roots").unwrap_or("")) };
        let music = if by_rows { Some(rows_of("music")) } else { get("music_roots").map(folder_lines) };
        state.store.save_import_criteria_with(&books, rule, music.as_ref().is_some_and(|m| !m.0.is_empty())).await.map_err(|e| ring_row(&pairs, "audiobook", e))?;
        state.store.save_import_excludes(false, &books_out).await.map_err(|e| ring_row(&pairs, "audiobook", e))?;
        if let Some((music, music_out)) = &music {
            state.store.save_music_roots(music).await.map_err(|e| ring_row(&pairs, "music", e))?;
            state.store.save_import_excludes(true, music_out).await.map_err(|e| ring_row(&pairs, "music", e))?;
        }
        if by_rows {
            for kind in ["photo", "ebook"] {
                let (folders, left_out) = rows_of(kind);
                state.store.save_media_roots(kind, &folders, &left_out).await.map_err(|e| ring_row(&pairs, kind, e))?;
            }
        }
        import_settings(&state, &s).await
    }
    .await;
    finish(&s, "/admin/settings?tab=import", outcome)
}

#[derive(Deserialize)]
pub struct RootRowQuery {
    kind: String,
}

/// One more empty row for the folders of a kind, to be added to the dialog.
pub async fn import_row(State(state): State<AppState>, s: AdminSession, Query(q): Query<RootRowQuery>) -> Response {
    if !ROOT_KINDS.contains(&q.kind.as_str()) {
        return plain_failure(StatusCode::UNPROCESSABLE_ENTITY, "err.input", s.lang, true);
    }
    // A number that no row of the page has: the time in milliseconds.
    match state.ui.import_row_fragment(view(&s), &q.kind, now_ms() % 1_000_000_000).await {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "a row of folders failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
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

/// The health check: no sign-in, no cookies, nothing but the state, so that a monitor, a load balancer or
/// `curl` can ask. 200 with `{"status":"ok"}` if the database answers for reading and writing, 503
/// otherwise. The version is told for the one who looks after the machine; nothing else is.
/// The start page (`/`). It holds nothing but the way to the administration interface;
/// the place is kept for content that follows later.
pub async fn start_page(headers: HeaderMap) -> Response {
    let (title, link) = match lang_of(&headers) {
        Lang::De => ("David", "Zur Verwaltung"),
        _ => ("David", "Administration"),
    };
    let html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{title}</title>\
         <style>body{{font-family:system-ui,sans-serif;margin:0;min-height:100vh;display:grid;place-items:center;background:#f6f6f4;color:#222}}\
         @media(prefers-color-scheme:dark){{body{{background:#1b1b1b;color:#eee}}}}main{{text-align:center}}a{{color:inherit}}</style></head>\
         <body><main><h1>{title}</h1><p><a href=\"/admin/\">{link}</a></p></main></body></html>"
    );
    Html(html).into_response()
}

pub async fn healthz(State(state): State<AppState>) -> Response {
    let ok = state.store.healthy().await;
    let body = serde_json::json!({ "status": if ok { "ok" } else { "unavailable" }, "version": env!("CARGO_PKG_VERSION"), "git": crate::buildinfo::GIT, "built": crate::buildinfo::BUILT });
    let status = if ok { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
    (status, [(header::CACHE_CONTROL, "no-store")], axum::Json(body)).into_response()
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
        // The kind comes before the file in the form, so it is known when the file starts.
        let mut kind = "audiobook";
        while let Some(mut field) = form.next_field().await.map_err(|_| too_big())? {
            if field.name() == Some("kind") {
                kind = if field.text().await.unwrap_or_default().trim() == "music" { "music" } else { "audiobook" };
                continue;
            }
            if field.name() != Some("archive") {
                continue;
            }
            let name = field.file_name().unwrap_or_default().to_owned();
            let ending = crate::upload::archive_ending(&name).ok_or_else(|| Failure::new(StatusCode::UNPROCESSABLE_ENTITY, "err.upload_type"))?;
            let id = state.store.create_upload_job(&crate::upload::clean_name(&name, "archive"), s.user_id, kind).await?;
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

/// Takes loose files and folders that were dropped on the page and puts them into the queue as one job. The
/// form has `kind` first, then for each file its `path` (the place inside the dropped folder) and the `file`
/// itself; only audio files and pictures are kept. Every file is written to disk as it arrives.
pub async fn upload_files(State(state): State<AppState>, s: AdminSession, headers: header::HeaderMap, mut form: axum::extract::Multipart) -> Response {
    use tokio::io::AsyncWriteExt;
    let outcome: Result<String, Failure> = async {
        let too_big = || Failure::new(StatusCode::PAYLOAD_TOO_LARGE, "err.upload_size");
        let disk_full = || Failure::new(StatusCode::INSUFFICIENT_STORAGE, "err.upload_disk");
        let reserve = crate::diskspace::reserve(&state.store).await;
        let announced = headers.get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
        let uploads_dir = state.uploads.archive_path(0, "x");
        let uploads_dir = uploads_dir.parent().and_then(std::path::Path::parent).map(std::path::Path::to_path_buf).unwrap_or_default();
        // The files are moved into the import folder afterwards, so the same room is needed again.
        if !crate::diskspace::fits(&uploads_dir, announced.saturating_mul(2), reserve) {
            return Err(disk_full());
        }
        let mut kind = "audiobook";
        let mut path: Option<Vec<String>> = None;
        let mut job: Option<(i64, std::path::PathBuf)> = None;
        let (mut size, mut count) = (0u64, 0usize);
        let mut first_name = String::new();
        let abort = |state: AppState, id: i64, dir: std::path::PathBuf| async move {
            let _ = state.store.set_upload_state(id, "failed", "the upload was interrupted, too large or the disk is full").await;
            let _ = tokio::fs::remove_dir_all(dir).await;
        };
        while let Some(mut field) = form.next_field().await.map_err(|_| too_big())? {
            match field.name() {
                Some("kind") => kind = if field.text().await.unwrap_or_default().trim() == "music" { "music" } else { "audiobook" },
                Some("path") => path = crate::upload::dropped_path(&field.text().await.unwrap_or_default()),
                Some("file") => {
                    // A file whose path was refused (not audio, not a picture) is read and dropped.
                    let Some(parts) = path.take() else {
                        while field.chunk().await.map_err(|_| too_big())?.is_some() {}
                        continue;
                    };
                    let (id, root) = match &job {
                        Some(j) => j.clone(),
                        None => {
                            let id = state.store.create_upload_job("files", s.user_id, kind).await?;
                            let root = state.uploads.files_dir(id);
                            job = Some((id, root.clone()));
                            (id, root)
                        }
                    };
                    let target = parts.iter().fold(root.clone(), |p, c| p.join(c));
                    if first_name.is_empty() {
                        first_name = parts.first().cloned().unwrap_or_default();
                    }
                    let written = async {
                        tokio::fs::create_dir_all(target.parent().ok_or(std::io::ErrorKind::InvalidInput)?).await?;
                        let mut file = tokio::fs::File::create(&target).await?;
                        while let Some(chunk) = field.chunk().await.map_err(|_| std::io::ErrorKind::InvalidData)? {
                            size += chunk.len() as u64;
                            if size > crate::upload::MAX_UPLOAD_BYTES {
                                return Err(std::io::ErrorKind::FileTooLarge.into());
                            }
                            if size / (64 * 1024 * 1024) != (size - chunk.len() as u64) / (64 * 1024 * 1024) && !crate::diskspace::fits(&target, 0, reserve) {
                                return Err(std::io::ErrorKind::StorageFull.into());
                            }
                            file.write_all(&chunk).await?;
                        }
                        file.flush().await?;
                        Ok::<(), std::io::Error>(())
                    }
                    .await;
                    if let Err(e) = written {
                        abort(state.clone(), id, root.parent().map(std::path::Path::to_path_buf).unwrap_or(root)).await;
                        return Err(match e.kind() {
                            std::io::ErrorKind::FileTooLarge => Failure::new(StatusCode::PAYLOAD_TOO_LARGE, "err.upload_size"),
                            std::io::ErrorKind::StorageFull => disk_full(),
                            _ => Failure::new(StatusCode::INSUFFICIENT_STORAGE, "err.upload_broken"),
                        });
                    }
                    count += 1;
                }
                _ => {}
            }
        }
        let Some((id, root)) = job else { return Err(Failure::new(StatusCode::UNPROCESSABLE_ENTITY, "err.upload_type")) };
        if size == 0 {
            abort(state.clone(), id, root.parent().map(std::path::Path::to_path_buf).unwrap_or(root)).await;
            return Err(Failure::input());
        }
        let name = if count == 1 { first_name } else { format!("{first_name} ({count} files)") };
        state.store.set_upload_name(id, &crate::upload::clean_name(&name, "files")).await?;
        state.store.set_upload_size(id, size as i64).await?;
        state.uploads.enqueue(id);
        upload_queue(&state, &s).await
    }
    .await;
    finish(&s, "/admin/import", outcome)
}

/// The address that devices are told: the name for a certificate from a certificate authority (the
/// certificate is checked against it), else the address of this machine.
pub(crate) fn public_address(state: &AppState) -> String {
    match &state.ca_host {
        Some(host) => format!("{host}:{}", state.listen.port()),
        None => reachable_address(state.listen),
    }
}

/// The address the pairing page offers for a device: the listen address, and when that is "every
/// interface" (0.0.0.0), the address of this machine in the network (the one that leads to the
/// default route), because a device cannot reach 0.0.0.0.
pub(crate) fn reachable_address(listen: std::net::SocketAddr) -> String {
    if !listen.ip().is_unspecified() {
        return listen.to_string();
    }
    let local = std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("192.0.2.1:9").map(|()| s))
        .and_then(|s| s.local_addr())
        .ok()
        .map(|a| a.ip())
        .filter(|ip| !ip.is_unspecified() && !ip.is_loopback());
    match local {
        Some(ip) => std::net::SocketAddr::new(ip, listen.port()).to_string(),
        None => listen.to_string(),
    }
}

// Weekly planner

#[derive(Deserialize)]
pub struct UserQuery {
    user: i64,
    msg: Option<String>,
}

pub async fn schedule_page(State(state): State<AppState>, s: AdminSession, Query(q): Query<UserQuery>) -> Response {
    let page = PageQuery { msg: q.msg, next: None, tab: None, q: None, problems: None, label: None, page: None, per: None };
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
        let planner = daytime(&f.hours_from, &f.hours_until).map_err(|e| e.at("hours_from"))?;
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
        let uri = crate::pairing::pairing_uri(&state.store.server_id().await?, address, &enrollment.code, state.fingerprint.as_deref(), state.ca_host.is_some());
        let minutes = ((enrollment.expires_at - now_ms()) / 60_000).max(1);
        Ok(state.ui.pairing_result_fragment(view(&s), &enrollment.user_name, &uri, minutes, s.htmx)?)
    }
    .await;
    match outcome {
        // Without JavaScript the code must be on the page that answers: it is shown only once.
        Ok(fragment) if !s.htmx => {
            render(state.ui.pairing(&state.store, view(&s), &public_address(&state), Some(fragment), state.https()).await)
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

#[cfg(test)]
mod tests {
    use super::reachable_address;

    #[test]
    fn the_pairing_page_offers_an_address_a_device_can_reach() {
        assert_eq!(reachable_address("192.168.24.250:8484".parse().unwrap()), "192.168.24.250:8484");
        assert_eq!(reachable_address("127.0.0.1:8484".parse().unwrap()), "127.0.0.1:8484");
        // "Every interface" becomes the address of this machine, if it has one, with the same port.
        let any = reachable_address("0.0.0.0:8484".parse().unwrap());
        assert!(any.ends_with(":8484"), "{any}");
        assert!(any == "0.0.0.0:8484" || !any.starts_with("0.0.0.0"), "{any}");
    }
}
