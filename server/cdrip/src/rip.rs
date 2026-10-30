// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reading, verifying, encoding and tagging the tracks of one disc into an output folder.
//!
//! Rules: the helper writes only below `out_dir` (every file name is checked to stay inside),
//! every file appears under its final name only when complete (`.part` then rename), and the
//! PCM work files are removed whatever happens.

use std::fs::{self, File};
use std::io::{BufReader, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cd_da_reader::{AudioSectorReader, AudioTrackStream, CdReaderError, Toc, open_track_stream};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::drive::{ErrorClass, classify};
use crate::encode::{self, EncoderSettings, SECTOR_BYTES};
use crate::id3::{self, Tags};

/// The format the helper can write. The server's settings name the format; others are refused.
pub const SUPPORTED_FORMAT: &str = "mp3";
/// The tag version the helper can write.
pub const SUPPORTED_TAG_VERSION: &str = "2.4";

#[derive(Debug, Clone, Deserialize)]
pub struct EncoderSpec {
    pub format: String,
    pub bitrate_kbps: u16,
    pub sample_rate_hz: u32,
    pub channel_mode: String,
    pub quality: u8,
    pub tag_version: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrackSpec {
    pub number: u8,
    /// File name below `out_dir`.
    pub file: String,
    pub title: String,
    pub tag_track: u32,
}

/// A track that an earlier run finished (or encoded) and that this run takes over without reading it again.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ResumeTrack {
    pub number: u8,
    /// The SHA-1 of the PCM that was read.
    pub sha1: String,
    /// The file has its final name and was confirmed by a second reading; else it is `<file>.part` and still has to be
    /// confirmed (the second reading only compares with `sha1`).
    pub verified: bool,
}

/// The job description the app sends on standard input.
#[derive(Debug, Clone, Deserialize)]
pub struct JobSpec {
    pub device: String,
    pub out_dir: PathBuf,
    pub tracks: Vec<TrackSpec>,
    pub album: String,
    pub artist: String,
    #[serde(default)]
    pub genre: String,
    pub disc: u32,
    #[serde(default)]
    pub discs_total: Option<u32>,
    #[serde(default)]
    pub cover: Option<PathBuf>,
    #[serde(default = "yes")]
    pub verify: bool,
    #[serde(default = "three")]
    pub max_reads: u32,
    pub encoder: EncoderSpec,
    /// Tracks of an earlier run that are taken over (the files lie in `out_dir`).
    #[serde(default)]
    pub resume: Vec<ResumeTrack>,
    /// The size of the ring between the drive and the encoder, in bytes (the drive reads on, over the border of the tracks, until it
    /// is full). At least a second of audio.
    #[serde(default = "default_buffer")]
    pub buffer_bytes: u64,
    /// Measurements of the run: the filling of the ring, the time the drive read, the waits of the reader.
    #[serde(skip)]
    pub stats: Option<std::sync::Arc<RipStats>>,
    /// Sectors (0 to 26) by which the commands of the second read are shifted against those of the first, so that a fault that sits
    /// at the border of a command is not read the same way twice. 0 reads both times on the same grid. Further reads (when the two
    /// disagree) use other shifts that follow from this one.
    #[serde(default = "default_verify_shift")]
    pub verify_shift_sectors: u32,
    /// A track of which no two readings agree is taken with its first reading (and reported as `Unverified`) instead of ending the rip with
    /// `verify`. Off by default: a file that could not be confirmed may hold a flaw.
    #[serde(default)]
    pub accept_unverified: bool,
    /// Readings that differ only by a few frames lost or repeated (a drive that does not read a CD bit-exact) count as the same
    /// reading. Costs one more reading of a track, and only of a track whose first two readings differ.
    #[serde(default = "yes")]
    pub verify_tolerant: bool,
    /// The nice value (-20 = highest priority, 19 = lowest) the encoder thread asks for, so that the PCM leaves the ring as fast as the
    /// drive fills it. If the system does not allow the value, the nearest one it allows is used. `None` leaves the priority alone.
    #[serde(default)]
    pub encoder_nice: Option<i32>,
}

/// Raises the priority of the calling thread to `wanted` or, if the system refuses, to the nearest value it allows (a plain user may
/// go down to the limit of `RLIMIT_NICE`, see `LimitNICE=` of systemd). Gives the value that was set; `None` if nothing below the
/// current priority was possible. The priority of one thread only is changed (Linux takes the thread id for `PRIO_PROCESS`).
#[cfg(target_os = "linux")]
pub fn raise_thread_priority(wanted: i32) -> Option<i32> {
    // SAFETY: gettid and setpriority take and give plain integers.
    let tid = unsafe { libc::syscall(libc::SYS_gettid) } as libc::id_t;
    (wanted.clamp(-20, 19)..=0).find(|&n| unsafe { libc::setpriority(libc::PRIO_PROCESS, tid, n) } == 0)
}

#[cfg(not(target_os = "linux"))]
pub fn raise_thread_priority(_wanted: i32) -> Option<i32> {
    None
}

fn default_verify_shift() -> u32 {
    DEFAULT_VERIFY_SHIFT
}

fn default_buffer() -> u64 {
    u64::from(DEFAULT_BUFFER_SECONDS) * crate::ring::BYTES_PER_SECOND as u64
}

fn yes() -> bool {
    true
}
fn three() -> u32 {
    3
}

/// One line of the helper's output.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    TrackStart { track: u8 },
    Progress { track: u8, fraction: f64 },
    /// The track is whole under its final name (and, with verification, was read alike twice).
    TrackDone { track: u8, file: String, sha1: String, size: u64 },
    /// How far the reading of one track has come: `pass` 1 is the first reading, 2 the second (the confirmation), 3 and more the repeated
    /// readings after the first two did not agree; `fraction` is that of the track (0 to 1). For a list of the tracks with their state.
    TrackProgress { track: u8, pass: u8, fraction: f64 },
    /// The track is encoded and written under the temporary name `<file>.part`; with verification it still has to be
    /// confirmed by the second reading. `sha1` is that of the PCM that was read.
    Encoded { track: u8, sha1: String, size: u64 },
    /// No two of the readings of the track agreed, and the job says to take the first one all the same (`accept_unverified`): the track is
    /// filed, but it was not confirmed. Comes before its `TrackDone`.
    Unverified { track: u8 },
    /// Two readings of the track agree but for sectors that the drive could not read and guessed differently each time (a scratch): the track is
    /// filed with one of them; `sectors` is how many sectors differ.
    Concealed { track: u8, sectors: u32 },
    Error { kind: &'static str, message: String, track: Option<u8> },
    Done,
}

/// Why a rip stopped. `kind` is what the app switches on: `read`, `verify`, `drive_gone`, `encode`,
/// `disk_full`, `io`, `unsupported`, `cancelled`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RipError {
    pub kind: &'static str,
    pub message: String,
    pub track: Option<u8>,
}

impl RipError {
    fn new(kind: &'static str, message: impl Into<String>, track: Option<u8>) -> Self {
        RipError { kind, message: message.into(), track }
    }
}

/// The error a [`Guard`] reports instead of reading: the cancel flag was set, or the inner reader failed.
#[derive(Debug)]
pub enum Guarded<E> {
    Cancelled,
    Inner(E),
}

impl<E: std::fmt::Display> std::fmt::Display for Guarded<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Guarded::Cancelled => f.write_str("cancelled"),
            Guarded::Inner(e) => e.fmt(f),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for Guarded<E> {}

/// A reader that looks at the cancel flag before every command to the drive. Together with the
/// drive's own command timeout (30 s in `cd-da-reader`) this bounds how long a cancel can take:
/// the command in flight, at most. The retries of the crate stop at once, because each retry
/// goes through here.
struct Guard<'a, R> {
    inner: &'a R,
    cancel: &'a AtomicBool,
}

impl<R: AudioSectorReader> AudioSectorReader for Guard<'_, R> {
    type Error = Guarded<R::Error>;

    fn read_audio_sectors(&self, start_lba: u32, count: u32) -> Result<Vec<u8>, Self::Error> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(Guarded::Cancelled);
        }
        self.inner.read_audio_sectors(start_lba, count).map_err(Guarded::Inner)
    }
}

/// `disk_full` for a full disk, else `io`.
fn io_error(e: &std::io::Error, track: u8) -> RipError {
    let code = e.raw_os_error();
    // A full disk or a used-up quota; Windows says ERROR_DISK_FULL (112) and ERROR_HANDLE_DISK_FULL (39) and has no quota code in libc.
    #[cfg(unix)]
    let full = code == Some(libc::ENOSPC) || code == Some(libc::EDQUOT);
    #[cfg(not(unix))]
    let full = code == Some(libc::ENOSPC) || code == Some(112) || code == Some(39);
    let kind = if full { "disk_full" } else { "io" };
    RipError::new(kind, e.to_string(), Some(track))
}

fn cancelled(track: Option<u8>) -> RipError {
    RipError::new("cancelled", "cancelled", track)
}

fn from_reader(e: &CdReaderError, track: u8, cancel: &AtomicBool) -> RipError {
    if cancel.load(Ordering::Relaxed) {
        return cancelled(Some(track));
    }
    // A real drive: the error of the drive sits inside the guard.
    let class = match e {
        CdReaderError::Backend(b) => match b.downcast_ref::<Guarded<CdReaderError>>() {
            Some(Guarded::Inner(inner)) => classify(inner),
            _ => ErrorClass::Read,
        },
        other => classify(other),
    };
    let kind = match class {
        ErrorClass::DriveGone => "drive_gone",
        ErrorClass::Read => "read",
    };
    RipError::new(kind, e.to_string(), Some(track))
}

/// The file name as a relative path that cannot leave `out_dir`: normal components only.
fn safe_relative(name: &str) -> Option<PathBuf> {
    let p = Path::new(name);
    let ok = !name.is_empty() && p.components().all(|c| matches!(c, Component::Normal(_)));
    ok.then(|| p.to_path_buf())
}

