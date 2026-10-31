-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0057: the wait after wrong PINs grows with each one (project owner, 2026-10-31: "a mal 2 hoch n, wobei a in den
-- Settings einstellbar ist", in steps of seconds). After n wrong PINs in a row the next try waits a * 2^n seconds; a right PIN or a
-- new PIN starts n at 0 again. It replaces the fixed rule "5 wrong PINs, then 15 minutes" and its two settings.
DELETE FROM setting WHERE key IN ('pin_max_failures', 'pin_lock_s');
INSERT INTO setting (key, value, confirmed) VALUES ('pin_backoff_s', '1', 1);
-- `pin_failures` now counts the wrong PINs in a row (it was reset at every lock); start every user afresh.
UPDATE profile SET pin_failures = 0, pin_locked_until = NULL;
