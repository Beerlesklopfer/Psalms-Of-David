// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The server side of the CD import (docs/spec-cd-import-api.md): the state of the import jobs, the cache of the
//! lookups, the hub that pushes changes to the devices, and the checking, naming, tagging and placing of the files
//! an app has ripped. The routes are in `api_cd`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::cd_settings::{self, CdSettings};
use crate::error::Result;
use crate::musicbrainz::{self, Candidate, HttpLookup, Lookup, Service, Toc};
use crate::store::{Store, now_ms};

/// Phases in which a job is under way. The app sets the first three, the server the other two.
pub const ACTIVE: &[&str] = &["ripping", "encoding", "uploading", "placing", "importing"];
/// Phases an app may report.
pub const CLIENT_PHASES: &[&str] = &["ripping", "encoding", "uploading", "failed"];
/// A job that has not been touched for this long is given up (an app that crashed must not block "busy" for ever).
pub const STALE: Duration = Duration::from_secs(15 * 60);
/// How long the answers of the lookups are kept: found, and "not there".
pub const CACHE_FOUND: Duration = Duration::from_secs(30 * 24 * 3600);
pub const CACHE_MISSING: Duration = Duration::from_secs(24 * 3600);
/// Leftovers in the staging folder are removed after this time.
pub const STAGING_KEEP: Duration = Duration::from_secs(24 * 3600);
/// The interval of the heartbeat on the event channel.
pub const HEARTBEAT: Duration = Duration::from_secs(25);
/// The version of the messages of the event channel.
pub const EVENTS_VERSION: u32 = 1;

/// Every code the CD import can show: an API error, a reason a job failed, or a state of the import. Each has the
/// texts `cd.err.<code>` in the catalogs (English and German); the codes themselves are English and stay the same
/// for the apps, which write their own texts by them. `other` is the text for a code that is not listed here.
pub const ERROR_CODES: &[&str] = &[
    "forbidden",
    "unauthorized",
    "no_contact",
    "invalid_toc",
    "invalid_query",
    "invalid_release",
    "lookup_failed",
    "not_found",
    "invalid_job",
    "job_closed",
    "invalid_meta",
    "track_numbers",
    "unreadable_file",
    "format_mismatch",
    "bad_extension",
    "unknown_file",
    "missing_file",
    "bad_cover",
    "bad_tags",
    "disc_exists",
    "no_space",
    "no_import_folder",
    "too_large",
    "bad_request",
    "bad_upgrade",
    "internal",
    "import_failed",
    "interrupted",
    "stale",
    "locked",
    "busy",
    "no_drive",
    "no_disc",
    "tray_open",
    "drive_denied",
    "eject_failed",
    "resume_mismatch",
    "disc_present",
    "not_present",
    "nothing_to_heal",
    "unreadable_track",
    "unverified_track",
    "heal_needed",
    "ambiguous_match",
    "no_match",
    "no_target",
    "udev_off",
    "resume_pending",
    "not_audio",
    "start_failed",
    "invalid_selection",
    "empty_selection",
    "verify",
    "client",
    "cancelled",
    "read",
    "drive_gone",
    "encode",
    "io",
    "unsupported",
    "other",
];

/// The phases a job can be in, with texts `cd.phase.<phase>`.
pub const PHASES: &[&str] = &["ripping", "verifying", "encoding", "uploading", "placing", "importing", "done", "failed", "skipped"];

/// The states of a track in the list of a rip, with texts `cd.state.<state>`.
pub const TRACK_STATES: &[&str] = &["waiting", "reading", "verifying", "encoding", "done", "failed"];

/// Things that keep the import from working, found by `readiness`; texts `cd.status.<code>`.
pub const STATUS_CODES: &[&str] = &["ready", "no_contact", "no_folder", "not_writable", "locked", "drive_denied"];

/// The catalog key of the text for a code.
pub fn error_key(code: &str) -> String {
    format!("cd.err.{}", if ERROR_CODES.contains(&code) { code } else { "other" })
}

/// What stands in the way of an import now: the codes of `STATUS_CODES` except `ready`; empty = ready.
pub async fn readiness(store: &Store, cd: &CdService, library: Option<&Path>) -> Vec<&'static str> {
    let mut out = Vec::new();
    if cd.drives.snapshot().0.iter().any(|d| d.denied) {
        out.push("drive_denied");
    }
    let Ok(set) = CdSettings::load(store).await else { return vec!["no_folder"] };
    if !set.lookup_ready {
        out.push("no_contact");
    }
    if let Ok(state) = store.import_state().await {
        let folders = if set.target_kind == "music" { &state.music_roots } else { &state.roots };
        match folders.iter().map(PathBuf::from).find(|p| p.is_dir()).or_else(|| library.filter(|p| p.is_dir()).map(Path::to_path_buf)) {
            None => out.push("no_folder"),
            Some(dir) if !writable(&dir) => out.push("not_writable"),
            Some(_) => {}
        }
        if !state.enabled {
            out.push("locked");
        }
    }
    out
}

/// May this process write into the folder?
pub fn writable_dir(dir: &Path) -> bool {
    writable(dir)
}

/// May this process write into the folder?
fn writable(dir: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        std::ffi::CString::new(dir.as_os_str().as_bytes()).is_ok_and(|c| unsafe { libc::access(c.as_ptr(), libc::W_OK) } == 0)
    }
    #[cfg(not(unix))]
    {
        std::fs::metadata(dir).is_ok_and(|m| !m.permissions().readonly())
    }
}

/// One message for the devices of an account.
#[derive(Debug, Clone)]
pub struct Event {
    pub user_id: i64,
    /// The finished JSON text.
    pub text: String,
}

/// The user of work that the server starts by itself (the automatic import): its jobs are everybody's.
pub const SYSTEM_USER: i64 = 0;

/// The user id of an event for every device (the state of the drive belongs to nobody).
pub const BROADCAST: i64 = -1;

/// What the routes share: the way to the internet, the staging folder and the hub.
#[derive(Clone)]
pub struct CdService {
    pub lookup: Arc<dyn Lookup>,
    pub staging: PathBuf,
    pub heartbeat: Duration,
    /// The drives of the server and the rips that run on them.
    pub drives: crate::cdrive::Drives,
    /// How long the finished tracks of an interrupted rip are kept, in hours (the setting, as the monitor last read it).
    pub resume_keep: Arc<std::sync::atomic::AtomicU64>,
    /// The development library folder (`--library`), where an import folder is looked for besides the ones of the settings.
    pub library: Option<PathBuf>,
    /// The settings that decide the audio, as last read (to tell whether an interrupted rip would be resumed).
    pub fingerprint: Arc<std::sync::Mutex<Option<crate::cdresume::Fingerprint>>>,
    /// What is known of the CDs that are in the library, by Disc ID, as last worked out (the `present` of the drive message).
    pub present_cache: Arc<std::sync::Mutex<HashMap<String, serde_json::Value>>>,
    hub: broadcast::Sender<Arc<Event>>,
}

impl CdService {
    /// The real service: MusicBrainz over the internet, staging below the data folder.
    pub fn new(data_dir: &Path) -> CdService {
        CdService::with_lookup(data_dir, Arc::new(HttpLookup::new()))
    }

