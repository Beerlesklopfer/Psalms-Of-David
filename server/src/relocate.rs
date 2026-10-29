// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Moves albums that are really audiobooks out of the music folders into the audiobook folder
//! (project owner, 2026-10-28: "David muss, wenn es ein Hörbuch erkennt, es auch nach Audiobooks
//! verschieben"). An album counts as an audiobook when the genre tag of its first file says so. It
//! goes to `<audiobook folder>/<author>/<title>`, named by its tags, and the book keeps its id, so
//! the listening progress stays. `plan` only looks; nothing moves before `execute`.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::store::Store;

/// Does the genre tag contain one of the words that make an album an audiobook? The words are a
/// setting (`audiobook_genres`), already in lower case.
pub fn is_audiobook_genre(genre: &str, words: &[String]) -> bool {
    let g = genre.to_lowercase();
    words.iter().any(|w| g.contains(w.as_str()))
}

/// What would happen to one album. Albums of one work that are split into CDs (`Title (CD 1)`,
/// `Title CD2`, ...) are put together: they become one audiobook `Author/Title` with a folder `CD n`
/// for each disc, and the book of the first disc stays (with its id and progress).
#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub id: i64,
    pub genre: String,
    pub author: String,
    /// The title of the book it ends up in (without a CD number if CDs are put together).
    pub title: String,
    pub from: PathBuf,
    /// Where this album's folder goes: `<audiobook folder>/Author/Title`, or `.../Title/CD n`.
    pub to: PathBuf,
    /// The place of the book below the audiobook folder (`Author/Title`), as the database keeps it.
    pub new_folder: String,
    /// The disc this album becomes when CDs are put together.
    pub disc: Option<i64>,
    /// The book that remains: this one, or the first disc of the group.
    pub keep: i64,
    /// Why it cannot be moved; the album stays where it is.
    pub problem: Option<String>,
}

/// A name that is safe as one folder name.
fn folder_name(raw: &str) -> Option<String> {
    let cleaned: String = raw.chars().map(|c| if c == '/' || c == '\\' || c == ':' || c.is_control() { '-' } else { c }).collect();
    let cleaned = cleaned.trim().trim_matches('.').trim().to_owned();
    let cleaned: String = cleaned.chars().take(150).collect();
    (!cleaned.is_empty()).then_some(cleaned)
}

/// `CD 3`, `cd3`, `Disc 2`, `Teil 4` -> the number.
fn disc_number(word: &str) -> Option<i64> {
    let w = word.trim().to_lowercase();
    let rest = ["cd", "disc", "disk", "teil"].iter().find_map(|p| w.strip_prefix(p))?;
    let digits = rest.trim().trim_end_matches('.');
    (!digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())).then(|| digits.parse().ok()).flatten().filter(|n| *n >= 1)
}

/// A title without its CD number, and the number: `Foo (CD 2)` and `Foo CD02` give `Foo` and 2.
pub(crate) fn strip_disc(title: &str) -> (String, Option<i64>) {
    let t = title.trim();
    let trim = |s: &str| s.trim().trim_end_matches(['-', ',', ' ']).trim().to_owned();
    let bracketed = t.strip_suffix(')').and_then(|x| x.rsplit_once('(')).or_else(|| t.strip_suffix(']').and_then(|x| x.rsplit_once('[')));
    if let Some(inner) = bracketed
        && let Some(n) = disc_number(inner.1)
    {
        return (trim(inner.0), Some(n));
    }
    let words: Vec<&str> = t.split_whitespace().collect();
    if let Some(last) = words.last() {
        if let Some(n) = disc_number(last) {
            return (trim(&words[..words.len() - 1].join(" ")), Some(n));
        }
        if words.len() >= 2
            && last.chars().all(|c| c.is_ascii_digit())
            && let Some(n) = disc_number(&format!("{}{last}", words[words.len() - 2]))
        {
            return (trim(&words[..words.len() - 2].join(" ")), Some(n));
        }
    }
    (t.to_owned(), None)
}

