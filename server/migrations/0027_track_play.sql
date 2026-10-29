-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0027: how often a profile has listened to a track to its end, and when last (project owner, 2026-10-28:
-- statistics in the app). `progress` keeps only where somebody stopped; this keeps the history per track.
-- `armed` is 1 while the next time the end is reached counts. A track counts once per pass: after it was counted the
-- row is disarmed until the listener goes back before the middle of the track (rewinds or starts again).
CREATE TABLE track_play (
    profile_id     INTEGER NOT NULL REFERENCES profile (id) ON DELETE CASCADE,
    track_id       INTEGER NOT NULL REFERENCES track (id) ON DELETE CASCADE,
    plays          INTEGER NOT NULL DEFAULT 0 CHECK (plays >= 0),
    last_played_at INTEGER,
    armed          INTEGER NOT NULL DEFAULT 1 CHECK (armed IN (0, 1)),
    PRIMARY KEY (profile_id, track_id)
) STRICT;
CREATE INDEX track_play_track ON track_play (track_id);

-- When a book was released (project owner, 2026-10-28: the start page of the app has a section "New" for each user).
-- A trigger keeps it, so every way of releasing is counted. Books that were released before start from when they were added.
ALTER TABLE book ADD COLUMN released_at INTEGER;
UPDATE book SET released_at = added_at WHERE released = 1;
CREATE TRIGGER book_released_at_set AFTER UPDATE OF released ON book FOR EACH ROW WHEN NEW.released = 1 AND OLD.released = 0
BEGIN
    UPDATE book SET released_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) WHERE id = NEW.id;
END;
CREATE TRIGGER book_released_at_clear AFTER UPDATE OF released ON book FOR EACH ROW WHEN NEW.released = 0 AND OLD.released = 1
BEGIN
    UPDATE book SET released_at = NULL WHERE id = NEW.id;
END;

-- How many days a book counts as new (a setting of the pages).
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('new_days', '30', 1);
