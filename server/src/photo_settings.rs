// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The settings of the picture sync (docs/spec-photo-sync.md, section 2a): the devices of the family copy their pictures to the server
//! (unisync, the backup) and, if a person chooses it, see the pictures of their other devices through it (bisync). Two of the three levels
//! live here: the server (administrators, keys `photo_*` of the `setting` table) and the profile (every person, table
//! `profile_photo_setting`). The third, the device, lives in the app. A lower level is never wider than the one above: a profile cannot
//! choose bisync that the server does not allow, nor videos the server does not take.
//!
//! Every choice has a list of options, and every option its own explanation (owner, 2026-11-07: choosing an option swaps the text that
//! explains it). The options are listed here, the texts are in the catalog: `photo.opt.<key>.<option>` names an option,
//! `photo.explain.<key>.<option>` explains it.
//!
//! The pictures of a person go into the picture folder of that profile (the Users page; `profile_folder` of kind `photo`), not into a folder
//! of the server: without a picture folder a profile cannot sync.

use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
use serde::Serialize;

use crate::error::Result;
use crate::store::Store;

/// A setting that is a choice: its key and its options, the first is the default.
#[derive(Debug, Clone, Copy)]
pub struct Choice {
    pub key: &'static str,
    pub options: &'static [&'static str],
}

/// A setting that is a whole number in a range.
#[derive(Debug, Clone, Copy)]
pub struct Number {
    pub key: &'static str,
    pub min: u32,
    pub max: u32,
    pub default: u32,
    /// Message key of the unit.
    pub unit: &'static str,
}

/// The choices of the server, in the order the page shows them. The first option is the default (the same as migration 0062).
pub const SERVER_CHOICES: &[Choice] = &[
    Choice { key: "photo_sync", options: &["off", "on"] },
    Choice { key: "photo_modes", options: &["uni", "both"] },
    Choice { key: "photo_videos", options: &["on", "off"] },
    Choice { key: "photo_children", options: &["parents", "off", "allowed"] },
    Choice { key: "photo_parents_may_view", options: &["off", "on"] },
];

/// The numbers of the server.
pub const SERVER_NUMBERS: &[Number] = &[
    Number { key: "photo_max_file_mb", min: 1, max: 65_536, default: 2048, unit: "unit.mb" },
    Number { key: "photo_quota_gb", min: 0, max: 1_000_000, default: 0, unit: "unit.gb" },
    Number { key: "photo_reserve_gb", min: 0, max: 1_000_000, default: 0, unit: "unit.gb" },
];

/// The choices of a profile, in the order the forms show them. The first option is the default (what a profile without a row has).
pub const PROFILE_CHOICES: &[Choice] = &[
    Choice { key: "enabled", options: &["off", "on"] },
    Choice { key: "mode", options: &["uni", "bi"] },
    Choice { key: "network", options: &["wifi", "any"] },
    Choice { key: "charging", options: &["charging", "always"] },
    Choice { key: "videos", options: &["on", "off"] },
    Choice { key: "deletes", options: &["never", "trash"] },
    Choice { key: "parent_view", options: &["off", "on"] },
];

/// All keys of the server, in the order of the page.
pub fn server_keys() -> impl Iterator<Item = &'static str> {
    SERVER_CHOICES.iter().map(|c| c.key).chain(SERVER_NUMBERS.iter().map(|n| n.key))
}

pub fn is_photo_key(key: &str) -> bool {
    server_keys().any(|k| k == key)
}

/// Checks a value of the server as typed or chosen: the value to store, or the code of the reason (`photo.verr.<code>`).
pub fn check(key: &str, raw: &str) -> std::result::Result<String, &'static str> {
    let raw = raw.trim();
    if let Some(c) = SERVER_CHOICES.iter().find(|c| c.key == key) {
        return c.options.contains(&raw).then(|| raw.to_owned()).ok_or("not_an_option");
    }
    if let Some(n) = SERVER_NUMBERS.iter().find(|n| n.key == key) {
        let v: u32 = raw.parse().map_err(|_| "not_a_number")?;
        return (n.min..=n.max).contains(&v).then(|| v.to_string()).ok_or("out_of_range");
    }
    Err("unknown")
}

