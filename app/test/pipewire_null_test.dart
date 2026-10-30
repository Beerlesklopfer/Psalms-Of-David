// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/audio/audio_backend.dart';
import 'package:psalm/audio/pipewire_backend.dart';

/// The real C library with the null output (`psalm_engine_new_null`): no PipeWire and no sound card are needed. It runs when the
/// library has been built (`cd audio && cargo build`, which `cargo test` does too); otherwise it is skipped.
String? _findLibrary() {
  final fromEnv = Platform.environment['PSALM_AUDIO_LIB'];
  if (fromEnv != null && File(fromEnv).existsSync()) return fromEnv;
  for (final rel in ['../audio/target/debug/libpsalm_audio.so', '../audio/target/release/libpsalm_audio.so']) {
    if (File(rel).existsSync()) return File(rel).absolute.path;
  }
  return null;
}

final String? _lib = _findLibrary();
final Object _skip = _lib == null ? 'libpsalm_audio.so not built (cd audio && cargo build)' : false;

void main() {
  group('PipeWireBackend on the null engine', () {
    test('opens, reports idle and refuses use after dispose', skip: _skip, () {
      final b = PipeWireBackend.openNull(libraryPath: _lib);
      expect(b.state, BackendState.idle);
      expect(b.duration, isNull);
      b.dispose();
      b.dispose(); // twice is fine
      expect(() => b.play(), throwsStateError);
    });

    test('load returns at once; the file is opened by a worker and plays to its end', skip: _skip, () async {
      final dir = Directory.systemTemp.createTempSync('psalm-null-dart');
      addTearDown(() => dir.deleteSync(recursive: true));
      final wav = File('${dir.path}/tone.wav')..writeAsBytesSync(toneWav(44100, 44100));
      final b = PipeWireBackend.openNull(libraryPath: _lib);
      addTearDown(b.dispose);

      b.load(wav.path, play: false, title: 'tone');
      expect(b.state, BackendState.paused, reason: 'while loading it reports what was asked for');
      expect(await _until(() => b.duration != null), isTrue, reason: 'the length is known when the worker is done');
      expect(b.duration!.inMilliseconds, 1000);
      expect(b.state, BackendState.paused);
      expect(b.position, Duration.zero);

      b.play();
      expect(await _until(() => b.state == BackendState.ended), isTrue);
      expect(b.position, b.duration);
    });

    test('a start position and a seek during loading are honoured', skip: _skip, () async {
      final dir = Directory.systemTemp.createTempSync('psalm-null-dart');
      addTearDown(() => dir.deleteSync(recursive: true));
      final wav = File('${dir.path}/tone.wav')..writeAsBytesSync(toneWav(44100, 4 * 44100));
      final b = PipeWireBackend.openNull(libraryPath: _lib);
      addTearDown(b.dispose);

      b.load(wav.path, start: const Duration(seconds: 1), play: false);
      expect(b.position, const Duration(seconds: 1), reason: 'the position is the start at once, not the old track');
      b.seek(const Duration(seconds: 3));
      expect(await _until(() => b.duration != null && b.position == const Duration(seconds: 3)), isTrue, reason: '${b.position}');
      expect(b.state, BackendState.paused);
    });

    test('a missing file is refused by load and leaves the state failed', skip: _skip, () {
      final b = PipeWireBackend.openNull(libraryPath: _lib);
      addTearDown(b.dispose);
      expect(() => b.load('/definitely/not/there.mp3'), throwsA(isA<AudioException>()));
      expect(b.state, BackendState.failed);
      expect(b.errorMessage, contains('not/there.mp3'));
    });

    test('a file that cannot be decoded fails after load returned, with a message', skip: _skip, () async {
      final dir = Directory.systemTemp.createTempSync('psalm-null-dart');
      addTearDown(() => dir.deleteSync(recursive: true));
      final junk = File('${dir.path}/junk.mp3')..writeAsBytesSync(List.filled(4000, 7));
      final b = PipeWireBackend.openNull(libraryPath: _lib);
      addTearDown(b.dispose);

      b.load(junk.path);
      expect(await _until(() => b.state == BackendState.failed), isTrue);
      expect(b.errorMessage, isNotEmpty);

      // The next load works.
      final wav = File('${dir.path}/tone.wav')..writeAsBytesSync(toneWav(44100, 4410));
      b.load(wav.path);
      expect(await _until(() => b.state == BackendState.ended), isTrue);
    });

    test('a file that hangs does not block the caller and a newer load replaces it', skip: _skip, () async {
      final dir = Directory.systemTemp.createTempSync('psalm-null-dart');
      addTearDown(() => dir.deleteSync(recursive: true));
      final fifo = '${dir.path}/hung.mp3';
      if (Process.runSync('mkfifo', [fifo]).exitCode != 0) return; // no mkfifo on this system
      final wav = File('${dir.path}/tone.wav')..writeAsBytesSync(toneWav(44100, 4410));
      final b = PipeWireBackend.openNull(libraryPath: _lib);
      addTearDown(b.dispose);

      final watch = Stopwatch()..start();
      b.load(fifo);
      expect(watch.elapsedMilliseconds, lessThan(500), reason: 'opening a pipe nobody writes to would block forever');
      b.load(wav.path);
      expect(await _until(() => b.state == BackendState.ended), isTrue);
      // Let the hung worker go; its answer is dropped.
      File(fifo).openSync(mode: FileMode.write).closeSync();
      await Future<void>.delayed(const Duration(milliseconds: 200));
      expect(b.state, BackendState.ended);
    });

    test('gapless: the queued track takes over by itself and onAdvanced reports it once', skip: _skip, () async {
      final dir = Directory.systemTemp.createTempSync('psalm-null-dart');
      addTearDown(() => dir.deleteSync(recursive: true));
      final a = File('${dir.path}/a.wav')..writeAsBytesSync(toneWav(44100, 44100));
      final b = File('${dir.path}/b.wav')..writeAsBytesSync(toneWav(44100, 2 * 44100));
      final backend = PipeWireBackend.openNull(libraryPath: _lib, speed: 4);
      addTearDown(backend.dispose);
      expect(backend.supportsGapless, isTrue);

      var advances = 0;
      Duration? durationAtAdvance;
      backend.onAdvanced = () {
        advances++;
        durationAtAdvance = backend.duration;
      };
      backend.load(a.path, play: false);
      backend.enqueueNext(b.path);
      await Future<void>.delayed(const Duration(milliseconds: 300));
      expect(backend.duration, const Duration(seconds: 1));
      backend.play();

      var sawEnded = false;
      final end = DateTime.now().add(const Duration(seconds: 10));
      while (DateTime.now().isBefore(end) && backend.state != BackendState.ended) {
        await Future<void>.delayed(const Duration(milliseconds: 5));
      }
      sawEnded = backend.state == BackendState.ended;
      expect(sawEnded, isTrue);
      expect(advances, 1, reason: 'one switch, reported once');
      expect(durationAtAdvance, const Duration(seconds: 2), reason: 'the callback sees the new track');
      expect(backend.duration, const Duration(seconds: 2));
      expect(backend.position, const Duration(seconds: 2));
    });

    test('gapless: the state is never ended between the two tracks', skip: _skip, () async {
      final dir = Directory.systemTemp.createTempSync('psalm-null-dart');
      addTearDown(() => dir.deleteSync(recursive: true));
      final a = File('${dir.path}/a.wav')..writeAsBytesSync(toneWav(44100, 22050));
      final b = File('${dir.path}/b.wav')..writeAsBytesSync(toneWav(44100, 22050));
      final backend = PipeWireBackend.openNull(libraryPath: _lib, speed: 1);
      addTearDown(backend.dispose);
      var advances = 0;
      backend.onAdvanced = () => advances++;
      backend.load(a.path, play: false);
      backend.enqueueNext(b.path);
      await Future<void>.delayed(const Duration(milliseconds: 300));
      backend.play();
      final states = <BackendState>[];
      final end = DateTime.now().add(const Duration(seconds: 10));
      while (DateTime.now().isBefore(end)) {
        final s = backend.state;
        states.add(s);
        if (s == BackendState.ended) break;
        await Future<void>.delayed(const Duration(milliseconds: 2));
      }
      expect(advances, 1);
      // The only ended is the final one, after the switch.
      expect(states.where((s) => s == BackendState.ended).length, 1);
      expect(states.last, BackendState.ended);
    });

    test('clearNext and a new load drop the queued track', skip: _skip, () async {
      final dir = Directory.systemTemp.createTempSync('psalm-null-dart');
      addTearDown(() => dir.deleteSync(recursive: true));
      final a = File('${dir.path}/a.wav')..writeAsBytesSync(toneWav(44100, 4410));
      final b = File('${dir.path}/b.wav')..writeAsBytesSync(toneWav(44100, 44100));
      final backend = PipeWireBackend.openNull(libraryPath: _lib);
      addTearDown(backend.dispose);
      var advances = 0;
      backend.onAdvanced = () => advances++;
      backend.load(a.path, play: false);
      backend.enqueueNext(b.path);
      backend.clearNext();
      await Future<void>.delayed(const Duration(milliseconds: 200));
      backend.play();
      expect(await _until(() => backend.state == BackendState.ended), isTrue);
      expect(backend.duration, const Duration(milliseconds: 100));
      backend.enqueueNext(b.path);
      backend.load(a.path);
      expect(await _until(() => backend.state == BackendState.ended), isTrue);
      await Future<void>.delayed(const Duration(milliseconds: 300));
      expect(advances, 0);
      expect(backend.duration, const Duration(milliseconds: 100));
    });

    test('a library without the version function is refused with a clear message', () {
      expect(
        () => PipeWireBackend.openNull(libraryPath: 'libc.so.6'),
        throwsA(isA<AudioException>().having((e) => e.message, 'message', contains('psalm_abi_version'))),
      );
    });

    test('a missing library is an AudioException', () {
      expect(() => PipeWireBackend.openNull(libraryPath: '/no/such/libpsalm_audio.so'), throwsA(isA<AudioException>()));
    });
  });
}