/// The folder of an album on the disk (the folder above its first track, however deep the track lies).
pub(crate) async fn book_dir(store: &Store, library: Option<&Path>, id: i64) -> Option<(PathBuf, PathBuf, bool)> {
    let folder = store.book_folder(id).await.ok()??;
    let tracks = store.tracks_of(id).await.ok()?;
    let first = tracks.first()?;
    let path = crate::api::resolve(store, library, &folder, &first.file).await?;
    let depth = first.file.split('/').count();
    // Tracks in folders of their own (`CD 1/01.mp3`), or on several discs, make merging unsafe.
    let flat = tracks.iter().all(|t| !t.file.contains('/') && t.disc == first.disc);
    Some((path.ancestors().nth(depth)?.to_path_buf(), path, flat))
}

struct Candidate {
    id: i64,
    genre: String,
    author: Option<String>,
    title: String,
    dir: PathBuf,
    dir_name: String,
    flat: bool,
}

/// Looks at the music albums (only `ids` if given) and says which are audiobooks and where they would go.
pub async fn plan(store: &Store, library: Option<&Path>, ids: Option<&[i64]>) -> Result<Vec<Plan>, String> {
    let state = store.import_state().await.map_err(|e| e.to_string())?;
    let root = PathBuf::from(state.roots.first().ok_or("no audiobook folder is set in the import settings")?);
    let music_roots: Vec<PathBuf> = state.music_roots.iter().filter_map(|r| std::fs::canonicalize(r).ok()).collect();
    let words = store.audiobook_genres().await.map_err(|e| e.to_string())?;
    let placeholders = store.placeholder_tags().await.map_err(|e| e.to_string())?;
    let rows = store.admin_book_rows().await.map_err(|e| e.to_string())?;
    let mut found: Vec<Candidate> = Vec::new();
    for row in rows.into_iter().filter(|r| r.kind == "music" && ids.is_none_or(|ids| ids.contains(&r.id))) {
        let Some((dir, first_file, flat)) = book_dir(store, library, row.id).await else { continue };
        let fields = tokio::task::spawn_blocking({
            let f = first_file.clone();
            move || crate::tags::read_fields(&f).unwrap_or_default()
        })
        .await
        .map_err(|e| e.to_string())?;
        let value = |name: &str| fields.iter().find(|(n, _)| n == name).and_then(|(_, v)| v.first()).map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
        let genre = value("Genre").unwrap_or_default();
        if !is_audiobook_genre(&genre, &words) && ids.is_none() {
            continue;
        }
        // The author comes from the tags only: the folder above an album is often "Hörbücher" or "Schule".
        let author = placeholders.real(value("AlbumArtist")).or_else(|| placeholders.real(value("TrackArtist")));
        let title = value("AlbumTitle").unwrap_or_else(|| row.title.clone());
        let dir_name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        found.push(Candidate { id: row.id, genre, author, title, dir, dir_name, flat });
    }

    // CDs of one work: the same author and the same title without the CD number.
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, c) in found.iter().enumerate() {
        let key = format!("{}\u{1}{}", c.author.as_deref().unwrap_or("").to_lowercase(), strip_disc(&c.title).0.to_lowercase());
        match groups.iter_mut().find(|g| g.0 == key) {
            Some(g) => g.1.push(i),
            None => groups.push((key, vec![i])),
        }
    }

    let mut plans: Vec<Plan> = Vec::new();
    for (_, members) in groups {
        let first = &found[members[0]];
        // Disc numbers: from the titles, else from the folder names.
        let mut numbered: Vec<(usize, Option<i64>)> = members.iter().map(|&i| (i, strip_disc(&found[i].title).1.or_else(|| strip_disc(&found[i].dir_name).1))).collect();
        let mut seen = std::collections::HashSet::new();
        // Without a CD number on every album, or with a number twice, the albums are copies of each
        // other rather than the CDs of one work: they are not put together.
        let unnumbered = numbered.iter().any(|(_, n)| n.is_none_or(|n| !seen.insert(n)));
        let together = members.len() > 1 && first.author.is_some() && !unnumbered;
        let base = if together { strip_disc(&first.title).0 } else { first.title.clone() };
        numbered.sort_by_key(|(_, n)| *n);
        let keep = found[numbered[0].0].id;
        let mut group_plans: Vec<Plan> = Vec::new();
        for (i, disc) in numbered {
            let c = &found[i];
            let mut plan = Plan {
                id: c.id,
                genre: c.genre.clone(),
                author: c.author.clone().unwrap_or_default(),
                title: base.clone(),
                from: c.dir.clone(),
                to: PathBuf::new(),
                new_folder: String::new(),
                disc: together.then_some(disc).flatten(),
                keep: if together { keep } else { c.id },
                problem: None,
            };
            match (c.author.as_deref().and_then(folder_name), folder_name(&base)) {
                (Some(a), Some(t)) => {
                    plan.new_folder = format!("{a}/{t}");
                    plan.to = if together { root.join(&a).join(&t).join(format!("CD {}", disc.unwrap_or(1))) } else { root.join(&a).join(&t) };
                }
                (None, _) => plan.problem = Some("the tags name no author (AlbumArtist or TrackArtist); set it first".into()),
                _ => plan.problem = Some("the tags give no usable title".into()),
            }
            if plan.problem.is_none() && members.len() > 1 && unnumbered {
                plan.problem = Some("another album has the same author and title but there are no CD numbers: probably a copy, see find_duplicates".into());
            }
            if plan.problem.is_none() && !is_audiobook_genre(&plan.genre, &words) {
                plan.problem = Some("the genre tag does not say audiobook".into());
            }
            if plan.problem.is_none() && together && !c.flat {
                plan.problem = Some("the album has folders or discs of its own, so it cannot be put together with its CDs".into());
            }
            if plan.problem.is_none() {
                plan.problem = blocked(&c.dir, &plan.to, &music_roots, &plans);
            }
            group_plans.push(plan);
        }
        // One blocked CD blocks the whole book: it is moved complete or not at all.
        if let Some(why) = group_plans.iter().find_map(|p| p.problem.clone()).filter(|_| group_plans.len() > 1) {
            for p in group_plans.iter_mut().filter(|p| p.problem.is_none()) {
                p.problem = Some(format!("another CD of this book is blocked: {why}"));
            }
        }
        plans.extend(group_plans);
    }
    plans.sort_by_key(|p| p.id);
    Ok(plans)
}