    pub fn with_lookup(data_dir: &Path, lookup: Arc<dyn Lookup>) -> CdService {
        CdService::with_backend(data_dir, lookup, Arc::new(crate::cdrive::SystemDrives))
    }

    /// With another drive behind it (the tests use a fake).
    pub fn with_backend(data_dir: &Path, lookup: Arc<dyn Lookup>, backend: Arc<dyn crate::cdrive::DriveBackend>) -> CdService {
        let (hub, _) = broadcast::channel(256);
        CdService { lookup, staging: data_dir.join("cd-import"), heartbeat: HEARTBEAT, drives: crate::cdrive::Drives::new(backend), resume_keep: Arc::new(std::sync::atomic::AtomicU64::new(48)), library: None, fingerprint: Arc::default(), present_cache: Arc::default(), hub }
    }

    pub fn with_library(mut self, library: Option<PathBuf>) -> CdService {
        self.library = library;
        self
    }

    pub fn with_heartbeat(mut self, heartbeat: Duration) -> CdService {
        self.heartbeat = heartbeat;
        self
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Event>> {
        self.hub.subscribe()
    }

    /// Adds what the service knows of a rip that runs: the fraction within the job and the state of every track.
    pub fn decorate(&self, job: &mut Job) {
        if let Some(l) = self.drives.live_of_job(job.id) {
            if job.active {
                job.fraction = Some(l.fraction.clamp(0.0, 1.0));
            }
            if job.active {
                job.buffer_seconds = Some((l.buffer_seconds() * 10.0).round() / 10.0);
                job.buffer_max = Some(l.buffer_max_seconds().round() as u32);
                job.buffer_percent = Some(l.buffer_percent());
                job.speed_x = l.speed_x();
                let (waits, longest) = l.reader_waits();
                job.reader_waits = Some(waits);
                job.reader_wait_max_s = Some(longest);
            }
            job.track_states = l.tracks;
        }
    }

    pub fn publish(&self, user_id: i64, kind: &str, job: &Job) {
        let mut job = job.clone();
        self.decorate(&mut job);
        let job = &job;
        // The jobs of the system concern every account.
        let user_id = if user_id == SYSTEM_USER { BROADCAST } else { user_id };
        let text = serde_json::json!({ "v": EVENTS_VERSION, "type": kind, "job": job }).to_string();
        self.publish_text(user_id, text);
    }

    pub fn publish_text(&self, user_id: i64, text: String) {
        let _ = self.hub.send(Arc::new(Event { user_id, text }));
    }

    /// Jobs that were under way when the server stopped cannot go on: they are closed as `interrupted` so that the devices
    /// see it. What their rips had finished stays in the staging area for a resume (see `cdresume`).
    pub async fn start(&self, store: &Store) {
        if let Ok(set) = CdSettings::load(store).await {
            self.apply_settings(&set);
        }
        match store.fail_active_cd_jobs("the server was restarted").await {
            Ok(n) if n > 0 => tracing::info!(jobs = n, "CD imports interrupted by a restart were closed; their finished tracks are kept for a resume"),
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "could not close old CD imports"),
        }
        self.sweep_staging();
    }

    /// Takes over the settings that the drive service needs to know without asking the database.
    pub fn apply_settings(&self, set: &CdSettings) {
        self.resume_keep.store(u64::from(set.resume_keep_hours), std::sync::atomic::Ordering::Relaxed);
        *self.fingerprint.lock().unwrap_or_else(|e| e.into_inner()) = Some(crate::cdresume::Fingerprint::of(set));
    }

    /// Removes what is not worth keeping from the staging area (see `cdresume::sweep`).
    pub fn sweep_staging(&self) {
        crate::cdresume::sweep(&self.staging, self.resume_keep.load(std::sync::atomic::Ordering::Relaxed), now_ms());
    }
}

/// An identifier of a device that is safe to show to the other devices of the account: a piece of the hash of its
/// token (the token itself must never be shown).
pub fn device_label(token: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(token.as_bytes()).iter().take(6).map(|b| format!("{b:02x}")).collect()
}

/// A job as the devices see it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Job {
    pub id: i64,
    /// Which device works on it.
    pub device: String,
    pub phase: String,
    pub active: bool,
    pub track: i64,
    pub tracks_total: i64,
    pub author: String,
    pub album: String,
    pub disc: i64,
    pub discs_total: i64,
    /// A short code when the job failed.
    pub error: String,
    /// A sentence for a person: what was done.
    pub message: String,
    /// Has the import of the library run (else it is locked or still waiting)?
    pub import_ran: bool,
    pub created_ms: i64,
    pub updated_ms: i64,
    /// The state in the words of the apps: `queued`, `working`, `done` or `failed`.
    pub state: &'static str,
    /// How far the job is, 0 to 1, from the track count (`null` while the number of tracks is not known).
    pub fraction: Option<f64>,
    /// The title of the book or album (the same as `album`).
    pub title: String,
    /// The state of each track while the server rips this job (and after it, until the list is closed).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub track_states: Vec<crate::cdrive::TrackState>,
    /// Seconds of audio waiting between the drive and the encoder now, and the most the buffer holds (a rip that the server does).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffer_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffer_max: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffer_percent: Option<u32>,
    /// The speed the drive reads at, as a multiple of 1x, measured (absent while too little has been read).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_x: Option<f64>,
    /// How often the reader had to wait for room in the buffer, and the longest wait in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reader_waits: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reader_wait_max_s: Option<f64>,
}

/// What an app tells about a job.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct JobPatch {
    pub phase: Option<String>,
    pub track: Option<i64>,
    pub tracks_total: Option<i64>,
    pub author: Option<String>,
    pub album: Option<String>,
    pub disc: Option<i64>,
    pub discs_total: Option<i64>,
    pub error: Option<String>,
    pub message: Option<String>,
    pub import_ran: Option<bool>,
}

const JOB_COLUMNS: &str = "id, device, phase, track, tracks_total, author, album, disc, discs_total, error, message, import_ran, created_ms, updated_ms";

fn job_from_row(r: &sea_orm::QueryResult) -> Result<Job> {
    let phase: String = r.try_get_by_index(2)?;
    let (track, total): (i64, i64) = (r.try_get_by_index(3)?, r.try_get_by_index(4)?);
    let album: String = r.try_get_by_index(6)?;
    Ok(Job {
        state: match phase.as_str() {
            "done" => "done",
            "failed" => "failed",
            "skipped" => "skipped",
            _ => "working",
        },
        fraction: (total > 0).then(|| (track as f64 / total as f64).clamp(0.0, 1.0)),
        title: album.clone(),
        track_states: Vec::new(),
        buffer_seconds: None,
        buffer_max: None,
        buffer_percent: None,
        speed_x: None,
        reader_waits: None,
        reader_wait_max_s: None,
        id: r.try_get_by_index(0)?,
        device: r.try_get_by_index(1)?,
        active: ACTIVE.contains(&phase.as_str()),
        phase,
        track,
        tracks_total: total,
        author: r.try_get_by_index(5)?,
        album,
        disc: r.try_get_by_index(7)?,
        discs_total: r.try_get_by_index(8)?,
        error: r.try_get_by_index(9)?,
        message: r.try_get_by_index(10)?,
        import_ran: r.try_get_by_index::<i64>(11)? != 0,
        created_ms: r.try_get_by_index(12)?,
        updated_ms: r.try_get_by_index(13)?,
    })
}

