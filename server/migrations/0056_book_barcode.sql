-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0056: the EAN/barcode of an album (project owner, 2026-10-30: "the app can scan them and find out whether the album is present").
-- `barcode` is the digits as cleaned from the tag `Barcode`; `gtin` is the same code as 14 digits (a UPC-A and the EAN-13 with a leading 0 are one product), NULL when the album has none.
-- Existing rows are filled by the next rescan (or a tag edit in the administration); nothing is backfilled here, the files hold the truth.
ALTER TABLE book ADD COLUMN barcode TEXT;
ALTER TABLE book ADD COLUMN gtin TEXT;
CREATE INDEX book_gtin ON book (gtin) WHERE gtin IS NOT NULL;
