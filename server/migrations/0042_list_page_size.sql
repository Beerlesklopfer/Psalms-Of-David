-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0042: the media lists of the administration pages come in pages (project owner, 2026-10-29: "for the media pagination is missing").
-- `list_page_size` is the number of books on a page (10 to 500); a page can ask for another size with `?per=`.
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('list_page_size', '50', 1);
