// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! What the server may do with the files of the library. Reading is needed for playing; writing for
//! tags, covers and moving albums. On a NAS the rights usually come from ACLs, so a folder can look
//! fine and still refuse the service user. The check asks the system (`access(2)`, which honours
//! ACLs) and names the folders where something is missing, with the command that fixes it. The
//! administration interface shows the result as a warning.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::store::Store;

const AUDIO: &[&str] = &["mp3", "m4a", "m4b", "flac", "ogg", "opus"];
const MAX_DEPTH: usize = 8;
/// A result is used this long before the folders are looked at again.
const FRESH: Duration = Duration::from_secs(600);
/// Folders named in a report; the rest is only counted.
const LISTED: usize = 40;

/// One folder with audio files where the service user lacks a right.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Finding {
    pub path: String,
    /// New files (a cover) and moving need the folder itself to be writable.
    pub folder_writable: bool,
    pub files: usize,
    pub not_readable: usize,
    pub not_writable: usize,
    pub example: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct Report {
    /// The user the server runs as.
    pub user: String,
    pub roots: Vec<String>,
    pub folders_checked: usize,
    pub files_checked: usize,
    /// At most `LISTED` folders, the worst first (not readable, then not writable).
    pub findings: Vec<Finding>,
    /// How many folders have a problem in all.
    pub folders_with_problems: usize,
    /// Roots that cannot be read at all.
    pub unreadable_roots: Vec<String>,
    /// Commands that give the user the rights, one per root.
    pub fixes: Vec<String>,
}

impl Report {
    pub fn is_ok(&self) -> bool {
        self.folders_with_problems == 0 && self.unreadable_roots.is_empty()
    }
}

/// The name of the user the server runs as, as far as the environment tells.
fn user_name() -> String {
    std::env::var("USER").or_else(|_| std::env::var("LOGNAME")).ok().filter(|u| !u.is_empty()).unwrap_or_else(|| "david".to_owned())
}

#[cfg(unix)]
pub(crate) fn may(path: &Path, mode: i32) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return false };
    // SAFETY: `c` is a valid NUL-terminated string that outlives the call; `access` only reads it.
    unsafe { libc::access(c.as_ptr(), mode) == 0 }
}

#[cfg(unix)]
pub(crate) const READ: i32 = libc::R_OK;
#[cfg(unix)]
pub(crate) const WRITE: i32 = libc::W_OK;
#[cfg(unix)]
pub(crate) const ENTER: i32 = libc::X_OK;

#[cfg(not(unix))]
pub(crate) fn may(path: &Path, mode: i32) -> bool {
    // Windows has no `access` with ACLs here: only the read-only attribute is known.
    path.metadata().is_ok_and(|m| mode & 2 == 0 || !m.permissions().readonly())
}

#[cfg(not(unix))]
pub(crate) const READ: i32 = 4;
#[cfg(not(unix))]
pub(crate) const WRITE: i32 = 2;
#[cfg(not(unix))]
pub(crate) const ENTER: i32 = 1;

fn is_audio(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| AUDIO.contains(&e.to_ascii_lowercase().as_str()))
}

/// Looks at every folder below `roots` (blocking). `skip` folders are left out, like in the scan.
pub fn check(roots: &[PathBuf], skip: &[PathBuf]) -> Report {
    let user = user_name();
    let mut report = Report { user: user.clone(), roots: roots.iter().map(|r| r.display().to_string()).collect(), ..Report::default() };
    let mut findings: Vec<Finding> = Vec::new();
    for root in roots {
        if !may(root, READ | ENTER) {
            report.unreadable_roots.push(root.display().to_string());
        } else {
            walk(root, 1, skip, &mut report, &mut findings);
        }
        report.fixes.push(format!("sudo setfacl -R -m u:{user}:rwX \"{0}\" && sudo setfacl -R -d -m u:{user}:rwX \"{0}\"", root.display()));
    }
    findings.sort_by(|a, b| b.not_readable.cmp(&a.not_readable).then(b.not_writable.cmp(&a.not_writable)).then(a.path.cmp(&b.path)));
    report.folders_with_problems = findings.len();
    findings.truncate(LISTED);
    report.findings = findings;
    report
}

