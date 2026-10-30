// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/demo_data.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/main.dart' show Shell;
import 'package:psalm/labels.dart';
import 'package:psalm/library_hiding.dart';
import 'package:psalm/models.dart';
import 'package:psalm/offline/library_cache.dart';
import 'package:psalm/offline/offline_controller.dart';
import 'package:psalm/offline/offline_view.dart';
import 'package:psalm/offline/outbox.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/server/api_client.dart';
import 'package:psalm/server/server_source.dart';

import 'labels_test.dart' show FakeLabels;

/// Records what the replay sends; an answer for a call can be a failure.
class FakeOutboxApi implements OutboxApi {
  final calls = <String>[];
  final failures = <String, ApiException>{};

  Future<void> _do(String call) async {
    calls.add(call);
    final f = failures[call];
    if (f != null) throw f;
  }

  @override
  Future<void> labels(String scope, int id, {int? part, required List<String> add, required List<String> remove}) =>
      _do('labels $scope $id${part == null ? '' : '/$part'} +${add.join(',')} -${remove.join(',')}');

  @override
  Future<void> hideBook(int id, bool hidden) => _do('hide book $id $hidden');

  @override
  Future<void> hideAuthor(String name, String kind, bool hidden) => _do('hide author $name/$kind $hidden');

  @override
  Future<void> progress({required int item, required int part, required int positionMs, required bool finished, required int at}) =>
      _do('progress $item/$part $positionMs $finished @$at');
}

/// A source whose downloaded parts are the ones listed.
class FakeSource implements MediaSource {
  FakeSource(this.downloaded);

  final Set<int> downloaded;
  final progress = <String>[];
  Object? progressFails;

  @override
  bool get canDownload => true;

  @override
  bool isDownloaded(Track t) => downloaded.contains(t.partId);

  @override
  Future<String> fetchPart(Track track) async => '/tmp/${track.partId}';

  @override
  Future<void> removeDownload(Track track) async {}

  @override
  Future<({int files, int bytes})> storageUsed() async => (files: 0, bytes: 0);

  @override
  Future<void> removeAllDownloads() async {}

  @override
  Future<void> saveProgress(Book book, Track track, Duration position, {required bool finished}) async {
    final fail = progressFails;
    if (fail != null) throw fail;
    progress.add('${book.id}/${track.partId} ${position.inSeconds}');
  }

  @override
  Future<PassageHit?> resolvePassage(String text) async => null;

  @override
  Future<void> writeM3u(String name, List<dynamic> entries) async {}

  @override
  String? relativePath(Track track) => null;
}

Track _t(int part, String title, {List<String> labels = const []}) =>
    Track(disc: 1, number: part % 100, title: title, length: const Duration(minutes: 5), partId: part, labels: labels);

Book _book(String id, String title, List<Track> tracks, {List<String> labels = const [], String author = 'Autor', String kind = 'audiobook', bool can = true}) => Book(
  id: id,
  title: title,
  author: author,
  color: const Color(0xFF6E1F2B),
  kind: kind,
  labels: labels,
  canLabel: can,
  tracks: tracks,
);

Widget _app(Widget home, {OfflineController? offline}) {
  final app = MaterialApp(
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    locale: const Locale('en'),
    home: Scaffold(body: home),
  );
  return offline == null ? app : OfflineScope(controller: offline, child: app);
}

