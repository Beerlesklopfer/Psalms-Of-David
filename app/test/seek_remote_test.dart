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

class _Source extends MediaSource {
  @override
  Future<String> fetchPart(Track track) async => '/cache/${track.partId}.audio';
  @override
  Future<void> saveProgress(
    Book book,
    Track track,
    Duration position, {
    required bool finished,
  }) async {}
  @override
  Future<PassageHit?> resolvePassage(String text) async => null;
}

void main() {
  testWidgets('with a track from the server the bar grows after the play button', (tester) async {
    var now = Duration.zero;
    final backend = FakeBackend(clock: () => now, fileLength: const Duration(minutes: 6));
    final book = Book(
      id: '1',
      title: 'B',
      author: 'A',
      color: const Color(0xFF000000),
      tracks: [
        Track(disc: 1, number: 1, title: 'T1', length: const Duration(minutes: 6), partId: 10),
      ],
    );
    final p = PlayerController([book], backend: backend, source: _Source());
    addTearDown(p.dispose);
    await tester.pumpWidget(
      PlayerScope(
        controller: p,
        child: MaterialApp(
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          home: const Scaffold(body: MiniPlayer()),
        ),
      ),
    );
    // The person presses play on the book that is already selected.
    await tester.tap(find.byIcon(Icons.play_arrow));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100)); // the file arrives, the engine starts
    expect(backend.loadedPath, '/cache/10.audio');

    now = const Duration(seconds: 90);
    await tester.pump(const Duration(milliseconds: 600));
    final value = tester.widget<Slider>(find.byType(Slider)).value;
    expect(value, closeTo(90000, 3000), reason: 'the bar follows the engine');
    p.toggle();
  });
}
