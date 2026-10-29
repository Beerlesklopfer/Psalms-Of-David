-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0018: the help text a remote LLM gets over MCP (project owner, 2026-10-28: "Das
-- Help-Prompt sollte bei MCP einstellbar sein"). One row at most; without it the built-in text is used.
CREATE TABLE mcp_prompt (
    id   INTEGER PRIMARY KEY CHECK (id = 1),
    text TEXT NOT NULL CHECK (length(text) BETWEEN 1 AND 8000)
);
