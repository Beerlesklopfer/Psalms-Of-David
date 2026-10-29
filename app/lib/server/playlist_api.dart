// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io';

import '../playlists.dart';
import 'api_client.dart';

/// The playlists of the server (`/api/v1/playlists`, `docs/spec-playlists.md`).
class ServerPlaylistApi implements PlaylistApi {
  ServerPlaylistApi(this.api);

  final ApiClient api;

  Future<Object?> _call(
    String method,
    String path, {
    Object? body,
    Map<String, String>? headers,
  }) async {
    final res = await api.sendRaw(method, path, body: body, headers: headers);
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
    return text.isEmpty ? null : jsonDecode(text);
  }

  @override
  Future<PlaylistListAnswer> playlists({String? etag}) async {
    final res = await api.sendRaw('GET', '/api/v1/playlists', headers: {'If-None-Match': ?etag});
    final text = await utf8.decodeStream(res);
    if (res.statusCode == HttpStatus.notModified) return PlaylistListAnswer(null, etag);
    if (res.statusCode >= 400) {
      var code = 'error';
      try {
        code = (jsonDecode(text) as Map<String, dynamic>)['error'] as String? ?? code;
      } on FormatException {
        // not JSON
      }
      throw ApiException(res.statusCode, code);
    }
    return PlaylistListAnswer([
      for (final l in (jsonDecode(text) as List).cast<Map<String, dynamic>>())
        PlaylistSummary.fromJson(l),
    ], res.headers.value(HttpHeaders.etagHeader));
  }

  @override
  Future<Playlist> playlist(int id) async =>
      Playlist.fromJson(await _call('GET', '/api/v1/playlists/$id') as Map<String, dynamic>);

  @override
  Future<({int id, String name})> createPlaylist(String name) async {
    final j =
        await _call('POST', '/api/v1/playlists', body: {'name': name}) as Map<String, dynamic>;
    return (id: j['id'] as int, name: j['name'] as String);
  }

  @override
  Future<void> renamePlaylist(int id, String name) async {
    await _call('PATCH', '/api/v1/playlists/$id', body: {'name': name});
  }

  @override
  Future<void> deletePlaylist(int id) async {
    await _call('DELETE', '/api/v1/playlists/$id');
  }

  @override
  Future<Playlist> addParts(int id, List<int> partIds, {int? at}) async => Playlist.fromJson(
    await _call('POST', '/api/v1/playlists/$id/items', body: {'part_ids': partIds, 'at': ?at})
        as Map<String, dynamic>,
  );

  @override
  Future<Playlist> addBook(int id, int bookId, {int? at}) async => Playlist.fromJson(
    await _call('POST', '/api/v1/playlists/$id/items', body: {'book_id': bookId, 'at': ?at})
        as Map<String, dynamic>,
  );

  @override
  Future<void> removeEntry(int id, int entryId) async {
    await _call('DELETE', '/api/v1/playlists/$id/items/$entryId');
  }

  @override
  Future<Playlist> reorder(int id, List<int> entryIds) async => Playlist.fromJson(
    await _call('PUT', '/api/v1/playlists/$id/order', body: {'item_ids': entryIds})
        as Map<String, dynamic>,
  );
}
