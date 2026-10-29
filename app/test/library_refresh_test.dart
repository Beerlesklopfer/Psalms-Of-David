// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/audio/fake_backend.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/server/server_source.dart';
import 'package:psalm/widgets.dart';

Book _book(String id, {int tracks = 3}) => Book(
  id: id,
  title: 'Book $id',
  author: 'A',
  color: const Color(0xFF000000),
  tracks: [
    for (var i = 1; i <= tracks; i++)
      Track(
        disc: 1,
        number: i,
        title: 'T$i',
        length: const Duration(minutes: 5),
        path: '/m/$id/$i.mp3',
      ),
  ],
);

void main() {
  test('a reloaded library brings new books and keeps what plays', () {
    final backend = FakeBackend();
    final p = PlayerController([_book('1')], backend: backend);
    p.open(p.library.first, track: 1);
    p.seek(const Duration(minutes: 2));

    p.updateLibrary(LoadedLibrary([_book('1'), _book('2')], {}));

    expect(p.library.map((b) => b.id), ['1', '2'], reason: 'the new CD is there');
    expect(p.book!.id, '1');
    expect(p.trackIndex, 1);
    expect(p.position, const Duration(minutes: 2));
    expect(p.playing, isTrue, reason: 'playing goes on');
    p.dispose();
  });

  test('if the open book is gone, the player stops', () {
    final backend = FakeBackend();
    final p = PlayerController([_book('1'), _book('2')], backend: backend);
    p.open(p.library.first, track: 0);
    p.updateLibrary(LoadedLibrary([_book('2')], {}));
    expect(p.book!.id, '2');
    expect(p.playing, isFalse);
    p.dispose();
  });

  testWidgets('the refresh button asks for the library once and shows progress meanwhile', (
    tester,
  ) async {
    final p = PlayerController([_book('1')]);
    var calls = 0;
    p.refresher = () async {
      calls++;
      await Future<void>.delayed(const Duration(milliseconds: 50));
    };
    await tester.pumpWidget(
      PlayerScope(
        controller: p,
        child: MaterialApp(
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          home: Scaffold(appBar: AppBar(actions: const [RefreshButton()])),
        ),
      ),
    );
    await tester.tap(find.byTooltip('Refresh the library'));
    await tester.pump();
    expect(find.byType(CircularProgressIndicator), findsOneWidget);
    await tester.tap(find.byType(CircularProgressIndicator), warnIfMissed: false);
    await tester.pump(const Duration(milliseconds: 100));
    expect(calls, 1);
    expect(find.byTooltip('Refresh the library'), findsOneWidget);
    p.dispose();
  });

  testWidgets('without a server there is no refresh button', (tester) async {
    final p = PlayerController([_book('1')]);
    await tester.pumpWidget(
      PlayerScope(
        controller: p,
        child: MaterialApp(
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          home: Scaffold(appBar: AppBar(actions: const [RefreshButton()])),
        ),
      ),
    );
    expect(find.byType(IconButton), findsNothing);
    p.dispose();
  });
}
