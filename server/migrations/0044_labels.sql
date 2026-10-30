-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0044: labels, the classic tagging (project owner, 2026-10-30: "everything about prayer, or school, or common holidays"). A label is a
-- free word of the family's list; a book (audiobook or album) carries any number of them, and the lists can be filtered by them. Not to be mixed up with
-- the ID3 tags of the files. The names are unique without regard to case. See docs/spec-admin-ui.md ("labels").
CREATE TABLE label (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL CHECK (length(name) >= 1),
    created_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0,
    updated_by TEXT
) STRICT;
CREATE UNIQUE INDEX label_name ON label (name COLLATE NOCASE);
CREATE TRIGGER audit_label_ai AFTER INSERT ON label FOR EACH ROW BEGIN
    UPDATE label SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_label_au AFTER UPDATE ON label FOR EACH ROW BEGIN
    UPDATE label SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;
CREATE TABLE book_label (
    id         INTEGER PRIMARY KEY,
    book_id    INTEGER NOT NULL REFERENCES book (id) ON DELETE CASCADE,
    label_id   INTEGER NOT NULL REFERENCES label (id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0,
    updated_by TEXT
) STRICT;
CREATE UNIQUE INDEX book_label_pair ON book_label (book_id, label_id);
CREATE INDEX book_label_label ON book_label (label_id);
CREATE TRIGGER audit_book_label_ai AFTER INSERT ON book_label FOR EACH ROW BEGIN
    UPDATE book_label SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_book_label_au AFTER UPDATE ON book_label FOR EACH ROW BEGIN
    UPDATE book_label SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;
