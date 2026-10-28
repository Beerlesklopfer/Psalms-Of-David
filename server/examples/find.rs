// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Looks for servers on the network for a few seconds and prints what it finds.
//! `cargo run --example find`
fn main() {
    let found = david_server::discovery::browse(std::time::Duration::from_secs(6)).expect("browse failed");
    if found.is_empty() {
        println!("nothing found");
    }
    for f in found {
        println!("{f:?}");
    }
}
