// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Quarantine for duplicates (project owner, 2026-10-28: "Dubletten bekommen einen Quarantäne-Ordner,
//! einen je Config-Eintrag. Dorthin werden sie verschoben."). Every import folder has its own, the hidden
//! folder `.quarantine` inside it, so a move stays on the same disk and is a rename, and the scan does
//! not see it. An album keeps its place below the import folder, so it is clear where it came from.
//! The book leaves the library; to bring it back, move the folder back and import again. Nothing is deleted.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::store::Store;

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub id: i64,
    pub title: String,
    pub author: String,
    pub from: PathBuf,
    pub to: PathBuf,
    /// The copies that stay.
    pub kept: Vec<i64>,
    /// The name of the quarantine folder inside the import folder (a setting).
    pub folder: String,
    pub problem: Option<String>,
}

/// What would go to quarantine for `ids`. Only a book that has a copy in the library can go, and
/// not every copy of a work: at least one stays.
pub async fn plan(store: &Store, library: Option<&Path>, ids: &[i64]) -> Result<Vec<Plan>, String> {
    let rows = store.admin_book_rows().await.map_err(|e| e.to_string())?;
    let groups = store.duplicate_groups().await.map_err(|e| e.to_string())?;
    let state = store.import_state().await.map_err(|e| e.to_string())?;
    let mut roots: Vec<PathBuf> = state.roots.iter().chain(state.music_roots.iter()).filter_map(|r| std::fs::canonicalize(r).ok()).collect();
    // The innermost folder wins: /srv/Musik/Audiobooks lies in /srv/Musik.
    roots.sort_by_key(|r| std::cmp::Reverse(r.components().count()));
    let stamp = crate::store::now_ms() / 1000;
    let folder = store.quarantine_folder().await.map_err(|e| e.to_string())?;
    let mut plans: Vec<Plan> = Vec::new();
    for &id in ids {
        let Some(row) = rows.iter().find(|r| r.id == id) else {
            plans.push(Plan { id, title: String::new(), author: String::new(), from: PathBuf::new(), to: PathBuf::new(), kept: vec![], folder: folder.clone(), problem: Some("no such book".into()) });
            continue;
        };
        let mut plan = Plan { id, title: row.title.clone(), author: row.author.clone(), from: PathBuf::new(), to: PathBuf::new(), kept: vec![], folder: folder.clone(), problem: None };
        // The other copies of the same group; the book must have a copy that is not itself put aside.
        let group = groups.iter().find(|g| g.kind != "parts" && g.books.iter().any(|b| b.id == id));
        plan.kept = group.map(|g| g.books.iter().map(|b| b.id).filter(|b| !ids.contains(b)).collect()).unwrap_or_default();
        if group.is_none() {
            plan.problem = Some("this book is not part of a group of duplicates (see find_duplicates)".into());
        } else if plan.kept.is_empty() {
            plan.problem = Some("every copy of this work would go to quarantine; one has to stay".into());
        }
        match crate::relocate::book_dir(store, library, id).await {
            None => plan.problem = plan.problem.or(Some("the files of the book are missing".into())),
            Some((dir, _, _)) => {
                plan.from = dir.clone();
                match std::fs::canonicalize(&dir).ok().and_then(|real| roots.iter().find(|r| real.starts_with(r)).map(|r| (real.clone(), r.clone()))) {
                    None => plan.problem = plan.problem.or(Some("the folder of the book lies in no import folder".into())),
                    Some((real, root)) => {
                        let relative = real.strip_prefix(&root).unwrap_or(&real).to_owned();
                        if relative.as_os_str().is_empty() {
                            plan.problem = plan.problem.or(Some("the book is the import folder itself".into()));
                        }
                        let mut to = root.join(&folder).join(&relative);
                        if to.exists() {
                            // The same place was used before: keep both.
                            to = root.join(&folder).join(format!("{}-{stamp}", relative.display()));
                        }
                        let fine = |p: &Path| crate::access::may(p, crate::access::WRITE | crate::access::ENTER);
                        let parent = real.parent().map(Path::to_path_buf).unwrap_or_default();
                        if plan.problem.is_none() && !fine(&parent) {
                            plan.problem = Some(format!("the server may not change {} (see the warning about rights)", parent.display()));
                        } else if plan.problem.is_none() && !fine(&root) {
                            plan.problem = Some(format!("the server may not write in {} (see the warning about rights)", root.display()));
                        }
                        plan.to = to;
                    }
                }
            }
        }
        plans.push(plan);
    }
    Ok(plans)
}

