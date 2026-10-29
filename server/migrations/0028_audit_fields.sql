-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0028: every table says when a row was made and last changed and by whom (project owner, 2026-10-28).
--   created_at  when the row was made (Unix ms)
--   created_by  who made it (text: `profile:<id>`, `mcp:<token name>` or `system`); some tables had a `created_by` of their own
--   updated_at  when it was last changed (Unix ms); a value that the program sets itself stays
--   updated_by  who changed it last (the same kind of text)
-- The fields are kept by triggers, so no program code has to remember them. Who acts is told by the table `actor`: the
-- program sets its one row before each statement of the write connection (see src/actor.rs); work of the server itself is `system`.
-- The triggers rely on `recursive_triggers` being OFF (the connection sets it). A table made later must be added the
-- same way; the test `every_table_keeps_the_audit_fields` fails until it is.
CREATE TABLE actor (
    id    INTEGER PRIMARY KEY CHECK (id = 1),
    label TEXT NOT NULL
) STRICT;
INSERT INTO actor (id, label) VALUES (1, 'system');


-- author
ALTER TABLE author ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE author ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE author ADD COLUMN created_by TEXT;
ALTER TABLE author ADD COLUMN updated_by TEXT;
UPDATE author SET created_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER);
UPDATE author SET updated_at = created_at;
CREATE TRIGGER audit_author_ai AFTER INSERT ON author FOR EACH ROW BEGIN
    UPDATE author SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_author_au AFTER UPDATE ON author FOR EACH ROW BEGIN
    UPDATE author SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;

-- avatar
ALTER TABLE avatar ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE avatar ADD COLUMN created_by TEXT;
ALTER TABLE avatar ADD COLUMN updated_by TEXT;
UPDATE avatar SET created_at = updated_at;
CREATE TRIGGER audit_avatar_ai AFTER INSERT ON avatar FOR EACH ROW BEGIN
    UPDATE avatar SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE profile_id IS NEW.profile_id;
END;
CREATE TRIGGER audit_avatar_au AFTER UPDATE ON avatar FOR EACH ROW BEGIN
    UPDATE avatar SET updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE profile_id IS NEW.profile_id;
END;

-- bible_anchor
ALTER TABLE bible_anchor ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE bible_anchor ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE bible_anchor ADD COLUMN created_by TEXT;
ALTER TABLE bible_anchor ADD COLUMN updated_by TEXT;
UPDATE bible_anchor SET created_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER);
UPDATE bible_anchor SET updated_at = created_at;
CREATE TRIGGER audit_bible_anchor_ai AFTER INSERT ON bible_anchor FOR EACH ROW BEGIN
    UPDATE bible_anchor SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE book_id IS NEW.book_id AND bible_book IS NEW.bible_book AND chapter IS NEW.chapter AND verse IS NEW.verse;
END;
CREATE TRIGGER audit_bible_anchor_au AFTER UPDATE ON bible_anchor FOR EACH ROW BEGIN
    UPDATE bible_anchor SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE book_id IS NEW.book_id AND bible_book IS NEW.bible_book AND chapter IS NEW.chapter AND verse IS NEW.verse;
END;

-- book
ALTER TABLE book ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE book ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE book ADD COLUMN created_by TEXT;
ALTER TABLE book ADD COLUMN updated_by TEXT;
UPDATE book SET created_at = added_at;
UPDATE book SET updated_at = created_at;
CREATE TRIGGER audit_book_ai AFTER INSERT ON book FOR EACH ROW BEGIN
    UPDATE book SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_book_au AFTER UPDATE ON book FOR EACH ROW BEGIN
    UPDATE book SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;

-- book_release
ALTER TABLE book_release ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE book_release ADD COLUMN updated_by TEXT;
UPDATE book_release SET updated_at = created_at;
CREATE TRIGGER audit_book_release_ai AFTER INSERT ON book_release FOR EACH ROW BEGIN
    UPDATE book_release SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_book_release_au AFTER UPDATE ON book_release FOR EACH ROW BEGIN
    UPDATE book_release SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;

