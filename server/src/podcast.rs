// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Podcast feeds: reading an RSS 2.0 or Atom document into the feed and its episodes (project owner,
//! 2026-10-28: podcasts and radio in the app, synchronised between the devices of an account). Only
//! the reading is here and it touches no network: the document is a string, so it is tested with
//! plain text. Fetching with the protection against reads of the own network is in `fetch`.

use roxmltree::{Document, Node};

/// One episode of a feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Episode {
    /// What identifies the episode in the feed (the `guid`, else the address of the audio).
    pub guid: String,
    pub title: String,
    /// Where the audio is.
    pub audio_url: String,
    /// Unix milliseconds; 0 if the feed gives none.
    pub published_ms: i64,
    pub duration_s: Option<i64>,
    pub description: String,
}

/// A feed with its episodes, newest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feed {
    pub title: String,
    /// The web page of the podcast.
    pub site: Option<String>,
    pub image: Option<String>,
    pub description: String,
    pub episodes: Vec<Episode>,
}

/// Most episodes read from one feed: a feed of ten years is not needed whole.
pub const MAX_EPISODES: usize = 500;

fn text(node: Node<'_, '_>) -> String {
    node.text().unwrap_or_default().trim().to_owned()
}

fn child<'a>(node: Node<'a, 'a>, name: &str) -> Option<Node<'a, 'a>> {
    node.children().find(|c| c.is_element() && c.tag_name().name() == name)
}

/// `1:02:03`, `12:30`, `90` or `90.5` as seconds.
fn duration(value: &str) -> Option<i64> {
    let parts: Vec<&str> = value.trim().split(':').collect();
    let mut seconds = 0.0f64;
    for part in &parts {
        seconds = seconds * 60.0 + part.trim().parse::<f64>().ok()?;
    }
    (parts.len() <= 3 && seconds >= 0.0).then_some(seconds as i64)
}

/// A date of a feed (RFC 2822 in RSS, RFC 3339 in Atom) as Unix milliseconds.
fn date(value: &str) -> i64 {
    let v = value.trim();
    jiff::fmt::rfc2822::parse(v)
        .map(|z| z.timestamp().as_millisecond())
        .or_else(|_| v.parse::<jiff::Timestamp>().map(|t| t.as_millisecond()))
        .unwrap_or(0)
}

fn is_web_address(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://")) && url.len() <= 2000 && !url.chars().any(|c| c.is_control() || c == ' ')
}

