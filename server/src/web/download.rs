// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The hidden feature "download" (`features.download`, see `features.rs` and `docs/spec-admin-ui.md`): an
//! administrator takes the ORIGINAL file of one title, or a whole album as one ZIP archive. The routes exist only
//! when the switch in `/etc/david.toml` is on; otherwise they are not registered and answer 404.
//!
//! Nothing of the request becomes a path: the file comes from the database row of the title and is looked up
//! below the library folders by `api::resolve`, which refuses anything that does not lie inside one of them
//! (`..`, links that lead out). The files are streamed from the disk in chunks, never read as a whole. Every
//! download is written to the change log (who, which album and title, how many bytes, whether it was complete);
//! the log never holds file content.

use std::collections::HashSet;
use std::io;
use std::path::PathBuf;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use hyper::body::Frame;
use sea_orm::EntityTrait;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::mpsc;

use super::handlers::plain_failure;
use super::{AdminSession, AppState};
use crate::entities::track;
use crate::store::Store;
use crate::zipstream::{Crc32, ZipWriter};

/// The field of the change log that marks a download.
const LOG_FIELD: &str = "Download";
/// How much of a file is read and handed to the connection at a time.
const CHUNK: usize = 64 * 1024;
/// How many chunks wait for a slow connection before the reading of the disk pauses.
const QUEUE: usize = 4;
/// A file name in the header is shortened to this many bytes (the extension is kept).
const MAX_NAME_BYTES: usize = 200;
/// The folder of a disc in the archive when the file itself does not say one (the name the library uses for discs).
const DISC_FOLDER: &str = "CD";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/admin/books/{book_id}/tracks/{track_id}/download", get(download_track))
        .route("/admin/books/{book_id}/download", get(download_album))
}

fn refuse(s: &AdminSession, status: StatusCode, key: &str) -> Response {
    plain_failure(status, key, s.lang, false)
}

// File names

/// A file name that is safe in a header and on the disk of whoever saves it: the last part of what is given (both kinds of
/// path separator), without control and direction characters, quotes and separators, shortened, never empty or only dots.
pub fn safe_name(raw: &str, fallback: &str) -> String {
    let last = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = last
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '"' | '\'' | '\\' | '/' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'))
        .collect();
    let mut name = cleaned.trim().to_owned();
    if name.len() > MAX_NAME_BYTES {
        let (stem, ext) = match name.rfind('.') {
            Some(dot) if name.len() - dot <= 12 => (name[..dot].to_owned(), name[dot..].to_owned()),
            _ => (name.clone(), String::new()),
        };
        let mut stem: String = stem;
        while stem.len() + ext.len() > MAX_NAME_BYTES {
            stem.pop();
        }
        name = format!("{}{}", stem.trim_end(), ext);
    }
    if name.chars().all(|c| c == '.' || c.is_whitespace()) { fallback.to_owned() } else { name }
}

/// `attachment; filename="ascii"; filename*=UTF-8''percent-encoded` (RFC 6266, RFC 5987).
pub fn content_disposition(name: &str) -> String {
    let ascii: String = name.chars().map(|c| if (c.is_ascii_graphic() && !matches!(c, '%' | '"' | '\\')) || c == ' ' { c } else { '_' }).collect();
    let mut encoded = String::new();
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&b) {
            encoded.push(b as char);
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")
}

// Range

#[derive(Debug, PartialEq, Eq)]
pub enum Wanted {
    /// No range, or one that is ignored (another unit, several ranges, a malformed one): the whole file.
    Whole,
    /// First and last byte, both included.
    Part(u64, u64),
    /// Starts behind the end of the file: `416`.
    Unsatisfiable,
}