impl Store {
    pub async fn create_cd_job(&self, user_id: i64, device: &str, patch: &JobPatch) -> Result<Job> {
        let now = now_ms();
        let res = self
            .db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO cd_job (user_id, device, phase, track, tracks_total, author, album, disc, discs_total, created_ms, updated_ms) \
                 VALUES (?, ?, ?, 0, ?, ?, ?, ?, ?, ?, ?)",
                [
                    user_id.into(),
                    device.to_owned().into(),
                    patch.phase.clone().unwrap_or_else(|| "ripping".to_owned()).into(),
                    patch.tracks_total.unwrap_or(0).into(),
                    patch.author.clone().unwrap_or_default().into(),
                    patch.album.clone().unwrap_or_default().into(),
                    patch.disc.unwrap_or(1).into(),
                    patch.discs_total.unwrap_or(1).into(),
                    now.into(),
                    now.into(),
                ],
            ))
            .await?;
        let id = res.last_insert_id() as i64;
        // The table is also the log of the import: only the last entries are kept.
        let keep = CdSettings::load(self).await.map_or(50, |s| s.log_keep);
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "DELETE FROM cd_job WHERE id NOT IN (SELECT id FROM cd_job ORDER BY id DESC LIMIT ?)",
                [i64::from(keep).into()],
            ))
            .await?;
        self.cd_job(user_id, id).await?.ok_or_else(|| crate::error::Error::WriteFailed("the job was not saved".into()))
    }

    /// A job of this account.
    pub async fn cd_job(&self, user_id: i64, id: i64) -> Result<Option<Job>> {
        let row = self
            .db
            .writer
            .query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, format!("SELECT {JOB_COLUMNS} FROM cd_job WHERE id = ? AND (user_id = ? OR user_id = 0)"), [id.into(), user_id.into()]))
            .await?;
        row.as_ref().map(job_from_row).transpose()
    }

    /// The jobs of an account that are under way, and the last ones that ended (newest first).
    pub async fn cd_jobs(&self, user_id: i64) -> Result<Vec<Job>> {
        self.expire_cd_jobs().await?;
        let rows = self
            .db
            .writer
            .query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("SELECT {JOB_COLUMNS} FROM cd_job WHERE (user_id = ? OR user_id = 0) AND phase <> 'skipped' ORDER BY updated_ms DESC, id DESC LIMIT 20"),
                [user_id.into()],
            ))
            .await?;
        rows.iter().map(job_from_row).collect()
    }

    /// The log for the settings page: the last entries of all accounts, newest first.
    pub async fn cd_log(&self, limit: u32) -> Result<Vec<Job>> {
        self.expire_cd_jobs().await?;
        let rows = self
            .db
            .writer
            .query_all_raw(Statement::from_sql_and_values(DbBackend::Sqlite, format!("SELECT {JOB_COLUMNS} FROM cd_job ORDER BY id DESC LIMIT ?"), [i64::from(limit).into()]))
            .await?;
        rows.iter().map(job_from_row).collect()
    }

    /// Applies what is known; fields that are not given stay. Returns the job as it is now.
    pub async fn update_cd_job(&self, user_id: i64, id: i64, patch: &JobPatch) -> Result<Option<Job>> {
        let mut sets = vec!["updated_ms = ?".to_owned()];
        let mut values: Vec<Value> = vec![now_ms().into()];
        let mut add = |name: &str, v: Value| {
            sets.push(format!("{name} = ?"));
            values.push(v);
        };
        if let Some(v) = &patch.phase {
            add("phase", v.clone().into());
        }
        if let Some(v) = patch.track {
            add("track", v.into());
        }
        if let Some(v) = patch.tracks_total {
            add("tracks_total", v.into());
        }
        if let Some(v) = &patch.author {
            add("author", v.clone().into());
        }
        if let Some(v) = &patch.album {
            add("album", v.clone().into());
        }
        if let Some(v) = patch.disc {
            add("disc", v.into());
        }
        if let Some(v) = patch.discs_total {
            add("discs_total", v.into());
        }
        if let Some(v) = &patch.error {
            add("error", v.clone().into());
        }
        if let Some(v) = &patch.message {
            add("message", v.clone().into());
        }
        if let Some(v) = patch.import_ran {
            add("import_ran", i64::from(v).into());
        }
        values.push(id.into());
        values.push(user_id.into());
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, format!("UPDATE cd_job SET {} WHERE id = ? AND user_id = ?", sets.join(", ")), values))
            .await?;
        self.cd_job(user_id, id).await
    }

    /// Gives up jobs that have not moved for `STALE`.
    /// Removes the finished and failed jobs from the log; a job that runs stays. How many were removed.
    pub async fn clear_cd_log(&self) -> Result<u64> {
        let marks = ACTIVE.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let values: Vec<Value> = ACTIVE.iter().map(|p| Value::from((*p).to_owned())).collect();
        let done = self.db.writer.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, format!("DELETE FROM cd_job WHERE phase NOT IN ({marks})"), values)).await?;
        Ok(done.rows_affected())
    }

    pub async fn expire_cd_jobs(&self) -> Result<()> {
        let limit = now_ms() - STALE.as_millis() as i64;
        let marks = ACTIVE.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let mut values: Vec<Value> = vec![now_ms().into()];
        values.extend(ACTIVE.iter().map(|p| Value::from((*p).to_owned())));
        values.push(limit.into());
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("UPDATE cd_job SET phase = 'failed', error = 'stale', message = 'nothing was heard of this job for a long time', updated_ms = ? WHERE phase IN ({marks}) AND updated_ms < ?"),
                values,
            ))
            .await?;
        Ok(())
    }

    pub async fn fail_active_cd_jobs(&self, why: &str) -> Result<u64> {
        let marks = ACTIVE.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let mut values: Vec<Value> = vec![why.to_owned().into(), now_ms().into()];
        values.extend(ACTIVE.iter().map(|p| Value::from((*p).to_owned())));
        let res = self
            .db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("UPDATE cd_job SET phase = 'failed', error = 'interrupted', message = ?, updated_ms = ? WHERE phase IN ({marks})"),
                values,
            ))
            .await?;
        Ok(res.rows_affected())
    }

    /// An answer of a lookup that is still fresh: `Some(None)` is a remembered "not there".
    pub async fn cd_cache_get(&self, key: &str) -> Result<Option<Option<Vec<u8>>>> {
        let row = self
            .db
            .writer
            .query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT found, body, fetched_ms FROM cd_cache WHERE key = ?", [key.to_owned().into()]))
            .await?;
        let Some(row) = row else { return Ok(None) };
        let (found, body, at): (i64, Vec<u8>, i64) = (row.try_get_by_index(0)?, row.try_get_by_index(1)?, row.try_get_by_index(2)?);
        let ttl = if found != 0 { CACHE_FOUND } else { CACHE_MISSING };
        if now_ms() - at > ttl.as_millis() as i64 {
            return Ok(None);
        }
        Ok(Some((found != 0).then_some(body)))
    }

    /// Forgets all answers of the lookups.
    pub async fn cd_cache_clear(&self) -> Result<()> {
        self.db.writer.execute_raw(Statement::from_string(DbBackend::Sqlite, "DELETE FROM cd_cache")).await?;
        Ok(())
    }

    pub async fn cd_cache_put(&self, key: &str, body: Option<&[u8]>) -> Result<()> {
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO cd_cache (key, found, body, fetched_ms) VALUES (?, ?, ?, ?) \
                 ON CONFLICT (key) DO UPDATE SET found = excluded.found, body = excluded.body, fetched_ms = excluded.fetched_ms",
                [key.to_owned().into(), i64::from(body.is_some()).into(), body.unwrap_or_default().to_vec().into(), now_ms().into()],
            ))
            .await?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Checking, naming, tagging and placing the files of one CD.

