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
            let mut names = vec!["localhost".to_owned(), "127.0.0.1".to_owned()];
            if !listen.ip().is_unspecified() && !listen.ip().is_loopback() {
                names.push(listen.ip().to_string());
            }
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
}
