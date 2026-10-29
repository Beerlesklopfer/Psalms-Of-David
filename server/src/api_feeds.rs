// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Podcasts and radio for the apps (project owner, 2026-10-28), under `/api/v1`:
//!
//! | Request | Answer |
//! |---|---|
//! | `GET /podcasts` | the feeds of the account, those waiting for a release marked `approved: false` |
//! | `POST /podcasts` `{url}` | follows a feed (an address of a feed or of a page that points to one); `201`, or `202` when a child asked and the request waits |
//! | `DELETE /podcasts/{id}` | stops following |
//! | `GET /podcasts/{id}/episodes` | the newest episodes with where this account stopped |
//! | `PUT /episodes/{id}/state` `{position_ms, finished}` | where the account stopped; every device of the account reads it |
//! | `POST /podcasts/{id}/assign` `{profile_id}` | a parent (administrator or member) gives the feed to a child, released at once |
//! | `GET /radio/search?q=` | stations of the directory in the settings (`radio_directory_url`) by name; `404 no_directory` if none is set |
//! | `GET /radio`, `POST /radio` `{stream_url, name, homepage?, logo_url?}`, `DELETE /radio/{id}`, `POST /radio/{id}/assign` | the same for stations |
//!
//! The audio is not proxied: the app plays it from the publisher. A guest may look but not add.

use axum::Json;
use axum::Router;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post, put};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api::{ApiError, ApiResult, ApiState, Device};
use crate::error::Error;
use crate::Store;
use crate::roles::Role;

/// The largest feed that is read when no setting says otherwise.
const DEFAULT_FEED_MAX_BYTES: usize = 5 * 1024 * 1024;
const EPISODES_SHOWN: i64 = 100;

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/v1/podcasts", get(podcasts).post(follow))
        .route("/api/v1/podcasts/{id}", delete(unfollow))
        .route("/api/v1/podcasts/{id}/episodes", get(episodes))
        .route("/api/v1/podcasts/{id}/assign", post(assign_podcast))
        .route("/api/v1/episodes/{id}/state", put(episode_state))
        .route("/api/v1/radio", get(radio).post(add_station))
        .route("/api/v1/radio/search", get(search_radio))
        .route("/api/v1/radio/{id}", delete(remove_station))
        .route("/api/v1/radio/{id}/assign", post(assign_station))
}

fn not_found() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "not_found")
}

fn refuse() -> ApiError {
    ApiError(StatusCode::FORBIDDEN, "forbidden")
}

/// Who may add: everybody but a guest. Whether it waits is decided by the role.
fn may_add(d: &Device) -> ApiResult<bool> {
    match d.role {
        Role::Guest => Err(refuse()),
        Role::Child => Ok(false),
        Role::Admin | Role::Member => Ok(true),
    }
}

fn may_assign(d: &Device) -> ApiResult<()> {
    matches!(d.role, Role::Admin | Role::Member).then_some(()).ok_or_else(refuse)
}

async fn podcasts(State(s): State<ApiState>, d: Device) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "podcasts": s.store.subscriptions(d.user_id, false).await? })))
}

#[derive(Deserialize)]
struct FollowRequest {
    url: String,
}

/// Reads a feed from an address; if that is a page, the feed it points to.
/// Reads the feed at `url` (or the feed that the page at `url` points to): its address and what it says. `invalid_feed` if it cannot be read.
pub(crate) async fn read_feed(store: &Store, url: &str) -> Result<(String, crate::podcast::Feed), ApiError> {
    let max = store.setting("feed_max_bytes").await?.and_then(|v| v.trim().parse::<usize>().ok()).filter(|n| *n >= 1024).unwrap_or(DEFAULT_FEED_MAX_BYTES);
    let invalid = || ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_feed");
    let fetch = move |address: String| async move { tokio::task::spawn_blocking(move || crate::fetch::download_text(&address, max)).await };
    let body = fetch(url.to_owned()).await.map_err(|_| invalid())?.map_err(|e| {
        tracing::info!(%url, error = %e, "feed not read");
        invalid()
    })?;
    match crate::podcast::parse(&body) {
        Ok(feed) => Ok((url.to_owned(), feed)),
        Err(_) => {
            let link = crate::podcast::feed_link_in_page(&body, url).ok_or_else(invalid)?;
            let body = fetch(link.clone()).await.map_err(|_| invalid())?.map_err(|_| invalid())?;
            Ok((link, crate::podcast::parse(&body).map_err(|_| invalid())?))
        }
    }
}

