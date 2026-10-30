-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0053: what became of a pairing code (project owner, 2026-10-30: the pairing request of the Linux app left no trace on the server).
-- `device_id` is the device that redeemed the code, `revoked_at` the moment an administrator withdrew a code that was still open. The administration
-- lists the codes of the last days with their state (open, used, expired, revoked). The code itself is never stored, only its hash. The audit
-- triggers of the table (0028) keep recording who changed a row.
ALTER TABLE enrollment ADD COLUMN device_id INTEGER REFERENCES device (id) ON DELETE SET NULL;
ALTER TABLE enrollment ADD COLUMN revoked_at INTEGER;
CREATE INDEX enrollment_created ON enrollment (created_at);