/// Polls until [cond] holds, up to [seconds] (the engine works on other threads).
Future<bool> _until(bool Function() cond, {int seconds = 5}) async {
  final end = DateTime.now().add(Duration(seconds: seconds));
  while (DateTime.now().isBefore(end)) {
    if (cond()) return true;
    await Future<void>.delayed(const Duration(milliseconds: 5));
  }
  return cond();
}

/// A mono 16-bit WAV of [frames] frames of a ramp, for the tests that need a file.
Uint8List toneWav(int rate, int frames) {
  final data = ByteData(44 + frames * 2);
  void tag(int at, String s) {
    for (var i = 0; i < 4; i++) {
      data.setUint8(at + i, s.codeUnitAt(i));
    }
  }

  tag(0, 'RIFF');
  data.setUint32(4, 36 + frames * 2, Endian.little);
  tag(8, 'WAVE');
  tag(12, 'fmt ');
  data.setUint32(16, 16, Endian.little);
  data.setUint16(20, 1, Endian.little);
  data.setUint16(22, 1, Endian.little);
  data.setUint32(24, rate, Endian.little);
  data.setUint32(28, rate * 2, Endian.little);
  data.setUint16(32, 2, Endian.little);
  data.setUint16(34, 16, Endian.little);
  tag(36, 'data');
  data.setUint32(40, frames * 2, Endian.little);
  for (var i = 0; i < frames; i++) {
    data.setInt16(44 + i * 2, (i % 100) * 100, Endian.little);
  }
  return data.buffer.asUint8List();
}
