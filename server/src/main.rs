// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
use std::process::ExitCode;

use david_server::cli::{self, Command, usage};
use david_server::platform;

fn main() -> ExitCode {
    // The file stands in for the environment: a flag or a variable beats it.
    let file_name = david_server::system::file_path(|k| std::env::var(k).ok());
    let file = match david_server::system::FileConfig::load(&file_name) {
        Ok(f) => f,
        Err(message) => {
            eprintln!("david: {message}");
            return ExitCode::from(2);
        }
    };
    let command = match cli::parse(std::env::args().skip(1), |k| std::env::var(k).ok().or_else(|| file.as_env(k))) {
        Ok(c) => c,
        Err(message) => {
            eprintln!("david: {message}\n\n{}", usage());
            return ExitCode::from(2);
        }
    };
    let code = match command {
        Command::Version => {
            println!("david {}", david_server::buildinfo::describe());
            0
        }
        Command::Help => {
            println!("{}", usage());
            0
        }
        Command::Init { data_dir, name } => init(&data_dir, &name),
        Command::ResetPin { data_dir, name } => reset_pin(&data_dir, &name),
        Command::Reindex { data_dir } => reindex(&data_dir),
        Command::Prune { data_dir, dry_run, force } => prune(&data_dir, dry_run, force),
        Command::RenewCert { data_dir, listen } => match david_server::tls::renew_certificate(&data_dir, listen) {
            Ok(m) => {
                println!("New certificate in {}/tls, fingerprint {}", data_dir.display(), m.fingerprint);
                println!("Names: {}", david_server::tls::certificate_names(listen).join(", "));
                println!("The old one is kept as cert.pem.old. Restart the server and pair every device again.");
                0
            }
            Err(e) => {
                eprintln!("david: {e}");
                1
            }
        },
        Command::Run(config) => {
            // Started as root, the server first sets the rights and then drops to the service user.
            #[cfg(unix)]
            if let Err(message) = david_server::system::become_service_user(&file, &config) {
                eprintln!("david: {message}");
                return ExitCode::from(1);
            }
            platform::run_foreground(config)
        }
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

/// The import, as the switch of the administration pages does it (the timer `david-reindex.timer` runs this).
fn reindex(data_dir: &std::path::Path) -> u8 {
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("david: {e}");
            return 1;
        }
    };
    let db = data_dir.join("david.db");
    if !db.is_file() {
        eprintln!("david: there is no database in {}", data_dir.display());
        return 1;
    }
    let result = runtime.block_on(async {
        let store = david_server::Store::open(&db).await?;
        store.run_import().await
    });
    match result {
        Ok(r) => {
            for p in &r.problems {
                eprintln!("david: {p}");
            }
            println!(
                "{} album(s) read: {} track(s) added, {} updated, {} removed; {} album(s) removed because their files are gone.",
                r.books, r.tracks_added, r.tracks_updated, r.tracks_removed, r.books_removed
            );
            0
        }
        Err(david_server::Error::ImportLocked) => {
            eprintln!("david: the import is locked: unlock it in the administration pages (Import)");
            0
        }
        Err(e) => {
            eprintln!("david: {e}");
            1
        }
    }
}

/// Takes the albums out of the library whose files are gone (the timer `david-prune.timer` runs this).
fn prune(data_dir: &std::path::Path, dry_run: bool, force: bool) -> u8 {
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("david: {e}");
            return 1;
        }
    };
    let db = data_dir.join("david.db");
    if !db.is_file() {
        eprintln!("david: there is no database in {}", data_dir.display());
        return 1;
    }
    let result = runtime.block_on(async {
        let store = david_server::Store::open(&db).await?;
        store.prune_missing(dry_run, force).await
    });
    match result {
        Ok(report) => {
            let verb = if dry_run { "would be removed" } else { "removed" };
            for g in &report.removed {
                println!("{verb}: {} / {} ({})", g.author, g.title, g.folder);
            }
            for g in &report.held_back {
                println!("left: {} / {} ({})", g.author, g.title, g.folder);
            }
            for p in &report.problems {
                eprintln!("david: {p}");
            }
            println!("{} album(s) {verb}, {} left.", report.removed.len(), report.held_back.len());
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
