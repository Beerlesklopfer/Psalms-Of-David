// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sea_orm::sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, SqlxSqliteConnector, Statement, TransactionTrait};

use crate::error::Result;

/// The position in this list is the version number (`PRAGMA user_version`).
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_init.sql"),
    include_str!("../migrations/0002_profile_limit_150.sql"),
    include_str!("../migrations/0003_roles_devices_release.sql"),
    include_str!("../migrations/0004_sessions_for_the_web.sql"),
    include_str!("../migrations/0005_bible_anchors.sql"),
    include_str!("../migrations/0006_avatar.sql"),
    include_str!("../migrations/0007_person_names.sql"),
    include_str!("../migrations/0008_language.sql"),
    include_str!("../migrations/0009_birth_date.sql"),
    include_str!("../migrations/0010_book_description.sql"),
    include_str!("../migrations/0011_upload_queue.sql"),
    include_str!("../migrations/0012_auto_import.sql"),
    include_str!("../migrations/0013_disk_reserve.sql"),
    include_str!("../migrations/0014_music.sql"),
    include_str!("../migrations/0015_narrator.sql"),
    include_str!("../migrations/0016_upload_kind.sql"),
    include_str!("../migrations/0017_mcp.sql"),
    include_str!("../migrations/0018_mcp_prompt.sql"),
    include_str!("../migrations/0019_ignore.sql"),
    include_str!("../migrations/0020_log_level.sql"),
    include_str!("../migrations/0021_tree.sql"),
    include_str!("../migrations/0022_tls_and_names.sql"),
    include_str!("../migrations/0023_podcasts_and_radio.sql"),
    include_str!("../migrations/0024_genres.sql"),
    include_str!("../migrations/0025_placeholder_tags.sql"),
    include_str!("../migrations/0026_cd_import.sql"),
    include_str!("../migrations/0027_track_play.sql"),
    include_str!("../migrations/0028_audit_fields.sql"),
    include_str!("../migrations/0029_cd_rip.sql"),
    include_str!("../migrations/0030_cd_read_speed.sql"),
    include_str!("../migrations/0031_cd_resume.sql"),
    include_str!("../migrations/0032_cd_present.sql"),
    include_str!("../migrations/0033_cd_buffer.sql"),
    include_str!("../migrations/0034_cd_encoder_priority.sql"),
    include_str!("../migrations/0035_playlists.sql"),
    include_str!("../migrations/0036_cd_verify_shift.sql"),
    include_str!("../migrations/0037_cd_accept_unverified.sql"),
    include_str!("../migrations/0038_library_hiding.sql"),
    include_str!("../migrations/0039_book_block.sql"),
    include_str!("../migrations/0040_book_owner.sql"),
    include_str!("../migrations/0041_profile_folder.sql"),
    include_str!("../migrations/0042_list_page_size.sql"),
    include_str!("../migrations/0043_profile_folder_ebook.sql"),
    include_str!("../migrations/0044_labels.sql"),
    include_str!("../migrations/0045_book_discs_total.sql"),
    include_str!("../migrations/0046_track_label.sql"),
    include_str!("../migrations/0047_feed_station_label.sql"),
    include_str!("../migrations/0048_blacklist.sql"),
    include_str!("../migrations/0050_blacklist_word.sql"),
    include_str!("../migrations/0051_track_format.sql"),
    include_str!("../migrations/0052_transcode_settings.sql"),
    include_str!("../migrations/0053_enrollment_trail.sql"),
    include_str!("../migrations/0054_listening_sharing.sql"),
    include_str!("../migrations/0055_listening_time.sql"),
    include_str!("../migrations/0056_book_barcode.sql"),
    include_str!("../migrations/0057_pin_backoff.sql"),
    include_str!("../migrations/0058_cd_rights.sql"),
    include_str!("../migrations/0059_cd_job_states.sql"),
    include_str!("../migrations/0060_inbox.sql"),
];

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const READERS: u32 = 4;

/// One write connection and a small read pool. SQLite has a single writer; in WAL
/// mode it does not block the readers.
#[derive(Clone)]
pub struct Db {
    pub writer: DatabaseConnection,
    pub reader: DatabaseConnection,
}

impl Db {
    pub async fn open(path: &Path) -> Result<Db> {
        let write_opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(BUSY_TIMEOUT)
            .foreign_keys(true)
            // The audit triggers (schema 0028) change the row they fire on; with this on they would fire themselves.
            .pragma("recursive_triggers", "0");
        // Who acts is told to the triggers through the table `actor`: set before each statement, when it changed.
        let told = Arc::new(Mutex::new(String::new()));
        let (on_connect, on_acquire) = (told.clone(), told);
        let writer = SqlitePoolOptions::new()
            .max_connections(1)
            .min_connections(1)
            // The one writer must not be replaced behind our back, a new connection would not know who acts.
            .idle_timeout(None)
            .max_lifetime(None)
            .after_connect(move |conn, _| {
                let told = on_connect.clone();
                Box::pin(async move {
                    tell_actor(conn, &told).await;
                    Ok(())
                })
            })
            .before_acquire(move |conn, _| {
                let told = on_acquire.clone();
                Box::pin(async move {
                    tell_actor(conn, &told).await;
                    Ok(true)
                })
            })
            .connect_with(write_opts)
            .await?;
        let writer = SqlxSqliteConnector::from_sqlx_sqlite_pool(writer);
        migrate(&writer).await?;

        // Only after the migration: readers open the existing file read-only.
        let read_opts = SqliteConnectOptions::new()
            .filename(path)
            .read_only(true)
            .busy_timeout(BUSY_TIMEOUT)
            .foreign_keys(true);
        let reader = SqlitePoolOptions::new()
            .max_connections(READERS)
            .connect_with(read_opts)
            .await?;
        let reader = SqlxSqliteConnector::from_sqlx_sqlite_pool(reader);

        Ok(Db { writer, reader })
    }
}

/// Writes the actor of the running request into the table `actor`, if it is not the one that stands there already.
/// Before the table exists (the first start) this does nothing.
async fn tell_actor(conn: &mut sea_orm::sqlx::SqliteConnection, told: &Mutex<String>) {
    let label = crate::actor::current();
    if *told.lock().unwrap_or_else(std::sync::PoisonError::into_inner) == label {
        return;
    }
    if sea_orm::sqlx::query("UPDATE actor SET label = ? WHERE id = 1").bind(&label).execute(&mut *conn).await.is_ok() {
        *told.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = label;
    }
}

async fn user_version(conn: &DatabaseConnection) -> Result<usize> {
    let row = conn
        .query_one_raw(Statement::from_string(DbBackend::Sqlite, "PRAGMA user_version"))
        .await?;
    let v: i64 = row.map(|r| r.try_get_by_index(0)).transpose()?.unwrap_or(0);
    Ok(v as usize)
}

/// Applies missing migrations, each in its own transaction.
pub async fn migrate(conn: &DatabaseConnection) -> Result<()> {
    let current = user_version(conn).await?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        let txn = conn.begin().await?;
        txn.execute_unprepared(sql).await?;
        txn.execute_unprepared(&format!("PRAGMA user_version = {}", i + 1)).await?;
        txn.commit().await?;
        tracing::info!(version = i + 1, "migration applied");
    }
    Ok(())
}
