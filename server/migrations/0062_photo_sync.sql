-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0062: the settings of the picture sync (docs/spec-photo-sync.md, section 2a; project owner, 2026-11-07: server, profile and
-- Android in one step, unisync and bisync, parents see a child's pictures only when the server and the child's profile allow it, videos
-- with a limit per file). The pictures go into the picture folder of each profile (profile_folder, kind photo).
--   setting photo_*         the server (administrators); the feature is off until it is switched on
--   profile_photo_setting   one row per profile that chose something; without a row the profile has the defaults (off, unisync, Wi-Fi only,
--                           only while charging, videos, deletions never propagated, parents do not look)
INSERT INTO setting (key, value, confirmed) VALUES
    ('photo_sync', 'off', 1),
    ('photo_modes', 'uni', 1),
    ('photo_videos', 'on', 1),
    ('photo_children', 'parents', 1),
    ('photo_parents_may_view', 'off', 1),
    ('photo_max_file_mb', '2048', 1),
    ('photo_quota_gb', '0', 1),
    ('photo_reserve_gb', '0', 1);
CREATE TABLE profile_photo_setting (
    profile_id  INTEGER PRIMARY KEY REFERENCES profile (id) ON DELETE CASCADE,
    enabled     INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    mode        TEXT    NOT NULL DEFAULT 'uni' CHECK (mode IN ('uni', 'bi')),
    network     TEXT    NOT NULL DEFAULT 'wifi' CHECK (network IN ('wifi', 'any')),
    charging    TEXT    NOT NULL DEFAULT 'charging' CHECK (charging IN ('charging', 'always')),
    videos      INTEGER NOT NULL DEFAULT 1 CHECK (videos IN (0, 1)),
    deletes     TEXT    NOT NULL DEFAULT 'never' CHECK (deletes IN ('never', 'trash')),
    parent_view INTEGER NOT NULL DEFAULT 0 CHECK (parent_view IN (0, 1)),
    created_at INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0,
    updated_by TEXT
) STRICT;
CREATE TRIGGER audit_profile_photo_setting_ai AFTER INSERT ON profile_photo_setting FOR EACH ROW BEGIN
    UPDATE profile_photo_setting SET created_at = CASE WHEN NEW.created_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.created_at END, updated_at = CASE WHEN NEW.updated_at = 0 THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = coalesce(NEW.updated_by, (SELECT label FROM actor WHERE id = 1)), created_by = coalesce(NEW.created_by, (SELECT label FROM actor WHERE id = 1))
    WHERE profile_id IS NEW.profile_id;
END;
CREATE TRIGGER audit_profile_photo_setting_au AFTER UPDATE ON profile_photo_setting FOR EACH ROW BEGIN
    UPDATE profile_photo_setting SET updated_at = CASE WHEN NEW.updated_at IS OLD.updated_at THEN CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) ELSE NEW.updated_at END, updated_by = (SELECT label FROM actor WHERE id = 1)
    WHERE profile_id IS NEW.profile_id;
END;
