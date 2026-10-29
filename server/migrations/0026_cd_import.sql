-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0026: importing audio CDs that an app has ripped (docs/spec-cd-import-api.md). Nothing is built into
-- the program (project owner, 2026-10-28): the format, the naming and the tags are settings, shown on the settings
-- page in the group "CD import". The apps read them with GET /api/v1/cd/settings and encode the same way.
--   cd_format                 the file format (mp3; the list of values is open for opus later)
--   cd_bitrate_kbps           constant bit rate in kbit/s
--   cd_sample_rate_hz         sample rate in Hz
--   cd_channel_mode           joint_stereo, stereo or mono
--   cd_quality                encoder quality 0 (best) to 9 (fastest), LAME's -q
--   cd_id3_version            2.4 or 2.3; ID3v2 is binding, ID3v1 alone is never written
--   cd_file_pattern           file name without the ending; {author} {title} {narrator} {year} {disc} {track} {track_title}  ({album} = {title})
--   cd_folder_pattern         folders below the import folder, separated by /; {author} {title} {narrator} {year}
--   cd_disc_folder_pattern    folder for one disc of a work with several discs; empty = all discs in the book folder
--   cd_title_pattern          the title tag: file_name (the file name without the ending) or track_title
--   cd_track_scheme           disc_times_100 (disc 2 track 5 = 205) or plain
--   cd_genre                  the genre tag of an audiobook (it should contain a word of audiobook_genres)
--   cd_cover_px               edge length of the cover in pixels
--   cd_eject_after            1 = the app ejects the disc after a successful import
--   cd_target_kind            audiobook or music: what a CD is imported as when the app does not say
--   cd_musicbrainz_url, cd_coverart_url   addresses of the lookup services; empty = the public ones
--   cd_udev                   1 = the Linux desktop app detects drives and discs by udev events instead of polling (0)
--   cd_log_keep               how many entries of the import log (cd_job) are kept (50)
--   musicbrainz_contact       contact for the User-Agent of MusicBrainz; without it there is no lookup
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_format', 'mp3', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_bitrate_kbps', '64', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_sample_rate_hz', '24000', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_channel_mode', 'joint_stereo', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_quality', '3', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_id3_version', '2.4', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_file_pattern', '{author}_{title}_CD{disc:02}_TR{track:02}', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_folder_pattern', '{author}/{title}', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_disc_folder_pattern', 'CD {disc}', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_title_pattern', 'file_name', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_track_scheme', 'disc_times_100', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_genre', 'Audiobook', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_cover_px', '500', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_eject_after', '1', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_target_kind', 'audiobook', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_musicbrainz_url', '', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_coverart_url', '', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_udev', '0', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('cd_log_keep', '50', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('musicbrainz_contact', '', 1);

-- Answers of MusicBrainz and the Cover Art Archive, so that a second look does not ask again. found = 0 is a
-- remembered "nothing there" (an empty body).
CREATE TABLE IF NOT EXISTS cd_cache (
    key        TEXT PRIMARY KEY,
    found      INTEGER NOT NULL,
    body       BLOB NOT NULL,
    fetched_ms INTEGER NOT NULL
);

-- One CD on its way into the library: announced by an app, ripped there, uploaded here, placed and imported.
CREATE TABLE IF NOT EXISTS cd_job (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id      INTEGER NOT NULL,
    device       TEXT NOT NULL,
    phase        TEXT NOT NULL,
    track        INTEGER NOT NULL DEFAULT 0,
    tracks_total INTEGER NOT NULL DEFAULT 0,
    author       TEXT NOT NULL DEFAULT '',
    album        TEXT NOT NULL DEFAULT '',
    disc         INTEGER NOT NULL DEFAULT 1,
    discs_total  INTEGER NOT NULL DEFAULT 1,
    error        TEXT NOT NULL DEFAULT '',
    message      TEXT NOT NULL DEFAULT '',
    import_ran   INTEGER NOT NULL DEFAULT 0,
    created_ms   INTEGER NOT NULL,
    updated_ms   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS cd_job_user ON cd_job (user_id, updated_ms);
