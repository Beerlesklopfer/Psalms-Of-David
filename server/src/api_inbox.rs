// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The inbox of the CDs for the apps, under `/api/v1/inbox` (docs/spec-cd-inbox.md, docs/spec-app-api.md):
//!
//! | Request | Answer |
//! |---|---|
//! | `GET /inbox?after=&limit=` | `{items, unread, tasks_open, next}` for the profile of the token, newest first |
//! | `POST /inbox/{id}/read` | `204`; `404 not_found` if the item is not theirs |
//! | `POST /inbox/read_all` | `204` |
//!
//! There is no route to decide a task: the buttons call `POST /cd/jobs/{job_id}/approve` or `/reject`, the same as everywhere.
//! A person without any right for CDs (a child, a guest, a member with no CD right) always gets an empty inbox.

use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api::{ApiError, ApiResult, ApiState, Device};
use crate::inbox::{PAGE_DEFAULT, PAGE_MAX};
use crate::roles::Role;

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/v1/inbox", get(list))
        .route("/api/v1/inbox/read_all", post(read_all))
        .route("/api/v1/inbox/{id}/read", post(read))
}

/// Does this person have an inbox at all?
async fn has_inbox(s: &ApiState, user: i64) -> bool {
    match s.store.cd_rights(user).await {
        Ok(r) => r.may_insert || r.may_approve,
        Err(_) => false,
    }
}

/// `(unread, tasks_open)` of a person; zero for one without an inbox.
pub(crate) async fn counts(s: &ApiState, user: i64) -> (i64, i64) {
    if !has_inbox(s, user).await {
        return (0, 0);
    }
    s.store.inbox_counts(user).await.unwrap_or((0, 0))
}

#[derive(Deserialize)]
struct Paging {
    after: Option<i64>,
    limit: Option<i64>,
}

async fn list(
    State(s): State<ApiState>,
    d: Device,
    Query(p): Query<Paging>,
) -> ApiResult<Json<Value>> {
    if p.limit.is_some_and(|l| l < 1) || p.after.is_some_and(|a| a < 1) {
        return Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_paging"));
    }
    if !matches!(d.role, Role::Admin | Role::Member) || !has_inbox(&s, d.user_id).await {
        return Ok(Json(
            json!({ "items": [], "unread": 0, "tasks_open": 0, "next": null }),
        ));
    }
    let limit = p.limit.unwrap_or(PAGE_DEFAULT).min(PAGE_MAX);
    // One more than asked tells whether there is a next page.
    let mut items = s.store.inbox_list(d.user_id, p.after, limit + 1).await?;
    let more = items.len() as i64 > limit;
    items.truncate(limit as usize);
    let next = if more {
        items.last().map(|i| i.id)
    } else {
        None
    };
    let (unread, tasks_open) = s.store.inbox_counts(d.user_id).await?;
    Ok(Json(
        json!({ "items": items.iter().map(|i| i.json()).collect::<Vec<_>>(), "unread": unread, "tasks_open": tasks_open, "next": next }),
    ))
}

async fn read(
    State(s): State<ApiState>,
    d: Device,
    UrlPath(id): UrlPath<i64>,
) -> ApiResult<StatusCode> {
    if !has_inbox(&s, d.user_id).await || !s.store.inbox_mark_read(d.user_id, id).await? {
        return Err(ApiError(StatusCode::NOT_FOUND, "not_found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn read_all(State(s): State<ApiState>, d: Device) -> ApiResult<StatusCode> {
    if has_inbox(&s, d.user_id).await {
        s.store.inbox_mark_all_read(d.user_id).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}
