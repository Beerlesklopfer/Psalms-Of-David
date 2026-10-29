// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The CD import for the apps, under `/api/v1/cd` (the full description is `docs/spec-cd-import-api.md`):
//!
//! | Request | Answer |
//! |---|---|
//! | `GET /cd/settings` | format, bit rate, naming and the rest of the settings of the group "CD import" |
//! | `POST /cd/lookup` `{toc}` | the Disc ID and the releases MusicBrainz knows for it |
//! | `GET /cd/cover?release=` | the front cover of a release (JPEG or PNG) from the Cover Art Archive |
//! | `POST /cd/jobs`, `PUT /cd/jobs/{id}/progress`, `GET /cd/jobs`, `GET /cd/jobs/{id}` | the state of an import |
//! | `POST /cd/import` | the ripped files: checked, named, tagged, placed and imported |
//! | `GET /cd/import/{id}` | the state of a job (the fallback for the event channel) |
//! | `GET /cd/events` | WebSocket: every change of a job, pushed to all devices of the account |
//!
//! Only an administrator or a member may use it; a child or a guest gets `403 forbidden`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Path as UrlPath, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use hyper_util::rt::TokioIo;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use crate::api::{ApiError, ApiResult, ApiState, Device};
use crate::cd_settings::CdSettings;
use crate::cdimport::{self, CLIENT_PHASES, Job, JobPatch, Meta, Refusal, StagedTrack};
use crate::musicbrainz::{self, Toc};
use crate::roles::Role;
use crate::ws;

/// The most one upload may hold (a CD of 99 tracks is far below), and the most one file.
const MAX_IMPORT_BYTES: usize = 4 * 1024 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 1024 * 1024 * 1024;

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/v1/cd/settings", get(settings))
        .route("/api/v1/cd/lookup", post(lookup))
        .route("/api/v1/cd/cover", get(cover))
        .route("/api/v1/cd/jobs", get(jobs).post(create_job))
        .route("/api/v1/cd/jobs/{id}", get(job))
        .route("/api/v1/cd/jobs/{id}/progress", put(progress))
        .route("/api/v1/cd/import", get(open_jobs).post(import).layer(DefaultBodyLimit::max(MAX_IMPORT_BYTES)))
        .route("/api/v1/cd/import/{id}", get(job))
        .route("/api/v1/cd/events", get(events))
        .route("/api/v1/cd/drive", get(drive))
        .route("/api/v1/cd/rip", post(rip))
        .route("/api/v1/cd/rip/{id}/cancel", post(rip_cancel))
        .route("/api/v1/cd/rip/dismiss", post(rip_dismiss))
        .route("/api/v1/cd/eject", post(eject))
}

fn refuse() -> ApiError {
    ApiError(StatusCode::FORBIDDEN, "forbidden")
}

fn may_import(d: &Device) -> ApiResult<()> {
    matches!(d.role, Role::Admin | Role::Member).then_some(()).ok_or_else(refuse)
}

fn err(status: u16, code: &'static str) -> ApiError {
    ApiError(StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR), code)
}

impl From<Refusal> for ApiError {
    fn from(r: Refusal) -> Self {
        err(r.0, r.1)
    }
}

async fn settings(State(s): State<ApiState>, d: Device) -> ApiResult<Json<Value>> {
    may_import(&d)?;
    let cd = CdSettings::load(&s.store).await?;
    // The values are at the top level, in the words of the apps; `device` is the name this device has in the jobs.
    let mut out = serde_json::to_value(&cd).map_err(|_| err(500, "internal"))?;
    out["device"] = json!(cdimport::device_label(&d.token));
    Ok(Json(out))
}

// Lookup

/// The table of contents as an object (`first_track`, `last_track`, `leadout_lba`, `starts_lba`), or as the
/// string of the MusicBrainz `toc` parameter that the Linux app sends.
#[derive(Deserialize)]
#[serde(untagged)]
enum TocInput {
    Param(String),
    Object(Toc),
}

#[derive(Deserialize)]
struct LookupRequest {
    /// Absent: the disc in the server's own drive.
    #[serde(default)]
    toc: Option<TocInput>,
    /// What the app computed itself; the server computes the ID again and uses its own.
    #[serde(default)]
    #[allow(dead_code)]
    disc_id: Option<String>,
    /// Words to search MusicBrainz for (title, artist): the releases are added after those the disc finds, without tracks.
    #[serde(default)]
    query: Option<String>,
    /// One release, to get it with the tracks of this disc (chosen from a search).
    #[serde(default)]
    release_id: Option<String>,
}

