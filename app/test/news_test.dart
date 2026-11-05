// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/inbox/inbox_notifier.dart';
import 'package:psalm/l10n/app_localizations_en.dart';
import 'package:psalm/news.dart';

class _Server implements NewsApi {
  final asked = <int?>[];
  NewsAnswer answer = const NewsAnswer(1000, []);
  bool fail = false;

  @override
  Future<NewsAnswer> news(int? since) async {
    asked.add(since);
    if (fail) throw const SocketException('away from home');
    return answer;
  }
}

class _Notifier implements InboxNotifier {
  final shown = <InboxNotice>[];
  final _events = StreamController<InboxNotifierEvent>.broadcast();
  void tap(int id) => _events.add(InboxNotifierEvent(id, null));

  @override
  Future<void> show(InboxNotice notice) async => shown.add(notice);
  @override
  Future<void> close(int id) async {}
  @override
  Stream<InboxNotifierEvent> get events => _events.stream;
  @override
  Future<void> dispose() async {}
}

NewsAnswer _answer(int now, List<NewsEntry> e) => NewsAnswer(now, e);

void main() {
  late Directory dir;
  late _Server server;
  late _Notifier notifier;
  late NewsWatcher w;

  setUp(() {
    dir = Directory.systemTemp.createTempSync('news');
    server = _Server();
    notifier = _Notifier();
    w = NewsWatcher(api: server, notifier: notifier, cursorFile: File('${dir.path}/news-since'))..l10n = AppLocalizationsEn();
  });
  tearDown(() {
    w.dispose();
    dir.deleteSync(recursive: true);
  });

  test('the first look tells nothing old and keeps the time; the next one asks from there and tells what is new', () async {
    server.answer = _answer(1000, const [NewsEntry(kind: 'music', id: 7, title: 'Old', from: 'X')]);
    await w.check();
    expect(notifier.shown, isEmpty);
    expect(server.asked, [null]);
    server.answer = _answer(2000, const [
      NewsEntry(kind: 'music', id: 8, title: 'Best of Classic, Vol. 1', from: 'Various Artists'),
      NewsEntry(kind: 'episode', id: 3, title: 'Folge 3', from: 'Kinderfunk'),
      NewsEntry(kind: 'station', id: 5, title: 'Radio A'),
    ]);
    await w.check();
    expect(server.asked, [null, 1000]);
    expect(notifier.shown.map((n) => (n.title, n.body)), [
      ('New album', 'Best of Classic, Vol. 1 · Various Artists'),
      ('New episode of Kinderfunk', 'Folge 3'),
      ('New station', 'Radio A'),
    ]);
    expect(notifier.shown.every((n) => n.id < 0), isTrue, reason: 'the inbox has the positive ids');
    await w.check();
    expect(server.asked.last, 2000);
  });

  test('many at once become one notification that counts them', () async {
    File('${dir.path}/news-since').writeAsStringSync('1');
    server.answer = _answer(2, [for (var i = 0; i < 6; i++) NewsEntry(kind: 'music', id: i, title: 'A$i')]);
    await w.check();
    expect(notifier.shown.single.title, '6 new media on David');
  });

  test('a tap on the notification of an album asks to open it; away from home nothing happens', () async {
    File('${dir.path}/news-since').writeAsStringSync('1');
    server.answer = _answer(2, const [NewsEntry(kind: 'audiobook', id: 42, title: 'Hörbuch')]);
    await w.check();
    expect(notifier.shown.single.title, 'New audiobook');
    notifier.tap(notifier.shown.single.id);
    await Future<void>.delayed(Duration.zero);
    expect(w.openItem.value, 42);
    server.fail = true;
    await w.check();
    expect(notifier.shown.length, 1);
    expect(File('${dir.path}/news-since').readAsStringSync(), '2', reason: 'the time stays');
  });

  test('the answer of the server is read', () {
    final a = NewsAnswer.fromJson(const {
      'now': 5,
      'items': [{'id': 1, 'kind': 'audiobook', 'title': 'T', 'creator': 'C'}],
      'episodes': [{'id': 2, 'podcast_id': 9, 'podcast': 'P', 'title': 'E'}],
      'stations': [{'id': 3, 'name': 'S'}],
    });
    expect(a.now, 5);
    expect(a.entries.map((e) => (e.kind, e.id, e.title, e.from)), [('audiobook', 1, 'T', 'C'), ('episode', 2, 'E', 'P'), ('station', 3, 'S', '')]);
  });
}
