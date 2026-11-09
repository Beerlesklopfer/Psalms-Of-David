// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
// The e-books in the app (docs/spec-ebooks.md): the list, the copies on the device, the place read, the reader of an EPUB.
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/ebooks/ebook.dart';
import 'package:psalm/ebooks/ebook_api.dart';
import 'package:psalm/ebooks/ebook_controller.dart';
import 'package:psalm/ebooks/ebook_ui.dart';
import 'package:psalm/ebooks/epub_reader.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/offline/offline_controller.dart';
import 'package:psalm/server/api_client.dart';

/// The text of an EPUB as the server makes it: two chapters, a picture, a table of contents.
final textJson = jsonEncode({
  'title': 'Institutio',
  'chapters': [
    {
      'title': 'Vorwort',
      'href': 'c1.xhtml',
      'blocks': [
        {'kind': 'h1', 'spans': [{'t': 'Vorwort'}]},
        {'kind': 'p', 'spans': [{'t': 'Der '}, {'t': 'Glaube', 'i': true}, {'t': ' ist '}, {'t': 'Gewissheit', 'b': true}, {'t': '.'}]},
        {'kind': 'img', 'src': 'img/map.png', 'spans': [{'t': 'Karte'}]},
      ],
    },
    {
      'title': 'Vom Glauben',
      'href': 'c2.xhtml',
      'blocks': [
        {'kind': 'h2', 'spans': [{'t': 'Vom Glauben'}]},
        {'kind': 'quote', 'spans': [{'t': 'Erste Zeile'}]},
        {'kind': 'li', 'spans': [{'t': 'Eins'}]},
        {'kind': 'p', 'spans': [{'t': 'Schluss'}]},
      ],
    },
  ],
  'toc': [
    {'title': 'Vorwort', 'chapter': 0, 'level': 0},
    {'title': 'Vom Glauben', 'chapter': 1, 'level': 0},
    {'title': 'ins Leere', 'chapter': 9, 'level': 0},
  ],
});

/// A 1×1 PNG.
final png = base64Decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==');

/// The server as the controller sees it.
class FakeEbookApi extends EbookApi {
  FakeEbookApi() : super(ApiClient(address: 'localhost:1'));

  List<Ebook> books = [
    const Ebook(id: 1, format: 'epub', title: 'Institutio', author: 'Calvin', mtimeMs: 10),
    const Ebook(id: 2, format: 'pdf', title: 'Kleinode', author: 'Spurgeon', mtimeMs: 20),
  ];
  bool down = false;
  final List<(int, EbookProgress)> saved = [];

  void _check() {
    if (down) throw ApiException(0, 'network');
  }

  @override
  Future<List<Ebook>> list() async {
    _check();
    return books;
  }

  @override
  Future<String> text(int id) async {
    _check();
    return textJson;
  }

  @override
  Future<Uint8List> cover(int id) async {
    _check();
    return png;
  }

  @override
  Future<Uint8List> picture(int id, String name) async {
    _check();
    return png;
  }

  @override
  Future<void> file(int id, File dest, {void Function(int bytes)? progress}) async {
    _check();
    await dest.parent.create(recursive: true);
    await dest.writeAsBytes(utf8.encode('%PDF-1.4'));
  }

  @override
  Future<void> saveProgress(int id, EbookProgress p) async {
    _check();
    saved.add((id, p));
  }
}

/// Lets the files of the device be read and written (real input and output, outside the fake time of the test).
Future<void> settle(WidgetTester tester) async {
  for (var i = 0; i < 5; i++) {
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 30)));
    await tester.pump();
  }
}

Widget app(Widget home, {OfflineController? offline}) {
  final a = MaterialApp(
    locale: const Locale('de'),
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    home: home,
  );
  return offline == null ? a : OfflineScope(controller: offline, child: a);
}

