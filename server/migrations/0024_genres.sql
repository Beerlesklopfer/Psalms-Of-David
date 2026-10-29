-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0024: genres that are looked after (project owner, 2026-10-28: "Psalm: Musik ist nach Genre
-- sortiert. Die Genres müssen gepflegt werden."). The tags say what they like ("Rock", "Rockmusik",
-- "Unbekanntes Genre"); the family keeps a list of genres in an order it chooses, and says which tag texts
-- belong to which genre (`genre_alias`). An album takes its genre from the alias of its tag text, unless
-- somebody set it by hand (`genre_locked`).
CREATE TABLE genre (
    id       INTEGER PRIMARY KEY,
    name     TEXT    NOT NULL UNIQUE COLLATE NOCASE CHECK (length(name) BETWEEN 1 AND 80),
    position INTEGER NOT NULL DEFAULT 0,
    visible  INTEGER NOT NULL DEFAULT 1 CHECK (visible IN (0, 1))
) STRICT;

CREATE TABLE genre_alias (
    tag_text TEXT    NOT NULL PRIMARY KEY COLLATE NOCASE CHECK (length(tag_text) BETWEEN 1 AND 200),
    genre_id INTEGER NOT NULL REFERENCES genre (id) ON DELETE CASCADE
) STRICT;

ALTER TABLE book ADD COLUMN genre_tag    TEXT;
ALTER TABLE book ADD COLUMN genre_id     INTEGER REFERENCES genre (id) ON DELETE SET NULL;
ALTER TABLE book ADD COLUMN genre_locked INTEGER NOT NULL DEFAULT 0 CHECK (genre_locked IN (0, 1));
CREATE INDEX book_genre ON book (genre_id);
