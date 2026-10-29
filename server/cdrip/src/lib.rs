// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reading an audio CD and turning it into tagged MP3 files: the drive (table of contents, tray state, eject),
//! the MusicBrainz disc id, the encoder, ID3v2.4 and the watch for the kernel's drive events. Used by the David
//! server (`server/src/cdrive.rs`); the files are copies of `audio/src/cd/` without the FFI part.
//!
//! See `docs/spec-cd-ripping.md`.

pub mod align;
pub mod discid;
pub mod drive;
pub mod encode;
pub mod id3;
pub mod rip;
pub mod ring;
pub mod uevent;
pub use cd_da_reader;
