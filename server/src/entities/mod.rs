// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Entities follow `migrations/0001_init.sql`. Joins and progress writes run as
//! SQL (see `store.rs` and `writer.rs`); simple access goes through the entities.
pub mod author;
pub mod book;
pub mod book_release;
pub mod device;
pub mod enrollment;
pub mod favorite;
pub mod profile;
pub mod progress;
pub mod role_policy;
pub mod server_identity;
pub mod session;
pub mod setting;
pub mod track;
