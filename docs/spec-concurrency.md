# Spec: concurrency and the write path

Status: implemented in `server/src/db.rs` and `server/src/writer.rs`, measured by `server/tests/zap_bench.rs`.

## Problem

SQLite has exactly one writer. With up to 150 profiles the load is still small, but it arrives in bursts: switching tracks quickly ("zapping") and position reports produce many tiny writes, and a library scan can hold the write lock for a long time. The risks are therefore not throughput but:

1. A blocking database call on an async runtime thread stalls unrelated requests.
2. A long write transaction makes position writes fail with `database is locked`.
3. Two updates for the same book arrive out of order and the older one wins.

Choosing an async ORM does not remove these. sqlx runs SQLite on a background thread as well; async only hides the blocking.

## Design

- **Two connection groups** (`Db`): one write connection (pool size 1) and a read pool (4 connections, opened read-only after the migration). In WAL mode readers are never blocked by the writer.
- **Per-connection settings**: `journal_mode=WAL`, `synchronous=NORMAL`, `foreign_keys=ON`, `busy_timeout=5s`.
- **Write queue for progress** (`WriteQueue`): handlers push `ProgressUpdate`s into a bounded channel and return immediately. One task collects them for a short window (500 ms), keeps only the newest update per `(profile, book)`, and writes the batch in one transaction.
- **Ordering**: every update carries `updated_at`. In the queue the newer one wins (a tie goes to the later arrival). In the database the upsert has `WHERE excluded.updated_at >= progress.updated_at`, so an older write can never replace a newer row, even across batches.
- **Failure isolation**: if a batch fails (for example a profile was deleted meanwhile), the writer retries row by row, drops only the bad rows and counts them in `WriterStats.dropped`.
- **Scan** (`Store::scan`): files are read on a blocking thread; the database is touched in short transactions (one for author and book, then at most 100 tracks per transaction).
- **Clients** should still send positions sparingly: every 10 to 15 seconds and on pause, track change and seek. The queue is a safety net, not a licence to send every tick.

## What was measured

`server/tests/zap_bench.rs`: up to 150 clients (`DAVID_BENCH_CLIENTS`, default 150) flip through 40 books (bursts of 3 to 8 updates, reads in between) while a background import adds books of 100 tracks. 8 s run in a debug build:

| Metric | Result |
|---|---|
| Failed operations | 0 |
| Dropped progress updates | 0 |
| Read p99 (continue listening + book list) | about 10 ms |
| Import of a 100-track book, p99 | about 3 ms |
| Stored progress equal to the latest update | 975 of 975 |

60 s run in a release build (27,339 progress updates, 2,679 background imports):

| Metric | Result |
|---|---|
| Failed operations / dropped updates | 0 / 0 |
| Read p50 / p99 / max | 13 ms / 41 ms / 60 ms |
| Import of a 100-track book p50 / p99 | 1 ms / 11 ms |
| Stored progress equal to the latest update | 1000 of 1000 |

150 clients, 30 s, release build (74,440 progress updates, 833 background imports):

| Metric | Result |
|---|---|
| Failed operations / dropped updates | 0 / 0 |
| Read p50 / p99 / max | 22 ms / 173 ms / 221 ms |
| Import of a 100-track book p50 / p99 / max | 2 ms / 115 ms / 172 ms |
| Progress batches | 59, about 1,100 rows each |
| Stored progress equal to the latest update | 6000 of 6000 |

This is much closer to the limits than the 25-client runs (read p99 41 ms), and the test's 250 ms bound for reads is nearly used up. Two causes, both visible in the numbers:

- **Read pool.** Every simulated client refreshes the book list and "continue listening" about 4 times a second, which is far more than a real app does. 150 clients make that roughly 450 list queries a second on 4 read connections, and each one aggregates over all tracks.
- **Large write batches.** With 150 profiles a 500 ms window collects about 1,100 rows, so one transaction holds the single writer for around 100 ms. That is what the import's p99 shows.

Correctness is unaffected (nothing failed or lost). If the margins matter in practice, the options are: more read connections, caching the book list, or a shorter coalescing window so batches stay small. None of these was needed or measured yet; the real client sends a position every 10 to 15 seconds.

Caveat: reads got slower than in the 8 s run (p99 10 ms to 41 ms). The cause is the data, not lock contention: the background importer keeps adding books, so by the end the database holds roughly 270,000 tracks and the book list aggregates over all of them. A real family library is far smaller. It does show that the list query scales with library size, so it should be cached or paged once libraries get large.

The test fails if any operation fails, any update is dropped, any final position is not the latest, enqueue p99 is 50 ms or more, or read p99 is 250 ms or more.

Reading the numbers honestly:

- "Enqueue" only measures putting an update into the channel. It is fast by construction and says nothing about durability. Durability is bounded by the 500 ms window plus one transaction.
- Coalescing was only about 11 % in this run because the simulated clients jump between random books. Repeated updates for the same book collapse almost completely (the test `a_burst_for_one_book_is_coalesced_into_one_write` sends 1000 updates and expects fewer than 10 writes).
- A crash loses at most the updates of the last window.

Run it longer with `DAVID_BENCH_SECS=60 cargo test --release --test zap_bench -- --nocapture`.

## Open

- The queue is in memory. If losing up to 500 ms of progress on a crash is not acceptable, shorten the window or flush on shutdown signals.
- Writes other than progress (profiles, favorites) go straight to the single write connection and wait for it. That is fine at this scale; if it ever shows up in measurements, route them through the same task.
