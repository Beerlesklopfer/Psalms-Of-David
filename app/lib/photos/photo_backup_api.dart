// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import '../server/api_client.dart';

/// The choices of the profile, in the order the screen shows them (`server/src/photo_settings.rs`, `PROFILE_CHOICES`).
const kPhotoChoices = <String, List<String>>{
  'enabled': ['off', 'on'],
  'mode': ['uni', 'bi'],
  'network': ['wifi', 'any'],
  'charging': ['charging', 'always'],
  'videos': ['on', 'off'],
  'deletes': ['never', 'trash'],
  'parent_view': ['off', 'on'],
};

/// What `GET /api/v1/photos/settings` says: what the server allows, the stored choices of the profile, what they are worth under the
/// rules of the server, and per choice the options that cannot be chosen with the reason.
class PhotoSettings {
  PhotoSettings({
    required this.folder,
    required this.active,
    required this.settings,
    required this.effective,
    required this.locked,
    this.bisync = false,
    this.videos = true,
    this.maxFileMb = 2048,
  });

  factory PhotoSettings.fromJson(Map<String, dynamic> j) {
    Map<String, String> strings(Object? o) => {
          if (o is Map)
            for (final e in o.entries)
              if (e.value is String) '${e.key}': e.value as String,
        };
    final server = j['server'] is Map ? j['server'] as Map : const {};
    return PhotoSettings(
      folder: j['folder'] == true,
      active: j['active'] == true,
      settings: strings(j['settings']),
      effective: strings(j['effective']),
      locked: {
        if (j['locked'] is Map)
          for (final e in (j['locked'] as Map).entries)
            if (e.value is List)
              '${e.key}': {
                for (final l in e.value as List)
                  if (l is Map && l['option'] is String && l['why'] is String) l['option'] as String: l['why'] as String,
              },
      },
      bisync: server['bisync'] == true,
      videos: server['videos'] != false,
      maxFileMb: server['max_file_mb'] is int ? server['max_file_mb'] as int : 2048,
    );
  }

  /// The profile has a picture folder on the server (an administrator sets it on the Users page).
  final bool folder;

  /// The backup of this profile runs: switched on, allowed by the server, and there is a folder.
  final bool active;

  /// The stored choices, by key (`enabled` → `on`); `parent_view` only for a child.
  final Map<String, String> settings;

  /// What the choices are worth under the rules of the server (a choice the server does not allow counts as its safe option).
  final Map<String, String> effective;

  /// Per key, the options that cannot be chosen and why (`{"mode": {"bi": "no_bisync"}}`).
  final Map<String, Map<String, String>> locked;

  final bool bisync;
  final bool videos;
  final int maxFileMb;

  bool get videosOn => effective['videos'] == 'on';

  /// Bisync with "deleting into the trash": deletions go to the server and to the other devices.
  bool get trashOn => effective['enabled'] == 'on' && effective['mode'] == 'bi' && effective['deletes'] == 'trash';
  bool get wifiOnly => effective['network'] != 'any';
  bool get chargingOnly => effective['charging'] != 'always';
}

/// What the person keeps on the server (`GET /api/v1/photos/state`).
class PhotoServerState {
  PhotoServerState({required this.count, required this.bytes, this.lastAt, this.quotaBytes, this.trashCount = 0});

  factory PhotoServerState.fromJson(Map<String, dynamic> j) => PhotoServerState(
        count: j['count'] is int ? j['count'] as int : 0,
        bytes: j['bytes'] is int ? j['bytes'] as int : 0,
        trashCount: j['trash_count'] is int ? j['trash_count'] as int : 0,
        lastAt: j['last_at'] is int ? DateTime.fromMillisecondsSinceEpoch(j['last_at'] as int) : null,
        quotaBytes: j['quota_bytes'] is int ? j['quota_bytes'] as int : null,
      );

  final int count;
  final int bytes;
  final DateTime? lastAt;
  final int? quotaBytes;

  /// In the trash (not in [count]).
  final int trashCount;
}

/// The answer to one piece of an upload.
sealed class PieceAnswer {
  const PieceAnswer();
}

/// The piece was taken; the next one starts at [received].
class PiecePartial extends PieceAnswer {
  const PiecePartial(this.received);
  final int received;
}

