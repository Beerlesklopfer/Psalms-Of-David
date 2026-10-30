-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0054: what the family sees of each other's listening (project owner, 2026-10-30). The dashboard of the app shows a row "recently
-- heard" for every other member. `share_listening` is the choice of the person (1: visible, the default); `may_hide_listening` is the decision of
-- an administrator whether the person may turn it off at all (1: may, the default). With 0 the listening is visible whatever `share_listening`
-- says (accountability). The audit triggers of the table (0028) keep recording who changed a row.
ALTER TABLE profile ADD COLUMN share_listening INTEGER NOT NULL DEFAULT 1 CHECK (share_listening IN (0, 1));
ALTER TABLE profile ADD COLUMN may_hide_listening INTEGER NOT NULL DEFAULT 1 CHECK (may_hide_listening IN (0, 1));
