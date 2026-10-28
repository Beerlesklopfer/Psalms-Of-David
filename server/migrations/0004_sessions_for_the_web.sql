-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0004: sessions of the administration interface and PIN lockout.
--
-- A browser session has no device, so session.device_id becomes optional. The
-- table is recreated; it was not used before this migration.
DROP TABLE session;

CREATE TABLE session (
    token_hash  TEXT    PRIMARY KEY,   -- SHA-256 of the token; the token itself is never stored
    user_id     INTEGER NOT NULL REFERENCES profile (id) ON DELETE CASCADE,
    device_id   INTEGER REFERENCES device (id) ON DELETE CASCADE,   -- NULL for the administration interface
    role        TEXT    NOT NULL CHECK (role IN ('admin', 'member', 'child', 'guest')),
    created_at  INTEGER NOT NULL,
    last_used   INTEGER NOT NULL,
    expires_at  INTEGER NOT NULL
) STRICT;

CREATE INDEX session_expiry ON session (expires_at);

-- Too many wrong PINs lock the user for a while.
ALTER TABLE profile ADD COLUMN pin_failures INTEGER NOT NULL DEFAULT 0;
ALTER TABLE profile ADD COLUMN pin_locked_until INTEGER;

INSERT INTO setting (key, value, confirmed) VALUES
    ('pin_max_failures', '5',   0),   -- PROPOSAL: lock after 5 wrong PINs
    ('pin_lock_s',       '900', 0);   -- PROPOSAL: for 15 minutes
