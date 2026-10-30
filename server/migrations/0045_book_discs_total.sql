-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0045: how many CDs a work has (project owner, 2026-10-30: "three CDs, the fourth is missing", "we cannot set a CD number by hand").
-- The CDs a book has are the discs of its tracks; what the family owns is not the whole set when `discs_total` is bigger than that. NULL: not said.
ALTER TABLE book ADD COLUMN discs_total INTEGER CHECK (discs_total IS NULL OR (discs_total >= 1 AND discs_total <= 99));
