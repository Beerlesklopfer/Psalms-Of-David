// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Write queue for listening progress.
//!
//! Many small updates (channel zapping, position reports) land in one channel. A
//! single task coalesces them per (profile, book) and writes them in one
//! transaction. This keeps the write connection busy only briefly and prevents an
//! older update from overwriting a newer one.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait, Value};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressUpdate {
    pub profile_id: i64,
    pub book_id: i64,
    pub track_id: i64,
    pub position_ms: i64,
    pub finished: bool,
    /// Unix milliseconds; on a tie the update that arrived later wins.
    pub updated_at: i64,
}

#[derive(Default, Debug)]
pub struct WriterStats {
    pub received: AtomicU64,
    pub written: AtomicU64,
    pub batches: AtomicU64,
    pub dropped: AtomicU64,
}

enum Cmd {
    Progress(ProgressUpdate),
    Flush(oneshot::Sender<std::result::Result<(), String>>),
}

#[derive(Clone)]
pub struct WriteQueue {
    tx: mpsc::Sender<Cmd>,
    stats: Arc<WriterStats>,
}

const UPSERT: &str = "\
INSERT INTO progress (profile_id, book_id, track_id, position_ms, finished, updated_at) \
VALUES (?, ?, ?, ?, ?, ?) \
ON CONFLICT (profile_id, book_id) DO UPDATE SET \
  track_id = excluded.track_id, position_ms = excluded.position_ms, \
  finished = excluded.finished, updated_at = excluded.updated_at \
WHERE excluded.updated_at >= progress.updated_at";

impl WriteQueue {
    pub fn spawn(conn: DatabaseConnection, window: Duration) -> WriteQueue {
        let (tx, rx) = mpsc::channel(4096);
        let stats = Arc::new(WriterStats::default());
        tokio::spawn(run(rx, conn, window, stats.clone()));
        WriteQueue { tx, stats }
    }

    pub async fn save(&self, update: ProgressUpdate) -> Result<()> {
        self.stats.received.fetch_add(1, Ordering::Relaxed);
        self.tx.send(Cmd::Progress(update)).await.map_err(|_| Error::WriterStopped)
    }

    /// Waits until everything submitted so far has been written.
    pub async fn flush(&self) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx.send(Cmd::Flush(tx)).await.map_err(|_| Error::WriterStopped)?;
        rx.await
            .map_err(|_| Error::WriterStopped)?
            .map_err(Error::WriteFailed)
    }

    pub fn stats(&self) -> &WriterStats {
        &self.stats
    }
}

async fn run(
    mut rx: mpsc::Receiver<Cmd>,
    conn: DatabaseConnection,
    window: Duration,
    stats: Arc<WriterStats>,
) {
    let mut pending: HashMap<(i64, i64), ProgressUpdate> = HashMap::new();
    let mut waiters = Vec::new();
    let mut deadline: Option<Instant> = None;

    loop {
        let due = deadline;
        let timer = async move {
            match due {
                Some(d) => tokio::time::sleep_until(d).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            cmd = rx.recv() => match cmd {
                None => {
                    let _ = write_batch(&conn, &mut pending, &stats).await;
                    break;
                }
                Some(Cmd::Progress(u)) => {
                    deadline.get_or_insert_with(|| Instant::now() + window);
                    merge(&mut pending, u);
                }
                Some(Cmd::Flush(tx)) => {
                    waiters.push(tx);
                    deadline = Some(Instant::now());
                }
            },
            _ = timer => {
                let result = write_batch(&conn, &mut pending, &stats).await;
                deadline = None;
                for w in waiters.drain(..) {
                    let _ = w.send(result.clone());
                }
            }
        }
    }
}

/// Keeps the newest update per key; on a tie the later one wins.
fn merge(pending: &mut HashMap<(i64, i64), ProgressUpdate>, u: ProgressUpdate) {
    pending
        .entry((u.profile_id, u.book_id))
        .and_modify(|old| {
            if u.updated_at >= old.updated_at {
                *old = u;
            }
        })
        .or_insert(u);
}

fn statement(u: &ProgressUpdate) -> Statement {
    let values: [Value; 6] = [
        u.profile_id.into(),
        u.book_id.into(),
        u.track_id.into(),
        u.position_ms.into(),
        i64::from(u.finished).into(),
        u.updated_at.into(),
    ];
    Statement::from_sql_and_values(DbBackend::Sqlite, UPSERT, values)
}

/// Writes everything in one transaction. If that fails (e.g. because a profile
/// was deleted in the meantime), it falls back to row by row so that one bad
/// update does not take the others down with it.
async fn write_batch(
    conn: &DatabaseConnection,
    pending: &mut HashMap<(i64, i64), ProgressUpdate>,
    stats: &WriterStats,
) -> std::result::Result<(), String> {
    if pending.is_empty() {
        return Ok(());
    }
    let items: Vec<ProgressUpdate> = pending.drain().map(|(_, v)| v).collect();
    stats.batches.fetch_add(1, Ordering::Relaxed);

    let batch = async {
        let txn = conn.begin().await?;
        for u in &items {
            txn.execute_raw(statement(u)).await?;
        }
        txn.commit().await
    };
    match batch.await {
        Ok(()) => {
            stats.written.fetch_add(items.len() as u64, Ordering::Relaxed);
            Ok(())
        }
        Err(e) => {
            tracing::warn!(error = %e, "batch failed, writing row by row");
            for u in &items {
                match conn.execute_raw(statement(u)).await {
                    Ok(_) => {
                        stats.written.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(e) => {
                        stats.dropped.fetch_add(1, Ordering::Relaxed);
                        tracing::warn!(error = %e, ?u, "progress update dropped");
                    }
                }
            }
            Ok(())
        }
    }
}
