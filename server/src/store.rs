// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use sea_orm::ActiveValue::{Set, Unchanged};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DbBackend, EntityTrait, FromQueryResult,
    QueryFilter, QueryOrder, Statement, Value,
};

use crate::db::Db;
use crate::entities::{book, book_release, device, profile, progress, role_policy, setting, track};
use crate::error::{Error, Result};
use crate::roles::{Role, Schedule};
use crate::writer::{ProgressUpdate, WriteQueue, WriterStats};

/// How long progress updates are collected before they are written together.
const COALESCE_WINDOW: Duration = Duration::from_millis(500);

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Entry point for everything the server stores.
///
/// Reads use the read pool, writes use the single write connection. Progress
/// updates go through the write queue; everything else is written directly.
#[derive(Clone)]
pub struct Store {
    pub(crate) db: Db,
    queue: WriteQueue,
    /// The last check of the rights on the library folders (see `access`).
    pub(crate) access_cache: std::sync::Arc<crate::access::Cache>,
}

#[derive(Debug, Clone, FromQueryResult)]
pub struct ContinueEntry {
    pub book_id: i64,
    pub title: String,
    pub author: String,
    pub track_id: i64,
    pub position_ms: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, FromQueryResult)]
pub struct BookRow {
    pub id: i64,
    /// The place of the book below its import folder (`NORA/Die Rotsteinkids/...`); the first part names the owner of music.
    pub folder: String,
    /// `audiobook` or `music`.
    pub kind: String,
    pub title: String,
    pub author: String,
    /// Who reads the audiobook; `None` if the files do not say.
    pub narrator: Option<String>,
    /// The genre of the list (not the raw tag text); `None` if the album has none or it is hidden.
    pub genre: Option<String>,
    pub min_age: Option<i64>,
    pub recommended_age: Option<i64>,
    pub released: bool,
    pub tracks: i64,
    pub discs: i64,
    pub duration_ms: i64,
    /// The nickname of the family member the book belongs to.
    pub owner: Option<String>,
}

/// What the overview page of the administration interface shows.
#[derive(Debug, Clone, Serialize)]
pub struct Overview {
    pub server_id: String,
    pub users: i64,
    pub unreleased_books: i64,
    pub unconfirmed_defaults: i64,
    pub import_enabled: bool,
}

/// One line of the user list of the administration interface.
#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct UserRow {
    pub id: i64,
    /// The nickname: what is shown and what the user signs in with.
    pub name: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub birth_date: Option<String>,
    pub role: String,
    pub max_age: Option<i64>,
    /// The user's own device limit; `None` means the default of the role.
    pub max_devices: Option<i64>,
    /// Active (not revoked) devices.
    pub devices: i64,
}

/// One book as the administration interface shows it (all books, released or not).
#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct AdminBookRow {
    pub id: i64,
    pub kind: String,
    /// The place below the import folder; what the book is known by.
    pub folder: String,
    pub title: String,
    pub author: String,
    pub narrator: Option<String>,
    /// The genre of the list, and whether somebody chose it by hand (then the tags do not change it).
    pub genre: Option<String>,
    pub genre_id: Option<i64>,
    pub genre_locked: bool,
    /// The raw genre text of the tags.
    pub genre_tag: Option<String>,
    pub released: bool,
    pub min_age: Option<i64>,
    pub recommended_age: Option<i64>,
    pub quota_exempt: bool,
    pub description: Option<String>,
    pub tracks: i64,
    pub discs: i64,
    pub duration_ms: i64,
    /// The family member the book belongs to (not the same as who may see it).
    pub owner_id: Option<i64>,
    /// Their nickname.
    pub owner: Option<String>,
    /// How many CDs the work has when it is known (the family may own fewer of them); `None`: not said.
    pub discs_total: Option<i64>,
}

/// One archive in the upload queue.
#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct UploadJob {
    pub id: i64,
    pub name: String,
    /// `audiobook` or `music`.
    pub kind: String,
    pub size_bytes: i64,
    /// `queued`, `working`, `done` or `failed`.
    pub state: String,
    pub message: String,
    pub created_at: i64,
}

/// A token for a remote LLM (MCP). The token itself is not kept, only its hash.
#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct McpToken {
    pub id: i64,
    pub name: String,
    pub can_write: bool,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

/// One tag value that was changed through MCP.
#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct TagChange {
    pub id: i64,
    pub at: i64,
    pub token_name: String,
    pub book_id: i64,
    pub file: String,
    pub field: String,
    pub old_value: String,
    pub new_value: String,
}

/// Longest blurb of a book, in characters.
pub const MAX_DESCRIPTION_CHARS: usize = 4000;

/// A new user. A child needs an age limit and a weekly planner.
#[derive(Debug, Clone)]
pub struct NewProfile {
    pub name: String,
    pub role: Role,
    pub max_age: Option<i64>,
    pub schedule: Option<Schedule>,
}

impl NewProfile {
    pub fn new(name: &str, role: Role) -> NewProfile {
        NewProfile { name: name.to_owned(), role, max_age: None, schedule: None }
    }

    pub fn child(name: &str, max_age: i64, schedule: Schedule) -> NewProfile {
        NewProfile { name: name.to_owned(), role: Role::Child, max_age: Some(max_age), schedule: Some(schedule) }
    }
}