/// Why `from` cannot go to `to`, if it cannot.
fn blocked(from: &Path, to: &Path, music_roots: &[PathBuf], earlier: &[Plan]) -> Option<String> {
    let real = std::fs::canonicalize(from).ok()?;
    if !music_roots.iter().any(|r| real.starts_with(r)) {
        return Some("the album does not lie in a music folder".into());
    }
    if to.exists() {
        return Some(format!("{} exists already", to.display()));
    }
    if earlier.iter().any(|p| p.problem.is_none() && p.to == to) {
        return Some("another album would go to the same place".into());
    }
    let parent = from.parent()?;
    if !crate::access::may(parent, crate::access::WRITE | crate::access::ENTER) {
        return Some(format!("the server may not change {} (see the warning about rights)", parent.display()));
    }
    let mut existing = to.parent();
    while let Some(p) = existing {
        if p.exists() {
            break;
        }
        existing = p.parent();
    }
    match existing {
        Some(p) if crate::access::may(p, crate::access::WRITE | crate::access::ENTER) => None,
        Some(p) => Some(format!("the server may not write in {} (see the warning about rights)", p.display())),
        None => Some("the audiobook folder cannot be reached".into()),
    }
}

/// Moves one book: a single album, or all the CDs of a work (`members` share `keep`). The folders
/// move first; if one cannot, or the database cannot follow, the ones already moved go back.
pub async fn execute(store: &Store, members: &[&Plan]) -> Result<(), String> {
    if let Some(problem) = members.iter().find_map(|p| p.problem.as_ref()) {
        return Err(problem.clone());
    }
    let mut moved: Vec<&Plan> = Vec::new();
    for plan in members {
        let (from, to) = (plan.from.clone(), plan.to.clone());
        let result = tokio::task::spawn_blocking(move || move_dir(&from, &to)).await.map_err(|e| e.to_string())?;
        if let Err(e) = result {
            undo(&moved).await;
            return Err(e);
        }
        moved.push(plan);
    }
    let first = members[0];
    let outcome = if first.disc.is_none() {
        store.relocate_book(first.id, &first.new_folder, &first.author, &first.title).await
    } else {
        let discs: Vec<(i64, i64)> = members.iter().map(|p| (p.id, p.disc.unwrap_or(1))).collect();
        store.merge_books(first.keep, &discs, &first.new_folder, &first.author, &first.title).await
    };
    if let Err(e) = outcome {
        undo(&moved).await;
        return Err(format!("database: {e}"));
    }
    for plan in &moved {
        prune_empty_parents(store, &plan.from).await;
    }
    Ok(())
}

