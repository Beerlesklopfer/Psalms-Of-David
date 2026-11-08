// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The name of the place a picture was taken (docs/spec-pictures.md; project owner, 2026-11-08: from OpenStreetMap). The server asks
//! Nominatim (`reverse`) with the coordinates rounded to three places (about 100 m), at most once a second as its rules demand, and keeps
//! every answer in `photo_place`, so a place is asked for once. With the setting `photo_places` = `off` nothing leaves the house.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};

use crate::error::Result;
use crate::picture_meta::Place;
use crate::store::{Store, now_ms};

const ENDPOINT: &str = "https://nominatim.openstreetmap.org/reverse";
/// Nominatim's rule: at most one request per second.
const GAP: Duration = Duration::from_millis(1100);
/// A failed request is not kept; the next picture of the place asks again, but not before this long.
const RETRY: Duration = Duration::from_secs(60);

/// The key of a place: the coordinates rounded to three places, and the language of the name.
pub fn key(p: Place, lang: &str) -> String {
    format!("{:.3},{:.3},{lang}", p.lat, p.lon)
}

/// The language of the names: the two letters of a known language, else English.
pub fn language(lang: &str) -> &'static str {
    match lang.get(..2).unwrap_or_default() {
        "de" => "de",
        _ => "en",
    }
}

/// "Oberstdorf, Deutschland" from the answer of Nominatim: the place (village, town, city, or the part of a city) and the country.
pub fn name_from(answer: &serde_json::Value) -> Option<String> {
    let a = answer.get("address")?;
    let get = |k: &str| a.get(k).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty());
    let local = ["village", "town", "city", "municipality", "hamlet", "suburb", "city_district", "county", "state"].into_iter().find_map(get);
    let mut parts: Vec<&str> = local.into_iter().collect();
    // In a city the part of it says more, then the city.
    if let (Some(part), Some(city)) = (get("suburb"), get("city"))
        && local == Some(city)
    {
        parts = vec![part, city];
    }
    if let Some(country) = get("country") {
        parts.push(country);
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}

struct Gate {
    last: Option<Instant>,
    failed: Option<Instant>,
}

fn gate() -> &'static Mutex<Gate> {
    static G: OnceLock<Mutex<Gate>> = OnceLock::new();
    G.get_or_init(|| Mutex::new(Gate { last: None, failed: None }))
}

/// Asks Nominatim (blocking; one request at a time, one per second). `Ok(None)`: the place has no name; `Err`: the request failed.
fn ask(p: Place, lang: &str) -> std::result::Result<Option<String>, String> {
    let mut g = gate().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if g.failed.is_some_and(|f| f.elapsed() < RETRY) {
        return Err("Nominatim failed a moment ago".into());
    }
    if let Some(last) = g.last {
        let wait = GAP.saturating_sub(last.elapsed());
        std::thread::sleep(wait);
    }
    let url = format!("{ENDPOINT}?format=jsonv2&zoom=14&addressdetails=1&lat={:.3}&lon={:.3}&accept-language={lang}", p.lat, p.lon);
    let agent = format!("David-Server/{} (family media server)", env!("CARGO_PKG_VERSION"));
    let answer = crate::fetch::get_bytes(&url, 256 * 1024, &agent);
    g.last = Some(Instant::now());
    match answer {
        Ok(Some(bytes)) => {
            g.failed = None;
            let v: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            Ok(name_from(&v))
        }
        Ok(None) => Ok(None),
        Err(e) => {
            g.failed = Some(Instant::now());
            Err(e)
        }
    }
}

impl Store {
    /// The name of a place in this language: kept, or asked of Nominatim (when `photo_places` is `osm`). `None`: no name, switched off,
    /// or the request failed (asked again later).
    pub async fn place_name(&self, p: Place, lang: &str) -> Result<Option<String>> {
        if self.setting("photo_places").await?.as_deref() != Some("osm") {
            return Ok(None);
        }
        let lang = language(lang);
        let k = key(p, lang);
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT name FROM photo_place WHERE key = ?", [Value::from(k.clone())]))
            .await?;
        if let Some(r) = row {
            return Ok(r.try_get_by_index(0)?);
        }
        let lang_owned = lang.to_owned();
        let asked = tokio::task::spawn_blocking(move || ask(p, &lang_owned)).await.unwrap_or_else(|e| Err(e.to_string()));
        match asked {
            Ok(name) => {
                self.db
                    .writer
                    .execute_raw(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "INSERT INTO photo_place (key, name, fetched_at) VALUES (?, ?, ?) ON CONFLICT (key) DO UPDATE SET name = excluded.name, fetched_at = excluded.fetched_at",
                        [Value::from(k), Value::from(name.clone()), Value::from(now_ms())],
                    ))
                    .await?;
                Ok(name)
            }
            Err(e) => {
                tracing::debug!("the name of a place: {e}");
                Ok(None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_name_is_the_place_and_the_country() {
        let v = json!({"address": {"village": "Oberstdorf", "county": "Oberallgäu", "country": "Deutschland"}});
        assert_eq!(name_from(&v).as_deref(), Some("Oberstdorf, Deutschland"));
        let v = json!({"address": {"suburb": "Kreuzberg", "city": "Berlin", "country": "Deutschland"}});
        assert_eq!(name_from(&v).as_deref(), Some("Kreuzberg, Berlin, Deutschland"));
        let v = json!({"address": {"country": "Österreich"}});
        assert_eq!(name_from(&v).as_deref(), Some("Österreich"));
        assert_eq!(name_from(&json!({"error": "Unable to geocode"})), None);
    }

    #[test]
    fn coordinates_are_rounded_to_about_100_m() {
        assert_eq!(key(Place { lat: 47.40987, lon: 10.27951 }, "de"), "47.410,10.280,de");
        assert_eq!(language("de-DE"), "de");
        assert_eq!(language("fr"), "en");
    }
}
