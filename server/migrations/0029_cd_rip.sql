-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0029: the CD drive sits at the server, so the server reads the discs (project owner, 2026-10-28). Settings of
-- the tab "CD import" for that; the switch cd_udev (0026) now works in the server. No new table.
--   cd_poll_seconds  without udev events the server looks at the drives this often, in seconds (5; 2 to 300)
--   cd_device        the drive the server uses, for example /dev/sr0; empty = every drive it finds
--   cd_verify_read   1 = every track is read twice and the two reads are compared (1)
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_poll_seconds', '5', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_device', '', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_verify_read', '1', 1);
