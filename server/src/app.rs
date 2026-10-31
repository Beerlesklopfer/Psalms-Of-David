// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Runtime of the server, independent of how it is started (console, systemd,
//! Windows service). The platform modules only decide what "shutdown" means.

use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::admin::pages::AdminUi;
use crate::error::{Error, Result};
use crate::roles::Role;
use crate::store::{NewProfile, Store};
use crate::web::{self, AppState};

/// Where the administration interface listens unless told otherwise. Only this
/// machine can reach it; there is no TLS yet, so the LAN is not offered by default.
pub const DEFAULT_LISTEN: &str = "127.0.0.1:8484";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Holds the database. Created on first start.
    pub data_dir: PathBuf,
    /// A library folder for development builds: scanned at start when
    /// `allow_import` is set. Normally the import folders are defined in the
    /// administration interface and kept in the database; this is not one of them.
    pub library: Option<PathBuf>,
    /// Imports are locked by default. Until the release criteria exist (see
    /// `docs/spec-discovery-profiles-roles.md`, section 3c) this developer switch
    /// is the only way to allow the scan at start. It can only be set in
    /// development builds (`debug_assertions`); in a release build the command
    /// line never sets it and `run` ignores it.
    pub allow_import: bool,
    /// Address of the administration interface. Anything but loopback is served
    /// with TLS (see `tls`).
    pub listen: SocketAddr,
    /// Further addresses the interface listens on at the same time (`/etc/david.toml`: `listen = [...]`),
    /// for example `[::]:8484` next to `0.0.0.0:8484`.
    pub also_listen: Vec<SocketAddr>,
    /// Announce the server on the network (mDNS) when it is reachable from there.
    pub announce: bool,
    /// Serve the MCP endpoint (`/mcp`) for a remote LLM; off unless `--mcp` or `DAVID_MCP=1`.
    pub mcp: bool,
    /// The log level the start fixes (flag, `DAVID_LOG` or the file); without one the setting in the
    /// pages decides.
    pub log: Option<tracing::Level>,
    /// A certificate and key of somebody else (Let's Encrypt, ...), instead of the self-signed one.
    pub tls: crate::tls::External,
    /// Hidden features (`[features]` in `/etc/david.toml`, `DAVID_FEATURE_*`); all off by default.
    pub features: crate::features::Features,
}

impl Config {
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("david.db")
    }

    /// Where the database lives unless told otherwise.
    #[cfg(unix)]
    pub fn default_data_dir() -> PathBuf {
        PathBuf::from("/var/lib/david")
    }

    #[cfg(windows)]
    pub fn default_data_dir() -> PathBuf {
        let base = std::env::var_os("PROGRAMDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
        base.join("David")
    }
}

