// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "profile")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub name: String,
    pub avatar: Option<String>,
    pub pin_hash: Option<String>,
    pub is_admin: bool,
    pub max_age: Option<i64>,
    pub created_at: i64,
    pub role: String,
    pub max_devices: Option<i64>,
    pub expires_at: Option<i64>,
    pub schedule: Option<Vec<u8>>,
    pub pin_failures: i64,
    pub pin_locked_until: Option<i64>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub language: Option<String>,
    pub birth_date: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
