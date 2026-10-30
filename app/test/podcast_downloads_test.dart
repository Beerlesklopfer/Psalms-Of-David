// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/models.dart';
import 'package:psalm/podcast_downloads.dart';
import 'package:psalm/podcasts.dart';
import 'package:psalm/server/api_client.dart';
import 'package:psalm/server/server_source.dart';

/// Writes a file of [size] bytes instead of asking the internet; can be held back, and made to fail.
class FakeFetcher implements EpisodeFetcher {
  final calls = <Uri>[];
  int size = 2000;
  Object? failWith;
  Completer<void>? gate;

  @override
  Future<int> fetch(Uri url, File dest, {required int maxBytes, required CancelToken cancel, void Function(int received, int? total)? onProgress}) async {
    calls.add(url);
    onProgress?.call(0, size);
    await gate?.future;
    if (cancel.isCancelled) throw const EpisodeDownloadException('cancelled');
    if (failWith != null) throw failWith!;
    if (size > maxBytes) throw const EpisodeDownloadException('too_large');
    dest.parent.createSync(recursive: true);
    dest.writeAsBytesSync(List.filled(size, 7));
    onProgress?.call(size, size);
    return size;
  }
}

Episode _episode(int id, {String url = 'https://example.org/1.mp3', bool finished = false, int published = 1760000000000, int position = 0}) => Episode(
  id: id,
  title: 'Folge $id',
  audioUrl: url,
  published: DateTime.fromMillisecondsSinceEpoch(published),
  duration: const Duration(minutes: 10),
  position: Duration(seconds: position),
  finished: finished,
);

const _podcast = Podcast(id: 1, title: 'Kinderfunk', description: '', approved: true, unheard: 0);

