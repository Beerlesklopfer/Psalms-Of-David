// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The state machine of a CD job (docs/spec-cd-inbox.md, "The flow of one CD"; docs/spec-cd-import-api.md, "States of a job").
//!
//! The state is stored in `cd_job.cd_state`; the pages and the apps only show it. **Every change of it goes through one function,
//! `Store::cd_move`**, which checks that the move is allowed from the state the job is in, who may make it, and refuses with
//! `invalid_move`, `forbidden` or `decided`. The same function writes the decision (who, when) and the message of the log.
//!
//! ```text
//!  ripping --read/verify error--> retry_wait --retry--> ripping
//!     |                               +--give up / cancel--> cancelled
//!     +--present--> present
//!     +--ripped--> ripped --(no approval needed)--> imported
//!                    +--hold--> awaiting_approval --approve--> ripped --> imported
//!                                      +--reject / expire--> rejected
//! ```
//!
//! `failed` is the end of a failure that no new try can mend, `skipped` an entry of the log of the automatic import (never a move).
//! The held files (`awaiting_approval`) live in a folder of their own, outside the library: see `hold`.

use std::path::{Path, PathBuf};

use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
use serde::{Deserialize, Serialize};

use crate::cd_settings::CdSettings;
use crate::cdimport::{self, CdService, FinalizeExtra, Job, Meta, Refusal, StagedTrack};
use crate::cdrights::CdRights;
use crate::error::Result;
use crate::store::{Store, now_ms};

/// The prefix of the folder of a held CD (`held-<job id>`), inside the folder the setting `cd_held_folder` names, or below the staging folder.
pub const HELD_PREFIX: &str = "held-";

/// The states of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CdState {
    Ripping,
    RetryWait,
    Ripped,
    AwaitingApproval,
    Imported,
    Rejected,
    Present,
    Cancelled,
    Failed,
    Skipped,
}

/// Every state, in the order of the flow (the catalogs have a text `cd.job.<state>` for each).
pub const STATES: &[CdState] = &[
    CdState::Ripping,
    CdState::RetryWait,
    CdState::Ripped,
    CdState::AwaitingApproval,
    CdState::Imported,
    CdState::Rejected,
    CdState::Present,
    CdState::Cancelled,
    CdState::Failed,
    CdState::Skipped,
];

impl CdState {
    pub fn as_str(self) -> &'static str {
        match self {
            CdState::Ripping => "ripping",
            CdState::RetryWait => "retry_wait",
            CdState::Ripped => "ripped",
            CdState::AwaitingApproval => "awaiting_approval",
            CdState::Imported => "imported",
            CdState::Rejected => "rejected",
            CdState::Present => "present",
            CdState::Cancelled => "cancelled",
            CdState::Failed => "failed",
            CdState::Skipped => "skipped",
        }
    }

    pub fn parse(s: &str) -> Option<CdState> {
        STATES.iter().copied().find(|st| st.as_str() == s)
    }

    /// A state in which a job is still part of the flow: the log may not drop it.
    pub fn is_open(self) -> bool {
        matches!(
            self,
            CdState::Ripping | CdState::RetryWait | CdState::Ripped | CdState::AwaitingApproval
        )
    }
}

/// A move of the machine. The first four are made by people, the rest by the server itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Move {
    /// `retry_wait` -> `ripping`: read the CD again (the resume folder is used). The starter, or somebody who may approve.
    Retry,
    /// `retry_wait` -> `cancelled`. The starter, or somebody who may approve.
    GiveUp,
    /// `awaiting_approval` -> `ripped` (and on to `imported` when the files are placed). Somebody who may approve.
    Approve,
    /// `awaiting_approval` -> `rejected`. Somebody who may approve.
    Reject,
    /// `ripping` -> `retry_wait`: a track could not be read or confirmed (the code), or the server stopped (`interrupted`).
    ReadFailed(String),
    /// `ripping` -> `retry_wait` after a restart; `ripped` without held files too (the resume folder has what was read).
    Interrupted,
    /// `ripping` -> `present`: the CD is whole in the library.
    Present,
    /// `ripping` -> `ripped`: all chosen tracks are files in the job folder.
    Ripped,
    /// `ripped` -> `awaiting_approval`: the files were put into the held folder (also back from an approval that could not place them).
    Hold,
    /// `ripped` -> `imported`: the files are placed.
    Place,
    /// `imported` -> `failed`: placed, but the import of the library failed.
    ImportFailed,
    /// `ripping` or `ripped` -> `failed`.
    Fail(String),
    /// `ripping` or `retry_wait` -> `cancelled`: a cancel was seen.
    Cancel,
    /// `awaiting_approval` -> `rejected` with the reason `expired`.
    Expire,
}

