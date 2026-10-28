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
}

pub fn router(state: ApiState) -> Router {
    Router::new()
        .route("/api/v1/pair", post(pair))
        .route("/api/v1/me", get(me))
        .route("/api/v1/logout", post(logout))
        .route("/api/v1/library/version", get(library_version))
        .route("/api/v1/items", get(items))
        .route("/api/v1/items/{id}", get(item_detail))
        .route("/api/v1/items/{id}/cover", get(item_cover))
        .route("/api/v1/continue", get(continue_listening))
        .route("/api/v1/parts/{id}/stream", get(stream))
        .route("/api/v1/progress", put(put_progress))
        .route("/api/v1/bible/resolve", get(bible_resolve))
        .with_state(state)
}

/// An error answer: a status and a code the app can switch on.
pub struct ApiError(StatusCode, &'static str);

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

type ApiResult<T> = Result<T, ApiError>;

/// A signed-in device. Extracting it checks the bearer token.
pub struct Device {
    pub user_id: i64,
    pub role: Role,
    token: String,
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
    let token = s.store.create_session(user.id, role, Some(paired.device_id)).await?;
    Ok(Json(json!({
        "token": token,
        "device_id": paired.device_id,
        "shared": paired.shared,
        "user": { "id": user.id, "name": user.name, "role": role.as_str() },
    })))
}

async fn me(State(s): State<ApiState>, d: Device) -> ApiResult<Json<Value>> {
    let user = s.store.profile_by_id(d.user_id).await?.ok_or(ApiError(StatusCode::UNAUTHORIZED, "unauthorized"))?;
    Ok(Json(json!({ "id": user.id, "name": user.name, "nickname": user.name, "first_name": user.first_name, "last_name": user.last_name,
                  "role": d.role.as_str(), "max_age": user.max_age, "language": user.language })))
}

async fn logout(State(s): State<ApiState>, d: Device) -> ApiResult<StatusCode> {
    s.store.delete_session(&d.token).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// A short value that changes whenever the library changes (a new CD, a release, a blurb). The
/// apps ask for it now and then and reload the library when it differs from the last one.
async fn library_version(State(s): State<ApiState>, _d: Device) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "version": s.store.library_version().await? })))
}

async fn items(State(s): State<ApiState>, d: Device) -> ApiResult<Json<Value>> {
    let rows = s.store.visible_books(d.user_id).await?;
    Ok(Json(Value::Array(rows.iter().map(item_json).collect())))
}

fn item_json(b: &crate::store::BookRow) -> Value {
    json!({
        "id": b.id, "kind": b.kind, "title": b.title, "creator": b.author, "narrator": b.narrator, "min_age": b.min_age,
        "recommended_age": b.recommended_age, "part_count": b.tracks, "discs": b.discs,
        "duration_ms": b.duration_ms,
    })
}

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
    let tracks: Vec<Value> = s
        .store
        .tracks_of(id)
        .await?
        .iter()
        .map(|t| json!({ "id": t.id, "disc": t.disc, "number": t.number, "title": t.title, "duration_ms": t.duration_ms }))
        .collect();
    let progress = s.store.progress_of(d.user_id, id).await?.map(|p| {
        json!({ "part_id": p.track_id, "position_ms": p.position_ms, "finished": p.finished, "updated_at": p.updated_at })
    });
    let description = book::Entity::find_by_id(id).one(&s.store.db.reader).await.map_err(Error::from)?.and_then(|b| b.description);
    let mut out = item_json(&row);
    out["description"] = description.map_or(Value::Null, Value::String);
    out["parts"] = Value::Array(tracks);
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
fn cover_in(dir: &Path) -> Option<(&'static str, Vec<u8>)> {
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
    let rows = s.store.continue_listening(d.user_id, 20).await?;
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
}

async fn put_progress(State(s): State<ApiState>, d: Device, Json(req): Json<ProgressRequest>) -> ApiResult<StatusCode> {
    let (weekday, minute) = local_time(&s.store).await;
    if s.store.access(d.user_id, req.item_id, weekday, minute).await? == Access::NotVisible {
        return Err(ApiError(StatusCode::NOT_FOUND, "not_found"));
    }
    let belongs = track::Entity::find_by_id(req.part_id)
        .one(&s.store.db.reader)
        .await
        .map_err(Error::from)?
        .is_some_and(|t| t.book_id == req.item_id);
    if !belongs || req.position_ms < 0 {
        return Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_progress"));
    }
    s.store
        .save_progress(ProgressUpdate {
            profile_id: d.user_id,
            book_id: req.item_id,
            track_id: req.part_id,
            position_ms: req.position_ms,
            finished: req.finished,
            updated_at: now_ms(),
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
