// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Command line: `david <command> [--data-dir DIR] [--library DIR]`.

use std::path::PathBuf;

use crate::app::Config;

const USAGE: &str = "\
Usage: david <command> [options]

Commands:
  init        Create the first administrator (reads the PIN; use before the first start)
  reset-pin   Set a new PIN or password for a user who forgot it (--name NAME is required);
              the user is signed out everywhere and any lock is lifted
  run         Run in the foreground (Ctrl-C or SIGTERM stops it)
  service     Windows only: entry point used by the service manager
  install     Windows only: register the Windows service (needs an elevated prompt)
  uninstall   Windows only: stop and remove the Windows service
  export-templates
              Write the built-in page templates and stylesheet to --to DIR, as a starting
              point for a distribution's own (see DAVID_TEMPLATES_DIR)
  help        Show this text

Options:
  --data-dir DIR   Folder for the database (env DAVID_DATA_DIR)
  --listen ADDR    Where the administration interface listens (env DAVID_LISTEN, default
                   127.0.0.1:8484). Anything but this machine has no TLS yet.
  --no-announce    Do not announce the server on the network (mDNS)
  --name NAME      Name of the administrator for `init` (default: Papa), or of the user for `reset-pin`
  --to DIR         Target folder of `export-templates`

Environment: DAVID_LOG = error | warn | info | debug | trace
             DAVID_TEMPLATES_DIR = a folder with templates that replace the built-in ones
             (also looked up: /etc/david/templates, /usr/share/david/templates)";

/// Only in development builds: the switch that allows the scan at start.
#[cfg(debug_assertions)]
const DEV_USAGE: &str = "

Development build only:
  --library DIR    A folder to scan at start together with --allow-import (env DAVID_LIBRARY).
                   Real import folders are defined in the administration interface.
  --allow-import   Scan --library at start. Imports are locked by default until the
                   release criteria are defined (env DAVID_ALLOW_IMPORT=1)";

/// The help text. The import switch is listed only in development builds, because
/// a release build does not have it.
pub fn usage() -> String {
    #[cfg(debug_assertions)]
    return format!("{USAGE}{DEV_USAGE}");
    #[cfg(not(debug_assertions))]
    return USAGE.to_owned();
}

/// Who the first administrator is called unless `--name` says otherwise: the one who
/// sets up the family's server (project owner, 2026-10-27).
pub const DEFAULT_ADMIN_NAME: &str = "Papa";

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    /// Create the first administrator.
    Init { data_dir: PathBuf, name: String },
    /// Sets a new PIN or password for an existing user (a forgotten one).
    ResetPin { data_dir: PathBuf, name: String },
    Run(Config),
    Service(Config),
    Install(Config),
    Uninstall,
    /// Write the built-in templates to a folder.
    ExportTemplates { to: PathBuf },
    Help,
}

