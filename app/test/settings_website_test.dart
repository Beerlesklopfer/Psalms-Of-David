// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/screens/settings_screen.dart';
import 'package:psalm/server/credentials.dart';

final _credentials = Credentials(
  serverId: 'srv-1',
  address: 'pdc.example.org:8484',
  fingerprint: null,
  token: 't',
  deviceName: 'Laptop',
  ca: true,
);

Widget _page(Future<bool> Function(Uri) open, Locale locale) => MaterialApp(
  locale: locale,
  localizationsDelegates: AppLocalizations.localizationsDelegates,
  supportedLocales: AppLocalizations.supportedLocales,
  home: Scaffold(
    body: SettingsScreen(credentials: _credentials, openUrl: open),
  ),
);

void main() {
  testWidgets('the settings link to the website of the server, in the browser', (tester) async {
    tester.view.physicalSize = const Size(900, 2400);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    final opened = <Uri>[];
    await tester.pumpWidget(
      _page((u) async {
        opened.add(u);
        return true;
      }, const Locale('en')),
    );
    expect(find.text('Website of the server'), findsOneWidget);
    await tester.tap(find.byKey(const Key('open-website')));
    await tester.pumpAndSettle();
    expect(opened.single.toString(), 'https://pdc.example.org:8484/');
    expect(find.text('The browser could not be opened.'), findsNothing);
  });

  testWidgets('German, and a browser that cannot be opened says so', (tester) async {
    tester.view.physicalSize = const Size(900, 2400);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(_page((_) async => false, const Locale('de')));
    expect(find.text('Website des Servers'), findsOneWidget);
    await tester.tap(find.byKey(const Key('open-website')));
    await tester.pumpAndSettle();
    expect(find.text('Der Browser konnte nicht geöffnet werden.'), findsOneWidget);
  });
}
