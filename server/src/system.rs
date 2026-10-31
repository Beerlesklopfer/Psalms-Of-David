// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! `/etc/david.toml` and the start as root. The file holds the system IDs of the service user and
//! the settings that belong to the machine (listen address, data folder, MCP). A value comes from, in
//! this order: the command line, the environment, the file, the built-in default; what the
//! administrator sets in the pages (import folders, rules) lives in the database.
//!
//! The service is started as root. Before it serves anything it gives the service user the rights
//! on the library folders (POSIX ACLs through libacl), hands the data folder to it and then
//! drops to that user for good. A process that is not root runs as it is.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Where the file is looked for unless `DAVID_CONFIG` names another one.
pub const DEFAULT_FILE: &str = "/etc/david.toml";

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct FileConfig {
    /// The account the server runs as after the start (default `david`).
    pub user: String,
    /// The numeric IDs of that account; the package creates it with them. Without them the
    /// system's own are used.
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    /// Give the service user the rights on the library folders at every start (needs root).
    pub acl: bool,
    pub data_dir: Option<PathBuf>,
    pub listen: Option<Listen>,
    pub mcp: Option<bool>,
    /// `error`, `warn`, `info`, `debug` or `trace`.
    pub log: Option<String>,
    /// A certificate and key from somebody else (Let's Encrypt, ...) instead of the self-signed one.
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    /// The name devices connect to when the certificate comes from a certificate authority.
    pub tls_host: Option<String>,
    /// How often a renewed certificate is looked for, in seconds.
    pub tls_reload_s: Option<u64>,
    /// Groups the service is a member of in addition to those of its account, for example the one
    /// that may read the certificates. Looked up by name when the service starts as root.
    pub groups: Vec<String>,
    /// Hidden features, off unless the table `[features]` switches them on (see `features.rs`).
    pub features: crate::features::Features,
}

impl Default for FileConfig {
    fn default() -> Self {
        FileConfig { user: "david".into(), uid: None, gid: None, acl: true, data_dir: None, listen: None, mcp: None, log: None, tls_cert: None, tls_key: None, tls_host: None, tls_reload_s: None, groups: Vec::new(), features: Default::default() }
    }
}

impl FileConfig {
    /// Reads the file; a missing file is the defaults, a broken one an error that says where.
    pub fn load(path: &Path) -> Result<FileConfig, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(FileConfig::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// The file's value for an environment variable of the command line parser, so that the
    /// environment (and the flags above it) win over the file.
    pub fn as_env(&self, key: &str) -> Option<String> {
        match key {
            "DAVID_DATA_DIR" => self.data_dir.as_ref().map(|p| p.display().to_string()),
            "DAVID_LISTEN" => self.listen.as_ref().map(Listen::joined).filter(|l| !l.is_empty()),
            "DAVID_LOG" => self.log.clone(),
            "DAVID_TLS_CERT" => self.tls_cert.as_ref().map(|p| p.display().to_string()),
            "DAVID_TLS_KEY" => self.tls_key.as_ref().map(|p| p.display().to_string()),
            "DAVID_TLS_HOST" => self.tls_host.clone(),
            "DAVID_TLS_RELOAD_S" => self.tls_reload_s.map(|s| s.to_string()),
            "DAVID_MCP" => self.mcp.map(|m| if m { "1" } else { "0" }.to_owned()),
            // Only a switch that is on is passed on: an environment variable set to 0 beats it either way.
            k if k == crate::features::Features::DOWNLOAD_ENV => self.features.download.then(|| "1".to_owned()),
            _ => None,
        }
    }
}

impl FileConfig {
    /// The defaults, with `acl` as the packaged file sets it (the test of the packaged file compares with it).
    #[cfg(test)]
    fn with_acl_from(mut self, text: &str) -> FileConfig {
        self.acl = !text.lines().any(|l| l.trim() == "acl = false");
        self
    }
}

/// `listen = "0.0.0.0:8484"` or `listen = ["0.0.0.0:8484", "[::]:8484"]`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum Listen {
    One(String),
    Many(Vec<String>),
}

impl Listen {
    /// As one value for the command line parser: the addresses separated by commas.
    fn joined(&self) -> String {
        match self {
            Listen::One(a) => a.clone(),
            Listen::Many(list) => list.join(","),
        }
    }
}

