// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:psalm/playlists.dart';
import 'package:psalm/server/api_client.dart';

PlaylistEntry entry(
  int entryId,
  int position,
  int partId,
  String title, {
  int bookId = 17,
  String book = 'Winter',
  String author = 'Follett',
  int seconds = 240,
  bool available = true,
}) => available
    ? PlaylistEntry(
        entryId: entryId,
        position: position,
        partId: partId,
        title: title,
        duration: Duration(seconds: seconds),
        bookId: bookId,
        itemTitle: book,
        itemAuthor: author,
      )
    : PlaylistEntry(entryId: entryId, position: position, available: false);

/// The server's playlists in memory, with its rules (names unique ignoring case, order must be a permutation).
class FakePlaylistApi implements PlaylistApi {
  final lists = <int, Playlist>{};
  int nextId = 1;
  int nextEntry = 100;
  String etag = 'v1';
  final calls = <String>[];
  String? lastEtagSent;
  Object? failWith;
  Object? failOnce;

  void _fail() {
    if (failOnce != null) {
      final e = failOnce!;
      failOnce = null;
      throw e;
    }
    if (failWith != null) throw failWith!;
  }

  PlaylistSummary summary(Playlist p) => PlaylistSummary(
    id: p.id,
    name: p.name,
    items: p.available.length,
    seconds: p.total.inSeconds,
  );

  Playlist add(String name, [List<PlaylistEntry> entries = const []]) {
    final p = Playlist(id: nextId++, name: name, entries: entries);
    lists[p.id] = p;
    return p;
  }

  @override
  Future<PlaylistListAnswer> playlists({String? etag}) async {
    calls.add('list');
    lastEtagSent = etag;
    _fail();
    if (etag != null && etag == this.etag) return PlaylistListAnswer(null, etag);
    return PlaylistListAnswer([for (final p in lists.values) summary(p)], this.etag);
  }

  @override
  Future<Playlist> playlist(int id) async {
    calls.add('get $id');
    _fail();
    final p = lists[id];
    if (p == null) throw ApiException(404, 'not_found');
    return p;
  }

  @override
  Future<({int id, String name})> createPlaylist(String name) async {
    calls.add('create $name');
    _fail();
    if (name.isEmpty || name.length > kPlaylistNameMax) throw ApiException(422, 'invalid_name');
    if (lists.values.any((p) => p.name.toLowerCase() == name.toLowerCase())) {
      throw ApiException(409, 'name_taken');
    }
    if (lists.length >= kPlaylistsMax) throw ApiException(422, 'too_many_playlists');
    final p = add(name);
    etag = 'v${nextId + nextEntry}';
    return (id: p.id, name: p.name);
  }

  @override
  Future<void> renamePlaylist(int id, String name) async {
    calls.add('rename $id $name');
    _fail();
    final p = lists[id] ?? (throw ApiException(404, 'not_found'));
    if (lists.values.any((o) => o.id != id && o.name.toLowerCase() == name.toLowerCase())) {
      throw ApiException(409, 'name_taken');
    }
    lists[id] = p.copyWith(name: name);
  }

  @override
  Future<void> deletePlaylist(int id) async {
    calls.add('delete $id');
    _fail();
    if (lists.remove(id) == null) throw ApiException(404, 'not_found');
  }

  Playlist _append(int id, List<PlaylistEntry> news, int? at) {
    final p = lists[id] ?? (throw ApiException(404, 'not_found'));
    if (p.entries.length + news.length > kPlaylistItemsMax) {
      throw ApiException(422, 'playlist_full');
    }
    final all = [...p.entries];
    all.insertAll(at == null ? all.length : at - 1, news);
    var n = 0;
    final numbered = [
      for (final e in all)
        PlaylistEntry(
          entryId: e.entryId,
          position: ++n,
          available: e.available,
          partId: e.partId,
          title: e.title,
          duration: e.duration,
          bookId: e.bookId,
          itemTitle: e.itemTitle,
          itemAuthor: e.itemAuthor,
        ),
    ];
    return lists[id] = p.copyWith(entries: numbered);
  }

  /// The parts the server knows, by id: (title, book id, book title).
  final parts = <int, ({String title, int bookId, String book, String author})>{
    880: (title: 'Chapter 1', bookId: 17, book: 'Winter', author: 'Follett'),
    881: (title: 'Chapter 2', bookId: 17, book: 'Winter', author: 'Follett'),
    900: (title: 'Song', bookId: 20, book: 'Album', author: 'Band'),
  };

  @override
  Future<Playlist> addParts(int id, List<int> partIds, {int? at}) async {
    calls.add('addParts $id $partIds');
    _fail();
    if (partIds.any((p) => !parts.containsKey(p))) throw ApiException(422, 'unknown_part');
    return _append(id, [
      for (final pid in partIds)
        entry(
          nextEntry++,
          0,
          pid,
          parts[pid]!.title,
          bookId: parts[pid]!.bookId,
          book: parts[pid]!.book,
          author: parts[pid]!.author,
        ),
    ], at);
  }

  @override
  Future<Playlist> addBook(int id, int bookId, {int? at}) async {
    calls.add('addBook $id $bookId');
    _fail();
    final ps = parts.entries.where((e) => e.value.bookId == bookId).toList();
    if (ps.isEmpty) throw ApiException(422, 'unknown_book');
    return _append(id, [
      for (final e in ps)
        entry(
          nextEntry++,
          0,
          e.key,
          e.value.title,
          bookId: bookId,
          book: e.value.book,
          author: e.value.author,
        ),
    ], at);
  }

  @override
  Future<void> removeEntry(int id, int entryId) async {
    calls.add('remove $id $entryId');
    _fail();
    final p = lists[id] ?? (throw ApiException(404, 'not_found'));
    if (!p.entries.any((e) => e.entryId == entryId)) throw ApiException(404, 'not_found');
    lists[id] = p.copyWith(entries: p.entries.where((e) => e.entryId != entryId).toList());
  }

  @override
  Future<Playlist> reorder(int id, List<int> entryIds) async {
    calls.add('reorder $id $entryIds');
    _fail();
    final p = lists[id] ?? (throw ApiException(404, 'not_found'));
    final have = p.entries.map((e) => e.entryId).toSet();
    if (entryIds.length != have.length || !have.containsAll(entryIds)) {
      throw ApiException(422, 'invalid_order');
    }
    return lists[id] = p.copyWith(
      entries: [for (final i in entryIds) p.entries.firstWhere((e) => e.entryId == i)],
    );
  }
}
