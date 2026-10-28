-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0005: anchors that let an audio Bible be opened at a passage.
-- See docs/spec-bible.md.
--
-- An anchor says: in this item, in this part, at this position, verse V of chapter C
-- of Bible book B begins. bible_book counts 1 (Genesis) to 66 (Revelation). The
-- start of a chapter is its verse 1. The same Bible can be recorded in several items
-- (translations, readers); each has its own anchors.
CREATE TABLE bible_anchor (
    book_id      INTEGER NOT NULL REFERENCES book (id) ON DELETE CASCADE,
    track_id     INTEGER NOT NULL REFERENCES track (id) ON DELETE CASCADE,
    bible_book   INTEGER NOT NULL CHECK (bible_book BETWEEN 1 AND 66),
    chapter      INTEGER NOT NULL CHECK (chapter >= 1),
    verse        INTEGER NOT NULL CHECK (verse >= 1),
    position_ms  INTEGER NOT NULL CHECK (position_ms >= 0),
    PRIMARY KEY (book_id, bible_book, chapter, verse)
) STRICT, WITHOUT ROWID;

CREATE INDEX bible_anchor_track ON bible_anchor (track_id);
