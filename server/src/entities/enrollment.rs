// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "enrollment")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub code_hash: String,
    pub user_id: i64,
    pub shared: bool,
    pub created_by: i64,
    pub expires_at: i64,
    pub used_at: Option<i64>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
