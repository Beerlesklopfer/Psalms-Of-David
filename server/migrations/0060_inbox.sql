-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0060: the inbox of the CDs (docs/spec-cd-inbox.md, "The inbox"; project owner, 2026-10-31).
--   inbox_item       one message: a `notice` (every recipient has their own read mark) or a `task` (one decision for everybody).
--                    `code` and `args` (JSON) are all it holds: the apps make the text in their language, so no prose is stored.
--                    created_at (the audit field) is the time it was made; resolved_* say who decided a task and how
--                    (`approved`, `rejected`, `expired`); `speak` (`starter` or `home`) and `device` (the label of the starter's device)
--                    are for the spoken messages of a later phase.
--   inbox_recipient  who gets an item, and since when they have read it; `decide` is 1 when the person may decide the task (a
--                    starter without the right to approve sees the entry of the job, without buttons)
--   inbox_keep_days  items older than this are removed (90; 1 to 3650); a task nobody decided yet is kept
--   cd_job.starter_device  the label of the device an app started the rip with (empty: web, MCP, the drive itself)
CREATE TABLE inbox_item (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    kind          TEXT NOT NULL CHECK (kind IN ('notice', 'task')),
    topic         TEXT NOT NULL DEFAULT 'cd',
    job_id        INTEGER,
    code          TEXT NOT NULL,
    args          TEXT NOT NULL DEFAULT '{}',
    speak         TEXT CHECK (speak IS NULL OR speak IN ('starter', 'home')),
    device        TEXT NOT NULL DEFAULT '',
    resolved_at   INTEGER,
    resolved_by   INTEGER,
    resolved_name TEXT NOT NULL DEFAULT '',
    resolution    TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_by TEXT
);
CREATE INDEX inbox_item_job ON inbox_item (job_id);
CREATE TRIGGER audit_inbox_item_ai AFTER INSERT ON inbox_item FOR EACH ROW BEGIN
    UPDATE inbox_item SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_inbox_item_au AFTER UPDATE ON inbox_item FOR EACH ROW BEGIN
    UPDATE inbox_item SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;

CREATE TABLE inbox_recipient (
    item_id    INTEGER NOT NULL REFERENCES inbox_item (id) ON DELETE CASCADE,
    profile_id INTEGER NOT NULL REFERENCES profile (id) ON DELETE CASCADE,
    read_at    INTEGER,
    decide     INTEGER NOT NULL DEFAULT 0 CHECK (decide IN (0, 1)),
    created_at INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_by TEXT,
    PRIMARY KEY (item_id, profile_id)
);
CREATE INDEX inbox_recipient_profile ON inbox_recipient (profile_id, item_id);
CREATE TRIGGER audit_inbox_recipient_ai AFTER INSERT ON inbox_recipient FOR EACH ROW BEGIN
    UPDATE inbox_recipient SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE item_id IS NEW.item_id AND profile_id IS NEW.profile_id;
END;
CREATE TRIGGER audit_inbox_recipient_au AFTER UPDATE ON inbox_recipient FOR EACH ROW BEGIN
    UPDATE inbox_recipient SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE item_id IS NEW.item_id AND profile_id IS NEW.profile_id;
END;

ALTER TABLE cd_job ADD COLUMN starter_device TEXT NOT NULL DEFAULT '';
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('inbox_keep_days', '90', 1);
