#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
# SPDX-License-Identifier: AGPL-3.0-or-later
# Writes dist/SHA256SUMS for everything in dist/ and, if GPG_KEY_ID is set (a key in the keyring),
# a detached signature dist/SHA256SUMS.asc. Anyone can then check a download:
#   sha256sum -c SHA256SUMS      and      gpg --verify SHA256SUMS.asc SHA256SUMS
set -euo pipefail
cd "$(dirname "$0")/../dist"
rm -f SHA256SUMS SHA256SUMS.asc
sha256sum $(ls | grep -v '^SHA256SUMS') > SHA256SUMS
if [ -n "${GPG_KEY_ID:-}" ]; then
    gpg --batch --yes --armor --local-user "$GPG_KEY_ID" --detach-sign SHA256SUMS
fi
ls -l
