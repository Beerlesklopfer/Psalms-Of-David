// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/screens/book_screen.dart';

Book _book({String? blurb}) => Book(
      id: '1',
      title: 'Kinder der Freiheit',
      author: 'Ken Follett',
      color: const Color(0xFF6E1F2B),
      tracks: const [Track(disc: 1, number: 1, title: 'T', length: Duration(minutes: 1))],
      blurb: blurb,
    );

Widget _app(Book book) => PlayerScope(
      controller: PlayerController([book]),
      child: MaterialApp(
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        home: BookScreen(book: book),
      ),
    );

void main() {
  testWidgets('the book screen shows the blurb', (tester) async {
    await tester.pumpWidget(_app(_book(blurb: 'Berlin, 1961: zwei Familien, zwei Seiten einer Mauer.')));
    expect(find.textContaining('zwei Seiten einer Mauer'), findsOneWidget);
  });

  testWidgets('without a blurb there is no empty gap or text', (tester) async {
    await tester.pumpWidget(_app(_book()));
    expect(find.textContaining('Berlin'), findsNothing);
    await tester.pumpWidget(_app(_book(blurb: '   ')));
    expect(find.byType(Text), findsWidgets);
  });
}
