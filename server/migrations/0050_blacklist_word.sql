-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0050: the words of the blacklist are rows of their own (project owner, 2026-10-30: "the blacklist must be managed with CRUD in the web").
-- Until now they were the lines of the setting `label_blacklist`; they move here, one row per word with an optional note, and the setting goes.
-- The marks (`blacklist_mark`, schema 0048) now also remember the field that hit (title, author, narrator, description, track, label, name).
CREATE TABLE blacklist_word (
    id         INTEGER PRIMARY KEY,
    pattern    TEXT NOT NULL CHECK (length(pattern) >= 1 AND length(pattern) <= 100),
    note       TEXT NOT NULL DEFAULT '' CHECK (length(note) <= 200),
    created_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0,
    updated_by TEXT
) STRICT;
CREATE UNIQUE INDEX blacklist_word_pattern ON blacklist_word (pattern COLLATE NOCASE);
CREATE TRIGGER audit_blacklist_word_ai AFTER INSERT ON blacklist_word FOR EACH ROW BEGIN
    UPDATE blacklist_word SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_blacklist_word_au AFTER UPDATE ON blacklist_word FOR EACH ROW BEGIN
    UPDATE blacklist_word SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;

-- The lines of the old setting become rows (idempotent: a word that is in already is skipped).
WITH RECURSIVE split(word, rest) AS (
    SELECT '', coalesce((SELECT value FROM setting WHERE key = 'label_blacklist'), '') || char(10)
    UNION ALL
    SELECT trim(substr(rest, 1, instr(rest, char(10)) - 1), ' ' || char(9) || char(13)), substr(rest, instr(rest, char(10)) + 1) FROM split WHERE rest <> ''
)
INSERT OR IGNORE INTO blacklist_word (pattern) SELECT word FROM split WHERE word <> '' AND length(word) <= 100;
DELETE FROM setting WHERE key = 'label_blacklist';

ALTER TABLE blacklist_mark ADD COLUMN field TEXT NOT NULL DEFAULT '';