/// What a rename would do to one album: the folder is only the filing (the ID tags decide author and title,
/// project owner 2026-10-28), but it is kept as `<import folder>/.../Author/Title`, so a new author or title
/// means a new place. Only the place changes; author and title stay as the tags say, and the id stays with
/// the listening progress.
#[derive(Debug, Clone, Serialize)]
pub struct Rename {
    pub id: i64,
    pub author: String,
    pub title: String,
    pub from: PathBuf,
    pub to: PathBuf,
    /// The place of the album below its import folder, as the database keeps it.
    pub new_folder: String,
    /// Why it cannot be done; nothing changes then.
    pub problem: Option<String>,
}

/// Looks at what renaming an album (a new `author` and/or `title`) would do. Nothing changes before `execute_rename`.
pub async fn plan_rename(store: &Store, library: Option<&Path>, id: i64, author: Option<&str>, title: Option<&str>) -> Result<Rename, String> {
    let row = store.admin_book_row(id).await.map_err(|e| e.to_string())?.ok_or("no such album")?;
    let (dir, _, _) = book_dir(store, library, id).await.ok_or("the files of the album cannot be found on the disk")?;
    let old_folder = store.book_folder(id).await.map_err(|e| e.to_string())?.ok_or("no such album")?;
    let mut names: Vec<String> = old_folder.split('/').map(str::to_owned).collect();
    let mut plan = Rename { id, author: row.author.clone(), title: row.title.clone(), from: dir.clone(), to: dir.clone(), new_folder: old_folder.clone(), problem: None };
    // The author and the title of the book are not touched; only the folder names follow what was asked.
    let clean = |what: &str, raw: &str| folder_name(raw).ok_or_else(|| format!("{what} gives no usable folder name"));
    if let Some(t) = title.map(str::trim).filter(|t| !t.is_empty()) {
        let name = clean("the title", t)?;
        *names.last_mut().expect("a folder has a name") = name;
    }
    if let Some(a) = author.map(str::trim).filter(|a| !a.is_empty()) {
        let name = clean("the author", a)?;
        // The author is the folder above the album; an album straight in the import folder gets one.
        if names.len() >= 2 {
            let at = names.len() - 2;
            names[at] = name;
        } else {
            names.insert(0, name);
        }
    }
    plan.new_folder = names.join("/");
    let depth = old_folder.split('/').count();
    let root = dir.ancestors().nth(depth).ok_or("the import folder of the album cannot be found")?.to_path_buf();
    plan.to = names.iter().fold(root.clone(), |p, n| p.join(n));
    check_place(store, &mut plan, &old_folder).await?;
    Ok(plan)
}

