// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! POSIX ACLs through libacl (linked, not the `setfacl` program): gives the service user read and
//! write on the library folders, the equivalent of `setfacl -R -m u:UID:rwX` and, for folders, the
//! same as a default entry so that new files get it too. Linux only; the entry for the user is
//! added or raised, nothing else is touched.

use std::ffi::{CString, c_int, c_uint, c_void};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

type Acl = *mut c_void;
type Entry = *mut c_void;
type Permset = *mut c_void;

const ACL_TYPE_ACCESS: c_uint = 0x8000;
const ACL_TYPE_DEFAULT: c_uint = 0x4000;
const ACL_USER: c_int = 0x02;
const ACL_FIRST_ENTRY: c_int = 0;
const ACL_NEXT_ENTRY: c_int = 1;
const ACL_READ: c_uint = 0x04;
const ACL_WRITE: c_uint = 0x02;
const ACL_EXECUTE: c_uint = 0x01;

#[link(name = "acl")]
unsafe extern "C" {
    fn acl_get_file(path: *const std::ffi::c_char, kind: c_uint) -> Acl;
    fn acl_set_file(path: *const std::ffi::c_char, kind: c_uint, acl: Acl) -> c_int;
    fn acl_from_mode(mode: u32) -> Acl;
    fn acl_get_entry(acl: Acl, which: c_int, entry: *mut Entry) -> c_int;
    fn acl_create_entry(acl: *mut Acl, entry: *mut Entry) -> c_int;
    fn acl_get_tag_type(entry: Entry, tag: *mut c_int) -> c_int;
    fn acl_set_tag_type(entry: Entry, tag: c_int) -> c_int;
    fn acl_get_qualifier(entry: Entry) -> *mut c_void;
    fn acl_set_qualifier(entry: Entry, qualifier: *const c_void) -> c_int;
    fn acl_get_permset(entry: Entry, set: *mut Permset) -> c_int;
    fn acl_add_perm(set: Permset, perm: c_uint) -> c_int;
    fn acl_get_perm(set: Permset, perm: c_uint) -> c_int;
    fn acl_calc_mask(acl: *mut Acl) -> c_int;
    fn acl_free(object: *mut c_void) -> c_int;
}

/// An ACL that is freed when it goes out of scope.
struct Owned(Acl);

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: the pointer came from libacl and is freed once.
        unsafe { acl_free(self.0) };
    }
}

fn cvt(r: c_int) -> io::Result<()> {
    if r == -1 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

fn c_path(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path with a NUL byte"))
}

/// Makes sure that `acl` has an entry for `uid` with the wanted permissions. `Ok(false)` if it had.
fn raise(acl: &mut Owned, uid: u32, perms: c_uint) -> io::Result<bool> {
    let mut entry: Entry = std::ptr::null_mut();
    let mut found: Option<Entry> = None;
    let mut which = ACL_FIRST_ENTRY;
    // SAFETY: every pointer passed is valid for the call; libacl owns the entries inside `acl`.
    unsafe {
        while acl_get_entry(acl.0, which, &mut entry) == 1 {
            which = ACL_NEXT_ENTRY;
            let mut tag = 0;
            cvt(acl_get_tag_type(entry, &mut tag))?;
            if tag != ACL_USER {
                continue;
            }
            let q = acl_get_qualifier(entry) as *mut u32;
            if q.is_null() {
                continue;
            }
            let same = *q == uid;
            acl_free(q.cast());
            if same {
                found = Some(entry);
                break;
            }
        }
        let entry = match found {
            Some(e) => e,
            None => {
                let mut e: Entry = std::ptr::null_mut();
                cvt(acl_create_entry(&mut acl.0, &mut e))?;
                cvt(acl_set_tag_type(e, ACL_USER))?;
                cvt(acl_set_qualifier(e, (&raw const uid).cast()))?;
                e
            }
        };
        let mut set: Permset = std::ptr::null_mut();
        cvt(acl_get_permset(entry, &mut set))?;
        let mut changed = found.is_none();
        for p in [ACL_READ, ACL_WRITE, ACL_EXECUTE] {
            if perms & p != 0 && acl_get_perm(set, p) != 1 {
                cvt(acl_add_perm(set, p))?;
                changed = true;
            }
        }
        if changed {
            cvt(acl_calc_mask(&mut acl.0))?;
        }
        Ok(changed)
    }
}

fn load(path: &Path, kind: c_uint) -> io::Result<Owned> {
    let c = c_path(path)?;
    // SAFETY: `c` is a valid C string for the call.
    let acl = unsafe { acl_get_file(c.as_ptr(), kind) };
    if acl.is_null() { Err(io::Error::last_os_error()) } else { Ok(Owned(acl)) }
}

fn store(path: &Path, kind: c_uint, acl: &Owned) -> io::Result<()> {
    let c = c_path(path)?;
    // SAFETY: valid C string and a valid ACL.
    cvt(unsafe { acl_set_file(c.as_ptr(), kind, acl.0) })
}

/// Gives `uid` read and write on one file or folder (execute on folders and on files somebody may
/// execute: the `X` of `setfacl`), and on a folder the same as a default entry. `Ok(true)` if
/// something changed.
pub fn grant(path: &Path, uid: u32) -> io::Result<bool> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Ok(false);
    }
    let mode = meta.permissions().mode();
    let dir = meta.is_dir();
    let perms = ACL_READ | ACL_WRITE | if dir || mode & 0o111 != 0 { ACL_EXECUTE } else { 0 };
    let mut access = load(path, ACL_TYPE_ACCESS)?;
    let mut changed = raise(&mut access, uid, perms)?;
    if changed {
        store(path, ACL_TYPE_ACCESS, &access)?;
    }
    if dir {
        let mut default = load(path, ACL_TYPE_DEFAULT)?;
        let mut probe: Entry = std::ptr::null_mut();
        // SAFETY: valid ACL and entry pointer.
        let empty = unsafe { acl_get_entry(default.0, ACL_FIRST_ENTRY, &mut probe) } != 1;
        if empty {
            // A default ACL needs the three base entries; start from the mode of the folder.
            // SAFETY: plain call; the result is freed by `Owned`.
            let base = unsafe { acl_from_mode(mode & 0o7777) };
            if base.is_null() {
                return Err(io::Error::last_os_error());
            }
            default = Owned(base);
        }
        if raise(&mut default, uid, ACL_READ | ACL_WRITE | ACL_EXECUTE)? {
            store(path, ACL_TYPE_DEFAULT, &default)?;
            changed = true;
        }
    }
    Ok(changed)
}

