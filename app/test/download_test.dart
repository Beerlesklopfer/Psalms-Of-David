// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/format.dart';

import 'package:psalm/screens/about_screen.dart';
import 'package:psalm/screens/book_screen.dart';
import 'package:psalm/server/server_source.dart';

/// A server that is a folder: fetching a part writes a file, so "downloaded" means "is there".
class _FolderSource extends MediaSource {
  _FolderSource(this.dir);

  final Directory dir;
  int fetched = 0;
  Completer? gate;

  File _f(Track t) => File('${dir.path}/${t.partId}.audio');

  @override
  bool get canDownload => true;

  @override
  bool isDownloaded(Track t) => _f(t).existsSync();

  @override
  Future<void> removeDownload(Track t) async {
    if (_f(t).existsSync()) _f(t).deleteSync();
  }

  @override
  Future<({int files, int bytes})> storageUsed() async {
    final files = dir.listSync().whereType<File>().toList();
    return (files: files.length, bytes: files.fold<int>(0, (n, f) => n + f.lengthSync()));
  }

  @override
  Future<void> removeAllDownloads() async {
    for (final f in dir.listSync().whereType<File>()) {
      f.deleteSync();
    }
  }

  @override
  Future<String> fetchPart(Track t) async {
    if (!_f(t).existsSync()) {
      fetched++;
      _f(t).writeAsStringSync('audio');
    }
    return _f(t).path;
  }

  @override
  Future<void> saveProgress(Book book, Track track, Duration position, {required bool finished}) async {}

  @override
  Future<PassageHit?> resolvePassage(String text) async => null;
}

Book _book() => Book(
      id: '1',
      title: 'Winter der Welt',
      author: 'Ken Follett',
      color: const Color(0xFF000000),
      tracks: [
        for (var i = 1; i <= 4; i++) Track(disc: 1, number: i, title: 'T$i', length: const Duration(minutes: 5), partId: i),
      ],
    );

void main() {
  late Directory dir;
  setUp(() => dir = Directory.systemTemp.createTempSync('psalm-dl'));
  tearDown(() => dir.deleteSync(recursive: true));

  test('a book is downloaded part by part, only what is missing, and can be removed', () async {
    final source = _FolderSource(dir);
    final b = _book();
    final p = PlayerController([b], source: source);
    await source.fetchPart(b.tracks[0]); // one part is there already
    source.fetched = 0;

    final seen = <int>[];
    p.addListener(() {
      final d = p.downloads['1'];
      if (d != null) seen.add(d.done);
    });
    await p.downloadBook(b);

    expect(source.fetched, 3, reason: 'the part that was there is not fetched again');
    expect(p.downloadedCount(b), 4);
    expect(seen, containsAllInOrder([1, 2, 3, 4]));
    expect(p.downloads, isEmpty);

    await p.removeDownload(b);
    expect(p.downloadedCount(b), 0);
    p.dispose();
  });

  test('a download can be stopped; what came is kept and the next start goes on', () async {
    final source = _FolderSource(dir);
    final b = _book();
    final p = PlayerController([b], source: source);
    void stopAtTwo() {
      if ((p.downloads['1']?.done ?? 0) == 2) p.cancelDownload(b);
    }

    p.addListener(stopAtTwo);
    await p.downloadBook(b);
    p.removeListener(stopAtTwo);
    expect(p.downloadedCount(b), 2);
    expect(p.downloads, isEmpty);

    await p.downloadBook(b);
    expect(p.downloadedCount(b), 4);
    p.dispose();
  });

  testWidgets('the book page has a download button that turns into a check mark', (tester) async {
    final source = _FolderSource(dir);
    final b = _book();
    final p = PlayerController([b], source: source);
    await tester.pumpWidget(PlayerScope(
      controller: p,
      child: MaterialApp(
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        home: BookScreen(book: b),
      ),
    ));
    await tester.tap(find.byTooltip('Download for listening without the server'));
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 200)));
    await tester.pumpAndSettle();
    expect(find.byIcon(Icons.download_done), findsOneWidget);
    expect(p.downloadedCount(b), 4);
    p.dispose();
  });

  test('sizes are written the way people read them, with the decimal sign of the language', () {
    expect(formatBytes('en', 0), '0 B');
    expect(formatBytes('en', 812000), '812 KB');
    expect(formatBytes('en', 1400000000), '1.4 GB');
    expect(formatBytes('de', 1400000000), '1,4 GB');
    expect(formatBytes('en', 250000000), '250 MB');
  });

  testWidgets('About shows what the downloads take and removes them all after asking', (tester) async {
    final source = _FolderSource(dir);
    final b = _book();
    final p = PlayerController([b], source: source);
    await p.downloadBook(b);
    await tester.pumpWidget(PlayerScope(
      controller: p,
      child: MaterialApp(
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        home: const Scaffold(body: AboutScreen()),
      ),
    ));
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 100)));
    await tester.pumpAndSettle();
    expect(find.text('Storage on this device'), findsOneWidget);
    expect(find.text('20 B in 4 downloaded parts'), findsOneWidget);

    await tester.ensureVisible(find.text('Remove'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Remove'));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Remove'));
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 100)));
    await tester.pumpAndSettle();
    expect(p.downloadedCount(b), 0);
    expect(find.text('No downloads'), findsOneWidget);
    p.dispose();
  });
}
