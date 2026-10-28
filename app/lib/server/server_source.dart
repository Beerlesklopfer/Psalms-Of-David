// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';
import 'dart:ui';

import '../models.dart';
import 'api_client.dart';

/// Where the passage of an audio Bible begins.
class PassageHit {
  PassageHit(this.itemId, this.partId, this.position, {required this.allowed});

  final String itemId;
  final int partId;
  final Duration position;

  /// False if the weekly planner forbids listening now.
  final bool allowed;
}

/// What the player needs from the server, besides the files: the file of a part,
/// reporting the position, and finding passages. The controller talks to this
/// interface; [ServerSource] is the real one.
abstract class MediaSource {
  /// A local file with the audio of [track].
  Future<String> fetchPart(Track track);

  /// Whether parts can be kept on the device for listening without the server.
  bool get canDownload => false;

  /// Is the file of [track] on the device already?
  bool isDownloaded(Track track) => false;

  /// Deletes the file of [track] from the device. The server keeps it.
  Future<void> removeDownload(Track track) async {}

  /// What the downloads take on this device.
  Future<({int files, int bytes})> storageUsed() async => (files: 0, bytes: 0);

  /// Deletes every downloaded part.
  Future<void> removeAllDownloads() async {}

  Future<void> saveProgress(Book book, Track track, Duration position, {required bool finished});

  Future<PassageHit?> resolvePassage(String text);
}

class ServerSource implements MediaSource {
  ServerSource(this.api, this.cache);

  final ApiClient api;

  /// Downloaded parts. Named by part id, so a part is fetched once.
  final Directory cache;

  @override
  bool get canDownload => true;

  File _fileOf(Track track) => File('${cache.path}/${track.partId}.audio');

  @override
  bool isDownloaded(Track track) => track.partId != null && _fileOf(track).existsSync();

  @override
  Future<void> removeDownload(Track track) async {
    if (track.partId == null) return;
    final file = _fileOf(track);
    if (file.existsSync()) await file.delete();
  }

  @override
  Future<({int files, int bytes})> storageUsed() async {
    if (!cache.existsSync()) return (files: 0, bytes: 0);
    var files = 0, bytes = 0;
    await for (final e in cache.list()) {
      // Parts only; a half-loaded one (`.part`) is counted too, it takes room.
      if (e is File) {
        files++;
        bytes += await e.length();
      }
    }
    return (files: files, bytes: bytes);
  }

  @override
  Future<void> removeAllDownloads() async {
    if (!cache.existsSync()) return;
    await for (final e in cache.list()) {
      if (e is File) await e.delete();
    }
  }

  @override
  Future<String> fetchPart(Track track) async {
    final id = track.partId!;
    final file = File('${cache.path}/$id.audio');
    if (!file.existsSync()) await api.downloadPart(id, file);
    return file.path;
  }

  @override
  Future<void> saveProgress(Book book, Track track, Duration position, {required bool finished}) {
    final partId = track.partId;
    if (partId == null) return Future.value();
    return api.putProgress(itemId: int.parse(book.id), partId: partId, position: position, finished: finished);
  }

  @override
  Future<PassageHit?> resolvePassage(String text) async {
    final List<Map<String, dynamic>> matches;
    try {
      matches = await api.resolvePassage(text);
    } on ApiException catch (e) {
      // Text that names no passage is no match, not a failure.
      if (e.code == 'invalid_reference') return null;
      rethrow;
    }
    if (matches.isEmpty) return null;
    final m = matches.first;
    return PassageHit(
      '${m['item_id']}',
      m['part_id'] as int,
      Duration(milliseconds: m['position_ms'] as int),
      allowed: (m['access'] as Map)['state'] == 'allowed',
    );
  }
}

/// The library of the signed-in user as the app's models, and where to resume.
class LoadedLibrary {
  LoadedLibrary(this.books, this.resume, {this.lastBookId, this.lastAt = 0, this.userName, this.recent = const []});

  /// The ids of the books listened to, the latest first (what "continue listening" says), at most 20.

  /// The nickname of the signed-in user, for the greeting; null if the server did not tell.

  /// The book the user listened to last on any device, and when (ms since 1970), from the server.
  final String? lastBookId;
  final int lastAt;
  final String? userName;
  final List<String> recent;

  final List<Book> books;
  final Map<String, ({int track, Duration position})> resume;
}

const _palette = [Color(0xFF6E1F2B), Color(0xFF1F2F4D), Color(0xFF2C4A3E), Color(0xFF4A3B1F), Color(0xFF3B2A4A)];

Future<LoadedLibrary> loadLibrary(ApiClient api) async {
  final items = await api.items();
  String? userName;
  try {
    userName = (await api.me())['nickname'] as String?;
  } on Object {
    // the greeting works without a name
  }
  // The most recently listened book first; a failure here only means no "last book" is known.
  String? lastBookId;
  var lastAt = 0;
  var recentIds = <String>[];
  try {
    final recent = await api.continueListening();
    recentIds = [for (final r in recent) '${r['item_id']}'];
    if (recent.isNotEmpty) {
      lastBookId = '${recent.first['item_id']}';
      lastAt = recent.first['updated_at'] as int;
    }
  } on Object {
    // not needed to show the library
  }
  final details = await Future.wait(items.map((i) => api.item(i['id'] as int)));
  // The pictures come alongside; an item without one, or one that cannot be fetched, is drawn.
  final covers = await Future.wait(items.map((i) async {
    try {
      return await api.cover(i['id'] as int);
    } on ApiException {
      return null;
    }
  }));
  final coverOf = {for (var i = 0; i < items.length; i++) items[i]['id'] as int: covers[i]};
  final books = <Book>[];
  final resume = <String, ({int track, Duration position})>{};
  for (final d in details) {
    final id = d['id'] as int;
    final parts = (d['parts'] as List).cast<Map<String, dynamic>>();
    if (parts.isEmpty) continue;
    books.add(Book(
      id: '$id',
      title: d['title'] as String,
      author: d['creator'] as String,
      color: _palette[id % _palette.length],
      cover: coverOf[id],
      blurb: d['description'] as String?,
      kind: d['kind'] as String? ?? 'audiobook',
      narrator: d['narrator'] as String?,
      tracks: [
        for (final p in parts)
          Track(
            disc: p['disc'] as int,
            number: p['number'] as int,
            title: p['title'] as String,
            length: Duration(milliseconds: p['duration_ms'] as int),
            partId: p['id'] as int,
          ),
      ],
    ));
    final progress = d['progress'];
    if (progress is Map && progress['finished'] != true) {
      final index = parts.indexWhereId(progress['part_id'] as int);
      if (index >= 0) {
        resume['$id'] = (track: index, position: Duration(milliseconds: progress['position_ms'] as int));
      }
    }
  }
  return LoadedLibrary(books, resume, lastBookId: lastBookId, lastAt: lastAt, userName: userName, recent: recentIds);
}

extension on List<Map<String, dynamic>> {
  int indexWhereId(int id) => indexWhere((p) => p['id'] == id);
}
