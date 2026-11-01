// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/audio/fake_backend.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/server/last_played.dart';
import 'package:psalm/server/server_source.dart';

LibraryItem _book(String id) => LibraryItem(
  id: id,
  title: 'Book $id',
  author: 'A',
  color: const Color(0xFF000000),
  tracks: [
    for (var i = 1; i <= 4; i++)
      Track(disc: 1, number: i, title: 'T$i', length: const Duration(minutes: 5), partId: int.parse(id) * 10 + i),
  ],
);

class _Source extends MediaSource {
  @override
  Future<String> fetchPart(Track track) async => '/nowhere';

  @override
  Future<void> saveProgress(LibraryItem book, Track track, Duration position, {required bool finished}) async {}

  @override
  Future<PassageHit?> resolvePassage(String text) async => null;
}

void main() {
  late Directory dir;
  late LastPlayedStore store;
  setUp(() {
    dir = Directory.systemTemp.createTempSync('psalm-last');
    store = LastPlayedStore(File('${dir.path}/last.json'));
  });
  tearDown(() => dir.deleteSync(recursive: true));

  PlayerController make() =>
      PlayerController([_book('1'), _book('2'), _book('3')], backend: FakeBackend(), lastStore: store);

  test('what was playing is written down, and the next start opens on it', () {
    final p = make();
    p.open(p.library[1], track: 2);
    p.seek(const Duration(minutes: 3));
    p.flush();
    p.dispose();

    final again = make()..attachServer(_Source(), LoadedLibrary([_book('1'), _book('2'), _book('3')], {}));
    expect(again.book!.id, '2', reason: 'not the first book of the library');
    expect(again.trackIndex, 2);
    expect(again.position, const Duration(minutes: 3));
    expect(again.playing, isFalse, reason: 'it opens ready, it does not start by itself');
    again.dispose();
  });

  test('the server wins when another device listened later, this device when it did', () {
    store.write(LastPlayed(bookId: '2', track: 1, position: const Duration(minutes: 1), at: 1000));
    final books = [_book('1'), _book('2'), _book('3')];

    final laterElsewhere = make()
      ..attachServer(
        _Source(),
        LoadedLibrary(books, {'3': (track: 3, position: const Duration(minutes: 2))}, lastItemId: '3', lastAt: 5000),
      );
    expect(laterElsewhere.book!.id, '3');
    expect((laterElsewhere.trackIndex, laterElsewhere.position), (3, const Duration(minutes: 2)));
    laterElsewhere.dispose();

    // (closing the first player wrote its own state; put the old one back)
    store.write(LastPlayed(bookId: '2', track: 1, position: const Duration(minutes: 1), at: 1000));
    final laterHere = make()..attachServer(_Source(), LoadedLibrary(books, {}, lastItemId: '3', lastAt: 500));
    expect(laterHere.book!.id, '2');
    laterHere.dispose();
  });

  test('a book that is gone or a damaged file falls back to the first book', () {
    store.write(LastPlayed(bookId: '99', track: 0, position: Duration.zero, at: 1));
    final p = make()..attachServer(_Source(), LoadedLibrary([_book('1'), _book('2')], {}));
    expect(p.book!.id, '1');
    p.dispose();

    store.file.writeAsStringSync('not json');
    final q = make()..attachServer(_Source(), LoadedLibrary([_book('1'), _book('2')], {}));
    expect(q.book!.id, '1');
    q.dispose();
  });

  test('a short part is not past its end after the book changed', () {
    store.write(LastPlayed(bookId: '2', track: 9, position: const Duration(minutes: 1), at: 1));
    final p = make()..attachServer(_Source(), LoadedLibrary([_book('1'), _book('2')], {}));
    expect(p.trackIndex, 3, reason: 'the last part, not an index that does not exist');
    p.dispose();
  });
}
