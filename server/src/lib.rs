// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! David server core: library, profiles and listening progress on SQLite.
// The list of the tools of the MCP interface is one `json!` literal; it grew past the default depth of the macro.
#![recursion_limit = "512"]

pub mod access;
pub mod actor;
pub mod admin;
pub mod api;
pub mod api_feeds;
pub mod api_me;
pub mod api_playlists;
pub mod audioformat;
pub mod app;
pub mod auth;
pub mod bible;
pub mod cd_settings;
pub mod musicbrainz;
pub mod cdflow;
pub mod inbox;
pub mod api_inbox;
pub mod cdimport;
pub mod cdrights;
pub mod cdrive;
pub mod cdpresent;
pub mod cdresume;
pub mod ws;
pub mod api_cd;
pub mod buildinfo;
pub mod cli;
pub mod db;
pub mod discovery;
pub mod features;
pub mod feeds;
pub mod fetch;
pub mod filing;
pub mod folders;
pub mod genres;
pub mod gtin;
pub mod hidden;
pub mod blacklist;
pub mod labels;
pub mod placeholders;
pub mod playlists;
pub mod entities;
pub mod error;
pub mod ignore;
pub mod importer;
pub mod logging;
pub mod mcp;
pub mod pairing;
pub mod ntacl;
pub mod platform;
pub mod plays;
pub mod podcast;
#[cfg(target_os = "linux")]
pub mod posixacl;
pub mod quarantine;
pub mod relocate;
pub mod profile;
pub mod prune;
pub mod roles;
pub mod scan;
pub mod search;
pub mod store;
pub mod transcode;
pub mod system;
pub mod tags;
pub mod diskspace;
pub mod dupes;
pub mod upload;
pub mod tls;
pub mod tree;
pub mod web;
pub mod writer;
pub mod zipstream;

pub use bible::{Anchor, BibleHit, Reference};
pub use error::{Error, MAX_PROFILES, Result};
pub use importer::{ImportState, RatingRule};
pub use pairing::{DeviceRow, Enrollment, PairedDevice};
pub use roles::{Role, Schedule};
pub use scan::{FoundBook, FoundTrack, ScanReport};
pub use store::{McpToken, TagChange, UploadJob, Access, AdminBookRow, BookRow, ContinueEntry, FamilyListening, MediaTime, MemberTime, NewDevice, NewProfile, Overview, Store, UserRow, now_ms};
pub use writer::{ProgressUpdate, WriterStats};
