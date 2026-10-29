// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Playlists for the apps (project owner, 2026-10-29), under `/api/v1`; the contract is `docs/spec-playlists.md`.
//!
//! | Request | Answer |
//! |---|---|
//! | `GET /playlists` | the lists of the profile with the number of available entries and their seconds; `ETag` is the fingerprint of the profile's lists |
//! | `POST /playlists` `{name}` | `201 {id, name}`; `409 name_taken`; `422 invalid_name` |
//! | `GET /playlists/{id}` | the entries in order; one the user may not hear is `available: false` without details; `404` for a list of somebody else |
//! | `PATCH /playlists/{id}` `{name}`, `DELETE /playlists/{id}` | rename, delete |
//! | `POST /playlists/{id}/items` `{part_ids}` or `{book_id}`, optional `at` | adds (a whole item in item order); answers like `GET`; `422 unknown_part` and nothing is written |
//! | `DELETE /playlists/{id}/items/{item_id}` | removes one entry |
//! | `PUT /playlists/{id}/order` `{item_ids}` | a permutation of all entries, else `422 invalid_order` |

use axum::Json;
use axum::Router;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, put};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api::{ApiError, ApiResult, ApiState, Device};
use crate::playlists::{PlaylistError, PlaylistView};

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/v1/playlists", get(list).post(create))
        .route("/api/v1/playlists/{id}", get(detail).patch(rename).delete(remove))
        .route("/api/v1/playlists/{id}/items", axum::routing::post(add))
        .route("/api/v1/playlists/{id}/items/{item_id}", delete(remove_item))
        .route("/api/v1/playlists/{id}/order", put(order))
}

impl From<PlaylistError> for ApiError {
    fn from(e: PlaylistError) -> Self {
        let (status, code) = match e {
            PlaylistError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            PlaylistError::NameTaken => (StatusCode::CONFLICT, "name_taken"),
            PlaylistError::InvalidName => (StatusCode::UNPROCESSABLE_ENTITY, "invalid_name"),
            PlaylistError::TooManyPlaylists => (StatusCode::UNPROCESSABLE_ENTITY, "too_many_playlists"),
            PlaylistError::Full => (StatusCode::UNPROCESSABLE_ENTITY, "playlist_full"),
            PlaylistError::UnknownPart => (StatusCode::UNPROCESSABLE_ENTITY, "unknown_part"),
            PlaylistError::UnknownBook => (StatusCode::UNPROCESSABLE_ENTITY, "unknown_book"),
            PlaylistError::InvalidPosition => (StatusCode::UNPROCESSABLE_ENTITY, "invalid_position"),
            PlaylistError::InvalidOrder => (StatusCode::UNPROCESSABLE_ENTITY, "invalid_order"),
            PlaylistError::Db(message) => {
                tracing::error!(error = %message, "playlist request failed");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal")
            }
        };
        ApiError(status, code)
    }
}

fn etag(version: &str) -> String {
    format!("\"{version}\"")
}

async fn list(State(s): State<ApiState>, d: Device, headers: HeaderMap) -> ApiResult<Response> {
    let tag = etag(&s.store.playlists_version(d.user_id).await?);
    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()).is_some_and(|v| v.trim() == tag) {
        return Ok((StatusCode::NOT_MODIFIED, [(header::ETAG, tag)]).into_response());
    }
    let lists: Vec<Value> = s
        .store
        .playlist_summaries(Some(d.user_id))
        .await?
        .iter()
        .map(|p| json!({ "id": p.id, "name": p.name, "items": p.items, "seconds": p.seconds, "updated_at": p.updated_at }))
        .collect();
    Ok(([(header::ETAG, tag)], Json(Value::Array(lists))).into_response())
}

#[derive(Deserialize)]
struct NameRequest {
    name: String,
}

async fn create(State(s): State<ApiState>, d: Device, Json(req): Json<NameRequest>) -> ApiResult<(StatusCode, Json<Value>)> {
    let id = s.store.create_playlist(d.user_id, &req.name).await?;
    let name = crate::playlists::clean_name(&req.name)?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id, "name": name }))))
}

/// The JSON of one entry. Of an entry that may not be heard nothing but its place is told.
pub fn entry_json(e: &crate::playlists::Entry) -> Value {
    let (part, book) = if e.available {
        (
            json!({ "id": e.track_id, "disc": e.disc, "number": e.number, "title": e.title, "duration_ms": e.duration_ms, "plays": e.plays, "last_played_at": e.last_played_at }),
            json!({ "id": e.book_id, "title": e.book_title, "author": e.author }),
        )
    } else {
        (Value::Null, Value::Null)
    };
    json!({ "item_id": e.item_id, "position": e.position, "available": e.available, "part": part, "book": book })
}

fn view_json(v: &PlaylistView) -> Value {
    json!({ "id": v.id, "name": v.name, "items": v.entries.iter().map(entry_json).collect::<Vec<_>>() })
}

async fn view(s: &ApiState, d: &Device, id: i64) -> ApiResult<Json<Value>> {
    let v = s.store.playlist_view(id, d.user_id).await?.ok_or(PlaylistError::NotFound)?;
    Ok(Json(view_json(&v)))
}

async fn detail(State(s): State<ApiState>, d: Device, UrlPath(id): UrlPath<i64>) -> ApiResult<Json<Value>> {
    view(&s, &d, id).await
}

async fn rename(State(s): State<ApiState>, d: Device, UrlPath(id): UrlPath<i64>, Json(req): Json<NameRequest>) -> ApiResult<Json<Value>> {
    s.store.rename_playlist(id, d.user_id, &req.name).await?;
    Ok(Json(json!({ "id": id, "name": crate::playlists::clean_name(&req.name)? })))
}

async fn remove(State(s): State<ApiState>, d: Device, UrlPath(id): UrlPath<i64>) -> ApiResult<StatusCode> {
    s.store.delete_playlist(id, d.user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct AddRequest {
    part_ids: Option<Vec<i64>>,
    book_id: Option<i64>,
    at: Option<i64>,
}

async fn add(State(s): State<ApiState>, d: Device, UrlPath(id): UrlPath<i64>, Json(req): Json<AddRequest>) -> ApiResult<Json<Value>> {
    match (req.part_ids, req.book_id) {
        (Some(parts), None) if !parts.is_empty() => s.store.add_playlist_parts(id, d.user_id, &parts, req.at).await?,
        (None, Some(book)) => s.store.add_playlist_book(id, d.user_id, book, req.at).await?,
        _ => return Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_request")),
    }
    view(&s, &d, id).await
}

async fn remove_item(State(s): State<ApiState>, d: Device, UrlPath((id, item)): UrlPath<(i64, i64)>) -> ApiResult<StatusCode> {
    s.store.remove_playlist_item(id, d.user_id, item).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct OrderRequest {
    item_ids: Vec<i64>,
}

async fn order(State(s): State<ApiState>, d: Device, UrlPath(id): UrlPath<i64>, Json(req): Json<OrderRequest>) -> ApiResult<Json<Value>> {
    s.store.reorder_playlist(id, d.user_id, &req.item_ids).await?;
    view(&s, &d, id).await
}
