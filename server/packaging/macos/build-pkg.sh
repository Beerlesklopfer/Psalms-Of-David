#!/bin/bash
# SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
# SPDX-License-Identifier: AGPL-3.0-or-later
# Builds the installer package of the David server for macOS from a (universal) binary. Run on a Mac (the CI job `package-server-macos` does):
#   VERSION=1.2.3 BINARY=path/to/david OUT=dist ARCHITECTURES=arm64,x86_64 server/packaging/macos/build-pkg.sh
# Optional signing (a "Developer ID Installer" certificate in the keychain): INSTALLER_IDENTITY="Developer ID Installer: Name (TEAM)".
# Result: $OUT/david-server-$VERSION-macos.pkg. `CHECK_ONLY=1` stops after the checks of the files (it needs no Mac tools).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../../.." && pwd)"
: "${VERSION:?VERSION is needed}"
: "${OUT:=dist}"
: "${ARCHITECTURES:=arm64,x86_64}"

# The files of the package must be what the scripts and the plists name.
for f in com.the-abraham-box.david.plist com.the-abraham-box.david-reindex.plist david.toml newsyslog-david.conf distribution.xml david-uninstall.sh scripts/preinstall scripts/postinstall; do
    [ -f "$here/$f" ] || { echo "missing: $f" >&2; exit 1; }
done
for f in david-uninstall.sh scripts/preinstall scripts/postinstall build-pkg.sh; do
    bash -n "$here/$f"
done
if command -v plutil >/dev/null 2>&1; then
    plutil -lint "$here/com.the-abraham-box.david.plist" "$here/com.the-abraham-box.david-reindex.plist"
fi
[ "${CHECK_ONLY:-0}" = 1 ] && { echo "the files of the package look right"; exit 0; }

: "${BINARY:?BINARY is needed (the david executable)}"
[ -f "$BINARY" ] || { echo "no such binary: $BINARY" >&2; exit 1; }

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
root="$work/root"
mkdir -p "$root/usr/local/bin" "$root/usr/local/share/david" "$root/Library/LaunchDaemons" "$root/etc/newsyslog.d"
install -m 755 "$BINARY" "$root/usr/local/bin/david"
install -m 755 "$here/david-uninstall.sh" "$root/usr/local/bin/david-uninstall.sh"
install -m 644 "$here/com.the-abraham-box.david.plist" "$root/Library/LaunchDaemons/com.the-abraham-box.david.plist"
install -m 644 "$here/com.the-abraham-box.david-reindex.plist" "$root/Library/LaunchDaemons/com.the-abraham-box.david-reindex.plist"
install -m 644 "$here/david.toml" "$root/usr/local/share/david/david.toml.default"
install -m 644 "$here/newsyslog-david.conf" "$root/etc/newsyslog.d/david.conf"

scripts="$work/scripts"
mkdir -p "$scripts"
install -m 755 "$here/scripts/preinstall" "$here/scripts/postinstall" "$scripts/"

resources="$work/Resources"
cp -R "$here/Resources" "$resources"
cp "$repo/LICENSE" "$resources/license.txt"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@ARCHITECTURES@/$ARCHITECTURES/g" "$here/distribution.xml" > "$work/distribution.xml"

mkdir -p "$OUT"
pkgbuild --root "$root" --scripts "$scripts" --identifier com.the-abraham-box.david --version "$VERSION" \
    --install-location / --ownership recommended "$work/david-component.pkg"
unsigned="$work/david-server-$VERSION-macos.pkg"
productbuild --distribution "$work/distribution.xml" --package-path "$work" --resources "$resources" "$unsigned"

result="$OUT/david-server-$VERSION-macos.pkg"
if [ -n "${INSTALLER_IDENTITY:-}" ]; then
    productsign --sign "$INSTALLER_IDENTITY" "$unsigned" "$result"
else
    echo "warning: the package is not signed (no INSTALLER_IDENTITY): open it with a right click and Open" >&2
    cp "$unsigned" "$result"
fi

# What is in the package, and does it hold the files the scripts expect?
payload="$(pkgutil --payload-files "$work/david-component.pkg")"
for f in ./usr/local/bin/david ./usr/local/bin/david-uninstall.sh ./Library/LaunchDaemons/com.the-abraham-box.david.plist \
         ./Library/LaunchDaemons/com.the-abraham-box.david-reindex.plist ./usr/local/share/david/david.toml.default ./etc/newsyslog.d/david.conf; do
    echo "$payload" | grep -qx "$f" || { echo "not in the package: $f" >&2; exit 1; }
done
echo "$result"
