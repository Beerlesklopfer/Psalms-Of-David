// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/screens/book_screen.dart';
import 'package:psalm/screens/home_screen.dart';
import 'package:psalm/screens/player_screen.dart';
import 'package:psalm/server/api_client.dart';
import 'package:psalm/server/server_source.dart';
import 'package:psalm/theme.dart';
import 'package:psalm/widgets.dart';

Map<String, Object?> _item(int id, {bool stats = true}) => {
  'id': id,
  'title': 'Book $id',
  'creator': 'Author $id',
  'parts': [
    {
      'id': id * 10 + 1,
      'disc': 1,
      'number': 1,
      'title': 'One',
      'duration_ms': 60000,
      if (stats) 'plays': 3,
      if (stats) 'last_played_at': 1793000000000,
    },
    {
      'id': id * 10 + 2,
      'disc': 1,
      'number': 2,
      'title': 'Two',
      'duration_ms': 60000,
      if (stats) 'plays': 0,
      if (stats) 'last_played_at': null,
    },
  ],
  if (stats) 'plays': 0,
  if (stats) 'last_played_at': 1793000000000,
};

/// A tiny stand-in for David. [newBody] null answers 404 on `/new` (an older server).
Future<(HttpServer, ApiClient)> _server({
  Object? newBody,
  bool stats = true,
  bool newBroken = false,
}) async {
  final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
  server.listen((req) {
    final path = req.uri.path;
    Object? body;
    var status = 200;
    if (path == '/api/v1/items') {
      body = [
        {'id': 1},
        {'id': 2},
      ];
    } else if (path == '/api/v1/items/1') {
      body = _item(1, stats: stats);
    } else if (path == '/api/v1/items/2') {
      body = _item(2, stats: stats);
    } else if (path == '/api/v1/new' && !newBroken && newBody != null) {
      body = newBody;
    } else if (path == '/api/v1/new' && newBroken) {
      body = 'garbage';
    } else {
      status = 404;
      body = {'error': 'not_found'};
    }
    req.response
      ..statusCode = status
      ..headers.contentType = ContentType.json
      ..write(jsonEncode(body));
    req.response.close();
  });
  return (server, ApiClient(address: 'localhost:${server.port}'));
}

Book _book(String id, {int plays = 0, List<int> trackPlays = const [0, 0], DateTime? last}) => Book(
  id: id,
  title: 'Title $id',
  author: 'Author $id',
  color: const Color(0xFF000000),
  plays: plays,
  lastPlayedAt: last,
  tracks: [
    for (var i = 0; i < trackPlays.length; i++)
      Track(
        disc: 1,
        number: i + 1,
        title: 'T${i + 1}',
        length: const Duration(seconds: 60),
        path: '/m/$id/$i.mp3',
        plays: trackPlays[i],
      ),
  ],
);

Widget _app(PlayerController p, Widget home, {String locale = 'en'}) => PlayerScope(
  controller: p,
  child: MaterialApp(
    locale: Locale(locale),
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    home: home,
  ),
);

