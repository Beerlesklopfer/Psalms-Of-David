// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The inbox of the CDs (docs/spec-cd-inbox.md, "The inbox" and "Delivery"; migration 0060).
//!
//! An item is a **notice** (every recipient has their own read mark) or a **task** (one decision for everybody). The database holds
//! only a `code` and its `args` (JSON); the apps and the web panel make the text in their language (`admin/messages.rs` has the
//! texts for the panel). `handle` is the one place that turns a `CdEvent` into items: it picks the recipients by the rights, writes
//! the items, and pushes them over the event channel to the open apps of the recipients (and only to them).

use std::collections::BTreeSet;

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde_json::{Value, json};

use crate::cdflow::CdEvent;
use crate::cdimport::{self, CdService, Job};
use crate::error::Result;
use crate::store::{Store, now_ms};

/// The default and the limits of the setting `inbox_keep_days`.
pub const KEEP_DAYS_DEFAULT: i64 = 90;
pub const KEEP_DAYS_RANGE: (i64, i64) = (1, 3650);
/// The page of the list: the default and the most.
pub const PAGE_DEFAULT: i64 = 50;
pub const PAGE_MAX: i64 = 200;

/// The codes of the messages (the apps and the panel know the texts). Only these are written.
pub const CODES: [&str; 11] = [
    "cd_present",
    "cd_imported",
    "cd_read_error",
    "cd_no_match",
    "cd_ambiguous",
    "cd_awaiting_approval",
    "cd_decided",
    "cd_expired",
    "cd_heal_needed",
    "cd_no_space",
    "cd_no_target",
];

/// Codes that a device may say aloud (`speak` is set for them only).
const SPOKEN: [&str; 4] = [
    "cd_imported",
    "cd_awaiting_approval",
    "cd_read_error",
    "cd_present",
];

/// One item as one recipient sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: i64,
    pub kind: String,
    pub job_id: Option<i64>,
    pub code: String,
    pub args: Value,
    pub created_ms: i64,
    pub read: bool,
    pub resolved_ms: Option<i64>,
    pub resolution: String,
    pub by: String,
    /// The recipient may decide this task now (an open task, and they are one of the deciders).
    pub can_decide: bool,
    /// `starter` or `home` for a message that may be spoken; `None` otherwise.
    pub speak: Option<String>,
    /// For `starter`: the id of the device that started the rip (the `device` of the `hello` message of the event channel).
    pub device: String,
    /// The state of the job now (`retry_wait` tells the app that "try again" still works).
    pub job_state: Option<String>,
}

impl Item {
    pub fn json(&self) -> Value {
        let mut v = json!({
            "id": self.id, "kind": self.kind, "code": self.code, "args": self.args, "job_id": self.job_id,
            "created_ms": self.created_ms, "read": self.read, "resolved": self.resolved_ms.is_some(),
            "can_decide": self.can_decide, "job_state": self.job_state,
        });
        if let Some(ms) = self.resolved_ms {
            v["resolved_ms"] = json!(ms);
            v["resolution"] = json!(self.resolution);
            v["by"] = json!(self.by);
        }
        if let Some(speak) = &self.speak {
            v["speak"] = json!(speak);
            v["device"] = json!(self.device);
        }
        v
    }
}

/// An item to write.
pub struct NewItem {
    pub kind: &'static str,
    pub job_id: Option<i64>,
    pub code: &'static str,
    pub args: Value,
    pub speak: Option<&'static str>,
    pub device: String,
}

const SELECT: &str = "SELECT i.id, i.kind, i.job_id, i.code, i.args, i.created_at, r.read_at, i.resolved_at, i.resolution, i.resolved_name, r.decide, \
                      i.speak, i.device, (SELECT cd_state FROM cd_job WHERE id = i.job_id) \
                      FROM inbox_item i JOIN inbox_recipient r ON r.item_id = i.id";
/// A task that is decided is no longer the business of somebody who could not decide it (the entry of the job of a starter goes).
const VISIBLE: &str = "NOT (i.kind = 'task' AND i.resolved_at IS NOT NULL AND r.decide = 0)";