/// Moves the folder into quarantine and takes the book out of the library; if the database cannot
/// follow, the folder goes back.
pub async fn execute(store: &Store, plan: &Plan) -> Result<(), String> {
    if let Some(problem) = &plan.problem {
        return Err(problem.clone());
    }
    let (from, to) = (plan.from.clone(), plan.to.clone());
    tokio::task::spawn_blocking(move || crate::relocate::move_dir(&from, &to)).await.map_err(|e| e.to_string())??;
    // On Windows the dot does not hide the folder: an NTFS ACL and the hidden attribute do (see `ntacl`).
    if let Some(root) = plan.to.ancestors().find(|a| a.file_name().is_some_and(|n| n == plan.folder.as_str())).map(Path::to_path_buf) {
        let hidden = tokio::task::spawn_blocking(move || crate::ntacl::hide(&root).map_err(|e| format!("{}: {e}", root.display()))).await;
        if let Ok(Err(e)) = hidden {
            tracing::warn!(error = %e, "the quarantine folder could not be hidden");
        }
    }
    if let Err(e) = store.delete_book(plan.id).await {
        let (from, to) = (plan.to.clone(), plan.from.clone());
        let _ = tokio::task::spawn_blocking(move || crate::relocate::move_dir(&from, &to)).await;
        return Err(format!("database: {e}"));
    }
    crate::relocate::prune_empty_parents(store, &plan.from).await;
    Ok(())
}

/// An album that lies in a quarantine folder.
#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    /// The import folder the quarantine belongs to.
    pub root: PathBuf,
    /// Where it lay below the import folder (the place in the quarantine is the same).
    pub was: String,
    pub audio_files: usize,
    pub megabytes: u64,
    /// `server` when David put it there (with who and when), `by hand` when nobody told David.
    pub put_there_by: String,
    pub at_ms: Option<i64>,
    /// The book of the library that was taken out for it, if David did it.
    pub book_id: Option<i64>,
}

const AUDIO: &[&str] = &["mp3", "m4a", "m4b", "flac", "ogg", "opus"];

/// Every album in the quarantine folders of the import folders, and who put it there: what the
/// change log says David did, the rest was done by hand on the disk.
pub async fn list(store: &Store) -> Result<Vec<Entry>, String> {
    let state = store.import_state().await.map_err(|e| e.to_string())?;
    let roots: Vec<PathBuf> = state.roots.iter().chain(state.music_roots.iter()).map(PathBuf::from).collect();
    let folder = store.quarantine_folder().await.map_err(|e| e.to_string())?;
    let log: Vec<crate::store::TagChange> = store.tag_changes(0, 5000).await.map_err(|e| e.to_string())?.into_iter().filter(|c| c.field == "Quarantine").collect();
    let mut found = Vec::new();
    for root in roots {
        let base = root.join(&folder);
        let entries = tokio::task::spawn_blocking({
            let base = base.clone();
            move || albums_below(&base)
        })
        .await
        .map_err(|e| e.to_string())?;
        for (dir, files, bytes) in entries {
            let was = dir.strip_prefix(&base).unwrap_or(&dir).display().to_string();
            let hit = log.iter().find(|c| Path::new(&c.new_value) == dir);
            found.push(Entry {
                root: root.clone(),
                was,
                audio_files: files,
                megabytes: bytes / 1_000_000,
                put_there_by: hit.map_or_else(|| "by hand".to_owned(), |c| format!("server ({})", c.token_name)),
                at_ms: hit.map(|c| c.at),
                book_id: hit.map(|c| c.book_id),
            });
        }
    }
    found.sort_by_key(|e| e.was.to_lowercase());
    Ok(found)
}

/// The folders below `dir` that hold audio files, with their number of audio files and their bytes.
fn albums_below(dir: &Path) -> Vec<(PathBuf, usize, u64)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    let (mut files, mut bytes) = (0usize, 0u64);
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            out.extend(albums_below(&path));
        } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| AUDIO.contains(&e.to_ascii_lowercase().as_str())) {
            files += 1;
            bytes += entry.metadata().map_or(0, |m| m.len());
        }
    }
    if files > 0 {
        out.push((dir.to_path_buf(), files, bytes));
    }
    out
}
