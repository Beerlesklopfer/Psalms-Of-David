// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The pages of the blacklist: the words (a list with search and pages, create, edit, delete, with a preview of what a word would hit) and the marked
//! media (with the word and the field that hit, approve and mark again). See `docs/spec-admin-ui.md` ("blacklist").

use std::collections::HashMap;

use super::*;
use crate::blacklist::{Kind, PreviewHit};

/// How many hits the preview of a word lists; the rest is counted.
const PREVIEW_LIST: usize = 10;

/// The title, owner and link of the media a page lists, looked up by (kind, id).
struct Names {
    books: HashMap<i64, (String, Option<String>)>,
    feeds: HashMap<i64, String>,
    stations: HashMap<i64, String>,
}

impl Names {
    async fn load(store: &Store) -> Result<Names> {
        let books = store.admin_book_rows().await?.into_iter().map(|b| (b.id, (if b.author.trim().is_empty() { b.title } else { format!("{} \u{2013} {}", b.author, b.title) }, b.owner))).collect();
        let feeds = store.feeds_with_followers().await?.into_iter().map(|(f, _)| (f.id, f.title)).collect();
        let stations = store.stations_with_owners().await?.into_iter().map(|(s, _)| (s.id, s.name)).collect();
        Ok(Names { books, feeds, stations })
    }

    /// `(title, owner, link)`; a medium that is gone has no title.
    fn of(&self, kind: &str, id: i64) -> (String, String, Option<String>) {
        match kind {
            "book" => self.books.get(&id).map_or_else(|| (String::new(), String::new(), None), |(t, o)| (t.clone(), o.clone().unwrap_or_default(), None)),
            "podcast" => (self.feeds.get(&id).cloned().unwrap_or_default(), String::new(), Some(format!("/admin/podcasts/{id}"))),
            _ => (self.stations.get(&id).cloned().unwrap_or_default(), String::new(), None),
        }
    }
}

impl AdminUi {
    /// The words of the list with their kind, note, number of hits and last change, cut into pages after the search. `report` is what a change just
    /// did to the marks (shown above the list).
    pub async fn blacklist<'a>(&self, store: &Store, view: impl Into<View<'a>>, query: Option<&str>, page: Option<usize>, per: Option<usize>, report: Option<crate::blacklist::BlacklistReport>) -> Result<String> {
        let needle = query.map(str::trim).filter(|q| !q.is_empty());
        let folded = needle.map(crate::tree::fold);
        let zone = store.setting("time_zone").await?;
        let words: Vec<_> = store.blacklist_entries().await?.into_iter().filter(|w| folded.as_ref().is_none_or(|n| crate::tree::fold(&w.pattern).contains(n.as_str()) || crate::tree::fold(&w.note).contains(n.as_str()))).collect();
        let paging = Paging::of(store, words.len(), page, per).await?;
        let counts = store.blacklist_word_counts().await?;
        let items: Vec<serde_json::Value> = words
            .into_iter()
            .skip(paging.from)
            .take(paging.per)
            .map(|w| {
                serde_json::json!({
                    "id": w.id, "pattern": w.pattern, "note": w.note, "kind": w.kind, "hits": counts.get(&w.pattern).copied().unwrap_or(0),
                    "updated": day(w.updated_at, zone.as_deref()), "updated_by": w.updated_by,
                })
            })
            .collect();
        let pager = paging.json("/admin/blacklist", needle);
        // Where the forms lead back to: this list, in this page and with this search.
        let back = match pager["base"].as_str().filter(|b| !b.is_empty()) {
            Some(base) => format!("/admin/blacklist?{base}&page={}", paging.page),
            None => format!("/admin/blacklist?page={}", paging.page),
        };
        let marked_total = store.blacklist_rows().await?.iter().filter(|r| !r.approved).count();
        self.render(
            "blacklist.html",
            context! { words => Serde(items), pager => Serde(pager), q => needle.unwrap_or_default(), searched => needle.is_some(), back => back, report => report.map(Serde), marked_total => marked_total, ..view.into().context() },
        )
    }

    /// The marked media with the word and the field that hit, cut into pages after the search.
    pub async fn blacklist_hits<'a>(&self, store: &Store, view: impl Into<View<'a>>, query: Option<&str>, page: Option<usize>, per: Option<usize>) -> Result<String> {
        let needle = query.map(str::trim).filter(|q| !q.is_empty());
        let folded = needle.map(crate::tree::fold);
        let names = Names::load(store).await?;
        let mut rows: Vec<serde_json::Value> = Vec::new();
        for r in store.blacklist_rows().await? {
            let (title, owner, href) = names.of(&r.kind, r.id);
            if title.is_empty() {
                continue;
            }
            if let Some(n) = &folded {
                let hay = format!("{} {} {}", crate::tree::fold(&title), crate::tree::fold(&r.word), crate::tree::fold(&owner));
                if !hay.contains(n.as_str()) {
                    continue;
                }
            }
            rows.push(serde_json::json!({ "kind": r.kind, "id": r.id, "title": title, "owner": owner, "href": href, "word": r.word, "field": r.field, "approved": r.approved }));
        }
        rows.sort_by(|a, b| a["title"].as_str().map(crate::tree::fold).cmp(&b["title"].as_str().map(crate::tree::fold)));
        let paging = Paging::of(store, rows.len(), page, per).await?;
        let items: Vec<serde_json::Value> = rows.into_iter().skip(paging.from).take(paging.per).collect();
        let pager = paging.json("/admin/blacklist/hits", needle);
        let back = format!("/admin/blacklist/hits?page={}", paging.page);
        self.render("blacklist_hits.html", context! { rows => Serde(items), pager => Serde(pager), q => needle.unwrap_or_default(), searched => needle.is_some(), back => back, ..view.into().context() })
    }

    /// The preview in the dialog of a word: how many media it would hit and the first ones (title, owner, field). Nothing is written.
    pub async fn blacklist_preview<'a>(&self, store: &Store, view: impl Into<View<'a>>, pattern: &str) -> Result<String> {
        let pattern = pattern.trim();
        if pattern.is_empty() {
            return self.render("fragments/blacklist_preview.html", context! { state => "empty", total => 0, hits => Vec::<String>::new(), ..view.into().context() });
        }
        let hits: Vec<PreviewHit> = match store.blacklist_preview_word(pattern).await {
            Ok(h) => h,
            Err(Error::InvalidName) => return self.render("fragments/blacklist_preview.html", context! { state => "bad", total => 0, hits => Vec::<String>::new(), ..view.into().context() }),
            Err(e) => return Err(e),
        };
        let names = Names::load(store).await?;
        let total = hits.len();
        let shown: Vec<serde_json::Value> = hits
            .iter()
            .take(PREVIEW_LIST)
            .map(|h| {
                let (title, owner, _) = names.of(&h.kind, h.id);
                serde_json::json!({ "kind": h.kind, "title": title, "owner": owner, "field": h.field })
            })
            .collect();
        self.render("fragments/blacklist_preview.html", context! { state => "ok", total => total, hits => Serde(shown), ..view.into().context() })
    }
}

/// The kinds of media a mark can be on, from the text a form sends.
pub fn kind_from(text: &str) -> Option<Kind> {
    match text {
        "book" => Some(Kind::Book),
        "podcast" => Some(Kind::Podcast),
        "station" => Some(Kind::Station),
        _ => None,
    }
}
