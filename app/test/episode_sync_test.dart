// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/models.dart';
import 'package:psalm/offline/offline_controller.dart';
import 'package:psalm/offline/outbox.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/podcast_downloads.dart';
import 'package:psalm/podcasts.dart';
import 'package:psalm/server/api_client.dart';
import 'package:psalm/server/server_source.dart';

import 'offline_test.dart' show FakeOutboxApi;
import 'podcast_downloads_test.dart' show FakeFetcher;

/// Records what is reported; a report can be made to fail for want of a connection.
class _Source extends MediaSource {
  final reports = <String>[];
  Object? failWith;

  @override
  Future<String> fetchPart(Track track) async => '/tmp/none';

  @override
  Future<void> saveProgress(LibraryItem book, Track track, Duration position, {required bool finished}) async {
    reports.add('${book.id} ${position.inSeconds} $finished');
    if (failWith != null) throw failWith!;
  }

  @override
  Future<PassageHit?> resolvePassage(String text) async => null;
}

Episode _episode(int id) => Episode(
  id: id,
  title: 'Folge $id',
  audioUrl: 'https://example.org/$id.mp3',
  published: DateTime.fromMillisecondsSinceEpoch(1760000000000),
  duration: const Duration(minutes: 10),
  position: Duration.zero,
  finished: false,
);

void main() {
  group('the outbox keeps where an episode was left', () {
    test('only the newest position of an episode stays, an older one is not taken, and it survives a restart', () {
      final dir = Directory.systemTemp.createTempSync('outbox');
      addTearDown(() => dir.deleteSync(recursive: true));
      final file = File('${dir.path}/outbox.json');
      final box = Outbox(file)
        ..addEpisode(id: 5, positionMs: 10000, finished: false, title: 'Folge 5', at: 1000)
        ..addEpisode(id: 5, positionMs: 20000, finished: false, title: 'Folge 5', at: 2000)
        ..addEpisode(id: 5, positionMs: 15000, finished: false, title: 'Folge 5', at: 1500)
        ..addEpisode(id: 6, positionMs: 1000, finished: true, at: 3000);
      expect(box.length, 2);
      final again = Outbox(file);
      expect([for (final e in again.entries) (e.data['id'], e.data['position_ms'], e.data['finished'], e.at)], [(5, 20000, false, 2000), (6, 1000, true, 3000)]);
      expect(again.visibleLength, 0, reason: 'the person is not told about a position');
    });

    test('the replay sends the time of the listening, in the order of the changes, and the server may take it twice', () async {
      final box = Outbox.memory()
        ..addEpisode(id: 5, positionMs: 20000, finished: false, at: 2000)
        ..addProgress(item: 3, part: 4, positionMs: 5000, finished: false, at: 2500);
      final api = FakeOutboxApi();
      final r = await box.replay(api);
      expect((r.sent, r.complete, box.isEmpty), (2, true, true));
      expect(api.calls, ['episode 5 20000 false @2000', 'progress 3/4 5000 false @2500']);
    });

    test('a refused change is dropped without a word, no connection keeps it', () async {
      final box = Outbox.memory()..addEpisode(id: 5, positionMs: 20000, finished: false, at: 2000);
      final api = FakeOutboxApi()..failures['episode 5 20000 false @2000'] = ApiException(404, 'not_found');
      final r = await box.replay(api);
      expect((r.rejected.length, box.isEmpty), (1, true));

      box.addEpisode(id: 5, positionMs: 30000, finished: false, at: 3000);
      final down = FakeOutboxApi()..failures['episode 5 30000 false @3000'] = ApiException(0, 'network');
      final r2 = await box.replay(down);
      expect((r2.complete, box.length), (false, 1));

      // The controller of the offline mode does not report a refused position to the person.
      final offline = OfflineController(outbox: Outbox.memory()..addEpisode(id: 9, positionMs: 1, finished: false, at: 1));
      await offline.sync(FakeOutboxApi()..failures['episode 9 1 false @1'] = ApiException(404, 'not_found'));
      expect(offline.rejected, isEmpty);
    });
  });

  group('the player reports an episode', () {
    late _Source source;
    late PlayerController p;
    setUp(() {
      source = _Source();
      p = PlayerController([], source: source);
    });
    tearDown(() => p.dispose());

    test('to the server, with its id (before, an episode was never reported: it has no part)', () {
      p.open(_episode(5).asItem('Kinderfunk'), start: const Duration(seconds: 42));
      p.flush();
      expect(source.reports, contains('ep:5 42 false'));
      p.toggle();
    });

    test('offline it waits in the outbox, and so does a report that finds no connection', () async {
      final offline = OfflineController(manual: true);
      p.offline = offline;
      p.open(_episode(5).asItem('Kinderfunk'), start: const Duration(seconds: 42));
      p.flush();
      expect(source.reports, isEmpty, reason: 'nothing is asked of the server');
      expect([for (final e in offline.outbox.entries) (e.kind, e.data['id'], e.data['position_ms'])], [(OutboxChange.episode, 5, 42000)]);

      // Online but the answer does not come.
      offline.manual = false;
      offline.outbox.clear();
      source.failWith = ApiException(0, 'network');
      p.flush();
      await Future<void>.delayed(Duration.zero);
      expect(offline.outbox.length, 1);
      p.toggle();
    });

    test('the offline list of the downloaded episodes learns where this device is', () async {
      final dir = Directory.systemTemp.createTempSync('episode-state');
      addTearDown(() => dir.deleteSync(recursive: true));
      final downloads = PodcastDownloads(cache: Directory('${dir.path}/parts'), fetcher: FakeFetcher());
      p.podcastDownloads = downloads;
      await downloads.download(_episode(5), const Podcast(id: 1, title: 'Kinderfunk', description: '', approved: true, unheard: 0));
      p.open(_episode(5).asItem('Kinderfunk'), start: const Duration(seconds: 42));
      p.flush();
      expect(downloads.entryOf(5)?.positionMs, 42000);
      p.toggle();
    });
  });
}
