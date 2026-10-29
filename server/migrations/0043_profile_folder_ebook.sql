-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0043: the folders of a profile get a fourth kind of medium, the e-books (project owner, 2026-10-29: "pictures and e-books are missing").
-- A CHECK cannot be changed in SQLite, so the table is made again with the same columns, audit fields and triggers.
CREATE TABLE profile_folder_new (
    id         INTEGER PRIMARY KEY,
    profile_id INTEGER NOT NULL REFERENCES profile (id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN ('audiobook', 'music', 'photo', 'ebook')),
    path       TEXT NOT NULL CHECK (length(path) >= 1),
    created_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0,
    updated_by TEXT
) STRICT;
INSERT INTO profile_folder_new (id, profile_id, kind, path, created_at, created_by, updated_at, updated_by)
    SELECT id, profile_id, kind, path, created_at, created_by, updated_at, updated_by FROM profile_folder;
DROP TABLE profile_folder;
ALTER TABLE profile_folder_new RENAME TO profile_folder;
CREATE UNIQUE INDEX profile_folder_kind ON profile_folder (profile_id, kind);
CREATE TRIGGER audit_profile_folder_ai AFTER INSERT ON profile_folder FOR EACH ROW BEGIN
    UPDATE profile_folder SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_profile_folder_au AFTER UPDATE ON profile_folder FOR EACH ROW BEGIN
    UPDATE profile_folder SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;
