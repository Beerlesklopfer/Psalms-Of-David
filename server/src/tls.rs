// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The server certificate and TLS for everything but loopback.
//!
//! A self-signed certificate is made at the first start and kept in `<data dir>/tls/`.
//! There is no certificate authority: a device that is paired by QR code gets the
//! SHA-256 fingerprint of the certificate in that code and accepts only a server that
//! shows exactly this certificate (spec `spec-discovery-profiles-roles.md`, section 4).
//! Because devices rely on it, an existing certificate is never replaced silently.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Once;

use rustls::pki_types::{CertificateDer, pem::PemObject};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

/// A certificate and its private key, as PEM, and the fingerprint that pairing uses.
#[derive(Clone)]
pub struct TlsMaterial {
    pub cert_pem: Vec<u8>,
    pub key_pem: Vec<u8>,
    /// SHA-256 of the certificate in DER form, 64 lower-case hex digits.
    pub fingerprint: String,
}

/// A certificate and key that somebody else looks after (Let's Encrypt, a company CA). Every value comes
/// from the settings of the machine: nothing about the place or the names is built in. `host` is the
/// name devices connect to; with a certificate from a CA it replaces the fingerprint in the pairing
/// code, because such a certificate is renewed and with it the fingerprint changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct External {
    pub cert: Option<PathBuf>,
    pub key: Option<PathBuf>,
    pub host: Option<String>,
    /// How often the files are looked at for a renewed certificate, in seconds; `None` is the default
    /// (`DEFAULT_RELOAD_S`).
    pub reload_s: Option<u64>,
}

/// Looked at every this many seconds when nothing else is set (a renewal is not urgent to the second).
pub const DEFAULT_RELOAD_S: u64 = 300;

impl External {
    /// Values that are not set here are taken from `other` (the database settings).
    pub fn or(self, other: External) -> External {
        External { cert: self.cert.or(other.cert), key: self.key.or(other.key), host: self.host.or(other.host), reload_s: self.reload_s.or(other.reload_s) }
    }

    /// Both files are named: the server uses them instead of its own certificate.
    pub fn is_set(&self) -> bool {
        self.cert.is_some() && self.key.is_some()
    }
}

/// Reads a certificate and a key that somebody else provides. The error names the file and says what to
/// check, most often the rights: the service has to be in the group that may read the files.
pub fn load_external(ext: &External) -> Result<TlsMaterial> {
    let (cert, key) = match (&ext.cert, &ext.key) {
        (Some(c), Some(k)) => (c, k),
        _ => return Err(tls_error("the certificate and the key have to be named together")),
    };
    let read = |path: &Path| -> Result<Vec<u8>> {
        std::fs::read(path).map_err(|e| {
            let hint = if e.kind() == std::io::ErrorKind::PermissionDenied { " (the service user may not read it: is it in the group that owns the file, and may that group read the folders above it?)" } else { "" };
            tls_error(format!("{}: {e}{hint}", path.display()))
        })
    };
    let cert_pem = read(cert)?;
    let key_pem = read(key)?;
    let fingerprint = fingerprint_of_pem(&cert_pem)?;
    // The key must be one that rustls can use; better to learn it here than at the first connection.
    install_crypto_provider();
    rustls::pki_types::PrivateKeyDer::from_pem_slice(&key_pem).map_err(|e| tls_error(format!("{}: not a private key: {e}", key.display())))?;
    Ok(TlsMaterial { cert_pem, key_pem, fingerprint })
}

/// When the two files were last changed, as one number (the newer of both), to notice a renewal.
fn changed_at(ext: &External) -> Option<std::time::SystemTime> {
    let m = |p: &Option<PathBuf>| p.as_ref().and_then(|p| std::fs::metadata(p).ok()).and_then(|m| m.modified().ok());
    m(&ext.cert).into_iter().chain(m(&ext.key)).max()
}

