// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Zapping benchmark: 150 clients (the profile limit) flip through books and tracks while a library
//! import keeps the writer busy. Checks that nothing fails, that reads and
//! enqueueing stay fast, and that the stored progress is the latest one.
//!
//! `DAVID_BENCH_SECS=60 DAVID_BENCH_CLIENTS=150 cargo test --release --test zap_bench -- --nocapture`

mod common;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{book, open};
use david_server::{ProgressUpdate, now_ms};

const BOOKS: usize = 40;

/// Tiny deterministic generator, so runs are comparable without a dependency.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

fn report(name: &str, mut v: Vec<Duration>) -> Duration {
    v.sort();
    let p99 = percentile(&v, 0.99);
    println!(
        "{name:<10} n={:<7} p50={:>8.2?} p99={:>8.2?} max={:>8.2?}",
        v.len(),
        percentile(&v, 0.5),
        p99,
        v.last().copied().unwrap_or_default()
    );
    p99
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn max_profiles_zapping_while_the_library_is_imported() {
    let client_count: usize =
        std::env::var("DAVID_BENCH_CLIENTS").ok().and_then(|s| s.parse().ok()).unwrap_or(150);
    let secs: u64 = std::env::var("DAVID_BENCH_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(8);
    let (_dir, store) = open().await;

    // Library: 40 books with 20 tracks each.
    let books: Vec<_> = (0..BOOKS).map(|i| book("Author", &format!("Book {i:02}"), &[10, 10])).collect();
    store.import_books(books).await.unwrap();
    let mut book_tracks: Vec<(i64, Vec<i64>)> = Vec::new();
    for i in 0..BOOKS {
        let b = store.find_book(&format!("Author/Book {i:02}")).await.unwrap().unwrap();
        // Only released books are shown, so the lists below must have something to return.
        store.release_book(b.id, 0).await.unwrap();
        let t = store.tracks_of(b.id).await.unwrap().into_iter().map(|t| t.id).collect();
        book_tracks.push((b.id, t));
    }
    let book_tracks = Arc::new(book_tracks);

    let mut profiles = Vec::new();
    for i in 0..client_count {
        profiles.push(store.create_profile(&format!("client {i}"), false).await.unwrap());
    }

    let stop = Arc::new(AtomicBool::new(false));
    let errors = Arc::new(AtomicU64::new(0));
    let enqueue = Arc::new(Mutex::new(Vec::new()));
    let reads = Arc::new(Mutex::new(Vec::new()));
    let imports = Arc::new(Mutex::new(Vec::new()));

    // Background import: new books of 100 tracks, as a rescan of a big library would.
    let importer = {
        let (store, stop, errors, imports) = (store.clone(), stop.clone(), errors.clone(), imports.clone());
        tokio::spawn(async move {
            let mut n = 0;
            while !stop.load(Ordering::Relaxed) {
                let t0 = Instant::now();
                if store.import_books(vec![book("Import", &format!("Bulk {n}"), &[50, 50])]).await.is_err() {
                    errors.fetch_add(1, Ordering::Relaxed);
                }
                imports.lock().unwrap().push(t0.elapsed());
                n += 1;
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
    };

    // Each client zaps: bursts of updates for random books and tracks, with reads in between.
    let mut clients = Vec::new();
    for (c, &profile_id) in profiles.iter().enumerate() {
        let (store, stop, errors, enqueue, reads) =
            (store.clone(), stop.clone(), errors.clone(), enqueue.clone(), reads.clone());
        let book_tracks = book_tracks.clone();
        clients.push(tokio::spawn(async move {
            let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ (c as u64 + 1));
            let mut last: HashMap<i64, ProgressUpdate> = HashMap::new();
            let (mut my_enqueue, mut my_reads) = (Vec::new(), Vec::new());
            let mut seq = 0i64;
            while !stop.load(Ordering::Relaxed) {
                // A zap: 3 to 8 quick changes.
                for _ in 0..(3 + rng.below(6)) {
                    let (book_id, tracks) = &book_tracks[rng.below(BOOKS)];
                    seq += 1;
                    let u = ProgressUpdate {
                        profile_id,
                        book_id: *book_id,
                        track_id: tracks[rng.below(tracks.len())],
                        position_ms: rng.below(240_000) as i64,
                        finished: false,
                        updated_at: now_ms() * 1_000 + seq, // strictly increasing per client
                    };
                    let t0 = Instant::now();
                    if store.save_progress(u).await.is_err() {
                        errors.fetch_add(1, Ordering::Relaxed);
                    }
                    my_enqueue.push(t0.elapsed());
                    last.insert(*book_id, u);
                    tokio::time::sleep(Duration::from_millis(5 + rng.below(30) as u64)).await;
                }
                let t0 = Instant::now();
                let ok = store.continue_listening(profile_id, 10).await.is_ok()
                    && store.visible_books(profile_id).await.is_ok();
                my_reads.push(t0.elapsed());
                if !ok {
                    errors.fetch_add(1, Ordering::Relaxed);
                }
                tokio::time::sleep(Duration::from_millis(50 + rng.below(250) as u64)).await;
            }
            enqueue.lock().unwrap().extend(my_enqueue);
            reads.lock().unwrap().extend(my_reads);
            last
        }));
    }

    tokio::time::sleep(Duration::from_secs(secs)).await;
    stop.store(true, Ordering::Relaxed);
    let mut expected: Vec<(i64, i64, ProgressUpdate)> = Vec::new();
    for (c, h) in clients.into_iter().enumerate() {
        for (book_id, u) in h.await.unwrap() {
            expected.push((profiles[c], book_id, u));
        }
    }
    importer.await.unwrap();
    let t0 = Instant::now();
    store.flush().await.unwrap();
    let flush = t0.elapsed();

    // The newest update per (profile, book) must be what is stored.
    let mut wrong = 0;
    for (profile_id, book_id, u) in &expected {
        let p = store.progress_of(*profile_id, *book_id).await.unwrap().unwrap();
        if (p.track_id, p.position_ms) != (u.track_id, u.position_ms) {
            wrong += 1;
        }
    }

    let stats = store.writer_stats();
    let received = stats.received.load(Ordering::Relaxed);
    let written = stats.written.load(Ordering::Relaxed);
    println!("\n{client_count} clients, {secs} s, {BOOKS} books + background import");
    let enqueue_p99 = report("enqueue", enqueue.lock().unwrap().clone());
    let read_p99 = report("read", reads.lock().unwrap().clone());
    report("import", imports.lock().unwrap().clone());
    println!(
        "progress updates received={received} written={written} batches={} dropped={} (coalesced {:.0}%)",
        stats.batches.load(Ordering::Relaxed),
        stats.dropped.load(Ordering::Relaxed),
        100.0 * (1.0 - written as f64 / received.max(1) as f64)
    );
    println!("final flush {flush:.2?}, wrong final positions: {wrong} of {}", expected.len());

    assert_eq!(errors.load(Ordering::Relaxed), 0, "no operation may fail");
    assert_eq!(stats.dropped.load(Ordering::Relaxed), 0);
    assert_eq!(wrong, 0, "the newest update per profile and book must win");
    assert!(enqueue_p99 < Duration::from_millis(50), "enqueue p99 {enqueue_p99:?}");
    assert!(read_p99 < Duration::from_millis(250), "read p99 {read_p99:?}");
}
