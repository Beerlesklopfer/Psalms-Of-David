// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:ui';

import 'package:fake_async/fake_async.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/audio/audio_backend.dart';
import 'package:psalm/audio/fake_backend.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player_controller.dart';

Book _book({bool withFiles = true}) => Book(
      id: 'b',
      title: 'Book',
      author: 'Author',
      color: const Color(0xFF000000),
      tracks: [
        for (var i = 1; i <= 3; i++)
          Track(
            disc: 1,
            number: i,
            title: 'T$i',
            length: const Duration(seconds: 20),
            path: withFiles ? '/music/$i.mp3' : null,
          ),
      ],
    );

void main() {
  late Duration now;
  late FakeBackend backend;

  PlayerController make(Book b) {
    now = Duration.zero;
    backend = FakeBackend(clock: () => now, fileLength: const Duration(seconds: 20));
    return PlayerController([b], backend: backend);
  }

  test('opening a book loads its resume track in the backend and plays', () {
    final p = make(_book());
    p.open(p.library.first, track: 1);
    expect(backend.loadedPath, '/music/2.mp3');
    expect(backend.state, BackendState.playing);
    expect(p.error, isNull);
    p.dispose();
  });

  test('toggle pauses and resumes the backend; seek and next follow', () {
    final p = make(_book());
    p.open(p.library.first, track: 0);
    p.toggle();
    expect(backend.state, BackendState.paused);
    p.toggle();
    expect(backend.state, BackendState.playing);
    p.seek(const Duration(seconds: 7));
    expect(backend.position, const Duration(seconds: 7));
    p.next();
    expect(backend.loadedPath, '/music/2.mp3');
    expect(backend.position, Duration.zero);
    p.dispose();
  });

  test('the position is read from the backend and the next track follows the end', () {
    fakeAsync((async) {
      final p = make(_book());
      p.open(p.library.first, track: 0);
      now = const Duration(seconds: 5);
      async.elapse(const Duration(milliseconds: 500));
      expect(p.position, const Duration(seconds: 5));
      now = const Duration(seconds: 21);
      async.elapse(const Duration(milliseconds: 500));
      expect(p.trackIndex, 1);
      expect(backend.loadedPath, '/music/2.mp3');
      expect(p.playing, isTrue);
      p.dispose();
    });
  });

  test('a track without a file runs on the demo clock and leaves the backend idle', () {
    final p = make(_book(withFiles: false));
    p.open(p.library.first, track: 0);
    expect(backend.state, BackendState.idle);
    expect(p.playing, isTrue);
    p.dispose();
  });

  test('a backend failure pauses and reports the error', () {
    final p = make(_book());
    backend.missing.add('/music/1.mp3');
    p.open(p.library.first, track: 0);
    expect(p.playing, isFalse);
    expect(p.error, 'cannot open /music/1.mp3');
    p.dispose();
  });
}