/// Creates the data folder. A folder that is made here is closed to other users of
/// the machine (mode 700 on Unix), because it holds the database with the PIN hashes
/// and the private key of the server. An existing folder is left as the owner set it.
pub fn create_data_dir(path: &std::path::Path) -> std::io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Opens the store, scans the library if the import is allowed, starts the
/// administration interface, calls `on_ready` with the address it listens on,
/// then serves until `shutdown` completes. Pending progress is flushed before
/// returning.
pub async fn run(
    config: Config,
    shutdown_future: impl Future<Output = ()> + Send + 'static,
    on_ready: impl FnOnce(SocketAddr),
) -> Result<()> {
    crate::buildinfo::mark_started();
    tracing::info!(version = %crate::buildinfo::describe(), "starting");
    create_data_dir(&config.data_dir)?;
    let store = Store::open(&config.db_path()).await?;
    // Without a level fixed at the start, the setting of the pages decides.
    if config.log.is_none()
        && let Some(level) = store.setting("log_level").await?.as_deref().and_then(crate::logging::parse)
    {
        crate::logging::set(level);
    }

    // The import. Belt and braces: even a Config built by other code cannot use the
    // development switch in a release build.
    let dev_library = config.library.as_ref().filter(|_| cfg!(debug_assertions) && config.allow_import);
    if let Some(library) = dev_library {
        if library.is_dir() {
            let report = store.scan(library).await?;
            tracing::info!(?report, library = %library.display(), "library scanned (development switch)");
        } else {
            // Keep running: a library that is not mounted yet should not stop the service.
            tracing::warn!(library = %library.display(), "library folder not found, skipping scan");
        }
    } else if store.import_state().await?.enabled {
        match store.run_import().await {
            Ok(report) => tracing::info!(?report, "import finished"),
            // A failing import must not keep the administration interface from starting.
            Err(e) => tracing::error!(error = %e, "import failed"),
        }
    } else {
        tracing::info!("import is locked; nothing is read from any folder");
    }

    // New files are picked up by themselves, at the interval of the settings (0 = never).
    let auto_import = {
        let store = store.clone();
        tokio::spawn(async move {
            let mut last = std::time::Instant::now();
            let mut tick = tokio::time::interval(Duration::from_secs(30));
            loop {
                tick.tick().await;
                let minutes: u64 = store.setting("import_interval_min").await.ok().flatten().and_then(|v| v.parse().ok()).unwrap_or(10);
                if minutes == 0 || last.elapsed() < Duration::from_secs(minutes * 60) {
                    continue;
                }
                last = std::time::Instant::now();
                if !store.import_state().await.map(|s| s.enabled).unwrap_or(false) {
                    continue;
                }
                match store.run_import().await {
                    Ok(r) if r.tracks_added + r.tracks_updated + r.tracks_removed > 0 => tracing::info!(report = ?r, "automatic import found changes"),
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, "automatic import failed"),
                }
            }
        })
    };
    let _ = store.purge_expired_sessions().await;
    let _ = store.refresh_ages().await;
    let purger = {
        let store = store.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(3600));
            loop {
                tick.tick().await;
                let _ = store.purge_expired_sessions().await;
                // A child's age limit rises on the birthday.
                let _ = store.refresh_ages().await;
            }
        })
    };

    // Everything but loopback speaks TLS: with a certificate made at the first start (devices pin its fingerprint) or,
    // if the settings name files, with the certificate from somebody else (a certificate authority: devices check the
    // name instead). What the settings of the pages say is the lowest level; the flags, the environment and the file win.
    let external = config.tls.clone().or(store.external_tls().await?);
    let wants_tls = crate::tls::wanted(config.listen) || config.also_listen.iter().any(|a| crate::tls::wanted(*a));
    let tls = if !wants_tls {
        None
    } else if external.is_set() {
        if external.host.as_deref().is_none_or(|h| h.trim().is_empty()) {
            return Err(Error::WriteFailed("TLS: a certificate from somebody else needs the name that devices connect to (tls_host)".into()));
        }
        Some(crate::tls::load_external(&external)?)
    } else {
        Some(crate::tls::ensure_certificate(&config.data_dir, config.listen)?)
    };
    // With a certificate from a certificate authority devices check the name, not the fingerprint.
    let ca_host = external.is_set().then(|| external.host.clone().unwrap_or_default().trim().to_owned()).filter(|_| tls.is_some());
    let ui = AdminUi::with_override_dirs(crate::admin::pages::default_template_dirs())?.with_features(config.features);
    for path in &ui.overrides {
        tracing::info!(template = %path.display(), "using a template that replaces the built-in one");
    }
    crate::feeds::start_refresher(store.clone());
    let cd = crate::cdimport::CdService::new(&config.data_dir).with_library(config.library.clone());
    cd.start(&store).await;
    tokio::spawn(crate::cdrive::monitor(cd.clone(), store.clone(), config.library.clone()));
    // CDs that wait for approval expire after the time of the settings.
    tokio::spawn(crate::cdflow::expiry_task(store.clone(), cd.clone()));
    if config.features.download {
        tracing::info!("hidden feature on: administrators may download files (features.download)");
    }
    let state = AppState {
        store: store.clone(),
        mcp: config.mcp,
        listen: config.listen,
        ui: Arc::new(ui),
        secure_cookie: tls.is_some(),
        fingerprint: tls.as_ref().filter(|_| ca_host.is_none()).map(|t| t.fingerprint.clone()),
        ca_host: ca_host.clone(),
        library: config.library.clone(),
        uploads: crate::upload::Uploads::start(store.clone(), &config.data_dir),
        cd: cd.clone(),
        features: config.features,
    };
    // Every address of the list, each its own socket: `[::]` listens on IPv6 only, so that it can sit next to
    // `0.0.0.0` (IPv4). A machine without IPv6 skips the IPv6 address with a warning, as long as another one is left.
    let mut all = vec![config.listen];
    all.extend(config.also_listen.iter().copied().filter(|a| *a != config.listen));
    let std_listeners = bind_all(&all)?;
    let std_listener = std_listeners.first().ok_or_else(|| Error::WriteFailed("no address to listen on".into()))?;
    let addr = std_listener.local_addr()?;
    let others: Vec<SocketAddr> = std_listeners.iter().skip(1).filter_map(|l| l.local_addr().ok()).collect();
    match &tls {
        None => tracing::info!(%addr, ?others, "administration interface is listening (this machine only, plain HTTP)"),
        Some(t) if ca_host.is_some() => tracing::info!(%addr, ?others, host = ca_host.as_deref().unwrap_or_default(), fingerprint = %t.fingerprint, "administration interface is listening (HTTPS, certificate from a certificate authority)"),
        Some(t) => tracing::info!(%addr, ?others, fingerprint = %t.fingerprint, "administration interface is listening (HTTPS, self-signed certificate)"),
    }
    // Announce on the network, but only if the network can reach the interface. The
    // announcement is withdrawn when the server stops.
    let _announcement = if config.announce && crate::discovery::should_announce(addr) {
        let name = store.setting("server_name").await?.unwrap_or_else(|| "David".to_owned());
        match crate::discovery::Announcement::start(&store.server_id().await?, &name, addr) {
            Ok(a) => {
                tracing::info!(service = a.fullname(), "announced on the network (mDNS)");
                Some(a)
            }
            Err(e) => {
                // Not fatal: the server works, devices just have to be given its address.
                tracing::warn!(error = %e, "could not announce on the network");
                None
            }
        }
    } else {
        None
    };
    on_ready(addr);

    let cd_stop = cd.clone();
    let cd_for_mcp = cd.clone();
    let mut router = web::router(state).merge(crate::api::router(crate::api::ApiState {
        store: store.clone(),
        library: config.library.clone(),
        cd,
        features: config.features,
    }));
    if config.mcp {
        tracing::info!("MCP endpoint on /mcp (tokens are made in the settings)");
        router = router.merge(crate::mcp::router(crate::mcp::McpState { store: store.clone(), library: config.library.clone(), cd: cd_for_mcp }));
    }
    match tls {
        None => {
            // One server per address, all with the same router; the first to stop ends them all.
            let shutdown = std::sync::Arc::new(tokio::sync::Notify::new());
            let stopper = shutdown.clone();
            tokio::spawn(async move {
                shutdown_future.await;
                // A CD that is being read finishes the track it is on (if that takes a few seconds), writes down what is
                // finished and ends; the next start can resume it.
                cd_stop.drives.stop_gracefully(crate::cdrive::STOP_GRACE, crate::cdrive::STOP_PATIENCE).await;
                stopper.notify_waiters();
            });
            let mut servers = tokio::task::JoinSet::new();
            for l in std_listeners {
                let listener = tokio::net::TcpListener::from_std(l)?;
                let router = router.clone();
                let stop = shutdown.clone();
                servers.spawn(async move { axum::serve(listener, router).with_graceful_shutdown(async move { stop.notified().await }).await });
            }
            while let Some(done) = servers.join_next().await {
                done.map_err(|e| Error::WriteFailed(e.to_string()))??;
            }
        }
        Some(material) => {
            crate::tls::install_crypto_provider();
            let rustls = axum_server::tls_rustls::RustlsConfig::from_pem(material.cert_pem, material.key_pem).await?;
            let watcher = ca_host.is_some().then(|| crate::tls::watch_external(rustls.clone(), external.clone()));
            let handle = axum_server::Handle::new();
            let stopper = handle.clone();
            tokio::spawn(async move {
                shutdown_future.await;
                cd_stop.drives.stop_gracefully(crate::cdrive::STOP_GRACE, crate::cdrive::STOP_PATIENCE).await;
                // Let running requests finish, but not for ever.
                stopper.graceful_shutdown(Some(Duration::from_secs(5)));
            });
            let mut servers = tokio::task::JoinSet::new();
            for l in std_listeners {
                let (rustls, handle, router) = (rustls.clone(), handle.clone(), router.clone());
                servers.spawn(async move { axum_server::from_tcp_rustls(l, rustls)?.handle(handle).serve(router.into_make_service()).await });
            }
            while let Some(done) = servers.join_next().await {
                done.map_err(|e| Error::WriteFailed(e.to_string()))??;
            }
            if let Some(w) = watcher {
                w.abort();
            }
        }
    }

    tracing::info!("shutting down");
    purger.abort();
    auto_import.abort();
    store.flush().await?;
    Ok(())
}

