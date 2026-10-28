-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0014: music next to audiobooks (project owner, 2026-10-28: "I put music into
-- the folder"). An album is a book with the kind 'music'; the same rules apply (release with an
-- age, planner, progress). Music has its own import folders (`music_roots`), laid out as
-- Artist/Album/*.mp3 like the audiobooks are Author/Title/*.mp3.
ALTER TABLE book ADD COLUMN kind TEXT NOT NULL DEFAULT 'audiobook' CHECK (kind IN ('audiobook', 'music'));
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('music_roots', '[]', 1);
