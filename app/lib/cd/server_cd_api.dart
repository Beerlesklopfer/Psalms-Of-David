// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io';

import '../server/api_client.dart';
import 'cd_events.dart';
import 'cd_models.dart';
import 'cd_ports.dart';

/// The server's CD import interface (`/api/v1/cd/...`, `docs/spec-cd-import-api.md`).
///
/// The drive of the server (described in the document, section "The drive of the server"):
/// `GET cd/drive`, `POST cd/rip` (body: [CdRipRequest]; answer `{"job":{...}}` or `{"id":..}`),
/// `POST cd/rip/{id}/cancel`, `POST cd/eject` (answer `{"ejected":bool}`), `POST cd/lookup` with an
/// empty `toc` (the server reads the disc itself) and the `drive` message of the event channel.
class ServerCdApi implements CdServerApi {
  ServerCdApi(this.api);

  final ApiClient api;

  @override
  Future<CdSettings> settings() async =>
      CdSettings.fromJson(await api.cdJson('GET', '/api/v1/cd/settings') as Map<String, dynamic>);

  @override
  Future<CdDrivesState> drives() async =>
      CdDrivesState.fromJson(await api.cdJson('GET', '/api/v1/cd/drive') as Map<String, dynamic>);

  @override
  Future<List<CdCandidate>> lookup({String? device, String? query, String? releaseId}) async {
    final j = await api.cdJson(
      'POST',
      '/api/v1/cd/lookup',
      body: {'device': ?device, 'query': ?query, 'release_id': ?releaseId},
    ) as Map<String, dynamic>;
    return [
      for (final c in (j['candidates'] as List? ?? const []).cast<Map<String, dynamic>>())
        CdCandidate.fromJson(c),
    ];
  }

  @override
  Future<CdRipStart> startRip(CdRipRequest request) async {
    final j =
        await api.cdJson('POST', '/api/v1/cd/rip', body: request.toJson()) as Map<String, dynamic>;
    final job = j['job'];
    return CdRipStart(
      '${job is Map<String, dynamic> ? job['id'] : j['id']}',
      resume: j['resume'] as String?,
    );
  }

  @override
  Future<void> dismissRip({String? device}) async {
    await api.cdJson('POST', '/api/v1/cd/rip/dismiss', body: {'device': ?device});
  }

  @override
  Future<void> cancelRip(String jobId) async {
    await api.cdJson('POST', '/api/v1/cd/rip/$jobId/cancel');
  }

  @override
  Future<bool> eject({String? device}) async {
    final j = await api.cdJson('POST', '/api/v1/cd/eject', body: {'device': ?device});
    return j is Map<String, dynamic> ? (j['ejected'] as bool?) ?? true : true;
  }
}

/// Calls the CD endpoints through the client's authenticated connection.
extension CdApiClient on ApiClient {
  Future<Object?> cdJson(String method, String path, {Object? body}) async {
    final res = await sendRaw(method, path, body: body);
    final text = await utf8.decodeStream(res);
    if (res.statusCode >= 400) throw ApiException(res.statusCode, _code(text), _body(text));
    return text.isEmpty ? null : jsonDecode(text);
  }
}

Map<String, dynamic>? _body(String text) {
  try {
    final j = jsonDecode(text);
    return j is Map<String, dynamic> ? j : null;
  } on Object {
    return null;
  }
}

String _code(String text) {
  try {
    return (jsonDecode(text) as Map<String, dynamic>)['error'] as String? ?? 'error';
  } on Object {
    return 'error';
  }
}

/// The push channel and job lookups of the real server.
CdEventsSource serverCdEvents(ApiClient api) => SocketCdEventsSource(
  url: api.socketUrl('/api/v1/cd/events'),
  headers: {if (api.token != null) HttpHeaders.authorizationHeader: 'Bearer ${api.token}'},
  socket: (url, headers) => api.openSocket('/api/v1/cd/events'),
  getJson: (path) => api.cdJson('GET', path),
);
