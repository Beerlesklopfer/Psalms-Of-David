// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! TLS over a real socket, with a client that behaves like a paired device: it
//! accepts a server only if the certificate has the fingerprint from the QR code.

use std::io::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use david_server::app::{Config, init_admin, run};
use david_server::tls::{fingerprint_of_der, fingerprint_of_pem};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio_rustls::TlsConnector;

/// What a paired device does: trust no authority, trust one certificate. The
/// signatures of the handshake are still checked; only the chain of trust is
/// replaced by the pinned fingerprint.
#[derive(Debug)]
struct Pinned {
    fingerprint: String,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if fingerprint_of_der(end_entity.as_ref()) == self.fingerprint {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General("the certificate is not the pinned one".into()))
        }
    }

    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

fn connector(pinned: &str) -> TlsConnector {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Pinned { fingerprint: pinned.to_owned(), provider }))
        .with_no_client_auth();
    TlsConnector::from(Arc::new(config))
}

async fn https(addr: SocketAddr, pinned: &str, request: &str) -> Result<String, String> {
    https_as(addr, pinned, "david", request).await
}

/// Like `https`, asking for the name `name` (what a browser or a device that connects by a name does).
async fn https_as(addr: SocketAddr, pinned: &str, name: &str, request: &str) -> Result<String, String> {
    let tcp = TcpStream::connect(addr).await.map_err(|e| e.to_string())?;
    let mut tls = connector(pinned)
        .connect(ServerName::try_from(name.to_owned()).unwrap(), tcp)
        .await
        .map_err(|e| format!("handshake: {e}"))?;
    tls.write_all(request.as_bytes()).await.map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    // The server closes after the answer because of `Connection: close`; a missing
    // close_notify at the end is not an error for this purpose.
    let _ = tls.read_to_end(&mut raw).await;
    Ok(String::from_utf8_lossy(&raw).into_owned())
}

