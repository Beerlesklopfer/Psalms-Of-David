// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
// The projects of the server over HTTP (a test without widgets, so its requests reach the server of the test).
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/pictures/show_api.dart';
import 'package:psalm/pictures/show_project.dart';
import 'package:psalm/pictures/show_ui.dart';
import 'package:psalm/server/api_client.dart';

import 'show_server_test.dart' show serve;

void main() {
  late Directory dir;
  setUp(() => dir = Directory.systemTemp.createTempSync('show-server-api'));
  tearDown(() => dir.deleteSync(recursive: true));

  test('the app lists the projects, fetches the files of one while it plays and throws them away afterwards', () async {
    final (server, api, seen) = await serve(dir, rev: () => 'aaaa');
    addTearDown(() => server.close(force: true));
    final show = ShowApi(api);
    final list = await show.shows();
    expect([for (final s in list) (s.id, s.title, s.pictures, s.rev)], [(7, 'Allgäu', 2, 'aaaa')]);
    final cache = Directory('${dir.path}/.stream-1');
    final p = await show.stream(7, cache);
    expect(File(await p.remote!.fetch('screen/b1')).readAsBytesSync(), [1, 2, 3]);
    await p.remote!.fetch('screen/b1');
    expect(seen.where((u) => u.endsWith('/b1/screen')).length, 1, reason: 'each file once');
    expect(File(await p.remote!.fetch('video/b2')).existsSync(), isTrue);
    await p.remote!.fetch('part/5');
    expect(seen, contains('/api/v1/parts/5/stream?as=mp3'), reason: 'the music as an MP3, which every device plays');
    expect(await p.remote!.place('b1'), 'Kempten');
    expect(await p.remote!.place('b2'), isNull);
    await expectLater(p.remote!.fetch('screen/b9'), throwsA(isA<ApiException>()));
    await p.remote!.dispose();
    expect(cache.existsSync(), isFalse);
  });

  test('loaded onto the device the copy knows its version; a change on the server marks it, the update replaces it', () async {
    var rev = 'aaaa';
    final (server, api, _) = await serve(dir, rev: () => rev);
    addTearDown(() => server.close(force: true));
    final c = ShowController(api: ShowApi(api), projects: ShowProjects(root: Directory('${dir.path}/slideshows')));
    await c.load();
    await c.loadServer();
    final s = c.server.single;
    expect((c.copyOf(s), c.outdated(s)), (null, false));
    final copy = await c.saveShow(s);
    expect((copy?.serverId, copy?.serverRev), (7, 'aaaa'));
    expect(c.copyOf(s)?.id, copy!.id);
    expect(c.outdated(s), isFalse);

    rev = 'bbbb';
    await c.loadServer();
    expect(c.outdated(c.server.single), isTrue);
    final newer = await c.saveShow(c.server.single);
    expect(newer!.serverRev, 'bbbb');
    expect(c.list.where((p) => p.serverId == 7), hasLength(1), reason: 'the older copy goes');
    expect(c.outdated(c.server.single), isFalse);
  });
}
