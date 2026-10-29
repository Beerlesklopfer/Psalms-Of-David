-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0034: the encoder thread of a rip runs at the highest priority, so that the PCM leaves the ring as fast as the drive
-- fills it (project owner, 2026-10-29).
--   cd_encoder_nice  the nice value the encoder thread asks for (-20 = highest priority, 19 = lowest; -20). If the system does not
--                    allow the value, the nearest one it allows is used (the service unit sets LimitNICE=40, which allows -20)
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_encoder_nice', '-20', 1);
