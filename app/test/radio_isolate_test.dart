// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/audio/audio_backend.dart';
import 'package:psalm/audio/radio_isolate.dart';

import 'radio_stream_test.dart' show FakeSink, icyBlock, icyBody, until;

void main() {
  group('IsolatedRadioSession', () {
    late HttpServer server;
    late FakeSink sink;
    late Future<void> Function(HttpRequest) handler;
    final sessions = <IsolatedRadioSession>[];

    setUp(() async {
      sink = FakeSink();
      server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      server.listen((request) async {
        try {
          await handler(request);
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

    IsolatedRadioSession session({
      void Function(String?)? onTitle,
      void Function(StreamDetails)? onDetails,
    }) {
      final s = IsolatedRadioSession(
        url: Uri.parse('http://127.0.0.1:${server.port}/live'),
        sink: sink,
        onTitle: onTitle,
        onDetails: onDetails,
        reconnectDelay: const Duration(milliseconds: 10),
        connectTimeout: const Duration(seconds: 2),
      );
      sessions.add(s);
      return s;
    }

    /// Sends [body] and keeps the connection open, as a live station does.
    Future<void> serve(HttpRequest r, List<int> body, {int metaInt = 0}) async {
      r.response.bufferOutput = false;
      r.response.headers.contentType = ContentType.parse('audio/mpeg');
      if (metaInt > 0) r.response.headers.set('icy-metaint', '$metaInt');
      for (var i = 0; i < body.length; i += 1000) {
        r.response.add(
          body.sublist(i, i + 1000 > body.length ? body.length : i + 1000),
        );
        await r.response.flush();
      }
      await Completer<void>().future; // hold the connection
    }

    Uint8List pattern(int n) => Uint8List.fromList([
      for (var i = 0; i < n; i++) (i * 7 + i ~/ 256) & 0xff,
    ]);

    test('bytes reach the sink in order and complete', () async {
      final data = pattern(50000);
      handler = (r) => serve(r, data);
      session().start();
      await until(() => sink.received.length >= data.length);
      await Future<void>.delayed(const Duration(milliseconds: 100));
      expect(sink.received.takeBytes(), data);
    });

    test('details and ICY titles arrive on the UI side, the blocks are not in the audio', () async {
      final b = icyBody(2000, [
        icyBlock("StreamTitle='First';"),
        icyBlock("StreamTitle='Second';"),
      ]);
      handler = (r) {
        r.response.headers.set('icy-name', 'DWG Radio');
        return serve(r, b.body, metaInt: 2000);
      };
      final titles = <String?>[];
      final details = <StreamDetails>[];
      session(onTitle: titles.add, onDetails: details.add).start();
      await until(
        () => titles.length == 2 && sink.received.length >= b.audio.length,
      );
      expect(titles, ['First', 'Second']);
      expect(details.single.name, 'DWG Radio');
      expect(details.single.format, 'MP3');
      expect(sink.received.takeBytes(), b.audio);
    });

    test('back pressure neither loses nor duplicates bytes', () async {
      // More than the in-flight limit, so the isolate really has to wait.
      final data = pattern(kRadioInFlightLimit * 2 + 12345);
      handler = (r) => serve(r, data);
      sink
        ..refuse = true
        ..takes = 777;
      session().start();
      await Future<void>.delayed(const Duration(milliseconds: 300));
      expect(sink.received.length, 0);
      sink.refuse = false;
      await until(
        () => sink.received.length >= data.length,
        timeout: const Duration(seconds: 15),
      );
      await Future<void>.delayed(const Duration(milliseconds: 200));
      expect(sink.received.takeBytes(), data);
    });

    test('close stops delivery and ends the isolate', () async {
      handler = (r) => serve(r, pattern(5000));
      final s = session()..start();
      await until(() => sink.received.length >= 5000);
      sink.refuse = true; // keep later chunks pending
      await s.close();
      final seen = sink.received.length;
      sink.refuse = false;
      await Future<void>.delayed(const Duration(milliseconds: 200));
      expect(sink.received.length, seen);
      expect(sink.failures, isEmpty);
      await s.done;
    });

    test('a sink that reports closed ends the session', () async {
      final more = Completer<void>();
      handler = (r) async {
        r.response.bufferOutput = false;
        r.response.headers.contentType = ContentType.parse('audio/mpeg');
        r.response.add(pattern(1000));
        await r.response.flush();
        await more.future;
        r.response.add(pattern(1000));
        await r.response.flush();
        await Completer<void>().future;
      };
      final s = session()..start();
      await until(() => sink.received.length >= 1000);
      sink.closed = true;
      more.complete();
      await s.done.timeout(const Duration(seconds: 5));
    });

    test('HTTP 404 is a server error', () async {
      handler = (r) async {
        r.response.statusCode = 404;
        await r.response.close();
      };
      session().start();
      await sink.failed.future.timeout(const Duration(seconds: 10));
      expect(sink.failures.single, ('HTTP 404', AudioErrorKind.streamServer));
    });

    test('an unsupported content type is reported as such', () async {
      handler = (r) async {
        r.response.headers.contentType = ContentType.parse('audio/aac');
        r.response.add([1, 2, 3]);
        await r.response.close();
      };
      session().start();
      await sink.failed.future.timeout(const Duration(seconds: 10));
      expect(sink.failures.single.$2, AudioErrorKind.streamUnsupported);
      expect(sink.received.length, 0);
    });
  });
}
