// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';
import 'dart:ui';

import '../m3u.dart';
import '../models.dart';
import '../podcast_downloads.dart';
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

  /// Writes the playlist file [name] (an extended M3U) next to the downloaded parts, listing [entries]; an
  /// empty list removes the file. Nothing happens where parts are not kept on the device.
  Future<void> writeM3u(String name, List<M3uEntry> entries) async {}

  /// The path of a downloaded part relative to the folder of the playlist file, or null if it is not on the device.
  String? relativePath(Track track) => null;
}

class ServerSource implements MediaSource {
  ServerSource(this.api, this.cache);

  final ApiClient api;

  /// Downloaded parts. Named by part id, so a part is fetched once.
  final Directory cache;

  @override
  bool get canDownload => true;

  File _fileOf(Track track) => File('${cache.path}/${track.partId}.audio');

  /// The file of a track: a part of the server by its id, an episode by its address (a radio stream has none).
  File? _anyFileOf(Track track) {
    final url = track.url;
    if (url != null) return track.live ? null : episodeCacheFile(cache, url);
    return track.partId == null ? null : _fileOf(track);
  }

  @override
  bool isDownloaded(Track track) => _anyFileOf(track)?.existsSync() ?? false;

  @override
  Future<void> removeDownload(Track track) async {
    final file = _anyFileOf(track);
    if (file != null && file.existsSync()) await file.delete();
  }

  @override
  Future<({int files, int bytes})> storageUsed() async {
    if (!cache.existsSync()) return (files: 0, bytes: 0);
    var files = 0, bytes = 0;
    await for (final e in cache.list()) {
      // Parts only; a half-loaded one (`.part`) is counted too, it takes room.
      if (e is File && !e.path.endsWith('.$kM3uEnding')) {
        files++;
        bytes += await e.length();
      }
    }
    return (files: files, bytes: bytes);
  }

  @override
  String? relativePath(Track track) => isDownloaded(track) ? '${track.partId}.audio' : null;

  @override
  Future<void> writeM3u(String name, List<M3uEntry> entries) async {
    final file = File('${cache.path}/$name.$kM3uEnding');
    if (entries.isEmpty) {
      if (file.existsSync()) await file.delete();
      return;
    }
    await cache.create(recursive: true);
    final temp = File('${file.path}.part');
    await temp.writeAsString(buildM3u8(entries));
    await temp.rename(file.path);
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
    final url = track.url;
    if (url != null) {
      // A radio stream is played from its address; an episode is fetched from the publisher once and kept.
      if (track.live) return url;
      final file = episodeCacheFile(cache, url);
      if (!file.existsSync()) {
        // An episode that is on the device (downloaded, or played before) is played from there, whether the server is reachable or not.
        if (!isWebAddress(url)) throw HttpException('not an http or https address', uri: Uri.tryParse(url));
        await _downloadFrom(url, file);
      }
      return file.path;
    }
    final id = track.partId!;
    final file = File('${cache.path}/$id.audio');
    if (!file.existsSync()) await api.downloadPart(id, file);
    return file.path;
  }

  /// Loads a file from the internet into [file] (through a `.part` file, so that a half-loaded file is never played).
  /// The publisher's own certificate rules apply: this is no connection to David.
  Future<void> _downloadFrom(String url, File file) async {
    final client = HttpClient()..connectionTimeout = const Duration(seconds: 20);
    try {
      // Redirects are followed by the client (publishers send episodes through trackers and content networks).
      final req = await client.getUrl(Uri.parse(url));
      final res = await req.close();
      if (res.statusCode >= 400) throw HttpException('HTTP ${res.statusCode}', uri: Uri.parse(url));
      await file.parent.create(recursive: true);
      final part = File('${file.path}.part');
      final sink = part.openWrite();
      try {
        await res.pipe(sink);
      } on Object {
        if (part.existsSync()) await part.delete();
        rethrow;
      }
      await part.rename(file.path);
    } finally {
      client.close(force: true);
    }
  }

  @override
  Future<void> saveProgress(Book book, Track track, Duration position, {required bool finished}) {
    // An episode goes back to the account's own state; a radio station has no position.
    if (book.kind == 'podcast') return api.putEpisodeState(int.parse(book.id.substring(3)), position, finished: finished);
    if (book.isStream) return Future.value();
    final partId = track.partId;
    if (partId == null) return Future.value();
    // A track in a playlist reports to the book it belongs to, as it does when played from the book.
    final itemId = track.itemId ?? book.id;
    return api.putProgress(itemId: int.parse(itemId), partId: partId, position: position, finished: finished);
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
  LoadedLibrary(this.books, this.resume, {this.lastBookId, this.lastAt = 0, this.userName, this.recent = const [], this.genres = const [], this.newIds = const []});

  /// The ids of the books listened to, the latest first (what "continue listening" says), at most 20.

  /// The nickname of the signed-in user, for the greeting; null if the server did not tell.

  /// The book the user listened to last on any device, and when (ms since 1970), from the server.
  final String? lastBookId;
  final int lastAt;
  final String? userName;
  final List<String> recent;

  /// The ids of the books that are new for this user, in the order of the server (newest first).
  final List<String> newIds;

  /// The genres of the list, in the order of the administration (music is shown by them).
  final List<String> genres;

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
  var genres = <String>[];
  try {
    genres = await api.genres();
  } on Object {
    // an older server has no list; the genres then follow the albums
  }
  var newIds = <String>[];
  try {
    newIds = [for (final id in await api.newItems()) '$id'];
  } on Object {
    // the "New" row is optional
  }
  final hiddenIds = {for (final i in items) if (i['hidden'] == true) i['id'] as int};
  final ownerOf = {for (final i in items) if (i['owner'] is String) i['id'] as int: i['owner'] as String};
  final labelsOf = {
    for (final i in items)
      i['id'] as int: [for (final l in (i['labels'] as List? ?? const [])) l as String],
  };
  final trackLabelsOf = {
    for (final i in items)
      i['id'] as int: [for (final l in (i['track_labels'] as List? ?? const [])) l as String],
  };
  final canLabel = {for (final i in items) if (i['can_label'] == true) i['id'] as int};
  final canShare = {for (final i in items) if (i['can_share'] == true) i['id'] as int};
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
      genre: d['genre'] as String?,
      plays: d['plays'] as int? ?? 0,
      lastPlayedAt: dateFromMs(d['last_played_at']),
      hidden: hiddenIds.contains(id),
      owner: ownerOf[id],
      labels: labelsOf[id] ?? const [],
      canLabel: canLabel.contains(id),
      canShare: canShare.contains(id),
      trackLabels: trackLabelsOf[id] ?? const [],
      tracks: [
        for (final p in parts)
          Track(
            disc: p['disc'] as int,
            number: p['number'] as int,
            title: p['title'] as String,
            length: Duration(milliseconds: p['duration_ms'] as int),
            partId: p['id'] as int,
            plays: p['plays'] as int? ?? 0,
            lastPlayedAt: dateFromMs(p['last_played_at']),
            labels: [for (final l in (p['labels'] as List? ?? const [])) l as String],
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
  return LoadedLibrary(books, resume, lastBookId: lastBookId, lastAt: lastAt, userName: userName, recent: recentIds, genres: genres, newIds: newIds);
}

extension on List<Map<String, dynamic>> {
  int indexWhereId(int id) => indexWhere((p) => p['id'] == id);
}
