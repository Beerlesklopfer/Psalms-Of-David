// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Podcasts and radio in the store (project owner, 2026-10-28). Feeds and stations are shared by all; what
//! belongs to an account (subscriptions, favourites, what was heard) is per profile, so every device of the
//! account reads and writes the same. What a child adds waits (`approved = 0`) until an administrator or a
//! parent releases it; a parent can also give content to a child directly.

use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait, Value};
use serde::Serialize;

use crate::error::{Error, Result};
use crate::podcast::{self, Feed};
use crate::store::{now_ms, Store};

/// A feed with what the account knows about it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FeedRow {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub site_url: Option<String>,
    pub image_url: Option<String>,
    pub description: String,
    pub last_error: Option<String>,
    /// False while a request of a child waits for a release.
    pub approved: bool,
    /// Episodes not yet heard to the end.
    pub unheard: i64,
}

/// A feed without the state of a profile: what the administration and the tools of the server list.
#[derive(Debug, Clone)]
pub struct FeedHead {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub last_error: Option<String>,
}

/// One episode with the state of the asking profile.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EpisodeRow {
    pub id: i64,
    pub title: String,
    pub audio_url: String,
    pub published_at: i64,
    pub duration_s: Option<i64>,
    pub description: String,
    pub position_ms: i64,
    pub finished: bool,
}

/// A radio station with its place in the favourites of the profile.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StationRow {
    pub id: i64,
    pub stream_url: String,
    pub name: String,
    pub homepage: Option<String>,
    pub logo_url: Option<String>,
    pub approved: bool,
}

/// What waits for a release: who asked for what.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PendingRow {
    /// `podcast` or `station`.
    pub kind: &'static str,
    pub profile_id: i64,
    pub profile: String,
    pub target_id: i64,
    pub title: String,
}

pub(crate) fn is_web_address(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://")) && url.len() <= 2000 && !url.chars().any(|c| c.is_control() || c == ' ')
}

impl Store {
    /// Stores a feed read from its address, with its episodes (new ones are added, known ones updated), and returns
    /// its id. The address is what identifies the feed.
    pub async fn save_feed(&self, url: &str, feed: &Feed) -> Result<i64> {
        if !is_web_address(url) {
            return Err(Error::WriteFailed("a feed is an http or https address".into()));
        }
        let txn = self.db.writer.begin().await?;
        let run = |sql: &'static str, values: Vec<Value>| Statement::from_sql_and_values(DbBackend::Sqlite, sql, values);
        txn.execute_raw(run(
            "INSERT INTO feed (url, title, site_url, image_url, description, last_fetched_at, last_error) VALUES (?, ?, ?, ?, ?, ?, NULL) \
             ON CONFLICT (url) DO UPDATE SET title = excluded.title, site_url = excluded.site_url, image_url = excluded.image_url, \
             description = excluded.description, last_fetched_at = excluded.last_fetched_at, last_error = NULL",
            vec![url.to_owned().into(), feed.title.clone().into(), feed.site.clone().into(), feed.image.clone().into(), feed.description.clone().into(), now_ms().into()],
        ))
        .await?;
        let id: i64 = txn
            .query_one_raw(run("SELECT id FROM feed WHERE url = ?", vec![url.to_owned().into()]))
            .await?
            .ok_or_else(|| Error::WriteFailed("feed not saved".into()))?
            .try_get_by_index(0)?;
        for e in feed.episodes.iter().take(podcast::MAX_EPISODES) {
            txn.execute_raw(run(
                "INSERT INTO episode (feed_id, guid, title, audio_url, published_at, duration_s, description) VALUES (?, ?, ?, ?, ?, ?, ?) \
                 ON CONFLICT (feed_id, guid) DO UPDATE SET title = excluded.title, audio_url = excluded.audio_url, \
                 published_at = excluded.published_at, duration_s = excluded.duration_s, description = excluded.description",
                vec![id.into(), e.guid.clone().into(), e.title.clone().into(), e.audio_url.clone().into(), e.published_ms.into(), e.duration_s.into(), e.description.clone().into()],
            ))
            .await?;
        }
        txn.commit().await?;
        Ok(id)
    }

