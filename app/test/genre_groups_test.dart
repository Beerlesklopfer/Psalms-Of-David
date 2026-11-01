// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/library_groups.dart';
import 'package:psalm/models.dart';

LibraryItem _b(String id, String title, String author, {String kind = 'music', String? genre, String? narrator}) => LibraryItem(
  id: id,
  title: title,
  author: author,
  color: const Color(0xFF000000),
  kind: kind,
  genre: genre,
  narrator: narrator,
  tracks: const [],
);

void main() {
  _narratorTest();
  test('music is grouped by genre: the administration order, then unknown ones, then none', () {
    final groups = groupByGenre(
      [
        _b('1', 'A', 'x', genre: 'Jazz'),
        _b('2', 'B', 'x', genre: 'Rock'),
        _b('3', 'C', 'x'),
        _b('4', 'D', 'x', genre: 'Blues'),
        _b('5', 'E', 'x', genre: 'Rock'),
      ],
      ['Rock', 'Pop'],
    );
    expect([for (final (g, _) in groups) g], ['Rock', 'Blues', 'Jazz', null]);
    expect(groups.first.$2.map((b) => b.id), ['2', '5']);
  });

  test('a search finds albums by title, creators and narrators by name, and a full name brings all albums', () {
    final pool = [
      _b('1', 'Beyond Belief', 'Petra'),
      _b('2', 'Back to the Street', 'Petra'),
      _b('3', 'Petra Praise', 'Various'),
      _b('4', 'Winter der Welt', 'Follett', kind: 'audiobook', narrator: 'Johannes Steck'),
    ];
    final f = searchGroups(pool, 'petra');
    expect(f.albums.map((b) => b.id), ['1', '2', '3'], reason: 'the full name brings the albums, the title adds one');
    expect(f.creators, ['Petra']);
    expect(f.narrators, isEmpty);

    final n = searchGroups(pool, 'steck');
    expect(n.albums, isEmpty);
    expect(n.narrators, ['Johannes Steck']);
    expect(searchGroups(pool, 'Johannes Steck').albums.map((b) => b.id), ['4']);
    expect(searchGroups(pool, '  ').isEmpty, isTrue);
  });

  test('a title inside an album is found too, with its place, and a hit on a track alone is not empty', () {
    Track t(int n, String title) => Track(disc: 1, number: n, title: title, length: const Duration(minutes: 3));
    final book = LibraryItem(
      id: '9',
      title: 'Spiele',
      author: 'Various',
      color: const Color(0xFF000000),
      tracks: [t(1, 'Ein Lied'), t(2, 'Monopoly'), t(3, 'Monopoly (Reprise)')],
    );
    final f = searchGroups([book, _b('1', 'Nichts', 'Niemand')], 'monopoly');
    expect(f.albums, isEmpty, reason: 'no album title says it');
    expect(f.tracks.map((h) => (h.book.id, h.index, h.track.title)), [('9', 1, 'Monopoly'), ('9', 2, 'Monopoly (Reprise)')]);
    expect(f.isEmpty, isFalse);
    expect(searchGroups([book], 'gibt es nicht').isEmpty, isTrue);
  });
}

void _narratorTest() {
  test('only audiobooks have narrators in the search; the composer of music does not', () {
    final pool = [
      _b('1', 'Hiersein', 'Duo Camillo', narrator: 'Sefora Nelson'),
      _b('2', 'Winter der Welt', 'Follett', kind: 'audiobook', narrator: 'Johannes Steck'),
    ];
    expect(searchGroups(pool, 'sef').narrators, isEmpty);
    expect(searchGroups(pool, 'steck').narrators, ['Johannes Steck']);
  });
}
