// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/screens/connect_screen.dart';

const _address = 'david://pair?v=1&id=abc&addr=127.0.0.1%3A8484&code=0123456789abcdef';

Widget _screen({String? initial}) => MaterialApp(
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      supportedLocales: AppLocalizations.supportedLocales,
      locale: const Locale('en'),
      home: ConnectScreen(
        deviceName: 'Laptop',
        initialAddress: initial,
        onPair: (address, name) async => null,
        onDemo: () {},
      ),
    );

/// Makes the clipboard of the test hold [text].
void _clipboard(WidgetTester tester, String? text) {
  tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, (call) async {
    if (call.method == 'Clipboard.getData') return text == null ? null : <String, dynamic>{'text': text};
    return null;
  });
  addTearDown(() => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, null));
}

String _field(WidgetTester tester) => tester.widget<TextField>(find.byType(TextField).first).controller!.text;

void main() {
  testWidgets('the paste button puts the clipboard into the address field', (tester) async {
    _clipboard(tester, '  $_address \n');
    await tester.pumpWidget(_screen());
    await tester.pumpAndSettle();
    // A copied pairing address is taken over at once ...
    expect(_field(tester), _address);
    // ... and the button works for a text copied later.
    _clipboard(tester, 'david://pair?v=1&addr=other%3A1&code=zz');
    await tester.tap(find.byIcon(Icons.content_paste));
    await tester.pumpAndSettle();
    expect(_field(tester), 'david://pair?v=1&addr=other%3A1&code=zz');
  });

  testWidgets('something else on the clipboard is not taken over on its own', (tester) async {
    _clipboard(tester, 'my shopping list');
    await tester.pumpWidget(_screen());
    await tester.pumpAndSettle();
    expect(_field(tester), '');
  });

  testWidgets('an address from the command line is already in the field', (tester) async {
    _clipboard(tester, null);
    await tester.pumpWidget(_screen(initial: _address));
    await tester.pumpAndSettle();
    expect(_field(tester), _address);
  });
}
