// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';

import '../library_hiding.dart';
import 'api_client.dart';

/// The own library on the server (`/api/v1/me/library`, `/api/v1/me/hidden`).
class ServerHidingApi implements HidingApi {
  ServerHidingApi(this.api);

  final ApiClient api;

  Future<HiddenState> _call(String method, String path, {Object? body}) async {
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
    return HiddenState.fromJson(jsonDecode(text) as Map<String, dynamic>);
  }

  @override
  Future<HiddenState> state() => _call('GET', '/api/v1/me/library');

  @override
  Future<HiddenState> setShowHidden(bool show) =>
      _call('PUT', '/api/v1/me/library', body: {'show_hidden': show});

  @override
  Future<HiddenState> hideBook(int id, bool hidden) =>
      _call('POST', '/api/v1/me/hidden', body: {'scope': 'book', 'id': id, 'hidden': hidden});

  @override
  Future<HiddenState> hideAuthor(String name, String kind, bool hidden) => _call(
    'POST',
    '/api/v1/me/hidden',
    body: {'scope': 'author', 'name': name, if (kind.isNotEmpty) 'kind': kind, 'hidden': hidden},
  );
}
