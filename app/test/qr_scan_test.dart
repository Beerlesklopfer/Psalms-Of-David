// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_scanner/mobile_scanner.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/screens/connect_screen.dart';
import 'package:psalm/screens/qr_scan_screen.dart';

const _pairing = 'david://pair?v=1&id=abc&addr=127.0.0.1%3A8484&code=0123456789abcdef';

BarcodeCapture _capture(List<String?> texts) => BarcodeCapture(barcodes: [for (final t in texts) Barcode(rawValue: t, format: BarcodeFormat.qrCode)]);

Widget _app(Widget home) => MaterialApp(
  localizationsDelegates: AppLocalizations.localizationsDelegates,
  supportedLocales: AppLocalizations.supportedLocales,
  locale: const Locale('en'),
  home: home,
);

/// A stand-in for the camera: the test calls [detect] or shows [error].
class _FakeCamera {
  void Function(BarcodeCapture)? detect;
  MobileScannerException? error;

  Widget build(BuildContext context, MobileScannerController c, void Function(BarcodeCapture) onDetect, Widget Function(BuildContext, MobileScannerException) errorBuilder) {
    detect = onDetect;
    final e = error;
    return e == null ? const ColoredBox(color: Colors.black) : errorBuilder(context, e);
  }
}

/// Opens the scanner from a button and keeps what it returned.
class _Host extends StatefulWidget {
  const _Host(this.camera);
  final _FakeCamera camera;

  @override
  State<_Host> createState() => _HostState();
}

class _HostState extends State<_Host> {
  Object? result = 'not closed';

  @override
  Widget build(BuildContext context) => Scaffold(
    body: Column(
      children: [
        TextButton(
          key: const ValueKey('open'),
          onPressed: () async {
            final r = await Navigator.of(context).push<QrScanOutcome>(MaterialPageRoute(builder: (_) => QrScanScreen(viewBuilder: widget.camera.build)));
            setState(() => result = r == null ? 'cancelled' : (r.text ?? 'picture'));
          },
          child: const Text('open'),
        ),
        Text('result: $result', key: const ValueKey('result')),
      ],
    ),
  );
}

String _result(WidgetTester tester) => (tester.widget<Text>(find.byKey(const ValueKey('result'), skipOffstage: false)).data ?? '').substring('result: '.length);

void main() {
  test('only a pairing address counts, the first one, trimmed', () {
    expect(firstPairingAddress([null, 'https://example.org', '  $_pairing \n', 'david://pair?v=1&code=2']), _pairing);
    expect(firstPairingAddress(['https://example.org', 'david://other', '']), isNull);
  });

  group('the scanner', () {
    testWidgets('a pairing code closes it with the text, once', (tester) async {
      final camera = _FakeCamera();
      await tester.pumpWidget(_app(_Host(camera)));
      await tester.tap(find.byKey(const ValueKey('open')));
      await tester.pumpAndSettle();
      camera.detect!(_capture([_pairing]));
      // A second detection while the screen closes changes nothing.
      camera.detect!(_capture(['david://pair?v=1&code=other']));
      await tester.pumpAndSettle();
      expect(_result(tester), _pairing);
    });

    testWidgets('another QR code is ignored with one hint, and the scanning goes on', (tester) async {
      final camera = _FakeCamera();
      await tester.pumpWidget(_app(_Host(camera)));
      await tester.tap(find.byKey(const ValueKey('open')));
      await tester.pumpAndSettle();
      camera.detect!(_capture(['https://example.org']));
      await tester.pump();
      expect(find.byKey(const ValueKey('qr-not-pairing')), findsOneWidget);
      expect(find.text('This is not a pairing code.'), findsOneWidget);
      // The hint goes away by itself and is not shown a second time.
      await tester.pump(const Duration(seconds: 5));
      expect(find.byKey(const ValueKey('qr-not-pairing')), findsNothing);
      camera.detect!(_capture(['https://example.org']));
      await tester.pump();
      expect(find.byKey(const ValueKey('qr-not-pairing')), findsNothing);
      // ... and a pairing code is still accepted afterwards.
      camera.detect!(_capture([_pairing]));
      await tester.pumpAndSettle();
      expect(_result(tester), _pairing);
    });

    testWidgets('cancel closes it without a result', (tester) async {
      final camera = _FakeCamera();
      await tester.pumpWidget(_app(_Host(camera)));
      await tester.tap(find.byKey(const ValueKey('open')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('qr-cancel')));
      await tester.pumpAndSettle();
      expect(_result(tester), 'cancelled');
    });

    testWidgets('"choose a picture instead" hands over to the picture chooser', (tester) async {
      final camera = _FakeCamera();
      await tester.pumpWidget(_app(_Host(camera)));
      await tester.tap(find.byKey(const ValueKey('open')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('qr-use-picture')));
      await tester.pumpAndSettle();
      expect(_result(tester), 'picture');
    });

    testWidgets('no camera: the picture chooser takes over by itself', (tester) async {
      final camera = _FakeCamera()..error = const MobileScannerException(errorCode: MobileScannerErrorCode.unsupported);
      await tester.pumpWidget(_app(_Host(camera)));
      await tester.tap(find.byKey(const ValueKey('open')));
      await tester.pumpAndSettle();
      expect(_result(tester), 'picture');
    });

    testWidgets('a camera that is not allowed explains it and offers the picture', (tester) async {
      final camera = _FakeCamera()..error = const MobileScannerException(errorCode: MobileScannerErrorCode.permissionDenied);
      await tester.pumpWidget(_app(_Host(camera)));
      await tester.tap(find.byKey(const ValueKey('open')));
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('qr-permission-denied')), findsOneWidget);
      expect(_result(tester), 'not closed');
      await tester.tap(find.byKey(const ValueKey('qr-use-picture-denied')));
      await tester.pumpAndSettle();
      expect(_result(tester), 'picture');
    });
  });

  group('the connect screen', () {
    Widget screen({required Future<QrScanOutcome?> Function(BuildContext) scan, Future<Uint8List?> Function(String)? pick, List<String>? pairs}) => _app(
      ConnectScreen(
        deviceName: 'iPhone',
        onPair: (address, name) async {
          pairs?.add(address);
          return null;
        },
        onDemo: () {},
        scanQr: scan,
        pickPicture: pick ?? (title) async => null,
      ),
    );

    String field(WidgetTester tester) => tester.widget<TextField>(find.byType(TextField).first).controller!.text;

    testWidgets('a scanned address is put into the field, nothing connects by itself', (tester) async {
      final pairs = <String>[];
      await tester.pumpWidget(screen(scan: (_) async => const QrScanOutcome.text(_pairing), pairs: pairs));
      await tester.tap(find.byIcon(Icons.qr_code_scanner));
      await tester.pumpAndSettle();
      expect(field(tester), _pairing);
      expect(pairs, isEmpty);
    });

    testWidgets('cancelling the scanner leaves the field as it was', (tester) async {
      await tester.pumpWidget(screen(scan: (_) async => null));
      await tester.enterText(find.byType(TextField).first, 'something typed');
      await tester.tap(find.byIcon(Icons.qr_code_scanner));
      await tester.pumpAndSettle();
      expect(field(tester), 'something typed');
    });

    testWidgets('without a camera the picture chooser opens', (tester) async {
      var asked = 0;
      await tester.pumpWidget(screen(scan: (_) async => const QrScanOutcome.picture(), pick: (title) async {
        asked++;
        return null;
      }));
      await tester.tap(find.byIcon(Icons.qr_code_scanner));
      await tester.pumpAndSettle();
      expect(asked, 1);
    });
  });
}
