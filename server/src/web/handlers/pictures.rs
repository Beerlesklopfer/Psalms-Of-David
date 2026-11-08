// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The slideshow in the browser (`/pictures`, docs/spec-pictures.md, step 2), for every person who signs in except a guest: the page with
//! the choice, and what its script loads, all with the session of the browser and the same rules as the apps (`crate::pictures`).
//!
//! | Request | Answer |
//! |---|---|
//! | `GET /pictures` | the page |
//! | `GET /pictures/list?source=…&from=…&to=…&videos=on` | like `GET /api/v1/pictures` |
//! | `GET /pictures/project?…&order=…&seconds=…&transition=…&info=…&music=…` | the project: a ZIP that plays without the server |
//! | `GET /pictures/{key}/thumb`, `/screen`, `/file`, `/info` | like the apps |
//! | `GET /pictures/music/{playlist}` | `{tracks: [{id, title, artist}]}`: the entries of an own playlist the person may hear now |
//! | `GET /pictures/track/{id}` | a track of such a playlist (with `Range`; `?as=mp3` for a browser that cannot play the format) |

use super::*;
use crate::picture_render::Size;

/// A guest has no pictures; the page and everything below it is "not found" for one.
fn no_guest(s: &Session) -> Option<Response> {
    (s.role == Role::Guest).then(|| StatusCode::NOT_FOUND.into_response())
}

pub async fn page(State(state): State<AppState>, s: Session, Query(q): Query<PageQuery>) -> Response {
    if let Some(r) = no_guest(&s) {
        return r;
    }
    render(state.ui.pictures(&state.store, view_for(&s, &q), s.user_id, s.role).await)
}

pub async fn list(State(state): State<AppState>, s: Session, RawQuery(raw): RawQuery) -> Response {
    if let Some(r) = no_guest(&s) {
        return r;
    }
    let q = crate::api_pictures::parse_query(raw.as_deref());
    match state.store.pictures(s.user_id, s.role, &q).await {
        Ok(p) => ([(header::CACHE_CONTROL, "no-store")], axum::Json(serde_json::json!({ "pictures": p }))).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "the list of pictures failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// The project of the slideshow: the same choice as one file that plays without the server (`crate::show_project`).
pub async fn project(State(state): State<AppState>, s: Session, RawQuery(raw): RawQuery) -> Response {
    crate::show_project::response(state.store.clone(), state.library.clone(), s.user_id, s.role, s.lang, raw).await
}

pub async fn picture(State(state): State<AppState>, s: Session, axum::extract::Path((key, what)): axum::extract::Path<(String, String)>, req: axum::extract::Request) -> Response {
    if let Some(r) = no_guest(&s) {
        return r;
    }
    match what.as_str() {
        "thumb" => crate::api_pictures::made(&state.store, s.user_id, s.role, &key, Size::Thumb, req).await,
        "screen" => crate::api_pictures::made(&state.store, s.user_id, s.role, &key, Size::Screen, req).await,
        "file" => crate::api_pictures::original(&state.store, s.user_id, s.role, &key, req).await,
        "info" => {
            let lang = s.lang.code().to_owned();
            let found = match state.store.picture(s.user_id, s.role, &key).await {
                Ok(Some(f)) => f,
                Ok(None) => return StatusCode::NOT_FOUND.into_response(),
                Err(e) => return crate::api::ApiError::from(e).into_response(),
            };
            match state.store.picture_info(&found, &lang).await {
                Ok(v) => axum::Json(v).into_response(),
                Err(e) => crate::api::ApiError::from(e).into_response(),
            }
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

/// The tracks of an own playlist that the person may hear now, in order.
pub async fn music(State(state): State<AppState>, s: Session, axum::extract::Path(id): axum::extract::Path<i64>) -> Response {
    if let Some(r) = no_guest(&s) {
        return r;
    }
    match state.store.playlist_view(id, s.user_id).await {
        Ok(Some(v)) => {
            let tracks: Vec<serde_json::Value> = v
                .entries
                .iter()
                .filter(|e| e.available)
                .map(|e| serde_json::json!({ "id": e.track_id, "title": e.title, "artist": e.author }))
                .collect();
            ([(header::CACHE_CONTROL, "no-store")], axum::Json(serde_json::json!({ "tracks": tracks }))).into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::error!(error = %e, "the music of the slideshow failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// A track for the music behind the slideshow: the same checks as the apps (the person may hear it, now).
pub async fn track(State(state): State<AppState>, s: Session, axum::extract::Path(id): axum::extract::Path<i64>, req: axum::extract::Request) -> Response {
    if let Some(r) = no_guest(&s) {
        return r;
    }
    crate::api::serve_track(&state.store, state.library.as_deref(), s.user_id, id, req).await.into_response()
}

pub async fn script(State(state): State<AppState>, headers: HeaderMap) -> Response {
    static_file(&state, &headers, "slideshow.js", "text/javascript; charset=utf-8", include_bytes!("../../../static/slideshow.js"))
}

pub async fn style(State(state): State<AppState>, headers: HeaderMap) -> Response {
    static_file(&state, &headers, "slideshow.css", "text/css; charset=utf-8", include_bytes!("../../../static/slideshow.css"))
}