-- cd_cache
ALTER TABLE cd_cache ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE cd_cache ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE cd_cache ADD COLUMN created_by TEXT;
ALTER TABLE cd_cache ADD COLUMN updated_by TEXT;
UPDATE cd_cache SET created_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER);
UPDATE cd_cache SET updated_at = created_at;
CREATE TRIGGER audit_cd_cache_ai AFTER INSERT ON cd_cache FOR EACH ROW BEGIN
    UPDATE cd_cache SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE key IS NEW.key;
END;
CREATE TRIGGER audit_cd_cache_au AFTER UPDATE ON cd_cache FOR EACH ROW BEGIN
    UPDATE cd_cache SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE key IS NEW.key;
END;

-- cd_job
ALTER TABLE cd_job ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE cd_job ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE cd_job ADD COLUMN created_by TEXT;
ALTER TABLE cd_job ADD COLUMN updated_by TEXT;
UPDATE cd_job SET created_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER);
UPDATE cd_job SET updated_at = created_at;
CREATE TRIGGER audit_cd_job_ai AFTER INSERT ON cd_job FOR EACH ROW BEGIN
    UPDATE cd_job SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_cd_job_au AFTER UPDATE ON cd_job FOR EACH ROW BEGIN
    UPDATE cd_job SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;

-- device
ALTER TABLE device ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE device ADD COLUMN updated_by TEXT;
UPDATE device SET updated_at = created_at;
CREATE TRIGGER audit_device_ai AFTER INSERT ON device FOR EACH ROW BEGIN
    UPDATE device SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_device_au AFTER UPDATE ON device FOR EACH ROW BEGIN
    UPDATE device SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;

-- enrollment
ALTER TABLE enrollment ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE enrollment ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE enrollment ADD COLUMN updated_by TEXT;
UPDATE enrollment SET created_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER);
UPDATE enrollment SET updated_at = created_at;
CREATE TRIGGER audit_enrollment_ai AFTER INSERT ON enrollment FOR EACH ROW BEGIN
    UPDATE enrollment SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1))
    WHERE code_hash IS NEW.code_hash;
END;
CREATE TRIGGER audit_enrollment_au AFTER UPDATE ON enrollment FOR EACH ROW BEGIN
    UPDATE enrollment SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE code_hash IS NEW.code_hash;
END;

-- episode
ALTER TABLE episode ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE episode ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE episode ADD COLUMN created_by TEXT;
ALTER TABLE episode ADD COLUMN updated_by TEXT;
UPDATE episode SET created_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER);
UPDATE episode SET updated_at = created_at;
CREATE TRIGGER audit_episode_ai AFTER INSERT ON episode FOR EACH ROW BEGIN
    UPDATE episode SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_episode_au AFTER UPDATE ON episode FOR EACH ROW BEGIN
    UPDATE episode SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;

-- episode_state
ALTER TABLE episode_state ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE episode_state ADD COLUMN created_by TEXT;
ALTER TABLE episode_state ADD COLUMN updated_by TEXT;
UPDATE episode_state SET created_at = updated_at;
CREATE TRIGGER audit_episode_state_ai AFTER INSERT ON episode_state FOR EACH ROW BEGIN
    UPDATE episode_state SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE profile_id IS NEW.profile_id AND episode_id IS NEW.episode_id;
END;
CREATE TRIGGER audit_episode_state_au AFTER UPDATE ON episode_state FOR EACH ROW BEGIN
    UPDATE episode_state SET updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE profile_id IS NEW.profile_id AND episode_id IS NEW.episode_id;
END;

-- favorite
ALTER TABLE favorite ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE favorite ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE favorite ADD COLUMN created_by TEXT;
ALTER TABLE favorite ADD COLUMN updated_by TEXT;
UPDATE favorite SET created_at = added_at;
UPDATE favorite SET updated_at = created_at;
CREATE TRIGGER audit_favorite_ai AFTER INSERT ON favorite FOR EACH ROW BEGIN
    UPDATE favorite SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE profile_id IS NEW.profile_id AND book_id IS NEW.book_id;
