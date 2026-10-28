// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:math';
import 'dart:typed_data';

import 'package:crypto/crypto.dart';

/// An answer of the server that is not a success. [code] is the server's error
/// code (`outside_schedule`, `not_found`, ...) or `network` if there was no answer.
class ApiException implements Exception {
  ApiException(this.status, this.code);

  final int status;
  final String code;

  @override
  String toString() => 'ApiException($status, $code)';
}

/// The contents of a pairing code: `david://pair?v=1&id=..&addr=..&fp=..&code=..`.
class PairingAddress {
  PairingAddress({required this.serverId, required this.address, required this.fingerprint, required this.code});

  final String serverId;
  final String address;

  /// SHA-256 of the server certificate, lower case hex; null for a server without TLS.
  final String? fingerprint;
  final String code;

  static PairingAddress? parse(String text) {
    final uri = Uri.tryParse(text.trim());
    if (uri == null || uri.scheme != 'david' || uri.host != 'pair') return null;
    final q = uri.queryParameters;
    final address = q['addr'];
    final code = q['code'];
    if (q['v'] != '1' || address == null || address.isEmpty || code == null || code.isEmpty) return null;
    return PairingAddress(serverId: q['id'] ?? '', address: address, fingerprint: q['fp'], code: code);
  }
}

/// The result of pairing.
class PairResult {
  PairResult(this.token, this.deviceId, this.userName, this.role);

  final String token;
  final int deviceId;
  final String userName;
  final String role;
}

/// Talks to the David server (`docs/spec-app-api.md`). With a fingerprint the
/// connection is HTTPS and only the server whose certificate has exactly this
/// fingerprint is accepted; no other certificate is, whoever signed it.
class ApiClient {
  ApiClient({required this.address, this.fingerprint, this.token, HttpClient? client})
      : _http = client ?? _pinnedClient(fingerprint);

  final String address;
  final String? fingerprint;
  String? token;
  final HttpClient _http;

  static HttpClient _pinnedClient(String? fingerprint) {
    final client = HttpClient(context: SecurityContext(withTrustedRoots: false));
    client.connectionTimeout = const Duration(seconds: 10);
    if (fingerprint != null) {
      final expected = fingerprint.toLowerCase();
      client.badCertificateCallback = (cert, host, port) => sha256.convert(cert.der).toString() == expected;
    }
    return client;
  }

  Uri _uri(String path, [Map<String, String>? query]) => Uri(
        scheme: fingerprint == null ? 'http' : 'https',
        host: Uri.parse('x://$address').host,
        port: Uri.parse('x://$address').port,
        path: path,
        queryParameters: query,
      );

  /// Pairs this device with the code from [pairing] and returns a client that holds
  /// the token. The WireGuard tunnel does not exist yet, so the key is random.
  static Future<(ApiClient, PairResult)> pair(PairingAddress pairing, String deviceName, {HttpClient? client}) async {
    final api = ApiClient(address: pairing.address, fingerprint: pairing.fingerprint, client: client);
    final random = Random.secure();
    final key = base64.encode(List<int>.generate(32, (_) => random.nextInt(256)));
    final j = await api._json('POST', '/api/v1/pair', body: {
      'code': pairing.code,
      'device_name': deviceName,
      'public_key': key,
    }) as Map<String, dynamic>;
    api.token = j['token'] as String;
    final user = j['user'] as Map<String, dynamic>;
    return (api, PairResult(api.token!, j['device_id'] as int, user['name'] as String, user['role'] as String));
  }

  Future<HttpClientResponse> _send(String method, Uri uri, {Object? body, Map<String, String>? headers}) async {
    try {
      final req = await _http.openUrl(method, uri);
      if (token != null) req.headers.set(HttpHeaders.authorizationHeader, 'Bearer $token');
      headers?.forEach(req.headers.set);
      if (body != null) {
        req.headers.contentType = ContentType.json;
        req.add(utf8.encode(jsonEncode(body)));
      }
      return await req.close().timeout(const Duration(seconds: 30));
    } on SocketException {
      throw ApiException(0, 'network');
    } on HandshakeException {
      throw ApiException(0, 'certificate');
    } on TimeoutException {
      throw ApiException(0, 'network');
    } on HttpException {
      throw ApiException(0, 'network');
    }
  }

  Future<Never> _fail(HttpClientResponse res) async {
    final text = await utf8.decodeStream(res);
    var code = 'error';
    try {
      code = (jsonDecode(text) as Map<String, dynamic>)['error'] as String? ?? code;
    } on FormatException {
      // not JSON: keep the generic code
    }
    throw ApiException(res.statusCode, code);
  }

  Future<Object?> _json(String method, String path, {Object? body, Map<String, String>? query}) async {
    final res = await _send(method, _uri(path, query), body: body);
    if (res.statusCode >= 400) await _fail(res);
    final text = await utf8.decodeStream(res);
    return text.isEmpty ? null : jsonDecode(text);
  }

  Future<Map<String, dynamic>> me() async => await _json('GET', '/api/v1/me') as Map<String, dynamic>;

  Future<void> logout() => _json('POST', '/api/v1/logout');

  Future<List<Map<String, dynamic>>> items() async =>
      ((await _json('GET', '/api/v1/items')) as List).cast<Map<String, dynamic>>();

  /// A short value that changes when the library does (a new CD, a release, a blurb).
  Future<String> libraryVersion() async =>
      (await _json('GET', '/api/v1/library/version') as Map<String, dynamic>)['version'] as String;

  Future<Map<String, dynamic>> item(int id) async => await _json('GET', '/api/v1/items/$id') as Map<String, dynamic>;

  /// The product picture of an item, or null if it has none (or may not be seen).
  Future<Uint8List?> cover(int itemId) async {
    final res = await _send('GET', _uri('/api/v1/items/$itemId/cover'));
    if (res.statusCode == 404) {
      await res.drain<void>();
      return null;
    }
    if (res.statusCode != 200) await _fail(res);
    final builder = BytesBuilder(copy: false);
    await for (final chunk in res) {
      builder.add(chunk);
    }
    return builder.takeBytes();
  }

  Future<List<Map<String, dynamic>>> continueListening() async =>
      ((await _json('GET', '/api/v1/continue')) as List).cast<Map<String, dynamic>>();

  Future<void> putProgress({required int itemId, required int partId, required Duration position, bool finished = false}) =>
      _json('PUT', '/api/v1/progress', body: {
        'item_id': itemId,
        'part_id': partId,
        'position_ms': position.inMilliseconds,
        'finished': finished,
      });

  /// Where an audio Bible opens the passage [text]. Empty if no item can.
  Future<List<Map<String, dynamic>>> resolvePassage(String text) async {
    final j = await _json('GET', '/api/v1/bible/resolve', query: {'q': text}) as Map<String, dynamic>;
    return (j['matches'] as List).cast<Map<String, dynamic>>();
  }

  /// Downloads a part into [dest]. The file appears under its name only when it
  /// is complete, so a cut-off download is never played.
  Future<void> downloadPart(int partId, File dest) async {
    final res = await _send('GET', _uri('/api/v1/parts/$partId/stream'));
    if (res.statusCode != 200) await _fail(res);
    await dest.parent.create(recursive: true);
    final partial = File('${dest.path}.part');
    final sink = partial.openWrite();
    try {
      await res.pipe(sink);
    } catch (_) {
      if (partial.existsSync()) partial.deleteSync();
      rethrow;
    }
    await partial.rename(dest.path);
  }

  void close() => _http.close(force: true);
}
