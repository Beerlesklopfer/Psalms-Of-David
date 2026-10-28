-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0012: David looks for new files by itself (project owner, 2026-10-28: "new
-- CDs do not reach Psalm, there is no update mechanism"). The interval is a setting; 0 turns
-- the automatic look off. It only works while the import is unlocked.
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('import_interval_min', '10', 1);
