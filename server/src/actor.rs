// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Who is acting (project owner, 2026-10-28: every table says who changed a row last). The triggers of schema 0028 read
//! the one row of the table `actor`; before each statement of the write connection that row is set to the actor of the
//! request that is running (see `db.rs`, `before_acquire`). A request is tagged by `scope`, and the code that finds out
//! who the caller is (session, device, MCP token) calls `set`. Work that nobody asked for (the importer, the refresher,
//! the queues) runs outside any request and is `system`.

use std::sync::{Arc, Mutex, PoisonError};

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;

/// The actor of work that nobody asked for.
pub const SYSTEM: &str = "system";
/// The actor of a request that has not said who it is (sign-in, pairing, the login page).
pub const ANONYMOUS: &str = "anonymous";

tokio::task_local! {
    static ACTOR: Arc<Mutex<String>>;
}

/// A signed-in profile (a session of the pages or a device of an app).
pub fn profile(id: i64) -> String {
    format!("profile:{id}")
}

/// An MCP token (by its name).
pub fn mcp(name: &str) -> String {
    format!("mcp:{name}")
}

/// Says who is acting in the request that is running. Outside a request nothing happens.
pub fn set(label: impl Into<String>) {
    let _ = ACTOR.try_with(|cell| *cell.lock().unwrap_or_else(PoisonError::into_inner) = label.into());
}

/// The actor of the running request, or `system` outside one.
pub fn current() -> String {
    ACTOR.try_with(|cell| cell.lock().unwrap_or_else(PoisonError::into_inner).clone()).unwrap_or_else(|_| SYSTEM.to_owned())
}

/// Middleware: gives the request a place to say who it is. The routers put it in front of everything.
pub async fn scope(req: Request, next: Next) -> Response {
    ACTOR.scope(Arc::new(Mutex::new(ANONYMOUS.to_owned())), next.run(req)).await
}

/// Runs work as somebody (a queue that works for the person who asked), outside a request.
pub async fn run_as<F: std::future::Future>(label: String, work: F) -> F::Output {
    ACTOR.scope(Arc::new(Mutex::new(label)), work).await
}

/// When and by whom a row was made and last changed (the audit fields of schema 0028).
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct Audit {
    pub created_at: i64,
    pub created_by: Option<String>,
    pub updated_at: i64,
    pub updated_by: Option<String>,
}

