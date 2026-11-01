// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';
import 'dart:ui' show Color;

import '../models.dart';
import '../server/app_paths.dart';
import '../server/server_source.dart';

/// The library as the app last got it from the server, kept on the device so that the offline view works after a restart without the server.
/// The pictures are files of their own (`cover-<id>`), the rest is one JSON file.
class LibraryCache {
  LibraryCache(this.dir);

  factory LibraryCache.standard() => LibraryCache(Directory('${AppPaths.base.path}/library'));

  final Directory dir;

  File get _file => File('${dir.path}/library.json');
  File _cover(String id) => File('${dir.path}/cover-$id');

  /// Writes the library down. Never throws: a cache that cannot be written is only a missing convenience.
  Future<void> write(LoadedLibrary lib) async {
    try {
      await dir.create(recursive: true);
      for (final b in lib.books) {
        final bytes = b.cover;
        if (bytes == null) continue;
        final f = _cover(b.id);
        if (!f.existsSync() || f.lengthSync() != bytes.length) await f.writeAsBytes(bytes);
      }
      final temp = File('${_file.path}.tmp');
      await temp.writeAsString(jsonEncode(libraryToJson(lib)));
      await temp.rename(_file.path);
      // The pictures of books that are gone.
      final ids = {for (final b in lib.books) b.id};
      await for (final e in dir.list()) {
        final name = e.path.split(Platform.pathSeparator).last;
        if (e is File && name.startsWith('cover-') && !ids.contains(name.substring(6))) await e.delete();
      }
    } on Object {
      // see above
    }
  }

  /// The library written last, or null if there is none (or it cannot be read).
  LoadedLibrary? read() {
    try {
      final lib = libraryFromJson(jsonDecode(_file.readAsStringSync()) as Map<String, dynamic>, (id) {
        final f = _cover(id);
        return f.existsSync() ? f.readAsBytesSync() : null;
      });
      return lib;
    } on Object {
      return null;
    }
  }

  Future<void> clear() async {
    try {
      if (dir.existsSync()) await dir.delete(recursive: true);
    } on Object {
      // left behind; the next write replaces it
    }
  }
}

Map<String, Object?> _trackToJson(Track t) => {
  'disc': t.disc,
  'number': t.number,
  'title': t.title,
  'length_ms': t.length.inMilliseconds,
  'part': t.partId,
  'plays': t.plays,
  'last_played_at': t.lastPlayedAt?.millisecondsSinceEpoch,
  'labels': t.labels,
};

Track _trackFromJson(Map<String, dynamic> j) => Track(
  disc: j['disc'] as int,
  number: j['number'] as int,
  title: j['title'] as String,
  length: Duration(milliseconds: j['length_ms'] as int),
  partId: j['part'] as int?,
  plays: j['plays'] as int? ?? 0,
  lastPlayedAt: dateFromMs(j['last_played_at']),
  labels: [for (final l in (j['labels'] as List? ?? const [])) l as String],
);

Map<String, Object?> bookToJson(LibraryItem b) => {
  'id': b.id,
  'title': b.title,
  'author': b.author,
  'color': b.color.toARGB32(),
  'kind': b.kind,
  'blurb': b.blurb,
  'narrator': b.narrator,
  'genre': b.genre,
  'plays': b.plays,
  'last_played_at': b.lastPlayedAt?.millisecondsSinceEpoch,
  'hidden': b.hidden,
  'owner': b.owner,
  'labels': b.labels,
  'track_labels': b.trackLabels,
  'can_label': b.canLabel,
  'can_share': b.canShare,
  'barcode': b.barcode,
  'year': b.year,
  'tracks': [for (final t in b.tracks) _trackToJson(t)],
};

LibraryItem bookFromJson(Map<String, dynamic> j, {List<int>? cover}) => LibraryItem(
  id: j['id'] as String,
  title: j['title'] as String,
  author: j['author'] as String,
  color: Color(j['color'] as int),
  kind: j['kind'] as String? ?? 'audiobook',
  blurb: j['blurb'] as String?,
  narrator: j['narrator'] as String?,
  genre: j['genre'] as String?,
  plays: j['plays'] as int? ?? 0,
  lastPlayedAt: dateFromMs(j['last_played_at']),
  hidden: j['hidden'] == true,
  owner: j['owner'] as String?,
  labels: [for (final l in (j['labels'] as List? ?? const [])) l as String],
  trackLabels: [for (final l in (j['track_labels'] as List? ?? const [])) l as String],
  canLabel: j['can_label'] == true,
  canShare: j['can_share'] == true,
  barcode: j['barcode'] as String?,
  year: j['year'] as int?,
  cover: cover == null ? null : (cover is Uint8List ? cover : Uint8List.fromList(cover)),
  tracks: [for (final t in (j['tracks'] as List)) _trackFromJson(t as Map<String, dynamic>)],
);

Map<String, Object?> libraryToJson(LoadedLibrary lib) => {
  'books': [for (final b in lib.books) bookToJson(b)],
  'resume': {for (final e in lib.resume.entries) e.key: {'track': e.value.track, 'position_ms': e.value.position.inMilliseconds}},
  'last_book': lib.lastBookId,
  'last_at': lib.lastAt,
  'user': lib.userName,
  'recent': lib.recent,
  'genres': lib.genres,
  'new_ids': lib.newIds,
};

LoadedLibrary libraryFromJson(Map<String, dynamic> j, List<int>? Function(String id) coverOf) {
  final books = [for (final b in (j['books'] as List)) bookFromJson(b as Map<String, dynamic>, cover: coverOf((b as Map)['id'] as String))];
  final resume = <String, ({int track, Duration position})>{
    for (final e in ((j['resume'] as Map?) ?? const {}).entries)
      e.key as String: (track: (e.value as Map)['track'] as int, position: Duration(milliseconds: (e.value as Map)['position_ms'] as int)),
  };
  return LoadedLibrary(
    books,
    resume,
    lastBookId: j['last_book'] as String?,
    lastAt: j['last_at'] as int? ?? 0,
    userName: j['user'] as String?,
    recent: [for (final r in (j['recent'] as List? ?? const [])) r as String],
    genres: [for (final g in (j['genres'] as List? ?? const [])) g as String],
    newIds: [for (final r in (j['new_ids'] as List? ?? const [])) r as String],
  );
}
