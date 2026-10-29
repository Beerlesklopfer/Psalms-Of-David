-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0031: a rip that was interrupted (a restart of the service, a scratched disc, a stop) keeps its finished
-- tracks and is resumed when the same disc is put in again (project owner, 2026-10-28).
--   cd_resume_keep_hours  how long the finished tracks of an interrupted rip are kept (48; 1 to 720)
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_resume_keep_hours', '48', 1);
