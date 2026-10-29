// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The buffer between the drive and the encoder: a ring of bytes made once, with a write position and a read position.
//!
//! The reader (the thread that talks to the drive) writes the PCM into the ring as fast as the drive delivers it and goes on over the
//! border of a track, because audio is continuous; it waits only when the ring is full. The encoder takes the PCM of one track after the
//! other from the ring; the end of each track is marked by its offset, so the encoder knows where one track stops. The encoder waits only
//! when the ring is empty. Both sides sleep on a condition variable and wake when the other side has moved, when the track ends, when
//! the writer is done, and when either side gives up (`abort`, `consumer_gone`): so nothing can wait for ever.

use std::io::Read;
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// What a rip measures of itself, to see what brakes the drive: the filling of the buffer now and at most, how long the drive read, and
/// how often and how long the reader had to wait for room.
#[derive(Debug, Default)]
pub struct RipStats {
    /// Bytes in the ring now, and the most that were.
    pub filled_bytes: std::sync::atomic::AtomicI64,
    pub peak_bytes: std::sync::atomic::AtomicI64,
    /// Milliseconds spent in the reads of the drive.
    pub read_ms: std::sync::atomic::AtomicU64,
    /// Sectors the drive has delivered in those reads (every reading: the first, the second and the repeated ones). With `read_ms`
    /// this is the speed the drive reads at, as a measurement: `sectors_read / (read_ms / 1000) / 75` is the multiple of 1x.
    pub sectors_read: std::sync::atomic::AtomicU64,
    /// How many times the reader found the ring full and had to wait, the time it waited in all and the longest wait (ms).
    pub reader_waits: std::sync::atomic::AtomicU64,
    pub wait_ms_total: std::sync::atomic::AtomicU64,
    pub wait_ms_longest: std::sync::atomic::AtomicU64,
    /// What became of the priority asked for the encoder thread: 0 = none asked or not started yet, 1 = raised to `encoder_nice`,
    /// 2 = the system refused every value that raises it.
    pub encoder_nice_state: std::sync::atomic::AtomicU8,
    /// The nice value the encoder thread runs at when `encoder_nice_state` is 1.
    pub encoder_nice: std::sync::atomic::AtomicI32,
}

/// Bytes of PCM in one second of audio (44.1 kHz, 16 bit, stereo).
pub const BYTES_PER_SECOND: usize = 176_400;

#[derive(Debug)]
struct State {
    buf: Vec<u8>,
    /// Where the next byte is read from.
    head: usize,
    /// Bytes in the ring.
    len: usize,
    /// Bytes written and read since the start (absolute positions).
    written: u64,
    read: u64,
    /// The absolute position at which track `i` ends, once the reader has finished it.
    ends: Vec<Option<u64>>,
    /// No more writing will come.
    closed: bool,
    /// Somebody gave up: both sides stop.
    aborted: bool,
    /// The encoder has gone: writing is pointless.
    consumer_gone: bool,
}

/// Why a write did not happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    Aborted,
    ConsumerGone,
}

#[derive(Debug)]
pub struct Ring {
    state: Mutex<State>,
    not_full: Condvar,
    not_empty: Condvar,
    capacity: usize,
    stats: Option<std::sync::Arc<RipStats>>,
}

/// What a write cost: how long it had to wait for room (zero if it did not wait).
#[derive(Debug, Clone, Copy, Default)]
pub struct Waited(pub Duration);

impl Ring {
    /// A ring of `capacity` bytes (at least one second of audio) for `tracks` tracks.
    pub fn new(capacity: usize, tracks: usize) -> Ring {
        Ring::with_stats(capacity, tracks, None)
    }

