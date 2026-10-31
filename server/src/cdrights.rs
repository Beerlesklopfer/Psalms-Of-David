// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The three rights a profile has for CDs (docs/spec-cd-inbox.md, "Rights per person"): to put in CDs, to need no approval for them, and
//! to approve the CDs of others. They are columns of the profile (migration 0058); the database refuses what must never be: a right for a
//! child or a guest, and a state in which nobody could approve.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::Serialize;

use crate::error::{Error, Result};
use crate::roles::Role;
use crate::store::Store;

/// What one profile may do with CDs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct CdRights {
    /// May start a rip from an app or the web, and is told about their own CDs.
    pub may_insert: bool,
    /// A CD of theirs goes into the library when it is read; otherwise it waits for approval.
    pub no_approval: bool,
    /// May decide the CDs of others.
    pub may_approve: bool,
}

impl CdRights {
    /// The rights of a role that has none: children and guests.
    pub fn none() -> CdRights {
        CdRights::default()
    }

    /// The rights an administrator starts with (and a profile that is made one).
    pub fn all() -> CdRights {
        CdRights {
            may_insert: true,
            no_approval: true,
            may_approve: true,
        }
    }
}

/// A change of the rights; a field that is `None` stays as it is.
#[derive(Debug, Clone, Copy, Default)]
pub struct CdRightsChange {
    pub may_insert: Option<bool>,
    pub no_approval: Option<bool>,
    pub may_approve: Option<bool>,
}

impl Store {
    /// The rights of a profile; none for a profile that does not exist, and none for a child or guest whatever the columns say.
    pub async fn cd_rights(&self, profile: i64) -> Result<CdRights> {
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT role, cd_may_insert, cd_no_approval, cd_may_approve FROM profile WHERE id = ?", [profile.into()]))
            .await?;
        let Some(row) = row else {
            return Ok(CdRights::none());
        };
        let role: String = row.try_get_by_index(0)?;
        if !matches!(role.parse::<Role>(), Ok(Role::Admin | Role::Member)) {
            return Ok(CdRights::none());
        }
        Ok(CdRights {
            may_insert: row.try_get_by_index::<i64>(1)? != 0,
            no_approval: row.try_get_by_index::<i64>(2)? != 0,
            may_approve: row.try_get_by_index::<i64>(3)? != 0,
        })
    }

    /// Sets rights. The database refuses a right for a child or guest (`CdRightsRole`) and taking `may_approve` from the last profile that
    /// has it (`LastCdApprover`); nothing changes then.
    pub async fn set_cd_rights(&self, profile: i64, change: CdRightsChange) -> Result<()> {
        let mut sets: Vec<&str> = Vec::new();
        let mut values: Vec<sea_orm::Value> = Vec::new();
        for (column, value) in [
            ("cd_may_insert", change.may_insert),
            ("cd_no_approval", change.no_approval),
            ("cd_may_approve", change.may_approve),
        ] {
            if let Some(v) = value {
                sets.push(column);
                values.push(i64::from(v).into());
            }
        }
        if sets.is_empty() {
            return Ok(());
        }
        let assignments = sets
            .iter()
            .map(|c| format!("{c} = ?"))
            .collect::<Vec<_>>()
            .join(", ");
        values.push(profile.into());
        let done = self
            .db
            .writer
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("UPDATE profile SET {assignments} WHERE id = ?"),
                values,
            ))
            .await
            .map_err(Error::from_db)?;
        if done.rows_affected() == 0 {
            return Err(Error::Db(sea_orm::DbErr::RecordNotUpdated));
        }
        Ok(())
    }

    /// The profiles that may approve CDs, with their nicknames (the people a task goes to).
    pub async fn cd_approvers(&self) -> Result<Vec<(i64, String)>> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_string(DbBackend::Sqlite, "SELECT id, name FROM profile WHERE cd_may_approve = 1 AND role IN ('admin', 'member') ORDER BY name"))
            .await?;
        rows.iter()
            .map(|r| Ok((r.try_get_by_index(0)?, r.try_get_by_index(1)?)))
            .collect()
    }

    /// The nickname of a profile (for the log of decisions).
    pub async fn profile_name(&self, id: i64) -> Result<Option<String>> {
        let row = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT name FROM profile WHERE id = ?",
                [id.into()],
            ))
            .await?;
        row.map(|r| Ok(r.try_get_by_index(0)?)).transpose()
    }
}
