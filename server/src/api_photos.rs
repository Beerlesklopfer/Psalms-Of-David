// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The photo backup for the apps (docs/spec-photo-sync.md, section 4), under `/api/v1/photos`: the settings of the profile of the device.
//! While the server has the backup off every route answers `404 photo_sync_off`, so the app offers nothing; a guest gets `403 no_guests`.
//!
//! | Request | Answer |
//! |---|---|
//! | `GET /photos/settings` | `{server, folder, active, settings, effective, locked}`: what the server allows, whether the profile has a picture folder, whether the backup runs, the stored choices, what they are worth under the rules of the server, and per choice the options that cannot be chosen with the reason (`{"mode": [{"option": "bi", "why": "no_bisync"}]}`) |
//! | `PUT /photos/settings` `{enabled?: "on", mode?: "bi", ...}` | the choices to change, by the names of `GET` (`on`/`off` for a switch); answers like `GET`. A choice the server does not allow is `422 {"error": "refused", "refused": {"mode": "no_bisync"}}` and nothing is saved; switching off is always allowed |
//! | `GET /photos/state` | `{count, bytes, last_at, quota_bytes, active, mode}`: what the person keeps on the server |
//! | `POST /photos/have` `{files: [{sha256, size}]}` (at most 1000) | `{missing: [sha256]}`: the files the server lacks; the device sends only these |
//! | `PUT /photos/upload/{client_id}` | a piece of a file, see below |
//! | `GET /photos/upload/{client_id}` | `{sha256, received}` of an upload under way, `404 unknown_upload` |
//! | `GET /photos/changes?since=<id>&limit=<n>` | bisync only (else `409 not_bisync`): `{photos: [{id, sha256, size, name, mime, taken_at, uploaded_at, thumb}], next}`, the pictures other devices of the person sent after `since`; `next` is the `since` of the next call |
//! | `GET /photos/{id}/file` | the file of an own picture (with `Range`), `404` for any other |
//! | `PUT /photos/{id}/thumb` | the preview of an own picture (`image/jpeg`, at most 512 KiB), sent by the device that sent the picture: `204`; `404 not_found` for any other picture, `415 wrong_type`, `413 too_large` |
//! | `GET /photos/{id}/thumb` | the preview, `404 no_thumb` when there is none (`changes` says so in `thumb`) |
//!
//! An upload: the device names the file by a `client_id` of its own (1 to 128 visible characters, no slash) and sends it in pieces with
//! `PUT /photos/upload/{client_id}`, each with the headers `X-Sha256` (of the whole file, lower-case hex), `X-Size` (of the whole file),
//! `Content-Type` (`image/*` or `video/*`), and on the first piece `X-File-Name` and `X-Taken-At` (ms). `Content-Range: bytes <start>-<end>/<size>`
//! says where the piece belongs (absent: at 0). A piece is at most 32 MiB. Answers:
//! `202 {received}` (go on from there), `201 {photo, known: false}` (complete, the SHA-256 fits, kept), `200 {photo, known: true}` (the server
//! had it already; nothing more to send). Refusals, each `{"error": code}`: `409 backup_off`, `409 no_folder`, `413 too_large`, `415 wrong_type`
//! (videos off, or neither picture nor video), `507 quota_full`, `507 disk_full`, `409 clash` (the client id is in use for another file),
//! `409 wrong_offset` with `received` (send from there), `422 hash_mismatch` (the bytes were not the announced ones; start again at 0),
//! `400 bad_request`. After a lost connection `GET /photos/upload/{client_id}` says where to go on; an upload left alone for 7 days is removed.
//!
//! The texts that explain the options are in the app (in its language); the server sends only names and codes.

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::api::{ApiError, ApiResult, ApiState, Device};
use crate::photo_settings::{PROFILE_CHOICES, ServerPhotoSettings};
use crate::photo_store::{HAVE_MAX, Offer, Owner, PIECE_MAX, Progress, Refusal, THUMB_MAX, is_jpeg};

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/v1/photos/settings", get(get_settings).put(put_settings))
        .route("/api/v1/photos/state", get(get_state))
        .route("/api/v1/photos/have", post(post_have))
        .route("/api/v1/photos/upload/{client_id}", get(get_upload).put(put_upload).layer(DefaultBodyLimit::max(PIECE_MAX + 1024)))
        .route("/api/v1/photos/changes", get(get_changes))
        .route("/api/v1/photos/{id}/file", get(get_file))
        .route("/api/v1/photos/{id}/thumb", get(get_thumb).put(put_thumb).layer(DefaultBodyLimit::max(THUMB_MAX + 1024)))
}

