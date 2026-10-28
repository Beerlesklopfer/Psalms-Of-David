#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
# SPDX-License-Identifier: AGPL-3.0-or-later
# Checks pause, seek, volume and start-paused against a recording of a temporary
# null sink. The speakers stay silent.
#
#   scripts/verify-controls.sh <scenario> <mp3 file>
#   scenarios: pause | seek | volume | paused-start
set -euo pipefail

# A role of its own: WirePlumber remembers volumes per role, and the checks must
# never change the volume of real programs (role Music).
export PSALM_AUDIO_ROLE=Test

SCENARIO=${1:?usage: verify-controls.sh <pause|seek|volume|paused-start> <mp3 file>}
SRC=${2:?usage: verify-controls.sh <scenario> <mp3 file>}
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

# 16 seconds of a 64 kbit/s file (8000 bytes per second)
head -c 128000 "$SRC" > "$WORK/clip.mp3"
ffmpeg -v error -y -i "$WORK/clip.mp3" -f f32le -ac 2 "$WORK/ref.f32"

case "$SCENARIO" in
    pause)        SCRIPT="3000:pause,6000:play,9000:quit"; EXTRA="" ;;
    seek)         SCRIPT="2000:seek=8000,6000:quit";      EXTRA="" ;;
    # The first and last action keep the remembered volume of the role Test at 1.0.
    volume)       SCRIPT="800:volume=1.0,4000:volume=0.5,7500:volume=1.0,9000:quit"; EXTRA="" ;;
    paused-start) SCRIPT="3000:play,6000:quit";           EXTRA="--paused" ;;
    *) echo "unknown scenario $SCENARIO" >&2; exit 2 ;;
esac

cargo build --quiet --examples --manifest-path "$HERE/Cargo.toml"

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

"$HERE/target/debug/examples/control" "$WORK/clip.mp3" --target "$SINK" $EXTRA --script "$SCRIPT" > "$WORK/log.txt"
sleep 1
kill -INT "$REC_PID"; wait "$REC_PID" 2>/dev/null || true
REC_PID=""

python3 "$HERE/scripts/check_controls.py" "$SCENARIO" "$WORK/rec.wav" "$WORK/ref.f32" "$WORK/log.txt"