/// A single `Range: bytes=...` of a file of `len` bytes (RFC 9110, 14.2). A request for several ranges gets the whole file,
/// which the standard allows.
pub fn parse_range(header: Option<&str>, len: u64) -> Wanted {
    let Some(spec) = header.and_then(|h| h.trim().strip_prefix("bytes=")) else { return Wanted::Whole };
    if spec.contains(',') {
        return Wanted::Whole;
    }
    let Some((from, to)) = spec.trim().split_once('-') else { return Wanted::Whole };
    let (from, to) = (from.trim(), to.trim());
    if from.is_empty() {
        // The last `n` bytes.
        let Ok(n) = to.parse::<u64>() else { return Wanted::Whole };
        return if n == 0 || len == 0 { Wanted::Unsatisfiable } else { Wanted::Part(len.saturating_sub(n), len - 1) };
    }
    let Ok(first) = from.parse::<u64>() else { return Wanted::Whole };
    let last = if to.is_empty() {
        u64::MAX
    } else {
        match to.parse::<u64>() {
            Ok(last) if last >= first => last,
            _ => return Wanted::Whole,
        }
    };
    if first >= len { Wanted::Unsatisfiable } else { Wanted::Part(first, last.min(len - 1)) }
}

// The body: a queue between the task that reads the disk and the connection

/// The end of the queue the connection reads. The task that fills it logs the download before it closes the queue,
/// so a client that has the whole body can rely on the entry being in the log.
struct QueueBody {
    rx: mpsc::Receiver<io::Result<Bytes>>,
}

impl hyper::body::Body for QueueBody {
    type Data = Bytes;
    type Error = io::Error;

    fn poll_frame(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, io::Error>>> {
        self.rx.poll_recv(cx).map(|item| item.map(|chunk| chunk.map(Frame::data)))
    }
}

/// The filling end: counts what was handed to the connection.
struct Sink {
    tx: mpsc::Sender<io::Result<Bytes>>,
    sent: u64,
}

impl Sink {
    fn new() -> (Sink, Body) {
        let (tx, rx) = mpsc::channel(QUEUE);
        (Sink { tx, sent: 0 }, Body::new(QueueBody { rx }))
    }

    /// False when the connection is gone.
    async fn put(&mut self, data: Vec<u8>) -> bool {
        let n = data.len() as u64;
        if self.tx.send(Ok(Bytes::from(data))).await.is_err() {
            return false;
        }
        self.sent += n;
        true
    }

    /// Ends the body with an error, so that the client sees a broken transfer and not a file that looks complete.
    async fn abort(&mut self, why: &str) {
        let _ = self.tx.send(Err(io::Error::other(why.to_owned()))).await;
    }
}

/// Reads `len` bytes of the file from `start` and hands them over. True when all of them were.
async fn pump_file(sink: &mut Sink, path: PathBuf, start: u64, len: u64) -> bool {
    let mut file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(_) => {
            sink.abort("the file cannot be opened").await;
            return false;
        }
    };
    if start > 0 && file.seek(io::SeekFrom::Start(start)).await.is_err() {
        sink.abort("the file cannot be read").await;
        return false;
    }
    let mut left = len;
    while left > 0 {
        let mut buf = vec![0u8; CHUNK.min(left as usize)];
        match file.read(&mut buf).await {
            Ok(0) | Err(_) => {
                // The file became shorter than it was, or the disk failed.
                sink.abort("the file cannot be read").await;
                return false;
            }
            Ok(n) => {
                buf.truncate(n);
                left -= n as u64;
                if !sink.put(buf).await {
                    return false;
                }
            }
        }
    }
    true
}

/// How a download ended, for the change log.
struct Outcome {
    sent: u64,
    /// What the response promised, when it is known beforehand (a title; not the archive).
    expected: Option<u64>,
    complete: bool,
}

/// Writes the entry of the change log. Only names, numbers and the outcome; no content.
async fn record(store: &Store, admin: i64, book: i64, file: &str, what: &str, o: Outcome) {
    let sent = o.sent;
    let outcome = match (o.complete, o.expected) {
        (true, _) => format!("complete, {sent} bytes"),
        (false, Some(of)) => format!("incomplete, {sent} of {of} bytes"),
        (false, None) => format!("incomplete, {sent} bytes"),
    };
    if let Err(e) = store.log_tag_change(&format!("admin:{admin}"), book, file, LOG_FIELD, what, &outcome).await {
        tracing::warn!(error = %e, book, "a download could not be written to the change log");
    }
}

// One title

