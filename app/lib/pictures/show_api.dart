// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import '../server/api_client.dart';
import 'show_project.dart';

/// A source of pictures the person may choose (`GET /api/v1/pictures/sources`): `own`, `p<id>` a child, `f:<folder>` an old folder.
class ShowSource {
  const ShowSource({required this.id, required this.kind, required this.name, required this.count, this.first, this.last});

  factory ShowSource.fromJson(Map<String, dynamic> j) => ShowSource(
        id: '${j['id']}',
        kind: '${j['kind']}',
        name: '${j['name'] ?? ''}',
        count: j['count'] is int ? j['count'] as int : 0,
        first: j['first'] is int ? DateTime.fromMillisecondsSinceEpoch(j['first'] as int) : null,
        last: j['last'] is int ? DateTime.fromMillisecondsSinceEpoch(j['last'] as int) : null,
      );

  final String id;
  final String kind;
  final String name;
  final int count;
  final DateTime? first;
  final DateTime? last;
}

/// What a slideshow is made of, as the page `/pictures` chooses it.
class ShowChoice {
  const ShowChoice({
    required this.sources,
    this.fromYear,
    this.toYear,
    this.choices = const {},
    this.music,
  });

  final List<String> sources;
  final int? fromYear;
  final int? toYear;

  /// `order`, `seconds`, `transition`, `videos`, `info` (`show_texts.dart`).
  final Map<String, String> choices;

  /// The id of an own playlist, or null for none.
  final int? music;

  /// The query of `GET /api/v1/pictures/project`: the same as the list of the pictures, and the choices.
  Map<String, Object> query() => {
        'source': sources,
        if (fromYear != null) 'from': '${DateTime(fromYear!).millisecondsSinceEpoch}',
        if (toYear != null) 'to': '${DateTime(toYear! + 1).millisecondsSinceEpoch - 1}',
        if (choices['videos'] != 'off') 'videos': 'on',
        ...choices,
        if (music != null) 'music': '$music',
      };
}

/// The pictures of the server for a slideshow (`server/src/api_pictures.rs`).
class ShowApi {
  ShowApi(this.api);

  final ApiClient api;

  Future<List<ShowSource>> sources() async {
    final res = await api.sendRaw('GET', '/api/v1/pictures/sources');
    final text = await utf8.decodeStream(res);
    if (res.statusCode >= 400) throw ApiException(res.statusCode, 'error');
    final j = jsonDecode(text) as Map<String, dynamic>;
    return [for (final s in (j['sources'] as List? ?? const []).whereType<Map<String, dynamic>>()) ShowSource.fromJson(s)];
  }

  /// Loads the project of [choice] into [dest] (a ZIP); [progress] hears the bytes so far. The file gets its name only when it is complete.
  /// The server writes the archive while it sends it, so the length is not known beforehand, and the first time a video can take a while.
  Future<void> download(ShowChoice choice, File dest, {void Function(int bytes)? progress}) =>
      _project(choice.query(), dest, progress: progress);

  /// Loads a project put together on the server ([ServerShow]) into [dest] (a ZIP), as [download].
  Future<void> downloadShow(int id, File dest, {void Function(int bytes)? progress}) => _project({'show': '$id'}, dest, progress: progress);

  Future<Object?> _json(String path) async {
    final res = await api.sendRaw('GET', path);
    final text = await utf8.decodeStream(res);
    if (res.statusCode >= 400) throw ApiException(res.statusCode, res.statusCode == 404 ? 'not_found' : 'error');
    return jsonDecode(text);
  }

  /// The projects put together on the server by this person, the one changed last first.
  Future<List<ServerShow>> shows() async {
    final j = await _json('/api/v1/pictures/shows');
    final list = j is Map<String, dynamic> && j['shows'] is List ? j['shows'] as List : const [];
    return [
      for (final s in list.whereType<Map<String, dynamic>>())
        if (s['id'] is int) ServerShow(id: s['id'] as int, title: '${s['title'] ?? ''}', pictures: s['pictures'] is int ? s['pictures'] as int : 0, rev: '${s['rev'] ?? ''}'),
    ];
  }