/// What the app says about the CD (the JSON field `meta` of the upload).
#[derive(Debug, Clone, Deserialize)]
pub struct Meta {
    pub author: String,
    #[serde(alias = "title")]
    pub album: String,
    #[serde(default)]
    pub narrator: Option<String>,
    #[serde(default)]
    pub year: Option<String>,
    #[serde(default = "one")]
    pub disc: u32,
    #[serde(default)]
    pub discs_total: Option<u32>,
    /// `audiobook` or `music`; absent = the setting.
    #[serde(default)]
    pub kind: Option<String>,
    /// For the cover, if the upload has none.
    #[serde(default)]
    pub mbid: Option<String>,
    #[serde(default)]
    pub disc_id: Option<String>,
    /// The genre of a music CD (an audiobook gets the genre of the settings).
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub job_id: Option<i64>,
    /// The tracks are a choice from a disc and keep their numbers on the disc: gaps are fine (the server's own rips).
    #[serde(skip)]
    pub allow_gaps: bool,
    pub tracks: Vec<MetaTrack>,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Deserialize)]
pub struct MetaTrack {
    pub number: u32,
    #[serde(default)]
    pub title: Option<String>,
    /// The file name the part of the upload carries.
    pub file: String,
}

/// Why something was refused: a status class and a code the app switches on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal(pub u16, pub &'static str);

/// Checks the numbers and texts of the metadata. The error is the code of the refusal.
pub fn check_meta(m: &Meta) -> std::result::Result<(), Refusal> {
    let bad = Refusal(422, "invalid_meta");
    let text_ok = |s: &str, max: usize| !s.trim().is_empty() && s.chars().count() <= max && !s.chars().any(char::is_control);
    if !text_ok(&m.author, 200) || !text_ok(&m.album, 200) {
        return Err(bad);
    }
    if m.narrator.as_deref().is_some_and(|s| !s.trim().is_empty() && !text_ok(s, 200)) || m.year.as_deref().is_some_and(|y| !y.is_empty() && (y.len() != 4 || !y.bytes().all(|b| b.is_ascii_digit()))) {
        return Err(bad);
    }
    if !(1..=99).contains(&m.disc) || m.discs_total.is_some_and(|t| t < m.disc || t > 99) {
        return Err(bad);
    }
    if m.kind.as_deref().is_some_and(|k| !cd_settings::KINDS.contains(&k)) || m.mbid.as_deref().is_some_and(|b| !crate::musicbrainz::is_mbid(b)) {
        return Err(bad);
    }
    if m.tracks.is_empty() || m.tracks.len() > 99 || m.tracks.iter().any(|t| t.file.is_empty() || t.file.chars().count() > 255 || t.title.as_deref().is_some_and(|s| s.chars().count() > 300 || s.chars().any(char::is_control))) {
        return Err(bad);
    }
    // 1, 2, 3, ... without a gap and without a double.
    let mut numbers: Vec<u32> = m.tracks.iter().map(|t| t.number).collect();
    numbers.sort_unstable();
    if m.allow_gaps {
        // A choice from a disc: numbers 1..99, no double.
        if numbers.iter().any(|n| !(1..=99).contains(n)) || numbers.windows(2).any(|w| w[0] == w[1]) {
            return Err(Refusal(422, "track_numbers"));
        }
    } else if numbers.iter().enumerate().any(|(i, n)| *n != i as u32 + 1) {
        return Err(Refusal(422, "track_numbers"));
    }
    let mut files: Vec<&str> = m.tracks.iter().map(|t| t.file.as_str()).collect();
    files.sort_unstable();
    if files.windows(2).any(|w| w[0] == w[1]) {
        return Err(Refusal(422, "invalid_meta"));
    }
    Ok(())
}