/// Reads an RSS 2.0 or Atom document. An error says in a few words why it is none.
pub fn parse(xml: &str) -> Result<Feed, String> {
    let doc = Document::parse_with_options(xml, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() }).map_err(|e| format!("not a feed: {e}"))?;
    let root = doc.root_element();
    match root.tag_name().name() {
        "rss" => {
            let channel = child(root, "channel").ok_or("not a feed: no channel")?;
            let title = child(channel, "title").map(text).unwrap_or_default();
            let site = child(channel, "link").map(text).filter(|l| is_web_address(l));
            let image = child(channel, "image")
                .and_then(|i| child(i, "url").map(text))
                .or_else(|| channel.children().find(|c| c.is_element() && c.tag_name().name() == "image" && c.attribute("href").is_some()).and_then(|c| c.attribute("href").map(str::to_owned)))
                .filter(|u| is_web_address(u));
            let description = child(channel, "description").map(text).unwrap_or_default();
            let mut episodes = Vec::new();
            for item in channel.children().filter(|c| c.is_element() && c.tag_name().name() == "item") {
                let enclosure = child(item, "enclosure");
                let Some(audio_url) = enclosure.and_then(|e| e.attribute("url")).map(str::trim).filter(|u| is_web_address(u)) else { continue };
                let guid = child(item, "guid").map(text).filter(|g| !g.is_empty()).unwrap_or_else(|| audio_url.to_owned());
                episodes.push(Episode {
                    guid,
                    title: child(item, "title").map(text).unwrap_or_default(),
                    audio_url: audio_url.to_owned(),
                    published_ms: child(item, "pubDate").map(|n| date(&text(n))).unwrap_or(0),
                    duration_s: child(item, "duration").and_then(|n| duration(&text(n))),
                    description: child(item, "description").map(text).unwrap_or_default(),
                });
                if episodes.len() >= MAX_EPISODES {
                    break;
                }
            }
            episodes.sort_by_key(|e| std::cmp::Reverse(e.published_ms));
            Ok(Feed { title, site, image, description, episodes })
        }
        "feed" => {
            let title = child(root, "title").map(text).unwrap_or_default();
            let link_of = |n: Node<'_, '_>, rel: &str| {
                n.children()
                    .filter(|c| c.is_element() && c.tag_name().name() == "link")
                    .find(|l| l.attribute("rel").unwrap_or("alternate") == rel)
                    .and_then(|l| l.attribute("href"))
                    .map(str::to_owned)
            };
            let site = link_of(root, "alternate").filter(|l| is_web_address(l));
            let image = child(root, "logo").or_else(|| child(root, "icon")).map(text).filter(|u| is_web_address(u));
            let description = child(root, "subtitle").map(text).unwrap_or_default();
            let mut episodes = Vec::new();
            for entry in root.children().filter(|c| c.is_element() && c.tag_name().name() == "entry") {
                let Some(audio_url) = link_of(entry, "enclosure").filter(|u| is_web_address(u)) else { continue };
                let guid = child(entry, "id").map(text).filter(|g| !g.is_empty()).unwrap_or_else(|| audio_url.clone());
                episodes.push(Episode {
                    guid,
                    title: child(entry, "title").map(text).unwrap_or_default(),
                    audio_url,
                    published_ms: child(entry, "published").or_else(|| child(entry, "updated")).map(|n| date(&text(n))).unwrap_or(0),
                    duration_s: None,
                    description: child(entry, "summary").map(text).unwrap_or_default(),
                });
                if episodes.len() >= MAX_EPISODES {
                    break;
                }
            }
            episodes.sort_by_key(|e| std::cmp::Reverse(e.published_ms));
            Ok(Feed { title, site, image, description, episodes })
        }
        other => Err(format!("not a feed: the document is a <{other}>")),
    }
}

/// Finds the address of a feed that a web page announces (`<link rel="alternate" type="application/rss+xml" href=...>`),
/// relative to the address of the page.
pub fn feed_link_in_page(html: &str, page_url: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut from = 0;
    while let Some(start) = lower[from..].find("<link") {
        let begin = from + start;
        let end = lower[begin..].find('>').map(|e| begin + e)?;
        let tag = &html[begin..end];
        let tag_lower = &lower[begin..end];
        if tag_lower.contains("alternate")
            && (tag_lower.contains("rss") || tag_lower.contains("atom"))
            && let Some(href) = attribute(tag, "href")
        {
            return resolve(page_url, &href);
        }
        from = end;
    }
    None
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let key = format!("{name}=");
    let at = lower.find(&key)? + key.len();
    let rest = &tag[at..];
    let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let inner = &rest[1..];
    inner.find(quote).map(|e| inner[..e].trim().to_owned())
}

