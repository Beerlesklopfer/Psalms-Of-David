// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The projects of a slideshow, put together by hand (docs/spec-pictures.md, 9; project owner, 2026-11-08): a person picks pictures in
//! a preview and puts them into a project, then sets their order by numbers and, for each picture, its transition and its seconds; the
//! properties of the project hold the title, the music and the choices every picture starts with. Only the person who made a project
//! sees it and changes it.
//!
//! A project plays like a choice of `/pictures`: the slideshow, the project file and the video all take what [`chosen`] gives them,
//! either the choice of a query (`source=…&from=…`) or a project (`show=<id>`).

use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait, Value};
use serde::Serialize;

use crate::error::Result;
use crate::pictures::{Item, Query};
use crate::roles::Role;
use crate::show_project::Settings;
use crate::store::Store;

/// The longest title of a project.
pub const TITLE_MAX: usize = 120;
/// The most pictures one project holds.
pub const PICTURES_MAX: usize = 5000;
/// The choices of a picture and of a project, as the database allows them.
pub const SECONDS: [u32; 5] = [3, 5, 8, 15, 30];
pub const TRANSITIONS: [&str; 4] = ["fade", "zoom", "cut", "random"];

/// A project as the list of `/pictures` shows it.
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub id: i64,
    pub title: String,
    pub pictures: i64,
    pub updated_at: i64,
}

/// A project with its properties.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Slideshow {
    pub id: i64,
    pub title: String,
    /// `numbers` or `random`.
    pub ordering: String,
    pub seconds: u32,
    pub transition: String,
    pub videos: String,
    pub info: String,
    pub music: Option<i64>,
}

/// The properties a person sets; what is not one of the choices is refused.
#[derive(Debug, Clone, Default)]
pub struct Properties {
    pub title: String,
    pub ordering: String,
    pub seconds: u32,
    pub transition: String,
    pub videos: String,
    pub info: String,
    pub music: Option<i64>,
}

impl Properties {
    /// The properties of a form; `None` when one is not a choice of the page (or the title is empty or too long).
    pub fn from_form(pairs: &[(String, String)]) -> Option<Properties> {
        let get = |k: &str| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| v.trim()).unwrap_or_default();
        let one = |k: &str, options: &[&str]| options.contains(&get(k)).then(|| get(k).to_owned());
        let title = get("title").to_owned();
        if title.is_empty() || title.chars().count() > TITLE_MAX {
            return None;
        }
        Some(Properties {
            title,
            ordering: one("ordering", &["numbers", "random"])?,
            seconds: get("seconds").parse().ok().filter(|s| SECONDS.contains(s))?,
            transition: one("transition", &TRANSITIONS)?,
            videos: one("videos", &["on", "off"])?,
            info: one("info", &["both", "date", "none"])?,
            music: match get("music") {
                "" => None,
                m => Some(m.parse().ok()?),
            },
        })
    }
}

/// A picture of a project: its own transition and seconds (`None`: those of the project).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Entry {
    #[serde(flatten)]
    pub item: Item,
    pub transition: Option<String>,
    pub seconds: Option<u32>,
}

/// What plays: the pictures in order, the choices, the music and (for a project) its title.
#[derive(Debug, Clone)]
pub struct Chosen {
    pub entries: Vec<Entry>,
    pub settings: Settings,
    pub music: Option<i64>,
    pub title: Option<String>,
}

/// The project a query names (`show=<id>`): `None` when it names none, `Some(None)` when what it names is no id.
pub fn show_of(raw: Option<&str>) -> Option<Option<i64>> {
    let pairs: Vec<(String, String)> = serde_urlencoded::from_str(raw.unwrap_or_default()).unwrap_or_default();
    pairs.into_iter().find(|(k, _)| k == "show").map(|(_, v)| v.parse().ok())
}