fn item_of(r: &sea_orm::QueryResult) -> Result<Item> {
    let kind: String = r.try_get_by_index(1)?;
    let args: String = r.try_get_by_index(4)?;
    let resolved_ms: Option<i64> = r.try_get_by_index(7)?;
    let decide = r.try_get_by_index::<i64>(10)? != 0;
    Ok(Item {
        id: r.try_get_by_index(0)?,
        can_decide: kind == "task" && resolved_ms.is_none() && decide,
        kind,
        job_id: r.try_get_by_index(2)?,
        code: r.try_get_by_index(3)?,
        args: serde_json::from_str(&args).unwrap_or(Value::Null),
        created_ms: r.try_get_by_index(5)?,
        read: r.try_get_by_index::<Option<i64>>(6)?.is_some(),
        resolved_ms,
        resolution: r.try_get_by_index(8)?,
        by: r.try_get_by_index(9)?,
        speak: r.try_get_by_index(11)?,
        device: r.try_get_by_index(12)?,
        job_state: r.try_get_by_index(13)?,
    })
}

impl Store {
    /// Writes an item for the recipients (`(profile, may decide)`); returns its id.
    pub async fn inbox_add(&self, item: &NewItem, recipients: &[(i64, bool)]) -> Result<i64> {
        let w = &self.db.writer;
        let res = w
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO inbox_item (kind, topic, job_id, code, args, speak, device) VALUES (?, 'cd', ?, ?, ?, ?, ?)",
                [item.kind.into(), item.job_id.into(), item.code.into(), item.args.to_string().into(), item.speak.map(str::to_owned).into(), item.device.clone().into()],
            ))
            .await?;
        let id = res.last_insert_id() as i64;
        for (profile, decide) in recipients {
            w.execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT OR IGNORE INTO inbox_recipient (item_id, profile_id, decide) VALUES (?, ?, ?)",
                [id.into(), (*profile).into(), i64::from(*decide).into()],
            ))
            .await?;
        }
        Ok(id)
    }

    /// One item as a recipient sees it; `None` if they are none.
    pub async fn inbox_item(&self, profile: i64, id: i64) -> Result<Option<Item>> {
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("{SELECT} WHERE r.profile_id = ? AND i.id = ? AND {VISIBLE}"),
                [profile.into(), id.into()],
            ))
            .await?;
        row.map(|r| item_of(&r)).transpose()
    }

    /// The items of a person, newest first: the ones with an id below `after`, at most `limit`.
    pub async fn inbox_list(
        &self,
        profile: i64,
        after: Option<i64>,
        limit: i64,
    ) -> Result<Vec<Item>> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("{SELECT} WHERE r.profile_id = ? AND i.id < ? AND {VISIBLE} ORDER BY i.id DESC LIMIT ?"),
                [profile.into(), after.unwrap_or(i64::MAX).into(), limit.into()],
            ))
            .await?;
        rows.iter().map(item_of).collect()
    }

    /// How many items a person has not read, and how many open tasks they may decide.
    pub async fn inbox_counts(&self, profile: i64) -> Result<(i64, i64)> {
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT coalesce(sum(r.read_at IS NULL), 0), coalesce(sum(i.kind = 'task' AND i.resolved_at IS NULL AND r.decide = 1), 0) \
                     FROM inbox_item i JOIN inbox_recipient r ON r.item_id = i.id WHERE r.profile_id = ? AND {VISIBLE}"
                ),
                [profile.into()],
            ))
            .await?;
        match row {
            Some(r) => Ok((r.try_get_by_index(0)?, r.try_get_by_index(1)?)),
            None => Ok((0, 0)),
        }
    }

    /// Marks an item read for one person. `false` if the person is no recipient of it. Reading twice is fine.
    pub async fn inbox_mark_read(&self, profile: i64, id: i64) -> Result<bool> {
        let res = self
            .db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE inbox_recipient SET read_at = coalesce(read_at, ?) WHERE item_id = ? AND profile_id = ?",
                [now_ms().into(), id.into(), profile.into()],
            ))
            .await?;
        Ok(res.rows_affected() > 0)
    }

    /// Marks everything read for one person. Returns how many were unread.
    pub async fn inbox_mark_all_read(&self, profile: i64) -> Result<u64> {
        let res = self
            .db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE inbox_recipient SET read_at = ? WHERE profile_id = ? AND read_at IS NULL",
                [now_ms().into(), profile.into()],
            ))
            .await?;
        Ok(res.rows_affected())
    }

    /// The open tasks of a job.
    async fn inbox_open_tasks(&self, job_id: i64) -> Result<Vec<i64>> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT id FROM inbox_item WHERE kind = 'task' AND job_id = ? AND resolved_at IS NULL", [job_id.into()]))
            .await?;
        rows.iter().map(|r| Ok(r.try_get_by_index(0)?)).collect()
    }

    /// The people an item went to.
    async fn inbox_recipients(&self, id: i64) -> Result<Vec<i64>> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT profile_id FROM inbox_recipient WHERE item_id = ?",
                [id.into()],
            ))
            .await?;
        rows.iter().map(|r| Ok(r.try_get_by_index(0)?)).collect()
    }

    /// The task is decided: it is done for everybody (and read: nothing is left to do). `by` is the profile and its nickname.
    async fn inbox_resolve(
        &self,
        id: i64,
        by: Option<(i64, &str)>,
        resolution: &str,
    ) -> Result<i64> {
        let now = now_ms();
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE inbox_item SET resolved_at = ?, resolved_by = ?, resolved_name = ?, resolution = ? WHERE id = ? AND resolved_at IS NULL",
                [now.into(), by.map(|b| b.0).into(), by.map_or("", |b| b.1).to_owned().into(), resolution.to_owned().into(), id.into()],
            ))
            .await?;
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE inbox_recipient SET read_at = ? WHERE item_id = ? AND read_at IS NULL",
                [now.into(), id.into()],
            ))
            .await?;
        Ok(now)
    }

    /// Removes the items made before `now - keep_days`, except a task that nobody decided yet. Returns how many went.
    pub async fn inbox_cleanup(&self, now: i64, keep_days: i64) -> Result<u64> {
        let before = now - keep_days * 24 * 3600 * 1000;
        let w = &self.db.writer;
        let old = "SELECT id FROM inbox_item WHERE created_at < ? AND NOT (kind = 'task' AND resolved_at IS NULL)";
        w.execute_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("DELETE FROM inbox_recipient WHERE item_id IN ({old})"),
            [before.into()],
        ))
        .await?;
        let res = w
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("DELETE FROM inbox_item WHERE id IN ({old})"),
                [before.into()],
            ))
            .await?;
        Ok(res.rows_affected())
    }

    /// What the two tables hold of every item: `(code, args)`, oldest first (the tests look that nothing but codes and data is stored).
    pub async fn inbox_stored(&self) -> Result<Vec<(String, String)>> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT code, args FROM inbox_item ORDER BY id",
            ))
            .await?;
        rows.iter()
            .map(|r| Ok((r.try_get_by_index(0)?, r.try_get_by_index(1)?)))
            .collect()
    }

    /// The device that started the rip of a job (empty: the web, MCP or the drive itself).
    pub async fn cd_starter_device(&self, job_id: i64) -> Result<String> {
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT starter_device FROM cd_job WHERE id = ?",
                [job_id.into()],
            ))
            .await?;
        Ok(row
            .map(|r| r.try_get_by_index(0))
            .transpose()?
            .unwrap_or_default())
    }

    /// Writes down which device an app started the job with.
    pub async fn set_cd_starter_device(&self, job_id: i64, device: &str) -> Result<()> {
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE cd_job SET starter_device = ? WHERE id = ?",
                [device.to_owned().into(), job_id.into()],
            ))
            .await?;
        Ok(())
    }
}

