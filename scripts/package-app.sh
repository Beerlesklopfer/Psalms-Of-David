#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
# SPDX-License-Identifier: AGPL-3.0-or-later
# Builds the Linux client into dist/: a .deb, an .rpm and a .tar.gz of the bundle. Needs the
# sound engine's build tools (libpipewire-0.3-dev, clang), Flutter and `nfpm` on the PATH.
#   VERSION=1.2.3 scripts/package-app.sh      (VERSION defaults to the one in pubspec.yaml)
set -euo pipefail
cd "$(dirname "$0")/.."
dist=$PWD/dist
mkdir -p "$dist"
arch=$(dpkg --print-architecture)
export VERSION=${VERSION:-$(sed -n 's/^version: *\([0-9.]*\).*/\1/p' app/pubspec.yaml | head -1)}
export ARCH=$arch
# Flutter names the build folder x64 or arm64.
case "$arch" in amd64) export BUNDLE_ARCH=x64 ;; *) export BUNDLE_ARCH=$arch ;; esac

# The sound engine goes into the bundle: the CMake install step picks it up from here.
(cd audio && cargo build --release --locked)
(cd app && flutter pub get && flutter build linux --release --build-name "$VERSION")

if command -v nfpm >/dev/null; then
    (cd app && nfpm package --config linux/packaging/nfpm.yaml --packager deb --target "$dist/psalm_${VERSION}_${arch}.deb" \
        && nfpm package --config linux/packaging/nfpm.yaml --packager rpm --target "$dist/psalm-${VERSION}-${arch}.rpm")
else
    # Without nfpm only the .deb can be made, with dpkg-deb; the .rpm needs nfpm (or rpmbuild).
    echo "nfpm not found: building the .deb with dpkg-deb, no .rpm" >&2
    stage=$(mktemp -d) && trap 'rm -rf "$stage"' EXIT
    mkdir -p "$stage/opt" "$stage/usr/bin" "$stage/usr/share/applications" "$stage/usr/share/doc/psalm" "$stage/DEBIAN"
    cp -r "app/build/linux/$BUNDLE_ARCH/release/bundle" "$stage/opt/psalm"
    rm -f "$stage/opt/psalm/install-desktop-entry.sh"
    ln -s /opt/psalm/psalm "$stage/usr/bin/psalm"
    cp app/linux/com.the-abraham-box.psalm.desktop "$stage/usr/share/applications/"
    cp -r app/linux/icons "$stage/usr/share/icons"
    cp LICENSE "$stage/usr/share/doc/psalm/"
    size=$(du -sk "$stage" | cut -f1)
    cat > "$stage/DEBIAN/control" <<CONTROL
Package: psalm
Version: $VERSION
Architecture: $arch
Maintainer: Joerg Bernau <Joerg@bernau.family>
Installed-Size: $size
Depends: libgtk-3-0, libpipewire-0.3-0
Section: sound
Priority: optional
Homepage: https://github.com/Beerlesklopfer/Psalms-Of-David
Description: Psalms, the audiobook and music player of the family
 Plays what a David server offers, with a profile for every family member.
CONTROL
    dpkg-deb --root-owner-group --build "$stage" "$dist/psalm_${VERSION}_${arch}.deb" >/dev/null
fi
tar -C "app/build/linux/$BUNDLE_ARCH/release" -czf "$dist/psalm-linux-$arch.tar.gz" bundle
ls -l "$dist"