  /// A project of the server to play from there; its files are fetched into [cache] while it plays.
  Future<ShowProject> stream(int id, Directory cache) async {
    final j = await _json('/api/v1/pictures/shows/$id');
    if (j is! Map<String, dynamic>) throw ApiException(500, 'error');
    await cache.create(recursive: true);
    return ShowProject.remote(j, _Remote(this, cache));
  }

  /// Writes the answer of [path] to [dest]; the file gets its name only when it is complete.
  Future<void> fetchTo(String path, File dest, {Map<String, Object>? query}) async {
    final res = await api.sendRaw('GET', path, query: query);
    if (res.statusCode != 200) {
      await res.drain<void>();
      throw ApiException(res.statusCode, 'error');
    }
    final partial = partialFileFor(dest);
    final sink = partial.openWrite();
    try {
      await res.timeout(const Duration(minutes: 5)).pipe(sink);
    } catch (_) {
      await sink.close().catchError((_) {});
      if (partial.existsSync()) partial.deleteSync();
      rethrow;
    }
    await partial.rename(dest.path);
  }

  Future<void> _project(Map<String, Object> query, File dest, {void Function(int bytes)? progress}) async {
    final res = await api.sendRaw('GET', '/api/v1/pictures/project', query: query);
    if (res.statusCode != 200) {
      await res.drain<void>();
      throw ApiException(res.statusCode, res.statusCode == 404 ? 'empty' : 'error');
    }
    await dest.parent.create(recursive: true);
    final partial = partialFileFor(dest);
    final sink = partial.openWrite();
    var bytes = 0;
    try {
      // A server that turns a long video into an MP4 is silent for that time; only a much longer silence counts as broken.
      await for (final chunk in res.timeout(const Duration(minutes: 10))) {
        sink.add(chunk);
        bytes += chunk.length;
        progress?.call(bytes);
      }
      await sink.close();
      if (bytes == 0) throw const HttpException('the project is empty');
    } catch (_) {
      await sink.close().catchError((_) {});
      if (partial.existsSync()) partial.deleteSync();
      rethrow;
    }
    await partial.rename(dest.path);
  }
}

/// A project put together on the server (the web editor of the pictures): its id, its title, how many pictures it plays and the mark of
/// its version, which changes with every change.
class ServerShow {
  const ServerShow({required this.id, required this.title, required this.pictures, required this.rev});

  final int id;
  final String title;
  final int pictures;
  final String rev;
}

/// The files of a project played from the server: fetched one by one into the cache, each once; the next ones while one is shown.
class _Remote implements ShowRemote {
  _Remote(this.show, this.cache);

  final ShowApi show;
  @override
  final Directory cache;
  final Map<String, Future<String>> _files = {};

  @override
  Future<String> fetch(String name) => _files.putIfAbsent(name, () async {
        final cut = name.indexOf('/');
        final (kind, id) = (name.substring(0, cut), name.substring(cut + 1));
        final file = File('${cache.path}/$kind-$id');
        if (file.existsSync()) return file.path;
        try {
          if (kind == 'part') {
            // The music as an MP3, which every device plays.
            await show.fetchTo('/api/v1/parts/$id/stream', file, query: {'as': 'mp3'});
          } else {
            await show.fetchTo('/api/v1/pictures/$id/$kind', file);
          }
        } catch (_) {
          // Not kept: the next try asks the server again.
          _files.remove(name);
          rethrow;
        }
        return file.path;
      });

  @override
  Future<String?> place(String key) async {
    try {
      final j = await show._json('/api/v1/pictures/$key/info');
      final p = j is Map<String, dynamic> ? j['place'] : null;
      return p is String && p.isNotEmpty ? p : null;
    } on Object {
      return null;
    }
  }

  @override
  Future<void> dispose() async {
    try {
      if (cache.existsSync()) await cache.delete(recursive: true);
    } on FileSystemException {
      // Left for the next tidying.
    }
  }
}
