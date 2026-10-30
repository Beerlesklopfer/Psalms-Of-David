-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0051: the format of every audio file (project owner, 2026-10-30: "the database needs the file format, and the mime type";
-- the album Caipiranha is Ogg Vorbis and did not play in the Linux app). `format` is the container/file kind (mp3, ogg, opus, flac, m4a, ...),
-- `codec` the coding (mpeg, vorbis, opus, flac, aac, pcm, ...), `mime_type` what the stream is served with. Old rows hold '' / 0 until the next
-- import has looked at their files (a row with an empty format is filled in at once, whether or not the file changed). The audit trigger of the
-- table (0028) records who changed the row.
ALTER TABLE track ADD COLUMN format TEXT NOT NULL DEFAULT '';
ALTER TABLE track ADD COLUMN codec TEXT NOT NULL DEFAULT '';
ALTER TABLE track ADD COLUMN mime_type TEXT NOT NULL DEFAULT '';
ALTER TABLE track ADD COLUMN bitrate_kbps INTEGER NOT NULL DEFAULT 0;
ALTER TABLE track ADD COLUMN sample_rate INTEGER NOT NULL DEFAULT 0;
ALTER TABLE track ADD COLUMN channels INTEGER NOT NULL DEFAULT 0;
CREATE INDEX track_format ON track (format);