/// Binds every address of the list. An IPv6 address listens for IPv6 only (so that `[::]` and `0.0.0.0` can
/// both be bound); a machine that has no IPv6 skips it with a warning if another address is left.
fn bind_all(addresses: &[SocketAddr]) -> Result<Vec<std::net::TcpListener>> {
    let mut out = Vec::new();
    for addr in addresses {
        match bind_one(*addr) {
            Ok(l) => out.push(l),
            Err(e) if addr.is_ipv6() && matches!(e.raw_os_error(), Some(97) | Some(99)) && addresses.len() > 1 => {
                tracing::warn!(address = %addr, error = %e, "this machine cannot listen on IPv6, skipping that address");
            }
            Err(e) => return Err(Error::WriteFailed(format!("cannot listen on {addr}: {e}"))),
        }
    }
    Ok(out)
}

fn bind_one(addr: SocketAddr) -> std::io::Result<std::net::TcpListener> {
    use socket2::{Domain, Protocol, Socket, Type};
    let socket = Socket::new(Domain::for_address(addr), Type::STREAM, Some(Protocol::TCP))?;
    if addr.is_ipv6() {
        socket.set_only_v6(true)?;
    }
    socket.set_reuse_address(true)?;
    socket.bind(&addr.into())?;
    socket.listen(1024)?;
    socket.set_nonblocking(true)?;
    Ok(socket.into())
}