    /// The same, telling its filling and the waits of the writer to `stats`.
    pub fn with_stats(capacity: usize, tracks: usize, stats: Option<std::sync::Arc<RipStats>>) -> Ring {
        let capacity = capacity.max(BYTES_PER_SECOND);
        Ring { state: Mutex::new(State { buf: vec![0u8; capacity], head: 0, len: 0, written: 0, read: 0, ends: vec![None; tracks], closed: false, aborted: false, consumer_gone: false }), not_full: Condvar::new(), not_empty: Condvar::new(), capacity, stats }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Bytes in the ring now.
    pub fn filled(&self) -> usize {
        self.lock().len
    }

    fn note(&self, len: usize) {
        if let Some(st) = &self.stats {
            use std::sync::atomic::Ordering::Relaxed;
            st.filled_bytes.store(len as i64, Relaxed);
            st.peak_bytes.fetch_max(len as i64, Relaxed);
        }
    }

    /// Writes all of `data`, waiting for room as long as it takes. Returns the time spent waiting.
    pub fn write(&self, mut data: &[u8]) -> Result<Waited, Stopped> {
        let mut waited = Duration::ZERO;
        let mut s = self.lock();
        while !data.is_empty() {
            if s.aborted {
                return Err(Stopped::Aborted);
            }
            if s.consumer_gone {
                return Err(Stopped::ConsumerGone);
            }
            let room = self.capacity - s.len;
            if room == 0 {
                let t = Instant::now();
                s = self.not_full.wait(s).unwrap_or_else(|e| e.into_inner());
                waited += t.elapsed();
                continue;
            }
            let n = room.min(data.len());
            let tail = (s.head + s.len) % self.capacity;
            let first = n.min(self.capacity - tail);
            s.buf[tail..tail + first].copy_from_slice(&data[..first]);
            s.buf[..n - first].copy_from_slice(&data[first..n]);
            s.len += n;
            s.written += n as u64;
            data = &data[n..];
            self.note(s.len);
            self.not_empty.notify_all();
        }
        if !waited.is_zero()
            && let Some(st) = &self.stats
        {
            use std::sync::atomic::Ordering::Relaxed;
            let ms = waited.as_millis() as u64;
            st.reader_waits.fetch_add(1, Relaxed);
            st.wait_ms_total.fetch_add(ms, Relaxed);
            st.wait_ms_longest.fetch_max(ms, Relaxed);
        }
        Ok(Waited(waited))
    }

    /// Marks the end of track `i` at the position written so far.
    pub fn finish_track(&self, i: usize) {
        let mut s = self.lock();
        let at = s.written;
        if let Some(e) = s.ends.get_mut(i) {
            *e = Some(at);
        }
        self.not_empty.notify_all();
    }

    /// No more writing will come (the reader has read everything it will read).
    pub fn close(&self) {
        self.lock().closed = true;
        self.not_empty.notify_all();
    }

    /// Somebody gave up: both sides stop at once.
    pub fn abort(&self) {
        self.lock().aborted = true;
        self.not_empty.notify_all();
        self.not_full.notify_all();
    }

    /// The consumer is gone: the writer stops waiting for room.
    pub fn consumer_gone(&self) {
        self.lock().consumer_gone = true;
        self.not_full.notify_all();
    }

    pub fn is_aborted(&self) -> bool {
        self.lock().aborted
    }

    /// A reader for the PCM of track `i`: it ends at the mark of the track (or when the ring is closed or aborted).
    pub fn track(&self, i: usize) -> TrackReader<'_> {
        TrackReader { ring: self, index: i }
    }
}

pub struct TrackReader<'a> {
    ring: &'a Ring,
    index: usize,
}

