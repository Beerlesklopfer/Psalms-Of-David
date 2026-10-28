-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0013: protection against filling the disk (project owner, 2026-10-28: "the
-- server needs protection against writing the disk full"). Uploads, unpacking and tag
-- writing stop when less than this many GB would stay free. 0 turns the protection off.
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('min_free_gb', '2', 1);
