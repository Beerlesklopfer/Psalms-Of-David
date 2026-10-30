-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0048: the blacklist (project owner, 2026-10-30: "there is a blacklist, MCP can search for it"). The administrators keep a list of
-- words (setting `label_blacklist`, one word or phrase per line, `*` and `?` as in the search); a medium whose text contains one of them carries the
-- system mark "Blacklist", which only administrators see. The mark is a row here, not a label of the family's list. Nothing is blocked by it.
-- `approved = 1`: an administrator took the mark off, so it is not put on again by the next run (it goes when the medium no longer hits at all).
CREATE TABLE blacklist_mark (
    id         INTEGER PRIMARY KEY,
    kind       TEXT NOT NULL CHECK (kind IN ('book', 'podcast', 'station')),
    item_id    INTEGER NOT NULL,
    word       TEXT NOT NULL,
    approved   INTEGER NOT NULL DEFAULT 0 CHECK (approved IN (0, 1)),
    created_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0,
    updated_by TEXT
) STRICT;
CREATE UNIQUE INDEX blacklist_mark_item ON blacklist_mark (kind, item_id);
CREATE TRIGGER audit_blacklist_mark_ai AFTER INSERT ON blacklist_mark FOR EACH ROW BEGIN
    UPDATE blacklist_mark SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_blacklist_mark_au AFTER UPDATE ON blacklist_mark FOR EACH ROW BEGIN
    UPDATE blacklist_mark SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;
-- No word comes with the program: the list is empty until the administrators fill it.
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('label_blacklist', '', 1);
