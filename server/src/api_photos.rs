// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The phone backup for the apps (docs/spec-photo-sync.md, section 4), under `/api/v1/photos`: the settings of the profile of the device.
//! While the server has the backup off every route answers `404 photo_sync_off`, so the app offers nothing; a guest gets `403 no_guests`.
//!
//! | Request | Answer |
//! |---|---|
//! | `GET /photos/settings` | `{server, folder, active, settings, effective, locked}`: what the server allows, whether the profile has a picture folder, whether the backup runs, the stored choices, what they are worth under the rules of the server, and per choice the options that cannot be chosen with the reason (`{"mode": [{"option": "bi", "why": "no_bisync"}]}`) |
//! | `PUT /photos/settings` `{enabled?: "on", mode?: "bi", ...}` | the choices to change, by the names of `GET` (`on`/`off` for a switch); answers like `GET`. A choice the server does not allow is `422 {"error": "refused", "refused": {"mode": "no_bisync"}}` and nothing is saved; switching off is always allowed |
//!
//! The texts that explain the options are in the app (in its language); the server sends only names and codes.

use std::collections::HashMap;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};

use crate::api::{ApiError, ApiResult, ApiState, Device};
use crate::photo_settings::{PROFILE_CHOICES, ServerPhotoSettings};

pub fn routes() -> Router<ApiState> {
    Router::new().route("/api/v1/photos/settings", get(get_settings).put(put_settings))
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