/// What `grant_tree` did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Granted {
    pub looked_at: usize,
    pub changed: usize,
    pub failed: usize,
    /// The first error, with its path.
    pub first_error: Option<String>,
}

/// `grant` for a folder and everything below it (symbolic links are not followed).
pub fn grant_tree(root: &Path, uid: u32) -> Granted {
    let mut done = Granted::default();
    walk(root, uid, &mut done);
    done
}

fn walk(path: &Path, uid: u32, done: &mut Granted) {
    done.looked_at += 1;
    match grant(path, uid) {
        Ok(true) => done.changed += 1,
        Ok(false) => {}
        Err(e) => {
            done.failed += 1;
            done.first_error.get_or_insert_with(|| format!("{}: {e}", path.display()));
        }
    }
    let is_dir = std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir());
    if !is_dir {
        return;
    }
    match std::fs::read_dir(path) {
        Ok(entries) => {
            for entry in entries.filter_map(|e| e.ok()) {
                walk(&entry.path(), uid, done);
            }
        }
        Err(e) => {
            done.failed += 1;
            done.first_error.get_or_insert_with(|| format!("{}: {e}", path.display()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads back whether `uid` has an entry with the wanted permissions.
    fn has(path: &Path, kind: c_uint, uid: u32, perms: c_uint) -> bool {
        let Ok(mut acl) = load(path, kind) else { return false };
        // A second `raise` changes nothing exactly when the entry is already there.
        raise(&mut acl, uid, perms).is_ok_and(|changed| !changed)
    }

    #[test]
    fn a_user_gets_read_and_write_on_a_tree_and_new_files_inherit_it() {
        let dir = tempfile::tempdir().unwrap();
        let album = dir.path().join("Artist/Album");
        std::fs::create_dir_all(&album).unwrap();
        let file = album.join("01.mp3");
        std::fs::write(&file, b"x").unwrap();
        // A folder that does not hold ACLs (some file systems) cannot show anything.
        if grant(&file, 4242).is_err() {
            return;
        }
        let done = grant_tree(dir.path(), 4242);
        assert_eq!(done.failed, 0, "{done:?}");
        assert!(has(&file, ACL_TYPE_ACCESS, 4242, ACL_READ | ACL_WRITE));
        assert!(!has(&file, ACL_TYPE_ACCESS, 4242, ACL_EXECUTE), "a plain file gets no execute");
        assert!(has(&album, ACL_TYPE_ACCESS, 4242, ACL_READ | ACL_WRITE | ACL_EXECUTE));
        assert!(has(&album, ACL_TYPE_DEFAULT, 4242, ACL_READ | ACL_WRITE | ACL_EXECUTE));
        // Looking again changes nothing.
        assert_eq!(grant_tree(dir.path(), 4242).changed, 0);
        // A file made later inherits the default entry.
        let later = album.join("02.mp3");
        std::fs::write(&later, b"y").unwrap();
        assert!(has(&later, ACL_TYPE_ACCESS, 4242, ACL_READ | ACL_WRITE));
    }
}
