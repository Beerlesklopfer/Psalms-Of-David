-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- David, schema 0022: settings that must not be built into the program (project owner, 2026-10-28:
-- "niemals hart coden"). An empty value means "not set". The flags, the environment and
-- /etc/david.toml win over these.
--   tls_cert, tls_key  files of a certificate and key from somebody else (Let's Encrypt, ...)
--   tls_host           the name devices connect to when the certificate comes from a CA
--   tls_reload_s       how often a renewed certificate is looked for, in seconds
--   quarantine_folder  the name of the quarantine folder inside each import folder
--   audiobook_genres   words in the genre tag that make an album an audiobook, separated by commas
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('tls_cert', '', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('tls_key', '', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('tls_host', '', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('tls_reload_s', '', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('quarantine_folder', '.quarantine', 1);
INSERT OR IGNORE INTO setting (key, value, confirmed) VALUES ('audiobook_genres', 'hörbuch,hoerbuch,hörspiel,hoerspiel,lesung,audiobook,audio book,spoken', 1);