fn user_agent(contact: &str) -> String {
    format!("{}/{} ( {contact} )", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
}

fn base(configured: &str, default: &str) -> String {
    if configured.is_empty() { default.to_owned() } else { configured.to_owned() }
}

/// One request to a service, through the cache. `Ok(None)` is "not there".
async fn cached(store: &Store, cd: &CdService, set: &CdSettings, key: &str, service: Service, url: String, max: usize) -> std::result::Result<Option<Vec<u8>>, Refusal> {
    if let Some(hit) = store.cd_cache_get(key).await.map_err(|_| Refusal(500, "internal"))? {
        return Ok(hit);
    }
    let lookup = Arc::clone(&cd.lookup);
    let agent = user_agent(&set.contact);
    let answer = tokio::task::spawn_blocking(move || lookup.get(service, &url, &agent, max)).await.map_err(|_| Refusal(500, "internal"))?;
    match answer {
        Ok(body) => {
            store.cd_cache_put(key, body.as_deref()).await.map_err(|_| Refusal(500, "internal"))?;
            Ok(body)
        }
        Err(e) => {
            tracing::info!(error = %e, "CD lookup failed");
            Err(Refusal(502, "lookup_failed"))
        }
    }
}

/// The Disc ID of a table of contents and the releases MusicBrainz knows for it (by ID, else by the table itself).
pub async fn lookup(store: &Store, cd: &CdService, set: &CdSettings, toc: &Toc) -> std::result::Result<(String, Vec<Candidate>), Refusal> {
    if !set.lookup_ready {
        return Err(Refusal(409, "no_contact"));
    }
    let id = musicbrainz::disc_id(toc);
    let mb = base(&set.musicbrainz_url, musicbrainz::DEFAULT_MUSICBRAINZ_URL);
    let parse = |body: Option<Vec<u8>>| -> std::result::Result<Vec<Candidate>, Refusal> {
        match body {
            None => Ok(Vec::new()),
            Some(bytes) => musicbrainz::parse_discid(&String::from_utf8_lossy(&bytes), &id).map_err(|e| {
                tracing::info!(error = %e, "CD lookup: the answer could not be read");
                Refusal(502, "lookup_failed")
            }),
        }
    };
    let exact = cached(store, cd, set, &format!("mb:discid:{id}"), Service::MusicBrainz, musicbrainz::discid_url(&mb, &id, None), musicbrainz::MAX_JSON_BYTES).await?;
    let mut candidates = parse(exact)?;
    // The table of contents finds releases whose disc is nearly the same (another pressing, another country): they are offered after the
    // exact ones, so that a disc whose ID belongs to a wrong or an incomplete entry is not left with that one choice.
    let near = cached(store, cd, set, &format!("mb:toc:{id}"), Service::MusicBrainz, musicbrainz::discid_url(&mb, "-", Some(toc)), musicbrainz::MAX_JSON_BYTES).await;
    match near {
        Ok(body) => {
            for c in parse(body)? {
                if !candidates.iter().any(|k| k.release_id == c.release_id) {
                    candidates.push(c);
                }
            }
        }
        // The exact answer stands when the second question fails.
        Err(_) if !candidates.is_empty() => {}
        Err(e) => return Err(e),
    }
    candidates.sort_by_key(|c| !c.disc_id_matched);
    Ok((id, candidates))
}

/// A search by words for releases (the disc is not known, or known wrongly). The releases come without tracks: see [`release_for`].
pub async fn search(store: &Store, cd: &CdService, set: &CdSettings, words: &str) -> std::result::Result<Vec<Candidate>, Refusal> {
    if !set.lookup_ready {
        return Err(Refusal(409, "no_contact"));
    }
    let words = words.trim();
    if words.chars().count() < 2 || words.chars().count() > 200 {
        return Err(Refusal(422, "invalid_query"));
    }
    let mb = base(&set.musicbrainz_url, musicbrainz::DEFAULT_MUSICBRAINZ_URL);
    let key = format!("mb:search:{}", words.to_lowercase());
    let body = cached(store, cd, set, &key, Service::MusicBrainz, musicbrainz::search_url(&mb, words), musicbrainz::MAX_JSON_BYTES).await?;
    match body {
        None => Ok(Vec::new()),
        Some(bytes) => musicbrainz::parse_search(&String::from_utf8_lossy(&bytes)).map_err(|e| {
            tracing::info!(error = %e, "CD search: the answer could not be read");
            Refusal(502, "lookup_failed")
        }),
    }
}

/// One release with the tracks of the disc: the disc whose number of tracks is `track_count` (else the first).
pub async fn release_for(store: &Store, cd: &CdService, set: &CdSettings, mbid: &str, track_count: usize) -> std::result::Result<Option<Candidate>, Refusal> {
    if !set.lookup_ready {
        return Err(Refusal(409, "no_contact"));
    }
    if !musicbrainz::is_mbid(mbid) {
        return Err(Refusal(422, "invalid_query"));
    }
    let mb = base(&set.musicbrainz_url, musicbrainz::DEFAULT_MUSICBRAINZ_URL);
    let body = cached(store, cd, set, &format!("mb:release:{mbid}"), Service::MusicBrainz, musicbrainz::release_url(&mb, mbid), musicbrainz::MAX_JSON_BYTES).await?;
    let Some(bytes) = body else { return Ok(None) };
    let v: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| Refusal(502, "lookup_failed"))?;
    Ok(musicbrainz::parse_release_of(&v, None, Some(track_count)))
}

/// The front cover of a release, as bytes and type; `None` if there is none. Needs the contact address.
pub async fn fetch_cover(store: &Store, cd: &CdService, set: &CdSettings, mbid: &str) -> std::result::Result<Option<(Vec<u8>, &'static str)>, Refusal> {
    if !set.lookup_ready {
        return Err(Refusal(409, "no_contact"));
    }
    let caa = base(&set.coverart_url, musicbrainz::DEFAULT_COVERART_URL);
    let key = format!("caa:{mbid}:{}", set.cover_px);
    let bytes = cached(store, cd, set, &key, Service::CoverArt, musicbrainz::cover_url(&caa, mbid, set.cover_px), musicbrainz::MAX_COVER_BYTES).await?;
    // The type is decided by the bytes; something that is no picture is no cover.
    Ok(bytes.and_then(|b| crate::profile::sniff_image(&b).filter(|m| *m != "image/webp").map(|m| (b, m))))
}

/// The folder an import goes to: the first existing folder of the kind, else the development library.
pub async fn import_root(store: &Store, library: Option<&Path>, kind: &str) -> std::result::Result<PathBuf, Refusal> {
    let state = store.import_state().await.map_err(|_| Refusal(500, "internal"))?;
    let folders = if kind == "music" { &state.music_roots } else { &state.roots };
    folders
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_dir())
        .or_else(|| library.filter(|p| p.is_dir()).map(Path::to_path_buf))
        .ok_or(Refusal(503, "no_import_folder"))
}

/// The end of every import, whether the files came by upload or were ripped here: the job goes to `placing`, the
/// files are checked, named, tagged and moved into the import folder, the job is closed (or goes to `importing`)
/// and the import of the library is started in the background unless it is locked. Returns the job, what was
/// placed and whether the library import was queued.
#[allow(clippy::too_many_arguments)]
pub async fn finalize(
    store: &Store,
    cd: &CdService,
    user_id: i64,
    job_id: i64,
    meta: &Meta,
    set: &CdSettings,
    root: PathBuf,
    kind: String,
    cover: Option<(Vec<u8>, &'static str)>,
    tracks: Vec<StagedTrack>,
    reserve: u64,
    extra: FinalizeExtra,
) -> std::result::Result<(Job, Placed, bool), Refusal> {
    let internal = |_| Refusal(500, "internal");
    let placing = store.update_cd_job(user_id, job_id, &JobPatch { phase: Some("placing".into()), track: Some(meta.tracks.len() as i64), ..JobPatch::default() }).await.map_err(internal)?;
    if let Some(j) = &placing {
        cd.publish(user_id, "progress", j);
    }
    let (meta2, set2, heal) = (meta.clone(), set.clone(), extra.heal.clone());
    let root2 = root.clone();
    let placed = tokio::task::spawn_blocking(move || {
        let job = PlaceJob { meta: &meta2, settings: &set2, tracks, root: &root2, cover, kind: &kind, reserve, heal: heal.as_ref() };
        place(&job)
    })
    .await
    .map_err(|_| Refusal(500, "internal"))??;

    // The disc is written down, so that it is known to be in the library (and healing finds its files).
    if let Some(mut row) = extra.record.clone() {
        let mut numbers: Vec<u32> = meta.tracks.iter().map(|t| t.number).collect();
        numbers.sort_unstable();
        if row.folder.is_empty() {
            row.folder = placed.folder.clone();
        }
        let mut tracks: Vec<crate::cdpresent::DiscTrackRow> = numbers
            .iter()
            .zip(&placed.files)
            .map(|(n, file)| {
                let (sha1, seconds) = extra.info.get(n).cloned().unwrap_or_default();
                crate::cdpresent::DiscTrackRow { number: *n, file: file.clone(), sha1, seconds }
            })
            .collect();
        // Healing: the files that were not read again are part of the disc too.
        if let Some(h) = &extra.heal {
            for (n, path) in h.existing.iter().filter(|(n, p)| !numbers.contains(n) && p.exists()) {
                let rel = path.strip_prefix(&root).unwrap_or(path).to_string_lossy().replace('\\', "/");
                tracks.push(crate::cdpresent::DiscTrackRow { number: *n, file: rel, sha1: String::new(), seconds: extra.info.get(n).map_or(0, |i| i.1) });
            }
        }
        if let Err(e) = store.record_cd_disc(&crate::cdpresent::DiscRecord { row, tracks }).await {
            tracing::warn!(error = %e, "the disc could not be written down");
        }
    }
    let state = store.import_state().await.map_err(internal)?;
    let mut summary = format!("{} - {}, CD {}: {} files placed", meta.author.trim(), meta.album.trim(), meta.disc, placed.files.len());
    if !extra.note.is_empty() {
        summary.push_str(&format!("; {}", extra.note));
    }
    let import_queued = state.enabled;
    let patch = if import_queued {
        JobPatch { phase: Some("importing".into()), message: Some(summary.clone()), error: extra.error.clone(), ..JobPatch::default() }
    } else {
        JobPatch { phase: Some("done".into()), message: Some(format!("{summary}; the import is locked, so it did not run")), import_ran: Some(false), error: extra.error.clone(), ..JobPatch::default() }
    };
    let updated = store.update_cd_job(user_id, job_id, &patch).await.map_err(internal)?.ok_or(Refusal(500, "internal"))?;
    cd.publish(user_id, if import_queued { "progress" } else { "done" }, &updated);
    if import_queued {
        let (store, cd) = (store.clone(), cd.clone());
        tokio::spawn(async move {
            let (patch, kind) = match store.run_import().await {
                Ok(report) => (
                    JobPatch { phase: Some("done".into()), message: Some(format!("{summary}; {} tracks added, {} changed", report.tracks_added, report.tracks_updated)), import_ran: Some(true), ..JobPatch::default() },
                    "done",
                ),
                Err(e) => {
                    tracing::warn!(error = %e, "CD import: the library import failed");
                    (JobPatch { phase: Some("failed".into()), error: Some("import_failed".into()), message: Some(format!("{summary}; the import failed")), ..JobPatch::default() }, "failed")
                }
            };
            if let Ok(Some(job)) = store.update_cd_job(user_id, job_id, &patch).await {
                cd.publish(user_id, kind, &job);
            }
        });
    }
    Ok((updated, placed, import_queued))
}

/// A file as the app delivered it, in the staging folder.
pub struct StagedTrack {
    pub number: u32,
    pub title: String,
    pub path: PathBuf,
}

/// The values the templates are filled with, for one track.
fn values_for(m: &Meta, t: &StagedTrack, filename: Option<&str>, cleaned: bool) -> HashMap<&'static str, String> {
    let c = |s: &str| if cleaned { cd_settings::clean(s) } else { s.to_owned() };
    let mut v: HashMap<&'static str, String> = HashMap::new();
    v.insert("author", c(m.author.trim()));
    v.insert("title", c(m.album.trim()));
    v.insert("album", c(m.album.trim()));
    v.insert("narrator", c(m.narrator.as_deref().unwrap_or("").trim()));
    v.insert("year", c(m.year.as_deref().unwrap_or("")));
    v.insert("disc", m.disc.to_string());
    v.insert("track", t.number.to_string());
    v.insert("track_title", c(t.title.trim()));
    if let Some(f) = filename {
        v.insert("file_name", f.to_owned());
    }
    v
}