/// The file is complete and its SHA-256 fits; [known] when the server had it already.
class PieceDone extends PieceAnswer {
  const PieceDone(this.photo, {required this.known});
  final int photo;
  final bool known;
}

/// The server refused the piece: [code] as in `server/src/api_photos.rs`; with `wrong_offset` [received] says where to go on.
class PieceRefused extends PieceAnswer {
  const PieceRefused(this.code, [this.received]);
  final String code;
  final int? received;
}

/// A picture that another device of the person sent (`GET /api/v1/photos/changes`).
class RemotePhoto {
  RemotePhoto({required this.id, required this.sha256, required this.size, required this.name, required this.mime, required this.takenAt, this.thumb = false});

  factory RemotePhoto.fromJson(Map<String, dynamic> j) => RemotePhoto(
        id: j['id'] as int,
        sha256: j['sha256'] as String? ?? '',
        size: j['size'] as int? ?? 0,
        name: j['name'] as String? ?? '',
        mime: j['mime'] as String? ?? 'application/octet-stream',
        takenAt: DateTime.fromMillisecondsSinceEpoch(j['taken_at'] as int? ?? 0),
        thumb: j['thumb'] == true,
      );

  final int id;
  final String sha256;
  final int size;
  final String name;
  final String mime;
  final DateTime takenAt;

  /// The server has a preview.
  final bool thumb;

  bool get video => mime.startsWith('video/');
}

/// A picture in the trash of the server (`GET /api/v1/photos/trash`).
class TrashedPhoto {
  TrashedPhoto({required this.id, required this.sha256, required this.name, required this.mime, required this.size, required this.takenAt, required this.purgeAt});

  factory TrashedPhoto.fromJson(Map<String, dynamic> j) => TrashedPhoto(
        id: j['id'] as int,
        sha256: j['sha256'] as String? ?? '',
        name: j['name'] as String? ?? '',
        mime: j['mime'] as String? ?? '',
        size: j['size'] as int? ?? 0,
        takenAt: DateTime.fromMillisecondsSinceEpoch(j['taken_at'] as int? ?? 0),
        purgeAt: DateTime.fromMillisecondsSinceEpoch(j['purge_at'] as int? ?? 0),
      );

  final int id;
  final String sha256;
  final String name;
  final String mime;
  final int size;
  final DateTime takenAt;

  /// When the server removes it for good.
  final DateTime purgeAt;

  bool get video => mime.startsWith('video/');
}

/// A deletion (`trash`) or a restore that another device of the person did (`GET /api/v1/photos/removals`).
class PhotoEvent {
  PhotoEvent(this.seq, this.photo, this.sha256, this.kind);

  factory PhotoEvent.fromJson(Map<String, dynamic> j) =>
      PhotoEvent(j['seq'] as int? ?? 0, j['photo'] as int? ?? 0, j['sha256'] as String? ?? '', j['kind'] as String? ?? '');

  final int seq;
  final int photo;
  final String sha256;
  final String kind;
}

/// A change of the choices that the server refused, by key (`{"mode": "no_bisync"}`); nothing was saved.
class PhotoChoiceRefused implements Exception {
  PhotoChoiceRefused(this.refused);
  final Map<String, String> refused;
}

/// The photo backup of the profile of this device (`docs/spec-photo-sync.md`, section 4).
abstract class PhotoBackupApi {
  /// The settings, or null when the server has the backup off (`404 photo_sync_off`) or the profile is a guest (`403 no_guests`).
  Future<PhotoSettings?> settings();

  /// Changes choices (`{'mode': 'bi'}`); throws [PhotoChoiceRefused] when the server does not allow one.
  Future<PhotoSettings> putSettings(Map<String, String> choices);

  Future<PhotoServerState> state();

  /// Of the files the device holds, the SHA-256 of those the server lacks.
  Future<Set<String>> missing(List<({String sha256, int size})> files);

  /// How much of an upload under way the server has, or null if it knows none under [clientId].
  Future<int?> received(String clientId);

  Future<PieceAnswer> putPiece({
    required String clientId,
    required String sha256,
    required int size,
    required String mime,
    required int start,
    required List<int> bytes,
    String? name,
    DateTime? takenAt,
  });

