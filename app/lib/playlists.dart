// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';

import 'models.dart';
import 'server/api_client.dart' show ApiException;
import 'server/app_paths.dart';

// The limits of the server (`docs/spec-playlists.md`, constants in `playlists.rs`). The server is the
// authority; the app only uses them to stop an entry that cannot succeed before it is sent.

/// Longest name of a playlist, in characters.
const int kPlaylistNameMax = 100;

/// Most playlists of one profile.
const int kPlaylistsMax = 100;

/// Most entries in one playlist.
const int kPlaylistItemsMax = 5000;

/// The kind of the virtual book that a playing playlist is. Its id is [kPlaylistBookPrefix] and the playlist's id.
const String kPlaylistKind = 'playlist';
const String kPlaylistBookPrefix = 'pl:';

/// The codes of the server's playlist errors (`{"error": "<code>"}`). Each has a text in the app.
const List<String> kPlaylistErrorCodes = [
  'name_taken',
  'invalid_name',
  'unknown_part',
  'unknown_book',
  'invalid_order',
  'invalid_position',
  'invalid_request',
  'playlist_full',
  'too_many_playlists',
  'not_found',
];

/// One line of the list of playlists.
class PlaylistSummary {
  const PlaylistSummary({
    required this.id,
    required this.name,
    this.items = 0,
    this.seconds = 0,
    this.updatedAt,
  });

  final int id;
  final String name;

  /// Available entries only.
  final int items;
  final int seconds;
  final DateTime? updatedAt;

  Duration get total => Duration(seconds: seconds);

  factory PlaylistSummary.fromJson(Map<String, dynamic> j) => PlaylistSummary(
    id: j['id'] as int,
    name: j['name'] as String,
    items: (j['items'] as int?) ?? 0,
    seconds: (j['seconds'] as num?)?.round() ?? 0,
    updatedAt: dateFromMs(j['updated_at']),
  );

  Map<String, Object?> toJson() => {
    'id': id,
    'name': name,
    'items': items,
    'seconds': seconds,
    'updated_at': updatedAt?.millisecondsSinceEpoch,
  };
}

/// One entry of a playlist: a part of a book. An entry the profile may not see now is [available] false and
/// carries nothing else; it stays in the list and is skipped when playing.
class PlaylistEntry {
  const PlaylistEntry({
    required this.entryId,
    required this.position,
    this.available = true,
    this.partId,
    this.title = '',
    this.duration = Duration.zero,
    this.plays = 0,
    this.lastPlayedAt,
    this.bookId,
    this.bookTitle = '',
    this.bookAuthor = '',
  });

  /// The id of the entry (for removing and ordering); not the id of the part.
  final int entryId;
  final int position;
  final bool available;

  /// The part to stream and to report progress for.
  final int? partId;
  final String title;
  final Duration duration;
  final int plays;
  final DateTime? lastPlayedAt;

  /// The book the part belongs to.
  final int? bookId;
  final String bookTitle;
  final String bookAuthor;

  factory PlaylistEntry.fromJson(Map<String, dynamic> j) {
    final part = j['part'] as Map<String, dynamic>?;
    final book = j['book'] as Map<String, dynamic>?;
    final available = (j['available'] as bool?) ?? (part != null);
    return PlaylistEntry(
      entryId: j['item_id'] as int,
      position: (j['position'] as int?) ?? 0,
      available: available && part != null,
      partId: part?['id'] as int?,
      title: (part?['title'] as String?) ?? '',
      duration: Duration(milliseconds: (part?['duration_ms'] as num?)?.round() ?? 0),
      plays: (part?['plays'] as int?) ?? 0,
      lastPlayedAt: dateFromMs(part?['last_played_at']),
      bookId: book?['id'] as int?,
      bookTitle: (book?['title'] as String?) ?? '',
      bookAuthor: (book?['author'] as String?) ?? '',
    );
  }

  Map<String, Object?> toJson() => {
    'item_id': entryId,
    'position': position,
    'available': available,
    'part': available
        ? {
            'id': partId,
            'title': title,
            'duration_ms': duration.inMilliseconds,
            'plays': plays,
            'last_played_at': lastPlayedAt?.millisecondsSinceEpoch,
          }
        : null,
    'book': available ? {'id': bookId, 'title': bookTitle, 'author': bookAuthor} : null,
  };
}

