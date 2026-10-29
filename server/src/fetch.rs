// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Fetching a picture from the internet for MCP (`set_cover_from_url`).
//!
//! The address comes from a remote caller, so the server must not be made to read its own network:
//! only `https`, only hosts whose every address is public (checked at each connection, so a
//! redirect to an internal address fails too), a size limit, a time limit, and the bytes must be a
//! picture (decided by the bytes, see `profile::sniff_image`).

use std::net::IpAddr;
use std::time::Duration;

use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};

/// Is this an address on the public internet? Loopback, private, link-local, shared (CGNAT),
/// multicast, documentation and unspecified addresses are not.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_documentation()
                || (a == 100 && (64..=127).contains(&b))
                || a == 0
                || a >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00 // unique local
                || (first & 0xffc0) == 0xfe80 // link-local
                || (first == 0x2001 && v6.segments()[1] == 0x0db8)) // documentation
        }
    }
}

/// The system resolver, but it keeps only public addresses and fails if there is none.
#[derive(Debug, Default)]
struct PublicOnly(DefaultResolverBox);

#[derive(Default)]
struct DefaultResolverBox(DefaultResolver);

impl std::fmt::Debug for DefaultResolverBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DefaultResolver")
    }
}

impl Resolver for PublicOnly {
    fn resolve(&self, uri: &ureq::http::Uri, config: &ureq::config::Config, timeout: ureq::unversioned::transport::NextTimeout) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let all = self.0.0.resolve(uri, config, timeout)?;
        let mut out = self.empty();
        for addr in all.iter().filter(|a| is_public(a.ip())) {
            out.push(*addr);
        }
        if out.is_empty() {
            return Err(ureq::Error::HostNotFound);
        }
        // One non-public address among public ones is a sign of a trick, not of a normal host.
        if out.len() != all.len() {
            return Err(ureq::Error::HostNotFound);
        }
        Ok(out)
    }
}

/// Downloads a picture: its type (from the bytes) and the bytes. Blocking; call it from a blocking task.
pub fn download_image(url: &str, max_bytes: usize) -> Result<(&'static str, Vec<u8>), String> {
    if !url.starts_with("https://") || url.len() > 2000 || url.chars().any(|c| c.is_control() || c == ' ') {
        return Err("the address must be an https:// URL".into());
    }
    let config = ureq::Agent::config_builder()
        .https_only(true)
        .max_redirects(3)
        .timeout_global(Some(Duration::from_secs(20)))
        .user_agent("David-server (cover fetch)")
        .build();
    let agent = ureq::Agent::with_parts(config, ureq::unversioned::transport::DefaultConnector::default(), PublicOnly::default());
    let mut response = agent.get(url).call().map_err(|e| match e {
        ureq::Error::HostNotFound => "the host does not exist or is not on the public internet".to_owned(),
        other => format!("download failed: {other}"),
    })?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(max_bytes as u64 + 1)
        .read_to_vec()
        .map_err(|e| format!("download failed: {e}"))?;
    if bytes.len() > max_bytes {
        return Err(format!("the picture is larger than {} MiB", max_bytes / 1024 / 1024));
    }
    let mime = crate::profile::sniff_image(&bytes).ok_or("the address does not deliver a picture (JPEG, PNG or WebP)")?;
    Ok((mime, bytes))
}

/// Downloads a podcast feed (or the page that points to one) as text, at most `max_bytes`. Blocking; call it from a
/// blocking task. Like `download_image` it reads from the public internet only, but a feed may be `http://` (many
/// are), so both schemes are taken.
pub fn download_text(url: &str, max_bytes: usize) -> Result<String, String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) || url.len() > 2000 || url.chars().any(|c| c.is_control() || c == ' ') {
        return Err("the address must be an http:// or https:// URL".into());
    }
    let config = ureq::Agent::config_builder()
        .max_redirects(5)
        .timeout_global(Some(Duration::from_secs(30)))
        .user_agent("David-server (feed fetch)")
        .build();
    let agent = ureq::Agent::with_parts(config, ureq::unversioned::transport::DefaultConnector::default(), PublicOnly::default());
    let mut response = agent.get(url).call().map_err(|e| match e {
        ureq::Error::HostNotFound => "the host does not exist or is not on the public internet".to_owned(),
        other => format!("download failed: {other}"),
    })?;
    let bytes = response.body_mut().with_config().limit(max_bytes as u64 + 1).read_to_vec().map_err(|e| format!("download failed: {e}"))?;
    if bytes.len() > max_bytes {
        return Err(format!("the feed is larger than {} MiB", max_bytes / 1024 / 1024));
    }
    String::from_utf8(bytes).or_else(|e| Ok::<String, String>(String::from_utf8_lossy(e.as_bytes()).into_owned()))
}

