-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0064: the trash of the photo backup (docs/spec-photo-sync.md, "The two modes", bisync, `deletes` = `trash`).
--   photo.trashed_at  set when a device of the person deleted the picture: the file is moved into the hidden folder .david-trash of the
--                     picture folder (`file` says where it is now) and removed for good 30 days later, unless the person restores it.
--                     The row stays while the picture is in the trash, so no other device sends it again.
--   photo_event       what the other devices of the person have to know: `trash` (remove it here too, after asking) or `restore`, in the
--                     order they happened (`seq` is the cursor of GET /photos/removals). `device_id` is the device that did it.
ALTER TABLE photo ADD COLUMN trashed_at INTEGER;
CREATE INDEX photo_trashed ON photo (trashed_at) WHERE trashed_at IS NOT NULL;
CREATE TABLE photo_event (
    seq        INTEGER PRIMARY KEY AUTOINCREMENT,
    owner_id   INTEGER NOT NULL REFERENCES profile (id) ON DELETE CASCADE,
    photo_id   INTEGER NOT NULL,
    sha256     TEXT    NOT NULL CHECK (length(sha256) = 64),
    kind       TEXT    NOT NULL CHECK (kind IN ('trash', 'restore')),
    device_id  INTEGER,
    at         INTEGER NOT NULL,
    created_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0,
    updated_by TEXT
) STRICT;
CREATE INDEX photo_event_owner ON photo_event (owner_id, seq);
CREATE TRIGGER audit_photo_event_ai AFTER INSERT ON photo_event FOR EACH ROW BEGIN
    UPDATE photo_event SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE seq IS NEW.seq;
END;
CREATE TRIGGER audit_photo_event_au AFTER UPDATE ON photo_event FOR EACH ROW BEGIN
    UPDATE photo_event SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE seq IS NEW.seq;
END;
