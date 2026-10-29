-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0035: playlists (project owner, 2026-10-29). See docs/spec-playlists.md.
-- A playlist belongs to exactly one profile and holds tracks in an order; the apps build the .m3u8 themselves.
-- The longest name is checked by the program (`playlists::NAME_MAX`), the case-insensitive uniqueness here.
-- Items are numbered 1..n without gaps (the program renumbers after each change); the same track may appear twice.
-- A track or book that leaves the library takes its items with it; a profile takes its playlists.
CREATE TABLE playlist (
    id          INTEGER PRIMARY KEY,
    profile_id  INTEGER NOT NULL REFERENCES profile (id) ON DELETE CASCADE,
    name        TEXT    NOT NULL CHECK (length(name) >= 1 AND name = trim(name)),
    created_at  INTEGER NOT NULL DEFAULT 0,
    created_by  TEXT,
    updated_at  INTEGER NOT NULL DEFAULT 0,
    updated_by  TEXT
) STRICT;
CREATE UNIQUE INDEX playlist_profile_name ON playlist (profile_id, name COLLATE NOCASE);
CREATE TRIGGER audit_playlist_ai AFTER INSERT ON playlist FOR EACH ROW BEGIN
    UPDATE playlist SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_playlist_au AFTER UPDATE ON playlist FOR EACH ROW BEGIN
    UPDATE playlist SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;

CREATE TABLE playlist_item (
    id          INTEGER PRIMARY KEY,
    playlist_id INTEGER NOT NULL REFERENCES playlist (id) ON DELETE CASCADE,
    position    INTEGER NOT NULL CHECK (position >= 1),
    track_id    INTEGER NOT NULL REFERENCES track (id) ON DELETE CASCADE,
    created_at  INTEGER NOT NULL DEFAULT 0,
    created_by  TEXT,
    updated_at  INTEGER NOT NULL DEFAULT 0,
    updated_by  TEXT
) STRICT;
CREATE INDEX playlist_item_order ON playlist_item (playlist_id, position, id);
CREATE INDEX playlist_item_track ON playlist_item (track_id);
CREATE TRIGGER audit_playlist_item_ai AFTER INSERT ON playlist_item FOR EACH ROW BEGIN
    UPDATE playlist_item SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_playlist_item_au AFTER UPDATE ON playlist_item FOR EACH ROW BEGIN
    UPDATE playlist_item SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;
