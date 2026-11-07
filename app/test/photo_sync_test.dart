// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io';
import 'dart:math';

import 'package:crypto/crypto.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/photos/photo_backup_api.dart';
import 'package:psalm/photos/photo_device.dart';
import 'package:psalm/photos/photo_sync.dart';
import 'package:psalm/server/api_client.dart';

/// A stand-in for the server: it keeps files by hash, takes pieces only at the right offset, and checks the hash at the end, as
/// `server/src/photo_store.rs` does.
class FakeServer implements PhotoBackupApi {
  final kept = <String, List<int>>{};
  final uploads = <String, List<int>>{};
  final haveCalls = <List<String>>[];
  final pieces = <({String clientId, int start, int length, String? name})>[];
  bool active = true;
  int maxFileMb = 2048;

  /// After this many pieces the connection "breaks" (once).
  int? breakAfterPieces;

  /// The next complete file is answered with `hash_mismatch` (once).
  bool mismatchOnce = false;

  @override
  Future<PhotoSettings?> settings() async => PhotoSettings(
        folder: true,
        active: active,
        settings: {'enabled': active ? 'on' : 'off', 'videos': 'on'},
        effective: {'enabled': active ? 'on' : 'off', 'videos': 'on', 'network': 'wifi', 'charging': 'charging'},
        locked: const {},
        maxFileMb: maxFileMb,
      );

  @override
  Future<PhotoSettings> putSettings(Map<String, String> choices) async => (await settings())!;

  @override
  Future<PhotoServerState> state() async => PhotoServerState(count: kept.length, bytes: kept.values.fold(0, (a, b) => a + b.length));

  @override
  Future<Set<String>> missing(List<({String sha256, int size})> files) async {
    haveCalls.add([for (final f in files) f.sha256]);
    return {for (final f in files) if (!kept.containsKey(f.sha256)) f.sha256};
  }

  @override
  Future<int?> received(String clientId) async => uploads[clientId]?.length;

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
    if (breakAfterPieces != null && pieces.length >= breakAfterPieces!) {
      breakAfterPieces = null;
      throw ApiException(0, 'network');
    }
    pieces.add((clientId: clientId, start: start, length: bytes.length, name: name));
    if (kept.containsKey(sha256)) return const PieceDone(1, known: true);
    final have = uploads.putIfAbsent(clientId, () => []);
    if (start != have.length) return PieceRefused('wrong_offset', have.length);
    have.addAll(bytes);
    if (have.length < size) return PiecePartial(have.length);
    uploads.remove(clientId);
    if (mismatchOnce || sha256Of(have) != sha256) {
      mismatchOnce = false;
      return const PieceRefused('hash_mismatch');
    }
    kept[sha256] = have;
    return PieceDone(kept.length, known: false);
  }
}

String sha256Of(List<int> b) => sha256.convert(b).toString();

class FakeLibrary extends PhotoLibrary {
  FakeLibrary(this.photos);

  final List<LocalPhoto> photos;
  DateTime? askedSince;
  bool access = true;

  @override
  Future<bool> hasAccess() async => access;

  @override
  Future<bool> requestAccess() async => access;

  @override
  Future<List<LocalPhoto>> list({DateTime? since, required bool videos, required String sources}) async {
    askedSince = since;
    return [for (final p in photos) if (since == null || !p.takenAt.isBefore(since)) p];
  }
}

