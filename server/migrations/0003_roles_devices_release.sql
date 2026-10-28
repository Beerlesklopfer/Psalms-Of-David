-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0003: roles, devices, limits, age release.
-- See docs/spec-discovery-profiles-roles.md.
--
-- Values that the project owner has not confirmed yet are PROPOSALS. They are
-- marked with confirmed = 0 in role_policy and setting, and the server warns at
-- start while any are left. Confirming a value sets confirmed = 1.

-- Users: a role, an optional device limit, an optional expiry (guests) and the
-- weekly planner (672 bits = 84 bytes, 15-minute slots, bit set = listening allowed).
ALTER TABLE profile ADD COLUMN role TEXT NOT NULL DEFAULT 'member'
    CHECK (role IN ('admin', 'member', 'child', 'guest'));
UPDATE profile SET role = CASE WHEN is_admin = 1 THEN 'admin' ELSE 'member' END;
ALTER TABLE profile ADD COLUMN max_devices INTEGER CHECK (max_devices IS NULL OR max_devices >= 0);
ALTER TABLE profile ADD COLUMN expires_at INTEGER;
ALTER TABLE profile ADD COLUMN schedule BLOB CHECK (schedule IS NULL OR length(schedule) = 84);

-- A child always has an age limit and a planner.
CREATE TRIGGER profile_child_insert BEFORE INSERT ON profile
WHEN NEW.role = 'child' AND (NEW.max_age IS NULL OR NEW.schedule IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'child_requires_age_and_schedule');
END;

CREATE TRIGGER profile_child_update BEFORE UPDATE ON profile
WHEN NEW.role = 'child' AND (NEW.max_age IS NULL OR NEW.schedule IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'child_requires_age_and_schedule');
END;

-- At least one admin must always exist.
CREATE TRIGGER profile_last_admin_delete BEFORE DELETE ON profile
WHEN OLD.role = 'admin' AND (SELECT count(*) FROM profile WHERE role = 'admin') <= 1
BEGIN
    SELECT RAISE(ABORT, 'last_admin');
END;

CREATE TRIGGER profile_last_admin_demote BEFORE UPDATE OF role ON profile
WHEN OLD.role = 'admin' AND NEW.role <> 'admin'
     AND (SELECT count(*) FROM profile WHERE role = 'admin') <= 1
BEGIN
    SELECT RAISE(ABORT, 'last_admin');
END;

-- Books: only released books are shown. Releasing means giving an age rating
-- (min_age). recommended_age is the medium's own recommendation (information only).
-- quota_exempt content is not subject to the weekly planner.
ALTER TABLE book ADD COLUMN recommended_age INTEGER
    CHECK (recommended_age IS NULL OR recommended_age >= 0);
ALTER TABLE book ADD COLUMN released INTEGER NOT NULL DEFAULT 0 CHECK (released IN (0, 1));
ALTER TABLE book ADD COLUMN quota_exempt INTEGER NOT NULL DEFAULT 0 CHECK (quota_exempt IN (0, 1));

CREATE TRIGGER book_release_needs_age_insert BEFORE INSERT ON book
WHEN NEW.released = 1 AND NEW.min_age IS NULL
BEGIN
    SELECT RAISE(ABORT, 'release_requires_age');
END;

CREATE TRIGGER book_release_needs_age_update BEFORE UPDATE ON book
WHEN NEW.released = 1 AND NEW.min_age IS NULL
BEGIN
    SELECT RAISE(ABORT, 'release_requires_age');
END;

-- Exceptions on top of the age rule: a book released to one user or to a role,
-- whatever its age rating. Exactly one of profile_id and role is set.
CREATE TABLE book_release (
    id          INTEGER PRIMARY KEY,
    book_id     INTEGER NOT NULL REFERENCES book (id) ON DELETE CASCADE,
    profile_id  INTEGER REFERENCES profile (id) ON DELETE CASCADE,
    role        TEXT CHECK (role IN ('member', 'child', 'guest')),
    created_by  INTEGER REFERENCES profile (id) ON DELETE SET NULL,
    created_at  INTEGER NOT NULL,
    CHECK ((profile_id IS NULL) <> (role IS NULL))
) STRICT;

CREATE UNIQUE INDEX book_release_user ON book_release (book_id, profile_id) WHERE profile_id IS NOT NULL;
CREATE UNIQUE INDEX book_release_role ON book_release (book_id, role) WHERE role IS NOT NULL;

-- Defaults per role. PROPOSALS (confirmed = 0): devices per role, session lifetime,
-- step-up window.
CREATE TABLE role_policy (
    role           TEXT PRIMARY KEY CHECK (role IN ('admin', 'member', 'child', 'guest')),
    max_devices    INTEGER NOT NULL CHECK (max_devices >= 0),
    session_ttl_s  INTEGER NOT NULL CHECK (session_ttl_s > 0),
    step_up_s      INTEGER NOT NULL CHECK (step_up_s >= 0),
    confirmed      INTEGER NOT NULL DEFAULT 0 CHECK (confirmed IN (0, 1))
) STRICT;

