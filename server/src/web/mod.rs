// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The HTTP layer of the administration interface.
//!
//! Pages and RPC-style procedures (`POST /admin/rpc/<noun>.<verb>`) answer with
//! HTML, whole pages or fragments for HTMX. See `docs/spec-admin-ui.md`.
//!
//! Every request passes three layers: security headers, the CSRF check for
//! state-changing requests, and, for the pages and procedures themselves, the
//! session check that demands an administrator.

mod handlers;

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::extract::{DefaultBodyLimit, FromRequestParts, Request};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;

use crate::admin::i18n::Lang;
use crate::admin::pages::AdminUi;
use crate::auth::{csrf_token, same_secret};
use crate::roles::Role;
use crate::store::Store;

pub const COOKIE: &str = "david_session";
/// Largest body a procedure accepts. The planner form is the biggest, a few KiB.
const MAX_BODY: usize = 256 * 1024;
/// Largest upload body: a picture of the largest allowed size plus the form around it.
const AVATAR_BODY: usize = crate::profile::MAX_AVATAR_BYTES + 16 * 1024;

impl AppState {
    /// Does the server speak HTTPS (with its own certificate or one from a certificate authority)?
    pub fn https(&self) -> bool {
        self.fingerprint.is_some() || self.ca_host.is_some()
    }
}

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    /// Whether the server was started with `--mcp` (the settings then say where to connect).
    pub mcp: bool,
    /// The address the interface listens on; the pairing page offers it as the
    /// address devices connect to, until the administrator types another.
    pub listen: std::net::SocketAddr,
    pub ui: Arc<AdminUi>,
    /// Adds `Secure` to the session cookie. On whenever the server speaks HTTPS.
    pub secure_cookie: bool,
    /// SHA-256 fingerprint of the server certificate, put into pairing codes. `None`
    /// when the server speaks plain HTTP (loopback only).
    pub fingerprint: Option<String>,
    /// The name devices connect to when the certificate comes from a certificate authority: devices
    /// check the name, so the pairing code carries it instead of a fingerprint. `None` otherwise.
    pub ca_host: Option<String>,
    /// The development library folder (`--library`), where covers and audio are looked up
    /// besides the import folders of the database.
    pub library: Option<std::path::PathBuf>,
    /// The queue of uploaded archives.
    pub uploads: crate::upload::Uploads,
    /// The CD drive of the server and the import of CDs.
    pub cd: crate::cdimport::CdService,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(handlers::start_page))
        .route("/admin", get(|| async { Redirect::to("/admin/") }))
        .route("/admin/", get(handlers::overview))
        .route("/healthz", get(handlers::healthz))
        .route("/admin/login", get(handlers::login_page).post(handlers::login_submit))
        .route("/admin/logout", post(handlers::logout))
        .route("/admin/users", get(handlers::users_page))
        .route("/admin/books", get(handlers::books_page))
        .route("/admin/music", get(handlers::music_page))
        .route("/admin/genres", get(handlers::genres_page))
        .route("/admin/releases", get(handlers::releases_page))
        .route("/admin/blacklist", get(handlers::blacklist_admin::page))
        .route("/admin/blacklist/hits", get(handlers::blacklist_admin::hits))
        .route("/admin/blacklist/preview", get(handlers::blacklist_admin::preview))
        .route("/admin/rpc/blacklist.add", post(handlers::blacklist_admin::add))
        .route("/admin/rpc/blacklist.update", post(handlers::blacklist_admin::update))
        .route("/admin/rpc/blacklist.delete", post(handlers::blacklist_admin::delete))
        .route("/admin/rpc/blacklist.approve", post(handlers::blacklist_admin::approve))
        .route("/admin/rpc/blacklist.unapprove", post(handlers::blacklist_admin::unapprove))
        .route("/admin/podcasts", get(handlers::podcasts_page))
        .route("/admin/podcasts/{id}", get(handlers::podcast_page))
        .route("/admin/radio", get(handlers::radio_page))
        .route("/admin/rpc/podcast.add", post(handlers::podcast_add))
        .route("/admin/rpc/podcast.labels", post(handlers::podcast_labels))
        .route("/admin/rpc/podcast.followers", post(handlers::podcast_followers))
        .route("/admin/rpc/podcast.refresh", post(handlers::podcast_refresh))
        .route("/admin/rpc/podcast.delete", post(handlers::podcast_delete))
        .route("/admin/rpc/podcast.decide", post(handlers::podcast_decide))
        .route("/admin/rpc/station.add", post(handlers::station_add))
        .route("/admin/rpc/station.update", post(handlers::station_update))
        .route("/admin/rpc/station.delete", post(handlers::station_delete))
        .route("/admin/rpc/station.favourites", post(handlers::station_favourites))
        .route("/admin/rpc/station.decide", post(handlers::station_decide))
        .route("/admin/rpc/release.decide", post(handlers::release_decide))
        .route("/admin/rpc/genre.add", post(handlers::genre_add))
        .route("/admin/rpc/genre.rename", post(handlers::genre_rename))
        .route("/admin/rpc/genre.visible", post(handlers::genre_visible))
        .route("/admin/rpc/genre.move", post(handlers::genre_move))
        .route("/admin/rpc/genre.delete", post(handlers::genre_delete))
        .route("/admin/rpc/genre.assign", post(handlers::genre_assign))
        .route("/admin/rpc/genre.from_tag", post(handlers::genre_from_tag))
        .route("/admin/rpc/genre.unassign", post(handlers::genre_unassign))
        .route("/admin/settings", get(handlers::settings_page))
        .route("/admin/import", get(handlers::import_page))
        .route("/admin/cd", get(handlers::cd_page))
        .route("/admin/schedule", get(handlers::schedule_page))
        .route("/admin/fragments/schedule", get(handlers::schedule_fragment))
        .route("/admin/pairing", get(handlers::pairing_page))
        .route("/admin/rpc/user.create", post(handlers::user_create))
        .route("/admin/cover/{id}", get(handlers::admin_cover))
        .route("/admin/rpc/book.update", post(handlers::book_update))
        .route("/admin/rpc/user.update", post(handlers::user_update))
        .route("/admin/rpc/user.set_names", post(handlers::user_names))
        .route("/admin/rpc/user.set_role", post(handlers::user_set_role))
        .route("/admin/rpc/user.set_age_limit", post(handlers::user_set_age_limit))
        .route("/admin/rpc/user.set_device_limit", post(handlers::user_set_device_limit))
        .route("/admin/rpc/user.set_pin", post(handlers::user_set_pin))
        .route("/admin/rpc/user.delete", post(handlers::user_delete))
        .route("/admin/rpc/user.remove_avatar", post(handlers::user_remove_avatar))
        .route("/admin/rpc/pairing.create", post(handlers::pairing_create))
        .route("/admin/rpc/device.revoke", post(handlers::device_revoke))
        .route("/admin/rpc/device.delete", post(handlers::device_delete))
        .route("/admin/fragments/pairing-codes", get(handlers::pairing_codes_fragment))
        .route("/admin/fragments/pairing-badge", get(handlers::pairing_badge_fragment))
        .route("/admin/rpc/pairing.revoke", post(handlers::pairing_code_revoke))
        .route("/admin/rpc/pairing.delete_code", post(handlers::pairing_code_delete))
        .route("/admin/rpc/schedule.save", post(handlers::schedule_save))
        .route("/admin/rpc/schedule.preset", post(handlers::schedule_preset))
        .route("/admin/rpc/schedule.remove", post(handlers::schedule_remove))
        .route("/admin/rpc/book.release", post(handlers::book_release))
        .route("/admin/rpc/book.unrelease", post(handlers::book_unrelease))
        .route("/admin/rpc/book.set_exempt", post(handlers::book_set_exempt))
        .route("/admin/rpc/book.delete", post(handlers::book_delete))
        .route("/admin/rpc/book.visibility", post(handlers::book_visibility))
        .route("/admin/rpc/book.owner", post(handlers::book_owner))
        .route("/admin/rpc/folders", get(handlers::folder_picker))
        .route("/admin/rpc/folders.create", post(handlers::folder_create))
        .route("/admin/rpc/book.tags_form", get(handlers::book_tags_form))
        .route("/admin/rpc/book.filing", get(handlers::book_filing))
        .route("/admin/rpc/label.suggest", get(handlers::label_suggest))
        .route("/admin/rpc/book.tracks", get(handlers::book_tracks))
        .route("/admin/rpc/book.track_labels", get(handlers::book_track_labels))
        .route("/admin/rpc/book.tags", post(handlers::book_tags).layer(DefaultBodyLimit::max(crate::tags::MAX_COVER_BYTES + 64 * 1024)))
        .route("/admin/rpc/import.save_criteria", post(handlers::import_save))
        .route("/admin/rpc/import.row", get(handlers::import_row))
        .route("/admin/rpc/import.unlock", post(handlers::import_unlock))
        .route("/admin/rpc/import.lock", post(handlers::import_lock))
        .route("/admin/rpc/import.run", post(handlers::import_run))
        .route("/admin/rpc/upload.create", post(handlers::upload_create).layer(DefaultBodyLimit::max(crate::upload::MAX_UPLOAD_BYTES as usize + 1024 * 1024)))
        .route("/admin/rpc/upload.files", post(handlers::upload_files).layer(DefaultBodyLimit::max(crate::upload::MAX_UPLOAD_BYTES as usize + 1024 * 1024)))
        .route("/admin/rpc/upload.list", get(handlers::upload_list))
        .route("/admin/rpc/settings.save", post(handlers::settings_save))
        .route("/admin/rpc/settings.cd", post(handlers::cd_settings_save))
        .route("/admin/rpc/cd.drive", get(handlers::cd_drive_panel))
        .route("/admin/rpc/cd.rip", post(handlers::cd_rip))
        .route("/admin/rpc/cd.preview", get(handlers::cd_preview))
        .route("/admin/rpc/cd.cancel", post(handlers::cd_cancel))
        .route("/admin/rpc/cd.dismiss", post(handlers::cd_dismiss))
        .route("/admin/rpc/cd.log_clear", post(handlers::cd_log_clear))
        .route("/admin/rpc/cd.eject", post(handlers::cd_eject))
        .route("/admin/rpc/mcp.token_create", post(handlers::mcp_token_create))
        .route("/admin/rpc/mcp.prompt_save", post(handlers::mcp_prompt_save))
        .route("/admin/rpc/access.check", post(handlers::access_check))
        .route("/admin/rpc/ignore.add", post(handlers::ignore_add))
        .route("/admin/rpc/quarantine.add", post(handlers::quarantine_add))
        .route("/admin/rpc/ignore.remove", post(handlers::ignore_remove))
        .route("/admin/rpc/mcp.token_revoke", post(handlers::mcp_token_revoke))
        .route("/admin/rpc/policies.save", post(handlers::policies_save))
        .route("/profile", get(handlers::profile_page).post(handlers::profile_save).layer(DefaultBodyLimit::max(AVATAR_BODY)))
        .route("/profile/pin", post(handlers::profile_pin))
        .route("/profile/names", post(handlers::profile_names))
        .route("/profile/language", post(handlers::profile_language))
        .route("/profile/avatar", post(handlers::profile_avatar).layer(DefaultBodyLimit::max(AVATAR_BODY)))
        .route("/profile/avatar.remove", post(handlers::profile_avatar_remove))
        .route("/avatar/{id}", get(handlers::avatar_image))
        .route("/admin/static/admin.css", get(handlers::stylesheet))
        .route("/admin/static/ui.js", get(handlers::ui_js))
        .route("/admin/static/crop.js", get(handlers::crop_js))
        .route("/admin/static/chips.js", get(handlers::chips_js))
        .route("/admin/static/fields.js", get(handlers::fields_js))
        .route("/admin/static/logo.svg", get(handlers::logo))
        .route("/admin/static/favicon.svg", get(handlers::favicon))
        .route("/admin/static/htmx.min.js", get(handlers::htmx))
        .layer(middleware::from_fn(crate::actor::scope))
        .layer(middleware::from_fn(security_headers))
        .layer(middleware::from_fn(csrf_check))
        .with_state(state)
}