#[derive(Debug, Clone)]
pub struct NewDevice {
    pub user_id: i64,
    pub name: String,
    pub wg_pubkey: String,
    pub tunnel_ip: String,
    pub shared: bool,
    pub created_by: Option<i64>,
}

/// What may a user do with a book right now?
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Access {
    Allowed,
    /// The book is not released for this user. Answer as if it did not exist.
    NotVisible,
    /// The weekly planner does not allow listening now. `next_start` is the next
    /// allowed slot as (weekday with Monday = 0, minute of day), if there is one.
    OutsideSchedule { next_start: Option<(u8, u16)> },
}

#[derive(Debug, FromQueryResult)]
struct AccessRow {
    quota_exempt: bool,
    schedule: Option<Vec<u8>>,
}

/// Every book with what the administration interface needs; callers add ordering or a filter.
const ADMIN_BOOKS: &str = "SELECT b.id, b.kind, b.folder, b.title, a.name AS author, b.narrator, g.name AS genre, b.genre_id, b.genre_locked, b.genre_tag, b.released, b.min_age, b.recommended_age, \
    b.quota_exempt, b.description, (SELECT count(*) FROM track t WHERE t.book_id = b.id) AS tracks, \
    (SELECT count(DISTINCT t.disc) FROM track t WHERE t.book_id = b.id) AS discs, \
    (SELECT coalesce(sum(t.duration_ms), 0) FROM track t WHERE t.book_id = b.id) AS duration_ms, b.owner_id, o.name AS owner, b.discs_total \
    FROM book b JOIN author a ON a.id = b.author_id LEFT JOIN genre g ON g.id = b.genre_id LEFT JOIN profile o ON o.id = b.owner_id";

/// The columns of one line of the user list; the callers add the ordering or the filter.
const USER_ROWS: &str = "SELECT p.id, p.name, p.first_name, p.last_name, p.birth_date, p.role, p.max_age, p.max_devices, \
    (SELECT count(*) FROM device d WHERE d.user_id = p.id AND d.revoked_at IS NULL) AS devices \
    FROM profile p";

/// The one place that decides which books a user sees (spec section 3b):
/// admins see everything (unless a book is closed to them: a couple that are both administrators can keep their own things apart); others see released books within their age limit
/// (users without a limit see all released books), plus exceptions given to the
/// user or to the user's role; a book that is closed to the user (`book_block`) is not seen by them, whatever else says. Needs the aliases `b` (book) and `p` (profile).
pub(crate) const VISIBLE: &str = "(NOT EXISTS (SELECT 1 FROM book_block k WHERE k.book_id = b.id AND k.profile_id = p.id) \
    AND (p.role = 'admin' \
         OR (b.released = 1 AND (p.max_age IS NULL OR b.min_age <= p.max_age)) \
         OR EXISTS (SELECT 1 FROM book_release r WHERE r.book_id = b.id \
                    AND (r.profile_id = p.id OR r.role = p.role))))";

impl Store {
    pub async fn open(path: &Path) -> Result<Store> {
        let db = Db::open(path).await?;
        let queue = WriteQueue::spawn(db.writer.clone(), COALESCE_WINDOW);
        let store = Store { db, queue, access_cache: std::sync::Arc::default() };
        store.ensure_server_identity().await?;
        let open = store.unconfirmed_defaults().await?;
        if open > 0 {
            tracing::warn!(
                count = open,
                "role policy and settings still hold proposals that the owner has not confirmed"
            );
        }
        Ok(store)
    }

    // Server identity and configuration

