// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later

mod common;

use common::open;
use david_server::podcast;

const XML: &str = r#"<?xml version="1.0"?><rss version="2.0"><channel><title>Kinderfunk</title><link>https://example.org/</link><description>Geschichten</description>
  <item><title>Folge 2</title><guid>b</guid><enclosure url="https://example.org/2.mp3" type="audio/mpeg"/><pubDate>Tue, 20 Oct 2026 10:00:00 GMT</pubDate></item>
  <item><title>Folge 1</title><guid>a</guid><enclosure url="https://example.org/1.mp3" type="audio/mpeg"/><pubDate>Tue, 13 Oct 2026 10:00:00 GMT</pubDate></item></channel></rss>"#;

#[tokio::test]
async fn a_feed_is_followed_per_profile_a_childs_request_waits_and_the_state_is_the_accounts() {
    let (_d, store) = open().await;
    let parent = store.create_profile("Parent", true).await.unwrap();
    let child = store.create_profile("Child", false).await.unwrap();
    let feed = podcast::parse(XML).unwrap();
    let id = store.save_feed("https://example.org/feed.xml", &feed).await.unwrap();
    // Reading it again changes nothing but the content: the feed and its episodes stay the same rows.
    assert_eq!(store.save_feed("https://example.org/feed.xml", &feed).await.unwrap(), id);

    store.subscribe(parent, id, true).await.unwrap();
    store.subscribe(child, id, false).await.unwrap();
    assert!(store.follows(parent, id).await.unwrap() && !store.follows(child, id).await.unwrap());
    // The child sees its request marked, and the pages see what waits; the feed is read for those with a release.
    let mine = store.subscriptions(child, false).await.unwrap();
    assert_eq!((mine.len(), mine[0].approved), (1, false));
    assert!(store.subscriptions(child, true).await.unwrap().is_empty());
    let pending = store.pending_releases().await.unwrap();
    assert_eq!((pending.len(), pending[0].kind, pending[0].profile.as_str(), pending[0].title.as_str()), (1, "podcast", "Child", "Kinderfunk"));
    assert_eq!(store.feeds_to_refresh().await.unwrap().len(), 1);

    // Asking twice does not release it; the release does.
    store.subscribe(child, id, true).await.unwrap();
    assert!(!store.follows(child, id).await.unwrap());
    store.decide_release("podcast", child, id, true).await.unwrap();
    assert!(store.follows(child, id).await.unwrap() && store.pending_releases().await.unwrap().is_empty());

    // The episodes, newest first, with the state of the profile that asks; what one device saves, the others read.
    let episodes = store.episodes(parent, id, 50).await.unwrap();
    assert_eq!(episodes.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), ["Folge 2", "Folge 1"]);
    assert_eq!(store.subscriptions(parent, true).await.unwrap()[0].unheard, 2);
    store.put_episode_state(parent, episodes[0].id, 90_000, false).await.unwrap();
    store.put_episode_state(parent, episodes[1].id, 0, true).await.unwrap();
    let again = store.episodes(parent, id, 50).await.unwrap();
    assert_eq!((again[0].position_ms, again[0].finished, again[1].finished), (90_000, false, true));
    assert_eq!(store.episodes(child, id, 50).await.unwrap()[0].position_ms, 0, "the state is of the profile");
    assert_eq!(store.subscriptions(parent, true).await.unwrap()[0].unheard, 1);
    assert!(store.put_episode_state(parent, 9999, 1, false).await.is_err());

    // A refused request is gone; unfollowing too.
    let other = store.create_profile("Other", false).await.unwrap();
    store.subscribe(other, id, false).await.unwrap();
    store.decide_release("podcast", other, id, false).await.unwrap();
    assert!(store.subscriptions(other, false).await.unwrap().is_empty());
    store.unsubscribe(parent, id).await.unwrap();
    assert!(!store.follows(parent, id).await.unwrap());
}

#[tokio::test]
async fn stations_are_favourites_of_a_profile_with_the_same_release_as_podcasts() {
    let (_d, store) = open().await;
    let parent = store.create_profile("Parent", true).await.unwrap();
    let child = store.create_profile("Child", false).await.unwrap();
    let a = store.save_station("https://stream.example.org/a", "Radio A", Some("https://a.example.org/"), None).await.unwrap();
    let b = store.save_station("https://stream.example.org/b", "Radio B", None, Some("not an address")).await.unwrap();
    assert_eq!(store.save_station("https://stream.example.org/a", "Radio A neu", None, None).await.unwrap(), a);
    assert!(store.save_station("ftp://x/y", "Bad", None, None).await.is_err() && store.save_station("https://x.example/y", " ", None, None).await.is_err());

    store.favorite_station(parent, b, true).await.unwrap();
    store.favorite_station(parent, a, true).await.unwrap();
    let list = store.favorite_stations(parent, true).await.unwrap();
    assert_eq!(list.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Radio B", "Radio A neu"], "in the order they were added");
    assert_eq!((list[0].logo_url.clone(), list[1].homepage.clone()), (None, Some("https://a.example.org/".to_owned())));

    store.favorite_station(child, a, false).await.unwrap();
    assert!(store.favorite_stations(child, true).await.unwrap().is_empty());
    assert_eq!(store.favorite_stations(child, false).await.unwrap().len(), 1);
    assert_eq!(store.pending_releases().await.unwrap()[0].kind, "station");
    store.decide_release("station", child, a, true).await.unwrap();
    assert_eq!(store.favorite_stations(child, true).await.unwrap().len(), 1);
    store.unfavorite_station(child, a).await.unwrap();
    assert!(store.favorite_stations(child, false).await.unwrap().is_empty());
}
