-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0032: knowing which CDs are in the library already, and healing them (project owner, 2026-10-28).
--   cd_disc / cd_disc_track  the CDs that were imported (or healed) by Disc ID: the book, the CD number and for every track its
--                            number, file, SHA-1 of the PCM that was read and length; filled by every successful import
--   cd_auto_import           1 = a CD that is put in is imported by itself when MusicBrainz knows exactly one release for it
--                            and it is not in the library yet (only with cd_udev on; 0)
--   cd_heal_tolerance_s      a file in the library may differ from the length by the table of contents by this much (3; 1 to 60)
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_auto_import', '0', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_heal_tolerance_s', '3', 1);

CREATE TABLE cd_disc (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    disc_id     TEXT NOT NULL UNIQUE,
    author      TEXT NOT NULL,
    title       TEXT NOT NULL,
    disc        INTEGER NOT NULL DEFAULT 1,
    discs_total INTEGER NOT NULL DEFAULT 1,
    kind        TEXT NOT NULL DEFAULT 'audiobook',
    -- The book folder below the import folder, with / as separator.
    folder      TEXT NOT NULL DEFAULT '',
    created_at  INTEGER NOT NULL DEFAULT 0,
    updated_at  INTEGER NOT NULL DEFAULT 0,
    created_by  TEXT,
    updated_by  TEXT
);
CREATE TRIGGER audit_cd_disc_ai AFTER INSERT ON cd_disc FOR EACH ROW BEGIN
    UPDATE cd_disc SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_cd_disc_au AFTER UPDATE ON cd_disc FOR EACH ROW BEGIN
    UPDATE cd_disc SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;

CREATE TABLE cd_disc_track (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    cd_disc_id INTEGER NOT NULL REFERENCES cd_disc (id) ON DELETE CASCADE,
    number     INTEGER NOT NULL,
    -- The file below the import folder, with / as separator.
    file       TEXT NOT NULL,
    sha1       TEXT NOT NULL DEFAULT '',
    seconds    INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_by TEXT,
    UNIQUE (cd_disc_id, number)
);
CREATE TRIGGER audit_cd_disc_track_ai AFTER INSERT ON cd_disc_track FOR EACH ROW BEGIN
    UPDATE cd_disc_track SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_cd_disc_track_au AFTER UPDATE ON cd_disc_track FOR EACH ROW BEGIN
    UPDATE cd_disc_track SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;