    async fn ensure_server_identity(&self) -> Result<()> {
        let values: [Value; 2] = [uuid::Uuid::new_v4().to_string().into(), now_ms().into()];
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO server_identity (id, server_id, created_at) VALUES (1, ?, ?) \
                 ON CONFLICT (id) DO NOTHING",
                values,
            ))
            .await?;
        Ok(())
    }

    /// The id of this server. Created at the first start and never changed.
    pub async fn server_id(&self) -> Result<String> {
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT server_id FROM server_identity WHERE id = 1",
            ))
            .await?
            .ok_or_else(|| Error::WriteFailed("server identity missing".into()))?;
        Ok(row.try_get_by_index(0)?)
    }

    pub async fn role_policies(&self) -> Result<Vec<role_policy::Model>> {
        Ok(role_policy::Entity::find()
            .order_by_asc(role_policy::Column::Role)
            .all(&self.db.reader)
            .await?)
    }

    pub async fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(setting::Entity::find_by_id(key.to_owned()).one(&self.db.reader).await?.map(|s| s.value))
    }

    /// How many role policies and settings are proposals that nobody confirmed.
    pub async fn unconfirmed_defaults(&self) -> Result<i64> {
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT (SELECT count(*) FROM role_policy WHERE confirmed = 0) \
                      + (SELECT count(*) FROM setting WHERE confirmed = 0)",
            ))
            .await?
            .ok_or_else(|| Error::WriteFailed("count returned no row".into()))?;
        Ok(row.try_get_by_index(0)?)
    }

    // Administration interface

    pub async fn overview(&self) -> Result<Overview> {
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT (SELECT count(*) FROM profile), (SELECT count(*) FROM book WHERE released = 0)",
            ))
            .await?
            .ok_or_else(|| Error::WriteFailed("count returned no row".into()))?;
        Ok(Overview {
            server_id: self.server_id().await?,
            users: row.try_get_by_index(0)?,
            unreleased_books: row.try_get_by_index(1)?,
            unconfirmed_defaults: self.unconfirmed_defaults().await?,
            import_enabled: self.setting("import_enabled").await?.as_deref() == Some("1"),
        })
    }

    pub async fn user_rows(&self) -> Result<Vec<UserRow>> {
        let stmt = Statement::from_string(DbBackend::Sqlite, format!("{USER_ROWS} ORDER BY p.name"));
        Ok(UserRow::find_by_statement(stmt).all(&self.db.reader).await?)
    }

    /// One line of the user list, for answering a change to a single user.
    pub async fn user_row(&self, id: i64) -> Result<Option<UserRow>> {
        let values: [Value; 1] = [id.into()];
        let stmt = Statement::from_sql_and_values(DbBackend::Sqlite, format!("{USER_ROWS} WHERE p.id = ?"), values);
        Ok(UserRow::find_by_statement(stmt).one(&self.db.reader).await?)
    }

    pub async fn admin_book_rows(&self) -> Result<Vec<AdminBookRow>> {
        let stmt = Statement::from_string(DbBackend::Sqlite, format!("{ADMIN_BOOKS} ORDER BY a.name, b.title"));
        Ok(AdminBookRow::find_by_statement(stmt).all(&self.db.reader).await?)
    }

    pub async fn admin_book_row(&self, id: i64) -> Result<Option<AdminBookRow>> {
        let values: [Value; 1] = [id.into()];
        let stmt = Statement::from_sql_and_values(DbBackend::Sqlite, format!("{ADMIN_BOOKS} WHERE b.id = ?"), values);
        Ok(AdminBookRow::find_by_statement(stmt).one(&self.db.reader).await?)
    }

    pub async fn setting_rows(&self) -> Result<Vec<setting::Model>> {
        Ok(setting::Entity::find().order_by_asc(setting::Column::Key).all(&self.db.reader).await?)
    }

    /// Saves values of settings and marks them as decided. A key that does not exist is
    /// refused; nothing is written then.
    pub async fn save_settings(&self, values: &[(String, String)]) -> Result<()> {
        for (key, _) in values {
            if self.setting(key).await?.is_none() {
                return Err(Error::WriteFailed(format!("no such setting: {key}")));
            }
        }
        for (key, value) in values {
            self.db
                .writer
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE setting SET value = ?, confirmed = 1 WHERE key = ?",
                    [Value::from(value.clone()), Value::from(key.clone())],
                ))
                .await?;
        }
        Ok(())
    }

    /// Saves the device limit and the session length of roles and marks them as decided.
    /// `(role, max_devices, session_ttl_s)`; a role that does not exist is refused.
    pub async fn save_role_policies(&self, policies: &[(String, i64, i64)]) -> Result<()> {
        let known: Vec<String> = self.role_policies().await?.into_iter().map(|p| p.role).collect();
        if let Some((role, ..)) = policies.iter().find(|(role, ..)| !known.contains(role)) {
            return Err(Error::WriteFailed(format!("no such role: {role}")));
        }
        for (role, max_devices, session_ttl_s) in policies {
            self.db
                .writer
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE role_policy SET max_devices = ?, session_ttl_s = ?, confirmed = 1 WHERE role = ?",
                    [Value::from(*max_devices), Value::from(*session_ttl_s), Value::from(role.clone())],
                ))
                .await?;
        }
        Ok(())
    }

    /// One user with all columns, for pages that need more than the list shows.
    pub async fn profile_by_id(&self, id: i64) -> Result<Option<profile::Model>> {
        Ok(profile::Entity::find_by_id(id).one(&self.db.reader).await?)
    }

    // Users

    /// Creates a user. Fails with `ProfileLimitReached` at the 151st and with
    /// `ChildRequiresAgeAndSchedule` for a child without age limit or planner.
    pub async fn create_user(&self, new: NewProfile) -> Result<i64> {
        let model = profile::ActiveModel {
            name: Set(new.name),
            is_admin: Set(new.role == Role::Admin),
            role: Set(new.role.as_str().to_owned()),
            max_age: Set(new.max_age),
            schedule: Set(new.schedule.map(|s| s.as_bytes().to_vec())),
            created_at: Set(now_ms()),
            ..Default::default()
        };
        Ok(model.insert(&self.db.writer).await.map_err(Error::from_db)?.id)
    }

    /// Shortcut for an admin or a member.
    pub async fn create_profile(&self, name: &str, is_admin: bool) -> Result<i64> {
        let role = if is_admin { Role::Admin } else { Role::Member };
        self.create_user(NewProfile::new(name, role)).await
    }

    pub async fn list_profiles(&self) -> Result<Vec<profile::Model>> {
        Ok(profile::Entity::find()
            .order_by_asc(profile::Column::Name)
            .all(&self.db.reader)
            .await?)
    }

    /// Fails with `LastAdmin` for the last admin.
    pub async fn delete_profile(&self, id: i64) -> Result<()> {
        profile::Entity::delete_by_id(id).exec(&self.db.writer).await.map_err(Error::from_db)?;
        Ok(())
    }

    pub async fn set_profile_role(&self, id: i64, role: Role) -> Result<()> {
        profile::ActiveModel {
            id: Unchanged(id),
            role: Set(role.as_str().to_owned()),
            is_admin: Set(role == Role::Admin),
            ..Default::default()
        }
        .update(&self.db.writer)
        .await
        .map_err(Error::from_db)?;
        Ok(())
    }

    /// Changes the role and, if given, the age limit and the planner in a single
    /// statement. Turning someone into a child therefore sees all three at once.
    pub async fn change_role(
        &self,
        id: i64,
        role: Role,
        max_age: Option<i64>,
        schedule: Option<&Schedule>,
    ) -> Result<()> {
        let mut model = profile::ActiveModel {
            id: Unchanged(id),
            role: Set(role.as_str().to_owned()),
            is_admin: Set(role == Role::Admin),
            ..Default::default()
        };
        if let Some(age) = max_age {
            model.max_age = Set(Some(age));
        }
        if let Some(schedule) = schedule {
            model.schedule = Set(Some(schedule.as_bytes().to_vec()));
        }
        model.update(&self.db.writer).await.map_err(Error::from_db)?;
        Ok(())
    }

    pub async fn set_profile_max_age(&self, id: i64, max_age: Option<i64>) -> Result<()> {
        profile::ActiveModel { id: Unchanged(id), max_age: Set(max_age), ..Default::default() }
            .update(&self.db.writer)
            .await
            .map_err(Error::from_db)?;
        Ok(())
    }

    pub async fn set_profile_schedule(&self, id: i64, schedule: Option<&Schedule>) -> Result<()> {
        profile::ActiveModel {
            id: Unchanged(id),
            schedule: Set(schedule.map(|s| s.as_bytes().to_vec())),
            ..Default::default()
        }
        .update(&self.db.writer)
        .await
        .map_err(Error::from_db)?;
        Ok(())
    }

    /// The user's own device limit; `None` means the default of the role.
    pub async fn set_profile_max_devices(&self, id: i64, max_devices: Option<i64>) -> Result<()> {
        profile::ActiveModel { id: Unchanged(id), max_devices: Set(max_devices), ..Default::default() }
            .update(&self.db.writer)
            .await
            .map_err(Error::from_db)?;
        Ok(())
    }

    // Books: release and exceptions

    /// Releases a book with an age rating (the release age, `min_age`).
    pub async fn release_book(&self, book_id: i64, min_age: i64) -> Result<()> {
        book::ActiveModel {
            id: Unchanged(book_id),
            released: Set(true),
            min_age: Set(Some(min_age)),
            ..Default::default()
        }
        .update(&self.db.writer)
        .await
        .map_err(Error::from_db)?;
        Ok(())
    }

    /// Withdraws the release. Listening progress is kept.
    pub async fn unrelease_book(&self, book_id: i64) -> Result<()> {
        book::ActiveModel { id: Unchanged(book_id), released: Set(false), ..Default::default() }
            .update(&self.db.writer)
            .await
            .map_err(Error::from_db)?;
        Ok(())
    }

    /// Marks content as outside the time quota (learning content).
    pub async fn set_book_quota_exempt(&self, book_id: i64, exempt: bool) -> Result<()> {
        book::ActiveModel { id: Unchanged(book_id), quota_exempt: Set(exempt), ..Default::default() }
            .update(&self.db.writer)
            .await
            .map_err(Error::from_db)?;
        Ok(())
    }

    /// The medium's own age recommendation (information only).
    /// The blurb of a book (the text of the back cover). Empty text removes it. Plain text; the pages escape it.
    pub async fn set_book_description(&self, id: i64, text: &str) -> Result<()> {
        let text = text.trim();
        if text.chars().count() > MAX_DESCRIPTION_CHARS {
            return Err(Error::DescriptionTooLong);
        }
        let value: Value = if text.is_empty() { Value::from(None::<String>) } else { Value::from(text.to_owned()) };
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE book SET description = ? WHERE id = ?", [value, Value::from(id)]))
            .await?;
        Ok(())
    }

    pub async fn set_book_recommended_age(&self, book_id: i64, age: Option<i64>) -> Result<()> {
        book::ActiveModel { id: Unchanged(book_id), recommended_age: Set(age), ..Default::default() }
            .update(&self.db.writer)
            .await
            .map_err(Error::from_db)?;
        Ok(())
    }

    /// Gives a book an owner (a profile) or none. `false` if the book or the profile does not exist.
    pub async fn set_book_owner(&self, book_id: i64, owner: Option<i64>) -> Result<bool> {
        if let Some(p) = owner {
            if self.db.reader.query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT 1 FROM profile WHERE id = ?", [p.into()])).await?.is_none() {
                return Ok(false);
            }
        }
        let res = self.db.writer.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE book SET owner_id = ? WHERE id = ?", [owner.into(), book_id.into()])).await?;
        Ok(res.rows_affected() > 0)
    }

    /// Releases a book to one user whatever its rating.
    pub async fn grant_release_to_user(&self, book_id: i64, profile_id: i64, by: Option<i64>) -> Result<()> {
        book_release::ActiveModel {
            book_id: Set(book_id),
            profile_id: Set(Some(profile_id)),
            created_by: Set(by),
            created_at: Set(now_ms()),
            ..Default::default()
        }
        .insert(&self.db.writer)
        .await
        .map_err(Error::from_db)?;
        Ok(())
    }

    /// Releases a book to a role whatever its rating. Admins need no release.
    pub async fn grant_release_to_role(&self, book_id: i64, role: Role, by: Option<i64>) -> Result<()> {
        book_release::ActiveModel {
            book_id: Set(book_id),
            role: Set(Some(role.as_str().to_owned())),
            created_by: Set(by),
            created_at: Set(now_ms()),
            ..Default::default()
        }
        .insert(&self.db.writer)
        .await
        .map_err(Error::from_db)?;
        Ok(())
    }

    // Devices

    /// Registers a device for a user. Fails with `DeviceLimitReached` when the
    /// user already owns as many active devices as allowed.
    pub async fn add_device(&self, new: NewDevice) -> Result<i64> {
        let model = device::ActiveModel {
            user_id: Set(new.user_id),
            name: Set(new.name),
            wg_pubkey: Set(new.wg_pubkey),
            tunnel_ip: Set(new.tunnel_ip),
            shared: Set(new.shared),
            created_by: Set(new.created_by),
            created_at: Set(now_ms()),
            ..Default::default()
        };
        Ok(model.insert(&self.db.writer).await.map_err(Error::from_db)?.id)
    }

    /// A revoked device frees its place under the limit.
    pub async fn revoke_device(&self, id: i64) -> Result<()> {
        device::ActiveModel { id: Unchanged(id), revoked_at: Set(Some(now_ms())), ..Default::default() }
            .update(&self.db.writer)
            .await
            .map_err(Error::from_db)?;
        // The device's sessions end with it.
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "DELETE FROM session WHERE device_id = ?",
                [Value::from(id)],
            ))
            .await?;
        Ok(())
    }

    pub async fn devices_of(&self, user_id: i64) -> Result<Vec<device::Model>> {
        Ok(device::Entity::find()
            .filter(device::Column::UserId.eq(user_id))
            .order_by_asc(device::Column::Id)
            .all(&self.db.reader)
            .await?)
    }

    // Progress

    /// Queues a progress update. Returns as soon as it is queued; it is written
    /// within the coalescing window.
    pub async fn save_progress(&self, update: ProgressUpdate) -> Result<()> {
        self.queue.save(update).await
    }

    /// Waits until all queued progress updates are written.
    pub async fn flush(&self) -> Result<()> {
        self.queue.flush().await
    }

    pub fn writer_stats(&self) -> &WriterStats {
        self.queue.stats()
    }

    pub async fn progress_of(&self, profile_id: i64, book_id: i64) -> Result<Option<progress::Model>> {
        Ok(progress::Entity::find_by_id((profile_id, book_id)).one(&self.db.reader).await?)
    }

    // Library queries

    pub async fn find_book(&self, folder: &str) -> Result<Option<book::Model>> {
        Ok(book::Entity::find()
            .filter(book::Column::Folder.eq(folder))
            .one(&self.db.reader)
            .await?)
    }

    /// A short fingerprint of everything an app shows of the library: the books, whether and from
    /// which age they are released, their blurbs, the tracks and the exceptions. It changes when
    /// anything an app would show changes, and an app that sees the same value need not reload.
    pub async fn library_version(&self) -> Result<String> {
        use sha2::{Digest, Sha256};
        const QUERIES: [&str; 8] = [
            "SELECT coalesce(group_concat(id || ':' || title || ':' || author_id || ':' || released || ':' || coalesce(min_age, '') || ':' || \
             quota_exempt || ':' || coalesce(description, '') || ':' || coalesce(genre_id, ''), '|'), '') FROM (SELECT * FROM book ORDER BY id)",
            "SELECT coalesce(group_concat(id || ':' || name || ':' || position || ':' || visible, '|'), '') FROM (SELECT * FROM genre ORDER BY id)",
            "SELECT coalesce(group_concat(id || ':' || book_id || ':' || disc || ':' || number || ':' || title || ':' || mtime || ':' || size_bytes, '|'), '') \
             FROM (SELECT * FROM track ORDER BY id)",
            "SELECT coalesce(group_concat(book_id || ':' || profile_id, '|'), '') FROM (SELECT * FROM book_release ORDER BY book_id, profile_id)",
            "SELECT coalesce(group_concat(book_id || ':' || name, '|'), '') FROM (SELECT bl.book_id, l.name FROM book_label bl JOIN label l ON l.id = bl.label_id ORDER BY bl.book_id, l.name)",
            "SELECT coalesce(group_concat(track_id || ':' || name, '|'), '') FROM (SELECT tl.track_id, l.name FROM track_label tl JOIN label l ON l.id = tl.label_id ORDER BY tl.track_id, l.name)",
            "SELECT coalesce(group_concat(feed_id || ':' || name, '|'), '') FROM (SELECT pl.feed_id, l.name FROM podcast_label pl JOIN label l ON l.id = pl.label_id ORDER BY pl.feed_id, l.name)",
            "SELECT coalesce(group_concat(station_id || ':' || name, '|'), '') FROM (SELECT sl.station_id, l.name FROM station_label sl JOIN label l ON l.id = sl.label_id ORDER BY sl.station_id, l.name)",
        ];
        let mut hasher = Sha256::new();
        for q in QUERIES {
            let row = self.db.reader.query_one_raw(Statement::from_string(DbBackend::Sqlite, q)).await?;
            let text: String = row.map(|r| r.try_get_by_index(0)).transpose()?.unwrap_or_default();
            hasher.update(text.as_bytes());
            hasher.update([0u8]);
        }
        Ok(hasher.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect())
    }

    /// Removes a book from the library with its tracks, progress and release exceptions (they
    /// go with it), and the authors that have no book left. The files are not touched here.
    /// `false` if there is no such book.
    pub async fn delete_book(&self, id: i64) -> Result<bool> {
        let res = self
            .db
            .writer
            .execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "DELETE FROM book WHERE id = ?", [Value::from(id)]))
            .await?;
        self.db
            .writer
            .execute_raw(Statement::from_string(DbBackend::Sqlite, "DELETE FROM author WHERE id NOT IN (SELECT author_id FROM book)"))
            .await?;
        // A label that no book carries any more goes with the last one.
        self.drop_unused_labels().await?;
        Ok(res.rows_affected() > 0)
    }

    /// Ends every session of a user (web and devices stay paired, only the sign-ins end).
    pub async fn end_sessions_of(&self, user_id: i64) -> Result<()> {
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "DELETE FROM session WHERE user_id = ?", [Value::from(user_id)]))
            .await?;
        Ok(())
    }

    // Upload queue

    pub async fn create_upload_job(&self, name: &str, user_id: i64, kind: &str) -> Result<i64> {
        let row = self
            .db
            .writer
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO upload_job (name, created_by, created_at, kind) VALUES (?, ?, ?, ?) RETURNING id",
                [Value::from(name.to_owned()), Value::from(user_id), Value::from(now_ms()), Value::from(kind.to_owned())],
            ))
            .await?
            .ok_or_else(|| Error::WriteFailed("upload insert returned no row".into()))?;
        Ok(row.try_get_by_index(0)?)
    }

    /// The name of a job that is only known once its files have arrived (dropped files and folders).
    pub async fn set_upload_name(&self, id: i64, name: &str) -> Result<()> {
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE upload_job SET name = ? WHERE id = ?", [Value::from(name.to_owned()), Value::from(id)]))
            .await?;
        Ok(())
    }

    pub async fn set_upload_size(&self, id: i64, size: i64) -> Result<()> {
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE upload_job SET size_bytes = ? WHERE id = ?",
                [Value::from(size), Value::from(id)],
            ))
            .await?;
        Ok(())
    }

    /// Moves a job to a state; `done` and `failed` also stamp the time.
    pub async fn set_upload_state(&self, id: i64, state: &str, message: &str) -> Result<()> {
        let finished = matches!(state, "done" | "failed").then(now_ms);
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE upload_job SET state = ?, message = ?, finished_at = ? WHERE id = ?",
                [Value::from(state.to_owned()), Value::from(message.to_owned()), Value::from(finished), Value::from(id)],
            ))
            .await?;
        Ok(())
    }

    /// The newest jobs first, at most `limit`.
    pub async fn upload_jobs(&self, limit: u64) -> Result<Vec<UploadJob>> {
        let stmt = Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, name, kind, size_bytes, state, message, created_at FROM upload_job ORDER BY id DESC LIMIT ?",
            [Value::from(limit as i64)],
        );
        Ok(UploadJob::find_by_statement(stmt).all(&self.db.reader).await?)
    }

    pub async fn upload_kind(&self, id: i64) -> Result<String> {
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT kind FROM upload_job WHERE id = ?", [Value::from(id)]))
            .await?;
        Ok(row.map(|r| r.try_get_by_index::<String>(0)).transpose()?.unwrap_or_else(|| "audiobook".to_owned()))
    }

    /// After a restart: jobs that were being worked on start again; returns all that wait, oldest first.
    pub async fn requeue_uploads(&self) -> Result<Vec<i64>> {
        self.db
            .writer
            .execute_raw(Statement::from_string(DbBackend::Sqlite, "UPDATE upload_job SET state = 'queued' WHERE state = 'working'"))
            .await?;
        let rows = self
            .db
            .writer
            .query_all_raw(Statement::from_string(DbBackend::Sqlite, "SELECT id FROM upload_job WHERE state = 'queued' ORDER BY id"))
            .await?;
        rows.iter().map(|r| Ok(r.try_get_by_index(0)?)).collect()
    }

    // MCP access

    /// Makes a token and returns it; this is the only time it can be seen.
    pub async fn create_mcp_token(&self, name: &str, can_write: bool, user_id: i64) -> Result<String> {
        let token = crate::auth::new_token();
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO mcp_token (name, token_hash, can_write, created_by, created_at) VALUES (?, ?, ?, ?, ?)",
                [
                    Value::from(name.to_owned()),
                    Value::from(crate::auth::sha256_hex(&token)),
                    Value::from(can_write),
                    Value::from(user_id),
                    Value::from(now_ms()),
                ],
            ))
            .await?;
        Ok(token)
    }

    pub async fn mcp_tokens(&self) -> Result<Vec<McpToken>> {
        let stmt = Statement::from_string(DbBackend::Sqlite, "SELECT id, name, can_write, created_at, last_used_at FROM mcp_token ORDER BY id");
        Ok(McpToken::find_by_statement(stmt).all(&self.db.reader).await?)
    }

    pub async fn revoke_mcp_token(&self, id: i64) -> Result<()> {
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "DELETE FROM mcp_token WHERE id = ?", [Value::from(id)]))
            .await?;
        Ok(())
    }

    /// The help text the project owner set for remote LLMs, if there is one.
    pub async fn mcp_prompt(&self) -> Result<Option<String>> {
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_string(DbBackend::Sqlite, "SELECT text FROM mcp_prompt WHERE id = 1"))
            .await?;
        Ok(row.map(|r| r.try_get_by_index::<String>(0)).transpose()?)
    }

    /// Sets the help text; an empty text brings the built-in one back.
    pub async fn set_mcp_prompt(&self, text: &str) -> Result<()> {
        let text = text.trim();
        let stmt = if text.is_empty() {
            Statement::from_string(DbBackend::Sqlite, "DELETE FROM mcp_prompt WHERE id = 1")
        } else {
            Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO mcp_prompt (id, text) VALUES (1, ?) ON CONFLICT(id) DO UPDATE SET text = excluded.text",
                [Value::from(text.to_owned())],
            )
        };
        self.db.writer.execute_raw(stmt).await?;
        Ok(())
    }

    /// The token behind a presented secret, if there is one; stamps the time of use.
    pub async fn mcp_token_for(&self, secret: &str) -> Result<Option<McpToken>> {
        let hash = crate::auth::sha256_hex(secret);
        let stmt = Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, name, can_write, created_at, last_used_at FROM mcp_token WHERE token_hash = ?",
            [Value::from(hash)],
        );
        let Some(token) = McpToken::find_by_statement(stmt).one(&self.db.reader).await? else { return Ok(None) };
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE mcp_token SET last_used_at = ? WHERE id = ?",
                [Value::from(now_ms()), Value::from(token.id)],
            ))
            .await?;
        Ok(Some(token))
    }

    pub async fn log_tag_change(&self, token_name: &str, book_id: i64, file: &str, field: &str, old: &str, new: &str) -> Result<()> {
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO tag_change (at, token_name, book_id, file, field, old_value, new_value) VALUES (?, ?, ?, ?, ?, ?, ?)",
                [
                    Value::from(now_ms()),
                    Value::from(token_name.to_owned()),
                    Value::from(book_id),
                    Value::from(file.to_owned()),
                    Value::from(field.to_owned()),
                    Value::from(old.to_owned()),
                    Value::from(new.to_owned()),
                ],
            ))
            .await?;
        Ok(())
    }

    /// The newest changes first, of one book or of all (`book_id` 0).
    pub async fn tag_changes(&self, book_id: i64, limit: u64) -> Result<Vec<TagChange>> {
        let stmt = Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, at, token_name, book_id, file, field, old_value, new_value FROM tag_change WHERE (? = 0 OR book_id = ?) ORDER BY id DESC LIMIT ?",
            [Value::from(book_id), Value::from(book_id), Value::from(limit as i64)],
        );
        Ok(TagChange::find_by_statement(stmt).all(&self.db.reader).await?)
    }

    /// The folder of a book below its library root, e.g. `Follett/Kinder der Freiheit`.
    pub async fn book_folder(&self, id: i64) -> Result<Option<String>> {
        Ok(book::Entity::find_by_id(id).one(&self.db.reader).await?.map(|b| b.folder))
    }

    /// Books with missing tracks: for every book the numbers that are missing in the sequence of a CD (1 to the highest number), by CD, as a
    /// text for the list (`6, 7, 11` for a book of one CD, `CD 4: 6, 7, 11` else), and whether a number is there twice. Books without a gap are not in it.
    pub async fn track_gaps(&self) -> Result<std::collections::HashMap<i64, String>> {
        use std::collections::{BTreeMap, HashMap};
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_string(DbBackend::Sqlite, "SELECT book_id, disc, number FROM track ORDER BY book_id, disc, number"))
            .await?;
        let mut books: HashMap<i64, BTreeMap<i64, Vec<i64>>> = HashMap::new();
        for r in rows {
            let (b, d, n): (i64, i64, i64) = (r.try_get_by_index(0)?, r.try_get_by_index(1)?, r.try_get_by_index(2)?);
            books.entry(b).or_default().entry(d).or_default().push(n);
        }
        let mut out = HashMap::new();
        for (book, discs) in books.iter() {
            let book = *book;
            let many = discs.len() > 1 || discs.keys().any(|d| *d > 1);
            let mut parts = Vec::new();
            for (disc, numbers) in discs {
                let max = numbers.iter().copied().max().unwrap_or(0).min(999);
                let missing: Vec<i64> = (1..=max).filter(|n| !numbers.contains(n)).collect();
                let double = numbers.windows(2).any(|w| w[0] == w[1]);
                if missing.is_empty() && !double {
                    continue;
                }
                let mut text: Vec<String> = missing.iter().take(12).map(i64::to_string).collect();
                if missing.len() > 12 {
                    text.push("…".to_owned());
                }
                let mut line = text.join(", ");
                if double {
                    line = if line.is_empty() { "doubles".to_owned() } else { format!("{line}; doubles") };
                }
                parts.push(if many { format!("CD {disc}: {line}") } else { line });
            }
            if !parts.is_empty() {
                out.insert(book, parts.join(" | "));
            }
        }
        // CDs that are missing: the work says it has more of them than the books holds (`CD 4: -`).
        let totals = self.db.reader.query_all_raw(Statement::from_string(DbBackend::Sqlite, "SELECT id, discs_total FROM book WHERE discs_total IS NOT NULL")).await?;
        for r in totals {
            let (book, total): (i64, i64) = (r.try_get_by_index(0)?, r.try_get_by_index(1)?);
            let have: Vec<i64> = books.get(&book).map(|d| d.keys().copied().collect()).unwrap_or_default();
            let lacking: Vec<String> = (1..=total).filter(|d| !have.contains(d)).map(|d| format!("CD {d}: -")).collect();
            if !lacking.is_empty() {
                let entry = out.entry(book).or_default();
                if !entry.is_empty() {
                    entry.push_str(" | ");
                }
                entry.push_str(&lacking.join(" | "));
            }
        }
        Ok(out)
    }

    /// Says how many CDs a work has (`None` takes it away). `false` if there is no such book.
    pub async fn set_book_discs_total(&self, id: i64, total: Option<i64>) -> Result<bool> {
        let res = self.db.writer.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE book SET discs_total = ? WHERE id = ?", [total.into(), id.into()])).await?;
        Ok(res.rows_affected() > 0)
    }

    /// Tracks of a book in playing order.
    pub async fn tracks_of(&self, book_id: i64) -> Result<Vec<track::Model>> {
        Ok(track::Entity::find()
            .filter(track::Column::BookId.eq(book_id))
            .order_by_asc(track::Column::Disc)
            .order_by_asc(track::Column::Number)
            .all(&self.db.reader)
            .await?)
    }

    /// Unfinished books of a user, most recently listened first. Only books the
    /// user may see.
    pub async fn continue_listening(&self, profile_id: i64, limit: i64) -> Result<Vec<ContinueEntry>> {
        let sql = format!(
            "SELECT b.id AS book_id, b.title, a.name AS author, g.track_id, g.position_ms, g.updated_at \
             FROM progress g \
             JOIN book b ON b.id = g.book_id \
             JOIN author a ON a.id = b.author_id \
             JOIN profile p ON p.id = g.profile_id \
             WHERE g.profile_id = ? AND g.finished = 0 AND {VISIBLE} \
             ORDER BY g.updated_at DESC LIMIT ?"
        );
        let values: [Value; 2] = [profile_id.into(), limit.into()];
        let stmt = Statement::from_sql_and_values(DbBackend::Sqlite, sql, values);
        Ok(ContinueEntry::find_by_statement(stmt).all(&self.db.reader).await?)
    }

    /// All books the user may see, ordered by author and title.
    pub async fn visible_books(&self, profile_id: i64) -> Result<Vec<BookRow>> {
        let sql = format!(
            "SELECT b.id, b.folder, b.kind, b.title, a.name AS author, b.narrator, CASE WHEN g.visible = 1 THEN g.name END AS genre, \
                    b.min_age, b.recommended_age, b.released, \
                    count(t.id) AS tracks, count(DISTINCT t.disc) AS discs, \
                    coalesce(sum(t.duration_ms), 0) AS duration_ms, o.name AS owner \
             FROM book b \
             JOIN author a ON a.id = b.author_id \
             LEFT JOIN profile o ON o.id = b.owner_id \
             LEFT JOIN genre g ON g.id = b.genre_id \
             LEFT JOIN track t ON t.book_id = b.id \
             JOIN profile p ON p.id = ? \
             WHERE {VISIBLE} \
             GROUP BY b.id ORDER BY a.name, b.title"
        );
        let values: [Value; 1] = [profile_id.into()];
        let stmt = Statement::from_sql_and_values(DbBackend::Sqlite, sql, values);
        Ok(BookRow::find_by_statement(stmt).all(&self.db.reader).await?)
    }

    /// May this user listen to this book now? `weekday` is 0 for Monday and
    /// `minute` the minute of the day, both in the server's time zone.
    ///
    /// Visibility comes from the same rule as the lists. The planner applies if
    /// the user has one, unless the book is outside the quota.
    pub async fn access(&self, profile_id: i64, book_id: i64, weekday: u8, minute: u16) -> Result<Access> {
        let sql = format!(
            "SELECT b.quota_exempt AS quota_exempt, p.schedule AS schedule \
             FROM book b JOIN profile p ON p.id = ? \
             WHERE b.id = ? AND {VISIBLE}"
        );
        let values: [Value; 2] = [profile_id.into(), book_id.into()];
        let stmt = Statement::from_sql_and_values(DbBackend::Sqlite, sql, values);
        let Some(row) = AccessRow::find_by_statement(stmt).one(&self.db.reader).await? else {
            return Ok(Access::NotVisible);
        };
        if row.quota_exempt {
            return Ok(Access::Allowed);
        }
        match row.schedule.as_deref().and_then(Schedule::from_bytes) {
            Some(schedule) if !schedule.is_allowed(weekday, minute) => {
                Ok(Access::OutsideSchedule { next_start: schedule.next_start(weekday, minute) })
            }
            _ => Ok(Access::Allowed),
        }
    }
}
