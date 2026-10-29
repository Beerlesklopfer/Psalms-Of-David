// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/audio/audio_backend.dart';
import 'package:psalm/audio/pipewire_backend.dart';

import 'radio_stream_test.dart' show icyBlock;

/// Internet radio through the real engine and a real PipeWire sink (use a null sink,
/// see audio/scripts/verify-stream.sh). Needs the same environment as the other
/// engine tests, plus ffmpeg for the local station. `PSALM_TEST_STREAM_URL` adds a
/// test against a real station (network).
void main() {
  final lib = Platform.environment['PSALM_AUDIO_LIB'];
  final sink = Platform.environment['PSALM_TEST_SINK'];
  final skip = (lib == null || sink == null) ? 'set PSALM_AUDIO_LIB and PSALM_TEST_SINK' : null;
  final realUrl = Platform.environment['PSALM_TEST_STREAM_URL'];

  late HttpServer server;
  late PipeWireBackend backend;
  late Uint8List mp3;
  late Future<void> Function(HttpRequest) handler;

  Future<void> waitFor(bool Function() test, String what, {int seconds = 8}) async {
    final end = DateTime.now().add(Duration(seconds: seconds));
    while (!test()) {
      if (DateTime.now().isAfter(end)) {
        fail('timeout waiting for $what (state ${backend.state}, ${backend.errorMessage})');
      }
      await Future<void>.delayed(const Duration(milliseconds: 25));
    }
  }

  /// The MP3 with a metadata block after every [metaInt] bytes, as a Shoutcast server sends it.
  Uint8List withIcy(Uint8List audio, int metaInt) {
    final out = BytesBuilder();
    var n = 0;
    for (var i = 0; i < audio.length; i += metaInt) {
      out.add(audio.sublist(i, i + metaInt > audio.length ? audio.length : i + metaInt));
      out.add(icyBlock(n++ == 0 ? "StreamTitle='Test Artist - Test Song';" : null));
    }
    return out.takeBytes();
  }

  setUpAll(() async {
    if (skip != null) return;
    final made = await Process.run('ffmpeg', [
      '-v',
      'error',
      '-f',
      'lavfi',
      '-i',
      'sine=frequency=440:duration=20:sample_rate=44100',
      '-ac',
      '2',
      '-b:a',
      '128k',
      '-f',
      'mp3',
      '-',
    ], stdoutEncoding: null);
    mp3 = Uint8List.fromList(made.stdout as List<int>);
    server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    server.listen((r) async {
      try {
        await handler(r);
      } on Object {
        // The client went away.
      }
    });
  });

  setUp(() {
    if (skip != null) return;
    backend = PipeWireBackend.open(target: sink, libraryPath: lib);
  });

  tearDown(() {
    if (skip != null) return;
    backend.dispose();
  });

  tearDownAll(() async {
    if (skip != null) return;
    await server.close(force: true);
  });

  String url() => 'http://127.0.0.1:${server.port}/live.mp3';

  test('a local station plays, shows its title, pauses, resumes and stops', skip: skip, () async {
    var requests = 0;
    handler = (r) async {
      requests++;
      r.response.bufferOutput = false;
      r.response.headers.contentType = ContentType.parse('audio/mpeg');
      r.response.headers.set('icy-metaint', '4096');
      // Paced like a live broadcast: 16 KB/s is the 128 kbit/s of the file.
      final body = withIcy(mp3, 4096);
      for (var i = 0; i < body.length; i += 8192) {
        r.response.add(body.sublist(i, i + 8192 > body.length ? body.length : i + 8192));
        await r.response.flush();
        await Future<void>.delayed(const Duration(milliseconds: 250));
      }
      await r.response.close();
    };

    backend.load(url(), title: 'Test station');
    expect(backend.state, anyOf(BackendState.buffering, BackendState.playing));
    await waitFor(() => backend.state == BackendState.playing, 'playing');
    await waitFor(() => backend.streamTitle.value == 'Test Artist - Test Song', 'the title');
    expect(backend.duration, isNull, reason: 'a live stream has no length');

    await Future<void>.delayed(const Duration(seconds: 3));
    expect(backend.position, greaterThan(const Duration(seconds: 1)));
    backend.seek(const Duration(minutes: 5)); // does nothing
    expect(backend.state, BackendState.playing);

    backend.pause();
    await waitFor(() => backend.state == BackendState.paused, 'paused');
    final held = backend.position;
    await Future<void>.delayed(const Duration(seconds: 1));
    expect(backend.position, held);

    backend.play();
    await waitFor(() => backend.state == BackendState.playing, 'playing again', seconds: 12);
    expect(requests, 1, reason: 'pausing did not give up the connection');

    backend.stop();
    await waitFor(() => backend.state == BackendState.idle, 'idle');
    expect(backend.streamTitle.value, isNull);
  });

  test('a file path still plays as a file after a stream', skip: skip, () async {
    handler = (r) async {
      r.response.bufferOutput = false;
      r.response.headers.contentType = ContentType.parse('audio/mpeg');
      r.response.add(mp3.sublist(0, 60000)); // about 3.7 s, more than the pre-buffer
      await r.response.flush();
      await Future<void>.delayed(const Duration(seconds: 5));
    };
    backend.load(url());
    await waitFor(() => backend.state == BackendState.playing, 'playing');
    final dir = Directory.systemTemp.createTempSync('psalm_stream_test');
    addTearDown(() => dir.deleteSync(recursive: true));
    final file = File('${dir.path}/clip.mp3')..writeAsBytesSync(mp3.sublist(0, 80000));
    backend.load(file.path);
    await waitFor(() => backend.state == BackendState.playing && backend.duration != null, 'the file');
    expect(backend.streamTitle.value, isNull);
    backend.seek(const Duration(seconds: 2)); // works for a file
    await Future<void>.delayed(const Duration(milliseconds: 500));
    expect(backend.position, greaterThan(const Duration(seconds: 1)));
  });

  test('a station that sends AAC fails with a clear message', skip: skip, () async {
    handler = (r) async {
      r.response.headers.contentType = ContentType.parse('audio/aacp');
      r.response.add([1, 2, 3]);
      await r.response.close();
    };
    backend.load(url());
    await waitFor(() => backend.state == BackendState.failed, 'failure');
    expect(backend.errorMessage, 'stream type not supported');
    expect(backend.errorKind, AudioErrorKind.streamUnsupported);
  });

  test('a page that pretends to be MP3 ends in a failure, not in endless waiting', skip: skip, () async {
    handler = (r) async {
      r.response.bufferOutput = false;
      r.response.headers.contentType = ContentType.parse('audio/mpeg'); // a lie
      r.response.add(Uint8List.fromList('<html>nothing to hear</html>'.codeUnits));
      await r.response.close();
    };
    backend.load(url());
    // The server hangs up each time, so the session gives up after its reconnects.
    await waitFor(() => backend.state == BackendState.failed, 'failure', seconds: 20);
    expect(backend.errorMessage, isNotNull);
  });

  test('an HTTP error becomes the error text', skip: skip, () async {
    handler = (r) async {
      r.response.statusCode = 404;
      await r.response.close();
    };
    backend.load(url());
    await waitFor(() => backend.state == BackendState.failed, 'failure');
    expect(backend.errorMessage, 'HTTP 404');
    expect(backend.errorKind, AudioErrorKind.streamServer);
  });

  test(
    'a real station plays',
    skip: skip ?? (realUrl == null ? 'set PSALM_TEST_STREAM_URL' : null),
    timeout: const Timeout(Duration(seconds: 60)),
    () async {
      backend.load(realUrl!, title: 'real station');
      await waitFor(() => backend.state == BackendState.playing, 'playing', seconds: 15);
      await Future<void>.delayed(const Duration(seconds: 8));
      // ignore: avoid_print
      print('real station: state ${backend.state}, position ${backend.position}, title ${backend.streamTitle.value}');
      expect(backend.state, BackendState.playing);
      expect(backend.position, greaterThan(const Duration(seconds: 4)));
      expect(backend.duration, isNull);
    },
  );
}
