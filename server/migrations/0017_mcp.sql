-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0017: access for a remote LLM over MCP (project owner, 2026-10-28: "--mcp, dann
-- könnte per remote ein LLM die ID-Tags berichtigen"). A token is shown once; only its SHA-256 is
-- kept. Every tag change made through MCP is logged with the old and the new value.
CREATE TABLE mcp_token (
    id           INTEGER PRIMARY KEY,
    name         TEXT    NOT NULL,
    token_hash   TEXT    NOT NULL UNIQUE,
    can_write    INTEGER NOT NULL DEFAULT 0,
    created_by   INTEGER REFERENCES profile(id) ON DELETE SET NULL,
    created_at   INTEGER NOT NULL,
    last_used_at INTEGER
);

CREATE TABLE tag_change (
    id         INTEGER PRIMARY KEY,
    at         INTEGER NOT NULL,
    token_name TEXT    NOT NULL,
    book_id    INTEGER NOT NULL,
    file       TEXT    NOT NULL,
    field      TEXT    NOT NULL,
    old_value  TEXT    NOT NULL,
    new_value  TEXT    NOT NULL
);
CREATE INDEX tag_change_book ON tag_change(book_id, id);
