// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:typed_data';

import 'package:image/image.dart' as img;
import 'package:zxing2/qrcode.dart';

/// Reads the text of the first QR code in a picture (PNG, JPEG, WebP, GIF), or null if
/// there is none to be found. Used on the computer, where the pairing code of the
/// administration is on the screen: a screenshot of it is enough. The phones will scan
/// with the camera instead.
String? readQrCode(Uint8List pictureBytes) {
  final decoded = img.decodeImage(pictureBytes);
  if (decoded == null) return null;
  // A large picture is read as it is; the QR code may be a small part of it.
  final rgba = decoded.convert(numChannels: 4);
  final pixels = rgba.getBytes(order: img.ChannelOrder.abgr).buffer.asInt32List();
  final source = RGBLuminanceSource(rgba.width, rgba.height, pixels);
  final hints = DecodeHints()..put(DecodeHintType.tryHarder);
  for (final bitmap in [
    BinaryBitmap(HybridBinarizer(source)),
    BinaryBitmap(GlobalHistogramBinarizer(source)),
  ]) {
    try {
      return QRCodeReader().decode(bitmap, hints: hints).text;
    } on ReaderException {
      // Not found this way; try the other way of telling black from white.
    }
  }
  return null;
}