/// Removes what is older than `inbox_keep_days` (the setting; 90 when it is missing or wrong).
pub async fn cleanup(store: &Store, now: i64) -> u64 {
    let days = store
        .setting("inbox_keep_days")
        .await
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .filter(|d| (KEEP_DAYS_RANGE.0..=KEEP_DAYS_RANGE.1).contains(d))
        .unwrap_or(KEEP_DAYS_DEFAULT);
    store.inbox_cleanup(now, days).await.unwrap_or_else(|e| {
        tracing::warn!(error = %e, "the inbox could not be cleaned");
        0
    })
}

// ---------------------------------------------------------------------------------------------------------------
// From events to items.

/// The id of the album with this author and title, if the library has it (best effort: the apps search by name otherwise).
async fn album_id(store: &Store, author: &str, title: &str) -> Option<i64> {
    let row = store
        .db
        .reader
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT b.id FROM book b JOIN author a ON a.id = b.author_id WHERE a.name = ? COLLATE NOCASE AND b.title = ? COLLATE NOCASE ORDER BY b.id LIMIT 1",
            [author.trim().to_owned().into(), title.trim().to_owned().into()],
        ))
        .await
        .ok()??;
    row.try_get_by_index(0).ok()
}

/// The person who started a job, if they are told about their own CDs.
async fn starter_of(store: &Store, job: &Job) -> Option<i64> {
    (job.user_id != cdimport::SYSTEM_USER && store.cd_rights(job.user_id).await.ok()?.may_insert)
        .then_some(job.user_id)
}

