#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
# SPDX-License-Identifier: AGPL-3.0-or-later
# Registers Psalms with the desktop of the current user, so that the launcher, the dock and the
# session manager show its icon and name, and david://pair links open it. Run it from the bundle
# folder (where `psalm` is); it points the entry at the program next to it. Nothing needs root.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
[ -x "$HERE/psalm" ] || { echo "run this from the folder that holds the program psalm" >&2; exit 1; }
DATA=${XDG_DATA_HOME:-$HOME/.local/share}
ID=com.the-abraham-box.psalm

mkdir -p "$DATA/applications" "$DATA/icons"
cp -r "$HERE/share/icons/hicolor" "$DATA/icons/"
sed "s|^Exec=.*|Exec=$HERE/psalm %U|" "$HERE/share/applications/$ID.desktop" > "$DATA/applications/$ID.desktop"
chmod 644 "$DATA/applications/$ID.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$DATA/applications" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$DATA/icons/hicolor" || true
command -v xdg-mime >/dev/null && xdg-mime default "$ID.desktop" x-scheme-handler/david || true
echo "Psalms is registered: $DATA/applications/$ID.desktop"