impl Move {
    pub fn name(&self) -> &'static str {
        match self {
            Move::Retry => "retry",
            Move::GiveUp => "give_up",
            Move::Approve => "approve",
            Move::Reject => "reject",
            Move::ReadFailed(_) => "read_failed",
            Move::Interrupted => "interrupted",
            Move::Present => "present",
            Move::Ripped => "ripped",
            Move::Hold => "hold",
            Move::Place => "place",
            Move::ImportFailed => "import_failed",
            Move::Fail(_) => "fail",
            Move::Cancel => "cancel",
            Move::Expire => "expire",
        }
    }

    /// Is this a move that only people make (they are the only ones the rights are asked for)?
    fn by_people(&self) -> bool {
        matches!(
            self,
            Move::Retry | Move::GiveUp | Move::Approve | Move::Reject
        )
    }

    /// The state the move leads to from `from`, or `None` if the move is not allowed there.
    pub fn target(&self, from: CdState) -> Option<CdState> {
        use CdState::*;
        match (self, from) {
            (Move::Retry, RetryWait) => Some(Ripping),
            (Move::GiveUp, RetryWait) | (Move::Cancel, Ripping | RetryWait) => Some(Cancelled),
            (Move::Approve, AwaitingApproval) => Some(Ripped),
            (Move::Reject | Move::Expire, AwaitingApproval) => Some(Rejected),
            (Move::ReadFailed(_) | Move::Interrupted, Ripping) | (Move::Interrupted, Ripped) => {
                Some(RetryWait)
            }
            (Move::Present, Ripping) => Some(CdState::Present),
            (Move::Ripped, Ripping) => Some(Ripped),
            (Move::Hold, Ripped) => Some(AwaitingApproval),
            (Move::Place, Ripped) => Some(Imported),
            (Move::ImportFailed, Imported) => Some(Failed),
            (Move::Fail(_), Ripping | Ripped) => Some(Failed),
            _ => None,
        }
    }
}

/// Who makes a move.
#[derive(Debug, Clone)]
pub enum Actor {
    /// A person (a profile) with the rights they have now.
    Person {
        id: i64,
        name: String,
        rights: CdRights,
    },
    /// The server itself.
    Server,
}

impl Actor {
    /// A person, with the rights read now.
    pub async fn person(store: &Store, id: i64) -> Result<Actor> {
        let rights = store.cd_rights(id).await?;
        let name = store.profile_name(id).await?.unwrap_or_default();
        Ok(Actor::Person { id, name, rights })
    }
}

/// Why a move was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveError {
    /// No such job.
    NotFound,
    /// The rights do not allow it (`403 forbidden`).
    Forbidden,
    /// The job is not in a state this move leads out of (`409 invalid_move`).
    Invalid { from: CdState },
    /// Another person decided first (`409 decided`): who and what (`approved`, `rejected` or `expired`).
    Decided {
        by: String,
        resolution: &'static str,
    },
    /// The database or the disk failed.
    Internal,
}

impl MoveError {
    /// The code of the API.
    pub fn code(&self) -> &'static str {
        match self {
            MoveError::NotFound => "not_found",
            MoveError::Forbidden => "forbidden",
            MoveError::Invalid { .. } => "invalid_move",
            MoveError::Decided { .. } => "decided",
            MoveError::Internal => "internal",
        }
    }

    pub fn status(&self) -> u16 {
        match self {
            MoveError::NotFound => 404,
            MoveError::Forbidden => 403,
            MoveError::Invalid { .. } | MoveError::Decided { .. } => 409,
            MoveError::Internal => 500,
        }
    }
}

impl From<crate::error::Error> for MoveError {
    fn from(e: crate::error::Error) -> Self {
        tracing::warn!(error = %e, "CD job: a move could not be made");
        MoveError::Internal
    }
}

/// A move that was made: the job as it is now and the state it was in.
#[derive(Debug, Clone)]
pub struct Moved {
    pub from: CdState,
    pub job: Job,
}

/// May this actor make this move on the job (whose starter is `user_id`)? People: `retry` and `give up` the starter or an approver,
/// `approve` and `reject` only an approver. The server makes only its own moves.
fn allowed(actor: &Actor, mv: &Move, starter: i64) -> bool {
    match actor {
        Actor::Server => !mv.by_people(),
        Actor::Person { id, rights, .. } => match mv {
            Move::Retry | Move::GiveUp => {
                rights.may_approve || (*id == starter && starter != cdimport::SYSTEM_USER)
            }
            Move::Approve | Move::Reject => rights.may_approve,
            _ => false,
        },
    }
}

/// The whole judgement of a move: may the actor make it (`Forbidden`), does it lead out of this state (`Invalid`), was it decided by somebody
/// else already (`Decided`)? On success the state it leads to.
fn judge(
    from: CdState,
    starter: i64,
    reason: &str,
    decided_ms: i64,
    decided_name: &str,
    mv: &Move,
    actor: &Actor,
) -> std::result::Result<CdState, MoveError> {
    if !allowed(actor, mv, starter) {
        return Err(MoveError::Forbidden);
    }
    mv.target(from).ok_or_else(|| {
        // Somebody was faster: a decision was made already (or the CD expired meanwhile).
        if matches!(mv, Move::Approve | Move::Reject) && (decided_ms > 0 || reason == "expired") {
            let resolution = if reason == "expired" {
                "expired"
            } else if from == CdState::Rejected {
                "rejected"
            } else {
                "approved"
            };
            MoveError::Decided {
                by: decided_name.to_owned(),
                resolution,
            }
        } else {
            MoveError::Invalid { from }
        }
    })
}

