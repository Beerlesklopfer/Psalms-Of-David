-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0002
-- Raises the profile limit from 25 to 150. The trigger is replaced instead of
-- editing 0001, so databases created with 0001 are upgraded on the next start.
DROP TRIGGER profile_limit;

CREATE TRIGGER profile_limit BEFORE INSERT ON profile
WHEN (SELECT count(*) FROM profile) >= 150
BEGIN
    SELECT RAISE(ABORT, 'profile_limit_reached');
END;
