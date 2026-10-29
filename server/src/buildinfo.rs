// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Which program is running: version, git revision and build time (set by `build.rs`), and the hash
//! of the binary itself, to compare with the one of a package.

use std::sync::OnceLock;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The short git revision, with `-dirty` when the tree had changes that were not committed.
pub const GIT: &str = env!("DAVID_GIT");
/// When the program was built (UTC).
pub const BUILT: &str = env!("DAVID_BUILT");

static STARTED: OnceLock<i64> = OnceLock::new();

/// Remembers the start; call once when the server starts.
pub fn mark_started() {
    let _ = STARTED.set(crate::store::now_ms());
}

/// `0.1.0 (a1b2c3d4e5, built 2026-10-28T12:00:00Z)`
pub fn describe() -> String {
    format!("{VERSION} ({GIT}, built {BUILT})")
}

/// The SHA-256 of the running binary, as `sha256sum` prints it.
pub fn binary_sha256() -> Option<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(std::env::current_exe().ok()?).ok()?;
    Some(Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect())
}

pub fn started_at() -> Option<i64> {
    STARTED.get().copied()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_build_says_what_it_is() {
        assert!(super::describe().starts_with(super::VERSION));
        assert!(super::BUILT.len() == 20 && super::BUILT.ends_with('Z') && super::BUILT.contains('T'), "{}", super::BUILT);
        assert!(!super::GIT.is_empty());
    }
}
