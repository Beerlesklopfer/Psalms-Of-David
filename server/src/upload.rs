// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Archives uploaded in the administration interface (ZIP, TAR, TAR.GZ, RAR, 7z), and the
//! queue that unpacks them.
//!
//! An upload is stored in `<data dir>/uploads/<id>/`, a row in `upload_job` records it, and one
//! worker takes the jobs one after the other: list the archive and refuse a bomb, unpack it
//! into the job's folder, keep only audio files and pictures, put every album into
//! `<import folder>/<Author>/<Title>/` and run the import if it is unlocked.
//!
//! Unpacking is done by `7z` (7-Zip), which reads all the formats above including RAR. It is
//! started without a shell and with the archive after `--`. What it writes is checked again here:
//! links are removed, nothing outside the job's folder is used, names are cleaned.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tokio::sync::mpsc;

use crate::store::Store;
use crate::tags;

/// Largest archive accepted. Audiobooks run to several GB, so the limit is generous; the file is
/// streamed to disk while it arrives and never held in memory, and the unpacking needs room for
/// the archive and its content at the same time.
pub const MAX_UPLOAD_BYTES: u64 = 32 * 1024 * 1024 * 1024;
/// Largest content of an archive once unpacked, and the most entries in it (a "zip bomb" guard).
const MAX_UNPACKED_BYTES: u64 = 64 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 20_000;

const AUDIO: &[&str] = &["mp3", "m4a", "m4b", "flac", "ogg", "opus"];
const PICTURES: &[&str] = &["jpg", "jpeg", "png"];
/// The endings that are accepted for an upload.
pub const ARCHIVE_ENDINGS: &[&str] = &["zip", "tar", "gz", "tgz", "bz2", "tbz2", "xz", "txz", "rar", "7z"];

/// The queue: where uploads are stored and how the worker is told about a new one.
#[derive(Clone)]
pub struct Uploads {
    dir: PathBuf,
    tx: mpsc::UnboundedSender<i64>,
}

impl Uploads {
    /// Starts the worker. Jobs that were waiting or running when the server stopped are taken up again.
    pub fn start(store: Store, data_dir: &Path) -> Uploads {
        let dir = data_dir.join("uploads");
        let (tx, mut rx) = mpsc::unbounded_channel::<i64>();
        let worker_dir = dir.clone();
        let again = tx.clone();
        tokio::spawn(async move {
            match store.requeue_uploads().await {
                Ok(ids) => ids.into_iter().for_each(|id| {
                    let _ = again.send(id);
                }),
                Err(e) => tracing::warn!(error = %e, "could not read the upload queue"),
            }
            while let Some(id) = rx.recv().await {
                let (state, message) = match process(&store, &worker_dir, id).await {
                    Ok(message) => ("done", message),
                    Err(message) => ("failed", message),
                };
                tracing::info!(job = id, state, %message, "upload processed");
                if let Err(e) = store.set_upload_state(id, state, &message).await {
                    tracing::warn!(job = id, error = %e, "could not record the state of an upload");
                }
                let _ = fs::remove_dir_all(worker_dir.join(id.to_string()));
            }
        });
        Uploads { dir, tx }
    }

    /// Where the archive of a job is written. The ending decides how it is read.
    pub fn archive_path(&self, id: i64, ending: &str) -> PathBuf {
        self.dir.join(id.to_string()).join(format!("archive.{ending}"))
    }

    pub fn enqueue(&self, id: i64) {
        let _ = self.tx.send(id);
    }
}

/// The ending of an uploaded file name if it is one of the accepted kinds ("a.tar.gz" gives "gz").
pub fn archive_ending(name: &str) -> Option<String> {
    let ending = name.rsplit_once('.')?.1.to_ascii_lowercase();
    ARCHIVE_ENDINGS.contains(&ending.as_str()).then_some(ending)
}

