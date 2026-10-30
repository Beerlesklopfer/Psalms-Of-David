// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! PINs, sign-in and sessions of the administration interface.
//!
//! - PINs are stored as Argon2id hashes. Too many wrong PINs lock the user.
//! - A session token is 256 random bits. Only its SHA-256 is stored.
//! - The CSRF token of a session is derived from the session token, so it needs no
//!   column of its own and is unknown to anyone who cannot read the cookie.

use argon2::Argon2;
use argon2::password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash};
use sea_orm::{ConnectionTrait, DbBackend, EntityTrait, Statement, Value};
use sha2::{Digest, Sha256};

use crate::entities::{profile, session};
use crate::error::{Error, Result};
use crate::roles::Role;
use crate::store::{Store, now_ms};

/// A PIN is 4 to 12 digits. (Proposal; the owner has not set a rule yet.)
pub const PIN_MIN_LEN: usize = 4;
pub const PIN_MAX_LEN: usize = 12;
pub const PASSWORD_MIN_LEN: usize = 8;
pub const PASSWORD_MAX_LEN: usize = 64;

/// Result of a sign-in attempt. Wrong name and wrong PIN are not told apart, so
/// the form cannot be used to find out which names exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Login {
    Ok { user_id: i64, role: Role },
    WrongCredentials,
    /// Too many wrong PINs; try again after this time (Unix milliseconds).
    Locked { until: i64 },
}

/// Who a valid session belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub user_id: i64,
    pub role: Role,
    pub expires_at: i64,
}

/// A password is alphanumeric (project owner, 2026-10-28: "PINs are numbers, a password is
/// alphanumeric"): 8 to 64 characters with at least one letter and one digit. Other
/// characters (spaces inside, symbols, umlauts) are allowed; control characters are not.
pub fn valid_password(password: &str) -> bool {
    let n = password.chars().count();
    (PASSWORD_MIN_LEN..=PASSWORD_MAX_LEN).contains(&n)
        && password.chars().all(|c| !c.is_control())
        && password.chars().any(char::is_alphabetic)
        && password.chars().any(|c| c.is_ascii_digit())
}

/// What a user may sign in with: a PIN (4 to 12 digits) or a password.
pub fn valid_secret(secret: &str) -> bool {
    valid_pin(secret) || valid_password(secret)
}

pub fn valid_pin(pin: &str) -> bool {
    (PIN_MIN_LEN..=PIN_MAX_LEN).contains(&pin.len()) && pin.bytes().all(|b| b.is_ascii_digit())
}

