-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0059: the state machine of a CD job (docs/spec-cd-inbox.md, "The flow of one CD"; project owner, 2026-10-31).
--   cd_job.cd_state    ripping | retry_wait | ripped | awaiting_approval | imported | rejected | present | cancelled | failed | skipped
--                      (`failed` is a failure that no new try can mend, `skipped` an entry of the log of the automatic import);
--                      changed only by cdflow::Store::cd_move
--   cd_job.reason      why a job is rejected: `rejected` (a person decided) or `expired`; empty otherwise
--   cd_job.decided_by  the profile that decided (approve, reject, give up), NULL for the server's own moves; decided_name keeps the
--                      nickname so the log still says who after a profile is deleted; decided_ms is the time
--   cd_job.request     the request of a rip of the server as JSON, so that `retry` starts the same rip again
--   cd_held            the CDs that wait for approval: the folder with the files (outside the library), what finalize needs
--                      to place them, and since when
--   cd_held_keep_days  how long a CD waits for approval before it expires (60; 1 to 365)
--   cd_held_folder     where the folders of the held CDs are made; empty = below the staging folder of the CD import
ALTER TABLE cd_job ADD COLUMN cd_state TEXT NOT NULL DEFAULT 'ripping';
ALTER TABLE cd_job ADD COLUMN reason TEXT NOT NULL DEFAULT '';
ALTER TABLE cd_job ADD COLUMN decided_by INTEGER;
ALTER TABLE cd_job ADD COLUMN decided_name TEXT NOT NULL DEFAULT '';
ALTER TABLE cd_job ADD COLUMN decided_ms INTEGER NOT NULL DEFAULT 0;
ALTER TABLE cd_job ADD COLUMN request TEXT NOT NULL DEFAULT '';
UPDATE cd_job SET cd_state = CASE phase
    WHEN 'done' THEN 'imported'
    WHEN 'skipped' THEN 'skipped'
    WHEN 'failed' THEN CASE error WHEN 'cancelled' THEN 'cancelled' ELSE 'failed' END
    ELSE 'ripping' END;
CREATE INDEX IF NOT EXISTS cd_job_state ON cd_job (cd_state);

CREATE TABLE cd_held (
    job_id   INTEGER PRIMARY KEY REFERENCES cd_job (id) ON DELETE CASCADE,
    folder   TEXT NOT NULL,
    context  TEXT NOT NULL,
    held_ms  INTEGER NOT NULL,
    created_at INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_by TEXT
);
CREATE TRIGGER audit_cd_held_ai AFTER INSERT ON cd_held FOR EACH ROW BEGIN
    UPDATE cd_held SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE job_id IS NEW.job_id;
END;
CREATE TRIGGER audit_cd_held_au AFTER UPDATE ON cd_held FOR EACH ROW BEGIN
    UPDATE cd_held SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE job_id IS NEW.job_id;
END;

INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_held_keep_days', '60', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_held_folder', '', 1);
