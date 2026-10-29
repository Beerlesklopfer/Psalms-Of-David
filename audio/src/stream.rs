// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The byte queue behind a *pushed* stream (internet radio).
//!
//! The network code lives in Dart (TLS, proxies, certificates, ICY metadata); it
//! pushes the plain MP3 bytes in with [`ByteQueue::push`]. The decoder thread reads
//! them out through a [`QueueReader`], which symphonia sees as a media source that
//! cannot seek and has no length.
//!
//! Rules:
//! - `push` never blocks. When the queue is full it accepts fewer bytes than offered
//!   (back pressure; the caller waits and offers the rest again).
//! - While the queue is in *discarding* mode (a live stream that is paused) `push`
//!   accepts everything and throws the oldest bytes away, so memory stays bounded and
//!   the connection can stay open. Resuming clears the queue and goes back to live.
//! - `read` blocks until bytes arrive, the stream ends, fails, is closed, or the
//!   reader's own abort flag is set. It polls the abort flag every [`ABORT_POLL`].

use std::collections::VecDeque;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use symphonia::core::io::MediaSource;

/// How often a blocked reader looks at its abort flag.
pub const ABORT_POLL: Duration = Duration::from_millis(50);

/// Capacity of the queue in bytes: about 65 s of a 128 kbit/s MP3 stream. It absorbs
/// network jitter; the decoder normally keeps it nearly empty.
pub const QUEUE_BYTES: usize = 1 << 20;

#[derive(Default)]
struct Inner {
    bytes: VecDeque<u8>,
    /// The sender has no more data; readers get EOF after the rest.
    ended: bool,
    /// The sender failed; readers get this error at once.
    failed: Option<String>,
    /// Nobody will read or write any more.
    closed: bool,
    discarding: bool,
}

pub struct ByteQueue {
    inner: Mutex<Inner>,
    changed: Condvar,
    capacity: usize,
}

/// Why a push was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushError {
    /// The queue was closed, ended or failed already.
    Closed,
}

impl ByteQueue {
    pub fn new(capacity: usize) -> Arc<ByteQueue> {
        Arc::new(ByteQueue { inner: Mutex::new(Inner::default()), changed: Condvar::new(), capacity: capacity.max(1) })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Offers `data`; returns how many bytes were taken (0 if the queue is full).
    pub fn push(&self, data: &[u8]) -> Result<usize, PushError> {
        let mut q = self.lock();
        if q.closed || q.ended || q.failed.is_some() {
            return Err(PushError::Closed);
        }
        let taken = if q.discarding {
            // Keep the newest `capacity` bytes.
            let data = &data[data.len().saturating_sub(self.capacity)..];
            let overflow = (q.bytes.len() + data.len()).saturating_sub(self.capacity);
            let drop_n = overflow.min(q.bytes.len());
            q.bytes.drain(..drop_n);
            q.bytes.extend(data);
            data.len()
        } else {
            let n = data.len().min(self.capacity - q.bytes.len());
            q.bytes.extend(&data[..n]);
            n
        };
        drop(q);
        self.changed.notify_all();
        Ok(taken)
    }

    /// No more data will come. What is queued is still read.
    pub fn end(&self) {
        self.lock().ended = true;
        self.changed.notify_all();
    }

    /// The sender gave up. Readers get an error with `message` at once.
    pub fn fail(&self, message: &str) {
        let mut q = self.lock();
        q.failed.get_or_insert_with(|| message.to_owned());
        q.bytes.clear();
        drop(q);
        self.changed.notify_all();
    }

    /// Drops the queue for good: pushes fail, blocked readers return an error.
    pub fn close(&self) {
        let mut q = self.lock();
        q.closed = true;
        q.bytes.clear();
        drop(q);
        self.changed.notify_all();
    }

    /// Throws away everything queued.
    pub fn clear(&self) {
        self.lock().bytes.clear();
        self.changed.notify_all();
    }

    /// In discarding mode a full queue drops its oldest bytes instead of refusing new ones.
    pub fn set_discarding(&self, on: bool) {
        self.lock().discarding = on;
    }

    /// The reason given to [`ByteQueue::fail`], if any.
    pub fn failure(&self) -> Option<String> {
        self.lock().failed.clone()
    }

    pub fn len(&self) -> usize {
        self.lock().bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Blocking read. `Ok(0)` is the end of the stream. Returns an error if the
    /// stream failed or was closed, or `abort` was set (`ErrorKind::ConnectionAborted`; not `Interrupted`, which readers retry).
    pub fn read(&self, out: &mut [u8], abort: &AtomicBool) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let mut q = self.lock();
        loop {
            if abort.load(Ordering::Acquire) {
                return Err(io::Error::new(io::ErrorKind::ConnectionAborted, "stream reading was cancelled"));
            }
            if q.closed {
                return Err(io::Error::new(io::ErrorKind::ConnectionAborted, "stream closed"));
            }
            if let Some(message) = &q.failed {
                return Err(io::Error::other(message.clone()));
            }
            if !q.bytes.is_empty() {
                let n = out.len().min(q.bytes.len());
                for (dst, src) in out.iter_mut().zip(q.bytes.drain(..n)) {
                    *dst = src;
                }
                return Ok(n);
            }
            if q.ended {
                return Ok(0);
            }
            q = self.changed.wait_timeout(q, ABORT_POLL).unwrap_or_else(|e| e.into_inner()).0;
        }
    }
}

/// The reading end as a symphonia media source: no seeking, no length.
pub struct QueueReader {
    queue: Arc<ByteQueue>,
    abort: Arc<AtomicBool>,
}

impl QueueReader {
    pub fn new(queue: Arc<ByteQueue>, abort: Arc<AtomicBool>) -> QueueReader {
        QueueReader { queue, abort }
    }
}

impl Read for QueueReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.queue.read(buf, &self.abort)
    }
}

