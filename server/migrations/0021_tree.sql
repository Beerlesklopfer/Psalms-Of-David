-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0021: the hierarchy of the media lists (project owner, 2026-10-28: "wir brauchen eine
-- Treeview, und deren Hierarchie muss in den Settings eingestellt werden"). One value for the
-- audiobooks and one for the music: flat, author (artist, then album), folder (the folders on the
-- disk) or letter_author (first letter, artist, album).
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('tree_audiobook', 'author', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('tree_music', 'folder', 1);
