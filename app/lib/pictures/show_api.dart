// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import '../server/api_client.dart';

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
  Future<void> download(ShowChoice choice, File dest, {void Function(int bytes)? progress}) async {
    final res = await api.sendRaw('GET', '/api/v1/pictures/project', query: choice.query());
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
