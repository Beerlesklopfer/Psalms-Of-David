// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The projects of a slideshow in the browser (`crate::slideshow`, docs/spec-pictures.md, 9): the editor of a project (its properties,
//! the order by numbers, the transition and the seconds of each picture) and the preview to pick its pictures. The requests are listed
//! in `handlers::pictures`. Only the person who made a project reaches it; for everybody else it does not exist.

use super::*;
use axum::extract::Path;

#[derive(Deserialize)]
pub struct Keys {
    keys: Vec<String>,
}

#[derive(Deserialize)]
pub struct One {
    key: String,
    transition: Option<String>,
    seconds: Option<String>,
}

fn no_guest(s: &Session) -> Option<Response> {
    (s.role == Role::Guest).then(|| StatusCode::NOT_FOUND.into_response())
}

fn failed(e: crate::error::Error, what: &str) -> Response {
    tracing::error!(error = %e, "{what}");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// The editor of a project.
pub async fn edit(State(state): State<AppState>, s: Session, Path(id): Path<i64>, Query(q): Query<PageQuery>) -> Response {
    if let Some(r) = no_guest(&s) {
        return r;
    }
    match state.ui.slideshow_edit(&state.store, view_for(&s, &q), s.user_id, s.role, id).await {
        Ok(Some(html)) => Html(html).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => failed(e, "the editor of a slideshow failed"),
    }
}

/// The preview to pick the pictures of a project.
pub async fn pick(State(state): State<AppState>, s: Session, Path(id): Path<i64>, Query(q): Query<PageQuery>) -> Response {
    if let Some(r) = no_guest(&s) {
        return r;
    }
    match state.ui.slideshow_pick(&state.store, view_for(&s, &q), s.user_id, s.role, id).await {
        Ok(Some(html)) => Html(html).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => failed(e, "the preview of a slideshow failed"),
    }
}

/// A new project (a plain form: `title`), then its preview to pick the first pictures.
pub async fn create(State(state): State<AppState>, s: Session, Form(pairs): Form<Vec<(String, String)>>) -> Response {
    if let Some(r) = no_guest(&s) {
        return r;
    }
    let title = pairs.iter().find(|(k, _)| k == "title").map(|(_, v)| v.trim()).unwrap_or_default();
    if title.is_empty() || title.chars().count() > crate::slideshow::TITLE_MAX {
        return Redirect::to("/pictures?msg=err.show_title").into_response();
    }
    match state.store.create_slideshow(s.user_id, title).await {
        Ok(id) => Redirect::to(&format!("/pictures/shows/{id}/pick")).into_response(),
        Err(e) => failed(e, "a new slideshow failed"),
    }
}

/// The properties of a project (the form of the editor, sent by its script when a field changes).
pub async fn properties(State(state): State<AppState>, s: Session, Path(id): Path<i64>, Form(pairs): Form<Vec<(String, String)>>) -> Response {
    if let Some(r) = no_guest(&s) {
        return r;
    }
    let Some(p) = crate::slideshow::Properties::from_form(&pairs) else { return StatusCode::UNPROCESSABLE_ENTITY.into_response() };
    match state.store.set_slideshow(s.user_id, id, &p).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => failed(e, "the properties of a slideshow failed"),
    }
}

async fn count(state: &AppState, id: i64) -> Response {
    match state.store.slideshow_pictures(id).await {
        Ok(p) => axum::Json(serde_json::json!({ "count": p.len() })).into_response(),
        Err(e) => failed(e, "the pictures of a slideshow failed"),
    }
}

/// Puts pictures at the end of a project (the page sends them in the order they were taken).
pub async fn add(State(state): State<AppState>, s: Session, Path(id): Path<i64>, axum::Json(k): axum::Json<Keys>) -> Response {
    if let Some(r) = no_guest(&s) {
        return r;
    }
    match state.store.add_slideshow_pictures(s.user_id, s.role, id, &k.keys).await {
        Ok(Some(_)) => count(&state, id).await,
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => failed(e, "adding to a slideshow failed"),
    }
}

pub async fn remove(State(state): State<AppState>, s: Session, Path(id): Path<i64>, axum::Json(k): axum::Json<Keys>) -> Response {
    if let Some(r) = no_guest(&s) {
        return r;
    }
    match state.store.remove_slideshow_pictures(s.user_id, id, &k.keys).await {
        Ok(Some(_)) => count(&state, id).await,
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => failed(e, "removing from a slideshow failed"),
    }
}

/// The new order of the pictures, all of them in a row.
pub async fn order(State(state): State<AppState>, s: Session, Path(id): Path<i64>, axum::Json(k): axum::Json<Keys>) -> Response {
    if let Some(r) = no_guest(&s) {
        return r;
    }
    match state.store.order_slideshow(s.user_id, id, &k.keys).await {
        Ok(true) => count(&state, id).await,
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => failed(e, "the order of a slideshow failed"),
    }
}

/// The transition and the seconds of one picture; empty: those of the project.
pub async fn picture(State(state): State<AppState>, s: Session, Path(id): Path<i64>, axum::Json(one): axum::Json<One>) -> Response {
    if let Some(r) = no_guest(&s) {
        return r;
    }
    let transition = match one.transition.as_deref().unwrap_or_default() {
        "" => None,
        t if ["fade", "zoom", "cut"].contains(&t) => Some(t),
        _ => return StatusCode::UNPROCESSABLE_ENTITY.into_response(),
    };
    let seconds = match one.seconds.as_deref().unwrap_or_default() {
        "" => None,
        t => match t.parse().ok().filter(|s| crate::slideshow::SECONDS.contains(s)) {
            Some(s) => Some(s),
            None => return StatusCode::UNPROCESSABLE_ENTITY.into_response(),
        },
    };
    match state.store.set_slideshow_picture(s.user_id, id, &one.key, transition, seconds).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => failed(e, "a picture of a slideshow failed"),
    }
}

/// Deletes a project (a plain form); its pictures stay where they are, a video made of it stays too.
pub async fn delete(State(state): State<AppState>, s: Session, Path(id): Path<i64>) -> Response {
    if let Some(r) = no_guest(&s) {
        return r;
    }
    match state.store.delete_slideshow(s.user_id, id).await {
        Ok(true) => Redirect::to("/pictures?msg=ok.show_deleted").into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => failed(e, "deleting a slideshow failed"),
    }
}

pub async fn script(State(state): State<AppState>, headers: HeaderMap) -> Response {
    static_file(&state, &headers, "show-editor.js", "text/javascript; charset=utf-8", include_bytes!("../../../static/show-editor.js"))
}
