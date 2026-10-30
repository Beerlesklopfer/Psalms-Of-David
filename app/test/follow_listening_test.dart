// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player_controller.dart';

Book _book(String id) => Book(
      id: id,
      title: 'Book $id',
      author: 'A',
      color: const Color(0xFF000000),
      tracks: [for (var i = 0; i < 3; i++) Track(disc: 1, number: i + 1, title: 'T${i + 1}', length: const Duration(minutes: 5), partId: int.parse(id) * 10 + i)],
    );

Map<String, dynamic> _entry(int item, int part, int ms, int at) => {'item_id': item, 'part_id': part, 'position_ms': ms, 'updated_at': at};

void main() {
  final future = DateTime.now().millisecondsSinceEpoch + 60000;

  test('another device went on with a later track: the player shows that place, not the old one', () {
    final p = PlayerController([_book('1'), _book('2')]);
    p.book = p.library[0];
    p.followServerListening([_entry(2, 21, 90000, future)]);
    expect(p.book?.id, '2');
    expect(p.trackIndex, 1);
    expect(p.position, const Duration(seconds: 90));
    p.dispose();
  });

  test('an older entry changes nothing on the screen but is remembered as the place of that book', () {
    final p = PlayerController([_book('1'), _book('2')]);
    p.book = p.library[0];
    p.open(p.library[0]); // this device just did something
    p.toggle();
    p.followServerListening([_entry(2, 21, 90000, 1000)]);
    expect(p.book?.id, '1');
    p.open(p.library[1]);
    expect(p.trackIndex, 1, reason: 'the place of the other device is where the book opens');
    p.dispose();
  });

  test('while something plays here the other device does not take the player away', () {
    final p = PlayerController([_book('1'), _book('2')]);
    p.open(p.library[0]);
    expect(p.playing, isTrue);
    p.followServerListening([_entry(2, 21, 90000, future)]);
    expect(p.book?.id, '1');
    p.dispose();
  });
}