class Playlist {
  const Playlist({required this.id, required this.name, this.entries = const []});

  final int id;
  final String name;
  final List<PlaylistEntry> entries;

  Iterable<PlaylistEntry> get available => entries.where((e) => e.available);
  Duration get total => available.fold(Duration.zero, (a, e) => a + e.duration);

  factory Playlist.fromJson(Map<String, dynamic> j) => Playlist(
    id: j['id'] as int,
    name: j['name'] as String,
    entries: [
      for (final e in (j['items'] as List? ?? const []).cast<Map<String, dynamic>>())
        PlaylistEntry.fromJson(e),
    ],
  );

  Map<String, Object?> toJson() => {
    'id': id,
    'name': name,
    'items': [for (final e in entries) e.toJson()],
  };

  Playlist copyWith({String? name, List<PlaylistEntry>? entries}) =>
      Playlist(id: id, name: name ?? this.name, entries: entries ?? this.entries);
}

/// The answer to `GET /playlists`: null lists mean "not modified" (304).
class PlaylistListAnswer {
  const PlaylistListAnswer(this.lists, this.etag);

  /// Null: the server answered 304, what the app has is current.
  final List<PlaylistSummary>? lists;
  final String? etag;
}

/// The server side of the playlists (`/api/v1/playlists`). [ApiClient] is the real one; tests use a fake.
abstract class PlaylistApi {
  /// The lists of the profile. With [etag] of the copy the app has: a 304 gives `lists == null`.
  Future<PlaylistListAnswer> playlists({String? etag});
  Future<Playlist> playlist(int id);
  Future<({int id, String name})> createPlaylist(String name);
  Future<void> renamePlaylist(int id, String name);
  Future<void> deletePlaylist(int id);

  /// Appends parts, or at the 1-based position [at]. Answers the list.
  Future<Playlist> addParts(int id, List<int> partIds, {int? at});

  /// Appends all parts of a book in the book's order. Answers the list.
  Future<Playlist> addBook(int id, int bookId, {int? at});
  Future<void> removeEntry(int id, int entryId);

  /// The new order: every entry id of the list, once. Answers the list.
  Future<Playlist> reorder(int id, List<int> entryIds);
}

/// The copy of the playlists on this device, in a small file, so that they show without the server.
/// A missing or damaged file is an empty copy.
class PlaylistStore {
  PlaylistStore(this.file);

  factory PlaylistStore.standard() => PlaylistStore(File('${AppPaths.base.path}/playlists.json'));

  final File file;

  PlaylistCopy read() {
    try {
      final j = jsonDecode(file.readAsStringSync()) as Map<String, dynamic>;
      return PlaylistCopy(
        etag: j['etag'] as String?,
        lists: [
          for (final l in (j['lists'] as List).cast<Map<String, dynamic>>())
            PlaylistSummary.fromJson(l),
        ],
        details: {
          for (final d in (j['details'] as List? ?? const []).cast<Map<String, dynamic>>())
            (d['id'] as int): Playlist.fromJson(d),
        },
        resume: {
          for (final e in ((j['resume'] as Map<String, dynamic>?) ?? const {}).entries)
            int.parse(e.key): (
              partId: (e.value as Map<String, dynamic>)['part'] as int,
              position: Duration(
                milliseconds: (e.value as Map<String, dynamic>)['position_ms'] as int,
              ),
            ),
        },
      );
    } on Object {
      return const PlaylistCopy();
    }
  }

  void write(PlaylistCopy c) {
    try {
      file.parent.createSync(recursive: true);
      final temp = File('${file.path}.tmp');
      temp.writeAsStringSync(
        jsonEncode({
          'etag': c.etag,
          'lists': [for (final l in c.lists) l.toJson()],
          'details': [for (final d in c.details.values) d.toJson()],
          'resume': {
            for (final e in c.resume.entries)
              '${e.key}': {'part': e.value.partId, 'position_ms': e.value.position.inMilliseconds},
          },
        }),
      );
      temp.renameSync(file.path);
    } on FileSystemException {
      // No copy is not worth an error: the server has the lists.
    }
  }
}

/// What the store holds: the lists, those whose entries were loaded, where each playlist was left.
class PlaylistCopy {
  const PlaylistCopy({
    this.etag,
    this.lists = const [],
    this.details = const {},
    this.resume = const {},
  });