/// The role of the device's profile, if the backup is open to it at all.
async fn role_of(s: &ApiState, d: &Device) -> ApiResult<(String, ServerPhotoSettings)> {
    let server = ServerPhotoSettings::load(&s.store).await?;
    if !server.on {
        return Err(ApiError(StatusCode::NOT_FOUND, "photo_sync_off"));
    }
    let role = s.store.profile_by_id(d.user_id).await?.map(|p| p.role).ok_or(ApiError(StatusCode::UNAUTHORIZED, "unauthorized"))?;
    if role == "guest" {
        return Err(ApiError(StatusCode::FORBIDDEN, "no_guests"));
    }
    Ok((role, server))
}

async fn settings_json(s: &ApiState, d: &Device, role: &str, server: &ServerPhotoSettings) -> ApiResult<Value> {
    let mine = s.store.profile_photo_settings(d.user_id).await?;
    let folder = s.store.profile_folders(d.user_id).await?.iter().any(|(k, _)| k == "photo");
    let effective = mine.effective(server, role);
    let mut locked = serde_json::Map::new();
    for c in PROFILE_CHOICES {
        let mut l = server.locked(role, c.key, true);
        if c.key == "enabled" && l.is_empty() && !folder {
            l.push(("on", "no_folder"));
        }
        if !l.is_empty() {
            locked.insert(c.key.into(), l.iter().map(|(o, w)| json!({ "option": o, "why": w })).collect());
        }
    }
    let choices = |p: &crate::photo_settings::ProfilePhotoSettings| -> Value {
        PROFILE_CHOICES.iter().filter(|c| c.key != "parent_view" || role == "child").map(|c| (c.key.to_owned(), json!(p.choice(c.key)))).collect::<serde_json::Map<_, _>>().into()
    };
    Ok(json!({
        "server": { "bisync": server.bisync, "videos": server.videos, "children": server.children, "parents_may_view": server.parents_may_view, "max_file_mb": server.max_file_mb },
        "folder": folder,
        "active": effective.enabled && folder,
        "settings": choices(&mine),
        "effective": choices(&effective),
        "locked": locked,
    }))
}

async fn get_settings(State(s): State<ApiState>, d: Device) -> ApiResult<Json<Value>> {
    let (role, server) = role_of(&s, &d).await?;
    Ok(Json(settings_json(&s, &d, &role, &server).await?))
}

async fn put_settings(State(s): State<ApiState>, d: Device, Json(body): Json<HashMap<String, Value>>) -> Response {
    let outcome = async {
        let (role, server) = role_of(&s, &d).await?;
        let form: HashMap<String, String> = body.into_iter().filter_map(|(k, v)| v.as_str().map(|v| (k, v.to_owned()))).collect();
        let refused = crate::photo_settings::save_profile(&s.store, d.user_id, &role, true, &form).await?;
        if !refused.is_empty() {
            let refused: serde_json::Map<String, Value> = refused.into_iter().map(|(k, w)| (k, json!(w))).collect();
            return Ok(Err(refused));
        }
        Ok(Ok(settings_json(&s, &d, &role, &server).await?))
    }
    .await;
    match outcome {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(refused)) => (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "error": "refused", "refused": refused }))).into_response(),
        Err::<_, ApiError>(e) => e.into_response(),
    }
}

/// The owner of the request with what the backup is worth for them (the device from the token).
async fn owner_of(s: &ApiState, d: &Device) -> ApiResult<Owner> {
    let (role, _) = role_of(s, d).await?;
    let device = s.store.device_of_token(&d.token).await?;
    Ok(s.store.photo_owner(d.user_id, &role, device).await?)
}

