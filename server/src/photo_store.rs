// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The pictures of the photo backup on the server (docs/spec-photo-sync.md, sections 3 and 4): which a device still has to send, the upload in
//! pieces that goes on after a lost connection, the check of the SHA-256 at the end, and the place of a picture in the picture folder of its
//! owner: `<year>/<month>/<original name>`, the month of the day it was taken (in the time zone of the server). A name that is taken by
//! another picture gets ` (2)`, ` (3)`, ... before the ending; nothing is ever overwritten.
//!
//! The owner is always the profile of the device's token, never a field of a request. The bytes of an upload under way are in the hidden
//! folder `.david-upload` of the picture folder (the same file system, so the finished file is only renamed into its place).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};

use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::photo_settings::{ProfilePhotoSettings, ServerPhotoSettings};
use crate::store::{Store, now_ms};

/// The hidden folder of the uploads under way, in the picture folder.
pub const UPLOAD_DIR: &str = ".david-upload";
/// An upload nobody went on with for this long is given up and its bytes removed.
pub const UPLOAD_KEEP_MS: i64 = 7 * 86_400_000;
/// The most files one `have` asks about.
pub const HAVE_MAX: usize = 1000;
/// The most bytes one piece of an upload may have (the app sends smaller ones).
pub const PIECE_MAX: usize = 32 * 1024 * 1024;

/// Why the server does not take a picture: the code the app gets, with the status it comes with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The person has the backup off (or the server does not allow it to them).
    BackupOff,
    /// The profile has no picture folder, or the server cannot write it.
    NoFolder,
    /// Larger than `photo_max_file_mb`.
    TooLarge,
    /// Neither a picture nor a video, or a video while videos are off.
    WrongType,
    /// The owner's limit (`photo_quota_gb`) would be passed.
    QuotaFull,
    /// The disk would keep less than the reserve free.
    DiskFull,
    /// The same `client_id` with another file.
    Clash,
    /// A piece that does not start where the upload stands.
    WrongOffset,
    /// The bytes are complete but their SHA-256 is another one; the upload starts over.
    HashMismatch,
    /// A header is missing or malformed.
    BadRequest,
}

impl Refusal {
    pub fn code(self) -> &'static str {
        match self {
            Refusal::BackupOff => "backup_off",
            Refusal::NoFolder => "no_folder",
            Refusal::TooLarge => "too_large",
            Refusal::WrongType => "wrong_type",
            Refusal::QuotaFull => "quota_full",
            Refusal::DiskFull => "disk_full",
            Refusal::Clash => "clash",
            Refusal::WrongOffset => "wrong_offset",
            Refusal::HashMismatch => "hash_mismatch",
            Refusal::BadRequest => "bad_request",
        }
    }
}

/// What a device says about a file it sends (the headers of the first and of every following piece).
#[derive(Debug, Clone)]
pub struct Offer {
    pub client_id: String,
    pub sha256: String,
    pub size: u64,
    pub name: String,
    pub mime: String,
    /// When it was taken (ms); the device knows it (the camera roll keeps it). Absent: now.
    pub taken_at: Option<i64>,
}

/// Where an upload stands after a piece.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Progress {
    /// So many bytes are there; the next piece starts here.
    Partial { received: u64 },
    /// Complete and checked: the picture has this id. `known` when the server had it already (from another device, or before).
    Done { photo: i64, known: bool },
}

/// A picture as the apps see it.
#[derive(Debug, Clone, Serialize)]
pub struct PhotoInfo {
    pub id: i64,
    pub sha256: String,
    pub size: i64,
    pub name: String,
    pub mime: String,
    pub taken_at: i64,
    pub uploaded_at: i64,
}

/// One lock per upload, so that two pieces of the same file are never written at once.
fn upload_lock(key: &str) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: OnceLock<StdMutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    let mut map = LOCKS.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    map.retain(|_, l| Arc::strong_count(l) > 1);
    map.entry(key.to_owned()).or_default().clone()
}

pub fn valid_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// A client id: 1 to 128 visible ASCII characters without a slash.
pub fn valid_client_id(s: &str) -> bool {
    (1..=128).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_graphic() && b != b'/' && b != b'\\')
}