impl Read for TrackReader<'_> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let r = self.ring;
        let mut s = r.lock();
        loop {
            if s.aborted {
                return Ok(0);
            }
            let end = s.ends.get(self.index).copied().flatten();
            // What of the ring belongs to this track: up to its end if that is known.
            let mine = match end {
                Some(e) => (e.saturating_sub(s.read) as usize).min(s.len),
                None => s.len,
            };
            if mine > 0 {
                let n = mine.min(out.len());
                let first = n.min(r.capacity - s.head);
                out[..first].copy_from_slice(&s.buf[s.head..s.head + first]);
                out[first..n].copy_from_slice(&s.buf[..n - first]);
                s.head = (s.head + n) % r.capacity;
                s.len -= n;
                s.read += n as u64;
                r.note(s.len);
                r.not_full.notify_all();
                return Ok(n);
            }
            // Nothing of this track is there now: the end of the track, or the end of everything, or wait.
            if end.is_some() || s.closed {
                return Ok(0);
            }
            s = r.not_empty.wait(s).unwrap_or_else(|e| e.into_inner());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha1::{Digest, Sha1};
    use std::sync::Arc;

    fn sha(b: &[u8]) -> String {
        Sha1::digest(b).iter().map(|x| format!("{x:02x}")).collect()
    }

    fn pattern(n: usize, seed: u8) -> Vec<u8> {
        (0..n).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect()
    }

    #[test]
    fn bytes_come_out_as_they_went_in_across_the_wrap_and_several_tracks() {
        // A small ring (one second) and tracks that are not a multiple of it: the data goes round the ring many times.
        let ring = Arc::new(Ring::new(BYTES_PER_SECOND, 3));
        let tracks: Vec<Vec<u8>> = vec![pattern(300_000, 1), pattern(123_457, 2), pattern(500_001, 3)];
        let expected: Vec<String> = tracks.iter().map(|t| sha(t)).collect();
        let writer = {
            let (ring, tracks) = (ring.clone(), tracks.clone());
            std::thread::spawn(move || {
                for (i, t) in tracks.iter().enumerate() {
                    for chunk in t.chunks(63_504) {
                        ring.write(chunk).unwrap();
                    }
                    ring.finish_track(i);
                }
                ring.close();
            })
        };
        for (i, want) in expected.iter().enumerate() {
            let mut got = Vec::new();
            ring.track(i).read_to_end(&mut got).unwrap();
            assert_eq!(got.len(), tracks[i].len(), "track {i}");
            assert_eq!(&sha(&got), want, "track {i}: nothing lost or changed over the wrap");
        }
        writer.join().unwrap();
    }

    #[test]
    fn the_writer_waits_when_full_and_the_reader_when_empty_and_both_are_woken() {
        let ring = Arc::new(Ring::new(BYTES_PER_SECOND, 1));
        // Fill it: a second write must wait until something is read.
        ring.write(&vec![1u8; BYTES_PER_SECOND]).unwrap();
        assert_eq!(ring.filled(), BYTES_PER_SECOND);
        let blocked = {
            let ring = ring.clone();
            std::thread::spawn(move || ring.write(&vec![2u8; 1000]).map(|w| w.0))
        };
        std::thread::sleep(Duration::from_millis(100));
        assert!(!blocked.is_finished(), "a full ring holds the writer");
        let mut buf = vec![0u8; 5000];
        assert_eq!(ring.track(0).read(&mut buf).unwrap(), 5000);
        let waited = blocked.join().unwrap().unwrap();
        assert!(waited >= Duration::from_millis(80), "the wait is told: {waited:?}");
        // Empty: a reader waits until there is something or the track ends.
        let ring2 = Arc::new(Ring::new(BYTES_PER_SECOND, 1));
        let reader = {
            let ring2 = ring2.clone();
            std::thread::spawn(move || {
                let mut b = vec![0u8; 10];
                ring2.track(0).read(&mut b)
            })
        };
        std::thread::sleep(Duration::from_millis(100));
        assert!(!reader.is_finished(), "an empty ring holds the reader");
        ring2.write(&[9u8; 4]).unwrap();
        assert_eq!(reader.join().unwrap().unwrap(), 4);
        // The end of the track ends the reading, with the bytes before it all read.
        ring2.write(&[8u8; 3]).unwrap();
        ring2.finish_track(0);
        let mut rest = Vec::new();
        ring2.track(0).read_to_end(&mut rest).unwrap();
        assert_eq!(rest, vec![8u8; 3]);
    }

    #[test]
    fn an_abort_or_a_gone_consumer_wakes_whoever_waits_so_nothing_hangs() {
        let full = Arc::new(Ring::new(BYTES_PER_SECOND, 1));
        full.write(&vec![0u8; BYTES_PER_SECOND]).unwrap();
        let writer = {
            let r = full.clone();
            std::thread::spawn(move || r.write(&[1u8; 10]))
        };
        let empty = Arc::new(Ring::new(BYTES_PER_SECOND, 1));
        let reader = {
            let r = empty.clone();
            std::thread::spawn(move || r.track(0).read(&mut [0u8; 10]).unwrap())
        };
        std::thread::sleep(Duration::from_millis(80));
        assert!(!writer.is_finished() && !reader.is_finished());
        full.abort();
        empty.abort();
        assert_eq!(writer.join().unwrap().unwrap_err(), Stopped::Aborted);
        assert_eq!(reader.join().unwrap(), 0, "an aborted ring ends the reading");
        // The consumer going away frees a writer that waits for room.
        let full = Arc::new(Ring::new(BYTES_PER_SECOND, 1));
        full.write(&vec![0u8; BYTES_PER_SECOND]).unwrap();
        let writer = {
            let r = full.clone();
            std::thread::spawn(move || r.write(&[1u8; 10]))
        };
        std::thread::sleep(Duration::from_millis(50));
        full.consumer_gone();
        assert_eq!(writer.join().unwrap().unwrap_err(), Stopped::ConsumerGone);
    }

    #[test]
    fn a_track_that_fits_goes_in_without_any_wait_while_nobody_reads() {
        let ring = Ring::new(BYTES_PER_SECOND * 10, 2);
        let track = pattern(BYTES_PER_SECOND * 6, 4);
        for chunk in track.chunks(63_504) {
            assert_eq!(ring.write(chunk).unwrap().0, Duration::ZERO);
        }
        ring.finish_track(0);
        // And the next one starts to come in over the border.
        ring.write(&pattern(BYTES_PER_SECOND * 3, 5)).unwrap();
        assert_eq!(ring.filled(), BYTES_PER_SECOND * 9);
        let mut got = Vec::new();
        ring.track(0).read_to_end(&mut got).unwrap();
        assert_eq!(got, track, "the first track ends at its mark, not at what came after");
        assert_eq!(ring.filled(), BYTES_PER_SECOND * 3);
    }
}