  /// The preview of a picture this device sent (a JPEG made by the gallery of the device).
  Future<void> putThumb(int photo, List<int> jpeg);

  /// Bisync: what the other devices of the person sent after [since], and the `since` of the next call.
  Future<({List<RemotePhoto> photos, int next})> changes(int since, {int limit = 200});

  /// The preview of a picture, or null when the server has none.
  Future<List<int>?> thumb(int photo);

  /// Loads the file of a picture into [dest] and checks it against [sha256]; the file appears under its name only when it is complete.
  Future<void> download(int photo, File dest, String sha256);

  /// The device deleted these pictures: into the trash of the server (bisync with `deletes` = `trash`); the ids moved.
  Future<List<int>> trash(List<String> sha256);

  Future<List<TrashedPhoto>> trashList();

  /// Takes a picture out of the trash.
  Future<void> restore(int photo);

  /// What the other devices deleted or restored after [since]; with null no events, and `next` is where to start.
  Future<({List<PhotoEvent> events, int next})> removals(int? since);
}

/// [PhotoBackupApi] over the app API of David.
class ServerPhotoBackupApi implements PhotoBackupApi {
  ServerPhotoBackupApi(this.api);

  final ApiClient api;

  Future<(int, Map<String, dynamic>)> _call(String method, String path, {Object? body}) async {
    final res = await api.sendRaw(method, path, body: body);
    return (res.statusCode, _object(await utf8.decodeStream(res)));
  }

  static Map<String, dynamic> _object(String text) {
    try {
      final j = jsonDecode(text);
      return j is Map<String, dynamic> ? j : {};
    } on FormatException {
      return {};
    }
  }

  static Never _fail(int status, Map<String, dynamic> j) => throw ApiException(status, j['error'] as String? ?? 'error', j);

  @override
  Future<PhotoSettings?> settings() async {
    final (status, j) = await _call('GET', '/api/v1/photos/settings');
    if ((status == 404 && j['error'] == 'photo_sync_off') || (status == 403 && j['error'] == 'no_guests')) return null;
    if (status >= 400) _fail(status, j);
    return PhotoSettings.fromJson(j);
  }

  @override
  Future<PhotoSettings> putSettings(Map<String, String> choices) async {
    final (status, j) = await _call('PUT', '/api/v1/photos/settings', body: choices);
    if (status == 422 && j['refused'] is Map) {
      throw PhotoChoiceRefused({for (final e in (j['refused'] as Map).entries) '${e.key}': '${e.value}'});
    }
    if (status >= 400) _fail(status, j);
    return PhotoSettings.fromJson(j);
  }

  @override
  Future<PhotoServerState> state() async {
    final (status, j) = await _call('GET', '/api/v1/photos/state');
    if (status >= 400) _fail(status, j);
    return PhotoServerState.fromJson(j);
  }

  @override
  Future<Set<String>> missing(List<({String sha256, int size})> files) async {
    final (status, j) = await _call('POST', '/api/v1/photos/have', body: {
      'files': [for (final f in files) {'sha256': f.sha256, 'size': f.size}],
    });
    if (status >= 400) _fail(status, j);
    return {for (final m in (j['missing'] as List? ?? const [])) '$m'};
  }

  @override
  Future<int?> received(String clientId) async {
    final (status, j) = await _call('GET', '/api/v1/photos/upload/${Uri.encodeComponent(clientId)}');
    if (status == 404) return null;
    if (status >= 400) _fail(status, j);
    return j['received'] as int?;
  }

  @override
  Future<PieceAnswer> putPiece({
    required String clientId,
    required String sha256,
    required int size,
    required String mime,
    required int start,
    required List<int> bytes,
    String? name,
    DateTime? takenAt,
  }) async {
    final end = start + bytes.length - 1;
    final res = await api.sendBytes('PUT', '/api/v1/photos/upload/${Uri.encodeComponent(clientId)}', bytes, contentType: mime, headers: {
      'X-Sha256': sha256,
      'X-Size': '$size',
      if (bytes.isNotEmpty) HttpHeaders.contentRangeHeader: 'bytes $start-$end/$size',
      // A header carries only ASCII safely: the name goes percent-encoded, the server decodes it.
      if (start == 0 && name != null && name.isNotEmpty) 'X-File-Name': Uri.encodeComponent(name),
      if (start == 0 && takenAt != null) 'X-Taken-At': '${takenAt.millisecondsSinceEpoch}',
    });
    final status = res.statusCode;
    final j = _object(await utf8.decodeStream(res));
    if (status == 202) return PiecePartial(j['received'] as int? ?? start + bytes.length);
    if (status == 200 || status == 201) return PieceDone(j['photo'] as int? ?? 0, known: status == 200);
    if (j['error'] is String && status != 401) return PieceRefused(j['error'] as String, j['received'] as int?);
    _fail(status, j);
  }

