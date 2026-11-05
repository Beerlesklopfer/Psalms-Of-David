// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Photos of a CD case or of an album (front, back, booklet), taken with the app, for the language model that reads them through MCP
//! (`get_photos`): the titles, the times, the EAN, and the front as the cover (`set_cover_from_photo`).
//!
//! They serve one process and go when it ends (project owner, 2026-11-05): the photos of a disc when the rip of that disc ends (imported,
//! present, decided, expired: `cdflow::notify`), those of an album when the model is done (`discard_photos`). What a process left behind
//! (a cancel, nobody finished) goes after [`KEEP_MS`]. They lie in `<data dir>/photos/`, not in the library: no list, stream or import sees them.

use std::path::{Path, PathBuf};

/// How many photos one disc or album may have.
pub const MAX_PHOTOS: usize = 12;
/// How long photos stay without a process that ends them.
pub const KEEP_MS: i64 = 24 * 3600 * 1000;
const JOB_FILE: &str = "job";
/// Who took the photos (the profile of the device that sent the first).
const BY_FILE: &str = "by";

/// What the photos belong to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Owner {
    /// The disc in a drive, by its MusicBrainz Disc ID: a CD that is not in the library yet.
    Disc(String),
    /// An album of the library.
    Album(i64),
}

impl Owner {
    /// The folder name; `None` for a Disc ID that is no Disc ID (it must not reach the path).
    fn dir_name(&self) -> Option<String> {
        match self {
            Owner::Disc(id) => {
                let ok = !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
                ok.then(|| format!("disc-{id}"))
            }
            Owner::Album(id) => (*id > 0).then(|| format!("album-{id}")),
        }
    }
}

/// One photo: its number (1, 2, ... in the order they came), its type and its size.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Photo {
    pub n: u32,
    pub mime: &'static str,
    pub bytes: u64,
}

/// Photos that wait for the language model: whose, how many, since when, who took them, and the rip that reads the disc (if one does).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub owner: Owner,
    pub photos: usize,
    pub since_ms: i64,
    pub by: String,
    pub job: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct Photos {
    base: PathBuf,
}

fn ext(mime: &str) -> &'static str {
    if mime == "image/png" { "png" } else { "jpg" }
}

impl Photos {
    pub fn new(data_dir: &Path) -> Photos {
        Photos { base: data_dir.join("photos") }
    }

    fn dir(&self, owner: &Owner) -> Option<PathBuf> {
        owner.dir_name().map(|n| self.base.join(n))
    }

