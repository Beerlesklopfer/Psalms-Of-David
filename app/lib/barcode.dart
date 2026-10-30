// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'models.dart';

/// Only the digits of [text]: a code pasted with spaces or hyphens ("4006381 333931", "4-006381-333931") becomes plain digits.
String cleanBarcode(String text) => text.replaceAll(RegExp(r'[^0-9]'), '');

/// True for a GTIN (EAN-8, UPC-A, EAN-13, GTIN-14): only digits, 8, 12, 13 or 14 of them, and the check digit is right.
bool isValidGtin(String code) {
  if (!const {8, 12, 13, 14}.contains(code.length) ||
      !RegExp(r'^[0-9]+$').hasMatch(code)) {
    return false;
  }
  var sum = 0;
  // From the right, the digit before the check digit counts three times, the next one once, and so on.
  for (var i = code.length - 2, weight = 3; i >= 0; i--, weight = 4 - weight) {
    sum += (code.codeUnitAt(i) - 0x30) * weight;
  }
  return (10 - sum % 10) % 10 == code.codeUnitAt(code.length - 1) - 0x30;
}

/// The code padded with zeros on the left to 14 digits, the common form: a UPC-A and the EAN-13 with a leading 0 are the same product.
String toGtin14(String code) => code.padLeft(14, '0');

/// The 12 digits of the UPC-A that a UPC-E (8 digits, number system 0 or 1) stands for; null if [upcE] is no UPC-E.
String? expandUpcE(String upcE) {
  if (upcE.length != 8 || !RegExp(r'^[01][0-9]{7}$').hasMatch(upcE)) {
    return null;
  }
  final d = upcE;
  final last = d[6];
  final String body;
  switch (last) {
    case '0' || '1' || '2':
      body = '${d.substring(0, 3)}${last}0000${d.substring(3, 6)}';
    case '3':
      body = '${d.substring(0, 4)}00000${d.substring(4, 6)}';
    case '4':
      body = '${d.substring(0, 5)}00000${d[5]}';
    default:
      body = '${d.substring(0, 6)}0000$last';
  }
  return '$body${d[7]}';
}

/// What a scan gives: the valid GTIN in [raw] (digits only), or null if it is none. A code that the camera read as UPC-E ([upcE]) is
/// expanded to its UPC-A when it is no valid GTIN-8 by itself.
String? scannedGtin(String raw, {bool upcE = false}) {
  final code = cleanBarcode(raw);
  if (upcE) {
    final wide = expandUpcE(code);
    if (wide != null && isValidGtin(wide)) return wide;
  }
  return isValidGtin(code) ? code : null;
}

/// The books whose barcode is the same product as [code] (compared as GTIN-14, so UPC-A and EAN-13 with a leading 0 match).
List<Book> booksWithBarcode(Iterable<Book> books, String code) {
  final wanted = toGtin14(cleanBarcode(code));
  return [
    for (final b in books)
      if (b.barcode != null &&
          b.barcode!.isNotEmpty &&
          toGtin14(cleanBarcode(b.barcode!)) == wanted)
        b,
  ];
}

/// One album of the library that the server found for a barcode.
class BarcodeItem {
  const BarcodeItem({
    required this.itemId,
    required this.kind,
    required this.title,
    required this.creator,
    required this.hasCover,
  });

  factory BarcodeItem.fromJson(Map<String, dynamic> j) => BarcodeItem(
    itemId: j['item_id'] as int,
    kind: j['kind'] as String? ?? 'audiobook',
    title: j['title'] as String? ?? '',
    creator: j['creator'] as String? ?? '',
    hasCover: j['has_cover'] == true,
  );

  final int itemId;
  final String kind;
  final String title;
  final String creator;
  final bool hasCover;
}

/// The answer of `GET /api/v1/barcode/{code}`. [invalid] is the server's `422 invalid_barcode`: then there are no items.
class BarcodeAnswer {
  const BarcodeAnswer({
    required this.code,
    this.gtin,
    this.found = false,
    this.items = const [],
    this.invalid = false,
  });

  const BarcodeAnswer.invalid(String code) : this(code: code, invalid: true);

  factory BarcodeAnswer.fromJson(Map<String, dynamic> j) {
    final items = [
      for (final i in (j['items'] as List? ?? const []))
        BarcodeItem.fromJson(i as Map<String, dynamic>),
    ];
    return BarcodeAnswer(
      code: '${j['code']}',
      gtin: j['gtin'] as String?,
      found: j['found'] == true && items.isNotEmpty,
      items: items,
    );
  }

  final String code;
  final String? gtin;
  final bool found;
  final bool invalid;
  final List<BarcodeItem> items;
}

/// Asks the server about a barcode ([ApiClient] is the real one; the screens take it as a function so that a test can put a fake in place).
typedef BarcodeLookup = Future<BarcodeAnswer> Function(String code);

/// What the screens need to ask the server for a barcode; [ApiClient] implements it next to [FeedApi].
abstract class BarcodeApi {
  Future<BarcodeAnswer> barcodeLookup(String code);
}
