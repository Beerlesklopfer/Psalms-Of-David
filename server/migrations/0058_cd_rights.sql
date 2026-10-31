-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0058: who may do what with CDs (docs/spec-cd-inbox.md, "Rights per person"; project owner, 2026-10-31).
--   cd_may_insert   may start a rip from an app or the web and is told about their own CDs (administrators 1, others 0)
--   cd_no_approval  a CD of theirs goes into the library when it is read; else it waits for approval (administrators 1, others 0)
--   cd_may_approve  may decide the CDs of others and is told about CDs put in at the server (administrators 1, others 0)
--   cd_auto_approval   what a CD put in at the server needs: 'wait' (wait for approval, the careful default) or 'now' (import at once)
-- Children and guests never hold a right (a trigger refuses it, and turning someone into a child or guest takes the rights away);
-- somebody who becomes an administrator gets all three; at least one profile keeps cd_may_approve (otherwise nobody could ever decide).
ALTER TABLE profile ADD COLUMN cd_may_insert INTEGER NOT NULL DEFAULT 0 CHECK (cd_may_insert IN (0, 1));
ALTER TABLE profile ADD COLUMN cd_no_approval INTEGER NOT NULL DEFAULT 0 CHECK (cd_no_approval IN (0, 1));
ALTER TABLE profile ADD COLUMN cd_may_approve INTEGER NOT NULL DEFAULT 0 CHECK (cd_may_approve IN (0, 1));
UPDATE profile SET cd_may_insert = 1, cd_no_approval = 1, cd_may_approve = 1 WHERE role = 'admin';

INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_auto_approval', 'wait', 1);

CREATE TRIGGER profile_cd_rights_insert BEFORE INSERT ON profile
WHEN NEW.role IN ('child', 'guest') AND (NEW.cd_may_insert = 1 OR NEW.cd_no_approval = 1 OR NEW.cd_may_approve = 1)
BEGIN
    SELECT RAISE(ABORT, 'cd_rights_role');
END;

CREATE TRIGGER profile_cd_rights_update BEFORE UPDATE OF cd_may_insert, cd_no_approval, cd_may_approve ON profile
WHEN NEW.role IN ('child', 'guest') AND (NEW.cd_may_insert = 1 OR NEW.cd_no_approval = 1 OR NEW.cd_may_approve = 1)
BEGIN
    SELECT RAISE(ABORT, 'cd_rights_role');
END;

CREATE TRIGGER profile_last_cd_approver_update BEFORE UPDATE OF cd_may_approve ON profile
WHEN OLD.cd_may_approve = 1 AND NEW.cd_may_approve = 0
     AND (SELECT count(*) FROM profile WHERE cd_may_approve = 1 AND id <> OLD.id) = 0
BEGIN
    SELECT RAISE(ABORT, 'last_cd_approver');
END;

CREATE TRIGGER profile_last_cd_approver_delete BEFORE DELETE ON profile
WHEN OLD.cd_may_approve = 1 AND (SELECT count(*) FROM profile WHERE cd_may_approve = 1 AND id <> OLD.id) = 0
     -- (the last administrator has his own refusal, which must be the one that is told)
     AND NOT (OLD.role = 'admin' AND (SELECT count(*) FROM profile WHERE role = 'admin') <= 1)
BEGIN
    SELECT RAISE(ABORT, 'last_cd_approver');
END;

-- A new administrator gets the careful defaults of an administrator; somebody who becomes a child or guest loses the rights.
CREATE TRIGGER profile_cd_rights_admin_insert AFTER INSERT ON profile
WHEN NEW.role = 'admin'
BEGIN
    UPDATE profile SET cd_may_insert = 1, cd_no_approval = 1, cd_may_approve = 1 WHERE id = NEW.id;
END;

CREATE TRIGGER profile_cd_rights_admin_promote AFTER UPDATE OF role ON profile
WHEN NEW.role = 'admin' AND OLD.role <> 'admin'
BEGIN
    UPDATE profile SET cd_may_insert = 1, cd_no_approval = 1, cd_may_approve = 1 WHERE id = NEW.id;
END;

CREATE TRIGGER profile_cd_rights_demote AFTER UPDATE OF role ON profile
WHEN NEW.role IN ('child', 'guest') AND (NEW.cd_may_insert = 1 OR NEW.cd_no_approval = 1 OR NEW.cd_may_approve = 1)
BEGIN
    UPDATE profile SET cd_may_insert = 0, cd_no_approval = 0, cd_may_approve = 0 WHERE id = NEW.id;
END;
