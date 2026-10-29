// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reads a real podcast feed from the internet: the download of the server (public addresses only, redirects checked) and the parser.
//! Ignored by default (it needs the network). `PSALM_TEST_FEED_URL=https://feeds.megaphone.fm/... cargo test --release --test feed_online -- --ignored --nocapture`.

#[test]
#[ignore = "needs the network and PSALM_TEST_FEED_URL"]
fn a_real_feed_is_downloaded_and_read() {
    let url = std::env::var("PSALM_TEST_FEED_URL").expect("set PSALM_TEST_FEED_URL to the address of a podcast feed");
    let body = david_server::fetch::download_text(&url, 8 * 1024 * 1024).expect("the feed is downloaded");
    let feed = david_server::podcast::parse(&body).expect("the feed is read");
    println!("title: {}\nsite: {:?}\nimage: {:?}\nepisodes: {}", feed.title, feed.site, feed.image, feed.episodes.len());
    for e in feed.episodes.iter().take(3) {
        println!("  {:?}", e);
    }
    assert!(!feed.title.is_empty(), "a title");
    assert!(!feed.episodes.is_empty(), "episodes");
    assert!(feed.episodes.iter().all(|e| !e.audio_url.is_empty()), "every episode has an audio address");
}