/// Fills in `problem` of a plan: nothing to change, outside the import folders, inside itself, a place that exists, or no right to write.
async fn check_place(store: &Store, plan: &mut Rename, old_folder: &str) -> Result<(), String> {
    if plan.new_folder == old_folder {
        plan.problem = Some("nothing to change".into());
        return Ok(());
    }
    if plan.to != plan.from {
        let state = store.import_state().await.map_err(|e| e.to_string())?;
        let roots: Vec<PathBuf> = state.roots.iter().chain(state.music_roots.iter()).filter_map(|r| std::fs::canonicalize(r).ok()).collect();
        let real = std::fs::canonicalize(&plan.from).map_err(|e| e.to_string())?;
        plan.problem = if !roots.iter().any(|r| real.starts_with(r)) {
            Some("the album does not lie in an import folder".into())
        } else if plan.to.starts_with(&plan.from) {
            Some("the new place lies inside the old one".into())
        } else if plan.to.exists() {
            Some(format!("{} exists already", plan.to.display()))
        } else {
            blocked_write(&plan.from, &plan.to)
        };
    }
    Ok(())
}

/// What moving a whole branch of the filing would do: every album whose place is `from` or lies below it goes to the same place below `to`
/// (`JÖRG/MP3-I/Reinhard Mey/Ikarus` -> `JÖRG/Reinhard Mey/Ikarus` for `from = JÖRG/MP3-I`, `to = JÖRG`). Places are written as the
/// database keeps them: below the import folder, with `/`. An empty `to` is the import folder itself. The ids stay, so progress and releases stay.
/// Nothing changes before `execute_rename` (one album after the other).
pub async fn plan_move_branch(store: &Store, library: Option<&Path>, from: &str, to: &str) -> Result<Vec<Rename>, String> {
    let clean = |raw: &str| -> Result<String, String> {
        let parts: Vec<&str> = raw.split('/').map(str::trim).filter(|p| !p.is_empty()).collect();
        if parts.iter().any(|p| *p == "." || *p == "..") {
            return Err("a place must not contain . or ..".into());
        }
        Ok(parts.join("/"))
    };
    let (from, to) = (clean(from)?, clean(to)?);
    if from.is_empty() {
        return Err("from is required: the place that is moved, below the import folder".into());
    }
    if from == to {
        return Err("from and to are the same place".into());
    }
    if to.starts_with(&format!("{from}/")) {
        return Err("to lies inside from".into());
    }
    let rows = store.admin_book_rows().await.map_err(|e| e.to_string())?;
    let mut plans: Vec<Rename> = Vec::new();
    let mut claimed: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for row in rows.iter().filter(|r| r.folder == from || r.folder.starts_with(&format!("{from}/"))) {
        let rest = row.folder[from.len()..].trim_start_matches('/');
        let new_folder = [to.as_str(), rest].iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join("/");
        let Some((dir, _, _)) = book_dir(store, library, row.id).await else {
            plans.push(Rename { id: row.id, author: row.author.clone(), title: row.title.clone(), from: PathBuf::new(), to: PathBuf::new(), new_folder, problem: Some("the files of the album cannot be found on the disk".into()) });
            continue;
        };
        let depth = row.folder.split('/').count();
        let Some(root) = dir.ancestors().nth(depth).map(Path::to_path_buf) else {
            plans.push(Rename { id: row.id, author: row.author.clone(), title: row.title.clone(), from: dir, to: PathBuf::new(), new_folder, problem: Some("the import folder of the album cannot be found".into()) });
            continue;
        };
        let mut plan = Rename { id: row.id, author: row.author.clone(), title: row.title.clone(), from: dir.clone(), to: new_folder.split('/').fold(root, |p, n| p.join(n)), new_folder, problem: None };
        check_place(store, &mut plan, &row.folder).await?;
        // Two albums of the branch must not end in one place.
        if plan.problem.is_none() && !claimed.insert(plan.to.clone()) {
            plan.problem = Some("another album of this move goes to the same place".into());
        }
        plans.push(plan);
    }
    plans.sort_by(|a, b| a.from.cmp(&b.from));
    Ok(plans)
}

