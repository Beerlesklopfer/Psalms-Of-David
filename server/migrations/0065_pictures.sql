-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0065: the pictures to look at and the slideshow (docs/spec-pictures.md; project owner, 2026-11-08).
--   setting photo_library   the folder of the family's pictures (e.g. /srv/Bilder): every folder in it that is not the picture folder of
--                           a profile is an old folder (Allgäu2017, Familie, ...), shown to the adults. Empty: none. The server only reads it.
--   setting photo_places    `osm`: the name of the place a picture was taken is asked of OpenStreetMap (Nominatim; the coordinates leave
--                           the house, rounded to about 100 m) and kept; `off`: no place is shown.
--   library_photo           one picture or video of an old folder: `folder` is the first folder below photo_library, `file` the path
--                           relative to photo_library. `taken_at` comes from the EXIF data, else from the time of the file. The server
--                           reads the folders again every hour and when the setting changes; `mtime`/`size_bytes` say whether a file changed.
--   photo_place             the names of the places already asked for, by rounded coordinates and language (`name` NULL: there is none).
INSERT INTO setting (key, value, confirmed) VALUES
    ('photo_library', '', 1),
    ('photo_places', 'osm', 1);
CREATE TABLE library_photo (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    folder      TEXT    NOT NULL,
    file        TEXT    NOT NULL UNIQUE,
    size_bytes  INTEGER NOT NULL CHECK (size_bytes >= 0),
    mtime       INTEGER NOT NULL,
    mime        TEXT    NOT NULL,
    taken_at    INTEGER NOT NULL,
    created_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0,
    updated_by TEXT
) STRICT;
CREATE INDEX library_photo_folder ON library_photo (folder, taken_at);
CREATE TABLE photo_place (
    key         TEXT    PRIMARY KEY,
    name        TEXT,
    fetched_at  INTEGER NOT NULL,
    created_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0,
    updated_by TEXT
) STRICT;
CREATE TRIGGER audit_library_photo_ai AFTER INSERT ON library_photo FOR EACH ROW BEGIN
    UPDATE library_photo SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_library_photo_au AFTER UPDATE ON library_photo FOR EACH ROW BEGIN
    UPDATE library_photo SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_photo_place_ai AFTER INSERT ON photo_place FOR EACH ROW BEGIN
    UPDATE photo_place SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE key IS NEW.key;
END;
CREATE TRIGGER audit_photo_place_au AFTER UPDATE ON photo_place FOR EACH ROW BEGIN
    UPDATE photo_place SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE key IS NEW.key;
END;