/// A candidate in the words of the apps: `tracks` are the titles, `cover` is the reference for `GET /cd/cover`.
fn candidate_json(c: &musicbrainz::Candidate) -> Value {
    json!({
        "release_id": c.release_id, "title": c.title, "author": c.author, "narrators": c.narrators, "credits": c.credits,
        "date": c.date, "year": c.year, "language": c.language,
        "disc": c.disc, "discs_total": c.discs_total, "disc_id_matched": c.disc_id_matched,
        "tracks": c.tracks.iter().map(|t| t.title.clone()).collect::<Vec<_>>(),
        "track_details": c.tracks,
        "cover": c.cover_available.then(|| c.release_id.clone()),
        "cover_available": c.cover_available,
        "audiobook": c.audiobook,
    })
}

async fn lookup(State(s): State<ApiState>, d: Device, Json(req): Json<LookupRequest>) -> ApiResult<Json<Value>> {
    may_import(&d)?;
    let set = CdSettings::load(&s.store).await?;
    // Without a table of contents from the caller the server reads the disc in its own drive.
    let toc = match req.toc {
        Some(TocInput::Object(t)) => Some(t).filter(Toc::is_valid).ok_or_else(|| err(422, "invalid_toc"))?,
        Some(TocInput::Param(p)) => Toc::from_param(&p).ok_or_else(|| err(422, "invalid_toc"))?,
        None => s.cd.drives.inserted_toc(&set.device).ok_or_else(|| err(409, "no_disc"))?,
    };
    let id = musicbrainz::disc_id(&toc);
    let candidates = if let Some(mbid) = req.release_id.as_deref() {
        cdimport::release_for(&s.store, &s.cd, &set, mbid, toc.starts_lba.len()).await?.into_iter().collect()
    } else {
        let (_, mut found) = cdimport::lookup(&s.store, &s.cd, &set, &toc).await?;
        if let Some(words) = req.query.as_deref().filter(|q| !q.trim().is_empty()) {
            for c in cdimport::search(&s.store, &s.cd, &set, words).await? {
                if !found.iter().any(|k| k.release_id == c.release_id) {
                    found.push(c);
                }
            }
        }
        found
    };
    Ok(Json(json!({ "disc_id": id, "candidates": candidates.iter().map(candidate_json).collect::<Vec<_>>() })))
}

#[derive(Deserialize)]
struct CoverQuery {
    /// The reference a candidate carries in `cover` (the release identifier); `release` is the same.
    #[serde(alias = "release")]
    r#ref: String,
}

async fn cover(State(s): State<ApiState>, d: Device, Query(q): Query<CoverQuery>) -> ApiResult<Response> {
    may_import(&d)?;
    if !musicbrainz::is_mbid(&q.r#ref) {
        return Err(err(422, "invalid_release"));
    }
    let set = CdSettings::load(&s.store).await?;
    if !set.lookup_ready {
        return Err(err(409, "no_contact"));
    }
    match fetch_cover(&s, &set, &q.r#ref).await? {
        Some((bytes, mime)) => Ok(([(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, "private, max-age=86400")], bytes).into_response()),
        None => Err(err(404, "not_found")),
    }
}

async fn fetch_cover(s: &ApiState, set: &CdSettings, mbid: &str) -> ApiResult<Option<(Vec<u8>, &'static str)>> {
    Ok(cdimport::fetch_cover(&s.store, &s.cd, set, mbid).await?)
}

// Jobs

fn text_ok(s: &str) -> bool {
    s.chars().count() <= 200 && !s.chars().any(char::is_control)
}

fn busy_elsewhere(jobs: &[Job], mine: &str) -> bool {
    jobs.iter().any(|j| j.active && j.device != mine)
}

