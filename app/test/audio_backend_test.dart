// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/audio/audio_backend.dart';
import 'package:psalm/audio/fake_backend.dart';
import 'package:psalm/audio/pipewire_backend.dart';

/// A clock that only moves when the test says so.
class Clock {
  Duration now = Duration.zero;
  void advance(Duration d) => now += d;
}

void main() {
  group('FakeBackend', () {
    late Clock clock;
    late FakeBackend b;

    setUp(() {
      clock = Clock();
      b = FakeBackend(clock: () => clock.now, fileLength: const Duration(minutes: 1));
    });

    test('starts idle and plays after a load', () {
      expect(b.state, BackendState.idle);
      b.load('a.mp3');
      expect(b.state, BackendState.playing);
      clock.advance(const Duration(seconds: 5));
      expect(b.position, const Duration(seconds: 5));
      expect(b.duration, const Duration(minutes: 1));
    });

    test('a load can start paused and at a place', () {
      b.load('a.mp3', start: const Duration(seconds: 20), play: false);
      expect(b.state, BackendState.paused);
      clock.advance(const Duration(seconds: 9));
      expect(b.position, const Duration(seconds: 20), reason: 'a paused player does not move');
      b.play();
      clock.advance(const Duration(seconds: 3));
      expect(b.position, const Duration(seconds: 23));
    });

    test('pause holds the position and play goes on from it', () {
      b.load('a.mp3');
      clock.advance(const Duration(seconds: 4));
      b.pause();
      clock.advance(const Duration(seconds: 30));
      expect(b.position, const Duration(seconds: 4));
      expect(b.state, BackendState.paused);
      b.play();
      clock.advance(const Duration(seconds: 2));
      expect(b.position, const Duration(seconds: 6));
    });

    test('seek keeps playing or staying paused', () {
      b.load('a.mp3');
      b.seek(const Duration(seconds: 30));
      expect(b.state, BackendState.playing);
      clock.advance(const Duration(seconds: 1));
      expect(b.position, const Duration(seconds: 31));
      b.pause();
      b.seek(const Duration(seconds: 10));
      expect(b.state, BackendState.paused);
      expect(b.position, const Duration(seconds: 10));
    });

    test('the end of the file ends playback and stays at the length', () {
      b.load('a.mp3', start: const Duration(seconds: 55));
      clock.advance(const Duration(seconds: 20));
      expect(b.state, BackendState.ended);
      expect(b.position, const Duration(minutes: 1));
    });

    test('a file that cannot be opened is an AudioException and a failed state', () {
      b.missing.add('gone.mp3');
      expect(() => b.load('gone.mp3'), throwsA(isA<AudioException>()));
      expect(b.state, BackendState.failed);
      b.load('a.mp3');
      expect(b.state, BackendState.playing, reason: 'the next load recovers');
    });

    test('volume stays between 0 and 1', () {
      b.setVolume(0.4);
      expect(b.volume, 0.4);
      b.setVolume(7);
      expect(b.volume, 1.0);
      b.setVolume(-1);
      expect(b.volume, 0.0);
    });

    test('stop unloads, and a disposed backend refuses everything', () {
      b.load('a.mp3');
      b.stop();
      expect(b.state, BackendState.idle);
      expect(b.duration, isNull);
      b.dispose();
      b.dispose(); // twice is fine
      expect(() => b.play(), throwsStateError);
      expect(() => b.position, throwsStateError);
    });
  });

  group('PipeWireBackend', () {
    final lib = Platform.environment['PSALM_AUDIO_LIB'];
    final sink = Platform.environment['PSALM_TEST_SINK'];
    final clip = Platform.environment['PSALM_TEST_CLIP'];
    // It plays real sound, so it runs only when given a sink to play into (a
    // virtual one, not the speakers) and a file. See audio/scripts/verify-dart.sh.
    final skip = (lib == null || sink == null || clip == null)
        ? 'set PSALM_AUDIO_LIB, PSALM_TEST_SINK and PSALM_TEST_CLIP (see audio/scripts/verify-dart.sh)'
        : false;

    test('loads, plays, pauses, seeks and changes the volume through the C library', skip: skip, () {
      final b = PipeWireBackend.open(target: sink, libraryPath: lib);
      addTearDown(b.dispose);

      b.load(clip!, play: false, title: 'Dart test');
      expect(b.state, BackendState.paused);
      expect(b.position, Duration.zero);
      // The engine opens the file on a worker thread: the length is known a moment after load returned.
      for (var i = 0; i < 200 && b.duration == null; i++) {
        sleep(const Duration(milliseconds: 10));
      }
      expect(b.duration, isNotNull);
      expect(b.duration!.inSeconds, greaterThan(5));

      b.play();
      sleep(const Duration(milliseconds: 1300));
      expect(b.state, BackendState.playing);
      final p = b.position;
      expect(p, greaterThan(const Duration(milliseconds: 900)));
      expect(p, lessThan(const Duration(milliseconds: 1800)));

      b.pause();
      sleep(const Duration(milliseconds: 150));
      final held = b.position;
      sleep(const Duration(milliseconds: 500));
      expect(b.position, held, reason: 'paused means standing still');
      expect(b.state, BackendState.paused);

      b.seek(const Duration(seconds: 5));
      sleep(const Duration(milliseconds: 400));
      expect(b.state, BackendState.paused, reason: 'a seek while paused stays paused');
      expect(b.position, greaterThanOrEqualTo(const Duration(seconds: 5)));
      expect(b.position, lessThan(const Duration(seconds: 5, milliseconds: 800)));

      b.setVolume(0.5);
      expect(b.volume, 0.5);

      expect(() => b.load('/definitely/not/there.mp3'), throwsA(isA<AudioException>()));
      expect(b.state, BackendState.failed);

      b.dispose();
      b.dispose();
      expect(() => b.play(), throwsStateError);
    });

    test('says why when the library or the engine is not there', () {
      expect(() => PipeWireBackend.open(libraryPath: '/no/such/libpsalm_audio.so'), throwsA(isA<AudioException>()));
    });
  });
}
