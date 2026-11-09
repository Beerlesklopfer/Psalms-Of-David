// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
// The projects put together on the server (the web editor of the pictures) in the app: listed, played from the server, loaded onto the
// device, and marked when the server has a newer version.
import 'dart:convert';
import 'dart:io';

import 'package:archive/archive_io.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/audio/fake_backend.dart';
import 'package:psalm/pictures/show_api.dart';
import 'package:psalm/pictures/show_player.dart';
import 'package:psalm/pictures/show_project.dart';
import 'package:psalm/pictures/show_ui.dart';
import 'package:psalm/server/api_client.dart';

import 'show_player_test.dart' show app, project;

/// The JSON of `GET /api/v1/pictures/shows/7`.
Map<String, dynamic> streamed({String rev = 'aaaa'}) => {
      'id': 7,
      'title': 'Allgäu',
      'rev': rev,
      'settings': {'order': 'date', 'seconds': 5, 'transition': 'cut', 'videos': 'on', 'info': 'both'},
      'pictures': [
        {'key': 'b1', 'video': false, 'taken_at': DateTime(2017, 7, 1, 12).millisecondsSinceEpoch, 'seconds': 3},
        {'key': 'b2', 'video': true, 'taken_at': DateTime(2018, 8, 2, 12).millisecondsSinceEpoch, 'transition': 'zoom'},
        {'key': '../x', 'video': false, 'taken_at': 0},
      ],
      'music': [
        {'part_id': 5, 'title': 'Air', 'artist': 'Bach'},
      ],
    };

/// The project file of a project of the server: the three pictures of `show_player_test.dart`.
Future<List<int>> projectZip(Directory dir) async {
  final zipDir = Directory('${dir.path}/zip-${DateTime.now().microsecondsSinceEpoch}')..createSync();
  await project(zipDir);
  final zip = ZipEncoder().encode(createArchiveFromDirectory(zipDir, includeDirName: false));
  zipDir.deleteSync(recursive: true);
  return zip;
}

/// The server as the rubric sees it: one project, whose mark the test changes.
class FakeShowApi extends ShowApi {
  FakeShowApi(this.zip) : super(ApiClient(address: 'localhost:1'));

  static Future<FakeShowApi> make(Directory dir) async => FakeShowApi(await projectZip(dir));

  final List<int> zip;
  String rev = 'aaaa';

  @override
  Future<List<ServerShow>> shows() async => [ServerShow(id: 7, title: 'Allgäu', pictures: 2, rev: rev)];

  @override
  Future<void> downloadShow(int id, File dest, {void Function(int bytes)? progress}) async => dest.writeAsBytesSync(zip);
}

/// A server with one project; [rev] is its mark, [seen] the requests.
Future<(HttpServer, ApiClient, List<String>)> serve(Directory dir, {required String Function() rev}) async {
  final zip = await projectZip(dir);
  final seen = <String>[];
  final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
  server.listen((req) async {
    final path = req.uri.path;
    seen.add(req.uri.toString());
    final res = req.response;
    switch (path) {
      case '/api/v1/pictures/shows':
        res.write(jsonEncode({
          'shows': [
            {'id': 7, 'title': 'Allgäu', 'pictures': 2, 'rev': rev()},
          ],
        }));
      case '/api/v1/pictures/shows/7':
        res.write(jsonEncode(streamed(rev: rev())));
      case '/api/v1/pictures/b1/screen' || '/api/v1/pictures/b2/screen' || '/api/v1/pictures/b2/video' || '/api/v1/parts/5/stream':
        res.add([1, 2, 3]);
      case '/api/v1/pictures/b1/info':
        res.write(jsonEncode({'taken_at': 1, 'place': 'Kempten'}));
      case '/api/v1/pictures/project':
        res.headers.contentType = ContentType('application', 'zip');
        res.add(zip);
      default:
        res.statusCode = 404;
        res.write(jsonEncode({'error': 'not_found'}));
    }
    await res.close();
  });
  return (server, ApiClient(address: 'localhost:${server.port}'), seen);
}

/// A server that gives the files at once from a folder, or none at all ([broken]).
class FakeRemote implements ShowRemote {
  FakeRemote(this.cache, {this.broken = false});

  @override
  final Directory cache;
  final bool broken;
  final List<String> asked = [];

