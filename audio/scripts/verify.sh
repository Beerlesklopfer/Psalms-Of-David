#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
# SPDX-License-Identifier: AGPL-3.0-or-later
# Plays a clip through psalm-audio into a temporary null sink, records the sink's
# monitor and compares the recording with the decoded file. The speakers stay silent.
#
#   scripts/verify.sh <mp3 file>
#
# Needs: pw-cli, pw-record, pw-dump, ffmpeg, python3 with numpy.
set -euo pipefail

# A role of its own: WirePlumber remembers volumes per role, and the checks must
# never change the volume of real programs (role Music).
export PSALM_AUDIO_ROLE=Test

SRC=${1:?usage: verify.sh <mp3 file>}
HERE=$(cd "$(dirname "$0")/.." && pwd)
WORK=$(mktemp -d)
SINK=psalm-verify
SINK_ID=""
REC_PID=""

cleanup() {
    [ -n "$REC_PID" ] && kill -INT "$REC_PID" 2>/dev/null || true
    [ -n "$SINK_ID" ] && pw-cli destroy "$SINK_ID" >/dev/null 2>&1 || true
    rm -rf "$WORK"
}
trap cleanup EXIT

# 10 seconds of a 64 kbit/s file (8000 bytes per second)
head -c 80000 "$SRC" > "$WORK/clip.mp3"
ffmpeg -v error -y -i "$WORK/clip.mp3" -f f32le -ac 2 "$WORK/ref.f32"

cargo build --quiet --example play --manifest-path "$HERE/Cargo.toml"

pw-cli create-node adapter "{ factory.name=support.null-audio-sink node.name=$SINK media.class=Audio/Sink object.linger=true audio.position=[FL,FR] }" >/dev/null
sleep 1
SINK_ID=$(pw-dump | python3 -c "
import json, sys
for o in json.load(sys.stdin):
    if o.get('info', {}).get('props', {}).get('node.name') == '$SINK':
        print(o['id']); break")
[ -n "$SINK_ID" ] || { echo "could not create the null sink" >&2; exit 1; }

pw-record --target "$SINK" -P stream.capture.sink=true --rate 48000 --channels 2 --format f32 "$WORK/rec.wav" >/dev/null 2>&1 &
REC_PID=$!
sleep 1.5

"$HERE/target/debug/examples/play" "$WORK/clip.mp3" --target "$SINK" --name "Psalm verification"
sleep 1
kill -INT "$REC_PID"; wait "$REC_PID" 2>/dev/null || true
REC_PID=""

python3 "$HERE/scripts/compare.py" "$WORK/rec.wav" "$WORK/ref.f32"