/// Why the server may not move a folder from `from` to `to` (rights), if it may not.
fn blocked_write(from: &Path, to: &Path) -> Option<String> {
    let parent = from.parent()?;
    if !crate::access::may(parent, crate::access::WRITE | crate::access::ENTER) {
        return Some(format!("the server may not change {} (see the warning about rights)", parent.display()));
    }
    let mut existing = to.parent();
    while let Some(p) = existing {
        if p.exists() {
            break;
        }
        existing = p.parent();
    }
    match existing {
        Some(p) if crate::access::may(p, crate::access::WRITE | crate::access::ENTER) => None,
        Some(p) => Some(format!("the server may not write in {} (see the warning about rights)", p.display())),
        None => Some("the folder cannot be reached".into()),
    }
}

/// Does what `plan_rename` showed: the folder moves, then the database follows; if it cannot, the folder goes back.
pub async fn execute_rename(store: &Store, plan: &Rename) -> Result<(), String> {
    if let Some(problem) = &plan.problem {
        return Err(problem.clone());
    }
    let moves = plan.to != plan.from;
    if moves {
        let (from, to) = (plan.from.clone(), plan.to.clone());
        tokio::task::spawn_blocking(move || move_dir(&from, &to)).await.map_err(|e| e.to_string())??;
    }
    if let Err(e) = store.rename_book(plan.id, &plan.new_folder, &plan.author, &plan.title).await {
        if moves {
            let (from, to) = (plan.to.clone(), plan.from.clone());
            let _ = tokio::task::spawn_blocking(move || move_dir(&from, &to)).await;
        }
        return Err(format!("database: {e}"));
    }
    if moves {
        prune_empty_parents(store, &plan.from).await;
    }
    Ok(())
}

/// Removes the folders that a move left empty, from `from` upwards, but never an import folder and never
/// one that holds anything. After an album left `Johannes Steck/Kinder der Freiheit`, the empty
/// `Johannes Steck` goes too; otherwise every import would complain about it.
pub(crate) async fn prune_empty_parents(store: &Store, from: &Path) {
    let Ok(state) = store.import_state().await else { return };
    let roots: Vec<PathBuf> = state.roots.iter().chain(state.music_roots.iter()).filter_map(|r| std::fs::canonicalize(r).ok()).collect();
    let from = from.to_path_buf();
    let _ = tokio::task::spawn_blocking(move || {
        let mut dir = from.parent().map(Path::to_path_buf);
        while let Some(d) = dir {
            let real = std::fs::canonicalize(&d).unwrap_or_else(|_| d.clone());
            // Stop at an import folder, or when there is something in the folder.
            if roots.contains(&real) || !roots.iter().any(|r| real.starts_with(r)) || std::fs::remove_dir(&d).is_err() {
                break;
            }
            tracing::info!(folder = %d.display(), "empty folder removed after a move");
            dir = d.parent().map(Path::to_path_buf);
        }
    })
    .await;
}

/// The folders below the import folders that hold no file at all (only empty folders inside), as
/// far up as possible: what albums that were moved away have left behind. Hidden folders (the
/// quarantine) and the import folders themselves are not listed.
pub async fn empty_folders(store: &Store) -> Result<Vec<PathBuf>, String> {
    let state = store.import_state().await.map_err(|e| e.to_string())?;
    let roots: Vec<PathBuf> = state.roots.iter().chain(state.music_roots.iter()).map(PathBuf::from).collect();
    tokio::task::spawn_blocking(move || {
        fn walk(dir: &Path, roots: &[PathBuf], out: &mut Vec<PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if !entry.file_type().is_ok_and(|t| t.is_dir()) || name.starts_with('.') || name == "@eaDir" || name == "lost+found" || roots.contains(&path) {
                    continue;
                }
                if crate::scan::is_empty_tree(&path) {
                    out.push(path);
                } else {
                    walk(&path, roots, out);
                }
            }
        }
        let mut out = Vec::new();
        for root in &roots {
            walk(root, &roots, &mut out);
        }
        out.sort();
        out.dedup();
        out
    })
    .await
    .map_err(|e| e.to_string())
}

/// Puts moved folders back where they were.
async fn undo(moved: &[&Plan]) {
    for plan in moved.iter().rev() {
        let (from, to) = (plan.to.clone(), plan.from.clone());
        let _ = tokio::task::spawn_blocking(move || move_dir(&from, &to)).await;
    }
}