/// The file to read: `DAVID_CONFIG`, else `/etc/david.toml`.
pub fn file_path(env: impl Fn(&str) -> Option<String>) -> PathBuf {
    env("DAVID_CONFIG").map_or_else(|| PathBuf::from(DEFAULT_FILE), PathBuf::from)
}

#[cfg(unix)]
mod unix {
    use std::ffi::CString;
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;

    use super::FileConfig;
    use crate::app::Config;

    /// Lets the service user, who is made out of root a moment later, raise the priority of a thread of its own: the encoder of a CD rip
    /// asks for the highest (`cd_settings::NICE_RANGE`). Without this a plain user may not go below nice 0. The limit stays when the user
    /// changes; a service that systemd starts has it from `LimitNICE=` already, and then nothing is changed. A failure only costs the
    /// priority (the encoder then runs at the nearest value allowed), so it is said and not fatal.
    #[cfg(target_os = "linux")]
    fn allow_priority() {
        let mut now = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        // SAFETY: plain system call with a valid pointer.
        if unsafe { libc::getrlimit(libc::RLIMIT_NICE, &mut now) } != 0 {
            return;
        }
        // `rlim_t` is a `u64` here and a `u32` on other targets: the cast is needed there.
        #[allow(clippy::unnecessary_cast)]
        let Some((soft, hard)) = super::nice_limits_for(crate::cd_settings::NICE_RANGE.0, now.rlim_cur as u64, now.rlim_max as u64) else {
            return;
        };
        let wanted = libc::rlimit { rlim_cur: soft as libc::rlim_t, rlim_max: hard as libc::rlim_t };
        // SAFETY: plain system call with a valid pointer.
        if unsafe { libc::setrlimit(libc::RLIMIT_NICE, &wanted) } == 0 {
            eprintln!("david: the service may raise the priority of the encoder of a CD rip (nice limit {soft})");
        } else {
            eprintln!("david: could not allow the encoder a higher priority: {}", std::io::Error::last_os_error());
        }
    }

    /// Other systems have no `RLIMIT_NICE`: the encoder then runs at the priority it has.
    #[cfg(not(target_os = "linux"))]
    fn allow_priority() {}

    /// The user ID and group ID of the service user: from the file, else from the system's user database.
    fn ids(file: &FileConfig) -> Result<(u32, u32), String> {
        if let (Some(uid), Some(gid)) = (file.uid, file.gid) {
            return Ok((uid, gid));
        }
        let name = CString::new(file.user.as_str()).map_err(|_| "the user name is not valid".to_owned())?;
        // SAFETY: `name` is a valid C string; the returned record is read at once, before another call.
        let entry = unsafe { libc::getpwnam(name.as_ptr()) };
        if entry.is_null() {
            return Err(format!("the user \"{}\" does not exist; install the package (it creates it) or give uid and gid in /etc/david.toml", file.user));
        }
        // SAFETY: not null, checked above.
        let (uid, gid) = unsafe { ((*entry).pw_uid, (*entry).pw_gid) };
        Ok((file.uid.unwrap_or(uid), file.gid.unwrap_or(gid)))
    }