async fn create_job(State(s): State<ApiState>, d: Device, Json(mut req): Json<JobPatch>) -> ApiResult<(StatusCode, Json<Value>)> {
    may_import(&d)?;
    let ok = req.author.as_deref().is_none_or(text_ok)
        && req.album.as_deref().is_none_or(text_ok)
        && req.tracks_total.is_none_or(|n| (0..=99).contains(&n))
        && req.disc.is_none_or(|n| (1..=99).contains(&n))
        && req.discs_total.is_none_or(|n| (1..=99).contains(&n))
        && req.phase.as_deref().is_none_or(|p| CLIENT_PHASES.contains(&p) && p != "failed");
    if !ok {
        return Err(err(422, "invalid_job"));
    }
    req.phase.get_or_insert_with(|| "ripping".to_owned());
    let device = cdimport::device_label(&d.token);
    let before = s.store.cd_jobs(d.user_id).await?;
    let job = s.store.create_cd_job(d.user_id, &device, &req).await?;
    s.cd.publish(d.user_id, "job", &job);
    Ok((StatusCode::CREATED, Json(json!({ "job": job, "busy_elsewhere": busy_elsewhere(&before, &device) }))))
}

async fn jobs(State(s): State<ApiState>, d: Device) -> ApiResult<Json<Value>> {
    may_import(&d)?;
    let mut list = s.store.cd_jobs(d.user_id).await?;
    list.iter_mut().for_each(|j| s.cd.decorate(j));
    let device = cdimport::device_label(&d.token);
    Ok(Json(json!({ "busy": list.iter().any(|j| j.active), "busy_elsewhere": busy_elsewhere(&list, &device), "device": device, "jobs": list })))
}

/// The jobs that are still open.
async fn open_jobs(State(s): State<ApiState>, d: Device) -> ApiResult<Json<Value>> {
    may_import(&d)?;
    let mut list: Vec<Job> = s.store.cd_jobs(d.user_id).await?.into_iter().filter(|j| j.active).collect();
    list.iter_mut().for_each(|j| s.cd.decorate(j));
    Ok(Json(json!({ "jobs": list })))
}

async fn job(State(s): State<ApiState>, d: Device, UrlPath(id): UrlPath<i64>) -> ApiResult<Json<Value>> {
    may_import(&d)?;
    s.store.expire_cd_jobs().await?;
    let mut job = s.store.cd_job(d.user_id, id).await?.ok_or_else(|| err(404, "not_found"))?;
    s.cd.decorate(&mut job);
    Ok(Json(json!({ "job": job })))
}

async fn progress(State(s): State<ApiState>, d: Device, UrlPath(id): UrlPath<i64>, Json(mut req): Json<JobPatch>) -> ApiResult<Json<Value>> {
    may_import(&d)?;
    s.store.expire_cd_jobs().await?;
    let current = s.store.cd_job(d.user_id, id).await?.ok_or_else(|| err(404, "not_found"))?;
    if !current.active || matches!(current.phase.as_str(), "placing" | "importing") {
        return Err(err(409, "job_closed"));
    }
    let ok = req.phase.as_deref().is_none_or(|p| CLIENT_PHASES.contains(&p))
        && req.track.is_none_or(|n| (0..=99).contains(&n))
        && req.tracks_total.is_none_or(|n| (0..=99).contains(&n))
        && req.author.as_deref().is_none_or(text_ok)
        && req.album.as_deref().is_none_or(text_ok)
        && req.disc.is_none_or(|n| (1..=99).contains(&n))
        && req.discs_total.is_none_or(|n| (1..=99).contains(&n))
        && req.error.as_deref().is_none_or(|e| e.chars().count() <= 60 && !e.chars().any(char::is_control))
        && req.message.as_deref().is_none_or(text_ok);
    if !ok {
        return Err(err(422, "invalid_job"));
    }
    // What only the server decides stays out of an app's hands.
    req.import_ran = None;
    let failed = req.phase.as_deref() == Some("failed");
    if failed && req.error.as_deref().is_none_or(str::is_empty) {
        req.error = Some("client".to_owned());
    }
    let job = s.store.update_cd_job(d.user_id, id, &req).await?.ok_or_else(|| err(404, "not_found"))?;
    s.cd.publish(d.user_id, if failed { "failed" } else { "progress" }, &job);
    Ok(Json(json!({ "job": job })))
}

// Import

/// A folder in the staging area that is removed when it goes out of scope, so that nothing stays behind after a
/// refusal or a broken connection.
struct Staged(PathBuf);

