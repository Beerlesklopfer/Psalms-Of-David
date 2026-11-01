// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'models.dart';

/// Music by genre: the genres of [order] first (the order of the administration), then the
/// genres only the albums know, alphabetically, and the albums without a genre last under `null`.
List<(String?, List<LibraryItem>)> groupByGenre(List<LibraryItem> books, List<String> order) {
  final by = <String, List<LibraryItem>>{};
  final none = <LibraryItem>[];
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

/// One title (a song, a chapter) that a search found: the track [index] of [book].
class TrackHit {
  const TrackHit(this.book, this.index);

  final LibraryItem book;
  final int index;

  Track get track => book.tracks[index];
}

/// Most titles a search lists, so that a short text does not bring thousands of lines.
const int kMaxTrackHits = 50;

/// What a search finds, in groups: the albums (or books) by their title, the creators (artists or
/// authors) and the narrators by their name, and single titles by their own title. A name typed in full also brings all their albums.
class SearchGroups {
  SearchGroups(this.albums, this.creators, this.narrators, [this.tracks = const []]);

  final List<LibraryItem> albums;
  final List<String> creators;
  final List<String> narrators;
  final List<TrackHit> tracks;

  bool get isEmpty => albums.isEmpty && creators.isEmpty && narrators.isEmpty && tracks.isEmpty;
}

SearchGroups searchGroups(List<LibraryItem> pool, String query) {
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
  // A label is found too: a word typed that is (part of) a label of the book.
  final albums = pool
      .where((b) => has(b.title) || is_(b.author) || is_(b.narrator) || b.allLabels.any(has))
      .toList();
  // A title of its own (a song, a chapter): found when its title contains the text, in the order of the albums given.
  final tracks = <TrackHit>[
    for (final b in pool)
      for (var i = 0; i < b.tracks.length; i++)
        if (has(b.tracks[i].title)) TrackHit(b, i),
  ];
  return SearchGroups(albums, creators, narrators, tracks.take(kMaxTrackHits).toList());
}

/// Music (or any list) by owner: the owners alphabetically by nickname, each with their albums in the order given, and the albums that belong to
/// nobody last under `null`. With no owner at all the result is one group `null`. The owner [first] (the signed-in person), if there is one,
/// comes before the others.
List<(String?, List<LibraryItem>)> groupByOwner(List<LibraryItem> books, {String? first}) {
  final by = <String, List<LibraryItem>>{};
  final none = <LibraryItem>[];
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