/// Is this request made by HTMX? Then the answer is a fragment, not a page.
pub fn is_htmx(headers: &HeaderMap) -> bool {
    headers.get("hx-request").is_some_and(|v| v == "true")
}

pub fn lang_of(headers: &HeaderMap) -> Lang {
    headers
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|v| v.to_str().ok())
        .map_or(Lang::En, Lang::from_accept_language)
}

/// Every value of the cookies of this name. A browser can send two: the one of an
/// older version of David (scoped to `/admin`) and the current one (scoped to `/`).
fn cookies(headers: &HeaderMap, name: &str) -> Vec<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .filter(|(k, _)| *k == name)
        .map(|(_, v)| v.to_owned())
        .collect()
}

/// The value of one cookie of the request.
pub(crate) fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_owned())
}

/// Headers on every response. The policy allows scripts and styles only from this
/// server, forbids framing, and keeps pages out of caches.
async fn security_headers(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; \
             connect-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'",
        ),
    );
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    if !h.contains_key(header::CACHE_CONTROL) {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    res
}

/// Refuses a state-changing request of a signed-in browser unless it carries the
/// CSRF token of its session, in the header `X-CSRF-Token` (HTMX) or in the form
/// field `csrf` (plain forms). Signing in is exempt, because there is no session
/// yet; the cookie is `SameSite=Strict`, which covers that case.
async fn csrf_check(req: Request, next: Next) -> Response {
    let needs_check = req.method() == Method::POST && req.uri().path() != "/admin/login";
    let session_tokens = cookies(req.headers(), COOKIE);
    if !needs_check || session_tokens.is_empty() {
        return next.run(req).await;
    }
    // The token of any of the session cookies of the request is good (see `cookies`).
    let expected: Vec<String> = session_tokens.iter().map(|t| csrf_token(t)).collect();
    let valid = |given: &str| expected.iter().any(|e| same_secret(given, e));
    let htmx = is_htmx(req.headers());
    let lang = lang_of(req.headers());
    let refuse = || handlers::plain_failure(StatusCode::FORBIDDEN, "err.csrf", lang, htmx);

    let given_header = req.headers().get("x-csrf-token").and_then(|v| v.to_str().ok()).map(str::to_owned);
    // An upload cannot be read here without holding the whole file twice; its form
    // carries the token in the address instead (`?csrf=`).
    let multipart = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("multipart/form-data"));
    if multipart {
        let in_query = req.uri().query().and_then(|q| {
            serde_urlencoded::from_str::<Vec<(String, String)>>(q).ok()?.into_iter().find(|(k, _)| k == "csrf").map(|(_, v)| v)
        });
        let given = given_header.or(in_query);
        return if given.is_some_and(|g| valid(&g)) { next.run(req).await } else { refuse() };
    }

    let (parts, body) = req.into_parts();
    let Ok(bytes) = to_bytes(body, MAX_BODY).await else {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    let given = given_header.or_else(|| {
        serde_urlencoded::from_bytes::<Vec<(String, String)>>(&bytes)
            .ok()?
            .into_iter()
            .find(|(k, _)| k == "csrf")
            .map(|(_, v)| v)
    });
    if !given.is_some_and(|g| valid(&g)) {
        return refuse();
    }
    next.run(Request::from_parts(parts, Body::from(bytes))).await
}

