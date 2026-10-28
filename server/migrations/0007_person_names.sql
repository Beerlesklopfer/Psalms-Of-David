-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0007: first and last name of a user. In a family with different
-- surnames (patchwork families) the name that is shown and used to sign in is the
-- nickname, which stays in `profile.name`; first and last name are for the
-- family's own order and are optional.
ALTER TABLE profile ADD COLUMN first_name TEXT;
ALTER TABLE profile ADD COLUMN last_name TEXT;