END;
CREATE TRIGGER audit_favorite_au AFTER UPDATE ON favorite FOR EACH ROW BEGIN
    UPDATE favorite SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE profile_id IS NEW.profile_id AND book_id IS NEW.book_id;
END;

-- feed
ALTER TABLE feed ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE feed ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE feed ADD COLUMN created_by TEXT;
ALTER TABLE feed ADD COLUMN updated_by TEXT;
UPDATE feed SET created_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER);
UPDATE feed SET updated_at = created_at;
CREATE TRIGGER audit_feed_ai AFTER INSERT ON feed FOR EACH ROW BEGIN
    UPDATE feed SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_feed_au AFTER UPDATE ON feed FOR EACH ROW BEGIN
    UPDATE feed SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;

-- genre
ALTER TABLE genre ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE genre ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE genre ADD COLUMN created_by TEXT;
ALTER TABLE genre ADD COLUMN updated_by TEXT;
UPDATE genre SET created_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER);
UPDATE genre SET updated_at = created_at;
CREATE TRIGGER audit_genre_ai AFTER INSERT ON genre FOR EACH ROW BEGIN
    UPDATE genre SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_genre_au AFTER UPDATE ON genre FOR EACH ROW BEGIN
    UPDATE genre SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;

-- genre_alias
ALTER TABLE genre_alias ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE genre_alias ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE genre_alias ADD COLUMN created_by TEXT;
ALTER TABLE genre_alias ADD COLUMN updated_by TEXT;
UPDATE genre_alias SET created_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER);
UPDATE genre_alias SET updated_at = created_at;
CREATE TRIGGER audit_genre_alias_ai AFTER INSERT ON genre_alias FOR EACH ROW BEGIN
    UPDATE genre_alias SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE tag_text IS NEW.tag_text;
END;
CREATE TRIGGER audit_genre_alias_au AFTER UPDATE ON genre_alias FOR EACH ROW BEGIN
    UPDATE genre_alias SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE tag_text IS NEW.tag_text;
END;

-- ignore_rule
ALTER TABLE ignore_rule ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE ignore_rule ADD COLUMN updated_by TEXT;
UPDATE ignore_rule SET updated_at = created_at;
CREATE TRIGGER audit_ignore_rule_ai AFTER INSERT ON ignore_rule FOR EACH ROW BEGIN
    UPDATE ignore_rule SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_ignore_rule_au AFTER UPDATE ON ignore_rule FOR EACH ROW BEGIN
    UPDATE ignore_rule SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;

-- mcp_prompt
ALTER TABLE mcp_prompt ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE mcp_prompt ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE mcp_prompt ADD COLUMN created_by TEXT;
ALTER TABLE mcp_prompt ADD COLUMN updated_by TEXT;
UPDATE mcp_prompt SET created_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER);
UPDATE mcp_prompt SET updated_at = created_at;
CREATE TRIGGER audit_mcp_prompt_ai AFTER INSERT ON mcp_prompt FOR EACH ROW BEGIN
    UPDATE mcp_prompt SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_mcp_prompt_au AFTER UPDATE ON mcp_prompt FOR EACH ROW BEGIN
    UPDATE mcp_prompt SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;

-- mcp_token
ALTER TABLE mcp_token ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE mcp_token ADD COLUMN updated_by TEXT;
UPDATE mcp_token SET updated_at = created_at;
CREATE TRIGGER audit_mcp_token_ai AFTER INSERT ON mcp_token FOR EACH ROW BEGIN
    UPDATE mcp_token SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_mcp_token_au AFTER UPDATE ON mcp_token FOR EACH ROW BEGIN
    UPDATE mcp_token SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;

