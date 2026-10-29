#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
# SPDX-License-Identifier: AGPL-3.0-or-later
# Runs the live-stream tests of the engine against a temporary null sink (silent).
#
#   scripts/verify-stream.sh [station url]
#
# With a URL the test against a real station runs too (needs network and curl).
set -euo pipefail
HERE=$(cd "$(dirname "$0")/.." && pwd)
SINK=psalm-verify-stream
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
[ -n "${1:-}" ] && export PSALM_TEST_STREAM_URL=$1
cd "$HERE"
cargo test --locked --release --test stream -- --ignored --nocapture --test-threads=1

# The same through the Dart backend (network part in Dart, ICY metadata).
cargo build --locked --release
cd "$HERE/../app"
PSALM_AUDIO_ROLE=Test PSALM_AUDIO_LIB="$HERE/target/release/libpsalm_audio.so" \
    flutter test test/player_stream_pipewire_test.dart
