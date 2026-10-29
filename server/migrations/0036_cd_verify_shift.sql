-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0036: the second read of a track (cd_verify_read) is shifted against the first, as cd-paranoia does ("jiggle"), so that a
-- fault tied to the border of a command to the drive is not read the same way twice (project owner, 2026-10-29).
--   cd_verify_shift  sectors by which the commands of the second read are shifted against those of the first (13, about half a command
--                    of 27 sectors; 0 to 26; 0 = both reads on the same grid)
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_verify_shift', '13', 1);