  final String? etag;
  final List<PlaylistSummary> lists;
  final Map<int, Playlist> details;

  /// Per playlist id: the part that was playing and the place in it.
  final Map<int, ({int partId, Duration position})> resume;
}

/// The playlists of the profile: the lists, the entries of those that were opened, the copy on the
/// device, and the changes. Works without a server ([api] null) on the copy.
class PlaylistController extends ChangeNotifier {
  PlaylistController({this.api, this.store}) {
    final c = store?.read();
    if (c != null) {
      lists = List.of(c.lists);
      details.addAll(c.details);
      _etag = c.etag;
      _resume.addAll(c.resume);
    }
  }

  /// The server; set when the app is paired, null otherwise.
  PlaylistApi? api;
  final PlaylistStore? store;

  /// Set by the app: whether a track of a playlist is to be offered now. Offline it is true only for the tracks on the device, so the playlist
  /// does not list what would fail for want of the server. Null: everything is offered.
  bool Function(Track)? offers;

  /// The offline mode changed: the playlists show what is on the device, or everything again.
  void offlineChanged() => notifyListeners();

  List<PlaylistSummary> lists = [];
  final Map<int, Playlist> details = {};
  final Map<int, ({int partId, Duration position})> _resume = {};
  String? _etag;

  /// The code of the last failed load, or null. The copy stays on show.
  String? loadError;
  bool loading = false;
  bool _disposed = false;

  void _changed() {
    if (!_disposed) notifyListeners();
  }

  void _save() =>
      store?.write(PlaylistCopy(etag: _etag, lists: lists, details: details, resume: _resume));

  /// Loads the lists (with `If-None-Match`, so an unchanged set costs one small answer) and the entries
  /// of the lists that were opened before, since a change of the library can change which entries are available.
  Future<void> load({bool force = false}) async {
    final a = api;
    if (a == null || loading) return;
    loading = true;
    try {
      final answer = await a.playlists(etag: force ? null : _etag);
      if (answer.lists != null) {
        lists = answer.lists!;
        _etag = answer.etag;
        details.removeWhere((id, _) => !lists.any((l) => l.id == id));
        _resume.removeWhere((id, _) => !lists.any((l) => l.id == id));
      }
      for (final id in details.keys.toList()) {
        try {
          details[id] = await a.playlist(id);
        } on Object catch (e) {
          if (codeOf(e) == 'not_found') details.remove(id);
        }
      }
      loadError = null;
      _save();
    } on Object catch (e) {
      loadError = codeOf(e);
    } finally {
      loading = false;
      _changed();
    }
  }

  /// The code of an error: the server's, or `network` when there was no answer.
  static String codeOf(Object e) => e is ApiException ? e.code : 'network';

  /// The entries of a playlist: the server's, or the copy if the server cannot be reached.
  Future<Playlist?> open(int id) async {
    final a = api;
    if (a != null) {
      try {
        final p = await a.playlist(id);
        details[id] = p;
        _save();
        _changed();
        return p;
      } on Object {
        // fall back to the copy
      }
    }
    return details[id];
  }

  void _take(Playlist p) {
    details[p.id] = p;
    final i = lists.indexWhere((l) => l.id == p.id);
    if (i >= 0) {
      lists[i] = PlaylistSummary(
        id: p.id,
        name: p.name,
        items: p.available.length,
        seconds: p.total.inSeconds,
        updatedAt: DateTime.now(),
      );
    }
    _save();
    _changed();
  }

  /// Makes a playlist. Throws the server's error (`name_taken`, `invalid_name`, `too_many_playlists`).
  Future<int> create(String name) async {
    final made = await api!.createPlaylist(name.trim());
    lists = [...lists, PlaylistSummary(id: made.id, name: made.name)];
    _save();
    _changed();
    unawaited2(load());
    return made.id;
  }

  Future<void> rename(int id, String name) async {
    await api!.renamePlaylist(id, name.trim());
    final i = lists.indexWhere((l) => l.id == id);
    if (i >= 0) {
      lists[i] = PlaylistSummary(
        id: id,
        name: name.trim(),
        items: lists[i].items,
        seconds: lists[i].seconds,
        updatedAt: lists[i].updatedAt,
      );
    }
    final d = details[id];
    if (d != null) details[id] = d.copyWith(name: name.trim());
    _save();
    _changed();
  }

