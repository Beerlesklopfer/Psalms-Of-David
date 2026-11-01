// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:ui' show Color;

import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/models.dart';
import 'package:psalm/voice/voice_match.dart';

LibraryItem book(
  String id,
  String title,
  String author, {
  List<String> labels = const [],
  String? narrator,
}) => LibraryItem(
  id: id,
  title: title,
  author: author,
  color: const Color(0xFF000000),
  tracks: const [],
  labels: labels,
  narrator: narrator,
);

void main() {
  final books = [
    book('1', 'Frohe Weihnachten', 'Familie Meier', labels: ['Advent']),
    book('2', 'Weihnachten im Stall', 'Anna Schmidt'),
    book('3', 'Märchen der Brüder Grimm', 'Grimm', labels: ['Schule', 'Gebet']),
    book('4', 'Die Bibel für Kinder', 'Weihnachten Verlag'),
  ];

  test('finds by title, ignoring case', () {
    expect(bestMatch(books, 'FROHE weihnachten')!.id, '1');
  });

  test('folds umlauts and sharp s', () {
    expect(bestMatch(books, 'Marchen der Bruder')!.id, '3');
    expect(foldForSearch('Straße Ärger'), 'strasse arger');
  });

  test('all words must match', () {
    expect(bestMatch(books, 'Frohe Ostern'), isNull);
    expect(bestMatch(books, 'Weihnachten Stall')!.id, '2');
  });

  test('title matches rank before author and label matches', () {
    final ranked = matchBooks(books, 'Weihnachten').map((b) => b.id).toList();
    expect(ranked.take(2), ['1', '2']);
    expect(ranked.last, '4');
  });

  test('exact title wins', () {
    expect(bestMatch(books, 'Weihnachten im Stall')!.id, '2');
  });

  test('finds by author and by label', () {
    expect(bestMatch(books, 'Anna Schmidt')!.id, '2');
    expect(bestMatch(books, 'Gebet')!.id, '3');
    expect(bestMatch(books, 'Advent')!.id, '1');
  });

  test('narrator counts as well', () {
    final withNarrator = [
      book('9', 'Hiob', 'Bibel', narrator: 'Gerd Westphal'),
    ];
    expect(bestMatch(withNarrator, 'Westphal')!.id, '9');
  });

  test('no match and empty query', () {
    expect(bestMatch(books, 'Raumschiff'), isNull);
    expect(bestMatch(books, '   '), isNull);
    expect(bestMatch(const [], 'x'), isNull);
  });
}
