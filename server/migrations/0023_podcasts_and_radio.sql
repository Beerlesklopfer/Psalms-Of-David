-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0023: podcasts and radio (project owner, 2026-10-28: both in the side bar of the app, the
-- devices of an account kept in step; what a child adds waits for a release). Feeds and stations are shared
-- by all; what belongs to an account (subscriptions, favourites, what was heard) is per profile and lives on
-- the server, so every device of the account reads and writes the same.
CREATE TABLE feed (
    id              INTEGER PRIMARY KEY,
    url             TEXT    NOT NULL UNIQUE,
    title           TEXT    NOT NULL,
    site_url        TEXT,
    image_url       TEXT,
    description     TEXT    NOT NULL DEFAULT '',
    last_fetched_at INTEGER,
    last_error      TEXT
) STRICT;

CREATE TABLE episode (
    id           INTEGER PRIMARY KEY,
    feed_id      INTEGER NOT NULL REFERENCES feed (id) ON DELETE CASCADE,
    guid         TEXT    NOT NULL,
    title        TEXT    NOT NULL,
    audio_url    TEXT    NOT NULL,
    published_at INTEGER NOT NULL,
    duration_s   INTEGER,
    description  TEXT    NOT NULL DEFAULT '',
    UNIQUE (feed_id, guid)
) STRICT;
CREATE INDEX episode_newest ON episode (feed_id, published_at DESC);

-- `approved` is 0 while a child's request waits for an administrator.
CREATE TABLE subscription (
    profile_id INTEGER NOT NULL REFERENCES profile (id) ON DELETE CASCADE,
    feed_id    INTEGER NOT NULL REFERENCES feed (id) ON DELETE CASCADE,
    added_at   INTEGER NOT NULL,
    approved   INTEGER NOT NULL DEFAULT 1 CHECK (approved IN (0, 1)),
    PRIMARY KEY (profile_id, feed_id)
) STRICT;

CREATE TABLE episode_state (
    profile_id  INTEGER NOT NULL REFERENCES profile (id) ON DELETE CASCADE,
    episode_id  INTEGER NOT NULL REFERENCES episode (id) ON DELETE CASCADE,
    position_ms INTEGER NOT NULL DEFAULT 0 CHECK (position_ms >= 0),
    finished    INTEGER NOT NULL DEFAULT 0 CHECK (finished IN (0, 1)),
    updated_at  INTEGER NOT NULL,
    PRIMARY KEY (profile_id, episode_id)
) STRICT;

CREATE TABLE station (
    id         INTEGER PRIMARY KEY,
    stream_url TEXT NOT NULL UNIQUE,
    name       TEXT NOT NULL,
    homepage   TEXT,
    logo_url   TEXT
) STRICT;

CREATE TABLE station_favorite (
    profile_id INTEGER NOT NULL REFERENCES profile (id) ON DELETE CASCADE,
    station_id INTEGER NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    position   INTEGER NOT NULL,
    added_at   INTEGER NOT NULL,
    approved   INTEGER NOT NULL DEFAULT 1 CHECK (approved IN (0, 1)),
    PRIMARY KEY (profile_id, station_id)
) STRICT;

-- Settings: how often the feeds are read (minutes; 0 = never by itself), where stations are searched
-- (the address of a directory; empty = no search), the largest feed read, in bytes.
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('feed_refresh_min', '60', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('radio_directory_url', '', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('feed_max_bytes', '5242880', 1);
