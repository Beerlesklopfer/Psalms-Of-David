// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/audio/audio_backend.dart';
import 'package:psalm/audio/radio_stream.dart';

/// An ICY metadata block for [text]: length byte, then the text padded with zeros
/// to a multiple of 16 bytes. Null or empty text gives the single zero byte.
List<int> icyBlock(String? text, {Encoding encoding = utf8}) {
  if (text == null) return [0];
  final bytes = encoding.encode(text);
  final blocks = (bytes.length + 15) ~/ 16;
  return [blocks, ...bytes, ...List.filled(blocks * 16 - bytes.length, 0)];
}

/// `metaInt` bytes of audio (a counting pattern, so a lost or doubled byte shows),
/// then a block, repeated.
({Uint8List body, Uint8List audio}) icyBody(int metaInt, List<List<int>> blocks) {
  final body = BytesBuilder();
  final audio = BytesBuilder();
  var n = 0;
  for (final block in blocks) {
    final chunk = Uint8List.fromList([for (var i = 0; i < metaInt; i++) (n++) & 0xff]);
    audio.add(chunk);
    body
      ..add(chunk)
      ..add(block);
  }
  return (body: body.takeBytes(), audio: audio.takeBytes());
}

/// Cuts [data] into chunks of [size] bytes.
Iterable<Uint8List> chunked(Uint8List data, int size) sync* {
  for (var i = 0; i < data.length; i += size) {
    yield Uint8List.sublistView(data, i, i + size > data.length ? data.length : i + size);
  }
}

class FakeSink implements StreamSink {
  final received = BytesBuilder(copy: true);
  final failures = <(String, AudioErrorKind)>[];

  /// How many bytes one push takes at most; 0 refuses everything while [refuse] is set.
  int takes = 1 << 20;
  bool refuse = false;
  bool closed = false;
  final failed = Completer<void>();

  @override
  int push(Uint8List data) {
    if (closed) return -1;
    if (refuse) return 0;
    final n = data.length < takes ? data.length : takes;
    received.add(Uint8List.sublistView(data, 0, n));
    return n;
  }

  @override
  void fail(String message, AudioErrorKind kind) {
    failures.add((message, kind));
    if (!failed.isCompleted) failed.complete();
  }
}

Future<void> until(bool Function() test, {Duration timeout = const Duration(seconds: 5)}) async {
  final end = DateTime.now().add(timeout);
  while (!test()) {
    if (DateTime.now().isAfter(end)) fail('condition not met in time');
    await Future<void>.delayed(const Duration(milliseconds: 10));
  }
}

