-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0041: a folder for every member of the family and every kind of medium (project owner, 2026-10-29): where the audiobooks, the music
-- and the pictures of that person live. Chosen in the window of a profile with a folder dialog that lists only folders the server can read, write and
-- enter (mode 0777 for the user it runs as). The path is what an administrator chose; the server checks it when it is saved and again when it is used.
-- See docs/spec-admin-ui.md ("folders of a profile").
CREATE TABLE profile_folder (
    id         INTEGER PRIMARY KEY,
    profile_id INTEGER NOT NULL REFERENCES profile (id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN ('audiobook', 'music', 'photo')),
    path       TEXT NOT NULL CHECK (length(path) >= 1),
    created_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0,
    updated_by TEXT
) STRICT;
CREATE UNIQUE INDEX profile_folder_kind ON profile_folder (profile_id, kind);
CREATE TRIGGER audit_profile_folder_ai AFTER INSERT ON profile_folder FOR EACH ROW BEGIN
    UPDATE profile_folder SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_profile_folder_au AFTER UPDATE ON profile_folder FOR EACH ROW BEGIN
    UPDATE profile_folder SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;
