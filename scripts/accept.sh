#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
# SPDX-License-Identifier: AGPL-3.0-or-later
# Local acceptance of the Linux app: a throwaway server (server/examples/serve_demo.rs) and the app in an own data folder, driven with xdotool
# (virtual cursor and keys), a screenshot after every step. Nothing of the real pairing or library is touched.
#
#   scripts/accept.sh [--build] [out-dir]
#
# --build builds the demo server and the release app first. The screenshots go to out-dir (default: a new folder in /tmp) and are looked at by a
# person (or by Claude): the script stops at the first step that fails (no window, the app died) and prints where the pictures are.
# The scenario ends with the server started again: the script waits (up to 200 s) until the bar of the offline mode is gone, which the app does
# by itself at its period (60 s) and tells from the colour of the bar; ACCEPT_DEBUG=1 prints the pixel it looks at.
# The app runs in the X11 window system (GDK_BACKEND=x11, under Wayland through XWayland), where xdotool can click. Needs xdotool, ImageMagick
# (import) and ffmpeg. Run it on a desktop of its own or leave mouse and keyboard alone meanwhile; with Xvfb (xvfb-run) it needs no screen.
set -euo pipefail
cd "$(dirname "$0")/.."
build=0
[ "${1:-}" = --build ] && { build=1; shift; }
out=${1:-$(mktemp -d /tmp/psalm-accept-XXXXXX)}
work=$(mktemp -d /tmp/psalm-accept-work-XXXXXX)
port=${ACCEPT_PORT:-18484}
mkdir -p "$out"
for tool in xdotool import convert ffmpeg; do command -v "$tool" >/dev/null || { echo "$tool is needed" >&2; exit 2; }; done

if [ "$build" = 1 ]; then
    (cd server && cargo build --example serve_demo)
    (cd app && flutter build linux --release)
fi
demo=server/target/debug/examples/serve_demo
app=app/build/linux/x64/release/bundle/psalm
[ -x "$demo" ] && [ -x "$app" ] || { echo "build first: scripts/accept.sh --build" >&2; exit 2; }

server_pid= app_pid=
cleanup() { [ -n "$app_pid" ] && kill "$app_pid" 2>/dev/null || true; [ -n "$server_pid" ] && kill "$server_pid" 2>/dev/null || true; rm -rf "$work"; }
trap cleanup EXIT

ffmpeg -loglevel error -f lavfi -i "sine=frequency=440:duration=30" -q:a 6 "$work/tone.mp3"
# The server keeps its data in $work/demo, so that it can be started again as the same server (the app stays paired).
start_server() {
    DEMO_KEEP_DIR="$work/demo" DEMO_TRACKS=3 "$demo" "127.0.0.1:$port" "$work/tone.mp3" "$work/pairing" >>"$work/server.log" 2>&1 &
    server_pid=$!
    for _ in $(seq 1 60); do [ -s "$work/pairing" ] && break; sleep 0.5; done
    [ -s "$work/pairing" ] || { echo "the demo server did not start" >&2; exit 1; }
}
start_server

GDK_BACKEND=x11 PSALM_DATA_DIR="$work/app" "$app" "$(cat "$work/pairing")" >"$work/app.log" 2>&1 &
app_pid=$!
win=
for _ in $(seq 1 60); do
    for id in $(xdotool search --pid "$app_pid" 2>/dev/null); do
        [ "$(xdotool getwindowname "$id" 2>/dev/null)" = Psalms ] && win=$id
    done
    [ -n "$win" ] && break
    sleep 0.5
done
[ -n "$win" ] || { echo "the app has no window" >&2; exit 1; }

# Where the window is on the screen; a click at (x, y) of a picture is this origin plus (x, y).
eval "$(xdotool getwindowgeometry --shell "$win")"
ox=$X oy=$Y
step=0
shot() { step=$((step + 1)); sleep "${2:-1.5}"; kill -0 "$app_pid" 2>/dev/null || { echo "the app died before: $1" >&2; exit 1; }
         import -window "$win" "$out/$(printf %02d "$step")-$1.png"; echo "$(printf %02d "$step") $1"; }
# Is the bar of the offline mode on the screen? It is the dark red strip at the top of the window (the colour is the secondary container).
bar_shown() {
    local px r g b
    import -window "$win" "$work/bar.png"
    px=$(convert "$work/bar.png" -crop 1x1+900+85 +repage txt:- | grep -o '#[0-9A-Fa-f]\{6\}' | head -1)
    r=$((16#${px:1:2})); g=$((16#${px:3:2})); b=$((16#${px:5:2}))
    [ -n "${ACCEPT_DEBUG:-}" ] && echo "bar pixel $px" >&2
    [ "$r" -gt 90 ] && [ "$g" -lt 80 ] && [ "$b" -lt 100 ]
}
click() { xdotool mousemove $((ox + $1)) $((oy + $2)) click 1; }

shot connect 3
click 666 604                                  # "Connect"
shot home 4
click 232 458                                  # the album "Bible" (3 titles)
shot album 2
click 989 494                                  # download the album
shot downloaded 4
click 54 98                                    # back
kill "$server_pid"; server_pid=                # the server goes away
shot server-gone 38                            # the look at the server runs every 30 s
click 1230 124                                 # "Stay offline" (second button of the bar)
shot stay-offline 2
click 232 488                                  # the album, offline: it is on the device
shot album-offline 2
click 408 676                                  # the second title (the bar of the offline mode pushed the page down by 30): it plays without the server
shot playing-offline 4
bar_shown || { echo "the offline bar is not shown, though the server is gone" >&2; exit 1; }
start_server                                   # the same server comes back; the app looks for it at its period (60 s)
back=0
for _ in $(seq 1 40); do
    bar_shown || { back=1; break; }
    sleep 5
done
[ "$back" = 1 ] || { shot still-offline 0; echo "the app did not go online again within 200 s" >&2; exit 1; }
shot back-online 3
echo "pictures in $out"
