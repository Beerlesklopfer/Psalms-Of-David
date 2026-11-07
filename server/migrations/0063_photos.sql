-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0063: the pictures of the photo backup (docs/spec-photo-sync.md, sections 3 and 4).
--   photo         one picture or video of a person, kept in the picture folder of the profile at `file` (relative to it, <year>/<month>/<name>).
--                 `sha256` is unique per owner: the same picture from two devices is kept once. `device_id` is the device that sent it (NULL
--                 when the device is gone); bisync gives a device the pictures of the others.
--   photo_upload  an upload under way: the bytes so far are in `part` (a file in the hidden folder .david-upload of the picture folder), so a
--                 device that lost the connection goes on where it stopped. `client_id` is the device's name for the file.
CREATE TABLE photo (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    owner_id      INTEGER NOT NULL REFERENCES profile (id) ON DELETE CASCADE,
    device_id     INTEGER REFERENCES device (id) ON DELETE SET NULL,
    sha256        TEXT    NOT NULL CHECK (length(sha256) = 64),
    size_bytes    INTEGER NOT NULL CHECK (size_bytes >= 0),
    file          TEXT    NOT NULL,
    original_name TEXT    NOT NULL,
    mime          TEXT    NOT NULL,
    taken_at      INTEGER NOT NULL,
    uploaded_at   INTEGER NOT NULL,
    created_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0,
    updated_by TEXT,
    UNIQUE (owner_id, sha256)
) STRICT;
CREATE INDEX photo_owner ON photo (owner_id, id);
CREATE TABLE photo_upload (
    id          INTEGER PRIMARY KEY,
    owner_id    INTEGER NOT NULL REFERENCES profile (id) ON DELETE CASCADE,
    device_id   INTEGER NOT NULL DEFAULT 0,
    client_id   TEXT    NOT NULL CHECK (length(client_id) BETWEEN 1 AND 128),
    sha256      TEXT    NOT NULL CHECK (length(sha256) = 64),
    size_bytes  INTEGER NOT NULL CHECK (size_bytes >= 0),
    received    INTEGER NOT NULL DEFAULT 0,
    name        TEXT    NOT NULL,
    mime        TEXT    NOT NULL,
    taken_at    INTEGER NOT NULL,
    part        TEXT    NOT NULL,
    touched_at  INTEGER NOT NULL,
    created_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0,
    updated_by TEXT,
    UNIQUE (owner_id, device_id, client_id)
) STRICT;
CREATE TRIGGER audit_photo_ai AFTER INSERT ON photo FOR EACH ROW BEGIN
    UPDATE photo SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_photo_au AFTER UPDATE ON photo FOR EACH ROW BEGIN
    UPDATE photo SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_photo_upload_ai AFTER INSERT ON photo_upload FOR EACH ROW BEGIN
    UPDATE photo_upload SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_photo_upload_au AFTER UPDATE ON photo_upload FOR EACH ROW BEGIN
    UPDATE photo_upload SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;