impl crate::store::Store {
    /// The audit fields of the row `id` of a table that has an integer `id` (`None` if there is no such row or table). The
    /// table name is checked against the schema, it is never taken into the statement unchecked.
    pub async fn audit_of(&self, table: &str, id: i64) -> crate::error::Result<Option<Audit>> {
        use sea_orm::{ConnectionTrait, DbBackend, Statement};
        let known = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?", [table.to_owned().into()]))
            .await?;
        if known.is_none() || !table.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
            return Ok(None);
        }
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, format!("SELECT created_at, created_by, updated_at, updated_by FROM {table} WHERE id = ?"), [id.into()]))
            .await
            .ok()
            .flatten();
        let Some(row) = row else { return Ok(None) };
        let text = |i: usize| -> crate::error::Result<Option<String>> { Ok(row.try_get_by_index::<Option<String>>(i).or_else(|_| row.try_get_by_index::<Option<i64>>(i).map(|n| n.map(|n| n.to_string())))?) };
        Ok(Some(Audit { created_at: row.try_get_by_index(0)?, created_by: text(1)?, updated_at: row.try_get_by_index(2)?, updated_by: text(3)? }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    async fn open() -> (tempfile::TempDir, crate::db::Db) {
        let dir = tempfile::TempDir::new().unwrap();
        let db = crate::db::Db::open(&dir.path().join("t.db")).await.unwrap();
        (dir, db)
    }

    async fn first(db: &crate::db::Db, sql: &str) -> sea_orm::QueryResult {
        db.reader.query_one_raw(Statement::from_string(DbBackend::Sqlite, sql)).await.unwrap().unwrap()
    }

    #[tokio::test]
    async fn every_table_keeps_the_audit_fields() {
        let (_d, db) = open().await;
        let tables = db.reader.query_all_raw(Statement::from_string(DbBackend::Sqlite, "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name <> 'actor'")).await.unwrap();
        assert!(tables.len() > 25, "the schema was read");
        for t in tables {
            let table: String = t.try_get_by_index(0).unwrap();
            let cols = db.reader.query_all_raw(Statement::from_string(DbBackend::Sqlite, format!("SELECT name FROM pragma_table_info('{table}')"))).await.unwrap();
            let cols: Vec<String> = cols.iter().map(|c| c.try_get_by_index(0).unwrap()).collect();
            for want in ["created_at", "created_by", "updated_at", "updated_by"] {
                assert!(cols.iter().any(|c| c == want), "{table} has no {want}: a new table needs the audit fields and triggers (see migration 0028)");
            }
            for trigger in [format!("audit_{table}_ai"), format!("audit_{table}_au")] {
                let n: i64 = db.reader.query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT count(*) FROM sqlite_master WHERE type = 'trigger' AND name = ?", [trigger.clone().into()])).await.unwrap().unwrap().try_get_by_index(0).unwrap();
                assert_eq!(n, 1, "{table} has no trigger {trigger}");
            }
        }
    }

    #[tokio::test]
    async fn the_triggers_set_the_times_and_who_acted_and_do_not_fire_themselves() {
        let (_d, db) = open().await;
        let exec = |sql: &'static str| {
            let db = db.clone();
            async move { db.writer.execute_raw(Statement::from_string(DbBackend::Sqlite, sql)).await.unwrap() }
        };
        // Work outside any request is the system.
        exec("INSERT INTO author (name) VALUES ('A')").await;
        let r = first(&db, "SELECT created_at, created_by, updated_at, updated_by FROM author WHERE name = 'A'").await;
        let (created, by, updated, upd_by): (i64, String, i64, String) = (r.try_get_by_index(0).unwrap(), r.try_get_by_index(1).unwrap(), r.try_get_by_index(2).unwrap(), r.try_get_by_index(3).unwrap());
        assert!(created > 1_700_000_000_000 && updated >= created, "times in Unix ms");
        assert_eq!((by.as_str(), upd_by.as_str()), (SYSTEM, SYSTEM));

        // A request that said who it is: the change carries it, the maker stays.
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        run_as(profile(7), async { exec("UPDATE author SET name = 'B' WHERE name = 'A'").await }).await;
        let r = first(&db, "SELECT created_by, updated_by, updated_at > created_at FROM author").await;
        assert_eq!((r.try_get_by_index::<String>(0).unwrap().as_str(), r.try_get_by_index::<String>(1).unwrap().as_str(), r.try_get_by_index::<i64>(2).unwrap()), (SYSTEM, "profile:7", 1));

        // Another actor, another row; a time that the program sets itself is kept.
        run_as(mcp("w"), async { exec("INSERT INTO author (name) VALUES ('C')").await }).await;
        run_as(mcp("w"), async { exec("UPDATE author SET name = 'C2' WHERE name = 'C'").await }).await;
        let r = first(&db, "SELECT created_by, updated_by FROM author WHERE name = 'C2'").await;
        assert_eq!((r.try_get_by_index::<String>(0).unwrap().as_str(), r.try_get_by_index::<String>(1).unwrap().as_str()), ("mcp:w", "mcp:w"));
        exec("UPDATE author SET updated_at = 42 WHERE name = 'C2'").await;
        assert_eq!(first(&db, "SELECT updated_at FROM author WHERE name = 'C2'").await.try_get_by_index::<i64>(0).unwrap(), 42);
        let n: i64 = first(&db, "PRAGMA recursive_triggers").await.try_get_by_index(0).unwrap();
        assert_eq!(n, 0, "the triggers must not fire themselves");
    }
}
