// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/m3u.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/playlists.dart';
import 'package:psalm/screens/playlists_screen.dart';
import 'package:psalm/server/api_client.dart';
import 'package:psalm/server/playlist_api.dart';
import 'package:psalm/server/server_source.dart';

import 'playlist_fakes.dart';

void main() {
  group('model and JSON', () {
    test('the list of playlists', () {
      final s = PlaylistSummary.fromJson({
        'id': 3,
        'name': 'Car trip',
        'items': 12,
        'seconds': 3420,
        'updated_at': 1793280000000,
      });
      expect((s.id, s.name, s.items, s.total), (3, 'Car trip', 12, const Duration(seconds: 3420)));
      expect(s.updatedAt, DateTime.fromMillisecondsSinceEpoch(1793280000000));
      expect(PlaylistSummary.fromJson(s.toJson()).name, 'Car trip');
    });

    test('an available entry has a part and a book; an unavailable one has neither', () {
      final p = Playlist.fromJson({
        'id': 3,
        'name': 'Car trip',
        'items': [
          {
            'item_id': 41,
            'position': 1,
            'available': true,
            'part': {
              'id': 880,
              'disc': 1,
              'number': 2,
              'title': 'Chapter 2',
              'duration_ms': 240000,
              'plays': 1,
              'last_played_at': 1793000000000,
            },
            'book': {'id': 17, 'title': 'Winter', 'author': 'Follett'},
          },
          {'item_id': 42, 'position': 2, 'available': false, 'part': null, 'book': null},
        ],
      });
      final a = p.entries[0];
      expect(
        (
          a.entryId,
          a.available,
          a.partId,
          a.title,
          a.duration,
          a.bookId,
          a.bookTitle,
          a.bookAuthor,
        ),
        (41, true, 880, 'Chapter 2', const Duration(minutes: 4), 17, 'Winter', 'Follett'),
      );
      final u = p.entries[1];
      expect((u.entryId, u.available, u.partId, u.bookId), (42, false, null, null));
      expect(p.available.length, 1);
      expect(p.total, const Duration(minutes: 4));
      final again = Playlist.fromJson(jsonDecode(jsonEncode(p.toJson())) as Map<String, dynamic>);
      expect(again.entries.map((e) => e.available), [true, false]);
      expect(again.entries[0].title, 'Chapter 2');
    });

    test('every error code of the server has a text in English and German', () async {
      final en = await AppLocalizations.delegate.load(const Locale('en'));
      final de = await AppLocalizations.delegate.load(const Locale('de'));
      for (final c in kPlaylistErrorCodes) {
        final a = playlistErrorText(en, c), b = playlistErrorText(de, c);
        expect(a, isNot(contains(c)), reason: c);
        expect(b, isNot(contains(c)), reason: c);
        expect(a, isNot(b), reason: '$c is translated');
      }
      expect(playlistErrorText(en, 'strange'), 'The playlist could not be changed (strange).');
      expect(
        playlistErrorText(de, 'strange'),
        'Die Playlist konnte nicht geändert werden (strange).',
      );
    });
  });

  group('copy on the device', () {
    late Directory dir;
    late File file;
    setUp(() {
      dir = Directory.systemTemp.createTempSync('pl');
      file = File('${dir.path}/playlists.json');
    });
    tearDown(() => dir.deleteSync(recursive: true));

    test('written and read back, with the entries and where each list was left', () {
      final store = PlaylistStore(file);
      store.write(
        PlaylistCopy(
          etag: '"abc"',
          lists: const [PlaylistSummary(id: 3, name: 'Car trip', items: 1, seconds: 240)],
          details: {
            3: Playlist(
              id: 3,
              name: 'Car trip',
              entries: [entry(41, 1, 880, 'Chapter 2'), entry(42, 2, 0, '', available: false)],
            ),
          },
          resume: {3: (partId: 880, position: const Duration(seconds: 61))},
        ),
      );
      final c = store.read();
      expect((c.etag, c.lists.single.name), ('"abc"', 'Car trip'));
      expect(c.details[3]!.entries.map((e) => e.available), [true, false]);
      expect(c.resume[3], (partId: 880, position: const Duration(seconds: 61)));
    });

    test('missing or damaged is an empty copy', () {
      expect(PlaylistStore(file).read().lists, isEmpty);
      file.writeAsStringSync('{ not json');
      expect(PlaylistStore(file).read().lists, isEmpty);
      file.writeAsStringSync('{"lists": "wrong"}');
      expect(PlaylistStore(file).read().lists, isEmpty);
    });

    test('the controller starts from the copy, without a server', () {
      PlaylistStore(file).write(
        PlaylistCopy(
          lists: const [PlaylistSummary(id: 3, name: 'Car trip')],
          details: {
            3: Playlist(id: 3, name: 'Car trip', entries: [entry(41, 1, 880, 'Chapter 2')]),
          },
        ),
      );
      final c = PlaylistController(store: PlaylistStore(file));
      expect(c.lists.single.name, 'Car trip');
      expect(c.details[3]!.entries.single.title, 'Chapter 2');
    });

    test(
      'a loaded list is written to the copy; opened without a server it comes from the copy',
      () async {
        final api = FakePlaylistApi()..add('Car trip', [entry(41, 1, 880, 'Chapter 2')]);
        final c = PlaylistController(api: api, store: PlaylistStore(file));
        await c.load();
        await c.open(1);
        final offline = PlaylistController(store: PlaylistStore(file));
        expect(offline.lists.single.name, 'Car trip');
        expect((await offline.open(1))!.entries.single.title, 'Chapter 2');
      },
    );
  });

  group('controller', () {
    late FakePlaylistApi api;
    late PlaylistController c;
    setUp(() {
      api = FakePlaylistApi();
      c = PlaylistController(api: api);
    });

    test('load sends the ETag it has; 304 leaves the lists alone', () async {
      api.add('Car trip');
      await c.load();
      expect(c.lists.map((l) => l.name), ['Car trip']);
      expect(api.lastEtagSent, isNull);
      api.calls.clear();
      await c.load();
      expect(api.lastEtagSent, 'v1');
      expect(c.lists.map((l) => l.name), ['Car trip']);
      api.etag = 'v2';
      api.add('Bed time');
      await c.load();
      expect(c.lists.map((l) => l.name), ['Car trip', 'Bed time']);
    });

    test(
      'the entries of the lists that were opened are loaded again (availability may have changed)',
      () async {
        final p = api.add('Car trip', [entry(41, 1, 880, 'Chapter 2')]);
        await c.load();
        await c.open(p.id);
        api.lists[p.id] = p.copyWith(entries: [entry(41, 1, 0, '', available: false)]);
        await c.load();
        expect(c.details[p.id]!.entries.single.available, isFalse);
      },
    );

    test('a list that is gone from the server is gone from the copy', () async {
      final p = api.add('Car trip');
      await c.load();
      await c.open(p.id);
      api.lists.remove(p.id);
      api.etag = 'v9';
      await c.load();
      expect(c.lists, isEmpty);
      expect(c.details, isEmpty);
    });

    test('a server that does not answer keeps the copy and says why', () async {
      api.add('Car trip');
      await c.load();
      api.failWith = ApiException(0, 'network');
      await c.load();
      expect(c.lists.single.name, 'Car trip');
      expect(c.loadError, 'network');
    });

    test('create: the new list shows at once; a taken name is refused with the code', () async {
      final id = await c.create('Car trip');
      expect(c.lists.single.name, 'Car trip');
      expect(id, 1);
      await expectLater(
        c.create('car TRIP'),
        throwsA(isA<ApiException>().having((e) => e.code, 'code', 'name_taken')),
      );
      await expectLater(
        c.create('  '),
        throwsA(isA<ApiException>().having((e) => e.code, 'code', 'invalid_name')),
      );
    });

    test('add parts and add a whole book (all parts, in the book\'s order)', () async {
      final id = await c.create('Car trip');
      var p = await c.addParts(id, [881]);
      expect(p.entries.map((e) => e.title), ['Chapter 2']);
      p = await c.addBook(id, 17);
      expect(p.entries.map((e) => e.title), [
        'Chapter 2',
        'Chapter 1',
        'Chapter 2',
      ], reason: 'duplicates are allowed');
      expect(c.details[id]!.entries.length, 3);
      expect(c.lists.single.items, 3);
      await expectLater(
        c.addParts(id, [1]),
        throwsA(isA<ApiException>().having((e) => e.code, 'code', 'unknown_part')),
      );
      await expectLater(
        c.addBook(id, 99),
        throwsA(isA<ApiException>().having((e) => e.code, 'code', 'unknown_book')),
      );
    });

    test('remove an entry: the others are numbered again', () async {
      final id = await c.create('Car trip');
      await c.addParts(id, [880, 881, 900]);
      await c.removeEntry(id, c.details[id]!.entries[1].entryId);
      expect(c.details[id]!.entries.map((e) => (e.position, e.title)), [
        (1, 'Chapter 1'),
        (2, 'Song'),
      ]);
    });

    test('reorder: the list follows at once and the server gets every entry id once', () async {
      final id = await c.create('Car trip');
      await c.addParts(id, [880, 881, 900]);
      final ids = c.details[id]!.entries.map((e) => e.entryId).toList();
      await c.reorder(id, 0, 2);
      expect(c.details[id]!.entries.map((e) => e.title), ['Chapter 2', 'Song', 'Chapter 1']);
      expect(api.calls.last, 'reorder $id [${ids[1]}, ${ids[2]}, ${ids[0]}]');
    });

    test(
      'reorder refused by the server: the old order comes back and the code is thrown',
      () async {
        final id = await c.create('Car trip');
        await c.addParts(id, [880, 881]);
        final before = c.details[id]!.entries.map((e) => e.title).toList();
        api.failOnce = ApiException(422, 'invalid_order');
        await expectLater(
          c.reorder(id, 0, 1),
          throwsA(isA<ApiException>().having((e) => e.code, 'code', 'invalid_order')),
        );
        expect(c.details[id]!.entries.map((e) => e.title), before);
      },
    );

    test('rename and delete', () async {
      final id = await c.create('Car trip');
      await c.rename(id, '  Road  ');
      expect(c.lists.single.name, 'Road');
      await c.delete(id);
      expect(c.lists, isEmpty);
    });

    test('the playlist plays as one virtual book: unavailable entries are left out, parts keep their own book', () async {
      final p = Playlist(
        id: 3,
        name: 'Car trip',
        entries: [
          entry(41, 1, 880, 'Chapter 1'),
          entry(42, 2, 0, '', available: false),
          entry(43, 3, 900, 'Song', bookId: 20, book: 'Album', author: 'Band'),
        ],
      );
      final b = c.bookOf(p);
      expect((b.id, b.kind, b.title), ('pl:3', 'playlist', 'Car trip'));
      expect(b.tracks.map((t) => t.title), [
        'Chapter 1',
        'Song',
      ], reason: 'the unavailable one is skipped, not deleted');
      expect(b.tracks.map((t) => t.itemId), ['17', '20']);
      expect(b.tracks.map((t) => t.partId), [880, 900]);
      expect(p.entries.length, 3);
    });

    test('offline the playlist offers only the tracks that are on the device', () {
      final p = Playlist(
        id: 3,
        name: 'Car trip',
        entries: [entry(41, 1, 880, 'Chapter 1'), entry(43, 2, 900, 'Song', bookId: 20)],
      );
      var offline = true;
      c.offers = (t) => !offline || t.partId == 900;
      expect(c.bookOf(p).tracks.map((t) => t.partId), [900]);
      offline = false;
      expect(c.bookOf(p).tracks.map((t) => t.partId), [880, 900], reason: 'online everything is offered again');
    });

    test('where the list was left is found again by the part, also after a reorder', () {
      final p = Playlist(
        id: 3,
        name: 'Car trip',
        entries: [entry(41, 1, 880, 'Chapter 1'), entry(43, 2, 900, 'Song', bookId: 20)],
      );
      final b = c.bookOf(p);
      expect(c.startOf(p, b), (track: 0, position: Duration.zero));
      c.remember(b, b.tracks[1], const Duration(seconds: 30));
      expect(c.startOf(p, b), (track: 1, position: const Duration(seconds: 30)));
      final reordered = Playlist(
        id: 3,
        name: 'Car trip',
        entries: [entry(43, 1, 900, 'Song', bookId: 20), entry(41, 2, 880, 'Chapter 1')],
      );
      expect(c.startOf(reordered, c.bookOf(reordered)).track, 0);
      final realBook = LibraryItem(
        id: '17',
        title: 'x',
        author: 'y',
        color: Colors.black,
        tracks: const [],
      );
      c.remember(
        realBook,
        const Track(disc: 1, number: 1, title: 't', length: Duration.zero, partId: 5),
        Duration.zero,
      );
      expect(c.startOf(p, b).track, 1, reason: 'a normal book is not remembered here');
    });
  });

  group('player', () {
    test(
      'the shown book is the one of the track that plays; the position of a playlist is passed on',
      () {
        final real = LibraryItem(
          id: '20',
          title: 'Album',
          author: 'Band',
          color: Colors.black,
          tracks: const [
            Track(disc: 1, number: 1, title: 'Song', length: Duration(minutes: 3), partId: 900),
          ],
        );
        final pl = PlaylistController();
        final book = pl.bookOf(
          Playlist(
            id: 3,
            name: 'Car trip',
            entries: [entry(41, 1, 900, 'Song', bookId: 20, book: 'Album', author: 'Band')],
          ),
        );
        final p = PlayerController([real]);
        LibraryItem? got;
        Duration? at;
        p.onPlaylistPosition = (b, t, pos) {
          got = b;
          at = pos;
        };
        p.open(book, track: 0, start: const Duration(seconds: 7));
        expect(p.shownBook?.id, '20');
        expect(p.book?.id, 'pl:3');
        p.flush();
        expect((got?.id, at), ('pl:3', const Duration(seconds: 7)));
        p.dispose();
      },
    );

    test('progress goes to the real book of the part, as always', () async {
      final seen = <Map<String, dynamic>>[];
      final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      addTearDown(() => server.close(force: true));
      server.listen((req) async {
        seen.add(jsonDecode(await utf8.decodeStream(req)) as Map<String, dynamic>);
        req.response.statusCode = 204;
        await req.response.close();
      });
      final api = ApiClient(address: 'localhost:${server.port}', token: 't');
      final source = ServerSource(api, Directory.systemTemp.createTempSync('c'));
      final book = PlaylistController().bookOf(
        Playlist(id: 3, name: 'Car trip', entries: [entry(41, 1, 900, 'Song', bookId: 20)]),
      );
      await source.saveProgress(
        book,
        book.tracks.single,
        const Duration(seconds: 5),
        finished: false,
      );
      expect(seen.single['item_id'], 20);
      expect(seen.single['part_id'], 900);
      api.close();
    });
  });

  group('playlist file (M3U8)', () {
    test('header, one #EXTINF with the seconds and "artist - title", then the path; in order', () {
      final text = buildM3u8(const [
        M3uEntry(
          duration: Duration(milliseconds: 240400),
          artist: 'Follett',
          title: 'Chapter 2',
          path: '881.audio',
        ),
        M3uEntry(
          duration: Duration(milliseconds: 59600),
          artist: 'Band',
          title: 'Song',
          path: '900.audio',
        ),
      ]);
      expect(
        text,
        '#EXTM3U\n#EXTINF:240,Follett - Chapter 2\n881.audio\n#EXTINF:60,Band - Song\n900.audio\n',
      );
    });

    test('no artist: the title alone; unknown length: -1', () {
      expect(
        buildM3u8(const [M3uEntry(duration: null, artist: '', title: 'Live', path: 'a.audio')]),
        '#EXTM3U\n#EXTINF:-1,Live\na.audio\n',
      );
    });

    test('special characters stay, a line break cannot break the file', () {
      final text = buildM3u8(const [
        M3uEntry(
          duration: Duration(seconds: 1),
          artist: 'Björk, "B"',
          title: 'Ünï\r\ncode ♪\t1',
          path: '1.audio',
        ),
      ]);
      expect(text.split('\n'), ['#EXTM3U', '#EXTINF:1,Björk, "B" - Ünï code ♪ 1', '1.audio', '']);
      expect(utf8.decode(utf8.encode(text)), text);
    });

    test('an empty list is just the header', () {
      expect(buildM3u8(const []), '#EXTM3U\n');
    });
  });

  group('downloads write the playlist file', () {
    test('a book and a playlist: only the parts on the device, relative paths, the file goes with the download', () async {
      final source = _FakeSource();
      final p = PlayerController([], source: source);
      final book = LibraryItem(
        id: '17',
        title: 'Winter',
        author: 'Follett',
        color: Colors.black,
        tracks: const [
          Track(disc: 1, number: 1, title: 'One', length: Duration(seconds: 100), partId: 880),
          Track(disc: 1, number: 2, title: 'Two', length: Duration(seconds: 200), partId: 881),
        ],
      );
      await p.downloadItem(book);
      expect(source.files['book-17'], hasLength(2));
      expect(source.files['book-17']!.first.path, '880.audio');
      expect(source.files['book-17']!.first.artist, 'Follett');
      expect(source.files['book-17']!.map((e) => e.title), ['One', 'Two']);

      final pl = PlaylistController().bookOf(
        Playlist(
          id: 3,
          name: 'Car trip',
          entries: [
            entry(41, 1, 881, 'Two', author: 'Follett'),
            entry(43, 2, 900, 'Song', bookId: 20, author: 'Band'),
          ],
        ),
      );
      await p.downloadItem(pl);
      expect(source.files['playlist-3']!.map((e) => (e.path, e.artist)), [
        ('881.audio', 'Follett'),
        ('900.audio', 'Band'),
      ]);

      await p.removeDownload(book);
      expect(source.files.containsKey('book-17'), isFalse, reason: 'no part left: no file');
      p.dispose();
    });

    test('a partly downloaded book lists what is there', () async {
      final source = _FakeSource()..have.add(880);
      final p = PlayerController([], source: source);
      final book = LibraryItem(
        id: '17',
        title: 'Winter',
        author: 'Follett',
        color: Colors.black,
        tracks: const [
          Track(disc: 1, number: 1, title: 'One', length: Duration(seconds: 100), partId: 880),
          Track(disc: 1, number: 2, title: 'Two', length: Duration(seconds: 200), partId: 881),
        ],
      );
      source.failOn = 881;
      await p.downloadItem(book);
      expect(source.files['book-17']!.map((e) => e.path), ['880.audio']);
      p.dispose();
    });

    test('an episode or a station gets no playlist file', () {
      final p = PlayerController([]);
      expect(
        p.m3uNameOf(
          LibraryItem(
            id: 'ep:5',
            title: 'e',
            author: 'a',
            color: Colors.black,
            kind: 'podcast',
            tracks: const [],
          ),
        ),
        isNull,
      );
      expect(
        p.m3uNameOf(
          LibraryItem(
            id: 'st:5',
            title: 's',
            author: 'a',
            color: Colors.black,
            kind: 'radio',
            tracks: const [],
          ),
        ),
        isNull,
      );
      p.dispose();
    });
  });

  group('the server interface', () {
    late HttpServer server;
    late ServerPlaylistApi api;
    final requests = <String>[];
    final headers = <String, String?>{};
    late int status;
    late String body;

    setUp(() async {
      requests.clear();
      status = 200;
      body = '[]';
      server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      server.listen((req) async {
        final text = await utf8.decodeStream(req);
        requests.add('${req.method} ${req.uri.path}${text.isEmpty ? '' : ' $text'}');
        headers['if-none-match'] = req.headers.value('if-none-match');
        req.response.statusCode = status;
        if (status == 200 && req.uri.path == '/api/v1/playlists') {
          req.response.headers.set('ETag', '"abc"');
        }
        req.response.write(body);
        await req.response.close();
      });
      api = ServerPlaylistApi(ApiClient(address: 'localhost:${server.port}', token: 't'));
    });
    tearDown(() => server.close(force: true));

    test('the list comes with its ETag; with the ETag a 304 says nothing changed', () async {
      body = '[{"id":3,"name":"Car trip","items":2,"seconds":300,"updated_at":1}]';
      final a = await api.playlists();
      expect((a.lists!.single.name, a.etag), ('Car trip', '"abc"'));
      expect(headers['if-none-match'], isNull);
      status = 304;
      body = '';
      final b = await api.playlists(etag: '"abc"');
      expect((b.lists, b.etag), (null, '"abc"'));
      expect(headers['if-none-match'], '"abc"');
    });

    test('errors are the server\'s codes', () async {
      for (final (st, code) in [
        (409, 'name_taken'),
        (422, 'invalid_name'),
        (422, 'too_many_playlists'),
        (404, 'not_found'),
        (422, 'playlist_full'),
      ]) {
        status = st;
        body = '{"error":"$code"}';
        await expectLater(
          api.createPlaylist('x'),
          throwsA(
            isA<ApiException>()
                .having((e) => e.code, 'code', code)
                .having((e) => e.status, 'status', st),
          ),
        );
      }
    });

    test('the calls: create, rename, delete, add parts, add a book, remove, order', () async {
      body = '{"id":3,"name":"Car trip"}';
      status = 201;
      expect((await api.createPlaylist('Car trip')).id, 3);
      status = 200;
      await api.renamePlaylist(3, 'Road');
      status = 204;
      body = '';
      await api.deletePlaylist(3);
      await api.removeEntry(3, 42);
      status = 200;
      body = '{"id":3,"name":"Car trip","items":[]}';
      await api.addParts(3, [880, 881], at: 2);
      await api.addBook(3, 17);
      await api.reorder(3, [42, 41]);
      expect(requests, [
        'POST /api/v1/playlists {"name":"Car trip"}',
        'PATCH /api/v1/playlists/3 {"name":"Road"}',
        'DELETE /api/v1/playlists/3',
        'DELETE /api/v1/playlists/3/items/42',
        'POST /api/v1/playlists/3/items {"part_ids":[880,881],"at":2}',
        'POST /api/v1/playlists/3/items {"book_id":17}',
        'PUT /api/v1/playlists/3/order {"item_ids":[42,41]}',
      ]);
    });

    test('the version of the library carries the fingerprint of the playlists', () async {
      body = '{"version":"v7","playlists":"p3"}';
      final c = ApiClient(address: 'localhost:${server.port}', token: 't');
      final v = await c.libraryVersions();
      expect((v.version, v.playlists), ('v7', 'p3'));
      body = '{"version":"v7"}';
      expect((await c.libraryVersions()).playlists, isNull);
      c.close();
    });
  });
}

/// A source that keeps the parts in a list and the playlist files in a map.
class _FakeSource extends MediaSource {
  final have = <int>{};
  final files = <String, List<M3uEntry>>{};
  int? failOn;

  @override
  bool get canDownload => true;

  @override
  bool isDownloaded(Track t) => have.contains(t.partId);

  @override
  Future<String> fetchPart(Track t) async {
    if (t.partId == failOn) throw const FileSystemException('disk full');
    have.add(t.partId!);
    return '/cache/${t.partId}.audio';
  }

  @override
  Future<void> removeDownload(Track t) async => have.remove(t.partId);

  @override
  String? relativePath(Track t) => have.contains(t.partId) ? '${t.partId}.audio' : null;

  @override
  Future<void> writeM3u(String name, List<M3uEntry> entries) async {
    if (entries.isEmpty) {
      files.remove(name);
    } else {
      files[name] = entries;
    }
  }

  @override
  Future<void> saveProgress(
    LibraryItem book,
    Track track,
    Duration position, {
    required bool finished,
  }) async {}

  @override
  Future<PassageHit?> resolvePassage(String text) async => null;
}
