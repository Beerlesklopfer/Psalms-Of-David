// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The personal profile for the apps (project owner, 2026-10-30), under `/api/v1/me`. What a user may change about themself, nothing more:
//! the names, the language, the PIN or password and the picture. Role, age limit, owner and the limits of the devices stay with the administrator.
//!
//! | Request | Answer |
//! |---|---|
//! | `GET /me` | the profile (see `api.rs`), with `has_avatar`, `avatar_version` and the device counts |
//! | `PUT /me` `{nickname?, first_name?, last_name?, language?}` | changes what is given (`language`: `en`, `de`, or `""`/`null` to follow the device); answers like `GET /me` |
//! | `PUT /me` also takes `share_listening` (bool): whether the family sees what you listened to; `false` answers `403 listening_not_hideable` when an administrator forbade hiding it |
//! | `PUT /me/pin` `{current, new}` | changes the PIN or password; a wrong current one counts like a wrong sign-in |
//! | `GET /me/devices` | the devices of the account: `[{id, name, shared, last_seen, this}]` |
//! | `GET /me/avatar` | the picture, `404` if there is none; `ETag` is the version, `304` for a repeated request |
//! | `PUT /me/avatar` | the picture as the body (`image/jpeg`, `image/png` or `image/webp`; the type is decided by the first bytes, at most 256 KiB); the app crops it square and downsizes it before |
//! | `DELETE /me/avatar` | removes the picture |

use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api::{ApiError, ApiResult, ApiState, Device};
use crate::error::Error;
use crate::profile::MAX_AVATAR_BYTES;

/// A body of this size is read at all; the picture itself may be `MAX_AVATAR_BYTES`.
const AVATAR_BODY: usize = 4 * MAX_AVATAR_BYTES;

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/v1/me", put(put_me))
        .route("/api/v1/me/pin", put(put_pin))
        .route("/api/v1/family/listening", get(family_listening))
        .route("/api/v1/family/media-time", get(family_media_time))
        .route("/api/v1/me/devices", get(devices))
        .route("/api/v1/me/avatar", get(get_avatar).put(put_avatar).delete(delete_avatar).layer(DefaultBodyLimit::max(AVATAR_BODY)))
}

#[derive(Deserialize)]
struct MeUpdate {
    nickname: Option<String>,
    first_name: Option<String>,
    last_name: Option<String>,
    /// `None` leaves it as it is; an empty text or an explicit `null` is not distinguishable here, so `""` is the way to follow the device.
    language: Option<String>,
    /// Show the own listening to the family (`false` is refused when an administrator forbade hiding it).
    share_listening: Option<bool>,
}

/// The profile as the apps read it.
pub(crate) async fn me_json(s: &ApiState, d: &Device) -> ApiResult<Value> {
    let user = s.store.profile_by_id(d.user_id).await?.ok_or(ApiError(StatusCode::UNAUTHORIZED, "unauthorized"))?;
    let avatar = s.store.avatar_version(d.user_id).await?;
    let (used, max) = s.store.device_usage(d.user_id).await?;
    let (share, may_hide) = s.store.listening_sharing(d.user_id).await?;
    // What the person may do with CDs. The effective role counts too: a session on a shared device of a child holds none.
    let mut cd = s.store.cd_rights(d.user_id).await?;
    if !matches!(d.role, crate::roles::Role::Admin | crate::roles::Role::Member) {
        cd = crate::cdrights::CdRights::none();
    }
    let (inbox_unread, inbox_tasks_open) = if cd.may_insert || cd.may_approve { s.store.inbox_counts(d.user_id).await? } else { (0, 0) };
    let mut me = json!({
        "inbox_unread": inbox_unread, "inbox_tasks_open": inbox_tasks_open,
        "cd_rights": { "may_insert": cd.may_insert, "no_approval": cd.no_approval, "may_approve": cd.may_approve },
        "share_listening": share || !may_hide, "may_hide_listening": may_hide,
        "id": user.id, "name": user.name, "nickname": user.name, "first_name": user.first_name, "last_name": user.last_name,
        "role": d.role.as_str(), "max_age": user.max_age, "language": user.language,
        "has_avatar": avatar.is_some(), "avatar_version": avatar, "device_count": used, "device_limit": max,
    });
    // Capabilities of hidden features appear only while they are on: a server without them says nothing about them.
    if s.features.download {
        me["downloads"] = json!(true);
    }
    Ok(me)
}