/// Creates the first administrator. Refuses if an administrator exists already, so
/// it cannot be used to add a second one behind the interface's back.
pub async fn init_admin(data_dir: &std::path::Path, name: &str, pin: &str) -> Result<()> {
    // Check the PIN or password first: a refused one must not leave an administrator without one.
    if !crate::auth::valid_secret(pin) {
        return Err(Error::InvalidPin);
    }
    create_data_dir(data_dir)?;
    let store = Store::open(&data_dir.join("david.db")).await?;
    if store.has_admin().await? {
        return Err(Error::AdminExists);
    }
    let id = store.create_user(NewProfile::new(name, Role::Admin)).await?;
    store.set_pin(id, pin).await?;
    Ok(())
}

/// Sets a new PIN or password for an existing user: for the one who forgot theirs. The user is
/// signed out everywhere (their sessions end) and a lock after failed attempts is lifted.
pub async fn reset_pin(data_dir: &std::path::Path, name: &str, pin: &str) -> Result<()> {
    if !crate::auth::valid_secret(pin) {
        return Err(Error::InvalidPin);
    }
    let db = data_dir.join("david.db");
    if !db.is_file() {
        return Err(Error::WriteFailed(format!("there is no database in {}", data_dir.display())));
    }
    let store = Store::open(&db).await?;
    let user = store
        .list_profiles()
        .await?
        .into_iter()
        .find(|p| p.name == name)
        .ok_or_else(|| Error::WriteFailed(format!("no user named {name}")))?;
    store.set_pin(user.id, pin).await?;
    store.end_sessions_of(user.id).await?;
    Ok(())
}

/// Completes on Ctrl-C and, on Unix, on SIGTERM (what systemd sends).
pub async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(windows)]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

pub fn init_stderr_logging(requested: Option<tracing::Level>) {
    crate::logging::init_stderr(requested);
}
