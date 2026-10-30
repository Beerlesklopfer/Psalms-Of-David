// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/offline/offline_controller.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/podcast_downloads.dart';
import 'package:psalm/podcasts.dart';
import 'package:psalm/screens/podcast_downloads_ui.dart';
import 'package:psalm/screens/podcasts_screen.dart';
import 'package:psalm/server/api_client.dart';

import 'podcast_downloads_test.dart' show FakeFetcher;

class _Feeds implements FeedApi {
  _Feeds({this.fail = false});

  final bool fail;
  final podcastList = [
    {'id': 1, 'title': 'Kinderfunk', 'description': 'Geschichten', 'approved': true, 'unheard': 2},
  ];
  final episodeList = <Map<String, dynamic>>[
    for (var i = 1; i <= 7; i++)
      {'id': i, 'title': 'Folge $i', 'audio_url': 'https://example.org/$i.mp3', 'published_at': 1760000000000 + i * 1000, 'duration_s': 600, 'position_ms': 0, 'finished': i == 7},
  ];

  @override
  Future<List<Map<String, dynamic>>> podcasts() async => fail ? throw ApiException(0, 'network') : podcastList;

  @override
  Future<List<Map<String, dynamic>>> episodes(int podcastId) async => fail ? throw ApiException(0, 'network') : episodeList;

  @override
  Future<Map<String, dynamic>> followPodcast(String url) async => {};

  @override
  Future<void> unfollowPodcast(int id) async {}

  @override
  Future<List<Map<String, dynamic>>> stations() async => [];

  @override
  Future<Map<String, dynamic>> addStation(String name, String streamUrl, {String? homepage}) async => {};

  @override
  Future<void> removeStation(int id) async {}
}

const _podcast = Podcast(id: 1, title: 'Kinderfunk', description: '', approved: true, unheard: 0);

Episode _episode(int id) => Episode(
  id: id,
  title: 'Folge $id',
  audioUrl: 'https://example.org/$id.mp3',
  published: DateTime.fromMillisecondsSinceEpoch(1760000000000 + id * 1000),
  duration: const Duration(minutes: 10),
  position: Duration.zero,
  finished: false,
);