const STATE_COLUMNS: &str = "cd_state, user_id, reason, decided_ms, decided_name";

impl Store {
    /// **The one function that changes the state of a job.** Checks the job exists, that the actor may make the move, and that the move
    /// leads out of the state the job is in; then changes the state in one statement that holds only while the state is still the one
    /// that was checked, so of two deciders exactly one wins (the other gets `Decided`). Also sets the phase, the decision (who, when) and
    /// the sentence of the log that belong to the move.
    pub async fn cd_move(
        &self,
        job_id: i64,
        mv: Move,
        actor: &Actor,
    ) -> std::result::Result<Moved, MoveError> {
        let row = self
            .db
            .writer
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("SELECT {STATE_COLUMNS} FROM cd_job WHERE id = ?"),
                [job_id.into()],
            ))
            .await
            .map_err(crate::error::Error::from)?
            .ok_or(MoveError::NotFound)?;
        let state: String = row.try_get_by_index(0).map_err(crate::error::Error::from)?;
        let starter: i64 = row.try_get_by_index(1).map_err(crate::error::Error::from)?;
        let reason: String = row.try_get_by_index(2).map_err(crate::error::Error::from)?;
        let decided_ms: i64 = row.try_get_by_index(3).map_err(crate::error::Error::from)?;
        let decided_name: String = row.try_get_by_index(4).map_err(crate::error::Error::from)?;
        let from = CdState::parse(&state).ok_or(MoveError::Internal)?;
        let to = judge(
            from,
            starter,
            &reason,
            decided_ms,
            &decided_name,
            &mv,
            actor,
        )?;
        let now = now_ms();
        let (actor_id, actor_name) = match actor {
            Actor::Person { id, name, .. } => (Some(*id), name.clone()),
            Actor::Server => (None, String::new()),
        };
        let mut sets = vec!["cd_state = ?".to_owned(), "updated_ms = ?".to_owned()];
        let mut values: Vec<Value> = vec![to.as_str().into(), now.into()];
        let mut set = |column: &str, v: Value| {
            sets.push(format!("{column} = ?"));
            values.push(v);
        };
        // The phase follows the state where the state says what is going on.
        match to {
            CdState::Ripping => {
                set("phase", "ripping".into());
                set("error", String::new().into());
            }
            CdState::RetryWait => set("phase", "retry_wait".into()),
            CdState::AwaitingApproval => set("phase", "awaiting_approval".into()),
            CdState::Rejected => set("phase", "rejected".into()),
            CdState::Present => set("phase", "present".into()),
            CdState::Cancelled | CdState::Failed => set("phase", "failed".into()),
            CdState::Ripped | CdState::Imported | CdState::Skipped => {}
        }
        // The sentence the log shows, and the decision.
        let who = if actor_name.is_empty() {
            "the server".to_owned()
        } else {
            actor_name.clone()
        };
        match &mv {
            Move::Approve => {
                set("decided_by", actor_id.into());
                set("decided_name", actor_name.clone().into());
                set("decided_ms", now.into());
                set("message", format!("approved by {who}").into());
            }
            Move::Reject => {
                set("decided_by", actor_id.into());
                set("decided_name", actor_name.clone().into());
                set("decided_ms", now.into());
                set("reason", "declined".into());
                set(
                    "message",
                    format!("rejected by {who}; the files were deleted").into(),
                );
            }
            Move::Expire => {
                set("reason", "expired".into());
                set(
                    "message",
                    "nobody decided in time; the files were deleted"
                        .to_owned()
                        .into(),
                );
            }
            Move::GiveUp => {
                set("decided_by", actor_id.into());
                set("decided_name", actor_name.clone().into());
                set("decided_ms", now.into());
                set("error", "cancelled".to_owned().into());
                set("message", format!("given up by {who}").into());
            }
            Move::Retry => set("message", format!("read again at the wish of {who}").into()),
            Move::Hold => {
                // A decision that could not be carried out (the files could not be placed) is open again.
                set("decided_by", Value::from(None::<i64>));
                set("decided_name", String::new().into());
                set("decided_ms", 0_i64.into());
            }
            Move::ReadFailed(code) => set("error", code.clone().into()),
            Move::Fail(code) => set("error", code.clone().into()),
            Move::ImportFailed => set("error", "import_failed".to_owned().into()),
            Move::Cancel => set("error", "cancelled".to_owned().into()),
            Move::Interrupted => set("error", "interrupted".to_owned().into()),
            Move::Present | Move::Ripped | Move::Place => {}
        }
        let sql = format!(
            "UPDATE cd_job SET {} WHERE id = ? AND cd_state = ?",
            sets.join(", ")
        );
        values.push(job_id.into());
        values.push(state.into());
        let done = self
            .db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                sql,
                values,
            ))
            .await
            .map_err(crate::error::Error::from)?;
        if done.rows_affected() == 0 {
            // The state changed between the check and the write: look again, which gives the right refusal.
            return Box::pin(self.cd_move(job_id, mv, actor)).await;
        }
        let job = self.cd_job_any(job_id).await?.ok_or(MoveError::NotFound)?;
        tracing::info!(job = job_id, from = from.as_str(), to = to.as_str(), mv = mv.name(), by = %who, "CD job moved");
        Ok(Moved { from, job })
    }

    /// Would this move be allowed now? The same judgement as `cd_move`, changing nothing (a start that has to do work first asks this).
    pub async fn cd_check(
        &self,
        job_id: i64,
        mv: &Move,
        actor: &Actor,
    ) -> std::result::Result<Job, MoveError> {
        let job = self.cd_job_any(job_id).await?.ok_or(MoveError::NotFound)?;
        let from = CdState::parse(&job.cd_state).ok_or(MoveError::Internal)?;
        judge(
            from,
            job.user_id,
            &job.reason,
            job.decided_ms,
            &job.decided_by,
            mv,
            actor,
        )?;
        Ok(job)
    }

    /// A job by its number, whoever started it (the server's own work and the decisions of approvers need this).
    pub async fn cd_job_any(&self, id: i64) -> Result<Option<Job>> {
        let row = self
            .db
            .writer
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("SELECT {} FROM cd_job WHERE id = ?", cdimport::JOB_COLUMNS),
                [id.into()],
            ))
            .await?;
        row.as_ref().map(cdimport::job_from_row).transpose()
    }

    /// The jobs an approver sees besides their own: those of others that wait for a decision or for a new try.
    pub async fn cd_jobs_waiting(&self) -> Result<Vec<Job>> {
        let rows = self
            .db
            .writer
            .query_all_raw(Statement::from_string(
                DbBackend::Sqlite,
                format!("SELECT {} FROM cd_job WHERE cd_state IN ('awaiting_approval', 'retry_wait') ORDER BY updated_ms DESC, id DESC", cdimport::JOB_COLUMNS),
            ))
            .await?;
        rows.iter().map(cdimport::job_from_row).collect()
    }

    /// Writes down the request of a rip of the server, so that `retry` can start the same rip again.
    pub async fn set_cd_request(&self, job_id: i64, request: &str) -> Result<()> {
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE cd_job SET request = ? WHERE id = ?",
                [request.to_owned().into(), job_id.into()],
            ))
            .await?;
        Ok(())
    }

    /// The request of a rip as written down (empty if there is none).
    pub async fn cd_request(&self, job_id: i64) -> Result<String> {
        let row = self
            .db
            .writer
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT request FROM cd_job WHERE id = ?",
                [job_id.into()],
            ))
            .await?;
        Ok(match row {
            Some(r) => r.try_get_by_index(0)?,
            None => String::new(),
        })
    }

    /// Jobs in the given state, oldest first (restart, expiry).
    pub async fn cd_ids_in(&self, state: CdState) -> Result<Vec<i64>> {
        let rows = self
            .db
            .writer
            .query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM cd_job WHERE cd_state = ? ORDER BY id",
                [state.as_str().into()],
            ))
            .await?;
        rows.iter().map(|r| Ok(r.try_get_by_index(0)?)).collect()
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The held folder.

