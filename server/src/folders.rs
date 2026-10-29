// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The folders of the profiles (project owner, 2026-10-29): for every member of the family and every kind of medium a folder, chosen in a dialog that
//! lists **only what the server can read, write and enter** (0777 for the user it runs as). The same check holds when a path is saved.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::access::{ENTER, READ, WRITE, may};
use crate::error::Result;
use crate::store::Store;

/// The kinds of medium a profile has a folder for.
pub const KINDS: [&str; 3] = ["audiobook", "music", "photo"];

/// May the server read, write and enter this folder?
pub fn usable(path: &Path) -> bool {
    path.is_dir() && may(path, READ | WRITE | ENTER)
}

/// One folder of the list.
#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub name: String,
    pub path: String,
    /// `path=<the path, encoded>`: the part of the address that asks for this folder.
    pub q: String,
}

fn query(path: &str) -> String {
    serde_urlencoded::to_string([("path", path)]).unwrap_or_default()
}

/// What the dialog shows: the folder it is in, the way up (only to a folder that is usable itself), and the usable folders below.
#[derive(Debug, Clone, Serialize)]
pub struct Browse {
    /// The folder shown; empty at the start (the list of the places the server works in).
    pub path: String,
    pub parent: Option<String>,
    /// The address part for the way up (`path=...`); empty when the way up is the start.
    pub parent_q: String,
    /// `true` when `path` can be chosen (it is a usable folder).
    pub choosable: bool,
    pub folders: Vec<Entry>,
}

/// Why a path cannot be taken.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FolderError {
    #[error("the path must be absolute")]
    NotAbsolute,
    #[error("there is no such folder")]
    NoSuchFolder,
    #[error("the server cannot read, write and enter this folder")]
    NotUsable,
}

/// A path as it is saved: absolute, an existing folder that the server can read, write and enter; symbolic links resolved.
pub fn check(path: &str) -> std::result::Result<PathBuf, FolderError> {
    let p = Path::new(path.trim());
    if !p.is_absolute() {
        return Err(FolderError::NotAbsolute);
    }
    let real = std::fs::canonicalize(p).map_err(|_| FolderError::NoSuchFolder)?;
    if !real.is_dir() {
        return Err(FolderError::NoSuchFolder);
    }
    if !usable(&real) {
        return Err(FolderError::NotUsable);
    }
    Ok(real)
}

/// Why a folder cannot be created.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CreateError {
    #[error("the parent folder cannot be used")]
    Parent(#[from] FolderError),
    #[error("the name is not a plain folder name")]
    BadName,
    #[error("there is a folder or file of this name already")]
    Exists,
    #[error("the folder could not be created")]
    Failed,
}

/// A name for a new folder: one path component, no separators, no dots-only name, no control characters.
pub fn valid_name(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty() && n.len() <= 255 && n != "." && n != ".." && !n.chars().any(|c| c == '/' || c == '\\' || c.is_control())
}

/// Creates the folder `name` below `parent` (which must be usable) and returns its path (blocking). A folder that exists is not an error
/// for the person's purpose, but it is reported, so that nothing is taken over by accident.
pub fn create(parent: &str, name: &str) -> std::result::Result<PathBuf, CreateError> {
    let parent = check(parent)?;
    if !valid_name(name) {
        return Err(CreateError::BadName);
    }
    let path = parent.join(name.trim());
    match std::fs::create_dir(&path) {
        Ok(()) => Ok(path),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(CreateError::Exists),
        Err(_) => Err(CreateError::Failed),
    }
}

/// The usable folders directly below `dir` (blocking), by name, without hidden ones (a name that starts with a dot).
fn children(dir: &Path) -> Vec<Entry> {
    let Ok(read) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Entry> = read
        .filter_map(|e| e.ok())
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| e.path())
        // A link that leads out is not followed blindly: the folder it leads to is the one that is checked.
        .filter_map(|p| std::fs::canonicalize(&p).ok().map(|real| (p, real)))
        .filter(|(_, real)| usable(real))
        .map(|(p, _)| Entry { name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), path: p.to_string_lossy().into_owned(), q: query(&p.to_string_lossy()) })
        .collect();
    out.sort_by_key(|e| e.name.to_lowercase());
    out
}

