// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/screens/item_screen.dart';
import 'package:psalm/screens/home_screen.dart';
import 'package:psalm/screens/library_screen.dart';
import 'package:psalm/screens/search_screen.dart';

LibraryItem _book(String id, String title, {String kind = 'audiobook', String? narrator}) => LibraryItem(
  id: id,
  title: title,
  author: 'Author $id',
  color: const Color(0xFF000000),
  kind: kind,
  narrator: narrator,
  tracks: [Track(disc: 1, number: 1, title: 'T1', length: const Duration(minutes: 5), partId: int.parse(id))],
);

Widget _app(PlayerController p, Widget home) => PlayerScope(
  controller: p,
  child: MaterialApp(
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    home: Scaffold(body: home),
  ),
);

void main() {
  testWidgets('the library has music, audiobooks, videos and e-books; videos and e-books are a stub', (tester) async {
    final p = PlayerController([_book('1', 'Winter der Welt'), _book('2', 'Versprochen', kind: 'music')]);
    await tester.pumpWidget(_app(p, const LibraryScreen()));

    for (final label in ['Music', 'Audiobooks', 'Videos', 'E-Books']) {
      expect(find.text(label), findsOneWidget, reason: label);
    }
    // Audiobooks first: only the audiobook is there.
    expect(find.text('Winter der Welt'), findsWidgets, reason: 'the cover and the caption both say it');
    expect(find.text('Versprochen'), findsNothing);

    await tester.tap(find.text('Music'));
    await tester.pumpAndSettle();
    expect(find.text('Versprochen'), findsWidgets);
    expect(find.text('Winter der Welt'), findsNothing);

    await tester.tap(find.text('Videos'));
    await tester.pumpAndSettle();
    expect(find.text('Not available yet'), findsOneWidget);
    expect(find.textContaining('Videos are planned'), findsOneWidget);

    await tester.tap(find.text('E-Books'));
    await tester.pumpAndSettle();
    expect(find.text('Not available yet'), findsOneWidget);
    expect(find.textContaining('E-books and PDF files are planned'), findsOneWidget);
    p.dispose();
  });

  testWidgets('an empty category says so', (tester) async {
    final p = PlayerController([_book('1', 'Winter der Welt')]);
    await tester.pumpWidget(_app(p, const LibraryScreen()));
    await tester.tap(find.text('Music'));
    await tester.pumpAndSettle();
    expect(find.text('No music in the library yet'), findsOneWidget);
    p.dispose();
  });

  testWidgets('the book page names the narrator', (tester) async {
    final p = PlayerController([_book('1', 'Winter der Welt', narrator: 'Philipp Schepmann')]);
    await tester.pumpWidget(_app(p, ItemScreen(book: p.library.first)));
    expect(find.text('Read by Philipp Schepmann'), findsOneWidget);
    p.dispose();
  });

  testWidgets('the search has the same media filter, with All in front', (tester) async {
    final p = PlayerController([_book('1', 'Winter der Welt'), _book('2', 'Versprochen', kind: 'music')]);
    await tester.pumpWidget(_app(p, const SearchScreen()));
    for (final label in ['All', 'Music', 'Audiobooks', 'Videos', 'E-Books']) {
      expect(find.text(label), findsOneWidget, reason: label);
    }
    // All: authors and artists of everything.
    expect(find.text('Authors and artists'), findsOneWidget);
    expect(find.text('Author 1'), findsOneWidget);
    expect(find.text('Author 2'), findsOneWidget);

    await tester.tap(find.text('Music'));
    await tester.pumpAndSettle();
    expect(find.text('Artists'), findsOneWidget);
    expect(find.text('Author 2'), findsOneWidget);
    expect(find.text('Author 1'), findsNothing);

    // A search is limited to the chosen kind.
    await tester.enterText(find.byType(TextField), 'welt');
    await tester.pumpAndSettle();
    expect(find.text('Nothing found'), findsOneWidget, reason: 'the audiobook is not music');
    await tester.tap(find.text('Audiobooks'));
    await tester.pumpAndSettle();
    expect(find.text('Winter der Welt'), findsWidgets);

    await tester.tap(find.text('Videos'));
    await tester.pumpAndSettle();
    expect(find.text('Not available yet'), findsOneWidget);
    p.dispose();
  });

  testWidgets('for music the extra line is the artist, not "read by"', (tester) async {
    final p = PlayerController([_book('1', 'Thea', kind: 'music', narrator: 'Thea Eichholz-Müller')]);
    await tester.pumpWidget(_app(p, ItemScreen(book: p.library.first)));
    expect(find.text('Artist: Thea Eichholz-Müller'), findsOneWidget);
    expect(find.textContaining('Read by'), findsNothing);
    p.dispose();
  });

  testWidgets('the home page does not repeat the library: no strips of all the music and all the audiobooks', (tester) async {
    tester.view.physicalSize = const Size(1000, 2400);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    final p = PlayerController([_book('1', 'Winter der Welt'), _book('2', 'Versprochen', kind: 'music')]);
    await tester.pumpWidget(_app(p, const HomeScreen()));
    expect(find.text('Your music'), findsNothing);
    expect(find.text('Your audiobooks'), findsNothing);
    p.dispose();
  });

  testWidgets('the home page greets the user by nickname and shows what was listened to last', (tester) async {
    tester.view.physicalSize = const Size(1000, 2400);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    final p = PlayerController([
      _book('1', 'First Book'),
      _book('2', 'Second Book'),
      _book('3', 'Third Book', kind: 'music'),
    ]);
    p.recentIds.addAll(['3', '1']);
    await tester.pumpWidget(_app(p, const HomeScreen()));
    expect(find.text('Recently listened'), findsOneWidget);
    expect(find.textContaining(', Papa'), findsNothing, reason: 'no name without a server');
    expect(p.recentItems().map((b) => b.id), ['3', '1'], reason: 'the latest first, any kind, only what exists');

    // Opening a book puts it in front at once; at most six are shown.
    p.open(p.library[1]);
    expect(p.recentItems().map((b) => b.id), ['2', '3', '1']);
    p.userName = 'Papa';
    p.notifyListeners();
    await tester.pump();
    expect(find.textContaining(', Papa'), findsOneWidget);
    p.dispose();
  });

  testWidgets('without anything listened to there is no heading for recent books', (tester) async {
    final p = PlayerController([_book('1', 'First Book')]);
    await tester.pumpWidget(_app(p, const HomeScreen()));
    expect(find.text('Recently listened'), findsNothing);
    p.dispose();
  });

  testWidgets('"Continue listening" is the first thing on the home page, above the recent books', (tester) async {
    tester.view.physicalSize = const Size(1000, 2400);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    final p = PlayerController([_book('1', 'First Book'), _book('2', 'Second Book')]);
    p.recentIds.addAll(['2', '1']);
    await tester.pumpWidget(_app(p, const HomeScreen()));
    final continueAt = tester.getTopLeft(find.text('Continue listening')).dy;
    final recentAt = tester.getTopLeft(find.text('Recently listened')).dy;
    expect(continueAt, lessThan(recentAt));
    p.dispose();
  });
}