/// What is needed to place a held CD later, as written to `cd_held.context`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeldContext {
    pub meta: Meta,
    pub allow_gaps: bool,
    pub kind: String,
    pub tracks: Vec<HeldTrack>,
    /// The file name of the cover inside the folder, and its type.
    pub cover: Option<(String, String)>,
    pub record: Option<crate::cdpresent::DiscRow>,
    pub info: std::collections::HashMap<u32, (String, u32)>,
    pub note: String,
    pub error: Option<String>,
    pub reserve: u64,
}

/// One file of a held CD: its number on the disc, its title and its name inside the folder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeldTrack {
    pub number: u32,
    pub title: String,
    pub file: String,
}

/// The folder of a held CD: below the folder of the setting `cd_held_folder`, else below the staging folder of the CD import.
/// A folder inside a library folder is never used (held files must stay out of the library); the staging folder is taken then.
pub async fn held_base(store: &Store, cd: &CdService, set: &CdSettings) -> PathBuf {
    let wanted = set.held_folder.trim();
    if wanted.is_empty() {
        return cd.staging.clone();
    }
    let path = PathBuf::from(wanted);
    let state = store.import_state().await.ok();
    let inside = |root: &str| {
        let root = root.trim_end_matches('/');
        !root.is_empty() && (path == Path::new(root) || path.starts_with(root))
    };
    let in_library = state.is_some_and(|s| {
        s.roots
            .iter()
            .chain(s.music_roots.iter())
            .any(|r| inside(r))
    }) || cd.library.as_deref().is_some_and(|l| path.starts_with(l));
    if in_library {
        tracing::warn!(
            folder = wanted,
            "cd_held_folder lies inside a library folder; the files of held CDs are kept below the staging folder instead"
        );
        return cd.staging.clone();
    }
    path
}

/// The folder of one held job.
pub fn held_dir(base: &Path, job_id: i64) -> PathBuf {
    base.join(format!("{HELD_PREFIX}{job_id}"))
}