/// `args` excludes the program name. `env` looks up environment variables
/// (passed in so the parser can be tested).
pub fn parse<I>(args: I, env: impl Fn(&str) -> Option<String>) -> Result<Command, String>
where
    I: IntoIterator<Item = String>,
{
    let mut args = args.into_iter();
    let command = args.next().unwrap_or_else(|| "help".to_owned());
    let mut data_dir = env("DAVID_DATA_DIR").map(PathBuf::from);
    // Like the import switch, the development folder exists only in development builds.
    #[cfg(debug_assertions)]
    let mut library = env("DAVID_LIBRARY").map(PathBuf::from);
    #[cfg(not(debug_assertions))]
    let library: Option<PathBuf> = None;
    let mut listen = env("DAVID_LISTEN");
    let mut name: Option<String> = None;
    let mut to: Option<PathBuf> = None;
    let mut announce = true;
    // The import switch exists only in development builds.
    #[cfg(debug_assertions)]
    let mut allow_import = env("DAVID_ALLOW_IMPORT").is_some_and(|v| v == "1");
    #[cfg(not(debug_assertions))]
    let allow_import = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => data_dir = Some(args.next().ok_or("--data-dir needs a value")?.into()),
            #[cfg(debug_assertions)]
            "--library" => library = Some(args.next().ok_or("--library needs a value")?.into()),
            "--listen" => listen = Some(args.next().ok_or("--listen needs a value")?),
            "--name" => name = Some(args.next().ok_or("--name needs a value")?),
            "--to" => to = Some(args.next().ok_or("--to needs a value")?.into()),
            "--no-announce" => announce = false,
            #[cfg(debug_assertions)]
            "--allow-import" => allow_import = true,
            "-h" | "--help" => return Ok(Command::Help),
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    let config = || -> Result<Config, String> {
        Ok(Config {
            data_dir: data_dir.clone().unwrap_or_else(Config::default_data_dir),
            library: library.clone(),
            allow_import,
            announce,
            listen: listen
                .as_deref()
                .unwrap_or(crate::app::DEFAULT_LISTEN)
                .parse()
                .map_err(|_| "--listen must look like 127.0.0.1:8484".to_owned())?,
        })
    };

    match command.as_str() {
        "init" => Ok(Command::Init {
            data_dir: data_dir.clone().unwrap_or_else(Config::default_data_dir),
            name: name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| DEFAULT_ADMIN_NAME.to_owned()),
        }),
        "reset-pin" => Ok(Command::ResetPin {
            data_dir: data_dir.clone().unwrap_or_else(Config::default_data_dir),
            name: name.clone().filter(|n| !n.trim().is_empty()).ok_or("--name is required for reset-pin")?,
        }),
        "run" => Ok(Command::Run(config()?)),
        "service" => Ok(Command::Service(config()?)),
        "install" => Ok(Command::Install(config()?)),
        "uninstall" => Ok(Command::Uninstall),
        "export-templates" => Ok(Command::ExportTemplates { to: to.clone().ok_or("--to is required for export-templates")? }),
        "help" | "-h" | "--help" => Ok(Command::Help),
        other => Err(format!("unknown command: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_owned).collect()
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn the_first_administrator_is_called_papa_unless_told_otherwise() {
        let Ok(Command::ResetPin { name, data_dir }) = parse(args("reset-pin --name Papa --data-dir /d"), no_env) else { panic!() };
        assert_eq!((name.as_str(), data_dir.to_str()), ("Papa", Some("/d")));
        assert!(parse(args("reset-pin"), no_env).is_err(), "the name is required");
        let Ok(Command::Init { name, .. }) = parse(args("init --data-dir /d"), no_env) else { panic!() };
        assert_eq!(name, "Papa");
        let Ok(Command::Init { name, .. }) = parse(args("init --name Mama"), no_env) else { panic!() };
        assert_eq!(name, "Mama");
        assert!(parse(args("init --name"), no_env).is_err(), "a name flag without a name is an error");
    }

    #[test]
    fn export_templates_needs_a_target() {
        assert_eq!(parse(args("export-templates --to /tmp/t"), no_env), Ok(Command::ExportTemplates { to: "/tmp/t".into() }));
        assert!(parse(args("export-templates"), no_env).is_err());
    }

    #[test]
    fn run_needs_no_library() {
        // The import folders are defined in the administration interface.
        let Ok(Command::Run(c)) = parse(args("run"), no_env) else { panic!() };
        assert_eq!(c.library, None);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn a_development_folder_comes_from_the_flag_or_the_environment() {
        let Ok(Command::Run(c)) = parse(args("run --library /flag"), no_env) else { panic!() };
        assert_eq!(c.library, Some(PathBuf::from("/flag")));
        let env = |k: &str| (k == "DAVID_LIBRARY").then(|| "/from/env".to_owned());
        let Ok(Command::Run(c)) = parse(args("run"), env) else { panic!() };
        assert_eq!(c.library, Some(PathBuf::from("/from/env")));
        // The flag beats the environment.
        let Ok(Command::Run(c)) = parse(args("run --library /flag"), env) else { panic!() };
        assert_eq!(c.library, Some(PathBuf::from("/flag")));
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn release_builds_have_no_library_flag() {
        assert!(parse(args("run --library /x"), no_env).is_err());
        let env = |k: &str| (k == "DAVID_LIBRARY").then(|| "/from/env".to_owned());
        let Ok(Command::Run(c)) = parse(args("run"), env) else { panic!() };
        assert_eq!(c.library, None, "the environment variable is ignored");
        assert!(!usage().contains("--library"));
    }

    #[test]
    fn options_beat_the_environment() {
        let env = |k: &str| (k == "DAVID_DATA_DIR").then(|| "/from/env".to_owned());
        let Ok(Command::Run(c)) = parse(args("run"), env) else { panic!() };
        assert_eq!(c.data_dir, PathBuf::from("/from/env"));
        let Ok(Command::Run(c)) = parse(args("run --data-dir /from/flag"), env) else { panic!() };
        assert_eq!(c.data_dir, PathBuf::from("/from/flag"));
    }

    #[test]
    fn the_interface_listens_on_this_machine_only_by_default() {
        let Ok(Command::Run(c)) = parse(args("run"), no_env) else { panic!() };
        assert_eq!(c.listen.to_string(), "127.0.0.1:8484");
        assert!(c.listen.ip().is_loopback());

        let Ok(Command::Run(c)) = parse(args("run --listen 0.0.0.0:9000"), no_env) else { panic!() };
        assert_eq!(c.listen.to_string(), "0.0.0.0:9000");
        assert!(parse(args("run --listen nonsense"), no_env).is_err());
    }

    #[test]
    fn init_takes_a_name_and_no_library() {
        let Ok(Command::Init { name, .. }) = parse(args("init --name Anna --data-dir /d"), no_env) else { panic!() };
        assert_eq!(name, "Anna");
    }

    #[test]
    fn import_is_locked_by_default() {
        let Ok(Command::Run(c)) = parse(args("run"), no_env) else { panic!() };
        assert!(!c.allow_import, "imports must be locked by default");
    }

    #[cfg(debug_assertions)]
    #[test]
    fn development_builds_can_allow_the_import() {
        let Ok(Command::Run(c)) = parse(args("run --library /x --allow-import"), no_env) else { panic!() };
        assert!(c.allow_import);

        let env = |k: &str| (k == "DAVID_ALLOW_IMPORT").then(|| "1".to_owned());
        let Ok(Command::Run(c)) = parse(args("run --library /x"), env) else { panic!() };
        assert!(c.allow_import);
        assert!(usage().contains("--allow-import"));
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn release_builds_have_no_import_switch() {
        // The flag is unknown, the environment variable is ignored, the help does not mention it.
        assert!(parse(args("run --allow-import"), no_env).is_err());
        let env = |k: &str| (k == "DAVID_ALLOW_IMPORT").then(|| "1".to_owned());
        let Ok(Command::Run(c)) = parse(args("run"), env) else { panic!() };
        assert!(!c.allow_import);
        assert!(!usage().contains("allow-import"));
    }

    #[test]
    fn uninstall_and_help_need_no_library() {
        assert_eq!(parse(args("uninstall"), no_env), Ok(Command::Uninstall));
        assert_eq!(parse(args("help"), no_env), Ok(Command::Help));
        assert_eq!(parse(Vec::new(), no_env), Ok(Command::Help));
    }

    #[test]
    fn rejects_unknown_input() {
        assert!(parse(args("fly"), no_env).is_err());
        assert!(parse(args("run --wat"), no_env).is_err());
        assert!(parse(args("run --library"), no_env).is_err());
    }
}