INSERT INTO role_policy (role, max_devices, session_ttl_s, step_up_s, confirmed) VALUES
    ('admin',  5, 2592000, 600, 0),   -- PROPOSAL: 5 devices, 30 days, 10 minutes
    ('member', 5, 2592000, 600, 0),   -- PROPOSAL: 5 devices, 30 days, 10 minutes
    ('child',  3, 7776000, 600, 0),   -- PROPOSAL: 3 devices, 90 days, 10 minutes
    ('guest',  1,   86400, 600, 0);   -- PROPOSAL: 1 device, 24 hours, 10 minutes

CREATE TABLE setting (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    confirmed  INTEGER NOT NULL DEFAULT 0 CHECK (confirmed IN (0, 1))
) STRICT;

INSERT INTO setting (key, value, confirmed) VALUES
    ('import_enabled',        '0',             1),  -- decided: imports are locked by default
    ('import_roots',          '[]',            1),  -- nothing may be imported until roots are defined
    ('rating_rule',           'manual',        0),  -- PROPOSAL: an admin rates each book
    ('enrollment_code_ttl_s', '900',           0),  -- PROPOSAL: a pairing code is valid for 15 minutes
    ('device_inactive_days',  '180',           0),  -- PROPOSAL
    ('guest_expire_days',     '7',             0),  -- PROPOSAL
    ('time_zone',             'Europe/Berlin', 0);  -- PROPOSAL: used to evaluate the planner

-- Devices belong to a user. The two tables were not used before this migration, so
-- they are recreated instead of altered.
DROP TABLE device;
DROP TABLE enrollment;

CREATE TABLE device (
    id          INTEGER PRIMARY KEY,
    user_id     INTEGER NOT NULL REFERENCES profile (id) ON DELETE CASCADE,
    name        TEXT    NOT NULL,
    wg_pubkey   TEXT    NOT NULL UNIQUE,
    tunnel_ip   TEXT    NOT NULL UNIQUE,
    shared      INTEGER NOT NULL DEFAULT 0 CHECK (shared IN (0, 1)),
    created_by  INTEGER REFERENCES profile (id) ON DELETE SET NULL,
    created_at  INTEGER NOT NULL,
    last_seen   INTEGER,
    revoked_at  INTEGER
) STRICT;

-- One-time pairing codes, created by an admin for a user.
CREATE TABLE enrollment (
    code_hash   TEXT    PRIMARY KEY,
    user_id     INTEGER NOT NULL REFERENCES profile (id) ON DELETE CASCADE,
    shared      INTEGER NOT NULL DEFAULT 0 CHECK (shared IN (0, 1)),
    created_by  INTEGER NOT NULL REFERENCES profile (id) ON DELETE CASCADE,
    expires_at  INTEGER NOT NULL,
    used_at     INTEGER
) STRICT;

-- A user may own max_devices active (not revoked) devices: the user's own value,
-- otherwise the default of the role.
CREATE TRIGGER device_limit BEFORE INSERT ON device
WHEN NEW.revoked_at IS NULL
 AND (SELECT count(*) FROM device WHERE user_id = NEW.user_id AND revoked_at IS NULL) >=
     coalesce((SELECT max_devices FROM profile WHERE id = NEW.user_id),
              (SELECT rp.max_devices FROM role_policy rp
                 JOIN profile p ON p.role = rp.role WHERE p.id = NEW.user_id))
BEGIN
    SELECT RAISE(ABORT, 'device_limit_reached');
END;

CREATE TRIGGER device_limit_unrevoke BEFORE UPDATE OF revoked_at ON device
WHEN NEW.revoked_at IS NULL AND OLD.revoked_at IS NOT NULL
 AND (SELECT count(*) FROM device WHERE user_id = NEW.user_id AND revoked_at IS NULL) >=
     coalesce((SELECT max_devices FROM profile WHERE id = NEW.user_id),
              (SELECT rp.max_devices FROM role_policy rp
                 JOIN profile p ON p.role = rp.role WHERE p.id = NEW.user_id))
BEGIN
    SELECT RAISE(ABORT, 'device_limit_reached');
END;

-- Sessions: the effective role is the lower of the user's role and the device
-- owner's role (shared devices).
CREATE TABLE session (
    token_hash  TEXT    PRIMARY KEY,
    user_id     INTEGER NOT NULL REFERENCES profile (id) ON DELETE CASCADE,
    device_id   INTEGER NOT NULL REFERENCES device (id)  ON DELETE CASCADE,
    role        TEXT    NOT NULL CHECK (role IN ('admin', 'member', 'child', 'guest')),
    created_at  INTEGER NOT NULL,
    last_used   INTEGER NOT NULL,
    expires_at  INTEGER NOT NULL
) STRICT;

CREATE INDEX session_expiry ON session (expires_at);

-- The id of this server, announced over mDNS and pinned by paired devices. The row
-- is created by the server at first start.
CREATE TABLE server_identity (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    server_id   TEXT    NOT NULL,
    created_at  INTEGER NOT NULL
) STRICT;
