// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The interface of the apps: JSON over HTTP under `/api/v1`.
//!
//! A device pairs once with the code from the QR code (`POST /pair`) and gets a
//! token, which it sends as `Authorization: Bearer <token>` afterwards. The token
//! is the session of the administration interface, only with a device behind it;
//! revoking the device ends it.
//!
//! | Request | Answer |
//! |---|---|
//! | `POST /api/v1/pair` | token, device and user |
//! | `GET /api/v1/me`, `POST /api/v1/logout` | the user; ends the session |
//! | `GET /api/v1/items` | the items (books, ...) the user may see |
//! | `GET /api/v1/items/{id}` | parts, the blurb (`description`), progress and whether it may be used now |
//! | `GET /api/v1/items/{id}/cover` | the product picture: `cover.jpg` (or `.png`, `folder.jpg`) in the item's folder, else the picture embedded in its first file; `404` if there is none |
//! | `GET /api/v1/continue` | unfinished items, latest first |
//! | `GET /api/v1/parts/{id}/stream` | the file of a part, with `Range` support |
//! | `PUT /api/v1/progress` | stores the listening position |
//! | `GET /api/v1/bible/resolve?q=` | where an audio Bible opens a passage, e.g. `q=Johannes 3,16` |
//!
//! An item is a book, an album, a film or a document (`kind`); a part is one file of
//! it (a track, an episode, the PDF). Today every item is an `audiobook`; see
//! `docs/spec-media-kinds.md`. An item that the user may not see answers `404`, as if it did not exist. Outside
//! the weekly planner the stream answers `403` with `outside_schedule` and the next
//! allowed slot. Errors are `{"error": "<code>"}`.

use std::path::{Path, PathBuf};

use axum::Router;
use axum::extract::{FromRequestParts, Path as UrlPath, Query, Request, State};
use axum::http::request::Parts;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::Json;
use sea_orm::EntityTrait;
use serde::Deserialize;
use serde_json::{Value, json};
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::entities::{book, track};
use crate::error::Error;
use crate::roles::Role;
use crate::store::{Access, Store, now_ms};
use crate::writer::ProgressUpdate;

/// The only kind the library holds so far.
const KIND: &str = "audiobook";

#[derive(Clone)]
pub struct ApiState {
    pub store: Store,
    /// The development library folder (`--library`); the import folders of the
    /// database are searched as well.
    pub library: Option<PathBuf>,
    /// The CD import: the way to MusicBrainz, the staging folder and the channel to the devices.
    pub cd: crate::cdimport::CdService,
}

pub fn router(state: ApiState) -> Router {
    Router::new()
        .route("/api/v1/pair", post(pair))
        .route("/api/v1/devices/pairing", get(pairing_usage).post(pairing_code))
        .route("/api/v1/me", get(me))
        .route("/api/v1/me/library", get(my_library).put(put_my_library))
        .route("/api/v1/me/hidden", post(set_hidden))
        .route("/api/v1/logout", post(logout))
        .route("/api/v1/library/version", get(library_version))
        .route("/api/v1/items", get(items))
        .route("/api/v1/genres", get(genres))
        .route("/api/v1/items/{id}", get(item_detail))
        .route("/api/v1/items/{id}/labels", get(item_labels).put(put_item_labels).patch(patch_item_labels))
        .route("/api/v1/items/{id}/parts/{part}/labels", put(put_part_labels).patch(patch_part_labels))
        .route("/api/v1/labels", get(all_labels))
        .route("/api/v1/labels/{name}", put(rename_label).delete(delete_label))
        .route("/api/v1/new", get(new_items))
        .route("/api/v1/items/{id}/cover", get(item_cover))
        .route("/api/v1/continue", get(continue_listening))
        .route("/api/v1/parts/{id}/stream", get(stream))
        .route("/api/v1/progress", put(put_progress))
        .route("/api/v1/bible/resolve", get(bible_resolve))
        .merge(crate::api_feeds::routes())
        .merge(crate::api_me::routes())
        .merge(crate::api_playlists::routes())
        .merge(crate::api_cd::routes())
        .layer(axum::middleware::from_fn(crate::actor::scope))
        .with_state(state)
}

/// An error answer: a status and a code the app can switch on.
pub struct ApiError(pub(crate) StatusCode, pub(crate) &'static str);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, [(header::CACHE_CONTROL, "no-store")], Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        match e {
            Error::EnrollmentInvalid => ApiError(StatusCode::FORBIDDEN, "enrollment_invalid"),
            Error::InvalidPublicKey => ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_public_key"),
            Error::InvalidDeviceName => ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_device_name"),
            Error::DeviceLimitReached => ApiError(StatusCode::CONFLICT, "device_limit_reached"),
            e => {
                tracing::error!(error = %e, "api request failed");
                ApiError(StatusCode::INTERNAL_SERVER_ERROR, "internal")
            }
        }
    }
}

pub(crate) type ApiResult<T> = Result<T, ApiError>;

/// A signed-in device. Extracting it checks the bearer token.
pub struct Device {
    pub user_id: i64,
    pub role: Role,
    pub(crate) token: String,
}

