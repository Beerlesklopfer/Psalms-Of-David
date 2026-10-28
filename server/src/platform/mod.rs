// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Everything that differs between operating systems. The rest of the crate is
//! platform independent. Each backend provides the same four entry points:
//! `run_foreground`, `run_service`, `install` and `uninstall`, returning the
//! process exit code.

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(not(any(unix, windows)))]
compile_error!("the David server supports Unix and Windows only");
