// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/audio/fake_backend.dart';
import 'package:psalm/audio/file_formats.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/screens/book_screen.dart';
import 'package:psalm/server/api_client.dart';
import 'package:psalm/server/server_source.dart';

Book _book() => Book(
  id: '1',
  title: 'Caipiranha',
  author: 'Duo Camillo',
  color: const Color(0xFF6E1F2B),
  tracks: const [
    Track(disc: 1, number: 1, title: 'Der neue Himmel', length: Duration(minutes: 3), partId: 10, format: 'ogg', mime: 'audio/ogg'),
    Track(disc: 1, number: 2, title: 'Warten', length: Duration(minutes: 3), partId: 11, format: 'mp3', mime: 'audio/mpeg'),
    Track(disc: 1, number: 3, title: 'Unbekannt', length: Duration(minutes: 3), partId: 12),
  ],
);

/// A tall window, so that the tracks are not under the mini player.
void _tall(WidgetTester tester) {
  tester.view.physicalSize = const Size(800, 1800);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
}

Widget _app(PlayerController p) => PlayerScope(
  controller: p,
  child: MaterialApp(
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    locale: const Locale('en'),
    home: BookScreen(book: p.library.first),
  ),
);

class _Source extends MediaSource {
  @override
  Future<String> fetchPart(Track track) async => '/cache/${track.partId}.audio';

  @override
  Future<void> saveProgress(Book book, Track track, Duration position, {required bool finished}) async {}

  @override
  Future<PassageHit?> resolvePassage(String text) async => null;
}

