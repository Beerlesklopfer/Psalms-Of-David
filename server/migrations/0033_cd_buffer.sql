-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0033: the buffer between the drive and the encoder is a ring in memory that the drive fills without waiting for the
-- encoder (project owner, 2026-10-28).
--   cd_buffer_seconds  seconds of audio the ring holds; 0 = automatic: the longest chosen track by the table of contents plus 10 s, so
--                      a whole track fits (0; 0 or 2 to 900; 1 s of audio is 176,400 bytes)
--   cd_buffer_max_mb   the most memory the ring may take, also for the automatic size (256; 16 to 4096)
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_buffer_seconds', '0', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_buffer_max_mb', '256', 1);
