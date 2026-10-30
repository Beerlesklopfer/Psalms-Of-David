-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0046: labels on single titles (project owner, 2026-10-30: "the tagging of the titles in the catalog is missing"). Like `book_label`, but
-- for one track; the labels of a title are its own plus the ones of its album. Dropped with the track or the label.
CREATE TABLE track_label (
    id         INTEGER PRIMARY KEY,
    track_id   INTEGER NOT NULL REFERENCES track (id) ON DELETE CASCADE,
    label_id   INTEGER NOT NULL REFERENCES label (id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0,
    updated_by TEXT
) STRICT;
CREATE UNIQUE INDEX track_label_pair ON track_label (track_id, label_id);
CREATE INDEX track_label_label ON track_label (label_id);
CREATE TRIGGER audit_track_label_ai AFTER INSERT ON track_label FOR EACH ROW BEGIN
    UPDATE track_label SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_track_label_au AFTER UPDATE ON track_label FOR EACH ROW BEGIN
    UPDATE track_label SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;