/// The settings of the server, read and checked (a damaged value falls back to its default).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServerPhotoSettings {
    /// The feature exists at all.
    pub on: bool,
    /// Bisync may be chosen by a profile.
    pub bisync: bool,
    /// Videos are taken.
    pub videos: bool,
    /// `parents` (an administrator switches it on for a child), `off` (never for a child), `allowed` (a child may itself).
    pub children: String,
    /// Administrators may look at the pictures of a child that allows it.
    pub parents_may_view: bool,
    pub max_file_mb: u32,
    /// 0 = no limit.
    pub quota_gb: u32,
    /// 0 = the reserve of the library (`min_free_gb`).
    pub reserve_gb: u32,
}

impl Default for ServerPhotoSettings {
    fn default() -> Self {
        Self::from_map(&HashMap::new())
    }
}

impl ServerPhotoSettings {
    pub async fn load(store: &Store) -> Result<ServerPhotoSettings> {
        let mut map = HashMap::new();
        for key in server_keys() {
            if let Some(v) = store.setting(key).await? {
                map.insert(key, v);
            }
        }
        Ok(Self::from_map(&map))
    }

    fn from_map(m: &HashMap<&'static str, String>) -> ServerPhotoSettings {
        let choice = |key: &str| -> String {
            let c = SERVER_CHOICES.iter().find(|c| c.key == key).expect("a known choice");
            m.get(key).filter(|v| c.options.contains(&v.as_str())).cloned().unwrap_or_else(|| c.options[0].to_owned())
        };
        let number = |key: &str| -> u32 {
            let n = SERVER_NUMBERS.iter().find(|n| n.key == key).expect("a known number");
            m.get(key).and_then(|v| v.parse().ok()).filter(|v| (n.min..=n.max).contains(v)).unwrap_or(n.default)
        };
        ServerPhotoSettings {
            on: choice("photo_sync") == "on",
            bisync: choice("photo_modes") == "both",
            videos: choice("photo_videos") == "on",
            children: choice("photo_children"),
            parents_may_view: choice("photo_parents_may_view") == "on",
            max_file_mb: number("photo_max_file_mb"),
            quota_gb: number("photo_quota_gb"),
            reserve_gb: number("photo_reserve_gb"),
        }
    }

    /// The options of a profile choice that the server does not allow for this role, each with the reason (`photo.why.<code>`).
    /// `self_service`: the person sets it for themselves (a child only when the server says `allowed`).
    pub fn locked(&self, role: &str, key: &str, self_service: bool) -> Vec<(&'static str, &'static str)> {
        let child = role == "child";
        match key {
            "enabled" if !self.on => vec![("on", "server_off")],
            "enabled" if child && self.children == "off" => vec![("on", "no_children")],
            "enabled" if child && self_service && self.children == "parents" => vec![("on", "parents_only")],
            "enabled" if role == "guest" => vec![("on", "no_guests")],
            "mode" if !self.bisync => vec![("bi", "no_bisync")],
            "videos" if !self.videos => vec![("on", "no_videos")],
            "parent_view" if !child => vec![("on", "children_only")],
            "parent_view" if !self.parents_may_view => vec![("on", "no_parent_view")],
            _ => Vec::new(),
        }
    }
}

/// The settings of one profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProfilePhotoSettings {
    pub enabled: bool,
    /// `uni` or `bi`.
    pub mode: String,
    /// `wifi` or `any`.
    pub network: String,
    /// `charging` or `always`.
    pub charging: String,
    pub videos: bool,
    /// `never` or `trash` (bisync only).
    pub deletes: String,
    pub parent_view: bool,
}

impl Default for ProfilePhotoSettings {
    fn default() -> Self {
        Self::from_choices(&HashMap::new())
    }
}