/// Looks at the files from time to time and loads a renewed certificate into the running server, so that
/// a renewal needs no restart. A file that cannot be read keeps the certificate in use.
pub fn watch_external(config: axum_server::tls_rustls::RustlsConfig, ext: External) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let every = std::time::Duration::from_secs(ext.reload_s.unwrap_or(DEFAULT_RELOAD_S).max(5));
        let mut last = changed_at(&ext);
        loop {
            tokio::time::sleep(every).await;
            let now = changed_at(&ext);
            if now == last {
                continue;
            }
            match load_external(&ext) {
                Ok(m) => match config.reload_from_pem(m.cert_pem, m.key_pem).await {
                    Ok(()) => {
                        tracing::info!(fingerprint = %m.fingerprint, "renewed certificate loaded");
                        last = now;
                    }
                    Err(e) => tracing::warn!(error = %e, "the renewed certificate could not be loaded; the old one stays"),
                },
                Err(e) => tracing::warn!(error = %e, "the renewed certificate cannot be read yet; the old one stays"),
            }
        }
    })
}

/// Does a server listening on `addr` speak TLS? Everything that other machines can
/// reach does. Loopback stays plain HTTP, so local tools and tests need no trust setup.
pub fn wanted(addr: SocketAddr) -> bool {
    !addr.ip().is_loopback()
}

/// Installs the crypto provider rustls needs. Safe to call many times.
pub fn install_crypto_provider() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

fn paths(data_dir: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let dir = data_dir.join("tls");
    (dir.clone(), dir.join("cert.pem"), dir.join("key.pem"))
}

fn tls_error(what: impl std::fmt::Display) -> Error {
    Error::WriteFailed(format!("TLS: {what}"))
}

/// The fingerprint of the first certificate in a PEM file.
pub fn fingerprint_of_pem(pem: &[u8]) -> Result<String> {
    let der = CertificateDer::from_pem_slice(pem).map_err(|e| tls_error(format!("not a certificate: {e}")))?;
    Ok(fingerprint_of_der(der.as_ref()))
}

pub fn fingerprint_of_der(der: &[u8]) -> String {
    Sha256::digest(der).iter().map(|b| format!("{b:02x}")).collect()
}

/// Loads the certificate of this server, or makes it if there is none yet.
///
/// If only one of the two files exists, or a file cannot be read, this fails
/// instead of making a new certificate: a new one would lock out every paired device.
pub fn ensure_certificate(data_dir: &Path, listen: SocketAddr) -> Result<TlsMaterial> {
    let (dir, cert_path, key_path) = paths(data_dir);
    match (cert_path.exists(), key_path.exists()) {
        (true, true) => {
            let cert_pem = std::fs::read(&cert_path)?;
            let key_pem = std::fs::read(&key_path)?;
            let fingerprint = fingerprint_of_pem(&cert_pem)?;
            Ok(TlsMaterial { cert_pem, key_pem, fingerprint })
        }
        (false, false) => {
            std::fs::create_dir_all(&dir)?;
            let names = certificate_names(listen);
            let key = rcgen::KeyPair::generate().map_err(tls_error)?;
            let mut params = rcgen::CertificateParams::new(names).map_err(tls_error)?;
            params.distinguished_name.push(rcgen::DnType::CommonName, "David server");
            let now = time::OffsetDateTime::now_utc();
            params.not_before = now - time::Duration::days(1);
            // Devices pin the certificate, so a long life is what keeps them paired.
            params.not_after = now + time::Duration::days(3650);
            let cert = params.self_signed(&key).map_err(tls_error)?;
            let (cert_pem, key_pem) = (cert.pem().into_bytes(), key.serialize_pem().into_bytes());
            write_private(&key_path, &key_pem)?;
            std::fs::write(&cert_path, &cert_pem)?;
            Ok(TlsMaterial { cert_pem, key_pem, fingerprint: fingerprint_of_der(cert.der().as_ref()) })
        }
        _ => Err(tls_error(format!(
            "{} has only one of cert.pem and key.pem; refusing to make a new certificate, \
             because paired devices would no longer trust the server",
            dir.display()
        ))),
    }
}

