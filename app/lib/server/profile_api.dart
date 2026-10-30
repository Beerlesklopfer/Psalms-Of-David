// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:typed_data';

import '../profile.dart';
import 'api_client.dart';

/// The personal profile on the server (`GET/PUT /api/v1/me`, `PUT /me/pin`, `GET /me/devices`, `GET/PUT/DELETE /me/avatar`).
class ServerProfileApi implements ProfileApi {
  ServerProfileApi(this.api);

  final ApiClient api;

  Future<(HttpResponse, String)> _call(String method, String path, {Object? body}) async {
    final res = await api.sendRaw(method, path, body: body);
    final text = await utf8.decodeStream(res);
    return (HttpResponse(res.statusCode), text);
  }

  Never _fail(int status, String text) {
    var code = 'error';
    try {
      code = (jsonDecode(text) as Map<String, dynamic>)['error'] as String? ?? code;
    } on FormatException {
      // not JSON: the generic code
    }
    throw ApiException(status, code);
  }

  Future<Profile> _profile(String method, String path, {Object? body}) async {
    final (res, text) = await _call(method, path, body: body);
    if (res.status >= 400) _fail(res.status, text);
    return Profile.fromJson(jsonDecode(text) as Map<String, dynamic>);
  }

  @override
  Future<Profile> get() => _profile('GET', '/api/v1/me');

  @override
  Future<Profile> update({String? nickname, String? firstName, String? lastName, String? language}) => _profile('PUT', '/api/v1/me', body: {
        'nickname': ?nickname,
        'first_name': ?firstName,
        'last_name': ?lastName,
        'language': ?language,
      });

  @override
  Future<void> changePin(String current, String next) async {
    final (res, text) = await _call('PUT', '/api/v1/me/pin', body: {'current': current, 'new': next});
    if (res.status >= 400) _fail(res.status, text);
  }

  @override
  Future<List<ProfileDevice>> devices() async {
    final (res, text) = await _call('GET', '/api/v1/me/devices');
    if (res.status >= 400) _fail(res.status, text);
    return [
      for (final d in jsonDecode(text) as List)
        ProfileDevice(
          id: (d as Map<String, dynamic>)['id'] as int,
          name: d['name'] as String? ?? '',
          shared: d['shared'] == true,
          lastSeen: d['last_seen'] is int ? DateTime.fromMillisecondsSinceEpoch(d['last_seen'] as int) : null,
          isThis: d['this'] == true,
        ),
    ];
  }

  @override
  Future<Uint8List?> avatar() async {
    final res = await api.sendRaw('GET', '/api/v1/me/avatar');
    if (res.statusCode == 404) {
      await res.drain<void>();
      return null;
    }
    if (res.statusCode >= 400) _fail(res.statusCode, await utf8.decodeStream(res));
    final builder = BytesBuilder(copy: false);
    await for (final chunk in res) {
      builder.add(chunk);
    }
    return builder.takeBytes();
  }

  @override
  Future<Profile> putAvatar(Uint8List jpeg) async {
    final res = await api.sendBytes('PUT', '/api/v1/me/avatar', jpeg, contentType: 'image/jpeg');
    final text = await utf8.decodeStream(res);
    if (res.statusCode >= 400) _fail(res.statusCode, text);
    return Profile.fromJson(jsonDecode(text) as Map<String, dynamic>);
  }

  @override
  Future<Profile> deleteAvatar() => _profile('DELETE', '/api/v1/me/avatar');
}

/// Only the status of a response, so that the text can be read before it is judged.
class HttpResponse {
  const HttpResponse(this.status);

  final int status;
}
