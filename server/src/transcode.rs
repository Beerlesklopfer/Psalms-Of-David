// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Transcoding to MP3 (project owner, 2026-10-30: take care of AAC, Vorbis and MP4). The Linux engine and Android play Ogg Vorbis, FLAC and
//! M4A, but the iPhone (AVPlayer) cannot play Ogg Vorbis, Opus, WMA and the like (`audioformat::ios_playable`). A device that cannot play a
//! file asks for the part with `?as=mp3`; the server decodes the file (symphonia), encodes it with LAME (the encoder of the CD import) and keeps
//! the result in a cache below the data folder. See `docs/spec-transcoding.md`.
//!
//! The cache file is named by track, change time and size of the source and the bitrate, so a changed file is made again and an old version
//! is removed. A file is written under another name and renamed when whole: a half-made file is never served. A limit of concurrent transcodes
//! keeps a family from melting the machine, and the cache has a size limit (least recently used files go first).

use std::collections::HashMap;
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use symphonia::core::audio::sample::Sample;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as DecodeError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use tokio::sync::{Mutex, Semaphore};

/// The bitrate of the transcoded MP3 when the setting `transcode_kbps` says nothing usable.
pub const DEFAULT_KBPS: u32 = 128;
/// The size limit of the cache in MiB when the setting `transcode_cache_mb` says nothing usable.
pub const DEFAULT_CACHE_MB: u64 = 2048;
/// A file under construction that is older than this was left by a crash and is removed.
const STALE_PART: Duration = Duration::from_secs(3600);

/// Why a file could not be transcoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscodeError(pub String);

impl std::fmt::Display for TranscodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The name of the cache file of one version of a track.
pub fn cache_name(track_id: i64, mtime_ms: i64, size: u64, kbps: u32) -> String {
    format!("t{track_id}-{mtime_ms}-{size}-{kbps}.mp3")
}

/// The prefix that all versions of one track share, for removing the old ones.
fn track_prefix(track_id: i64) -> String {
    format!("t{track_id}-")
}

/// One file of the cache as the cleaning sees it.
#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub name: String,
    pub size: u64,
    pub used: SystemTime,
}

/// Which files go when the cache is over `limit` bytes: the least recently used first, never `keep` (the file just made or served).
pub fn plan_eviction(entries: &[CacheEntry], limit: u64, keep: &str) -> Vec<String> {
    let mut total: u64 = entries.iter().map(|e| e.size).sum();
    let mut by_age: Vec<&CacheEntry> = entries.iter().filter(|e| e.name != keep).collect();
    by_age.sort_by_key(|e| e.used);
    let mut gone = Vec::new();
    for e in by_age {
        if total <= limit {
            break;
        }
        total = total.saturating_sub(e.size);
        gone.push(e.name.clone());
    }
    gone
}