#[tokio::test]
async fn a_device_that_pins_the_fingerprint_connects_and_any_other_server_is_refused() {
    let dir = tempfile::TempDir::new().unwrap();
    init_admin(dir.path(), "Boss", "483920").await.unwrap();
    let config = Config {
        data_dir: dir.path().to_owned(),
        library: None,
        allow_import: false,
        announce: false,
        mcp: false,
        log: None,
        tls: Default::default(),
        also_listen: Vec::new(),
        // Not loopback, so the server speaks TLS. Port 0 lets the system choose.
        listen: "0.0.0.0:0".parse().unwrap(),
    };
    let (ready_tx, ready_rx) = oneshot::channel();
    let (stop, stop_rx) = oneshot::channel::<()>();
    let task = tokio::spawn(run(
        config,
        async move {
            let _ = stop_rx.await;
        },
        move |addr| {
            let _ = ready_tx.send(addr);
        },
    ));
    let bound = tokio::time::timeout(Duration::from_secs(20), ready_rx).await.expect("server did not start").unwrap();
    let addr: SocketAddr = ([127, 0, 0, 1], bound.port()).into();

    // The fingerprint a pairing code would carry is the one of the certificate on disk.
    let pem = std::fs::read(dir.path().join("tls/cert.pem")).unwrap();
    let fp = fingerprint_of_pem(&pem).unwrap();

    // 1. The pinned server is accepted, and the page comes through.
    let get = "GET /admin/login HTTP/1.1\r\nHost: david\r\nConnection: close\r\n\r\n";
    let page = https(addr, &fp, get).await.expect("the pinned certificate must be accepted");
    assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    assert!(page.contains("Sign in"));

    // 2. Sign-in over TLS gives a cookie marked Secure.
    // The page set the sign-in cookie, and its form carries the matching token.
    let login_cookie = page
        .lines()
        .find_map(|l| l.to_lowercase().starts_with("set-cookie: david_login=").then(|| l["set-cookie: ".len()..].split(';').next().unwrap().to_owned()))
        .expect("the sign-in page sets a cookie");
    assert!(page.to_lowercase().contains("set-cookie: david_login=") && page.to_lowercase().contains("; secure"));
    let marker = "name=\"csrf\" value=\"";
    let from = page.find(marker).unwrap() + marker.len();
    let token = page[from..].split('"').next().unwrap();
    let body = format!("name=Boss&pin=483920&csrf={token}");
    let post = format!(
        "POST /admin/login HTTP/1.1\r\nHost: david\r\nConnection: close\r\nCookie: {login_cookie}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let login = https(addr, &fp, &post).await.unwrap().to_lowercase();
    assert!(login.starts_with("http/1.1 303"), "{login}");
    assert!(login.contains("set-cookie: david_session=") && login.contains("; secure"), "{login}");

    // 3. A server that shows another certificate is refused in the handshake, before any request.
    let wrong = "00".repeat(32);
    let refused = https(addr, &wrong, get).await;
    let message = refused.expect_err("a different fingerprint must be refused");
    assert!(message.contains("handshake"), "{message}");

    // 4. Plain HTTP on the TLS port gets no page.
    let mut plain = TcpStream::connect(addr).await.unwrap();
    plain.write_all(get.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(3), plain.read_to_end(&mut raw)).await;
    assert!(!String::from_utf8_lossy(&raw).contains("Sign in"), "plain HTTP must not be served on the TLS port");

    // 5. The server stops cleanly, and the certificate is the same one after a restart.
    let _ = stop.send(());
    tokio::time::timeout(Duration::from_secs(15), task).await.expect("server did not stop").unwrap().expect("a clean shutdown");
    let again = david_server::tls::ensure_certificate(dir.path(), "0.0.0.0:0".parse().unwrap()).unwrap();
    assert_eq!(again.fingerprint, fp, "paired devices must still recognise the server");
    let _ = std::io::stdout().flush();
}

/// A certificate for `name`, as a certificate authority would hand it out (here made by us).
fn issue(name: &str) -> (String, String, String) {
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = rcgen::CertificateParams::new(vec![name.to_owned()]).unwrap().self_signed(&key).unwrap();
    (cert.pem(), key.serialize_pem(), fingerprint_of_der(cert.der().as_ref()))
}

#[tokio::test]
async fn a_certificate_from_somebody_else_is_served_and_a_renewal_is_loaded_without_a_restart() {
    use david_server::tls::External;
    let dir = tempfile::TempDir::new().unwrap();
    init_admin(dir.path(), "Boss", "483920").await.unwrap();
    let certs = tempfile::TempDir::new().unwrap();
    let (cert_file, key_file) = (certs.path().join("fullchain.pem"), certs.path().join("privkey.pem"));
    let (cert, key, first) = issue("david.example.test");
    std::fs::write(&cert_file, cert).unwrap();
    std::fs::write(&key_file, key).unwrap();

    let config = Config {
        data_dir: dir.path().to_owned(),
        library: None,
        allow_import: false,
        announce: false,
        mcp: false,
        log: None,
        tls: External { cert: Some(cert_file.clone()), key: Some(key_file.clone()), host: Some("david.example.test".into()), reload_s: Some(5) },
        also_listen: Vec::new(),
        listen: "0.0.0.0:0".parse().unwrap(),
    };
    let (ready_tx, ready_rx) = oneshot::channel();
    let (stop, stop_rx) = oneshot::channel::<()>();
    let task = tokio::spawn(run(config, async move { let _ = stop_rx.await; }, move |addr| { let _ = ready_tx.send(addr); }));
    let bound = tokio::time::timeout(Duration::from_secs(20), ready_rx).await.expect("server did not start").unwrap();
    let addr: SocketAddr = ([127, 0, 0, 1], bound.port()).into();
    let get = "GET /admin/login HTTP/1.1\r\nHost: david\r\nConnection: close\r\n\r\n";

    // The server shows the certificate of the files, not one of its own.
    let name = "david.example.test";
    assert!(https_as(addr, &first, name, get).await.expect("the certificate from the files").starts_with("HTTP/1.1 200"));
    assert!(!dir.path().join("tls/cert.pem").exists(), "no self-signed certificate is made");

    // A renewal: new files; within a few seconds the running server shows the new certificate.
    let (cert, key, second) = issue("david.example.test");
    assert_ne!(first, second);
    std::thread::sleep(Duration::from_millis(1100)); // so that the files are newer
    std::fs::write(&key_file, key).unwrap();
    std::fs::write(&cert_file, cert).unwrap();
    let mut loaded = false;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if https_as(addr, &second, name, get).await.is_ok() {
            loaded = true;
            break;
        }
    }
    assert!(loaded, "the renewed certificate was not loaded");
    assert!(https_as(addr, &first, name, get).await.is_err(), "the old certificate is gone");

    let _ = stop.send(());
    let _ = tokio::time::timeout(Duration::from_secs(10), task).await;
}