    /// The library folders the user needs rights on, read from the database of the data folder (only for the ACLs: Linux).
    #[cfg(target_os = "linux")]
    fn library_folders(data_dir: &Path) -> Vec<std::path::PathBuf> {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return Vec::new() };
        runtime.block_on(async {
            let Ok(store) = crate::Store::open(data_dir).await else { return Vec::new() };
            let Ok(state) = store.import_state().await else { return Vec::new() };
            state.roots.iter().chain(state.music_roots.iter()).map(std::path::PathBuf::from).filter(|p| p.is_dir()).collect()
        })
    }

    /// Hands everything below `dir` to the user, touching only what belongs to somebody else.
    fn chown_tree(dir: &Path, uid: u32, gid: u32) -> std::io::Result<()> {
        use std::os::unix::ffi::OsStrExt;
        let own = |path: &Path| -> std::io::Result<()> {
            let meta = std::fs::symlink_metadata(path)?;
            if meta.uid() != uid || meta.gid() != gid {
                let c = CString::new(path.as_os_str().as_bytes()).map_err(|_| std::io::Error::other("path with a NUL byte"))?;
                // SAFETY: valid C string; `lchown` does not follow links.
                if unsafe { libc::lchown(c.as_ptr(), uid, gid) } != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        };
        own(dir)?;
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if std::fs::symlink_metadata(&path)?.is_dir() {
                chown_tree(&path, uid, gid)?;
            } else {
                own(&path)?;
            }
        }
        Ok(())
    }

    /// The groups of the account as the system lists them. `getgrouplist` takes and gives `int` on macOS and `gid_t` on Linux.
    #[allow(clippy::unnecessary_cast)]
    fn system_groups(user: &CString, gid: u32) -> Vec<libc::gid_t> {
        #[cfg(target_os = "macos")]
        type Raw = libc::c_int;
        #[cfg(not(target_os = "macos"))]
        type Raw = libc::gid_t;
        let mut count: libc::c_int = 32;
        let mut groups: Vec<Raw> = vec![0; count as usize];
        // SAFETY: `groups` has room for `count` entries; the call updates `count` and says -1 if it was too small.
        unsafe {
            if libc::getgrouplist(user.as_ptr(), gid as Raw, groups.as_mut_ptr(), &mut count) == -1 {
                groups.resize(count.max(1) as usize, 0);
                libc::getgrouplist(user.as_ptr(), gid as Raw, groups.as_mut_ptr(), &mut count);
            }
        }
        groups.truncate(count.max(0) as usize);
        groups.into_iter().map(|g| g as libc::gid_t).collect()
    }

    /// `setgroups` takes the number of groups as `size_t` on Linux and as `int` on macOS.
    #[allow(clippy::unnecessary_cast)]
    fn set_groups(groups: &[libc::gid_t]) -> bool {
        #[cfg(target_os = "macos")]
        let n = groups.len() as libc::c_int;
        #[cfg(not(target_os = "macos"))]
        let n = groups.len();
        // SAFETY: the pointer and the number belong to the same slice.
        unsafe { libc::setgroups(n, groups.as_ptr()) == 0 }
    }

    /// The groups of the account (as the system lists them) and the ones named in the file.
    fn group_list(user: &CString, gid: u32, extra: &[String]) -> Vec<libc::gid_t> {
        let mut groups = system_groups(user, gid);
        for name in extra {
            let Ok(c) = CString::new(name.as_str()) else { continue };
            // SAFETY: valid C string; the record is read at once.
            let entry = unsafe { libc::getgrnam(c.as_ptr()) };
            if entry.is_null() {
                eprintln!("david: the group \"{name}\" from /etc/david.toml does not exist, skipped");
                continue;
            }
            // SAFETY: not null, checked above.
            let g = unsafe { (*entry).gr_gid };
            if !groups.contains(&g) {
                groups.push(g);
            }
        }
        groups
    }

    /// As root: rights on the library, the data folder to the user, then become the user. As anyone
    /// else: nothing happens.
    pub fn become_service_user(file: &FileConfig, config: &Config) -> Result<(), String> {
        // SAFETY: plain system call.
        if unsafe { libc::geteuid() } != 0 {
            return Ok(());
        }
        let (uid, gid) = ids(file)?;
        if uid == 0 {
            return Err("the service user must not be root".into());
        }
        std::fs::create_dir_all(&config.data_dir).map_err(|e| format!("{}: {e}", config.data_dir.display()))?;
        #[cfg(not(target_os = "linux"))]
        if file.acl {
            eprintln!("david: rights on the library folders are not set on this system (POSIX ACLs are Linux only): the folders must be readable by {}", file.user);
        }
        #[cfg(target_os = "linux")]
        if file.acl {
            // Read the folders first: the database may be new, so it is handed over afterwards.
            let folders = library_folders(&config.data_dir);
            for folder in &folders {
                let done = crate::posixacl::grant_tree(folder, uid);
                eprintln!(
                    "david: rights for uid {uid} on {}: {} looked at, {} changed, {} failed{}",
                    folder.display(),
                    done.looked_at,
                    done.changed,
                    done.failed,
                    done.first_error.map(|e| format!(" (first: {e})")).unwrap_or_default()
                );
            }
        }
        chown_tree(&config.data_dir, uid, gid).map_err(|e| format!("handing {} to uid {uid}: {e}", config.data_dir.display()))?;
        allow_priority();
        let name = CString::new(file.user.as_str()).map_err(|_| "the user name is not valid".to_owned())?;
        let groups = group_list(&name, gid, &file.groups);
        // SAFETY: plain system calls in the order that works: groups first, then the group, then the user.
        unsafe {
            if !set_groups(&groups) || libc::setgid(gid) != 0 || libc::setuid(uid) != 0 {
                return Err(format!("cannot become {}: {}", file.user, std::io::Error::last_os_error()));
            }
            // A way back would defeat the point.
            if libc::setuid(0) == 0 {
                return Err("still able to become root after dropping the rights".into());
            }
        }
        // SAFETY: still single-threaded here (the runtime above was dropped), as `set_var` needs.
        unsafe {
            std::env::set_var("USER", &file.user);
            std::env::set_var("LOGNAME", &file.user);
        }
        eprintln!("david: running as {} (uid {uid}, gid {gid})", file.user);
        Ok(())
    }
}

