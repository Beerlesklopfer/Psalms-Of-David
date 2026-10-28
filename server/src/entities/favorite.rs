// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "favorite")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub profile_id: i64,
    #[sea_orm(primary_key, auto_increment = false)]
    pub book_id: i64,
    pub added_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