/// Checks the whole job before the drive is touched.
pub fn validate(spec: &JobSpec) -> Result<(), RipError> {
    if spec.encoder.format != SUPPORTED_FORMAT {
        return Err(RipError::new("unsupported", format!("format \"{}\"", spec.encoder.format), None));
    }
    if spec.encoder.tag_version != SUPPORTED_TAG_VERSION {
        return Err(RipError::new("unsupported", format!("ID3 version \"{}\"", spec.encoder.tag_version), None));
    }
    if spec.verify_shift_sectors as usize >= CHUNK_SECTORS {
        return Err(RipError::new("unsupported", format!("a shift of {} sectors (0 to {})", spec.verify_shift_sectors, CHUNK_SECTORS - 1), None));
    }
    settings(spec).validate().map_err(|m| RipError::new("unsupported", m, None))?;
    for t in &spec.tracks {
        if safe_relative(&t.file).is_none() {
            return Err(RipError::new("io", format!("file name \"{}\" is not allowed", t.file), Some(t.number)));
        }
    }
    Ok(())
}

fn settings(spec: &JobSpec) -> EncoderSettings {
    EncoderSettings {
        bitrate_kbps: spec.encoder.bitrate_kbps,
        sample_rate_hz: spec.encoder.sample_rate_hz,
        channel_mode: spec.encoder.channel_mode.clone(),
        quality: spec.encoder.quality,
    }
}

/// A work file that is removed when dropped.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Sectors one command to the drive reads (`cd-da-reader` asks for 27 at most: 64 KiB is the limit of a `READ CD`).
pub const CHUNK_SECTORS: usize = 27;
/// How many sectors the commands of the second read of a track are shifted against those of the first (the first command of the second
/// read is this short). Half a command (27 sectors) puts every border of a command of one read into the middle of one of the other.
pub const DEFAULT_VERIFY_SHIFT: u32 = 13;
/// The default of the buffer: seconds of audio that may wait between the drive and the encoder (a setting of the server, 60 s are
/// about 10 MB). The drive reads on while the encoder works, and waits only when this much is not taken yet, so it is not stopped
/// and started again at every track.
pub const DEFAULT_BUFFER_SECONDS: u32 = 60;
/// How many finished tracks may wait for the writer; the encoder goes on with the next track at once (a few, not many: they are MP3s of a few MB).
pub const WRITE_QUEUE: usize = 2;
/// Seconds of audio that `bytes` of ring hold.
pub fn bytes_to_seconds(bytes: i64) -> f64 {
    bytes.max(0) as f64 / crate::ring::BYTES_PER_SECOND as f64
}

/// What encodes the PCM of a track: reads it to the end and returns the MP3 stream. The real one is [`encode::encode`].
pub type Encoder = dyn Fn(&mut dyn std::io::Read, &EncoderSettings) -> Result<Vec<u8>, String> + Sync;

fn real_encoder(input: &mut dyn std::io::Read, s: &EncoderSettings) -> Result<Vec<u8>, String> {
    encode::encode(input, s, |_| {})
}

pub use crate::ring::{RipStats, Ring};

fn hex(hash: Sha1) -> String {
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// The shift of read number `pass` (0 = the first read, which is not shifted): a multiple of `base` modulo the size of a command, never
/// the grid of the first read again unless `base` is 0.
pub fn pass_shift(base: u32, pass: u32) -> u32 {
    if base == 0 || pass == 0 {
        return 0;
    }
    match (base * pass) % CHUNK_SECTORS as u32 {
        0 => base,
        s => s,
    }
}

/// How two PCM work files compare.
enum Agreement {
    /// The same music, at most with a few frames lost or repeated.
    Same,
    /// The same music, but the drive could not read some sectors and guessed them differently each time (a scratch): the number of
    /// sectors that differ.
    Concealed(usize),
}

/// Whether two PCM work files carry the same music, allowing for the small shifts of a drive that does not read bit-exact and for the
/// guesses of a drive at a scratch.
fn compare_files(a: &Path, b: &Path) -> Option<Agreement> {
    let (Ok(a), Ok(b)) = (fs::read(a), fs::read(b)) else { return None };
    if crate::align::agree(&a, &b).is_some() {
        return Some(Agreement::Same);
    }
    crate::align::concealed_agree(&a, &b).map(Agreement::Concealed)
}

/// A track stream whose first command reads only `shift` sectors, so that every later command lies `shift` sectors off the grid of
/// an unshifted stream. Nothing outside the track is read.
struct Shifted<'a, R: AudioSectorReader> {
    stream: Option<AudioTrackStream<'a, R>>,
    pending: bool,
}

impl<'a, R: AudioSectorReader> Shifted<'a, R> {
    fn open(reader: &'a R, toc: &Toc, number: u8, shift: u32) -> Result<Self, CdReaderError> {
        let stream = open_track_stream(reader, toc, number)?;
        Ok(if shift == 0 { Shifted { stream: Some(stream), pending: false } } else { Shifted { stream: Some(stream.with_sectors_per_chunk(shift)), pending: true } })
    }

    fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, CdReaderError> {
        let got = self.stream.as_mut().expect("the stream is there").next_chunk();
        if self.pending && matches!(got, Ok(Some(_))) {
            self.pending = false;
            self.stream = self.stream.take().map(|s| s.with_sectors_per_chunk(CHUNK_SECTORS as u32));
        }
        got
    }

    fn total_sectors(&self) -> u32 {
        self.stream.as_ref().map_or(0, |s| s.total_sectors())
    }

    fn current_sector(&self) -> u32 {
        self.stream.as_ref().map_or(0, |s| s.current_sector())
    }
}

/// Sectors read from a far place of the disc before a track is read again (about 4 MB, more than the cache of a drive holds).
pub const CACHE_FLUSH_SECTORS: u32 = 1800;

/// Makes the drive forget the track it read last: a drive keeps the data it read in a cache, and a second reading of the same place
/// may be served from it, which confirms any fault of the first. Reading somewhere far away fills the cache with other data. The place
/// is the start of the track of the disc that lies farthest from `number` (the disc has one track only: nothing is done). What the
/// reads return is not used, and a failure does not matter.
fn flush_cache<R: AudioSectorReader>(reader: &R, toc: &Toc, number: u8) {
    let Some(here) = toc.tracks.iter().find(|t| t.number == number).map(|t| t.start_lba) else { return };
    let Some((i, far)) = toc.tracks.iter().enumerate().filter(|(_, t)| t.is_audio && t.number != number).max_by_key(|(_, t)| t.start_lba.abs_diff(here)) else { return };
    let end = toc.tracks.get(i + 1).map_or(toc.leadout_lba, |t| t.start_lba);
    let mut left = CACHE_FLUSH_SECTORS.min(end.saturating_sub(far.start_lba));
    let mut lba = far.start_lba;
    while left > 0 {
        let n = left.min(CHUNK_SECTORS as u32);
        if reader.read_audio_sectors(lba, n).is_err() {
            return;
        }
        lba += n;
        left -= n;
    }
}

/// Reads [`CACHE_FLUSH_SECTORS`] from `lba` and drops them: a far place for the drive to fill its cache with (for tools that read by hand).
pub fn read_far<R: AudioSectorReader>(reader: &R, lba: u32) -> usize {
    let mut got = 0;
    let mut at = lba;
    while got < CACHE_FLUSH_SECTORS as usize {
        let n = (CACHE_FLUSH_SECTORS as usize - got).min(CHUNK_SECTORS) as u32;
        if reader.read_audio_sectors(at, n).is_err() {
            break;
        }
        at += n;
        got += n as usize;
    }
    got
}

/// Reads a track again into a work file (the slow path: only after two reads disagreed); the SHA-1 of the PCM.
fn read_to_file<R: AudioSectorReader>(
    reader: &R,
    toc: &Toc,
    number: u8,
    pcm: &Path,
    cancel: &AtomicBool,
    shift: u32,
    stats: Option<&RipStats>,
    progress: &mut dyn FnMut(f64),
) -> Result<String, RipError> {
    let guard = Guard { inner: reader, cancel };
    // A repeated reading must come from the disc, not from what the drive kept of the one before.
    flush_cache(&guard, toc, number);
    let mut stream = Shifted::open(&guard, toc, number, shift).map_err(|e| from_reader(&e, number, cancel))?;
    let total = f64::from(stream.total_sectors().max(1));
    let mut last_report = -1.0f64;
    let mut file = std::io::BufWriter::with_capacity(1 << 20, File::create(pcm).map_err(|e| io_error(&e, number))?);
    let mut hash = Sha1::new();
    loop {
        let t0 = std::time::Instant::now();
        let next = stream.next_chunk();
        if let Some(st) = stats {
            st.read_ms.fetch_add(t0.elapsed().as_millis() as u64, Ordering::Relaxed);
        }
        match next {
            Ok(Some(chunk)) => {
                if let Some(st) = stats {
                    st.sectors_read.fetch_add((chunk.len() / SECTOR_BYTES) as u64, Ordering::Relaxed);
                }
                hash.update(&chunk);
                file.write_all(&chunk).map_err(|e| io_error(&e, number))?;
                let f = f64::from(stream.current_sector()) / total;
                if f - last_report >= 0.02 {
                    last_report = f;
                    progress(f.min(1.0));
                }
            }
            Ok(None) => break,
            Err(e) => return Err(from_reader(&e, number, cancel)),
        }
    }
    file.flush().map_err(|e| io_error(&e, number))?;
    Ok(hex(hash))
}

/// The ID3 tag and the MP3 stream of a track in one buffer.
fn tagged(mp3: Vec<u8>, spec: &JobSpec, t: &TrackSpec, cover: &Option<Vec<u8>>) -> Vec<u8> {
    let mut out = id3::build(&Tags {
        title: t.title.clone(),
        artist: spec.artist.clone(),
        album: spec.album.clone(),
        genre: spec.genre.clone(),
        track: t.tag_track,
        disc: spec.disc,
        discs_total: spec.discs_total,
        cover: cover.clone(),
    });
    out.extend(mp3);
    out
}

fn part_of(target: &Path) -> PathBuf {
    let mut s = target.to_path_buf().into_os_string();
    s.push(".part");
    PathBuf::from(s)
}