-- profile
ALTER TABLE profile ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE profile ADD COLUMN created_by TEXT;
ALTER TABLE profile ADD COLUMN updated_by TEXT;
UPDATE profile SET updated_at = created_at;
CREATE TRIGGER audit_profile_ai AFTER INSERT ON profile FOR EACH ROW BEGIN
    UPDATE profile SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_profile_au AFTER UPDATE ON profile FOR EACH ROW BEGIN
    UPDATE profile SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;

-- progress
ALTER TABLE progress ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE progress ADD COLUMN created_by TEXT;
ALTER TABLE progress ADD COLUMN updated_by TEXT;
UPDATE progress SET created_at = updated_at;
CREATE TRIGGER audit_progress_ai AFTER INSERT ON progress FOR EACH ROW BEGIN
    UPDATE progress SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE profile_id IS NEW.profile_id AND book_id IS NEW.book_id;
END;
CREATE TRIGGER audit_progress_au AFTER UPDATE ON progress FOR EACH ROW BEGIN
    UPDATE progress SET updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE profile_id IS NEW.profile_id AND book_id IS NEW.book_id;
END;

-- role_policy
ALTER TABLE role_policy ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE role_policy ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE role_policy ADD COLUMN created_by TEXT;
ALTER TABLE role_policy ADD COLUMN updated_by TEXT;
UPDATE role_policy SET created_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER);
UPDATE role_policy SET updated_at = created_at;
CREATE TRIGGER audit_role_policy_ai AFTER INSERT ON role_policy FOR EACH ROW BEGIN
    UPDATE role_policy SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE role IS NEW.role;
END;
CREATE TRIGGER audit_role_policy_au AFTER UPDATE ON role_policy FOR EACH ROW BEGIN
    UPDATE role_policy SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE role IS NEW.role;
END;

-- server_identity
ALTER TABLE server_identity ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE server_identity ADD COLUMN created_by TEXT;
ALTER TABLE server_identity ADD COLUMN updated_by TEXT;
UPDATE server_identity SET updated_at = created_at;
CREATE TRIGGER audit_server_identity_ai AFTER INSERT ON server_identity FOR EACH ROW BEGIN
    UPDATE server_identity SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_server_identity_au AFTER UPDATE ON server_identity FOR EACH ROW BEGIN
    UPDATE server_identity SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;

-- session
ALTER TABLE session ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE session ADD COLUMN created_by TEXT;
ALTER TABLE session ADD COLUMN updated_by TEXT;
UPDATE session SET updated_at = created_at;
CREATE TRIGGER audit_session_ai AFTER INSERT ON session FOR EACH ROW BEGIN
    UPDATE session SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE token_hash IS NEW.token_hash;
END;
CREATE TRIGGER audit_session_au AFTER UPDATE ON session FOR EACH ROW BEGIN
    UPDATE session SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE token_hash IS NEW.token_hash;
END;

-- setting
ALTER TABLE setting ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE setting ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE setting ADD COLUMN created_by TEXT;
ALTER TABLE setting ADD COLUMN updated_by TEXT;
UPDATE setting SET created_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER);
UPDATE setting SET updated_at = created_at;
CREATE TRIGGER audit_setting_ai AFTER INSERT ON setting FOR EACH ROW BEGIN
    UPDATE setting SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE key IS NEW.key;
END;
CREATE TRIGGER audit_setting_au AFTER UPDATE ON setting FOR EACH ROW BEGIN
    UPDATE setting SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE key IS NEW.key;
END;

-- station
ALTER TABLE station ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE station ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE station ADD COLUMN created_by TEXT;
ALTER TABLE station ADD COLUMN updated_by TEXT;
UPDATE station SET created_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER);
UPDATE station SET updated_at = created_at;
CREATE TRIGGER audit_station_ai AFTER INSERT ON station FOR EACH ROW BEGIN
    UPDATE station SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_station_au AFTER UPDATE ON station FOR EACH ROW BEGIN
    UPDATE station SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;