async fn get_state(State(s): State<ApiState>, d: Device) -> ApiResult<Json<Value>> {
    let o = owner_of(&s, &d).await?;
    let (count, bytes, last_at) = s.store.photo_totals(o.id).await?;
    let quota = u64::from(o.server.quota_gb) * 1024 * 1024 * 1024;
    Ok(Json(json!({
        "count": count, "bytes": bytes, "last_at": last_at, "quota_bytes": (quota > 0).then_some(quota),
        "active": o.settings.enabled && o.folder.is_some(), "mode": o.settings.mode,
    })))
}

#[derive(Deserialize)]
struct HaveFile {
    sha256: String,
    size: u64,
}

#[derive(Deserialize)]
struct Have {
    files: Vec<HaveFile>,
}

async fn post_have(State(s): State<ApiState>, d: Device, Json(req): Json<Have>) -> ApiResult<Json<Value>> {
    let o = owner_of(&s, &d).await?;
    if req.files.len() > HAVE_MAX || req.files.iter().any(|f| !crate::photo_store::valid_sha256(&f.sha256)) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "bad_request"));
    }
    let files: Vec<(String, u64)> = req.files.into_iter().map(|f| (f.sha256, f.size)).collect();
    Ok(Json(json!({ "missing": s.store.photos_missing(o.id, &files).await? })))
}

async fn get_upload(State(s): State<ApiState>, d: Device, Path(client_id): Path<String>) -> ApiResult<Json<Value>> {
    let o = owner_of(&s, &d).await?;
    match s.store.photo_upload_state(o.id, o.device, &client_id).await? {
        Some((sha256, received)) => Ok(Json(json!({ "sha256": sha256, "received": received }))),
        None => Err(ApiError(StatusCode::NOT_FOUND, "unknown_upload")),
    }
}

/// `bytes <start>-<end>/<size>` -> start; absent -> 0; malformed -> None.
fn range_start(h: &HeaderMap) -> Option<u64> {
    let Some(v) = h.get(header::CONTENT_RANGE) else { return Some(0) };
    let v = v.to_str().ok()?.trim().strip_prefix("bytes ")?;
    v.split(['-', '/']).next()?.trim().parse().ok()
}

fn status_of(r: Refusal) -> StatusCode {
    match r {
        Refusal::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        Refusal::WrongType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        Refusal::QuotaFull | Refusal::DiskFull => StatusCode::INSUFFICIENT_STORAGE,
        Refusal::HashMismatch => StatusCode::UNPROCESSABLE_ENTITY,
        Refusal::BadRequest => StatusCode::BAD_REQUEST,
        Refusal::BackupOff | Refusal::NoFolder | Refusal::Clash | Refusal::WrongOffset => StatusCode::CONFLICT,
    }
}

async fn put_upload(State(s): State<ApiState>, d: Device, Path(client_id): Path<String>, headers: HeaderMap, body: Bytes) -> Response {
    let text = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(|v| v.trim().to_owned());
    let outcome = async {
        let o = owner_of(&s, &d).await?;
        let (Some(sha256), Some(size), Some(start)) = (text("x-sha256"), text("x-size").and_then(|v| v.parse::<u64>().ok()), range_start(&headers)) else {
            return Err(ApiError(StatusCode::BAD_REQUEST, "bad_request"));
        };
        let mime = text("content-type").map(|m| m.split(';').next().unwrap_or_default().trim().to_ascii_lowercase()).unwrap_or_default();
        let offer = Offer {
            client_id,
            sha256: sha256.to_ascii_lowercase(),
            size,
            name: text("x-file-name").map(|n| percent_decode(&n)).unwrap_or_default(),
            mime,
            taken_at: text("x-taken-at").and_then(|v| v.parse().ok()),
        };
        let r = s.store.photo_piece(&o, &offer, start, &body).await?;
        let received = match r {
            Err(Refusal::WrongOffset) => s.store.photo_upload_state(o.id, o.device, &offer.client_id).await?.map(|(_, n)| n),
            _ => None,
        };
        Ok((r, received))
    }
    .await;
    match outcome {
        Ok((Ok(Progress::Partial { received }), _)) => (StatusCode::ACCEPTED, Json(json!({ "received": received }))).into_response(),
        Ok((Ok(Progress::Done { photo, known }), _)) => (if known { StatusCode::OK } else { StatusCode::CREATED }, Json(json!({ "photo": photo, "known": known }))).into_response(),
        Ok((Err(why), received)) => {
            let mut body = json!({ "error": why.code() });
            if let Some(n) = received {
                body["received"] = json!(n);
            }
            (status_of(why), Json(body)).into_response()
        }
        Err(e) => e.into_response(),
    }
}

