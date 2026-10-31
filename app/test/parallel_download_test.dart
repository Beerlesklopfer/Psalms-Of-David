// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later

import 'dart:async';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/models.dart';
import 'package:psalm/server/api_client.dart';
import 'package:psalm/server/server_source.dart';

void main() {
  test('the same part asked for twice at once is downloaded once and arrives whole', () async {
    // Playing a book while it is downloaded for listening offline asks for the same part twice. Both used to write into
    // one `<part>.part` file, and the mixed result was kept as downloaded.
    HttpOverrides.global = null;
    final body = List<int>.generate(256 * 1024, (i) => i % 251);
    final release = Completer<void>();
    var requests = 0;
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    server.listen((req) async {
      requests++;
      // The first half, then wait, so both downloads are surely running at the same time.
      req.response.add(body.sublist(0, body.length ~/ 2));
      await req.response.flush();
      await release.future;
      req.response.add(body.sublist(body.length ~/ 2));
      await req.response.close();
    });
    final dir = Directory.systemTemp.createTempSync('psalm-parallel-');
    addTearDown(() async {
      await server.close(force: true);
      dir.deleteSync(recursive: true);
    });
    ApiClient api() => ApiClient(address: '127.0.0.1:${server.port}', token: 't');
    const track = Track(disc: 1, number: 1, title: 'A', length: Duration(minutes: 1), partId: 7, format: 'mp3');

    // One source, as the player and its downloads share it.
    final source = ServerSource(api(), dir);
    final first = source.fetchPart(track);
    final second = source.fetchPart(track);
    await Future<void>.delayed(const Duration(milliseconds: 100));
    release.complete();
    final paths = await Future.wait([first, second]);

    expect(paths[0], paths[1]);
    expect(requests, 1);
    expect(File(paths[0]).readAsBytesSync(), body);
    expect(dir.listSync().where((e) => e.path.endsWith('.part')), isEmpty, reason: 'no half file is left behind');
  });

  test('the half-loaded files of an earlier run are deleted at the start, the downloaded ones stay', () {
    final dir = Directory.systemTemp.createTempSync('psalm-partials-');
    addTearDown(() => dir.deleteSync(recursive: true));
    File('${dir.path}/7.mp3').writeAsStringSync('whole');
    File('${dir.path}/8.mp3.123-0.part').writeAsStringSync('half');
    File('${dir.path}/ep-x.audio.part').writeAsStringSync('half');
    ServerSource.dropPartials(dir);
    expect(dir.listSync().map((e) => e.uri.pathSegments.last), ['7.mp3']);
  });

  test('every download gets a file of its own until it is complete', () {
    final dest = File('${Directory.systemTemp.path}/x.mp3');
    final a = partialFileFor(dest), b = partialFileFor(dest);
    expect(a.path, isNot(b.path));
    expect(a.path.startsWith('${dest.path}.') && a.path.endsWith('.part'), isTrue);
  });
}
