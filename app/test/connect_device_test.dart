// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/screens/connect_device.dart';
import 'package:psalm/screens/settings_screen.dart';
import 'package:psalm/server/api_client.dart';
import 'package:psalm/server/credentials.dart';
import 'package:psalm/server/device_pairing.dart';

class _FakeApi implements DevicePairingApi {
  _FakeApi({
    this.used = 1,
    this.max = 2,
    this.usageError,
    this.codeError,
  });

  int used;
  int? max;
  ApiException? usageError;
  ApiException? codeError;
  final Duration expiresIn = const Duration(seconds: 5);
  int made = 0;

  @override
  Future<PairingUsage> usage() async {
    if (usageError != null) throw usageError!;
    return PairingUsage(used: used, max: max, full: max != null && used >= max!);
  }

  @override
  Future<PairingCode> makeCode() async {
    if (codeError != null) throw codeError!;
    made++;
    return PairingCode(
      code: 'code$made',
      serverId: 'srv-1',
      expiresIn: expiresIn,
      used: used,
      max: max,
    );
  }
}

final _credentials = Credentials(
  serverId: 'srv-1',
  address: 'pdc.example.org:8484',
  fingerprint: null,
  token: 't',
  deviceName: 'Laptop',
  ca: true,
);

Widget _page(_FakeApi api, {Locale locale = const Locale('en')}) => MaterialApp(
  locale: locale,
  localizationsDelegates: AppLocalizations.localizationsDelegates,
  supportedLocales: AppLocalizations.supportedLocales,
  home: Scaffold(
    body: SettingsScreen(credentials: _credentials, devicePairing: api),
  ),
);

void main() {
  test('the address of the QR code has the form the server makes and reads back', () {
    final a = PairingAddress(
      serverId: 'srv-1',
      address: 'pdc.example.org:8484',
      fingerprint: null,
      code: 'abc',
      ca: true,
    );
    expect(a.toUri(), 'david://pair?v=1&id=srv-1&addr=pdc.example.org%3A8484&ca=1&code=abc');
    final back = PairingAddress.parse(a.toUri())!;
    expect(
      (back.serverId, back.address, back.fingerprint, back.code, back.ca),
      ('srv-1', 'pdc.example.org:8484', null, 'abc', true),
    );
    // A server that is checked by its fingerprint: no `ca`, the fingerprint goes in.
    final pinned = PairingAddress(
      serverId: 'x',
      address: '192.168.24.250:8484',
      fingerprint: 'ab12',
      code: 'c d',
    );
    expect(pinned.toUri(), contains('fp=ab12'));
    expect(pinned.toUri(), isNot(contains('ca=')));
    final p = PairingAddress.parse(pinned.toUri())!;
    expect((p.fingerprint, p.code, p.ca), ('ab12', 'c d', false));
  });

  testWidgets(
    'the card shows how many devices there are and a QR code with a countdown for the next one',
    (tester) async {
      final api = _FakeApi();
      // The clipboard of the test: what the app copies.
      String? copied;
      tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, (
        call,
      ) async {
        if (call.method == 'Clipboard.setData') copied = (call.arguments as Map)['text'] as String?;
        return null;
      });
      addTearDown(
        () => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
          SystemChannels.platform,
          null,
        ),
      );
      await tester.pumpWidget(_page(api));
      await tester.pumpAndSettle();
      expect(find.text('1 of 2 devices connected'), findsOneWidget);
      await tester.tap(find.byKey(const Key('connect-device')));
      await tester.pump();
      await tester.pump();
      expect(find.byKey(const Key('pair-qr')), findsOneWidget);
      // The text of the code is not shown; for a computer without a camera it can be copied, and it is the address this device uses itself.
      await tester.tap(find.byKey(const Key('pair-copy')));
      await tester.pump();
      expect(
        copied,
        'david://pair?v=1&id=srv-1&addr=pdc.example.org%3A8484&ca=1&code=code1',
        reason: 'the address this device uses, the code of the server',
      );
      expect(find.text('Valid for 0:05'), findsOneWidget);
      await tester.pump(const Duration(seconds: 2));
      expect(find.text('Valid for 0:03'), findsOneWidget);
      // It runs out: the code is greyed and says so; "New code" makes another.
      await tester.pump(const Duration(seconds: 4));
      expect(find.text('The code has expired.'), findsOneWidget);
      await tester.tap(find.byKey(const Key('pair-new')));
      await tester.pump();
      await tester.pump();
      expect(api.made, 2);
      await tester.tap(find.byKey(const Key('pair-copy')));
      await tester.pump();
      expect(copied, endsWith('code=code2'));
      await tester.tap(find.byKey(const Key('pair-close')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('pair-qr')), findsNothing);
    },
  );

  testWidgets('with as many devices as allowed there is no code, and it says so', (tester) async {
    final api = _FakeApi(used: 2, max: 2, codeError: ApiException(409, 'device_limit_reached'));
    await tester.pumpWidget(_page(api));
    await tester.pumpAndSettle();
    expect(find.text('2 of 2 devices connected'), findsOneWidget);
    await tester.tap(find.byKey(const Key('connect-device')));
    await tester.pump();
    await tester.pump();
    expect(find.byKey(const Key('pair-full')), findsOneWidget);
    expect(find.byKey(const Key('pair-qr')), findsNothing);
    expect(
      find.byKey(const Key('pair-new')),
      findsNothing,
      reason: 'another code would be refused as well',
    );
    await tester.tap(find.byKey(const Key('pair-close')));
    await tester.pumpAndSettle();
  });

  testWidgets('a child or a guest (the server says forbidden) does not see the card', (
    tester,
  ) async {
    final api = _FakeApi(usageError: ApiException(403, 'forbidden'));
    await tester.pumpWidget(_page(api));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('connect-device')), findsNothing);
    expect(find.byType(ConnectDeviceCard), findsOneWidget, reason: 'the card is there and empty');
    expect(find.text('Devices'), findsNothing);
  });

  testWidgets('without a limit the count says only how many; in German', (tester) async {
    final api = _FakeApi(used: 3, max: null);
    await tester.pumpWidget(_page(api, locale: const Locale('de')));
    await tester.pumpAndSettle();
    expect(find.text('3 Geräte angebunden'), findsOneWidget);
    expect(find.text('Weiteres Gerät anbinden'), findsWidgets);
    await tester.tap(find.byKey(const Key('connect-device')));
    await tester.pump();
    await tester.pump();
    expect(find.text('Scanne diesen Code mit Psalm auf dem anderen Gerät.'), findsOneWidget);
    await tester.tap(find.byKey(const Key('pair-close')));
    await tester.pumpAndSettle();
  });

  testWidgets('a failure that is not a refusal says the code could not be made', (tester) async {
    final api = _FakeApi(codeError: ApiException(500, 'internal'));
    await tester.pumpWidget(_page(api));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('connect-device')));
    await tester.pump();
    await tester.pump();
    expect(find.byKey(const Key('pair-error')), findsOneWidget);
    await tester.tap(find.byKey(const Key('pair-close')));
    await tester.pumpAndSettle();
  });
}
