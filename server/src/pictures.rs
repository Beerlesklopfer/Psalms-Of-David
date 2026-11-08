// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The pictures a person may look at, for the slideshow of the apps and the browser (docs/spec-pictures.md; project owner, 2026-11-08):
//!
//! - **own**: the pictures the devices of the person backed up (table `photo`, not in the trash). Everybody but a guest.
//! - **a child's**: an administrator (the parents, docs/spec-photo-sync.md) sees the backup of a child when the server allows it
//!   (`photo_parents_may_view`) and the child's profile does (`parent_view`).
//! - **the old folders**: every folder in `photo_library` (e.g. /srv/Bilder) that is not the picture folder of a profile (Allgäu2017,
//!   Familie, ...), for the adults (administrators and members), not for children and guests.
//!
//! A picture is named by a key: `b<id>` for one of the backup, `l<id>` for one of an old folder. Every request checks again that the person
//! may see it; a key of somebody else's picture is the same "not found" as one that does not exist.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
use serde::Serialize;

use crate::error::Result;
use crate::photo_settings::ServerPhotoSettings;
use crate::roles::Role;
use crate::store::{Store, now_ms};

/// The files of the old folders, by their ending.
pub fn mime_of(name: &str) -> Option<&'static str> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "jpg" | "jpeg" | "jpe" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "heic" => "image/heic",
        "heif" => "image/heif",
        "avif" => "image/avif",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "3gp" => "video/3gpp",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        "mts" | "m2ts" => "video/mp2t",
        _ => return None,
    })
}

/// How deep the old folders are read.
const MAX_DEPTH: usize = 16;

/// Where the pictures come from, as the person chooses them.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Source {
    /// `own`, `p<profile id>` (a child), `f:<folder>` (an old folder).
    pub id: String,
    /// `own`, `child`, `folder`.
    pub kind: &'static str,
    /// The name of the child or of the folder; empty for `own`.
    pub name: String,
    pub count: i64,
    /// The oldest and the newest picture (ms).
    pub first: Option<i64>,
    pub last: Option<i64>,
}

/// One picture of a list.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Item {
    pub key: String,
    pub source: String,
    pub name: String,
    pub mime: String,
    pub video: bool,
    pub taken_at: i64,
}

/// What a request asks for: these sources (empty: all that may be seen), between these times (ms, both ends included), with or without videos.
#[derive(Debug, Clone, Default)]
pub struct Query {
    pub sources: Vec<String>,
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub videos: bool,
}

/// The most pictures one list has.
pub const LIST_MAX: usize = 50_000;

/// A picture as a request finds it: the file, its type, and (for the backup) the preview its device sent.
#[derive(Debug, Clone)]
pub struct Found {
    pub path: PathBuf,
    pub mime: String,
    pub device_thumb: Option<PathBuf>,
    pub taken_at: i64,
}

fn adult(role: Role) -> bool {
    matches!(role, Role::Admin | Role::Member)
}

fn library_lock() -> &'static tokio::sync::Mutex<()> {
    static L: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    L.get_or_init(Default::default)
}

/// One file found in the old folders.
#[derive(Debug, Clone)]
struct Seen {
    folder: String,
    file: String,
    size: i64,
    mtime: i64,
    mime: &'static str,
}

/// Walks the old folders: every picture and video below `root`, without hidden folders and files and without `skip` (the picture folders
/// of the profiles). Symbolic links are not followed.
fn walk(root: &Path, skip: &HashSet<PathBuf>) -> Vec<Seen> {
    let mut out = Vec::new();
    let mut todo = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = todo.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name.starts_with('@') || name == "#recycle" || name.eq_ignore_ascii_case("$RECYCLE.BIN") {
                continue;
            }
            let Ok(kind) = e.file_type() else { continue };
            let path = e.path();
            if kind.is_dir() {
                let canonical = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                if depth < MAX_DEPTH && !skip.contains(&canonical) {
                    todo.push((path, depth + 1));
                }
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            let Some(mime) = mime_of(&name) else { continue };
            let Ok(rel) = path.strip_prefix(root) else { continue };
            let rel = rel.to_string_lossy().replace('\\', "/");
            let folder = rel.split_once('/').map(|(f, _)| f.to_owned()).unwrap_or_default();
            let Ok(meta) = e.metadata() else { continue };
            let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0));
            out.push(Seen { folder, file: rel, size: i64::try_from(meta.len()).unwrap_or(i64::MAX), mtime, mime });
        }
    }
    out
}