void main() {
  late Directory dir;
  late PhotoDeviceStore store;
  final day = DateTime(2026, 11, 7);

  setUp(() {
    dir = Directory.systemTemp.createTempSync('photo-sync-');
    store = PhotoDeviceStore(File('${dir.path}/photo_backup.json'), File('${dir.path}/photo_device.json'));
    store.saveSettings(PhotoDeviceSettings(enabled: true, since: day));
  });
  tearDown(() => dir.deleteSync(recursive: true));

  LocalPhoto photo(String name, List<int> bytes, {DateTime? taken}) {
    final f = File('${dir.path}/$name')..writeAsBytesSync(bytes);
    final at = taken ?? day.add(const Duration(hours: 9));
    return LocalPhoto(id: name, name: name, mime: 'image/jpeg', takenAt: at, modified: at, open: () async => f);
  }

  List<int> bytes(int n, int seed) {
    final r = Random(seed);
    return List<int>.generate(n, (_) => r.nextInt(256));
  }

  test('the missing pictures are sent once, the one the server has is only marked, and a second run sends nothing', () async {
    final server = FakeServer();
    final a = bytes(1000, 1), b = bytes(2000, 2), c = bytes(3000, 3);
    server.kept[sha256Of(b)] = b; // from another device of the person
    final lib = FakeLibrary([photo('a.jpg', a), photo('b.jpg', b), photo('c.jpg', c)]);

    final first = await PhotoSync(api: server, library: lib, store: store).run();
    expect(first.stop, isNull);
    expect(first.sent, 2);
    expect(first.known, 1);
    expect(first.waiting, 0);
    expect(server.kept.keys, containsAll([sha256Of(a), sha256Of(c)]));
    expect(server.pieces.map((p) => p.name), ['a.jpg', 'c.jpg']);

    server.pieces.clear();
    server.haveCalls.clear();
    final second = await PhotoSync(api: server, library: lib, store: store).run();
    expect(second.sent + second.known, 0);
    expect(server.pieces, isEmpty);
    expect(server.haveCalls, isEmpty, reason: 'what is done is not even offered again');
  });

  test('only pictures from the day the device was switched on, unless the older ones are asked for; a device not switched on sends nothing', () async {
    final server = FakeServer();
    final old = photo('old.jpg', bytes(100, 4), taken: DateTime(2020, 1, 1));
    final lib = FakeLibrary([old, photo('new.jpg', bytes(100, 5))]);

    expect((await PhotoSync(api: server, library: lib, store: store).run()).sent, 1);
    expect(lib.askedSince, day);

    store.saveSettings(store.loadSettings()..olderToo = true);
    expect((await PhotoSync(api: server, library: lib, store: store).run()).sent, 1);
    expect(lib.askedSince, isNull);

    store.saveSettings(PhotoDeviceSettings());
    final off = await PhotoSync(api: server, library: lib, store: store).run();
    expect(off.stop, PhotoRunStop.deviceOff);
  });

  test('a broken connection in the middle of a large file goes on in the next run where the server is', () async {
    final server = FakeServer()..breakAfterPieces = 1;
    final big = bytes(kPhotoPiece * 2 + 1234, 6);
    final lib = FakeLibrary([photo('video.mp4', big)]);

    final first = await PhotoSync(api: server, library: lib, store: store).run();
    expect(first.stop, PhotoRunStop.network);
    expect(first.waiting, 1);
    expect(store.load().errors.first.code, 'network');
    expect(store.load().waiting, 1);

    final second = await PhotoSync(api: server, library: lib, store: store).run();
    expect(second.sent, 1);
    expect(server.kept[sha256Of(big)], big);
    // the first piece is not sent twice
    expect(server.pieces.map((p) => p.start), [0, kPhotoPiece, kPhotoPiece * 2]);
  });

  test('a file larger than the server takes is not offered, and is tried again when the limit grows', () async {
    final server = FakeServer()..maxFileMb = 1;
    final big = bytes(1024 * 1024 + 1, 7);
    final lib = FakeLibrary([photo('big.mp4', big)]);

    final first = await PhotoSync(api: server, library: lib, store: store).run();
    expect(first.skipped, 1);
    expect(server.haveCalls, isEmpty);
    expect(store.load().errors.first.code, 'too_large');

    expect((await PhotoSync(api: server, library: lib, store: store).run()).skipped, 0, reason: 'the same rules: not again');

    server.maxFileMb = 2;
    expect((await PhotoSync(api: server, library: lib, store: store).run()).sent, 1);
  });

  test('a picture whose bytes did not fit the hash is read again in the next run', () async {
    final server = FakeServer()..mismatchOnce = true;
    final lib = FakeLibrary([photo('a.jpg', bytes(500, 8))]);

    final first = await PhotoSync(api: server, library: lib, store: store).run();
    expect(first.sent, 0);
    expect(store.load().errors.first.code, 'hash_mismatch');
    expect(store.load().entries, isEmpty);

    expect((await PhotoSync(api: server, library: lib, store: store).run()).sent, 1);
  });

  test('a backup that the profile has off, or that another run holds, does nothing', () async {
    final server = FakeServer()..active = false;
    final lib = FakeLibrary([photo('a.jpg', bytes(10, 9))]);
    expect((await PhotoSync(api: server, library: lib, store: store).run()).stop, PhotoRunStop.inactive);

    server.active = true;
    expect(store.tryLock(), isTrue);
    expect((await PhotoSync(api: server, library: lib, store: store).run()).stop, PhotoRunStop.busy);
    store.unlock();
    expect((await PhotoSync(api: server, library: lib, store: store).run()).sent, 1);
  });

  group('ServerPhotoBackupApi', () {
    late HttpServer http;
    final seen = <HttpRequest>[];
    final heads = <Map<String, String?>>[];
    late Future<void> Function(HttpRequest) answer;

    setUp(() async {
      seen.clear();
      heads.clear();
      http = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      http.listen((req) async {
        seen.add(req);
        heads.add({
          for (final h in ['x-sha256', 'x-size', 'content-range', 'x-file-name', 'x-taken-at', 'content-type']) h: req.headers.value(h),
        });
        await answer(req);
      });
    });
    tearDown(() => http.close(force: true));

    Future<void> reply(HttpRequest req, int status, Object body) async {
      await req.drain<void>();
      req.response
        ..statusCode = status
        ..headers.contentType = ContentType.json
        ..write(jsonEncode(body));
      await req.response.close();
    }

    ServerPhotoBackupApi api() => ServerPhotoBackupApi(ApiClient(address: 'localhost:${http.port}', token: 't'));

    test('a piece carries the hash, the size and its range; the name and the time go with the first piece only, percent-encoded', () async {
      answer = (req) => reply(req, 202, {'received': 3});
      final first = await api().putPiece(
          clientId: 'abc', sha256: 'ff' * 32, size: 5, mime: 'image/jpeg', start: 0, bytes: [1, 2, 3], name: 'Ölberg 1.jpg', takenAt: DateTime.utc(2026));
      expect(first, isA<PiecePartial>().having((p) => p.received, 'received', 3));
      expect(seen.single.uri.path, '/api/v1/photos/upload/abc');
      expect(heads.single['x-sha256'], 'ff' * 32);
      expect(heads.single['x-size'], '5');
      expect(heads.single['content-range'], 'bytes 0-2/5');
      expect(Uri.decodeComponent(heads.single['x-file-name']!), 'Ölberg 1.jpg');
      expect(heads.single['x-taken-at'], '${DateTime.utc(2026).millisecondsSinceEpoch}');
      expect(heads.single['content-type'], 'image/jpeg');

      answer = (req) => reply(req, 201, {'photo': 7, 'known': false});
      final done = await api().putPiece(clientId: 'abc', sha256: 'ff' * 32, size: 5, mime: 'image/jpeg', start: 3, bytes: [4, 5], name: 'x.jpg');
      expect(done, isA<PieceDone>().having((d) => d.known, 'known', false));
      expect(heads.last['content-range'], 'bytes 3-4/5');
      expect(heads.last['x-file-name'], isNull);
    });

    test('a refusal of a piece is an answer with its code; wrong_offset says where to go on', () async {
      answer = (req) => reply(req, 409, {'error': 'wrong_offset', 'received': 10});
      final r = await api().putPiece(clientId: 'abc', sha256: 'ff' * 32, size: 20, mime: 'image/jpeg', start: 0, bytes: [1]);
      expect(r, isA<PieceRefused>().having((r) => r.code, 'code', 'wrong_offset').having((r) => r.received, 'received', 10));
    });

    test('a server with the backup off has no settings; a refused choice says why and nothing else', () async {
      answer = (req) => reply(req, 404, {'error': 'photo_sync_off'});
      expect(await api().settings(), isNull);

      answer = (req) => reply(req, 422, {'error': 'refused', 'refused': {'mode': 'no_bisync'}});
      await expectLater(api().putSettings({'mode': 'bi'}), throwsA(isA<PhotoChoiceRefused>().having((e) => e.refused, 'refused', {'mode': 'no_bisync'})));
    });

    test('the settings are read with the locked options and the reasons', () async {
      answer = (req) => reply(req, 200, {
            'server': {'bisync': false, 'videos': true, 'max_file_mb': 100},
            'folder': true,
            'active': true,
            'settings': {'enabled': 'on', 'mode': 'uni'},
            'effective': {'enabled': 'on', 'mode': 'uni', 'network': 'any', 'charging': 'always', 'videos': 'off'},
            'locked': {
              'mode': [
                {'option': 'bi', 'why': 'no_bisync'},
              ],
            },
          });
      final s = (await api().settings())!;
      expect(s.locked, {'mode': {'bi': 'no_bisync'}});
      expect(s.maxFileMb, 100);
      expect(s.wifiOnly, isFalse);
      expect(s.chargingOnly, isFalse);
      expect(s.videosOn, isFalse);
    });
  });
}