/// The name a file is kept under: the last part of what the device says, without control characters and without a leading dot; empty
/// becomes the start of the hash with an ending from the type.
pub fn clean_name(name: &str, sha256: &str, mime: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let mut clean: String = last.chars().filter(|c| !c.is_control()).collect::<String>().trim().trim_start_matches('.').to_owned();
    while clean.len() > 200 {
        clean.pop();
    }
    if clean.is_empty() {
        let ending = match mime {
            "image/jpeg" => "jpg",
            "image/png" => "png",
            "image/heic" => "heic",
            "image/webp" => "webp",
            "video/mp4" => "mp4",
            "video/quicktime" => "mov",
            _ => "bin",
        };
        clean = format!("{}.{ending}", &sha256[..12.min(sha256.len())]);
    }
    clean
}

/// A free name in `dir`: the name itself, else `stem (2).ext`, `stem (3).ext`, ...
fn free_name(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_owned(), format!(".{e}")),
        _ => (name.to_owned(), String::new()),
    };
    (2..).map(|n| dir.join(format!("{stem} ({n}){ext}"))).find(|p| !p.exists()).expect("some number is free")
}

/// The SHA-256 of a file, in lower-case hex.
pub fn file_sha256(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// What the backup of a person is worth now: the settings under the rules of the server, and the picture folder.
pub struct Owner {
    pub id: i64,
    pub device: i64,
    pub server: ServerPhotoSettings,
    pub settings: ProfilePhotoSettings,
    pub folder: Option<PathBuf>,
}

impl Store {
    /// The device of a token of the apps (0 for a session of the browser).
    pub async fn device_of_token(&self, token: &str) -> Result<i64> {
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT device_id FROM session WHERE token_hash = ?",
                [Value::from(crate::auth::sha256_hex(token))],
            ))
            .await?;
        Ok(row.and_then(|r| r.try_get_by_index::<Option<i64>>(0).ok().flatten()).unwrap_or(0))
    }

    /// The owner of a request, with what the backup is worth for them.
    pub async fn photo_owner(&self, user: i64, role: &str, device: i64) -> Result<Owner> {
        let server = ServerPhotoSettings::load(self).await?;
        let settings = self.profile_photo_settings(user).await?.effective(&server, role);
        let folder = self.profile_folders(user).await?.into_iter().find(|(k, _)| k == "photo").map(|(_, p)| PathBuf::from(p));
        Ok(Owner { id: user, device, server, settings, folder })
    }

    /// Of these files (SHA-256, size), the ones the owner has not kept yet.
    pub async fn photos_missing(&self, owner: i64, files: &[(String, u64)]) -> Result<Vec<String>> {
        let mut missing = Vec::new();
        for (sha, _) in files {
            let row = self
                .db
                .reader
                .query_one_raw(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT 1 FROM photo WHERE owner_id = ? AND sha256 = ?",
                    [Value::from(owner), Value::from(sha.clone())],
                ))
                .await?;
            if row.is_none() && !missing.contains(sha) {
                missing.push(sha.clone());
            }
        }
        Ok(missing)
    }

    /// How many pictures the owner keeps, their bytes and the time of the newest upload.
    pub async fn photo_totals(&self, owner: i64) -> Result<(i64, i64, Option<i64>)> {
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT count(*), coalesce(sum(size_bytes), 0), max(uploaded_at) FROM photo WHERE owner_id = ?",
                [Value::from(owner)],
            ))
            .await?;
        let Some(r) = row else { return Ok((0, 0, None)) };
        Ok((r.try_get_by_index(0)?, r.try_get_by_index(1)?, r.try_get_by_index(2)?))
    }

    async fn photo_by_sha(&self, owner: i64, sha: &str) -> Result<Option<i64>> {
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM photo WHERE owner_id = ? AND sha256 = ?",
                [Value::from(owner), Value::from(sha.to_owned())],
            ))
            .await?;
        Ok(row.map(|r| r.try_get_by_index(0)).transpose()?)
    }

    /// Where the upload of this client id stands: its bytes so far and its file, or `None`.
    pub async fn photo_upload_state(&self, owner: i64, device: i64, client_id: &str) -> Result<Option<(String, u64)>> {
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT sha256, received FROM photo_upload WHERE owner_id = ? AND device_id = ? AND client_id = ?",
                [Value::from(owner), Value::from(device), Value::from(client_id.to_owned())],
            ))
            .await?;
        Ok(match row {
            Some(r) => Some((r.try_get_by_index(0)?, u64::try_from(r.try_get_by_index::<i64>(1)?).unwrap_or(0))),
            None => None,
        })
    }

    /// Removes the uploads nobody went on with for [`UPLOAD_KEEP_MS`], with their bytes.
    pub async fn prune_photo_uploads(&self) -> Result<usize> {
        let before = now_ms() - UPLOAD_KEEP_MS;
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT id, part FROM photo_upload WHERE touched_at < ?", [Value::from(before)]))
            .await?;
        for r in &rows {
            let (id, part): (i64, String) = (r.try_get_by_index(0)?, r.try_get_by_index(1)?);
            let _ = std::fs::remove_file(&part);
            self.db.writer.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "DELETE FROM photo_upload WHERE id = ?", [Value::from(id)])).await?;
        }
        Ok(rows.len())
    }

    /// Takes a piece of a file: `start` is where it belongs. The first piece (start 0) opens the upload after all checks; the last one
    /// finishes it, checks the SHA-256 and puts the picture into its place.
    pub async fn photo_piece(&self, o: &Owner, offer: &Offer, start: u64, bytes: &[u8]) -> Result<std::result::Result<Progress, Refusal>> {
        if !valid_sha256(&offer.sha256) || !valid_client_id(&offer.client_id) {
            return Ok(Err(Refusal::BadRequest));
        }
        if !o.settings.enabled {
            return Ok(Err(Refusal::BackupOff));
        }
        let Some(folder) = o.folder.clone().filter(|f| crate::folders::usable(f)) else { return Ok(Err(Refusal::NoFolder)) };
        // Known already (another device of the person, or sent before): nothing to send.
        if let Some(id) = self.photo_by_sha(o.id, &offer.sha256).await? {
            self.drop_upload(o, &offer.client_id).await?;
            return Ok(Ok(Progress::Done { photo: id, known: true }));
        }
        let lock = upload_lock(&format!("{}/{}/{}", o.id, o.device, offer.client_id));
        let _held = lock.lock().await;
        let state = self.photo_upload_state(o.id, o.device, &offer.client_id).await?;
        let received = match &state {
            Some((sha, _)) if *sha != offer.sha256 => return Ok(Err(Refusal::Clash)),
            Some((_, received)) => *received,
            None => {
                if let Some(why) = self.photo_refusal(o, offer, &folder).await? {
                    return Ok(Err(why));
                }
                self.prune_photo_uploads().await?;
                let dir = folder.join(UPLOAD_DIR);
                std::fs::create_dir_all(&dir).map_err(|e| Error::WriteFailed(format!("{}: {e}", dir.display())))?;
                let part = dir.join(format!("{}-{}-{}.part", o.id, o.device, uuid::Uuid::new_v4().simple()));
                std::fs::write(&part, b"").map_err(|e| Error::WriteFailed(format!("{}: {e}", part.display())))?;
                self.db
                    .writer
                    .execute_raw(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "INSERT INTO photo_upload (owner_id, device_id, client_id, sha256, size_bytes, received, name, mime, taken_at, part, touched_at) \
                         VALUES (?, ?, ?, ?, ?, 0, ?, ?, ?, ?, ?)",
                        [
                            Value::from(o.id),
                            Value::from(o.device),
                            Value::from(offer.client_id.clone()),
                            Value::from(offer.sha256.clone()),
                            Value::from(i64::try_from(offer.size).unwrap_or(i64::MAX)),
                            Value::from(clean_name(&offer.name, &offer.sha256, &offer.mime)),
                            Value::from(offer.mime.clone()),
                            Value::from(offer.taken_at.unwrap_or_else(now_ms)),
                            Value::from(part.to_string_lossy().into_owned()),
                            Value::from(now_ms()),
                        ],
                    ))
                    .await?;
                0
            }
        };
        let (part, size, name, mime, taken_at) = self.upload_row(o, &offer.client_id).await?;
        if start != received {
            return Ok(Err(Refusal::WrongOffset));
        }
        if received + bytes.len() as u64 > size {
            return Ok(Err(Refusal::BadRequest));
        }
        if !bytes.is_empty() {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new().append(true).open(&part).map_err(|e| Error::WriteFailed(format!("{}: {e}", part.display())))?;
            // The file may hold more than the table says (a write whose answer was lost): cut it back to what was counted.
            f.set_len(received).map_err(|e| Error::WriteFailed(e.to_string()))?;
            f.write_all(bytes).and_then(|()| f.sync_data()).map_err(|e| Error::WriteFailed(format!("{}: {e}", part.display())))?;
        }
        let received = received + bytes.len() as u64;
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE photo_upload SET received = ?, touched_at = ? WHERE owner_id = ? AND device_id = ? AND client_id = ?",
                [Value::from(i64::try_from(received).unwrap_or(i64::MAX)), Value::from(now_ms()), Value::from(o.id), Value::from(o.device), Value::from(offer.client_id.clone())],
            ))
            .await?;
        if received < size {
            return Ok(Ok(Progress::Partial { received }));
        }
        // Complete: the bytes must be the ones announced, else the upload starts over.
        let check = part.clone();
        let sha = tokio::task::spawn_blocking(move || file_sha256(&check)).await.map_err(|e| Error::WriteFailed(e.to_string()))?.map_err(|e| Error::WriteFailed(e.to_string()))?;
        if sha != offer.sha256 {
            tracing::warn!(owner = o.id, client_id = %offer.client_id, "a picture arrived with another SHA-256 than announced");
            self.drop_upload(o, &offer.client_id).await?;
            return Ok(Err(Refusal::HashMismatch));
        }
        let zone = self.setting("time_zone").await?;
        let tz = zone.as_deref().and_then(|z| jiff::tz::TimeZone::get(z).ok()).unwrap_or_else(jiff::tz::TimeZone::system);
        let when = jiff::Timestamp::from_millisecond(taken_at).unwrap_or_else(|_| jiff::Timestamp::now()).to_zoned(tz);
        let month = PathBuf::from(format!("{:04}", when.year())).join(format!("{:02}", when.month()));
        let dir = folder.join(&month);
        std::fs::create_dir_all(&dir).map_err(|e| Error::WriteFailed(format!("{}: {e}", dir.display())))?;
        let target = free_name(&dir, &name);
        std::fs::rename(&part, &target).map_err(|e| Error::WriteFailed(format!("{}: {e}", target.display())))?;
        let file = target.strip_prefix(&folder).unwrap_or(&target).to_string_lossy().into_owned();
        let device = (o.device != 0).then_some(o.device);
        let inserted = self
            .db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO photo (owner_id, device_id, sha256, size_bytes, file, original_name, mime, taken_at, uploaded_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                [
                    Value::from(o.id),
                    Value::from(device),
                    Value::from(sha.clone()),
                    Value::from(i64::try_from(size).unwrap_or(i64::MAX)),
                    Value::from(file.clone()),
                    Value::from(name.clone()),
                    Value::from(mime),
                    Value::from(taken_at),
                    Value::from(now_ms()),
                ],
            ))
            .await;
        self.drop_upload(o, &offer.client_id).await?;
        match inserted {
            Ok(r) => {
                let id = i64::try_from(r.last_insert_id()).unwrap_or(0);
                tracing::info!(owner = o.id, photo = id, %file, "picture backed up");
                Ok(Ok(Progress::Done { photo: id, known: false }))
            }
            // Another device finished the same picture a moment earlier: keep that one.
            Err(_) => {
                let _ = std::fs::remove_file(&target);
                let id = self.photo_by_sha(o.id, &sha).await?.ok_or_else(|| Error::WriteFailed("picture vanished".into()))?;
                Ok(Ok(Progress::Done { photo: id, known: true }))
            }
        }
    }

    /// Why a new upload is not taken, checked before its first byte.
    async fn photo_refusal(&self, o: &Owner, offer: &Offer, folder: &Path) -> Result<Option<Refusal>> {
        let video = offer.mime.starts_with("video/");
        if !(offer.mime.starts_with("image/") || video) || (video && !o.settings.videos) {
            return Ok(Some(Refusal::WrongType));
        }
        if offer.size > u64::from(o.server.max_file_mb) * 1024 * 1024 {
            return Ok(Some(Refusal::TooLarge));
        }
        if o.server.quota_gb > 0 {
            let (_, bytes, _) = self.photo_totals(o.id).await?;
            if u64::try_from(bytes).unwrap_or(0) + offer.size > u64::from(o.server.quota_gb) * 1024 * 1024 * 1024 {
                return Ok(Some(Refusal::QuotaFull));
            }
        }
        let reserve = if o.server.reserve_gb > 0 { u64::from(o.server.reserve_gb) * 1024 * 1024 * 1024 } else { crate::diskspace::reserve(self).await };
        if !crate::diskspace::fits(folder, offer.size, reserve) {
            return Ok(Some(Refusal::DiskFull));
        }
        Ok(None)
    }

    async fn upload_row(&self, o: &Owner, client_id: &str) -> Result<(PathBuf, u64, String, String, i64)> {
        let r = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT part, size_bytes, name, mime, taken_at FROM photo_upload WHERE owner_id = ? AND device_id = ? AND client_id = ?",
                [Value::from(o.id), Value::from(o.device), Value::from(client_id.to_owned())],
            ))
            .await?
            .ok_or_else(|| Error::WriteFailed("upload vanished".into()))?;
        Ok((
            PathBuf::from(r.try_get_by_index::<String>(0)?),
            u64::try_from(r.try_get_by_index::<i64>(1)?).unwrap_or(0),
            r.try_get_by_index(2)?,
            r.try_get_by_index(3)?,
            r.try_get_by_index(4)?,
        ))
    }

    async fn drop_upload(&self, o: &Owner, client_id: &str) -> Result<()> {
        if let Ok((part, ..)) = self.upload_row(o, client_id).await {
            let _ = std::fs::remove_file(part);
        }
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "DELETE FROM photo_upload WHERE owner_id = ? AND device_id = ? AND client_id = ?",
                [Value::from(o.id), Value::from(o.device), Value::from(client_id.to_owned())],
            ))
            .await?;
        Ok(())
    }

    /// Bisync: the pictures of the owner after `since` (an id) that other devices sent, oldest first, at most `limit`.
    pub async fn photo_changes(&self, owner: i64, device: i64, since: i64, limit: u64) -> Result<Vec<PhotoInfo>> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, sha256, size_bytes, original_name, mime, taken_at, uploaded_at FROM photo \
                 WHERE owner_id = ? AND id > ? AND (device_id IS NULL OR device_id != ?) ORDER BY id LIMIT ?",
                [Value::from(owner), Value::from(since), Value::from(device), Value::from(i64::try_from(limit).unwrap_or(500))],
            ))
            .await?;
        rows.iter()
            .map(|r| {
                Ok(PhotoInfo {
                    id: r.try_get_by_index(0)?,
                    sha256: r.try_get_by_index(1)?,
                    size: r.try_get_by_index(2)?,
                    name: r.try_get_by_index(3)?,
                    mime: r.try_get_by_index(4)?,
                    taken_at: r.try_get_by_index(5)?,
                    uploaded_at: r.try_get_by_index(6)?,
                })
            })
            .collect()
    }

    /// The file of a picture of this owner, and its type; `None` if the owner has no such picture.
    pub async fn photo_file(&self, owner: i64, id: i64) -> Result<Option<(PathBuf, String)>> {
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT file, mime FROM photo WHERE owner_id = ? AND id = ?", [Value::from(owner), Value::from(id)]))
            .await?;
        let Some(r) = row else { return Ok(None) };
        let folder = self.profile_folders(owner).await?.into_iter().find(|(k, _)| k == "photo").map(|(_, p)| PathBuf::from(p));
        let Some(folder) = folder else { return Ok(None) };
        let file: String = r.try_get_by_index(0)?;
        Ok(Some((folder.join(file), r.try_get_by_index(1)?)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_keeps_its_last_part_and_an_empty_one_comes_from_the_hash() {
        let sha = "a".repeat(64);
        assert_eq!(clean_name("DCIM/Camera/IMG_0001.jpg", &sha, "image/jpeg"), "IMG_0001.jpg");
        assert_eq!(clean_name("..\\..\\evil.jpg", &sha, "image/jpeg"), "evil.jpg");
        assert_eq!(clean_name(".hidden", &sha, "image/png"), "hidden");
        assert_eq!(clean_name("", &sha, "video/mp4"), "aaaaaaaaaaaa.mp4");
        assert_eq!(clean_name("..", &sha, "image/heic"), "aaaaaaaaaaaa.heic");
    }

    #[test]
    fn a_taken_name_gets_a_number_and_nothing_is_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(free_name(dir.path(), "a.jpg"), dir.path().join("a.jpg"));
        std::fs::write(dir.path().join("a.jpg"), b"x").unwrap();
        std::fs::write(dir.path().join("a (2).jpg"), b"x").unwrap();
        assert_eq!(free_name(dir.path(), "a.jpg"), dir.path().join("a (3).jpg"));
        std::fs::write(dir.path().join("README"), b"x").unwrap();
        assert_eq!(free_name(dir.path(), "README"), dir.path().join("README (2)"));
    }

    #[test]
    fn hashes_and_client_ids_are_checked() {
        assert!(valid_sha256(&"0f".repeat(32)));
        assert!(!valid_sha256(&"0F".repeat(32)) && !valid_sha256("abc"));
        assert!(!valid_client_id("content://media/123"), "no slashes");
        assert!(valid_client_id("media-123") && !valid_client_id("") && !valid_client_id(&"x".repeat(129)));
    }
}
