// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/podcast_downloads.dart';
import 'package:psalm/podcasts.dart';
import 'package:psalm/podcasts/bulk_preflight.dart';
import 'package:psalm/podcasts/download_environment.dart';
import 'package:psalm/podcasts/download_gate.dart';
import 'package:psalm/screens/podcast_downloads_ui.dart';

Episode _episode(int id, {int minutes = 60, int published = 1760000000000}) => Episode(
      id: id,
      title: 'Folge $id',
      audioUrl: 'https://example.org/$id.mp3',
      published: DateTime.fromMillisecondsSinceEpoch(published),
      duration: Duration(minutes: minutes),
      position: Duration.zero,
      finished: false,
    );

void main() {
  group('the plan of a bulk download', () {
    test('an hour of audio is about 58 MB, an episode without a duration is assumed at 40 MB', () {
      expect(estimatedEpisodeBytes(_episode(1)), 3600 * 16000);
      expect(estimatedEpisodeBytes(_episode(1, minutes: 0)), kAssumedEpisodeBytes);
    });

    test('everything that fits the limit is taken as it is', () {
      final plan = planBulk([for (var i = 0; i < 10; i++) _episode(i)], limitMegabytes: 1000);
      expect((plan.count, plan.fittingCount, plan.exceedsLimit, plan.blocked), (10, 10, false, false));
    });

    test('more than the limit: only the newest that fit are taken (the list is newest first)', () {
      // 100 episodes of an hour are about 5.8 GB; with a 1 GB limit 17 fit (17 x 57.6 MB = 979 MB).
      final plan = planBulk([for (var i = 0; i < 100; i++) _episode(i)], limitMegabytes: 1000);
      expect(plan.exceedsLimit, isTrue);
      expect(plan.fittingCount, 17);
      expect(plan.fitting.first.id, 0, reason: 'the newest first');
      expect(plan.fittingBytes, lessThanOrEqualTo(plan.limitBytes));
      expect(plan.totalBytes, greaterThan(5 * 1000 * 1000 * 1000));
    });

    test('an episode larger than the limit blocks the plan', () {
      final plan = planBulk([_episode(1, minutes: 600)], limitMegabytes: 250);
      expect((plan.blocked, plan.fittingCount), (true, 0));
    });

    test('the limits of the settings go from 250 MB to 20 GB and the default is 1 GB', () {
      expect(kBulkSizeChoices.first, 250);
      expect(kBulkSizeChoices.last, 20000);
      expect(kBulkDefaultMegabytes, 1000);
    });
  });

  group('the setting and the selection', () {
    late Directory dir;
    setUp(() => dir = Directory.systemTemp.createTempSync('psalm-bulk'));
    tearDown(() => dir.deleteSync(recursive: true));

    test('the limit is 1 GB until it is changed, and the change is kept', () {
      final manifest = File('${dir.path}/podcast-downloads.json');
      final d = PodcastDownloads(cache: Directory('${dir.path}/cache'), file: manifest);
      expect(d.bulkMegabytes, 1000);
      d.setBulkMegabytes(5000);
      final again = PodcastDownloads(cache: Directory('${dir.path}/cache'), file: manifest);
      expect(again.bulkMegabytes, 5000);
      d.dispose();
      again.dispose();
    });

    test('the whole collection is every episode that is not on the device, the newest first', () {
      final d = PodcastDownloads(cache: Directory('${dir.path}/cache'), file: File('${dir.path}/m.json'));
      final list = [_episode(1, published: 1000), _episode(2, published: 3000), _episode(3, published: 2000)];
      expect(d.allToFetch(list).map((e) => e.id), [2, 3, 1]);
      d.dispose();
    });
  });

  group('the question', () {
    Future<List<Episode>?> ask(WidgetTester tester, BulkPlan plan, {Locale locale = const Locale('en')}) async {
      List<Episode>? answer;
      var asked = false;
      await tester.pumpWidget(MaterialApp(
        locale: locale,
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        home: Builder(builder: (context) => TextButton(onPressed: () async {
              answer = await askBulkDownload(context, plan);
              asked = true;
            }, child: const Text('go'))),
      ));
      await tester.tap(find.text('go'));
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('bulk-download-dialog')), findsOneWidget);
      if (tester.any(find.byKey(const ValueKey('bulk-download-start')))) {
        await tester.tap(find.byKey(const ValueKey('bulk-download-start')));
        await tester.pumpAndSettle();
        expect(asked, isTrue);
      }
      return answer;
    }

    testWidgets('it says how many episodes and about how large, and "Download" returns them', (tester) async {
      final plan = planBulk([for (var i = 0; i < 10; i++) _episode(i)], limitMegabytes: 1000);
      final answer = await ask(tester, plan);
      expect(answer, hasLength(10));
    });

    testWidgets('over the limit it says what will be loaded instead', (tester) async {
      final plan = planBulk([for (var i = 0; i < 100; i++) _episode(i)], limitMegabytes: 1000);
      Widget? seen;
      List<Episode>? answer;
      await tester.pumpWidget(MaterialApp(
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        home: Builder(builder: (context) => TextButton(onPressed: () async => answer = await askBulkDownload(context, plan), child: const Text('go'))),
      ));
      await tester.tap(find.text('go'));
      await tester.pumpAndSettle();
      seen = tester.widget(find.byKey(const ValueKey('bulk-download-dialog')));
      expect(find.textContaining('only the newest 17'), findsOneWidget);
      await tester.tap(find.byKey(const ValueKey('bulk-download-start')));
      await tester.pumpAndSettle();
      expect(answer, hasLength(17), reason: 'only what fits is returned');
      expect(seen, isNotNull);
    });

    testWidgets('a plan that is blocked has no "Download" button, only Cancel', (tester) async {
      final plan = planBulk([_episode(1, minutes: 600)], limitMegabytes: 250);
      await tester.pumpWidget(MaterialApp(
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        home: Builder(builder: (context) => TextButton(onPressed: () => askBulkDownload(context, plan), child: const Text('go'))),
      ));
      await tester.tap(find.text('go'));
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('bulk-download-start')), findsNothing);
      expect(find.text('Cancel'), findsOneWidget);
    });

    testWidgets('Cancel returns nothing: nothing is loaded without the answer', (tester) async {
      final plan = planBulk([for (var i = 0; i < 3; i++) _episode(i)], limitMegabytes: 1000);
      List<Episode>? answer = [];
      await tester.pumpWidget(MaterialApp(
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        home: Builder(builder: (context) => TextButton(onPressed: () async => answer = await askBulkDownload(context, plan), child: const Text('go'))),
      ));
      await tester.tap(find.text('go'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Cancel'));
      await tester.pumpAndSettle();
      expect(answer, isNull);
    });
  });

  group('the room of the device and the connection', () {
    test('free space cuts the plan: the limit stays, 1 GB stays free', () {
      // 3 GB are free, 1 GB stays: 2 GB may be used, although the person allows 5 GB.
      final plan = planBulk([for (var i = 0; i < 100; i++) _episode(i)], limitMegabytes: 5000, freeBytes: 3 * 1000 * 1000 * 1000);
      expect(plan.spaceBound, isTrue);
      expect(plan.limitBytes, 2 * 1000 * 1000 * 1000);
      expect(plan.fittingBytes, lessThanOrEqualTo(2 * 1000 * 1000 * 1000));
    });

    test('plenty of room changes nothing, an unknown room is not a reason to refuse', () {
      final todo = [for (var i = 0; i < 10; i++) _episode(i)];
      expect(planBulk(todo, limitMegabytes: 1000, freeBytes: 500 * 1000 * 1000 * 1000).spaceBound, isFalse);
      expect(planBulk(todo, limitMegabytes: 1000).spaceBound, isFalse);
    });

    test('less than the reserve free blocks the plan', () {
      final plan = planBulk([_episode(1)], limitMegabytes: 1000, freeBytes: 500 * 1000 * 1000);
      expect((plan.blocked, plan.spaceBound), (true, true));
    });

    group('the preflight', () {
      late Directory dir;
      setUp(() => dir = Directory.systemTemp.createTempSync('psalm-pre'));
      tearDown(() => dir.deleteSync(recursive: true));

      PodcastDownloads downloads(_Env env) => PodcastDownloads(cache: Directory('${dir.path}/cache'), file: File('${dir.path}/m.json'), environment: env);

      test('on mobile data nothing starts while the setting says Wi-Fi only (the default)', () async {
        final d = downloads(_Env(onWifi: false, free: 100 * 1000 * 1000 * 1000));
        expect(d.bulkWifiOnly, isTrue);
        expect(await preflightBulk(d, [_episode(1)]), isA<BulkMobileData>());
        d.dispose();
      });

      test('with the setting off, mobile data is allowed', () async {
        final d = downloads(_Env(onWifi: false, free: 100 * 1000 * 1000 * 1000))..setBulkWifiOnly(false);
        expect(await preflightBulk(d, [_episode(1)]), isA<BulkReady>());
        d.dispose();
      });

      test('Wi-Fi and a connection that cannot be told both pass; the room is taken into the plan', () async {
        for (final unmetered in [true, null]) {
          final d = downloads(_Env(onWifi: unmetered, free: 1500 * 1000 * 1000));
          final r = await preflightBulk(d, [for (var i = 0; i < 100; i++) _episode(i)]);
          expect(r, isA<BulkReady>());
          expect((r as BulkReady).plan.spaceBound, isTrue);
          d.dispose();
        }
      });

      test('the setting is kept', () {
        final manifest = File('${dir.path}/m2.json');
        final d = PodcastDownloads(cache: Directory('${dir.path}/c2'), file: manifest, environment: _Env())..setBulkWifiOnly(false);
        final again = PodcastDownloads(cache: Directory('${dir.path}/c2'), file: manifest, environment: _Env());
        expect(again.bulkWifiOnly, isFalse);
        d.dispose();
        again.dispose();
      });
    });
  });
}

/// A device that says what the test wants.
class _Env implements DownloadEnvironment {
  _Env({this.onWifi, this.free});

  final bool? onWifi;
  final int? free;

  @override
  Future<int?> freeBytes(String path) async => free;

  @override
  Future<bool?> unmetered() async => onWifi;
}
