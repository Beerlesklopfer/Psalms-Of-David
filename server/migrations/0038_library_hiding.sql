-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0038: every profile looks after its own library (project owner, 2026-10-29). See docs/spec-library-hiding.md.
-- A profile hides single books, or every book of an author/artist (of one kind or of both); a hidden item leaves the lists of this profile
-- only. It is no access rule: a book the profile may not see stays unseen, and a hidden one can still be opened by its id.
-- `show_hidden` is the setting of the profile: the lists then carry the hidden items, marked, so that they can be shown again.
ALTER TABLE profile ADD COLUMN show_hidden INTEGER NOT NULL DEFAULT 0 CHECK (show_hidden IN (0, 1));

CREATE TABLE profile_hidden (
    id         INTEGER PRIMARY KEY,
    profile_id INTEGER NOT NULL REFERENCES profile (id) ON DELETE CASCADE,
    book_id    INTEGER REFERENCES book (id) ON DELETE CASCADE,
    author_id  INTEGER REFERENCES author (id) ON DELETE CASCADE,
    -- For an author: '' (every kind), 'audiobook' or 'music'. For a book: ''.
    kind       TEXT NOT NULL DEFAULT '' CHECK (kind IN ('', 'audiobook', 'music')),
    created_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0,
    updated_by TEXT,
    CHECK ((book_id IS NULL) <> (author_id IS NULL))
) STRICT;
CREATE UNIQUE INDEX profile_hidden_book ON profile_hidden (profile_id, book_id) WHERE book_id IS NOT NULL;
CREATE UNIQUE INDEX profile_hidden_author ON profile_hidden (profile_id, author_id, kind) WHERE author_id IS NOT NULL;
CREATE TRIGGER audit_profile_hidden_ai AFTER INSERT ON profile_hidden FOR EACH ROW BEGIN
    UPDATE profile_hidden SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_profile_hidden_au AFTER UPDATE ON profile_hidden FOR EACH ROW BEGIN
    UPDATE profile_hidden SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;