/// The list for the dialog. Without a path: the places the server works in (the import folders that it can use, and folders chosen already), so
/// that a person finds the way even when the parents of those places are closed to the server. With a path: the usable folders below it.
pub fn browse(path: &str, starts: &[PathBuf]) -> std::result::Result<Browse, FolderError> {
    let path = path.trim();
    if path.is_empty() {
        let mut folders: Vec<Entry> = starts
            .iter()
            .filter(|p| usable(p))
            .map(|p| Entry { name: p.to_string_lossy().into_owned(), path: p.to_string_lossy().into_owned(), q: query(&p.to_string_lossy()) })
            .collect();
        folders.sort_by(|a, b| a.path.cmp(&b.path));
        folders.dedup_by(|a, b| a.path == b.path);
        return Ok(Browse { path: String::new(), parent: None, parent_q: String::new(), choosable: false, folders });
    }
    let real = check(path)?;
    let parent = real.parent().filter(|p| usable(p)).map(|p| p.to_string_lossy().into_owned());
    let parent_q = parent.as_deref().map(query).unwrap_or_default();
    Ok(Browse { path: real.to_string_lossy().into_owned(), parent, parent_q, choosable: true, folders: children(&real) })
}

impl Store {
    /// The folders of a profile by kind.
    pub async fn profile_folders(&self, profile_id: i64) -> Result<Vec<(String, String)>> {
        use sea_orm::{ConnectionTrait, DbBackend, Statement};
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT kind, path FROM profile_folder WHERE profile_id = ? ORDER BY kind", [profile_id.into()]))
            .await?;
        rows.iter().map(|r| Ok((r.try_get_by_index(0)?, r.try_get_by_index(1)?))).collect()
    }

    /// The folders of all profiles: profile id (as text, for the templates) -> kind -> path.
    pub async fn all_profile_folders(&self) -> Result<std::collections::HashMap<String, std::collections::HashMap<String, String>>> {
        use sea_orm::{ConnectionTrait, DbBackend, Statement};
        let rows = self.db.reader.query_all_raw(Statement::from_string(DbBackend::Sqlite, "SELECT profile_id, kind, path FROM profile_folder")).await?;
        let mut out: std::collections::HashMap<String, std::collections::HashMap<String, String>> = std::collections::HashMap::new();
        for r in rows {
            let (id, kind, path): (i64, String, String) = (r.try_get_by_index(0)?, r.try_get_by_index(1)?, r.try_get_by_index(2)?);
            out.entry(id.to_string()).or_default().insert(kind, path);
        }
        Ok(out)
    }

