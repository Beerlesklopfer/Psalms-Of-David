// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import '../cd/server_cd_api.dart';
import '../server/api_client.dart';
import 'inbox_models.dart';

/// The routes of the inbox (`docs/spec-app-api.md`, "The inbox of the CDs"). Errors are [ApiException]s with the server's code:
/// `422 invalid_paging`, `404 not_found`, `403 forbidden`, `409 invalid_move`, and `409 decided` with `{by, resolution}` in the body.
abstract class InboxApi {
  /// `GET /inbox?after=&limit=`, newest first.
  Future<InboxPage> list({int? after, int? limit});

  /// `POST /inbox/{id}/read`.
  Future<void> markRead(int id);

  /// `POST /inbox/read_all`.
  Future<void> markAllRead();

  /// `POST /cd/jobs/{job_id}/approve`: the one way to decide a task.
  Future<void> approve(int jobId);

  /// `POST /cd/jobs/{job_id}/reject`.
  Future<void> reject(int jobId);

  /// `POST /cd/jobs/{job_id}/retry`, only while the job is in `retry_wait`.
  Future<void> retry(int jobId);

  /// `POST /cd/jobs/{job_id}/give_up`: ends a CD that could not be read (the job becomes `cancelled`); only in `retry_wait`.
  Future<void> giveUp(int jobId);
}

/// The inbox of the real server.
class ServerInboxApi implements InboxApi {
  ServerInboxApi(this.api);

  final ApiClient api;

  @override
  Future<InboxPage> list({int? after, int? limit}) async {
    final query = <String, String>{
      if (after != null) 'after': '$after',
      if (limit != null) 'limit': '$limit',
    };
    final j = await api.cdJson(
      'GET',
      '/api/v1/inbox',
      query: query.isEmpty ? null : query,
    );
    if (j is! Map<String, dynamic>) throw ApiException(200, 'error');
    return InboxPage.fromJson(j);
  }

  @override
  Future<void> markRead(int id) async {
    await api.cdJson('POST', '/api/v1/inbox/$id/read');
  }

  @override
  Future<void> markAllRead() async {
    await api.cdJson('POST', '/api/v1/inbox/read_all');
  }

  @override
  Future<void> approve(int jobId) async {
    await api.cdJson('POST', '/api/v1/cd/jobs/$jobId/approve');
  }

  @override
  Future<void> reject(int jobId) async {
    await api.cdJson('POST', '/api/v1/cd/jobs/$jobId/reject');
  }

  @override
  Future<void> retry(int jobId) async {
    await api.cdJson('POST', '/api/v1/cd/jobs/$jobId/retry');
  }

  @override
  Future<void> giveUp(int jobId) async {
    await api.cdJson('POST', '/api/v1/cd/jobs/$jobId/give_up');
  }
}
