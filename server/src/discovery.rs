// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Finding the server on the local network with mDNS / DNS-SD.
//!
//! The server announces `_david._tcp` with a few text records. Nothing about
//! people or the library is announced: only what a device needs to recognise the
//! server it was paired with (spec `spec-discovery-profiles-roles.md`, section 1).
//! mDNS is not authenticated and never crosses routers or the tunnel, so what is
//! found here is a candidate; trust comes from the identity pinned at pairing.

use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

use crate::error::{Error, Result};

pub const SERVICE_TYPE: &str = "_david._tcp.local.";
/// Version of the announcement and of the pairing address.
pub const PROTOCOL_VERSION: &str = "1";
pub const API_PATH: &str = "/v1";

fn mdns_error(e: mdns_sd::Error) -> Error {
    Error::WriteFailed(format!("mDNS: {e}"))
}

/// Is it worth announcing a server that listens on `addr`? Not if only this machine
/// can reach it: the announcement would send devices to an address they cannot use.
pub fn should_announce(addr: SocketAddr) -> bool {
    !addr.ip().is_loopback()
}

/// A running announcement. Dropping it withdraws the service (a goodbye is sent),
/// so a stopped server disappears from the network at once.
pub struct Announcement {
    daemon: ServiceDaemon,
    fullname: String,
}

impl Announcement {
    /// Announces the server. `addr` is where it listens; an unspecified address
    /// (`0.0.0.0`) announces every address of this machine.
    pub fn start(server_id: &str, name: &str, addr: SocketAddr) -> Result<Announcement> {
        let short = server_id.chars().take(8).collect::<String>();
        // The short id keeps two servers called "David" on one network apart.
        let instance = format!("{name} ({short})");
        let host = format!("david-{short}.local.");
        let properties = [
            ("v", PROTOCOL_VERSION),
            ("id", server_id),
            ("name", name),
            ("api", API_PATH),
        ];
        let ip = if addr.ip().is_unspecified() { String::new() } else { addr.ip().to_string() };
        let mut info = ServiceInfo::new(SERVICE_TYPE, &instance, &host, ip, addr.port(), &properties[..]).map_err(mdns_error)?;
        if addr.ip().is_unspecified() {
            info = info.enable_addr_auto();
        }
        let fullname = info.get_fullname().to_owned();
        let daemon = ServiceDaemon::new().map_err(mdns_error)?;
        daemon.register(info).map_err(mdns_error)?;
        Ok(Announcement { daemon, fullname })
    }

    pub fn fullname(&self) -> &str {
        &self.fullname
    }
}

impl Drop for Announcement {
    fn drop(&mut self) {
        // Send the goodbye and give it a moment to leave before the daemon stops.
        if let Ok(done) = self.daemon.unregister(&self.fullname) {
            let _ = done.recv_timeout(Duration::from_secs(1));
        }
        let _ = self.daemon.shutdown();
    }
}

/// A server found on the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundServer {
    pub instance: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub api: String,
    pub addresses: Vec<IpAddr>,
    pub port: u16,
}

/// Looks for servers for `wait` and returns every one resolved in that time. This is
/// what a client does; the app will have its own implementation of the same thing.
pub fn browse(wait: Duration) -> Result<Vec<FoundServer>> {
    let daemon = ServiceDaemon::new().map_err(mdns_error)?;
    let events = daemon.browse(SERVICE_TYPE).map_err(mdns_error)?;
    let deadline = Instant::now() + wait;
    let mut found: Vec<FoundServer> = Vec::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match events.recv_timeout(left) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                let text = |key: &str| info.get_properties().get_property_val_str(key).unwrap_or("").to_owned();
                let server = FoundServer {
                    instance: info.get_fullname().to_owned(),
                    id: text("id"),
                    name: text("name"),
                    version: text("v"),
                    api: text("api"),
                    addresses: info.get_addresses().iter().map(|a| a.to_ip_addr()).collect(),
                    port: info.get_port(),
                };
                if !found.iter().any(|f| f.instance == server.instance) {
                    found.push(server);
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    let _ = daemon.shutdown();
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_server_that_others_can_reach_is_announced() {
        assert!(!should_announce("127.0.0.1:8484".parse().unwrap()));
        assert!(!should_announce("[::1]:8484".parse().unwrap()));
        assert!(should_announce("192.168.24.250:8484".parse().unwrap()));
        assert!(should_announce("0.0.0.0:8484".parse().unwrap()));
    }
}