    /// Adds a photo (JPEG or PNG, already checked) that `by` took; its number. `Err("photo_limit")` when there are [`MAX_PHOTOS`] already,
    /// `Err("invalid_disc")` for an owner that cannot be a folder.
    pub fn add(&self, owner: &Owner, bytes: &[u8], mime: &str, by: &str) -> Result<u32, &'static str> {
        let dir = self.dir(owner).ok_or("invalid_disc")?;
        let list = self.list(owner);
        if list.len() >= MAX_PHOTOS {
            return Err("photo_limit");
        }
        std::fs::create_dir_all(&dir).map_err(|_| "internal")?;
        if list.is_empty() && !by.is_empty() {
            let _ = std::fs::write(dir.join(BY_FILE), by);
        }
        let n = list.last().map_or(1, |p| p.n + 1);
        let part = dir.join(format!("{n:02}.part"));
        std::fs::write(&part, bytes).map_err(|_| "internal")?;
        std::fs::rename(&part, dir.join(format!("{n:02}.{}", ext(mime)))).map_err(|_| "internal")?;
        Ok(n)
    }

    /// The photos, in order.
    pub fn list(&self, owner: &Owner) -> Vec<Photo> {
        let Some(dir) = self.dir(owner) else { return Vec::new() };
        let mut out: Vec<Photo> = std::fs::read_dir(dir)
            .map(|rd| {
                rd.flatten()
                    .filter_map(|e| {
                        let name = e.file_name().to_string_lossy().into_owned();
                        let (stem, ext) = name.rsplit_once('.')?;
                        let mime = match ext {
                            "jpg" => "image/jpeg",
                            "png" => "image/png",
                            _ => return None,
                        };
                        Some(Photo { n: stem.parse().ok()?, mime, bytes: e.metadata().ok()?.len() })
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.sort_by_key(|p| p.n);
        out
    }

    /// One photo with its bytes.
    pub fn read(&self, owner: &Owner, n: u32) -> Option<(&'static str, Vec<u8>)> {
        let p = self.list(owner).into_iter().find(|p| p.n == n)?;
        let bytes = std::fs::read(self.dir(owner)?.join(format!("{n:02}.{}", ext(p.mime)))).ok()?;
        Some((p.mime, bytes))
    }

    /// Removes all photos of the owner. Returns how many there were.
    pub fn discard(&self, owner: &Owner) -> usize {
        let n = self.list(owner).len();
        if let Some(dir) = self.dir(owner) {
            let _ = std::fs::remove_dir_all(dir);
        }
        n
    }

    /// Everything that has photos waiting: the tasks of the language model, oldest first.
    pub fn backlog(&self) -> Vec<Task> {
        let Ok(rd) = std::fs::read_dir(&self.base) else { return Vec::new() };
        let mut out: Vec<Task> = rd
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let owner = if let Some(id) = name.strip_prefix("disc-") {
                    Owner::Disc(id.to_owned())
                } else {
                    Owner::Album(name.strip_prefix("album-")?.parse().ok()?)
                };
                let photos = self.list(&owner);
                if photos.is_empty() {
                    return None;
                }
                let since = std::fs::read_dir(e.path())
                    .ok()?
                    .flatten()
                    .filter_map(|f| f.metadata().and_then(|m| m.modified()).ok())
                    .min()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_millis() as i64);
                let by = std::fs::read_to_string(e.path().join(BY_FILE)).unwrap_or_default();
                let job = std::fs::read_to_string(e.path().join(JOB_FILE)).ok().and_then(|t| t.trim().parse().ok());
                Some(Task { owner, photos: photos.len(), since_ms: since, by, job })
            })
            .collect();
        out.sort_by_key(|t| t.since_ms);
        out
    }

    /// The rip `job_id` reads this disc: its end ends the photos of the disc.
    pub fn attach_job(&self, disc_id: &str, job_id: i64) {
        if let Some(dir) = self.dir(&Owner::Disc(disc_id.to_owned()))
            && dir.is_dir()
        {
            let _ = std::fs::write(dir.join(JOB_FILE), job_id.to_string());
        }
    }

    /// The rip `job_id` ended: the photos of its disc go.
    pub fn discard_job(&self, job_id: i64) {
        let Ok(rd) = std::fs::read_dir(&self.base) else { return };
        for e in rd.flatten() {
            let attached = std::fs::read_to_string(e.path().join(JOB_FILE)).ok().and_then(|t| t.trim().parse::<i64>().ok());
            if attached == Some(job_id) {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }

    /// Removes the photos that no process ended within [`KEEP_MS`] of their last change.
    pub fn sweep(&self, now_ms: i64) {
        let Ok(rd) = std::fs::read_dir(&self.base) else { return };
        for e in rd.flatten() {
            let changed = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as i64);
            if now_ms - changed > KEEP_MS {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 1, 2];

    #[test]
    fn photos_are_numbered_listed_read_and_discarded() {
        let d = tempfile::tempdir().unwrap();
        let p = Photos::new(d.path());
        let disc = Owner::Disc("AM3Ds7LYFUg7YH7ywvK9zGrpNsU-".into());
        assert_eq!(p.add(&disc, JPEG, "image/jpeg", "Anna"), Ok(1));
        assert_eq!(p.add(&disc, b"\x89PNG", "image/png", ""), Ok(2));
        assert_eq!(p.list(&disc).iter().map(|x| (x.n, x.mime)).collect::<Vec<_>>(), vec![(1, "image/jpeg"), (2, "image/png")]);
        assert_eq!(p.read(&disc, 1), Some(("image/jpeg", JPEG.to_vec())));
        assert!(p.list(&Owner::Album(3)).is_empty(), "each owner has its own");
        let tasks = p.backlog();
        assert_eq!(tasks.iter().map(|t| (t.owner.clone(), t.photos, t.by.as_str())).collect::<Vec<_>>(), vec![(disc.clone(), 2, "Anna")]);
        assert_eq!(p.discard(&disc), 2);
        assert!(p.backlog().is_empty());
        assert!(p.list(&disc).is_empty());
    }

    #[test]
    fn a_disc_id_that_is_no_disc_id_reaches_no_path_and_the_number_is_limited() {
        let d = tempfile::tempdir().unwrap();
        let p = Photos::new(d.path());
        for bad in ["../x", "a/b", "", "x y"] {
            assert_eq!(p.add(&Owner::Disc(bad.into()), JPEG, "image/jpeg", ""), Err("invalid_disc"), "{bad}");
        }
        let a = Owner::Album(5);
        for _ in 0..MAX_PHOTOS {
            p.add(&a, JPEG, "image/jpeg", "").unwrap();
        }
        assert_eq!(p.add(&a, JPEG, "image/jpeg", ""), Err("photo_limit"));
    }

    #[test]
    fn the_end_of_the_rip_ends_the_photos_of_its_disc_and_the_sweep_takes_what_is_left() {
        let d = tempfile::tempdir().unwrap();
        let p = Photos::new(d.path());
        let (one, two) = (Owner::Disc("A".into()), Owner::Disc("B".into()));
        p.add(&one, JPEG, "image/jpeg", "").unwrap();
        p.add(&two, JPEG, "image/jpeg", "").unwrap();
        p.attach_job("A", 41);
        p.discard_job(40);
        assert_eq!(p.list(&one).len(), 1, "another job ends nothing");
        p.discard_job(41);
        assert!(p.list(&one).is_empty());
        assert_eq!(p.list(&two).len(), 1);
        p.sweep(crate::store::now_ms());
        assert_eq!(p.list(&two).len(), 1, "fresh photos stay");
        p.sweep(crate::store::now_ms() + KEEP_MS + 60_000);
        assert!(p.list(&two).is_empty());
    }
}