pub(crate) fn sha256_hex(data: &str) -> String {
    Sha256::digest(data.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// The CSRF token that belongs to a session token.
pub fn csrf_token(session_token: &str) -> String {
    sha256_hex(&format!("david-csrf:{session_token}"))
}

/// Compares in constant time, so the comparison does not leak how much matched.
pub fn same_secret(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = (a.len() ^ b.len()) as u8;
    for i in 0..a.len().max(b.len()) {
        diff |= a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0);
    }
    diff == 0
}

pub(crate) fn new_token() -> String {
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

fn hash_pin(pin: &str) -> Result<String> {
    Argon2::default()
        .hash_password(pin.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| Error::WriteFailed(format!("cannot hash the PIN: {e}")))
}

fn pin_matches(pin: &str, stored: &str) -> bool {
    PasswordHash::new(stored)
        .map(|parsed| Argon2::default().verify_password(pin.as_bytes(), &parsed).is_ok())
        .unwrap_or(false)
}

impl Store {
    /// Sets or replaces the PIN or password of a user and clears a lockout.
    pub async fn set_pin(&self, user_id: i64, pin: &str) -> Result<()> {
        if !valid_secret(pin) {
            return Err(Error::InvalidPin);
        }
        let hash = hash_pin(pin)?;
        let values: [Value; 2] = [hash.into(), user_id.into()];
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE profile SET pin_hash = ?, pin_failures = 0, pin_locked_until = NULL WHERE id = ?",
                values,
            ))
            .await?;
        Ok(())
    }

    /// Checks a name and a PIN. Counts failures and locks the user after too many.
    pub async fn verify_login(&self, name: &str, pin: &str) -> Result<Login> {
        let now = now_ms();
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, role, pin_hash, pin_locked_until, expires_at FROM profile WHERE name = ?",
                [Value::from(name.to_owned())],
            ))
            .await?;
        let Some(row) = row else {
            // Take about as long as for a real user, so timing does not tell names apart.
            let _ = pin_matches(pin, dummy_hash());
            return Ok(Login::WrongCredentials);
        };
        let id: i64 = row.try_get_by_index(0)?;
        let role: String = row.try_get_by_index(1)?;
        let stored: Option<String> = row.try_get_by_index(2)?;
        let locked_until: Option<i64> = row.try_get_by_index(3)?;
        let expires_at: Option<i64> = row.try_get_by_index(4)?;

        if let Some(until) = locked_until.filter(|&u| u > now) {
            return Ok(Login::Locked { until });
        }
        if expires_at.is_some_and(|e| e <= now) {
            return Ok(Login::WrongCredentials);
        }
        let Some(stored) = stored else {
            let _ = pin_matches(pin, dummy_hash());
            return Ok(Login::WrongCredentials);
        };

        if pin_matches(pin, &stored) {
            self.db
                .writer
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE profile SET pin_failures = 0, pin_locked_until = NULL WHERE id = ?",
                    [Value::from(id)],
                ))
                .await?;
            let role = role.parse().map_err(|e: String| Error::WriteFailed(e))?;
            return Ok(Login::Ok { user_id: id, role });
        }

        let max: i64 = self.setting("pin_max_failures").await?.and_then(|v| v.parse().ok()).unwrap_or(5);
        let lock_s: i64 = self.setting("pin_lock_s").await?.and_then(|v| v.parse().ok()).unwrap_or(900);
        // One statement, so concurrent attempts cannot both be counted as the first.
        let values: [Value; 4] = [max.into(), (now + lock_s * 1000).into(), max.into(), id.into()];
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE profile SET \
                   pin_locked_until = CASE WHEN pin_failures + 1 >= ? THEN ? ELSE pin_locked_until END, \
                   pin_failures = CASE WHEN pin_failures + 1 >= ? THEN 0 ELSE pin_failures + 1 END \
                 WHERE id = ?",
                values,
            ))
            .await?;
        let locked: Option<i64> = self
            .db
            .writer
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT pin_locked_until FROM profile WHERE id = ?",
                [Value::from(id)],
            ))
            .await?
            .and_then(|r| r.try_get_by_index(0).ok());
        match locked.filter(|&u| u > now) {
            Some(until) => Ok(Login::Locked { until }),
            None => Ok(Login::WrongCredentials),
        }
    }

    /// Opens a session and returns its token. The lifetime comes from the policy
    /// of the role. `device_id` is `None` for the administration interface.
    pub async fn create_session(&self, user_id: i64, role: Role, device_id: Option<i64>) -> Result<String> {
        let ttl_s = self
            .role_policies()
            .await?
            .into_iter()
            .find(|p| p.role == role.as_str())
            .map_or(86_400, |p| p.session_ttl_s);
        let token = new_token();
        let now = now_ms();
        let values: [Value; 6] = [
            sha256_hex(&token).into(),
            user_id.into(),
            device_id.into(),
            role.as_str().to_owned().into(),
            now.into(),
            (now + ttl_s * 1000).into(),
        ];
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO session (token_hash, user_id, device_id, role, created_at, last_used, expires_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
                [values[0].clone(), values[1].clone(), values[2].clone(), values[3].clone(), values[4].clone(), values[4].clone(), values[5].clone()],
            ))
            .await?;
        // A paired device is seen when it signs in (afterwards `session_for_token` renews it once a minute).
        if let Some(device) = device_id {
            self.db
                .writer
                .execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE device SET last_seen = ? WHERE id = ?", [Value::from(now), Value::from(device)]))
                .await?;
        }
        Ok(token)
    }

    /// The user behind a token, if the session exists and has not expired. The
    /// role is read from the user again, so a demotion takes effect at once.
    pub async fn session_for_token(&self, token: &str) -> Result<Option<SessionInfo>> {
        let now = now_ms();
        let hash = sha256_hex(token);
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT s.user_id, p.role, s.expires_at, s.last_used, p.expires_at \
                 FROM session s JOIN profile p ON p.id = s.user_id WHERE s.token_hash = ?",
                [Value::from(hash.clone())],
            ))
            .await?;
        let Some(row) = row else { return Ok(None) };
        let user_id: i64 = row.try_get_by_index(0)?;
        let role: String = row.try_get_by_index(1)?;
        let expires_at: i64 = row.try_get_by_index(2)?;
        let last_used: i64 = row.try_get_by_index(3)?;
        let user_expires: Option<i64> = row.try_get_by_index(4)?;
        if expires_at <= now || user_expires.is_some_and(|e| e <= now) {
            return Ok(None);
        }
        // Record use at most once a minute, so a page view is not a write. The paired device of the session shows "last seen" from it
        // (the profile page and the administration list the devices with that time); a browser session has no device.
        if now - last_used > 60_000 {
            self.db
                .writer
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE device SET last_seen = ? WHERE id = (SELECT device_id FROM session WHERE token_hash = ?)",
                    [Value::from(now), Value::from(hash.clone())],
                ))
                .await?;
            self.db
                .writer
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE session SET last_used = ? WHERE token_hash = ?",
                    [Value::from(now), Value::from(hash)],
                ))
                .await?;
        }
        let role = role.parse().map_err(|e: String| Error::WriteFailed(e))?;
        Ok(Some(SessionInfo { user_id, role, expires_at }))
    }

    /// Shortens the life of a session to at most `max_ms` from now.
    pub async fn limit_session(&self, token: &str, max_ms: i64) -> Result<()> {
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE session SET expires_at = min(expires_at, ?) WHERE token_hash = ?",
                [Value::from(now_ms() + max_ms), Value::from(sha256_hex(token))],
            ))
            .await?;
        Ok(())
    }

    pub async fn delete_session(&self, token: &str) -> Result<()> {
        session::Entity::delete_by_id(sha256_hex(token)).exec(&self.db.writer).await?;
        Ok(())
    }

    /// Removes expired sessions; returns how many.
    pub async fn purge_expired_sessions(&self) -> Result<u64> {
        let res = self
            .db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "DELETE FROM session WHERE expires_at <= ?",
                [Value::from(now_ms())],
            ))
            .await?;
        Ok(res.rows_affected())
    }

    /// Is there at least one admin? `david init` creates the first one.
    pub async fn has_admin(&self) -> Result<bool> {
        Ok(profile::Entity::find()
            .all(&self.db.reader)
            .await?
            .iter()
            .any(|p| p.role == "admin"))
    }
}

