// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'models.dart';

/// Music by genre: the genres of [order] first (the order of the administration), then the
/// genres only the albums know, alphabetically, and the albums without a genre last under `null`.
List<(String?, List<Book>)> groupByGenre(List<Book> books, List<String> order) {
  final by = <String, List<Book>>{};
  final none = <Book>[];
  for (final b in books) {
    final g = b.genre;
    if (g == null || g.trim().isEmpty) {
      none.add(b);
    } else {
      by.putIfAbsent(g, () => []).add(b);
    }
  }
  final names = [
    ...order.where(by.containsKey),
    ...(by.keys.where((g) => !order.contains(g)).toList()
      ..sort((a, b) => a.toLowerCase().compareTo(b.toLowerCase()))),
  ];
  return [for (final g in names) (g, by[g]!), if (none.isNotEmpty) (null, none)];
}

/// What a search finds, in groups: the albums (or books) by their title, the creators (artists or
/// authors) and the narrators by their name. A name typed in full also brings all their albums.
class SearchGroups {
  SearchGroups(this.albums, this.creators, this.narrators);

  final List<Book> albums;
  final List<String> creators;
  final List<String> narrators;

  bool get isEmpty => albums.isEmpty && creators.isEmpty && narrators.isEmpty;
}

SearchGroups searchGroups(List<Book> pool, String query) {
  final q = query.trim().toLowerCase();
  if (q.isEmpty) return SearchGroups(const [], const [], const []);
  bool has(String? s) => s != null && s.toLowerCase().contains(q);
  bool is_(String? s) => s != null && s.toLowerCase() == q;
  final creators = {
    for (final b in pool)
      if (has(b.author)) b.author,
  }.toList()..sort();
  // Only audiobooks have a narrator; the composer tag of music says who wrote it.
  final narrators = {
    for (final b in pool)
      if (b.kind == 'audiobook' && has(b.narrator)) b.narrator!,
  }.toList()..sort();
  final albums = pool.where((b) => has(b.title) || is_(b.author) || is_(b.narrator)).toList();
  return SearchGroups(albums, creators, narrators);
}

/// Music (or any list) by owner: the owners alphabetically by nickname, each with their albums in the order given, and the albums that belong to
/// nobody last under `null`. With no owner at all the result is one group `null`. The owner [first] (the signed-in person), if there is one,
/// comes before the others.
List<(String?, List<Book>)> groupByOwner(List<Book> books, {String? first}) {
  final by = <String, List<Book>>{};
  final none = <Book>[];
  for (final b in books) {
    final o = b.owner;
    if (o == null || o.trim().isEmpty) {
      none.add(b);
    } else {
      by.putIfAbsent(o, () => []).add(b);
    }
  }
  final names = by.keys.toList()
    ..sort((a, b) {
      if (a == first) return b == first ? 0 : -1;
      if (b == first) return 1;
      return a.toLowerCase().compareTo(b.toLowerCase());
    });
  return [for (final n in names) (n, by[n]!), if (none.isNotEmpty) (null, none)];
}
