// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Protection against filling the disk. Everything that writes a lot (uploads, unpacking
//! archives, writing a cover into every file of a book) asks first whether the bytes still
//! fit and leaves a reserve free, the setting `min_free_gb` (default 2, 0 = no protection).

use std::path::Path;

use crate::store::Store;

const GIB: u64 = 1024 * 1024 * 1024;
const DEFAULT_RESERVE_GB: u64 = 2;

/// Bytes available to this program on the disk that holds `path`. `None` if the system does
/// not tell (then nothing is refused: the write itself still fails with "disk full").
pub fn free_bytes(path: &Path) -> Option<u64> {
    // The folder may not exist yet: ask the nearest one that does.
    let mut dir = Some(path);
    while let Some(p) = dir {
        if p.exists() {
            return fs4::available_space(p).ok();
        }
        dir = p.parent();
    }
    None
}

/// The bytes to keep free, from the settings.
pub async fn reserve(store: &Store) -> u64 {
    let gb = store.setting("min_free_gb").await.ok().flatten().and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(DEFAULT_RESERVE_GB);
    gb.saturating_mul(GIB)
}

/// Would `need` more bytes at `path` still leave `reserve` free?
pub fn fits(path: &Path, need: u64, reserve: u64) -> bool {
    if reserve == 0 {
        return true;
    }
    free_bytes(path).is_none_or(|free| free >= need.saturating_add(reserve))
}

/// Do the two paths lie on the same disk? (Moving a file between them then costs no space.)
pub fn same_disk(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let nearest = |p: &Path| p.ancestors().find_map(|a| std::fs::metadata(a).ok()).map(|m| m.dev());
        matches!((nearest(a), nearest(b)), (Some(x), Some(y)) if x == y)
    }
    #[cfg(not(unix))]
    {
        let _ = (a, b);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reserve_that_cannot_be_met_refuses_and_zero_never_does() {
        let tmp = std::env::temp_dir();
        assert!(free_bytes(&tmp).is_some_and(|f| f > 0));
        assert!(fits(&tmp, 1, 0));
        assert!(!fits(&tmp, u64::MAX / 2, 1), "more than any disk holds");
        assert!(!fits(&tmp, 0, u64::MAX / 2), "a reserve larger than any disk");
        // A folder that does not exist yet is judged by the disk it would be on.
        assert!(fits(&tmp.join("not/yet/there"), 1, 1));
        assert!(same_disk(&tmp, &tmp.join("not/yet")));
    }
}