fn walk(dir: &Path, depth: usize, skip: &[PathBuf], report: &mut Report, findings: &mut Vec<Finding>) {
    if depth > MAX_DEPTH || skip.iter().any(|s| s == dir) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let (mut files, mut subdirs) = (Vec::new(), Vec::new());
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            if !name.starts_with('.') && name != "@eaDir" && name != "lost+found" {
                subdirs.push(path);
            }
        } else if path.is_file() && is_audio(&path) {
            files.push(path);
        }
    }
    if !files.is_empty() {
        report.folders_checked += 1;
        report.files_checked += files.len();
        let folder_writable = may(dir, WRITE | ENTER);
        let not_readable: Vec<&PathBuf> = files.iter().filter(|f| !may(f, READ)).collect();
        let not_writable: Vec<&PathBuf> = files.iter().filter(|f| !may(f, WRITE)).collect();
        if !folder_writable || !not_readable.is_empty() || !not_writable.is_empty() {
            let example = not_readable.first().or(not_writable.first()).and_then(|f| f.file_name()).map(|n| n.to_string_lossy().into_owned());
            findings.push(Finding {
                path: dir.display().to_string(),
                folder_writable,
                files: files.len(),
                not_readable: not_readable.len(),
                not_writable: not_writable.len(),
                example,
            });
        }
    }
    for sub in subdirs {
        walk(&sub, depth + 1, skip, report, findings);
    }
}

/// The last result and the folders it was made for; one per store.
pub type Cache = Mutex<Option<(Vec<String>, Instant, Report)>>;

/// The folders to look at: the audiobook and the music folders, and what they leave out.
async fn folders(store: &Store) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let list = |key: &'static str| async move {
        store.setting(key).await.ok().flatten().and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok()).unwrap_or_default()
    };
    let mut roots = list("import_roots").await;
    roots.extend(list("music_roots").await);
    let mut skip = list("import_excludes").await;
    skip.extend(list("music_excludes").await);
    (roots.into_iter().map(PathBuf::from).collect(), skip.into_iter().map(PathBuf::from).collect())
}

/// Looks again now and remembers the result.
pub async fn refresh(store: &Store) -> Report {
    let (roots, skip) = folders(store).await;
    let key: Vec<String> = roots.iter().map(|r| r.display().to_string()).collect();
    let report = tokio::task::spawn_blocking(move || check(&roots, &skip)).await.unwrap_or_default();
    if let Ok(mut cache) = store.access_cache.lock() {
        *cache = Some((key, Instant::now(), report.clone()));
    }
    report
}

/// The last result for the current folders, for a page. A page never waits for the walk: without a
/// fresh result it starts one in the background and shows what it has (nothing, the first time).
pub async fn current(store: &Store) -> Option<Report> {
    let (roots, _) = folders(store).await;
    let key: Vec<String> = roots.iter().map(|r| r.display().to_string()).collect();
    let (known, fresh) = match store.access_cache.lock() {
        Ok(cache) => match cache.as_ref() {
            Some((k, at, report)) if *k == key => (Some(report.clone()), at.elapsed() < FRESH),
            _ => (None, false),
        },
        Err(_) => (None, true),
    };
    if !fresh && !roots.is_empty() {
        let store = store.clone();
        tokio::spawn(async move {
            refresh(&store).await;
        });
    }
    known
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn names_the_folders_where_a_right_is_missing_and_how_to_fix_it() {
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("Artist/Good");
        let bad = dir.path().join("Artist/Bad");
        std::fs::create_dir_all(&good).unwrap();
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(good.join("01.mp3"), b"x").unwrap();
        std::fs::write(bad.join("01.mp3"), b"x").unwrap();
        std::fs::set_permissions(bad.join("01.mp3"), std::fs::Permissions::from_mode(0o444)).unwrap();
        // A user that may write everything (root in a container) cannot show the problem.
        if may(&bad.join("01.mp3"), WRITE) {
            return;
        }
        let report = check(&[dir.path().to_owned()], &[]);
        assert_eq!((report.folders_checked, report.files_checked, report.folders_with_problems), (2, 2, 1));
        let f = &report.findings[0];
        assert!(f.path.ends_with("Artist/Bad") && f.not_writable == 1 && f.not_readable == 0 && f.folder_writable, "{f:?}");
        assert!(report.fixes[0].contains("setfacl -R -m u:") && report.fixes[0].contains(&dir.path().display().to_string()));
        assert!(!report.is_ok());
    }

    #[test]
    fn a_library_where_everything_is_allowed_gives_no_warning() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("A/B")).unwrap();
        std::fs::write(dir.path().join("A/B/01.mp3"), b"x").unwrap();
        assert!(check(&[dir.path().to_owned()], &[]).is_ok());
    }
}