impl ProfilePhotoSettings {
    /// The value of a choice as the forms name it (`on`/`off` for a switch).
    pub fn choice(&self, key: &str) -> &str {
        let on = |b: bool| if b { "on" } else { "off" };
        match key {
            "enabled" => on(self.enabled),
            "mode" => &self.mode,
            "network" => &self.network,
            "charging" => &self.charging,
            "videos" => on(self.videos),
            "deletes" => &self.deletes,
            "parent_view" => on(self.parent_view),
            _ => "",
        }
    }

    /// From chosen values; a missing or unknown one is the default.
    pub fn from_choices(m: &HashMap<String, String>) -> ProfilePhotoSettings {
        let get = |key: &str| -> String {
            let c = PROFILE_CHOICES.iter().find(|c| c.key == key).expect("a known choice");
            m.get(key).filter(|v| c.options.contains(&v.as_str())).cloned().unwrap_or_else(|| c.options[0].to_owned())
        };
        ProfilePhotoSettings {
            enabled: get("enabled") == "on",
            mode: get("mode"),
            network: get("network"),
            charging: get("charging"),
            videos: get("videos") == "on",
            deletes: get("deletes"),
            parent_view: get("parent_view") == "on",
        }
    }

    /// What the settings are worth under the rules of the server: an option the server does not allow counts as the safe one (off, uni).
    /// What is stored stays, so that a person's choice comes back when the server allows it again.
    pub fn effective(&self, server: &ServerPhotoSettings, role: &str) -> ProfilePhotoSettings {
        let mut e = self.clone();
        let child = role == "child";
        e.enabled &= server.on && role != "guest" && !(child && server.children == "off");
        if !server.bisync {
            e.mode = "uni".into();
        }
        if e.mode != "bi" {
            e.deletes = "never".into();
        }
        e.videos &= server.videos;
        e.parent_view &= child && server.parents_may_view;
        e
    }
}

/// Why a profile's choice is refused, as a code of `photo.why.<code>`.
pub fn refusal(server: &ServerPhotoSettings, role: &str, self_service: bool, key: &str, value: &str) -> Option<&'static str> {
    if !PROFILE_CHOICES.iter().any(|c| c.key == key && c.options.contains(&value)) {
        return Some("not_an_option");
    }
    server.locked(role, key, self_service).into_iter().find(|(o, _)| *o == value).map(|(_, why)| why)
}

impl Store {
    /// The stored settings of a profile (the defaults when it has none).
    pub async fn profile_photo_settings(&self, profile: i64) -> Result<ProfilePhotoSettings> {
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT enabled, mode, network, charging, videos, deletes, parent_view FROM profile_photo_setting WHERE profile_id = ?",
                [Value::from(profile)],
            ))
            .await?;
        let Some(r) = row else { return Ok(ProfilePhotoSettings::default()) };
        let on = |b: i64| if b != 0 { "on" } else { "off" }.to_owned();
        let mut m = HashMap::new();
        m.insert("enabled".to_owned(), on(r.try_get("", "enabled")?));
        m.insert("mode".to_owned(), r.try_get::<String>("", "mode")?);
        m.insert("network".to_owned(), r.try_get::<String>("", "network")?);
        m.insert("charging".to_owned(), r.try_get::<String>("", "charging")?);
        m.insert("videos".to_owned(), on(r.try_get("", "videos")?));
        m.insert("deletes".to_owned(), r.try_get::<String>("", "deletes")?);
        m.insert("parent_view".to_owned(), on(r.try_get("", "parent_view")?));
        Ok(ProfilePhotoSettings::from_choices(&m))
    }

    /// Stores the settings of a profile as they are (the caller checked them with [`refusal`]).
    pub async fn save_profile_photo_settings(&self, profile: i64, s: &ProfilePhotoSettings) -> Result<()> {
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO profile_photo_setting (profile_id, enabled, mode, network, charging, videos, deletes, parent_view) VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
                 ON CONFLICT (profile_id) DO UPDATE SET enabled = excluded.enabled, mode = excluded.mode, network = excluded.network, \
                 charging = excluded.charging, videos = excluded.videos, deletes = excluded.deletes, parent_view = excluded.parent_view",
                [
                    Value::from(profile),
                    Value::from(i64::from(s.enabled)),
                    Value::from(s.mode.clone()),
                    Value::from(s.network.clone()),
                    Value::from(s.charging.clone()),
                    Value::from(i64::from(s.videos)),
                    Value::from(s.deletes.clone()),
                    Value::from(i64::from(s.parent_view)),
                ],
            ))
            .await?;
        Ok(())
    }
}

