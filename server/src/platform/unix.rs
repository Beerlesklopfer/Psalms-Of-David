// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Linux and other Unix systems: runs in the foreground and is supervised by
//! systemd (`packaging/david.service`, `Type=notify`).

use sd_notify::NotifyState;

use crate::app::{self, Config};

/// Runs until Ctrl-C or SIGTERM. Tells systemd when startup is complete and when
/// shutdown begins; outside systemd these notifications are silently ignored.
pub fn run_foreground(config: Config) -> u8 {
    app::init_stderr_logging();
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("david: cannot start the async runtime: {e}");
            return 1;
        }
    };
    let result = runtime.block_on(async {
        let shutdown = async {
            app::shutdown_signal().await;
            let _ = sd_notify::notify(&[NotifyState::Stopping]);
        };
        app::run(config, shutdown, |_addr| {
            let _ = sd_notify::notify(&[NotifyState::Ready]);
        })
        .await
    });
    match result {
        Ok(()) => 0,
        Err(e) => {
            tracing::error!(error = %e, "server stopped with an error");
            1
        }
    }
}

const WINDOWS_ONLY: &str = "this command is only available on Windows; on Linux use the systemd unit in server/packaging/";

pub fn run_service(_config: Config) -> u8 {
    eprintln!("david: {WINDOWS_ONLY}");
    2
}

pub fn install(_config: Config) -> u8 {
    eprintln!("david: {WINDOWS_ONLY}");
    2
}

pub fn uninstall() -> u8 {
    eprintln!("david: {WINDOWS_ONLY}");
    2
}