    /// Remembers that reading a feed failed, and why.
    pub async fn feed_failed(&self, feed_id: i64, why: &str) -> Result<()> {
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE feed SET last_fetched_at = ?, last_error = ? WHERE id = ?", [now_ms().into(), why.to_owned().into(), feed_id.into()]))
            .await?;
        Ok(())
    }

    /// The address of a feed.
    pub async fn feed_url(&self, feed_id: i64) -> Result<Option<String>> {
        let row = self.db.reader.query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT url FROM feed WHERE id = ?", [feed_id.into()])).await?;
        Ok(row.map(|r| r.try_get_by_index(0)).transpose()?)
    }

    /// Every feed somebody follows with a release (the ones to read again), id and address.
    pub async fn feeds_to_refresh(&self) -> Result<Vec<(i64, String)>> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_string(DbBackend::Sqlite, "SELECT DISTINCT f.id, f.url FROM feed f JOIN subscription s ON s.feed_id = f.id WHERE s.approved = 1 ORDER BY f.id"))
            .await?;
        rows.into_iter().map(|r| Ok((r.try_get_by_index(0)?, r.try_get_by_index(1)?))).collect()
    }

    /// Every feed with the profiles that follow it (a request of a child that waits is marked), by title: `(feed, [(profile name, approved)])`.
    pub async fn feeds_with_followers(&self) -> Result<Vec<(FeedHead, Vec<(String, bool)>)>> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT f.id, f.url, f.title, f.last_error, p.name, s.approved FROM feed f \
                 LEFT JOIN subscription s ON s.feed_id = f.id LEFT JOIN profile p ON p.id = s.profile_id \
                 ORDER BY f.title COLLATE NOCASE, f.id, p.name COLLATE NOCASE",
            ))
            .await?;
        let mut out: Vec<(FeedHead, Vec<(String, bool)>)> = Vec::new();
        for r in rows {
            let head = FeedHead { id: r.try_get_by_index(0)?, url: r.try_get_by_index(1)?, title: r.try_get_by_index(2)?, last_error: r.try_get_by_index(3)? };
            let name: Option<String> = r.try_get_by_index(4)?;
            let approved: Option<bool> = r.try_get_by_index(5)?;
            if out.last().is_none_or(|(h, _)| h.id != head.id) {
                out.push((head, Vec::new()));
            }
            if let (Some(name), Some(approved)) = (name, approved) {
                out.last_mut().expect("just pushed").1.push((name, approved));
            }
        }
        Ok(out)
    }

    /// Follows a feed. `approved` is false for a child: it then waits for a release. Following again changes nothing
    /// (a release is never taken back by asking twice).
    pub async fn subscribe(&self, profile_id: i64, feed_id: i64, approved: bool) -> Result<()> {
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO subscription (profile_id, feed_id, added_at, approved) VALUES (?, ?, ?, ?) ON CONFLICT (profile_id, feed_id) DO NOTHING",
                [profile_id.into(), feed_id.into(), now_ms().into(), i64::from(approved).into()],
            ))
            .await
            .map_err(|_| Error::WriteFailed("no such feed or profile".into()))?;
        Ok(())
    }

    pub async fn unsubscribe(&self, profile_id: i64, feed_id: i64) -> Result<()> {
        self.db.writer.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "DELETE FROM subscription WHERE profile_id = ? AND feed_id = ?", [profile_id.into(), feed_id.into()])).await?;
        Ok(())
    }

    /// The feeds of a profile, with the waiting ones marked (`only_approved` leaves them out), by title.
    pub async fn subscriptions(&self, profile_id: i64, only_approved: bool) -> Result<Vec<FeedRow>> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT f.id, f.url, f.title, f.site_url, f.image_url, f.description, f.last_error, s.approved, \
                 (SELECT count(*) FROM episode e LEFT JOIN episode_state st ON st.episode_id = e.id AND st.profile_id = s.profile_id \
                  WHERE e.feed_id = f.id AND coalesce(st.finished, 0) = 0) \
                 FROM subscription s JOIN feed f ON f.id = s.feed_id WHERE s.profile_id = ? AND (s.approved = 1 OR ? = 0) \
                 ORDER BY f.title COLLATE NOCASE",
                [profile_id.into(), i64::from(only_approved).into()],
            ))
            .await?;
        rows.into_iter()
            .map(|r| {
                Ok(FeedRow {
                    id: r.try_get_by_index(0)?,
                    url: r.try_get_by_index(1)?,
                    title: r.try_get_by_index(2)?,
                    site_url: r.try_get_by_index(3)?,
                    image_url: r.try_get_by_index(4)?,
                    description: r.try_get_by_index(5)?,
                    last_error: r.try_get_by_index(6)?,
                    approved: r.try_get_by_index::<i64>(7)? == 1,
                    unheard: r.try_get_by_index(8)?,
                })
            })
            .collect()
    }

    /// May this profile see the feed (it follows it with a release)?
    pub async fn follows(&self, profile_id: i64, feed_id: i64) -> Result<bool> {
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT 1 FROM subscription WHERE profile_id = ? AND feed_id = ? AND approved = 1", [profile_id.into(), feed_id.into()]))
            .await?;
        Ok(row.is_some())
    }

    /// The newest episodes of a feed with the state of a profile.
    pub async fn episodes(&self, profile_id: i64, feed_id: i64, limit: i64) -> Result<Vec<EpisodeRow>> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT e.id, e.title, e.audio_url, e.published_at, e.duration_s, e.description, coalesce(st.position_ms, 0), coalesce(st.finished, 0) \
                 FROM episode e LEFT JOIN episode_state st ON st.episode_id = e.id AND st.profile_id = ? \
                 WHERE e.feed_id = ? ORDER BY e.published_at DESC, e.id DESC LIMIT ?",
                [profile_id.into(), feed_id.into(), limit.into()],
            ))
            .await?;
        rows.into_iter()
            .map(|r| {
                Ok(EpisodeRow {
                    id: r.try_get_by_index(0)?,
                    title: r.try_get_by_index(1)?,
                    audio_url: r.try_get_by_index(2)?,
                    published_at: r.try_get_by_index(3)?,
                    duration_s: r.try_get_by_index(4)?,
                    description: r.try_get_by_index(5)?,
                    position_ms: r.try_get_by_index(6)?,
                    finished: r.try_get_by_index::<i64>(7)? == 1,
                })
            })
            .collect()
    }

    /// The feed an episode belongs to.
    pub async fn episode_feed(&self, episode_id: i64) -> Result<Option<i64>> {
        let row = self.db.reader.query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT feed_id FROM episode WHERE id = ?", [episode_id.into()])).await?;
        Ok(row.map(|r| r.try_get_by_index(0)).transpose()?)
    }

    /// Where a profile stopped in an episode; the devices of the account all see the latest.
    pub async fn put_episode_state(&self, profile_id: i64, episode_id: i64, position_ms: i64, finished: bool) -> Result<()> {
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO episode_state (profile_id, episode_id, position_ms, finished, updated_at) VALUES (?, ?, ?, ?, ?) \
                 ON CONFLICT (profile_id, episode_id) DO UPDATE SET position_ms = excluded.position_ms, finished = excluded.finished, updated_at = excluded.updated_at",
                [profile_id.into(), episode_id.into(), position_ms.max(0).into(), i64::from(finished).into(), now_ms().into()],
            ))
            .await
            .map_err(|_| Error::WriteFailed("no such episode".into()))?;
        Ok(())
    }

    /// Adds a station (known by its stream address) and returns its id; a known one keeps what it has unless
    /// the new name is not empty.
    pub async fn save_station(&self, stream_url: &str, name: &str, homepage: Option<&str>, logo_url: Option<&str>) -> Result<i64> {
        let name = name.trim();
        if !is_web_address(stream_url) || name.is_empty() || name.chars().count() > 200 {
            return Err(Error::WriteFailed("a station has a name and an http or https stream address".into()));
        }
        let ok = |u: Option<&str>| u.filter(|u| is_web_address(u)).map(str::to_owned);
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO station (stream_url, name, homepage, logo_url) VALUES (?, ?, ?, ?) \
                 ON CONFLICT (stream_url) DO UPDATE SET name = excluded.name, homepage = coalesce(excluded.homepage, homepage), logo_url = coalesce(excluded.logo_url, logo_url)",
                [stream_url.to_owned().into(), name.to_owned().into(), ok(homepage).into(), ok(logo_url).into()],
            ))
            .await?;
        let row = self
            .db
            .writer
            .query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT id FROM station WHERE stream_url = ?", [stream_url.to_owned().into()]))
            .await?
            .ok_or_else(|| Error::WriteFailed("station not saved".into()))?;
        Ok(row.try_get_by_index(0)?)
    }

    /// Puts a station among the favourites of a profile (at the end). Doing it again changes nothing.
    pub async fn favorite_station(&self, profile_id: i64, station_id: i64, approved: bool) -> Result<()> {
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO station_favorite (profile_id, station_id, position, added_at, approved) \
                 VALUES (?, ?, (SELECT coalesce(max(position), 0) + 1 FROM station_favorite WHERE profile_id = ?), ?, ?) ON CONFLICT (profile_id, station_id) DO NOTHING",
                [profile_id.into(), station_id.into(), profile_id.into(), now_ms().into(), i64::from(approved).into()],
            ))
            .await
            .map_err(|_| Error::WriteFailed("no such station or profile".into()))?;
        Ok(())
    }

    pub async fn unfavorite_station(&self, profile_id: i64, station_id: i64) -> Result<()> {
        self.db.writer.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "DELETE FROM station_favorite WHERE profile_id = ? AND station_id = ?", [profile_id.into(), station_id.into()])).await?;
        Ok(())
    }

    /// The favourite stations of a profile in their order, the waiting ones marked (`only_approved` leaves them out).
    pub async fn favorite_stations(&self, profile_id: i64, only_approved: bool) -> Result<Vec<StationRow>> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT s.id, s.stream_url, s.name, s.homepage, s.logo_url, f.approved FROM station_favorite f JOIN station s ON s.id = f.station_id \
                 WHERE f.profile_id = ? AND (f.approved = 1 OR ? = 0) ORDER BY f.position",
                [profile_id.into(), i64::from(only_approved).into()],
            ))
            .await?;
        rows.into_iter()
            .map(|r| {
                Ok(StationRow {
                    id: r.try_get_by_index(0)?,
                    stream_url: r.try_get_by_index(1)?,
                    name: r.try_get_by_index(2)?,
                    homepage: r.try_get_by_index(3)?,
                    logo_url: r.try_get_by_index(4)?,
                    approved: r.try_get_by_index::<i64>(5)? == 1,
                })
            })
            .collect()
    }

    /// Every station with the names of the profiles that have it among their favourites, by name (for the administration).
    pub async fn stations_with_owners(&self) -> Result<Vec<(StationRow, Vec<String>)>> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_string(DbBackend::Sqlite, "SELECT id, stream_url, name, homepage, logo_url FROM station ORDER BY name COLLATE NOCASE, id"))
            .await?;
        let owners = self
            .db
            .reader
            .query_all_raw(Statement::from_string(DbBackend::Sqlite, "SELECT f.station_id, p.name, f.approved FROM station_favorite f JOIN profile p ON p.id = f.profile_id ORDER BY p.name COLLATE NOCASE"))
            .await?;
        let mut out = Vec::new();
        for r in rows {
            let id: i64 = r.try_get_by_index(0)?;
            let names = owners
                .iter()
                .filter(|o| o.try_get_by_index::<i64>(0).is_ok_and(|s| s == id))
                .filter_map(|o| {
                    let name: String = o.try_get_by_index(1).ok()?;
                    let approved: i64 = o.try_get_by_index(2).ok()?;
                    Some(if approved == 1 { name } else { format!("{name} (waiting)") })
                })
                .collect();
            out.push((StationRow { id, stream_url: r.try_get_by_index(1)?, name: r.try_get_by_index(2)?, homepage: r.try_get_by_index(3)?, logo_url: r.try_get_by_index(4)?, approved: true }, names));
        }
        Ok(out)
    }

    /// Changes what is known of a station. A field that is `None` stays; an empty text for the home page or the logo
    /// removes it. The name and the stream address are checked like those of a new station.
    pub async fn update_station(&self, id: i64, name: Option<&str>, stream_url: Option<&str>, homepage: Option<&str>, logo_url: Option<&str>) -> Result<()> {
        let current = self.stations_with_owners().await?.into_iter().find(|(s, _)| s.id == id).map(|(s, _)| s).ok_or_else(|| Error::WriteFailed("no such station".into()))?;
        let name = name.map_or(current.name.clone(), |n| n.trim().to_owned());
        let stream_url = stream_url.map_or(current.stream_url.clone(), |u| u.trim().to_owned());
        let keep_or = |new: Option<&str>, old: Option<String>| -> Option<String> { match new { None => old, Some(t) if t.trim().is_empty() => None, Some(t) => Some(t.trim().to_owned()) } };
        let homepage = keep_or(homepage, current.homepage);
        let logo = keep_or(logo_url, current.logo_url);
        if !is_web_address(&stream_url) || name.is_empty() || name.chars().count() > 200 || homepage.as_deref().is_some_and(|u| !is_web_address(u)) || logo.as_deref().is_some_and(|u| !is_web_address(u)) {
            return Err(Error::WriteFailed("a station has a name and http or https addresses".into()));
        }
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE station SET name = ?, stream_url = ?, homepage = ?, logo_url = ? WHERE id = ?",
                [name.into(), stream_url.into(), homepage.into(), logo.into(), id.into()],
            ))
            .await
            .map_err(|_| Error::WriteFailed("another station has this stream address".into()))?;
        Ok(())
    }

    /// Removes a station from the library: it disappears from every list of favourites.
    pub async fn delete_station(&self, id: i64) -> Result<()> {
        self.db.writer.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "DELETE FROM station WHERE id = ?", [id.into()])).await?;
        // A label that only this station carried goes with it.
        self.drop_unused_labels().await?;
        Ok(())
    }

    /// What waits for a release, oldest first.
    pub async fn pending_releases(&self) -> Result<Vec<PendingRow>> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT 'podcast', s.profile_id, p.name, s.feed_id, f.title, s.added_at FROM subscription s JOIN profile p ON p.id = s.profile_id JOIN feed f ON f.id = s.feed_id WHERE s.approved = 0 \
                 UNION ALL \
                 SELECT 'station', k.profile_id, p.name, k.station_id, t.name, k.added_at FROM station_favorite k JOIN profile p ON p.id = k.profile_id JOIN station t ON t.id = k.station_id WHERE k.approved = 0 \
                 ORDER BY 6",
            ))
            .await?;
        rows.into_iter()
            .map(|r| {
                let kind: String = r.try_get_by_index(0)?;
                Ok(PendingRow { kind: if kind == "podcast" { "podcast" } else { "station" }, profile_id: r.try_get_by_index(1)?, profile: r.try_get_by_index(2)?, target_id: r.try_get_by_index(3)?, title: r.try_get_by_index(4)? })
            })
            .collect()
    }

    /// Releases (or, with `release = false`, refuses) what a profile asked for. `kind` is `podcast` or `station`.
    pub async fn decide_release(&self, kind: &str, profile_id: i64, target_id: i64, release: bool) -> Result<()> {
        let (table, column) = match kind {
            "podcast" => ("subscription", "feed_id"),
            "station" => ("station_favorite", "station_id"),
            _ => return Err(Error::WriteFailed("kind is podcast or station".into())),
        };
        let sql = if release {
            format!("UPDATE {table} SET approved = 1 WHERE profile_id = ? AND {column} = ?")
        } else {
            format!("DELETE FROM {table} WHERE profile_id = ? AND {column} = ? AND approved = 0")
        };
        self.db.writer.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, sql, [profile_id.into(), target_id.into()])).await?;
        Ok(())
    }
}