void main() {
  tearDown(() => runsOnApple = () => false);

  test('an Apple platform (iPhone, iPad, Mac) cannot play Ogg and a few others; every other platform and an unknown format plays', () {
    runsOnApple = () => true;
    for (final f in ['ogg', 'OGG', 'opus', 'wma', 'ape']) {
      expect(formatPlayableHere(f), isFalse, reason: f);
    }
    for (final f in ['mp3', 'm4a', 'flac', 'wav', '', null]) {
      expect(formatPlayableHere(f), isTrue, reason: '$f');
    }
    runsOnApple = () => false;
    expect(formatPlayableHere('ogg'), isTrue, reason: 'Android and Linux play Ogg');
  });

  test('the player does not try a file the platform cannot decode, and says which track', () {
    runsOnApple = () => true;
    final backend = FakeBackend();
    final p = PlayerController([_book()], backend: backend, source: _Source());
    p.open(p.library.first, track: 0);
    expect(p.unplayable?.title, 'Der neue Himmel');
    expect(p.playing, isFalse);
    expect(backend.loadedPath, isNull, reason: 'nothing was loaded');
    // A playable track clears it again.
    p.open(p.library.first, track: 1);
    expect(p.unplayable, isNull);
    p.dispose();
  });

  testWidgets('on an iPhone a tap on an Ogg track shows the message and starts nothing; an MP3 track plays', (tester) async {
    runsOnApple = () => true;
    _tall(tester);
    final p = PlayerController([_book()]);
    await tester.pumpWidget(_app(p));
    await tester.tap(find.text('Der neue Himmel'));
    await tester.pump();
    expect(find.text('This file format cannot be played on Apple devices (iPhone, iPad, Mac) yet.'), findsOneWidget);
    expect(p.playing, isFalse);
    await tester.tap(find.text('Warten'));
    await tester.pump();
    expect(p.playing, isTrue);
    p.dispose();
    await tester.pump();
  });

  testWidgets('on other platforms the Ogg track plays', (tester) async {
    runsOnApple = () => false;
    _tall(tester);
    final p = PlayerController([_book()]);
    await tester.pumpWidget(_app(p));
    await tester.tap(find.text('Der neue Himmel'));
    await tester.pump();
    expect(find.text('This file format cannot be played on Apple devices (iPhone, iPad, Mac) yet.'), findsNothing);
    expect(p.playing, isTrue);
    p.dispose();
    await tester.pump();
  });

  test('a track the server can make an MP3 of is playable on the iPhone, one it cannot is not', () {
    runsOnApple = () => true;
    expect(trackPlayableHere('ogg', 'mp3'), isTrue);
    expect(trackPlayableHere('ogg', null), isFalse, reason: 'an older server: the message as before');
    expect(trackPlayableHere('mp3', null), isTrue);
    expect(needsTranscode('ogg', 'mp3'), isTrue);
    expect(needsTranscode('ogg', null), isFalse);
    expect(needsTranscode('mp3', 'mp3'), isFalse, reason: 'a file that plays is asked for as it is');
    runsOnApple = () => false;
    expect(needsTranscode('ogg', 'mp3'), isFalse, reason: 'Android and Linux play Ogg themselves');
  });

  test('on the iPhone a track with an MP3 on offer is played, and the message stays only if the server cannot make it', () async {
    runsOnApple = () => true;
    const ogg = Track(disc: 1, number: 1, title: 'Der neue Himmel', length: Duration(minutes: 3), partId: 10, format: 'ogg', transcode: 'mp3');
    Book book() => Book(id: '1', title: 'Caipiranha', author: 'Duo Camillo', color: const Color(0xFF6E1F2B), tracks: const [ogg]);

    final backend = FakeBackend();
    final p = PlayerController([book()], backend: backend, source: _Source());
    p.open(p.library.first, track: 0);
    await pumpEventQueue();
    expect(p.unplayable, isNull);
    expect(backend.loadedPath, '/cache/10.audio', reason: 'the source gives the path of the made MP3');
    p.dispose();

    final failing = PlayerController([book()], backend: FakeBackend(), source: _FailingSource());
    failing.open(failing.library.first, track: 0);
    await pumpEventQueue();
    expect(failing.unplayable?.title, 'Der neue Himmel', reason: 'the server answered transcode_failed: the message as without transcoding');
    expect(failing.playing, isFalse);
    expect(failing.error, isNull);
    failing.dispose();
  });

  test('the part is asked for as an MP3 and kept under its own name next to the original', () async {
    runsOnApple = () => true;
    // The test binding answers every HTTP request with 400; this test talks to a server of its own on the loopback.
    HttpOverrides.global = null;
    final asked = <String>[];
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    server.listen((req) async {
      asked.add(req.uri.toString());
      req.response.add(const [1, 2, 3, 4]);
      await req.response.close();
    });
    final dir = Directory.systemTemp.createTempSync('psalm-format-');
    addTearDown(() async {
      await server.close(force: true);
      dir.deleteSync(recursive: true);
    });
    final source = ServerSource(ApiClient(address: '127.0.0.1:${server.port}', token: 't'), dir);
    const ogg = Track(disc: 1, number: 1, title: 'A', length: Duration(minutes: 1), partId: 10, format: 'ogg', transcode: 'mp3');
    const mp3 = Track(disc: 1, number: 2, title: 'B', length: Duration(minutes: 1), partId: 11, format: 'mp3', transcode: null);

    final made = await source.fetchPart(ogg);
    expect(made, '${dir.path}/10-mp3.audio');
    expect(asked, ['/api/v1/parts/10/stream?as=mp3']);
    expect(source.isDownloaded(ogg), isTrue);
    expect(source.relativePath(ogg), '10-mp3.audio');
    // A second time it is on the device: no request.
    await source.fetchPart(ogg);
    expect(asked, hasLength(1));
    // A file that plays is fetched as it is.
    expect(await source.fetchPart(mp3), '${dir.path}/11.audio');
    expect(asked.last, '/api/v1/parts/11/stream');
    // Where the platform plays Ogg itself, the original is asked for.
    runsOnApple = () => false;
    expect(await source.fetchPart(ogg), '${dir.path}/10.audio');
    expect(asked.last, '/api/v1/parts/10/stream');
  });
}

class _FailingSource extends MediaSource {
  @override
  Future<String> fetchPart(Track track) => Future.error(ApiException(422, 'transcode_failed'));

  @override
  Future<void> saveProgress(Book book, Track track, Duration position, {required bool finished}) async {}

  @override
  Future<PassageHit?> resolvePassage(String text) async => null;
}