/// The names a certificate must carry so that a client can check the host it connects to: this
/// machine (`localhost`), the address it listens on or, when it listens on every interface, all
/// addresses of this machine, and its host name (also as `<name>.local`, which mDNS resolves).
pub fn certificate_names(listen: SocketAddr) -> Vec<String> {
    let mut names = vec!["localhost".to_owned(), "127.0.0.1".to_owned(), "::1".to_owned()];
    let mut add = |name: String| {
        if !names.contains(&name) {
            names.push(name);
        }
    };
    if listen.ip().is_unspecified() {
        for interface in if_addrs::get_if_addrs().unwrap_or_default() {
            let ip = interface.ip();
            // A link-local IPv6 address needs a zone to be used; nobody connects to it by name.
            let link_local = matches!(ip, std::net::IpAddr::V6(v6) if (v6.segments()[0] & 0xffc0) == 0xfe80);
            if !ip.is_loopback() && !link_local {
                add(ip.to_string());
            }
        }
        if let Some(host) = host_name() {
            add(format!("{host}.local"));
            add(host);
        }
    } else if !listen.ip().is_loopback() {
        add(listen.ip().to_string());
    }
    names
}

/// The host name of this machine, if it can be found.
fn host_name() -> Option<String> {
    #[cfg(unix)]
    let name = std::fs::read_to_string("/proc/sys/kernel/hostname").or_else(|_| std::fs::read_to_string("/etc/hostname")).ok();
    #[cfg(windows)]
    let name = std::env::var("COMPUTERNAME").ok();
    let name = name?.trim().to_owned();
    // Only a plain host name: it becomes a DNS name in the certificate.
    (!name.is_empty() && name.len() <= 63 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')).then_some(name)
}

/// Replaces the certificate by a new one with the current names (`david renew-cert`). The old files
/// are kept as `cert.pem.old` and `key.pem.old`. Every paired device pins the fingerprint of the
/// certificate, so each one has to be paired again afterwards.
pub fn renew_certificate(data_dir: &Path, listen: SocketAddr) -> Result<TlsMaterial> {
    let (_, cert_path, key_path) = paths(data_dir);
    for path in [&cert_path, &key_path] {
        if path.exists() {
            let mut old = path.clone().into_os_string();
            old.push(".old");
            std::fs::rename(path, old)?;
        }
    }
    ensure_certificate(data_dir, listen)
}

/// Writes the private key so that only the owner can read it.
fn write_private(path: &Path, data: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(data)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr() -> SocketAddr {
        "192.168.24.250:8484".parse().unwrap()
    }

    #[test]
    fn only_addresses_that_others_can_reach_get_tls() {
        assert!(!wanted("127.0.0.1:8484".parse().unwrap()));
        assert!(!wanted("[::1]:8484".parse().unwrap()));
        assert!(wanted("192.168.24.250:8484".parse().unwrap()));
        assert!(wanted("0.0.0.0:8484".parse().unwrap()));
    }

    #[test]
    fn a_certificate_is_made_once_and_loaded_afterwards() {
        let dir = tempfile::TempDir::new().unwrap();
        let first = ensure_certificate(dir.path(), addr()).unwrap();
        assert_eq!(first.fingerprint.len(), 64);
        assert!(first.fingerprint.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
        assert!(String::from_utf8_lossy(&first.cert_pem).contains("BEGIN CERTIFICATE"));
        assert!(String::from_utf8_lossy(&first.key_pem).contains("PRIVATE KEY"));

        // The second call gives the same certificate, not a new one.
        let second = ensure_certificate(dir.path(), addr()).unwrap();
        assert_eq!(second.fingerprint, first.fingerprint);
        assert_eq!(second.cert_pem, first.cert_pem);
        // And the fingerprint of the file equals the one computed when it was made.
        assert_eq!(fingerprint_of_pem(&first.cert_pem).unwrap(), first.fingerprint);
    }

    #[cfg(unix)]
    #[test]
    fn the_private_key_is_readable_by_its_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().unwrap();
        ensure_certificate(dir.path(), addr()).unwrap();
        let mode = std::fs::metadata(dir.path().join("tls/key.pem")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "mode {mode:o}");
    }

    #[test]
    fn a_half_present_certificate_is_an_error_and_nothing_is_replaced() {
        let dir = tempfile::TempDir::new().unwrap();
        let made = ensure_certificate(dir.path(), addr()).unwrap();
        std::fs::remove_file(dir.path().join("tls/key.pem")).unwrap();
        let r = ensure_certificate(dir.path(), addr());
        assert!(r.is_err(), "must not quietly make a new certificate");
        assert_eq!(std::fs::read(dir.path().join("tls/cert.pem")).unwrap(), made.cert_pem, "the old certificate stays");
    }

    #[test]
    fn a_damaged_certificate_file_is_an_error() {
        let dir = tempfile::TempDir::new().unwrap();
        ensure_certificate(dir.path(), addr()).unwrap();
        std::fs::write(dir.path().join("tls/cert.pem"), b"not a certificate").unwrap();
        assert!(ensure_certificate(dir.path(), addr()).is_err());
    }

    #[test]
    fn the_certificate_names_the_machine_and_the_listen_address() {
        let dir = tempfile::TempDir::new().unwrap();
        let m = ensure_certificate(dir.path(), addr()).unwrap();
        let text = String::from_utf8_lossy(&m.cert_pem).into_owned();
        assert!(text.starts_with("-----BEGIN CERTIFICATE-----"));
        // Names are inside the DER; looking for the readable parts is enough here.
        let der = CertificateDer::from_pem_slice(&m.cert_pem).unwrap();
        let bytes = der.as_ref();
        assert!(bytes.windows(9).any(|w| w == b"localhost"));
        assert!(bytes.windows(12).any(|w| w == b"David server"));
    }

    #[test]
    fn listening_on_every_interface_names_the_addresses_of_the_machine() {
        let names = certificate_names("0.0.0.0:8484".parse().unwrap());
        assert!(names.iter().any(|n| n == "localhost") && names.iter().any(|n| n == "127.0.0.1"));
        let own: Vec<String> = if_addrs::get_if_addrs().unwrap().into_iter().map(|i| i.ip()).filter(|ip| ip.is_ipv4() && !ip.is_loopback()).map(|ip| ip.to_string()).collect();
        for ip in &own {
            assert!(names.contains(ip), "{ip} is missing in {names:?}");
        }
        // One address: that one, and no other machine's.
        let one = certificate_names("192.168.24.250:8484".parse().unwrap());
        assert!(one.contains(&"192.168.24.250".to_owned()) && one.len() == 4, "{one:?}");
        assert_eq!(certificate_names("127.0.0.1:8484".parse().unwrap()).len(), 3);
    }

    #[test]
    fn renewing_makes_a_new_certificate_and_keeps_the_old_one_aside() {
        let dir = tempfile::TempDir::new().unwrap();
        let first = ensure_certificate(dir.path(), addr()).unwrap();
        let second = renew_certificate(dir.path(), addr()).unwrap();
        assert_ne!(first.fingerprint, second.fingerprint);
        assert_eq!(std::fs::read(dir.path().join("tls/cert.pem.old")).unwrap(), first.cert_pem);
        assert!(dir.path().join("tls/key.pem.old").exists());
        // Afterwards the new one is the one that is loaded.
        assert_eq!(ensure_certificate(dir.path(), addr()).unwrap().fingerprint, second.fingerprint);
    }
}