impl Drop for Staged {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn import(State(s): State<ApiState>, d: Device, headers: HeaderMap, mut form: Multipart) -> ApiResult<(StatusCode, Json<Value>)> {
    may_import(&d)?;
    let mut job: Option<Job> = None;
    let mut info = JobPatch::default();
    let result = import_inner(&s, &d, &headers, &mut form, &mut job, &mut info).await;
    if let Err(e) = &result {
        // A refusal is an entry of the log too, also when it came before there was a job.
        let patch = JobPatch { phase: Some("failed".into()), error: Some(e.1.to_owned()), ..JobPatch::default() };
        let failed = match &job {
            Some(j) => s.store.update_cd_job(d.user_id, j.id, &patch).await.ok().flatten(),
            None => {
                let first = JobPatch { phase: Some("failed".into()), ..info };
                match s.store.create_cd_job(d.user_id, &cdimport::device_label(&d.token), &first).await {
                    Ok(j) => s.store.update_cd_job(d.user_id, j.id, &patch).await.ok().flatten(),
                    Err(_) => None,
                }
            }
        };
        if let Some(failed) = failed {
            s.cd.publish(d.user_id, "failed", &failed);
        }
    }
    result
}

/// The extensions of audio files that are not the format of the settings: such a file is refused, not ignored.
const OTHER_AUDIO: &[&str] = &["mp3", "wav", "flac", "ogg", "opus", "m4a", "m4b", "aac", "wma"];

/// The name of a part: the `path` field that came before it (the apps send the place of the file inside the staging
/// folder there), else the file name of the part, which an app may have percent-encoded.
fn part_name(path: Option<String>, file_name: &str) -> String {
    let raw = path.unwrap_or_else(|| percent_decode(file_name));
    raw.replace('\\', "/").rsplit('/').next().unwrap_or("").to_owned()
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() && let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The track number of a file an app has named and tagged already: the number in the tag (minus the disc, for the
/// scheme disc x 100 + track), else the `TR07` of the name.
fn derive_number(path: &Path, name: &str) -> Option<u32> {
    let from_tag = crate::tags::read(path).ok().and_then(|t| t.track).filter(|n| *n > 0).map(|n| if n >= 100 { n % 100 } else { n }).filter(|n| *n > 0);
    from_tag.or_else(|| {
        let upper = name.to_ascii_uppercase();
        let at = upper.rfind("_TR")? + 3;
        let digits: String = upper[at..].chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok().filter(|n| *n > 0)
    })
}

struct Received {
    name: String,
    path: PathBuf,
}

async fn import_inner(s: &ApiState, d: &Device, headers: &HeaderMap, form: &mut Multipart, job: &mut Option<Job>, info: &mut JobPatch) -> ApiResult<(StatusCode, Json<Value>)> {
    let set = CdSettings::load(&s.store).await?;
    let reserve = crate::diskspace::reserve(&s.store).await;
    let announced = headers.get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    s.cd.sweep_staging();
    let staged = Staged(s.cd.staging.join(uuid::Uuid::new_v4().to_string()));
    tokio::fs::create_dir_all(&staged.0).await.map_err(|_| err(500, "internal"))?;
    if !crate::diskspace::fits(&staged.0, announced, reserve) {
        return Err(err(507, "no_space"));
    }

    // Two ways to say what the CD is: a JSON part `meta` first (it names every file and track), or plain text fields
    // (`kind`, `author`, `title`, `disc`, `discs_total`) before the files, which are then read for their track numbers.
    let too_big = || err(413, "too_large");
    let mut meta_json: Option<Meta> = None;
    let mut fields: HashMap<String, String> = HashMap::new();
    let mut cover: Option<(Vec<u8>, &'static str)> = None;
    let mut received: Vec<Received> = Vec::new();
    let mut pending_path: Option<String> = None;
    let mut total = 0u64;
    while let Some(mut field) = form.next_field().await.map_err(|_| err(400, "bad_request"))? {
        let name = field.name().unwrap_or("").to_owned();
        match name.as_str() {
            "meta" => {
                if meta_json.is_some() || !received.is_empty() {
                    return Err(err(422, "invalid_meta"));
                }
                let text = field.text().await.map_err(|_| err(422, "invalid_meta"))?;
                let m: Meta = serde_json::from_str(&text).map_err(|_| err(422, "invalid_meta"))?;
                info.author = Some(m.author.chars().take(200).collect());
                info.album = Some(m.album.chars().take(200).collect());
                info.disc = Some(i64::from(m.disc.clamp(1, 99)));
                info.discs_total = m.discs_total.map(|t| i64::from(t.clamp(1, 99)));
                info.tracks_total = Some(m.tracks.len() as i64);
                meta_json = Some(m);
            }
            "kind" | "author" | "title" | "album" | "narrator" | "year" | "disc" | "discs_total" | "mbid" | "disc_id" | "genre" | "job_id" => {
                let text = field.text().await.map_err(|_| err(422, "invalid_meta"))?;
                if text.len() > 500 {
                    return Err(err(422, "invalid_meta"));
                }
                let text = text.trim().to_owned();
                match name.as_str() {
                    "author" => info.author = Some(text.chars().filter(|c| !c.is_control()).take(200).collect()),
                    "title" | "album" => info.album = Some(text.chars().filter(|c| !c.is_control()).take(200).collect()),
                    "disc" => info.disc = text.parse::<i64>().ok().filter(|n| (1..=99).contains(n)),
                    "discs_total" => info.discs_total = text.parse::<i64>().ok().filter(|n| (1..=99).contains(n)),
                    _ => {}
                }
                fields.insert(name, text);
            }
            "path" => pending_path = Some(field.text().await.map_err(|_| err(422, "invalid_meta"))?),
            "cover" => {
                let bytes = field.bytes().await.map_err(|_| too_big())?;
                if bytes.len() > crate::tags::MAX_COVER_BYTES {
                    return Err(too_big());
                }
                let mime = crate::profile::sniff_image(&bytes).filter(|m| *m != "image/webp").ok_or_else(|| err(422, "bad_cover"))?;
                cover = Some((bytes.to_vec(), mime));
            }
            "file" => {
                let part = part_name(pending_path.take(), field.file_name().unwrap_or(""));
                let ending = part.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
                let audio = ending == set.extension;
                let picture = matches!(ending.as_str(), "jpg" | "jpeg" | "png");
                if !audio && !picture {
                    if OTHER_AUDIO.contains(&ending.as_str()) {
                        return Err(err(415, "bad_extension"));
                    }
                    // Something else (a playlist, a log): read and dropped.
                    while field.chunk().await.map_err(|_| too_big())?.is_some() {}
                    continue;
                }
                if audio {
                    if meta_json.as_ref().is_some_and(|m| !m.tracks.iter().any(|t| t.file == part)) {
                        return Err(err(422, "unknown_file"));
                    }
                    if received.iter().any(|r| r.name == part) {
                        return Err(err(422, "invalid_meta"));
                    }
                }
                let path = staged.0.join(if audio { format!("{:03}.{}", received.len() + 1, set.extension) } else { format!("picture.{ending}") });
                let mut file = tokio::fs::File::create(&path).await.map_err(|_| err(500, "internal"))?;
                let mut size = 0u64;
                while let Some(chunk) = field.chunk().await.map_err(|_| too_big())? {
                    size += chunk.len() as u64;
                    total += chunk.len() as u64;
                    if size > MAX_FILE_BYTES || total > MAX_IMPORT_BYTES as u64 {
                        return Err(too_big());
                    }
                    if size / (64 * 1024 * 1024) != (size - chunk.len() as u64) / (64 * 1024 * 1024) && !crate::diskspace::fits(&path, 0, reserve) {
                        return Err(err(507, "no_space"));
                    }
                    file.write_all(&chunk).await.map_err(|_| err(507, "no_space"))?;
                }
                file.flush().await.map_err(|_| err(507, "no_space"))?;
                if size == 0 {
                    return Err(err(422, "unreadable_file"));
                }
                if audio {
                    received.push(Received { name: part, path });
                } else if cover.is_none() {
                    // A picture among the files is the cover (the app sends `cover.jpg` this way).
                    let bytes = tokio::fs::read(&path).await.map_err(|_| err(500, "internal"))?;
                    if bytes.len() <= crate::tags::MAX_COVER_BYTES {
                        cover = crate::profile::sniff_image(&bytes).filter(|m| *m != "image/webp").map(|m| (bytes, m));
                    }
                }
            }
            _ => {
                // Unknown parts are read and dropped, so a newer app can send more.
                while field.chunk().await.map_err(|_| too_big())?.is_some() {}
            }
        }
    }

    // What the CD is.
    let num = |k: &str| fields.get(k).filter(|v| !v.is_empty()).map(|v| v.parse::<u32>());
    let (meta, staged_tracks) = match meta_json {
        Some(m) => {
            cdimport::check_meta(&m)?;
            let mut staged_tracks = Vec::new();
            for t in &m.tracks {
                let r = received.iter().find(|r| r.name == t.file).ok_or_else(|| err(422, "missing_file"))?;
                staged_tracks.push(StagedTrack { number: t.number, title: t.title.clone().unwrap_or_default(), path: r.path.clone() });
            }
            (m, staged_tracks)
        }
        None => {
            let text = |k: &str| fields.get(k).filter(|v| !v.is_empty()).cloned();
            let mut tracks = Vec::new();
            let mut staged_tracks = Vec::new();
            for r in &received {
                let number = derive_number(&r.path, &r.name).ok_or_else(|| err(422, "track_numbers"))?;
                let title = if set.title_pattern == "track_title" { crate::tags::read(&r.path).ok().and_then(|t| t.title).unwrap_or_default() } else { String::new() };
                tracks.push(cdimport::MetaTrack { number, title: Some(title.clone()), file: r.name.clone() });
                staged_tracks.push(StagedTrack { number, title, path: r.path.clone() });
            }
            let m = Meta {
                author: text("author").unwrap_or_default(),
                album: text("title").or_else(|| text("album")).unwrap_or_default(),
                narrator: text("narrator"),
                year: text("year"),
                disc: num("disc").transpose().map_err(|_| err(422, "invalid_meta"))?.unwrap_or(1),
                discs_total: num("discs_total").transpose().map_err(|_| err(422, "invalid_meta"))?,
                kind: text("kind"),
                mbid: text("mbid"),
                disc_id: text("disc_id"),
                genre: text("genre"),
                allow_gaps: false,
                job_id: num("job_id").transpose().map_err(|_| err(422, "invalid_meta"))?.map(i64::from),
                tracks,
            };
            cdimport::check_meta(&m)?;
            (m, staged_tracks)
        }
    };
    if received.len() != meta.tracks.len() {
        return Err(err(422, "missing_file"));
    }
    *job = Some(start_job(s, d, &meta).await?);
    let job_now = job.clone().ok_or_else(|| err(500, "internal"))?;

    let kind = meta.kind.clone().unwrap_or_else(|| set.target_kind.clone());
    let root = import_root(s, &kind).await?;
    if cover.is_none()
        && set.lookup_ready
        && let Some(mbid) = &meta.mbid
    {
        // Best effort: an import does not fail because the picture could not be fetched.
        cover = fetch_cover(s, &set, mbid).await.ok().flatten();
    }
    let kind2 = kind.clone();
    // A disc that comes with its Disc ID (from an app) is written down too.
    let extra = cdimport::FinalizeExtra {
        record: meta.disc_id.clone().filter(|id| !id.is_empty()).map(|disc_id| crate::cdpresent::DiscRow { disc_id, author: meta.author.trim().to_owned(), title: meta.album.trim().to_owned(), disc: meta.disc, discs_total: meta.discs_total.unwrap_or(1), kind: kind.clone(), folder: String::new() }),
        ..Default::default()
    };
    let (updated, placed, import_queued) = cdimport::finalize(&s.store, &s.cd, d.user_id, job_now.id, &meta, &set, root, kind2, cover, staged_tracks, reserve, extra).await?;
    drop(staged);
    Ok((
        StatusCode::CREATED,
        Json(json!({ "id": updated.id, "job": updated, "placed": placed, "import": if import_queued { "queued" } else { "locked" }, "eject": set.eject_after })),
    ))
}

/// Announces the job of an upload: the one the app made before, or a new one.
async fn start_job(s: &ApiState, d: &Device, m: &Meta) -> ApiResult<Job> {
    let patch = JobPatch {
        phase: Some("uploading".into()),
        track: Some(0),
        tracks_total: Some(m.tracks.len() as i64),
        author: Some(m.author.trim().to_owned()),
        album: Some(m.album.trim().to_owned()),
        disc: Some(i64::from(m.disc)),
        discs_total: Some(i64::from(m.discs_total.unwrap_or(1))),
        ..JobPatch::default()
    };
    s.store.expire_cd_jobs().await?;
    let device = cdimport::device_label(&d.token);
    let (job, kind) = match m.job_id {
        Some(id) => {
            let current = s.store.cd_job(d.user_id, id).await?.ok_or_else(|| err(404, "not_found"))?;
            if !current.active {
                return Err(err(409, "job_closed"));
            }
            (s.store.update_cd_job(d.user_id, id, &patch).await?.ok_or_else(|| err(404, "not_found"))?, "progress")
        }
        None => (s.store.create_cd_job(d.user_id, &device, &patch).await?, "job"),
    };
    s.cd.publish(d.user_id, kind, &job);
    Ok(job)
}

/// The folder an import goes to: the first existing folder of the kind, else the development library.
async fn import_root(s: &ApiState, kind: &str) -> ApiResult<PathBuf> {
    Ok(cdimport::import_root(&s.store, s.library.as_deref(), kind).await?)
}

// The drive of the server

/// The state of the drives, as in the `drive` message of the event channel.
async fn drive(State(s): State<ApiState>, d: Device) -> ApiResult<Json<Value>> {
    may_import(&d)?;
    let set = CdSettings::load(&s.store).await?;
    s.cd.apply_settings(&set);
    let fp = crate::cdresume::Fingerprint::of(&set);
    let drives = s.cd.drives_json_full(&s.store, s.library.as_deref(), &set).await;
    let _ = fp;
    Ok(Json(json!({ "mode": s.cd.drives.snapshot().1, "busy": s.cd.drives.is_ripping(), "drives": drives })))
}

/// Starts ripping the CD that is in the server's drive. Answers `201 {id, job}` at once; the progress comes as
/// events. `409 busy` when the drive is ripping already, `409 no_disc` / `tray_open` / `no_drive`, `503 drive_denied`.
async fn rip(State(s): State<ApiState>, d: Device, Json(req): Json<crate::cdrive::RipRequest>) -> Response {
    if let Err(e) = may_import(&d) {
        return e.into_response();
    }
    match crate::cdrive::start_rip(&s.store, &s.cd, s.library.clone(), d.user_id, req).await {
        Ok(st) => {
            let mut job = st.job;
            s.cd.decorate(&mut job);
            (StatusCode::CREATED, Json(json!({ "id": job.id, "job": job, "resume": st.resume }))).into_response()
        }
        Err(r) => {
            // `disc_exists` names the tracks that are in the library already.
            let mut body = json!({ "error": r.refusal.1 });
            if !r.taken.is_empty() {
                body["taken"] = json!(r.taken);
            }
            // `disc_present`: the CD is in the library already; what is missing or damaged is told.
            if let Some(p) = &r.present {
                body["book"] = p["book"].clone();
                body["author"] = p["author"].clone();
                body["disc"] = p["disc"].clone();
                body["missing"] = p["missing"].clone();
                body["damaged"] = p["damaged"].clone();
                body["present"] = p.clone();
            }
            (StatusCode::from_u16(r.refusal.0).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR), [(header::CACHE_CONTROL, "no-store")], Json(body)).into_response()
        }
    }
}

async fn rip_cancel(State(s): State<ApiState>, d: Device, UrlPath(id): UrlPath<i64>) -> ApiResult<(StatusCode, Json<Value>)> {
    may_import(&d)?;
    if !s.cd.drives.cancel(id) {
        return Err(err(404, "not_found"));
    }
    Ok((StatusCode::ACCEPTED, Json(json!({ "id": id, "cancelling": true }))))
}

/// Closes the list of tracks of a rip that has ended (it stays with its result until then).
async fn rip_dismiss(State(s): State<ApiState>, d: Device, body: Option<Json<EjectRequest>>) -> ApiResult<StatusCode> {
    may_import(&d)?;
    let device = body.and_then(|Json(b)| b.device);
    let (states, _) = s.cd.drives.snapshot();
    let found = states.iter().filter(|x| device.as_deref().is_none_or(|w| w.is_empty() || x.device == w)).any(|x| s.cd.drives.dismiss(&x.device));
    if found {
        s.cd.publish_drive();
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(err(404, "not_found"))
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct EjectRequest {
    device: Option<String>,
}

/// Opens the tray when a disc is in and the tray is shut; never closes it. `409 busy` while a rip runs.
async fn eject(State(s): State<ApiState>, d: Device, body: Option<Json<EjectRequest>>) -> ApiResult<Json<Value>> {
    may_import(&d)?;
    let device = body.and_then(|Json(b)| b.device);
    let outcome = crate::cdrive::eject(&s.store, &s.cd, device).await?;
    Ok(Json(json!({ "result": outcome, "ejected": outcome == crate::cdrive::EjectResult::Ejected })))
}

// Events (WebSocket)

async fn events(State(s): State<ApiState>, d: Device, req: Request) -> Response {
    if let Err(e) = may_import(&d) {
        return e.into_response();
    }
    let Some(accept) = ws::accept_value(req.headers()) else { return err(400, "bad_upgrade").into_response() };
    let on_upgrade = hyper::upgrade::on(req);
    let (user, token) = (d.user_id, d.token.clone());
    tokio::spawn(async move {
        match on_upgrade.await {
            Ok(upgraded) => session(s, user, token, TokioIo::new(upgraded)).await,
            Err(e) => tracing::debug!(error = %e, "CD events: the upgrade failed"),
        }
    });
    Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header(header::UPGRADE, "websocket")
        .header(header::CONNECTION, "Upgrade")
        .header("sec-websocket-accept", accept)
        .body(Body::empty())
        .unwrap_or_default()
}

async fn snapshot_text(s: &ApiState, user: i64, device: &str) -> Option<String> {
    let list = s.store.cd_jobs(user).await.ok()?;
    Some(json!({ "v": cdimport::EVENTS_VERSION, "type": "snapshot", "busy": list.iter().any(|j| j.active), "busy_elsewhere": busy_elsewhere(&list, device), "jobs": list }).to_string())
}

/// One connection: a hello and a snapshot, then every change of the account's jobs, a ping at the interval of the
/// heartbeat, and a close when the token has ended, the client has gone quiet or the client asks for it.
async fn session<IO>(s: ApiState, user: i64, token: String, io: IO)
where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + 'static,
{
    use std::time::Instant;
    let device = cdimport::device_label(&token);
    let (mut rd, mut wr) = tokio::io::split(io);
    // Reading is its own task: a frame is never cut in half by the select below.
    let (tx, mut frames) = tokio::sync::mpsc::channel::<std::io::Result<Option<ws::Frame>>>(8);
    let reader = tokio::spawn(async move {
        loop {
            let r = ws::read_frame(&mut rd).await;
            let stop = !matches!(r, Ok(Some(_)));
            if tx.send(r).await.is_err() || stop {
                break;
            }
        }
    });
    let mut events = s.cd.subscribe();
    let heartbeat = s.cd.heartbeat;
    let hello = json!({ "v": cdimport::EVENTS_VERSION, "type": "hello", "device": device, "heartbeat_s": heartbeat.as_secs_f64() }).to_string();
    let mut ok = ws::write_text(&mut wr, &hello).await.is_ok();
    if ok && let Some(snap) = snapshot_text(&s, user, &device).await {
        ok = ws::write_text(&mut wr, &snap).await.is_ok();
    }
    if ok {
        ok = ws::write_text(&mut wr, &s.cd.drive_message()).await.is_ok();
    }
    let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + heartbeat, heartbeat);
    let mut last_heard = Instant::now();
    let mut closing: Option<(u16, &str)> = None;
    while ok && closing.is_none() {
        tokio::select! {
            event = events.recv() => match event {
                Ok(e) if e.user_id == user || e.user_id == cdimport::BROADCAST => ok = ws::write_text(&mut wr, &e.text).await.is_ok(),
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    if let Some(snap) = snapshot_text(&s, user, &device).await {
                        ok = ws::write_text(&mut wr, &snap).await.is_ok();
                    }
                }
                Err(_) => closing = Some((ws::CLOSE_GOING_AWAY, "server stopping")),
            },
            frame = frames.recv() => {
                last_heard = Instant::now();
                match frame {
                    Some(Ok(Some(ws::Frame::Ping(payload)))) => ok = ws::write_frame(&mut wr, 10, &payload).await.is_ok(),
                    Some(Ok(Some(ws::Frame::Close(_)))) => closing = Some((ws::CLOSE_NORMAL, "")),
                    Some(Ok(Some(_))) => {}
                    Some(Ok(None)) | None => ok = false,
                    Some(Err(e)) if e.kind() == std::io::ErrorKind::FileTooLarge => closing = Some((ws::CLOSE_TOO_BIG, "frame too large")),
                    Some(Err(_)) => closing = Some((ws::CLOSE_PROTOCOL, "protocol error")),
                }
            },
            _ = tick.tick() => {
                // A token that was revoked or signed out ends the connection.
                if matches!(s.store.session_for_token(&token).await, Ok(None)) {
                    closing = Some((ws::CLOSE_REVOKED, "session ended"));
                } else if last_heard.elapsed() > heartbeat * 3 {
                    closing = Some((ws::CLOSE_TIMEOUT, "no answer to the heartbeat"));
                } else {
                    ok = ws::write_frame(&mut wr, 9, b"david").await.is_ok();
                }
            },
        }
    }
    if let Some((code, why)) = closing {
        let _ = ws::write_close(&mut wr, code, why).await;
    }
    let _ = wr.shutdown().await;
    reader.abort();
}