  Future<void> delete(int id) async {
    await api!.deletePlaylist(id);
    lists = lists.where((l) => l.id != id).toList();
    details.remove(id);
    _resume.remove(id);
    _save();
    _changed();
  }

  Future<Playlist> addParts(int id, List<int> partIds, {int? at}) async {
    final p = await api!.addParts(id, partIds, at: at);
    _take(p);
    return p;
  }

  Future<Playlist> addBook(int id, int bookId, {int? at}) async {
    final p = await api!.addBook(id, bookId, at: at);
    _take(p);
    return p;
  }

  Future<void> removeEntry(int id, int entryId) async {
    await api!.removeEntry(id, entryId);
    final d = details[id];
    if (d != null) {
      final rest = d.entries.where((e) => e.entryId != entryId).toList();
      var n = 0;
      _take(
        d.copyWith(
          entries: [
            for (final e in rest)
              PlaylistEntry(
                entryId: e.entryId,
                position: ++n,
                available: e.available,
                partId: e.partId,
                title: e.title,
                duration: e.duration,
                plays: e.plays,
                lastPlayedAt: e.lastPlayedAt,
                bookId: e.bookId,
                bookTitle: e.bookTitle,
                bookAuthor: e.bookAuthor,
              ),
          ],
        ),
      );
    }
  }

  /// Moves the entry at [from] to the place [to] (the index it has afterwards). The list on screen follows at
  /// once and the server is told; if it refuses, the old order comes back and the error is thrown.
  Future<void> reorder(int id, int from, int to) async {
    final d = details[id];
    if (d == null) return;
    final moved = [...d.entries];
    final e = moved.removeAt(from);
    moved.insert(to, e);
    var n = 0;
    final numbered = [
      for (final x in moved)
        PlaylistEntry(
          entryId: x.entryId,
          position: ++n,
          available: x.available,
          partId: x.partId,
          title: x.title,
          duration: x.duration,
          plays: x.plays,
          lastPlayedAt: x.lastPlayedAt,
          bookId: x.bookId,
          bookTitle: x.bookTitle,
          bookAuthor: x.bookAuthor,
        ),
    ];
    details[id] = d.copyWith(entries: numbered);
    _changed();
    try {
      final p = await api!.reorder(id, [for (final x in numbered) x.entryId]);
      _take(p);
    } on Object {
      details[id] = d;
      _changed();
      rethrow;
    }
  }

  // ---- Playing ------------------------------------------------------------------------------

  /// The playlist as one virtual book: the available entries in order; the others are left out, not deleted.
  /// Every track keeps its own part and book, so progress goes to the real item as it always does.
  Book bookOf(Playlist p) => Book(
    id: '$kPlaylistBookPrefix${p.id}',
    title: p.name,
    author: '',
    color: const Color(0xFF3B2A4A),
    kind: kPlaylistKind,
    tracks: [
      for (final e in p.available)
        if (offers == null || offers!(_trackOf(e)))
          _trackOf(e),
    ],
  );

  Track _trackOf(PlaylistEntry e) => Track(
    disc: 1,
    number: e.position,
    title: e.title,
    length: e.duration,
    partId: e.partId,
    itemId: e.bookId == null ? null : '${e.bookId}',
    bookTitle: e.bookTitle,
    bookAuthor: e.bookAuthor,
    plays: e.plays,
    lastPlayedAt: e.lastPlayedAt,
  );

  /// Where to go on in [p]: the track and place that were left, else the start.
  ({int track, Duration position}) startOf(Playlist p, Book b) {
    final r = _resume[p.id];
    if (r != null) {
      final i = b.tracks.indexWhere((t) => t.partId == r.partId);
      if (i >= 0) return (track: i, position: r.position);
    }
    return (track: 0, position: Duration.zero);
  }

  /// Remembers the place in a playlist that plays (by part, so a reordered list still finds it).
  void remember(Book b, Track t, Duration position) {
    if (b.kind != kPlaylistKind || t.partId == null) return;
    final id = int.tryParse(b.id.substring(kPlaylistBookPrefix.length));
    if (id == null) return;
    _resume[id] = (partId: t.partId!, position: position);
    _save();
  }

  @override
  void dispose() {
    _disposed = true;
    super.dispose();
  }
}

/// Fire and forget without a lint.
void unawaited2(Future<void> f) {
  f.catchError((Object _) {});
}