/// What a query asks to play: a project of the person (`show=<id>`; `None` when there is no such project of the person), or else the
/// choice of the query. Only the pictures the person may see now.
pub async fn chosen(store: &Store, user: i64, role: Role, raw: Option<&str>) -> Result<Option<Chosen>> {
    let Some(id) = show_of(raw) else {
        let q = crate::api_pictures::parse_query(raw);
        let (settings, music) = crate::show_project::settings(raw);
        let entries = store.pictures(user, role, &q).await?.into_iter().map(|item| Entry { item, transition: None, seconds: None }).collect();
        return Ok(Some(Chosen { entries, settings, music, title: None }));
    };
    let Some(id) = id else { return Ok(None) };
    let Some(show) = store.slideshow(user, id).await? else { return Ok(None) };
    let seen: HashMap<String, Item> = visible(store, user, role).await?;
    let mut entries = Vec::new();
    for p in store.slideshow_pictures(id).await? {
        let Some(mut item) = seen.get(&p.key).cloned() else { continue };
        // Without videos a video shows as its picture.
        if show.videos == "off" {
            item.video = false;
        }
        entries.push(Entry { item, transition: p.transition, seconds: p.seconds });
    }
    let settings = Settings {
        // The players keep the order of the list unless it is shuffled.
        order: if show.ordering == "random" { "random".into() } else { "date".into() },
        seconds: show.seconds,
        transition: show.transition.clone(),
        videos: show.videos.clone(),
        info: show.info.clone(),
    };
    Ok(Some(Chosen { entries, settings, music: show.music, title: Some(show.title) }))
}

/// Every picture and video the person may see now, by key.
pub async fn visible(store: &Store, user: i64, role: Role) -> Result<HashMap<String, Item>> {
    let all = store.pictures(user, role, &Query { videos: true, ..Query::default() }).await?;
    Ok(all.into_iter().map(|i| (i.key.clone(), i)).collect())
}

/// A picture as the project keeps it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Kept {
    pub key: String,
    pub position: i64,
    pub transition: Option<String>,
    pub seconds: Option<u32>,
}

fn sql<const N: usize>(text: &str, values: [Value; N]) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, text, values)
}

fn now_touch(id: i64) -> Statement {
    // An update that changes nothing makes the trigger stamp the time and the actor.
    sql("UPDATE slideshow SET updated_at = updated_at WHERE id = ?", [id.into()])
}

async fn renumber(txn: &impl ConnectionTrait, id: i64) -> Result<()> {
    let rows = txn.query_all_raw(sql("SELECT key, position FROM slideshow_picture WHERE slideshow_id = ? ORDER BY position, key", [id.into()])).await?;
    for (n, r) in rows.iter().enumerate() {
        let (key, pos): (String, i64) = (r.try_get_by_index(0)?, r.try_get_by_index(1)?);
        if pos != n as i64 + 1 {
            txn.execute_raw(sql("UPDATE slideshow_picture SET position = ? WHERE slideshow_id = ? AND key = ?", [(n as i64 + 1).into(), id.into(), key.into()])).await?;
        }
    }
    txn.execute_raw(now_touch(id)).await?;
    Ok(())
}

fn show_row(r: &sea_orm::QueryResult) -> std::result::Result<Slideshow, sea_orm::DbErr> {
    let seconds: i64 = r.try_get("", "seconds")?;
    Ok(Slideshow {
        id: r.try_get("", "id")?,
        title: r.try_get("", "title")?,
        ordering: r.try_get("", "ordering")?,
        seconds: seconds as u32,
        transition: r.try_get("", "transition")?,
        videos: r.try_get("", "videos")?,
        info: r.try_get("", "info")?,
        music: r.try_get("", "music")?,
    })
}

