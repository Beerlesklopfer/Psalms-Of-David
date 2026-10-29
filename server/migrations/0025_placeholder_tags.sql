-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0025: the words that mark a tag as a placeholder of a ripper or a tagger instead of a real
-- value (project owner, 2026-10-28: "niemals hart coden"). A tag that starts with one of them (any case) is
-- treated as missing, so that the folder name speaks instead. Separated by commas; the settings page edits it.
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('placeholder_tags', 'unbekannt,unknown,kein künstler,kein titel,various artists,verschiedene', 1);
