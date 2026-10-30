// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The personal profile for the apps (project owner, 2026-10-30), under `/api/v1/me`. What a user may change about themself, nothing more:
//! the names, the language, the PIN or password and the picture. Role, age limit, owner and the limits of the devices stay with the administrator.
//!
//! | Request | Answer |
//! |---|---|
//! | `GET /me` | the profile (see `api.rs`), with `has_avatar`, `avatar_version` and the device counts |
//! | `PUT /me` `{nickname?, first_name?, last_name?, language?}` | changes what is given (`language`: `en`, `de`, or `""`/`null` to follow the device); answers like `GET /me` |
//! | `PUT /me/pin` `{current, new}` | changes the PIN or password; a wrong current one counts like a wrong sign-in |
//! | `GET /me/devices` | the devices of the account: `[{id, name, shared, last_seen, this}]` |
//! | `GET /me/avatar` | the picture, `404` if there is none; `ETag` is the version, `304` for a repeated request |
//! | `PUT /me/avatar` | the picture as the body (`image/jpeg`, `image/png` or `image/webp`; the type is decided by the first bytes, at most 256 KiB); the app crops it square and downsizes it before |
//! | `DELETE /me/avatar` | removes the picture |

use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
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
}

/// The profile as the apps read it.
pub(crate) async fn me_json(s: &ApiState, d: &Device) -> ApiResult<Value> {
    let user = s.store.profile_by_id(d.user_id).await?.ok_or(ApiError(StatusCode::UNAUTHORIZED, "unauthorized"))?;
    let avatar = s.store.avatar_version(d.user_id).await?;
    let (used, max) = s.store.device_usage(d.user_id).await?;
    Ok(json!({
        "id": user.id, "name": user.name, "nickname": user.name, "first_name": user.first_name, "last_name": user.last_name,
        "role": d.role.as_str(), "max_age": user.max_age, "language": user.language,
        "has_avatar": avatar.is_some(), "avatar_version": avatar, "device_count": used, "device_limit": max,
    }))
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
    Ok(Json(me_json(&s, &d).await?))
}

#[derive(Deserialize)]
struct PinChange {
    current: String,
    new: String,
}

async fn put_pin(State(s): State<ApiState>, d: Device, Json(req): Json<PinChange>) -> ApiResult<StatusCode> {
    // The sessions of the browser end, the paired devices keep theirs (this one included).
    s.store.change_pin(d.user_id, &req.current, &req.new, &d.token).await.map_err(|e| match e {
        Error::WrongPin => ApiError(StatusCode::FORBIDDEN, "wrong_pin"),
        Error::PinLocked => ApiError(StatusCode::TOO_MANY_REQUESTS, "locked"),
        Error::InvalidPin => ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_pin"),
        e => e.into(),
    })?;
    Ok(StatusCode::NO_CONTENT)
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
