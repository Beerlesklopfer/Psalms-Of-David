-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0006: the picture of a user. At most 256 KiB; the server checks the
-- file type by its first bytes before it stores the picture, and serves it only
-- as one of these types. The old column profile.avatar stays unused.
CREATE TABLE avatar (
    profile_id  INTEGER PRIMARY KEY REFERENCES profile (id) ON DELETE CASCADE,
    mime        TEXT    NOT NULL CHECK (mime IN ('image/png', 'image/jpeg', 'image/webp')),
    data        BLOB    NOT NULL CHECK (length(data) BETWEEN 1 AND 262144),
    updated_at  INTEGER NOT NULL
) STRICT;
