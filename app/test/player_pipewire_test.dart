// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';
import 'dart:ui';

import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/audio/pipewire_backend.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player_controller.dart';

/// The controller on the real engine and a real PipeWire sink. Needs the same
/// environment as the backend test (see audio/scripts/verify-dart.sh).
void main() {
  final lib = Platform.environment['PSALM_AUDIO_LIB'];
  final sink = Platform.environment['PSALM_TEST_SINK'];
  final clip = Platform.environment['PSALM_TEST_CLIP'];
  final skip = (lib == null || sink == null || clip == null)
      ? 'set PSALM_AUDIO_LIB, PSALM_TEST_SINK and PSALM_TEST_CLIP'
      : null;

  test('PlayerController plays, pauses and moves on through PipeWire', skip: skip, () async {
    final book = LibraryItem(
      id: 'b',
      title: 'Book',
      author: 'Author',
      color: const Color(0xFF000000),
      tracks: [
        for (var i = 1; i <= 2; i++)
          Track(disc: 1, number: i, title: 'T$i', length: const Duration(seconds: 30), path: clip),
      ],
    );
    final p = PlayerController([book], backend: PipeWireBackend.open(target: sink, libraryPath: lib));
    addTearDown(p.dispose);

    p.open(book, track: 0);
    await Future<void>.delayed(const Duration(seconds: 3));
    expect(p.error, isNull);
    expect(p.position, greaterThan(const Duration(seconds: 2)));
    expect(p.position, lessThan(const Duration(seconds: 5)));

    p.toggle();
    final paused = p.position;
    await Future<void>.delayed(const Duration(seconds: 1));
    expect(p.position, paused);

    p.toggle();
    p.seek(const Duration(seconds: 20));
    await Future<void>.delayed(const Duration(seconds: 1));
    expect(p.position, greaterThan(const Duration(seconds: 20)));

    p.next();
    expect(p.trackIndex, 1);
    await Future<void>.delayed(const Duration(seconds: 1));
    expect(p.position, lessThan(const Duration(seconds: 3)));
    expect(p.playing, isTrue);
  });
}
