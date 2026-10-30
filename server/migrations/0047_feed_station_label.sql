-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0047: labels on podcasts and radio stations (project owner, 2026-10-30: "the tagging of the app covers ALL media"). Like `book_label`:
-- a podcast is a `feed`, a radio station is a `station`; both are shared by the family, so their labels are shared too. Dropped with the feed, the
-- station or the label. Photos and e-books are import folders only (no rows yet) and playlists belong to a person: they carry no labels.
CREATE TABLE podcast_label (
    id         INTEGER PRIMARY KEY,
    feed_id    INTEGER NOT NULL REFERENCES feed (id) ON DELETE CASCADE,
    label_id   INTEGER NOT NULL REFERENCES label (id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0,
    updated_by TEXT
) STRICT;
CREATE UNIQUE INDEX podcast_label_pair ON podcast_label (feed_id, label_id);
CREATE INDEX podcast_label_label ON podcast_label (label_id);
CREATE TRIGGER audit_podcast_label_ai AFTER INSERT ON podcast_label FOR EACH ROW BEGIN
    UPDATE podcast_label SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_podcast_label_au AFTER UPDATE ON podcast_label FOR EACH ROW BEGIN
    UPDATE podcast_label SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;
CREATE TABLE station_label (
    id         INTEGER PRIMARY KEY,
    station_id INTEGER NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    label_id   INTEGER NOT NULL REFERENCES label (id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0,
    updated_by TEXT
) STRICT;
CREATE UNIQUE INDEX station_label_pair ON station_label (station_id, label_id);
CREATE INDEX station_label_label ON station_label (label_id);
CREATE TRIGGER audit_station_label_ai AFTER INSERT ON station_label FOR EACH ROW BEGIN
    UPDATE station_label SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_station_label_au AFTER UPDATE ON station_label FOR EACH ROW BEGIN
    UPDATE station_label SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;
