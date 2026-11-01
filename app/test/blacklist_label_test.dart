// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/labels.dart';
import 'package:psalm/models.dart';
import 'package:psalm/screens/labels_ui.dart';

import 'labels_test.dart' show FakeLabels;

LibraryItem _book(String id, List<String> labels) => LibraryItem(
  id: id,
  title: 'T$id',
  author: 'A',
  color: const Color(0xFF6E1F2B),
  labels: labels,
  tracks: const [Track(disc: 1, number: 1, title: 'Eins', length: Duration(minutes: 5))],
);

void main() {
  test('the reserved name cannot be typed, in any spelling, and is not suggested', () {
    expect(cleanLabel('Blacklist'), isNull);
    expect(cleanLabel('  blackLIST '), isNull);
    expect(cleanLabel('Black list'), 'Black list');
    final books = [_book('1', ['Blacklist', 'Gebet']), _book('2', ['Gebet'])];
    expect(labelSuggestions(books, '', const []), ['Gebet']);
    expect(labelSuggestions(books, 'black', const []), isEmpty);
  });

  testWidgets('the settings list shows the system label of an administrator without rename and delete', (tester) async {
    final api = FakeLabels()..current = const LabelList([('Blacklist', 2), ('Gebet', 3)], true);
    final controller = LabelsController()..api = api;
    await tester.pumpWidget(
      MaterialApp(
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        locale: const Locale('en'),
        home: Scaffold(body: LabelsCard(controller: controller)),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('label-row-Blacklist')), findsOneWidget);
    expect(find.byKey(const ValueKey('label-rename-Blacklist')), findsNothing);
    expect(find.byKey(const ValueKey('label-delete-Blacklist')), findsNothing);
    expect(find.byKey(const ValueKey('label-rename-Gebet')), findsOneWidget);
  });
}
