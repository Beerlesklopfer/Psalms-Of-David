-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0061: the hash of a track file, so that the app can check a download (project owner, 2026-11-01).
--   sha256         the SHA-256 of the file as hex; empty until the first download asks for it (it is made then, not at the scan).
--   hashed_mtime / hashed_size   the modification time (seconds) and the size the file had when it was hashed: a file that was
--                  changed since (tags written) is hashed again, so the app never gets the hash of an older file.
ALTER TABLE track ADD COLUMN sha256 TEXT NOT NULL DEFAULT '';
ALTER TABLE track ADD COLUMN hashed_mtime INTEGER NOT NULL DEFAULT 0;
ALTER TABLE track ADD COLUMN hashed_size INTEGER NOT NULL DEFAULT 0;