impl FromRequestParts<ApiState> for Device {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &ApiState) -> ApiResult<Self> {
        let unauthorized = || ApiError(StatusCode::UNAUTHORIZED, "unauthorized");
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or_else(unauthorized)?;
        let info = state.store.session_for_token(token).await?.ok_or_else(unauthorized)?;
        crate::actor::set(crate::actor::profile(info.user_id));
        Ok(Device { user_id: info.user_id, role: info.role, token: token.to_owned() })
    }
}

#[derive(Deserialize)]
struct PairRequest {
    code: String,
    device_name: String,
    /// The WireGuard public key of the device (32 bytes, base64). The tunnel is not
    /// built yet; until then the app sends a random key.
    public_key: String,
}

async fn pair(State(s): State<ApiState>, Json(req): Json<PairRequest>) -> ApiResult<Json<Value>> {
    let paired = s.store.redeem_enrollment(req.code.trim(), &req.public_key, &req.device_name).await?;
    let user = s
        .store
        .profile_by_id(paired.user_id)
        .await?
        .ok_or(ApiError(StatusCode::INTERNAL_SERVER_ERROR, "internal"))?;
    let role: Role = user.role.parse().map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "internal"))?;
    crate::actor::set(crate::actor::profile(user.id));
    let token = s.store.create_session(user.id, role, Some(paired.device_id)).await?;
    Ok(Json(json!({
        "token": token,
        "device_id": paired.device_id,
        "shared": paired.shared,
        "user": { "id": user.id, "name": user.name, "role": role.as_str() },
    })))
}

/// Only a parent (an administrator or a member) connects a further device of the own profile: a child or a guest does not.
fn may_pair(d: &Device) -> ApiResult<()> {
    matches!(d.role, Role::Admin | Role::Member).then_some(()).ok_or(ApiError(StatusCode::FORBIDDEN, "forbidden"))
}

/// How many devices the own profile has and how many it may have: `{used, max, full}` (`max` is `null` without a limit). The app shows the button
/// "connect a further device" only when this answers, and says so when `full`.
async fn pairing_usage(State(s): State<ApiState>, d: Device) -> ApiResult<Json<Value>> {
    may_pair(&d)?;
    let (used, max) = s.store.device_usage(d.user_id).await?;
    Ok(Json(json!({ "used": used, "max": max, "full": max.is_some_and(|m| used >= m) })))
}

/// A one-time code for a further device of the own profile (the app makes the QR code of it, with the address it uses itself). The same check as
/// in the administration applies: with as many devices as allowed there is no code (`409 device_limit_reached`).
async fn pairing_code(State(s): State<ApiState>, d: Device) -> ApiResult<Json<Value>> {
    may_pair(&d)?;
    let e = s.store.create_enrollment(d.user_id, false, d.user_id).await?;
    let (used, max) = s.store.device_usage(d.user_id).await?;
    Ok(Json(json!({
        "code": e.code,
        "server_id": s.store.server_id().await?,
        "expires_at": e.expires_at,
        "expires_in_s": ((e.expires_at - crate::store::now_ms()) / 1000).max(0),
        "used": used,
        "max": max,
    })))
}

async fn me(State(s): State<ApiState>, d: Device) -> ApiResult<Json<Value>> {
    Ok(Json(crate::api_me::me_json(&s, &d).await?))
}

async fn logout(State(s): State<ApiState>, d: Device) -> ApiResult<StatusCode> {
    s.store.delete_session(&d.token).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// A short value that changes whenever the library changes (a new CD, a release, a blurb). The
/// apps ask for it now and then and reload the library when it differs from the last one.
///
/// `playlists` is the same for the lists of the calling profile only (see `docs/spec-playlists.md`), so a change of one
/// profile's playlists does not make the devices of the others reload.
async fn library_version(State(s): State<ApiState>, d: Device) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "version": s.store.library_version().await?, "playlists": s.store.playlists_version(d.user_id).await?, "hidden": s.store.hidden_version(d.user_id).await? })))
}

/// The items of the lists of this user: what they hid is left out, or (with the setting `show_hidden`) listed and marked `hidden`.
async fn items(State(s): State<ApiState>, d: Device) -> ApiResult<Json<Value>> {
    let rows = s.store.visible_books(d.user_id).await?;
    let hidden = s.store.hidden_of(d.user_id).await?;
    let ids = hidden.ids_in(&rows);
    let labels = s.store.book_labels().await?;
    // The labels that single titles of an item carry: the item is found by them too ("Items list": `track_labels`).
    let track_labels = s.store.track_labels_by_book().await?;
    // The system mark "Blacklist" is seen by administrators only.
    let marked = if d.role == Role::Admin { s.store.blacklist_marked(crate::blacklist::Kind::Book).await? } else { Default::default() };
    // Whether this user may set the labels of a book: its owner (a member) and an administrator, whoever owns it.
    let me = s.store.profile_by_id(d.user_id).await?.map(|u| u.name);
    Ok(Json(Value::Array(
        rows.iter()
            .filter(|b| hidden.show_hidden || !ids.contains(&b.id))
            .map(|b| {
                let mut j = item_json(b);
                j["hidden"] = json!(ids.contains(&b.id));
                j["owner"] = json!(b.owner);
                j["labels"] = json!(crate::blacklist::with_mark(labels.get(&b.id).cloned().unwrap_or_default(), marked.contains(&b.id), d.role == Role::Admin));
                j["track_labels"] = json!(track_labels.get(&b.id).cloned().unwrap_or_default());
                // Only a parent (an administrator or a member) passes content on; a child or a guest does not.
                j["can_share"] = json!(matches!(d.role, Role::Admin | Role::Member));
                j["can_label"] = json!(d.role == Role::Admin || (d.role == Role::Member && me.is_some() && b.owner == me));
                j
            })
            .collect(),
    )))
}