async fn put_me(State(s): State<ApiState>, d: Device, Json(req): Json<MeUpdate>) -> ApiResult<Json<Value>> {
    let user = s.store.profile_by_id(d.user_id).await?.ok_or(ApiError(StatusCode::UNAUTHORIZED, "unauthorized"))?;
    if req.nickname.is_some() || req.first_name.is_some() || req.last_name.is_some() {
        let nickname = req.nickname.as_deref().unwrap_or(&user.name).trim().to_owned();
        let first = req.first_name.or(user.first_name).unwrap_or_default();
        let last = req.last_name.or(user.last_name).unwrap_or_default();
        if nickname.is_empty() || [nickname.as_str(), first.trim(), last.trim()].iter().any(|n| n.chars().count() > 80) {
            return Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_name"));
        }
        if !s.store.nickname_free(&nickname, d.user_id).await? {
            return Err(ApiError(StatusCode::CONFLICT, "exists"));
        }
        s.store.set_person_names(d.user_id, &nickname, &first, &last).await.map_err(|e| match e {
            Error::InvalidName => ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_name"),
            e if e.to_string().contains("UNIQUE constraint failed: profile.name") => ApiError(StatusCode::CONFLICT, "exists"),
            e => e.into(),
        })?;
    }
    if let Some(language) = req.language {
        let chosen = (!language.is_empty()).then_some(language.as_str());
        s.store.set_language(d.user_id, chosen).await.map_err(|e| match e {
            Error::InvalidName => ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_language"),
            e => e.into(),
        })?;
    }
    if let Some(share) = req.share_listening {
        let (_, may_hide) = s.store.listening_sharing(d.user_id).await?;
        if !share && !may_hide {
            return Err(ApiError(StatusCode::FORBIDDEN, "listening_not_hideable"));
        }
        s.store.set_share_listening(d.user_id, share).await?;
    }
    Ok(Json(me_json(&s, &d).await?))
}

#[derive(Deserialize)]
struct PinChange {
    current: String,
    new: String,
}

async fn put_pin(State(s): State<ApiState>, d: Device, Json(req): Json<PinChange>) -> axum::response::Response {
    use axum::response::IntoResponse;
    // The sessions of the browser end, the paired devices keep theirs (this one included).
    let error = match s.store.change_pin(d.user_id, &req.current, &req.new, &d.token).await {
        Ok(()) => return StatusCode::NO_CONTENT.into_response(),
        Err(Error::WrongPin) => ApiError(StatusCode::FORBIDDEN, "wrong_pin"),
        Err(Error::PinLocked) => ApiError(StatusCode::TOO_MANY_REQUESTS, "locked"),
        Err(Error::InvalidPin) => ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_pin"),
        Err(e) => ApiError::from(e),
    };
    // After a wrong current PIN (and while it waits) the answer says how long the next try waits, for the countdown of
    // the app: `retry_after_s` in the body and the header `Retry-After` (seconds).
    let wait_s = if matches!(error.1, "wrong_pin" | "locked") { wait_of(&s, d.user_id).await } else { None };
    let mut res = match wait_s {
        Some(w) => (error.0, [(axum::http::header::CACHE_CONTROL, "no-store")], Json(serde_json::json!({ "error": error.1, "retry_after_s": w }))).into_response(),
        None => error.into_response(),
    };
    if let Some(w) = wait_s
        && let Ok(v) = axum::http::HeaderValue::from_str(&w.to_string())
    {
        res.headers_mut().insert(axum::http::header::RETRY_AFTER, v);
    }
    res
}

/// Seconds until the user may try a PIN again, if they have to wait.
async fn wait_of(s: &ApiState, user_id: i64) -> Option<i64> {
    let profile = s.store.profile_by_id(user_id).await.ok().flatten()?;
    let until = s.store.sign_in_wait(&profile.name).await.ok().flatten()?;
    Some((until - crate::store::now_ms() + 999) / 1000)
}