/// Does a CD of this person wait for approval after it is read? For a CD put in at the server (`SYSTEM_USER`) the setting `cd_auto_approval`
/// decides, for everybody else the right `cd_no_approval`.
pub async fn needs_approval(store: &Store, user_id: i64, set: &CdSettings) -> bool {
    if user_id == cdimport::SYSTEM_USER {
        return set.auto_approval != "now";
    }
    !store
        .cd_rights(user_id)
        .await
        .map(|r| r.no_approval)
        .unwrap_or(false)
}

/// What `conclude` did.
pub enum Concluded {
    /// The files are placed (and the import of the library is queued, or locked): the job, what was placed, whether the import is queued.
    Imported(Job, cdimport::Placed, bool),
    /// The files wait for approval.
    Held(Job),
}

/// The end of a read CD: the job goes `ripping` -> `ripped`, and then either the files are placed at once (`imported`), or, when the person
/// needs approval, they go to the held folder (`awaiting_approval`) and nobody sees them until somebody approves. A repair (`heal`) never
/// waits: the CD is in the library already.
#[allow(clippy::too_many_arguments)]
pub async fn conclude(
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
) -> std::result::Result<Concluded, Refusal> {
    let moved = store
        .cd_move(job_id, Move::Ripped, &Actor::Server)
        .await
        .map_err(|e| Refusal(e.status(), e.code()))?;
    cd.publish(user_id, "progress", &moved.job);
    let wait = extra.heal.is_none() && needs_approval(store, user_id, set).await;
    if wait {
        let job = hold(
            store, cd, user_id, job_id, meta, set, &kind, cover, tracks, extra, reserve,
        )
        .await?;
        return Ok(Concluded::Held(job));
    }
    let (job, placed, queued) = cdimport::finalize(
        store, cd, user_id, job_id, meta, set, root, kind, cover, tracks, reserve, extra,
    )
    .await?;
    notify(&CdEvent::Imported { job: job.clone() });
    Ok(Concluded::Imported(job, placed, queued))
}

/// Puts the files of a read CD into the held folder, writes down what is needed to place them, and moves the job to `awaiting_approval`.
#[allow(clippy::too_many_arguments)]
async fn hold(
    store: &Store,
    cd: &CdService,
    user_id: i64,
    job_id: i64,
    meta: &Meta,
    set: &CdSettings,
    kind: &str,
    cover: Option<(Vec<u8>, &'static str)>,
    tracks: Vec<StagedTrack>,
    extra: FinalizeExtra,
    reserve: u64,
) -> std::result::Result<Job, Refusal> {
    let base = held_base(store, cd, set).await;
    let dir = held_dir(&base, job_id);
    let (meta2, ext, kind2) = (meta.clone(), set.extension.clone(), kind.to_owned());
    let dir2 = dir.clone();
    // Moving files is blocking work.
    let context = tokio::task::spawn_blocking(move || -> std::io::Result<HeldContext> {
        std::fs::create_dir_all(&dir2)?;
        let mut held = Vec::new();
        for t in tracks {
            let file = format!("{:03}.{}", t.number, ext);
            cdimport::move_file(&t.path, &dir2.join(&file))?;
            held.push(HeldTrack { number: t.number, title: t.title, file });
        }
        let cover = match cover {
            Some((bytes, mime)) => {
                std::fs::write(dir2.join("cover.bin"), bytes)?;
                Some(("cover.bin".to_owned(), mime.to_owned()))
            }
            None => None,
        };
        Ok(HeldContext { allow_gaps: meta2.allow_gaps, meta: meta2, kind: kind2, tracks: held, cover, record: extra.record, info: extra.info, note: extra.note, error: extra.error, reserve })
    })
    .await
    .map_err(|_| Refusal(500, "internal"))?
    .map_err(|e| {
        tracing::warn!(error = %e, job = job_id, "CD: the files could not be put into the held folder");
        let _ = std::fs::remove_dir_all(&dir);
        Refusal(500, "internal")
    })?;
    let text = serde_json::to_string(&context).map_err(|_| Refusal(500, "internal"))?;
    let write = store
        .db
        .writer
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO cd_held (job_id, folder, context, held_ms) VALUES (?, ?, ?, ?) ON CONFLICT (job_id) DO UPDATE SET folder = excluded.folder, context = excluded.context, held_ms = excluded.held_ms",
            [job_id.into(), dir.to_string_lossy().into_owned().into(), text.into(), now_ms().into()],
        ))
        .await;
    if write.is_err() {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(Refusal(500, "internal"));
    }
    let moved = match store.cd_move(job_id, Move::Hold, &Actor::Server).await {
        Ok(m) => m,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            let _ = forget_held(store, job_id).await;
            return Err(Refusal(e.status(), e.code()));
        }
    };
    let message = format!(
        "{} - {}, CD {}: {} tracks wait for approval",
        meta.author.trim(),
        meta.album.trim(),
        meta.disc,
        meta.tracks.len()
    );
    let job = store
        .update_cd_job(
            user_id,
            job_id,
            &cdimport::JobPatch {
                message: Some(message),
                track: Some(meta.tracks.len() as i64),
                ..Default::default()
            },
        )
        .await
        .ok()
        .flatten()
        .unwrap_or(moved.job);
    cd.publish(user_id, "progress", &job);
    notify(&CdEvent::AwaitingApproval { job: job.clone() });
    Ok(job)
}