    /// Sets the folder of a profile for a kind of medium, or removes it (`None`). The path is not checked here (see [`check`]).
    pub async fn set_profile_folder(&self, profile_id: i64, kind: &str, path: Option<&str>) -> Result<()> {
        use sea_orm::{ConnectionTrait, DbBackend, Statement};
        match path {
            Some(p) => {
                self.db
                    .writer
                    .execute_raw(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "INSERT INTO profile_folder (profile_id, kind, path) VALUES (?, ?, ?) ON CONFLICT (profile_id, kind) DO UPDATE SET path = excluded.path",
                        [profile_id.into(), kind.to_owned().into(), p.to_owned().into()],
                    ))
                    .await?;
            }
            None => {
                self.db.writer.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "DELETE FROM profile_folder WHERE profile_id = ? AND kind = ?", [profile_id.into(), kind.to_owned().into()])).await?;
            }
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn mode(p: &Path, m: u32) {
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
    }

    #[test]
    fn only_folders_the_server_can_read_write_and_enter_are_listed_and_taken() {
        let root = tempfile::tempdir().unwrap();
        for d in ["Musik", "Hörbücher", "nur-lesbar", "ohne-zutritt", ".versteckt"] {
            std::fs::create_dir(root.path().join(d)).unwrap();
        }
        std::fs::write(root.path().join("datei.txt"), b"x").unwrap();
        mode(&root.path().join("nur-lesbar"), 0o555);
        mode(&root.path().join("ohne-zutritt"), 0o000);
        let b = browse(root.path().to_str().unwrap(), &[]).unwrap();
        let names: Vec<&str> = b.folders.iter().map(|e| e.name.as_str()).collect();
        // (A test that runs as root can write everywhere: then only the fixed facts are checked.)
        assert!(names.contains(&"Musik") && names.contains(&"Hörbücher"), "{names:?}");
        assert!(!names.contains(&"datei.txt") && !names.contains(&".versteckt"), "a file and a hidden folder are not listed: {names:?}");
        let root_user = std::fs::metadata(root.path()).unwrap().uid_is_root();
        if !root_user {
            assert!(!names.contains(&"nur-lesbar") && !names.contains(&"ohne-zutritt"), "{names:?}");
            assert_eq!(check(root.path().join("nur-lesbar").to_str().unwrap()), Err(FolderError::NotUsable));
        }
        assert!(b.choosable);
        // Saving: an absolute path of a usable folder; links are resolved; a relative path, a file and a missing folder are refused.
        assert_eq!(check(root.path().join("Musik").to_str().unwrap()).unwrap(), std::fs::canonicalize(root.path().join("Musik")).unwrap());
        assert_eq!(check("Musik"), Err(FolderError::NotAbsolute));
        assert_eq!(check(root.path().join("datei.txt").to_str().unwrap()), Err(FolderError::NoSuchFolder));
        assert_eq!(check(root.path().join("gibt-es-nicht").to_str().unwrap()), Err(FolderError::NoSuchFolder));
        mode(&root.path().join("nur-lesbar"), 0o755);
        mode(&root.path().join("ohne-zutritt"), 0o755);
    }

    #[test]
    fn the_start_lists_the_places_the_server_works_in_and_the_way_up_stops_at_a_closed_folder() {
        let root = tempfile::tempdir().unwrap();
        let musik = root.path().join("Musik");
        std::fs::create_dir(&musik).unwrap();
        let start = browse("", &[musik.clone(), PathBuf::from("/nonexistent/place")]).unwrap();
        assert_eq!((start.path.as_str(), start.choosable, start.folders.len()), ("", false, 1));
        assert_eq!(start.folders[0].path, musik.to_string_lossy());
        let inside = browse(musik.to_str().unwrap(), &[]).unwrap();
        assert_eq!(inside.parent.as_deref(), std::fs::canonicalize(root.path()).unwrap().to_str(), "the parent is usable, so the way up is offered");
        // A folder the server may not write is not shown and cannot be entered by its path either.
        assert_eq!(browse("/proc", &[]).map(|b| b.choosable).unwrap_or(false), false);
    }

    #[test]
    fn a_folder_is_created_below_a_usable_folder_with_a_plain_name_only() {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().to_str().unwrap();
        let made = create(base, " Neu ").unwrap();
        assert!(made.is_dir() && made.ends_with("Neu"));
        assert_eq!(create(base, "Neu"), Err(CreateError::Exists));
        for bad in ["", "  ", ".", "..", "a/b", "../x", "a\\b", "tab\there"] {
            assert_eq!(create(base, bad), Err(CreateError::BadName), "{bad:?}");
        }
        assert_eq!(create("relativ", "x"), Err(CreateError::Parent(FolderError::NotAbsolute)));
        assert_eq!(create(root.path().join("gibt-es-nicht").to_str().unwrap(), "x"), Err(CreateError::Parent(FolderError::NoSuchFolder)));
    }

    trait RootCheck {
        fn uid_is_root(&self) -> bool;
    }
    impl RootCheck for std::fs::Metadata {
        fn uid_is_root(&self) -> bool {
            // The test directory belongs to the user that runs the test; root is the one that ignores the modes.
            std::os::unix::fs::MetadataExt::uid(self) == 0
        }
    }
}
