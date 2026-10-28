// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
use std::path::Path;
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
            .foreign_keys(true);
        let writer = SqlitePoolOptions::new()
            .max_connections(1)
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
