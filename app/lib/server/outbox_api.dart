// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';

import '../offline/outbox.dart';
import 'api_client.dart';

/// The replay of the changes made without the server: label deltas (`PATCH`), hiding (`POST /api/v1/me/hidden`, sets a state) and the listening
/// (`PUT /api/v1/progress` and `PUT /api/v1/episodes/{id}/state`, both with the time of the listening).
class ServerOutboxApi implements OutboxApi {
  ServerOutboxApi(this.api);

  final ApiClient api;

  Future<void> _call(String method, String path, Object body) async {
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
  }

  @override
  Future<void> labels(String scope, int id, {int? part, required List<String> add, required List<String> remove}) {
    final path = switch (scope) {
      'book' => '/api/v1/items/$id/labels',
      'part' => '/api/v1/items/$id/parts/$part/labels',
      'podcast' => '/api/v1/podcasts/$id/labels',
      'station' => '/api/v1/radio/$id/labels',
      _ => throw ArgumentError('unknown scope $scope'),
    };
    return _call('PATCH', path, {'add': add, 'remove': remove});
  }

  @override
  Future<void> hideBook(int id, bool hidden) => _call('POST', '/api/v1/me/hidden', {'scope': 'book', 'id': id, 'hidden': hidden});

  @override
  Future<void> hideAuthor(String name, String kind, bool hidden) =>
      _call('POST', '/api/v1/me/hidden', {'scope': 'author', 'name': name, if (kind.isNotEmpty) 'kind': kind, 'hidden': hidden});

  @override
  Future<void> episodeState({required int id, required int positionMs, required bool finished, required int at}) =>
      _call('PUT', '/api/v1/episodes/$id/state', {'position_ms': positionMs, 'finished': finished, 'at': at});

  @override
  Future<void> progress({required int item, required int part, required int positionMs, required bool finished, required int at}) =>
      _call('PUT', '/api/v1/progress', {'item_id': item, 'part_id': part, 'position_ms': positionMs, 'finished': finished, 'at': at});
}
