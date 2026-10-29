// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pairing a device: an administrator creates a one-time code for a user, the new
//! device redeems it with its WireGuard public key and becomes that user's device.
//! See `docs/spec-discovery-profiles-roles.md`, sections 3 and 4.

use std::collections::HashSet;

use sea_orm::{ConnectionTrait, DbBackend, FromQueryResult, Statement, TransactionTrait, Value};
use serde::Serialize;

use crate::auth::{new_token, sha256_hex};
use crate::error::{Error, Result};
use crate::store::{Store, now_ms};

/// A code that has just been created. Only its hash is stored, so this is the one
/// moment the code can be shown.
#[derive(Debug, Clone)]
pub struct Enrollment {
    pub code: String,
    pub expires_at: i64,
    pub user_name: String,
}

/// The result of redeeming a code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairedDevice {
    pub device_id: i64,
    pub user_id: i64,
    pub tunnel_ip: String,
    pub shared: bool,
}

/// One line of the device list.
#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct DeviceRow {
    pub id: i64,
    pub user_name: String,
    pub name: String,
    pub tunnel_ip: String,
    pub shared: bool,
    pub created_at: i64,
    pub last_seen: Option<i64>,
    pub revoked_at: Option<i64>,
}

const DEVICE_ROWS: &str = "SELECT d.id, p.name AS user_name, d.name, d.tunnel_ip, d.shared, d.created_at, \
    d.last_seen, d.revoked_at FROM device d JOIN profile p ON p.id = d.user_id";

/// The address that goes into the QR code (spec section 4). `fingerprint` is the
/// SHA-256 fingerprint of the server certificate; the device accepts only a server
/// that shows exactly this certificate. It is left out for a server without TLS.
pub fn pairing_uri(server_id: &str, address: &str, code: &str, fingerprint: Option<&str>, ca: bool) -> String {
    let mut fields = vec![("v", "1"), ("id", server_id), ("addr", address)];
    if let Some(fp) = fingerprint {
        fields.push(("fp", fp));
    }
    // A certificate from a certificate authority: the device checks the name in `addr` against the
    // certificate and the usual trust, there is no fingerprint to pin (it changes with each renewal).
    if ca {
        fields.push(("ca", "1"));
    }
    fields.push(("code", code));
    let query = serde_urlencoded::to_string(fields).expect("plain strings always encode");
    format!("david://pair?{query}")
}

/// The QR code of `data` as an SVG image, with the quiet zone a reader needs.
pub fn qr_svg(data: &str) -> Result<String> {
    let code = qrcode::QrCode::new(data.as_bytes()).map_err(|e| Error::WriteFailed(format!("cannot make a QR code: {e}")))?;
    Ok(code.render::<qrcode::render::svg::Color>().min_dimensions(260, 260).quiet_zone(true).build())
}

/// Is this usable as the address of the server in a pairing code: a host name or
/// address, optionally with a port, and nothing that could break the code?
pub fn valid_address(address: &str) -> bool {
    !address.is_empty()
        && address.len() <= 255
        && address.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':' | b'[' | b']'))
}

/// A WireGuard public key is 32 bytes in base64: 43 characters and one `=`.
pub fn valid_public_key(key: &str) -> bool {
    key.len() == 44
        && key.ends_with('=')
        && key[..43].bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
        // 32 bytes leave the last base64 character with only four meaningful bits.
        && matches!(key.as_bytes()[42], b'A' | b'E' | b'I' | b'M' | b'Q' | b'U' | b'Y' | b'c' | b'g' | b'k' | b'o' | b's' | b'w' | b'0' | b'4' | b'8')
}

/// The address of the n-th device in the tunnel network `10.77.0.0/16`, avoiding
/// `.0` and `.1` of each block of 250.
fn tunnel_ip(n: usize) -> String {
    format!("10.77.{}.{}", n / 250, n % 250 + 2)
}

