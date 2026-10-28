// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
use std::process::ExitCode;

use david_server::cli::{self, Command, usage};
use david_server::platform;

fn main() -> ExitCode {
    let command = match cli::parse(std::env::args().skip(1), |k| std::env::var(k).ok()) {
        Ok(c) => c,
        Err(message) => {
            eprintln!("david: {message}\n\n{}", usage());
            return ExitCode::from(2);
        }
    };
    let code = match command {
        Command::Help => {
            println!("{}", usage());
            0
        }
        Command::Init { data_dir, name } => init(&data_dir, &name),
        Command::ResetPin { data_dir, name } => reset_pin(&data_dir, &name),
        Command::Run(config) => platform::run_foreground(config),
        Command::Service(config) => platform::run_service(config),
        Command::Install(config) => platform::install(config),
        Command::Uninstall => platform::uninstall(),
        Command::ExportTemplates { to } => match david_server::admin::pages::export_templates(&to) {
            Ok(n) => {
                println!("{n} files written to {}", to.display());
                0
            }
            Err(e) => {
                eprintln!("david: cannot write to {}: {e}", to.display());
                1
            }
        },
    };
    ExitCode::from(code)
}

/// Asks for a PIN or password (twice, hidden, at a terminal; one line from standard input
/// otherwise, so scripts can use it). `None` after saying why if it is refused.
fn read_secret() -> Option<String> {
    use std::io::{BufRead, IsTerminal};

    let pin = if std::io::stdin().is_terminal() {
        let first = rpassword::prompt_password("PIN or password: ").unwrap_or_default();
        let second = rpassword::prompt_password("Again: ").unwrap_or_default();
        if first != second {
            eprintln!("david: the two PINs differ");
            return None;
        }
        first
    } else {
        let mut line = String::new();
        let _ = std::io::stdin().lock().read_line(&mut line);
        line.trim().to_owned()
    };
    if !david_server::auth::valid_secret(&pin) {
        eprintln!("david: a PIN has 4 to 12 digits; a password has 8 to 64 characters with letters and digits");
        return None;
    }
    Some(pin)
}

/// Sets a new PIN or password for an existing user.
fn reset_pin(data_dir: &std::path::Path, name: &str) -> u8 {
    let Some(pin) = read_secret() else { return 1 };
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("david: {e}");
            return 1;
        }
    };
    match runtime.block_on(david_server::app::reset_pin(data_dir, name, &pin)) {
        Ok(()) => {
            println!("New PIN set for {name}; they are signed out everywhere and any lock is lifted.");
            0
        }
        Err(e) => {
            eprintln!("david: {e}");
            1
        }
    }
}

/// Asks for a PIN and creates the first administrator.
fn init(data_dir: &std::path::Path, name: &str) -> u8 {
    let Some(pin) = read_secret() else { return 1 };
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("david: {e}");
            return 1;
        }
    };
    match runtime.block_on(david_server::app::init_admin(data_dir, name, &pin)) {
        Ok(()) => {
            println!("Administrator {name} created in {}.", data_dir.display());
            println!("Start the server with `david run` and open http://127.0.0.1:8484/admin/");
            0
        }
        Err(e) => {
            eprintln!("david: {e}");
            1
        }
    }
}