/// The result of placing a CD.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Placed {
    /// The book folder, relative to the import folder.
    pub folder: String,
    pub files: Vec<String>,
}

/// The file name (with the ending) of a track, by the file scheme.
pub fn file_name_of(m: &Meta, s: &CdSettings, t: &StagedTrack) -> String {
    let stem = cd_settings::clean(&cd_settings::render(&s.file_pattern, &values_for(m, t, None, true)));
    let stem = if stem.is_empty() { format!("CD{:02}_TR{:02}", m.disc, t.number) } else { stem };
    format!("{stem}.{}", s.extension)
}

/// The book folder and the folder the files of this disc go into, below `root`.
pub fn layout(m: &Meta, s: &CdSettings, root: &Path) -> (PathBuf, PathBuf) {
    let t0 = StagedTrack { number: 1, title: String::new(), path: PathBuf::new() };
    let v = values_for(m, &t0, None, true);
    let mut book = root.to_path_buf();
    for segment in s.folder_pattern.split('/') {
        book.push(cd_settings::clean(&cd_settings::render(segment, &v)).pipe_nonempty("Unknown"));
    }
    let flat_audio_there = dir_has_audio(&book);
    let multi = m.discs_total.unwrap_or(1) > 1 || m.disc > 1;
    let dest = if multi && !s.disc_folder_pattern.is_empty() && !flat_audio_there {
        let mut v = HashMap::new();
        v.insert("disc", m.disc.to_string());
        v.insert("title", cd_settings::clean(m.album.trim()));
        v.insert("album", cd_settings::clean(m.album.trim()));
        book.join(cd_settings::clean(&cd_settings::render(&s.disc_folder_pattern, &v)).pipe_nonempty("CD"))
    } else {
        book.clone()
    };
    (book, dest)
}

/// The numbers of the tracks of `m` whose files exist in the library already (nothing is ever overwritten).
pub fn taken_numbers(m: &Meta, s: &CdSettings, root: &Path) -> Vec<u32> {
    let (_, dest) = layout(m, s, root);
    let mut out: Vec<u32> = m
        .tracks
        .iter()
        .filter(|t| dest.join(file_name_of(m, s, &StagedTrack { number: t.number, title: t.title.clone().unwrap_or_default(), path: PathBuf::new() })).exists())
        .map(|t| t.number)
        .collect();
    out.sort_unstable();
    out
}

/// Where the repaired files of a CD go (healing, `mode: heal`): into the folder where the others of this CD lie, with the
/// names of the pattern of those. A damaged file that is there is put into the quarantine first, never overwritten.
#[derive(Debug, Clone)]
pub struct HealPlan {
    pub dest_dir: PathBuf,
    /// The file names by number on the disc.
    pub names: HashMap<u32, String>,
    /// The name of the quarantine folder inside the import folder (the setting `quarantine_folder`).
    pub quarantine_folder: String,
    /// All the files of this CD that are there now, by number (so that the disc is written down whole).
    pub existing: std::collections::BTreeMap<u32, PathBuf>,
}

/// What `finalize` does besides placing: healing, and writing down the disc.
#[derive(Debug, Clone, Default)]
pub struct FinalizeExtra {
    pub heal: Option<HealPlan>,
    /// The disc to write down in `cd_disc` once the files are placed (the track rows are made from the files placed).
    pub record: Option<crate::cdpresent::DiscRow>,
    /// SHA-1 of the PCM and seconds by number on the disc.
    pub info: HashMap<u32, (String, u32)>,
    /// A remark for the summary of the job (the tracks that could not be read), and the code the job carries then.
    pub note: String,
    pub error: Option<String>,
}

/// Everything `place` needs.
pub struct PlaceJob<'a> {
    pub meta: &'a Meta,
    pub settings: &'a CdSettings,
    pub tracks: Vec<StagedTrack>,
    pub root: &'a Path,
    pub cover: Option<(Vec<u8>, &'static str)>,
    pub kind: &'a str,
    pub reserve: u64,
    pub heal: Option<&'a HealPlan>,
}