impl Seek for QueueReader {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "a live stream cannot seek"))
    }
}

impl MediaSource for QueueReader {
    fn is_seekable(&self) -> bool {
        false
    }

    fn byte_len(&self) -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Instant;

    fn no_abort() -> AtomicBool {
        AtomicBool::new(false)
    }

    #[test]
    fn bytes_come_out_in_order() {
        let q = ByteQueue::new(16);
        assert_eq!(q.push(b"hello ").unwrap(), 6);
        assert_eq!(q.push(b"world").unwrap(), 5);
        let mut out = [0u8; 20];
        let abort = no_abort();
        let n = q.read(&mut out, &abort).unwrap();
        assert_eq!(&out[..n], b"hello world");
        assert!(q.is_empty());
    }

    #[test]
    fn a_full_queue_pushes_back() {
        let q = ByteQueue::new(4);
        assert_eq!(q.push(b"abcdef").unwrap(), 4, "only what fits is taken");
        assert_eq!(q.push(b"ef").unwrap(), 0, "full: nothing is taken");
        let mut out = [0u8; 2];
        assert_eq!(q.read(&mut out, &no_abort()).unwrap(), 2);
        assert_eq!(q.push(b"ef").unwrap(), 2, "room again after a read");
    }

    #[test]
    fn a_discarding_queue_keeps_the_newest_bytes() {
        let q = ByteQueue::new(4);
        q.set_discarding(true);
        assert_eq!(q.push(b"ab").unwrap(), 2);
        assert_eq!(q.push(b"cdef").unwrap(), 4, "everything is accepted");
        assert_eq!(q.len(), 4);
        assert_eq!(q.push(b"0123456789").unwrap(), 4, "an oversized push keeps its tail");
        let mut out = [0u8; 8];
        let n = q.read(&mut out, &no_abort()).unwrap();
        assert_eq!(&out[..n], b"6789");
    }

    #[test]
    fn the_end_comes_after_the_rest() {
        let q = ByteQueue::new(16);
        q.push(b"xyz").unwrap();
        q.end();
        assert_eq!(q.push(b"more"), Err(PushError::Closed));
        let mut out = [0u8; 8];
        assert_eq!(q.read(&mut out, &no_abort()).unwrap(), 3);
        assert_eq!(q.read(&mut out, &no_abort()).unwrap(), 0, "EOF");
    }

    #[test]
    fn a_failure_is_reported_to_the_reader() {
        let q = ByteQueue::new(16);
        q.push(b"abc").unwrap();
        q.fail("connection lost");
        let mut out = [0u8; 8];
        let err = q.read(&mut out, &no_abort()).unwrap_err();
        assert_eq!(err.to_string(), "connection lost");
        assert_eq!(q.push(b"x"), Err(PushError::Closed));
    }

    #[test]
    fn a_reader_waits_for_data_that_arrives_later() {
        let q = ByteQueue::new(16);
        let writer = {
            let q = q.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(100));
                q.push(b"late").unwrap();
            })
        };
        let mut out = [0u8; 8];
        let n = q.read(&mut out, &no_abort()).unwrap();
        assert_eq!(&out[..n], b"late");
        writer.join().unwrap();
    }

    #[test]
    fn an_underrun_blocks_and_the_abort_flag_ends_it() {
        let q = ByteQueue::new(16);
        let abort = Arc::new(AtomicBool::new(false));
        let reader = {
            let (q, abort) = (q.clone(), abort.clone());
            thread::spawn(move || q.read(&mut [0u8; 4], &abort))
        };
        thread::sleep(Duration::from_millis(150));
        assert!(!reader.is_finished(), "an empty queue blocks the reader");
        let started = Instant::now();
        abort.store(true, Ordering::Release);
        let err = reader.join().unwrap().unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::ConnectionAborted);
        assert!(started.elapsed() < Duration::from_secs(2), "the abort is noticed quickly");
    }

    #[test]
    fn closing_wakes_a_blocked_reader() {
        let q = ByteQueue::new(16);
        let reader = {
            let q = q.clone();
            thread::spawn(move || q.read(&mut [0u8; 4], &AtomicBool::new(false)))
        };
        thread::sleep(Duration::from_millis(100));
        q.close();
        assert_eq!(reader.join().unwrap().unwrap_err().kind(), io::ErrorKind::ConnectionAborted);
        assert_eq!(q.push(b"x"), Err(PushError::Closed));
    }

    #[test]
    fn the_reader_is_a_media_source_without_seek_or_length() {
        let q = ByteQueue::new(16);
        let mut r = QueueReader::new(q, Arc::new(AtomicBool::new(false)));
        assert!(!r.is_seekable());
        assert_eq!(r.byte_len(), None);
        assert!(r.seek(SeekFrom::Start(0)).is_err());
    }
}
