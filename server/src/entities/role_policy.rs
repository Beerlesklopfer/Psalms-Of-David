// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "role_policy")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub role: String,
    pub max_devices: i64,
    pub session_ttl_s: i64,
    pub step_up_s: i64,
    pub confirmed: bool,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