/// Does the whole job; the text is the result shown to the administrator.
async fn process(store: &Store, dir: &Path, id: i64) -> Result<String, String> {
    store.set_upload_state(id, "working", "").await.map_err(|e| e.to_string())?;
    let job_dir = dir.join(id.to_string());
    let archive = fs::read_dir(&job_dir)
        .map_err(|_| "the uploaded file is gone".to_owned())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| p.file_stem().is_some_and(|s| s == "archive"))
        .ok_or_else(|| "the uploaded file is gone".to_owned())?;

    let state = store.import_state().await.map_err(|e| e.to_string())?;
    let target_root = if store.upload_kind(id).await.map_err(|e| e.to_string())? == "music" { &state.music_roots } else { &state.roots }
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_dir())
        .unwrap_or_default();
    // Audiobooks go into the first audiobook folder, music into the first music folder.
    let music = store.upload_kind(id).await.map_err(|e| e.to_string())? == "music";
    let (folders, what) = if music { (&state.music_roots, "music folder") } else { (&state.roots, "import folder") };
    let root = folders
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_dir())
        .ok_or_else(|| format!("there is no {what}; set one in the settings first"))?;

    let unpacked = job_dir.join("unpacked");
    // Unpacking needs room next to the archive, and the albums need room in the import folder
    // unless they stay on the same disk. A reserve is always kept free.
    let reserve = crate::diskspace::reserve(store).await;
    let staging = job_dir.clone();
    let roomy = move |need: u64| -> Result<(), String> {
        let mut places = vec![staging.clone()];
        if !crate::diskspace::same_disk(&staging, &target_root) {
            places.push(target_root.clone());
        }
        for place in places {
            if !crate::diskspace::fits(&place, need, reserve) {
                return Err(format!("not enough free disk space for this archive (needs about {} MB and a reserve of {} MB)", need / 1_048_576, reserve / 1_048_576));
            }
        }
        Ok(())
    };
    let target = root.clone();
    let albums = tokio::task::spawn_blocking(move || -> Result<Vec<String>, String> {
        unpack(&archive, &unpacked, &roomy)?;
        place(&unpacked, &target)
    })
    .await
    .map_err(|_| "internal error".to_owned())??;
    if albums.is_empty() {
        return Err("the archive holds no audio files".to_owned());
    }
    let list = albums.join(", ");
    if !state.enabled {
        return Ok(format!("stored in the import folder ({list}); the import is locked, so it was not run"));
    }
    let report = store.run_import().await.map_err(|e| format!("stored ({list}), but the import failed: {e}"))?;
    Ok(format!("{list}: {} tracks added, {} changed", report.tracks_added, report.tracks_updated))
}

