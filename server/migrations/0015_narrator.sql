-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0015: the narrator of an audiobook (project owner, 2026-10-28: "audiobooks
-- have an author and a narrator too"). The author is the artist of the files, the narrator
-- is read from the composer tag (TCOM in ID3, the same field Audiobookshelf uses) of the first
-- file. NULL when the files do not say.
ALTER TABLE book ADD COLUMN narrator TEXT;