  @override
  Future<String> fetch(String name) async {
    asked.add(name);
    if (broken) throw const SocketException('no server');
    final f = File('${cache.path}/${name.replaceAll('/', '-')}')..writeAsBytesSync([0]);
    return f.path;
  }

  @override
  Future<String?> place(String key) async => key == 'b1' ? 'Kempten' : null;

  @override
  Future<void> dispose() async {}
}

void main() {
  late Directory dir;
  setUp(() => dir = Directory.systemTemp.createTempSync('show-server'));
  tearDown(() => dir.deleteSync(recursive: true));

  String date() => (find.byKey(const ValueKey('show-date')).evaluate().single.widget as Text).data!;

  test('a project of the server is read as it plays from there: pictures, videos and music by name, nothing that leads out', () {
    final p = ShowProject.remote(streamed(), FakeRemote(dir));
    expect([for (final e in p.pictures) e.image], ['screen/b1', 'screen/b2']);
    expect([for (final e in p.pictures) e.video], [null, 'video/b2']);
    expect((p.pictures[0].seconds, p.pictures[1].transition), (3, 'zoom'));
    expect([for (final t in p.music) t.file], ['part/5']);
    expect((p.serverId, p.serverRev, p.title), (7, 'aaaa', 'Allgäu'));
    expect(showKeyOf('screen/b1'), 'b1');
  });

  testWidgets('played from the server: each picture is fetched before it shows, the place is asked for, the music comes from there',
      (tester) async {
    final remote = FakeRemote(dir);
    final p = ShowProject.remote(streamed(), remote);
    final music = FakeBackend();
    await tester.pumpWidget(app(ShowPlayerScreen(project: p, musicEngine: () => music)));
    await tester.pump();
    await tester.pump();
    expect(date(), '1. Juli 2017');
    expect(find.text('Kempten'), findsOneWidget);
    expect(music.loadedPath, endsWith('part-5'));
    expect(remote.asked, containsAll(['screen/b1', 'part/5', 'screen/b2']), reason: 'the next one is fetched while one is shown');
    await tester.pump(const Duration(seconds: 3));
    await tester.pump();
    expect(date(), '2. August 2018');
    await tester.pumpWidget(const SizedBox());
    await tester.pump(const Duration(seconds: 1));
  });

  testWidgets('when the server gives none of the pictures, the show stops and says so instead of going round', (tester) async {
    final p = ShowProject.remote(streamed(), FakeRemote(dir, broken: true));
    await tester.pumpWidget(app(ShowPlayerScreen(project: p)));
    for (var i = 0; i < 4; i++) {
      await tester.pump();
    }
    expect(find.byKey(const ValueKey('show-unreachable')), findsOneWidget);
    expect(find.text('Der Server ist gerade nicht erreichbar.'), findsOneWidget);
    await tester.pumpWidget(const SizedBox());
    await tester.pump(const Duration(seconds: 1));
  });

  testWidgets('the rubric lists the projects of the server; a copy on the device says so, a newer version on the server too', (tester) async {
    final api = (await tester.runAsync(() => FakeShowApi.make(dir)))!;
    final c = ShowController(api: api, projects: ShowProjects(root: Directory('${dir.path}/slideshows')));
    await tester.runAsync(c.load);
    await tester.pumpWidget(app(Scaffold(body: CustomScrollView(slivers: [PicturesBody(controller: c)]))));
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 100)));
    await tester.pump();
    expect(find.text('Projekte auf dem Server'), findsOneWidget);
    expect(find.byKey(const ValueKey('show-server-7')), findsOneWidget);
    expect(find.byKey(const ValueKey('show-download-7')), findsOneWidget);

    await tester.runAsync(() => c.saveShow(c.server.single));
    await tester.pump();
    expect(find.text('2 Bilder · auf dem Gerät'), findsOneWidget);
    expect(find.byKey(const ValueKey('show-download-7')), findsNothing);

    api.rev = 'bbbb';
    await tester.runAsync(c.loadServer);
    await tester.pump();
    expect(find.text('2 Bilder · neuere Fassung auf dem Server'), findsOneWidget);
    expect(tester.widget<IconButton>(find.byKey(const ValueKey('show-download-7'))).tooltip, 'Aktualisieren');
  });
}
