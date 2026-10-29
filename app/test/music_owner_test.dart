// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/library_folds.dart';
import 'package:psalm/library_groups.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/screens/library_screen.dart';

Book _music(String id, String title, {String? owner, String? genre}) => Book(
  id: id,
  title: title,
  author: 'Band $id',
  color: const Color(0xFF000000),
  kind: 'music',
  owner: owner,
  genre: genre,
  tracks: [
    Track(
      disc: 1,
      number: 1,
      title: 'T1',
      length: const Duration(minutes: 5),
      partId: int.parse(id),
    ),
  ],
);

Widget _app(PlayerController p, Locale locale, {LibraryFolds? folds}) => PlayerScope(
  controller: p,
  child: MaterialApp(
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    locale: locale,
    home: Scaffold(body: LibraryScreen(folds: folds ?? LibraryFolds())),
  ),
);

Book _audio(String id, String title, {String? owner}) => Book(
  id: id,
  title: title,
  author: 'Author $id',
  color: const Color(0xFF000000),
  owner: owner,
  tracks: [
    Track(
      disc: 1,
      number: 1,
      title: 'T1',
      length: const Duration(minutes: 5),
      partId: int.parse(id),
    ),
  ],
);

void main() {
  testWidgets(
    'the audiobooks are under "Audiobooks of <Nickname>" too, and in German "Hörbücher von <Nickname>"',
    (tester) async {
      tester.view.physicalSize = const Size(1000, 4000);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      final p = PlayerController([
        _audio('1', 'Erstes Buch', owner: 'Nora'),
        _audio('2', 'Zweites Buch'),
      ]);
      await tester.pumpWidget(_app(p, const Locale('en')));
      expect(find.text('Audiobooks of Nora'), findsOneWidget);
      await tester.scrollUntilVisible(
        find.text('Other audiobooks'),
        300,
        scrollable: find.byType(Scrollable).first,
      );
      expect(find.text('Other audiobooks'), findsOneWidget);
      await tester.pumpWidget(_app(p, const Locale('de')));
      await tester.pumpAndSettle();
      expect(find.text('Hörbücher von Nora'), findsOneWidget);
      p.dispose();
      // Nobody owns anything: the plain list as before.
      final q = PlayerController([_audio('1', 'Erstes Buch')]);
      await tester.pumpWidget(_app(q, const Locale('en')));
      await tester.pumpAndSettle();
      expect(find.textContaining('Audiobooks of'), findsNothing);
      q.dispose();
    },
  );

  test('by owner: the nicknames in alphabetical order, each with their albums, and what belongs to nobody last', () {
    final g = groupByOwner([
      _music('1', 'A', owner: 'Nora'),
      _music('2', 'B'),
      _music('3', 'C', owner: 'Anni'),
      _music('4', 'D', owner: 'Nora'),
    ]);
    expect(g.map((e) => e.$1).toList(), ['Anni', 'Nora', null]);
    expect(g[1].$2.map((b) => b.title).toList(), ['A', 'D']);
    expect(
      groupByOwner([_music('1', 'A')]).single.$1,
      isNull,
      reason: 'no owner at all: one group',
    );
  });

  testWidgets(
    'the music is under "Music of <Nickname>" for every owner, with the albums of nobody under "Other music"',
    (tester) async {
      final p = PlayerController([
        _music('1', 'Erstes Album', owner: 'Nora', genre: 'Pop'),
        _music('2', 'Zweites Album', owner: 'Anni', genre: 'Rock'),
        _music('3', 'Drittes Album'),
      ]);
      await tester.pumpWidget(_app(p, const Locale('en')));
      await tester.tap(find.text('Music'));
      await tester.pumpAndSettle();
      expect(find.text('Music of Anni'), findsOneWidget);
      expect(find.text('Music of Nora'), findsOneWidget);
      await tester.scrollUntilVisible(
        find.text('Other music'),
        300,
        scrollable: find.byType(Scrollable).first,
      );
      expect(find.text('Other music'), findsOneWidget);
      // Nobody is signed in here, so every group is closed until it is opened.
      expect(find.text('Erstes Album'), findsNothing);
      await tester.tap(find.byKey(const Key('fold-music-Nora')));
      await tester.pumpAndSettle();
      expect(find.text('Erstes Album'), findsWidgets);
      await tester.scrollUntilVisible(
        find.text('Music of Anni'),
        -300,
        scrollable: find.byType(Scrollable).first,
      );
      // A chosen genre narrows every group; the group without an album disappears.
      await tester.tap(find.byKey(const Key('genre-Rock')));
      await tester.pumpAndSettle();
      expect(find.text('Music of Anni'), findsOneWidget);
      expect(find.text('Music of Nora'), findsNothing);
      await tester.tap(find.byKey(const Key('fold-music-Anni')));
      await tester.pumpAndSettle();
      expect(find.text('Zweites Album'), findsWidgets);
      p.dispose();
    },
  );

  testWidgets(
    'in German it says "Musik von <Nickname>"; without any owner the music stays by genre',
    (tester) async {
      final p = PlayerController([_music('1', 'Erstes Album', owner: 'Nora', genre: 'Pop')]);
      await tester.pumpWidget(_app(p, const Locale('de')));
      await tester.tap(find.text('Musik'));
      await tester.pumpAndSettle();
      expect(find.text('Musik von Nora'), findsOneWidget);
      p.dispose();

      final q = PlayerController([_music('1', 'Erstes Album', genre: 'Pop')]);
      await tester.pumpWidget(_app(q, const Locale('en')));
      await tester.tap(find.text('Music'));
      await tester.pumpAndSettle();
      expect(find.textContaining('Music of'), findsNothing);
      expect(find.text('Pop'), findsWidgets, reason: 'the genre as the group');
      q.dispose();
    },
  );

  testWidgets(
    'with ten siblings only the own group is open and first; it can be closed too, and the choice is kept',
    (tester) async {
      tester.view.physicalSize = const Size(1000, 6000);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      final dir = Directory.systemTemp.createTempSync('folds');
      addTearDown(() => dir.deleteSync(recursive: true));
      final file = File('${dir.path}/library_folds.json');
      final names = ['Anni', 'Bert', 'Clara', 'Dirk', 'Nora'];
      final p = PlayerController([
        for (var i = 0; i < names.length; i++)
          _audio('${i + 1}', 'Buch von ${names[i]}', owner: names[i]),
      ])..userName = 'Nora';
      await tester.pumpWidget(_app(p, const Locale('en'), folds: LibraryFolds(file)));
      await tester.pumpAndSettle();
      // The own group comes first and is open; the nine others are closed.
      expect(find.text('Buch von Nora'), findsWidgets);
      for (final n in names.where((n) => n != 'Nora')) {
        expect(find.text('Buch von $n'), findsNothing, reason: n);
      }
      expect(
        tester.getTopLeft(find.text('Audiobooks of Nora')).dy,
        lessThan(tester.getTopLeft(find.text('Audiobooks of Anni')).dy),
      );
      // Another group opens; the own one closes: both work the same.
      await tester.tap(find.byKey(const Key('fold-audiobooks-Bert')));
      await tester.tap(find.byKey(const Key('fold-audiobooks-Nora')));
      await tester.pumpAndSettle();
      expect(find.text('Buch von Bert'), findsWidgets);
      expect(find.text('Buch von Nora'), findsNothing);
      // A new start (a new object on the same file) finds the same state.
      await tester.pumpWidget(const SizedBox());
      await tester.pumpWidget(_app(p, const Locale('en'), folds: LibraryFolds(file)));
      await tester.pumpAndSettle();
      expect(find.text('Buch von Bert'), findsWidgets);
      expect(find.text('Buch von Nora'), findsNothing);
      expect(find.text('Buch von Anni'), findsNothing);
      p.dispose();
    },
  );

  test('the first owner comes before the others in the groups', () {
    final g = groupByOwner([
      _music('1', 'A', owner: 'Anni'),
      _music('2', 'B', owner: 'Nora'),
    ], first: 'Nora');
    expect(g.map((e) => e.$1).toList(), ['Nora', 'Anni']);
  });
}
