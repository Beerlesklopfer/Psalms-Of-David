// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! What a user may change about themselves: the picture and the PIN.

use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};

use crate::auth::{Login, valid_secret};
use crate::error::{Error, Result};
use crate::store::{Store, now_ms};

/// Largest picture, in bytes. The table enforces the same number.
pub const MAX_AVATAR_BYTES: usize = 256 * 1024;

/// A stored picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Avatar {
    pub mime: &'static str,
    pub data: Vec<u8>,
    pub updated_at: i64,
}

/// The type of a picture, decided by its first bytes and never by a file name or a
/// header the sender chose. SVG and GIF are refused on purpose: SVG can carry script.
pub fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// Whole years between `born` and `today`, both as `YYYY-MM-DD`. `None` if either is not a
/// date. A birthday that is still to come this year has not been celebrated yet.
pub fn age_on(born: &str, today: &str) -> Option<i64> {
    let born: jiff::civil::Date = born.parse().ok()?;
    let today: jiff::civil::Date = today.parse().ok()?;
    let mut years = i64::from(today.year()) - i64::from(born.year());
    if (today.month(), today.day()) < (born.month(), born.day()) {
        years -= 1;
    }
    Some(years)
}

impl Store {
    /// Today's date in the time zone of the server, as `YYYY-MM-DD`.
    pub async fn today(&self) -> Result<String> {
        let zone = self.setting("time_zone").await?;
        let now = jiff::Timestamp::now();
        let zoned = match zone.as_deref().map(|z| now.in_tz(z)) {
            Some(Ok(z)) => z,
            _ => now.to_zoned(jiff::tz::TimeZone::system()),
        };
        Ok(zoned.strftime("%Y-%m-%d").to_string())
    }

    /// Sets or clears the birthday. With a birthday the age limit of the user is its
    /// age today. Clearing it leaves the limit where it is. An administrator's call:
    /// a child must not be able to move their own birthday.
    pub async fn set_birth_date(&self, user_id: i64, born: Option<&str>) -> Result<()> {
        let Some(born) = born else {
            self.db
                .writer
                .execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE profile SET birth_date = NULL WHERE id = ?", [Value::from(user_id)]))
                .await?;
            return Ok(());
        };
        let today = self.today().await?;
        let age = age_on(born, &today).filter(|a| (0..=120).contains(a)).ok_or(Error::InvalidBirthDate)?;
        let values: [Value; 3] = [born.to_owned().into(), age.into(), user_id.into()];
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE profile SET birth_date = ?, max_age = ? WHERE id = ?", values))
            .await?;
        Ok(())
    }

    /// Renews the age limit of everybody with a birthday. Returns how many changed.
    pub async fn refresh_ages(&self) -> Result<u64> {
        let today = self.today().await?;
        self.refresh_ages_on(&today).await
    }