async fn forget_held(store: &Store, job_id: i64) -> Result<()> {
    store
        .db
        .writer
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM cd_held WHERE job_id = ?",
            [job_id.into()],
        ))
        .await?;
    Ok(())
}

/// The held record of a job: the folder and what is needed to place the files.
async fn held_of(store: &Store, job_id: i64) -> Result<Option<(PathBuf, HeldContext, i64)>> {
    let row = store
        .db
        .writer
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT folder, context, held_ms FROM cd_held WHERE job_id = ?",
            [job_id.into()],
        ))
        .await?;
    let Some(r) = row else { return Ok(None) };
    let folder: String = r.try_get_by_index(0)?;
    let text: String = r.try_get_by_index(1)?;
    let held_ms: i64 = r.try_get_by_index(2)?;
    Ok(serde_json::from_str::<HeldContext>(&text)
        .ok()
        .map(|c| (PathBuf::from(folder), c, held_ms)))
}

/// When the CD of a job was put into the held folder (milliseconds), if it waits.
pub async fn held_since(store: &Store, job_id: i64) -> Option<i64> {
    held_of(store, job_id).await.ok().flatten().map(|h| h.2)
}

/// Deletes the held files of a job and forgets the record.
async fn discard_held(store: &Store, job_id: i64) {
    if let Ok(Some((dir, _, _))) = held_of(store, job_id).await {
        let _ = std::fs::remove_dir_all(dir);
    }
    let _ = forget_held(store, job_id).await;
}

/// A person approves a held CD: the files go into the library and are imported as every other CD is. Of two deciders the first wins.
/// If the files cannot be placed (the same files exist now, no room), the job waits for approval again with the reason in its message.
pub async fn approve(
    store: &Store,
    cd: &CdService,
    library: Option<&Path>,
    job_id: i64,
    actor: &Actor,
) -> std::result::Result<Job, MoveError> {
    let moved = store.cd_move(job_id, Move::Approve, actor).await?;
    let user_id = moved.job.user_id;
    cd.publish(user_id, "progress", &moved.job);
    let Some((dir, ctx, _)) = held_of(store, job_id).await? else {
        // Nothing to place: the files are gone. Say so, and do not leave the job half decided.
        let _ = store
            .cd_move(job_id, Move::Fail("io".into()), &Actor::Server)
            .await;
        return Err(MoveError::Internal);
    };
    let set = CdSettings::load(store).await?;
    let root = match cdimport::import_root(store, library, &ctx.kind).await {
        Ok(r) => r,
        Err(r) => return Err(unplace(store, cd, user_id, job_id, r.1).await),
    };
    let mut meta = ctx.meta.clone();
    meta.allow_gaps = ctx.allow_gaps;
    let staged: Vec<StagedTrack> = ctx
        .tracks
        .iter()
        .map(|t| StagedTrack {
            number: t.number,
            title: t.title.clone(),
            path: dir.join(&t.file),
        })
        .collect();
    let cover = ctx.cover.as_ref().and_then(|(file, mime)| {
        let bytes = std::fs::read(dir.join(file)).ok()?;
        let mime: &'static str = if mime == "image/png" {
            "image/png"
        } else {
            "image/jpeg"
        };
        Some((bytes, mime))
    });
    let extra = FinalizeExtra {
        heal: None,
        record: ctx.record.clone(),
        info: ctx.info.clone(),
        note: ctx.note.clone(),
        error: ctx.error.clone(),
    };
    match cdimport::finalize(
        store,
        cd,
        user_id,
        job_id,
        &meta,
        &set,
        root,
        ctx.kind.clone(),
        cover,
        staged,
        ctx.reserve,
        extra,
    )
    .await
    {
        Ok((job, _, _)) => {
            discard_held(store, job_id).await;
            notify(&CdEvent::Decided {
                job: job.clone(),
                approved: true,
                by: actor_name(actor),
            });
            Ok(job)
        }
        Err(r) => Err(unplace(store, cd, user_id, job_id, r.1).await),
    }
}

/// An approval that could not be carried out: the job waits for approval again, with the reason.
async fn unplace(
    store: &Store,
    cd: &CdService,
    user_id: i64,
    job_id: i64,
    code: &str,
) -> MoveError {
    let _ = store
        .update_cd_job(
            user_id,
            job_id,
            &cdimport::JobPatch {
                message: Some(format!(
                    "the files could not be placed [{code}]; the CD waits for approval again"
                )),
                ..Default::default()
            },
        )
        .await;
    // From `ripped` (approved) back to `awaiting_approval`; if finalize had moved on already this is refused and the job stays as it is.
    if let Ok(m) = store.cd_move(job_id, Move::Hold, &Actor::Server).await {
        cd.publish(user_id, "progress", &m.job);
    }
    tracing::warn!(
        job = job_id,
        code,
        "an approved CD could not be placed; it waits for approval again"
    );
    MoveError::Internal
}

