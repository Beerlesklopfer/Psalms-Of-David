-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0039: a book can be closed to one profile (project owner, 2026-10-29: a matrix of checkboxes, books by family members, in the list
-- of the media). Until now a book was visible by release and age, plus exceptions that open it (`book_release`); there was no way to close
-- one book to one person. A row here closes `book_id` to `profile_id` whatever the release, the age or an exception of the role says.
-- Administrators see everything and are not closed out. See docs/spec-admin-ui.md ("visibility matrix").
CREATE TABLE book_block (
    id         INTEGER PRIMARY KEY,
    book_id    INTEGER NOT NULL REFERENCES book (id) ON DELETE CASCADE,
    profile_id INTEGER NOT NULL REFERENCES profile (id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0,
    updated_by TEXT
) STRICT;
CREATE UNIQUE INDEX book_block_pair ON book_block (book_id, profile_id);
CREATE INDEX book_block_profile ON book_block (profile_id);
CREATE TRIGGER audit_book_block_ai AFTER INSERT ON book_block FOR EACH ROW BEGIN
    UPDATE book_block SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_book_block_au AFTER UPDATE ON book_block FOR EACH ROW BEGIN
    UPDATE book_block SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;