async fn follow(State(s): State<ApiState>, d: Device, Json(req): Json<FollowRequest>) -> ApiResult<(StatusCode, Json<Value>)> {
    let approved = may_add(&d)?;
    let (address, feed) = read_feed(&s.store, req.url.trim()).await?;
    let id = s.store.save_feed(&address, &feed).await.map_err(|_| ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_feed"))?;
    s.store.subscribe(d.user_id, id, approved).await?;
    let released = s.store.follows(d.user_id, id).await?;
    let status = if released { StatusCode::CREATED } else { StatusCode::ACCEPTED };
    Ok((status, Json(json!({ "id": id, "title": feed.title, "approved": released }))))
}

async fn unfollow(State(s): State<ApiState>, d: Device, UrlPath(id): UrlPath<i64>) -> ApiResult<StatusCode> {
    s.store.unsubscribe(d.user_id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn episodes(State(s): State<ApiState>, d: Device, UrlPath(id): UrlPath<i64>) -> ApiResult<Json<Value>> {
    if !s.store.follows(d.user_id, id).await? {
        return Err(not_found());
    }
    Ok(Json(json!({ "episodes": s.store.episodes(d.user_id, id, EPISODES_SHOWN).await? })))
}

#[derive(Deserialize)]
struct StateRequest {
    position_ms: i64,
    #[serde(default)]
    finished: bool,
}

async fn episode_state(State(s): State<ApiState>, d: Device, UrlPath(id): UrlPath<i64>, Json(req): Json<StateRequest>) -> ApiResult<StatusCode> {
    let feed = s.store.episode_feed(id).await?.ok_or_else(not_found)?;
    if !s.store.follows(d.user_id, feed).await? {
        return Err(not_found());
    }
    if req.position_ms < 0 {
        return Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_state"));
    }
    s.store.put_episode_state(d.user_id, id, req.position_ms, req.finished).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct AssignRequest {
    profile_id: i64,
}

/// A parent gives what they follow to a child (or another member): released at once, also a waiting request.
async fn assign_podcast(State(s): State<ApiState>, d: Device, UrlPath(id): UrlPath<i64>, Json(req): Json<AssignRequest>) -> ApiResult<StatusCode> {
    may_assign(&d)?;
    if s.store.feed_url(id).await?.is_none() || s.store.profile_by_id(req.profile_id).await?.is_none() {
        return Err(not_found());
    }
    s.store.subscribe(req.profile_id, id, true).await?;
    s.store.decide_release("podcast", req.profile_id, id, true).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
}

async fn search_radio(State(s): State<ApiState>, _d: Device, Query(q): Query<SearchQuery>) -> ApiResult<Json<Value>> {
    match crate::feeds::search_stations(&s.store, &q.q).await {
        Ok(hits) => Ok(Json(json!({ "stations": hits }))),
        Err(why) if why == "no_directory" => Err(ApiError(StatusCode::NOT_FOUND, "no_directory")),
        Err(why) => {
            tracing::info!(%why, "station search failed");
            Err(ApiError(StatusCode::BAD_GATEWAY, "directory_unreachable"))
        }
    }
}

async fn radio(State(s): State<ApiState>, d: Device) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "stations": s.store.favorite_stations(d.user_id, false).await? })))
}

#[derive(Deserialize)]
struct StationRequest {
    stream_url: String,
    name: String,
    homepage: Option<String>,
    logo_url: Option<String>,
}

async fn add_station(State(s): State<ApiState>, d: Device, Json(req): Json<StationRequest>) -> ApiResult<(StatusCode, Json<Value>)> {
    let approved = may_add(&d)?;
    let id = s
        .store
        .save_station(req.stream_url.trim(), &req.name, req.homepage.as_deref(), req.logo_url.as_deref())
        .await
        .map_err(|e| match e {
            Error::WriteFailed(_) => ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_station"),
            e => e.into(),
        })?;
    s.store.favorite_station(d.user_id, id, approved).await?;
    let released = s.store.favorite_stations(d.user_id, true).await?.iter().any(|st| st.id == id);
    let status = if released { StatusCode::CREATED } else { StatusCode::ACCEPTED };
    Ok((status, Json(json!({ "id": id, "approved": released }))))
}

async fn remove_station(State(s): State<ApiState>, d: Device, UrlPath(id): UrlPath<i64>) -> ApiResult<StatusCode> {
    s.store.unfavorite_station(d.user_id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn assign_station(State(s): State<ApiState>, d: Device, UrlPath(id): UrlPath<i64>, Json(req): Json<AssignRequest>) -> ApiResult<StatusCode> {
    may_assign(&d)?;
    if s.store.favorite_stations(d.user_id, false).await?.iter().all(|st| st.id != id) || s.store.profile_by_id(req.profile_id).await?.is_none() {
        return Err(not_found());
    }
    s.store.favorite_station(req.profile_id, id, true).await?;
    s.store.decide_release("station", req.profile_id, id, true).await?;
    Ok(StatusCode::NO_CONTENT)
}
