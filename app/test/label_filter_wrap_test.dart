// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/screens/labels_ui.dart';

Widget _page(List<(String, int)> counts, Set<String> chosen, ValueChanged<Set<String>> on) => MaterialApp(
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      supportedLocales: AppLocalizations.supportedLocales,
      home: Scaffold(body: SizedBox(width: 900, child: LabelFilter(counts: counts, chosen: chosen, onChanged: on))),
    );

void main() {
  testWidgets('a few labels are chips that wrap into new lines', (tester) async {
    await tester.pumpWidget(_page([for (var i = 0; i < 6; i++) ('L$i', i + 1)], {}, (_) {}));
    expect(find.byType(FilterChip), findsNWidgets(6));
    expect(find.byKey(const ValueKey('label-filter-combo')), findsNothing);
  });

  testWidgets('more than three lines of labels become a combo box with a check box for each', (tester) async {
    Set<String> chosen = {};
    final counts = [for (var i = 0; i < 40; i++) ('Schlagwort$i', i + 1)];
    await tester.pumpWidget(_page(counts, chosen, (c) => chosen = c));
    expect(find.byType(FilterChip), findsNothing);
    await tester.tap(find.byKey(const ValueKey('label-filter-combo')));
    await tester.pumpAndSettle();
    expect(find.byType(CheckboxMenuButton), findsWidgets);
    await tester.tap(find.byKey(const ValueKey('label-filter-Schlagwort0')).first);
    await tester.pumpAndSettle();
    expect(chosen, {'Schlagwort0'});
  });
}