/// Checks the files, names and tags them by the settings and moves them into the import folder. Nothing is left
/// half done: when a file cannot be moved, the ones already moved are removed again.
pub fn place(job: &PlaceJob) -> std::result::Result<Placed, Refusal> {
    let (m, s) = (job.meta, job.settings);
    let internal = |why: &str, e: &dyn std::fmt::Display| {
        tracing::warn!(error = %e, "CD import: {why}");
        Refusal(500, "internal")
    };
    // 1. The files are what the settings say.
    let mut total = 0u64;
    for t in &job.tracks {
        total += std::fs::metadata(&t.path).map(|md| md.len()).unwrap_or(0);
        check_file(&t.path, s)?;
    }
    // 2. Names and tags.
    let genre = if job.kind == "audiobook" { Some(s.genre.clone()) } else { m.genre.clone().filter(|g| !g.trim().is_empty()) };
    let mut named: Vec<(&StagedTrack, String)> = Vec::new();
    let mut tracks: Vec<&StagedTrack> = job.tracks.iter().collect();
    tracks.sort_by_key(|t| t.number);
    for t in tracks {
        named.push((t, job.heal.and_then(|h| h.names.get(&t.number).cloned()).unwrap_or_else(|| file_name_of(m, s, t))));
    }
    let mut names: Vec<&String> = named.iter().map(|(_, n)| n).collect();
    names.sort();
    if names.windows(2).any(|w| w[0] == w[1]) {
        return Err(Refusal(422, "invalid_meta"));
    }
    for (t, file) in &named {
        let stem = file.strip_suffix(&format!(".{}", s.extension)).unwrap_or(file);
        // The title tag is the file name, or the title of the track when the setting says so (and the app knows it).
        let title = if s.title_pattern == "track_title" && !t.title.trim().is_empty() { t.title.trim().to_owned() } else { stem.to_owned() };
        write_tags(&t.path, m, s, t, &title, genre.as_deref(), job.cover.as_ref())?;
    }
    // 3. Where they go.
    let (book, dest_dir) = match job.heal {
        Some(h) => (h.dest_dir.clone(), h.dest_dir.clone()),
        None => layout(m, s, job.root),
    };
    if job.heal.is_none() && named.iter().any(|(_, n)| dest_dir.join(n).exists()) {
        return Err(Refusal(409, "disc_exists"));
    }
    if !crate::diskspace::same_disk(&job.tracks[0].path, job.root) && !crate::diskspace::fits(job.root, total, job.reserve) {
        return Err(Refusal(507, "no_space"));
    }
    // 4. A damaged file that is there is put into the quarantine first (healing); it comes back if anything fails.
    let mut put_aside: Vec<(PathBuf, PathBuf)> = Vec::new();
    let restore = |put_aside: &[(PathBuf, PathBuf)]| {
        for (original, aside) in put_aside {
            let _ = std::fs::rename(aside, original);
        }
    };
    if let Some(h) = job.heal {
        let rel = dest_dir.strip_prefix(job.root).unwrap_or(&dest_dir);
        let quarantine = job.root.join(&h.quarantine_folder).join(rel);
        std::fs::create_dir_all(&quarantine).map_err(|e| internal("could not create the quarantine folder", &e))?;
        let stamp = crate::store::now_ms() / 1000;
        for (_, file) in &named {
            let original = dest_dir.join(file);
            if original.exists() {
                let aside = quarantine.join(format!("{file}.{stamp}"));
                if let Err(e) = std::fs::rename(&original, &aside) {
                    restore(&put_aside);
                    return Err(internal("could not put a damaged file into the quarantine", &e));
                }
                put_aside.push((original, aside));
            }
        }
    }
    // 5. Move.
    let created = !dest_dir.exists();
    std::fs::create_dir_all(&dest_dir).map_err(|e| internal("could not create the folder", &e))?;
    let mut moved: Vec<PathBuf> = Vec::new();
    for (t, file) in &named {
        let to = dest_dir.join(file);
        if let Err(e) = move_file(&t.path, &to) {
            for done in &moved {
                let _ = std::fs::remove_file(done);
            }
            restore(&put_aside);
            if created {
                let _ = std::fs::remove_dir(&dest_dir);
            }
            return Err(internal("could not move a file", &e));
        }
        moved.push(to);
    }
    // 6. The cover next to the files of the book (the first disc makes it, the others leave it; healing leaves it alone).
    if let (None, Some((bytes, mime))) = (job.heal, &job.cover) {
        let name = if *mime == "image/png" { "cover.png" } else { "cover.jpg" };
        let target = book.join(name);
        if !book.join("cover.jpg").exists()
            && !book.join("cover.png").exists()
            && !book.join("folder.jpg").exists()
            && let Err(e) = std::fs::write(&target, bytes)
        {
            tracing::warn!(error = %e, "CD import: the cover file was not written");
        }
    }
    let rel = |p: &Path| p.strip_prefix(job.root).unwrap_or(p).to_string_lossy().replace('\\', "/");
    Ok(Placed { folder: rel(&book), files: named.iter().map(|(_, n)| rel(&dest_dir.join(n))).collect() })
}

trait PipeNonEmpty {
    fn pipe_nonempty(self, fallback: &str) -> String;
}
impl PipeNonEmpty for String {
    fn pipe_nonempty(self, fallback: &str) -> String {
        if self.is_empty() { fallback.to_owned() } else { self }
    }
}

fn dir_has_audio(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|rd| {
        rd.flatten().any(|e| e.path().is_file() && e.path().extension().is_some_and(|x| ["mp3", "m4a", "m4b", "flac", "ogg", "opus"].contains(&x.to_string_lossy().to_ascii_lowercase().as_str())))
    })
}

fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to)?;
    let _ = std::fs::remove_file(from);
    Ok(())
}

/// Is the file the format the settings demand?
fn check_file(path: &Path, s: &CdSettings) -> std::result::Result<(), Refusal> {
    use lofty::file::{AudioFile, FileType, TaggedFileExt};
    let tagged = lofty::read_from_path(path).map_err(|_| Refusal(422, "unreadable_file"))?;
    if s.format == "mp3" && tagged.file_type() != FileType::Mpeg {
        return Err(Refusal(422, "unreadable_file"));
    }
    let p = tagged.properties();
    if p.duration().as_millis() == 0 {
        return Err(Refusal(422, "unreadable_file"));
    }
    let rate_ok = p.sample_rate().is_none_or(|r| r == s.sample_rate_hz);
    // A constant bit rate is within a kilobit of the setting; VBR headers round.
    let rate_kbps_ok = p.audio_bitrate().is_none_or(|b| b.abs_diff(s.bitrate_kbps) <= 1);
    let channels_ok = p.channels().is_none_or(|c| if s.channel_mode == "mono" { c == 1 } else { c == 2 });
    if rate_ok && rate_kbps_ok && channels_ok { Ok(()) } else { Err(Refusal(422, "format_mismatch")) }
}

