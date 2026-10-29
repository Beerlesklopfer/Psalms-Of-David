-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0037: a track of which no two readings agree is not the end of the rip (project owner, 2026-10-29: the drive is a Blu-ray
-- drive, which does not read audio CDs bit-exact, so two readings of the same place can differ a little).
--   cd_accept_unverified  1 = such a track is filed with its first reading and marked "not confirmed" (in the list and in the log);
--                         0 = it is left out and reported as unreadable, and the rest of the CD is imported (0)
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_accept_unverified', '0', 1);
