// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! mDNS with the real program: a running `david` is found by a client, with its
//! records, and it disappears when it is stopped. The server runs as its own
//! process, because two mDNS daemons inside one process do not see each other.
//! Needs a network interface with an address other than loopback; where there is
//! none the test says so and stops.

use std::io::Write;
use std::net::{IpAddr, TcpStream};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use david_server::discovery::{browse, should_announce};

/// An address of a real LAN interface of this machine. The route to the outside is
/// no help: with a VPN as the default route it leads to the tunnel, where mDNS does
/// not reach the neighbours. So the interfaces are looked at: not loopback, not a
/// tunnel or a virtual bridge, and not a single-host (/32) address. The variable
/// `DAVID_TEST_LAN_IP` overrides the choice.
fn lan_ip() -> Option<IpAddr> {
    if let Ok(ip) = std::env::var("DAVID_TEST_LAN_IP") {
        return ip.parse().ok();
    }
    const VIRTUAL: [&str; 7] = ["lo", "tun", "tap", "wg", "virbr", "docker", "veth"];
    if_addrs::get_if_addrs().ok()?.into_iter().find_map(|i| {
        let if_addrs::IfAddr::V4(v4) = &i.addr else { return None };
        let single_host = v4.netmask.octets() == [255, 255, 255, 255];
        let virtual_name = VIRTUAL.iter().any(|p| i.name.starts_with(p));
        (!i.is_loopback() && !virtual_name && !single_host).then_some(IpAddr::V4(v4.ip))
    })
}

fn wait_until_listening(ip: IpAddr, port: u16) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if TcpStream::connect_timeout(&(ip, port).into(), Duration::from_millis(200)).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("the server did not start listening on {ip}:{port}");
}

#[test]
fn a_running_server_is_found_with_its_records_and_withdrawn_when_it_stops() {
    let Some(ip) = lan_ip() else {
        eprintln!("skipped: no network interface other than loopback");
        return;
    };
    let port = 18489;
    let dir = tempfile::TempDir::new().unwrap();
    let david = env!("CARGO_BIN_EXE_david");

    let mut init = Command::new(david)
        .args(["init", "--name", "Boss", "--data-dir"])
        .arg(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    init.stdin.take().unwrap().write_all(b"483920\n").unwrap();
    assert!(init.wait().unwrap().success());

    let mut server = Command::new(david)
        .args(["run", "--data-dir"])
        .arg(dir.path())
        .args(["--listen", &format!("{ip}:{port}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_until_listening(ip, port);

    let found = browse(Duration::from_secs(8)).unwrap();
    let ours = found.iter().find(|f| f.port == port && f.addresses.contains(&ip));
    let ours = match ours {
        Some(f) => f.clone(),
        None => {
            let _ = server.kill();
            panic!("not found among {found:?}");
        }
    };
    assert_eq!((ours.name.as_str(), ours.version.as_str(), ours.api.as_str()), ("David", "1", "/v1"));
    assert!(ours.instance.starts_with("David ("), "{}", ours.instance);
    assert_eq!(ours.id.len(), 36, "the server id, a UUID");

    // Nothing about people or the library is announced.
    let everything = format!("{ours:?}").to_lowercase();
    assert!(!everything.contains("boss"), "the user name must not be announced: {everything}");

    // Stopping the server withdraws the service. SIGINT lets it say goodbye (a kill would not).
    let pid = server.id().to_string();
    assert!(Command::new("kill").args(["-INT", &pid]).status().unwrap().success());
    assert!(server.wait().unwrap().success(), "a clean exit");
    std::thread::sleep(Duration::from_secs(1));
    let after = browse(Duration::from_secs(3)).unwrap();
    assert!(!after.iter().any(|f| f.id == ours.id), "the goodbye must remove it: {after:?}");
}

#[test]
fn a_server_that_only_this_machine_can_reach_is_not_announced() {
    assert!(!should_announce("127.0.0.1:8484".parse().unwrap()));
    assert!(!should_announce("[::1]:8484".parse().unwrap()));
    assert!(should_announce("0.0.0.0:8484".parse().unwrap()));
}