fn write_tags(path: &Path, m: &Meta, s: &CdSettings, t: &StagedTrack, title: &str, genre: Option<&str>, cover: Option<&(Vec<u8>, &'static str)>) -> std::result::Result<(), Refusal> {
    let broken = |e: crate::error::Error| {
        tracing::warn!(error = %e, "CD import: the tags could not be written");
        Refusal(422, "bad_tags")
    };
    let tag_track = s.tag_track(m.disc, t.number).to_string();
    let mut fields: Vec<(String, Vec<String>)> = vec![
        ("TrackTitle".into(), vec![title.to_owned()]),
        ("TrackNumber".into(), vec![tag_track]),
        ("DiscNumber".into(), vec![m.disc.to_string()]),
    ];
    if let Some(total) = m.discs_total {
        fields.push(("DiscTotal".into(), vec![total.to_string()]));
    }
    crate::tags::write_fields(path, &fields).map_err(broken)?;
    let album = crate::tags::AlbumTags {
        album: Some(m.album.trim().to_owned()),
        artist: Some(m.author.trim().to_owned()),
        narrator: m.narrator.as_deref().map(str::trim).filter(|n| !n.is_empty()).map(str::to_owned),
        year: m.year.clone().filter(|y| !y.is_empty()),
        genre: genre.map(str::to_owned),
        cover: cover.cloned(),
    };
    crate::tags::write_album(path, &album).map_err(broken)?;
    if s.id3_version == "2.3" {
        save_as_id3v23(path).map_err(broken)?;
    }
    if crate::tags::lacks_id3v2(path) {
        return Err(Refusal(422, "bad_tags"));
    }
    Ok(())
}

/// Writes the ID3v2 tag again in the older version 2.3 (some players do not read 2.4).
fn save_as_id3v23(path: &Path) -> std::result::Result<(), crate::error::Error> {
    use lofty::config::WriteOptions;
    use lofty::file::TaggedFileExt;
    use lofty::tag::{TagExt, TagType};
    let fail = |m: String| crate::error::Error::Tags { file: path.display().to_string(), message: m };
    let tagged = lofty::read_from_path(path).map_err(|e| fail(e.to_string()))?;
    let tag = tagged.tag(TagType::Id3v2).ok_or_else(|| fail("no ID3v2 tag".into()))?;
    tag.save_to_path(path, WriteOptions::new().use_id3v23(true)).map_err(|e| fail(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::messages::{DE, EN};

    /// Every code, phase and state of the import has a text in both languages.
    #[test]
    fn every_code_has_an_english_and_a_german_text() {
        let has = |table: &[(&str, &str)], key: &str| table.iter().any(|(k, v)| *k == key && !v.is_empty());
        let mut keys: Vec<String> = ERROR_CODES.iter().map(|c| format!("cd.err.{c}")).collect();
        keys.extend(PHASES.iter().map(|p| format!("cd.phase.{p}")));
        keys.extend(TRACK_STATES.iter().map(|p| format!("cd.state.{p}")));
        keys.extend(["ripping", "verifying", "encoding", "placing", "importing"].iter().map(|p| format!("cd.step.{p}")));
        keys.extend(STATUS_CODES.iter().map(|c| format!("cd.status.{c}")));
        keys.extend(crate::cd_settings::VALIDATION_CODES.iter().map(|c| format!("cd.verr.{c}")));
        keys.extend(crate::cd_settings::KEYS.iter().map(|k| format!("setting.{k}")));
        for key in keys {
            assert!(has(EN, &key), "English text missing: {key}");
            assert!(has(DE, &key), "German text missing: {key}");
        }
        assert_eq!(error_key("disc_exists"), "cd.err.disc_exists");
        assert_eq!(error_key("something_new"), "cd.err.other");
    }
}

#[cfg(test)]
mod place_tests {
    use super::*;

    /// A valid MP3 of the format of the default settings: two seconds of silence.
    fn mp3() -> Vec<u8> {
        let s = cdrip::encode::EncoderSettings { bitrate_kbps: 64, sample_rate_hz: 24000, channel_mode: "joint_stereo".into(), quality: 3 };
        cdrip::encode::encode(&vec![0u8; 44100 * 4 * 2][..], &s, |_| {}).unwrap()
    }

    fn meta(numbers: &[u32]) -> Meta {
        Meta {
            author: "Ken Follett".into(),
            album: "Sturz der Titanen".into(),
            narrator: None,
            year: None,
            disc: 2,
            discs_total: Some(12),
            kind: Some("audiobook".into()),
            mbid: None,
            disc_id: None,
            genre: None,
            job_id: None,
            allow_gaps: true,
            tracks: numbers.iter().map(|n| MetaTrack { number: *n, title: None, file: format!("{n:02}.mp3") }).collect(),
        }
    }

    #[test]
    fn a_damaged_file_goes_into_the_quarantine_first_and_comes_back_when_the_placing_fails() {
        let root = tempfile::tempdir().unwrap();
        let book = root.path().join("Ken Follett/Sturz der Titanen");
        std::fs::create_dir_all(&book).unwrap();
        let staging = tempfile::tempdir().unwrap();
        let set = CdSettings::defaults();
        let old = book.join("Follett_Sturz der Titanen_CD02_TR02.mp3");
        std::fs::write(&old, b"damaged").unwrap();
        let staged = |n: u32| {
            let path = staging.path().join(format!("{n:02}.mp3"));
            std::fs::write(&path, mp3()).unwrap();
            StagedTrack { number: n, title: String::new(), path }
        };
        let plan = |names: &[(u32, &str)]| HealPlan {
            dest_dir: book.clone(),
            names: names.iter().map(|(n, f)| (*n, (*f).to_owned())).collect(),
            quarantine_folder: ".quarantine".into(),
            existing: Default::default(),
        };

        // The second file cannot be placed (its folder is not there): the first is back, nothing new is left.
        let bad = plan(&[(2, "Follett_Sturz der Titanen_CD02_TR02.mp3"), (3, "missing folder/x.mp3")]);
        let m = meta(&[2, 3]);
        let job = PlaceJob { meta: &m, settings: &set, tracks: vec![staged(2), staged(3)], root: root.path(), cover: None, kind: "audiobook", reserve: 0, heal: Some(&bad) };
        assert!(place(&job).is_err());
        assert_eq!(std::fs::read(&old).unwrap(), b"damaged", "the damaged file is back where it was");
        let aside: Vec<_> = std::fs::read_dir(root.path().join(".quarantine/Ken Follett/Sturz der Titanen")).unwrap().flatten().collect();
        assert!(aside.is_empty(), "nothing stays in the quarantine when the file came back");

        // Placing works: the damaged file is in the quarantine (with its content), the new one has its name.
        let good = plan(&[(2, "Follett_Sturz der Titanen_CD02_TR02.mp3")]);
        let m = meta(&[2]);
        let job = PlaceJob { meta: &m, settings: &set, tracks: vec![staged(2)], root: root.path(), cover: None, kind: "audiobook", reserve: 0, heal: Some(&good) };
        let placed = place(&job).unwrap();
        assert_eq!(placed.files, vec!["Ken Follett/Sturz der Titanen/Follett_Sturz der Titanen_CD02_TR02.mp3"]);
        assert!(std::fs::read(&old).unwrap().len() > 1000, "the new file");
        let aside: Vec<_> = std::fs::read_dir(root.path().join(".quarantine/Ken Follett/Sturz der Titanen")).unwrap().flatten().collect();
        assert_eq!(aside.len(), 1);
        assert_eq!(std::fs::read(aside[0].path()).unwrap(), b"damaged");
        assert!(aside[0].file_name().to_string_lossy().starts_with("Follett_Sturz der Titanen_CD02_TR02.mp3."));
    }
}