  @override
  Future<void> putThumb(int photo, List<int> jpeg) async {
    final res = await api.sendBytes('PUT', '/api/v1/photos/$photo/thumb', jpeg, contentType: 'image/jpeg');
    final status = res.statusCode;
    final j = _object(await utf8.decodeStream(res));
    if (status >= 400) _fail(status, j);
  }

  @override
  Future<({List<RemotePhoto> photos, int next})> changes(int since, {int limit = 200}) async {
    final res = await api.sendRaw('GET', '/api/v1/photos/changes', query: {'since': '$since', 'limit': '$limit'});
    final status = res.statusCode;
    final j = _object(await utf8.decodeStream(res));
    if (status >= 400) _fail(status, j);
    return (
      photos: [for (final p in j['photos'] as List? ?? const []) if (p is Map<String, dynamic> && p['id'] is int) RemotePhoto.fromJson(p)],
      next: j['next'] as int? ?? since,
    );
  }

  @override
  Future<List<int>?> thumb(int photo) async {
    final res = await api.sendRaw('GET', '/api/v1/photos/$photo/thumb');
    if (res.statusCode == 404) {
      await res.drain<void>();
      return null;
    }
    if (res.statusCode >= 400) _fail(res.statusCode, _object(await utf8.decodeStream(res)));
    final b = BytesBuilder(copy: false);
    await for (final chunk in res.timeout(kDownloadIdleTimeout)) {
      b.add(chunk);
    }
    return b.takeBytes();
  }

  @override
  Future<List<int>> trash(List<String> sha256) async {
    final (status, j) = await _call('POST', '/api/v1/photos/trash', body: {'sha256': sha256});
    if (status >= 400) _fail(status, j);
    return [for (final id in j['trashed'] as List? ?? const []) if (id is int) id];
  }

  @override
  Future<List<TrashedPhoto>> trashList() async {
    final (status, j) = await _call('GET', '/api/v1/photos/trash');
    if (status >= 400) _fail(status, j);
    return [for (final p in j['photos'] as List? ?? const []) if (p is Map<String, dynamic> && p['id'] is int) TrashedPhoto.fromJson(p)];
  }

  @override
  Future<void> restore(int photo) async {
    final (status, j) = await _call('POST', '/api/v1/photos/$photo/restore');
    if (status >= 400) _fail(status, j);
  }

  @override
  Future<({List<PhotoEvent> events, int next})> removals(int? since) async {
    final res = await api.sendRaw('GET', '/api/v1/photos/removals', query: {if (since != null) 'since': '$since', 'limit': '500'});
    final status = res.statusCode;
    final j = _object(await utf8.decodeStream(res));
    if (status >= 400) _fail(status, j);
    return (
      events: [for (final e in j['events'] as List? ?? const []) if (e is Map<String, dynamic>) PhotoEvent.fromJson(e)],
      next: j['next'] as int? ?? since ?? 0,
    );
  }

  @override
  Future<void> download(int photo, File dest, String sha256) async {
    final res = await api.sendRaw('GET', '/api/v1/photos/$photo/file');
    if (res.statusCode >= 400) _fail(res.statusCode, _object(await utf8.decodeStream(res)));
    await dest.parent.create(recursive: true);
    final partial = partialFileFor(dest);
    try {
      await res.timeout(kDownloadIdleTimeout).pipe(partial.openWrite());
      await verifyDownload(partial, expectedLength: res.contentLength, expectedSha256: sha256);
    } catch (_) {
      if (partial.existsSync()) partial.deleteSync();
      rethrow;
    }
    await partial.rename(dest.path);
  }
}