impl Store {
    /// How many active devices `user_id` has and how many it may have (`None`: no limit is set): the limit of the profile, else that of its role.
    pub async fn device_usage(&self, user_id: i64) -> Result<(i64, Option<i64>)> {
        let row = self
            .db
            .writer
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT (SELECT count(*) FROM device d WHERE d.user_id = p.id AND d.revoked_at IS NULL), \
                   coalesce(p.max_devices, (SELECT rp.max_devices FROM role_policy rp WHERE rp.role = p.role)) \
                 FROM profile p WHERE p.id = ?",
                [Value::from(user_id)],
            ))
            .await?
            .ok_or_else(|| Error::WriteFailed("no such user".into()))?;
        Ok((row.try_get_by_index(0)?, row.try_get_by_index(1)?))
    }

    /// Creates a one-time pairing code for `user_id`. Refuses if the user already
    /// owns as many active devices as allowed, so no code is handed out that could
    /// not be used.
    pub async fn create_enrollment(&self, user_id: i64, shared: bool, created_by: i64) -> Result<Enrollment> {
        let row = self
            .db
            .writer
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT p.name, \
                   (SELECT count(*) FROM device d WHERE d.user_id = p.id AND d.revoked_at IS NULL) >= \
                   coalesce(p.max_devices, (SELECT rp.max_devices FROM role_policy rp WHERE rp.role = p.role)) \
                 FROM profile p WHERE p.id = ?",
                [Value::from(user_id)],
            ))
            .await?
            .ok_or_else(|| Error::WriteFailed("no such user".into()))?;
        let user_name: String = row.try_get_by_index(0)?;
        let full: bool = row.try_get_by_index(1)?;
        if full {
            return Err(Error::DeviceLimitReached);
        }

        let ttl_s: i64 = self.setting("enrollment_code_ttl_s").await?.and_then(|v| v.parse().ok()).unwrap_or(900);
        let code = new_token()[..32].to_owned();
        let expires_at = now_ms() + ttl_s * 1000;
        let values: [Value; 5] = [
            sha256_hex(&code).into(),
            user_id.into(),
            i64::from(shared).into(),
            created_by.into(),
            expires_at.into(),
        ];
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO enrollment (code_hash, user_id, shared, created_by, expires_at) VALUES (?, ?, ?, ?, ?)",
                values,
            ))
            .await?;
        Ok(Enrollment { code, expires_at, user_name })
    }

    /// Redeems a code: the device becomes a device of the user the code was made
    /// for. The code works once. Unknown, used and expired codes are not told apart.
    /// If the device cannot be added (limit reached, key already in use), the code
    /// stays usable.
    pub async fn redeem_enrollment(&self, code: &str, wg_pubkey: &str, device_name: &str) -> Result<PairedDevice> {
        if !valid_public_key(wg_pubkey) {
            return Err(Error::InvalidPublicKey);
        }
        let name = device_name.trim();
        if name.is_empty() || name.chars().count() > 80 {
            return Err(Error::InvalidDeviceName);
        }
        let now = now_ms();
        let txn = self.db.writer.begin().await?;

        // Marking the code as used and checking it are one statement, so two devices
        // redeeming the same code at the same time cannot both succeed.
        let used = txn
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE enrollment SET used_at = ? WHERE code_hash = ? AND used_at IS NULL AND expires_at > ?",
                [Value::from(now), Value::from(sha256_hex(code)), Value::from(now)],
            ))
            .await?;
        if used.rows_affected() != 1 {
            return Err(Error::EnrollmentInvalid);
        }
        let row = txn
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT user_id, shared, created_by FROM enrollment WHERE code_hash = ?",
                [Value::from(sha256_hex(code))],
            ))
            .await?
            .ok_or(Error::EnrollmentInvalid)?;
        let user_id: i64 = row.try_get_by_index(0)?;
        let shared: bool = row.try_get_by_index(1)?;
        let created_by: i64 = row.try_get_by_index(2)?;

        let taken: HashSet<String> = txn
            .query_all_raw(Statement::from_string(DbBackend::Sqlite, "SELECT tunnel_ip FROM device"))
            .await?
            .into_iter()
            .filter_map(|r| r.try_get_by_index(0).ok())
            .collect();
        let tunnel_ip = (0..62_500).map(tunnel_ip).find(|ip| !taken.contains(ip)).ok_or(Error::WriteFailed("no free tunnel address".into()))?;

        let values: [Value; 7] = [
            user_id.into(),
            name.to_owned().into(),
            wg_pubkey.to_owned().into(),
            tunnel_ip.clone().into(),
            i64::from(shared).into(),
            created_by.into(),
            now.into(),
        ];
        let inserted = txn
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO device (user_id, name, wg_pubkey, tunnel_ip, shared, created_by, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
                values,
            ))
            .await
            .map_err(Error::from_db);
        let device_id: i64 = match inserted {
            Ok(Some(r)) => r.try_get_by_index(0)?,
            Ok(None) => return Err(Error::WriteFailed("device insert returned no row".into())),
            // Dropping the transaction rolls it back, which also un-uses the code.
            Err(e) => return Err(e),
        };
        txn.commit().await?;
        Ok(PairedDevice { device_id, user_id, tunnel_ip, shared })
    }

    pub async fn device_rows(&self) -> Result<Vec<DeviceRow>> {
        let stmt = Statement::from_string(DbBackend::Sqlite, format!("{DEVICE_ROWS} ORDER BY p.name, d.id"));
        Ok(DeviceRow::find_by_statement(stmt).all(&self.db.reader).await?)
    }

    pub async fn device_row(&self, id: i64) -> Result<Option<DeviceRow>> {
        let stmt = Statement::from_sql_and_values(DbBackend::Sqlite, format!("{DEVICE_ROWS} WHERE d.id = ?"), [Value::from(id)]);
        Ok(DeviceRow::find_by_statement(stmt).one(&self.db.reader).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_public_key_has_the_shape_of_32_bytes_in_base64() {
        assert!(valid_public_key("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="));
        assert!(valid_public_key("8Dc3+ZF7E6kYcQ4o1n0mGdJ4o5k0r0b0b0b0b0b0b0o="));
        for bad in ["", "short=", &"A".repeat(44), "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAB=", &format!("{}!=", "A".repeat(42))] {
            assert!(!valid_public_key(bad), "{bad:?}");
        }
    }

    #[test]
    fn the_pairing_address_carries_what_a_device_needs_and_survives_odd_input() {
        let uri = pairing_uri("0b9f-1", "192.168.24.250:8484", "abc123", None, false);
        assert_eq!(uri, "david://pair?v=1&id=0b9f-1&addr=192.168.24.250%3A8484&code=abc123");
        // With TLS the certificate fingerprint travels in the code, before the one-time code.
        let pinned = pairing_uri("0b9f-1", "192.168.24.250:8484", "abc123", Some("ab12"), false);
        assert_eq!(pinned, "david://pair?v=1&id=0b9f-1&addr=192.168.24.250%3A8484&fp=ab12&code=abc123");
        // Characters that would break a query are encoded, not passed on.
        let odd = pairing_uri("a&b=c", "h", "x y", None, false);
        assert!(odd.contains("id=a%26b%3Dc") && odd.contains("code=x+y"), "{odd}");
    }

    #[test]
    fn only_plausible_addresses_are_accepted() {
        for good in ["192.168.24.250:8484", "david.local", "my-server.example.org:443", "[fe80::1]:8484"] {
            assert!(valid_address(good), "{good}");
        }
        for bad in ["", "a b", "x/y", "http://x", "a&b", "a?b", &"x".repeat(256), "<script>"] {
            assert!(!valid_address(bad), "{bad:?}");
        }
    }

    #[test]
    fn tunnel_addresses_are_distinct_and_skip_the_network_and_gateway_hosts() {
        assert_eq!(tunnel_ip(0), "10.77.0.2");
        assert_eq!(tunnel_ip(249), "10.77.0.251");
        assert_eq!(tunnel_ip(250), "10.77.1.2");
        let all: HashSet<_> = (0..2000).map(tunnel_ip).collect();
        assert_eq!(all.len(), 2000);
    }
}
