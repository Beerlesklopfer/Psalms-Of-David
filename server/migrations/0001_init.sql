-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0001
-- This file is the source of truth for the schema. The entities in src/entities/
-- follow it. Times are Unix milliseconds (INTEGER), paths are relative to the
-- library. foreign_keys, WAL and busy_timeout are set per connection in src/db.rs.

CREATE TABLE profile (
    id          INTEGER PRIMARY KEY,
    name        TEXT    NOT NULL COLLATE NOCASE UNIQUE,
    avatar      TEXT,
    pin_hash    TEXT,
    is_admin    INTEGER NOT NULL DEFAULT 0 CHECK (is_admin IN (0, 1)),
    max_age     INTEGER CHECK (max_age IS NULL OR max_age >= 0),
    created_at  INTEGER NOT NULL
) STRICT;

-- At most 25 profiles in this migration; 0002 raises the limit to 150. The
-- server turns the abort into an error.
CREATE TRIGGER profile_limit BEFORE INSERT ON profile
WHEN (SELECT count(*) FROM profile) >= 25
BEGIN
    SELECT RAISE(ABORT, 'profile_limit_reached');
END;

CREATE TABLE author (
    id    INTEGER PRIMARY KEY,
    name  TEXT NOT NULL COLLATE NOCASE UNIQUE
) STRICT;

CREATE TABLE book (
    id         INTEGER PRIMARY KEY,
    title      TEXT    NOT NULL,
    author_id  INTEGER NOT NULL REFERENCES author (id),
    folder     TEXT    NOT NULL UNIQUE,
    min_age    INTEGER CHECK (min_age IS NULL OR min_age >= 0),
    added_at   INTEGER NOT NULL
) STRICT;

CREATE TABLE track (
    id           INTEGER PRIMARY KEY,
    book_id      INTEGER NOT NULL REFERENCES book (id) ON DELETE CASCADE,
    disc         INTEGER NOT NULL CHECK (disc >= 1),
    number       INTEGER NOT NULL CHECK (number >= 1),
    title        TEXT    NOT NULL,
    file         TEXT    NOT NULL,
    duration_ms  INTEGER NOT NULL CHECK (duration_ms >= 0),
    size_bytes   INTEGER NOT NULL CHECK (size_bytes >= 0),
    mtime        INTEGER NOT NULL,
    UNIQUE (book_id, disc, number),
    UNIQUE (book_id, file)
) STRICT;

CREATE TABLE progress (
    profile_id   INTEGER NOT NULL REFERENCES profile (id) ON DELETE CASCADE,
    book_id      INTEGER NOT NULL REFERENCES book (id)    ON DELETE CASCADE,
    track_id     INTEGER NOT NULL REFERENCES track (id)   ON DELETE CASCADE,
    position_ms  INTEGER NOT NULL DEFAULT 0 CHECK (position_ms >= 0),
    finished     INTEGER NOT NULL DEFAULT 0 CHECK (finished IN (0, 1)),
    updated_at   INTEGER NOT NULL,
    PRIMARY KEY (profile_id, book_id)
) STRICT;

CREATE INDEX progress_recent ON progress (profile_id, updated_at DESC);

CREATE TABLE favorite (
    profile_id  INTEGER NOT NULL REFERENCES profile (id) ON DELETE CASCADE,
    book_id     INTEGER NOT NULL REFERENCES book (id)    ON DELETE CASCADE,
    added_at    INTEGER NOT NULL,
    PRIMARY KEY (profile_id, book_id)
) STRICT;

-- A device is not the same as a profile: the key belongs to the device, the
-- profile is chosen by the person in the app.
CREATE TABLE device (
    id          INTEGER PRIMARY KEY,
    name        TEXT    NOT NULL,
    wg_pubkey   TEXT    NOT NULL UNIQUE,
    tunnel_ip   TEXT    NOT NULL UNIQUE,
    created_by  INTEGER REFERENCES profile (id) ON DELETE SET NULL,
    created_at  INTEGER NOT NULL,
    last_seen   INTEGER,
    revoked_at  INTEGER
) STRICT;

-- One-time codes with which an admin enrolls a new device.
CREATE TABLE enrollment (
    code_hash   TEXT    PRIMARY KEY,
    created_by  INTEGER NOT NULL REFERENCES profile (id) ON DELETE CASCADE,
    expires_at  INTEGER NOT NULL,
    used_at     INTEGER
) STRICT;
