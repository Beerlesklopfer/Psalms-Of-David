// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "track")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub book_id: i64,
    pub disc: i64,
    pub number: i64,
    pub title: String,
    pub file: String,
    pub duration_ms: i64,
    pub size_bytes: i64,
    pub mtime: i64,
    /// The container/file kind (`mp3`, `ogg`, `flac`, `m4a`, ...); empty until an import has looked at the file.
    pub format: String,
    pub codec: String,
    pub mime_type: String,
    pub bitrate_kbps: i64,
    pub sample_rate: i64,
    pub channels: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