async fn approvers(store: &Store) -> Vec<i64> {
    store
        .cd_approvers()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|a| a.0)
        .collect()
}

/// Who gets the notices of a job: the starter, and the approvers when the CD was put in at the server or its starter needs approval.
async fn watchers(store: &Store, job: &Job) -> Vec<i64> {
    let mut out = BTreeSet::new();
    let starter = starter_of(store, job).await;
    out.extend(starter);
    let needs = match starter {
        Some(id) => !store
            .cd_rights(id)
            .await
            .map(|r| r.no_approval)
            .unwrap_or(false),
        None => true,
    };
    if needs {
        out.extend(approvers(store).await);
    }
    out.into_iter().collect()
}

/// `speak` and the device for a message of a job.
async fn speech(store: &Store, job: &Job, code: &str) -> (Option<&'static str>, String) {
    if !SPOKEN.contains(&code) {
        return (None, String::new());
    }
    // The device is known for a rip an app started; for one started at the web, by MCP or at the drive it is not, and the message is for the home.
    let device = store.cd_starter_device(job.id).await.unwrap_or_default();
    if device.is_empty() {
        (Some("home"), String::new())
    } else {
        (Some("starter"), device)
    }
}

fn job_args(job: &Job) -> serde_json::Map<String, Value> {
    let mut m = serde_json::Map::new();
    m.insert("job_id".into(), json!(job.id));
    m.insert("author".into(), json!(job.author));
    m.insert("title".into(), json!(job.title));
    m.insert("disc".into(), json!(job.disc));
    m
}

/// Writes a notice for people and pushes it.
async fn notice(
    store: &Store,
    cd: &CdService,
    job: Option<&Job>,
    code: &'static str,
    args: Value,
    to: &[i64],
) {
    if to.is_empty() {
        return;
    }
    let (speak, device) = match job {
        Some(j) => speech(store, j, code).await,
        None => (SPOKEN.contains(&code).then_some("home"), String::new()),
    };
    let item = NewItem {
        kind: "notice",
        job_id: job.map(|j| j.id),
        code,
        args,
        speak,
        device,
    };
    let rec: Vec<(i64, bool)> = to.iter().map(|p| (*p, false)).collect();
    match store.inbox_add(&item, &rec).await {
        Ok(id) => push(store, cd, id, to).await,
        Err(e) => tracing::warn!(error = %e, code, "an inbox notice could not be written"),
    }
}

/// Sends an item to the open apps of the people it went to.
async fn push(store: &Store, cd: &CdService, id: i64, to: &[i64]) {
    for p in to {
        let (Ok(Some(item)), Ok((unread, tasks_open))) =
            (store.inbox_item(*p, id).await, store.inbox_counts(*p).await)
        else {
            continue;
        };
        let text = json!({ "v": cdimport::EVENTS_VERSION, "type": "inbox", "item": item.json(), "unread": unread, "tasks_open": tasks_open }).to_string();
        cd.publish_text(*p, text);
    }
}

/// A task is decided: marks it done for everybody and tells the open apps of the people it went to.
async fn resolve_tasks(
    store: &Store,
    cd: &CdService,
    job_id: i64,
    by: Option<(i64, &str)>,
    resolution: &str,
) {
    for id in store.inbox_open_tasks(job_id).await.unwrap_or_default() {
        let people = store.inbox_recipients(id).await.unwrap_or_default();
        let resolved_ms = match store.inbox_resolve(id, by, resolution).await {
            Ok(ms) => ms,
            Err(e) => {
                tracing::warn!(error = %e, "an inbox task could not be marked as decided");
                continue;
            }
        };
        for p in people {
            let (unread, tasks_open) = store.inbox_counts(p).await.unwrap_or((0, 0));
            let text = json!({
                "v": cdimport::EVENTS_VERSION, "type": "inbox_resolved", "id": id, "job_id": job_id, "resolution": resolution,
                "by": by.map_or("", |b| b.1), "resolved_ms": resolved_ms, "unread": unread, "tasks_open": tasks_open,
            })
            .to_string();
            cd.publish_text(p, text);
        }
    }
}

