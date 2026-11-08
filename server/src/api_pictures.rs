// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The pictures to look at, for the slideshow of the apps (docs/spec-pictures.md), under `/api/v1/pictures`. Who may see what is in
//! [`crate::pictures`]; a guest sees nothing.
//!
//! | Request | Answer |
//! |---|---|
//! | `GET /pictures/sources` | `{sources: [{id, kind, name, count, first, last}], places, videos}`: what the person may choose (`own`, `p<id>` a child, `f:<folder>` an old folder); `places`: the server names the places; `videos`: it can show a frame of a video |
//! | `GET /pictures?source=<id>&source=…&from=<ms>&to=<ms>&videos=on` | `{pictures: [{key, source, name, mime, video, taken_at}]}`, the oldest first (no `source`: all); at most 50000 |
//! | `GET /pictures/project?source=…&from=…&to=…&videos=on&order=…&seconds=…&transition=…&info=…&music=<playlist>` | the project of a slideshow: a ZIP with `show.json`, the pictures, videos and music, that plays without the server (`crate::show_project`) |
//! | `GET /pictures/{key}/thumb` | a preview (JPEG, at most 480 px), `404 no_preview` when the server cannot make one |
//! | `GET /pictures/{key}/screen` | the picture for the screen (JPEG, at most 2048 px, upright); for a video one frame of it; `404 no_preview` |
//! | `GET /pictures/{key}/file` | the original (with `Range`, for a video) |
//! | `GET /pictures/{key}/info` | `{taken_at, place}`: `place` is the name of the place (in the language of `Accept-Language`) or null |
//!
//! A key the person may not see is `404 not_found`, the same as one that does not exist.

use std::path::Path as FsPath;

use axum::extract::{Path, RawQuery, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::api::{ApiError, ApiResult, ApiState, Device};
use crate::picture_render::Size;
use crate::pictures::Query;
use crate::store::Store;

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/v1/pictures", get(list))
        .route("/api/v1/pictures/sources", get(sources))
        .route("/api/v1/pictures/project", get(project))
        .route("/api/v1/pictures/{key}/thumb", get(thumb))
        .route("/api/v1/pictures/{key}/screen", get(screen))
        .route("/api/v1/pictures/{key}/file", get(file))
        .route("/api/v1/pictures/{key}/info", get(info))
}

/// What the server can do for the slideshow, for `sources`.
pub async fn capabilities(store: &Store) -> crate::error::Result<(bool, bool)> {
    let places = store.setting("photo_places").await?.as_deref() == Some("osm");
    Ok((places, crate::picture_render::has_ffmpeg()))
}

async fn sources(State(s): State<ApiState>, d: Device) -> ApiResult<Json<Value>> {
    let list = s.store.picture_sources(d.user_id, d.role).await?;
    let (places, videos) = capabilities(&s.store).await?;
    Ok(Json(json!({ "sources": list, "places": places, "videos": videos })))
}

/// `source=…` (repeated), `from`, `to`, `videos=on`.
pub fn parse_query(raw: Option<&str>) -> Query {
    let pairs: Vec<(String, String)> = serde_urlencoded::from_str(raw.unwrap_or_default()).unwrap_or_default();
    let mut q = Query::default();
    for (k, v) in pairs {
        match k.as_str() {
            "source" => q.sources.push(v),
            "from" => q.from = v.parse().ok(),
            "to" => q.to = v.parse().ok(),
            "videos" => q.videos = v == "on",
            _ => {}
        }
    }
    q
}

async fn list(State(s): State<ApiState>, d: Device, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    let q = parse_query(raw.as_deref());
    let pictures = s.store.pictures(d.user_id, d.role, &q).await?;
    Ok(Json(json!({ "pictures": pictures })))
}