/// What a form shows of a choice: the value, and every option with the reason it cannot be chosen (empty: it can).
fn choice_view(c: &Choice, value: &str, locked: &[(&'static str, &'static str)]) -> serde_json::Value {
    let options: Vec<serde_json::Value> =
        c.options.iter().map(|o| serde_json::json!({ "value": o, "why": locked.iter().find(|(l, _)| l == o).map_or("", |(_, w)| *w) })).collect();
    serde_json::json!({ "key": c.key, "value": value, "options": options })
}

/// The tab of the settings page: the rules of the server (`typed` holds values a refused save sent, shown as they were typed), and who
/// backs up their phone.
pub async fn server_view(store: &Store, typed: &HashMap<String, String>) -> Result<serde_json::Value> {
    let mut choices = Vec::new();
    for c in SERVER_CHOICES {
        let stored = store.setting(c.key).await?.filter(|v| c.options.contains(&v.as_str())).unwrap_or_else(|| c.options[0].to_owned());
        choices.push(choice_view(c, typed.get(c.key).unwrap_or(&stored), &[]));
    }
    let mut numbers = Vec::new();
    for n in SERVER_NUMBERS {
        let stored = store.setting(n.key).await?.unwrap_or_else(|| n.default.to_string());
        numbers.push(serde_json::json!({ "key": n.key, "value": typed.get(n.key).unwrap_or(&stored), "unit": n.unit }));
    }
    let server = ServerPhotoSettings::load(store).await?;
    let folders = store.all_profile_folders().await?;
    let mut profiles = Vec::new();
    for p in store.list_profiles().await? {
        if p.role == "guest" {
            continue;
        }
        let e = store.profile_photo_settings(p.id).await?.effective(&server, &p.role);
        let folder = folders.get(&p.id.to_string()).and_then(|k| k.get("photo")).is_some();
        profiles.push(serde_json::json!({ "name": p.name, "role": p.role, "folder": folder, "enabled": e.enabled, "mode": e.mode }));
    }
    Ok(serde_json::json!({ "choices": choices, "numbers": numbers, "profiles": profiles }))
}

/// The choices of one profile as a form shows them, with the options the server does not allow locked and the reason why.
/// `self_service`: the person looks at their own settings (else an administrator does).
pub async fn profile_view(store: &Store, profile: i64, role: &str, self_service: bool) -> Result<serde_json::Value> {
    let server = ServerPhotoSettings::load(store).await?;
    let mine = store.profile_photo_settings(profile).await?;
    let folder = store.profile_folders(profile).await?.iter().any(|(k, _)| k == "photo");
    let mut choices = Vec::new();
    for c in PROFILE_CHOICES {
        if c.key == "parent_view" && role != "child" {
            continue;
        }
        let mut locked = server.locked(role, c.key, self_service);
        if c.key == "enabled" && locked.is_empty() && !folder {
            locked.push(("on", "no_folder"));
        }
        choices.push(choice_view(c, mine.choice(c.key), &locked));
    }
    let effective = mine.effective(&server, role);
    Ok(serde_json::json!({ "server_on": server.on, "folder": folder, "active": effective.enabled && folder, "choices": choices }))
}