void main() {
  late Directory dir;
  setUp(() => dir = Directory.systemTemp.createTempSync('psalm-offline'));
  tearDown(() => dir.deleteSync(recursive: true));

  group('outbox', () {
    test('changes survive a restart, in the order in which they were made', () {
      final file = File('${dir.path}/outbox.json');
      final box = Outbox(file)
        ..addLabels('book', 1, add: ['Gebet'], title: 'Eins')
        ..addHideBook(2, true, title: 'Zwei')
        ..addProgress(item: 3, part: 30, positionMs: 5000, finished: false, at: 100);
      final again = Outbox(file);
      expect([for (final e in again.entries) e.kind], [OutboxChange.labels, OutboxChange.hideBook, OutboxChange.progress]);
      expect(again.entries.first.add, ['Gebet']);
      expect(again.entries.first.title, 'Eins');
      expect(again.length, box.length);
      // A new change goes behind the old ones, also after the restart.
      again.addHideBook(9, true);
      expect(again.entries.last.seq, greaterThan(again.entries[2].seq));
    });

    test('label changes of the same thing become one delta in which the last thing done to a word counts', () {
      final box = Outbox.memory()
        ..addLabels('book', 1, add: ['Gebet', 'Schule'], remove: ['Alt'])
        ..addLabels('book', 1, add: ['alt'], remove: ['schule', 'Neu'])
        ..addLabels('book', 7, add: ['X']);
      final first = box.entries.first;
      // Alt was taken off and then added again: it is added. Schule was added and then taken off: it is taken off. Neu is taken off.
      expect(first.add, ['Gebet', 'alt']);
      expect(first.remove, ['schule', 'Neu']);
      expect(box.length, 2);
      // A word that is added twice is added once; an empty delta is no change.
      box.addLabels('book', 7, add: ['x']);
      expect(box.entries.last.add, ['X']);
      box.addLabels('book', 8);
      expect(box.length, 2);
      // A title has its own delta, apart from the book.
      box.addLabels('part', 1, part: 10, add: ['Abend']);
      expect(box.length, 3);
    });

    test('the last state of a hidden book wins, and only the newest listening position of a book stays', () {
      final box = Outbox.memory()
        ..addHideBook(1, true)
        ..addHideBook(1, false)
        ..addHideAuthor('Meier', '', true)
        ..addHideAuthor('meier', '', false)
        ..addProgress(item: 5, part: 50, positionMs: 1000, finished: false, at: 10)
        ..addProgress(item: 5, part: 50, positionMs: 3000, finished: false, at: 30)
        ..addProgress(item: 5, part: 50, positionMs: 2000, finished: false, at: 20);
      expect(box.length, 3);
      expect(box.entries[0].data['hidden'], false);
      expect(box.entries[1].data['hidden'], false);
      expect(box.entries[2].data['position_ms'], 3000, reason: 'an older report does not replace a newer one');
    });

    test('the replay sends in order, drops what the server refuses and tells it, and goes on', () async {
      final box = Outbox.memory()
        ..addLabels('book', 1, add: ['A'], title: 'Eins')
        ..addHideBook(2, true, title: 'Zwei')
        ..addLabels('part', 3, part: 33, add: ['B'], title: 'Drei')
        ..addProgress(item: 4, part: 44, positionMs: 7000, finished: true, at: 1234);
      final api = FakeOutboxApi()
        ..failures['labels book 1 +A -'] = ApiException(403, 'forbidden')
        ..failures['hide book 2 true'] = ApiException(404, 'not_found');
      final r = await box.replay(api);
      expect(api.calls, ['labels book 1 +A -', 'hide book 2 true', 'labels part 3/33 +B -', 'progress 4/44 7000 true @1234']);
      expect((r.sent, r.complete), (2, true));
      expect([for (final x in r.rejected) (x.status, x.change.title)], [(403, 'Eins'), (404, 'Zwei')]);
      expect(box.isEmpty, true);
      // Nothing is left to send twice.
      final second = FakeOutboxApi();
      await box.replay(second);
      expect(second.calls, isEmpty);
    });

    test('a server that cannot be reached keeps the rest, and the next round goes on from there', () async {
      final box = Outbox.memory()
        ..addHideBook(1, true)
        ..addHideBook(2, true)
        ..addHideBook(3, true);
      final api = FakeOutboxApi()..failures['hide book 2 true'] = ApiException(0, 'network');
      final r = await box.replay(api);
      expect(r.complete, false);
      expect(r.sent, 1);
      expect(box.length, 2, reason: 'the second and the third wait');
      api.failures.clear();
      await box.replay(api);
      expect(box.isEmpty, true);
      expect(api.calls.last, 'hide book 3 true');
    });
  });

  group('offline mode', () {
    test('the switch is kept on the device; offline is the switch or an unreachable server', () {
      final store = OfflineSettingsStore(File('${dir.path}/offline.json'));
      final c = OfflineController(store: store);
      expect(c.active, false);
      c.setUnreachable(true);
      expect(c.active, true);
      c.setUnreachable(false);
      c.setManual(true);
      expect(c.active, true);
      expect(OfflineController(store: store).manual, true, reason: 'still on after a restart');
      c.setManual(false);
      expect(OfflineController(store: store).manual, false);
    });

    test('the sending tells what the server refused (not the listening) and keeps it until it was seen', () async {
      final c = OfflineController();
      c.outbox
        ..addHideBook(1, true, title: 'Eins')
        ..addProgress(item: 2, part: 20, positionMs: 1, finished: false);
      final api = FakeOutboxApi()
        ..failures['hide book 1 true'] = ApiException(403, 'forbidden')
        ..failures.addAll({'progress 2/20 1 false @${c.outbox.entries.last.at}': ApiException(404, 'not_found')});
      final r = await c.sync(api);
      expect(r!.rejected.length, 2);
      expect([for (final x in c.rejected) x.change.title], ['Eins']);
      c.dismissRejected();
      expect(c.rejected, isEmpty);
      expect(await c.sync(api), isNull, reason: 'nothing waits');
    });
  });

  group('offline view', () {
    final all = [
      _book('1', 'Ganz', [_t(11, 'a'), _t(12, 'b')]),
      _book('2', 'Teils', [_t(21, 'a'), _t(22, 'b'), _t(23, 'c')]),
      _book('3', 'Nichts', [_t(31, 'a')]),
      _book('4', 'Sender', [_t(41, 'live')], kind: 'radio'),
    ];

    test('only the downloaded titles are shown: a partly downloaded album keeps the downloaded ones, an album without any goes', () {
      final shown = downloadedOnly(all, (t) => {11, 12, 22}.contains(t.partId));
      expect([for (final b in shown) b.title], ['Ganz', 'Teils']);
      expect([for (final t in shown[1].tracks) t.title], ['b']);
      expect(shown[0].tracks.length, 2);
    });

    test('the player shows the downloaded titles offline and everything again online; what plays stays', () {
      final source = FakeSource({11, 22});
      final p = PlayerController(all, source: source);
      p.book = all[1];
      p.trackIndex = 1; // the title 22
      p.setOfflineView(true);
      expect([for (final b in p.library) b.title], ['Ganz', 'Teils']);
      expect(p.library[1].tracks.length, 1);
      expect(p.book!.tracks[p.trackIndex].partId, 22, reason: 'the open title is still the open title');
      expect(p.fullLibrary.length, 4);
      p.setOfflineView(false);
      expect(p.library.length, 4);
    });

    test('a change of a book made on the device shows at once, in the offline view too', () {
      final p = PlayerController(all, source: FakeSource({11, 12}));
      p.setOfflineView(true);
      p.patchBook('1', (b) => b.copyWith(labels: ['Gebet']));
      expect(p.library.single.labels, ['Gebet']);
      expect(p.fullLibrary.firstWhere((b) => b.id == '1').labels, ['Gebet']);
      p.removeBookLocally('1');
      expect(p.library, isEmpty);
      expect(p.fullLibrary.any((b) => b.id == '1'), false);
    });

    test('the library of the last time is kept on the device with its pictures', () async {
      final cache = LibraryCache(Directory('${dir.path}/library'));
      expect(cache.read(), isNull);
      final lib = LoadedLibrary(
        [
          Book(id: '5', title: 'Fünf', author: 'A', color: const Color(0xFF123456), cover: Uint8List.fromList([1, 2, 3]), labels: const ['x'], trackLabels: const ['y'], canLabel: true, tracks: [_t(51, 'a', labels: const ['y'])]),
          _book('6', 'Sechs', [_t(61, 'b')]),
        ],
        {'5': (track: 0, position: const Duration(seconds: 42))},
        lastBookId: '5',
        lastAt: 99,
        userName: 'Anna',
        recent: const ['5'],
        genres: const ['Rock'],
        newIds: const ['6'],
      );
      await cache.write(lib);
      final back = cache.read()!;
      expect([for (final b in back.books) b.title], ['Fünf', 'Sechs']);
      expect(back.books[0].cover, [1, 2, 3]);
      expect(back.books[1].cover, isNull);
      expect(back.books[0].labels, ['x']);
      expect(back.books[0].trackLabels, ['y']);
      expect(back.books[0].tracks.single.labels, ['y']);
      expect(back.books[0].canLabel, true);
      expect(back.resume['5']!.position, const Duration(seconds: 42));
      expect((back.lastBookId, back.lastAt, back.userName), ('5', 99, 'Anna'));
      expect(back.recent, ['5']);
      expect(back.genres, ['Rock']);
      expect(back.newIds, ['6']);
      // A book that is gone loses its picture.
      await cache.write(LoadedLibrary([_book('6', 'Sechs', [_t(61, 'b')])], const {}));
      expect(File('${dir.path}/library/cover-5').existsSync(), false);
    });
  });

  group('changes without the server', () {
    test('labels offline: a delta waits, the book shows the new labels, and two edits merge', () async {
      final offline = OfflineController()..setManual(true);
      final p = PlayerController([_book('1', 'Eins', [_t(11, 'a')], labels: ['Alt'])]);
      final api = FakeLabels();
      final labels = LabelsController()
        ..api = api
        ..offline = offline
        ..bookOf = ((id) => p.fullLibrary.where((b) => b.id == id).firstOrNull)
        ..patchBook = p.patchBook;
      expect(await labels.save('1', ['Alt', 'Gebet']), true);
      expect(await labels.save('1', ['Gebet', 'Schule']), true);
      expect(api.calls, isEmpty, reason: 'nothing goes to the server offline');
      expect(p.library.single.labels, ['Gebet', 'Schule']);
      final e = offline.outbox.entries.single;
      expect(e.add, ['Gebet', 'Schule']);
      expect(e.remove, ['Alt']);
      // A title.
      expect(await labels.saveTrack('1', 11, ['Abend']), true);
      expect(p.library.single.tracks.single.labels, ['Abend']);
      expect(p.library.single.trackLabels, ['Abend']);
      expect(offline.pending, 2);
    });

    test('labels online but the connection fails: the change waits instead of being lost', () async {
      final offline = OfflineController();
      final p = PlayerController([_book('1', 'Eins', [_t(11, 'a')])]);
      final api = FakeLabels()..refuse = ApiException(0, 'network');
      final labels = LabelsController()
        ..api = api
        ..offline = offline
        ..bookOf = ((id) => p.fullLibrary.where((b) => b.id == id).firstOrNull)
        ..patchBook = p.patchBook;
      expect(await labels.save('1', ['Gebet']), true);
      expect(offline.outbox.entries.single.add, ['Gebet']);
      // A refusal of the server is no waiting: it is shown as it is.
      api.refuse = ApiException(403, 'forbidden');
      expect(await labels.save('1', ['Gebet', 'Schule']), false);
      expect(labels.error, 'forbidden');
      expect(offline.pending, 1);
    });

    test('hiding offline: the book leaves the lists at once and the change waits', () async {
      final offline = OfflineController()..setManual(true);
      final p = PlayerController([_book('1', 'Eins', [_t(11, 'a')], author: 'Meier'), _book('2', 'Zwei', [_t(21, 'a')], author: 'Meier'), _book('3', 'Drei', [_t(31, 'a')], author: 'Müller')]);
      final hiding = HidingController()
        ..offline = offline
        ..bookOf = ((id) => p.fullLibrary.where((b) => b.id == id).firstOrNull)
        ..booksNow = (() => p.library)
        ..removeBook = p.removeBookLocally;
      expect(await hiding.hideBook('1', true), true);
      expect([for (final b in p.library) b.id], ['2', '3']);
      expect(hiding.state.books.single.title, 'Eins');
      expect(await hiding.hideAuthor('Meier', '', true), true);
      expect([for (final b in p.library) b.id], ['3']);
      expect(hiding.state.authors.single.name, 'Meier');
      expect(offline.outbox.entries.map((e) => e.kind), [OutboxChange.hideBook, OutboxChange.hideAuthor]);
      // Showing it again is the last state.
      expect(await hiding.hideBook('1', false), true);
      expect(offline.outbox.entries.firstWhere((e) => e.kind == OutboxChange.hideBook).data['hidden'], false);
      expect(hiding.state.books, isEmpty);
    });

    test('the listening is kept for the server offline, with the time; online a failed report waits too', () async {
      final offline = OfflineController()..setManual(true);
      final source = FakeSource({11});
      final p = PlayerController([_book('1', 'Eins', [_t(11, 'a')])], source: source)..offline = offline;
      p.book = p.library.first;
      p.trackIndex = 0;
      p.position = const Duration(seconds: 90);
      p.flush();
      expect(source.progress, isEmpty, reason: 'the server is not asked');
      final e = offline.outbox.entries.single;
      expect((e.kind, e.data['item'], e.data['part'], e.data['position_ms']), (OutboxChange.progress, 1, 11, 90000));
      expect(e.data['at'], isA<int>());
      // Back online, but the report fails for want of a connection.
      offline.setManual(false);
      offline.outbox.clear();
      source.progressFails = ApiException(0, 'network');
      p.position = const Duration(seconds: 120);
      p.flush();
      await Future<void>.delayed(Duration.zero);
      expect(offline.outbox.entries.single.data['position_ms'], 120000);
    });
  });

  group('screens', () {
    testWidgets('the bar says that the app is offline and how many changes wait', (tester) async {
      final offline = OfflineController();
      var retried = 0;
      await tester.pumpWidget(_app(OfflineBanner(onSync: () => retried++), offline: offline));
      expect(find.byKey(const ValueKey('offline-banner')), findsNothing);
      offline.setUnreachable(true);
      offline.outbox.addHideBook(1, true);
      offline.touch();
      await tester.pump();
      expect(find.byKey(const ValueKey('offline-banner')), findsOneWidget);
      expect(find.textContaining('Server not reachable'), findsOneWidget);
      expect(find.textContaining('1 change waits for the server'), findsOneWidget);
      await tester.tap(find.byKey(const ValueKey('offline-retry')));
      expect(retried, 1);
      // The person's own switch: the bar says so and has no button to look for the server.
      offline.setUnreachable(false);
      offline.setManual(true);
      await tester.pump();
      expect(find.textContaining('Offline mode'), findsOneWidget);
      expect(find.byKey(const ValueKey('offline-retry')), findsNothing);
    });

    testWidgets('a page that needs the server says so offline', (tester) async {
      final offline = OfflineController()..setManual(true);
      await tester.pumpWidget(_app(const OnlineOnly(child: Text('Podcasts')), offline: offline));
      expect(find.text('Podcasts'), findsNothing);
      expect(find.byKey(const ValueKey('offline-needs-network')), findsOneWidget);
      offline.setManual(false);
      await tester.pump();
      expect(find.text('Podcasts'), findsOneWidget);
    });

    testWidgets('the app: offline the bar is on top and the pages that need the server say so', (tester) async {
      final offline = OfflineController();
      await tester.pumpWidget(PlayerScope(
        controller: PlayerController(demoBooks),
        child: MaterialApp(
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          locale: const Locale('en'),
          home: Shell(offline: offline),
        ),
      ));
      expect(find.byKey(const ValueKey('offline-banner')), findsNothing);
      offline.setUnreachable(true);
      await tester.pump();
      expect(find.byKey(const ValueKey('offline-banner')), findsOneWidget);
      await tester.tap(find.text('Podcasts').first);
      await tester.pump();
      expect(find.byKey(const ValueKey('offline-needs-network')), findsWidgets);
      offline.setUnreachable(false);
      await tester.pump();
      expect(find.byKey(const ValueKey('offline-banner')), findsNothing);
    });

    testWidgets('the settings: the switch, the waiting changes with the button, and what the server refused', (tester) async {
      final offline = OfflineController();
      offline.outbox.addLabels('book', 1, add: ['Gebet'], title: 'Eins');
      var synced = 0;
      await tester.pumpWidget(_app(SingleChildScrollView(child: OfflineCard(controller: offline, onSync: () => synced++)), offline: offline));
      expect(find.textContaining('1 change waits'), findsOneWidget);
      await tester.tap(find.byKey(const ValueKey('offline-sync')));
      expect(synced, 1);
      await tester.tap(find.byKey(const ValueKey('offline-switch')));
      await tester.pump();
      expect(offline.manual, true);
      expect(find.byKey(const ValueKey('offline-sync')), findsNothing, reason: 'offline by choice nothing is sent');
      // A refusal is listed in words until it was seen.
      offline.rejected = [RejectedChange(offline.outbox.entries.single, 403, 'forbidden')];
      offline.touch();
      await tester.pump();
      expect(find.textContaining('Labels of “Eins” (not allowed)'), findsOneWidget);
      await tester.tap(find.byKey(const ValueKey('offline-dismiss')));
      await tester.pump();
      expect(find.byKey(const ValueKey('offline-rejected')), findsNothing);
    });
  });
}
