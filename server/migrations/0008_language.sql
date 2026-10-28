-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0008: the language a user chose in their profile. NULL means
-- "follow the browser or the device": English comes first, German second.
ALTER TABLE profile ADD COLUMN language TEXT CHECK (language IS NULL OR language IN ('en', 'de'));
