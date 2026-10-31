// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';
import 'dart:ui';

import '../audio/file_formats.dart';
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

  /// The file of a part: named by its id; an MP3 that the server made for a platform that cannot play the format has its own name, so both can coexist.
  File _fileOf(Track track) {
    final mp3 = needsTranscode(track.format, track.transcode);
    final ending = audioFileEnding(format: track.format, mime: track.mime, mp3: mp3);
    final file = File('${cache.path}/${track.partId}${mp3 ? '-mp3' : ''}.$ending');
    // A part loaded by an earlier version is called `<id>.audio`: it keeps its place (renamed once, no new download).
    if (ending != 'audio' && !file.existsSync()) {
      final old = File('${cache.path}/${track.partId}${mp3 ? '-mp3' : ''}.audio');
      try {
        if (old.existsSync()) old.renameSync(file.path);
      } on FileSystemException {
        // stays where it is; the part is loaded again
      }
    }
    return file;
  }

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
  String? relativePath(Track track) => isDownloaded(track) ? _fileOf(track).uri.pathSegments.last : null;

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

  /// The downloads of this source that are running, by the path of their file. The player loads a part, fetches the next
  /// one ahead and downloads a whole book for listening offline, all through the one source of the player, and these can
  /// ask for the same file at the same time: the second one waits for the first instead of writing the same file again.
  final Map<String, Future<void>> _running = {};

  /// Deletes the half-loaded files (`*.part`) that a run left behind when it was closed during a download. Every download
  /// writes a file of its own (see [partialFileFor]), so a later one does not reuse them; called once at the start, before
  /// anything is downloaded.
  static void dropPartials(Directory cache) {
    try {
      if (!cache.existsSync()) return;
      for (final e in cache.listSync()) {
        if (e is File && e.path.endsWith('.part')) e.deleteSync();
      }
    } on FileSystemException {
      // Left for the next start.
    }
  }

  /// Runs [download] for [file] unless the same file is being downloaded already; then it waits for that one.
  Future<void> _once(File file, Future<void> Function() download) {
    final running = _running[file.path];
    if (running != null) return running;
    // A block, not an arrow: `remove` returns the removed future, and a future returned to `whenComplete` is waited
    // for, so the download would wait for itself and never end.
    final future = download().whenComplete(() {
      _running.remove(file.path);
    });
    _running[file.path] = future;
    return future;
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
        await _once(file, () => _downloadFrom(url, file));
      }
      return file.path;
    }
    final id = track.partId!;
    final file = _fileOf(track);
    if (!file.existsSync()) await _once(file, () => api.downloadPart(id, file, asMp3: needsTranscode(track.format, track.transcode)));
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
      final part = partialFileFor(file);
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

/// One member of the family and the books they listened to lately, the latest first.
class FamilyMember {
  const FamilyMember({required this.id, required this.name, required this.bookIds});

  final int id;
  final String name;
  final List<String> bookIds;
}

/// The media time of one member of the family: the milliseconds listened on each of the last [days] days (the last is today), per kind of
/// medium (`audiobook`, `music`, `podcast`) and together.
class FamilyTime {
  const FamilyTime({required this.id, required this.name, required this.me, required this.fromDay, required this.kinds, required this.total});

  final int id;
  final String name;

  /// The member is the signed-in user.
  final bool me;

  /// The first day as days since 1970-01-01 (UTC).
  final int fromDay;
  final Map<String, List<int>> kinds;
  final List<int> total;
}

/// The library of the signed-in user as the app's models, and where to resume.
class LoadedLibrary {
  LoadedLibrary(this.books, this.resume, {this.lastBookId, this.lastAt = 0, this.userName, this.recent = const [], this.genres = const [], this.newIds = const [], this.family = const [], this.heard = const [], this.shareListening = true, this.mayHideListening = true});

  /// What the other members of the family listened to lately (the rows of the home page); empty for a server that does not tell.
  final List<FamilyMember> heard;

  /// The media time of the family (the charts of the home page); empty for a server that does not tell.
  final List<FamilyTime> family;

  /// Whether the family sees what this user listens to, and whether the user may turn that off (an administrator decides the latter).
  final bool shareListening;
  final bool mayHideListening;

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
  var shareListening = true;
  var mayHideListening = true;
  try {
    final me = await api.me();
    userName = me['nickname'] as String?;
    shareListening = me['share_listening'] != false;
    mayHideListening = me['may_hide_listening'] != false;
  } on Object {
    // the greeting works without a name
  }
  var heard = <FamilyMember>[];
  try {
    heard = [
      for (final m in await api.familyListening())
        FamilyMember(id: m['user_id'] as int, name: '${m['name']}', bookIds: [for (final i in m['items'] as List) '${(i as Map)['item_id']}']),
    ];
  } on Object {
    // an older server has no such list; the rows are optional
  }
  var family = <FamilyTime>[];
  try {
    final time = await api.familyMediaTime();
    List<int> ints(Object? v) => [for (final n in v as List) n as int];
    family = [
      for (final m in time['members'] as List)
        FamilyTime(
          id: (m as Map)['user_id'] as int,
          name: '${m['name']}',
          me: m['me'] == true,
          fromDay: time['from_day'] as int,
          kinds: {for (final e in (m['kinds'] as Map).entries) '${e.key}': ints(e.value)},
          total: ints(m['total']),
        ),
    ];
  } on Object {
    // an older server has no charts; they are optional
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
      barcode: d['barcode'] as String?,
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
            format: p['format'] as String?,
            mime: p['mime'] as String?,
            transcode: p['transcode'] as String?,
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
  return LoadedLibrary(books, resume, lastBookId: lastBookId, lastAt: lastAt, userName: userName, recent: recentIds, genres: genres, newIds: newIds, family: family, heard: heard, shareListening: shareListening, mayHideListening: mayHideListening);
}

extension on List<Map<String, dynamic>> {
  int indexWhereId(int id) => indexWhere((p) => p['id'] == id);
}
