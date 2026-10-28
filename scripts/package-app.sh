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

# The sound engine goes into the bundle: the CMake install step picks it up from here.
(cd audio && cargo build --release --locked)
(cd app && flutter pub get && flutter build linux --release --build-name "$VERSION")

(cd app && nfpm package --config linux/packaging/nfpm.yaml --packager deb --target "$dist/psalm_${VERSION}_${arch}.deb" \
    && nfpm package --config linux/packaging/nfpm.yaml --packager rpm --target "$dist/psalm-${VERSION}.rpm")
tar -C app/build/linux/x64/release -czf "$dist/psalm-linux-$arch.tar.gz" bundle
ls -l "$dist"
