-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0016: an uploaded archive is audiobooks or music; the kind decides which import
-- folder it is unpacked into.
ALTER TABLE upload_job ADD COLUMN kind TEXT NOT NULL DEFAULT 'audiobook' CHECK (kind IN ('audiobook', 'music'));
