-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0055: how long each person listened, per day and kind of medium (project owner, 2026-10-30: the start page of the app shows the
-- media time of the family as a line chart). Triggers add to it whenever a position report moves forward on the same track (an audiobook or a
-- music track in `progress`, a podcast episode in `episode_state`): the time credited is the smaller of the distance the position moved and the time
-- between the two reports, and at most two minutes, so pausing, seeking and a device that was offline are not counted as listening.
-- `slot` is a quarter of an hour counted from 1970-01-01 00:00 UTC (every time zone of the world starts its hours on a quarter), so the server can
-- put the time on the days of its own clock (the setting `time_zone`, daylight saving time included) when it reads it; `kind` is the kind of the
-- book (`audiobook`, `music`) or `podcast`.
-- Nothing is known about the time before this migration.
CREATE TABLE listening_time (
    profile_id INTEGER NOT NULL REFERENCES profile (id) ON DELETE CASCADE,
    slot       INTEGER NOT NULL CHECK (slot >= 0),
    kind       TEXT    NOT NULL CHECK (length(kind) >= 1),
    ms         INTEGER NOT NULL DEFAULT 0 CHECK (ms >= 0),
    created_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0,
    updated_by TEXT,
    PRIMARY KEY (profile_id, slot, kind)
) STRICT;
CREATE TRIGGER audit_listening_time_ai AFTER INSERT ON listening_time FOR EACH ROW BEGIN
    UPDATE listening_time SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE profile_id IS NEW.profile_id AND slot IS NEW.slot AND kind IS NEW.kind;
END;
CREATE TRIGGER audit_listening_time_au AFTER UPDATE ON listening_time FOR EACH ROW BEGIN
    UPDATE listening_time SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE profile_id IS NEW.profile_id AND slot IS NEW.slot AND kind IS NEW.kind;
END;

CREATE TRIGGER progress_listening AFTER UPDATE OF position_ms, updated_at ON progress FOR EACH ROW
WHEN NEW.track_id = OLD.track_id AND NEW.position_ms > OLD.position_ms AND NEW.updated_at > OLD.updated_at
BEGIN
    INSERT INTO listening_time (profile_id, slot, kind, ms)
    VALUES (NEW.profile_id, NEW.updated_at / 900000, (SELECT kind FROM book WHERE id = NEW.book_id),
            min(NEW.position_ms - OLD.position_ms, NEW.updated_at - OLD.updated_at, 120000))
    ON CONFLICT (profile_id, slot, kind) DO UPDATE SET ms = ms + excluded.ms;
END;

CREATE TRIGGER episode_state_listening AFTER UPDATE OF position_ms, updated_at ON episode_state FOR EACH ROW
WHEN NEW.position_ms > OLD.position_ms AND NEW.updated_at > OLD.updated_at
BEGIN
    INSERT INTO listening_time (profile_id, slot, kind, ms)
    VALUES (NEW.profile_id, NEW.updated_at / 900000, 'podcast',
            min(NEW.position_ms - OLD.position_ms, NEW.updated_at - OLD.updated_at, 120000))
    ON CONFLICT (profile_id, slot, kind) DO UPDATE SET ms = ms + excluded.ms;
END;
