// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io';
import 'dart:math';

import 'package:crypto/crypto.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/photos/photo_backup_api.dart';
import 'package:psalm/photos/photo_backup_controller.dart';
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
  String mode = 'uni';
  String deletes = 'never';

  /// The trash: the hashes that went in, and the events for the other devices.
  final trashed = <String>[];
  final events = <PhotoEvent>[];
  int removalsCalls = 0;

  /// The pictures of the other devices (bisync), with their bytes and previews.
  final remote = <RemotePhoto>[];
  final remoteBytes = <int, List<int>>{};
  final thumbs = <int, List<int>>{};
  int changesCalls = 0;

  /// After this many pieces the connection "breaks" (once).
  int? breakAfterPieces;

  /// The next complete file is answered with `hash_mismatch` (once).
  bool mismatchOnce = false;

  @override
  Future<PhotoSettings?> settings() async => PhotoSettings(
        folder: true,
        active: active,
        settings: {'enabled': active ? 'on' : 'off', 'videos': 'on', 'mode': mode, 'deletes': deletes},
        effective: {'enabled': active ? 'on' : 'off', 'videos': 'on', 'network': 'wifi', 'charging': 'charging', 'mode': mode, 'deletes': deletes},
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

  @override
  Future<void> putThumb(int photo, List<int> jpeg) async => thumbs[photo] = jpeg;

  @override
  Future<({List<RemotePhoto> photos, int next})> changes(int since, {int limit = 200}) async {
    changesCalls++;
    if (mode != 'bi') throw ApiException(409, 'not_bisync');
    final page = remote.where((r) => r.id > since).take(limit).toList();
    return (photos: page, next: page.isEmpty ? since : page.last.id);
  }

  @override
  Future<List<int>?> thumb(int photo) async => thumbs[photo];

  @override
  Future<void> download(int photo, File dest, String sha256) async {
    final b = remoteBytes[photo]!;
    if (sha256Of(b) != sha256) throw const HttpException('the download does not match its hash');
    dest
      ..parent.createSync(recursive: true)
      ..writeAsBytesSync(b);
  }

  @override
  Future<List<int>> trash(List<String> sha256) async {
    if (mode != 'bi' || deletes != 'trash') throw ApiException(409, 'no_trash');
    trashed.addAll(sha256);
    return [for (var i = 0; i < sha256.length; i++) i + 1];
  }

  @override
  Future<List<TrashedPhoto>> trashList() async => [];

  @override
  Future<void> restore(int photo) async {}

  @override
  Future<({List<PhotoEvent> events, int next})> removals(int? since) async {
    removalsCalls++;
    final head = events.isEmpty ? 0 : events.last.seq;
    if (since == null) return (events: <PhotoEvent>[], next: head);
    final page = events.where((e) => e.seq > since).toList();
    return (events: page, next: page.isEmpty ? since : page.last.seq);
  }

  /// A picture that another device of the person sent.
  RemotePhoto addRemote(int id, List<int> bytes, {DateTime? taken, bool thumb = true}) {
    final r = RemotePhoto(id: id, sha256: sha256Of(bytes), size: bytes.length, name: 'remote-$id.jpg', mime: 'image/jpeg', takenAt: taken ?? DateTime(2026, 11, 8), thumb: thumb);
    remote.add(r);
    remoteBytes[id] = bytes;
    if (thumb) thumbs[id] = [0xFF, 0xD8, 0xFF, id];
    return r;
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

  final gallery = <String, List<int>>{};
  bool full = true;
  final deletedIds = <String>[];

  @override
  Future<bool> fullAccess() async => full;

  @override
  Future<bool> exists(String id) async => photos.any((p) => p.id == id) || hidden.contains(id);

  /// Pictures that are on the device but not among the chosen ones (another folder).
  final hidden = <String>{};

  @override
  Future<List<String>> delete(List<String> ids) async {
    deletedIds.addAll(ids);
    photos.removeWhere((p) => ids.contains(p.id));
    return ids;
  }

  @override
  Future<LocalPhoto?> saveToGallery(File file, {required String name, required String mime, required DateTime takenAt}) async {
    final bytes = file.readAsBytesSync();
    final copy = File('${file.parent.path}/gallery-$name')..writeAsBytesSync(bytes);
    gallery[name] = bytes;
    final p = LocalPhoto(id: 'gallery-$name', name: name, mime: mime, takenAt: takenAt, modified: takenAt, open: () async => copy);
    photos.add(p);
    return p;
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
    return LocalPhoto(id: name, name: name, mime: 'image/jpeg', takenAt: at, modified: at, open: () async => f, thumbnail: () async => [0xFF, 0xD8, 0xFF, 0]);
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

  test('a picture that was sent gets its preview on the server; one the server had already does not', () async {
    final server = FakeServer();
    final b = bytes(200, 10);
    server.kept[sha256Of(b)] = b;
    final lib = FakeLibrary([photo('a.jpg', bytes(100, 11)), photo('b.jpg', b)]);
    await PhotoSync(api: server, library: lib, store: store).run();
    expect(server.thumbs.keys, [server.kept.length]);
  });

  test('with backup only, nothing is received and the server is not even asked', () async {
    final server = FakeServer()..addRemote(1, bytes(10, 12));
    await PhotoSync(api: server, library: FakeLibrary([]), store: store).run();
    expect(server.changesCalls, 0);
    expect(store.load().received, isEmpty);
  });

  test('bisync in the app: the previews of the other devices, from the day the device was switched on, the own pictures not again', () async {
    final server = FakeServer()..mode = 'bi';
    final own = bytes(300, 13);
    server.addRemote(1, bytes(100, 14));
    server.addRemote(2, own); // the same picture is on this device
    server.addRemote(3, bytes(100, 15), taken: DateTime(2025, 1, 1)); // before the device was switched on
    server.addRemote(4, bytes(100, 16), thumb: false);
    final lib = FakeLibrary([photo('own.jpg', own)]);

    final first = await PhotoSync(api: server, library: lib, store: store).run();
    expect(first.stop, isNull);
    expect(first.received, 2);
    final st = store.load();
    expect(st.received.map((r) => r.id), [1, 4]);
    expect(st.received.first.thumb, isTrue);
    expect(store.thumbFile(1).readAsBytesSync(), server.thumbs[1]);
    expect(st.received.last.thumb, isFalse, reason: 'no preview on the server: a placeholder');
    expect(st.changesSince, 4);
    expect(lib.gallery, isEmpty, reason: 'in the app only: the gallery stays as it is');

    expect((await PhotoSync(api: server, library: lib, store: store).run()).received, 0);

    // "Also the older ones" reads the changes again from the start, and only what is new comes.
    store.saveSettings(store.loadSettings()..olderToo = true);
    final older = await PhotoSync(api: server, library: lib, store: store).run();
    expect(older.received, 1);
    expect(store.load().received.map((r) => r.id), [1, 4, 3]);
  });

  test('bisync into the gallery: the original, checked, in the gallery, and the backup does not send it back', () async {
    final server = FakeServer()..mode = 'bi';
    final pic = bytes(5000, 17);
    server.addRemote(9, pic);
    store.saveSettings(store.loadSettings()..receive = 'gallery');
    final lib = FakeLibrary([]);

    final first = await PhotoSync(api: server, library: lib, store: store).run();
    expect(first.received, 1);
    expect(lib.gallery['remote-9.jpg'], pic);
    expect(store.load().received.single.gallery, isTrue);
    expect(store.receivedDir.listSync().whereType<File>().where((f) => f.path.contains('incoming')), isEmpty);

    server.pieces.clear();
    server.haveCalls.clear();
    final second = await PhotoSync(api: server, library: lib, store: store).run();
    expect(second.sent + second.known, 0);
    expect(server.haveCalls, isEmpty, reason: 'the copy in the gallery is known on this device');
  });

  test('a download that does not fit its hash stops the run, and the same picture comes next time', () async {
    final server = FakeServer()..mode = 'bi';
    server.addRemote(5, bytes(100, 18));
    server.remoteBytes[5] = bytes(100, 19); // broken on the way
    store.saveSettings(store.loadSettings()..receive = 'gallery');
    final lib = FakeLibrary([]);

    final first = await PhotoSync(api: server, library: lib, store: store).run();
    expect(first.stop, PhotoRunStop.network);
    expect(store.load().received, isEmpty);
    expect(store.load().changesSince, 0);

    server.remoteBytes[5] = bytes(100, 18);
    expect((await PhotoSync(api: server, library: lib, store: store).run()).received, 1);
  });

  group('the trash', () {
    test('a picture deleted on the device goes into the trash of the server; one only outside of the chosen pictures does not', () async {
      final server = FakeServer()
        ..mode = 'bi'
        ..deletes = 'trash';
      final a = bytes(100, 20), b = bytes(100, 21);
      final lib = FakeLibrary([photo('a.jpg', a), photo('b.jpg', b)]);
      await PhotoSync(api: server, library: lib, store: store).run();

      lib.photos.removeWhere((p) => p.id == 'a.jpg'); // deleted
      lib.photos.removeWhere((p) => p.id == 'b.jpg');
      lib.hidden.add('b.jpg'); // still on the device, only not among the chosen pictures
      final r = await PhotoSync(api: server, library: lib, store: store).run();
      expect(r.trashed, 1);
      expect(server.trashed, [sha256Of(a)]);
      expect(store.load().entries.values.map((e) => e.sha256), [sha256Of(b)]);

      expect((await PhotoSync(api: server, library: lib, store: store).run()).trashed, 0, reason: 'once only');
    });

    test('without "deleting into the trash", or with access to only some pictures, nothing goes into the trash', () async {
      final server = FakeServer()..mode = 'bi';
      final lib = FakeLibrary([photo('a.jpg', bytes(100, 22))]);
      await PhotoSync(api: server, library: lib, store: store).run();
      lib.photos.clear();
      await PhotoSync(api: server, library: lib, store: store).run();
      expect(server.trashed, isEmpty);
      expect(server.removalsCalls, 0);

      server.deletes = 'trash';
      lib.full = false;
      await PhotoSync(api: server, library: lib, store: store).run();
      expect(server.trashed, isEmpty);
      expect(store.load().entries, hasLength(1));
    });

    test('many pictures gone at once wait for the person', () async {
      final server = FakeServer()
        ..mode = 'bi'
        ..deletes = 'trash';
      final lib = FakeLibrary([for (var i = 0; i < kPhotoTrashAsk + 1; i++) photo('p$i.jpg', bytes(50, 100 + i))]);
      await PhotoSync(api: server, library: lib, store: store).run();
      lib.photos.clear();

      await PhotoSync(api: server, library: lib, store: store).run();
      expect(server.trashed, isEmpty);
      expect(store.load().toTrash, hasLength(kPhotoTrashAsk + 1));
      await PhotoSync(api: server, library: lib, store: store).run();
      expect(store.load().toTrash, hasLength(kPhotoTrashAsk + 1), reason: 'not twice on the list');

      final c = PhotoBackupController(api: server, library: lib, store: store, schedule: (_, _, _) async {});
      await c.trashGone();
      expect(server.trashed, hasLength(kPhotoTrashAsk + 1));
      expect(store.load().toTrash, isEmpty);
      expect(store.load().entries, isEmpty);
    });

    test('what another device deleted: a preview goes at once, a picture of the gallery only when the person says so; a restore takes it back', () async {
      final server = FakeServer()
        ..mode = 'bi'
        ..deletes = 'trash';
      final mine = bytes(100, 30), other = bytes(100, 31), third = bytes(100, 32);
      server.addRemote(1, other);
      final lib = FakeLibrary([photo('mine.jpg', mine), photo('third.jpg', third)]);
      await PhotoSync(api: server, library: lib, store: store).run();
      expect(store.load().received.map((r) => r.id), [1]);
      expect(store.load().removalsSince, 0, reason: 'starts now: nothing of before');

      server.events
        ..add(PhotoEvent(1, 1, sha256Of(other), 'trash'))
        ..add(PhotoEvent(2, 7, sha256Of(mine), 'trash'))
        ..add(PhotoEvent(3, 8, sha256Of(third), 'trash'))
        ..add(PhotoEvent(4, 8, sha256Of(third), 'restore'));
      await PhotoSync(api: server, library: lib, store: store).run();
      final st = store.load();
      expect(st.received, isEmpty, reason: 'the preview of the app goes without asking');
      expect(store.thumbFile(1).existsSync(), isFalse);
      expect(st.toDelete.map((g) => g.assetId), ['mine.jpg'], reason: 'restored again: not asked for');
      expect(lib.deletedIds, isEmpty, reason: 'nothing in the gallery without the person');
      expect(st.removalsSince, 4);

      final c = PhotoBackupController(api: server, library: lib, store: store, schedule: (_, _, _) async {});
      await c.deleteHereToo();
      expect(lib.deletedIds, ['mine.jpg']);
      expect(store.load().toDelete, isEmpty);
      expect(store.load().entries.values.map((e) => e.sha256), [sha256Of(third)]);
      await PhotoSync(api: server, library: lib, store: store).run();
      expect(server.trashed, isEmpty, reason: 'deleted because of the other device: not sent to the trash again');
    });
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

    test('the changes are asked for with since in the query; a missing preview is null; a download that does not fit its hash is dropped', () async {
      answer = (req) async {
        if (req.uri.path == '/api/v1/photos/changes') {
          return reply(req, 200, {
            'photos': [
              {'id': 7, 'sha256': 'ab', 'size': 3, 'name': 'x.jpg', 'mime': 'image/jpeg', 'taken_at': 1000, 'uploaded_at': 2000, 'thumb': true},
            ],
            'next': 7,
          });
        }
        if (req.uri.path == '/api/v1/photos/7/thumb') return reply(req, 404, {'error': 'no_thumb'});
        await req.drain<void>();
        req.response
          ..statusCode = 200
          ..add([1, 2, 3]);
        await req.response.close();
      };
      final c = await api().changes(5);
      expect(seen.single.uri.queryParameters['since'], '5');
      expect((c.next, c.photos.single.id, c.photos.single.thumb), (7, 7, true));
      expect(await api().thumb(7), isNull);
      final dest = File('${Directory.systemTemp.createTempSync('dl-').path}/x.jpg');
      await expectLater(api().download(7, dest, 'ff' * 32), throwsA(isA<HttpException>()));
      expect(dest.existsSync(), isFalse);
      expect(dest.parent.listSync(), isEmpty, reason: 'no part file is left');
      await api().download(7, dest, sha256Of([1, 2, 3]));
      expect(dest.readAsBytesSync(), [1, 2, 3]);
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