/// `X-File-Name` may be percent-encoded (a header carries only ASCII safely); a name that is not stays as it is.
fn percent_decode(v: &str) -> String {
    let b = v.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(byte) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| v.to_owned())
}

#[derive(Deserialize)]
struct ChangesQuery {
    since: Option<i64>,
    limit: Option<u64>,
}

async fn get_changes(State(s): State<ApiState>, d: Device, Query(q): Query<ChangesQuery>) -> ApiResult<Json<Value>> {
    let o = owner_of(&s, &d).await?;
    if o.settings.mode != "bi" || !o.settings.enabled {
        return Err(ApiError(StatusCode::CONFLICT, "not_bisync"));
    }
    let since = q.since.unwrap_or(0);
    let photos = s.store.photo_changes(o.id, o.device, since, q.limit.unwrap_or(500).clamp(1, 1000)).await?;
    let next = photos.last().map_or(since, |p| p.id);
    Ok(Json(json!({ "photos": photos, "next": next })))
}

async fn get_file(State(s): State<ApiState>, d: Device, Path(id): Path<i64>, req: Request) -> Response {
    let found = async {
        role_of(&s, &d).await?;
        Ok::<_, ApiError>(s.store.photo_file(d.user_id, id).await?)
    }
    .await;
    match found {
        Ok(Some((path, mime))) => {
            // ServeFile answers Range requests (a long video); the type is the one the device sent, not a guess from the name.
            match tower_http::services::ServeFile::new(&path).oneshot(req).await {
                Ok(r) => {
                    let mut r = r.map(axum::body::Body::new).into_response();
                    if r.status().is_success()
                        && let Ok(m) = axum::http::HeaderValue::from_str(&mime)
                    {
                        r.headers_mut().insert(header::CONTENT_TYPE, m);
                    }
                    r
                }
                Err(_) => ApiError(StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response(),
            }
        }
        Ok(None) => ApiError(StatusCode::NOT_FOUND, "not_found").into_response(),
        Err(e) => e.into_response(),
    }
}

async fn put_thumb(State(s): State<ApiState>, d: Device, Path(id): Path<i64>, headers: HeaderMap, body: Bytes) -> ApiResult<StatusCode> {
    role_of(&s, &d).await?;
    let mime = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or_default();
    if !mime.trim().eq_ignore_ascii_case("image/jpeg") || !is_jpeg(&body) {
        return Err(ApiError(StatusCode::UNSUPPORTED_MEDIA_TYPE, "wrong_type"));
    }
    if body.len() > THUMB_MAX {
        return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE, "too_large"));
    }
    if s.store.put_photo_thumb(d.user_id, id, &body).await? { Ok(StatusCode::NO_CONTENT) } else { Err(ApiError(StatusCode::NOT_FOUND, "not_found")) }
}

async fn get_thumb(State(s): State<ApiState>, d: Device, Path(id): Path<i64>) -> Response {
    let found = async {
        role_of(&s, &d).await?;
        Ok::<_, ApiError>(s.store.photo_thumb_path(d.user_id, id).await?)
    }
    .await;
    match found {
        Ok(Some(path)) => match tokio::fs::read(&path).await {
            Ok(b) => ([(header::CONTENT_TYPE, "image/jpeg")], b).into_response(),
            Err(_) => ApiError(StatusCode::NOT_FOUND, "no_thumb").into_response(),
        },
        Ok(None) => ApiError(StatusCode::NOT_FOUND, "not_found").into_response(),
        Err(e) => e.into_response(),
    }
}