void main() {
  group('IcyFilter', () {
    test('without metadata the bytes pass unchanged', () {
      final f = IcyFilter(0);
      final data = Uint8List.fromList([1, 2, 3, 0, 5]);
      expect(f.process(data), data);
    });

    test('removes the blocks and announces the titles', () {
      final titles = <String?>[];
      final body = icyBody(32, [
        icyBlock("StreamTitle='Artist - Song';"),
        icyBlock(null),
        icyBlock("StreamTitle='Next - One';StreamUrl='';"),
      ]);
      final f = IcyFilter(32, onTitle: titles.add);
      expect(f.process(body.body), body.audio);
      expect(titles, [
        'Artist - Song',
        'Next - One',
      ], reason: 'a block of length 0 changes nothing');
    });

    test('chunks may end anywhere, also inside a block or on the length byte', () {
      final body = icyBody(20, [
        icyBlock("StreamTitle='First title that is longer than sixteen bytes';"),
        icyBlock(null),
        icyBlock("StreamTitle='Second';"),
        icyBlock(null),
      ]);
      for (final size in [1, 2, 3, 7, 16, 20, 21, 22, 37, 1000]) {
        final titles = <String?>[];
        final f = IcyFilter(20, onTitle: titles.add);
        final audio = BytesBuilder();
        for (final c in chunked(body.body, size)) {
          audio.add(f.process(c));
        }
        expect(audio.takeBytes(), body.audio, reason: 'chunk size $size');
        expect(titles, [
          'First title that is longer than sixteen bytes',
          'Second',
        ], reason: 'chunk size $size');
      }
    });

    test('an empty title is announced as null, a block without a title is not announced', () {
      final titles = <String?>[];
      final f = IcyFilter(8, onTitle: titles.add);
      f.process(
        icyBody(8, [
          icyBlock("StreamTitle='';"),
          icyBlock("StreamUrl='http://x';"),
          icyBlock('garbage \u0001 no key'),
        ]).body,
      );
      expect(titles, [null]);
    });

    test('quotes inside the title, Latin-1 and UTF-8', () {
      expect(parseStreamTitle(utf8.encode("StreamTitle='Rock 'n' Roll';")), "Rock 'n' Roll");
      expect(parseStreamTitle(latin1.encode("StreamTitle='Grüße';")), 'Grüße');
      expect(parseStreamTitle(utf8.encode("StreamTitle='Grüße';")), 'Grüße');
      expect(parseStreamTitle(utf8.encode("StreamTitle='  padded  ';\u0000\u0000")), 'padded');
      expect(parseStreamTitle(utf8.encode("StreamTitle='no end")), 'no end');
      expect(parseStreamTitle(<int>[]), isNull);
    });
  });

  group('RadioStreamSession', () {
    late HttpServer server;
    late FakeSink sink;
    final sessions = <RadioStreamSession>[];
    var requests = <HttpRequest>[];
    late Future<void> Function(HttpRequest request, int number) handler;

    Uri url() => Uri.parse('http://127.0.0.1:${server.port}/stream.mp3');

    RadioStreamSession session({
      void Function(String?)? onTitle,
      void Function(StreamDetails)? onDetails,
      int attempts = 3,
    }) {
      final s = RadioStreamSession(
        url: url(),
        sink: sink,
        onTitle: onTitle,
        onDetails: onDetails,
        reconnectDelay: const Duration(milliseconds: 20),
        reconnectAttempts: attempts,
        idleTimeout: const Duration(milliseconds: 500),
        connectTimeout: const Duration(seconds: 2),
      );
      sessions.add(s);
      return s;
    }

    setUp(() async {
      sink = FakeSink();
      requests = [];
      server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      server.listen((request) async {
        requests.add(request);
        try {
          await handler(request, requests.length);
        } on Object {
          // The client went away.
        }
      });
    });

    tearDown(() async {
      for (final s in sessions) {
        await s.close();
      }
      sessions.clear();
      await server.close(force: true);
    });

    Future<void> serve(
      HttpRequest r,
      Uint8List body, {
      int metaInt = 0,
      String type = 'audio/mpeg',
      bool keepOpen = false,
    }) async {
      // Without this the server holds small writes back, even after flush().
      r.response.bufferOutput = false;
      r.response.headers.contentType = ContentType.parse(type);
      if (metaInt > 0) r.response.headers.set('icy-metaint', '$metaInt');
      for (final c in chunked(body, 37)) {
        r.response.add(c);
        await r.response.flush();
      }
      if (keepOpen) return;
      await r.response.close();
    }

    test('reports what the station says about itself in its headers', () async {
      handler = (r, n) async {
        r.response.headers.set('icy-name', 'DWG Radio');
        r.response.headers.set('icy-genre', 'Christian Music');
        r.response.headers.set('icy-br', '128');
        r.response.headers.set('icy-url', 'https://radio.example.org/');
        await serve(r, Uint8List.fromList(List.filled(100, 1)), keepOpen: true);
      };
      final seen = <StreamDetails>[];
      session(onDetails: seen.add).start();
      await until(() => seen.isNotEmpty);
      expect(
        seen.first,
        const StreamDetails(
          name: 'DWG Radio',
          genre: 'Christian Music',
          website: 'https://radio.example.org/',
          bitrateKbps: 128,
          format: 'MP3',
        ),
      );
    });

    test('a header text that arrived as Latin-1 is put right, one that is plain stays', () {
      expect(decodeHeaderText(String.fromCharCodes(utf8.encode('Röhre Radio '))), 'Röhre Radio');
      expect(decodeHeaderText('Plain'), 'Plain');
      expect(decodeHeaderText('Köln'), 'Köln', reason: 'Latin-1 that is no UTF-8 is not touched');
      expect(decodeHeaderText('  '), isNull);
      expect(decodeHeaderText(null), isNull);
    });

    test('connects with Icy-MetaData, strips the blocks and reports the titles', () async {
      final body = icyBody(64, [
        icyBlock("StreamTitle='Artist - Song';"),
        icyBlock(null),
        icyBlock("StreamTitle='Other - Tune';"),
      ]);
      handler = (r, n) => serve(r, body.body, metaInt: 64, keepOpen: true);
      final titles = <String?>[];
      session(onTitle: titles.add).start();
      await until(() => sink.received.length >= body.audio.length);
      expect(sink.received.toBytes(), body.audio);
      expect(titles, ['Artist - Song', 'Other - Tune']);
      expect(requests.single.headers.value('icy-metadata'), '1');
      expect(requests.single.headers.value('user-agent'), kStreamUserAgent);
    });

    test('a server without metadata is played as it is', () async {
      final data = Uint8List.fromList(List.generate(500, (i) => i & 0xff));
      handler = (r, n) => serve(r, data, keepOpen: true);
      session().start();
      await until(() => sink.received.length >= data.length);
      expect(sink.received.toBytes(), data);
    });

    test('a full engine queue holds the stream back instead of losing bytes', () async {
      final data = Uint8List.fromList(List.generate(2000, (i) => i & 0xff));
      handler = (r, n) => serve(r, data, keepOpen: true);
      sink
        ..refuse = true
        ..takes = 50;
      session().start();
      await Future<void>.delayed(const Duration(milliseconds: 150));
      expect(sink.received.length, 0, reason: 'nothing is accepted while the queue is full');
      sink.refuse = false;
      await until(() => sink.received.length >= data.length);
      expect(sink.received.toBytes(), data, reason: 'partial pushes are completed in order');
    });

    test('a 404 fails at once without trying again', () async {
      handler = (r, n) async {
        r.response.statusCode = 404;
        await r.response.close();
      };
      session().start();
      await sink.failed.future.timeout(const Duration(seconds: 5));
      expect(sink.failures.single, ('HTTP 404', AudioErrorKind.streamServer));
      expect(requests.length, 1);
    });

    test('a 503 is tried again 3 times, then it fails', () async {
      handler = (r, n) async {
        r.response.statusCode = 503;
        await r.response.close();
      };
      session().start();
      await sink.failed.future.timeout(const Duration(seconds: 5));
      expect(requests.length, 1 + kStreamReconnectAttempts);
      expect(sink.failures.single.$2, AudioErrorKind.streamConnection);
      expect(sink.failures.single.$1, contains('HTTP 503'));
    });

    test('AAC and playlists are refused with a clear message', () async {
      for (final type in ['audio/aacp', 'application/vnd.apple.mpegurl', 'text/html']) {
        sink = FakeSink();
        requests.clear();
        handler = (r, n) => serve(r, Uint8List.fromList([1, 2, 3]), type: type);
        session().start();
        await sink.failed.future.timeout(const Duration(seconds: 5));
        expect(sink.failures.single, (
          kStreamUnsupportedMessage,
          AudioErrorKind.streamUnsupported,
        ), reason: type);
        expect(requests.length, 1);
        expect(sink.received.length, 0);
      }
    });

    test('a dropped connection is reopened and the stream goes on', () async {
      final first = Uint8List.fromList(List.generate(300, (i) => i & 0xff));
      final second = Uint8List.fromList(List.generate(300, (i) => 255 - (i & 0xff)));
      handler = (r, n) => serve(r, n == 1 ? first : second, keepOpen: n > 1);
      final s = session()..start();
      await until(() => sink.received.length >= 600);
      expect(sink.received.toBytes(), [...first, ...second]);
      expect(s.connections, 2);
      expect(sink.failures, isEmpty);
    });

    test('a connection that keeps dropping fails after the attempts, one that delivered data starts the count again', () async {
      // Every connection delivers a healthy amount, then ends: never fails.
      handler = (r, n) => serve(r, Uint8List(kStreamHealthyBytes));
      final s = session()..start();
      await until(() => s.connections >= 3 * kStreamReconnectAttempts);
      expect(sink.failures, isEmpty);
      await s.close();

      // Every connection ends at once without data: fails after the attempts.
      sink = FakeSink();
      requests.clear();
      // Nothing, or a few bytes of junk, then the server hangs up: fails after the attempts.
      handler = (r, n) => serve(r, Uint8List(n.isEven ? 0 : 20));
      final dead = session()..start();
      await sink.failed.future.timeout(const Duration(seconds: 5));
      expect(dead.connections, 1 + kStreamReconnectAttempts);
      expect(sink.failures.single.$2, AudioErrorKind.streamConnection);
    });

    test('a connection that goes silent is dropped and reopened', () async {
      handler = (r, n) async {
        r.response.bufferOutput = false;
        r.response.headers.contentType = ContentType.parse('audio/mpeg');
        r.response.add([1, 2, 3]);
        await r.response.flush(); // then nothing, the line stays open
        await Future<void>.delayed(const Duration(seconds: 3));
      };
      final s = session()..start();
      await until(() => s.connections >= 2, timeout: const Duration(seconds: 5));
      expect(sink.received.length, greaterThanOrEqualTo(3));
    });

    test('no server at all fails with a connection error', () async {
      final port = server.port;
      await server.close(force: true);
      final s = RadioStreamSession(
        url: Uri.parse('http://127.0.0.1:$port/x'),
        sink: sink,
        reconnectDelay: const Duration(milliseconds: 10),
        connectTimeout: const Duration(seconds: 2),
      );
      sessions.add(s);
      s.start();
      await sink.failed.future.timeout(const Duration(seconds: 5));
      expect(sink.failures.single.$2, AudioErrorKind.streamConnection);
      expect(s.connections, 1 + kStreamReconnectAttempts);
      server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0); // for tearDown
    });

    test('a failed TLS handshake is not retried', () async {
      handler = (r, n) async {};
      final s = RadioStreamSession(
        url: Uri.parse('https://127.0.0.1:${server.port}/x'), // the server speaks plain HTTP
        sink: sink,
        reconnectDelay: const Duration(milliseconds: 10),
        connectTimeout: const Duration(seconds: 2),
      );
      sessions.add(s);
      s.start();
      await sink.failed.future.timeout(const Duration(seconds: 5));
      expect(sink.failures.single.$2, AudioErrorKind.streamCertificate);
      expect(s.connections, 1);
    });

    test('close stops the stream at once and nothing more reaches the sink', () async {
      handler = (r, n) async {
        r.response.bufferOutput = false;
        r.response.headers.contentType = ContentType.parse('audio/mpeg');
        for (var i = 0; i < 1000; i++) {
          r.response.add([i & 0xff]);
          await r.response.flush();
          await Future<void>.delayed(const Duration(milliseconds: 5));
        }
      };
      final s = session()..start();
      await until(() => sink.received.length > 5);
      await s.close().timeout(const Duration(seconds: 3));
      final seen = sink.received.length;
      await Future<void>.delayed(const Duration(milliseconds: 100));
      expect(sink.received.length, seen);
      expect(sink.failures, isEmpty, reason: 'closing is not a failure');
    });

    test('a sink that says it is closed ends the session quietly', () async {
      handler = (r, n) => serve(r, Uint8List(100), keepOpen: true);
      sink.closed = true;
      final s = session()..start();
      await s.done.timeout(const Duration(seconds: 3));
      expect(sink.failures, isEmpty);
    });

    test('redirects are followed', () async {
      handler = (r, n) async {
        if (r.uri.path == '/stream.mp3') {
          r.response.redirect(Uri.parse('http://127.0.0.1:${server.port}/real.mp3'));
        } else {
          await serve(r, Uint8List.fromList([9, 8, 7]), keepOpen: true);
        }
      };
      session().start();
      await until(() => sink.received.length >= 3);
      expect(sink.received.toBytes(), [9, 8, 7]);
    });
  });
}