/// Fetches an address with a User-Agent of the caller's choosing (MusicBrainz demands one with a contact address):
/// the bytes, or `None` for a plain 404. `http://` and `https://`, public addresses only, redirects followed (the
/// Cover Art Archive answers with a redirect to archive.org), at most `max_bytes`. Blocking; call it from a blocking task.
pub fn get_bytes(url: &str, max_bytes: usize, user_agent: &str) -> Result<Option<Vec<u8>>, String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) || url.len() > 2000 || url.chars().any(|c| c.is_control() || c == ' ') {
        return Err("the address must be an http:// or https:// URL".into());
    }
    let config = ureq::Agent::config_builder()
        .max_redirects(5)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(30)))
        .user_agent(user_agent)
        .build();
    let agent = ureq::Agent::with_parts(config, ureq::unversioned::transport::DefaultConnector::default(), PublicOnly::default());
    let mut response = agent.get(url).call().map_err(|e| match e {
        ureq::Error::HostNotFound => "the host does not exist or is not on the public internet".to_owned(),
        other => format!("request failed: {other}"),
    })?;
    let status = response.status().as_u16();
    if status == 404 {
        return Ok(None);
    }
    if !(200..300).contains(&status) {
        return Err(format!("the service answered {status}"));
    }
    let bytes = response.body_mut().with_config().limit(max_bytes as u64 + 1).read_to_vec().map_err(|e| format!("download failed: {e}"))?;
    if bytes.len() > max_bytes {
        return Err("the answer is too large".into());
    }
    Ok(Some(bytes))
}

/// Only for the tests below: the address of an IPv4 as a plain value.
#[cfg(test)]
fn v4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
    IpAddr::V4(std::net::Ipv4Addr::new(a, b, c, d))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_addresses_of_the_public_internet_count() {
        for ip in [v4(8, 8, 8, 8), v4(93, 184, 216, 34), "2606:4700:4700::1111".parse().unwrap()] {
            assert!(is_public(ip), "{ip}");
        }
        for ip in [
            v4(127, 0, 0, 1),
            v4(10, 1, 2, 3),
            v4(172, 16, 0, 1),
            v4(192, 168, 24, 250),
            v4(169, 254, 169, 254),
            v4(100, 64, 0, 1),
            v4(0, 0, 0, 0),
            v4(224, 0, 0, 1),
            v4(255, 255, 255, 255),
            "::1".parse().unwrap(),
            "fe80::1".parse().unwrap(),
            "fd00::1".parse().unwrap(),
            "::ffff:192.168.1.1".parse().unwrap(),
        ] {
            assert!(!is_public(ip), "{ip}");
        }
    }

    #[test]
    fn a_bad_address_is_refused_before_anything_is_fetched() {
        for url in ["http://example.com/a.jpg", "ftp://example.com/a.jpg", "file:///etc/passwd", "https://exa mple.com/", ""] {
            assert!(download_image(url, 1024).is_err(), "{url}");
        }
        // An internal host is refused by the resolver, not by luck of a closed port.
        let err = download_image("https://127.0.0.1:1/cover.jpg", 1024).unwrap_err();
        assert!(err.contains("public internet"), "{err}");
        let err = download_image("https://localhost/cover.jpg", 1024).unwrap_err();
        assert!(err.contains("public internet"), "{err}");
    }

    /// Needs the internet; run with `cargo test -- --ignored`.
    #[test]
    #[ignore = "needs the internet"]
    fn a_real_picture_is_downloaded_and_a_page_is_not_taken_for_one() {
        let (mime, bytes) = download_image("https://upload.wikimedia.org/wikipedia/commons/4/47/PNG_transparency_demonstration_1.png", 8 * 1024 * 1024).unwrap();
        assert_eq!(mime, "image/png");
        assert!(bytes.len() > 1000);
        let err = download_image("https://example.com/", 1024 * 1024).unwrap_err();
        assert!(err.contains("picture"), "{err}");
        let err = download_image("https://upload.wikimedia.org/wikipedia/commons/4/47/PNG_transparency_demonstration_1.png", 1000).unwrap_err();
        assert!(err.contains("larger"), "{err}");
    }
}
