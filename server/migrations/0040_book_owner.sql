-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0040: a book has an owner (project owner, 2026-10-29): the family member it belongs to ("Musik von <Nickname>"). The owner is
-- another thing than the release: it says whom a book belongs to, not who may see it (release, age, exceptions and `book_block` do that).
-- It is chosen in the window of a book and with the MCP tool `set_owner`; `assign_owners` fills it from the folders (the first folder of a
-- music album is the complete name of a member). A member that is deleted leaves its books without an owner.
ALTER TABLE book ADD COLUMN owner_id INTEGER REFERENCES profile (id) ON DELETE SET NULL;
CREATE INDEX book_owner ON book (owner_id);
