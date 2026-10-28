-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0011: the queue of uploaded archives (project owner, 2026-10-28: "Upload
-- zip/tar/rar" and "put the items into a queue"). An archive is stored, then unpacked and
-- imported one after the other by a worker; the state of each is kept here so the page can
-- show the queue and a restart does not lose it.
CREATE TABLE upload_job (
    id          INTEGER PRIMARY KEY,
    name        TEXT    NOT NULL,
    size_bytes  INTEGER NOT NULL DEFAULT 0,
    state       TEXT    NOT NULL DEFAULT 'queued' CHECK (state IN ('queued', 'working', 'done', 'failed')),
    message     TEXT    NOT NULL DEFAULT '',
    created_by  INTEGER REFERENCES profile(id) ON DELETE SET NULL,
    created_at  INTEGER NOT NULL,
    finished_at INTEGER
);