-- station_favorite
ALTER TABLE station_favorite ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE station_favorite ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE station_favorite ADD COLUMN created_by TEXT;
ALTER TABLE station_favorite ADD COLUMN updated_by TEXT;
UPDATE station_favorite SET created_at = added_at;
UPDATE station_favorite SET updated_at = created_at;
CREATE TRIGGER audit_station_favorite_ai AFTER INSERT ON station_favorite FOR EACH ROW BEGIN
    UPDATE station_favorite SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE profile_id IS NEW.profile_id AND station_id IS NEW.station_id;
END;
CREATE TRIGGER audit_station_favorite_au AFTER UPDATE ON station_favorite FOR EACH ROW BEGIN
    UPDATE station_favorite SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE profile_id IS NEW.profile_id AND station_id IS NEW.station_id;
END;

-- subscription
ALTER TABLE subscription ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE subscription ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE subscription ADD COLUMN created_by TEXT;
ALTER TABLE subscription ADD COLUMN updated_by TEXT;
UPDATE subscription SET created_at = added_at;
UPDATE subscription SET updated_at = created_at;
CREATE TRIGGER audit_subscription_ai AFTER INSERT ON subscription FOR EACH ROW BEGIN
    UPDATE subscription SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE profile_id IS NEW.profile_id AND feed_id IS NEW.feed_id;
END;
CREATE TRIGGER audit_subscription_au AFTER UPDATE ON subscription FOR EACH ROW BEGIN
    UPDATE subscription SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE profile_id IS NEW.profile_id AND feed_id IS NEW.feed_id;
END;

-- tag_change
ALTER TABLE tag_change ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tag_change ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tag_change ADD COLUMN created_by TEXT;
ALTER TABLE tag_change ADD COLUMN updated_by TEXT;
UPDATE tag_change SET created_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER);
UPDATE tag_change SET updated_at = created_at;
CREATE TRIGGER audit_tag_change_ai AFTER INSERT ON tag_change FOR EACH ROW BEGIN
    UPDATE tag_change SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_tag_change_au AFTER UPDATE ON tag_change FOR EACH ROW BEGIN
    UPDATE tag_change SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;

-- track
ALTER TABLE track ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE track ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE track ADD COLUMN created_by TEXT;
ALTER TABLE track ADD COLUMN updated_by TEXT;
UPDATE track SET created_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER);
UPDATE track SET updated_at = created_at;
CREATE TRIGGER audit_track_ai AFTER INSERT ON track FOR EACH ROW BEGIN
    UPDATE track SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_track_au AFTER UPDATE ON track FOR EACH ROW BEGIN
    UPDATE track SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;

-- track_play
ALTER TABLE track_play ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE track_play ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE track_play ADD COLUMN created_by TEXT;
ALTER TABLE track_play ADD COLUMN updated_by TEXT;
UPDATE track_play SET created_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER);
UPDATE track_play SET updated_at = created_at;
CREATE TRIGGER audit_track_play_ai AFTER INSERT ON track_play FOR EACH ROW BEGIN
    UPDATE track_play SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE profile_id IS NEW.profile_id AND track_id IS NEW.track_id;
END;
CREATE TRIGGER audit_track_play_au AFTER UPDATE ON track_play FOR EACH ROW BEGIN
    UPDATE track_play SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE profile_id IS NEW.profile_id AND track_id IS NEW.track_id;
END;

-- upload_job
ALTER TABLE upload_job ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE upload_job ADD COLUMN updated_by TEXT;
UPDATE upload_job SET updated_at = created_at;
CREATE TRIGGER audit_upload_job_ai AFTER INSERT ON upload_job FOR EACH ROW BEGIN
    UPDATE upload_job SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1))
    WHERE id IS NEW.id;
END;
CREATE TRIGGER audit_upload_job_au AFTER UPDATE ON upload_job FOR EACH ROW BEGIN
    UPDATE upload_job SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE id IS NEW.id;
END;