/// Who is signed in, for the header of every page: name, picture, role.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Me {
    pub id: i64,
    pub name: String,
    pub role: String,
    pub has_avatar: bool,
    /// The first letter of the name, shown when there is no picture.
    pub initial: String,
    /// 0 to 7: which of the colours a user without a picture gets.
    pub tone: u8,
}

impl Me {
    pub fn new(id: i64, name: &str, role: Role, has_avatar: bool) -> Me {
        let tone = (name.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b))) % 8) as u8;
        Me { id, name: name.to_owned(), role: role.as_str().to_owned(), has_avatar, initial: name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default(), tone }
    }
}

/// A signed-in user of any role. Extracting it checks the session cookie.
pub struct Session {
    pub user_id: i64,
    pub role: Role,
    /// The CSRF token of the session, for the pages to embed.
    pub csrf: String,
    pub token: String,
    pub lang: Lang,
    pub htmx: bool,
    pub me: Me,
}

/// A signed-in administrator. Extracting it checks the session cookie and the role.
pub struct AdminSession(pub Session);

impl std::ops::Deref for AdminSession {
    type Target = Session;
    fn deref(&self) -> &Session {
        &self.0
    }
}

/// Why a request has no administrator session.
pub enum Denied {
    /// Not signed in: go to the sign-in page.
    /// `next` is where to go after signing in, for pages outside `/admin`.
    SignIn { htmx: bool, next: Option<&'static str> },
    /// Signed in, but not as an administrator.
    Forbidden { lang: Lang, htmx: bool },
}

impl IntoResponse for Denied {
    fn into_response(self) -> Response {
        match self {
            Denied::SignIn { htmx: true, .. } => {
                (StatusCode::UNAUTHORIZED, [("hx-redirect", "/admin/login")]).into_response()
            }
            Denied::SignIn { htmx: false, next: Some(next) } => {
                Redirect::to(&format!("/admin/login?next={}", next.replace('/', "%2F"))).into_response()
            }
            Denied::SignIn { htmx: false, next: None } => Redirect::to("/admin/login").into_response(),
            Denied::Forbidden { lang, htmx } => handlers::plain_failure(StatusCode::FORBIDDEN, "err.forbidden", lang, htmx),
        }
    }
}

impl FromRequestParts<AppState> for Session {
    type Rejection = Denied;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Denied> {
        let htmx = is_htmx(&parts.headers);
        let mut lang = lang_of(&parts.headers);
        // After signing in, the person goes back to the page they asked for, if it lies outside
        // /admin (the profile): the cookie of an older version reaches only /admin/*.
        let next = (parts.uri.path() == "/profile").then_some("/profile");
        // The first of the session cookies that holds a live session counts.
        let mut found = None;
        for token in cookies(&parts.headers, COOKIE) {
            if let Ok(Some(info)) = state.store.session_for_token(&token).await {
                found = Some((token, info));
                break;
            }
        }
        let Some((token, info)) = found else {
            return Err(Denied::SignIn { htmx, next });
        };
        let Ok(Some(profile)) = state.store.profile_by_id(info.user_id).await else {
            return Err(Denied::SignIn { htmx, next });
        };
        // The language chosen in the profile beats the one of the browser.
        if let Some(chosen) = profile.language.as_deref().and_then(Lang::from_code) {
            lang = chosen;
        }
        crate::actor::set(crate::actor::profile(info.user_id));
        let has_avatar = state.store.has_avatar(info.user_id).await.unwrap_or(false);
        let me = Me::new(info.user_id, &profile.name, info.role, has_avatar);
        Ok(Session { user_id: info.user_id, role: info.role, csrf: csrf_token(&token), token, lang, htmx, me })
    }
}

impl FromRequestParts<AppState> for AdminSession {
    type Rejection = Denied;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Denied> {
        let session = Session::from_request_parts(parts, state).await?;
        if session.role != Role::Admin {
            return Err(Denied::Forbidden { lang: session.lang, htmx: session.htmx });
        }
        Ok(AdminSession(session))
    }
}