impl Store {
    /// The projects of a person, the one changed last first.
    pub async fn slideshows(&self, user: i64) -> Result<Vec<Summary>> {
        let rows = self
            .db
            .reader
            .query_all_raw(sql(
                "SELECT s.id, s.title, s.updated_at, (SELECT count(*) FROM slideshow_picture p WHERE p.slideshow_id = s.id) AS n \
                 FROM slideshow s WHERE s.user_id = ? ORDER BY s.updated_at DESC, s.id DESC",
                [user.into()],
            ))
            .await?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(Summary { id: r.try_get("", "id")?, title: r.try_get("", "title")?, pictures: r.try_get("", "n")?, updated_at: r.try_get("", "updated_at")? });
        }
        Ok(out)
    }

    /// A project of the person; `None` when the person has no such project.
    pub async fn slideshow(&self, user: i64, id: i64) -> Result<Option<Slideshow>> {
        let r = self
            .db
            .reader
            .query_one_raw(sql(
                "SELECT id, title, ordering, seconds, transition, videos, info, music FROM slideshow WHERE user_id = ? AND id = ?",
                [user.into(), id.into()],
            ))
            .await?;
        Ok(r.as_ref().map(show_row).transpose()?)
    }

    /// A new, empty project with the usual choices.
    pub async fn create_slideshow(&self, user: i64, title: &str) -> Result<i64> {
        let r = self.db.writer.query_one_raw(sql("INSERT INTO slideshow (user_id, title) VALUES (?, ?) RETURNING id", [user.into(), title.into()])).await?;
        Ok(r.map(|r| r.try_get("", "id")).transpose()?.unwrap_or_default())
    }

    /// Sets the properties; false when the person has no such project. A playlist that is not one of the person is no music.
    pub async fn set_slideshow(&self, user: i64, id: i64, p: &Properties) -> Result<bool> {
        let music = match p.music {
            Some(m) => self
                .db
                .reader
                .query_one_raw(sql("SELECT id FROM playlist WHERE id = ? AND profile_id = ?", [m.into(), user.into()]))
                .await?
                .map(|_| m),
            None => None,
        };
        let n = self
            .db
            .writer
            .execute_raw(sql(
                "UPDATE slideshow SET title = ?, ordering = ?, seconds = ?, transition = ?, videos = ?, info = ?, music = ? WHERE user_id = ? AND id = ?",
                [
                    p.title.clone().into(),
                    p.ordering.clone().into(),
                    (p.seconds as i64).into(),
                    p.transition.clone().into(),
                    p.videos.clone().into(),
                    p.info.clone().into(),
                    music.into(),
                    user.into(),
                    id.into(),
                ],
            ))
            .await?
            .rows_affected();
        Ok(n > 0)
    }

    pub async fn delete_slideshow(&self, user: i64, id: i64) -> Result<bool> {
        let n = self.db.writer.execute_raw(sql("DELETE FROM slideshow WHERE user_id = ? AND id = ?", [user.into(), id.into()])).await?.rows_affected();
        Ok(n > 0)
    }

    /// The pictures of a project in their order (the caller has checked that it is the person's).
    pub async fn slideshow_pictures(&self, id: i64) -> Result<Vec<Kept>> {
        let rows = self
            .db
            .reader
            .query_all_raw(sql("SELECT key, position, transition, seconds FROM slideshow_picture WHERE slideshow_id = ? ORDER BY position", [id.into()]))
            .await?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let seconds: Option<i64> = r.try_get_by_index(3)?;
            out.push(Kept { key: r.try_get_by_index(0)?, position: r.try_get_by_index(1)?, transition: r.try_get_by_index(2)?, seconds: seconds.map(|s| s as u32) });
        }
        Ok(out)
    }

    /// Puts pictures at the end of a project, in the order given (by the time they were taken, when the page asks): only those the
    /// person may see, not one twice, at most [`PICTURES_MAX`]. How many came in; `None` when the person has no such project.
    pub async fn add_slideshow_pictures(&self, user: i64, role: Role, id: i64, keys: &[String]) -> Result<Option<usize>> {
        if self.slideshow(user, id).await?.is_none() {
            return Ok(None);
        }
        let seen = crate::slideshow::visible(self, user, role).await?;
        let txn = self.db.writer.begin().await?;
        let have: Vec<String> = txn
            .query_all_raw(sql("SELECT key FROM slideshow_picture WHERE slideshow_id = ?", [id.into()]))
            .await?
            .iter()
            .map(|r| r.try_get_by_index(0))
            .collect::<std::result::Result<_, _>>()?;
        let mut have: std::collections::HashSet<String> = have.into_iter().collect();
        let mut next = have.len() as i64 + 1;
        let mut added = 0;
        for key in keys {
            if have.len() >= PICTURES_MAX {
                break;
            }
            if !seen.contains_key(key) || have.contains(key) {
                continue;
            }
            txn.execute_raw(sql("INSERT INTO slideshow_picture (slideshow_id, key, position) VALUES (?, ?, ?)", [id.into(), key.clone().into(), next.into()])).await?;
            have.insert(key.clone());
            next += 1;
            added += 1;
        }
        renumber(&txn, id).await?;
        txn.commit().await?;
        Ok(Some(added))
    }

    /// Takes pictures out of a project; the others close up. `None` when the person has no such project.
    pub async fn remove_slideshow_pictures(&self, user: i64, id: i64, keys: &[String]) -> Result<Option<usize>> {
        if self.slideshow(user, id).await?.is_none() {
            return Ok(None);
        }
        let txn = self.db.writer.begin().await?;
        let mut n = 0;
        for key in keys {
            n += txn.execute_raw(sql("DELETE FROM slideshow_picture WHERE slideshow_id = ? AND key = ?", [id.into(), key.clone().into()])).await?.rows_affected() as usize;
        }
        renumber(&txn, id).await?;
        txn.commit().await?;
        Ok(Some(n))
    }

    /// Puts the pictures in this order; a picture of the project the order leaves out keeps its place after them, a key that is not in
    /// the project is passed over. False when the person has no such project.
    pub async fn order_slideshow(&self, user: i64, id: i64, keys: &[String]) -> Result<bool> {
        if self.slideshow(user, id).await?.is_none() {
            return Ok(false);
        }
        let txn = self.db.writer.begin().await?;
        let have: Vec<String> = txn
            .query_all_raw(sql("SELECT key FROM slideshow_picture WHERE slideshow_id = ? ORDER BY position", [id.into()]))
            .await?
            .iter()
            .map(|r| r.try_get_by_index(0))
            .collect::<std::result::Result<_, _>>()?;
        let mut order: Vec<&String> = Vec::with_capacity(have.len());
        for k in keys {
            if have.contains(k) && !order.contains(&k) {
                order.push(k);
            }
        }
        for k in &have {
            if !order.contains(&k) {
                order.push(k);
            }
        }
        for (n, k) in order.iter().enumerate() {
            txn.execute_raw(sql("UPDATE slideshow_picture SET position = ? WHERE slideshow_id = ? AND key = ?", [(n as i64 + 1).into(), id.into(), (*k).clone().into()])).await?;
        }
        txn.execute_raw(now_touch(id)).await?;
        txn.commit().await?;
        Ok(true)
    }

    /// The transition and the seconds of one picture of a project (`None`: those of the project). False when there is no such picture
    /// in a project of the person.
    pub async fn set_slideshow_picture(&self, user: i64, id: i64, key: &str, transition: Option<&str>, seconds: Option<u32>) -> Result<bool> {
        if self.slideshow(user, id).await?.is_none() {
            return Ok(false);
        }
        let txn = self.db.writer.begin().await?;
        let n = txn
            .execute_raw(sql(
                "UPDATE slideshow_picture SET transition = ?, seconds = ? WHERE slideshow_id = ? AND key = ?",
                [transition.map(str::to_owned).into(), seconds.map(|s| s as i64).into(), id.into(), key.into()],
            ))
            .await?
            .rows_affected();
        txn.execute_raw(now_touch(id)).await?;
        txn.commit().await?;
        Ok(n > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(a, b)| ((*a).to_owned(), (*b).to_owned())).collect()
    }

    #[test]
    fn the_properties_take_only_the_choices_of_the_page() {
        let ok = [("title", " Allgäu "), ("ordering", "numbers"), ("seconds", "15"), ("transition", "zoom"), ("videos", "off"), ("info", "date"), ("music", "7")];
        let p = Properties::from_form(&form(&ok)).unwrap();
        assert_eq!((p.title.as_str(), p.ordering.as_str(), p.seconds, p.transition.as_str(), p.music), ("Allgäu", "numbers", 15, "zoom", Some(7)));
        let mut no_music = form(&ok);
        no_music[6].1 = String::new();
        assert_eq!(Properties::from_form(&no_music).unwrap().music, None);
        for (k, bad) in [("title", "  "), ("ordering", "date"), ("seconds", "4"), ("transition", "<b>"), ("videos", "x"), ("info", "all"), ("music", "x")] {
            let mut f = form(&ok);
            f.iter_mut().find(|(n, _)| n == k).unwrap().1 = bad.to_owned();
            assert!(Properties::from_form(&f).is_none(), "{k}={bad}");
        }
        let mut long = form(&ok);
        long[0].1 = "x".repeat(TITLE_MAX + 1);
        assert!(Properties::from_form(&long).is_none());
    }

    #[test]
    fn a_project_is_named_in_the_query() {
        assert_eq!(show_of(Some("show=12&source=own")), Some(Some(12)));
        assert_eq!(show_of(Some("source=own")), None);
        assert_eq!(show_of(Some("show=x")), Some(None));
    }
}