/// One episode as the administration lists it: what it is and who has heard it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AdminEpisode {
    pub id: i64,
    pub title: String,
    pub published_at: i64,
    pub duration_s: Option<i64>,
    /// The profiles that heard it to the end.
    pub finished_by: Vec<String>,
    /// The profiles that began it and have not finished it.
    pub started_by: Vec<String>,
}

impl Store {
    /// One feed without the state of a profile, or `None`.
    pub async fn feed_head(&self, feed_id: i64) -> Result<Option<FeedHead>> {
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT id, url, title, last_error FROM feed WHERE id = ?", [feed_id.into()]))
            .await?;
        row.map(|r| Ok(FeedHead { id: r.try_get_by_index(0)?, url: r.try_get_by_index(1)?, title: r.try_get_by_index(2)?, last_error: r.try_get_by_index(3)? })).transpose()
    }

    /// Removes a podcast from the library: its episodes, the follows and the state of listening go with it, and a label that only it carried.
    pub async fn delete_feed(&self, feed_id: i64) -> Result<()> {
        self.db.writer.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "DELETE FROM feed WHERE id = ?", [feed_id.into()])).await?;
        self.drop_unused_labels().await?;
        Ok(())
    }

    /// How many episodes a feed has.
    pub async fn episode_count(&self, feed_id: i64) -> Result<usize> {
        let row = self.db.reader.query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT count(*) FROM episode WHERE feed_id = ?", [feed_id.into()])).await?;
        Ok(row.map(|r| r.try_get_by_index::<i64>(0)).transpose()?.unwrap_or(0).max(0) as usize)
    }

    /// A page of the episodes of a feed, newest first, with who has heard each one (for the administration).
    pub async fn admin_episodes(&self, feed_id: i64, limit: usize, offset: usize) -> Result<Vec<AdminEpisode>> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, title, published_at, duration_s FROM episode WHERE feed_id = ? ORDER BY published_at DESC, id DESC LIMIT ? OFFSET ?",
                [feed_id.into(), (limit as i64).into(), (offset as i64).into()],
            ))
            .await?;
        let mut out: Vec<AdminEpisode> = rows
            .into_iter()
            .map(|r| Ok(AdminEpisode { id: r.try_get_by_index(0)?, title: r.try_get_by_index(1)?, published_at: r.try_get_by_index(2)?, duration_s: r.try_get_by_index(3)?, finished_by: Vec::new(), started_by: Vec::new() }))
            .collect::<Result<_>>()?;
        if out.is_empty() {
            return Ok(out);
        }
        let states = self
            .db
            .reader
            .query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT st.episode_id, p.name, st.finished FROM episode_state st JOIN profile p ON p.id = st.profile_id JOIN episode e ON e.id = st.episode_id \
                 WHERE e.feed_id = ? AND (st.finished = 1 OR st.position_ms > 0) ORDER BY p.name COLLATE NOCASE",
                [feed_id.into()],
            ))
            .await?;
        for r in states {
            let (episode, name, finished): (i64, String, i64) = (r.try_get_by_index(0)?, r.try_get_by_index(1)?, r.try_get_by_index(2)?);
            if let Some(e) = out.iter_mut().find(|e| e.id == episode) {
                if finished == 1 { e.finished_by.push(name) } else { e.started_by.push(name) }
            }
        }
        Ok(out)
    }
}

