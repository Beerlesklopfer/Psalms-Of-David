// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/demo_data.dart';
import 'package:psalm/main.dart';
import 'package:psalm/player_controller.dart';

void main() {
  group('PlayerController', () {
    test('skip crosses track boundaries', () {
      final p = PlayerController(demoBooks);
      p.seek(p.track!.length - const Duration(seconds: 5));
      final before = p.trackIndex;
      p.skip(const Duration(seconds: 30));
      expect(p.trackIndex, before + 1);
      expect(p.position, const Duration(seconds: 25));
      p.dispose();
    });

    test('skipping back at the start of a track goes to the previous track', () {
      final p = PlayerController(demoBooks);
      p.seek(Duration.zero);
      final before = p.trackIndex;
      p.skip(const Duration(seconds: -10));
      expect(p.trackIndex, before - 1);
      p.dispose();
    });

    test('progress of a book that is not playing is remembered', () {
      final p = PlayerController(demoBooks);
      final first = demoBooks.first;
      final other = demoBooks[1];
      p.open(first, track: 3);
      p.open(other);
      expect(p.progressOf(first), greaterThan(0));
      expect(p.progressOf(other), 0);
      p.dispose();
    });
  });

  testWidgets('home shows greeting and mini player', (tester) async {
    await tester.pumpWidget(const PsalmApp(demo: true, locale: Locale('de')));
    expect(find.textContaining('Guten'), findsOneWidget);
    expect(find.text('Kinder der Freiheit'), findsWidgets);
    expect(find.text('Suche'), findsOneWidget);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('search filters by author', (tester) async {
    await tester.pumpWidget(const PsalmApp(demo: true, locale: Locale('de')));
    await tester.tap(find.text('Suche'));
    await tester.pump();
    await tester.enterText(find.byType(TextField), 'Storm');
    await tester.pump();
    // Only Storm's book remains, as exactly one result row.
    expect(find.byType(ListTile), findsOneWidget);
    expect(find.text('Theodor Storm'), findsOneWidget);
    expect(find.text('Faust'), findsNothing);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('unsupported language falls back to English', (tester) async {
    await tester.pumpWidget(const PsalmApp(demo: true, locale: Locale('fr')));
    await tester.pump();
    expect(find.text('Search'), findsOneWidget);
    expect(find.text('Suche'), findsNothing);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('German is available as second language', (tester) async {
    await tester.pumpWidget(const PsalmApp(demo: true, locale: Locale('de')));
    expect(find.text('Bibliothek'), findsOneWidget);
    expect(find.text('Library'), findsNothing);
    await tester.pumpWidget(const SizedBox());
  });
}
