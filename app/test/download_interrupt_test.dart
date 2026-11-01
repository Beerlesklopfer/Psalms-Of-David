// A half-downloaded album, the app closed, the download restarted: every part is whole afterwards and the album plays.
// The server sends the hash of the whole file, as David does, so that a cut-off answer that ends cleanly is found.
import 'dart:async';
import 'dart:io';

import 'package:crypto/crypto.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/models.dart';
import 'package:psalm/offline/offline_view.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/server/api_client.dart';
import 'package:psalm/server/server_source.dart';

final _body = List<int>.generate(200 * 1024, (i) => i % 251);
final _hash = sha256.convert(_body).toString();

LibraryItem _book() => LibraryItem(
  id: '1',
  title: 'Album',
  author: 'X',
  color: const Color(0xFF000000),
  tracks: [
    for (var i = 1; i <= 3; i++)
      Track(disc: 1, number: i, title: 'T$i', length: const Duration(minutes: 5), partId: i, format: 'mp3'),
  ],
);

/// mode of part 2: 'ok', 'cut' (content-length, socket destroyed half way), 'early' (chunked, ends cleanly half way),
/// 'hang' (half sent, then nothing: the app is closed meanwhile).
Future<HttpServer> _serve(String Function() mode) async {
  final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
  server.listen((req) async {
    final id = int.tryParse(req.uri.pathSegments.length > 3 ? req.uri.pathSegments[3] : '') ?? 0;
    final m = id == 2 ? mode() : 'ok';
    final half = _body.sublist(0, _body.length ~/ 2);
    if (m == 'ok') {
      req.response.headers.set('x-content-sha256', _hash);
      req.response.contentLength = _body.length;
      req.response.add(_body);
      await req.response.close();
    } else if (m == 'cut') {
      req.response.contentLength = _body.length;
      final s = await req.response.detachSocket();
      s.add(half);
      await s.flush();
      s.destroy();
    } else if (m == 'early') {
      req.response.headers.set('x-content-sha256', _hash);
      req.response.add(half); // no content length: chunked, ended cleanly
      await req.response.close();
    } else {
      req.response.contentLength = _body.length;
      req.response.add(half);
      await req.response.flush();
      await Completer<void>().future;
    }
  });
  return server;
}

void main() {
  for (final failMode in ['cut', 'early', 'hang']) {
    test('part 2 interrupted ($failMode), restart, download again -> playable', () async {
      HttpOverrides.global = null;
      var mode = failMode;
      final server = await _serve(() => mode);
      final dir = Directory.systemTemp.createTempSync('psalm-zz-');
      addTearDown(() async {
        await server.close(force: true);
        dir.deleteSync(recursive: true);
      });
      ApiClient api() => ApiClient(address: '127.0.0.1:${server.port}', token: 't');
      final b = _book();

      // Run 1
      final s1 = ServerSource(api(), dir);
      final p1 = PlayerController([b], source: s1);
      final run1 = p1.downloadItem(b);
      if (failMode == 'hang') {
        await Future<void>.delayed(const Duration(seconds: 1)); // "app closed" while part 2 is half there
      } else {
        await run1.timeout(const Duration(seconds: 10));
      }
      // ignore: avoid_print
      print('[$failMode] after run1: ${dir.listSync().map((e) => '${e.uri.pathSegments.last}:${(e as File).lengthSync()}').toList()} error=${p1.downloadError}');

      // Restart
      mode = 'ok';
      ServerSource.dropPartials(dir);
      final s2 = ServerSource(api(), dir);
      final p2 = PlayerController([b], source: s2);
      await p2.downloadItem(b).timeout(const Duration(seconds: 10));
      // ignore: avoid_print
      print('[$failMode] after run2: ${dir.listSync().map((e) => '${e.uri.pathSegments.last}:${(e as File).lengthSync()}').toList()} error=${p2.downloadError}');

      for (final t in b.tracks) {
        final path = await s2.fetchPart(t);
        expect(File(path).lengthSync(), _body.length, reason: 'part ${t.partId} complete');
        expect(File(path).readAsBytesSync(), _body);
        expect(s2.isDownloaded(t), isTrue);
      }
      expect(p2.downloadedCount(b), 3);
      final off = downloadedOnly([b], s2.isDownloaded);
      expect(off.single.tracks.length, 3);
      expect(dir.listSync().where((e) => e.path.endsWith('.part')), isEmpty);
    });
  }

  test('after an interruption without restart, only complete parts count as downloaded and the offline view is playable', () async {
    HttpOverrides.global = null;
    final server = await _serve(() => 'cut');
    final dir = Directory.systemTemp.createTempSync('psalm-zz-');
    addTearDown(() async {
      await server.close(force: true);
      dir.deleteSync(recursive: true);
    });
    final b = _book();
    final s = ServerSource(ApiClient(address: '127.0.0.1:${server.port}', token: 't'), dir);
    final p = PlayerController([b], source: s);
    await p.downloadItem(b);
    expect(s.isDownloaded(b.tracks[0]), isTrue);
    expect(s.isDownloaded(b.tracks[1]), isFalse);
    expect(s.isDownloaded(b.tracks[2]), isFalse);
    final off = downloadedOnly([b], s.isDownloaded);
    expect(off.single.tracks.map((t) => t.partId), [1]);
  });

  test('a corrupt file on the device is found against the hash of the server and fetched again', () async {
    HttpOverrides.global = null;
    final server = await _serve(() => 'ok');
    final dir = Directory.systemTemp.createTempSync('psalm-zz-');
    addTearDown(() async {
      await server.close(force: true);
      dir.deleteSync(recursive: true);
    });
    final b = _book();
    final s = ServerSource(ApiClient(address: '127.0.0.1:${server.port}', token: 't'), dir);
    final p = PlayerController([b], source: s);
    await p.downloadItem(b);
    expect(p.downloadedCount(b), 3);
    // Same size, other content: only the hash can tell.
    final bad = File(await s.fetchPart(b.tracks[1]));
    bad.writeAsBytesSync(List<int>.filled(_body.length, 7));
    await p.downloadItem(b);
    expect(bad.readAsBytesSync(), _body, reason: 'the corrupt part was fetched again');
    expect(File(await s.fetchPart(b.tracks[0])).readAsBytesSync(), _body, reason: 'a good part stays');
  });
}