/// An address that may be relative, made absolute against the page it was found on.
fn resolve(page_url: &str, href: &str) -> Option<String> {
    if is_web_address(href) {
        return Some(href.to_owned());
    }
    let (scheme, rest) = page_url.split_once("://")?;
    let host_end = rest.find('/').unwrap_or(rest.len());
    let (host, path) = rest.split_at(host_end);
    let joined = if let Some(abs) = href.strip_prefix('/') {
        if abs.starts_with('/') { format!("{scheme}:{href}") } else { format!("{scheme}://{host}/{abs}") }
    } else {
        let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
        format!("{scheme}://{host}{dir}/{href}")
    };
    is_web_address(&joined).then_some(joined)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS: &str = r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
 <channel>
  <title>Die Bibel in einem Jahr</title>
  <link>https://example.org/bibel</link>
  <description>Jeden Tag ein Abschnitt.</description>
  <image><url>https://example.org/logo.jpg</url></image>
  <item><title>Tag 1</title><guid>g-1</guid><pubDate>Mon, 02 Mar 2026 06:00:00 +0100</pubDate>
    <enclosure url="https://cdn.example.org/1.mp3" type="audio/mpeg" length="123"/><itunes:duration>12:30</itunes:duration></item>
  <item><title>Tag 2</title><pubDate>Tue, 03 Mar 2026 06:00:00 +0100</pubDate>
    <enclosure url="https://cdn.example.org/2.mp3" type="audio/mpeg"/><itunes:duration>1:02:03</itunes:duration></item>
  <item><title>Nur Text</title></item>
 </channel>
</rss>"#;

    #[test]
    fn an_rss_feed_gives_its_episodes_newest_first_and_skips_what_has_no_audio() {
        let f = parse(RSS).unwrap();
        assert_eq!((f.title.as_str(), f.site.as_deref(), f.image.as_deref()), ("Die Bibel in einem Jahr", Some("https://example.org/bibel"), Some("https://example.org/logo.jpg")));
        assert_eq!(f.episodes.len(), 2, "the item without audio is not an episode");
        assert_eq!(f.episodes[0].title, "Tag 2");
        assert_eq!(f.episodes[0].guid, "https://cdn.example.org/2.mp3", "no guid: the audio address identifies it");
        assert_eq!((f.episodes[0].duration_s, f.episodes[1].duration_s), (Some(3723), Some(750)));
        assert!(f.episodes[0].published_ms > f.episodes[1].published_ms && f.episodes[1].published_ms > 0);
    }

    #[test]
    fn an_atom_feed_is_read_too() {
        let atom = r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Radio Wissen</title><link href="https://example.org/"/>
          <entry><id>e1</id><title>Folge</title><published>2026-03-01T10:00:00Z</published>
            <link rel="enclosure" href="https://cdn.example.org/a.mp3"/><summary>Kurz</summary></entry></feed>"#;
        let f = parse(atom).unwrap();
        assert_eq!((f.title.as_str(), f.episodes.len(), f.episodes[0].guid.as_str()), ("Radio Wissen", 1, "e1"));
        assert_eq!(f.episodes[0].published_ms, 1_772_359_200_000);
    }

    #[test]
    fn anything_else_is_refused_with_a_reason() {
        assert!(parse("<html><body>hello</body></html>").unwrap_err().contains("<html>"));
        assert!(parse("not xml at all").is_err());
        assert!(parse("<rss><nothing/></rss>").unwrap_err().contains("no channel"));
    }

    #[test]
    fn unsafe_addresses_are_not_taken_over() {
        let xml = r#"<rss><channel><title>T</title><link>javascript:alert(1)</link>
          <item><enclosure url="file:///etc/passwd"/></item><item><enclosure url="https://ok.example/a.mp3"/></item></channel></rss>"#;
        let f = parse(xml).unwrap();
        assert_eq!((f.site, f.episodes.len()), (None, 1), "only http(s) addresses");
    }

    #[test]
    fn a_page_that_announces_a_feed_is_found_with_a_relative_link() {
        let html = r#"<html><head><link rel="stylesheet" href="s.css"><link rel='alternate' type='application/rss+xml' title='RSS' href='/feed.xml'></head></html>"#;
        assert_eq!(feed_link_in_page(html, "https://example.org/podcast/index.html").as_deref(), Some("https://example.org/feed.xml"));
        let html2 = r#"<link rel="alternate" type="application/atom+xml" href="atom.xml">"#;
        assert_eq!(feed_link_in_page(html2, "https://example.org/blog/").as_deref(), Some("https://example.org/blog/atom.xml"));
        assert_eq!(feed_link_in_page("<html></html>", "https://example.org/"), None);
    }
}
