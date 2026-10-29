// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:image/image.dart' as img;
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/screens/connect_screen.dart';
import 'package:psalm/server/qr_reader.dart';
import 'package:zxing2/qrcode.dart';

const _address =
    'david://pair?v=1&id=1ba36f04-1652-4962-8596-4eb7c0d23cea&addr=192.168.24.127%3A8484&fp=e74d9b5ec1ff8e0a&code=0123456789abcdef0123456789abcdef';

/// A picture of the QR code of [text]: [scale] pixels per module, a quiet zone around it,
/// on a [canvas] of white that is [canvas] pixels wide (a screenshot with the code somewhere in it).
Uint8List _qrPicture(String text, {int scale = 6, int canvas = 0}) {
  final qr = Encoder.encode(text, ErrorCorrectionLevel.m);
  final m = qr.matrix!;
  final side = (m.width + 8) * scale;
  final size = canvas > side ? canvas : side;
  final image = img.Image(width: size, height: size);
  img.fill(image, color: img.ColorRgb8(255, 255, 255));
  final offset = canvas > side ? (canvas - side) ~/ 3 : 0;
  for (var y = 0; y < m.height; y++) {
    for (var x = 0; x < m.width; x++) {
      if (m.get(x, y) == 1) {
        img.fillRect(
          image,
          x1: offset + (x + 4) * scale,
          y1: offset + (y + 4) * scale,
          x2: offset + (x + 5) * scale - 1,
          y2: offset + (y + 5) * scale - 1,
          color: img.ColorRgb8(0, 0, 0),
        );
      }
    }
  }
  return Uint8List.fromList(img.encodePng(image));
}

void main() {
  test('a QR code in a picture is read back exactly', () {
    expect(readQrCode(_qrPicture(_address)), _address);
    expect(readQrCode(_qrPicture(_address, scale: 3)), _address, reason: 'small modules');
  });

  test('a QR code somewhere inside a large screenshot is found', () {
    expect(readQrCode(_qrPicture(_address, scale: 4, canvas: 1600)), _address);
  });

  test('pictures without a QR code, and files that are no pictures, give null', () {
    final blank = img.Image(width: 300, height: 300);
    img.fill(blank, color: img.ColorRgb8(240, 240, 240));
    expect(readQrCode(Uint8List.fromList(img.encodePng(blank))), isNull);
    expect(readQrCode(Uint8List.fromList([1, 2, 3, 4])), isNull);
    expect(readQrCode(Uint8List(0)), isNull);
  });

  // The picture that the real server's pairing page gives (see the comment of server_e2e_test).
  final real = Platform.environment['PSALM_TEST_QR_PICTURE'];
  test(
    'the QR code of a real pairing page is read',
    skip: real == null ? 'set PSALM_TEST_QR_PICTURE to a screenshot of the pairing page' : null,
    () {
      final text = readQrCode(File(real!).readAsBytesSync());
      expect(text, isNotNull);
      expect(text, startsWith('david://pair?v=1'));
    },
  );

  Widget screen(Future<Uint8List?> Function(String) pick) => MaterialApp(
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    locale: const Locale('en'),
    home: ConnectScreen(
      deviceName: 'Laptop',
      pickPicture: pick,
      onPair: (a, n) async => null,
      onDemo: () {},
    ),
  );

  testWidgets('the QR button reads the chosen picture into the address field', (tester) async {
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
      SystemChannels.platform,
      (call) async => null,
    );
    addTearDown(
      () => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        SystemChannels.platform,
        null,
      ),
    );
    String? title;
    await tester.pumpWidget(
      screen((t) async {
        title = t;
        return _qrPicture(_address);
      }),
    );
    await tester.tap(find.byIcon(Icons.qr_code_scanner));
    await tester.pumpAndSettle();
    expect(title, 'Choose a picture of the QR code');
    expect(tester.widget<TextField>(find.byType(TextField).first).controller!.text, _address);
  });

  testWidgets('a picture without a code says so; cancelling changes nothing', (tester) async {
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
      SystemChannels.platform,
      (call) async => null,
    );
    addTearDown(
      () => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        SystemChannels.platform,
        null,
      ),
    );
    final blank = img.Image(width: 200, height: 200);
    img.fill(blank, color: img.ColorRgb8(250, 250, 250));
    await tester.pumpWidget(screen((t) async => Uint8List.fromList(img.encodePng(blank))));
    await tester.tap(find.byIcon(Icons.qr_code_scanner));
    await tester.pumpAndSettle();
    expect(
      find.text('No QR code found in this picture. Try a sharper or larger one.'),
      findsOneWidget,
    );

    await tester.pumpWidget(screen((t) async => null));
    await tester.pumpAndSettle();
    await tester.tap(find.byIcon(Icons.qr_code_scanner));
    await tester.pumpAndSettle();
    expect(tester.widget<TextField>(find.byType(TextField).first).controller!.text, '');
  });
}
