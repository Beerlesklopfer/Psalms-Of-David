// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The e-books for the reader of the app (docs/spec-ebooks.md, `crate::ebooks`). An e-book the person may not see is `404 not_found`,
//! the same as one that does not exist.
//!
//! | Request | Answer |
//! |---|---|
//! | `GET /ebooks` | `{ebooks: [{id, format, title, author, language, size_bytes, mtime_ms, released, min_age, added_at, progress}]}`; `progress` `{locator, percent, read_at}` or null |
//! | `GET /ebooks/{id}/text` | an EPUB as the reader draws it (`crate::ebooks::Text`): `{title, chapters: [{title, href, blocks: [{kind, spans: [{t, b?, i?, sup?}], src?}]}], toc: [{title, chapter, level}]}`; `404 no_text` for a PDF |
//! | `GET /ebooks/{id}/file` | the file itself (EPUB or PDF, with `Range`) |
//! | `GET /ebooks/{id}/cover` | the cover (the cover picture of an EPUB, the first page of a PDF), `404 no_cover` |
//! | `GET /ebooks/{id}/res?name=<name>` | a picture of the text of an EPUB, by its name inside the EPUB |
//! | `GET /ebooks/{id}/progress` | `{locator, percent, read_at}` or `404 no_progress` |
//! | `PUT /ebooks/{id}/progress` | `{locator, percent, read_at}`: where the person is; an older report than the kept one changes nothing; `204` |

use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api::{ApiError, ApiResult, ApiState, Device};
use crate::ebooks::{Ebook, Progress};

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/v1/ebooks", get(list))
        .route("/api/v1/ebooks/{id}/text", get(text))
        .route("/api/v1/ebooks/{id}/file", get(file))
        .route("/api/v1/ebooks/{id}/cover", get(cover))
        .route("/api/v1/ebooks/{id}/res", get(resource))
        .route("/api/v1/ebooks/{id}/progress", get(progress).put(put_progress))
}

const NOT_FOUND: ApiError = ApiError(StatusCode::NOT_FOUND, "not_found");

async fn found(s: &ApiState, d: &Device, id: i64) -> ApiResult<(Ebook, std::path::PathBuf)> {
    s.store.ebook(d.user_id, id).await?.ok_or(NOT_FOUND)
}

async fn list(State(s): State<ApiState>, d: Device) -> ApiResult<Json<Value>> {
    let books = s.store.ebooks(d.user_id).await?;
    let mut progress = s.store.ebook_progresses(d.user_id).await?;
    let out: Vec<Value> = books
        .into_iter()
        .map(|b| {
            let p = progress.remove(&b.id);
            let mut v = serde_json::to_value(&b).unwrap_or(Value::Null);
            v["progress"] = serde_json::to_value(p).unwrap_or(Value::Null);
            v
        })
        .collect();
    Ok(Json(json!({ "ebooks": out })))
}

fn bytes(body: Vec<u8>, mime: &str, cache: &'static str) -> Response {
    let mut r = body.into_response();
    if let Ok(m) = HeaderValue::from_str(mime) {
        r.headers_mut().insert(header::CONTENT_TYPE, m);
    }
    r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    r
}

async fn text(State(s): State<ApiState>, d: Device, Path(id): Path<i64>) -> ApiResult<Response> {
    let (e, path) = found(&s, &d, id).await?;
    match s.store.ebook_text(&e, &path).await? {
        Some(b) => Ok(bytes(b, "application/json", "private, no-cache")),
        None => Err(ApiError(StatusCode::NOT_FOUND, "no_text")),
    }
}

async fn file(State(s): State<ApiState>, d: Device, Path(id): Path<i64>, req: Request) -> ApiResult<Response> {
    let (e, path) = found(&s, &d, id).await?;
    let mime = if e.format == "pdf" { "application/pdf" } else { "application/epub+zip" };
    Ok(crate::api_pictures::serve(&path, mime, false, req).await)
}

async fn cover(State(s): State<ApiState>, d: Device, Path(id): Path<i64>) -> ApiResult<Response> {
    let (e, path) = found(&s, &d, id).await?;
    match s.store.ebook_cover(&e, &path).await {
        Some((b, mime)) => Ok(bytes(b, &mime, "private, max-age=3600")),
        None => Err(ApiError(StatusCode::NOT_FOUND, "no_cover")),
    }
}

#[derive(Deserialize)]
struct Name {
    name: String,
}

async fn resource(State(s): State<ApiState>, d: Device, Path(id): Path<i64>, Query(q): Query<Name>) -> ApiResult<Response> {
    let (e, path) = found(&s, &d, id).await?;
    if e.format != "epub" {
        return Err(NOT_FOUND);
    }
    match s.store.ebook_resource(&path, &q.name).await {
        // Only pictures: a chapter is read through the text.
        Some((b, mime)) if mime.starts_with("image/") => Ok(bytes(b, &mime, "private, max-age=86400")),
        _ => Err(NOT_FOUND),
    }
}

async fn progress(State(s): State<ApiState>, d: Device, Path(id): Path<i64>) -> ApiResult<Json<Progress>> {
    found(&s, &d, id).await?;
    s.store.ebook_progress(d.user_id, id).await?.map(Json).ok_or(ApiError(StatusCode::NOT_FOUND, "no_progress"))
}

async fn put_progress(State(s): State<ApiState>, d: Device, Path(id): Path<i64>, Json(p): Json<Progress>) -> ApiResult<StatusCode> {
    found(&s, &d, id).await?;
    if p.locator.is_empty() || p.locator.len() > 200 || !p.percent.is_finite() {
        return Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_progress"));
    }
    // A time from the future of the device is now.
    let now = (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64;
    let p = Progress { read_at: p.read_at.clamp(1, now), ..p };
    s.store.save_ebook_progress(d.user_id, id, &p).await?;
    Ok(StatusCode::NO_CONTENT)
}