void main() {
  late Directory root;
  late FakeFetcher fetcher;
  late PodcastDownloads downloads;
  late PlayerController player;

  setUp(() {
    root = Directory.systemTemp.createTempSync('podcast-ui');
    fetcher = FakeFetcher();
    downloads = PodcastDownloads(cache: Directory('${root.path}/parts'), file: File('${root.path}/pd.json'), fetcher: fetcher);
    player = PlayerController([])..podcastDownloads = downloads;
  });
  tearDown(() {
    player.dispose();
    root.deleteSync(recursive: true);
  });

  Widget app(Widget child, {OfflineController? offline}) {
    Widget w = PlayerScope(
      controller: player,
      child: MaterialApp(
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        locale: const Locale('en'),
        home: Scaffold(body: child),
      ),
    );
    if (offline != null) w = OfflineScope(controller: offline, child: w);
    return w;
  }

  testWidgets('the button downloads, shows the check mark, and removes the episode after asking', (tester) async {
    await tester.pumpWidget(app(EpisodeDownloadButton(episode: _episode(5), podcast: _podcast)));
    expect(find.byKey(const ValueKey('episode-download-5')), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('episode-download-5')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('episode-downloaded-5')), findsOneWidget);
    expect(downloads.isDownloaded('https://example.org/5.mp3'), isTrue);

    await tester.tap(find.byKey(const ValueKey('episode-downloaded-5')));
    await tester.pumpAndSettle();
    expect(find.text('Remove the download?'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('episode-remove-yes')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('episode-download-5')), findsOneWidget);
    expect(downloads.isDownloaded('https://example.org/5.mp3'), isFalse);
  });

  testWidgets('a download in progress shows how far it is and can be stopped', (tester) async {
    fetcher.gate = Completer<void>();
    await tester.pumpWidget(app(EpisodeDownloadButton(episode: _episode(5), podcast: _podcast)));
    await tester.tap(find.byKey(const ValueKey('episode-download-5')));
    await tester.pump();
    expect(find.byKey(const ValueKey('episode-download-cancel-5')), findsOneWidget);
    expect(find.textContaining('of 2 KB'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('episode-download-cancel-5')));
    fetcher.gate!.complete();
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('episode-download-5')), findsOneWidget);
    expect(downloads.entries.isEmpty, isTrue);
  });

  testWidgets('a download that fails says why', (tester) async {
    fetcher.failWith = const EpisodeDownloadException('http', 'HTTP 404');
    await tester.pumpWidget(app(EpisodeDownloadButton(episode: _episode(5), podcast: _podcast)));
    await tester.tap(find.byKey(const ValueKey('episode-download-5')));
    await tester.pumpAndSettle();
    expect(find.text('The publisher refused the download.'), findsOneWidget);
    await tester.pump(const Duration(seconds: 6));
    await tester.pumpAndSettle();
    // An address that is no web address never reaches the internet.
    fetcher.failWith = null;
    final odd = Episode(id: 9, title: 'x', audioUrl: 'file:///etc/passwd', published: null, duration: Duration.zero, position: Duration.zero, finished: false);
    await tester.pumpWidget(app(EpisodeDownloadButton(episode: odd, podcast: _podcast)));
    await tester.tap(find.byKey(const ValueKey('episode-download-9')));
    await tester.pumpAndSettle();
    expect(find.textContaining('no http or https address'), findsOneWidget);
    expect(fetcher.calls.length, 1);
  });

  testWidgets('the list of the episodes offers the download of the newest unheard ones and shows the size', (tester) async {
    final feeds = _Feeds();
    await tester.pumpWidget(app(EpisodesScreen(api: feeds, podcast: _podcast, onUnfollowed: () async {})));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('episode-3')), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('download-newest')));
    await tester.pumpAndSettle();
    // Episode 7 is heard: the newest five of the rest are 6, 5, 4, 3, 2.
    expect(downloads.entries.map((e) => e.episodeId).toSet(), {6, 5, 4, 3, 2});
    expect(find.descendant(of: find.byKey(const ValueKey('episode-6')), matching: find.textContaining('2 KB')), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('download-newest')));
    await tester.pumpAndSettle();
    expect(downloads.entries.length, 6, reason: 'the next round takes episode 1');
    await tester.tap(find.byKey(const ValueKey('download-newest')));
    await tester.pumpAndSettle();
    expect(find.text('Nothing new to download.'), findsOneWidget);
  });

  testWidgets('without the server the episodes on the device are still listed, and they play', (tester) async {
    await downloads.download(_episode(5), _podcast);
    await tester.pumpWidget(app(EpisodesScreen(api: _Feeds(fail: true), podcast: _podcast, onUnfollowed: () async {})));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('episode-5')), findsOneWidget);
    expect(find.byKey(const ValueKey('episode-4')), findsNothing);
    await tester.tap(find.byKey(const ValueKey('episode-5')));
    await tester.pump();
    expect((player.book?.id, player.book?.kind), ('ep:5', 'podcast'));
    player.toggle();
  });

  testWidgets('in the offline view the podcasts are the ones with episodes on the device, nothing is asked of the server', (tester) async {
    final offline = OfflineController(manual: true);
    final controller = PodcastsController(_Feeds(fail: true));
    addTearDown(controller.dispose);
    Widget body() => app(CustomScrollView(slivers: [PodcastsBody(controller: controller)]), offline: offline);

    await tester.pumpWidget(body());
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('offline-podcasts-none')), findsOneWidget);

    await downloads.download(_episode(5), _podcast);
    await tester.pumpWidget(body());
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('offline-podcast-1')), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('offline-podcast-1')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('episode-5')), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('episode-5')));
    await tester.pump();
    expect(player.book?.id, 'ep:5');
    player.toggle();

    // Online again the list of the server is back.
    offline.setManual(false);
    await tester.pageBack();
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('offline-podcasts-none')), findsNothing);
  });

  testWidgets('the settings: what the downloads take, delete after listening, the largest download, delete all', (tester) async {
    await downloads.download(_episode(5), _podcast);
    await downloads.download(_episode(6), _podcast);
    await tester.pumpWidget(app(SingleChildScrollView(child: PodcastDownloadsCard(downloads: downloads))));
    expect(find.text('4 KB in 2 episodes'), findsOneWidget);

    await tester.tap(find.byKey(const ValueKey('podcast-delete-after')));
    await tester.pump();
    expect(downloads.deleteAfterListening, isTrue);

    await tester.tap(find.byKey(const ValueKey('podcast-max-size')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('1000 MB').last);
    await tester.pumpAndSettle();
    expect(downloads.maxMegabytes, 1000);

    await tester.tap(find.byKey(const ValueKey('podcast-downloads-delete')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('podcast-downloads-delete-yes')));
    await tester.pumpAndSettle();
    expect(downloads.entries.isEmpty, isTrue);
    expect(find.text('No podcast episodes on this device.'), findsOneWidget);
  });
}
