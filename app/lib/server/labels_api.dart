// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';

import '../labels.dart';
import 'api_client.dart';

/// The labels of a book, a podcast and a radio station on the server (`PUT /api/v1/items/{id}/labels`, `/podcasts/{id}/labels`, `/radio/{id}/labels`).
class ServerLabelApi implements LabelApi {
  ServerLabelApi(this.api);

  final ApiClient api;

  @override
  Future<List<String>> setLabels(int id, List<String> labels) async {
    final res = await api.sendRaw('PUT', '/api/v1/items/$id/labels', body: {'labels': labels});
    final text = await utf8.decodeStream(res);
    if (res.statusCode >= 400) {
      var code = 'error';
      try {
        code = (jsonDecode(text) as Map<String, dynamic>)['error'] as String? ?? code;
      } on FormatException {
        // not JSON: the generic code
      }
      throw ApiException(res.statusCode, code);
    }
    return [
      for (final l in (jsonDecode(text) as Map<String, dynamic>)['labels'] as List) l as String,
    ];
  }

  @override
  Future<List<String>> setTrackLabels(int id, int partId, List<String> labels) async {
    final (_, text) = await _call('PUT', '/api/v1/items/$id/parts/$partId/labels', body: {'labels': labels});
    return [
      for (final l in (jsonDecode(text) as Map<String, dynamic>)['labels'] as List) l as String,
    ];
  }

  @override
  Future<List<String>> setPodcastLabels(int id, List<String> labels) async {
    final (_, text) = await _call('PUT', '/api/v1/podcasts/$id/labels', body: {'labels': labels});
    return [for (final l in (jsonDecode(text) as Map<String, dynamic>)['labels'] as List) l as String];
  }

  @override
  Future<List<String>> setStationLabels(int id, List<String> labels) async {
    final (_, text) = await _call('PUT', '/api/v1/radio/$id/labels', body: {'labels': labels});
    return [for (final l in (jsonDecode(text) as Map<String, dynamic>)['labels'] as List) l as String];
  }

  Future<(String, String)> _call(String method, String path, {Object? body}) async {
    final res = await api.sendRaw(method, path, body: body);
    final text = await utf8.decodeStream(res);
    if (res.statusCode >= 400) {
      var code = 'error';
      try {
        code = (jsonDecode(text) as Map<String, dynamic>)['error'] as String? ?? code;
      } on FormatException {
        // not JSON: the generic code
      }
      throw ApiException(res.statusCode, code);
    }
    return (res.statusCode.toString(), text);
  }

  @override
  Future<LabelList> list() async {
    final (_, text) = await _call('GET', '/api/v1/labels');
    final j = jsonDecode(text) as Map<String, dynamic>;
    return LabelList(
      [
        for (final l in (j['labels'] as List? ?? const []))
          // `count` adds books, podcasts and stations; an older server only has `books`.
          ((l as Map<String, dynamic>)['name'] as String, (l['count'] ?? l['books']) as int),
      ],
      j['can_manage'] == true,
    );
  }

  @override
  Future<void> rename(String name, String to) async {
    await _call('PUT', '/api/v1/labels/${Uri.encodeComponent(name)}', body: {'name': to});
  }

  @override
  Future<void> delete(String name) async {
    await _call('DELETE', '/api/v1/labels/${Uri.encodeComponent(name)}');
  }
}
