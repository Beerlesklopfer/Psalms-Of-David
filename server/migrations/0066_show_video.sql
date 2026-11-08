-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0066: the slideshow as a video for a television or a USB stick (docs/spec-pictures.md, 7; project owner, 2026-11-08).
--   show_video   one video a person asked for on /pictures: `query` is the choice (the query of the project), the server makes it in the
--                background, one after the other (`state` queued, running, done, failed; `progress` 0 to 1000), keeps the file at
--                `<data dir>/show-videos/<id>.mp4` until the person deletes it. A video that was running when the server stopped is made
--                again from the start.
CREATE TABLE show_video (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id     INTEGER NOT NULL REFERENCES profile (id) ON DELETE CASCADE,
    title       TEXT    NOT NULL,
    query       TEXT    NOT NULL,
    lang        TEXT    NOT NULL,
    state       TEXT    NOT NULL DEFAULT 'queued' CHECK (state IN ('queued', 'running', 'done', 'failed')),
    progress    INTEGER NOT NULL DEFAULT 0 CHECK (progress BETWEEN 0 AND 1000),
    error       TEXT,
    pictures    INTEGER NOT NULL DEFAULT 0,
    size_bytes  INTEGER NOT NULL DEFAULT 0,
    duration_ms INTEGER NOT NULL DEFAULT 0,
    finished_at INTEGER,
    created_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0,
    updated_by TEXT
) STRICT;
CREATE INDEX show_video_user ON show_video (user_id, id);
CREATE INDEX show_video_state ON show_video (state, id);
CREATE TRIGGER audit_show_video_ai AFTER INSERT ON show_video FOR EACH ROW BEGIN
    UPDATE show_video SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_show_video_au AFTER UPDATE ON show_video FOR EACH ROW BEGIN
    UPDATE show_video SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;