/// The transcoder of a server: the cache folder, the limit of concurrent transcodes and a lock per file so that two requests for the same
/// track make it once.
pub struct Transcoder {
    dir: PathBuf,
    permits: Arc<Semaphore>,
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl Transcoder {
    /// `parallel` transcodes at the same time at most (at least one).
    pub fn new(dir: PathBuf, parallel: usize) -> Transcoder {
        Transcoder { dir, permits: Arc::new(Semaphore::new(parallel.max(1))), locks: Mutex::new(HashMap::new()) }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The MP3 of `source` for the track `track_id`: from the cache if it is there, else made now. `kbps` is the wish (the nearest bitrate LAME
    /// offers is used), `limit_mb` the size limit of the cache.
    pub async fn mp3_of(&self, track_id: i64, source: &Path, kbps: u32, limit_mb: u64) -> Result<PathBuf, TranscodeError> {
        let meta = tokio::fs::metadata(source).await.map_err(|e| TranscodeError(format!("{}: {e}", source.display())))?;
        let mtime_ms = meta.modified().ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as i64);
        let kbps = u32::from(cdrip::encode::nearest_bitrate(kbps));
        let name = cache_name(track_id, mtime_ms, meta.len(), kbps);
        let path = self.dir.join(&name);
        if let Some(hit) = touch(&path) {
            return Ok(hit);
        }
        // One transcode per file at a time; a second request for the same track waits and then finds the file.
        let lock = self.locks.lock().await.entry(name.clone()).or_default().clone();
        let _one = lock.lock().await;
        if let Some(hit) = touch(&path) {
            return Ok(hit);
        }
        let _slot = self.permits.acquire().await.map_err(|_| TranscodeError("the transcoder was closed".into()))?;
        let (dir, source, dest, part) = (self.dir.clone(), source.to_owned(), path.clone(), self.dir.join(format!("{name}.part")));
        let prefix = track_prefix(track_id);
        let keep = name.clone();
        let result = tokio::task::spawn_blocking(move || -> Result<(), TranscodeError> {
            std::fs::create_dir_all(&dir).map_err(|e| TranscodeError(format!("{}: {e}", dir.display())))?;
            if let Err(e) = transcode_file(&source, &part, kbps) {
                let _ = std::fs::remove_file(&part);
                return Err(e);
            }
            std::fs::rename(&part, &dest).map_err(|e| TranscodeError(e.to_string()))?;
            // Older versions of this track are of no use any more; then the size limit.
            clean(&dir, &prefix, &keep, limit_mb.max(1).saturating_mul(1_048_576));
            Ok(())
        })
        .await
        .map_err(|e| TranscodeError(e.to_string()))?;
        self.locks.lock().await.remove(&name);
        result.map(|()| path)
    }
}

/// The file if it exists; its time of use is set to now (the cleaning goes by it).
fn touch(path: &Path) -> Option<PathBuf> {
    if !path.is_file() {
        return None;
    }
    if let Ok(f) = File::options().append(true).open(path) {
        let _ = f.set_modified(SystemTime::now());
    }
    Some(path.to_owned())
}

/// Removes the other versions of the track, files left under construction, and the least recently used files above the limit.
fn clean(dir: &Path, prefix: &str, keep: &str, limit: u64) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut entries = Vec::new();
    for item in read.flatten() {
        let name = item.file_name().to_string_lossy().into_owned();
        let Ok(meta) = item.metadata() else { continue };
        let used = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if name.ends_with(".part") {
            if SystemTime::now().duration_since(used).is_ok_and(|age| age > STALE_PART) {
                let _ = std::fs::remove_file(item.path());
            }
            continue;
        }
        if name.starts_with(prefix) && name != keep {
            let _ = std::fs::remove_file(item.path());
            continue;
        }
        if name.ends_with(".mp3") {
            entries.push(CacheEntry { name, size: meta.len(), used });
        }
    }
    for name in plan_eviction(&entries, limit, keep) {
        let _ = std::fs::remove_file(dir.join(name));
    }
}

/// Decodes `source` and writes the MP3 to `dest`. At most two channels are kept (the first two of a file with more).
fn transcode_file(source: &Path, dest: &Path, kbps: u32) -> Result<(), TranscodeError> {
    let fail = |what: &str, e: &dyn std::fmt::Display| TranscodeError(format!("{}: {what}: {e}", source.display()));
    let file = File::open(source).map_err(|e| fail("open", &e))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = source.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| fail("not a file that can be decoded", &e))?;
    let track = format.default_track(TrackType::Audio).ok_or_else(|| TranscodeError("no audio track".into()))?;
    let track_id = track.id;
    let params = track.codec_params.as_ref().and_then(|p| p.audio()).ok_or_else(|| TranscodeError("no audio parameters".into()))?;
    let mut decoder = symphonia::default::get_codecs().make_audio_decoder(params, &AudioDecoderOptions::default()).map_err(|e| fail("no decoder", &e))?;

