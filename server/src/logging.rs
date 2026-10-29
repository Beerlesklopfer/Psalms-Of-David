// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The log level, changeable while the server runs (project owner, 2026-10-28: "Setting für Loglevel
//! im Frontend und Kommandozeile"). The level comes from, in this order: the flag `--log`, the
//! environment `DAVID_LOG`, `/etc/david.toml`, the setting in the pages, `info`. The first three are
//! fixed at the start; the setting changes the level at once when it is saved.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use tracing::Level;
use tracing_subscriber::Layer;
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// The names a person can give, from the least to the most talkative.
pub const LEVELS: [&str; 5] = ["error", "warn", "info", "debug", "trace"];

static CURRENT: AtomicU8 = AtomicU8::new(2);
static FIXED: OnceLock<Level> = OnceLock::new();

fn code(level: Level) -> u8 {
    match level {
        Level::ERROR => 0,
        Level::WARN => 1,
        Level::INFO => 2,
        Level::DEBUG => 3,
        Level::TRACE => 4,
    }
}

/// The crates of this project: `debug` is the level for what David itself does. The libraries it uses (the database layer logs every query
/// at `debug`) are quiet below `trace`, so that `--log debug` stays readable.
const OWN_TARGETS: [&str; 2] = ["david_server", "cdrip"];

/// Whether an event of `level_code` from `target` is written when the level is `current` (both as `code`): our own crates at the level that
/// was asked for, other crates at most at `info` unless the level is `trace`.
fn allowed(target: &str, level_code: u8, current: u8) -> bool {
    let own = OWN_TARGETS.iter().any(|t| target == *t || target.strip_prefix(t).is_some_and(|rest| rest.starts_with("::")));
    level_code <= current && (own || level_code <= code(Level::INFO) || current >= code(Level::TRACE))
}

/// `error`, `warn`, `info`, `debug` or `trace` (any case).
pub fn parse(text: &str) -> Option<Level> {
    LEVELS.iter().position(|l| l.eq_ignore_ascii_case(text.trim())).map(|i| [Level::ERROR, Level::WARN, Level::INFO, Level::DEBUG, Level::TRACE][i])
}

pub fn current() -> Level {
    [Level::ERROR, Level::WARN, Level::INFO, Level::DEBUG, Level::TRACE][usize::from(CURRENT.load(Ordering::Relaxed)).min(4)]
}

/// Changes the level now.
pub fn set(level: Level) {
    CURRENT.store(code(level), Ordering::Relaxed);
}

/// The level that the start fixed (flag, environment or file), if there was one.
pub fn fixed() -> Option<Level> {
    FIXED.get().copied()
}

/// Starts logging to `writer` at `level` (the one the start fixed, else the default); the level can be
/// changed afterwards with `set`.
pub fn init<W>(requested: Option<Level>, writer: W, ansi: bool)
where
    W: for<'a> tracing_subscriber::fmt::MakeWriter<'a> + Send + Sync + 'static,
{
    if let Some(level) = requested {
        let _ = FIXED.set(level);
        set(level);
    }
    // A level above what the program was built for cannot be had; the filter reads the live value.
    let filter = filter_fn(|meta| allowed(meta.target(), code(*meta.level()), CURRENT.load(Ordering::Relaxed)));
    let layer = tracing_subscriber::fmt::layer().with_writer(writer).with_ansi(ansi).with_filter(filter);
    let _ = tracing_subscriber::registry().with(layer).try_init();
}

pub fn init_stderr(requested: Option<Level>) {
    init(requested, std::io::stderr, true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_is_for_our_own_crates_and_the_libraries_are_quiet_until_trace() {
        let (info, debug, trace) = (code(Level::INFO), code(Level::DEBUG), code(Level::TRACE));
        // Our own crates (also their modules) at the level that was asked for.
        assert!(allowed("david_server::cdrive", debug, debug));
        assert!(allowed("david_server", debug, debug));
        assert!(allowed("cdrip::rip", debug, debug));
        assert!(!allowed("david_server::cdrive", debug, info), "debug is off at info");
        // The libraries: what they say at info and above is kept, their debug is not (the queries of the database).
        assert!(!allowed("sqlx::query", debug, debug));
        assert!(allowed("sqlx::query", info, debug));
        assert!(allowed("sqlx::query", code(Level::WARN), info));
        assert!(!allowed("sqlx::query", info, code(Level::WARN)), "warn level: no info of anybody");
        // Everything at trace.
        assert!(allowed("sqlx::query", debug, trace));
        // A crate whose name only starts like ours is not ours.
        assert!(!allowed("david_serverish::x", debug, debug));
    }

    #[test]
    fn the_names_are_the_five_levels_in_any_case() {
        assert_eq!(parse("DEBUG"), Some(Level::DEBUG));
        assert_eq!(parse(" warn "), Some(Level::WARN));
        assert_eq!(parse("loud"), None);
        set(Level::TRACE);
        assert_eq!(current(), Level::TRACE);
        set(Level::INFO);
        assert_eq!(current(), Level::INFO);
    }
}
