#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
# SPDX-License-Identifier: AGPL-3.0-or-later
# Runs every audio check against a recording of a temporary null sink.
#   scripts/verify-all.sh <mp3 file at 64 kbit/s, at least 16 seconds long>
set -euo pipefail
SRC=${1:?usage: verify-all.sh <mp3 file>}
HERE=$(cd "$(dirname "$0")" && pwd)
"$HERE/verify.sh" "$SRC"
for scenario in pause seek volume paused-start; do
    echo "== $scenario"
    "$HERE/verify-controls.sh" "$scenario" "$SRC"
done
