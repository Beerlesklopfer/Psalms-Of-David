-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0009: the birthday of a user (project owner, 2026-10-27: "Age limit is
-- a date picker of the birthday; the limitations are then calculated automatically").
-- With a birthday, profile.max_age is not typed in any more but follows from it: it is
-- set when the birthday is set and renewed by the server every hour, so a child's
-- limit rises on its own birthday. Without a birthday, max_age works as before.
ALTER TABLE profile ADD COLUMN birth_date TEXT
    CHECK (birth_date IS NULL OR birth_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]');