async fn download_track(State(state): State<AppState>, s: AdminSession, method: Method, headers: HeaderMap, UrlPath((book_id, track_id)): UrlPath<(i64, i64)>) -> Response {
    let Ok(Some(t)) = track::Entity::find_by_id(track_id).one(&state.store.db.reader).await else { return refuse(&s, StatusCode::NOT_FOUND, "err.not_found") };
    // The title has to belong to the album of the address.
    if t.book_id != book_id {
        return refuse(&s, StatusCode::NOT_FOUND, "err.not_found");
    }
    let Ok(Some(folder)) = state.store.book_folder(book_id).await else { return refuse(&s, StatusCode::NOT_FOUND, "err.not_found") };
    let Some(path) = crate::api::resolve(&state.store, state.library.as_deref(), &folder, &t.file).await else {
        return refuse(&s, StatusCode::NOT_FOUND, "err.files_missing");
    };
    let Ok(len) = tokio::fs::metadata(&path).await.map(|m| m.len()) else { return refuse(&s, StatusCode::NOT_FOUND, "err.files_missing") };

    let range = parse_range(headers.get(header::RANGE).and_then(|v| v.to_str().ok()), len);
    // There is no validator (ETag, date) that `If-Range` could be compared with, so a conditional range gets the whole file.
    let range = if headers.contains_key(header::IF_RANGE) { Wanted::Whole } else { range };
    let (status, start, count) = match range {
        Wanted::Whole => (StatusCode::OK, 0, len),
        Wanted::Part(first, last) => (StatusCode::PARTIAL_CONTENT, first, last - first + 1),
        Wanted::Unsatisfiable => {
            let mut res = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
            if let Ok(v) = HeaderValue::from_str(&format!("bytes */{len}")) {
                res.headers_mut().insert(header::CONTENT_RANGE, v);
            }
            return res;
        }
    };

    let name = safe_name(&t.file, "title");
    let mime = crate::audioformat::mime_or_guess(&t.mime_type, &t.file);
    let mut res = Response::new(Body::empty());
    *res.status_mut() = status;
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_str(&mime).unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")));
    h.insert(header::CONTENT_LENGTH, HeaderValue::from(count));
    h.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    if let Ok(v) = HeaderValue::from_str(&content_disposition(&name)) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    if status == StatusCode::PARTIAL_CONTENT
        && let Ok(v) = HeaderValue::from_str(&format!("bytes {start}-{}/{len}", start + count - 1))
    {
        h.insert(header::CONTENT_RANGE, v);
    }
    // A HEAD request gets the headers only and is not a download.
    if method == Method::HEAD || count == 0 {
        return res;
    }

    let (mut sink, body) = Sink::new();
    *res.body_mut() = body;
    let (store, admin, file) = (state.store.clone(), s.user_id, t.file.clone());
    let what = if status == StatusCode::PARTIAL_CONTENT { format!("bytes {start}-{}/{len}", start + count - 1) } else { String::new() };
    tokio::spawn(crate::actor::run_as(crate::actor::profile(admin), async move {
        let complete = pump_file(&mut sink, path, start, count).await;
        // Logged before the queue is closed (the sink is dropped last), see `QueueBody`.
        record(&store, admin, book_id, &file, &what, Outcome { sent: sink.sent, expected: Some(count), complete }).await;
    }));
    res
}

// One album

struct Planned {
    name: String,
    path: PathBuf,
    size: u64,
}

/// The names of the entries of the archive: `<disc folder>/<file name>` for an album of several discs (the folder
/// of the file when it has one, else `CD <n>`), the plain file name for one disc. Names that would collide get a number.
pub fn entry_names(tracks: &[track::Model]) -> Vec<String> {
    let discs: HashSet<i64> = tracks.iter().map(|t| t.disc).collect();
    let many = discs.len() > 1;
    let mut taken = HashSet::new();
    tracks
        .iter()
        .map(|t| {
            let file = safe_name(&t.file, "title");
            let mut parts: Vec<String> = Vec::new();
            if many {
                let dirs: Vec<&str> = t.file.split(['/', '\\']).collect();
                parts = dirs[..dirs.len().saturating_sub(1)].iter().map(|d| safe_name(d, "")).filter(|d| !d.is_empty()).collect();
                if parts.is_empty() {
                    parts.push(format!("{DISC_FOLDER} {}", t.disc));
                }
            }
            let dir = parts.join("/");
            let mut n = 1;
            loop {
                let leaf = if n == 1 {
                    file.clone()
                } else {
                    match file.rfind('.') {
                        Some(dot) if dot > 0 => format!("{} ({n}){}", &file[..dot], &file[dot..]),
                        _ => format!("{file} ({n})"),
                    }
                };
                let full = if dir.is_empty() { leaf } else { format!("{dir}/{leaf}") };
                if taken.insert(full.to_lowercase()) {
                    return full;
                }
                n += 1;
            }
        })
        .collect()
}