void main() {
  late Directory dir;
  setUp(() => dir = Directory.systemTemp.createTempSync('ebooks'));
  tearDown(() => dir.deleteSync(recursive: true));

  test('the text of the server is read with its emphasis; a table of contents entry that leads nowhere is left out', () {
    final t = EbookText.parse(textJson);
    expect(t.chapters.length, 2);
    expect(t.chapters[0].blocks[1].plain, 'Der Glaube ist Gewissheit.');
    expect(t.chapters[0].blocks[1].spans[1].italic, isTrue);
    expect(t.chapters[0].blocks[1].spans[3].bold, isTrue);
    expect(t.toc.map((e) => e.title), ['Vorwort', 'Vom Glauben']);
    expect(t.pictures, ['img/map.png']);
    expect(t.percentAt(0, 0), 0);
    expect(t.percentAt(1, 0), closeTo(3 / 7, 1e-9));
    expect(const EbookProgress(locator: 'epub:1:2', percent: 0, readAt: 0).epub, (1, 2));
    expect(const EbookProgress(locator: 'pdf:7', percent: 0, readAt: 0).pdfPage, 7);
    expect(const EbookProgress(locator: 'x', percent: 0, readAt: 0).epub, isNull);
  });

  test('loaded onto the device an e-book opens without the server; the list stays for offline; a new version is marked', () async {
    final api = FakeEbookApi();
    final c = EbookController(api: api, shelf: EbookShelf(root: dir));
    await c.load();
    expect(c.books.map((e) => e.title), ['Institutio', 'Kleinode']);
    expect(c.stale, isFalse);
    expect(await c.download(c.books[0]), isTrue);
    expect(await c.download(c.books[1]), isTrue);
    expect((c.isKept(c.books[0]), c.outdated(c.books[0])), (true, false));
    expect(File('${dir.path}/1/text.json').existsSync(), isTrue);
    expect(c.shelf.pictureOf(1, 'img/map.png').existsSync(), isTrue);
    expect(File('${dir.path}/2/book.pdf').existsSync(), isTrue);

    // Without the server: the last list, the copies open.
    api.down = true;
    final off = EbookController(api: api, shelf: EbookShelf(root: dir));
    await off.load();
    expect((off.stale, off.books.length), (true, 2));
    expect((await off.text(off.books[0])).chapters.length, 2);
    expect(await off.picture(off.books[0], 'img/map.png'), png);
    expect(await off.pdf(off.books[1]), endsWith('/2/book.pdf'));
    expect(await off.cover(off.books[0]), png);

    // The server has a new version of the PDF.
    api.down = false;
    api.books = [api.books[0], const Ebook(id: 2, format: 'pdf', title: 'Kleinode', author: 'Spurgeon', mtimeMs: 21)];
    await off.load();
    expect(off.outdated(off.books[1]), isTrue);
    await off.remove(off.books[1]);
    expect(off.isKept(off.books[1]), isFalse);
  });

  test('the place read is kept on the device at once and goes to the server when it answers; the newer place wins', () async {
    final api = FakeEbookApi()..down = true;
    final c = EbookController(api: api, shelf: EbookShelf(root: dir));
    await c.load();
    final e = const Ebook(id: 1, format: 'epub', title: 'Institutio', author: 'Calvin', progress: EbookProgress(locator: 'epub:0:1', percent: 0.1, readAt: 100));
    await c.saveProgress(e, const EbookProgress(locator: 'epub:1:2', percent: 0.7, readAt: 200));
    expect(api.saved, isEmpty);
    expect(c.progressOf(e)?.locator, 'epub:1:2', reason: 'the place of this device is newer than the one of the list');
    expect(c.progressOf(e.withProgress(const EbookProgress(locator: 'epub:1:3', percent: 0.8, readAt: 300)))?.locator, 'epub:1:3', reason: 'read further on another device');

    api.down = false;
    final again = EbookController(api: api, shelf: EbookShelf(root: dir));
    await again.load();
    expect(api.saved.map((s) => (s.$1, s.$2.locator)), [(1, 'epub:1:2')], reason: 'what waited goes to the server with the next list');
    await again.load();
    expect(api.saved.length, 1, reason: 'and only once');
  });

  testWidgets('the rubric lists the e-books, what is begun first; the reader shows the text, the chapters and keeps the place', (tester) async {
    final api = FakeEbookApi();
    api.books = [api.books[0].withProgress(const EbookProgress(locator: 'epub:0:1', percent: 0.2, readAt: 5)), api.books[1]];
    final c = EbookController(api: api, shelf: EbookShelf(root: dir));
    await tester.pumpWidget(app(Scaffold(body: CustomScrollView(slivers: [EbooksBody(controller: c)]))));
    await settle(tester);
    expect(find.text('Weiterlesen'), findsOneWidget);
    expect(find.byKey(const ValueKey('ebook-begun-1')), findsOneWidget);
    expect(find.byKey(const ValueKey('ebook-2')), findsOneWidget);
    expect(find.textContaining('20 % gelesen'), findsWidgets);

    await tester.tap(find.byKey(const ValueKey('ebook-begun-1')));
    await settle(tester);
    await tester.pumpAndSettle();
    expect(find.byType(EpubReaderScreen), findsOneWidget);
    expect(find.textContaining('Gewissheit', findRichText: true), findsOneWidget, reason: 'opens where the person was');
    await tester.scrollUntilVisible(find.byKey(const ValueKey('chapter-next')), 200, scrollable: find.byType(Scrollable).last);
    await tester.tap(find.byKey(const ValueKey('chapter-next')));
    await tester.pumpAndSettle();
    expect(find.text('Erste Zeile', findRichText: true), findsOneWidget);
    expect(c.progressOf(c.books[0])?.locator, 'epub:1:0', reason: 'a new chapter is kept at once');

    await tester.tap(find.byKey(const ValueKey('reader-contents')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('toc-0-Vorwort')));
    await tester.pumpAndSettle();
    expect(find.text('Vorwort', findRichText: true), findsWidgets);
    expect(c.progressOf(c.books[0])?.locator, 'epub:0:0');
  });

  testWidgets('offline only what is on the device opens; the others are dimmed and say why', (tester) async {
    final api = FakeEbookApi();
    final c = EbookController(api: api, shelf: EbookShelf(root: dir));
    await tester.runAsync(() async {
      await c.load();
      await c.download(c.books[0]);
    });
    final offline = OfflineController()
      ..setManual(true)
      ..toggleShowAll();
    await tester.pumpWidget(app(Scaffold(body: CustomScrollView(slivers: [EbooksBody(controller: c)])), offline: offline));
    await settle(tester);
    expect(find.byKey(const ValueKey('ebook-dim-2')), findsOneWidget);
    expect(find.byKey(const ValueKey('ebook-dim-1')), findsNothing);
    await tester.tap(find.byKey(const ValueKey('ebook-2')));
    await tester.pump();
    expect(find.byKey(const ValueKey('ebook-not-here')), findsOneWidget);
    // Only the downloaded ones, as the bar says by default.
    offline.toggleShowAll();
    await tester.pump();
    expect(find.byKey(const ValueKey('ebook-2')), findsNothing);
    expect(find.byKey(const ValueKey('ebook-1')), findsOneWidget);
  });
}
