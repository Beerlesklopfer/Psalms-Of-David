#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
# SPDX-License-Identifier: AGPL-3.0-or-later
# Runs the Dart test that drives the real engine through dart:ffi, playing into a
# temporary null sink so that the speakers stay silent.
#
#   scripts/verify-dart.sh <mp3 file at 64 kbit/s, at least 10 seconds long>
set -euo pipefail

# A role of its own, so that the remembered volume of real programs (role Music) stays untouched.
export PSALM_AUDIO_ROLE=Test

SRC=${1:?usage: verify-dart.sh <mp3 file>}
HERE=$(cd "$(dirname "$0")/.." && pwd)
APP=$(cd "$HERE/../app" && pwd)
WORK=$(mktemp -d)
SINK=psalm-verify
SINK_ID=""

cleanup() {
    [ -n "$SINK_ID" ] && pw-cli destroy "$SINK_ID" >/dev/null 2>&1 || true
    rm -rf "$WORK"
}
trap cleanup EXIT

head -c 128000 "$SRC" > "$WORK/clip.mp3"
cargo build --quiet --release --manifest-path "$HERE/Cargo.toml"

pw-cli create-node adapter "{ factory.name=support.null-audio-sink node.name=$SINK media.class=Audio/Sink object.linger=true audio.position=[FL,FR] }" >/dev/null
sleep 1
SINK_ID=$(pw-dump | python3 -c "
import json, sys
for o in json.load(sys.stdin):
    if o.get('info', {}).get('props', {}).get('node.name') == '$SINK':
        print(o['id']); break")
[ -n "$SINK_ID" ] || { echo "could not create the null sink" >&2; exit 1; }

cd "$APP"
PSALM_AUDIO_LIB="$HERE/target/release/libpsalm_audio.so" PSALM_TEST_SINK="$SINK" PSALM_TEST_CLIP="$WORK/clip.mp3" \
    flutter test test/audio_backend_test.dart
