-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0010: the blurb of a book ("Klappentext": the text of the back cover or
-- the publisher's description, not a table of contents), written by an administrator
-- in the edit window of the book. Plain text, at most 4000 characters (checked by the
-- server).
ALTER TABLE book ADD COLUMN description TEXT;
