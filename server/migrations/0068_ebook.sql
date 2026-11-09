-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0068: the e-books (docs/spec-ebooks.md; project owner, 2026-11-09: a reader in the app, EPUB and PDF, for biblical
-- exegesis and for leisure; who sees what as with the audiobooks).
--   ebook           one file of the e-book folders (`ebook_roots`), by its path: title, author, language from the file; `format` epub or
--                   pdf; `size_bytes` and `mtime_ms` tell when it changed (the text made of it is made again then). `released` and
--                   `min_age` as on `book`: an administrator releases a book; the others see released books within their age limit,
--                   and the exceptions of `ebook_release`. `missing` 1: the file is gone (kept for the positions and notes, not listed).
--   ebook_release   an exception like `book_release`: one profile or one role sees the e-book although it is not released.
--   ebook_progress  where a person is in an e-book: `locator` as the app writes it (`epub:<chapter>:<block>` or `pdf:<page>`), the
--                   share read so far (0..1); the newer report wins.
CREATE TABLE ebook (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    path        TEXT    NOT NULL UNIQUE,
    format      TEXT    NOT NULL CHECK (format IN ('epub', 'pdf')),
    title       TEXT    NOT NULL,
    author      TEXT    NOT NULL DEFAULT '',
    language    TEXT,
    size_bytes  INTEGER NOT NULL DEFAULT 0,
    mtime_ms    INTEGER NOT NULL DEFAULT 0,
    released    INTEGER NOT NULL DEFAULT 0 CHECK (released IN (0, 1)),
    min_age     INTEGER NOT NULL DEFAULT 0 CHECK (min_age >= 0),
    missing     INTEGER NOT NULL DEFAULT 0 CHECK (missing IN (0, 1)),
    added_at    INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0,
    updated_by TEXT
) STRICT;
CREATE TABLE ebook_release (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    ebook_id    INTEGER NOT NULL REFERENCES ebook (id) ON DELETE CASCADE,
    profile_id  INTEGER REFERENCES profile (id) ON DELETE CASCADE,
    role        TEXT CHECK (role IN ('member', 'child', 'guest')),
    created_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0,
    updated_by TEXT,
    CHECK ((profile_id IS NULL) <> (role IS NULL))
) STRICT;
CREATE INDEX ebook_release_book ON ebook_release (ebook_id);
CREATE TABLE ebook_progress (
    profile_id  INTEGER NOT NULL REFERENCES profile (id) ON DELETE CASCADE,
    ebook_id    INTEGER NOT NULL REFERENCES ebook (id) ON DELETE CASCADE,
    locator     TEXT    NOT NULL,
    percent     REAL    NOT NULL DEFAULT 0 CHECK (percent >= 0 AND percent <= 1),
    read_at     INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0,
    updated_by TEXT,
    PRIMARY KEY (profile_id, ebook_id)
) STRICT;
CREATE INDEX ebook_progress_recent ON ebook_progress (profile_id, read_at DESC);
CREATE TRIGGER audit_ebook_ai AFTER INSERT ON ebook FOR EACH ROW BEGIN
    UPDATE ebook SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_ebook_au AFTER UPDATE ON ebook FOR EACH ROW BEGIN
    UPDATE ebook SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_ebook_release_ai AFTER INSERT ON ebook_release FOR EACH ROW BEGIN
    UPDATE ebook_release SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_ebook_release_au AFTER UPDATE ON ebook_release FOR EACH ROW BEGIN
    UPDATE ebook_release SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_ebook_progress_ai AFTER INSERT ON ebook_progress FOR EACH ROW BEGIN
    UPDATE ebook_progress SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE profile_id IS NEW.profile_id AND ebook_id IS NEW.ebook_id;
END;
CREATE TRIGGER audit_ebook_progress_au AFTER UPDATE ON ebook_progress FOR EACH ROW BEGIN
    UPDATE ebook_progress SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE profile_id IS NEW.profile_id AND ebook_id IS NEW.ebook_id;
END;