    /// The same for a given day (`YYYY-MM-DD`), so that it can be tested.
    pub async fn refresh_ages_on(&self, today: &str) -> Result<u64> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_string(DbBackend::Sqlite, "SELECT id, birth_date, max_age FROM profile WHERE birth_date IS NOT NULL"))
            .await?;
        let mut changed = 0;
        for row in rows {
            let id: i64 = row.try_get_by_index(0)?;
            let born: String = row.try_get_by_index(1)?;
            let current: Option<i64> = row.try_get_by_index(2)?;
            let Some(age) = age_on(&born, today).map(|a| a.max(0)) else { continue };
            if current != Some(age) {
                self.db
                    .writer
                    .execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE profile SET max_age = ? WHERE id = ?", [Value::from(age), Value::from(id)]))
                    .await?;
                changed += 1;
            }
        }
        Ok(changed)
    }

    /// Sets the nickname (what is shown and what the user signs in with), the first
    /// name and the last name. The nickname must be free; the others are optional
    /// and may be empty. Used by the user themself and by an administrator.
    pub async fn set_person_names(&self, user_id: i64, nickname: &str, first_name: &str, last_name: &str) -> Result<()> {
        let (nickname, first, last) = (nickname.trim(), first_name.trim(), last_name.trim());
        if nickname.is_empty() || [nickname, first, last].iter().any(|n| n.chars().count() > 80) {
            return Err(Error::InvalidName);
        }
        let optional = |n: &str| if n.is_empty() { Value::from(None::<String>) } else { Value::from(n.to_owned()) };
        let values: [Value; 4] = [nickname.to_owned().into(), optional(first), optional(last), user_id.into()];
        let done = self
            .db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE profile SET name = ?, first_name = ?, last_name = ? WHERE id = ?",
                values,
            ))
            .await
            .map_err(Error::from_db)?;
        if done.rows_affected() == 0 {
            return Err(Error::Db(sea_orm::DbErr::RecordNotUpdated));
        }
        Ok(())
    }

    /// Is this nickname free for this user (every other user has another)? The comparison
    /// ignores case, like the unique index.
    pub async fn nickname_free(&self, nickname: &str, user_id: i64) -> Result<bool> {
        Ok(self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 FROM profile WHERE name = ? COLLATE NOCASE AND id <> ?",
                [Value::from(nickname.trim().to_owned()), Value::from(user_id)],
            ))
            .await?
            .is_none())
    }

    /// The language a user chose: `en`, `de`, or `None` to follow the browser or device.
    pub async fn set_language(&self, user_id: i64, language: Option<&str>) -> Result<()> {
        if language.is_some_and(|l| !matches!(l, "en" | "de")) {
            return Err(Error::InvalidName);
        }
        let values: [Value; 2] = [language.map(str::to_owned).into(), user_id.into()];
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE profile SET language = ? WHERE id = ?", values))
            .await?;
        Ok(())
    }

    /// `(share_listening, may_hide_listening)` of a user; `(true, true)` for an unknown one.
    pub async fn listening_sharing(&self, user_id: i64) -> Result<(bool, bool)> {
        let values: [Value; 1] = [user_id.into()];
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT share_listening, may_hide_listening FROM profile WHERE id = ?", values))
            .await?;
        Ok(match row {
            Some(r) => (r.try_get_by_index::<i64>(0)? != 0, r.try_get_by_index::<i64>(1)? != 0),
            None => (true, true),
        })
    }

    /// The person's own choice whether the family sees their listening. The caller checks `may_hide_listening` before it accepts `false`.
    pub async fn set_share_listening(&self, user_id: i64, share: bool) -> Result<()> {
        let values: [Value; 2] = [i64::from(share).into(), user_id.into()];
        self.db.writer.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE profile SET share_listening = ? WHERE id = ?", values)).await?;
        Ok(())
    }

    /// The decision of an administrator whether the person may hide their listening.
    pub async fn set_may_hide_listening(&self, user_id: i64, may: bool) -> Result<()> {
        let values: [Value; 2] = [i64::from(may).into(), user_id.into()];
        self.db.writer.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE profile SET may_hide_listening = ? WHERE id = ?", values)).await?;
        Ok(())
    }

    pub async fn set_avatar(&self, user_id: i64, bytes: &[u8]) -> Result<()> {
        if bytes.len() > MAX_AVATAR_BYTES {
            return Err(Error::AvatarSize);
        }
        let mime = sniff_image(bytes).ok_or(Error::AvatarType)?;
        let values: [Value; 4] = [user_id.into(), mime.to_owned().into(), bytes.to_vec().into(), now_ms().into()];
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO avatar (profile_id, mime, data, updated_at) VALUES (?, ?, ?, ?) \
                 ON CONFLICT (profile_id) DO UPDATE SET mime = excluded.mime, data = excluded.data, \
                 updated_at = excluded.updated_at",
                values,
            ))
            .await?;
        Ok(())
    }

    pub async fn remove_avatar(&self, user_id: i64) -> Result<()> {
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "DELETE FROM avatar WHERE profile_id = ?",
                [Value::from(user_id)],
            ))
            .await?;
        Ok(())
    }

    pub async fn avatar(&self, user_id: i64) -> Result<Option<Avatar>> {
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT mime, data, updated_at FROM avatar WHERE profile_id = ?",
                [Value::from(user_id)],
            ))
            .await?;
        let Some(row) = row else { return Ok(None) };
        let mime: String = row.try_get_by_index(0)?;
        Ok(Some(Avatar {
            mime: match mime.as_str() {
                "image/png" => "image/png",
                "image/jpeg" => "image/jpeg",
                _ => "image/webp",
            },
            data: row.try_get_by_index(1)?,
            updated_at: row.try_get_by_index(2)?,
        }))
    }

    /// The version of the picture (when it was stored), `None` without a picture. The apps load it again when it changed.
    pub async fn avatar_version(&self, user_id: i64) -> Result<Option<i64>> {
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT updated_at FROM avatar WHERE profile_id = ?", [Value::from(user_id)]))
            .await?;
        row.map(|r| Ok(r.try_get_by_index(0)?)).transpose()
    }

    /// The device a session token belongs to; `None` for a session of the administration pages.
    pub async fn session_device(&self, token: &str) -> Result<Option<i64>> {
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT device_id FROM session WHERE token_hash = ?",
                [Value::from(crate::auth::sha256_hex(token))],
            ))
            .await?;
        Ok(match row {
            Some(r) => r.try_get_by_index::<Option<i64>>(0)?,
            None => None,
        })
    }

    pub async fn has_avatar(&self, user_id: i64) -> Result<bool> {
        Ok(self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 FROM avatar WHERE profile_id = ?",
                [Value::from(user_id)],
            ))
            .await?
            .is_some())
    }

    /// Changes the PIN of the user themself: the current PIN must be right (wrong
    /// ones count like wrong sign-ins and can lock the user). The sessions of the
    /// browser other than `keep_session` end; paired devices keep theirs.
    pub async fn change_pin(&self, user_id: i64, current: &str, new: &str, keep_session: &str) -> Result<()> {
        if !valid_secret(new) {
            return Err(Error::InvalidPin);
        }
        let profile = self.profile_by_id(user_id).await?.ok_or(Error::WrongPin)?;
        match self.verify_login(&profile.name, current).await? {
            Login::Ok { user_id: id, .. } if id == user_id => {}
            Login::Locked { .. } => return Err(Error::PinLocked),
            _ => return Err(Error::WrongPin),
        }
        self.set_pin(user_id, new).await?;
        let values: [Value; 2] = [user_id.into(), crate::auth::sha256_hex(keep_session).into()];
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "DELETE FROM session WHERE user_id = ? AND device_id IS NULL AND token_hash <> ?",
                values,
            ))
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::age_on;

    #[test]
    fn the_age_counts_whole_years_and_turns_on_the_birthday() {
        assert_eq!(age_on("2018-10-27", "2026-10-26"), Some(7), "the day before the 8th birthday");
        assert_eq!(age_on("2018-10-27", "2026-10-27"), Some(8), "on it");
        assert_eq!(age_on("2018-10-27", "2027-01-01"), Some(8));
        assert_eq!(age_on("2000-02-29", "2026-02-28"), Some(25), "a leap day is celebrated from 1 March");
        assert_eq!(age_on("2000-02-29", "2026-03-01"), Some(26));
        assert_eq!(age_on("2030-01-01", "2026-10-27"), Some(-4), "the future gives a negative age, which callers refuse");
        assert_eq!(age_on("not a date", "2026-10-27"), None);
        assert_eq!(age_on("2018-13-40", "2026-10-27"), None);
    }
}