/// Checks and stores the choices of a form for a profile. The refused choices come back as (key, reason); then nothing is stored.
/// Switching something off is always allowed, so that an old choice the server no longer allows can be taken back.
pub async fn save_profile(store: &Store, profile: i64, role: &str, self_service: bool, form: &HashMap<String, String>) -> Result<Vec<(String, &'static str)>> {
    let server = ServerPhotoSettings::load(store).await?;
    let before = store.profile_photo_settings(profile).await?;
    let folder = store.profile_folders(profile).await?.iter().any(|(k, _)| k == "photo");
    let mut chosen: HashMap<String, String> = PROFILE_CHOICES.iter().map(|c| (c.key.to_owned(), before.choice(c.key).to_owned())).collect();
    let mut refused = Vec::new();
    for c in PROFILE_CHOICES {
        let Some(v) = form.get(c.key).map(|v| v.trim()) else { continue };
        if v == before.choice(c.key) {
            continue;
        }
        let why = refusal(&server, role, self_service, c.key, v).or_else(|| (c.key == "enabled" && v == "on" && !folder).then_some("no_folder"));
        match why {
            Some(w) => refused.push((c.key.to_owned(), w)),
            None => {
                chosen.insert(c.key.to_owned(), v.to_owned());
            }
        }
    }
    if refused.is_empty() {
        store.save_profile_photo_settings(profile, &ProfilePhotoSettings::from_choices(&chosen)).await?;
    }
    Ok(refused)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_of_the_server_is_an_option_or_a_number_in_its_range() {
        assert_eq!(check("photo_sync", " on "), Ok("on".into()));
        assert_eq!(check("photo_sync", "maybe"), Err("not_an_option"));
        assert_eq!(check("photo_max_file_mb", "4096"), Ok("4096".into()));
        assert_eq!(check("photo_max_file_mb", "0"), Err("out_of_range"));
        assert_eq!(check("photo_quota_gb", "x"), Err("not_a_number"));
        assert_eq!(check("cd_format", "mp3"), Err("unknown"));
    }

    #[test]
    fn the_defaults_are_the_first_options_and_the_feature_is_off() {
        let s = ServerPhotoSettings::default();
        assert!(!s.on && !s.bisync && s.videos && s.children == "parents" && !s.parents_may_view && s.max_file_mb == 2048);
        let p = ProfilePhotoSettings::default();
        assert!(!p.enabled && p.mode == "uni" && p.network == "wifi" && p.charging == "charging" && p.videos && p.deletes == "never" && !p.parent_view);
    }

    #[test]
    fn a_profile_is_never_wider_than_the_server() {
        let mut server = ServerPhotoSettings { on: true, ..ServerPhotoSettings::default() };
        let all = ProfilePhotoSettings { enabled: true, mode: "bi".into(), deletes: "trash".into(), videos: true, parent_view: true, ..ProfilePhotoSettings::default() };
        let e = all.effective(&server, "member");
        assert!(e.enabled && e.mode == "uni" && e.deletes == "never" && !e.parent_view, "no bisync allowed, parent view only for children");
        server.bisync = true;
        server.videos = false;
        let e = all.effective(&server, "member");
        assert!(e.mode == "bi" && e.deletes == "trash" && !e.videos);
        assert!(!all.effective(&server, "guest").enabled);
        server.children = "off".into();
        assert!(!all.effective(&server, "child").enabled);
        server.children = "parents".into();
        server.parents_may_view = true;
        assert!(all.effective(&server, "child").parent_view);
    }

    #[test]
    fn a_child_switches_it_on_only_when_the_server_allows_it() {
        let server = ServerPhotoSettings { on: true, ..ServerPhotoSettings::default() };
        assert_eq!(refusal(&server, "child", true, "enabled", "on"), Some("parents_only"));
        assert_eq!(refusal(&server, "child", false, "enabled", "on"), None, "an administrator may");
        assert_eq!(refusal(&server, "child", true, "enabled", "off"), None, "switching off is always allowed");
        let allowed = ServerPhotoSettings { children: "allowed".into(), ..server.clone() };
        assert_eq!(refusal(&allowed, "child", true, "enabled", "on"), None);
        assert_eq!(refusal(&server, "member", true, "mode", "bi"), Some("no_bisync"));
        assert_eq!(refusal(&server, "member", true, "mode", "sideways"), Some("not_an_option"));
        let off = ServerPhotoSettings::default();
        assert_eq!(refusal(&off, "admin", true, "enabled", "on"), Some("server_off"));
    }
}
