-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0019: things the family chose to hide from the reports (project owner, 2026-10-28:
-- "wir haben keine Mechanik, Dinge auszublenden, zu ignorieren"; duplicates that are wanted on purpose
-- need an override). A rule says what is ignored (`what`: `duplicate`, a problem code of find_problems,
-- or `*` for everything) for which books (`scope`):
--   album : one book, key = its folder
--   title : every book with this title (key = the title without case, CD number and punctuation)
--   work  : the book with all its CDs and copies by that author (key = title key, a unit separator, author key)
CREATE TABLE ignore_rule (
    id         INTEGER PRIMARY KEY,
    scope      TEXT    NOT NULL CHECK (scope IN ('album', 'title', 'work')),
    key        TEXT    NOT NULL CHECK (length(key) BETWEEN 1 AND 500),
    what       TEXT    NOT NULL CHECK (length(what) BETWEEN 1 AND 60),
    note       TEXT,
    created_by TEXT    NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE (scope, key, what)
) STRICT;
