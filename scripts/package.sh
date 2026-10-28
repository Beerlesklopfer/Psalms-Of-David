#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
# SPDX-License-Identifier: AGPL-3.0-or-later
# Builds the release artifacts into dist/: the server .deb and .rpm and .tar.gz files for
# the server and the audio library. Used by CI and runnable locally.
set -euo pipefail
cd "$(dirname "$0")/.."
dist=$PWD/dist
rm -rf "$dist" && mkdir -p "$dist"
arch=$(dpkg --print-architecture)

(cd server && cargo build --release --locked && cargo deb --no-build --output "$dist/" \
    && cargo generate-rpm -o "$dist/")

stage=$(mktemp -d) && trap 'rm -rf "$stage"' EXIT
# Server tarball
mkdir -p "$stage/david-server"
cp server/target/release/david server/packaging/david.service README.md LICENSE "$stage/david-server/"
tar -C "$stage" -czf "$dist/david-server-linux-$arch.tar.gz" david-server
# Audio library tarball
(cd audio && cargo build --release --locked)
mkdir -p "$stage/psalm-audio"
cp audio/target/release/libpsalm_audio.so audio/include/psalm_audio.h LICENSE "$stage/psalm-audio/"
tar -C "$stage" -czf "$dist/psalm-audio-linux-$arch.tar.gz" psalm-audio

ls -l "$dist"