/// Writes in one piece, to disk, under the temporary name.
fn write_part(part: &Path, bytes: &[u8], track: u8) -> Result<(), RipError> {
    let mut f = File::create(part).map_err(|e| io_error(&e, track))?;
    f.write_all(bytes).map_err(|e| io_error(&e, track))?;
    f.sync_all().map_err(|e| io_error(&e, track))
}

/// The temporary names of the files. They are left alone when a rip ends badly: a track that was written whole is
/// worth keeping for a resume, and the caller removes the folder when it does not want them.
struct Parts(Vec<PathBuf>);

/// Share of the work that is reading (the rest: the last encoding and the placing).
const READ_SHARE: f64 = 0.97;

/// Rips every wanted track. Events go to `emit`. On error no temporary file stays; a file has its name only when it
/// is whole (and, with `verify`, only when every track was read twice alike).
///
/// The work is a pipeline so that the drive is kept reading:
/// * this thread reads the tracks one after the other (in the direction of the disc) and hands the PCM, chunk by chunk,
///   to a second thread that encodes and tags, and a third that writes; the PCM waits in a ring (`buffer_bytes`, made once) between
///   the first two, and the reader goes on over the border of a track, so
///   the drive is never held up by the encoder (unless the encoder is slower than the drive) and never reads far ahead;
/// * with `verify`, the second reading is a second pass over the tracks in the same direction, after the first pass
///   (one jump back to the start, not one per track), which only compares the SHA-1 of the PCM; a track whose two
///   hashes differ is read once more (up to `max_reads` in all) and the two reads that agree win, the MP3 is made
///   again from the work file if the first read was the wrong one.
pub fn rip_disc<R: AudioSectorReader>(reader: &R, toc: &Toc, spec: &JobSpec, cancel: &AtomicBool, emit: &mut dyn FnMut(&Event)) -> Result<(), RipError> {
    rip_disc_with(reader, toc, spec, cancel, emit, &real_encoder)
}

/// [`rip_disc`] with another encoder (the tests use a slow one).
pub fn rip_disc_with<R: AudioSectorReader>(reader: &R, toc: &Toc, spec: &JobSpec, cancel: &AtomicBool, emit: &mut dyn FnMut(&Event), encoder: &Encoder) -> Result<(), RipError> {
    rip_disc_with_parts(reader, toc, spec, cancel, emit, encoder, &write_part)
}

/// What writes a finished track to its temporary file. The real one is `write_part`.
pub type Writer = dyn Fn(&Path, &[u8], u8) -> Result<(), RipError> + Sync;