    let mut out = File::create(dest).map_err(|e| fail("create", &e))?;
    let mut encoder: Option<cdrip::encode::StreamEncoder> = None;
    let (mut channels, mut written) = (0usize, 0usize);
    let (mut scratch, mut pcm, mut bytes) = (Vec::<f32>::new(), Vec::<i16>::new(), Vec::<u8>::new());
    let mut frames = 0u64;
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(e) => return Err(fail("read", &e)),
        };
        if packet.track_id != track_id {
            continue;
        }
        let buf = match decoder.decode(&packet) {
            Ok(b) => b,
            // A damaged frame is skipped, like players do.
            Err(DecodeError::DecodeError(_)) => continue,
            Err(e) => return Err(fail("decode", &e)),
        };
        let in_channels = buf.spec().channels().count();
        scratch.resize(buf.samples_interleaved(), f32::MID);
        buf.copy_to_slice_interleaved(&mut scratch);
        if scratch.is_empty() || in_channels == 0 {
            continue;
        }
        if encoder.is_none() {
            channels = in_channels.min(2);
            encoder = Some(cdrip::encode::StreamEncoder::new(channels as u8, buf.spec().rate(), kbps).map_err(|e| fail("encoder", &e))?);
        }
        pcm.clear();
        for frame in scratch.chunks_exact(in_channels) {
            for sample in &frame[..channels.min(in_channels)] {
                pcm.push((sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16);
            }
        }
        frames += (pcm.len() / channels) as u64;
        bytes.clear();
        if let Some(e) = encoder.as_mut() {
            e.push(&pcm, &mut bytes).map_err(|e| fail("encode", &e))?;
        }
        out.write_all(&bytes).map_err(|e| fail("write", &e))?;
        written += bytes.len();
    }
    let Some(encoder) = encoder.filter(|_| frames > 0) else { return Err(TranscodeError(format!("{}: no audio in the file", source.display()))) };
    bytes.clear();
    let tag = encoder.finish(&mut bytes).map_err(|e| fail("encode", &e))?;
    out.write_all(&bytes).map_err(|e| fail("write", &e))?;
    written += bytes.len();
    // The Xing/Info frame belongs at the start; the stream reserved room for it.
    if !tag.is_empty() && written >= tag.len() {
        out.seek(SeekFrom::Start(0)).map_err(|e| fail("write", &e))?;
        out.write_all(&tag).map_err(|e| fail("write", &e))?;
    }
    out.flush().map_err(|e| fail("write", &e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, size: u64, age_s: u64) -> CacheEntry {
        CacheEntry { name: name.into(), size, used: SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 - age_s) }
    }

    #[test]
    fn the_name_of_a_cache_file_says_which_version_of_which_track_it_is() {
        assert_eq!(cache_name(7, 1_700_000_000_000, 4096, 128), "t7-1700000000000-4096-128.mp3");
        // Another time or size, another name: a changed file is made again.
        assert_ne!(cache_name(7, 1, 4096, 128), cache_name(7, 2, 4096, 128));
        assert_ne!(cache_name(7, 1, 4096, 128), cache_name(7, 1, 4097, 128));
        assert_ne!(cache_name(7, 1, 4096, 128), cache_name(7, 1, 4096, 64));
        // The prefix of track 7 does not take track 70 along.
        assert!(!cache_name(70, 1, 1, 128).starts_with(&track_prefix(7)));
    }

    #[test]
    fn the_least_recently_used_files_go_first_and_the_kept_one_stays() {
        let entries = [entry("old", 400, 300), entry("mid", 400, 200), entry("new", 400, 100), entry("kept", 400, 500)];
        // 1600 bytes, limit 1000: two files go, the oldest of the others first; "kept" is the oldest of all but stays.
        assert_eq!(plan_eviction(&entries, 1000, "kept"), vec!["old", "mid"]);
        // Under the limit nothing goes.
        assert!(plan_eviction(&entries, 1600, "kept").is_empty());
        // Even if the kept file alone is over the limit it stays.
        assert_eq!(plan_eviction(&[entry("kept", 5000, 1)], 10, "kept"), Vec::<String>::new());
    }

    #[test]
    fn cleaning_removes_old_versions_and_enforces_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, n: usize| std::fs::write(dir.path().join(name), vec![0u8; n]).unwrap();
        write("t7-1-10-128.mp3", 100); // an old version of track 7
        write("t7-2-10-128.mp3", 100); // the new one
        write("t8-1-10-128.mp3", 100);
        write("t9-1-10-128.mp3", 100);
        write("t5-1-10-128.mp3.part", 50); // under construction (young): stays
        clean(dir.path(), &track_prefix(7), "t7-2-10-128.mp3", 250);
        let mut left: Vec<String> = std::fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert!(!left.contains(&"t7-1-10-128.mp3".to_string()), "the old version of the track is gone: {left:?}");
        assert!(left.contains(&"t7-2-10-128.mp3".to_string()) && left.contains(&"t5-1-10-128.mp3.part".to_string()), "{left:?}");
        let mp3s = left.iter().filter(|n| n.ends_with(".mp3")).count();
        assert!(mp3s <= 2, "300 bytes of MP3 left within a limit of 250: {left:?}");
    }
}