void main() {
  group('loading', () {
    // The test binding blocks real HTTP; these tests talk to a stand-in server on loopback.
    setUpAll(() => HttpOverrides.global = null);
    test(
      'plays and last played come from the detail answer; the new ids keep the server order',
      () async {
        final (server, api) = await _server(
          newBody: {
            'days': 7,
            'items': [
              {'id': 2, 'new_at': 5},
              {'id': 1, 'new_at': 4},
            ],
          },
        );
        addTearDown(() => server.close(force: true));
        final lib = await loadLibrary(api);
        final b = lib.books.first;
        expect(b.plays, 0);
        expect(b.lastPlayedAt, DateTime.fromMillisecondsSinceEpoch(1793000000000));
        expect(b.tracks[0].plays, 3);
        expect(b.tracks[0].lastPlayedAt, DateTime.fromMillisecondsSinceEpoch(1793000000000));
        expect(b.tracks[1].plays, 0);
        expect(b.tracks[1].lastPlayedAt, isNull);
        expect(lib.newIds, ['2', '1']);
      },
    );

    test(
      'an older server without the fields: 0 and null; without /new: no ids and no error',
      () async {
        final (server, api) = await _server(stats: false);
        addTearDown(() => server.close(force: true));
        final lib = await loadLibrary(api);
        expect(lib.books.first.plays, 0);
        expect(lib.books.first.lastPlayedAt, isNull);
        expect(lib.books.first.tracks.first.plays, 0);
        expect(lib.newIds, isEmpty);
      },
    );

    test('newItems: answer, 404 and a broken answer', () async {
      final (s1, a1) = await _server(
        newBody: {
          'days': 3,
          'items': [
            {'id': 9, 'new_at': 1},
          ],
        },
      );
      addTearDown(() => s1.close(force: true));
      expect(await a1.newItems(), [9]);
      final (s2, a2) = await _server();
      addTearDown(() => s2.close(force: true));
      expect(await a2.newItems(), isEmpty);
      final (s3, a3) = await _server(newBroken: true);
      addTearDown(() => s3.close(force: true));
      expect(await a3.newItems(), isEmpty);
    });
  });

  group('check mark', () {
    testWidgets('the book page marks played tracks only', (tester) async {
      final b = _book('1', trackPlays: [2, 0]);
      final p = PlayerController([b]);
      await tester.pumpWidget(_app(p, BookScreen(book: b)));
      expect(find.byKey(const ValueKey('played-mark')), findsOneWidget);
      expect(find.byTooltip('Played'), findsOneWidget);
      final icon = tester.widget<Icon>(find.byKey(const ValueKey('played-mark')));
      expect(icon.icon, Icons.check_circle);
      expect(icon.color, Palette.played);
      p.dispose();
    });

    testWidgets('the player list marks played tracks, in German too', (tester) async {
      final b = _book('1', trackPlays: [0, 1]);
      final p = PlayerController([b]);
      tester.view.physicalSize = const Size(800, 3000);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(_app(p, const PlayerScreen(), locale: 'de'));
      expect(find.byKey(const ValueKey('played-mark')), findsOneWidget);
      expect(find.byTooltip('Gespielt'), findsOneWidget);
      p.dispose();
    });

    testWidgets(
      'without a backend the demo ticks mark the track heard; updateLibrary takes the server back',
      (tester) async {
        final b = _book('1');
        final p = PlayerController([b]);
        p.open(b, track: 0);
        p.seek(const Duration(seconds: 56));
        expect(p.isTrackPlayed(b, 0), isFalse);
        await tester.pump(const Duration(milliseconds: 500));
        await tester.pump(const Duration(milliseconds: 500));
        expect(p.isTrackPlayed(b, 0), isTrue, reason: 'within 3 s of the end');
        expect(p.isTrackPlayed(b, 1), isFalse);
        p.updateLibrary(LoadedLibrary([_book('1')], {}));
        expect(p.isTrackPlayed(p.library.first, 0), isFalse, reason: 'the server decides again');
        p.dispose();
      },
    );
  });

  group('plays line', () {
    final last = DateTime(2026, 10, 27, 12);
    testWidgets('English with plural and date', (tester) async {
      final b = _book('1', plays: 3, last: last);
      final p = PlayerController([b]);
      await tester.pumpWidget(_app(p, BookScreen(book: b)));
      expect(find.text('Played 3 times · last played on Oct 27, 2026'), findsOneWidget);
      p.dispose();
    });

    testWidgets('once', (tester) async {
      final b = _book('1', plays: 1, last: last);
      final p = PlayerController([b]);
      await tester.pumpWidget(_app(p, BookScreen(book: b)));
      expect(find.textContaining('Played once ·'), findsOneWidget);
      p.dispose();
    });

    testWidgets('German with date', (tester) async {
      final b = _book('1', plays: 3, last: last);
      final p = PlayerController([b]);
      await tester.pumpWidget(_app(p, BookScreen(book: b), locale: 'de'));
      expect(
        find.textContaining('3-mal vollständig gehört · zuletzt gespielt am '),
        findsOneWidget,
      );
      expect(find.textContaining('2026'), findsWidgets);
      p.dispose();
    });

    testWidgets('nothing played: no line', (tester) async {
      final b = _book('1');
      final p = PlayerController([b]);
      await tester.pumpWidget(_app(p, BookScreen(book: b)));
      expect(find.byKey(const ValueKey('plays-line')), findsNothing);
      p.dispose();
    });
  });

  group('New row', () {
    Future<void> pumpHome(WidgetTester tester, PlayerController p, {String locale = 'en'}) async {
      tester.view.physicalSize = const Size(1000, 2400);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(_app(p, const Scaffold(body: HomeScreen()), locale: locale));
    }

    testWidgets('appears with the books of the list, in server order, unknown ids skipped', (
      tester,
    ) async {
      final p = PlayerController([_book('1'), _book('2'), _book('3')]);
      p.newIds = ['3', '99', '1'];
      expect(p.newBooks().map((b) => b.id), ['3', '1']);
      await pumpHome(tester, p);
      expect(find.text('New'), findsWidgets);
      expect(find.byKey(const ValueKey('book-badge')), findsNWidgets(2));
      final badges = find.byKey(const ValueKey('book-badge'));
      expect(
        tester.getTopLeft(badges.at(0)).dx,
        lessThan(tester.getTopLeft(badges.at(1)).dx),
        reason: 'the first card is the first id of the server',
      );
      final firstCard = find.ancestor(of: badges.at(0), matching: find.byType(BookCard));
      expect(find.descendant(of: firstCard, matching: find.text('Title 3')), findsWidgets);
      p.dispose();
    });

    testWidgets('empty list or only unknown ids: no row at all', (tester) async {
      final p = PlayerController([_book('1')]);
      await pumpHome(tester, p);
      expect(find.byKey(const ValueKey('book-badge')), findsNothing);
      expect(find.text('New'), findsNothing);
      p.newIds = ['42'];
      p.notifyListeners();
      await tester.pump();
      expect(find.text('New'), findsNothing);
      p.dispose();
    });

    testWidgets('German heading and badge; a reload replaces the list', (tester) async {
      final p = PlayerController([_book('1')]);
      p.newIds = ['1'];
      await pumpHome(tester, p, locale: 'de');
      expect(find.text('Neu'), findsWidgets);
      p.updateLibrary(LoadedLibrary([_book('1')], {}));
      await tester.pump();
      expect(find.byKey(const ValueKey('book-badge')), findsNothing);
      p.dispose();
    });
  });
}
