// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Windows: runs as a system service under the Service Control Manager (SCM).
//!
//! `install` registers the executable with `service --data-dir ... --library ...`
//! as its command line, so the SCM starts it through `run_service`. Stop and
//! shutdown requests from the SCM end the server the same way Ctrl-C does:
//! pending progress is flushed first.

use std::ffi::OsString;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use windows_service::service::{
    ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode,
    ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_service::{define_windows_service, service_dispatcher};

use crate::app::{self, Config};

const SERVICE_NAME: &str = "DavidServer";
const DISPLAY_NAME: &str = "David server";
const DESCRIPTION: &str = "Audiobook and music library for the family.";
const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;
/// Least privilege. The service account needs read access to the library and
/// write access to the data folder (the service creates it on first start).
const ACCOUNT: &str = r"NT AUTHORITY\LocalService";

/// The SCM calls `service_main` without arguments, so the configuration is
/// handed over through this cell.
static CONFIG: OnceLock<Config> = OnceLock::new();

/// Console run (for development); the service itself uses `run_service`.
pub fn run_foreground(config: Config) -> u8 {
    app::init_stderr_logging();
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("david: cannot start the async runtime: {e}");
            return 1;
        }
    };
    match runtime.block_on(app::run(config, app::shutdown_signal(), |_addr| {})) {
        Ok(()) => 0,
        Err(e) => {
            tracing::error!(error = %e, "server stopped with an error");
            1
        }
    }
}

define_windows_service!(ffi_service_main, service_main);

/// Hands the process over to the SCM. Fails with error 1063 if started from a
/// console instead of by the SCM.
pub fn run_service(config: Config) -> u8 {
    let _ = CONFIG.set(config);
    match service_dispatcher::start(SERVICE_NAME, ffi_service_main) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!(
                "david: could not connect to the service manager ({e}). \
                 `service` is started by Windows; use `run` in a console."
            );
            1
        }
    }
}

fn service_main(_arguments: Vec<OsString>) {
    if let Err(e) = run_as_service() {
        tracing::error!(error = %e, "service failed");
    }
}

fn status(state: ServiceState, accept: ServiceControlAccept, exit: u32, wait_hint: Duration) -> ServiceStatus {
    ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: state,
        controls_accepted: accept,
        exit_code: ServiceExitCode::Win32(exit),
        checkpoint: 0,
        wait_hint,
        process_id: None,
    }
}

fn run_as_service() -> Result<(), Box<dyn std::error::Error>> {
    let config = CONFIG.get().cloned().ok_or("service started without configuration")?;
    init_file_logging(&config);

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let stop_tx = Mutex::new(Some(stop_tx));
    let handle = service_control_handler::register(SERVICE_NAME, move |event| match event {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            if let Some(tx) = stop_tx.lock().unwrap().take() {
                let _ = tx.send(());
            }
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    })?;

    handle.set_service_status(status(
        ServiceState::StartPending,
        ServiceControlAccept::empty(),
        0,
        Duration::from_secs(30),
    ))?;

    let runtime = tokio::runtime::Runtime::new()?;
    let result = runtime.block_on(app::run(
        config,
        async move {
            let _ = stop_rx.await;
            let _ = handle.set_service_status(status(
                ServiceState::StopPending,
                ServiceControlAccept::empty(),
                0,
                Duration::from_secs(30),
            ));
        },
        move |_addr| {
            let _ = handle.set_service_status(status(
                ServiceState::Running,
                ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
                0,
                Duration::ZERO,
            ));
        },
    ));

    // 0 = success; ERROR_SERVICE_SPECIFIC_ERROR (1066) tells the SCM we failed.
    let exit = if result.is_ok() { 0 } else { 1066 };
    handle.set_service_status(status(ServiceState::Stopped, ServiceControlAccept::empty(), exit, Duration::ZERO))?;
    result.map_err(Into::into)
}

/// A service has no console, so log to a file in the data folder.
fn init_file_logging(config: &Config) {
    let _ = std::fs::create_dir_all(&config.data_dir);
    if let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(config.data_dir.join("david.log")) {
        let _ = tracing_subscriber::fmt()
            .with_max_level(app::log_level())
            .with_ansi(false)
            .with_writer(Mutex::new(file))
            .try_init();
    }
}

/// Registers the service. Needs an elevated prompt.
pub fn install(config: Config) -> u8 {
    if !config.data_dir.is_absolute() {
        eprintln!("david: --data-dir must be an absolute path for a service");
        return 2;
    }
    match try_install(config) {
        Ok(()) => {
            println!("Service {SERVICE_NAME} installed. Start it with: sc start {SERVICE_NAME}");
            0
        }
        Err(e) => {
            eprintln!("david: install failed: {e} (run from an elevated prompt)");
            1
        }
    }
}

fn try_install(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CREATE_SERVICE)?;
    let info = ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from(DISPLAY_NAME),
        service_type: SERVICE_TYPE,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: std::env::current_exe()?,
        launch_arguments: vec![
            OsString::from("service"),
            OsString::from("--data-dir"),
            config.data_dir.into_os_string(),
            OsString::from("--listen"),
            OsString::from(config.listen.to_string()),
        ],
        dependencies: vec![],
        account_name: Some(OsString::from(ACCOUNT)),
        account_password: None,
    };
    let service = manager.create_service(&info, ServiceAccess::CHANGE_CONFIG)?;
    service.set_description(DESCRIPTION)?;
    Ok(())
}

/// Stops the service if it is running and removes it. Needs an elevated prompt.
pub fn uninstall() -> u8 {
    match try_uninstall() {
        Ok(()) => {
            println!("Service {SERVICE_NAME} removed.");
            0
        }
        Err(e) => {
            eprintln!("david: uninstall failed: {e} (run from an elevated prompt)");
            1
        }
    }
}

fn try_uninstall() -> Result<(), Box<dyn std::error::Error>> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    let service = manager.open_service(
        SERVICE_NAME,
        ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE,
    )?;
    if service.query_status()?.current_state != ServiceState::Stopped {
        service.stop()?;
        // Give the server time to flush; the SCM removes the service when it has stopped.
        for _ in 0..30 {
            if service.query_status()?.current_state == ServiceState::Stopped {
                break;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    service.delete()?;
    Ok(())
}