pub(crate) fn move_dir(from: &Path, to: &Path) -> Result<(), String> {
    let fail = |what: &str, e: std::io::Error| format!("{what}: {e}");
    std::fs::create_dir_all(to.parent().ok_or("no parent folder")?).map_err(|e| fail("creating the folder", e))?;
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            // Another disk: copy everything, check the sizes, and only then remove the original.
            copy_tree(from, to).map_err(|e| {
                let _ = std::fs::remove_dir_all(to);
                fail("copying", e)
            })?;
            if tree_size(from) != tree_size(to) {
                let _ = std::fs::remove_dir_all(to);
                return Err("the copy differs from the original; nothing was removed".into());
            }
            std::fs::remove_dir_all(from).map_err(|e| fail("removing the original after copying", e))
        }
        Err(e) => Err(fail("moving", e)),
    }
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Number of files and their bytes, to compare a copy with its original.
fn tree_size(dir: &Path) -> (u64, u64) {
    let mut total = (0, 0);
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.filter_map(|e| e.ok()) {
            match e.metadata() {
                Ok(m) if m.is_dir() => {
                    let (n, b) = tree_size(&e.path());
                    total = (total.0 + n, total.1 + b);
                }
                Ok(m) => total = (total.0 + 1, total.1 + m.len()),
                Err(_) => {}
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_genre_decides() {
        let words: Vec<String> = ["hörbuch", "lesung", "hörspiel", "audiobook", "spoken"].iter().map(|w| w.to_string()).collect();
        for g in ["Hörbuch", "Lesung", "Hörspiel", "Audiobook", "Spoken Word"] {
            assert!(is_audiobook_genre(g, &words), "{g}");
        }
        for g in ["Rock", "Christian", "", "Unbekanntes Genre"] {
            assert!(!is_audiobook_genre(g, &words), "{g}");
        }
    }

    #[test]
    fn the_cd_number_comes_off_a_title() {
        assert_eq!(strip_disc("Die Vermessung der Welt (CD 2)"), ("Die Vermessung der Welt".into(), Some(2)));
        assert_eq!(strip_disc("Die Vermessung der Welt CD05"), ("Die Vermessung der Welt".into(), Some(5)));
        assert_eq!(strip_disc("Paolo Coelho - Auf dem Jakobsweg CD1"), ("Paolo Coelho - Auf dem Jakobsweg".into(), Some(1)));
        assert_eq!(strip_disc("Foo - Disc 3"), ("Foo".into(), Some(3)));
        assert_eq!(strip_disc("Klassenhits [Disc 2]"), ("Klassenhits".into(), Some(2)));
        assert_eq!(strip_disc("Kolibri [Disk 1]"), ("Kolibri".into(), Some(1)));
        assert_eq!(strip_disc("Foo CD 4"), ("Foo".into(), Some(4)));
        assert_eq!(strip_disc("Ginger 2-1"), ("Ginger 2-1".into(), None));
        assert_eq!(strip_disc("Abbey Road (Remastered)"), ("Abbey Road (Remastered)".into(), None));
    }

    #[test]
    fn a_folder_name_cannot_leave_its_place() {
        assert_eq!(folder_name("AC/DC: Live").as_deref(), Some("AC-DC- Live"));
        assert_eq!(folder_name("..").as_deref(), None);
        assert_eq!(folder_name("  ").as_deref(), None);
    }

    #[test]
    fn a_tree_is_moved_and_a_cross_disk_copy_is_checked() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("a/Book");
        std::fs::create_dir_all(from.join("CD 1")).unwrap();
        std::fs::write(from.join("CD 1/01.mp3"), b"abc").unwrap();
        let to = dir.path().join("b/Author/Book");
        move_dir(&from, &to).unwrap();
        assert!(!from.exists() && to.join("CD 1/01.mp3").is_file());
        let copy = dir.path().join("c");
        copy_tree(&to, &copy).unwrap();
        assert_eq!(tree_size(&to), tree_size(&copy));
    }
}