#[test]
fn files_that_cannot_be_used_are_named_with_a_hint() {
    use david_server::tls::{External, load_external};
    let dir = tempfile::TempDir::new().unwrap();
    let (cert, key, _) = issue("david.example.test");
    let (c, k) = (dir.path().join("c.pem"), dir.path().join("k.pem"));
    std::fs::write(&c, &cert).unwrap();
    // The key is missing.
    let err = load_external(&External { cert: Some(c.clone()), key: Some(k.clone()), ..Default::default() }).err().unwrap().to_string();
    assert!(err.contains("k.pem"), "{err}");
    // Not a key.
    std::fs::write(&k, "hello").unwrap();
    let err = load_external(&External { cert: Some(c.clone()), key: Some(k.clone()), ..Default::default() }).err().unwrap().to_string();
    assert!(err.contains("not a private key"), "{err}");
    // Both there: fine; only one named: refused.
    std::fs::write(&k, key).unwrap();
    assert!(load_external(&External { cert: Some(c.clone()), key: Some(k), ..Default::default() }).is_ok());
    assert!(load_external(&External { cert: Some(c), key: None, ..Default::default() }).is_err());
}

#[tokio::test]
async fn a_list_of_addresses_serves_ipv4_and_ipv6_at_the_same_time() {
    let dir = tempfile::TempDir::new().unwrap();
    init_admin(dir.path(), "Boss", "483920").await.unwrap();
    // A free port that both addresses use.
    let port = std::net::TcpListener::bind("0.0.0.0:0").unwrap().local_addr().unwrap().port();
    let config = Config {
        data_dir: dir.path().to_owned(),
        library: None,
        allow_import: false,
        announce: false,
        mcp: false,
        log: None,
        tls: Default::default(),
        also_listen: vec![format!("[::]:{port}").parse().unwrap()],
        listen: format!("0.0.0.0:{port}").parse().unwrap(),
    };
    let (ready_tx, ready_rx) = oneshot::channel();
    let (stop, stop_rx) = oneshot::channel::<()>();
    let task = tokio::spawn(run(config, async move { let _ = stop_rx.await; }, move |addr| { let _ = ready_tx.send(addr); }));
    tokio::time::timeout(Duration::from_secs(20), ready_rx).await.expect("server did not start").unwrap();
    let fp = fingerprint_of_pem(&std::fs::read(dir.path().join("tls/cert.pem")).unwrap()).unwrap();
    let get = "GET /healthz HTTP/1.1\r\nHost: david\r\nConnection: close\r\n\r\n";

    let v4: SocketAddr = ([127, 0, 0, 1], port).into();
    let page = https(v4, &fp, get).await.expect("IPv4");
    assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    let v6: SocketAddr = (std::net::Ipv6Addr::LOCALHOST, port).into();
    let page = https(v6, &fp, get).await.expect("IPv6");
    assert!(page.starts_with("HTTP/1.1 200"), "{page}");

    let _ = stop.send(());
    let _ = tokio::time::timeout(Duration::from_secs(10), task).await;
}