/// The limits (`RLIMIT_NICE`, soft and hard) to set so that a thread may ask for the nice value `lowest` (-20 = highest priority), given
/// what they are now: `None` if they allow it already. The limit is `20 - nice`, so -20 needs 40.
pub fn nice_limits_for(lowest: i32, soft: u64, hard: u64) -> Option<(u64, u64)> {
    if lowest >= 0 {
        return None;
    }
    let need = (20 - lowest).min(40) as u64;
    (soft < need || hard < need).then(|| (soft.max(need), hard.max(need)))
}

#[cfg(unix)]
pub use unix::become_service_user;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nice_limit_is_raised_only_as_far_as_the_encoder_needs() {
        assert_eq!(nice_limits_for(-20, 0, 0), Some((40, 40)), "the default of a shell");
        assert_eq!(nice_limits_for(-20, 0, 40), Some((40, 40)));
        assert_eq!(nice_limits_for(-20, 40, 40), None, "LimitNICE=40 of the service unit");
        assert_eq!(nice_limits_for(-20, 60, 60), None, "never lower than it is");
        assert_eq!(nice_limits_for(-10, 20, 30), Some((30, 30)), "nice -10 needs 30");
        assert_eq!(nice_limits_for(-10, 30, 30), None);
        assert_eq!(nice_limits_for(0, 0, 0), None, "no raise asked for");
    }

    #[test]
    fn the_file_gives_ids_and_settings_and_the_environment_beats_it() {
        let file: FileConfig = toml::from_str("user = \"david\"\nuid = 10050\ngid = 10051\nlisten = \"0.0.0.0:8484\"\nmcp = true\nacl = false\n").unwrap();
        assert_eq!((file.uid, file.gid, file.acl), (Some(10050), Some(10051), false));
        assert_eq!(file.as_env("DAVID_LISTEN").as_deref(), Some("0.0.0.0:8484"));
        assert_eq!(file.as_env("DAVID_MCP").as_deref(), Some("1"));
        assert_eq!(file.as_env("DAVID_DATA_DIR"), None);
        let with_log: FileConfig = toml::from_str("log = \"debug\"").unwrap();
        assert_eq!(with_log.as_env("DAVID_LOG").as_deref(), Some("debug"));

        // The file stands in for the environment of the parser: flags and the environment win.
        let env = |k: &str| (k == "DAVID_LISTEN").then(|| "127.0.0.1:9".to_owned()).or_else(|| file.as_env(k));
        let Ok(crate::cli::Command::Run(c)) = crate::cli::parse(["run".to_owned()], env) else { panic!() };
        assert_eq!((c.listen.to_string().as_str(), c.mcp), ("127.0.0.1:9", true));
    }

    #[test]
    fn a_hidden_feature_is_on_only_when_the_environment_or_else_the_file_says_so() {
        let on: FileConfig = toml::from_str("[features]\ndownload = true\n").unwrap();
        let run = |env: Option<&str>, file: &FileConfig| {
            let lookup = |k: &str| env.filter(|_| k == "DAVID_FEATURE_DOWNLOAD").map(str::to_owned).or_else(|| file.as_env(k));
            let Ok(crate::cli::Command::Run(c)) = crate::cli::parse(["run".to_owned()], lookup) else { panic!() };
            c.features.download
        };
        assert!(!run(None, &FileConfig::default()), "off by default");
        assert!(run(None, &on), "the file switches it on");
        assert!(run(Some("1"), &FileConfig::default()), "so does the environment");
        assert!(!run(Some("0"), &on), "the environment wins over the file");
        assert!(toml::from_str::<FileConfig>("[features]\nmovies = true\n").is_err(), "an unknown switch is a mistake");
    }

    #[test]
    fn the_macos_package_file_parses_and_names_the_account_the_launchd_daemon_drops_to() {
        let text = include_str!("../packaging/macos/david.toml");
        let file: FileConfig = toml::from_str(text).unwrap();
        assert_eq!(file.user, "_david", "the hidden account the installer creates");
        assert!(!file.acl, "POSIX ACLs exist on Linux only");
        assert_eq!(file.data_dir.as_deref(), Some(Path::new("/Library/Application Support/David")));
        assert_eq!(file.as_env("DAVID_LISTEN").as_deref(), Some("0.0.0.0:8484,[::]:8484"));
        // The commented examples that are whole lines are real keys, too.
        for line in text.lines().filter_map(|l| l.strip_prefix("# ")).filter(|r| r.contains(" = ") && !r.contains(": ") && !r.contains(". ")) {
            assert!(toml::from_str::<FileConfig>(line).is_ok(), "{line}");
        }
    }

    #[test]
    fn the_packaged_file_parses_and_every_key_in_its_comments_is_a_real_one() {
        let text = include_str!("../packaging/david.toml");
        assert_eq!(toml::from_str::<FileConfig>(text).unwrap(), FileConfig::default().with_acl_from(text));
        // Uncommenting the examples must give a valid file with the certificate keys set.
        let all: String = text.lines().map(|l| l.strip_prefix("# ").filter(|r| (r.contains(" = ") || *r == "[features]") && !r.starts_with(' ')).unwrap_or(l)).collect::<Vec<_>>().join("\n");
        let file: FileConfig = toml::from_str(&all).unwrap();
        assert!(file.tls_cert.is_some() && file.tls_key.is_some() && file.tls_host.is_some() && file.tls_reload_s == Some(300), "{file:?}");
        assert_eq!(file.as_env("DAVID_TLS_HOST").as_deref(), Some("david.example.org"));
        assert!(file.features.download, "the commented example of the hidden feature is a real key");
        assert_eq!(file.as_env("DAVID_FEATURE_DOWNLOAD").as_deref(), Some("1"));
        assert_eq!(FileConfig::default().as_env("DAVID_FEATURE_DOWNLOAD"), None, "off by default, and a file that is off passes nothing on");
        assert_eq!(file.as_env("DAVID_LISTEN").as_deref(), Some("0.0.0.0:8484,[::]:8484"), "a list of addresses");
        // One address as a string works too, and a list reaches the parser as several addresses.
        let one: FileConfig = toml::from_str("listen = \"127.0.0.1:9\"").unwrap();
        assert_eq!(one.as_env("DAVID_LISTEN").as_deref(), Some("127.0.0.1:9"));
        let env = |k: &str| file.as_env(k);
        let Ok(crate::cli::Command::Run(c)) = crate::cli::parse(["run".to_owned()], env) else { panic!() };
        assert_eq!((c.listen.to_string().as_str(), c.also_listen.iter().map(|a| a.to_string()).collect::<Vec<_>>()), ("0.0.0.0:8484", vec!["[::]:8484".to_string()]));
    }

    #[test]
    fn a_missing_file_is_the_defaults_and_a_wrong_one_is_named() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(FileConfig::load(&dir.path().join("none.toml")).unwrap(), FileConfig::default());
        let bad = dir.path().join("bad.toml");
        std::fs::write(&bad, "uid = \"ten\"\n").unwrap();
        assert!(FileConfig::load(&bad).unwrap_err().contains("bad.toml"));
        std::fs::write(&bad, "colour = 1\n").unwrap();
        assert!(FileConfig::load(&bad).is_err(), "an unknown key is a mistake, not ignored");
        assert_eq!(file_path(|_| None), PathBuf::from("/etc/david.toml"));
    }
}