/// What this user hid, and their setting: `{ "show_hidden": bool, "books": [{id, title, author}], "authors": [{id, name, kind}] }`.
async fn my_library(State(s): State<ApiState>, d: Device) -> ApiResult<Json<Value>> {
    Ok(Json(json!(s.store.hidden_of(d.user_id).await?)))
}

#[derive(serde::Deserialize)]
struct LibrarySettings {
    show_hidden: bool,
}

/// The setting of the user: whether the lists still carry what they hid.
async fn put_my_library(State(s): State<ApiState>, d: Device, Json(req): Json<LibrarySettings>) -> ApiResult<Json<Value>> {
    s.store.set_show_hidden(d.user_id, req.show_hidden).await?;
    Ok(Json(json!(s.store.hidden_of(d.user_id).await?)))
}

#[derive(serde::Deserialize)]
struct HideRequest {
    /// `book` or `author`.
    scope: String,
    /// The id of the book (`scope` = book).
    #[serde(default)]
    id: Option<i64>,
    /// The name of the author or artist (`scope` = author).
    #[serde(default)]
    name: Option<String>,
    /// For an author: `audiobook`, `music`, or absent for both.
    #[serde(default)]
    kind: Option<String>,
    hidden: bool,
}

/// Hides a book or the books of an author for this user, or shows them again.
async fn set_hidden(State(s): State<ApiState>, d: Device, Json(req): Json<HideRequest>) -> ApiResult<Json<Value>> {
    let found = match req.scope.as_str() {
        "book" => {
            let id = req.id.ok_or(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_request"))?;
            s.store.set_book_hidden(d.user_id, id, req.hidden).await?
        }
        "author" => {
            let name = req.name.as_deref().map(str::trim).filter(|n| !n.is_empty() && n.chars().count() <= 200).ok_or(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_request"))?;
            let kind = req.kind.as_deref().unwrap_or("");
            if !matches!(kind, "" | "audiobook" | "music") {
                return Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_request"));
            }
            s.store.set_author_hidden(d.user_id, name, kind, req.hidden).await?
        }
        _ => return Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_request")),
    };
    if !found {
        return Err(ApiError(StatusCode::NOT_FOUND, "not_found"));
    }
    Ok(Json(json!(s.store.hidden_of(d.user_id).await?)))
}

/// The genres of the list that the apps show, in the order the administrator gave them.
async fn genres(State(s): State<ApiState>, _d: Device) -> ApiResult<Json<Value>> {
    let list = s.store.genres().await?;
    Ok(Json(Value::Array(list.iter().filter(|g| g.visible).map(|g| json!({ "name": g.name })).collect())))
}

/// The labels of the library (classic tagging): `{ "labels": [{ "name": "Gebet", "books": 3, "podcasts": 1, "stations": 0, "count": 4 }], "can_manage": bool }`.
/// Only what this user sees counts (books, the podcasts they follow, their favourite stations); an item counts once for a label, and `count` adds the
/// kinds up. An administrator may rename and delete labels (`can_manage`).
async fn all_labels(State(s): State<ApiState>, d: Device) -> ApiResult<Json<Value>> {
    let visible: std::collections::HashSet<i64> = s.store.visible_books(d.user_id).await?.iter().map(|b| b.id).collect();
    // The name as first spelled, and the numbers of books, podcasts and stations.
    let mut counts: std::collections::BTreeMap<String, (String, [i64; 3])> = std::collections::BTreeMap::new();
    let mut count = |names: Vec<String>, kind: usize| {
        let mut seen = std::collections::HashSet::new();
        for n in names {
            if seen.insert(n.to_lowercase()) {
                counts.entry(n.to_lowercase()).or_insert((n, [0; 3])).1[kind] += 1;
            }
        }
    };
    // A book counts once for a label, whether the book or one of its titles carries it.
    let mut per_book = s.store.book_labels().await?;
    for (id, names) in s.store.track_labels_by_book().await? {
        per_book.entry(id).or_default().extend(names);
    }
    for (id, names) in per_book {
        if visible.contains(&id) {
            count(names, 0);
        }
    }
    let podcast_labels = s.store.podcast_labels().await?;
    for p in s.store.subscriptions(d.user_id, true).await? {
        count(podcast_labels.get(&p.id).cloned().unwrap_or_default(), 1);
    }
    let station_labels = s.store.station_labels().await?;
    for st in s.store.favorite_stations(d.user_id, true).await? {
        count(station_labels.get(&st.id).cloned().unwrap_or_default(), 2);
    }
    let mut labels: Vec<Value> = counts
        .into_values()
        .filter(|(_, n)| n.iter().sum::<i64>() > 0)
        .map(|(name, n)| json!({ "name": name, "books": n[0], "podcasts": n[1], "stations": n[2], "count": n.iter().sum::<i64>() }))
        .collect();
    // The system mark of the blacklist: administrators see it with the number of media that carry it.
    if d.role == Role::Admin {
        use crate::blacklist::Kind;
        let n = [s.store.blacklist_marked(Kind::Book).await?.len() as i64, s.store.blacklist_marked(Kind::Podcast).await?.len() as i64, s.store.blacklist_marked(Kind::Station).await?.len() as i64];
        if n.iter().sum::<i64>() > 0 {
            labels.insert(0, json!({ "name": crate::blacklist::MARK, "books": n[0], "podcasts": n[1], "stations": n[2], "count": n.iter().sum::<i64>() }));
        }
    }
    Ok(Json(json!({ "labels": labels, "can_manage": d.role == Role::Admin })))
}

#[derive(serde::Deserialize)]
struct RenameRequest {
    name: String,
}

/// Renames a label for every book that carries it; if the new name exists the two become one. Administrators only.
async fn rename_label(State(s): State<ApiState>, d: Device, UrlPath(old): UrlPath<String>, Json(req): Json<RenameRequest>) -> ApiResult<Json<Value>> {
    if d.role != Role::Admin {
        return Err(ApiError(StatusCode::FORBIDDEN, "forbidden"));
    }
    match s.store.rename_label(&old, &req.name).await {
        Ok(true) => Ok(Json(json!({ "name": crate::labels::clean_name(&req.name) }))),
        Ok(false) => Err(ApiError(StatusCode::NOT_FOUND, "not_found")),
        Err(Error::InvalidName) => Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_label")),
        Err(e) => Err(e.into()),
    }
}

/// Takes a label off every book. Administrators only.
async fn delete_label(State(s): State<ApiState>, d: Device, UrlPath(name): UrlPath<String>) -> ApiResult<StatusCode> {
    if d.role != Role::Admin {
        return Err(ApiError(StatusCode::FORBIDDEN, "forbidden"));
    }
    if s.store.delete_label(&name).await? { Ok(StatusCode::NO_CONTENT) } else { Err(ApiError(StatusCode::NOT_FOUND, "not_found")) }
}

/// The labels of a book this user sees.
async fn item_labels(State(s): State<ApiState>, d: Device, UrlPath(id): UrlPath<i64>) -> ApiResult<Json<Value>> {
    if !s.store.visible_books(d.user_id).await?.iter().any(|b| b.id == id) {
        return Err(ApiError(StatusCode::NOT_FOUND, "not_found"));
    }
    Ok(Json(json!({ "labels": shown_labels(&s, &d, crate::blacklist::Kind::Book, id, s.store.labels_of(id).await?).await? })))
}

/// The labels as this user sees them: an administrator sees the system mark of the blacklist on a marked medium, nobody else does.
pub(crate) async fn shown_labels(s: &ApiState, d: &Device, kind: crate::blacklist::Kind, id: i64, labels: Vec<String>) -> ApiResult<Vec<String>> {
    if d.role != Role::Admin {
        return Ok(labels);
    }
    let marked = s.store.blacklist_marked(kind).await?.contains(&id);
    Ok(crate::blacklist::with_mark(labels, marked, true))
}

/// An administrator who took the mark off the labels of a marked medium approves it: the next check does not put it on again.
pub(crate) async fn approve_if_removed(s: &ApiState, d: &Device, kind: crate::blacklist::Kind, id: i64, removed: bool) -> ApiResult<()> {
    if removed && d.role == Role::Admin {
        s.store.blacklist_approve(kind, id).await?;
    }
    Ok(())
}

#[derive(serde::Deserialize)]
struct LabelsRequest {
    labels: Vec<String>,
}

/// Labels to add and to take off (a change that was made on a device without the server and is sent later: the labels others set in the meantime stay).
#[derive(Deserialize)]
struct LabelsDelta {
    #[serde(default)]
    add: Vec<String>,
    #[serde(default)]
    remove: Vec<String>,
}

/// Who may change the labels of a book (and of its titles): the owner and an administrator; 404 for a book the user does not see.
async fn may_label_book(s: &ApiState, d: &Device, id: i64) -> ApiResult<()> {
    if !s.store.visible_books(d.user_id).await?.iter().any(|b| b.id == id) {
        return Err(ApiError(StatusCode::NOT_FOUND, "not_found"));
    }
    let row = s.store.admin_book_row(id).await?.ok_or(ApiError(StatusCode::NOT_FOUND, "not_found"))?;
    if d.role == Role::Admin || (d.role == Role::Member && row.owner_id == Some(d.user_id)) {
        Ok(())
    } else {
        Err(ApiError(StatusCode::FORBIDDEN, "forbidden"))
    }
}

/// Adds and removes labels of a book (`{ "add": [], "remove": [] }`); the others stay. Same permission as the PUT. Sending the same change twice does no harm.
async fn patch_item_labels(State(s): State<ApiState>, d: Device, UrlPath(id): UrlPath<i64>, Json(req): Json<LabelsDelta>) -> ApiResult<Json<Value>> {
    may_label_book(&s, &d, id).await?;
    // The reserved name is no ordinary label: adding it does nothing, an administrator who removes it approves the medium.
    let ((add, _), (remove, removes)) = (crate::blacklist::split_mark(&req.add), crate::blacklist::split_mark(&req.remove));
    match s.store.change_book_labels(id, &add, &remove).await {
        Ok(Some(labels)) => {
            approve_if_removed(&s, &d, crate::blacklist::Kind::Book, id, removes).await?;
            Ok(Json(json!({ "labels": shown_labels(&s, &d, crate::blacklist::Kind::Book, id, labels).await? })))
        }
        Ok(None) => Err(ApiError(StatusCode::NOT_FOUND, "not_found")),
        Err(Error::InvalidName) => Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_label")),
        Err(e) => Err(e.into()),
    }
}

/// The same for one title (part) of a book.
async fn patch_part_labels(State(s): State<ApiState>, d: Device, UrlPath((id, part)): UrlPath<(i64, i64)>, Json(req): Json<LabelsDelta>) -> ApiResult<Json<Value>> {
    if s.store.book_of_track(part).await? != Some(id) {
        return Err(ApiError(StatusCode::NOT_FOUND, "not_found"));
    }
    may_label_book(&s, &d, id).await?;
    let ((add, _), (remove, _)) = (crate::blacklist::split_mark(&req.add), crate::blacklist::split_mark(&req.remove));
    match s.store.change_track_labels(part, &add, &remove).await {
        Ok(Some(labels)) => Ok(Json(json!({ "labels": labels }))),
        Ok(None) => Err(ApiError(StatusCode::NOT_FOUND, "not_found")),
        Err(Error::InvalidName) => Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_label")),
        Err(e) => Err(e.into()),
    }
}

/// Sets the labels of a book (all of them at once). The owner of the book may, and an administrator may always (project owner, 2026-10-30:
/// "individual for one's own content, the administrator can override"). A child or a guest may not. Everybody who sees the book sees its labels.
async fn put_item_labels(State(s): State<ApiState>, d: Device, UrlPath(id): UrlPath<i64>, Json(req): Json<LabelsRequest>) -> ApiResult<Json<Value>> {
    if !s.store.visible_books(d.user_id).await?.iter().any(|b| b.id == id) {
        return Err(ApiError(StatusCode::NOT_FOUND, "not_found"));
    }
    let row = s.store.admin_book_row(id).await?.ok_or(ApiError(StatusCode::NOT_FOUND, "not_found"))?;
    let allowed = d.role == Role::Admin || (d.role == Role::Member && row.owner_id == Some(d.user_id));
    if !allowed {
        return Err(ApiError(StatusCode::FORBIDDEN, "forbidden"));
    }
    // An administrator who sends the list of a marked medium without the mark approves it; the mark is never set by a list.
    let (names, wants_mark) = crate::blacklist::split_mark(&req.labels);
    match s.store.set_book_labels(id, &names).await {
        Ok(Some(labels)) => {
            approve_if_removed(&s, &d, crate::blacklist::Kind::Book, id, !wants_mark).await?;
            Ok(Json(json!({ "labels": shown_labels(&s, &d, crate::blacklist::Kind::Book, id, labels).await? })))
        }
        Ok(None) => Err(ApiError(StatusCode::NOT_FOUND, "not_found")),
        Err(Error::InvalidName) => Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_label")),
        Err(e) => Err(e.into()),
    }
}

/// Sets the labels of one title (part) of a book, all of its own at once; the labels of the album stay as they are. Same permission as for the book:
/// its owner and an administrator. `part` is the id of the part as the item lists it.
async fn put_part_labels(State(s): State<ApiState>, d: Device, UrlPath((id, part)): UrlPath<(i64, i64)>, Json(req): Json<LabelsRequest>) -> ApiResult<Json<Value>> {
    if !s.store.visible_books(d.user_id).await?.iter().any(|b| b.id == id) || s.store.book_of_track(part).await? != Some(id) {
        return Err(ApiError(StatusCode::NOT_FOUND, "not_found"));
    }
    let row = s.store.admin_book_row(id).await?.ok_or(ApiError(StatusCode::NOT_FOUND, "not_found"))?;
    let allowed = d.role == Role::Admin || (d.role == Role::Member && row.owner_id == Some(d.user_id));
    if !allowed {
        return Err(ApiError(StatusCode::FORBIDDEN, "forbidden"));
    }
    let (names, _) = crate::blacklist::split_mark(&req.labels);
    match s.store.set_track_labels(part, &names).await {
        Ok(Some(labels)) => Ok(Json(json!({ "labels": labels }))),
        Ok(None) => Err(ApiError(StatusCode::NOT_FOUND, "not_found")),
        Err(Error::InvalidName) => Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_label")),
        Err(e) => Err(e.into()),
    }
}

fn item_json(b: &crate::store::BookRow) -> Value {
    json!({
        "id": b.id, "kind": b.kind, "title": b.title, "creator": b.author, "narrator": b.narrator, "genre": b.genre, "min_age": b.min_age,
        "recommended_age": b.recommended_age, "part_count": b.tracks, "discs": b.discs,
        "duration_ms": b.duration_ms,
    })
}

/// What is new for the user: the items that became visible to them in the last `new_days` days and that they have not
/// started, newest first. The apps show them on the start page.
async fn new_items(State(s): State<ApiState>, d: Device) -> ApiResult<Json<Value>> {
    let days = s.store.new_days().await?;
    let since = now_ms() - days * 24 * 3_600_000;
    let mut items = s.store.new_books(d.user_id, since, NEW_ITEMS_MAX).await?;
    // What this user hid is not news to them (unless they chose to see it).
    let hidden = s.store.hidden_of(d.user_id).await?;
    if !hidden.show_hidden {
        let ids = hidden.ids_in(&s.store.visible_books(d.user_id).await?);
        items.retain(|b| !ids.contains(&b.id));
    }
    Ok(Json(json!({ "days": days, "items": items })))
}

/// Most items the section "New" holds.
const NEW_ITEMS_MAX: i64 = 30;

async fn item_detail(State(s): State<ApiState>, d: Device, UrlPath(id): UrlPath<i64>) -> ApiResult<Json<Value>> {
    let (weekday, minute) = local_time(&s.store).await;
    let access = s.store.access(d.user_id, id, weekday, minute).await?;
    if access == Access::NotVisible {
        return Err(ApiError(StatusCode::NOT_FOUND, "not_found"));
    }
    let row = s
        .store
        .visible_books(d.user_id)
        .await?
        .into_iter()
        .find(|b| b.id == id)
        .ok_or(ApiError(StatusCode::NOT_FOUND, "not_found"))?;
    // What this user has heard to the end: per part, and for the whole item (complete plays, last played).
    let heard = s.store.part_plays(d.user_id, id).await?;
    // The labels of its own of each title (the ones of the album come with the item).
    let part_labels = s.store.track_labels().await?;
    let tracks: Vec<Value> = s
        .store
        .tracks_of(id)
        .await?
        .iter()
        .map(|t| {
            let h = heard.get(&t.id).copied().unwrap_or_default();
            json!({ "id": t.id, "disc": t.disc, "number": t.number, "title": t.title, "duration_ms": t.duration_ms, "plays": h.plays, "last_played_at": h.last_played_at, "labels": part_labels.get(&t.id).cloned().unwrap_or_default() })
        })
        .collect();
    let all: Vec<crate::plays::TrackPlays> = heard.values().copied().collect();
    let book_heard = crate::plays::book_plays(&all);
    let progress = s.store.progress_of(d.user_id, id).await?.map(|p| {
        json!({ "part_id": p.track_id, "position_ms": p.position_ms, "finished": p.finished, "updated_at": p.updated_at })
    });
    let description = book::Entity::find_by_id(id).one(&s.store.db.reader).await.map_err(Error::from)?.and_then(|b| b.description);
    let mut out = item_json(&row);
    out["description"] = description.map_or(Value::Null, Value::String);
    out["parts"] = Value::Array(tracks);
    out["plays"] = json!(book_heard.plays);
    out["last_played_at"] = json!(book_heard.last_played_at);
    out["progress"] = progress.unwrap_or(Value::Null);
    out["access"] = access_json(&access);
    Ok(Json(out))
}

fn access_json(access: &Access) -> Value {
    match access {
        Access::OutsideSchedule { next_start } => json!({
            "state": "outside_schedule",
            "next_start": next_start.map(|(weekday, minute)| json!({ "weekday": weekday, "minute": minute })),
        }),
        _ => json!({ "state": "allowed" }),
    }
}

#[derive(Deserialize)]
struct ResolveQuery {
    /// The passage as typed or dictated: "Johannes 3,16", "1 Cor 13:4-7", "Psalm 23".
    q: String,
    /// Only this item, if given.
    item: Option<i64>,
}

/// Opens a passage: the items the user may see that hold an audio Bible with an
/// anchor for it, each with the part and position to start at. The app sends the
/// text its speech recognizer heard; the server does the parsing, so every app
/// understands the same spellings.
async fn bible_resolve(State(s): State<ApiState>, d: Device, Query(q): Query<ResolveQuery>) -> ApiResult<Json<Value>> {
    let reference = crate::bible::parse(&q.q).ok_or(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_reference"))?;
    let (weekday, minute) = local_time(&s.store).await;
    let mut matches = Vec::new();
    for hit in s.store.resolve_bible(d.user_id, reference).await? {
        if q.item.is_some_and(|i| i != hit.book_id) {
            continue;
        }
        let access = s.store.access(d.user_id, hit.book_id, weekday, minute).await?;
        matches.push(json!({
            "item_id": hit.book_id, "kind": KIND, "title": hit.title, "part_id": hit.track_id,
            "position_ms": hit.position_ms, "matched_verse": hit.matched_verse, "exact": hit.exact,
            "access": access_json(&access),
        }));
    }
    Ok(Json(json!({ "reference": reference, "matches": matches })))
}

/// File names that hold the cover of a folder, in order of preference.
pub(crate) const COVER_FILES: [&str; 6] = ["cover.jpg", "cover.png", "folder.jpg", "folder.png", "front.jpg", "front.png"];

/// The cover of a book: a picture file in its folder, else the picture embedded in its
/// first file. Used by the interface of the apps and by the administration. The type is
/// decided by the bytes; only PNG, JPEG and WebP are ever returned.
pub(crate) async fn find_cover(store: &Store, library: Option<&Path>, id: i64) -> Option<(&'static str, Vec<u8>)> {
    let b = book::Entity::find_by_id(id).one(&store.db.reader).await.ok()??;
    let first = store.tracks_of(id).await.ok()?.into_iter().next()?;
    let file = resolve(store, library, &b.folder, &first.file).await?;
    // The folder of the book: the track may lie in a disc folder below it (`CD 1/01.mp3`).
    let depth = first.file.split('/').count();
    let book_dir = file.ancestors().nth(depth).map(Path::to_path_buf);
    tokio::task::spawn_blocking(move || {
        let folders: Vec<PathBuf> = book_dir.into_iter().chain(file.parent().map(Path::to_path_buf)).collect();
        folders.iter().find_map(|d| cover_in(d)).or_else(|| crate::tags::embedded_cover(&file))
    })
    .await
    .ok()?
}

/// How much a file name looks like the cover of its folder, lower is better; `None` if it does not.
/// Names are compared without regard to case (`Folder.jpg`), and the pictures that Windows Media
/// Player leaves behind count too (`AlbumArt_{GUID}_Large.jpg`, `AlbumArtSmall.jpg`, `cdcover.jpg`).
pub(crate) fn cover_rank(name: &str) -> Option<u8> {
    let lower = name.to_lowercase();
    let (stem, ext) = lower.rsplit_once('.')?;
    if !matches!(ext, "jpg" | "jpeg" | "png" | "webp") {
        return None;
    }
    match stem {
        "cover" => Some(0),
        "folder" => Some(1),
        "front" => Some(2),
        "cdcover" => Some(3),
        s if s.starts_with("albumart") && s.contains("large") => Some(4),
        s if s.starts_with("albumart") && s.contains("small") => Some(6),
        s if s.starts_with("albumart") => Some(5),
        _ => None,
    }
}

/// The best picture file of a folder that is a real picture of a sensible size.
pub(crate) fn cover_in(dir: &Path) -> Option<(&'static str, Vec<u8>)> {
    let mut candidates: Vec<(u8, PathBuf)> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter_map(|e| Some((cover_rank(&e.file_name().to_string_lossy())?, e.path())))
        .collect();
    candidates.sort();
    for (_, path) in candidates {
        // Only a real file inside the folder of the item.
        let Ok(real) = path.canonicalize() else { continue };
        if !real.starts_with(dir.canonicalize().ok()?) || !real.is_file() {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&real)
            && bytes.len() <= crate::tags::MAX_COVER_BYTES
            && let Some(mime) = crate::profile::sniff_image(&bytes)
        {
            return Some((mime, bytes));
        }
    }
    None
}

/// The product picture of an item. A picture file in its folder wins (it can be
/// replaced without touching the audio); otherwise the picture embedded in the
/// first file. Only PNG, JPEG and WebP are handed out, whatever the file says.
async fn item_cover(State(s): State<ApiState>, d: Device, UrlPath(id): UrlPath<i64>) -> ApiResult<Response> {
    let not_found = || ApiError(StatusCode::NOT_FOUND, "not_found");
    let (weekday, minute) = local_time(&s.store).await;
    if s.store.access(d.user_id, id, weekday, minute).await? == Access::NotVisible {
        return Err(not_found());
    }
    let found = find_cover(&s.store, s.library.as_deref(), id).await;
    let (mime, bytes) = found.ok_or_else(not_found)?;
    Ok(([(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, "private, max-age=3600")], bytes).into_response())
}

async fn continue_listening(State(s): State<ApiState>, d: Device) -> ApiResult<Json<Value>> {
    let mut rows = s.store.continue_listening(d.user_id, 20).await?;
    let hidden = s.store.hidden_of(d.user_id).await?;
    if !hidden.show_hidden {
        let ids = hidden.ids_in(&s.store.visible_books(d.user_id).await?);
        rows.retain(|c| !ids.contains(&c.book_id));
    }
    Ok(Json(Value::Array(
        rows.iter()
            .map(|c| {
                json!({ "item_id": c.book_id, "kind": KIND, "title": c.title, "creator": c.author,
                        "part_id": c.track_id, "position_ms": c.position_ms, "updated_at": c.updated_at })
            })
            .collect(),
    )))
}

async fn stream(State(s): State<ApiState>, d: Device, UrlPath(id): UrlPath<i64>, req: Request) -> ApiResult<Response> {
    let not_found = || ApiError(StatusCode::NOT_FOUND, "not_found");
    let t = track::Entity::find_by_id(id).one(&s.store.db.reader).await.map_err(Error::from)?.ok_or_else(not_found)?;
    let (weekday, minute) = local_time(&s.store).await;
    match s.store.access(d.user_id, t.book_id, weekday, minute).await? {
        Access::Allowed => {}
        Access::NotVisible => return Err(not_found()),
        Access::OutsideSchedule { .. } => return Err(ApiError(StatusCode::FORBIDDEN, "outside_schedule")),
    }
    let b = book::Entity::find_by_id(t.book_id).one(&s.store.db.reader).await.map_err(Error::from)?.ok_or_else(not_found)?;
    let path = resolve(&s.store, s.library.as_deref(), &b.folder, &t.file).await.ok_or_else(not_found)?;
    let res = ServeFile::new(path).oneshot(req).await.map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "internal"))?;
    Ok(res.map(axum::body::Body::new).into_response())
}

/// The file of a track below one of the library folders. Only a file that really
/// lies inside the folder counts, whatever the database says.
pub(crate) async fn resolve(store: &Store, library: Option<&Path>, folder: &str, file: &str) -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = library.into_iter().map(Path::to_path_buf).collect();
    for key in ["import_roots", "music_roots"] {
        if let Ok(Some(json)) = store.setting(key).await {
            roots.extend(serde_json::from_str::<Vec<String>>(&json).unwrap_or_default().into_iter().map(PathBuf::from));
        }
    }
    for root in roots {
        let candidate = root.join(folder).join(file);
        if let Some(found) = inside(&root, &candidate).await {
            return Some(found);
        }
    }
    None
}

