// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:math';
import 'dart:typed_data';

import 'package:crypto/crypto.dart';

import '../barcode.dart';
import '../podcasts.dart';

/// An answer of the server that is not a success. [code] is the server's error
/// code (`outside_schedule`, `not_found`, ...) or `network` if there was no answer.
class ApiException implements Exception {
  ApiException(this.status, this.code, [this.body]);

  final int status;
  final String code;

  /// The whole JSON answer, when it had more than the code (for instance `taken` of `disc_exists`).
  final Map<String, dynamic>? body;

  @override
  String toString() => 'ApiException($status, $code)';
}

/// The contents of a pairing code: `david://pair?v=1&id=..&addr=..&fp=..&code=..`.
class PairingAddress {
  PairingAddress({required this.serverId, required this.address, required this.fingerprint, required this.code, this.ca = false});

  final String serverId;
  final String address;

  /// SHA-256 of the server certificate, lower case hex; null for a server without TLS.
  final String? fingerprint;
  final String code;

  /// The server has a certificate from a certificate authority (Let's Encrypt, ...): there is no
  /// fingerprint, the device checks the name in [address] and the usual trust instead.
  final bool ca;

  /// The address of the QR code (the same form the server makes): `david://pair?v=1&id=..&addr=..[&fp=..][&ca=1]&code=..`.
  String toUri() => Uri(
    scheme: 'david',
    host: 'pair',
    queryParameters: {
      'v': '1',
      'id': serverId,
      'addr': address,
      if (fingerprint != null && fingerprint!.isNotEmpty) 'fp': fingerprint!,
      if (ca) 'ca': '1',
      'code': code,
    },
  ).toString();