void main() {
  late Directory root;
  late Directory cache;
  late File manifest;
  late FakeFetcher fetcher;
  late PodcastDownloads downloads;

  PodcastDownloads fresh() => PodcastDownloads(cache: cache, file: manifest, fetcher: fetcher);

  setUp(() {
    root = Directory.systemTemp.createTempSync('podcast-downloads');
    cache = Directory('${root.path}/parts');
    manifest = File('${root.path}/podcast-downloads.json');
    fetcher = FakeFetcher();
    downloads = fresh();
  });
  tearDown(() {
    downloads.dispose();
    root.deleteSync(recursive: true);
  });

  test('a download puts the file in the folder of the downloads and the episode in the list, and the list survives a restart', () async {
    final e = _episode(5);
    expect(downloads.isDownloaded(e.audioUrl), isFalse);
    final seen = <int?>[];
    downloads.addListener(() => seen.add(downloads.progressOf(5)?.received));
    expect(await downloads.download(e, _podcast), isNull);
    expect(seen.first, 0, reason: 'the progress starts at once');
    expect(downloads.isDownloaded(e.audioUrl), isTrue);
    expect(episodeCacheFile(cache, e.audioUrl).existsSync(), isTrue, reason: 'named by the address, like an episode that was played');
    expect((downloads.sizeOf(5), downloads.usage()), (2000, (files: 1, bytes: 2000)));
    expect(downloads.podcasts.single.title, 'Kinderfunk');
    expect(downloads.isDownloading(5), isFalse);

    final again = fresh();
    addTearDown(again.dispose);
    expect(again.episodesOf(1).single.toEpisode().title, 'Folge 5');
    expect(again.episodesOf(1).single.toEpisode().duration, const Duration(minutes: 10));
  });

  test('only http and https addresses are fetched', () async {
    for (final url in ['file:///etc/passwd', 'ftp://example.org/a.mp3', 'content://x/y', 'javascript:alert(1)', 'https:///no-host', '']) {
      expect(await downloads.download(_episode(7, url: url), _podcast), 'scheme', reason: url);
    }
    expect(fetcher.calls, isEmpty);
    expect(downloads.entries, isEmpty);
  });

  test('a failed download leaves nothing behind and says why', () async {
    fetcher.failWith = const EpisodeDownloadException('network', 'no route');
    expect(await downloads.download(_episode(5), _podcast), 'network');
    fetcher.failWith = const EpisodeDownloadException('http', 'HTTP 404');
    expect(await downloads.download(_episode(5), _podcast), 'http');
    expect((downloads.entries.isEmpty, downloads.isDownloaded('https://example.org/1.mp3'), downloads.isDownloading(5)), (true, false, false));
    // The limit of the settings: the largest download is a setting.
    fetcher.failWith = null;
    downloads.setMaxMegabytes(10);
    fetcher.size = 11 * 1024 * 1024;
    expect(await downloads.download(_episode(5), _podcast), 'too_large');
    expect(downloads.entries.isEmpty, isTrue);
    downloads.setMaxMegabytes(100);
    expect(await downloads.download(_episode(5), _podcast), isNull);
  });

  test('a running download can be cancelled', () async {
    fetcher.gate = Completer<void>();
    final running = downloads.download(_episode(5), _podcast);
    await Future<void>.delayed(Duration.zero);
    expect(downloads.isDownloading(5), isTrue);
    downloads.cancel(5);
    fetcher.gate!.complete();
    expect(await running, 'cancelled');
    expect((downloads.isDownloading(5), downloads.entries.isEmpty, downloads.isDownloaded('https://example.org/1.mp3')), (false, true, false));
  });

  test('removing an episode deletes the file and the entry; the entry of a file that is gone is dropped', () async {
    await downloads.download(_episode(5), _podcast);
    await downloads.download(_episode(6, url: 'https://example.org/2.mp3'), _podcast);
    await downloads.remove(5);
    expect((downloads.entryOf(5), downloads.isDownloaded('https://example.org/1.mp3')), (null, false));
    // Deleted from outside (the "delete all downloads" of the about page): the list follows the files.
    episodeCacheFile(cache, 'https://example.org/2.mp3').deleteSync();
    expect(downloads.entries, isEmpty);
  });

  test('delete all takes the episodes that were only played as well, and a half-loaded file of an earlier run is cleaned up', () async {
    await downloads.download(_episode(5), _podcast);
    final played = episodeCacheFile(cache, 'https://example.org/played.mp3')..writeAsBytesSync([1, 2, 3]);
    final half = File('${episodeCacheFile(cache, 'https://example.org/x.mp3').path}.part')..writeAsBytesSync([1]);
    final book = File('${cache.path}/12.audio')..writeAsBytesSync([9]);
    downloads.dispose();
    downloads = fresh();
    expect(half.existsSync(), isFalse, reason: 'cleaned at the start');
    await downloads.removeAll();
    expect((downloads.entries.isEmpty, played.existsSync(), book.existsSync()), (true, false, true), reason: 'the parts of books are not touched');
  });

  test('forgetting the list (the pairing is gone) keeps the files; looking at the episodes again brings them back', () async {
    await downloads.download(_episode(5), _podcast);
    downloads.forgetList();
    expect((downloads.entries.isEmpty, downloads.isDownloaded('https://example.org/1.mp3')), (true, true));
    await downloads.noteListed(_podcast, [_episode(5, position: 30)]);
    expect(downloads.entryOf(5)?.positionMs, 30000);
  });

  test('the state is kept for the offline list; a heard episode is deleted only if the person switched that on', () async {
    await downloads.download(_episode(5), _podcast);
    downloads.noteState(5, const Duration(seconds: 90), false);
    expect((downloads.entryOf(5)?.positionMs, downloads.entryOf(5)?.finished), (90000, false));
    downloads.noteState(5, const Duration(minutes: 10), true);
    expect(downloads.entryOf(5)?.finished, isTrue);
    expect(downloads.isDownloaded('https://example.org/1.mp3'), isTrue, reason: 'off by default: nothing is deleted');

    downloads.setDeleteAfterListening(true);
    downloads.noteState(5, const Duration(minutes: 10), true);
    await Future<void>.delayed(const Duration(milliseconds: 50));
    expect((downloads.entryOf(5), downloads.isDownloaded('https://example.org/1.mp3')), (null, false));
    // The switch is a setting of the device.
    final again = fresh();
    addTearDown(again.dispose);
    expect((again.deleteAfterListening, again.maxMegabytes), (true, 500));
  });

  test('the episodes as the server lists them: played ones get an entry, and with the switch on the heard and the withdrawn ones go', () async {
    // An episode that was played (the player keeps the file) has no entry yet.
    episodeCacheFile(cache, 'https://example.org/1.mp3').createSync(recursive: true);
    episodeCacheFile(cache, 'https://example.org/old.mp3').createSync(recursive: true);
    episodeCacheFile(cache, 'https://example.org/withdrawn.mp3').createSync(recursive: true);
    episodeCacheFile(cache, 'https://example.org/heard.mp3').createSync(recursive: true);
    final listed = [
      _episode(5, published: 1760000000000),
      _episode(2, url: 'https://example.org/old.mp3', published: 1750000000000),
      _episode(3, url: 'https://example.org/heard.mp3', published: 1755000000000, finished: true),
    ];
    // Entries of earlier downloads (the withdrawn one is newer than the oldest listed, the old one is not in the list but older).
    await downloads.download(_episode(8, url: 'https://example.org/withdrawn.mp3', published: 1758000000000), _podcast);
    await downloads.download(_episode(1, url: 'https://example.org/older.mp3', published: 1740000000000), _podcast);
    await downloads.noteListed(_podcast, listed);
    expect(downloads.entryOf(5)?.title, 'Folge 5', reason: 'a played episode is listed');
    expect(downloads.entries.map((e) => e.episodeId).toSet(), {5, 2, 3, 8, 1}, reason: 'switched off: nothing is deleted');

    downloads.setDeleteAfterListening(true);
    await downloads.noteListed(_podcast, listed);
    expect(downloads.entries.map((e) => e.episodeId).toSet(), {5, 2, 1}, reason: 'the heard (3) and the withdrawn (8) go; 1 is older than everything listed and stays');
    expect(episodeCacheFile(cache, 'https://example.org/heard.mp3').existsSync(), isFalse);
  });

  test('the newest unheard episodes that are not on the device are chosen', () async {
    await downloads.download(_episode(1, url: 'https://example.org/a.mp3'), _podcast);
    final list = [
      for (var i = 1; i <= 8; i++) _episode(i, url: 'https://example.org/${i == 1 ? 'a' : '$i'}.mp3', published: 1760000000000 + i * 1000, finished: i == 8),
    ];
    final picked = downloads.newestToFetch(list, limit: 3);
    expect(picked.map((e) => e.id), [7, 6, 5], reason: 'not the heard one (8), not the one that is here (1), the newest first');
  });

  group('the file of an episode and the player', () {
    test('an episode on the device plays from there, without asking the internet', () async {
      final track = Track(disc: 1, number: 1, title: 'T', length: Duration.zero, url: 'https://example.org/1.mp3');
      final file = episodeCacheFile(cache, 'https://example.org/1.mp3')..createSync(recursive: true);
      final source = ServerSource(ApiClient(address: '127.0.0.1:9', token: 't'), cache);
      expect(source.isDownloaded(track), isTrue);
      expect(await source.fetchPart(track), file.path, reason: 'nothing is fetched: there is no server and no internet here');
      await source.removeDownload(track);
      expect(source.isDownloaded(track), isFalse);
    });

    test('an address that is not http or https is not fetched', () async {
      final track = Track(disc: 1, number: 1, title: 'T', length: Duration.zero, url: 'file:///etc/passwd');
      final source = ServerSource(ApiClient(address: '127.0.0.1:9', token: 't'), cache);
      await expectLater(source.fetchPart(track), throwsA(isA<HttpException>()));
    });
  });

  group('the real client', () {
    late HttpServer server;
    setUp(() async {
      server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      server.listen((req) async {
        switch (req.uri.path) {
          case '/ep.mp3':
            req.response.contentLength = 5000;
            req.response.add(List.filled(5000, 1));
          case '/redirect.mp3':
            req.response.statusCode = 302;
            req.response.headers.set('location', '/ep.mp3');
          case '/missing.mp3':
            req.response.statusCode = 404;
          case '/slow.mp3':
            req.response.contentLength = 100000;
            for (var i = 0; i < 100; i++) {
              req.response.add(List.filled(1000, 2));
              await req.response.flush();
              await Future<void>.delayed(const Duration(milliseconds: 5));
            }
        }
        await req.response.close();
      });
    });
    tearDown(() => server.close(force: true));

    Uri url(String path) => Uri.parse('http://127.0.0.1:${server.port}$path');

    test('it follows a redirect, reports the progress and leaves no .part file', () async {
      final f = HttpEpisodeFetcher();
      final dest = File('${cache.path}/ep-x.audio');
      final steps = <int>[];
      expect(await f.fetch(url('/redirect.mp3'), dest, maxBytes: 1 << 20, cancel: CancelToken(), onProgress: (r, t) => steps.add(r)), 5000);
      expect((dest.lengthSync(), File('${dest.path}.part').existsSync(), steps.last), (5000, false, 5000));
    });

    test('an error answer, a file over the limit, a refused connection and a wrong scheme each give their own code', () async {
      final f = HttpEpisodeFetcher();
      Future<String> code(Uri u, {int max = 1 << 20}) async {
        try {
          await f.fetch(u, File('${cache.path}/ep-y.audio'), maxBytes: max, cancel: CancelToken());
        } on EpisodeDownloadException catch (e) {
          return e.code;
        }
        return 'ok';
      }

      expect(await code(url('/missing.mp3')), 'http');
      expect(await code(url('/ep.mp3'), max: 1000), 'too_large');
      expect(await code(Uri.parse('http://127.0.0.1:1/none.mp3')), 'network');
      expect(await code(Uri.parse('file:///etc/passwd')), 'scheme');
      expect(File('${cache.path}/ep-y.audio').existsSync() || File('${cache.path}/ep-y.audio.part').existsSync(), isFalse);
    });

    test('cancelling stops the transfer and deletes the half-loaded file', () async {
      final f = HttpEpisodeFetcher();
      final token = CancelToken();
      final dest = File('${cache.path}/ep-z.audio');
      Object? failure;
      await f.fetch(url('/slow.mp3'), dest, maxBytes: 1 << 20, cancel: token, onProgress: (r, t) {
        if (r > 10000) token.cancel();
      }).catchError((Object e) {
        failure = e;
        return 0;
      });
      expect((failure as EpisodeDownloadException).code, 'cancelled');
      expect((dest.existsSync(), File('${dest.path}.part').existsSync()), (false, false));
    });
  });
}
