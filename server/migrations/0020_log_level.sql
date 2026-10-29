-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0020: the log level as a setting (project owner, 2026-10-28: "Setting für Loglevel im
-- Frontend und Kommandozeile"). It applies at once when saved in the pages; at the start a
-- command line flag, DAVID_LOG or /etc/david.toml wins over it.
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('log_level', 'info', 1);
