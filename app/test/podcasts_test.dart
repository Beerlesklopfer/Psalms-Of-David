// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/podcasts.dart';
import 'package:psalm/widgets.dart';

void main() {
  test('an episode from the server plays as one track from the publisher and keeps where the account stopped', () {
    final e = Episode.fromJson({
      'id': 5,
      'title': 'Folge 1',
      'audio_url': 'https://example.org/1.mp3',
      'published_at': 1760349600000,
      'duration_s': 600,
      'position_ms': 61000,
      'finished': false,
    });
    expect((e.position, e.duration, e.finished), (const Duration(seconds: 61), const Duration(minutes: 10), false));
    final book = e.asItem('Kinderfunk');
    expect((book.id, book.kind, book.author, book.isStream), ('ep:5', 'podcast', 'Kinderfunk', false));
    expect((book.tracks.single.url, book.tracks.single.live), ('https://example.org/1.mp3', false));
  });

  test('a station plays as a live stream, a request that waits is marked', () {
    final s = Station.fromJson({
      'id': 3,
      'name': 'Radio A',
      'stream_url': 'https://stream.example.org/a',
      'approved': false,
    });
    expect(s.approved, isFalse);
    final book = s.asItem();
    expect((book.id, book.isStream, book.tracks.single.live), ('st:3', true, true));
    final p = Podcast.fromJson({'id': 1, 'title': 'Kinderfunk', 'description': '', 'approved': false, 'unheard': 2});
    expect((p.approved, p.unheard), (false, 2));
  });

  testWidgets('a live stream has no seek bar and shows Live', (tester) async {
    final p = PlayerController([
      Station.fromJson({'id': 3, 'name': 'Radio A', 'stream_url': 'https://stream.example.org/a'}).asItem(),
    ]);
    await tester.pumpWidget(
      PlayerScope(
        controller: p,
        child: MaterialApp(
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          home: const Scaffold(body: SeekBar()),
        ),
      ),
    );
    expect(find.byType(Slider), findsNothing);
  });
}