/// Unpacks into `out`. A tarball of a compressed kind comes out as one `.tar`, which is unpacked in turn.
fn unpack(archive: &Path, out: &Path, room: &dyn Fn(u64) -> Result<(), String>) -> Result<(), String> {
    fs::create_dir_all(out).map_err(|e| e.to_string())?;
    room(check_listing(archive)?)?;
    extract(archive, out)?;
    for _ in 0..2 {
        let entries: Vec<PathBuf> = fs::read_dir(out).map_err(|e| e.to_string())?.filter_map(|e| e.ok().map(|e| e.path())).collect();
        let [only] = entries.as_slice() else { break };
        // The one file is the tar inside a .gz, .bz2 or .xz (its name may have no ending any more).
        // If 7-Zip cannot read it as an archive, it is an ordinary file and stays.
        let Some(inner_size) = (if only.is_file() { check_listing(only).ok() } else { None }) else { break };
        room(inner_size)?;
        let inner = only.clone();
        let next = out.join("inner");
        extract(&inner, &next)?;
        fs::remove_file(&inner).map_err(|e| e.to_string())?;
        for e in fs::read_dir(&next).map_err(|e| e.to_string())?.filter_map(|e| e.ok()) {
            fs::rename(e.path(), out.join(e.file_name())).map_err(|e| e.to_string())?;
        }
        fs::remove_dir(&next).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn seven_zip() -> Command {
    // Both names are in use; the first that starts is taken.
    for name in ["7z", "7zz", "7za"] {
        if Command::new(name).arg("--help").output().is_ok() {
            return Command::new(name);
        }
    }
    Command::new("7z")
}

/// Lists the archive without unpacking and refuses one that is too big or has too many entries.
fn check_listing(archive: &Path) -> Result<u64, String> {
    let out = seven_zip()
        .args(["l", "-slt", "-ba", "--"])
        .arg(archive)
        .output()
        .map_err(|_| "7-Zip is not installed on the server (package 7zip or p7zip-full)".to_owned())?;
    if !out.status.success() {
        return Err("the archive cannot be read (damaged, encrypted or an unsupported kind)".to_owned());
    }
    let (mut entries, mut total) = (0usize, 0u64);
    let mut size = 0u64;
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some(v) = line.strip_prefix("Size = ") {
            size = v.trim().parse().unwrap_or(0);
        } else if line.starts_with("Path = ") {
            entries += 1;
            total += size;
            size = 0;
        }
    }
    total += size;
    if entries > MAX_ENTRIES || total > MAX_UNPACKED_BYTES {
        return Err("the archive is too large once unpacked".to_owned());
    }
    Ok(total)
}

fn extract(archive: &Path, out: &Path) -> Result<(), String> {
    let mut out_arg = std::ffi::OsString::from("-o");
    out_arg.push(out);
    let result = seven_zip()
        .args(["x", "-y", "-bd", "-aos"])
        .arg(out_arg)
        .arg("--")
        .arg(archive)
        .output()
        .map_err(|_| "7-Zip is not installed on the server".to_owned())?;
    if result.status.success() {
        Ok(())
    } else {
        Err("the archive could not be unpacked (damaged or password protected)".to_owned())
    }
}

/// Moves the audio files and pictures of every album folder under `unpacked` into
/// `root/<Author>/<Title>/` and returns the names of the albums.
fn place(unpacked: &Path, root: &Path) -> Result<Vec<String>, String> {
    let base = unpacked.canonicalize().map_err(|e| e.to_string())?;
    let mut folders: Vec<PathBuf> = Vec::new();
    collect(&base, &base, &mut folders)?;
    folders.sort();
    let mut albums = Vec::new();
    for folder in folders {
        let mut audio: Vec<PathBuf> = Vec::new();
        let mut pictures: Vec<PathBuf> = Vec::new();
        for entry in fs::read_dir(&folder).map_err(|e| e.to_string())?.filter_map(|e| e.ok()) {
            let path = entry.path();
            // Links are not followed and not kept.
            if !entry.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            match ending(&path) {
                Some(e) if AUDIO.contains(&e.as_str()) => audio.push(path),
                Some(e) if PICTURES.contains(&e.as_str()) => pictures.push(path),
                _ => {}
            }
        }
        if audio.is_empty() {
            continue;
        }
        audio.sort();
        let first = tags::read(&audio[0]).ok();
        let author = first.as_ref().and_then(|t| t.album_artist.clone().or_else(|| t.artist.clone()));
        let title = first.as_ref().and_then(|t| t.album.clone());
        let relative = folder.strip_prefix(&base).unwrap_or(&folder);
        let names: Vec<String> = relative.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        let (folder_author, folder_title) = match names.as_slice() {
            [.., a, t] => (Some(a.clone()), Some(t.clone())),
            [t] => (None, Some(t.clone())),
            [] => (None, None),
        };
        let author = clean_name(&author.or(folder_author).unwrap_or_default(), "Unknown");
        let title = clean_name(&title.or(folder_title).unwrap_or_default(), "Unknown");
        let dest = root.join(&author).join(&title);
        fs::create_dir_all(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
        for path in audio.iter().chain(pictures.iter()) {
            let name = path.file_name().map(|n| clean_name(&n.to_string_lossy(), "file")).unwrap_or_default();
            move_file(path, &dest.join(name))?;
        }
        albums.push(format!("{author} - {title}"));
    }
    Ok(albums)
}

/// The folders below `dir` (and `dir` itself), without following links.
fn collect(base: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    out.push(dir.to_owned());
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())?.filter_map(|e| e.ok()) {
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            let path = entry.path();
            // Whatever 7-Zip wrote stays inside; this makes sure.
            if path.canonicalize().is_ok_and(|p| p.starts_with(base)) {
                collect(base, &path, out)?;
            }
        }
    }
    Ok(())
}

fn ending(path: &Path) -> Option<String> {
    path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase())
}

/// A name that is safe as one folder or file name on every system: no separators, no
/// control characters, no leading dots, not too long.
pub fn clean_name(name: &str, fallback: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect();
    let cleaned: String = cleaned.trim().trim_start_matches('.').trim().chars().take(120).collect();
    if cleaned.is_empty() { fallback.to_owned() } else { cleaned }
}

fn move_file(from: &Path, to: &Path) -> Result<(), String> {
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    // Another file system: copy, then remove.
    fs::copy(from, to).map_err(|e| format!("{}: {e}", to.display()))?;
    let _ = fs::remove_file(from);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endings_and_names() {
        assert_eq!(archive_ending("Book.ZIP").as_deref(), Some("zip"));
        assert_eq!(archive_ending("a.tar.gz").as_deref(), Some("gz"));
        assert_eq!(archive_ending("a.exe"), None);
        assert_eq!(archive_ending("noending"), None);
        let bad = clean_name("../../etc/passwd", "x");
        assert!(!bad.contains('/') && !bad.starts_with('.'), "{bad}");
        assert_eq!(clean_name("", "x"), "x");
        assert_eq!(clean_name("a:b*c", "x"), "a_b_c");
        assert_eq!(clean_name(&"x".repeat(300), "x").chars().count(), 120);
    }
}
