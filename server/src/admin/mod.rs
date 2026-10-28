// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The administration interface: pages rendered on the server from HTML
//! templates (askama). This module has no HTTP layer yet; it renders pages to
//! strings from the data of the `Store`. See `docs/spec-admin-ui.md`.

pub mod i18n;
pub mod messages;
pub mod pages;
