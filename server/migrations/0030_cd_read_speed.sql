-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0030: how fast the drive reads while a disc is ripped. A drive that reads at full speed and then waits for
-- a slow consumer slows down and speeds up again, which delays the import. (0029 was in a package before this setting was
-- added, so it comes in a migration of its own.)
--   cd_read_speed  a multiple of 1x = 176 kB/s (8; 0 = the drive's own speed; 0 to 52)
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_read_speed', '8', 1);
