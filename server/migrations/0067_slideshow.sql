-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0067: the projects of a slideshow, put together by hand (docs/spec-pictures.md, 9; project owner, 2026-11-08).
--   slideshow           a project of a person, seen and changed only by that person: its title and the choices it starts with
--                       (`ordering` numbers: in the order of the numbers, random: shuffled; seconds, transition, videos, info as on
--                       /pictures; `music` an own playlist, NULL: none). It plays as a slideshow, is saved as a project file and is
--                       made into a video, like a choice of /pictures.
--   slideshow_picture   a picture of a project: `key` as the pictures have it (`b<id>` the backup, `l<id>` an old folder), `position`
--                       1..n without gaps (the program renumbers after each change), `transition` and `seconds` NULL: those of the
--                       project. A picture the person may no longer see, or that is gone, stays here but is left out when it plays.
CREATE TABLE slideshow (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id     INTEGER NOT NULL REFERENCES profile (id) ON DELETE CASCADE,
    title       TEXT    NOT NULL CHECK (length(title) >= 1 AND title = trim(title)),
    ordering    TEXT    NOT NULL DEFAULT 'numbers' CHECK (ordering IN ('numbers', 'random')),
    seconds     INTEGER NOT NULL DEFAULT 8 CHECK (seconds IN (3, 5, 8, 15, 30)),
    transition  TEXT    NOT NULL DEFAULT 'fade' CHECK (transition IN ('fade', 'zoom', 'cut', 'random')),
    videos      TEXT    NOT NULL DEFAULT 'on' CHECK (videos IN ('on', 'off')),
    info        TEXT    NOT NULL DEFAULT 'both' CHECK (info IN ('both', 'date', 'none')),
    music       INTEGER REFERENCES playlist (id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0,
    updated_by TEXT
) STRICT;
CREATE INDEX slideshow_user ON slideshow (user_id, id);
CREATE TABLE slideshow_picture (
    slideshow_id INTEGER NOT NULL REFERENCES slideshow (id) ON DELETE CASCADE,
    key          TEXT    NOT NULL,
    position     INTEGER NOT NULL CHECK (position >= 1),
    transition   TEXT    CHECK (transition IN ('fade', 'zoom', 'cut')),
    seconds      INTEGER CHECK (seconds IN (3, 5, 8, 15, 30)),
    created_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0,
    updated_by TEXT,
    PRIMARY KEY (slideshow_id, key)
) STRICT;
CREATE INDEX slideshow_picture_position ON slideshow_picture (slideshow_id, position);
CREATE TRIGGER audit_slideshow_ai AFTER INSERT ON slideshow FOR EACH ROW BEGIN
    UPDATE slideshow SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_slideshow_au AFTER UPDATE ON slideshow FOR EACH ROW BEGIN
    UPDATE slideshow SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_slideshow_picture_ai AFTER INSERT ON slideshow_picture FOR EACH ROW BEGIN
    UPDATE slideshow_picture SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE slideshow_id IS NEW.slideshow_id AND key IS NEW.key;
END;
CREATE TRIGGER audit_slideshow_picture_au AFTER UPDATE ON slideshow_picture FOR EACH ROW BEGIN
    UPDATE slideshow_picture SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE slideshow_id IS NEW.slideshow_id AND key IS NEW.key;
END;
