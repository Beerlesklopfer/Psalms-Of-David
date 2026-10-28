// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/models.dart';
import 'package:psalm/widgets.dart';

/// A 1x1 PNG.
final Uint8List _png = base64Decode(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==',
);

Book _book({Uint8List? cover}) => Book(
      id: '1',
      title: 'Kinder der Freiheit',
      author: 'Ken Follett',
      color: const Color(0xFF6E1F2B),
      tracks: const [Track(disc: 1, number: 1, title: 'T', length: Duration(minutes: 1))],
      cover: cover,
    );

Widget _host(Widget child) => MaterialApp(home: Scaffold(body: Center(child: SizedBox.square(dimension: 200, child: child))));

void main() {
  testWidgets('a book with a product picture shows the picture', (tester) async {
    await tester.pumpWidget(_host(Cover(book: _book(cover: _png))));
    expect(find.byType(Image), findsOneWidget);
    expect(find.text('Kinder der Freiheit'), findsNothing, reason: 'the title is on the picture, not drawn over it');
  });

  testWidgets('a book without one gets the drawn cover with its title', (tester) async {
    await tester.pumpWidget(_host(Cover(book: _book())));
    expect(find.byType(Image), findsNothing);
    expect(find.text('Kinder der Freiheit'), findsOneWidget);
  });

  testWidgets('bytes that are no picture fall back to the drawn cover', (tester) async {
    await tester.pumpWidget(_host(Cover(book: _book(cover: Uint8List.fromList([1, 2, 3])))));
    await tester.pump();
    expect(find.text('Kinder der Freiheit'), findsOneWidget);
  });
}
