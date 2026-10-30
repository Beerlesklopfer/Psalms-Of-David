// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Command line: `david <command> [--data-dir DIR] [--library DIR]`.

use std::net::SocketAddr;
use std::path::PathBuf;

use crate::app::Config;

const USAGE: &str = "\
Usage: david <command> [options]

Commands:
  init        Create the first administrator (reads the PIN; use before the first start)
  reset-pin   Set a new PIN or password for a user who forgot it (--name NAME is required);
              the user is signed out everywhere and any lock is lifted
  reindex     Read the import folders again, like the switch 'Import now' of the administration pages: new and
              changed files are read, albums whose files are gone are taken out (also: david --reindex). Run
              by the timer david-reindex.timer. The import has to be unlocked.
  prune       Only the tidying of reindex: take the albums out of the library whose files are gone from the disk;
              --dry-run only lists them, --force lifts the stop for a run that
              would remove more than a third of the albums.
  renew-cert  Make a new server certificate with the current addresses and host name of this
              machine (--data-dir, --listen). The old one is kept as cert.pem.old. Paired devices
              pin the certificate, so every device has to be paired again; stop the server first
  run         Run in the foreground (Ctrl-C or SIGTERM stops it)
  service     Windows only: entry point used by the service manager
  install     Windows only: register the Windows service (needs an elevated prompt)
  uninstall   Windows only: stop and remove the Windows service
  export-templates
              Write the built-in page templates and stylesheet to --to DIR, as a starting
              point for a distribution's own (see DAVID_TEMPLATES_DIR)
  version      Show the version, the git revision and the build time
  help        Show this text

Options:
  --data-dir DIR   Folder for the database (env DAVID_DATA_DIR)
  --listen ADDR    Where the administration interface listens (env DAVID_LISTEN, default
                   127.0.0.1:8484). Several addresses, separated by commas, are served at the same
                   time: 0.0.0.0:8484,[::]:8484 (IPv4 and IPv6). Anything but this machine speaks TLS.
  --no-announce    Do not announce the server on the network (mDNS)
  --mcp            Serve /mcp for a remote LLM that looks at the library and corrects ID tags
                   (env DAVID_MCP=1). Access needs a token made in the settings (LLM access).
  --log LEVEL      How much is logged: error, warn, info (default), debug or trace (env DAVID_LOG). debug is
                   for David's own code; the libraries it uses (the database queries) join in at trace.
                   Also a setting in the administration pages, where it changes at once.
  --tls-cert FILE  A certificate (PEM, with its chain) from somebody else, for example Let's Encrypt, instead of
  --tls-key FILE   the self-signed one, and its private key (env DAVID_TLS_CERT, DAVID_TLS_KEY). The service
  --tls-host NAME  user has to be able to read both. NAME is what devices connect to (DAVID_TLS_HOST); they then
                   check the name, not a fingerprint. A renewed certificate is loaded without a restart.
  --name NAME      Name of the administrator for `init` (default: Papa), or of the user for `reset-pin`
  --to DIR         Target folder of `export-templates`
  --dry-run        `prune`: only say what would be removed
  --force          `prune`: also when many albums are gone (a disk that is not mounted looks like that)

