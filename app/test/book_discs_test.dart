// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/screens/item_screen.dart';

/// A book of which CD 3 has not been imported (yet): the CDs are 1, 2 and 4.
LibraryItem _gappy() => LibraryItem(
  id: '1',
  title: 'Sturz der Titanen',
  author: 'Ken Follett',
  color: const Color(0xFF6E1F2B),
  tracks: [
    for (final (disc, n) in [(1, 1), (1, 2), (2, 1), (4, 1), (4, 2), (4, 5)])
      Track(disc: disc, number: n, title: 'CD$disc TR$n', length: const Duration(minutes: 5)),
  ],
);

Widget _app(LibraryItem book) => PlayerScope(
  controller: PlayerController([book]),
  child: MaterialApp(
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    home: ItemScreen(book: book),
  ),
);

void main() {
  test(
    'the numbers of the CDs are the ones the book has, in order, whatever is missing between them',
    () {
      final b = _gappy();
      expect(b.discNumbers, [1, 2, 4]);
      expect(b.discs, 3, reason: 'the count stays the count');
    },
  );

  testWidgets('a missing CD 3 leaves no empty CD 3, and CD 4 is shown with its tracks', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(900, 3000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(_app(_gappy()));
    await tester.pumpAndSettle();
    expect(find.text('CD 1'), findsOneWidget);
    expect(find.text('CD 2'), findsOneWidget);
    expect(find.text('CD 3'), findsNothing, reason: 'there is no CD 3 in this book');
    expect(find.text('CD 4'), findsOneWidget);
    for (final title in ['CD4 TR1', 'CD4 TR2', 'CD4 TR5']) {
      expect(find.text(title), findsOneWidget, reason: title);
    }
  });
}