/// A station found in the directory.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StationHit {
    pub name: String,
    pub stream_url: String,
    pub homepage: Option<String>,
    pub logo_url: Option<String>,
}

/// Most stations an answer of a search holds.
pub const MAX_STATION_HITS: usize = 25;

/// Reads the answer of a station directory in the format of radio-browser.info (a JSON array with `name`, `url_resolved`
/// or `url`, `homepage` and `favicon`). What has no name or no http(s) stream address is left out.
pub fn parse_directory(json: &str) -> std::result::Result<Vec<StationHit>, String> {
    let rows: Vec<serde_json::Value> = serde_json::from_str(json).map_err(|_| "the directory did not answer with a list of stations".to_owned())?;
    let text = |row: &serde_json::Value, key: &str| row.get(key).and_then(serde_json::Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
    Ok(rows
        .iter()
        .filter_map(|row| {
            let name = text(row, "name")?;
            let stream_url = [text(row, "url_resolved"), text(row, "url")].into_iter().flatten().find(|u| is_web_address(u))?;
            Some(StationHit { name, stream_url, homepage: text(row, "homepage").filter(|u| is_web_address(u)), logo_url: text(row, "favicon").filter(|u| is_web_address(u)) })
        })
        .take(MAX_STATION_HITS)
        .collect())
}

/// Searches the station directory of the settings (`radio_directory_url`; empty = no search) by name.
pub async fn search_stations(store: &Store, query: &str) -> std::result::Result<Vec<StationHit>, String> {
    let base = store.setting("radio_directory_url").await.ok().flatten().map(|v| v.trim().trim_end_matches('/').to_owned()).unwrap_or_default();
    if base.is_empty() {
        return Err("no_directory".into());
    }
    let q: String = query.chars().filter(|c| !c.is_control()).take(80).collect();
    if q.trim().is_empty() {
        return Ok(Vec::new());
    }
    let encoded = serde_urlencoded::to_string([("name", q.trim()), ("limit", &MAX_STATION_HITS.to_string()), ("hidebroken", "true"), ("order", "clickcount"), ("reverse", "true")]).map_err(|e| e.to_string())?;
    let address = format!("{base}/json/stations/search?{encoded}");
    let body = tokio::task::spawn_blocking(move || crate::fetch::download_text(&address, 2 * 1024 * 1024)).await.map_err(|e| e.to_string())??;
    parse_directory(&body)
}

/// Reads one feed again and stores it; the reason is kept with the feed when it fails.
pub async fn refresh_feed(store: &Store, feed_id: i64, url: &str) -> std::result::Result<(), String> {
    let max = store.setting("feed_max_bytes").await.ok().flatten().and_then(|v| v.trim().parse::<usize>().ok()).filter(|n| *n >= 1024).unwrap_or(5 * 1024 * 1024);
    let address = url.to_owned();
    let outcome = match tokio::task::spawn_blocking(move || crate::fetch::download_text(&address, max)).await {
        Ok(text) => text.and_then(|body| podcast::parse(&body)),
        Err(e) => Err(e.to_string()),
    };
    match outcome {
        Ok(feed) => store.save_feed(url, &feed).await.map(|_| ()).map_err(|e| e.to_string()),
        Err(why) => {
            let _ = store.feed_failed(feed_id, &why).await;
            Err(why)
        }
    }
}

/// How long to wait when the setting says "never by itself" (0): it is looked at again after this time.
const IDLE_CHECK: std::time::Duration = std::time::Duration::from_secs(300);

/// Starts the task that reads the feeds that somebody follows, every `feed_refresh_min` minutes (a setting; 0 = never by itself).
pub fn start_refresher(store: Store) {
    tokio::spawn(async move {
        loop {
            let minutes = store.setting("feed_refresh_min").await.ok().flatten().and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(60);
            if minutes == 0 {
                tokio::time::sleep(IDLE_CHECK).await;
                continue;
            }
            tokio::time::sleep(std::time::Duration::from_secs(minutes * 60)).await;
            for (id, url) in store.feeds_to_refresh().await.unwrap_or_default() {
                if let Err(why) = refresh_feed(&store, id, &url).await {
                    tracing::info!(feed = %url, %why, "feed not refreshed");
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<?xml version="1.0"?><rss version="2.0"><channel><title>Kinderfunk</title><link>https://example.org/</link><description>Geschichten</description>
      <item><title>Folge 2</title><guid>b</guid><enclosure url="https://example.org/2.mp3" type="audio/mpeg"/><pubDate>Tue, 20 Oct 2026 10:00:00 GMT</pubDate></item>
      <item><title>Folge 1</title><guid>a</guid><enclosure url="https://example.org/1.mp3" type="audio/mpeg"/><pubDate>Tue, 13 Oct 2026 10:00:00 GMT</pubDate></item></channel></rss>"#;

    #[test]
    fn a_directory_answer_gives_stations_with_a_stream_address_and_a_name() {
        let json = r#"[{"name":" Radio A ","url":"http://old.example.org/a","url_resolved":"https://stream.example.org/a","homepage":"https://a.example.org","favicon":"not an address"},
                       {"name":"","url_resolved":"https://x.example.org/x"},{"name":"No stream","url":"ftp://x/y"},{"name":"Radio B","url":"http://b.example.org/live"}]"#;
        let hits = parse_directory(json).unwrap();
        assert_eq!(hits.iter().map(|h| (h.name.as_str(), h.stream_url.as_str())).collect::<Vec<_>>(), [("Radio A", "https://stream.example.org/a"), ("Radio B", "http://b.example.org/live")]);
        assert_eq!((hits[0].homepage.as_deref(), hits[0].logo_url.as_deref()), (Some("https://a.example.org"), None));
        assert!(parse_directory("<html>").is_err());
    }

    #[test]
    fn the_sample_feed_reads() {
        assert_eq!(podcast::parse(XML).unwrap().episodes.len(), 2);
    }
}