/// When a file was taken: the EXIF data, else the time of the file.
fn taken_at(path: &Path, mime: &str, mtime: i64, tz: &jiff::tz::TimeZone) -> i64 {
    if mime.starts_with("image/") {
        crate::picture_meta::read(path).taken.and_then(|t| t.millis(tz)).unwrap_or(mtime)
    } else {
        mtime
    }
}

impl Store {
    /// The folder of the family's pictures (`photo_library`), if it is set and can be read.
    pub async fn photo_library(&self) -> Result<Option<PathBuf>> {
        let root = self.setting("photo_library").await?.unwrap_or_default();
        let root = root.trim();
        if root.is_empty() {
            return Ok(None);
        }
        let p = PathBuf::from(root);
        Ok(p.is_dir().then_some(p))
    }

    async fn time_zone(&self) -> Result<jiff::tz::TimeZone> {
        let zone = self.setting("time_zone").await?;
        Ok(zone.as_deref().and_then(|z| jiff::tz::TimeZone::get(z).ok()).unwrap_or_else(jiff::tz::TimeZone::system))
    }

    /// Reads the old folders again: new and changed files are taken in (with the date of their EXIF data), files that are gone are dropped
    /// with what was made of them. Without `photo_library` every entry goes. Returns (taken in, dropped). One reading at a time.
    pub async fn scan_photo_library(&self) -> Result<(usize, usize)> {
        let _one = library_lock().lock().await;
        let root = self.photo_library().await?;
        let skip: HashSet<PathBuf> = self
            .all_profile_folders()
            .await?
            .values()
            .filter_map(|k| k.get("photo"))
            .map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| PathBuf::from(p)))
            .collect();
        let seen = match root.clone() {
            Some(r) => tokio::task::spawn_blocking(move || walk(&r, &skip)).await.unwrap_or_default(),
            None => Vec::new(),
        };
        let rows = self.db.reader.query_all_raw(Statement::from_string(DbBackend::Sqlite, "SELECT id, file, size_bytes, mtime FROM library_photo")).await?;
        let mut known: HashMap<String, (i64, i64, i64)> = HashMap::new();
        for r in &rows {
            known.insert(r.try_get_by_index(1)?, (r.try_get_by_index(0)?, r.try_get_by_index(2)?, r.try_get_by_index(3)?));
        }
        let tz = self.time_zone().await?;
        let mut taken = 0;
        let mut present = HashSet::new();
        for s in seen {
            present.insert(s.file.clone());
            let old = known.get(&s.file).copied();
            if old.is_some_and(|(_, size, mtime)| size == s.size && mtime == s.mtime) {
                continue;
            }
            let Some(root) = root.as_ref() else { break };
            let (path, mime, mtime, zone) = (root.join(&s.file), s.mime, s.mtime, tz.clone());
            let at = tokio::task::spawn_blocking(move || taken_at(&path, mime, mtime, &zone)).await.unwrap_or(s.mtime);
            self.db
                .writer
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "INSERT INTO library_photo (folder, file, size_bytes, mtime, mime, taken_at) VALUES (?, ?, ?, ?, ?, ?) \
                     ON CONFLICT (file) DO UPDATE SET folder = excluded.folder, size_bytes = excluded.size_bytes, mtime = excluded.mtime, \
                     mime = excluded.mime, taken_at = excluded.taken_at",
                    [Value::from(s.folder), Value::from(s.file), Value::from(s.size), Value::from(s.mtime), Value::from(s.mime), Value::from(at)],
                ))
                .await?;
            if let Some((id, ..)) = old {
                crate::picture_render::forget(&self.data_dir, &format!("l{id}"));
            }
            taken += 1;
        }
        let mut dropped = 0;
        for (file, (id, ..)) in known {
            if present.contains(&file) {
                continue;
            }
            self.db.writer.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "DELETE FROM library_photo WHERE id = ?", [Value::from(id)])).await?;
            crate::picture_render::forget(&self.data_dir, &format!("l{id}"));
            dropped += 1;
        }
        if taken > 0 || dropped > 0 {
            tracing::info!(taken, dropped, "the old picture folders were read");
        }
        Ok((taken, dropped))
    }

    /// The children whose backup this person may look at: an administrator, the server allows it, the child's profile does.
    async fn viewable_children(&self, role: Role) -> Result<Vec<(i64, String)>> {
        if role != Role::Admin {
            return Ok(Vec::new());
        }
        let server = ServerPhotoSettings::load(self).await?;
        if !server.parents_may_view {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for p in self.list_profiles().await? {
            if p.role == "child" && self.profile_photo_settings(p.id).await?.effective(&server, "child").parent_view {
                out.push((p.id, p.name));
            }
        }
        Ok(out)
    }

    /// The sources this person may choose from, each with how many pictures it has (videos counted) and its first and last day.
    pub async fn picture_sources(&self, user: i64, role: Role) -> Result<Vec<Source>> {
        if role == Role::Guest {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        let backup = |owner: i64| {
            Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT count(*), min(taken_at), max(taken_at) FROM photo WHERE owner_id = ? AND trashed_at IS NULL",
                [Value::from(owner)],
            )
        };
        if let Some(r) = self.db.reader.query_one_raw(backup(user)).await? {
            let count: i64 = r.try_get_by_index(0)?;
            if count > 0 {
                out.push(Source { id: "own".into(), kind: "own", name: String::new(), count, first: r.try_get_by_index(1)?, last: r.try_get_by_index(2)? });
            }
        }
        for (id, name) in self.viewable_children(role).await? {
            if let Some(r) = self.db.reader.query_one_raw(backup(id)).await? {
                let count: i64 = r.try_get_by_index(0)?;
                if count > 0 {
                    out.push(Source { id: format!("p{id}"), kind: "child", name, count, first: r.try_get_by_index(1)?, last: r.try_get_by_index(2)? });
                }
            }
        }
        if adult(role)
            && let Some(root) = self.photo_library().await?
        {
            // The pictures right in the folder of the family are under its own name.
            let top = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let rows = self
                .db
                .reader
                .query_all_raw(Statement::from_string(
                    DbBackend::Sqlite,
                    "SELECT folder, count(*), min(taken_at), max(taken_at) FROM library_photo GROUP BY folder ORDER BY folder COLLATE NOCASE",
                ))
                .await?;
            for r in rows {
                let folder: String = r.try_get_by_index(0)?;
                out.push(Source {
                    id: format!("f:{folder}"),
                    kind: "folder",
                    name: if folder.is_empty() { top.clone() } else { folder },
                    count: r.try_get_by_index(1)?,
                    first: r.try_get_by_index(2)?,
                    last: r.try_get_by_index(3)?,
                });
            }
        }
        Ok(out)
    }

    /// The pictures of the chosen sources (only those the person may see; none chosen: all of them), the oldest first.
    pub async fn pictures(&self, user: i64, role: Role, q: &Query) -> Result<Vec<Item>> {
        let allowed: Vec<String> = self.picture_sources(user, role).await?.into_iter().map(|s| s.id).collect();
        let chosen: Vec<&String> = if q.sources.is_empty() { allowed.iter().collect() } else { allowed.iter().filter(|a| q.sources.contains(a)).collect() };
        let from = q.from.unwrap_or(i64::MIN);
        let to = q.to.unwrap_or(i64::MAX);
        let mut out = Vec::new();
        for source in chosen {
            let (sql, owner_or_folder) = if source == "own" {
                ("SELECT 'b' || id, original_name, mime, taken_at FROM photo WHERE owner_id = ? AND trashed_at IS NULL AND taken_at BETWEEN ? AND ?", Value::from(user))
            } else if let Some(id) = source.strip_prefix('p').and_then(|s| s.parse::<i64>().ok()) {
                ("SELECT 'b' || id, original_name, mime, taken_at FROM photo WHERE owner_id = ? AND trashed_at IS NULL AND taken_at BETWEEN ? AND ?", Value::from(id))
            } else if let Some(folder) = source.strip_prefix("f:") {
                ("SELECT 'l' || id, file, mime, taken_at FROM library_photo WHERE folder = ? AND taken_at BETWEEN ? AND ?", Value::from(folder.to_owned()))
            } else {
                continue;
            };
            let rows = self.db.reader.query_all_raw(Statement::from_sql_and_values(DbBackend::Sqlite, sql, [owner_or_folder, Value::from(from), Value::from(to)])).await?;
            for r in rows {
                let mime: String = r.try_get_by_index(2)?;
                let video = mime.starts_with("video/");
                if video && !q.videos {
                    continue;
                }
                let name: String = r.try_get_by_index(1)?;
                let name = name.rsplit('/').next().unwrap_or_default().to_owned();
                out.push(Item { key: r.try_get_by_index(0)?, source: source.clone(), name, mime, video, taken_at: r.try_get_by_index(3)? });
            }
        }
        out.sort_by(|a, b| a.taken_at.cmp(&b.taken_at).then_with(|| a.key.cmp(&b.key)));
        out.truncate(LIST_MAX);
        Ok(out)
    }

    /// A picture by its key, if this person may see it.
    pub async fn picture(&self, user: i64, role: Role, key: &str) -> Result<Option<Found>> {
        if role == Role::Guest {
            return Ok(None);
        }
        if let Some(id) = key.strip_prefix('b').and_then(|s| s.parse::<i64>().ok()) {
            let row = self
                .db
                .reader
                .query_one_raw(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT owner_id, file, mime, taken_at FROM photo WHERE id = ? AND trashed_at IS NULL",
                    [Value::from(id)],
                ))
                .await?;
            let Some(r) = row else { return Ok(None) };
            let owner: i64 = r.try_get_by_index(0)?;
            if owner != user && !self.viewable_children(role).await?.iter().any(|(c, _)| *c == owner) {
                return Ok(None);
            }
            let Some(folder) = self.profile_folders(owner).await?.into_iter().find(|(k, _)| k == "photo").map(|(_, p)| PathBuf::from(p)) else {
                return Ok(None);
            };
            let file: String = r.try_get_by_index(1)?;
            let thumb = folder.join(crate::photo_store::THUMB_DIR).join(format!("{id}.jpg"));
            return Ok(Some(Found {
                path: folder.join(file),
                mime: r.try_get_by_index(2)?,
                device_thumb: thumb.is_file().then_some(thumb),
                taken_at: r.try_get_by_index(3)?,
            }));
        }
        if let Some(id) = key.strip_prefix('l').and_then(|s| s.parse::<i64>().ok()) {
            if !adult(role) {
                return Ok(None);
            }
            let Some(root) = self.photo_library().await? else { return Ok(None) };
            let row = self
                .db
                .reader
                .query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT file, mime, taken_at FROM library_photo WHERE id = ?", [Value::from(id)]))
                .await?;
            let Some(r) = row else { return Ok(None) };
            let file: String = r.try_get_by_index(0)?;
            let path = root.join(&file);
            // Only what lies below the folder: a link that leads out of it is not served.
            let (Ok(real), Ok(base)) = (std::fs::canonicalize(&path), std::fs::canonicalize(&root)) else { return Ok(None) };
            if !real.starts_with(&base) {
                return Ok(None);
            }
            return Ok(Some(Found { path: real, mime: r.try_get_by_index(1)?, device_thumb: None, taken_at: r.try_get_by_index(2)? }));
        }
        Ok(None)
    }

    /// When and where a picture was taken: the time, and the name of the place in this language (when the server names places).
    pub async fn picture_info(&self, found: &Found, lang: &str) -> Result<serde_json::Value> {
        let place = if found.mime.starts_with("image/") {
            let path = found.path.clone();
            tokio::task::spawn_blocking(move || crate::picture_meta::read(&path).place).await.ok().flatten()
        } else {
            None
        };
        let name = match place {
            Some(p) => self.place_name(p, lang).await?,
            None => None,
        };
        Ok(serde_json::json!({ "taken_at": found.taken_at, "place": name, "at": now_ms() }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_endings_say_the_type() {
        assert_eq!(mime_of("IMG_0001.HEIC"), Some("image/heic"));
        assert_eq!(mime_of("a.b.JPG"), Some("image/jpeg"));
        assert_eq!(mime_of("clip.MOV"), Some("video/quicktime"));
        assert_eq!(mime_of("Thumbs.db"), None);
        assert_eq!(mime_of("README"), None);
    }

    #[test]
    fn the_walk_skips_hidden_folders_and_the_picture_folders_of_the_profiles() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for p in ["Allgäu2017/Tag 1/a.jpg", "Allgäu2017/b.heic", "Jörg/2026/11/c.jpg", "Familie/.david-thumbs/1.jpg", "Familie/d.txt", "e.png", ".recyclebin/f.jpg"] {
            let path = root.join(p);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"x").unwrap();
        }
        let skip: HashSet<PathBuf> = [std::fs::canonicalize(root.join("Jörg")).unwrap()].into_iter().collect();
        let mut seen: Vec<(String, String)> = walk(root, &skip).into_iter().map(|s| (s.folder, s.file)).collect();
        seen.sort();
        assert_eq!(
            seen,
            vec![
                ("".into(), "e.png".into()),
                ("Allgäu2017".into(), "Allgäu2017/Tag 1/a.jpg".into()),
                ("Allgäu2017".into(), "Allgäu2017/b.heic".into()),
            ]
        );
    }
}