async fn pump_zip(sink: &mut Sink, plan: Vec<Planned>) -> bool {
    let mut zip = ZipWriter::new();
    for entry in plan {
        let mut file = match tokio::fs::File::open(&entry.path).await {
            Ok(f) => f,
            Err(_) => {
                sink.abort("a file cannot be opened").await;
                return false;
            }
        };
        if !sink.put(zip.start_entry(&entry.name, entry.size)).await {
            return false;
        }
        let (mut crc, mut done) = (Crc32::new(), 0u64);
        loop {
            let mut buf = vec![0u8; CHUNK];
            match file.read(&mut buf).await {
                Err(_) => {
                    sink.abort("a file cannot be read").await;
                    return false;
                }
                Ok(0) => break,
                Ok(n) => {
                    buf.truncate(n);
                    crc.update(&buf);
                    done += n as u64;
                    zip.data_written(n as u64);
                    // The header promised `size` bytes (and decided on ZIP64 with it): a file that changed while it is read breaks the archive.
                    if done > entry.size || !sink.put(buf).await {
                        if done > entry.size {
                            sink.abort("a file changed while it was read").await;
                        }
                        return false;
                    }
                }
            }
        }
        if done != entry.size {
            sink.abort("a file changed while it was read").await;
            return false;
        }
        if !sink.put(zip.finish_entry(crc.finish(), done)).await {
            return false;
        }
    }
    sink.put(zip.finish()).await
}