Environment: DAVID_LOG = error | warn | info | debug | trace (like --log)
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
    /// The import, as the switch "Import now" of the administration pages does it: new and changed files are read, albums whose files are gone are
    /// taken out. What `david-reindex.timer` runs. The import has to be unlocked, as it has to be for the switch.
    Reindex { data_dir: PathBuf },
    /// Removes the albums whose files are gone from the disk, only that (`--dry-run` lists them first). `dry_run` only lists them; `force` lifts the stop for many.
    Prune { data_dir: PathBuf, dry_run: bool, force: bool },
    /// Replaces the server certificate (devices have to be paired again).
    RenewCert { data_dir: PathBuf, listen: SocketAddr },
    Run(Config),
    Service(Config),
    Install(Config),
    Uninstall,
    /// Print the version with the git revision and the build time.
    Version,
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
    let mut dry_run = false;
    let mut force = false;
    let mut mcp = env("DAVID_MCP").is_some_and(|v| v == "1");
    let mut log = env("DAVID_LOG");
    let mut tls_cert = env("DAVID_TLS_CERT").filter(|v| !v.is_empty()).map(PathBuf::from);
    let mut tls_key = env("DAVID_TLS_KEY").filter(|v| !v.is_empty()).map(PathBuf::from);
    let mut tls_host = env("DAVID_TLS_HOST").filter(|v| !v.is_empty());
    let tls_reload_s = env("DAVID_TLS_RELOAD_S").and_then(|v| v.parse().ok());
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
            "--dry-run" => dry_run = true,
            "--force" => force = true,
            "--mcp" => mcp = true,
            "--log" => log = Some(args.next().ok_or("--log needs a value")?),
            "--tls-cert" => tls_cert = Some(args.next().ok_or("--tls-cert needs a value")?.into()),
            "--tls-key" => tls_key = Some(args.next().ok_or("--tls-key needs a value")?.into()),
            "--tls-host" => tls_host = Some(args.next().ok_or("--tls-host needs a value")?),
            #[cfg(debug_assertions)]
            "--allow-import" => allow_import = true,
            "-h" | "--help" => return Ok(Command::Help),
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    // One address, or several separated by commas: `0.0.0.0:8484,[::]:8484`.
    let addresses: Vec<SocketAddr> = listen
        .as_deref()
        .map(|l| l.split(',').map(str::trim).filter(|a| !a.is_empty()).map(|a| a.parse().map_err(|_| format!("--listen must look like 127.0.0.1:8484 (several separated by commas), not {a}"))).collect::<Result<_, _>>())
        .transpose()?
        .unwrap_or_default();
    let config = || -> Result<Config, String> {
        Ok(Config {
            data_dir: data_dir.clone().unwrap_or_else(Config::default_data_dir),
            library: library.clone(),
            allow_import,
            announce,
            mcp,
            tls: {
                if tls_cert.is_some() != tls_key.is_some() {
                    return Err("--tls-cert and --tls-key belong together".to_owned());
                }
                crate::tls::External { cert: tls_cert.clone(), key: tls_key.clone(), host: tls_host.clone(), reload_s: tls_reload_s }
            },
            log: match log.as_deref().filter(|l| !l.trim().is_empty()) {
                None => None,
                Some(l) => Some(crate::logging::parse(l).ok_or("--log must be error, warn, info, debug or trace")?),
            },
            listen: addresses.first().copied().unwrap_or_else(|| crate::app::DEFAULT_LISTEN.parse().expect("the default address is valid")),
            also_listen: addresses.iter().skip(1).copied().collect(),
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
        "reindex" | "--reindex" => Ok(Command::Reindex { data_dir: data_dir.clone().unwrap_or_else(Config::default_data_dir) }),
        "prune" => Ok(Command::Prune { data_dir: data_dir.clone().unwrap_or_else(Config::default_data_dir), dry_run, force }),
        "renew-cert" => Ok(Command::RenewCert {
            data_dir: data_dir.clone().unwrap_or_else(Config::default_data_dir),
            // Without an address the certificate names every interface of the machine.
            listen: listen.as_deref().unwrap_or("0.0.0.0:8484").parse().map_err(|_| "--listen must look like 0.0.0.0:8484".to_owned())?,
        }),
        "run" => Ok(Command::Run(config()?)),
        "service" => Ok(Command::Service(config()?)),
        "install" => Ok(Command::Install(config()?)),
        "uninstall" => Ok(Command::Uninstall),
        "export-templates" => Ok(Command::ExportTemplates { to: to.clone().ok_or("--to is required for export-templates")? }),
        "version" | "--version" | "-V" => Ok(Command::Version),
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
    fn the_log_level_comes_from_the_flag_the_environment_or_nowhere() {
        let Ok(Command::Run(c)) = parse(args("run"), no_env) else { panic!() };
        assert_eq!(c.log, None, "without one the setting of the pages decides");
        let env = |k: &str| (k == "DAVID_LOG").then(|| "warn".to_owned());
        let Ok(Command::Run(c)) = parse(args("run"), env) else { panic!() };
        assert_eq!(c.log, Some(tracing::Level::WARN));
        let Ok(Command::Run(c)) = parse(args("run --log DEBUG"), env) else { panic!() };
        assert_eq!(c.log, Some(tracing::Level::DEBUG), "the flag beats the environment");
        assert!(parse(args("run --log loud"), no_env).is_err());
        assert!(parse(args("run --log"), no_env).is_err());
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
    fn mcp_is_off_unless_the_flag_or_the_environment_asks_for_it() {
        let run = |a: &str, mcp_env: bool| match parse(args(a), |k| (mcp_env && k == "DAVID_MCP").then(|| "1".to_owned())) {
            Ok(Command::Run(c)) => c.mcp,
            other => panic!("{other:?}"),
        };
        assert!(!run("run", false));
        assert!(run("run --mcp", false));
        assert!(run("run", true));
    }

    #[test]
    fn renew_cert_names_every_interface_unless_told_an_address() {
        assert_eq!(
            parse(args("renew-cert --data-dir /d"), no_env),
            Ok(Command::RenewCert { data_dir: "/d".into(), listen: "0.0.0.0:8484".parse().unwrap() })
        );
        assert_eq!(
            parse(args("renew-cert --listen 10.0.0.5:8484"), no_env),
            Ok(Command::RenewCert { data_dir: Config::default_data_dir(), listen: "10.0.0.5:8484".parse().unwrap() })
        );
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