/// What the other members of the family listened to lately, one entry per member (`docs/spec-app-api.md`). Guests see nothing.
async fn family_listening(State(s): State<ApiState>, d: Device) -> ApiResult<Json<Value>> {
    if d.role == crate::roles::Role::Guest {
        return Ok(Json(Value::Array(Vec::new())));
    }
    let hidden = s.store.hidden_of(d.user_id).await?;
    let hidden_ids = if hidden.show_hidden { Default::default() } else { hidden.ids_in(&s.store.visible_books(d.user_id).await?) };
    let members = s.store.family_listening(d.user_id, 10).await?;
    Ok(Json(Value::Array(
        members
            .iter()
            .filter_map(|m| {
                let books: Vec<Value> = m
                    .books
                    .iter()
                    .filter(|c| !hidden_ids.contains(&c.book_id))
                    .map(|c| json!({ "item_id": c.book_id, "kind": "audiobook", "title": c.title, "creator": c.author, "part_id": c.track_id, "position_ms": c.position_ms, "updated_at": c.updated_at }))
                    .collect();
                (!books.is_empty()).then(|| json!({ "user_id": m.member_id, "name": m.member, "has_avatar": m.has_avatar, "items": books }))
            })
            .collect(),
    )))
}

/// The media time of the family: per member the time listened on each of the last days (`?days=`, 7 to 90, 14 by default), per kind of medium
/// and together (`docs/spec-app-api.md`). Guests see only themselves.
async fn family_media_time(State(s): State<ApiState>, d: Device, Query(q): Query<TimeQuery>) -> ApiResult<Json<Value>> {
    let days = q.days.unwrap_or(14).clamp(7, 90);
    let mut time = s.store.media_time(d.user_id, days).await?;
    if d.role == crate::roles::Role::Guest {
        time.members.retain(|m| m.me);
    }
    let members: Vec<Value> = time
        .members
        .iter()
        .map(|m| {
            let mut total = vec![0_i64; days as usize];
            let mut kinds = serde_json::Map::new();
            for (kind, per_day) in &m.kinds {
                for (t, ms) in total.iter_mut().zip(per_day) {
                    *t += ms;
                }
                kinds.insert(kind.clone(), json!(per_day));
            }
            json!({ "user_id": m.member_id, "name": m.member, "has_avatar": m.has_avatar, "me": m.me, "kinds": kinds, "total": total })
        })
        .collect();
    Ok(Json(json!({ "from_day": time.from_day, "days": days, "members": members })))
}

#[derive(Deserialize)]
struct TimeQuery {
    days: Option<i64>,
}

async fn devices(State(s): State<ApiState>, d: Device) -> ApiResult<Json<Value>> {
    let this = s.store.session_device(&d.token).await?;
    let rows = s.store.devices_of(d.user_id).await?;
    Ok(Json(Value::Array(
        rows.iter()
            .filter(|x| x.revoked_at.is_none())
            .map(|x| json!({ "id": x.id, "name": x.name, "shared": x.shared, "last_seen": x.last_seen, "this": this == Some(x.id) }))
            .collect(),
    )))
}

async fn get_avatar(State(s): State<ApiState>, d: Device, headers: HeaderMap) -> Response {
    let avatar = match s.store.avatar(d.user_id).await {
        Ok(Some(a)) => a,
        Ok(None) => return ApiError(StatusCode::NOT_FOUND, "not_found").into_response(),
        Err(e) => return ApiError::from(e).into_response(),
    };
    let etag = format!("\"{}\"", avatar.updated_at);
    let cache = [(header::ETAG, etag.clone()), (header::CACHE_CONTROL, "private, no-cache".to_owned())];
    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
        return (StatusCode::NOT_MODIFIED, cache).into_response();
    }
    (cache, [(header::CONTENT_TYPE, avatar.mime)], avatar.data).into_response()
}

async fn put_avatar(State(s): State<ApiState>, d: Device, body: Bytes) -> ApiResult<Json<Value>> {
    if body.is_empty() {
        return Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "avatar_none"));
    }
    s.store.set_avatar(d.user_id, &body).await.map_err(|e| match e {
        Error::AvatarType => ApiError(StatusCode::UNSUPPORTED_MEDIA_TYPE, "avatar_type"),
        Error::AvatarSize => ApiError(StatusCode::PAYLOAD_TOO_LARGE, "avatar_size"),
        e => e.into(),
    })?;
    Ok(Json(me_json(&s, &d).await?))
}

async fn delete_avatar(State(s): State<ApiState>, d: Device) -> ApiResult<Json<Value>> {
    s.store.remove_avatar(d.user_id).await?;
    Ok(Json(me_json(&s, &d).await?))
}