/// A person rejects a held CD: its files are deleted. Of two deciders the first wins.
pub async fn reject(
    store: &Store,
    cd: &CdService,
    job_id: i64,
    actor: &Actor,
) -> std::result::Result<Job, MoveError> {
    let moved = store.cd_move(job_id, Move::Reject, actor).await?;
    discard_held(store, job_id).await;
    cd.publish(moved.job.user_id, "failed", &moved.job);
    notify(&CdEvent::Decided {
        job: moved.job.clone(),
        approved: false,
        by: actor_name(actor),
    });
    Ok(moved.job)
}

fn actor_name(actor: &Actor) -> String {
    match actor {
        Actor::Person { name, .. } => name.clone(),
        Actor::Server => String::new(),
    }
}

/// Lets the held CDs that waited longer than `cd_held_keep_days` expire: `rejected` with the reason `expired`, files deleted. `now` is a
/// parameter so that a test can set the clock. Returns the ids that expired.
pub async fn expire_held(store: &Store, cd: &CdService, now: i64) -> Vec<i64> {
    let Ok(set) = CdSettings::load(store).await else {
        return Vec::new();
    };
    let keep_ms = i64::from(set.held_keep_days) * 24 * 3600 * 1000;
    let Ok(rows) = store
        .db
        .writer
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT job_id FROM cd_held WHERE held_ms + ? <= ?",
            [keep_ms.into(), now.into()],
        ))
        .await
    else {
        return Vec::new();
    };
    let mut expired = Vec::new();
    for row in rows {
        let Ok(id) = row.try_get_by_index::<i64>(0) else {
            continue;
        };
        match store.cd_move(id, Move::Expire, &Actor::Server).await {
            Ok(moved) => {
                discard_held(store, id).await;
                cd.publish(moved.job.user_id, "failed", &moved.job);
                notify(&CdEvent::Expired { job: moved.job });
                expired.push(id);
            }
            // Decided meanwhile: nothing to do.
            Err(MoveError::Invalid { .. } | MoveError::Decided { .. }) => {}
            Err(e) => tracing::warn!(job = id, error = e.code(), "a held CD could not be expired"),
        }
    }
    expired
}

/// The background task: looks for expired held CDs every few minutes, and once at the start.
pub async fn expiry_task(store: Store, cd: CdService) {
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(600));
    loop {
        tick.tick().await;
        expire_held(&store, &cd, now_ms()).await;
    }
}

/// After a restart: the jobs of the server's rips that were under way become `retry_wait` (what was read waits in the resume folder), other
/// jobs that were under way (an upload, a placing) fail as `interrupted`; a CD that was approved but not placed waits for approval again.
/// Held folders without a record (a crash while holding) are removed. Returns how many jobs were touched.
pub async fn recover(store: &Store, cd: &CdService) -> u64 {
    let mut n = 0;
    for id in store.cd_ids_in(CdState::Ripping).await.unwrap_or_default() {
        let Ok(Some(job)) = store.cd_job_any(id).await else {
            continue;
        };
        let by_server = matches!(job.device.as_str(), "server" | "auto");
        let mv = if by_server && matches!(job.phase.as_str(), "ripping" | "encoding" | "verifying")
        {
            Move::Interrupted
        } else {
            Move::Fail("interrupted".into())
        };
        if let Ok(m) = store.cd_move(id, mv, &Actor::Server).await {
            n += 1;
            let _ = store
                .update_cd_job(
                    job.user_id,
                    id,
                    &cdimport::JobPatch {
                        message: Some("the server was restarted".into()),
                        ..Default::default()
                    },
                )
                .await;
            cd.publish(m.job.user_id, "failed", &m.job);
        }
    }
    for id in store.cd_ids_in(CdState::Ripped).await.unwrap_or_default() {
        let held = held_of(store, id).await.ok().flatten().is_some();
        let mv = if held { Move::Hold } else { Move::Interrupted };
        if store.cd_move(id, mv, &Actor::Server).await.is_ok() {
            n += 1;
        }
    }
    n
}

