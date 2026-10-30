// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/labels.dart';
import 'package:psalm/models.dart';
import 'package:psalm/screens/labels_ui.dart';
import 'package:psalm/screens/share_button.dart';

import 'labels_test.dart' show FakeLabels;

Book _book({required bool can}) => Book(
  id: '1',
  title: 'Eins',
  author: 'Autor',
  color: const Color(0xFF6E1F2B),
  canShare: can,
  tracks: const [Track(disc: 1, number: 1, title: 'Eins', length: Duration(minutes: 5))],
);

Widget _app(Widget home) => MaterialApp(
  localizationsDelegates: AppLocalizations.localizationsDelegates,
  supportedLocales: AppLocalizations.supportedLocales,
  locale: const Locale('en'),
  home: Scaffold(body: home),
);

void main() {
  testWidgets('a parent shares a medium: title, creator and the address of the server', (tester) async {
    final sent = <(String, String)>[];
    await tester.pumpWidget(_app(ShareButton(
      book: _book(can: true),
      address: 'david.example:8443',
      share: (text, subject) async => sent.add((text, subject)),
    )));
    await tester.tap(find.byKey(const ValueKey('share-book')));
    expect(sent, [('Eins by Autor\nhttps://david.example:8443/', 'Eins')]);
  });

  testWidgets('a child or a guest has no share button', (tester) async {
    await tester.pumpWidget(_app(ShareButton(book: _book(can: false), share: (_, _) async {})));
    expect(find.byKey(const ValueKey('share-book')), findsNothing);
  });

  testWidgets('the labels card lists labels; an administrator renames and deletes', (tester) async {
    final api = FakeLabels();
    final controller = LabelsController()..api = api;
    await tester.pumpWidget(_app(LabelsCard(controller: controller)));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('label-row-Gebet')), findsOneWidget);

    await tester.tap(find.byKey(const ValueKey('label-rename-Gebet')));
    await tester.pumpAndSettle();
    await tester.enterText(find.byKey(const ValueKey('label-rename-field')), 'Andacht');
    await tester.tap(find.byKey(const ValueKey('label-rename-save')));
    await tester.pumpAndSettle();
    expect(api.managed, ['rename:Gebet:Andacht']);
    expect(find.byKey(const ValueKey('label-row-Andacht')), findsOneWidget);

    await tester.tap(find.byKey(const ValueKey('label-delete-Schule')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('label-delete-yes')));
    await tester.pumpAndSettle();
    expect(api.managed.last, 'delete:Schule');
    expect(find.byKey(const ValueKey('label-row-Schule')), findsNothing);
  });

  testWidgets('a member sees the labels but cannot manage them', (tester) async {
    final api = FakeLabels()..current = const LabelList([('Gebet', 2)], false);
    await tester.pumpWidget(_app(LabelsCard(controller: LabelsController()..api = api)));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('label-row-Gebet')), findsOneWidget);
    expect(find.byKey(const ValueKey('label-rename-Gebet')), findsNothing);
  });
}
