#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
# SPDX-License-Identifier: AGPL-3.0-or-later
# Checks the playback speed against a recording of a temporary null sink: the
# position must advance at the set speed and the pitch must stay. Silent speakers.
#
#   scripts/verify-speed.sh <mp3 file> [speed]
set -euo pipefail
export PSALM_AUDIO_ROLE=Test   # see verify-controls.sh

SRC=${1:?usage: verify-speed.sh <mp3 file> [speed]}
SPEED=${2:-1.5}
HERE=$(cd "$(dirname "$0")/.." && pwd)
WORK=$(mktemp -d)
SINK=psalm-verify-speed
SINK_ID=""
REC_PID=""
cleanup() {
    [ -n "$REC_PID" ] && kill -INT "$REC_PID" 2>/dev/null || true
    [ -n "$SINK_ID" ] && pw-cli destroy "$SINK_ID" >/dev/null 2>&1 || true
    [ -n "${KEEP:-}" ] && cp "$WORK/log.txt" "$KEEP" || true
    rm -rf "$WORK"
}
trap cleanup EXIT

head -c 400000 "$SRC" > "$WORK/clip.mp3"   # 50 s
ffmpeg -v error -y -i "$WORK/clip.mp3" -f f32le -ac 2 "$WORK/ref.f32"
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
"$HERE/target/debug/examples/control" "$WORK/clip.mp3" --target "$SINK" --script "500:speed=$SPEED,9500:quit" > "$WORK/log.txt"
sleep 1
kill -INT "$REC_PID"; wait "$REC_PID" 2>/dev/null || true
REC_PID=""
python3 "$HERE/scripts/check_speed.py" "$WORK/rec.wav" "$WORK/ref.f32" "$WORK/log.txt" "$SPEED"