  static PairingAddress? parse(String text) {
    final uri = Uri.tryParse(text.trim());
    if (uri == null || uri.scheme != 'david' || uri.host != 'pair') return null;
    final q = uri.queryParameters;
    final address = q['addr'];
    final code = q['code'];
    if (q['v'] != '1' || address == null || address.isEmpty || code == null || code.isEmpty) return null;
    return PairingAddress(serverId: q['id'] ?? '', address: address, fingerprint: q['fp'], code: code, ca: q['ca'] == '1');
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

/// Talks to the David server (`docs/spec-app-api.md`). With a certificate authority ([ca]) the name
/// of the server is checked against the certificate and the trust of the system. With a fingerprint the
/// connection is HTTPS and only the server whose certificate has exactly this
/// fingerprint is accepted; no other certificate is, whoever signed it.
class ApiClient implements FeedApi, BarcodeApi {
  ApiClient({required this.address, this.fingerprint, this.ca = false, this.token, HttpClient? client})
      : _http = client ?? _pinnedClient(fingerprint, ca);

  final String address;
  final String? fingerprint;
  final bool ca;
  String? token;
  final HttpClient _http;

  static HttpClient _pinnedClient(String? fingerprint, bool ca) {
    // With a certificate authority the roots of the system count and the host name is checked, as usual.
    // Otherwise no authority is trusted: only the pinned certificate is.
    final client = HttpClient(context: SecurityContext(withTrustedRoots: ca));
    client.connectionTimeout = const Duration(seconds: 10);
    if (!ca && fingerprint != null) {
      final expected = fingerprint.toLowerCase();
      client.badCertificateCallback = (cert, host, port) => sha256.convert(cert.der).toString() == expected;
    }
    return client;
  }

  /// `https` for a server with a pinned certificate or one from a certificate authority, else `http`.
  String get scheme => (ca || fingerprint != null) ? 'https' : 'http';

  Uri _uri(String path, [Map<String, String>? query]) => Uri(
        scheme: scheme,
        host: Uri.parse('x://$address').host,
        port: Uri.parse('x://$address').port,
        path: path,
        queryParameters: query,
      );

  /// Pairs this device with the code from [pairing] and returns a client that holds
  /// the token. The WireGuard tunnel does not exist yet, so the key is random.
  static Future<(ApiClient, PairResult)> pair(PairingAddress pairing, String deviceName, {HttpClient? client}) async {
    final api = ApiClient(address: pairing.address, fingerprint: pairing.fingerprint, ca: pairing.ca, client: client);
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

  /// What the other members of the family listened to lately: `[{user_id, name, has_avatar, items: [{item_id, ...}]}]`.
  Future<List<Map<String, dynamic>>> familyListening() async =>
      ((await _json('GET', '/api/v1/family/listening')) as List).cast<Map<String, dynamic>>();

  /// The media time of the family for the line charts: `{from_day, days, members: [{user_id, name, me, kinds: {kind: [ms…]}, total: [ms…]}]}`.
  Future<Map<String, dynamic>> familyMediaTime({int days = 14}) async =>
      // The query goes in the `query` parameter: in the path a "?" would be percent-encoded and the server would not find the route.
      await _json('GET', '/api/v1/family/media-time', query: {'days': '$days'}) as Map<String, dynamic>;

  /// Shows or hides the own listening to the family; refused (403) when an administrator forbade hiding it.
  Future<void> setShareListening(bool share) => _json('PUT', '/api/v1/me', body: {'share_listening': share});

  Future<Map<String, dynamic>> me() async => await _json('GET', '/api/v1/me') as Map<String, dynamic>;

  Future<void> logout() => _json('POST', '/api/v1/logout');

  /// Sends [bytes] as the body (a picture) and returns the response without reading it.
  Future<HttpClientResponse> sendBytes(String method, String path, List<int> bytes, {required String contentType}) async {
    try {
      final req = await _http.openUrl(method, _uri(path));
      if (token != null) req.headers.set(HttpHeaders.authorizationHeader, 'Bearer $token');
      req.headers.set(HttpHeaders.contentTypeHeader, contentType);
      req.headers.contentLength = bytes.length;
      req.add(bytes);
      return await req.close().timeout(const Duration(seconds: 60));
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

  Future<List<Map<String, dynamic>>> items() async =>
      ((await _json('GET', '/api/v1/items')) as List).cast<Map<String, dynamic>>();

  @override
  Future<List<Map<String, dynamic>>> podcasts() async =>
      ((await _json('GET', '/api/v1/podcasts') as Map<String, dynamic>)['podcasts'] as List).cast<Map<String, dynamic>>();

  /// Follows a feed; the answer says whether it is released (`approved`) or waits for a parent.
  @override
  Future<Map<String, dynamic>> followPodcast(String url) async =>
      await _json('POST', '/api/v1/podcasts', body: {'url': url}) as Map<String, dynamic>;

  @override
  Future<void> unfollowPodcast(int id) => _json('DELETE', '/api/v1/podcasts/$id');

  @override
  Future<List<Map<String, dynamic>>> episodes(int podcastId) async =>
      ((await _json('GET', '/api/v1/podcasts/$podcastId/episodes') as Map<String, dynamic>)['episodes'] as List).cast<Map<String, dynamic>>();

  Future<void> putEpisodeState(int episodeId, Duration position, {required bool finished}) =>
      _json('PUT', '/api/v1/episodes/$episodeId/state', body: {'position_ms': position.inMilliseconds, 'finished': finished});

  @override
  Future<List<Map<String, dynamic>>> stations() async =>
      ((await _json('GET', '/api/v1/radio') as Map<String, dynamic>)['stations'] as List).cast<Map<String, dynamic>>();

  @override
  Future<Map<String, dynamic>> addStation(String name, String streamUrl, {String? homepage}) async =>
      await _json('POST', '/api/v1/radio', body: {'name': name, 'stream_url': streamUrl, 'homepage': ?homepage}) as Map<String, dynamic>;

  @override
  Future<void> removeStation(int id) => _json('DELETE', '/api/v1/radio/$id');

  /// The genres of the list, in the order the administrator gave them.
  Future<List<String>> genres() async =>
      [for (final g in (await _json('GET', '/api/v1/genres')) as List) (g as Map<String, dynamic>)['name'] as String];

  /// A short value that changes when the library does (a new CD, a release, a blurb).
  Future<String> libraryVersion() async =>
      (await _json('GET', '/api/v1/library/version') as Map<String, dynamic>)['version'] as String;

  /// The version of the library, the fingerprint of this profile's playlists and of what it hid (`null` from an older server).
  Future<({String version, String? playlists, String? hidden})> libraryVersions() async {
    final j = await _json('GET', '/api/v1/library/version') as Map<String, dynamic>;
    return (version: j['version'] as String, playlists: j['playlists'] as String?, hidden: j['hidden'] as String?);
  }

  /// Which albums of the library carry this barcode. A code that the server calls invalid (422 `invalid_barcode`) is an answer with `invalid` set,
  /// not an exception; other failures throw an [ApiException].
  @override
  Future<BarcodeAnswer> barcodeLookup(String code) async {
    try {
      return BarcodeAnswer.fromJson(await _json('GET', '/api/v1/barcode/${Uri.encodeComponent(code)}') as Map<String, dynamic>);
    } on ApiException catch (e) {
      if (e.status == 422 && e.code == 'invalid_barcode') return BarcodeAnswer.invalid(code);
      rethrow;
    }
  }

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
        // When the listening happened: the server keeps the newer state, whatever order the reports arrive in.
        'at': DateTime.now().millisecondsSinceEpoch,
      });

  /// Where an audio Bible opens the passage [text]. Empty if no item can.
  Future<List<Map<String, dynamic>>> resolvePassage(String text) async {
    final j = await _json('GET', '/api/v1/bible/resolve', query: {'q': text}) as Map<String, dynamic>;
    return (j['matches'] as List).cast<Map<String, dynamic>>();
  }

  /// Downloads a part into [dest]. The file appears under its name only when it
  /// is complete, so a cut-off download is never played.
  ///
  /// With [asMp3] the server makes an MP3 of a file the device cannot play (it can take seconds the first time).
  Future<void> downloadPart(int partId, File dest, {bool asMp3 = false}) async {
    final res = await _send('GET', _uri('/api/v1/parts/$partId/stream', asMp3 ? {'as': 'mp3'} : null));
    if (res.statusCode != 200) await _fail(res);
    await dest.parent.create(recursive: true);
    final partial = partialFileFor(dest);
    final sink = partial.openWrite();
    try {
      await res.pipe(sink);
    } catch (_) {
      if (partial.existsSync()) partial.deleteSync();
      rethrow;
    }
    await partial.rename(dest.path);
  }

  /// Sends a request and returns the response without reading it.
  Future<HttpClientResponse> sendRaw(String method, String path, {Object? body, Map<String, String>? query, Map<String, String>? headers}) =>
      _send(method, _uri(path, query), body: body, headers: headers);

  /// A WebSocket to [path] (`ws`/`wss` instead of `http`/`https`) over the same HttpClient, so the
  /// pinned fingerprint or the CA mode applies to it too, with the bearer token as header.
  Future<WebSocket> openSocket(String path) {
    final u = _uri(path);
    final url = u.replace(scheme: u.scheme == 'https' ? 'wss' : 'ws').toString();
    return WebSocket.connect(url, headers: {if (token != null) HttpHeaders.authorizationHeader: 'Bearer $token'}, customClient: _http);
  }

  /// The URL [openSocket] uses, for display and tests.
  String socketUrl(String path) {
    final u = _uri(path);
    return u.replace(scheme: u.scheme == 'https' ? 'wss' : 'ws').toString();
  }

  void close() => _http.close(force: true);

  /// The ids of the items that became visible to this user recently and that they have not begun,
  /// newest first, as the server decides ("new" window and maximum are server settings).
  /// An older server without the route, or any failure, is an empty list: this is no error to show.
  Future<List<int>> newItems() async {
    try {
      final j = await _json('GET', '/api/v1/new');
      if (j is! Map) return const [];
      final items = j['items'];
      if (items is! List) return const [];
      return [
        for (final i in items)
          if (i is Map && i['id'] is int) i['id'] as int,
      ];
    } on Object {
      return const [];
    }
  }
}

/// The file a download of [dest] is written to until it is complete: `<dest>.<unique>.part`. Each download has its own,
/// so two downloads of the same file that run at the same time cannot write into one file and leave a broken one behind.
File partialFileFor(File dest) => File('${dest.path}.${DateTime.now().microsecondsSinceEpoch}-${_partialCounter++}.part');

int _partialCounter = 0;
