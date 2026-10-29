// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! A throwaway server for trying the apps: a temporary data folder with one
//! administrator, one member (Anna), one released book that plays the given audio
//! file, an anchor for John 3:16, and a pairing address written to a file.
//!
//!   cargo run --example serve_demo -- <listen addr> <audio file> <pairing file>
//!
//! Stops with Ctrl-C and removes everything it made. Anything but loopback is
//! served with TLS and announced over mDNS, like the real server.
use std::net::SocketAddr;
use std::path::PathBuf;

use david_server::app::{Config, run};
use david_server::pairing::pairing_uri;
use david_server::tls::ensure_certificate;
use david_server::{Anchor, FoundBook, FoundTrack, NewProfile, Role, Store};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let listen: SocketAddr = args.next().ok_or("listen address")?.parse()?;
    let audio = PathBuf::from(args.next().ok_or("audio file")?);
    let pairing_file = PathBuf::from(args.next().ok_or("pairing file")?);

    let dir = tempfile::tempdir()?;
    let data_dir = dir.path().join("data");
    let library = dir.path().join("library");
    std::fs::create_dir_all(library.join("Test/Bible"))?;
    std::fs::copy(&audio, library.join("Test/Bible/01.mp3"))?;

    david_server::app::create_data_dir(&data_dir)?;
    let tls = if david_server::tls::wanted(listen) { Some(ensure_certificate(&data_dir, listen)?) } else { None };
    let store = Store::open(&data_dir.join("david.db")).await?;
    let admin = store.create_profile("Boss", true).await?;
    let anna = store.create_user(NewProfile::new("Anna", Role::Member)).await?;
    store
        .import_books(vec![FoundBook {
            kind: "audiobook".into(),
            narrator: None,
            genre: None,
            author: "Test".into(),
            title: "Bible".into(),
            folder: "Test/Bible".into(),
            recommended_age: None,
            tracks: vec![FoundTrack {
                disc: 1,
                number: 1,
                title: "Gospel of John".into(),
                file: "01.mp3".into(),
                duration_ms: 30_000,
                size_bytes: std::fs::metadata(&audio)?.len() as i64,
                mtime: 0,
            }],
        }])
        .await?;
    let book = store.find_book("Test/Bible").await?.ok_or("book")?;
    store.release_book(book.id, 0).await?;
    let track = store.tracks_of(book.id).await?[0].id;
    store
        .set_bible_anchors(
            book.id,
            &[
                Anchor { track_id: track, bible_book: 43, chapter: 3, verse: 1, position_ms: 2_000 },
                Anchor { track_id: track, bible_book: 43, chapter: 3, verse: 16, position_ms: 12_000 },
            ],
        )
        .await?;
    let code = store.create_enrollment(anna, false, admin).await?.code;
    let address = std::env::var("DEMO_ADDRESS").unwrap_or_else(|_| listen.to_string());
    let uri = pairing_uri(&store.server_id().await?, &address, &code, tls.as_ref().map(|t| t.fingerprint.as_str()), false);
    std::fs::write(&pairing_file, &uri)?;
    println!("pairing address: {uri}");
    drop(store);

    let config = Config { data_dir, library: Some(library), allow_import: false, listen, announce: true, mcp: false, log: None, tls: Default::default(), also_listen: Vec::new() };
    run(config, async { let _ = tokio::signal::ctrl_c().await; }, |addr| println!("listening on {addr}")).await?;
    Ok(())
}