async fn download_album(State(state): State<AppState>, s: AdminSession, method: Method, UrlPath(book_id): UrlPath<i64>) -> Response {
    let Ok(Some(row)) = state.store.admin_book_row(book_id).await else { return refuse(&s, StatusCode::NOT_FOUND, "err.not_found") };
    let Ok(Some(folder)) = state.store.book_folder(book_id).await else { return refuse(&s, StatusCode::NOT_FOUND, "err.not_found") };
    let Ok(tracks) = state.store.tracks_of(book_id).await else { return refuse(&s, StatusCode::INTERNAL_SERVER_ERROR, "err.server") };
    if tracks.is_empty() {
        return refuse(&s, StatusCode::NOT_FOUND, "err.not_found");
    }
    // Every file is looked up (and its size read) before the first byte goes out, so that an album with a missing file is refused as a whole.
    let names = entry_names(&tracks);
    let mut plan = Vec::with_capacity(tracks.len());
    for (t, name) in tracks.iter().zip(names) {
        let Some(path) = crate::api::resolve(&state.store, state.library.as_deref(), &folder, &t.file).await else {
            return refuse(&s, StatusCode::CONFLICT, "err.files_missing");
        };
        let Ok(size) = tokio::fs::metadata(&path).await.map(|m| m.len()) else { return refuse(&s, StatusCode::CONFLICT, "err.files_missing") };
        plan.push(Planned { name, path, size });
    }
    let count = plan.len();

    let archive = format!("{}.zip", safe_name(&row.title, "album").trim_end_matches('.'));
    let mut res = Response::new(Body::empty());
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/zip"));
    // The length is not announced: the archive is written as it is read, and a file may change until it is.
    if let Ok(v) = HeaderValue::from_str(&content_disposition(&archive)) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    if method == Method::HEAD {
        return res;
    }
    let (mut sink, body) = Sink::new();
    *res.body_mut() = body;
    let (store, admin) = (state.store.clone(), s.user_id);
    tokio::spawn(crate::actor::run_as(crate::actor::profile(admin), async move {
        let complete = pump_zip(&mut sink, plan).await;
        record(&store, admin, book_id, "", &format!("album, {count} files"), Outcome { sent: sink.sent, expected: None, complete }).await;
    }));
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_lose_what_is_dangerous_and_keep_umlauts() {
        assert_eq!(safe_name("CD 1/Kapitel 01 – Äpfel.mp3", "x"), "Kapitel 01 – Äpfel.mp3");
        assert_eq!(safe_name("..\\..\\evil\"name\r\n.mp3", "x"), "evilname.mp3");
        assert_eq!(safe_name("a\u{202E}b\u{0}c.mp3", "x"), "abc.mp3");
        assert_eq!(safe_name("..", "fallback"), "fallback");
        assert_eq!(safe_name("", "fallback"), "fallback");
        assert_eq!(safe_name("dir/", "fallback"), "fallback");
        let long = format!("{}.flac", "ä".repeat(300));
        let short = safe_name(&long, "x");
        assert!(short.len() <= MAX_NAME_BYTES && short.ends_with(".flac"), "{}", short.len());
    }

    #[test]
    fn the_header_carries_an_ascii_name_and_the_encoded_one() {
        assert_eq!(content_disposition("Fünf 100%.mp3"), "attachment; filename=\"F_nf 100_.mp3\"; filename*=UTF-8''F%C3%BCnf%20100%25.mp3");
        assert!(HeaderValue::from_str(&content_disposition("ü\u{1F3B5}\"x")).is_ok());
    }

    #[test]
    fn ranges() {
        assert_eq!(parse_range(None, 100), Wanted::Whole);
        assert_eq!(parse_range(Some("bytes=0-9"), 100), Wanted::Part(0, 9));
        assert_eq!(parse_range(Some("bytes=90-"), 100), Wanted::Part(90, 99));
        assert_eq!(parse_range(Some("bytes=-10"), 100), Wanted::Part(90, 99));
        assert_eq!(parse_range(Some("bytes=-500"), 100), Wanted::Part(0, 99));
        assert_eq!(parse_range(Some("bytes=50-5000"), 100), Wanted::Part(50, 99));
        assert_eq!(parse_range(Some("bytes=100-"), 100), Wanted::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=-0"), 100), Wanted::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=0-"), 0), Wanted::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=5-1"), 100), Wanted::Whole);
        assert_eq!(parse_range(Some("bytes=0-1,5-6"), 100), Wanted::Whole);
        assert_eq!(parse_range(Some("items=0-1"), 100), Wanted::Whole);
        assert_eq!(parse_range(Some("bytes=a-b"), 100), Wanted::Whole);
    }

    fn t(disc: i64, file: &str) -> track::Model {
        track::Model { id: 0, book_id: 1, disc, number: 1, title: String::new(), file: file.into(), duration_ms: 0, size_bytes: 0, mtime: 0, format: String::new(), codec: String::new(), mime_type: String::new(), bitrate_kbps: 0, sample_rate: 0, channels: 0, sha256: String::new(), hashed_mtime: 0, hashed_size: 0 }
    }

    #[test]
    fn the_entries_of_one_disc_are_flat_and_those_of_several_discs_sit_in_folders() {
        assert_eq!(entry_names(&[t(1, "CD 1/a.mp3"), t(1, "CD 1/b.mp3")]), ["a.mp3", "b.mp3"]);
        assert_eq!(entry_names(&[t(1, "CD 1/a.mp3"), t(2, "CD 2/a.mp3")]), ["CD 1/a.mp3", "CD 2/a.mp3"]);
        // Files without a folder of their own get one per disc; equal names are told apart.
        assert_eq!(entry_names(&[t(1, "a.mp3"), t(2, "a.mp3"), t(2, "A.mp3")]), ["CD 1/a.mp3", "CD 2/a.mp3", "CD 2/A (2).mp3"]);
        assert_eq!(entry_names(&[t(1, "x.mp3"), t(1, "x.mp3")]), ["x.mp3", "x (2).mp3"]);
    }
}
