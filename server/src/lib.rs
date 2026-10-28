// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! David server core: library, profiles and listening progress on SQLite.

pub mod admin;
pub mod api;
pub mod app;
pub mod auth;
pub mod bible;
pub mod cli;
pub mod db;
pub mod discovery;
pub mod entities;
pub mod error;
pub mod importer;
pub mod pairing;
pub mod platform;
pub mod profile;
pub mod roles;
pub mod scan;
pub mod store;
pub mod tags;
pub mod diskspace;
pub mod upload;
pub mod tls;
pub mod web;
pub mod writer;

pub use bible::{Anchor, BibleHit, Reference};
pub use error::{Error, MAX_PROFILES, Result};
pub use importer::{ImportState, RatingRule};
pub use pairing::{DeviceRow, Enrollment, PairedDevice};
pub use roles::{Role, Schedule};
pub use scan::{FoundBook, FoundTrack, ScanReport};
pub use store::{UploadJob, Access, AdminBookRow, BookRow, ContinueEntry, NewDevice, NewProfile, Overview, Store, UserRow, now_ms};
pub use writer::{ProgressUpdate, WriterStats};
