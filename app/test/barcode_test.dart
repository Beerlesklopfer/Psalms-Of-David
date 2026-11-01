// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/barcode.dart';
import 'package:psalm/models.dart';
import 'package:psalm/offline/library_cache.dart';
import 'package:psalm/server/server_source.dart';

LibraryItem _book(String id, {String? barcode}) => LibraryItem(
  id: id,
  title: 'T$id',
  author: 'A',
  color: const Color(0xFF000000),
  barcode: barcode,
  tracks: [Track(disc: 1, number: 1, title: 'x', length: const Duration(minutes: 1), partId: int.parse(id))],
);

void main() {
  test('cleanBarcode keeps only the digits', () {
    expect(cleanBarcode('4006381 333931'), '4006381333931');
    expect(cleanBarcode('4-006381-333931\n'), '4006381333931');
    expect(cleanBarcode('abc'), '');
  });

  test('known-good codes are valid', () {
    expect(isValidGtin('4006381333931'), isTrue); // EAN-13
    expect(isValidGtin('036000291452'), isTrue); // UPC-A
    expect(isValidGtin('96385074'), isTrue); // EAN-8
    expect(isValidGtin('04006381333931'), isTrue); // GTIN-14
  });

  test('bad codes are not valid', () {
    expect(isValidGtin('4006381333932'), isFalse); // wrong check digit
    expect(isValidGtin('036000291453'), isFalse);
    expect(isValidGtin('96385075'), isFalse);
    expect(isValidGtin(''), isFalse);
    expect(isValidGtin('123456789'), isFalse); // 9 digits
    expect(isValidGtin('40063813339a1'), isFalse);
    expect(isValidGtin('4006381 333931'), isFalse); // clean it first
  });

  test('toGtin14 pads on the left', () {
    expect(toGtin14('96385074'), '00000096385074');
    expect(toGtin14('036000291452'), '00036000291452');
    expect(toGtin14('0036000291452'), toGtin14('036000291452'));
  });

  test('a UPC-E expands to its UPC-A', () {
    expect(expandUpcE('12345678'), isNotNull);
    expect(expandUpcE('4006381'), isNull);
    expect(expandUpcE('21234565'), isNull);
    final wide = expandUpcE('04252614')!; // a real UPC-E: 0 425261 4 -> 042100005264
    expect(wide, '042100005264');
    expect(isValidGtin(wide), isTrue);
    expect(scannedGtin('04252614', upcE: true), '042100005264');
  });

  test('scannedGtin accepts only valid codes', () {
    expect(scannedGtin('4006381333931'), '4006381333931');
    expect(scannedGtin('4006381333932'), isNull);
    expect(scannedGtin('https://example.org'), isNull);
  });

  test('books are matched by GTIN-14, so UPC-A and EAN-13 with a leading 0 meet', () {
    final books = [_book('1', barcode: '0036000291452'), _book('2', barcode: '4006381333931'), _book('3'), _book('4', barcode: '')];
    expect([for (final b in booksWithBarcode(books, '036000291452')) b.id], ['1']);
    expect([for (final b in booksWithBarcode(books, '0036000291452')) b.id], ['1']);
    expect([for (final b in booksWithBarcode(books, '4006381 333931')) b.id], ['2']);
    expect(booksWithBarcode(books, '96385074'), isEmpty);
  });

  test('the answer of the server is read, invalid and found', () {
    final a = BarcodeAnswer.fromJson({
      'code': '4006381333931',
      'gtin': '04006381333931',
      'found': true,
      'items': [
        {'item_id': 7, 'kind': 'music', 'title': 'Album', 'creator': 'Band', 'has_cover': true},
      ],
    });
    expect(a.found, isTrue);
    expect(a.items.single.itemId, 7);
    expect(a.items.single.kind, 'music');
    expect(a.items.single.hasCover, isTrue);
    expect(BarcodeAnswer.invalid('1').invalid, isTrue);
    expect(BarcodeAnswer.fromJson({'code': '1', 'found': false, 'items': []}).found, isFalse);
  });

  test('the barcode survives the offline cache, and an old cache without it still loads', () {
    final json = bookToJson(_book('5', barcode: '4006381333931'));
    expect(bookFromJson(json).barcode, '4006381333931');
    json.remove('barcode');
    expect(bookFromJson(json).barcode, isNull);
    final lib = LoadedLibrary([_book('5', barcode: '96385074')], const {});
    final back = libraryFromJson(libraryToJson(lib), (_) => null);
    expect(back.books.single.barcode, '96385074');
  });
}
