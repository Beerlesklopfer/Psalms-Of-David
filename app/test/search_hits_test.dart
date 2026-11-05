// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/screens/item_screen.dart';
import 'package:psalm/screens/player_screen.dart';
import 'package:psalm/screens/search_screen.dart';

final _album = LibraryItem(
  id: '7',
  title: 'Er hört dein Gebet',
  author: 'Christoph Zehendner',
  color: const Color(0xFF000000),
  kind: 'music',
  tracks: [
    Track(disc: 1, number: 1, title: 'Lobgesang', length: const Duration(minutes: 3), partId: 71),
    Track(disc: 1, number: 2, title: 'Abendlied', length: const Duration(minutes: 4), partId: 72),
  ],
);

Widget _app(PlayerController p, {Widget home = const SearchScreen()}) => PlayerScope(
  controller: p,
  child: MaterialApp(
    locale: const Locale('en'),
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    home: Scaffold(body: home),
  ),
);

void main() {
  testWidgets('a track that is found leads to its album, with the tile of the album page', (tester) async {
    await tester.pumpWidget(_app(PlayerController([_album])));
    await tester.enterText(find.byType(TextField), 'abendlied');
    await tester.pump();
    final hit = find.byKey(const ValueKey('search-track-7-0'));
    expect(hit, findsOneWidget);
    expect(find.descendant(of: hit, matching: find.text('Er hört dein Gebet · Christoph Zehendner')), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('open-album-7-2')));
    await tester.pumpAndSettle();
    expect(find.byType(ItemScreen), findsOneWidget);
  });

  testWidgets('an album that is found has the download of the album page', (tester) async {
    await tester.pumpWidget(_app(PlayerController([_album])));
    await tester.enterText(find.byType(TextField), 'gebet');
    await tester.pump();
    expect(find.descendant(of: find.byType(AlbumTile), matching: find.byType(DownloadButton)), findsOneWidget);
  });

  testWidgets('the chapters of the player are the tiles of the album page, counted through and with the disc', (tester) async {
    tester.view.physicalSize = const Size(800, 3000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    final p = PlayerController([_album]);
    await tester.pumpWidget(_app(p, home: const PlayerScreen()));
    expect(find.byType(TrackTile), findsNWidgets(2));
    expect(find.descendant(of: find.byType(TrackTile).last, matching: find.text('2')), findsOneWidget);
    p.dispose();
  });
}