/// Serves a file (with `Range`) as this type; a made picture may be kept by the device for a day.
pub async fn serve(path: &FsPath, mime: &str, cache: bool, req: Request) -> Response {
    match tower_http::services::ServeFile::new(path).oneshot(req).await {
        Ok(r) => {
            let mut r = r.map(axum::body::Body::new).into_response();
            if r.status().is_success() {
                if let Ok(m) = HeaderValue::from_str(mime) {
                    r.headers_mut().insert(header::CONTENT_TYPE, m);
                }
                let control = if cache { "private, max-age=86400" } else { "private, no-cache" };
                r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static(control));
            }
            r
        }
        Err(_) => ApiError(StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response(),
    }
}

/// The made picture of a key in this size (for the preview the one the device sent, if there is one).
pub async fn made(store: &Store, user: i64, role: crate::roles::Role, key: &str, size: Size, req: Request) -> Response {
    let found = match store.picture(user, role, key).await {
        Ok(Some(f)) => f,
        Ok(None) => return ApiError(StatusCode::NOT_FOUND, "not_found").into_response(),
        Err(e) => return ApiError::from(e).into_response(),
    };
    if size == Size::Thumb
        && let Some(t) = &found.device_thumb
    {
        return serve(t, "image/jpeg", true, req).await;
    }
    match crate::picture_render::rendered(&store.data_dir, key, &found.path, &found.mime, size).await {
        Some(p) => serve(&p, "image/jpeg", true, req).await,
        // A video without ffmpeg still has the preview its device sent.
        None => match (&found.device_thumb, size) {
            (Some(t), Size::Screen) => serve(t, "image/jpeg", true, req).await,
            _ => ApiError(StatusCode::NOT_FOUND, "no_preview").into_response(),
        },
    }
}

/// The project of a slideshow (`crate::show_project`): the device keeps it and plays it without the server.
async fn project(State(s): State<ApiState>, d: Device, headers: HeaderMap, RawQuery(raw): RawQuery) -> Response {
    let lang = crate::admin::i18n::Lang::from_accept_language(headers.get(header::ACCEPT_LANGUAGE).and_then(|v| v.to_str().ok()).unwrap_or_default());
    crate::show_project::response(s.store.clone(), s.library.clone(), d.user_id, d.role, lang, raw).await
}

async fn thumb(State(s): State<ApiState>, d: Device, Path(key): Path<String>, req: Request) -> Response {
    made(&s.store, d.user_id, d.role, &key, Size::Thumb, req).await
}

async fn screen(State(s): State<ApiState>, d: Device, Path(key): Path<String>, req: Request) -> Response {
    made(&s.store, d.user_id, d.role, &key, Size::Screen, req).await
}

pub async fn original(store: &Store, user: i64, role: crate::roles::Role, key: &str, req: Request) -> Response {
    match store.picture(user, role, key).await {
        Ok(Some(f)) => serve(&f.path, &f.mime, false, req).await,
        Ok(None) => ApiError(StatusCode::NOT_FOUND, "not_found").into_response(),
        Err(e) => ApiError::from(e).into_response(),
    }
}

async fn file(State(s): State<ApiState>, d: Device, Path(key): Path<String>, req: Request) -> Response {
    original(&s.store, d.user_id, d.role, &key, req).await
}

/// The language of a request: the first of `Accept-Language`.
pub fn language(headers: &HeaderMap) -> String {
    headers.get(header::ACCEPT_LANGUAGE).and_then(|v| v.to_str().ok()).and_then(|v| v.split([',', ';']).next()).unwrap_or("en").trim().to_owned()
}

async fn info(State(s): State<ApiState>, d: Device, Path(key): Path<String>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let found = s.store.picture(d.user_id, d.role, &key).await?.ok_or(ApiError(StatusCode::NOT_FOUND, "not_found"))?;
    Ok(Json(s.store.picture_info(&found, &language(&headers)).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_query_takes_several_sources() {
        let q = parse_query(Some("source=own&source=f%3AAllg%C3%A4u2017&from=10&to=20&videos=on&x=1"));
        assert_eq!(q.sources, vec!["own".to_owned(), "f:Allgäu2017".to_owned()]);
        assert_eq!((q.from, q.to, q.videos), (Some(10), Some(20), true));
        let q = parse_query(None);
        assert!(q.sources.is_empty() && !q.videos);
    }
}