async fn inside(root: &Path, candidate: &Path) -> Option<PathBuf> {
    let root = tokio::fs::canonicalize(root).await.ok()?;
    let real = tokio::fs::canonicalize(candidate).await.ok()?;
    (real.starts_with(&root) && real.is_file()).then_some(real)
}

#[derive(Deserialize)]
struct ProgressRequest {
    item_id: i64,
    part_id: i64,
    position_ms: i64,
    #[serde(default)]
    finished: bool,
    /// When the listening happened (Unix milliseconds on the device), for a report that is sent later (the device had no connection). The newer
    /// state wins: an older report never overwrites what a more recent one (from this or another device) wrote. Without it, or in the future, now counts.
    #[serde(default)]
    at: Option<i64>,
}

async fn put_progress(State(s): State<ApiState>, d: Device, Json(req): Json<ProgressRequest>) -> ApiResult<StatusCode> {
    let (weekday, minute) = local_time(&s.store).await;
    if s.store.access(d.user_id, req.item_id, weekday, minute).await? == Access::NotVisible {
        return Err(ApiError(StatusCode::NOT_FOUND, "not_found"));
    }
    let part = track::Entity::find_by_id(req.part_id).one(&s.store.db.reader).await.map_err(Error::from)?.filter(|t| t.book_id == req.item_id);
    let Some(part) = part.filter(|_| req.position_ms >= 0) else {
        return Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_progress"));
    };
    // The statistics: a part that reached its end counts once per pass (see plays.rs).
    s.store.note_part_progress(d.user_id, part.id, req.position_ms, part.duration_ms, req.finished).await?;
    s.store
        .save_progress(ProgressUpdate {
            profile_id: d.user_id,
            book_id: req.item_id,
            track_id: req.part_id,
            position_ms: req.position_ms,
            finished: req.finished,
            updated_at: req.at.map_or_else(now_ms, |at| at.clamp(1, now_ms())),
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Weekday (Monday = 0) and minute of the day in the time zone of the server, as
/// the weekly planner counts. An unknown zone falls back to the system's.
pub(crate) async fn local_time(store: &Store) -> (u8, u16) {
    let zone = store.setting("time_zone").await.ok().flatten();
    local_time_now(zone.as_deref())
}

/// The same for a time zone that is already known; an unknown zone falls back to the system's.
pub(crate) fn local_time_now(zone: Option<&str>) -> (u8, u16) {
    let now = jiff::Timestamp::now();
    let zoned = match zone.map(|z| now.in_tz(z)) {
        Some(Ok(z)) => z,
        _ => now.to_zoned(jiff::tz::TimeZone::system()),
    };
    (zoned.weekday().to_monday_zero_offset() as u8, zoned.hour() as u16 * 60 + zoned.minute() as u16)
}

#[cfg(test)]
mod cover_tests {
    use super::cover_rank;

    #[test]
    fn folder_pictures_are_recognised_by_name_without_regard_to_case_and_in_order() {
        assert_eq!(cover_rank("cover.jpg"), Some(0));
        assert_eq!(cover_rank("Folder.JPG"), Some(1));
        assert_eq!(cover_rank("Front.png"), Some(2));
        assert_eq!(cover_rank("cdcover.jpg"), Some(3));
        assert!(cover_rank("AlbumArt_{0D98}_Large.jpg") < cover_rank("AlbumArt_{0D98}_Small.jpg"));
        assert!(cover_rank("AlbumArtSmall.jpg").is_some());
        assert_eq!(cover_rank("01 Track.jpg"), None);
        assert_eq!(cover_rank("cover.txt"), None);
        assert_eq!(cover_rank("cover"), None);
        // A better name wins over a worse one.
        let mut names = vec!["AlbumArtSmall.jpg", "cdcover.jpg", "Folder.jpg", "cover.png"];
        names.sort_by_key(|n| cover_rank(n));
        assert_eq!(names, ["cover.png", "Folder.jpg", "cdcover.jpg", "AlbumArtSmall.jpg"]);
    }
}