/// The one place that turns an event of a CD into items of the inbox.
pub async fn handle(store: &Store, cd: &CdService, event: &CdEvent) {
    match event {
        CdEvent::Present { job } => {
            let mut args = job_args(job);
            args.insert(
                "album_id".into(),
                json!(album_id(store, &job.author, &job.title).await),
            );
            notice(
                store,
                cd,
                Some(job),
                "cd_present",
                Value::Object(args),
                &watchers(store, job).await,
            )
            .await;
        }
        CdEvent::Imported { job } => {
            let mut args = job_args(job);
            args.insert(
                "album_id".into(),
                json!(album_id(store, &job.author, &job.title).await),
            );
            args.insert("discs_total".into(), json!(job.discs_total));
            args.insert("tracks".into(), json!(job.tracks_total));
            notice(
                store,
                cd,
                Some(job),
                "cd_imported",
                Value::Object(args),
                &watchers(store, job).await,
            )
            .await;
        }
        CdEvent::ReadError { job, tracks } => {
            let mut args = job_args(job);
            args.insert("tracks".into(), json!(tracks));
            args.insert("reason".into(), json!(job.error));
            notice(
                store,
                cd,
                Some(job),
                "cd_read_error",
                Value::Object(args),
                &watchers(store, job).await,
            )
            .await;
        }
        CdEvent::AwaitingApproval { job } => {
            if store
                .inbox_open_tasks(job.id)
                .await
                .map(|t| !t.is_empty())
                .unwrap_or(false)
            {
                return;
            }
            let approvers = approvers(store).await;
            let mut people: Vec<(i64, bool)> = approvers.iter().map(|a| (*a, true)).collect();
            // The starter sees the CD as an entry of the job, without the buttons.
            if let Some(s) = starter_of(store, job).await
                && !approvers.contains(&s)
            {
                people.push((s, false));
            }
            let mut args = job_args(job);
            args.insert("discs_total".into(), json!(job.discs_total));
            args.insert("tracks".into(), json!(job.tracks_total));
            args.insert(
                "by".into(),
                json!(if job.user_id == cdimport::SYSTEM_USER {
                    "auto"
                } else {
                    job.starter.as_str()
                }),
            );
            let (speak, device) = speech(store, job, "cd_awaiting_approval").await;
            let item = NewItem {
                kind: "task",
                job_id: Some(job.id),
                code: "cd_awaiting_approval",
                args: Value::Object(args),
                speak,
                device,
            };
            match store.inbox_add(&item, &people).await {
                Ok(id) => {
                    push(
                        store,
                        cd,
                        id,
                        &people.iter().map(|p| p.0).collect::<Vec<_>>(),
                    )
                    .await
                }
                Err(e) => tracing::warn!(error = %e, "an inbox task could not be written"),
            }
        }
        CdEvent::Decided {
            job,
            approved,
            by_id,
            by,
        } => {
            let resolution = if *approved { "approved" } else { "rejected" };
            resolve_tasks(store, cd, job.id, Some((*by_id, by)), resolution).await;
            let mut to: BTreeSet<i64> = approvers(store).await.into_iter().collect();
            to.extend(starter_of(store, job).await);
            to.remove(by_id);
            let mut args = job_args(job);
            args.insert("approved".into(), json!(approved));
            args.insert("by".into(), json!(by));
            notice(
                store,
                cd,
                Some(job),
                "cd_decided",
                Value::Object(args),
                &to.into_iter().collect::<Vec<_>>(),
            )
            .await;
        }
        CdEvent::Expired { job } => {
            resolve_tasks(store, cd, job.id, None, "expired").await;
            let mut to: BTreeSet<i64> = approvers(store).await.into_iter().collect();
            to.extend(starter_of(store, job).await);
            notice(
                store,
                cd,
                Some(job),
                "cd_expired",
                Value::Object(job_args(job)),
                &to.into_iter().collect::<Vec<_>>(),
            )
            .await;
        }
        CdEvent::NotStarted {
            disc_id,
            reason,
            author,
            title,
            disc,
            detail,
        } => {
            let code = match reason.as_str() {
                "no_match" => "cd_no_match",
                "ambiguous_match" => "cd_ambiguous",
                "heal_needed" => "cd_heal_needed",
                "no_space" => "cd_no_space",
                "no_target" => "cd_no_target",
                _ => return,
            };
            let mut args = serde_json::Map::new();
            args.insert("disc_id".into(), json!(disc_id));
            if code == "cd_ambiguous" {
                args.insert(
                    "candidates".into(),
                    json!(detail.parse::<u32>().unwrap_or(0)),
                );
            }
            if !title.is_empty() {
                args.insert("author".into(), json!(author));
                args.insert("title".into(), json!(title));
                args.insert("disc".into(), json!(disc));
            }
            notice(
                store,
                cd,
                None,
                code,
                Value::Object(args),
                &approvers(store).await,
            )
            .await;
        }
    }
}