/// [`rip_disc_with`] with another writer as well (the tests use a slow one).
#[allow(clippy::too_many_arguments)]
pub fn rip_disc_with_parts<R: AudioSectorReader>(reader: &R, toc: &Toc, spec: &JobSpec, cancel: &AtomicBool, emit: &mut dyn FnMut(&Event), encoder: &Encoder, writer: &Writer) -> Result<(), RipError> {
    use std::sync::mpsc::channel;

    validate(spec)?;
    let cover = match &spec.cover {
        Some(p) => fs::read(p).ok().filter(|d| id3::picture_mime(d).is_some()),
        None => None,
    };
    let enc = settings(spec);
    let info = crate::drive::toc_info(toc);
    let lengths: std::collections::HashMap<u8, u32> = crate::drive::track_sectors(&info).into_iter().collect();
    let sectors_of = |n: u8| f64::from(lengths.get(&n).copied().unwrap_or(1).max(1));
    let total_sectors: f64 = spec.tracks.iter().map(|t| sectors_of(t.number)).sum::<f64>().max(1.0);
    let passes = if spec.verify { 2.0 } else { 1.0 };
    let max_reads = if spec.verify { spec.max_reads.max(2) } else { 1 };

    let targets: Vec<PathBuf> = spec.tracks.iter().map(|t| spec.out_dir.join(safe_relative(&t.file).expect("validated"))).collect();
    for (t, target) in spec.tracks.iter().zip(&targets) {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error(&e, t.number))?;
        }
    }
    let parts = Parts(targets.iter().map(|t| part_of(t)).collect());
    let scratch = Scratch(spec.out_dir.join(".reread.pcm.part"));
    let scratch2 = Scratch(spec.out_dir.join(".reread2.pcm.part"));

    let outcome = std::thread::scope(|scope| -> Result<(), RipError> {
        let ring = Arc::new(Ring::with_stats(spec.buffer_bytes.min(usize::MAX as u64) as usize, spec.tracks.len(), spec.stats.clone()));
        // If this function ends early (an error, a cancel), both sides of the ring are woken so that nothing waits for ever.
        struct Release(Arc<Ring>);
        impl Drop for Release {
            fn drop(&mut self) {
                self.0.abort();
            }
        }
        let _release = Release(ring.clone());
        let (job_tx, job_rx) = channel::<usize>();
        let (done_tx, done_rx) = channel::<(usize, Result<(), RipError>)>();
        let (enc_ref, cover_ref, parts_ref, specs) = (&enc, &cover, &parts.0, &spec.tracks);
        // Three threads: this one reads; one encodes (and tags) a track into memory and hands it on at once; one writes the files (`.part`,
        // sync) one after the other. The encoder takes the PCM of the next track from the ring without waiting for the disk, so the ring
        // is emptied also at the borders of the tracks.
        let (write_tx, write_rx) = std::sync::mpsc::sync_channel::<(usize, Result<Vec<u8>, RipError>)>(WRITE_QUEUE);
        scope.spawn(move || {
            for (i, bytes) in write_rx {
                let result = bytes.and_then(|b| writer(&parts_ref[i], &b, specs[i].number));
                let _ = done_tx.send((i, result));
            }
        });
        let enc_ring = ring.clone();
        let (nice, stats) = (spec.encoder_nice, spec.stats.clone());
        scope.spawn(move || {
            if let (Some(wanted), Some(st)) = (nice, &stats) {
                use std::sync::atomic::Ordering::Relaxed;
                match raise_thread_priority(wanted) {
                    Some(n) => {
                        st.encoder_nice.store(n, Relaxed);
                        st.encoder_nice_state.store(1, Relaxed);
                    }
                    None => st.encoder_nice_state.store(2, Relaxed),
                }
            }
            for i in job_rx {
                let t = &specs[i];
                let result = (|| {
                    let mut input = enc_ring.track(i);
                    let mp3 = encoder(&mut input, enc_ref).map_err(|m| RipError::new("encode", m, Some(t.number)))?;
                    // A track that was cut short by an abort is no track.
                    if enc_ring.is_aborted() {
                        return Err(RipError::new("cancelled", "the rip was stopped", Some(t.number)));
                    }
                    Ok(tagged(mp3, spec, t, cover_ref))
                })();
                if write_tx.send((i, result)).is_err() {
                    break;
                }
            }
            // Whatever the reason: a reader that waits for room does not wait for ever.
            enc_ring.consumer_gone();
        });

        let mut encoded = vec![false; spec.tracks.len()];
        let mut hashes: Vec<Vec<String>> = vec![Vec::new(); spec.tracks.len()];
        let mut published = vec![false; spec.tracks.len()];
        let mut fresh = vec![true; spec.tracks.len()];
        let mut sectors_done = 0.0f64;
        // What an earlier run did counts as done work.
        for (i, t) in spec.tracks.iter().enumerate() {
            if let Some(r) = spec.resume.iter().find(|r| r.number == t.number) {
                encoded[i] = true;
                fresh[i] = false;
                hashes[i].push(r.sha1.clone());
                if r.verified || !spec.verify {
                    sectors_done += sectors_of(t.number) * passes;
                } else {
                    sectors_done += sectors_of(t.number);
                }
                published[i] = r.verified;
            }
        }
        let mut last_reported = -1.0f64;
        let mut progress = |emit: &mut dyn FnMut(&Event), number: u8, add: f64| {
            sectors_done += add;
            let p = (sectors_done / (passes * total_sectors)).min(1.0) * READ_SHARE;
            if p - last_reported >= 0.01 {
                last_reported = p;
                emit(&Event::Progress { track: number, fraction: p });
            }
        };
        let size_of = |path: &Path| fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        // Takes in what the encoder reports (waiting for track `want` if it is named): a failure ends the rip.
        let collect = |encoded: &mut Vec<bool>, want: Option<usize>, emit: &mut dyn FnMut(&Event), hashes: &[Vec<String>]| -> Result<(), RipError> {
            loop {
                let got = match want {
                    Some(i) if !encoded[i] => Some(done_rx.recv().map_err(|_| RipError::new("encode", "the encoder stopped", None))?),
                    _ => done_rx.try_recv().ok(),
                };
                match got {
                    Some((i, r)) => {
                        r?;
                        encoded[i] = true;
                        emit(&Event::Encoded { track: spec.tracks[i].number, sha1: hashes[i].first().cloned().unwrap_or_default(), size: size_of(&parts.0[i]) });
                    }
                    None => return Ok(()),
                }
            }
        };
        // Without verification a track is finished when it is encoded: it gets its name then, never earlier.
        let publish = |encoded: &[bool], published: &mut Vec<bool>, emit: &mut dyn FnMut(&Event), hashes: &[Vec<String>]| -> Result<(), RipError> {
            for (i, t) in spec.tracks.iter().enumerate() {
                if encoded[i] && !published[i] {
                    fs::rename(&parts.0[i], &targets[i]).map_err(|e| io_error(&e, t.number))?;
                    published[i] = true;
                    emit(&Event::TrackDone { track: t.number, file: t.file.clone(), sha1: hashes[i].first().cloned().unwrap_or_default(), size: size_of(&targets[i]) });
                }
            }
            Ok(())
        };

        // Pass 1: read, hash, hand over to the encoder.
        for (i, t) in spec.tracks.iter().enumerate() {
            let number = t.number;
            if !fresh[i] {
                continue;
            }
            if cancel.load(Ordering::Relaxed) {
                return Err(cancelled(Some(number)));
            }
            emit(&Event::TrackStart { track: number });
            let guard = Guard { inner: reader, cancel };
            let mut stream = open_track_stream(&guard, toc, number).map_err(|e| from_reader(&e, number, cancel))?;
            job_tx.send(i).map_err(|_| RipError::new("encode", "the encoder stopped", Some(number)))?;
            let mut hash = Sha1::new();
            let mut last_sector = 0u32;
            let track_total = f64::from(stream.total_sectors().max(1));
            let mut last_track_report = -1.0f64;
            loop {
                let t0 = std::time::Instant::now();
                let next = stream.next_chunk();
                if let Some(st) = &spec.stats {
                    st.read_ms.fetch_add(t0.elapsed().as_millis() as u64, Ordering::Relaxed);
                }
                match next {
                    Ok(Some(chunk)) => {
                        if let Some(st) = &spec.stats {
                            st.sectors_read.fetch_add((chunk.len() / SECTOR_BYTES) as u64, Ordering::Relaxed);
                        }
                        hash.update(&chunk);
                        let now = stream.current_sector();
                        let step = f64::from(now.saturating_sub(last_sector));
                        last_sector = now;
                        let f = f64::from(now) / track_total;
                        if f - last_track_report >= 0.02 {
                            last_track_report = f;
                            emit(&Event::TrackProgress { track: number, pass: 1, fraction: f.min(1.0) });
                        }
                        // A full queue waits here: the encoder is slower than the drive.
                        // The reader writes into the ring and waits only when it is full (and the wait is measured by the ring).
                        if ring.write(&chunk).is_err() {
                            collect(&mut encoded, Some(i), emit, &hashes)?;
                            return Err(RipError::new("encode", "the encoder stopped", Some(number)));
                        }
                        progress(emit, number, step);
                        collect(&mut encoded, None, emit, &hashes)?;
                        if !spec.verify {
                            publish(&encoded, &mut published, emit, &hashes)?;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => return Err(from_reader(&e, number, cancel)),
                }
            }
            // The hash is known before the encoder can report the track.
            hashes[i].push(hex(hash));
            ring.finish_track(i);
        }
        ring.close();
        drop(job_tx);
        if !spec.verify {
            // The last tracks are still being encoded: wait for them (the drive has done its work).
            for i in 0..spec.tracks.len() {
                collect(&mut encoded, Some(i), emit, &hashes)?;
            }
            publish(&encoded, &mut published, emit, &hashes)?;
        }

        // Pass 2 (verify): the same tracks again in the same direction, hashes only.
        if spec.verify {
            for (i, t) in spec.tracks.iter().enumerate() {
                let number = t.number;
                if published[i] {
                    // Confirmed by an earlier run.
                    continue;
                }
                if cancel.load(Ordering::Relaxed) {
                    return Err(cancelled(Some(number)));
                }
                let guard = Guard { inner: reader, cancel };
                let mut stream = Shifted::open(&guard, toc, number, pass_shift(spec.verify_shift_sectors, 1)).map_err(|e| from_reader(&e, number, cancel))?;
                let mut hash = Sha1::new();
                let mut last_sector = 0u32;
                let track_total = f64::from(stream.total_sectors().max(1));
                let mut last_track_report = -1.0f64;
                loop {
                    let t0 = std::time::Instant::now();
                    let next = stream.next_chunk();
                    if let Some(st) = &spec.stats {
                        st.read_ms.fetch_add(t0.elapsed().as_millis() as u64, Ordering::Relaxed);
                    }
                    match next {
                        Ok(Some(chunk)) => {
                            if let Some(st) = &spec.stats {
                                st.sectors_read.fetch_add((chunk.len() / SECTOR_BYTES) as u64, Ordering::Relaxed);
                            }
                            hash.update(&chunk);
                            let now = stream.current_sector();
                            let step = f64::from(now.saturating_sub(last_sector));
                            last_sector = now;
                            let f = f64::from(now) / track_total;
                            if f - last_track_report >= 0.02 {
                                last_track_report = f;
                                emit(&Event::TrackProgress { track: number, pass: 2, fraction: f.min(1.0) });
                            }
                            progress(emit, number, step);
                            collect(&mut encoded, None, emit, &hashes)?;
                        }
                        Ok(None) => break,
                        Err(e) => return Err(from_reader(&e, number, cancel)),
                    }
                }
                hashes[i].push(hex(hash));
                collect(&mut encoded, Some(i), emit, &hashes)?;
                let mut accepted = hashes[i][0].clone();
                if hashes[i][0] != hashes[i][1] {
                    // The two reads disagree: read again until two agree. Two reads that differ only by a few frames lost or
                    // repeated (a drive that does not read a CD bit-exact) agree too, once there are two of them in files.
                    let mut found: Option<(String, PathBuf)> = None;
                    let mut files: Vec<PathBuf> = Vec::new();
                    for pass in 2..max_reads.max(4) {
                        // The reading that follows is number `pass + 1` of the track: it is shown with its progress, not as a silent wait.
                        let reading = (pass + 1) as u8;
                        emit(&Event::TrackProgress { track: number, pass: reading, fraction: 0.0 });
                        let file = if pass % 2 == 0 { scratch.0.clone() } else { scratch2.0.clone() };
                        let h = read_to_file(reader, toc, number, &file, cancel, pass_shift(spec.verify_shift_sectors, pass), spec.stats.as_deref(), &mut |f| {
                            emit(&Event::TrackProgress { track: number, pass: reading, fraction: f });
                        })?;
                        if hashes[i].contains(&h) {
                            found = Some((h, file));
                            break;
                        }
                        if let Some(prev) = files.last() {
                            match compare_files(prev, &file) {
                                Some(Agreement::Same) => {
                                    found = Some((h, file));
                                    break;
                                }
                                Some(Agreement::Concealed(sectors)) => {
                                    emit(&Event::Concealed { track: number, sectors: sectors as u32 });
                                    found = Some((h, file));
                                    break;
                                }
                                None => {}
                            }
                        }
                        hashes[i].push(h);
                        files.push(file);
                        // Without the tolerance the number of reads is `max_reads`.
                        if !spec.verify_tolerant && pass + 1 >= max_reads {
                            break;
                        }
                    }
                    let (good, good_file) = match found {
                        Some(g) => g,
                        // Taken as it is: the first reading is encoded and written already, so nothing has to be made again.
                        None if spec.accept_unverified => {
                            emit(&Event::Unverified { track: number });
                            (hashes[i][0].clone(), scratch.0.clone())
                        }
                        None => return Err(RipError::new("verify", format!("track {number} was not read the same way twice in {max_reads} tries"), Some(number))),
                    };
                    if good != hashes[i][0] {
                        // The first read was the wrong one: make the MP3 again from the PCM that was read twice.
                        let pcm = BufReader::new(File::open(&good_file).map_err(|e| io_error(&e, number))?);
                        let mp3 = encoder_from_file(encoder, pcm, &enc).map_err(|m| RipError::new("encode", m, Some(number)))?;
                        write_part(&parts.0[i], &tagged(mp3, spec, t, &cover), number)?;
                    }
                    accepted = good;
                }
                fs::rename(&parts.0[i], &targets[i]).map_err(|e| io_error(&e, number))?;
                published[i] = true;
                emit(&Event::TrackDone { track: number, file: t.file.clone(), sha1: accepted, size: size_of(&targets[i]) });
            }
        }
        Ok(())
    });
    outcome?;
    emit(&Event::Done);
    Ok(())
}

fn encoder_from_file(encoder: &Encoder, mut input: impl std::io::Read, s: &EncoderSettings) -> Result<Vec<u8>, String> {
    encoder(&mut input, s)
}

/// Bytes of PCM per second of audio, for sizes shown to the user.
pub const PCM_BYTES_PER_SECOND: usize = SECTOR_BYTES * 75;

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::{Cell, RefCell};

    use cd_da_reader::Track;

    use super::*;
    use crate::encode::tests::sine;

    #[derive(Debug)]
    pub(crate) struct FakeErr(String);
    impl std::fmt::Display for FakeErr {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.0)
        }
    }
    impl std::error::Error for FakeErr {}

    /// A disc in memory: 2 tracks of 3 and 4 seconds, with switches for the cases of a bad drive.
    pub(crate) struct FakeDisc {
        pcm: Vec<u8>,
        /// Sector from which reads fail with an error (None: never).
        pub(crate) fail_from: Cell<Option<u32>>,
        /// Reads of the first sector that return flipped data, once per number in this list.
        corrupt_reads: RefCell<Vec<u32>>,
        pub(crate) calls: Cell<u32>,
        /// The first sector of every read, in order.
        pub(crate) starts: RefCell<Vec<u32>>,
    }

    impl FakeDisc {
        pub(crate) fn new() -> Self {
            let mut pcm = sine(3.0, 440.0);
            pcm.truncate(pcm.len() / SECTOR_BYTES * SECTOR_BYTES);
            let mut second = sine(4.0, 660.0);
            second.truncate(second.len() / SECTOR_BYTES * SECTOR_BYTES);
            pcm.extend(second);
            FakeDisc { pcm, fail_from: Cell::new(None), corrupt_reads: RefCell::new(vec![]), calls: Cell::new(0), starts: RefCell::new(vec![]) }
        }
        pub(crate) fn toc(&self) -> Toc {
            let sectors = (self.pcm.len() / SECTOR_BYTES) as u32;
            let first = (3.0 * 44100.0 * 4.0 / SECTOR_BYTES as f32) as u32;
            Toc {
                first_track: 1,
                last_track: 2,
                tracks: vec![
                    Track { number: 1, start_lba: 0, start_msf: (0, 2, 0), is_audio: true },
                    Track { number: 2, start_lba: first, start_msf: (0, 2, 0), is_audio: true },
                ],
                leadout_lba: sectors,
            }
        }
    }

    impl AudioSectorReader for FakeDisc {
        type Error = FakeErr;
        fn read_audio_sectors(&self, start_lba: u32, count: u32) -> Result<Vec<u8>, FakeErr> {
            self.calls.set(self.calls.get() + 1);
            self.starts.borrow_mut().push(start_lba);
            if let Some(f) = self.fail_from.get()
                && start_lba + count > f
            {
                return Err(FakeErr("read error".into()));
            }
            let a = start_lba as usize * SECTOR_BYTES;
            let b = a + count as usize * SECTOR_BYTES;
            if b > self.pcm.len() {
                return Err(FakeErr("beyond the end of the disc".into()));
            }
            let mut v = self.pcm[a..b].to_vec();
            if start_lba == 0 && !self.corrupt_reads.borrow().is_empty() {
                self.corrupt_reads.borrow_mut().pop();
                v[100] ^= 0xFF;
            }
            Ok(v)
        }
    }

    pub(crate) fn spec(dir: &Path) -> JobSpec {
        JobSpec {
            device: "fake".into(),
            out_dir: dir.to_path_buf(),
            tracks: vec![
                TrackSpec { number: 1, file: "A_B_CD02_TR01.mp3".into(), title: "A_B_CD02_TR01".into(), tag_track: 201 },
                TrackSpec { number: 2, file: "A_B_CD02_TR02.mp3".into(), title: "A_B_CD02_TR02".into(), tag_track: 202 },
            ],
            album: "B".into(),
            artist: "A".into(),
            genre: "Audiobook".into(),
            disc: 2,
            discs_total: Some(12),
            cover: None,
            verify: true,
            max_reads: 3,
            encoder: EncoderSpec { format: "mp3".into(), bitrate_kbps: 64, sample_rate_hz: 24_000, channel_mode: "joint_stereo".into(), quality: 3, tag_version: "2.4".into() },
            resume: Vec::new(),
            buffer_bytes: 8 * crate::ring::BYTES_PER_SECOND as u64,
            stats: None,
            encoder_nice: None,
            verify_shift_sectors: DEFAULT_VERIFY_SHIFT,
            accept_unverified: false,
            verify_tolerant: true,
        }
    }

    /// The files with a final name (work files `.part` may stay when a rip ends badly: the caller decides).
    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| !n.ends_with(".part")).collect();
        v.sort();
        v
    }

    #[test]
    fn a_disc_becomes_tagged_mp3_files_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let disc = FakeDisc::new();
        let mut events = Vec::new();
        rip_disc(&disc, &disc.toc(), &spec(dir.path()), &AtomicBool::new(false), &mut |e| events.push(e.clone())).unwrap();

        assert_eq!(names(dir.path()), vec!["A_B_CD02_TR01.mp3", "A_B_CD02_TR02.mp3"], "no work files, no .part");
        assert_eq!(events.first(), Some(&Event::TrackStart { track: 1 }));
        assert_eq!(events.last(), Some(&Event::Done));
        assert!(events.iter().any(|e| matches!(e, Event::TrackDone { track: 2, file, size, .. } if file == "A_B_CD02_TR02.mp3" && *size > 0)));
        assert_eq!(events.iter().filter(|e| matches!(e, Event::Encoded { .. })).count(), 2, "every track is reported once it is encoded");
        // The progress is that of the whole job (both passes): it never goes backwards and ends near the end.
        let p: Vec<f64> = events.iter().filter_map(|e| if let Event::Progress { fraction, .. } = e { Some(*fraction) } else { None }).collect();
        assert!(p.windows(2).all(|w| w[0] <= w[1]), "{p:?}");
        assert!(p.last().unwrap() > &0.9, "{p:?}");

        let bytes = fs::read(dir.path().join("A_B_CD02_TR02.mp3")).unwrap();
        let frames = id3::tests::read_frames(&bytes[..bytes.len().min(2000)]);
        let get = |id: &str| String::from_utf8(frames.iter().find(|(i, _)| i == id).unwrap().1[1..].to_vec()).unwrap();
        assert_eq!((get("TIT2").as_str(), get("TRCK").as_str(), get("TPOS").as_str()), ("A_B_CD02_TR02", "202", "2/12"));
        let (rate, kbps, _) = encode::first_frame(&bytes).unwrap();
        assert_eq!((rate, kbps), (24_000, 64));
    }

    #[test]
    fn cover_is_embedded_when_it_is_a_picture() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        fs::create_dir(&out).unwrap();
        let cover = dir.path().join("cover.jpg");
        fs::write(&cover, [0xFF, 0xD8, 0xFF, 0xE0, 7, 7, 7]).unwrap();
        let mut s = spec(&out);
        s.cover = Some(cover);
        let disc = FakeDisc::new();
        rip_disc(&disc, &disc.toc(), &s, &AtomicBool::new(false), &mut |_| {}).unwrap();
        let bytes = fs::read(out.join("A_B_CD02_TR01.mp3")).unwrap();
        assert!(bytes.windows(10).any(|w| w == b"image/jpeg"));
    }

    #[test]
    fn a_read_that_differs_once_is_read_again() {
        let dir = tempfile::tempdir().unwrap();
        let disc = FakeDisc::new();
        disc.corrupt_reads.borrow_mut().push(1); // the first pass of track 1 is wrong
        rip_disc(&disc, &disc.toc(), &spec(dir.path()), &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(names(dir.path()).len(), 2);
        // The wrong first read did not get into the file: the result is the same as that of a clean disc.
        let clean_dir = tempfile::tempdir().unwrap();
        let clean = FakeDisc::new();
        rip_disc(&clean, &clean.toc(), &spec(clean_dir.path()), &AtomicBool::new(false), &mut |_| {}).unwrap();
        for n in names(dir.path()) {
            assert_eq!(fs::read(dir.path().join(&n)).unwrap(), fs::read(clean_dir.path().join(&n)).unwrap(), "{n}");
        }
    }

    #[test]
    fn a_repeated_reading_is_reported_with_its_progress_and_not_as_a_silent_wait() {
        let dir = tempfile::tempdir().unwrap();
        let disc = FakeDisc::new();
        disc.corrupt_reads.borrow_mut().push(1); // the first pass of track 1 is wrong: the second does not agree, a third is made
        let mut events = Vec::new();
        rip_disc(&disc, &disc.toc(), &spec(dir.path()), &AtomicBool::new(false), &mut |e| events.push(e.clone())).unwrap();
        let third: Vec<f64> = events.iter().filter_map(|e| if let Event::TrackProgress { track: 1, pass: 3, fraction } = e { Some(*fraction) } else { None }).collect();
        assert!(third.first() == Some(&0.0) && third.last().is_some_and(|f| *f > 0.9), "the third reading of track 1 is reported from 0 to the end: {third:?}");
        assert!(third.windows(2).all(|w| w[0] <= w[1]), "and only forwards: {third:?}");
        // A clean disc has no such reading.
        let clean_dir = tempfile::tempdir().unwrap();
        let clean = FakeDisc::new();
        let mut clean_events = Vec::new();
        rip_disc(&clean, &clean.toc(), &spec(clean_dir.path()), &AtomicBool::new(false), &mut |e| clean_events.push(e.clone())).unwrap();
        assert!(!clean_events.iter().any(|e| matches!(e, Event::TrackProgress { pass: 3.., .. })));
    }

    #[test]
    fn the_sectors_that_the_drive_delivered_are_counted_for_the_speed_it_reads_at() {
        let dir = tempfile::tempdir().unwrap();
        let disc = FakeDisc::new();
        let stats = Arc::new(RipStats::default());
        let mut s = spec(dir.path());
        s.stats = Some(stats.clone());
        rip_disc(&disc, &disc.toc(), &s, &AtomicBool::new(false), &mut |_| {}).unwrap();
        let sectors = (disc.pcm.len() / SECTOR_BYTES) as u64;
        // With the check every track is read twice.
        assert_eq!(stats.sectors_read.load(Ordering::Relaxed), 2 * sectors);
        // A repeated reading is counted as well: the one wrong first read of track 1 makes it three readings of that track.
        let again = FakeDisc::new();
        again.corrupt_reads.borrow_mut().push(1);
        let stats = Arc::new(RipStats::default());
        let dir2 = tempfile::tempdir().unwrap();
        let mut s = spec(dir2.path());
        s.stats = Some(stats.clone());
        rip_disc(&again, &again.toc(), &s, &AtomicBool::new(false), &mut |_| {}).unwrap();
        let first_track = u64::from(again.toc().tracks[1].start_lba);
        assert_eq!(stats.sectors_read.load(Ordering::Relaxed), 2 * sectors + first_track);
    }

    #[test]
    fn the_shift_of_a_read_follows_from_the_base_and_is_never_the_grid_of_the_first_read() {
        assert_eq!(pass_shift(13, 0), 0, "the first read is not shifted");
        assert_eq!((pass_shift(13, 1), pass_shift(13, 2), pass_shift(13, 3)), (13, 26, 12));
        assert_eq!(pass_shift(0, 1), 0, "a base of 0 leaves every read on the same grid");
        for base in 1..CHUNK_SECTORS as u32 {
            for pass in 1..30 {
                let sh = pass_shift(base, pass);
                assert!(sh > 0 && (sh as usize) < CHUNK_SECTORS, "base {base}, pass {pass}: {sh}");
            }
        }
    }

    #[test]
    fn the_second_read_has_its_commands_off_the_grid_of_the_first() {
        for (shift, off_grid) in [(0u32, false), (13, true)] {
            let dir = tempfile::tempdir().unwrap();
            let disc = FakeDisc::new();
            let mut s = spec(dir.path());
            s.verify = true;
            s.verify_shift_sectors = shift;
            rip_disc(&disc, &disc.toc(), &s, &AtomicBool::new(false), &mut |_| {}).unwrap();
            let second_track = disc.toc().tracks[1].start_lba;
            let starts: Vec<u32> = disc.starts.borrow().iter().copied().filter(|s| *s < second_track).collect();
            let shifted = starts.iter().filter(|s| *s % CHUNK_SECTORS as u32 == shift).count();
            assert_eq!(shifted > 0 && shift > 0, off_grid, "shift {shift}: starts {starts:?}");
            assert!(starts.iter().all(|s| *s % CHUNK_SECTORS as u32 == 0 || shift > 0), "without a shift every command is on the grid: {starts:?}");
            if shift > 0 {
                assert!(starts.contains(&shift), "the first command of the second read is {shift} sectors long: {starts:?}");
            }
        }
    }

    #[test]
    fn a_fault_at_the_border_of_the_commands_is_found_only_when_the_second_read_is_shifted() {
        /// A drive that damages the first sector of every command it gets, the same way every time.
        struct BorderFault<'a>(&'a FakeDisc);
        impl AudioSectorReader for BorderFault<'_> {
            type Error = FakeErr;
            fn read_audio_sectors(&self, s: u32, c: u32) -> Result<Vec<u8>, FakeErr> {
                let mut v = self.0.read_audio_sectors(s, c)?;
                v[7] ^= 0x55;
                Ok(v)
            }
        }
        for (shift, found) in [(0u32, false), (13, true)] {
            let dir = tempfile::tempdir().unwrap();
            let disc = FakeDisc::new();
            let mut s = spec(dir.path());
            s.verify = true;
            s.verify_shift_sectors = shift;
            let result = rip_disc(&BorderFault(&disc), &disc.toc(), &s, &AtomicBool::new(false), &mut |_| {});
            match (found, result) {
                (false, r) => assert!(r.is_ok(), "on the same grid both reads are wrong in the same way and agree: {r:?}"),
                (true, Err(e)) => assert_eq!(e.kind, "verify", "{e:?}"),
                (true, Ok(())) => panic!("a fault at the borders of the commands went through with a shifted second read"),
            }
        }
    }

    #[test]
    fn reads_that_never_agree_are_taken_with_the_first_reading_and_reported_when_the_job_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let disc = FakeDisc::new();
        // Every reading of the first sector of track 1 differs from the one before (see the test that refuses it).
        struct Flaky<'a>(&'a FakeDisc, Cell<u8>);
        impl AudioSectorReader for Flaky<'_> {
            type Error = FakeErr;
            fn read_audio_sectors(&self, s: u32, c: u32) -> Result<Vec<u8>, FakeErr> {
                let mut v = self.0.read_audio_sectors(s, c)?;
                if s == 0 {
                    self.1.set(self.1.get().wrapping_add(1));
                    v[11] = self.1.get().wrapping_mul(40); // steps of 10240: not a guess of a drive, garbage
                }
                Ok(v)
            }
        }
        let flaky = Flaky(&disc, Cell::new(0));
        let mut s = spec(dir.path());
        s.accept_unverified = true;
        let mut events = Vec::new();
        rip_disc(&flaky, &disc.toc(), &s, &AtomicBool::new(false), &mut |e| events.push(e.clone())).unwrap();
        assert_eq!(names(dir.path()).len(), 2, "both tracks are filed: {:?}", names(dir.path()));
        assert!(events.contains(&Event::Unverified { track: 1 }), "track 1 is reported as not confirmed");
        assert!(!events.iter().any(|e| matches!(e, Event::Unverified { track: 2 })), "track 2 was confirmed");
        let at = |f: &dyn Fn(&Event) -> bool| events.iter().position(|e| f(e)).unwrap();
        assert!(at(&|e| matches!(e, Event::Unverified { track: 1 })) < at(&|e| matches!(e, Event::TrackDone { track: 1, .. })), "the report comes before the end of the track");
        assert_eq!(events.last(), Some(&Event::Done));
    }

    /// A drive that reads every reading of track 1 a few frames further on than the one before (it does not read bit-exact).
    struct Sliding<'a>(&'a FakeDisc, Cell<u32>);
    impl AudioSectorReader for Sliding<'_> {
        type Error = FakeErr;
        fn read_audio_sectors(&self, s: u32, c: u32) -> Result<Vec<u8>, FakeErr> {
            if s == 0 {
                self.1.set(self.1.get() + 1);
            }
            let skip = (self.1.get().saturating_sub(1) * 4) as usize;
            let mut v = self.0.read_audio_sectors(s, c)?;
            if skip > 0 {
                let next = self.0.read_audio_sectors(s + c, 1).unwrap_or_else(|_| vec![0; SECTOR_BYTES]);
                v.extend_from_slice(&next[..skip.min(next.len())]);
                v.drain(..skip);
            }
            Ok(v)
        }
    }

    #[test]
    fn readings_that_differ_only_by_a_few_frames_confirm_each_other() {
        let dir = tempfile::tempdir().unwrap();
        let disc = FakeDisc::new();
        // Readings 1 to 4 of track 1 start 0, 1, 2 and 3 frames late: no two have the same hash, the last two are the same music.
        let sliding = Sliding(&disc, Cell::new(0));
        let mut events = Vec::new();
        rip_disc(&sliding, &disc.toc(), &spec(dir.path()), &AtomicBool::new(false), &mut |e| events.push(e.clone())).unwrap();
        assert_eq!(names(dir.path()).len(), 2, "both tracks are filed: {:?}", names(dir.path()));
        assert!(!events.iter().any(|e| matches!(e, Event::Unverified { .. })), "the track is confirmed, not taken on trust");
        assert!(events.iter().any(|e| matches!(e, Event::TrackProgress { track: 1, pass: 4, .. })), "a fourth reading was needed");
        assert!(!dir.path().join(".reread.pcm.part").exists() && !dir.path().join(".reread2.pcm.part").exists(), "work files are removed");
    }

    #[test]
    fn without_the_tolerance_readings_that_slip_do_not_confirm_each_other() {
        let dir = tempfile::tempdir().unwrap();
        let disc = FakeDisc::new();
        let sliding = Sliding(&disc, Cell::new(0));
        let mut s = spec(dir.path());
        s.verify_tolerant = false;
        let err = rip_disc(&sliding, &disc.toc(), &s, &AtomicBool::new(false), &mut |_| {}).unwrap_err();
        assert_eq!((err.kind, err.track), ("verify", Some(1)));
    }

    #[test]
    fn a_track_that_is_read_again_is_read_after_a_far_place_of_the_disc_has_been_read() {
        let dir = tempfile::tempdir().unwrap();
        let disc = FakeDisc::new();
        let toc = disc.toc();
        let second = toc.tracks[1].start_lba;
        let pcm = dir.path().join("x.pcm");
        disc.starts.borrow_mut().clear();
        read_to_file(&disc, &toc, 1, &pcm, &AtomicBool::new(false), 0, None, &mut |_| {}).unwrap();
        let starts = disc.starts.borrow().clone();
        assert_eq!(starts.first(), Some(&second), "the first command goes to track 2, the far track: {starts:?}");
        assert!(starts.iter().skip(1).any(|s| *s == 0), "then track 1 is read from its start: {starts:?}");
        // The same for track 2, whose far place is track 1.
        disc.starts.borrow_mut().clear();
        read_to_file(&disc, &toc, 2, &pcm, &AtomicBool::new(false), 0, None, &mut |_| {}).unwrap();
        assert_eq!(disc.starts.borrow().first(), Some(&0));
    }

    /// A drive at a scratch: the sectors 3 to 5 of track 1 come back with other small guesses at every reading.
    struct Concealing<'a>(&'a FakeDisc, Cell<u8>);
    impl AudioSectorReader for Concealing<'_> {
        type Error = FakeErr;
        fn read_audio_sectors(&self, s: u32, c: u32) -> Result<Vec<u8>, FakeErr> {
            let mut v = self.0.read_audio_sectors(s, c)?;
            if s == 0 {
                self.1.set(self.1.get().wrapping_add(1));
            }
            for k in 0..c {
                if (3..6).contains(&(s + k)) {
                    let at = k as usize * SECTOR_BYTES + 400;
                    v[at] = v[at].wrapping_add(self.1.get());
                    v[at + 1] = v[at + 1] & 0x0F;
                }
            }
            Ok(v)
        }
    }

    #[test]
    fn readings_that_differ_only_where_the_drive_guessed_are_filed_and_the_guess_is_told() {
        let dir = tempfile::tempdir().unwrap();
        let disc = FakeDisc::new();
        let flaky = Concealing(&disc, Cell::new(0));
        let mut events = Vec::new();
        rip_disc(&flaky, &disc.toc(), &spec(dir.path()), &AtomicBool::new(false), &mut |e| events.push(e.clone())).unwrap();
        assert_eq!(names(dir.path()).len(), 2, "both tracks are filed: {:?}", names(dir.path()));
        assert!(events.iter().any(|e| matches!(e, Event::Concealed { track: 1, sectors } if *sectors >= 1)), "{events:?}");
        assert!(!events.iter().any(|e| matches!(e, Event::Unverified { .. })), "it is not taken on trust");
        assert_eq!(events.last(), Some(&Event::Done));
    }

    #[test]
    fn reads_that_never_agree_fail_the_track_and_leave_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let disc = FakeDisc::new();
        disc.corrupt_reads.borrow_mut().extend([1, 2, 3, 4, 5, 6, 7, 8]);
        // Every read of the first sector flips a byte; because the stream reads that sector again,
        // hashes differ only if the flip is applied differently, so make each pass differ:
        struct Flaky<'a>(&'a FakeDisc, Cell<u8>);
        impl AudioSectorReader for Flaky<'_> {
            type Error = FakeErr;
            fn read_audio_sectors(&self, s: u32, c: u32) -> Result<Vec<u8>, FakeErr> {
                let mut v = self.0.read_audio_sectors(s, c)?;
                if s == 0 {
                    self.1.set(self.1.get().wrapping_add(1));
                    v[11] = self.1.get().wrapping_mul(40); // steps of 10240: not a guess of a drive, garbage
                }
                Ok(v)
            }
        }
        let flaky = Flaky(&disc, Cell::new(0));
        let mut events = Vec::new();
        let err = rip_disc(&flaky, &disc.toc(), &spec(dir.path()), &AtomicBool::new(false), &mut |e| events.push(e.clone())).unwrap_err();
        assert_eq!((err.kind, err.track), ("verify", Some(1)));
        assert!(names(dir.path()).is_empty(), "nothing half-done: {:?}", names(dir.path()));
        assert!(!events.contains(&Event::Done));
    }

    #[test]
    fn an_unreadable_sector_stops_the_rip_and_leaves_no_half_file() {
        let dir = tempfile::tempdir().unwrap();
        let disc = FakeDisc::new();
        let first = disc.toc().tracks[1].start_lba;
        disc.fail_from.set(Some(first + 10));
        let err = rip_disc(&disc, &disc.toc(), &spec(dir.path()), &AtomicBool::new(false), &mut |_| {}).unwrap_err();
        assert_eq!((err.kind, err.track), ("read", Some(2)));
        assert!(names(dir.path()).is_empty(), "with verification nothing is named before every track was read twice: {:?}", names(dir.path()));
        // Without verification too: whatever was not named by then (encoding runs beside the reading) is removed.
        let dir2 = tempfile::tempdir().unwrap();
        let mut s = spec(dir2.path());
        s.verify = false;
        let err = rip_disc(&disc, &disc.toc(), &s, &AtomicBool::new(false), &mut |_| {}).unwrap_err();
        assert_eq!((err.kind, err.track), ("read", Some(2)));
        assert!(names(dir2.path()).iter().all(|n| n == "A_B_CD02_TR01.mp3"), "only a whole file may stay: {:?}", names(dir2.path()));
    }

    /// A disc of one long track that reads quickly, to watch how far the drive runs ahead of the encoder.
    struct LongDisc {
        sectors: u32,
        reads: Cell<usize>,
        consumed: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        max_lead: Cell<usize>,
        starts: RefCell<Vec<u32>>,
    }

    impl LongDisc {
        fn new(sectors: u32) -> Self {
            LongDisc { sectors, reads: Cell::new(0), consumed: Default::default(), max_lead: Cell::new(0), starts: RefCell::new(vec![]) }
        }
        fn toc(&self) -> Toc {
            Toc { first_track: 1, last_track: 1, tracks: vec![Track { number: 1, start_lba: 0, start_msf: (0, 2, 0), is_audio: true }], leadout_lba: self.sectors }
        }
        fn spec(&self, dir: &Path, verify: bool) -> JobSpec {
            let mut s = spec(dir);
            s.tracks.truncate(1);
            s.verify = verify;
            s
        }
    }

    impl AudioSectorReader for LongDisc {
        type Error = FakeErr;
        fn read_audio_sectors(&self, start_lba: u32, count: u32) -> Result<Vec<u8>, FakeErr> {
            self.starts.borrow_mut().push(start_lba);
            self.reads.set(self.reads.get() + 1);
            let lead = self.reads.get().saturating_sub(self.consumed.load(Ordering::SeqCst));
            self.max_lead.set(self.max_lead.get().max(lead));
            Ok(vec![0u8; count as usize * SECTOR_BYTES])
        }
    }

    /// An encoder that is slow: it takes a while for every chunk it is given.
    fn slow(consumed: std::sync::Arc<std::sync::atomic::AtomicUsize>, pause_ms: u64) -> impl Fn(&mut dyn std::io::Read, &EncoderSettings) -> Result<Vec<u8>, String> + Sync {
        move |input, _| {
            let mut buf = vec![0u8; CHUNK_SECTORS * SECTOR_BYTES];
            loop {
                let n = std::io::Read::read(input, &mut buf).map_err(|e| e.to_string())?;
                if n == 0 {
                    return Ok(Vec::new());
                }
                consumed.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(pause_ms));
            }
        }
    }

    /// Chunks that a ring of this many bytes holds.
    fn ring_chunks(bytes: u64) -> usize {
        bytes as usize / (CHUNK_SECTORS * SECTOR_BYTES)
    }

    #[test]
    fn the_drive_reads_on_while_the_encoder_works_and_waits_only_when_the_ring_is_full() {
        let dir = tempfile::tempdir().unwrap();
        let disc = LongDisc::new(27 * 150); // 150 chunks, 54 seconds
        let encoder = slow(disc.consumed.clone(), 3);
        let started = std::time::Instant::now();
        let spec = disc.spec(dir.path(), false); // a ring of 8 seconds
        let rc = ring_chunks(spec.buffer_bytes);
        rip_disc_with(&disc, &disc.toc(), &spec, &AtomicBool::new(false), &mut |_| {}, &encoder).unwrap();
        // The drive ran ahead until the ring was full (a chunk in the reader's hand and one in the encoder's make two more) ...
        assert!(disc.max_lead.get() + 1 >= rc, "ran ahead only {} chunks of {rc}", disc.max_lead.get());
        // ... and not further: a full ring holds it back.
        assert!(disc.max_lead.get() <= rc + 3, "ran ahead {} chunks, more than the ring holds ({rc})", disc.max_lead.get());
        assert_eq!(disc.reads.get(), 150);
        // Reading and encoding overlapped (the encoder alone takes 150 x 3 ms).
        assert!(started.elapsed() < std::time::Duration::from_millis(150 * 3 + 600), "{:?}", started.elapsed());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_encoder_thread_asks_for_a_higher_priority_and_gets_the_nearest_one_the_system_allows() {
        use std::sync::atomic::Ordering::Relaxed;
        let run = |nice: Option<i32>| {
            let dir = tempfile::tempdir().unwrap();
            let disc = LongDisc::new(27 * 20);
            let encoder = slow(disc.consumed.clone(), 0);
            let mut s = disc.spec(dir.path(), false);
            s.encoder_nice = nice;
            let stats = Arc::new(RipStats::default());
            s.stats = Some(stats.clone());
            rip_disc_with(&disc, &disc.toc(), &s, &AtomicBool::new(false), &mut |_| {}, &encoder).unwrap();
            (stats.encoder_nice_state.load(Relaxed), stats.encoder_nice.load(Relaxed))
        };
        // Nothing asked: the priority is left alone.
        assert_eq!(run(None).0, 0);
        // The highest priority: a plain user may not have it, then the nearest allowed value (at best the one it started with) is used.
        let (state, got) = run(Some(-20));
        assert!(state == 1 && (-20..=0).contains(&got), "state {state}, nice {got}");
        // A thread of its own: the test thread keeps its priority (the process was not changed).
        let tid = unsafe { libc::syscall(libc::SYS_gettid) } as libc::id_t;
        let mine = unsafe { libc::getpriority(libc::PRIO_PROCESS, tid) };
        assert!(mine >= got, "the calling thread has nice {mine}, the encoder {got}");
    }

    #[test]
    fn the_size_of_the_ring_is_what_the_job_says_and_a_track_that_fits_is_pumped_in_without_a_wait() {
        for seconds in [2u64, 8, 16] {
            let dir = tempfile::tempdir().unwrap();
            let disc = LongDisc::new(27 * 400); // 144 seconds of audio in one track
            let encoder = slow(disc.consumed.clone(), 2);
            let mut s = disc.spec(dir.path(), false);
            s.buffer_bytes = seconds * crate::ring::BYTES_PER_SECOND as u64;
            let stats = Arc::new(RipStats::default());
            s.stats = Some(stats.clone());
            rip_disc_with(&disc, &disc.toc(), &s, &AtomicBool::new(false), &mut |_| {}, &encoder).unwrap();
            let rc = ring_chunks(s.buffer_bytes);
            assert!(disc.max_lead.get() + 1 >= rc && disc.max_lead.get() <= rc + 3, "{seconds} s: ran ahead {} of {rc}", disc.max_lead.get());
            assert_eq!(stats.filled_bytes.load(Ordering::Relaxed), 0, "the ring is empty at the end");
            let peak = stats.peak_bytes.load(Ordering::Relaxed) as u64;
            assert!(peak <= s.buffer_bytes && peak + 2 * 63_504 >= s.buffer_bytes, "{seconds} s: the ring got full ({peak} of {})", s.buffer_bytes);
            assert!(stats.reader_waits.load(Ordering::Relaxed) > 0, "the track does not fit: the reader waited");
            assert!(stats.read_ms.load(Ordering::Relaxed) < 5_000);
        }
        // A track that fits whole: the reader never waits, however slow the encoder is.
        let dir = tempfile::tempdir().unwrap();
        let disc = LongDisc::new(27 * 100); // 36 seconds
        let encoder = slow(disc.consumed.clone(), 5);
        let mut s = disc.spec(dir.path(), false);
        s.buffer_bytes = 40 * crate::ring::BYTES_PER_SECOND as u64;
        let stats = Arc::new(RipStats::default());
        s.stats = Some(stats.clone());
        let started = std::time::Instant::now();
        let mut read_done_at = None;
        rip_disc_with(&disc, &disc.toc(), &s, &AtomicBool::new(false), &mut |e| {
            if matches!(e, Event::Progress { fraction, .. } if *fraction > 0.9) && read_done_at.is_none() {
                read_done_at = Some(started.elapsed());
            }
        }, &encoder)
        .unwrap();
        assert_eq!(stats.reader_waits.load(Ordering::Relaxed), 0);
        assert_eq!(stats.wait_ms_longest.load(Ordering::Relaxed), 0);
        assert!(read_done_at.unwrap() < std::time::Duration::from_millis(100 * 5 / 2), "the whole track was read before the encoder had done half of it: {:?}", read_done_at);
    }

    /// Three tracks of 10 seconds with a ring of 12 seconds and a writer that takes long at the border of every track.
    #[test]
    fn a_slow_writer_at_the_border_of_a_track_does_not_hold_the_reader_up() {
        struct Three {
            reads: Cell<usize>,
        }
        impl AudioSectorReader for Three {
            type Error = FakeErr;
            fn read_audio_sectors(&self, _s: u32, c: u32) -> Result<Vec<u8>, FakeErr> {
                self.reads.set(self.reads.get() + 1);
                Ok(vec![0u8; c as usize * SECTOR_BYTES])
            }
        }
        let sectors = 750u32; // 10 seconds
        let toc = Toc {
            first_track: 1,
            last_track: 3,
            tracks: (0..3).map(|i| Track { number: i as u8 + 1, start_lba: i * sectors, start_msf: (0, 2, 0), is_audio: true }).collect(),
            leadout_lba: 3 * sectors,
        };
        let dir = tempfile::tempdir().unwrap();
        let mut s = spec(dir.path());
        s.verify = false;
        s.tracks = (1..=3).map(|n| TrackSpec { number: n, file: format!("{n}.mp3"), title: String::new(), tag_track: u32::from(n) }).collect();
        s.buffer_bytes = 12 * crate::ring::BYTES_PER_SECOND as u64;
        let stats = Arc::new(RipStats::default());
        s.stats = Some(stats.clone());
        // The encoder is fast; the writer takes 400 ms for every file.
        let fast = |input: &mut dyn std::io::Read, _: &EncoderSettings| -> Result<Vec<u8>, String> {
            let mut sink = Vec::new();
            input.read_to_end(&mut sink).map_err(|e| e.to_string())?;
            Ok(vec![1u8; 100])
        };
        let writes = Arc::new(std::sync::Mutex::new(Vec::new()));
        let writes_in = writes.clone();
        let slow_writer = move |path: &Path, bytes: &[u8], track: u8| -> Result<(), RipError> {
            std::thread::sleep(std::time::Duration::from_millis(400));
            writes_in.lock().unwrap().push(track);
            fs::write(path, bytes).map_err(|e| io_error(&e, track))
        };
        let disc = Three { reads: Cell::new(0) };
        let started = std::time::Instant::now();
        let mut done = Vec::new();
        rip_disc_with_parts(&disc, &toc, &s, &AtomicBool::new(false), &mut |e| {
            if let Event::TrackDone { track, .. } = e {
                done.push(*track);
            }
        }, &fast, &slow_writer)
        .unwrap();
        // The reader was never held up by the writer (the ring is emptied by the encoder at once), and the order of the tracks holds.
        assert_eq!(stats.reader_waits.load(Ordering::Relaxed), 0, "the reader waited for the ring although the encoder is fast");
        assert_eq!(*writes.lock().unwrap(), vec![1, 2, 3]);
        assert_eq!(done, vec![1, 2, 3]);
        assert!(started.elapsed() >= std::time::Duration::from_millis(1200), "the writer did its work");
        assert_eq!(names(dir.path()), vec!["1.mp3", "2.mp3", "3.mp3"]);
    }

    #[test]
    fn an_error_of_the_writer_ends_the_rip_without_a_hang_and_a_cancel_wakes_a_full_ring() {
        // A writer that fails: the rip ends with its error (no hang on the full ring behind it).
        let dir = tempfile::tempdir().unwrap();
        let disc = LongDisc::new(27 * 300);
        let encoder = slow(disc.consumed.clone(), 1);
        let mut s = disc.spec(dir.path(), false);
        s.buffer_bytes = 2 * crate::ring::BYTES_PER_SECOND as u64;
        let failing = |_: &Path, _: &[u8], track: u8| -> Result<(), RipError> { Err(RipError::new("disk_full", "no room", Some(track))) };
        let started = std::time::Instant::now();
        let err = rip_disc_with_parts(&disc, &disc.toc(), &s, &AtomicBool::new(false), &mut |_| {}, &encoder, &failing).unwrap_err();
        assert_eq!(err.kind, "disk_full");
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "{:?}", started.elapsed());
        // A cancel while the reader waits for room on a full ring (the encoder is very slow).
        let dir = tempfile::tempdir().unwrap();
        let disc = LongDisc::new(27 * 300);
        let encoder = slow(disc.consumed.clone(), 200);
        let mut s = disc.spec(dir.path(), false);
        s.buffer_bytes = 2 * crate::ring::BYTES_PER_SECOND as u64;
        let cancel = AtomicBool::new(false);
        let started = std::time::Instant::now();
        let mut seen = 0;
        let err = rip_disc_with(&disc, &disc.toc(), &s, &cancel, &mut |e| {
            if matches!(e, Event::Progress { .. }) {
                seen += 1;
                if seen == 2 {
                    cancel.store(true, Ordering::Relaxed);
                }
            }
        }, &encoder)
        .unwrap_err();
        assert_eq!(err.kind, "cancelled");
        assert!(started.elapsed() < std::time::Duration::from_secs(3), "{:?}", started.elapsed());
    }

    #[test]
    fn the_second_reading_is_a_second_pass_in_the_direction_of_the_disc() {
        let dir = tempfile::tempdir().unwrap();
        let disc = FakeDisc::new();
        // Without a shift both reads lie on the same grid, so the order of the starts can be compared as it is.
        let mut s = spec(dir.path());
        s.verify_shift_sectors = 0;
        rip_disc(&disc, &disc.toc(), &s, &AtomicBool::new(false), &mut |_| {}).unwrap();
        let starts = disc.starts.borrow();
        // Track 1, track 2, then track 1, track 2 again: one jump back, not one per track.
        let jumps = starts.windows(2).filter(|w| w[1] < w[0]).count();
        assert_eq!(jumps, 1, "{starts:?}");
        assert_eq!(starts.len() % 2, 0);
        assert_eq!(starts[..starts.len() / 2], starts[starts.len() / 2..]);
    }

    #[test]
    fn a_cancel_ends_the_rip_at_once_even_when_the_encoder_is_slow() {
        let dir = tempfile::tempdir().unwrap();
        let disc = LongDisc::new(27 * 400);
        let encoder = slow(disc.consumed.clone(), 5);
        let cancel = AtomicBool::new(false);
        let started = std::time::Instant::now();
        let mut seen = 0;
        let err = rip_disc_with(&disc, &disc.toc(), &disc.spec(dir.path(), true), &cancel, &mut |e| {
            if matches!(e, Event::Progress { .. }) {
                seen += 1;
                if seen == 3 {
                    cancel.store(true, Ordering::Relaxed);
                }
            }
        }, &encoder)
        .unwrap_err();
        assert_eq!(err.kind, "cancelled");
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "{:?}", started.elapsed());
        assert!(names(dir.path()).is_empty(), "{:?}", names(dir.path()));
        assert!(disc.reads.get() < 400, "the drive was not read to the end");
    }

    #[test]
    fn a_slow_encoder_with_verification_reads_every_track_twice_and_keeps_the_names_to_the_end() {
        let dir = tempfile::tempdir().unwrap();
        let disc = LongDisc::new(27 * 60);
        let encoder = slow(disc.consumed.clone(), 1);
        let mut events = Vec::new();
        rip_disc_with(&disc, &disc.toc(), &disc.spec(dir.path(), true), &AtomicBool::new(false), &mut |e| events.push(e.clone()), &encoder).unwrap();
        // 60 commands of 27 sectors in the first read; the second read has one more (its first command is the short one of the shift).
        assert_eq!(disc.reads.get(), 60 + 61);
        assert_eq!(names(dir.path()), vec!["A_B_CD02_TR01.mp3"]);
        assert_eq!(events.last(), Some(&Event::Done));
    }

    #[test]
    fn without_verification_every_track_is_read_once() {
        let dir = tempfile::tempdir().unwrap();
        let disc = FakeDisc::new();
        let mut s = spec(dir.path());
        s.verify = false;
        rip_disc(&disc, &disc.toc(), &s, &AtomicBool::new(false), &mut |_| {}).unwrap();
        let single = disc.calls.get();
        let dir2 = tempfile::tempdir().unwrap();
        let disc2 = FakeDisc::new();
        rip_disc(&disc2, &disc2.toc(), &spec(dir2.path()), &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(disc2.calls.get(), single * 2);
    }

    #[test]
    fn file_names_cannot_leave_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        for bad in ["../x.mp3", "/etc/x.mp3", "a/../../x.mp3", ""] {
            let mut s = spec(dir.path());
            s.tracks[0].file = bad.into();
            let disc = FakeDisc::new();
            let err = rip_disc(&disc, &disc.toc(), &s, &AtomicBool::new(false), &mut |_| {}).unwrap_err();
            assert_eq!(err.kind, "io", "{bad}");
            assert_eq!(disc.calls.get(), 0, "the drive is not touched");
        }
        let mut s = spec(dir.path());
        s.tracks[0].file = "sub/dir/x.mp3".into();
        assert!(validate(&s).is_ok());
    }

    #[test]
    fn formats_and_tags_the_helper_cannot_write_are_refused_before_reading() {
        let dir = tempfile::tempdir().unwrap();
        let disc = FakeDisc::new();
        for change in [|s: &mut JobSpec| s.encoder.format = "flac".into(), |s: &mut JobSpec| s.encoder.tag_version = "2.3".into(), |s: &mut JobSpec| s.encoder.bitrate_kbps = 65] {
            let mut s = spec(dir.path());
            change(&mut s);
            assert_eq!(rip_disc(&disc, &disc.toc(), &s, &AtomicBool::new(false), &mut |_| {}).unwrap_err().kind, "unsupported");
        }
        assert_eq!(disc.calls.get(), 0);
    }

    #[test]
    fn a_full_disk_is_told_apart() {
        assert_eq!(io_error(&std::io::Error::from_raw_os_error(libc::ENOSPC), 3).kind, "disk_full");
        assert_eq!(io_error(&std::io::Error::from_raw_os_error(libc::EACCES), 3).kind, "io");
    }

    #[test]
    fn the_job_comes_from_json() {
        let j = r#"{"device":"d","out_dir":"/o","tracks":[{"number":1,"file":"a.mp3","title":"t","tag_track":101}],
            "album":"b","artist":"a","disc":1,"encoder":{"format":"mp3","bitrate_kbps":64,"sample_rate_hz":24000,"channel_mode":"joint_stereo","quality":3,"tag_version":"2.4"}}"#;
        let s: JobSpec = serde_json::from_str(j).unwrap();
        assert!(s.verify && s.max_reads == 3 && s.cover.is_none());
        assert_eq!(serde_json::to_string(&Event::Progress { track: 1, fraction: 0.5 }).unwrap(), r#"{"event":"progress","track":1,"fraction":0.5}"#);
        assert_eq!(serde_json::to_string(&Event::Done).unwrap(), r#"{"event":"done"}"#);
    }
}
