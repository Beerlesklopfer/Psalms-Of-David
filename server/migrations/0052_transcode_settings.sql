-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0052: the transcoding to MP3 for devices that cannot play a format (project owner, 2026-10-30; docs/spec-transcoding.md).
-- `transcode_kbps` is the bitrate of the MP3 (the nearest LAME offers), `transcode_cache_mb` the size limit of the cache of the made files and
-- `transcode_parallel` how many files are transcoded at the same time (read at the start of the server).
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('transcode_kbps', '128', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('transcode_cache_mb', '2048', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('transcode_parallel', '2', 1);