/// Removes folders `held-<id>` of the held base that no record belongs to.
pub async fn sweep_held(store: &Store, cd: &CdService) {
    let Ok(set) = CdSettings::load(store).await else {
        return;
    };
    let base = held_base(store, cd, &set).await;
    let Ok(entries) = std::fs::read_dir(&base) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(id) = name
            .strip_prefix(HELD_PREFIX)
            .and_then(|n| n.parse::<i64>().ok())
        else {
            continue;
        };
        if !matches!(held_of(store, id).await, Ok(Some(_))) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The hooks for the inbox (phase 3).

/// What happened to a CD that a person may need to be told about. The inbox of phase 3 turns each into a notice or a task for the people who
/// should get it (the starter, the approvers); until then `notify` does nothing.
#[derive(Debug, Clone)]
pub enum CdEvent {
    /// The CD is whole in the library already (`cd_present`); the disc was ejected.
    Present { job: Job },
    /// The files are placed (`cd_imported`).
    Imported { job: Job },
    /// A track could not be read; the job waits in `retry_wait` (`cd_read_error`).
    ReadError { job: Job, tracks: Vec<u32> },
    /// A CD waits for a decision (`cd_awaiting_approval`, a task for everybody who may approve).
    AwaitingApproval { job: Job },
    /// A person decided (`cd_decided`).
    Decided {
        job: Job,
        approved: bool,
        by: String,
    },
    /// The CD waited too long and was rejected (`cd_expired`).
    Expired { job: Job },
    /// The automatic import did not start: MusicBrainz knows no release (`cd_no_match`) or several (`cd_ambiguous`).
    NotStarted { disc_id: String, reason: String },
}

/// The hook of phase 3: tells the inbox that something happened. Empty on purpose until the inbox exists; every place that makes one of these
/// events calls it already.
pub fn notify(event: &CdEvent) {
    tracing::debug!(?event, "CD event (no inbox yet)");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moves() -> Vec<Move> {
        vec![
            Move::Retry,
            Move::GiveUp,
            Move::Approve,
            Move::Reject,
            Move::ReadFailed("read".into()),
            Move::Interrupted,
            Move::Present,
            Move::Ripped,
            Move::Hold,
            Move::Place,
            Move::ImportFailed,
            Move::Fail("io".into()),
            Move::Cancel,
            Move::Expire,
        ]
    }

    /// Every allowed move and its target; everything not listed is refused.
    #[test]
    fn the_table_of_moves_is_exactly_the_flow_of_the_spec() {
        use CdState::*;
        let allowed: Vec<(CdState, &str, CdState)> = vec![
            (Ripping, "read_failed", RetryWait),
            (Ripping, "interrupted", RetryWait),
            (Ripped, "interrupted", RetryWait),
            (RetryWait, "retry", Ripping),
            (RetryWait, "give_up", Cancelled),
            (RetryWait, "cancel", Cancelled),
            (Ripping, "cancel", Cancelled),
            (Ripping, "present", CdState::Present),
            (Ripping, "ripped", Ripped),
            (Ripped, "hold", AwaitingApproval),
            (Ripped, "place", Imported),
            (AwaitingApproval, "approve", Ripped),
            (AwaitingApproval, "reject", Rejected),
            (AwaitingApproval, "expire", Rejected),
            (Imported, "import_failed", Failed),
            (Ripping, "fail", Failed),
            (Ripped, "fail", Failed),
        ];
        for from in STATES {
            for mv in moves() {
                let want = allowed
                    .iter()
                    .find(|(f, m, _)| f == from && *m == mv.name())
                    .map(|(_, _, t)| *t);
                assert_eq!(
                    mv.target(*from),
                    want,
                    "{} from {}",
                    mv.name(),
                    from.as_str()
                );
            }
        }
        // The end states lead nowhere.
        for end in [
            Imported,
            Rejected,
            CdState::Present,
            Cancelled,
            Failed,
            Skipped,
        ] {
            let out = moves()
                .iter()
                .filter(|m| m.target(end).is_some())
                .map(|m| m.name())
                .collect::<Vec<_>>();
            assert!(
                out.is_empty() || (end == Imported && out == ["import_failed"]),
                "{} leads to {out:?}",
                end.as_str()
            );
        }
        assert_eq!(CdState::parse("awaiting_approval"), Some(AwaitingApproval));
        assert_eq!(CdState::parse("nonsense"), None);
    }

    #[test]
    fn rights_decide_who_may_make_which_move() {
        let person = |id: i64, may_approve: bool| Actor::Person {
            id,
            name: format!("p{id}"),
            rights: CdRights {
                may_insert: true,
                no_approval: false,
                may_approve,
            },
        };
        let starter = 5;
        // retry and give up: the starter or somebody who may approve; the drive's own jobs (user 0) only the latter.
        for mv in [Move::Retry, Move::GiveUp] {
            assert!(
                allowed(&person(5, false), &mv, starter),
                "the starter may {}",
                mv.name()
            );
            assert!(
                allowed(&person(9, true), &mv, starter),
                "an approver may {}",
                mv.name()
            );
            assert!(
                !allowed(&person(9, false), &mv, starter),
                "somebody else may not {}",
                mv.name()
            );
            assert!(
                !allowed(&person(0, false), &mv, cdimport::SYSTEM_USER),
                "nobody is the starter of a CD put in at the server"
            );
            assert!(allowed(&person(9, true), &mv, cdimport::SYSTEM_USER));
            assert!(
                !allowed(&Actor::Server, &mv, starter),
                "the server makes no move of people"
            );
        }
        // approve and reject: only somebody who may approve, the starter of the CD included in the rule.
        for mv in [Move::Approve, Move::Reject] {
            assert!(allowed(&person(9, true), &mv, starter));
            assert!(
                !allowed(&person(5, false), &mv, starter),
                "the starter may not decide their own CD"
            );
            assert!(!allowed(&Actor::Server, &mv, starter));
        }
        // The server's own moves are not for people.
        for mv in moves().into_iter().filter(|m| !m.by_people()) {
            assert!(allowed(&Actor::Server, &mv, starter));
            assert!(
                !allowed(&person(9, true), &mv, starter),
                "{} is the server's",
                mv.name()
            );
        }
    }
}