/// A real hash of a PIN nobody knows, made once. Checking against it takes as long
/// as a real check, so unknown names cannot be told apart by timing.
fn dummy_hash() -> &'static str {
    static HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HASH.get_or_init(|| hash_pin(&new_token()).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pin_is_four_to_twelve_digits() {
        assert!(valid_pin("1234") && valid_pin("123456789012"));
        assert!(!valid_pin("123") && !valid_pin("1234567890123"));
        assert!(!valid_pin("12a4") && !valid_pin("") && !valid_pin("12 34"));
    }

    #[test]
    fn a_password_is_alphanumeric_and_a_pin_is_not_a_password() {
        for good in ["abcdefg1", "Sommer2026", "correct horse 9", "Straße-7nach", &format!("a1{}", "x".repeat(62))] {
            assert!(valid_password(good), "{good}");
        }
        for bad in ["abcdefgh", "12345678", "abc123", "", "abc\n1234567", &format!("a1{}", "x".repeat(63))] {
            assert!(!valid_password(bad), "{bad:?}");
        }
        // Digits alone of PIN length are a PIN; the same digits are never a password.
        assert!(valid_secret("123456") && valid_secret("abcdefg1") && !valid_secret("12ab") && !valid_secret("abcdefgh"));
    }

    #[test]
    fn the_csrf_token_depends_on_the_session_token() {
        assert_eq!(csrf_token("abc"), csrf_token("abc"));
        assert_ne!(csrf_token("abc"), csrf_token("abd"));
        assert_eq!(csrf_token("abc").len(), 64);
        assert_ne!(csrf_token("abc"), sha256_hex("abc"), "must not equal the stored hash");
    }

    #[test]
    fn secrets_are_compared_completely() {
        assert!(same_secret("abc", "abc"));
        assert!(!same_secret("abc", "abd") && !same_secret("abc", "ab") && !same_secret("", "a"));
    }

    #[test]
    fn tokens_are_long_and_unique() {
        let (a, b) = (new_token(), new_token());
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
    }
}
