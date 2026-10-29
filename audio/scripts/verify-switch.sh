#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
# SPDX-License-Identifier: AGPL-3.0-or-later
# Measures the change from one track to the next in the engine against a temporary null sink (silent).
set -euo pipefail
HERE=$(cd "$(dirname "$0")/.." && pwd)
SINK=psalm-verify-switch
SINK_ID=""
trap '[ -n "$SINK_ID" ] && pw-cli destroy "$SINK_ID" >/dev/null 2>&1 || true' EXIT

pw-cli create-node adapter "{ factory.name=support.null-audio-sink node.name=$SINK media.class=Audio/Sink object.linger=true audio.position=[FL,FR] }" >/dev/null
sleep 1
SINK_ID=$(pw-dump | python3 -c "
import json, sys
for o in json.load(sys.stdin):
    if o.get('info', {}).get('props', {}).get('node.name') == '$SINK':
        print(o['id']); break")
[ -n "$SINK_ID" ] || { echo "could not create the null sink" >&2; exit 1; }

export PSALM_TEST_SINK=$SINK
cd "$HERE"
cargo test --locked --release --test switch -- --ignored --nocapture --test-threads=1
