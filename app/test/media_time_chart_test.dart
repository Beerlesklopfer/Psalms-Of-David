// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/screens/media_time_chart.dart';
import 'package:psalm/server/server_source.dart';

Widget _page(FamilyTime t) => MaterialApp(
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      supportedLocales: AppLocalizations.supportedLocales,
      home: Scaffold(body: SizedBox(width: 600, child: MediaTimeChart(time: t))),
    );

void main() {
  testWidgets('the chart has a line for every kind and one for all together, with a legend', (tester) async {
    const min = 60000;
    final t = FamilyTime(
      id: 1,
      name: 'Anna',
      me: true,
      fromDay: 20000,
      kinds: {'audiobook': [for (var i = 0; i < 14; i++) i * min], 'music': [for (var i = 0; i < 14; i++) 2 * min]},
      total: [for (var i = 0; i < 14; i++) (i + 2) * min],
    );
    await tester.pumpWidget(_page(t));
    expect(find.text('Audiobooks'), findsOneWidget);
    expect(find.text('Music'), findsOneWidget);
    expect(find.text('Total'), findsOneWidget);
    expect(find.byType(CustomPaint), findsWidgets);
  });

  testWidgets('without any listening the chart says so', (tester) async {
    final t = FamilyTime(id: 1, name: 'Anna', me: true, fromDay: 20000, kinds: const {}, total: List.filled(14, 0));
    await tester.pumpWidget(_page(t));
    expect(find.text('Nothing listened to in the last days.'), findsOneWidget);
  });
}
