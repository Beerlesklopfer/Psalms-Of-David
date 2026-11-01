// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_scanner/mobile_scanner.dart';
import 'package:psalm/barcode.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/models.dart';
import 'package:psalm/offline/offline_controller.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/screens/barcode_scan_screen.dart';
import 'package:psalm/server/api_client.dart';

import 'barcode_api_test.dart' show barcodeServer;

LibraryItem _book(String id, {String? barcode}) => LibraryItem(
  id: id,
  title: 'Title $id',
  author: 'Artist $id',
  color: const Color(0xFF000000),
  kind: 'music',
  barcode: barcode,
  tracks: [Track(disc: 1, number: 1, title: 'x', length: const Duration(minutes: 1), partId: int.parse(id))],
);

Widget _app(PlayerController p, Widget home) => PlayerScope(
  controller: p,
  child: MaterialApp(
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    locale: const Locale('en'),
    home: home,
  ),
);

Future<void> _check(WidgetTester t, String text) async {
  await t.enterText(find.byKey(const Key('barcode-field')), text);
  await t.tap(find.byKey(const Key('barcode-check')));
  await t.pumpAndSettle();
}

void main() {
  late PlayerController player;
  var asked = <String>[];
  BarcodeAnswer answer = const BarcodeAnswer(code: '', found: false);

  Future<BarcodeAnswer> lookup(String code) async {
    asked.add(code);
    return answer;
  }

  setUp(() {
    asked = [];
    answer = const BarcodeAnswer(code: '', found: false);
    player = PlayerController([_book('1', barcode: '4006381333931'), _book('2')]);
  });

  Widget screen({BarcodeLookup? l, List<LibraryItem>? opened}) => _app(
    player,
    BarcodeScanScreen(lookup: l ?? lookup, useCamera: false, opener: (c, b) => opened?.add(b)),
  );

  testWidgets('found locally: the book is listed with an Open button, the server is not asked', (t) async {
    final opened = <LibraryItem>[];
    await t.pumpWidget(screen(opened: opened));
    await _check(t, '4006381 333931');
    expect(find.byKey(const Key('barcode-in-library')), findsOneWidget);
    expect(find.text('Title 1'), findsWidgets);
    expect(find.textContaining('Artist 1'), findsOneWidget);
    expect(asked, isEmpty);
    await t.tap(find.byKey(const Key('barcode-open-1')));
    await t.pumpAndSettle();
    expect(opened.single.id, '1');
  });

  testWidgets('found only on the server: listed, and openable when the book is in the library', (t) async {
    answer = const BarcodeAnswer(
      code: '96385074',
      found: true,
      items: [BarcodeItem(itemId: 2, kind: 'music', title: 'Server album', creator: 'Server band', hasCover: true), BarcodeItem(itemId: 99, kind: 'audiobook', title: 'Unknown here', creator: '', hasCover: false)],
    );
    await t.pumpWidget(screen());
    await _check(t, '96385074');
    expect(asked, ['96385074']);
    expect(find.text('Server album'), findsOneWidget);
    expect(find.text('Unknown here'), findsOneWidget);
    expect(find.byKey(const Key('barcode-open-2')), findsOneWidget);
    expect(find.byKey(const Key('barcode-open-99')), findsNothing);
  });

  testWidgets('not found: says so with the code', (t) async {
    await t.pumpWidget(screen());
    await _check(t, '96385074');
    expect(find.byKey(const Key('barcode-not-in-library')), findsOneWidget);
    expect(find.text('Not in the library: 96385074'), findsOneWidget);
    expect(find.byKey(const Key('barcode-note')), findsNothing);
  });

  testWidgets('an invalid code is refused on the spot, nothing is asked', (t) async {
    await t.pumpWidget(screen());
    await _check(t, '4006381333932');
    expect(find.text('This is not a valid barcode. Check the digits.'), findsOneWidget);
    expect(find.byKey(const Key('barcode-result')), findsNothing);
    expect(asked, isEmpty);
  });

  testWidgets('offline: only the local library answers and the screen says the server was not asked', (t) async {
    player.offline = OfflineController(manual: true);
    await t.pumpWidget(screen());
    await _check(t, '96385074');
    expect(asked, isEmpty);
    expect(find.byKey(const Key('barcode-not-in-library')), findsOneWidget);
    expect(find.textContaining('server was not asked'), findsOneWidget);
    // A code that the device has is still found offline.
    await t.tapAt(const Offset(5, 5));
    await t.pumpAndSettle();
    await _check(t, '4006381333931');
    expect(find.byKey(const Key('barcode-in-library')), findsOneWidget);
  });

  testWidgets('a failing server gives a hint, not a crash', (t) async {
    await t.pumpWidget(screen(l: (_) async => throw ApiException(500, 'boom')));
    await _check(t, '96385074');
    expect(find.textContaining('could not be asked'), findsOneWidget);
  });

  testWidgets('the real client against the stand-in server finds an album that the device lacks', (t) async {
    player = PlayerController([_book('1'), _book('2')]);
    // The test binding answers every request with 400; this test talks to a real socket on the loopback.
    HttpOverrides.global = null;
    await t.runAsync(() async {
      final (server, api, seen) = await barcodeServer();
      addTearDown(() => server.close(force: true));
      await t.pumpWidget(screen(l: api.barcodeLookup));
      await t.enterText(find.byKey(const Key('barcode-field')), '4006381333931');
      await t.tap(find.byKey(const Key('barcode-check')));
      for (var i = 0; i < 50 && seen.isEmpty; i++) {
        await Future<void>.delayed(const Duration(milliseconds: 20));
      }
      await Future<void>.delayed(const Duration(milliseconds: 100));
      await t.pumpAndSettle();
      expect(seen.single.path, '/api/v1/barcode/4006381333931');
    });
    expect(find.byKey(const Key('barcode-in-library')), findsOneWidget);
    expect(find.text('Album'), findsOneWidget);
    expect(find.byKey(const Key('barcode-open-7')), findsNothing);
  });

  testWidgets('camera: the first valid code stops the scan and is checked; invalid codes are ignored', (t) async {
    final controller = MobileScannerController(autoStart: false, formats: const [BarcodeFormat.ean13]);
    addTearDown(controller.dispose);
    void Function(BarcodeCapture)? detect;
    await t.pumpWidget(
      _app(
        player,
        BarcodeScanScreen(
          lookup: lookup,
          useCamera: true,
          controller: controller,
          viewBuilder: (c, ctl, onDetect, err) {
            detect = onDetect;
            return const ColoredBox(color: Colors.black);
          },
        ),
      ),
    );
    expect(find.byKey(const Key('barcode-scan-hint')), findsOneWidget);
    detect!(const BarcodeCapture(barcodes: [Barcode(rawValue: '4006381333932', format: BarcodeFormat.ean13)]));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('barcode-result')), findsNothing);
    detect!(const BarcodeCapture(barcodes: [Barcode(rawValue: 'hello'), Barcode(rawValue: '4006381333931', format: BarcodeFormat.ean13)]));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('barcode-in-library')), findsOneWidget);
  });
}
